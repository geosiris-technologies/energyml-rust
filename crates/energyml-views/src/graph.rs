//! The boxes and the arrows the graph views share.
//!
//! A graph view (the provenance graph, the schema of the tables) decides what its nodes and edges are and how they
//! look ([`BoxLook`], [`EdgeLook`]); `energyml_sheet::draw::layered` places them ([`place`]); this module puts what
//! is placed ([`Placed`]) into a display list ([`draw`]): keyed boxes, measured text, arrowheads, the edges drawn
//! against the flow dashed, a legend. No view writes this a second time.
//!
//! Text is measured with `energyml_sheet::draw::text` (the font the SVG is written in), never guessed from a
//! character count; a line wider than a box may be is cut with `...`.

use energyml_sheet::draw::layered::{
    BoxSpec, Flow, LayerInput, LayeredOptions, Methods, Placed, RouteKind, Spacing, layered,
};
use energyml_sheet::draw::text::{char_width_pt, string_width_pt};
use energyml_sheet::draw::{
    Anchor, Colour, DisplayList, Element, Item, Layer, PageUnit, Paint, PathData, Rect, Segment, Stroke,
};

use crate::kind::ViewNote;
use crate::refusal::ViewRefusal;
use crate::setting::{Settled, ViewSetting};
use crate::style;

/// The ways the layers follow each other, the first the default.
pub const FLOWS: &[&str] = &["left_right", "top_down"];

/// The orders inside a layer, the first the default (the ids of `OrderingMethods`).
pub const ORDERINGS: &[&str] = &["barycenter", "median"];

/// The room above the graph: the title.
pub const HEADER: f64 = 34.0;

/// The dash of an edge drawn against the flow.
pub const BACK_DASH: [f64; 2] = [5.0, 4.0];

/// The most notes a view lists one by one.
pub const MOST_NOTES: usize = 20;

const PAD_X: f64 = 10.0;
const PAD_Y: f64 = 6.0;
const ACCENT_WIDTH: f64 = 5.0;
const MIN_BOX_WIDTH: f64 = 96.0;
const MAX_BOX_WIDTH: f64 = 280.0;
const LINE_GAP: f64 = 4.0;
const ARROW_LENGTH: f64 = 8.0;
const ARROW_HALF_WIDTH: f64 = 3.5;
const MARGIN: f64 = 16.0;
const LEGEND_LINE: f64 = 14.0;
const LEGEND_SAMPLE: f64 = 28.0;

/// How a line of a box reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// A small muted caption: what the box is.
    Caption,
    /// The body: its name.
    Body,
    /// A body line that is the key of something (a key column).
    Key,
    /// A body line that is secondary.
    Muted,
}

impl Tone {
    fn size(self) -> f64 {
        match self {
            Tone::Caption => style::SMALL,
            Tone::Body | Tone::Key | Tone::Muted => style::TEXT,
        }
    }

    fn ink(self) -> Colour {
        match self {
            Tone::Caption | Tone::Muted => style::MUTED,
            Tone::Body => style::INK,
            Tone::Key => style::SUBJECT,
        }
    }
}

/// One line of text in a box.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// The words.
    pub text: String,
    /// How it reads.
    pub tone: Tone,
}

impl Line {
    /// A line of this tone.
    pub fn new(text: impl Into<String>, tone: Tone) -> Self {
        Line {
            text: text.into(),
            tone,
        }
    }
}

/// The outline of a box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    /// An ordinary node.
    Plain,
    /// The node the view is about.
    Subject,
    /// Named and not held, or not resolved: dashed.
    Ghost,
}

/// A node as it is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxLook {
    /// What a user points at: the full uri, and what tells it apart.
    pub key: String,
    /// `data-kind` of the element (`node`, `table`...).
    pub kind: &'static str,
    /// The tooltip.
    pub tip: String,
    /// The lines of text, already cut to fit ([`BoxLook::new`]).
    pub lines: Vec<Line>,
    /// The outline.
    pub frame: Frame,
    /// A stripe of colour at the left edge; only a colour the file gives.
    pub accent: Option<Colour>,
    /// Other `data-*` attributes of the element.
    pub data: Vec<(String, String)>,
}

