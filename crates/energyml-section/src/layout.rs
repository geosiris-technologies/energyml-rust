//! A catalogue and a view, laid out in numbers (ADR-0015).
//!
//! [`layout`] decides, and that is all that can be wrong in the drawing: the
//! window and the scales, the **vertical exaggeration they result in**
//! (written on the drawing), the graduations, what is drawn and in which order,
//! the fills, the wells' colours and labels, the warnings. [`crate::svg`] only
//! writes what is decided here — which is why `tests/layout.rs` asserts on
//! numbers, and `tests/svg.rs` on the markup.
//!
//! Nothing is read and no geometry of the model is computed: a layout is
//! cheap, and the front end redraws on every change of the view.
//!
//! # Draw order
//!
//! `envelopes` (the grids' fills, at the back) → `fills` (the surfaces')
//! → `outlines` (the grids' tops, bottoms and faults) → `traces` → `wells` →
//! `marks` (vertices, crossings) → `labels`. The layers are in [`Layout::layers`]
//! in that order; the writer follows it.
//!
//! # What `View` does here
//!
//! * `hidden` — absent: no group, no label, no fill, and no fill *from* it nor
//!   *towards* it (a hidden line is not a neighbour);
//! * `highlight` — marked ([`LaidObject::highlighted`]), nothing else changes;
//! * `portion` — a **truncation**: what falls in `[start, end]`, bounds
//!   included, abscissas unchanged (*a well at 150 m stays at 150 m*);
//!   a trace is cut at the bounds, a well is kept whole when its head's
//!   abscissa is inside;
//! * `window` — the viewport. `None`: everything, with a margin.
//!
//! # Axes
//!
//! `AxisMode::Projected` is drawn. `Coordinates` is drawn too — the trace is
//! in the scene's system, so the true `(x, y)` at each tick is one
//! `fence::point_at` away. `Geodesic` is **not**: it needs a scale factor along
//! the trace from `energyml-proj` (and a reprojection when the scene's system
//! is not the one the factor is known in), and this crate has no dependency on
//! it. The layout falls back to the projected distance **and says so** in a
//! warning ([`Layout::x_axis`]'s `mode` is the one actually drawn); the shape
//! to fill — a function from `s` to a geodesic distance, applied to the tick
//! values — is [`Axis`]: ticks carry their `value` and their `label`
//! separately for that reason.

pub(crate) mod fill;
mod labels;
mod marking;
mod ticks;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::{
    AxisMode, Catalogue, CutSource, EnvelopeItem, FORMAT, FillChoice, Inputs, Interval, Item, LabelMode, Mark, Outcome, Paint, Plan,
    Section, SectionKey, Side, Target, TraceItem, TraceRole, UnstratifiedStyle, View, WellColouring, WellItem, Window, NEUTRAL_COLOUR,
};
use fill::Span;

// ---------------------------------------------------------------------------
// The layout

/// Everything the SVG writer needs, decided: positions in pixels, ticks,
/// fills, labels, warnings.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// What is drawn.
    pub target: Target,
    /// The drawing's own title ("Long section", "Cross section 2", "Plan").
    pub title: String,
    /// Width and height, pixels.
    pub size: [f64; 2],
    /// The background colour.
    pub background: String,
    /// The plot area, pixels.
    pub frame: Rect,
    /// The window really drawn (what `View::window` asked, or everything with a
    /// margin; in the plan, widened to the frame's proportions).
    pub window: Window,
    /// The window this drawing would have with `View::window` unset: the whole
    /// extent. The same whatever window was asked, so a front end that holds a
    /// zoomed drawing still knows what to clamp its zooms to.
    pub whole: Window,
    /// Pixels per unit of the window, `[horizontal, vertical]`.
    pub scale: [f64; 2],
    /// The vertical exaggeration those scales give — `scale[1] / scale[0]` —
    /// for a section; `None` for the plan, which is equal on both axes.
    pub exaggeration: Option<f64>,
    /// What is written about it on the drawing: `"vertical exaggeration ×2.5"`.
    pub exaggeration_text: String,
    /// The position of the trace's first point (`s = 0`) with its EPSG code,
    /// written at the centre of the title band — shortened to the code, or left
    /// out (and counted in the warnings), when the band is too narrow. Empty
    /// when the view asks for none.
    pub origin_text: String,
    /// Where that text starts, pixels: centred on the frame, clamped to the free
    /// space between the title and the exaggeration.
    pub origin_x: f64,
    /// The horizontal axis.
    pub x_axis: Axis,
    /// The vertical axis.
    pub y_axis: Axis,
    /// The layers, back to front.
    pub layers: Vec<Layer>,
    /// What the drawing prints about itself: refused objects, wells set aside,
    /// a projection in time, an axis it could not draw, labels folded.
    pub warnings: Vec<String>,
    /// How many labels did not fit and were folded.
    pub folded_labels: usize,
    /// How many lines of notes the drawing has room for under its plot —
    /// what [`Layout::written_warnings`] writes. `0` when the view asks for no
    /// notes.
    pub note_lines: usize,
    /// What the SVG's `<metadata>` carries.
    pub metadata: Metadata,
    /// Pixels kept on the left of the axes for the strata legend; `0` when there
    /// is none. The axes' own furniture (the vertical title) moves by as much.
    pub left_shift: f64,
}

/// A rectangle, pixels.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    /// Left.
    pub x: f64,
    /// Top.
    pub y: f64,
    /// Width.
    pub w: f64,
    /// Height.
    pub h: f64,
}

/// One axis: its title, the mode actually drawn, its ticks.
#[derive(Clone, Debug, PartialEq)]
pub struct Axis {
    /// The title, with its unit.
    pub title: String,
    /// What it says — may differ from what the view asked (geodesic → projected).
    pub mode: AxisMode,
    /// The ticks, round numbers.
    pub ticks: Vec<Tick>,
}

/// A graduation.
#[derive(Clone, Debug, PartialEq)]
pub struct Tick {
    /// The value, in the window's unit.
    pub value: f64,
    /// Where it is, pixels (x for the horizontal axis, y for the vertical one).
    pub at: f64,
    /// Its text.
    pub label: String,
    /// A second line of text — the other coordinate on a `Coordinates` axis.
    pub sublabel: Option<String>,
}

/// A group of objects drawn together, in one z-order slot.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    /// `envelopes`, `fills`, `outlines`, `traces`, `wells`, `marks`, `labels`.
    pub name: &'static str,
    /// Whether the plot area clips it (labels above the frame are not).
    pub clipped: bool,
    /// Its groups.
    pub objects: Vec<LaidObject>,
}

/// What a group is *about* — one attribute each, so that `data-key` stays the
/// one-per-object handle the front end reads.
#[derive(Clone, Debug, PartialEq)]
pub enum Attr {
    /// An object: its page key (`ObjectId::key`).
    Key(String),
    /// A section of the plan: `long`, `cross-2`.
    Section(String),
    /// The fill of an object's colour.
    FillOf(String),
    /// The label of an object.
    LabelOf(String),
    /// A decoration (a mark), named.
    Decor(String),
    /// A strata legend entry: the decoration's name, the full uri of the
    /// unit it shows, and the packages whose wells show it in this colour —
    /// what the front end opens the unit's colour by, and in which packages.
    UnitEntry(String, String, Vec<String>),
    /// A marker (`View::markers`): its name and, on the drawing, the key of the
    /// object it marks — `data-marker` and `data-key`. In the legend, no key.
    Marker {
        /// The marker's name.
        name: String,
        /// The marked object's page key; `None` for a legend entry.
        key: Option<String>,
    },
}

/// A group of shapes.
#[derive(Clone, Debug, PartialEq)]
pub struct LaidObject {
    /// What it is about.
    pub attr: Attr,
    /// `well`, `trace`, `envelope`, `fill`, `label`, `vertex`, `crossing`…
    pub kind: String,
    /// The object's title, as the package wrote it (the writer escapes it).
    pub title: String,
    /// Put forward on purpose (`View::highlight`).
    pub highlighted: bool,
    /// For a well: the head, in the window's units — where it was projected.
    pub at: Option<[f64; 2]>,
    /// What is drawn.
    pub shapes: Vec<Shape>,
    /// For a well: one group per interval, with its unit.
    pub intervals: Vec<LaidInterval>,
}

/// One stratigraphic interval of a well, drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct LaidInterval {
    /// MD start.
    pub from: f64,
    /// MD end.
    pub to: f64,
    /// The unit's title.
    pub unit: String,
    /// The unit's full uri, when the interval names one — what the front end finds
    /// the unit by (never its title).
    pub unit_uri: Option<String>,
    /// The colour drawn — the interval's own, or the well's when it has none.
    pub colour: String,
    /// The line's width, pixels — the well's.
    pub width: f64,
    /// The piece of path, pixels.
    pub points: Vec<[f64; 2]>,
    /// A short black line across the well where this interval begins against
    /// the end of the one before it ([`View::strata_ticks`]): its two ends, pixels.
    pub tick: Option<[[f64; 2]; 2]>,
}

/// Where a text sits against its anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// Starts at the point.
    Start,
    /// Centred on it.
    Middle,
    /// Ends at it.
    End,
}

/// A primitive, in pixels. Colours are strings, written as attributes.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// An open line.
    Polyline {
        /// Its points.
        points: Vec<[f64; 2]>,
        /// Colour.
        stroke: String,
        /// Width.
        width: f64,
        /// Dashed.
        dash: bool,
    },
    /// An open line drawn dashed, with the lengths it was asked: a dash and a
    /// gap in pixels (`stroke-dasharray`), butt caps so that a gap stays a gap.
    Dashed {
        /// Its points.
        points: Vec<[f64; 2]>,
        /// Colour.
        stroke: String,
        /// Width.
        width: f64,
        /// The length of a dash.
        dash: f64,
        /// The gap between two dashes.
        gap: f64,
    },
    /// A closed area.
    Polygon {
        /// Its points.
        points: Vec<[f64; 2]>,
        /// Fill colour; `"none"` for an outline.
        fill: String,
        /// Fill opacity.
        opacity: f64,
        /// Outline colour, if any.
        stroke: Option<String>,
        /// Outline dashed.
        dash: bool,
    },
    /// A disc.
    Circle {
        /// Centre.
        c: [f64; 2],
        /// Radius.
        r: f64,
        /// Fill colour.
        fill: String,
    },
    /// A marker's symbol, centred on `at` and `size` pixels across: written as
    /// paths, so nothing is loaded and nothing is stripped, and kept at its
    /// pixel size by the zoom script (`data-zoom-fixed`) like a well's head.
    Symbol {
        /// Its centre.
        at: [f64; 2],
        /// The shape, one of [`crate::markers::declared`]'s ids that is drawn.
        kind: String,
        /// Its extent, pixels.
        size: f64,
        /// Fill colour; `"none"` for an outline.
        fill: String,
        /// Outline colour.
        stroke: String,
    },
    /// A text.
    Text {
        /// Where.
        at: [f64; 2],
        /// What.
        text: String,
        /// Alignment.
        anchor: Anchor,
        /// Font size, pixels.
        size: f64,
        /// Colour.
        fill: String,
    },
}

/// What the SVG's `<metadata>` holds: enough to refuse by name
/// and to redraw — the format, the version of this crate, the inputs (the trace
/// itself, in coordinates and system) and the view.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    /// [`FORMAT`].
    pub format: u32,
    /// This crate's version.
    pub version: String,
    /// What made the catalogue.
    pub inputs: Inputs,
    /// How it was looked at.
    pub view: View,
}

impl Layout {
    /// The warnings as they are written under the plot: every one when they
    /// fit in [`Layout::note_lines`], else as many as fit less one, and a last
    /// line counting the others — a note dropped is still said to exist.
    pub fn written_warnings(&self) -> Vec<String> {
        let room = self.note_lines;
        if self.warnings.len() <= room {
            return self.warnings.clone();
        }
        if room == 0 {
            return Vec::new();
        }
        let mut out: Vec<String> = self.warnings[..room - 1].to_vec();
        let hidden = self.warnings.len() - (room - 1);
        out.push(if out.is_empty() {
            format!("{hidden} notes — not shown at this size")
        } else {
            format!("+ {hidden} more notes — not shown at this size")
        });
        out
    }

    /// The front end keys of every object drawn, in drawing order — one per object,
    /// whatever its layers (fills and labels are not keys).
    pub fn keys(&self) -> Vec<&str> {
        self.layers
            .iter()
            .flat_map(|l| l.objects.iter())
            .filter_map(|o| if let Attr::Key(k) = &o.attr { Some(k.as_str()) } else { None })
            .collect()
    }

    /// A layer by name.
    pub fn layer(&self, name: &str) -> Option<&Layer> {
        self.layers.iter().find(|l| l.name == name)
    }
}

// ---------------------------------------------------------------------------
// Constants of the drawing

const MARGIN_LEFT: f64 = 64.0;
const MARGIN_RIGHT: f64 = 20.0;
const MARGIN_TOP: f64 = 72.0;
const MARGIN_BOTTOM: f64 = 46.0;
const LINE_PX: f64 = 14.0;
/// The plan's top margin: its title — its labels are inside its frame.
const PLAN_TOP: f64 = 36.0;
/// The smallest share of the drawing's height the plot keeps: notes are folded
/// and the band of well labels lowered before the plot is squeezed under it
/// (otherwise the top-view plot ends up squashed by the legends and the padding).
const PLOT_SHARE: f64 = 0.5;
/// The smallest plot, pixels, whatever the share.
const PLOT_MIN_PX: f64 = 40.0;
const FONT_PX: f64 = 11.0;
const SMALL_PX: f64 = 10.0;
const FILL_OPACITY: f64 = 0.35;

