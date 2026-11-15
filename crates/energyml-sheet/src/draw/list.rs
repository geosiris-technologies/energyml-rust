//! The display list: what a view decides to draw, and nothing of EnergyML.
//!
//! Keyed [`Element`]s in ordered [`Layer`]s, back to front, in user units
//! (pixels or millimetres), y down. [`crate::draw::svg::write`] serialises it,
//! [`crate::draw::hit::hit`] says what is under a point. The shape follows the
//! section view's `Layout` (calques, one group per object, primitives in user
//! units), without the fields only a section has (window, axes, exaggeration).

use std::sync::Arc;

use crate::draw::raster::Raster;
use crate::draw::svg::{DrawError, num};

/// How many straight pieces a cubic is cut into by [`PathData::flatten`] (the hit test).
pub const CUBIC_STEPS: usize = 16;

const NOT_FINITE: &str = "a coordinate is not finite";
const NEGATIVE: &str = "a length is negative";

/// The unit of a list's coordinates, which is also the SVG's user unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PageUnit {
    /// Screen pixels (`width="800"`): numbers written with 2 decimals.
    #[default]
    Pixel,
    /// Millimetres on paper (`width="210mm"`, the sheet's unit): 3 decimals.
    Millimetre,
}

/// A colour, 8 bits a channel, straight (not premultiplied) alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Colour {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
    /// Alpha channel, 255 is opaque.
    pub a: u8,
}

impl Colour {
    /// An opaque colour.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Colour { r, g, b, a: 255 }
    }

    /// A colour with an alpha channel.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Colour { r, g, b, a }
    }

    /// `#rrggbb`, lower case (the alpha is not part of it: see [`Colour::opacity`]).
    pub fn hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// `a / 255`.
    pub fn opacity(&self) -> f64 {
        f64::from(self.a) / 255.0
    }
}

/// An axis-aligned rectangle: left, top, width, height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// Left edge, user units.
    pub x: f64,
    /// Top edge, user units.
    pub y: f64,
    /// Width, user units.
    pub w: f64,
    /// Height, user units.
    pub h: f64,
}

impl Rect {
    /// Whether `(x, y)` lies inside, edges included.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }
}

/// How the ends of an open line are drawn (SVG `stroke-linecap`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LineCap {
    /// Flat end, cut at the line's end point.
    Butt,
    /// Rounded end, a half disc past the end point.
    #[default]
    Round,
    /// Flat end pushed half a line width past the end point.
    Square,
}

/// How two lines meet at a corner (SVG `stroke-linejoin`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LineJoin {
    /// Sharp corner, the outer edges extended until they meet.
    Miter,
    /// Rounded corner.
    #[default]
    Round,
    /// Corner cut flat.
    Bevel,
}

/// How a line is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    /// Colour of the line.
    pub colour: Colour,
    /// Width, user units; >= 0.
    pub width: f64,
    /// Dash and gap lengths, user units (`stroke-dasharray`); empty: solid.
    pub dash: Vec<f64>,
    /// Shape of the line's ends.
    pub cap: LineCap,
    /// Shape of the corners.
    pub join: LineJoin,
}

impl Stroke {
    /// A solid line, round caps and joins.
    pub fn solid(colour: Colour, width: f64) -> Self {
        Stroke {
            colour,
            width,
            dash: Vec::new(),
            cap: LineCap::Round,
            join: LineJoin::Round,
        }
    }
}

/// Parallel lines in a colour, `spacing` apart, rotated by `angle` degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hatch {
    /// Colour of the lines.
    pub colour: Colour,
    /// Distance between two lines, user units.
    pub spacing: f64,
    /// Rotation of the lines, degrees.
    pub angle: f64,
    /// Width of each line, user units.
    pub width: f64,
}

/// What an area is filled with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Paint {
    /// A single colour.
    Solid(Colour),
    /// Parallel lines.
    Hatch(Hatch),
}

/// Where a text sits against its anchor point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Anchor {
    /// The text starts at the anchor point.
    #[default]
    Start,
    /// The text is centred on the anchor point.
    Middle,
    /// The text ends at the anchor point.
    End,
}

