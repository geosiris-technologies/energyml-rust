//! The drawing of a geosteering section: the plot frame (the one arithmetic
//! between drawing units and pixels), the [`DisplayList`], its SVG with the
//! request and the view in its metadata, and the drawing made again from it.

use std::collections::BTreeMap;
use std::sync::Arc;

use energyml_domain::ArraySource;
use energyml_domain::stats::quantile::QuantileMethod;
use energyml_domain::stats::scale::{Fraction, Scale, ScaleKind, auto_bounds, propose_log};
use energyml_geometry::around_well::projection::WellProjections;
use energyml_sheet::draw::list::{
    Anchor, Colour, DisplayList, Element, Item, Layer, PageUnit, Paint, Rect, Stroke, TextStyle,
};
use energyml_sheet::draw::svg::{self, Metadata};
use energyml_sheet::draw::ticks::{label, log_label, log_ticks, nice_ticks};

use super::horizons::PlacedMesh;
use super::image::curtain_image;
use super::model::{AlongWellRefusal, FORMAT, METADATA_ELEMENT, Model, ProjectionChoice, Request, build};
use super::sources::CurtainSources;

/// The left margin of the plot, pixels (the graduations of the ordinate).
const MARGIN_LEFT: f64 = 64.0;
/// The top margin, pixels (the exaggeration text).
const MARGIN_TOP: f64 = 30.0;
/// The right margin, pixels.
const MARGIN_RIGHT: f64 = 24.0;
/// The extra right margin of a legend, pixels.
const LEGEND_WIDTH: f64 = 80.0;
/// The bottom margin before the notes, pixels (the graduations of the abscissa).
const MARGIN_BOTTOM: f64 = 36.0;
/// The height of one line of notes, pixels.
const NOTE_LINE: f64 = 12.0;
/// The smallest and largest vertical exaggeration.
const EXAGGERATION_RANGE: [f64; 2] = [1.0 / 1000.0, 1000.0];

/// Mirror of `ScaleKind` (the domain takes no `serde`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScaleKindAsk {
    /// Equal steps of the value.
    Linear,
    /// Equal steps of `log10` of the value.
    Log10,
}

impl From<ScaleKindAsk> for ScaleKind {
    fn from(ask: ScaleKindAsk) -> ScaleKind {
        match ask {
            ScaleKindAsk::Linear => ScaleKind::Linear,
            ScaleKindAsk::Log10 => ScaleKind::Log10,
        }
    }
}

impl From<ScaleKind> for ScaleKindAsk {
    fn from(kind: ScaleKind) -> ScaleKindAsk {
        match kind {
            ScaleKind::Linear => ScaleKindAsk::Linear,
            ScaleKind::Log10 => ScaleKindAsk::Log10,
        }
    }
}

/// The legend's value scale: `kind: None` proposes log10 from 3 decades (S8, `propose_log`).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegendAsk {
    /// Linear or log10; `None`: proposed from the decades the values span.
    pub kind: Option<ScaleKindAsk>,
    /// The lower bound; `None`: the first percentile of the values.
    pub min: Option<f64>,
    /// The upper bound; `None`: the ninety-ninth percentile.
    pub max: Option<f64>,
}

/// How the section is shown: what changes the picture, never the model.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    /// Pixels.
    pub size: [f64; 2],
    /// `[x0, tvd0, x1, tvd1]` in drawing units; `None`: everything, 5 % margin.
    pub window: Option<[f64; 4]>,
    /// Vertical over horizontal scale; `None`: what the window and the size give. Clamped to [1/1000, 1000].
    pub exaggeration: Option<f64>,
    /// Draw the curtain.
    pub show_curtain: bool,
    /// Draw the boundaries.
    pub show_boundaries: bool,
    /// Draw the horizons.
    pub show_surfaces: bool,
    /// Draw the tops.
    pub show_tops: bool,
    /// Line colours `#rrggbb` by full uri, resolved by the face through the one door of colours; absent: black.
    pub colours: BTreeMap<String, String>,
    /// The legend, when asked.
    pub legend: Option<LegendAsk>,
    /// Write the curtain into the SVG as an image (an `Item::Image`, encoded in every build); the PNG of that
    /// SVG, made by `draw::png::render`, shows it.
    pub embed_curtain: bool,
}