impl BoxLook {
    /// A plain box, its lines cut to the widest a box may be.
    pub fn new(key: impl Into<String>, kind: &'static str, lines: Vec<Line>) -> Self {
        let room = MAX_BOX_WIDTH - 2.0 * PAD_X - ACCENT_WIDTH;
        let lines = lines
            .into_iter()
            .map(|line| Line {
                text: fit(&line.text, line.tone.size(), room),
                tone: line.tone,
            })
            .collect();
        BoxLook {
            key: key.into(),
            kind,
            tip: String::new(),
            lines,
            frame: Frame::Plain,
            accent: None,
            data: Vec::new(),
        }
    }

    /// The size the box needs for its lines: what the placement is given.
    pub fn spec(&self) -> BoxSpec {
        let widest = self
            .lines
            .iter()
            .map(|line| string_width_pt(&line.text, line.tone.size()))
            .fold(0.0, f64::max);
        let height: f64 = self.lines.iter().map(|line| line.tone.size() + LINE_GAP).sum();
        BoxSpec {
            width: (widest + 2.0 * PAD_X + ACCENT_WIDTH).max(MIN_BOX_WIDTH),
            height: height + 2.0 * PAD_Y,
        }
    }
}

/// An edge as it is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeLook {
    /// What a user points at: the keys of its two ends, and what kind of edge it is.
    pub key: String,
    /// The tooltip.
    pub tip: String,
    /// The colour of the line and of its head.
    pub colour: Colour,
    /// The width of the line.
    pub width: f64,
    /// Dash and gap, user units; empty: solid. An edge drawn against the flow is dashed whatever this says.
    pub dash: Vec<f64>,
    /// Words written along the edge.
    pub label: Option<String>,
    /// Other `data-*` attributes of the element.
    pub data: Vec<(String, String)>,
}

/// One entry of the legend: a sample of a line and what it says.
#[derive(Clone, Debug, PartialEq)]
pub struct LegendEntry {
    /// What the line means.
    pub label: String,
    /// Its colour.
    pub colour: Colour,
    /// Its width.
    pub width: f64,
    /// Its dash; empty: solid.
    pub dash: Vec<f64>,
}

/// A text cut to `max_width`, ending in `...` when something had to go.
pub fn fit(text: &str, size: f64, max_width: f64) -> String {
    if string_width_pt(text, size) <= max_width {
        return text.to_string();
    }
    let dots = "...";
    let room = max_width - string_width_pt(dots, size);
    let mut out = String::new();
    let mut used = 0.0;
    for ch in text.chars() {
        let width = char_width_pt(ch, size);
        if used + width > room {
            break;
        }
        out.push(ch);
        used += width;
    }
    out.push_str(dots);
    out
}

/// The settings every graph view takes to place its graph: `flow` and `ordering`.
pub fn layout_settings() -> Vec<ViewSetting> {
    vec![
        ViewSetting::choice(
            "flow",
            FLOWS,
            "which way the layers follow each other: left to right, or top down",
        )
        .default(serde_json::Value::from(FLOWS[0])),
        ViewSetting::choice(
            "ordering",
            ORDERINGS,
            "how the boxes of a layer are ordered to cross the fewest edges",
        )
        .default(serde_json::Value::from(ORDERINGS[0])),
    ]
}

/// The options of the placement for the settings of [`layout_settings`], and a layering when the view names one.
pub fn layered_options(settings: &Settled, layering: Option<&str>) -> LayeredOptions {
    let defaults = LayeredOptions::default();
    LayeredOptions {
        flow: match settings.text("flow") {
            Some("top_down") => Flow::TopToBottom,
            _ => Flow::LeftToRight,
        },
        ordering: settings.text("ordering").unwrap_or(&defaults.ordering).to_string(),
        layering: layering.unwrap_or(&defaults.layering).to_string(),
        spacing: Spacing {
            layer_gap: 88.0,
            node_gap: 18.0,
            ..Spacing::default()
        },
        ..defaults
    }
}

/// The graph placed, or the placement's refusal by its own words.
///
/// # Errors
///
/// [`ViewRefusal::Computing`] with what the placement says (too many nodes for the bound, an unknown method...).
pub fn place(view: &str, input: &LayerInput, options: &LayeredOptions) -> Result<Placed, ViewRefusal> {
    layered(input, options, &Methods::default()).map_err(|why| ViewRefusal::computing(view, why))
}