/// How a text is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    /// Font size, user units (never points: the sheet's SVG says why).
    pub size: f64,
    /// Colour of the text.
    pub colour: Colour,
    /// Where the text sits against its anchor point.
    pub anchor: Anchor,
    /// Rotation about the anchor point, degrees, clockwise on the page (y down).
    pub rotate: f64,
}

/// A marker's shape, centred on its point, drawn as paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolKind {
    /// A ring.
    Circle,
    /// A small filled disc.
    Point,
    /// A square.
    Square,
    /// A square turned by 45 degrees.
    Diamond,
    /// A triangle pointing up.
    TriangleUp,
    /// A triangle pointing down.
    TriangleDown,
    /// A diagonal cross (an X).
    Cross,
    /// An upright cross (a +).
    Plus,
    /// A five-pointed star.
    Star,
}

impl SymbolKind {
    /// `circle`, `point`, `square`, `diamond`, `triangle-up`, `triangle-down`, `cross`, `plus`, `star`.
    pub fn id(&self) -> &'static str {
        match self {
            SymbolKind::Circle => "circle",
            SymbolKind::Point => "point",
            SymbolKind::Square => "square",
            SymbolKind::Diamond => "diamond",
            SymbolKind::TriangleUp => "triangle-up",
            SymbolKind::TriangleDown => "triangle-down",
            SymbolKind::Cross => "cross",
            SymbolKind::Plus => "plus",
            SymbolKind::Star => "star",
        }
    }
}

/// One piece of a path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
    /// Start a subpath at this point.
    MoveTo([f64; 2]),
    /// A straight line from the current point to this point.
    LineTo([f64; 2]),
    /// A cubic Bezier curve from the current point.
    CubicTo {
        /// First control point.
        c1: [f64; 2],
        /// Second control point.
        c2: [f64; 2],
        /// End point.
        to: [f64; 2],
    },
    /// Close the current subpath.
    Close,
}

/// A path: what an SVG `d` and a Slint `Path { commands }` both carry.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct PathData {
    /// The pieces of the path, in order.
    pub segments: Vec<Segment>,
}

impl PathData {
    /// An open line through these points (one `MoveTo`, then `LineTo`s).
    pub fn polyline(points: &[[f64; 2]]) -> Self {
        let mut segments = Vec::with_capacity(points.len());
        for (i, p) in points.iter().enumerate() {
            segments.push(if i == 0 {
                Segment::MoveTo(*p)
            } else {
                Segment::LineTo(*p)
            });
        }
        PathData { segments }
    }

    /// The SVG path commands, e.g. `M10,20 L30,40 C1,2 3,4 5,6 Z`, numbers with
    /// `decimals` decimals at most: the only writer of a `d` in this module.
    pub fn to_commands(&self, decimals: usize) -> String {
        let pair = |p: &[f64; 2]| format!("{},{}", num(p[0], decimals), num(p[1], decimals));
        let mut tokens: Vec<String> = Vec::with_capacity(self.segments.len());
        for segment in &self.segments {
            tokens.push(match segment {
                Segment::MoveTo(p) => format!("M{}", pair(p)),
                Segment::LineTo(p) => format!("L{}", pair(p)),
                Segment::CubicTo { c1, c2, to } => format!("C{} {} {}", pair(c1), pair(c2), pair(to)),
                Segment::Close => "Z".to_string(),
            });
        }
        tokens.join(" ")
    }

    /// The path as polylines, one per subpath, each cubic cut into
    /// [`CUBIC_STEPS`] straight pieces; a `Close` repeats the subpath's first point.
    pub fn flatten(&self) -> Vec<Vec<[f64; 2]>> {
        let mut out: Vec<Vec<[f64; 2]>> = Vec::new();
        let mut current: Vec<[f64; 2]> = Vec::new();
        for segment in &self.segments {
            match *segment {
                Segment::MoveTo(p) => {
                    if !current.is_empty() {
                        out.push(std::mem::take(&mut current));
                    }
                    current.push(p);
                }
                Segment::LineTo(p) => {
                    if current.is_empty() {
                        current.push([0.0, 0.0]);
                    }
                    current.push(p);
                }
                Segment::CubicTo { c1, c2, to } => {
                    if current.is_empty() {
                        current.push([0.0, 0.0]);
                    }
                    let p0 = *current.last().unwrap_or(&[0.0, 0.0]);
                    for k in 1..=CUBIC_STEPS {
                        let t = k as f64 / CUBIC_STEPS as f64;
                        let u = 1.0 - t;
                        let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                        current.push([
                            a * p0[0] + b * c1[0] + c * c2[0] + d * to[0],
                            a * p0[1] + b * c1[1] + c * c2[1] + d * to[1],
                        ]);
                    }
                }
                Segment::Close => {
                    if let Some(first) = current.first().copied() {
                        current.push(first);
                    }
                }
            }
        }
        if !current.is_empty() {
            out.push(current);
        }
        out
    }
}