const LEFT_COLOUR: &str = "#1f77b4";
const RIGHT_COLOUR: &str = "#d95f02";
const INK: &str = "#222222";
const MARK_COLOUR: &str = "#7b1fa2";

// ---------------------------------------------------------------------------
// Entry

/// Lay a catalogue out for a view. Reads nothing, computes no geometry of the
/// model — only where things go.
pub fn layout(catalogue: &Catalogue, view: &View) -> Layout {
    match view.target {
        Target::Section { key } => section(catalogue, view, key),
        Target::Plan => plan(catalogue, view),
    }
}

/// The front end's own metadata of a layout.
fn metadata(catalogue: &Catalogue, view: &View) -> Metadata {
    Metadata {
        format: FORMAT,
        version: env!("CARGO_PKG_VERSION").to_string(),
        inputs: catalogue.inputs.clone(),
        view: view.clone(),
    }
}

fn unit_of(catalogue: &Catalogue) -> String {
    catalogue.inputs.system.as_ref().map(|s| s.unit.clone()).unwrap_or_default()
}

/// Warnings common to every target — what the drawing owes its reader.
fn common_warnings(catalogue: &Catalogue) -> Vec<String> {
    let mut out = Vec::new();
    let refused = catalogue.objects.iter().filter(|r| matches!(r.outcome, Outcome::Refused { .. })).count();
    if refused > 0 {
        out.push(format!("{refused} objects refused — see the objects list"));
    }
    let set_aside = catalogue.plan.wells.iter().filter(|w| w.rejected.is_some()).count();
    if set_aside > 0 {
        out.push(format!("{set_aside} wells set aside — projected nowhere"));
    }
    out
}

/// The pixel map of a window onto a frame.
#[derive(Clone, Copy)]
struct Map {
    window: Window,
    frame: Rect,
    sx: f64,
    sy: f64,
}

impl Map {
    fn px(&self, p: [f64; 2]) -> [f64; 2] {
        [
            self.frame.x + (p[0] - self.window.x[0]) * self.sx,
            self.frame.y + self.frame.h - (p[1] - self.window.y[0]) * self.sy,
        ]
    }
    fn points(&self, pts: &[[f64; 2]]) -> Vec<[f64; 2]> {
        dedupe(pts.iter().filter(|p| p[0].is_finite() && p[1].is_finite()).map(|&p| self.px(p)).collect())
    }
}

/// Consecutive points closer than a twentieth of a pixel are one point: the
/// SVG's size is the number of nodes, and a well of a thousand nodes at a
/// small scale is a few dozen distinct pixels.
fn dedupe(points: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    let mut out: Vec<[f64; 2]> = Vec::with_capacity(points.len());
    for p in points {
        if out.last().is_none_or(|q| (q[0] - p[0]).abs() >= 0.05 || (q[1] - p[1]).abs() >= 0.05) {
            out.push(p);
        }
    }
    out
}

/// A growing bounding box.
#[derive(Default)]
struct Extent {
    x: Option<[f64; 2]>,
    y: Option<[f64; 2]>,
}

impl Extent {
    fn add(&mut self, p: [f64; 2]) {
        if !(p[0].is_finite() && p[1].is_finite()) {
            return;
        }
        let grow = |r: &mut Option<[f64; 2]>, v: f64| {
            *r = Some(match *r {
                Some([a, b]) => [a.min(v), b.max(v)],
                None => [v, v],
            })
        };
        grow(&mut self.x, p[0]);
        grow(&mut self.y, p[1]);
    }
    fn add_all(&mut self, pts: &[[f64; 2]]) {
        pts.iter().for_each(|&p| self.add(p));
    }
}

/// A range made drawable: a margin, and never a zero width.
fn padded(range: Option<[f64; 2]>, margin: f64) -> [f64; 2] {
    let [a, b] = range.unwrap_or([0.0, 1.0]);
    let span = b - a;
    if span <= f64::EPSILON * a.abs().max(1.0) {
        return [a - 0.5, b + 0.5];
    }
    [a - span * margin, b + span * margin]
}

/// The plot area: the drawing less its margins, `lines` of warnings below, and
/// `right` pixels kept on the right (the legend of codes) and `left` on the
/// left of the axes (the strata legend).
fn frame_of(view: &View, lines: usize, right: f64, left: f64, top: f64) -> Rect {
    let [w, h] = view.size_px;
    let bottom = MARGIN_BOTTOM + LINE_PX * lines as f64;
    Rect {
        x: MARGIN_LEFT + left,
        y: top,
        w: (w - MARGIN_LEFT - left - MARGIN_RIGHT - right).max(10.0),
        h: (h - top - bottom).max(10.0),
    }
}

/// How a drawing of height `h` shares itself out: the top margin for `levels`
/// levels of well labels (`top_for`), and the lines of notes. Notes go first —
/// down to one, the line that counts them — then levels of labels down to one,
/// until the plot keeps [`PLOT_SHARE`] of the height. Returns the top margin,
/// the levels kept and the lines of notes.
fn budget(h: f64, wanted_levels: usize, wanted_lines: usize, top_for: impl Fn(usize) -> f64) -> (f64, usize, usize) {
    let floor = (h * PLOT_SHARE).max(PLOT_MIN_PX);
    let (mut levels, mut lines) = (wanted_levels, wanted_lines);
    loop {
        let plot = h - top_for(levels) - MARGIN_BOTTOM - LINE_PX * lines as f64;
        if plot >= floor {
            break;
        }
        if lines > 1 {
            lines -= 1;
        } else if levels > 1 {
            levels -= 1;
        } else {
            break;
        }
    }
    (top_for(levels), levels, lines)
}

fn is_hidden(view: &View, key: &str) -> bool {
    view.hidden.iter().any(|h| h == key)
}

/// The width an object is drawn at: the view's own for it when it is valid,
/// else `default`. A refused width is not drawn here — [`width_warnings`] names it.
fn width_of(view: &View, key: &str, default: f64) -> f64 {
    view.widths.iter().find(|(k, _)| k == key).map(|(_, w)| *w).filter(|w| valid_width(*w)).unwrap_or(default)
}

fn valid_width(w: f64) -> bool {
    w.is_finite() && w > 0.0 && w <= crate::model::MAX_WIDTH_PX
}

/// One warning per width the view asks for that cannot be drawn — never a
/// silent clamp. The object is named by its uri (the key's store part dropped).
fn width_warnings(view: &View) -> Vec<String> {
    view.widths
        .iter()
        .filter(|(_, w)| !valid_width(*w))
        .map(|(k, w)| {
            let uri = k.split('\0').next_back().unwrap_or(k);
            format!("line width {w} refused for {uri} — it must be above 0 and at most {} px; the default is drawn", crate::model::MAX_WIDTH_PX)
        })
        .collect()
}

pub(crate) fn colour_of(view: &View, key: &str) -> Option<String> {
    view.colours.iter().find(|(k, _)| k == key).map(|(_, c)| c.clone())
}

// ---------------------------------------------------------------------------
// A section

fn key_text(key: SectionKey) -> String {
    match key {
        SectionKey::Long => "long".to_string(),
        SectionKey::Cross { index } => format!("cross-{index}"),
    }
}

fn find_section(catalogue: &Catalogue, key: SectionKey) -> Option<&Section> {
    match key {
        SectionKey::Long => Some(&catalogue.long),
        SectionKey::Cross { .. } => catalogue.crosses.iter().find(|s| s.key == key),
    }
}

/// How far a span runs along the section.
fn span_extent(span: &[[f64; 2]]) -> f64 {
    match (span.first(), span.last()) {
        (Some(a), Some(b)) => (b[0] - a[0]).abs(),
        _ => 0.0,
    }
}

/// The colour of a surface neither the scene nor the file colours: the hue the
/// 3D view gives it, derived from its uuid (`energyml_domain::color::derived_colour`,
/// the front end's `colourFor`) — so a drawing without a scene shows the surfaces in
/// the colours the 3D view shows them, rather than one colour for all. An object with no uuid takes its role's.
pub(crate) fn fallback_colour(uuid: &str, role: TraceRole) -> String {
    if uuid.is_empty() {
        default_trace_colour(role).to_string()
    } else {
        energyml_domain::color::derived_colour(uuid)
    }
}

/// A grid envelope's colour: the scene's, the file's, then the 3D view's hue.
fn envelope_colour(view: &View, e: &EnvelopeItem) -> String {
    colour_of(view, &e.object.key())
        .or_else(|| e.colour.clone())
        .unwrap_or_else(|| fallback_colour(&e.object.uuid, TraceRole::Other))
}

/// A surface's colour: the scene's first (the front end's stack), then the one the
/// file gives, then the 3D view's hue — never a colour invented from its name.
/// **One resolution** for the drawing and the 3D panels (`panels.rs`): two
/// would let a panel and the line it fills from disagree.
pub(crate) fn trace_colour(view: &View, t: &TraceItem) -> String {
    colour_of(view, &t.object.key())
        .or_else(|| t.colour.clone())
        .unwrap_or_else(|| fallback_colour(&t.object.uuid, t.role))
}

fn default_trace_colour(role: TraceRole) -> &'static str {
    match role {
        TraceRole::Horizon => "#2e7d32",
        TraceRole::Fault => "#c62828",
        TraceRole::Other => NEUTRAL_COLOUR,
    }
}

/// A trace kept for drawing: its object, its colour, its spans cut to the
/// portion.
struct KeptTrace<'a> {
    item: &'a TraceItem,
    key: String,
    colour: String,
    spans: Vec<Span>,
}