/// At most [`MOST_NOTES`] notes: the rest are counted in one last note.
pub fn capped(mut notes: Vec<ViewNote>) -> Vec<ViewNote> {
    if notes.len() > MOST_NOTES {
        let more = notes.len() - (MOST_NOTES - 1);
        notes.truncate(MOST_NOTES - 1);
        notes.push(ViewNote {
            key: "more-notes",
            says: format!("{more} more notes are not listed"),
        });
    }
    notes
}

fn shifted(path: &mut PathData, by: [f64; 2]) {
    let go = |p: &mut [f64; 2]| {
        p[0] += by[0];
        p[1] += by[1];
    };
    for segment in &mut path.segments {
        match segment {
            Segment::MoveTo(p) | Segment::LineTo(p) => go(p),
            Segment::CubicTo { c1, c2, to } => {
                go(c1);
                go(c2);
                go(to);
            }
            Segment::Close => {}
        }
    }
}

/// The corners of an arrowhead whose tip is the end of the last piece, turned the way the curve arrives.
fn arrow(last: &[[f64; 2]; 4], by: [f64; 2]) -> Option<Vec<[f64; 2]>> {
    let end = last[3];
    let from = [last[2], last[1], last[0]]
        .into_iter()
        .find(|p| (p[0] - end[0]).hypot(p[1] - end[1]) > 1.0e-6)?;
    let (dx, dy) = (end[0] - from[0], end[1] - from[1]);
    let length = dx.hypot(dy);
    let (ux, uy) = (dx / length, dy / length);
    let base = [end[0] - ux * ARROW_LENGTH, end[1] - uy * ARROW_LENGTH];
    let (px, py) = (-uy * ARROW_HALF_WIDTH, ux * ARROW_HALF_WIDTH);
    Some(vec![
        [end[0] + by[0], end[1] + by[1]],
        [base[0] + px + by[0], base[1] + py + by[1]],
        [base[0] - px + by[0], base[1] - py + by[1]],
    ])
}

/// The point halfway along a piece `[start, c1, c2, end]` of a cubic curve.
fn middle(piece: &[[f64; 2]; 4]) -> [f64; 2] {
    [
        (piece[0][0] + 3.0 * piece[1][0] + 3.0 * piece[2][0] + piece[3][0]) / 8.0,
        (piece[0][1] + 3.0 * piece[1][1] + 3.0 * piece[2][1] + piece[3][1]) / 8.0,
    ]
}