/// The plot area and its scales: the one arithmetic between drawing units and
/// pixels, used by [`layout`], `curtain_image`, [`pick`](super::pick::pick) and [`pixel_of`](super::pick::pixel_of).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlotFrame {
    /// `[x, y, width, height]`, pixels.
    pub frame: [f64; 4],
    /// `[x0, tvd0, x1, tvd1]`, drawing units.
    pub window: [f64; 4],
    /// Pixels per unit, horizontally and vertically.
    pub scale: [f64; 2],
}

impl PlotFrame {
    /// The pixel of a point of the drawing; the true vertical depth grows downwards on the page.
    pub fn to_pixel(&self, xy: [f64; 2]) -> [f64; 2] {
        [
            self.frame[0] + (xy[0] - self.window[0]) * self.scale[0],
            self.frame[1] + (xy[1] - self.window[1]) * self.scale[1],
        ]
    }

    /// The point of the drawing under a pixel.
    pub fn to_data(&self, pixel: [f64; 2]) -> [f64; 2] {
        [
            self.window[0] + (pixel[0] - self.frame[0]) / self.scale[0],
            self.window[1] + (pixel[1] - self.frame[1]) / self.scale[1],
        ]
    }

    /// Vertical scale over horizontal scale.
    pub fn exaggeration(&self) -> f64 {
        self.scale[1] / self.scale[0]
    }

    /// Whether a pixel lies inside the frame.
    pub fn contains(&self, pixel: [f64; 2]) -> bool {
        Rect {
            x: self.frame[0],
            y: self.frame[1],
            w: self.frame[2],
            h: self.frame[3],
        }
        .contains(pixel[0], pixel[1])
    }
}

/// The extent of everything the model draws, `[x0, tvd0, x1, tvd1]`.
fn data_bounds(model: &Model) -> Option<[f64; 4]> {
    let mut bounds: Option<[f64; 4]> = None;
    let mut take = |p: [f64; 2]| {
        if p[0].is_finite() && p[1].is_finite() {
            bounds = Some(match bounds {
                None => [p[0], p[1], p[0], p[1]],
                Some(b) => [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])],
            });
        }
    };
    model.trace.iter().flatten().for_each(|p| take(*p));
    if let Some(curtain) = &model.curtain {
        for cell in &curtain.geometry.cells {
            cell.corners.iter().for_each(|p| take(*p));
        }
    }
    if let Some(found) = &model.boundaries {
        for point in found
            .above
            .iter()
            .chain(found.below.iter())
            .flatten()
            .chain(found.projected.iter().flatten())
        {
            take(point.at);
        }
    }
    for trace in &model.horizons {
        for point in trace.spans.iter().flatten() {
            take(point.at);
        }
    }
    for tops in &model.tops {
        tops.placed.iter().flatten().for_each(|p| take(*p));
    }
    model.markers.iter().for_each(|tick| take(tick.at));
    bounds
}

/// Everything, with a 5 % margin.
fn default_window(model: &Model) -> [f64; 4] {
    let Some([x0, y0, x1, y1]) = data_bounds(model) else {
        return [0.0, 0.0, 1.0, 1.0];
    };
    let (dx, dy) = (x1 - x0, y1 - y0);
    let (dx, dy) = (if dx > 0.0 { dx } else { 1.0 }, if dy > 0.0 { dy } else { 1.0 });
    [x0 - 0.05 * dx, y0 - 0.05 * dy, x1 + 0.05 * dx, y1 + 0.05 * dy]
}