/// A primitive, in user units.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// A path of lines and cubic curves (graph edges, curves of a log).
    Path {
        /// The path.
        path: PathData,
        /// What the inside is filled with; `None`: unfilled.
        fill: Option<Paint>,
        /// How the outline is drawn; `None`: no outline.
        stroke: Option<Stroke>,
    },
    /// An open line with a width: a thick polyline (a well's strip, a trajectory).
    Polyline {
        /// The points of the line, in order.
        points: Vec<[f64; 2]>,
        /// How the line is drawn.
        stroke: Stroke,
    },
    /// A rectangle, corners rounded by `corner` (a graph node, a histogram bar).
    Rect {
        /// The rectangle.
        rect: Rect,
        /// Corner radius, user units.
        corner: f64,
        /// What the inside is filled with; `None`: unfilled.
        fill: Option<Paint>,
        /// How the outline is drawn; `None`: no outline.
        stroke: Option<Stroke>,
    },
    /// A closed polygon (a cell, a band, an envelope).
    Polygon {
        /// The corners, in order.
        points: Vec<[f64; 2]>,
        /// What the inside is filled with; `None`: unfilled.
        fill: Option<Paint>,
        /// How the outline is drawn; `None`: no outline.
        stroke: Option<Stroke>,
    },
    /// A disc or a ring outline.
    Circle {
        /// The centre.
        centre: [f64; 2],
        /// The radius, user units.
        radius: f64,
        /// What the inside is filled with; `None`: unfilled.
        fill: Option<Paint>,
        /// How the outline is drawn; `None`: no outline.
        stroke: Option<Stroke>,
    },
    /// A ring sector between two radii and two angles: radians on the page,
    /// 0 along +x, growing clockwise on screen (y down); the view converts its
    /// own angle convention before. `end - start` in (0, 2 pi]; 2 pi or more is
    /// the whole ring.
    Sector {
        /// The centre.
        centre: [f64; 2],
        /// Inner radius, user units.
        inner: f64,
        /// Outer radius, user units.
        outer: f64,
        /// Start angle, radians.
        start: f64,
        /// End angle, radians.
        end: f64,
        /// What the sector is filled with; `None`: unfilled.
        fill: Option<Paint>,
        /// How the outline is drawn; `None`: no outline.
        stroke: Option<Stroke>,
    },
    /// A marker's symbol, `size` user units across.
    Symbol {
        /// The centre of the symbol.
        at: [f64; 2],
        /// The shape.
        kind: SymbolKind,
        /// Size across, user units.
        size: f64,
        /// Fill colour; `None`: unfilled.
        fill: Option<Colour>,
        /// Outline colour.
        stroke: Colour,
    },
    /// One line of text, its baseline at `at`.
    Text {
        /// The anchor point; the baseline sits here.
        at: [f64; 2],
        /// The text.
        text: String,
        /// Size, colour, anchor and rotation.
        style: TextStyle,
    },
    /// A raster stretched over `rect`, each of its pixels a sharp block (never smoothed).
    Image {
        /// Where the raster is stretched.
        rect: Rect,
        /// The pixels.
        raster: Arc<Raster>,
    },
}