fn section(catalogue: &Catalogue, view: &View, key: SectionKey) -> Layout {
    let mut warnings = common_warnings(catalogue);
    warnings.extend(width_warnings(view));
    let unit = unit_of(catalogue);
    let title = match key {
        SectionKey::Long => "Long section".to_string(),
        SectionKey::Cross { index } => format!("Cross section {index}"),
    };
    let sec = find_section(catalogue, key);
    if sec.is_none() {
        warnings.push(format!("{title}: not in the catalogue — nothing to draw"));
    }
    let empty = Section {
        key,
        trace: Vec::new(),
        length: 0.0,
        marks: Vec::new(),
        band: Vec::new(),
        items: Vec::new(),
        z_range: None,
    };
    let sec = sec.unwrap_or(&empty);

    // The portion: a truncation, abscissas unchanged.
    let portion = view.portion.map(|[a, b]| if a <= b { [a, b] } else { [b, a] });
    let (plo, phi) = portion.map_or((f64::NEG_INFINITY, f64::INFINITY), |[a, b]| (a, b));
    let in_portion = |s: f64| s >= plo && s <= phi;

    // What is kept.
    let mut wells: Vec<&WellItem> = Vec::new();
    let mut traces: Vec<KeptTrace> = Vec::new();
    // An envelope, its top and bottom kept within the window, and its fills and faults.
    #[allow(clippy::type_complexity)]
    let mut envelopes: Vec<(&EnvelopeItem, Vec<Span>, Vec<Span>, Vec<Vec<[f64; 2]>>, Vec<Vec<[f64; 2]>>)> =
        Vec::new();
    let mut time_drawn = false;
    for item in &sec.items {
        match item {
            Item::Well(w) => {
                let k = w.object.key();
                if !is_hidden(view, &k) && in_portion(w.s) {
                    wells.push(w);
                }
            }
            Item::Trace(t) => {
                let k = t.object.key();
                if is_hidden(view, &k) {
                    continue;
                }
                let spans: Vec<Span> = t.spans.iter().filter_map(|s| fill::clip_span(s, plo, phi)).collect();
                if spans.is_empty() {
                    continue;
                }
                time_drawn |= t.time;
                // The scene's colour first (the front end's stack), then the one the
                // file gives, then the role's — never a colour invented in the
                // file's name.
                let colour = trace_colour(view, t);
                traces.push(KeptTrace { item: t, key: k, colour, spans });
            }
            Item::Envelope(e) => {
                let k = e.object.key();
                if is_hidden(view, &k) {
                    continue;
                }
                let top: Vec<Span> = e.top.iter().filter_map(|s| fill::clip_span(s, plo, phi)).collect();
                let bottom: Vec<Span> = e.bottom.iter().filter_map(|s| fill::clip_span(s, plo, phi)).collect();
                let fills: Vec<_> =
                    e.fills.iter().map(|p| fill::clip_polygon(p, plo, phi)).filter(|p| !p.is_empty()).collect();
                let faults: Vec<_> = e.faults.iter().filter_map(|s| fill::clip_span(s, plo, phi)).collect();
                if top.is_empty() && bottom.is_empty() && fills.is_empty() && faults.is_empty() {
                    continue;
                }
                time_drawn |= e.time;
                envelopes.push((e, top, bottom, fills, faults));
            }
        }
    }
    if time_drawn {
        warnings.push(match catalogue.inputs.time_factor {
            Some(f) => format!("surfaces in time are drawn on the depth axis at {f} depth units per time unit"),
            None => "surfaces in time are drawn on the depth axis without a time scale".to_string(),
        });
    }
    // What the catalogue noticed about this section, one line per kind.
    for (code, one, many) in [
        (crate::erosion::CODE, "erosion suspected", "erosions suspected"),
        ("crossingMismatch", "crossing mismatch", "crossing mismatches"),
    ] {
        let count = catalogue.notes.iter().filter(|n| n.code == code && n.sections.contains(&key)).count();
        if count > 0 {
            warnings.push(format!("{count} {} — see the notes", if count == 1 { one } else { many }));
        }
    }

    // The window.
    // Computed whatever the view asks: `whole` is written on the drawing.
    let whole = {
        let mut e = Extent::default();
        e.add([portion.map_or(0.0, |p| p[0]), 0.0]);
        e.add([portion.map_or(sec.length, |p| p[1]), 0.0]);
        let mut z = Extent::default();
        for w in &wells {
            z.add(w.head);
            z.add_all(&w.path);
        }
        for t in &traces {
            t.spans.iter().for_each(|s| z.add_all(s));
        }
        for (_, top, bottom, fills, faults) in &envelopes {
            top.iter().chain(bottom).chain(faults).for_each(|s| z.add_all(s));
            fills.iter().for_each(|s| z.add_all(s));
        }
        Window { x: padded(e.x, 0.02), y: padded(z.y, 0.05) }
    };
    let window = view.window.unwrap_or(whole);
    // Wells wholly outside a given window are not written (the cost of a drawing is its nodes).
    wells.retain(|w| {
        view.window.is_none()
            || w.path.iter().chain(std::iter::once(&w.head)).any(|p| p[0] >= window.x[0] && p[0] <= window.x[1])
    });

    // Axes: what can be drawn.
    let mut mode = view.axis;
    if mode == AxisMode::Geodesic {
        warnings.push(
            "geodesic axis not available: the horizontal axis shows the projected distance \
             (a scale factor along the trace from energyml-proj is needed)"
                .to_string(),
        );
        mode = AxisMode::Projected;
    }
    // A paint the scene gives a well that cannot be laid on it is said now, so
    // that its line has its room under the plot; the well is drawn as before.
    for w in &wells {
        if let Some((paint, _)) = paint_of(view, w)
            && let Err(why) = crate::paint::check(w.path.len(), paint, &view.joints)
        {
            warnings.push(paint_refusal(w, &why));
        }
    }
    // The strata legend: what the wells could show, decided before the frame
    // since it takes its room on the left. What is *drawn* is read back from the
    // laid wells below, so that it is never a second reading of the colours.
    let strata = if view.strata_legend { strata_units(&wells, view) } else { Vec::new() };
    if view.strata_legend && strata.is_empty() {
        warnings.push(strata_refusal(&wells, view));
    }
    let unstratified = effective_style(view, &mut warnings);
    if unstratified == UnstratifiedStyle::Hidden && view.well_colouring == WellColouring::Strata {
        let lost = wells.iter().filter(|w| !painted(view, w) && has_unstratified(w)).count();
        if lost > 0 {
            warnings.push(format!(
                "{lost} well(s) drawn only where a stratigraphic unit is known — the portions with no strata information are not drawn"
            ));
        }
    }
    // A style that is neither the default nor hidden would be read as a unit by
    // someone who does not know it: the legend says what it is.
    let no_strata = (view.strata_legend
        && !strata.is_empty()
        && unstratified != UnstratifiedStyle::Hidden
        && !unstratified.is_default()
        && wells.iter().any(|w| !painted(view, w) && has_unstratified(w)))
    .then(|| unstratified.clone());
    // The markers: decided in the drawing's units, before the frame, because the
    // legend takes its room on the right.
    let marked = marking::resolve(view, &wells, &traces, &mut warnings);
    let strata_w = strata_width(&strata, no_strata.is_some(), view);
    let left_shift = if strata_w > 0.0 { strata_w + STRATA_GAP_PX } else { 0.0 };
    let labelled = view.show.names || view.show.distances;
    // One line more than the warnings known now: the labels folded are only
    // known once they are placed.
    let reserved = if view.show.notes { warnings.len() + usize::from(labelled && !wells.is_empty()) } else { 0 };
    // Names: in codes mode, the codes and their legend — decided before the
    // frame, since the legend takes its room on the right.
    let coded = (view.labels == LabelMode::Codes).then(|| codes(catalogue));
    let mut entries: Vec<LegendEntry> = Vec::new();
    if let Some(coded) = &coded {
        for w in &wells {
            if let Some(code) = coded.get(&w.object.key()) {
                entries.push(LegendEntry { code: code.clone(), title: well_name(&w.object), colour: NEUTRAL_COLOUR.into(), round: true });
            }
        }
        for t in &traces {
            if let Some(code) = coded.get(&t.key) {
                entries.push(LegendEntry { code: code.clone(), title: t.item.object.title.clone(), colour: t.colour.clone(), round: false });
            }
        }
        for (e, ..) in &envelopes {
            if let Some(code) = coded.get(&e.object.key()) {
                entries.push(LegendEntry { code: code.clone(), title: e.object.title.clone(), colour: envelope_colour(view, e), round: false });
            }
        }
    }
    let legend_w = legend_width(&entries);
    let markers_w = marked.width();
    let right_w = legend_w.max(markers_w);
    // The band above the plot is the wells' labels': room for
    // [`HEAD_BAND_LEVELS`] levels of them, so a crowded line of heads climbs
    // rather than spilling into the drawing.
    let label_lines = usize::from(view.show.names) + usize::from(view.show.distances);
    let banded = label_lines > 0 && !wells.is_empty();
    let line_h = (label_lines as f64 - 1.0) * LINE_PX + FONT_PX + 3.0;
    let top_for = |levels: usize| {
        if banded { TITLE_BAND_PX + levels as f64 * (line_h + 3.0) + 8.0 } else { MARGIN_TOP }
    };
    let (top, _, note_lines) = budget(view.size_px[1], HEAD_BAND_LEVELS, reserved, top_for);
    let frame = frame_of(view, note_lines, if right_w > 0.0 { right_w + 12.0 } else { 0.0 }, left_shift, top);
    let sx = frame.w / (window.x[1] - window.x[0]);
    let sy = frame.h / (window.y[1] - window.y[0]);
    let map = Map { window, frame, sx, sy };
    let exaggeration = sy / sx;
    let exaggeration_text = format!("vertical exaggeration ×{}", round_sig(exaggeration));
    let (origin_text, origin_x) = origin_line(
        view.show.origin,
        sec.trace.first().copied(),
        catalogue.inputs.system.as_ref(),
        &title,
        &exaggeration_text,
        &frame,
        &mut warnings,
    );

    let mut layers = Vec::new();

    // envelopes — the grids' fills, at the very back; their outlines come
    // after every fill (`outlines`), so that nothing coloured covers a line
    // (objects with a coloured background go to the back of the drawing so that the
    // others stay visible).
    let mut env_fills = Vec::new();
    let mut env_lines = Vec::new();
    for (e, top, bottom, fills, faults) in &envelopes {
        let key = e.object.key();
        let colour = envelope_colour(view, e);
        let polygons: Vec<Shape> = fills
            .iter()
            .map(|poly| Shape::Polygon { points: map.points(poly), fill: colour.clone(), opacity: FILL_OPACITY, stroke: None, dash: false })
            .collect();
        if !polygons.is_empty() {
            env_fills.push(LaidObject {
                attr: Attr::FillOf(key.clone()),
                kind: "fill".into(),
                title: e.object.title.clone(),
                highlighted: false,
                at: None,
                shapes: polygons,
                intervals: Vec::new(),
            });
        }
        let lines: Vec<Shape> = top
            .iter()
            .chain(bottom)
            .chain(faults)
            .map(|line| Shape::Polyline { points: map.points(line), stroke: colour.clone(), width: width_of(view, &key, 1.0), dash: false })
            .collect();
        env_lines.push(LaidObject {
            highlighted: view.highlight.contains(&key),
            attr: Attr::Key(key),
            kind: "envelope".into(),
            title: e.object.title.clone(),
            at: None,
            shapes: lines,
            intervals: Vec::new(),
        });
    }
    layers.push(Layer { name: "envelopes", clipped: true, objects: env_fills });

    // fills — a trace to its neighbour, per the view's choices.
    let lines: Vec<fill::Line<'_>> =
        traces.iter().map(|t| fill::Line { key: &t.key, item: t.item, spans: &t.spans }).collect();
    let mut fill_objects = Vec::new();
    for (t, mine) in traces.iter().zip(&lines) {
        let Some(FillChoice { up, down, .. }) = view.fills.iter().find(|f| f.key == t.key) else {
            continue;
        };
        let mut shapes = Vec::new();
        for (wanted, is_up) in [(*up, true), (*down, false)] {
            if !wanted {
                continue;
            }
            for poly in fill::polygons(mine, &lines, is_up) {
                shapes.push(Shape::Polygon {
                    points: map.points(&poly),
                    fill: t.colour.clone(),
                    opacity: FILL_OPACITY,
                    stroke: None,
                    dash: false,
                });
            }
        }
        if !shapes.is_empty() {
            fill_objects.push(LaidObject {
                attr: Attr::FillOf(t.key.clone()),
                kind: "fill".into(),
                title: t.item.object.title.clone(),
                highlighted: false,
                at: None,
                shapes,
                intervals: Vec::new(),
            });
        }
    }
    layers.push(Layer { name: "fills", clipped: true, objects: fill_objects });
    layers.push(Layer { name: "outlines", clipped: true, objects: env_lines });

    // erosions — a background of the lines, so it sits before them; a section
    // with none suspected adds no layer, and its drawing is what it was.
    let erosion_objects = erosion_marks(sec, &traces, [plo, phi], &map);
    if !erosion_objects.is_empty() {
        layers.push(Layer { name: "erosions", clipped: true, objects: erosion_objects });
    }

    // traces
    let mut trace_objects: Vec<LaidObject> = Vec::new();
    let mut names: Vec<LineName> = Vec::new();
    for t in &traces {
        let highlighted = view.highlight.contains(&t.key);
        let line_width = width_of(view, &t.key, 1.5) + if highlighted { 1.5 } else { 0.0 };
        // What the scene paints along the surface, when its cut says where
        // each point comes from: per span, else the flat colour as before.
        let paint = view.paints.iter().find(|(k, _)| *k == t.key).map(|(_, paint)| paint);
        let sources = cut_sources(t.item);
        let mut paint_refusal: Option<String> = None;
        let mut shapes: Vec<Shape> = Vec::new();
        for s in &t.spans {
            if let (Some(paint), Some(sources)) = (paint, sources)
                && let Some(from) = source_of_span(t.item, sources, s)
            {
                let px: Vec<[f64; 2]> = s.iter().map(|&p| map.px(p)).collect();
                if px.iter().all(|p| p[0].is_finite() && p[1].is_finite()) {
                    match crate::paint::surfaces::trace_pieces(&px, from, paint) {
                        Ok(pieces) => {
                            shapes.extend(pieces.into_iter().map(|piece| Shape::Polyline {
                                stroke: crate::paint::over(&piece.colour, piece.opacity, &view.background),
                                points: piece.points,
                                width: line_width,
                                dash: false,
                            }));
                            continue;
                        }
                        Err(why) => {
                            paint_refusal.get_or_insert(why);
                        }
                    }
                }
            }
            shapes.push(Shape::Polyline { points: map.points(s), stroke: t.colour.clone(), width: line_width, dash: false });
        }
        if let Some(why) = paint_refusal {
            warnings.push(format!(
                "scene colours refused for {} — {why}; the surface is drawn in its own colour",
                t.item.object.title
            ));
        }
        // Its name at the right end of its longest span, in its own colour: a
        // section of ten grey-green lines reads as one, and the eye looks for
        // the name at the end the line runs out on. Placed below, all together,
        // so two names never sit on each other (`line_names`).
        if view.show.surface_names
            && let Some(longest) = t.spans.iter().max_by(|a, b| span_extent(a).total_cmp(&span_extent(b)))
            && let Some(&end) = longest.last()
            && let Some(&end) = map.points(&[end]).first()
        {
            names.push(LineName {
                owner: trace_objects.len(),
                end,
                text: shown_name(coded.as_ref(), &t.item.object),
                colour: t.colour.clone(),
            });
        }
        trace_objects.push(LaidObject {
            attr: Attr::Key(t.key.clone()),
            kind: match t.item.role {
                TraceRole::Horizon => "horizon",
                TraceRole::Fault => "fault",
                TraceRole::Other => "surface",
            }
            .into(),
            title: t.item.object.title.clone(),
            highlighted,
            at: None,
            shapes,
            intervals: Vec::new(),
        });
    }
    // marks
    let mut mark_shapes = Vec::new();
    let mut mark_placer = labels::Placer::new(labels::Rect2 { x0: frame.x, y0: frame.y, x1: frame.x + frame.w, y1: frame.y + frame.h });
    let mut marks_unlabelled = 0usize;
    for mark in &sec.marks {
        let (s, kind, text, show) = match *mark {
            Mark::Vertex { s, index } => (s, "vertex", format!("V{index}"), view.show.vertices),
            Mark::Crossing { s, other } => {
                let text = match other {
                    SectionKey::Long => "× long".to_string(),
                    SectionKey::Cross { index } => format!("× cross {index}"),
                };
                (s, "crossing", text, view.show.crossings)
            }
        };
        if !show || !in_portion(s) || s < window.x[0] || s > window.x[1] {
            continue;
        }
        let x = map.px([s, 0.0])[0];
        let (y0, y1) = if kind == "vertex" {
            (frame.y + frame.h - 8.0, frame.y + frame.h)
        } else {
            (frame.y, frame.y + frame.h)
        };
        mark_shapes.push(Shape::Polyline {
            points: vec![[x, y0], [x, y1]],
            stroke: MARK_COLOUR.into(),
            width: 1.0,
            dash: kind == "crossing",
        });
        // Its text, in the first place that is free (on a
        // phone, nine crossings on 2 160 m wrote "× cross 0 × cross 1 …" on
        // each other). In order of preference, level by level from the edge
        // the text hugs: the whole text on the right of the line, then on its
        // left, then the short form on the right and on the left — a short text
        // on its own level reads better than a long one stacked into the plot,
        // where it covers what is drawn. The line is always drawn: a mark with
        // no place keeps it and loses only its text, and that is counted.
        let base_y = if kind == "vertex" { frame.y + frame.h - 10.0 } else { frame.y + SMALL_PX + 2.0 };
        let step = if kind == "vertex" { -MARK_LEVEL_PX } else { MARK_LEVEL_PX };
        let mut forms = vec![text.clone()];
        let short = short_mark_text(&text);
        if short != text {
            forms.push(short);
        }
        let mut candidates: Vec<(String, labels::Rect2, [f64; 2], Anchor)> = Vec::new();
        for level in 0..MARK_LEVELS {
            let y = base_y + step * level as f64;
            for form in &forms {
                let w = labels::width_px(form, SMALL_PX);
                for (text_x, anchor, x0) in [(x + 3.0, Anchor::Start, x + 3.0), (x - 3.0, Anchor::End, x - 3.0 - w)] {
                    // The same box `text_rects` reads back for this text.
                    let rect = labels::Rect2 { x0, y0: y - SMALL_PX, x1: x0 + w, y1: y + 2.0 };
                    candidates.push((form.clone(), rect, [text_x, y], anchor));
                }
            }
        }
        let rects: Vec<labels::Rect2> = candidates.iter().map(|c| c.1).collect();
        match mark_placer.place(&rects) {
            Some(at) => {
                let (text, _, at, anchor) = candidates.swap_remove(at);
                mark_shapes.push(Shape::Text { at, text, anchor, size: SMALL_PX, fill: MARK_COLOUR.into() });
            }
            None => marks_unlabelled += 1,
        }
    }
    if marks_unlabelled > 0 {
        warnings.push(format!("{marks_unlabelled} section marks unlabelled — they did not fit"));
    }
    let marks_group = if mark_shapes.is_empty() {
        Vec::new()
    } else {
        vec![LaidObject {
            attr: Attr::Decor("marks".into()),
            kind: "marks".into(),
            title: String::new(),
            highlighted: false,
            at: None,
            shapes: mark_shapes,
            intervals: Vec::new(),
        }]
    };
    // The marks' texts are where they are: the names placed after them keep clear.
    let mark_texts = text_rects(marks_group.iter().flat_map(|o| o.shapes.iter()));
    let bounds = labels::Rect2 { x0: frame.x, y0: frame.y, x1: frame.x + frame.w, y1: frame.y + frame.h };
    let (placed, names_folded) = line_names(names, bounds, &mark_texts);
    for (owner, text) in placed {
        trace_objects[owner].shapes.push(text);
    }
    if names_folded > 0 {
        warnings.push(format!("{names_folded} surface names folded — they did not fit"));
    }
    // A contour is a line under the lines it marks: behind the traces and the wells.
    let contours = marked.contours(&map);
    if !contours.is_empty() {
        layers.push(Layer { name: "marker-contours", clipped: true, objects: contours });
    }
    layers.push(Layer { name: "traces", clipped: true, objects: trace_objects });

    // wells
    let mut well_objects = Vec::new();
    for w in &wells {
        well_objects.push(laid_well(w, view, &unstratified, &map, &unit, &mut warnings));
    }
    // What the wells drew, read back for the strata legend: the units and the
    // colours of the intervals as laid.
    let drawn_units: std::collections::HashSet<(String, String)> =
        well_objects.iter().flat_map(|o| o.intervals.iter()).map(|iv| (iv.unit.clone(), iv.colour.clone())).collect();
    layers.push(Layer { name: "wells", clipped: true, objects: well_objects });

    let symbols = marked.symbols(&map);
    if !symbols.is_empty() {
        layers.push(Layer { name: "markers", clipped: true, objects: symbols });
    }

    layers.push(Layer { name: "marks", clipped: true, objects: marks_group });

    // labels
    let (label_objects, folded) = well_labels(&wells, view, &map, &unit, coded.as_ref(), &mark_texts);
    layers.push(Layer { name: "labels", clipped: false, objects: label_objects });
    let legend_box = legend(entries, frame.x + frame.w + 12.0, legend_w, frame.y, frame.y + frame.h);
    // The legend of markers goes on the right, **under** the legend of codes: that
    // margin is already reserved for a legend, the strata legend owns the left, and
    // the drawing's own frame stays clear.
    let codes_bottom = legend_box.as_ref().and_then(marking::bottom_of);
    layers.push(Layer { name: "legend", clipped: false, objects: legend_box.into_iter().collect() });
    if !marked.is_empty() {
        let top = codes_bottom.map_or(frame.y, |b| b + 8.0);
        let objects = marked.legend(frame.x + frame.w + 12.0, markers_w, top, frame.y + frame.h);
        layers.push(Layer { name: MARKERS_LAYER, clipped: false, objects });
    }
    if folded > 0 {
        warnings.push(format!("{folded} well labels folded — they did not fit"));
    }
    if view.strata_legend {
        let shown: Vec<StrataEntry> = strata.into_iter().filter(|e| drawn_units.contains(&(e.unit.clone(), e.colour.clone()))).collect();
        let objects = strata_legend(&shown, view, no_strata.as_ref(), STRATA_LEFT_PX, strata_w, frame.y, frame.y + frame.h);
        layers.push(Layer { name: STRATA_LAYER, clipped: false, objects });
    }

    // axes
    let x_axis = x_axis(sec, &map, mode, &unit);
    let y_axis = y_axis(&map, &unit, "z");

    Layout {
        target: view.target,
        title,
        size: view.size_px,
        background: view.background.clone(),
        frame,
        window,
        whole,
        scale: [sx, sy],
        exaggeration: Some(exaggeration),
        exaggeration_text,
        origin_text,
        origin_x,
        x_axis,
        y_axis,
        layers,
        warnings,
        folded_labels: folded,
        note_lines,
        metadata: metadata(catalogue, view),
        left_shift,
    }
}