/// The plot frame of a model under a view.
///
/// The exaggeration, when fixed, recalculates the height of the window keeping
/// its centre and its horizontal range (clamped to [1/1000, 1000]).
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn plot_frame(model: &Model, view: &View) -> PlotFrame {
    let [width, height] = view.size;
    let right = MARGIN_RIGHT + if view.legend.is_some() { LEGEND_WIDTH } else { 0.0 };
    let bottom = MARGIN_BOTTOM + NOTE_LINE * model.notes.len() as f64;
    let frame = [
        MARGIN_LEFT,
        MARGIN_TOP,
        (width - MARGIN_LEFT - right).max(1.0),
        (height - MARGIN_TOP - bottom).max(1.0),
    ];
    let mut window = view.window.unwrap_or_else(|| default_window(model));
    if !(window[2] > window[0]) {
        window[2] = window[0] + 1.0;
    }
    if !(window[3] > window[1]) {
        window[3] = window[1] + 1.0;
    }
    let sx = frame[2] / (window[2] - window[0]);
    let mut sy = frame[3] / (window[3] - window[1]);
    if let Some(wanted) = view.exaggeration
        && wanted.is_finite()
    {
        sy = wanted.clamp(EXAGGERATION_RANGE[0], EXAGGERATION_RANGE[1]) * sx;
        let centre = (window[1] + window[3]) / 2.0;
        let half = frame[3] / sy / 2.0;
        window[1] = centre - half;
        window[3] = centre + half;
    }
    PlotFrame {
        frame,
        window,
        scale: [sx, sy],
    }
}

// -- items ---------------------------------------------------------------------------

fn black() -> Colour {
    Colour::rgb(0, 0, 0)
}

/// A colour's text as a colour: `energyml_domain::color::Rgba::parse`, the one
/// reader (D3). A text that does not read is `None`, and the caller draws black.
fn parse_hex(text: &str) -> Option<Colour> {
    energyml_domain::color::Rgba::parse(text).ok().map(Colour::from)
}

fn colour_of(view: &View, key: &str) -> Colour {
    view.colours
        .get(key)
        .and_then(|hex| parse_hex(hex))
        .unwrap_or_else(black)
}

fn text(at: [f64; 2], text: String, anchor: Anchor) -> Item {
    Item::Text {
        at,
        text,
        style: TextStyle {
            size: 10.0,
            colour: black(),
            anchor,
            rotate: 0.0,
        },
    }
}