/// One thing a user can point at: one group in the SVG.
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    /// The handle a face finds it by (`data-key`): a full uri, a uri and a
    /// row, a pair of uris for an edge. `None`: a decoration, never hit.
    pub key: Option<String>,
    /// What it is (`data-kind`): `node`, `edge`, `bar`, `cell`, `tick`... Empty: none written.
    pub kind: String,
    /// Its tooltip (`<title>`); empty: none.
    pub title: String,
    /// Other `data-*` attributes, in order: names `[a-z][a-z0-9-]*`, not `key`, `kind` or `layer`.
    pub data: Vec<(String, String)>,
    /// What is drawn, back to front.
    pub items: Vec<Item>,
}

impl Element {
    /// A decoration: no key, empty kind, title and data.
    pub fn new(items: Vec<Item>) -> Self {
        Element {
            key: None,
            kind: String::new(),
            title: String::new(),
            data: Vec::new(),
            items,
        }
    }

    /// An element a face can find by its key.
    pub fn keyed(key: impl Into<String>, kind: impl Into<String>, items: Vec<Item>) -> Self {
        Element {
            key: Some(key.into()),
            kind: kind.into(),
            title: String::new(),
            data: Vec::new(),
            items,
        }
    }
}

/// Elements drawn together, in one z-order slot.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    /// `data-layer`.
    pub name: String,
    /// What the layer is clipped to; `None`: nothing.
    pub clip: Option<Rect>,
    /// The elements of the layer, back to front.
    pub elements: Vec<Element>,
}

impl Layer {
    /// An empty layer with this name and no clip.
    pub fn new(name: impl Into<String>) -> Self {
        Layer {
            name: name.into(),
            clip: None,
            elements: Vec::new(),
        }
    }
}

/// What a view decides to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct DisplayList {
    /// Width and height, user units.
    pub size: [f64; 2],
    /// The unit of the coordinates.
    pub unit: PageUnit,
    /// The drawing's title (`<title>`); empty: none.
    pub title: String,
    /// Filled under everything; `None`: transparent.
    pub background: Option<Colour>,
    /// Back to front.
    pub layers: Vec<Layer>,
}

impl DisplayList {
    /// An empty list of this size, in this unit.
    pub fn new(size: [f64; 2], unit: PageUnit) -> Self {
        DisplayList {
            size,
            unit,
            title: String::new(),
            background: None,
            layers: Vec::new(),
        }
    }

    /// The keys of every keyed element, in drawing order.
    pub fn keys(&self) -> Vec<&str> {
        self.layers
            .iter()
            .flat_map(|layer| layer.elements.iter())
            .filter_map(|element| element.key.as_deref())
            .collect()
    }

    /// A layer by name.
    pub fn layer(&self, name: &str) -> Option<&Layer> {
        self.layers.iter().find(|layer| layer.name == name)
    }