/// The colour of a suspected erosion: its own, apart from every surface's.
const EROSION_COLOUR: &str = "#d81b60";

/// The suspected erosions of a section, laid out: the lens between the two
/// surfaces over the portion, and the older surface thickened there — it is the
/// one that should stop. Only what is drawn counts: a hidden surface marks
/// nothing, and a portion cut to the view's `portion`.
fn erosion_marks(sec: &Section, traces: &[KeptTrace<'_>], [plo, phi]: [f64; 2], map: &Map) -> Vec<LaidObject> {
    let mut out = Vec::new();
    for portion in crate::erosion::of_section(sec) {
        let (Some(older), Some(younger)) = (
            traces.iter().find(|t| t.item.object == portion.older),
            traces.iter().find(|t| t.item.object == portion.younger),
        ) else {
            continue;
        };
        let (lo, hi) = (portion.s0.max(plo), portion.s1.min(phi));
        if hi <= lo {
            continue;
        }
        let part = |t: &KeptTrace<'_>| t.item.spans.iter().find_map(|s| fill::clip_span(s, lo, hi));
        let (Some(upper), Some(lower)) = (part(older), part(younger)) else { continue };
        let mut ring = upper.clone();
        ring.extend(lower.iter().rev().copied());
        out.push(LaidObject {
            attr: Attr::Decor("erosion".into()),
            kind: "erosion".into(),
            title: format!("{} / {}", portion.older.title, portion.younger.title),
            highlighted: false,
            at: None,
            shapes: vec![
                Shape::Polygon { points: map.points(&ring), fill: EROSION_COLOUR.into(), opacity: FILL_OPACITY, stroke: None, dash: false },
                Shape::Polyline { points: map.points(&upper), stroke: EROSION_COLOUR.into(), width: 3.0, dash: true },
            ],
            intervals: Vec::new(),
        });
    }
    out
}

/// Levels a mark's text may drop to, from the edge it hugs.
const MARK_LEVELS: usize = 3;
/// The distance between two levels: a text's height plus the air the
/// placer keeps between two labels, so a lower level is clear of the upper.
const MARK_LEVEL_PX: f64 = SMALL_PX + 2.0 + labels::GAP_PX;

/// The short form of a mark's text: "× cross 3" is "×3", "× long" is "×L".
/// A vertex's "V3" is short already and comes back as it is.
fn short_mark_text(text: &str) -> String {
    match text.strip_prefix("× cross ") {
        Some(n) => format!("×{n}"),
        None if text == "× long" => "×L".to_string(),
        None => text.to_string(),
    }
}

/// Pixels of line per chevron on a long segment.
const CHEVRON_EVERY_PX: f64 = 100.0;
/// A chevron keeps this far (along the line) from a vertex.
const CHEVRON_CLEAR_PX: f64 = 6.0;
/// A chevron's half length along the line, and half spread across it.
const CHEVRON_ALONG_PX: f64 = 3.0;
const CHEVRON_ACROSS_PX: f64 = 3.5;

/// Chevrons (`>` laid on the line) and a dot at the start, for a section's line
/// in pixels: the way the abscissa grows from point 0. One per
/// [`CHEVRON_EVERY_PX`] of each segment and at least one per segment, so a bend
/// shows its new direction; each oriented along **its** segment, evenly
/// spaced inside it and clear of its vertices. A segment too short to keep
/// clear of both (under twice [`CHEVRON_CLEAR_PX`]) gets none.
fn direction_marks(px: &[[f64; 2]], colour: &str) -> Vec<Shape> {
    let mut out = Vec::new();
    let Some(first) = px.first() else { return out };
    for pair in px.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        if len < 2.0 * CHEVRON_CLEAR_PX {
            continue;
        }
        let (ux, uy) = ((b[0] - a[0]) / len, (b[1] - a[1]) / len);
        let (nx, ny) = (-uy, ux);
        let n = ((len / CHEVRON_EVERY_PX).floor() as usize).max(1);
        for i in 0..n {
            let t = (i as f64 + 0.5) / n as f64 * len;
            let c = [a[0] + ux * t, a[1] + uy * t];
            let tip = [c[0] + ux * CHEVRON_ALONG_PX, c[1] + uy * CHEVRON_ALONG_PX];
            let back = [c[0] - ux * CHEVRON_ALONG_PX, c[1] - uy * CHEVRON_ALONG_PX];
            let left = [back[0] + nx * CHEVRON_ACROSS_PX, back[1] + ny * CHEVRON_ACROSS_PX];
            let right = [back[0] - nx * CHEVRON_ACROSS_PX, back[1] - ny * CHEVRON_ACROSS_PX];
            out.push(Shape::Polyline { points: vec![left, tip, right], stroke: colour.to_string(), width: 1.2, dash: false });
        }
    }
    if !out.is_empty() {
        out.push(Shape::Circle { c: *first, r: 2.5, fill: colour.to_string() });
    }
    out
}

/// The title's font size and the air kept between the title band's texts.
const TITLE_PX: f64 = 14.0;
const TITLE_GAP_PX: f64 = 16.0;

/// Where a drawing's first point is, for its title band: `origin (s = 0): X 500000.0 ·
/// Y 6000000.0 — EPSG:2154`. The band is shared with the title (left, bold) and
/// the exaggeration (right): the line goes at the centre of the free space
/// between them, in the form that fits — whole, then the EPSG code alone, then
/// nothing — and a shortened or dropped one is counted in `warnings`, like the
/// marks that find no room. Widths are the embedded font's, the title's with
/// the margin a bold face needs. The vertical system is not written: the
/// catalogue does not carry it.
fn origin_line(
    on: bool,
    origin: Option<[f64; 2]>,
    system: Option<&crate::model::System>,
    title: &str,
    right_text: &str,
    frame: &Rect,
    warnings: &mut Vec<String>,
) -> (String, f64) {
    let (true, Some([x, y])) = (on, origin) else { return (String::new(), 0.0) };
    if !x.is_finite() || !y.is_finite() {
        return (String::new(), 0.0);
    }
    let code = system.map(|s| format!("EPSG:{}", s.epsg));
    let whole = match &code {
        Some(c) => format!("origin (s = 0): X {x:.1} · Y {y:.1} — {c}"),
        None => format!("origin (s = 0): X {x:.1} · Y {y:.1}"),
    };
    let left = frame.x + labels::width_px(title, TITLE_PX) * 1.1 + TITLE_GAP_PX;
    let right = frame.x + frame.w - labels::width_px(right_text, FONT_PX) - TITLE_GAP_PX;
    let room = right - left;
    let put = |text: String| {
        let w = labels::width_px(&text, FONT_PX);
        let x = (frame.x + frame.w / 2.0 - w / 2.0).clamp(left, (right - w).max(left));
        (text, x)
    };
    if labels::width_px(&whole, FONT_PX) <= room {
        return put(whole);
    }
    if let Some(c) = code {
        let short = format!("origin: {c}");
        if labels::width_px(&short, FONT_PX) <= room {
            warnings.push("origin position shortened to its EPSG code — the title band is too narrow".to_string());
            return put(short);
        }
    }
    warnings.push("origin position not written — the title band is too narrow".to_string());
    (String::new(), 0.0)
}