/// Runs of consecutive points, cut wherever one is missing: a hole stays a hole.
fn runs(points: impl Iterator<Item = Option<[f64; 2]>>) -> Vec<Vec<[f64; 2]>> {
    let mut out: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut current: Vec<[f64; 2]> = Vec::new();
    for point in points {
        match point {
            Some(p) if p[0].is_finite() && p[1].is_finite() => current.push(p),
            _ => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn polylines(runs: Vec<Vec<[f64; 2]>>, stroke: &Stroke) -> Vec<Item> {
    runs.into_iter()
        .filter(|run| run.len() >= 2)
        .map(|points| Item::Polyline {
            points,
            stroke: stroke.clone(),
        })
        .collect()
}

/// The frame and its graduations. A degenerate window gets no graduation on
/// that axis, never a refusal of the whole drawing.
fn frame_items(plot: &PlotFrame) -> Vec<Item> {
    let [x, y, w, h] = plot.frame;
    let [x0, y0, x1, y1] = plot.window;
    let ink = Stroke::solid(black(), 1.0);
    let mut items = vec![Item::Rect {
        rect: Rect { x, y, w, h },
        corner: 0.0,
        fill: None,
        stroke: Some(ink.clone()),
    }];
    if let Ok(ticks) = nice_ticks(x0, x1, (w / 90.0).max(1.0)) {
        for &value in ticks.values.iter().filter(|v| **v >= x0 && **v <= x1) {
            let px = plot.to_pixel([value, y0])[0];
            items.push(Item::Polyline {
                points: vec![[px, y + h], [px, y + h + 4.0]],
                stroke: ink.clone(),
            });
            items.push(text([px, y + h + 15.0], label(value, ticks.step), Anchor::Middle));
        }
    }
    if let Ok(ticks) = nice_ticks(y0, y1, (h / 90.0).max(1.0)) {
        for &value in ticks.values.iter().filter(|v| **v >= y0 && **v <= y1) {
            let py = plot.to_pixel([x0, value])[1];
            items.push(Item::Polyline {
                points: vec![[x - 4.0, py], [x, py]],
                stroke: ink.clone(),
            });
            items.push(text([x - 7.0, py + 3.0], label(value, ticks.step), Anchor::End));
        }
    }
    items
}

/// The scale the curtain's values are coloured and graduated on: the kind asked, or log10 proposed from the decades
/// the finite values span (S8, `propose_log`); the bounds asked, or the robust ones (`auto_bounds`). `None` without
/// a curtain. **The one decision**: the legend draws by it, and a face that colours the curtain asks it too.
///
/// # Errors
///
/// Bounds that do not make a scale (no finite value, a log scale over values that are not positive).
pub fn curtain_scale(model: &Model, ask: &LegendAsk) -> Result<Option<Scale>, AlongWellRefusal> {
    let Some(curtain) = &model.curtain else {
        return Ok(None);
    };
    let finite: Vec<f64> = curtain
        .columns
        .values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();
    let refuse = |why: String| AlongWellRefusal::Legend(why);
    let kind = match ask.kind {
        Some(kind) => ScaleKind::from(kind),
        None => {
            let positive = finite.iter().copied().filter(|value| *value > 0.0);
            let low = positive.clone().fold(f64::INFINITY, f64::min);
            let high = positive.fold(f64::NEG_INFINITY, f64::max);
            let decades = (high > low).then(|| (high / low).log10());
            if propose_log(decades) {
                ScaleKind::Log10
            } else {
                ScaleKind::Linear
            }
        }
    };
    let auto = if ask.min.is_none() || ask.max.is_none() {
        Some(auto_bounds(&finite, kind, QuantileMethod::Auto).map_err(|why| refuse(why.to_string()))?)
    } else {
        None
    };
    let min = ask.min.or(auto.map(|bounds| bounds.0)).unwrap_or(0.0);
    let max = ask.max.or(auto.map(|bounds| bounds.1)).unwrap_or(1.0);
    Scale::new(kind, min, max, false)
        .map(Some)
        .map_err(|why| refuse(why.to_string()))
}

/// The legend: a ramp sampled by the caller's colour closure, graduated on the value scale.
fn legend_items(
    model: &Model,
    ask: &LegendAsk,
    colour: &dyn Fn(f64) -> Option<[u8; 4]>,
    plot: &PlotFrame,
) -> Result<Vec<Item>, AlongWellRefusal> {
    let Some(scale) = curtain_scale(model, ask)? else {
        return Ok(Vec::new());
    };
    let (kind, min, max) = (scale.kind(), scale.min(), scale.max());

    let [x, y, w, h] = plot.frame;
    let left = x + w + 14.0;
    let ramp = h.min(200.0);
    let steps = 32usize;
    let mut items = Vec::new();
    for step in 0..steps {
        let value = scale.value_at((step as f64 + 0.5) / steps as f64);
        if let Some(pixel) = colour(value) {
            items.push(Item::Rect {
                rect: Rect {
                    x: left,
                    y: y + ramp - (step as f64 + 1.0) * ramp / steps as f64,
                    w: 14.0,
                    h: ramp / steps as f64 + 0.5,
                },
                corner: 0.0,
                fill: Some(Paint::Solid(Colour::rgba(pixel[0], pixel[1], pixel[2], pixel[3]))),
                stroke: None,
            });
        }
    }
    let target = (ramp / 40.0).max(1.0);
    let graduations: Vec<(f64, String)> = match kind {
        ScaleKind::Linear => nice_ticks(min, max, target)
            .map(|ticks| ticks.values.iter().map(|v| (*v, label(*v, ticks.step))).collect())
            .unwrap_or_default(),
        ScaleKind::Log10 => log_ticks(min, max, target)
            .map(|ticks| ticks.values.iter().map(|v| (*v, log_label(*v))).collect())
            .unwrap_or_default(),
    };
    for (value, name) in graduations {
        if let Fraction::At(fraction) = scale.fraction(value) {
            let py = y + ramp * (1.0 - fraction);
            items.push(text([left + 18.0, py + 3.0], name, Anchor::Start));
        }
    }
    Ok(items)
}

/// The drawing, in numbers: frame, graduations, trajectory, boundaries,
/// horizons, tops, legend, notes, and the curtain image when asked and a
/// colour is given. Nothing is read or computed again here.
///
/// # Errors
///
/// An unknown projection, a curtain image that cannot be made, a legend that
/// was asked and has no scale.
pub fn layout(
    model: &Model,
    view: &View,
    projections: &WellProjections,
    colour: Option<&dyn Fn(f64) -> Option<[u8; 4]>>,
) -> Result<DisplayList, AlongWellRefusal> {
    let choice = ProjectionChoice::of(&model.request);
    let projection = choice.get(&model.request.projection, projections)?;
    let plot = plot_frame(model, view);
    let [fx, fy, fw, fh] = plot.frame;
    let clip = Rect {
        x: fx,
        y: fy,
        w: fw,
        h: fh,
    };
    let mut list = DisplayList::new(view.size, PageUnit::Pixel);
    list.title = "Geosteering section".to_string();
    list.background = Some(Colour::rgb(255, 255, 255));

    // The curtain, as an image, when asked and a colour is given.
    let mut curtain_layer = Layer::new("curtain");
    curtain_layer.clip = Some(clip);
    if view.show_curtain
        && view.embed_curtain
        && let (Some(curtain), Some(colour)) = (&model.curtain, colour)
    {
        let (raster, _counts) = curtain_image(
            &curtain.geometry,
            &curtain.columns.values,
            curtain.layout.rows_up.len(),
            colour,
            &plot,
        )?;
        curtain_layer.elements.push(Element::keyed(
            curtain.columns.property.clone(),
            "curtain",
            vec![Item::Image {
                rect: clip,
                raster: Arc::new(raster),
            }],
        ));
    }
    list.layers.push(curtain_layer);

    // The frame, its graduations, and the exaggeration text.
    let mut frame_layer = Layer::new("frame");
    frame_layer.elements.push(Element::new(frame_items(&plot)));
    let exaggeration = plot.exaggeration();
    let words = if projection.abscissa_is_length() {
        format!("vertical exaggeration x{exaggeration:.2}")
    } else {
        format!("vertical scale x{exaggeration:.2} against MD")
    };
    frame_layer
        .elements
        .push(Element::new(vec![text([fx, fy - 10.0], words, Anchor::Start)]));
    list.layers.push(frame_layer);

    // The trajectory: a hole the projection refuses stays a hole.
    let mut trajectory_layer = Layer::new("trajectory");
    trajectory_layer.clip = Some(clip);
    let trajectory_key = model
        .request
        .trajectory
        .clone()
        .unwrap_or_else(|| model.request.frame.clone());
    let ink = Stroke::solid(colour_of(view, &trajectory_key), 1.5);
    trajectory_layer.elements.push(Element::keyed(
        trajectory_key,
        "trajectory",
        polylines(
            runs(model.trace.iter().map(|p| p.map(|p| plot.to_pixel(p)))),
            &ink,
        ),
    ));
    list.layers.push(trajectory_layer);

    // The boundaries, told apart by their pattern and never by colour alone.
    let mut boundary_layer = Layer::new("boundaries");
    boundary_layer.clip = Some(clip);
    if view.show_boundaries
        && let (Some(found), Some(ask)) = (&model.boundaries, &model.request.boundaries)
    {
        for (kind, points, dash) in [
            ("boundary-above", &found.above, vec![6.0, 3.0]),
            ("boundary-below", &found.below, vec![2.0, 3.0]),
        ] {
            let mut stroke = Stroke::solid(colour_of(view, &ask.radius), 1.5);
            stroke.dash = dash;
            boundary_layer.elements.push(Element::keyed(
                ask.radius.clone(),
                kind,
                polylines(
                    runs(points.iter().map(|p| p.map(|p| plot.to_pixel(p.at)))),
                    &stroke,
                ),
            ));
        }
    }
    list.layers.push(boundary_layer);

    // The horizons: one polyline per run, keyed by the surface.
    let mut horizon_layer = Layer::new("horizons");
    horizon_layer.clip = Some(clip);
    if view.show_surfaces {
        for trace in &model.horizons {
            let stroke = Stroke::solid(colour_of(view, &trace.surface), 1.5);
            let all = trace
                .spans
                .iter()
                .flat_map(|span| polylines(vec![span.iter().map(|p| plot.to_pixel(p.at)).collect()], &stroke))
                .collect();
            horizon_layer
                .elements
                .push(Element::keyed(trace.surface.clone(), "horizon", all));
        }
    }
    list.layers.push(horizon_layer);

    // The tops of the hole.
    let mut top_layer = Layer::new("tops");
    top_layer.clip = Some(clip);
    if view.show_tops {
        for tops in &model.tops {
            let ink = colour_of(view, &tops.surface);
            let items = tops
                .placed
                .iter()
                .flatten()
                .map(|p| Item::Symbol {
                    at: plot.to_pixel(*p),
                    kind: energyml_sheet::draw::list::SymbolKind::Diamond,
                    size: 7.0,
                    fill: Some(ink),
                    stroke: ink,
                })
                .collect();
            top_layer
                .elements
                .push(Element::keyed(tops.surface.clone(), "top", items));
        }
    }
    list.layers.push(top_layer);

    // The legend, when asked and a colour is given.
    let mut legend_layer = Layer::new("legend");
    if let (Some(ask), Some(colour)) = (&view.legend, colour) {
        let items = legend_items(model, ask, colour, &plot)?;
        if !items.is_empty() {
            legend_layer.elements.push(Element::new(items));
        }
    }
    list.layers.push(legend_layer);

    // The notes, in English: a face translates for its screen.
    let mut notes_layer = Layer::new("notes");
    let notes: Vec<Item> = model
        .notes
        .iter()
        .enumerate()
        .map(|(i, note)| {
            text(
                [fx, fy + fh + MARGIN_BOTTOM - 6.0 + NOTE_LINE * (i as f64 + 1.0)],
                note.english(),
                Anchor::Start,
            )
        })
        .collect();
    if !notes.is_empty() {
        notes_layer.elements.push(Element::new(notes));
    }
    list.layers.push(notes_layer);
    Ok(list)
}

/// [`layout`], written as SVG with the request and the view in its metadata.
///
/// # Errors
///
/// What [`layout`] refuses, and the refusals of the SVG writer.
pub fn draw(
    model: &Model,
    view: &View,
    projections: &WellProjections,
    colour: Option<&dyn Fn(f64) -> Option<[u8; 4]>>,
) -> Result<String, AlongWellRefusal> {
    let list = layout(model, view, projections, colour)?;
    let body = serde_json::to_string(&(&model.request, view))
        .map_err(|why| AlongWellRefusal::BadMetadata(why.to_string()))?;
    let metadata = Metadata {
        element: METADATA_ELEMENT.to_string(),
        namespace: "urn:energyml:along-well-view".to_string(),
        format: FORMAT,
        version: env!("CARGO_PKG_VERSION").to_string(),
        body,
    };
    Ok(svg::write(&list, Some(&metadata))?)
}

/// The request and the view a drawing was made from, read back.
///
/// # Errors
///
/// A drawing with no such metadata, of another format, or whose body does not read.
pub fn read_metadata(svg_text: &str) -> Result<(Request, View), AlongWellRefusal> {
    let metadata = svg::read_metadata(svg_text, METADATA_ELEMENT)?;
    if metadata.format != FORMAT {
        return Err(AlongWellRefusal::BadMetadata(format!(
            "format {} where this build reads {FORMAT}",
            metadata.format
        )));
    }
    serde_json::from_str::<(Request, View)>(&metadata.body)
        .map_err(|why| AlongWellRefusal::BadMetadata(why.to_string()))
}

/// A drawing made again from its own metadata, over the same package: the same bytes.
///
/// # Errors
///
/// What [`read_metadata`], `build` and [`draw`] refuse.
pub fn redraw(
    svg_text: &str,
    source: &dyn ArraySource,
    surfaces: &[PlacedMesh],
    sources: &CurtainSources,
    projections: &WellProjections,
    colour: Option<&dyn Fn(f64) -> Option<[u8; 4]>>,
) -> Result<String, AlongWellRefusal> {
    let (request, view) = read_metadata(svg_text)?;
    let model = build(&request, source, surfaces, sources, projections)?;
    draw(&model, &view, projections, colour)
}