/// The display list of a graph placed: the layers `edges`, `nodes`, `title` and, with entries, `legend`.
///
/// `boxes` and `edges` are in the order of the nodes and edges the placement was given. The page is the graph's own
/// size: the title above it, the swing of the edges drawn against the flow inside it, the legend under it.
///
/// # Errors
///
/// [`ViewRefusal::Computing`] when the looks are not one per node and one per edge of the placement.
pub fn draw(
    view: &str,
    title: &str,
    placed: &Placed,
    boxes: &[BoxLook],
    edges: &[EdgeLook],
    legend: &[LegendEntry],
) -> Result<DisplayList, ViewRefusal> {
    if boxes.len() != placed.boxes.len() || edges.len() != placed.routes.len() {
        return Err(ViewRefusal::computing(
            view,
            format!(
                "{} boxes and {} edges drawn over a placement of {} and {}",
                boxes.len(),
                edges.len(),
                placed.boxes.len(),
                placed.routes.len()
            ),
        ));
    }
    let by = [0.0, HEADER];

    // The room: the placement, and every point an edge reaches (a back edge swings out below).
    let mut right = placed.size[0];
    let mut bottom = placed.size[1];
    for route in &placed.routes {
        for piece in &route.segments {
            for point in piece {
                right = right.max(point[0] + 8.0);
                bottom = bottom.max(point[1] + 8.0);
            }
        }
    }
    let title_width = string_width_pt(title, style::TITLE) + 2.0 * MARGIN;
    let legend_width = legend
        .iter()
        .map(|entry| 12.0 + LEGEND_SAMPLE + 6.0 + string_width_pt(&entry.label, style::TEXT) + MARGIN)
        .fold(0.0, f64::max);
    let width = right.max(title_width).max(legend_width);
    let legend_height = if legend.is_empty() {
        0.0
    } else {
        10.0 + LEGEND_LINE * legend.len() as f64 + 8.0
    };
    let height = HEADER + bottom + legend_height;

    let mut list = DisplayList::new([width, height], PageUnit::Pixel);
    list.title = format!("{view} - {title}");
    list.background = Some(style::PAPER);

    // Edges, under the boxes.
    let mut under = Layer::new("edges");
    for (route, look) in placed.routes.iter().zip(edges) {
        let mut path = route.path();
        shifted(&mut path, by);
        let mut dash = look.dash.clone();
        if route.kind == RouteKind::Back {
            dash = BACK_DASH.to_vec();
        }
        let mut items = vec![Item::Path {
            path,
            fill: None,
            stroke: Some(Stroke {
                dash,
                ..Stroke::solid(look.colour, look.width)
            }),
        }];
        if let Some(head) = route.segments.last().and_then(|last| arrow(last, by)) {
            items.push(Item::Polygon {
                points: head,
                fill: Some(Paint::Solid(look.colour)),
                stroke: None,
            });
        }
        if let (Some(label), Some(piece)) = (&look.label, route.segments.get(route.segments.len() / 2)) {
            let at = middle(piece);
            items.push(style::label_item(
                [at[0] + by[0], at[1] + by[1] - 4.0],
                label.clone(),
                style::text(style::SMALL, style::MUTED, Anchor::Middle),
            ));
        }
        let mut element = Element::keyed(look.key.clone(), "edge", items);
        element.title = look.tip.clone();
        element.data = look.data.clone();
        under.elements.push(element);
    }
    list.layers.push(under);

    // Boxes.
    let mut nodes = Layer::new("nodes");
    for (at, look) in placed.boxes.iter().zip(boxes) {
        let rect = Rect {
            x: at.x + by[0],
            y: at.y + by[1],
            w: at.width,
            h: at.height,
        };
        let (ink, width, dash) = match look.frame {
            Frame::Plain => (style::INK, 1.2, Vec::new()),
            Frame::Subject => (style::SUBJECT, 2.4, Vec::new()),
            Frame::Ghost => (style::MUTED, 1.2, vec![4.0, 3.0]),
        };
        let mut items = vec![Item::Rect {
            rect,
            corner: 4.0,
            fill: Some(Paint::Solid(style::PAPER)),
            stroke: Some(Stroke {
                dash,
                ..Stroke::solid(ink, width)
            }),
        }];
        if let Some(accent) = look.accent {
            items.push(Item::Rect {
                rect: Rect {
                    x: rect.x,
                    y: rect.y,
                    w: ACCENT_WIDTH,
                    h: rect.h,
                },
                corner: 0.0,
                fill: Some(Paint::Solid(accent)),
                stroke: None,
            });
        }
        let mut cursor = rect.y + PAD_Y;
        for line in &look.lines {
            let size = line.tone.size();
            items.push(style::label_item(
                [rect.x + ACCENT_WIDTH + PAD_X, cursor + size],
                line.text.clone(),
                style::text(size, line.tone.ink(), Anchor::Start),
            ));
            cursor += size + LINE_GAP;
        }
        let mut element = Element::keyed(look.key.clone(), look.kind, items);
        element.title = look.tip.clone();
        element.data = look.data.clone();
        nodes.elements.push(element);
    }
    list.layers.push(nodes);

    let mut head = Layer::new("title");
    head.elements.push(Element::new(vec![style::label_item(
        [MARGIN, 22.0],
        title,
        style::text(style::TITLE, style::INK, Anchor::Start),
    )]));
    list.layers.push(head);

    if !legend.is_empty() {
        let mut layer = Layer::new("legend");
        let mut items = Vec::new();
        let mut y = HEADER + bottom + 10.0 + LEGEND_LINE / 2.0;
        for entry in legend {
            items.push(Item::Polyline {
                points: vec![[12.0, y], [12.0 + LEGEND_SAMPLE, y]],
                stroke: Stroke {
                    dash: entry.dash.clone(),
                    ..Stroke::solid(entry.colour, entry.width)
                },
            });
            items.push(style::label_item(
                [12.0 + LEGEND_SAMPLE + 6.0, y + 3.5],
                entry.label.clone(),
                style::text(style::TEXT, style::INK, Anchor::Start),
            ));
            y += LEGEND_LINE;
        }
        layer.elements.push(Element::new(items));
        list.layers.push(layer);
    }
    Ok(list)
}