/// The exaggeration as it is written: three significant figures.
fn round_sig(v: f64) -> String {
    if !v.is_finite() || v <= 0.0 {
        return "?".to_string();
    }
    let decimals = (2 - v.log10().floor() as i64).clamp(0, 6) as usize;
    let text = format!("{v:.decimals$}");
    if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_string() } else { text }
}

fn x_axis(sec: &Section, map: &Map, mode: AxisMode, unit: &str) -> Axis {
    let (step, values) = ticks::values(map.window.x[0], map.window.x[1], map.frame.w / 90.0);
    let suffix = if unit.is_empty() { String::new() } else { format!(" ({unit})") };
    let ticks = values
        .into_iter()
        .map(|v| {
            let sublabel = (mode == AxisMode::Coordinates && sec.trace.len() >= 2).then(|| {
                let [x, y] = energyml_geometry::fence::point_at(&sec.trace, v);
                format!("{x:.0}, {y:.0}")
            });
            Tick { value: v, at: map.px([v, 0.0])[0], label: ticks::label(v, step), sublabel }
        })
        .collect();
    Axis {
        title: match mode {
            AxisMode::Coordinates => format!("distance along the trace{suffix} — true coordinates (x, y)"),
            _ => format!("distance along the trace{suffix}"),
        },
        mode,
        ticks,
    }
}

fn y_axis(map: &Map, unit: &str, name: &str) -> Axis {
    let (step, values) = ticks::values(map.window.y[0], map.window.y[1], map.frame.h / 40.0);
    let suffix = if unit.is_empty() { String::new() } else { format!(" ({unit})") };
    Axis {
        title: format!("{name}{suffix}"),
        mode: AxisMode::Projected,
        ticks: values
            .into_iter()
            .map(|v| Tick { value: v, at: map.px([0.0, v])[1], label: ticks::label(v, step), sublabel: None })
            .collect(),
    }
}

/// The colour of a well's whole line, per the view's choice.
fn well_colour(w: &WellItem, view: &View) -> String {
    let side = match w.side {
        Side::Left => LEFT_COLOUR,
        Side::Right => RIGHT_COLOUR,
    };
    match view.well_colouring {
        WellColouring::Side => side.to_string(),
        // The scene's colour — or, when the scene gives none, a neutral one
        // rather than a side colour the user did not ask for.
        WellColouring::Scene => colour_of(view, &w.object.key()).unwrap_or_else(|| NEUTRAL_COLOUR.to_string()),
        // Under strata, the line itself is the well's scene colour or neutral:
        // an interval with no colour is drawn in it, never left transparent.
        WellColouring::Strata => colour_of(view, &w.object.key()).unwrap_or_else(|| NEUTRAL_COLOUR.to_string()),
    }
}

/// Where each point of a surface's cut comes from (`SurfaceCut::sources`),
/// parallel to its spans — what lets `paint::surfaces::trace_pieces` lay a
/// per-node colour along it. **The catalogue's `TraceItem` does not carry it
/// yet** (only `SurfaceCut` does, and `assemble` does not copy it): until the
/// contract gives the item a `sources` field this answers `None`, and every
/// surface is drawn in its flat colour. The one line to change is here.
fn cut_sources(item: &TraceItem) -> Option<&[Vec<CutSource>]> {
    (!item.sources.is_empty()).then_some(item.sources.as_slice())
}

/// The sources of one kept span: the original span it still is, point for
/// point — a span a portion cut (or whose repeated points were dropped) is no
/// longer parallel to its sources and is not painted.
fn source_of_span<'a>(item: &TraceItem, sources: &'a [Vec<CutSource>], span: &[[f64; 2]]) -> Option<&'a [CutSource]> {
    item.spans.iter().zip(sources).find(|(own, from)| own.as_slice() == span && from.len() == span.len()).map(|(_, from)| from.as_slice())
}

/// The paint the scene gives a well: its own key's first, and — one line being
/// the frame and the trajectory of one wellbore, folded — then the keys
/// of the objects folded into it, in the order the catalogue lists them. The
/// first that has one wins; the line that is drawn is the kept object's, so a
/// paint given to the frame is the common case and the others are the fallback
/// (a front end that resolved the trajectory's colours, not the frame's).
fn paint_of<'a>(view: &'a View, w: &WellItem) -> Option<(&'a Paint, String)> {
    // **Only under scene colouring**, which is what it means: "the colour the
    // scene gives them". Under strata colouring the strata are what was asked,
    // and a well the scene happens to paint kept neither its units nor its ticks
    // (the black ticks at the stratigraphic changes would vanish and the colour would
    // look uniform along the boreholes).
    if view.well_colouring != WellColouring::Scene {
        return None;
    }
    std::iter::once(w.object.key()).chain(w.merged.iter().map(|o| o.key())).find_map(|key| {
        view.paints.iter().find(|(k, _)| *k == key).map(|(_, paint)| (paint, key))
    })
}

fn paint_refusal(w: &WellItem, why: &str) -> String {
    format!("scene colours refused for {} — {why}; the well is drawn in its own colour", well_name(&w.object))
}

/// The MD at a pixel point of a well's laid path: the nearest segment's, the
/// two nodes' MDs interpolated.
fn md_at_px(path_px: &[[f64; 2]], md: &[f64], p: [f64; 2]) -> f64 {
    let mut best = (f64::INFINITY, md.first().copied().unwrap_or(0.0));
    for (i, pair) in path_px.windows(2).enumerate() {
        let (a, b) = (pair[0], pair[1]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
        let (ex, ey) = (a[0] + dx * t - p[0], a[1] + dy * t - p[1]);
        let d2 = ex * ex + ey * ey;
        if d2 < best.0 {
            best = (d2, md[i] + (md[i + 1] - md[i]) * t);
        }
    }
    best.1
}

fn laid_well(w: &WellItem, view: &View, unstratified: &UnstratifiedStyle, map: &Map, unit: &str, warnings: &mut Vec<String>) -> LaidObject {
    let key = w.object.key();
    let colour = well_colour(w, view);
    let highlighted = view.highlight.contains(&key);
    let width = width_of(view, &key, view.well_width_px.max(1.0));
    let path = map.points(&w.path);
    let mut shapes = Vec::new();
    if highlighted {
        shapes.push(Shape::Polyline { points: path.clone(), stroke: "#ffb300".into(), width: width + 5.0, dash: false });
    }

    // What the scene paints along the well, under scene colouring, when it gives
    // something that fits: the pieces replace the single line — the scene's
    // paint is what the scene shows. A paint that does not fit was
    // said before the frame (`check`); the well is then drawn as below.
    let painted = paint_of(view, w).filter(|(paint, _)| crate::paint::check(w.path.len(), paint, &view.joints).is_ok());
    if let Some((paint, _)) = painted {
        // One pixel per node, none dropped and none merged: the paint is read per node.
        let path_px: Vec<[f64; 2]> = w.path.iter().map(|&p| map.px(p)).collect();
        match crate::paint::well_pieces(&path_px, paint, &view.joints) {
            Ok(pieces) => {
                shapes.push(Shape::Circle { c: map.px(w.head), r: (width / 2.0 + 1.5).max(2.5), fill: colour.clone() });
                let md = w.md.as_ref().filter(|md| md.len() == w.path.len());
                // The writer's interval has a butt cap: a gap between two
                // pieces stays a gap (a polyline's round cap would close it).
                let intervals = pieces
                    .into_iter()
                    .map(|piece| {
                        let (from, to) = match (md, piece.points.first(), piece.points.last()) {
                            (Some(md), Some(&a), Some(&b)) => (md_at_px(&path_px, md, a), md_at_px(&path_px, md, b)),
                            _ => (0.0, 0.0),
                        };
                        let stroke = crate::paint::over(&piece.colour, piece.opacity, &view.background);
                        LaidInterval { from, to, unit: String::new(), unit_uri: None, colour: stroke, width, points: piece.points, tick: None }
                    })
                    .collect();
                return LaidObject {
                    attr: Attr::Key(key),
                    kind: "well".into(),
                    title: w.object.title.clone(),
                    highlighted,
                    at: Some(w.head),
                    shapes,
                    intervals,
                };
            }
            Err(why) => warnings.push(paint_refusal(w, &why)),
        }
    }

    // The line itself is the well's colour (`well_colour`: the scene's, or the
    // neutral grey) and shows wherever no interval is drawn over it — the dark
    // portions of a well with no strata information. Not a "no information"
    // style, only what lies under the intervals. Off: only the intervals draw.
    if view.well_colouring != WellColouring::Strata || unstratified.is_default() {
        // The line under the intervals — what a drawing always was, byte for byte.
        shapes.push(Shape::Polyline { points: path, stroke: colour.clone(), width, dash: false });
    } else {
        shapes.extend(unstratified_shapes(w, map, unstratified, width));
    }
    shapes.push(Shape::Circle { c: map.px(w.head), r: (width / 2.0 + 1.5).max(2.5), fill: colour.clone() });

    let mut intervals = Vec::new();
    let mut last_to: Option<f64> = None;
    if let Some(md) = w.md.as_ref().filter(|md| md.len() == w.path.len()) {
        for iv in &w.intervals {
            let pts = sub_path(&w.path, md, iv.from, iv.to);
            let points = map.points(&pts);
            if points.len() < 2 {
                continue;
            }
            // Without a colour of its own, the interval takes the well's.
            let drawn = interval_colour(iv, &w.object.store, &colour, view);
            let tick = if view.strata_ticks && view.well_colouring == WellColouring::Strata {
                let adjacent = last_to.is_some_and(|before| (before - iv.from).abs() < 1e-9);
                if adjacent { tick_across(&points, width) } else { None }
            } else {
                None
            };
            last_to = Some(iv.to);
            intervals.push(LaidInterval { from: iv.from, to: iv.to, unit: iv.unit.clone(), unit_uri: iv.unit_uri.clone(), colour: drawn, width, points, tick });
        }
    }
    let _ = unit;
    LaidObject {
        attr: Attr::Key(key),
        kind: "well".into(),
        title: w.object.title.clone(),
        highlighted,
        at: Some(w.head),
        shapes,
        intervals,
    }
}

/// The style the unstratified portions are drawn in: the view's, or the default
/// with a note naming why when it cannot be drawn (`UnstratifiedStyle::refusal`).
fn effective_style(view: &View, warnings: &mut Vec<String>) -> UnstratifiedStyle {
    let style = view.unstratified_style();
    match style.refusal() {
        None => style,
        Some(why) => {
            warnings.push(format!("unstratified style not drawn: {why}; the default is drawn"));
            UnstratifiedStyle::default()
        }
    }
}

/// The id of the `<pattern>` the hatched style strokes with — prefixed, so that
/// it cannot clash with an id of a drawing this one is pasted into.
pub const HATCH_ID: &str = "esv-unstratified-hatch";

/// What the stroke or fill of the hatched style is: the pattern's reference.
pub fn hatch_ref() -> String {
    format!("url(#{HATCH_ID})")
}

/// The portions of a well's path (as MD intervals) that **no interval covers**,
/// between its first and last MD. `None`: the well has no MD to lay intervals
/// on, so the whole path is one portion. **The one reading** of "unstratified":
/// the drawing, the note counting lost wells and the legend all ask it.
pub fn unstratified_spans(w: &WellItem) -> Option<Vec<(f64, f64)>> {
    let md = w.md.as_ref().filter(|md| md.len() == w.path.len() && md.len() > 1)?;
    let (lo, hi) = (md[0], md[md.len() - 1]);
    let mut covered: Vec<(f64, f64)> = w.intervals.iter().map(|iv| (iv.from.max(lo), iv.to.min(hi))).filter(|(a, b)| b > a).collect();
    covered.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    let mut reached = lo;
    for (a, b) in covered {
        if a > reached + 1e-9 {
            out.push((reached, a));
        }
        reached = reached.max(b);
    }
    if reached < hi - 1e-9 {
        out.push((reached, hi));
    }
    Some(out)
}

/// The unstratified portions of a well, drawn in `style` (not the default, which
/// is the well's line under its intervals): one shape per portion, laid along
/// the path between its two MDs.
fn unstratified_shapes(w: &WellItem, map: &Map, style: &UnstratifiedStyle, width: f64) -> Vec<Shape> {
    let pieces: Vec<Vec<[f64; 2]>> = match (unstratified_spans(w), w.md.as_ref()) {
        (Some(spans), Some(md)) => spans.into_iter().map(|(a, b)| map.points(&sub_path(&w.path, md, a, b))).collect(),
        _ => vec![map.points(&w.path)],
    };
    pieces
        .into_iter()
        .filter(|points| points.len() >= 2)
        .filter_map(|points| match style {
            UnstratifiedStyle::Hidden => None,
            UnstratifiedStyle::Solid { colour } => Some(Shape::Polyline { points, stroke: colour.clone(), width, dash: false }),
            UnstratifiedStyle::Dashed { colour, dash, gap } => {
                Some(Shape::Dashed { points, stroke: colour.clone(), width, dash: *dash, gap: *gap })
            }
            UnstratifiedStyle::Hatched { .. } => Some(Shape::Polyline { points, stroke: hatch_ref(), width, dash: false }),
        })
        .collect()
}

/// How far a boundary tick reaches beyond the well's own edge, pixels.
const TICK_OVERHANG_PX: f64 = 2.0;

/// The tick across a well at the start of `points` (an interval's piece of path):
/// perpendicular to the path there, centred on it, as wide as the well and a
/// little more. `None` when the piece has no direction.
fn tick_across(points: &[[f64; 2]], width: f64) -> Option<[[f64; 2]; 2]> {
    let (a, b) = (points.first()?, points.get(1)?);
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length = dx.hypot(dy);
    // A `NaN` length is no direction either: `is_finite` says so, `> 0.0` the rest.
    if !length.is_finite() || length <= 0.0 {
        return None;
    }
    let half = width / 2.0 + TICK_OVERHANG_PX;
    let (nx, ny) = (-dy / length * half, dx / length * half);
    Some([[a[0] - nx, a[1] - ny], [a[0] + nx, a[1] + ny]])
}

/// The colour a unit is given **now** — [`View::unit_colours`], by the key of the
/// well's package and the unit's full uri — else the one baked into the interval
/// when the catalogue was made. **The one reading** both the wells and the strata
/// legend make of "the colour of an interval", so they cannot disagree.
fn own_colour(iv: &Interval, store: &str, view: &View) -> Option<String> {
    let told = iv.unit_uri.as_deref().and_then(|uri| {
        let key = crate::model::object_key(store, uri);
        view.unit_colours.iter().find(|(k, _)| *k == key).map(|(_, colour)| colour.clone())
    });
    told.or_else(|| iv.colour.clone())
}

/// The colour an interval is drawn in: its own under strata colouring, the
/// well's when the file gives it none or the wells are coloured another way.
/// **The one place** it is decided — the strata legend asks the same.
fn interval_colour(iv: &Interval, store: &str, well_colour: &str, view: &View) -> String {
    match view.well_colouring {
        WellColouring::Strata => own_colour(iv, store, view).unwrap_or_else(|| well_colour.to_string()),
        _ => well_colour.to_string(),
    }
}

/// The piece of a path between two MDs, the ends interpolated.
fn sub_path(path: &[[f64; 2]], md: &[f64], from: f64, to: f64) -> Vec<[f64; 2]> {
    let mut out: Vec<[f64; 2]> = Vec::new();
    for i in 0..path.len().saturating_sub(1) {
        let (m0, m1) = (md[i], md[i + 1]);
        let (lo, hi) = (from.max(m0), to.min(m1));
        if hi < lo || m1 <= m0 {
            continue;
        }
        let at = |m: f64| {
            let t = (m - m0) / (m1 - m0);
            [path[i][0] + (path[i + 1][0] - path[i][0]) * t, path[i][1] + (path[i + 1][1] - path[i][1]) * t]
        };
        let a = at(lo);
        if out.last() != Some(&a) {
            out.push(a);
        }
        out.push(at(hi));
    }
    out
}

/// A well's name: its title, or the head of its uuid when the title is empty.
fn well_name(object: &crate::model::ObjectId) -> String {
    if object.title.is_empty() { object.uuid.chars().take(8).collect() } else { object.title.clone() }
}

fn side_letter(side: Side) -> &'static str {
    match side {
        Side::Left => "L",
        Side::Right => "R",
    }
}