    /// Every number finite, every width and radius non-negative, every name
    /// well formed: the checks `svg::write` starts with (and so every PNG).
    ///
    /// A polyline of fewer than 2 points or a polygon of fewer than 3 is not a
    /// fault: it is not drawn, and its group keeps its key.
    pub fn validate(&self) -> Result<(), DrawError> {
        let [w, h] = self.size;
        if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
            return Err(DrawError::BadPage {
                why: "the page size is not positive and finite",
            });
        }
        for layer in &self.layers {
            if layer.name.is_empty() || layer.name.chars().any(char::is_control) {
                return Err(DrawError::BadName {
                    what: "layer",
                    name: layer.name.clone(),
                });
            }
            if let Some(clip) = &layer.clip {
                let ok = [clip.x, clip.y, clip.w, clip.h].iter().all(|v| v.is_finite())
                    && clip.w > 0.0
                    && clip.h > 0.0;
                if !ok {
                    return Err(DrawError::BadItem {
                        layer: layer.name.clone(),
                        element: 0,
                        why: "the layer's clip is empty or not finite",
                    });
                }
            }
            for (index, element) in layer.elements.iter().enumerate() {
                for (name, _) in &element.data {
                    if !valid_name(name) || matches!(name.as_str(), "key" | "kind" | "layer") {
                        return Err(DrawError::BadName {
                            what: "data attribute",
                            name: name.clone(),
                        });
                    }
                }
                for item in &element.items {
                    if let Err(why) = check_item(item) {
                        return Err(DrawError::BadItem {
                            layer: layer.name.clone(),
                            element: index,
                            why,
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

/// `^[a-z][a-z0-9-]*$`.
pub(crate) fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn all_finite(values: &[f64]) -> bool {
    values.iter().all(|v| v.is_finite())
}

fn check_points(points: &[[f64; 2]]) -> Result<(), &'static str> {
    if points.iter().all(|p| all_finite(p)) {
        Ok(())
    } else {
        Err(NOT_FINITE)
    }
}

fn check_length(value: f64) -> Result<(), &'static str> {
    if !value.is_finite() {
        Err(NOT_FINITE)
    } else if value < 0.0 {
        Err(NEGATIVE)
    } else {
        Ok(())
    }
}

fn check_stroke(stroke: &Option<Stroke>) -> Result<(), &'static str> {
    let Some(stroke) = stroke else { return Ok(()) };
    check_length(stroke.width)?;
    for length in &stroke.dash {
        check_length(*length)?;
    }
    if !stroke.dash.is_empty() && stroke.dash.iter().all(|l| *l == 0.0) {
        return Err("the dash pattern has no length");
    }
    Ok(())
}

fn check_paint(fill: &Option<Paint>) -> Result<(), &'static str> {
    match fill {
        Some(Paint::Hatch(hatch)) => {
            if !all_finite(&[hatch.spacing, hatch.angle, hatch.width]) {
                return Err(NOT_FINITE);
            }
            if hatch.spacing < 0.0 || hatch.width < 0.0 {
                return Err(NEGATIVE);
            }
            if hatch.spacing == 0.0 {
                return Err("a hatch spacing is not above zero");
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn check_item(item: &Item) -> Result<(), &'static str> {
    match item {
        Item::Path { path, fill, stroke } => {
            for segment in &path.segments {
                match segment {
                    Segment::MoveTo(p) | Segment::LineTo(p) => check_points(&[*p])?,
                    Segment::CubicTo { c1, c2, to } => check_points(&[*c1, *c2, *to])?,
                    Segment::Close => {}
                }
            }
            check_paint(fill)?;
            check_stroke(stroke)
        }
        Item::Polyline { points, stroke } => {
            check_points(points)?;
            check_stroke(&Some(stroke.clone()))
        }
        Item::Rect {
            rect,
            corner,
            fill,
            stroke,
        } => {
            if !all_finite(&[rect.x, rect.y, rect.w, rect.h]) {
                return Err(NOT_FINITE);
            }
            check_length(rect.w)?;
            check_length(rect.h)?;
            check_length(*corner)?;
            check_paint(fill)?;
            check_stroke(stroke)
        }
        Item::Polygon { points, fill, stroke } => {
            check_points(points)?;
            check_paint(fill)?;
            check_stroke(stroke)
        }
        Item::Circle {
            centre,
            radius,
            fill,
            stroke,
        } => {
            check_points(&[*centre])?;
            check_length(*radius)?;
            check_paint(fill)?;
            check_stroke(stroke)
        }
        Item::Sector {
            centre,
            inner,
            outer,
            start,
            end,
            fill,
            stroke,
        } => {
            check_points(&[*centre])?;
            if !all_finite(&[*inner, *outer, *start, *end]) {
                return Err(NOT_FINITE);
            }
            check_length(*inner)?;
            check_length(*outer)?;
            if inner > outer {
                return Err("the inner radius is above the outer one");
            }
            if end <= start {
                return Err("the sector's end angle is not after its start");
            }
            check_paint(fill)?;
            check_stroke(stroke)
        }
        Item::Symbol { at, size, .. } => {
            check_points(&[*at])?;
            check_length(*size)
        }
        Item::Text { at, style, .. } => {
            check_points(&[*at])?;
            if !style.rotate.is_finite() {
                return Err(NOT_FINITE);
            }
            check_length(style.size)
        }
        Item::Image { rect, .. } => {
            if !all_finite(&[rect.x, rect.y, rect.w, rect.h]) {
                return Err(NOT_FINITE);
            }
            if rect.w <= 0.0 || rect.h <= 0.0 {
                return Err("the image rectangle is empty");
            }
            Ok(())
        }
    }
}