/// The wells' labels, placed without overlap ([`head_labels`]); the second value
/// is how many were folded. In codes mode a well is named by its code.
fn well_labels(
    wells: &[&WellItem],
    view: &View,
    map: &Map,
    unit: &str,
    coded: Option<&HashMap<String, String>>,
    obstacles: &[labels::Rect2],
) -> (Vec<LaidObject>, usize) {
    if !(view.show.names || view.show.distances) {
        return (Vec::new(), 0);
    }
    let mut cands = Vec::new();
    for w in wells {
        let head = map.px(w.head);
        if head[0] < map.frame.x || head[0] > map.frame.x + map.frame.w {
            continue;
        }
        let mut lines = Vec::new();
        if view.show.names {
            lines.push(labels::fit(&shown_name(coded, &w.object), FONT_PX, labels::MAX_WIDTH_PX));
        }
        if view.show.distances {
            let space = if unit.is_empty() { "" } else { " " };
            lines.push(format!("{:.0}{space}{unit} {}", w.d, side_letter(w.side)));
        }
        // A head near the top of the plot is labelled above the plot — the
        // labels do not cover the data, the marks or each other's wells.
        let floor = if head[1] < map.frame.y + map.frame.h * 0.2 { map.frame.y - 2.0 } else { head[1] - 4.0 };
        cands.push(HeadLabel { key: w.object.key(), title: w.object.title.clone(), head, lines, floor });
    }
    // Anywhere below the drawing's title and left of the legend.
    let bounds =
        labels::Rect2 { x0: 2.0, y0: TITLE_BAND_PX, x1: map.frame.x + map.frame.w, y1: map.frame.y + map.frame.h };
    head_labels(cands, bounds, obstacles)
}

// ---------------------------------------------------------------------------
// The plan

fn plan(catalogue: &Catalogue, view: &View) -> Layout {
    let p: &Plan = &catalogue.plan;
    let mut warnings = common_warnings(catalogue);
    warnings.extend(width_warnings(view));
    let unit = unit_of(catalogue);
    if view.axis != AxisMode::Projected {
        warnings.push("the plan is drawn in local plan coordinates whatever the axis mode".to_string());
    }
    let wells: Vec<_> = p.wells.iter().filter(|w| !is_hidden(view, &w.object.key())).collect();

    let mut e = Extent::default();
    for (_, t) in &p.sections {
        e.add_all(t);
    }
    if view.show.bands {
        for (_, b) in &p.bands {
            e.add_all(b);
        }
    }
    for w in &wells {
        if w.rejected.is_none() || view.show.rejected {
            e.add(w.head);
        }
        for (_, f) in &w.feet {
            e.add(*f);
        }
    }
    let given = view.window;
    let everything = Window { x: padded(e.x, 0.05), y: padded(e.y, 0.05) };
    let wanted = given.unwrap_or(everything);
    if view.strata_legend {
        warnings.push("strata legend not drawn: the plan shows no strata along its wells".to_string());
    }
    if !view.markers.is_empty() {
        warnings.push(format!("{} marker(s) not drawn: markers are drawn in sections only", view.markers.len()));
    }
    let labelled = view.show.names;
    let reserved = if view.show.notes { warnings.len() + usize::from(labelled && !wells.is_empty()) } else { 0 };
    // Codes mode: the same codes as on every section, and their legend.
    let coded = (view.labels == LabelMode::Codes).then(|| codes(catalogue));
    let mut entries: Vec<LegendEntry> = Vec::new();
    if let Some(coded) = &coded {
        for w in &wells {
            if w.rejected.is_some() && !view.show.rejected {
                continue;
            }
            if let Some(code) = coded.get(&w.object.key()) {
                entries.push(LegendEntry { code: code.clone(), title: well_name(&w.object), colour: NEUTRAL_COLOUR.into(), round: true });
            }
        }
    }
    let legend_w = legend_width(&entries);
    let (_, _, note_lines) = budget(view.size_px[1], 1, reserved, |_| PLAN_TOP);
    let frame = frame_of(view, note_lines, if legend_w > 0.0 { legend_w + 12.0 } else { 0.0 }, 0.0, PLAN_TOP);
    // Equal scales: the window is widened, never one axis stretched.
    let equal = |w: Window| {
        let scale = (frame.w / (w.x[1] - w.x[0])).min(frame.h / (w.y[1] - w.y[0]));
        let (cx, cy) = ((w.x[0] + w.x[1]) / 2.0, (w.y[0] + w.y[1]) / 2.0);
        (
            scale,
            Window {
                x: [cx - frame.w / scale / 2.0, cx + frame.w / scale / 2.0],
                y: [cy - frame.h / scale / 2.0, cy + frame.h / scale / 2.0],
            },
        )
    };
    let (scale, window) = equal(wanted);
    let whole = if given.is_some() { equal(everything).1 } else { window };
    let map = Map { window, frame, sx: scale, sy: scale };

    let mut layers = Vec::new();
    let mut section_objects = Vec::new();
    if view.show.bands {
        for (k, band) in &p.bands {
            section_objects.push(LaidObject {
                attr: Attr::Section(key_text(*k)),
                kind: "band".into(),
                title: String::new(),
                highlighted: false,
                at: None,
                shapes: vec![Shape::Polygon {
                    points: map.points(band),
                    fill: "#90a4ae".into(),
                    opacity: 0.15,
                    stroke: Some("#90a4ae".into()),
                    dash: true,
                }],
                intervals: Vec::new(),
            });
        }
    }
    layers.push(Layer { name: "fills", clipped: true, objects: section_objects });

    let mut lines = Vec::new();
    let mut directions = Vec::new();
    for (k, t) in &p.sections {
        if view.show.direction {
            let colour = if *k == SectionKey::Long { INK } else { MARK_COLOUR };
            let shapes = direction_marks(&map.points(t), colour);
            if !shapes.is_empty() {
                directions.push(LaidObject {
                    attr: Attr::Decor(format!("direction-{}", key_text(*k))),
                    kind: "direction".into(),
                    title: String::new(),
                    highlighted: false,
                    at: None,
                    shapes,
                    intervals: Vec::new(),
                });
            }
        }
        lines.push(LaidObject {
            attr: Attr::Section(key_text(*k)),
            kind: "section".into(),
            title: String::new(),
            highlighted: false,
            at: None,
            shapes: vec![Shape::Polyline {
                points: map.points(t),
                stroke: if *k == SectionKey::Long { INK } else { MARK_COLOUR }.into(),
                width: if *k == SectionKey::Long { 2.0 } else { 1.2 },
                dash: false,
            }],
            intervals: Vec::new(),
        });
    }
    lines.extend(directions);
    layers.push(Layer { name: "traces", clipped: true, objects: lines });

    let mut well_objects = Vec::new();
    for w in &wells {
        let key = w.object.key();
        let out = w.rejected.is_some();
        if out && !view.show.rejected {
            continue;
        }
        let colour = if out { "#9e9e9e".to_string() } else { colour_of(view, &key).unwrap_or_else(|| NEUTRAL_COLOUR.into()) };
        let head = map.px(w.head);
        let mut shapes = Vec::new();
        for (_, foot) in &w.feet {
            shapes.push(Shape::Polyline {
                points: vec![head, map.px(*foot)],
                stroke: colour.clone(),
                width: width_of(view, &key, 1.0),
                dash: true,
            });
        }
        shapes.push(Shape::Circle { c: head, r: 3.5, fill: colour });
        well_objects.push(LaidObject {
            highlighted: view.highlight.contains(&key),
            attr: Attr::Key(key),
            kind: if out { "well-rejected" } else { "well" }.into(),
            title: w.object.title.clone(),
            at: Some(w.head),
            shapes,
            intervals: Vec::new(),
        });
    }
    layers.push(Layer { name: "wells", clipped: true, objects: well_objects });
    layers.push(Layer { name: "marks", clipped: true, objects: Vec::new() });

    // Labels: names only, above the head, by the same placer as in a section.
    let mut label_objects = Vec::new();
    let mut folded = 0;
    if view.show.names {
        let mut cands = Vec::new();
        for w in &wells {
            if w.rejected.is_some() && !view.show.rejected {
                continue;
            }
            let head = map.px(w.head);
            if head[0] < frame.x || head[0] > frame.x + frame.w || head[1] < frame.y || head[1] > frame.y + frame.h {
                continue;
            }
            cands.push(HeadLabel {
                key: w.object.key(),
                title: w.object.title.clone(),
                head,
                lines: vec![labels::fit(&shown_name(coded.as_ref(), &w.object), FONT_PX, labels::MAX_WIDTH_PX)],
                floor: head[1] - 4.0,
            });
        }
        // A label never covers a head: a head is what the plan is read for.
        let heads: Vec<labels::Rect2> = cands
            .iter()
            .map(|c| labels::Rect2 { x0: c.head[0] - 4.0, y0: c.head[1] - 4.0, x1: c.head[0] + 4.0, y1: c.head[1] + 4.0 })
            .collect();
        let bounds = labels::Rect2 { x0: frame.x, y0: frame.y, x1: frame.x + frame.w, y1: frame.y + frame.h };
        (label_objects, folded) = head_labels(cands, bounds, &heads);
    }
    layers.push(Layer { name: "labels", clipped: false, objects: label_objects });
    let legend_box = legend(entries, frame.x + frame.w + 12.0, legend_w, frame.y, frame.y + frame.h);
    layers.push(Layer { name: "legend", clipped: false, objects: legend_box.into_iter().collect() });
    if folded > 0 {
        warnings.push(format!("{folded} well labels folded — they did not fit"));
    }

    let (xs, xv) = ticks::values(window.x[0], window.x[1], frame.w / 90.0);
    let x_axis = Axis {
        title: format!("x, local{}", if unit.is_empty() { String::new() } else { format!(" ({unit})") }),
        mode: AxisMode::Projected,
        ticks: xv
            .into_iter()
            .map(|v| Tick { value: v, at: map.px([v, 0.0])[0], label: ticks::label(v, xs), sublabel: None })
            .collect(),
    };
    let y_axis = y_axis(&map, &unit, "y, local");

    let (origin_text, origin_x) = origin_line(
        view.show.origin,
        Some(catalogue.plan.origin),
        catalogue.inputs.system.as_ref().or(catalogue.plan.system.as_ref()),
        "Plan",
        "scale 1:1 on both axes",
        &frame,
        &mut warnings,
    );

    Layout {
        target: view.target,
        title: "Plan".to_string(),
        size: view.size_px,
        background: view.background.clone(),
        frame,
        window,
        whole,
        scale: [scale, scale],
        exaggeration: None,
        exaggeration_text: "scale 1:1 on both axes".to_string(),
        origin_text,
        origin_x,
        x_axis,
        y_axis,
        layers,
        warnings,
        folded_labels: folded,
        note_lines,
        metadata: metadata(catalogue, view),
        left_shift: 0.0,
    }
}

// ---------------------------------------------------------------------------
// Names: placed without overlap, or shortened to codes with a legend

/// How many levels above its head a well's label may climb before it is folded.
const HEAD_LEVELS: usize = 5;
/// The levels of well labels the band above a section's plot is made for.
const HEAD_BAND_LEVELS: usize = 3;
/// The drawing's title band, which no label enters.
const TITLE_BAND_PX: f64 = 22.0;
/// How many steps a surface's name may slide left along its line.
const NAME_SHIFTS: usize = 6;
/// The legend's width bounds, pixels.
const LEGEND_MIN_PX: f64 = 120.0;
const LEGEND_MAX_PX: f64 = 280.0;

/// The code of every object the catalogue draws — `W` wells, `H` horizons, `F`
/// faults, `S` other surfaces, `G` grids — numbered in **one order for the whole
/// catalogue**, so an object keeps its code on every section and on the plan:
/// wells by their abscissa on the long section, then by cross section; horizons
/// and other surfaces top first (their mean height where first cut); faults
/// along the trace.
fn codes(catalogue: &Catalogue) -> HashMap<String, String> {
    let sections: Vec<&Section> = std::iter::once(&catalogue.long).chain(&catalogue.crosses).collect();
    let mut wells: Vec<(String, usize, f64)> = Vec::new();
    // (key, family, sort value) — the first cut met decides.
    let mut surfaces: Vec<(String, char, f64)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (rank, sec) in sections.iter().enumerate() {
        for item in &sec.items {
            match item {
                Item::Well(w) => {
                    if seen.insert(w.object.key()) {
                        wells.push((w.object.key(), rank, w.s));
                    }
                }
                Item::Trace(t) => {
                    if seen.insert(t.object.key()) {
                        let (family, value) = match t.role {
                            TraceRole::Fault => ('F', mean_of(&t.spans, 0)),
                            TraceRole::Horizon => ('H', -mean_of(&t.spans, 1)),
                            TraceRole::Other => ('S', -mean_of(&t.spans, 1)),
                        };
                        surfaces.push((t.object.key(), family, rank as f64 * 1e12 + value));
                    }
                }
                Item::Envelope(e) => {
                    if seen.insert(e.object.key()) {
                        surfaces.push((e.object.key(), 'G', rank as f64 * 1e12 - mean_of(&e.top, 1)));
                    }
                }
            }
        }
    }
    for w in &catalogue.plan.wells {
        let key = w.object.key();
        if seen.insert(key.clone()) {
            wells.push((key, usize::MAX, 0.0));
        }
    }
    wells.sort_by(|a, b| a.1.cmp(&b.1).then(a.2.total_cmp(&b.2)));
    let mut out: HashMap<String, String> = HashMap::new();
    for (n, (key, _, _)) in wells.into_iter().enumerate() {
        out.insert(key, format!("W{n}"));
    }
    for family in ['H', 'F', 'S', 'G'] {
        let mut these: Vec<&(String, char, f64)> = surfaces.iter().filter(|s| s.1 == family).collect();
        these.sort_by(|a, b| a.2.total_cmp(&b.2));
        for (n, (key, _, _)) in these.into_iter().enumerate() {
            out.insert(key.clone(), format!("{family}{n}"));
        }
    }
    out
}

/// The mean of one coordinate over spans (0: s, 1: z); 0 when there is none.
fn mean_of(spans: &[Vec<[f64; 2]>], axis: usize) -> f64 {
    let (sum, n) = spans.iter().flatten().fold((0.0, 0usize), |(sum, n), p| (sum + p[axis], n + 1));
    if n == 0 { 0.0 } else { sum / n as f64 }
}

/// What an object is called on the drawing: its code in codes mode, else its
/// title (or the head of its uuid).
fn shown_name(codes: Option<&HashMap<String, String>>, object: &crate::model::ObjectId) -> String {
    codes.and_then(|c| c.get(&object.key()).cloned()).unwrap_or_else(|| well_name(object))
}

/// A label that hangs above a head: a well's name and distance.
struct HeadLabel {
    key: String,
    title: String,
    head: [f64; 2],
    lines: Vec<String>,
    /// The lowest a label's box may reach, pixels.
    floor: f64,
}

/// Labels above heads, without overlap (on overlap a label goes
/// higher, possibly shifted to a side with no label nearby, and a dotted leader
/// joins it to the head when the name is far away). Each label tries, level by level going up, its own place centred
/// on the head, then a whole width to the right, then to the left; a label not
/// centred straight above its head gets a **dotted leader** to it. A label that
/// finds no place within [`HEAD_LEVELS`] levels is folded and counted.
fn head_labels(mut cands: Vec<HeadLabel>, bounds: labels::Rect2, obstacles: &[labels::Rect2]) -> (Vec<LaidObject>, usize) {
    cands.sort_by(|a, b| a.head[0].total_cmp(&b.head[0]));
    let mut placer = labels::Placer::new(bounds);
    obstacles.iter().for_each(|r| placer.block(*r));
    let mut out = Vec::new();
    let mut folded = 0;
    for c in cands {
        let n = c.lines.len().max(1) as f64;
        let w = c.lines.iter().map(|l| labels::width_px(l, FONT_PX)).fold(0.0, f64::max);
        let h = (n - 1.0) * LINE_PX + FONT_PX + 3.0;
        let mut spots: Vec<(labels::Rect2, f64, f64)> = Vec::new(); // rect, centre x, last baseline
        // Up first, straight above the head; then a little aside (the right
        // first: the labels are taken left to right, so the right is where
        // nothing is yet); a whole width aside last, which clears a neighbour
        // centred on almost the same head.
        for dx in [0.0, w / 2.0 + 8.0, -(w / 2.0 + 8.0), w + 8.0, -(w + 8.0)] {
            for level in 0..HEAD_LEVELS {
                let baseline = c.head[1] - 7.0 - level as f64 * (h + 3.0);
                let cx = c.head[0] + dx;
                let rect =
                    labels::Rect2 { x0: cx - w / 2.0, y0: baseline + 3.0 - h, x1: cx + w / 2.0, y1: baseline + 3.0 };
                // A label never goes down into the plot past its own head's
                // band: above the plot when the head is near its top.
                if rect.y1 <= c.floor {
                    spots.push((rect, cx, baseline));
                }
            }
        }
        let rects: Vec<labels::Rect2> = spots.iter().map(|s| s.0).collect();
        let Some(at) = placer.place(&rects) else {
            folded += 1;
            continue;
        };
        let (rect, cx, baseline) = spots[at];
        let mut shapes = Vec::new();
        if at > 0 {
            // Not straight above its head: a dotted line says whose label it is.
            shapes.push(Shape::Polyline {
                points: vec![[cx, rect.y1], [c.head[0], c.head[1] - 4.0]],
                stroke: "#888888".into(),
                width: 0.7,
                dash: true,
            });
        }
        let lines = c.lines.len();
        for (j, line) in c.lines.iter().enumerate() {
            shapes.push(Shape::Text {
                at: [cx, baseline - (lines - 1 - j) as f64 * LINE_PX],
                text: line.clone(),
                anchor: Anchor::Middle,
                size: FONT_PX,
                fill: INK.into(),
            });
        }
        out.push(LaidObject {
            attr: Attr::LabelOf(c.key),
            kind: "label".into(),
            title: c.title,
            highlighted: false,
            at: None,
            shapes,
            intervals: Vec::new(),
        });
    }
    (out, folded)
}

/// The boxes of the texts among `shapes`, measured with the sheet's table — what
/// a placer must keep clear of.
fn text_rects<'a>(shapes: impl Iterator<Item = &'a Shape>) -> Vec<labels::Rect2> {
    shapes
        .filter_map(|shape| match shape {
            Shape::Text { at, text, anchor, size, .. } => {
                let w = labels::width_px(text, *size);
                let x0 = match anchor {
                    Anchor::Start => at[0],
                    Anchor::Middle => at[0] - w / 2.0,
                    Anchor::End => at[0] - w,
                };
                Some(labels::Rect2 { x0, y0: at[1] - size, x1: x0 + w, y1: at[1] + 2.0 })
            }
            _ => None,
        })
        .collect()
}

/// A surface's name to place at the end of its line.
struct LineName {
    /// Which trace object (index into the traces' layer) it belongs to.
    owner: usize,
    end: [f64; 2],
    text: String,
    colour: String,
}

/// Surface names without overlap: each name first just above
/// the end of its line, then just below, then slid left along the line by its
/// own width — above, below — step by step; the names are taken top first, so a
/// name far from the others finds its first place free (the *reset*). A name
/// with no place within [`NAME_SHIFTS`] steps is folded and counted.
fn line_names(mut names: Vec<LineName>, bounds: labels::Rect2, obstacles: &[labels::Rect2]) -> (Vec<(usize, Shape)>, usize) {
    names.sort_by(|a, b| a.end[1].total_cmp(&b.end[1]));
    let mut placer = labels::Placer::new(bounds);
    obstacles.iter().for_each(|r| placer.block(*r));
    let mut out = Vec::new();
    let mut folded = 0;
    for n in names {
        let w = labels::width_px(&n.text, SMALL_PX);
        let mut spots: Vec<(labels::Rect2, [f64; 2])> = Vec::new(); // rect, text anchor (end, baseline)
        for k in 0..NAME_SHIFTS {
            let right = n.end[0] - 3.0 - k as f64 * (w + 10.0);
            let above = n.end[1] - 3.0;
            let below = n.end[1] + SMALL_PX + 2.0;
            for baseline in [above, below] {
                let rect = labels::Rect2 { x0: right - w, y0: baseline - SMALL_PX, x1: right, y1: baseline + 2.0 };
                spots.push((rect, [right, baseline]));
            }
        }
        let rects: Vec<labels::Rect2> = spots.iter().map(|s| s.0).collect();
        match placer.place(&rects) {
            Some(at) => out.push((
                n.owner,
                Shape::Text { at: spots[at].1, text: n.text, anchor: Anchor::End, size: SMALL_PX, fill: n.colour },
            )),
            None => folded += 1,
        }
    }
    (out, folded)
}

/// The layer the strata legend is written in (`svg.rs` gives it a group of its own).
pub(crate) const STRATA_LAYER: &str = "strata-legend";
/// The layer the legend of markers is written in (an Inkscape layer of its own, like the strata's).
pub(crate) const MARKERS_LAYER: &str = "markers-legend";
/// Where the strata legend starts from the drawing's left edge, pixels.
const STRATA_LEFT_PX: f64 = 4.0;
/// The space between the strata legend and the axes' own margin, pixels.
const STRATA_GAP_PX: f64 = 8.0;

/// One unit the wells show, with the colour its interval is drawn in — and the
/// packages whose wells show it so. A unit is one entry per colour, not per
/// package: two packages that agree are one line, two that do not are two.
#[derive(Clone, Debug, PartialEq, Eq)]
struct StrataEntry {
    unit: String,
    colour: String,
    /// The unit's full uri, when its intervals name one.
    unit_uri: Option<String>,
    /// The front end's store ids, in the order their wells were met.
    stores: Vec<String>,
}

impl StrataEntry {
    /// The same unit in the same colour — what merges two wells' intervals into
    /// one entry, whatever package each well is in.
    fn same_line(&self, unit: &str, colour: &str, unit_uri: Option<&str>) -> bool {
        self.unit == unit && self.colour == colour && self.unit_uri.as_deref() == unit_uri
    }

    /// The same unit, whatever its colour: by its uri, or by its name when it
    /// has none.
    fn same_unit(&self, other: &StrataEntry) -> bool {
        match (&self.unit_uri, &other.unit_uri) {
            (Some(a), Some(b)) => a == b,
            (None, None) => self.unit == other.unit,
            _ => false,
        }
    }
}

/// The title each entry is written with: the unit's, and — when the same unit
/// shows in another entry too, in another colour — the names of the packages
/// whose wells show it in this one. One place, read by the width and by the
/// drawing, so the box is as wide as what it says.
fn strata_titles(entries: &[StrataEntry], view: &View) -> Vec<String> {
    let name = |store: &str| {
        view.store_names
            .iter()
            .find(|(id, _)| id == store)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| store.to_string())
    };
    entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let title = unit_title(&e.unit);
            let twice = entries.iter().enumerate().any(|(j, other)| j != i && other.same_unit(e));
            if twice && !e.stores.is_empty() {
                format!("{title} — {}", e.stores.iter().map(|s| name(s)).collect::<Vec<_>>().join(", "))
            } else {
                title
            }
        })
        .collect()
}

fn painted(view: &View, w: &WellItem) -> bool {
    paint_of(view, w).is_some_and(|(paint, _)| crate::paint::check(w.path.len(), paint, &view.joints).is_ok())
}

/// The units the wells could show in a legend, in stratigraphic order — top
/// down along the wells, so the younger first. Only an interval with a colour of
/// its own counts (without one it takes its well's, which says nothing of the
/// unit), under strata colouring, on a well the scene does not paint (its
/// pieces carry no unit). The order is the one the wells agree on: a unit
/// comes before the one under it in any well; where they disagree, or where no
/// well relates two units, the first met (wells left to right) comes first.
fn strata_units(wells: &[&WellItem], view: &View) -> Vec<StrataEntry> {
    if view.well_colouring != WellColouring::Strata {
        return Vec::new();
    }
    let mut nodes: Vec<StrataEntry> = Vec::new();
    let mut edges: Vec<(usize, usize)> = Vec::new();
    for w in wells {
        if painted(view, w) || w.md.as_ref().is_none_or(|md| md.len() != w.path.len()) {
            continue;
        }
        let mut intervals: Vec<&Interval> = w.intervals.iter().filter(|iv| own_colour(iv, &w.object.store, view).is_some()).collect();
        intervals.sort_by(|a, b| a.from.total_cmp(&b.from));
        let mut previous: Option<usize> = None;
        for iv in intervals {
            let colour = interval_colour(iv, &w.object.store, "", view);
            let at = match nodes.iter().position(|n| n.same_line(&iv.unit, &colour, iv.unit_uri.as_deref())) {
                Some(at) => {
                    if !nodes[at].stores.contains(&w.object.store) {
                        nodes[at].stores.push(w.object.store.clone());
                    }
                    at
                }
                None => {
                    nodes.push(StrataEntry {
                        unit: iv.unit.clone(),
                        colour,
                        unit_uri: iv.unit_uri.clone(),
                        stores: vec![w.object.store.clone()],
                    });
                    nodes.len() - 1
                }
            };
            if let Some(before) = previous.filter(|&p| p != at) {
                edges.push((before, at));
            }
            previous = Some(at);
        }
    }
    // A stable topological order; a cycle (wells that disagree) breaks at the
    // first unit met.
    let mut waiting: Vec<usize> = vec![0; nodes.len()];
    for &(_, to) in &edges {
        waiting[to] += 1;
    }
    let mut done = vec![false; nodes.len()];
    let mut order = Vec::with_capacity(nodes.len());
    while order.len() < nodes.len() {
        let next = (0..nodes.len()).find(|&i| !done[i] && waiting[i] == 0).or_else(|| (0..nodes.len()).find(|&i| !done[i]));
        let Some(next) = next else { break };
        done[next] = true;
        order.push(nodes[next].clone());
        for &(from, to) in &edges {
            if from == next && waiting[to] > 0 {
                waiting[to] -= 1;
            }
        }
    }
    order
}

/// Whether some part of a well's path (by MD) is covered by no interval — or the
/// well has no MD to lay intervals on, so none of it is.
fn has_unstratified(w: &WellItem) -> bool {
    unstratified_spans(w).is_none_or(|spans| !spans.is_empty())
}

/// Why the strata legend asked for is not drawn — one note, naming the cause.
fn strata_refusal(wells: &[&WellItem], view: &View) -> String {
    if view.well_colouring != WellColouring::Strata {
        return "strata legend not drawn: the wells are not coloured by strata".to_string();
    }
    if wells.is_empty() {
        return "strata legend not drawn: no well is drawn".to_string();
    }
    let by_scene = wells.iter().filter(|w| painted(view, w)).count();
    if by_scene > 0 {
        format!(
            "strata legend not drawn: no unit with a colour of its own on the wells drawn \
             ({by_scene} well(s) are painted by the scene, whose colours carry no unit)"
        )
    } else {
        "strata legend not drawn: no well shows a stratigraphic unit with a colour of its own".to_string()
    }
}

/// The strata legend's width for these units, pixels — `0` when there is none.
fn strata_width(entries: &[StrataEntry], no_strata: bool, view: &View) -> f64 {
    if entries.is_empty() {
        return 0.0;
    }
    let widest = strata_titles(entries, view)
        .iter()
        .map(|title| labels::width_px(title, SMALL_PX))
        .chain(no_strata.then(|| labels::width_px(NO_STRATA_TITLE, SMALL_PX)))
        .fold(labels::width_px("Stratigraphy", FONT_PX), f64::max);
    (widest + 36.0).clamp(LEGEND_MIN_PX, LEGEND_MAX_PX)
}

/// What the legend calls the portions no interval covers, when they are drawn in
/// a style of their own.
const NO_STRATA_TITLE: &str = "No strata";

fn unit_title(unit: &str) -> String {
    if unit.is_empty() { "(unnamed unit)".to_string() } else { unit.to_string() }
}

/// The strata legend, in a box at `x` from `top` down to at most `bottom`: a
/// heading, then a swatch and a title per unit — **one object per unit**, with
/// the unit's title and the colour its interval is drawn in. What does not fit is
/// said on its last line. Nothing when there is no unit: never an empty box.
fn strata_legend(entries: &[StrataEntry], view: &View, no_strata: Option<&UnstratifiedStyle>, x: f64, width: f64, top: f64, bottom: f64) -> Vec<LaidObject> {
    if entries.is_empty() {
        return Vec::new();
    }
    let room = (((bottom - top) - LINE_PX - 8.0) / LINE_PX).floor().max(1.0) as usize;
    let extra = usize::from(no_strata.is_some());
    let shown = if entries.len() + extra > room { room.saturating_sub(1 + extra) } else { entries.len() };
    let height = LINE_PX * (shown + 1 + extra + usize::from(shown < entries.len())) as f64 + 10.0;
    let decor = |name: &str, title: String, shapes: Vec<Shape>| LaidObject {
        attr: Attr::Decor(name.into()),
        kind: name.into(),
        title,
        highlighted: false,
        at: None,
        shapes,
        intervals: Vec::new(),
    };
    let mut frame = vec![
        Shape::Polygon {
            points: vec![[x, top], [x + width, top], [x + width, top + height], [x, top + height]],
            fill: "#ffffff".into(),
            opacity: 0.92,
            stroke: Some("#999999".into()),
            dash: false,
        },
        Shape::Text { at: [x + 8.0, top + LINE_PX], text: "Stratigraphy".into(), anchor: Anchor::Start, size: FONT_PX, fill: INK.into() },
    ];
    if shown < entries.len() {
        frame.push(Shape::Text {
            at: [x + 8.0, top + LINE_PX * ((shown + extra) as f64 + 2.0)],
            text: format!("+ {} more units", entries.len() - shown),
            anchor: Anchor::Start,
            size: SMALL_PX,
            fill: INK.into(),
        });
    }
    let mut out = vec![decor("strata-box", String::new(), frame)];
    let titles = strata_titles(entries, view);
    for (i, e) in entries.iter().take(shown).enumerate() {
        let y = top + LINE_PX * (i as f64 + 2.0);
        let title = titles[i].clone();
        let mut entry = decor(
            "strata-entry",
            e.unit.clone(),
            vec![
                Shape::Polygon {
                    points: vec![[x + 8.0, y - 9.0], [x + 22.0, y - 9.0], [x + 22.0, y - 1.0], [x + 8.0, y - 1.0]],
                    fill: e.colour.clone(),
                    opacity: 1.0,
                    stroke: Some("#666666".into()),
                    dash: false,
                },
                Shape::Text {
                    at: [x + 28.0, y],
                    text: labels::fit(&title, SMALL_PX, (width - 28.0 - 6.0).max(20.0)),
                    anchor: Anchor::Start,
                    size: SMALL_PX,
                    fill: INK.into(),
                },
            ],
        );
        if let Some(uri) = &e.unit_uri {
            entry.attr = Attr::UnitEntry("strata-entry".into(), uri.clone(), e.stores.clone());
        }
        out.push(entry);
    }
    if let Some(style) = no_strata {
        // After the units: the portions no unit covers, in the style they are drawn in.
        let y = top + LINE_PX * (shown as f64 + 2.0);
        let swatch = [[x + 8.0, y - 9.0], [x + 22.0, y - 9.0], [x + 22.0, y - 1.0], [x + 8.0, y - 1.0]];
        let boxed = |fill: String| Shape::Polygon { points: swatch.to_vec(), fill, opacity: 1.0, stroke: Some("#666666".into()), dash: false };
        let mut shapes = match style {
            UnstratifiedStyle::Solid { colour } => vec![boxed(colour.clone())],
            UnstratifiedStyle::Dashed { colour, dash, gap } => vec![Shape::Dashed {
                points: vec![[x + 6.0, y - 5.0], [x + 24.0, y - 5.0]],
                stroke: colour.clone(),
                width: 3.0,
                dash: *dash,
                gap: *gap,
            }],
            UnstratifiedStyle::Hatched { .. } => vec![boxed(hatch_ref())],
            UnstratifiedStyle::Hidden => Vec::new(),
        };
        shapes.push(Shape::Text {
            at: [x + 28.0, y],
            text: labels::fit(NO_STRATA_TITLE, SMALL_PX, (width - 28.0 - 6.0).max(20.0)),
            anchor: Anchor::Start,
            size: SMALL_PX,
            fill: INK.into(),
        });
        out.push(decor("strata-none", String::new(), shapes));
    }
    out
}

/// One line of the legend of codes.
struct LegendEntry {
    code: String,
    title: String,
    colour: String,
    round: bool,
}

/// The legend's width for these entries, pixels — `0` when there is none.
fn legend_width(entries: &[LegendEntry]) -> f64 {
    if entries.is_empty() {
        return 0.0;
    }
    let widest = entries
        .iter()
        .map(|e| labels::width_px(&e.code, SMALL_PX) + 6.0 + labels::width_px(&e.title, SMALL_PX))
        .fold(0.0, f64::max);
    (widest + 30.0).clamp(LEGEND_MIN_PX, LEGEND_MAX_PX)
}

/// The legend of codes, in a box at `x`, from `top` down to at most `bottom`: a
/// swatch, the code, the title cut to the box. What does not fit is said on its
/// last line, never dropped silently.
fn legend(mut entries: Vec<LegendEntry>, x: f64, width: f64, top: f64, bottom: f64) -> Option<LaidObject> {
    if entries.is_empty() {
        return None;
    }
    let family = |c: &str| ["W", "H", "F", "S", "G"].iter().position(|f| c.starts_with(f)).unwrap_or(9);
    let number = |c: &str| c[1..].parse::<usize>().unwrap_or(0);
    entries.sort_by(|a, b| family(&a.code).cmp(&family(&b.code)).then(number(&a.code).cmp(&number(&b.code))));
    let room = (((bottom - top) - LINE_PX - 8.0) / LINE_PX).floor().max(1.0) as usize;
    let shown = if entries.len() > room { room.saturating_sub(1) } else { entries.len() };
    let height = LINE_PX * (shown + 1 + usize::from(shown < entries.len())) as f64 + 10.0;
    let mut shapes = vec![
        Shape::Polygon {
            points: vec![[x, top], [x + width, top], [x + width, top + height], [x, top + height]],
            fill: "#ffffff".into(),
            opacity: 0.92,
            stroke: Some("#999999".into()),
            dash: false,
        },
        Shape::Text {
            at: [x + 8.0, top + LINE_PX],
            text: "Legend".into(),
            anchor: Anchor::Start,
            size: FONT_PX,
            fill: INK.into(),
        },
    ];
    let code_w = entries.iter().map(|e| labels::width_px(&e.code, SMALL_PX)).fold(0.0, f64::max);
    for (i, e) in entries.iter().take(shown).enumerate() {
        let y = top + LINE_PX * (i as f64 + 2.0);
        if e.round {
            shapes.push(Shape::Circle { c: [x + 12.0, y - 4.0], r: 3.5, fill: e.colour.clone() });
        } else {
            shapes.push(Shape::Polyline {
                points: vec![[x + 6.0, y - 4.0], [x + 18.0, y - 4.0]],
                stroke: e.colour.clone(),
                width: 3.0,
                dash: false,
            });
        }
        shapes.push(Shape::Text {
            at: [x + 24.0, y],
            text: e.code.clone(),
            anchor: Anchor::Start,
            size: SMALL_PX,
            fill: INK.into(),
        });
        let room = width - 24.0 - code_w - 12.0;
        shapes.push(Shape::Text {
            at: [x + 24.0 + code_w + 6.0, y],
            text: labels::fit(&e.title, SMALL_PX, room.max(20.0)),
            anchor: Anchor::Start,
            size: SMALL_PX,
            fill: INK.into(),
        });
    }
    if shown < entries.len() {
        shapes.push(Shape::Text {
            at: [x + 8.0, top + LINE_PX * (shown as f64 + 2.0)],
            text: format!("+ {} more — in the objects list", entries.len() - shown),
            anchor: Anchor::Start,
            size: SMALL_PX,
            fill: INK.into(),
        });
    }
    Some(LaidObject {
        attr: Attr::Decor("legend".into()),
        kind: "legend".into(),
        title: "Legend".into(),
        highlighted: false,
        at: None,
        shapes,
        intervals: Vec::new(),
    })
}
