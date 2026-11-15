//! The SVG writer of a [`DisplayList`], and the reading back of its metadata.
//!
//! Four rules, taken from the section view's writer:
//!
//! 1. **Grouped and annotated.** One `<g>` per element, `data-key` and
//!    `data-kind` on it, one `<g data-layer>` per layer, even an empty one.
//! 2. **Colours are attributes**, never classes: a file edited by hand or
//!    opened in a vector editor keeps what it shows.
//! 3. **Everything is escaped**: a title or a key comes from a file and may
//!    hold anything.
//! 4. **`<metadata>`** carries what the caller needs to redraw; this module
//!    never parses its body.
//!
//! The output is deterministic: nothing iterates a hash map.

use std::fmt::Write as _;

use crate::draw::list::{
    Anchor, Colour, DisplayList, Hatch, Item, LineCap, LineJoin, PageUnit, Paint, Stroke, SymbolKind,
    valid_name,
};
use crate::draw::text::FONT_STACK;

/// What went wrong drawing. Typed; the text names what is missing.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DrawError {
    /// The build lacks a cargo feature this operation needs.
    #[error(
        "this build cannot {what}: energyml-sheet was built without its `{feature}` feature (rebuild with `--features {feature}`)"
    )]
    FeatureNotEnabled {
        /// The missing cargo feature.
        feature: &'static str,
        /// What cannot be done without it.
        what: &'static str,
    },
    /// An `<image>` that is not a PNG.
    #[error("an <image> of type `{mime}` cannot be drawn: only a PNG embedded as a data URL is")]
    ImageNotPng {
        /// The media type found.
        mime: String,
    },
    /// An `<image>` whose `href` is not an embedded data URL.
    #[error(
        "an <image> that points to `{href}` cannot be drawn: only an image embedded as a data URL is (no file is read)"
    )]
    ImageNotEmbedded {
        /// The `href` found.
        href: String,
    },
    /// An embedded PNG that cannot be decoded.
    #[error("an embedded PNG does not decode: {why}")]
    ImageUnreadable {
        /// What the decoder said.
        why: String,
    },
    /// The SVG text could not be parsed.
    #[error("the SVG did not parse: {0}")]
    Svg(String),
    /// The requested image size is out of range.
    #[error("cannot make an image of {width} x {height} pixels (at most {limit} pixels, at least one)")]
    Size {
        /// Requested width, pixels.
        width: u64,
        /// Requested height, pixels.
        height: u64,
        /// The most pixels allowed.
        limit: u64,
    },
    /// The PNG encoder failed.
    #[error("encoding the PNG: {0}")]
    Encode(String),
    /// The display list as a whole is not drawable.
    #[error("the display list cannot be drawn: {why}")]
    BadPage {
        /// What is wrong with the page.
        why: &'static str,
    },
    /// An element of a layer is not drawable.
    #[error("element {element} of layer `{layer}` cannot be drawn: {why}")]
    BadItem {
        /// Name of the layer.
        layer: String,
        /// Index of the element in the layer.
        element: usize,
        /// What is wrong with it.
        why: &'static str,
    },
    /// A name (a layer, a `data-*` attribute) is not well formed.
    #[error(
        "`{name}` is not a valid {what} name (lower-case letters, digits and `-`, starting with a letter)"
    )]
    BadName {
        /// What kind of name it was meant to be.
        what: &'static str,
        /// The offending name.
        name: String,
    },
    /// The gap between spread labels is negative or not finite.
    #[error("labels cannot be spread {gap} apart: the gap must be a finite number from 0")]
    BadGap {
        /// The gap given.
        gap: f64,
    },
    /// The window is not an increasing range of finite numbers.
    #[error("the window [{from}, {to}] is not an increasing range of finite numbers")]
    BadWindow {
        /// Start of the window.
        from: f64,
        /// End of the window.
        to: f64,
    },
    /// The image has no pixel along the axis to map.
    #[error("no pixel to map: the image is {pixels} pixels along this axis")]
    NoPixels {
        /// The pixel count along the axis.
        pixels: u32,
    },
    /// Positions that must increase do not.
    #[error("the positions are not increasing at sample {at}")]
    NotIncreasing {
        /// Index of the sample where the order breaks.
        at: usize,
    },
    /// A single sample has no neighbour to give it a width.
    #[error("a single sample has no neighbour to take its width from: give a cap")]
    SingleSampleNeedsWidth,
    /// A grid is given a number of values that does not match its shape.
    #[error("a grid of {rows} x {cols} needs {expected} values, and {found} were given")]
    GridShape {
        /// Number of rows.
        rows: usize,
        /// Number of columns.
        cols: usize,
        /// Number of values the shape needs.
        expected: usize,
        /// Number of values given.
        found: usize,
    },
    /// A map picks an index outside the grid's axis.
    #[error("the {axis} map picks {pick}, and the grid has {len} {axis}")]
    PickOutOfRange {
        /// The axis picked on.
        axis: &'static str,
        /// The index picked.
        pick: usize,
        /// The length of the axis.
        len: usize,
    },
    /// A raster is given a number of bytes that does not match its size.
    #[error("a raster of {width} x {height} pixels needs {expected} bytes, and {found} were given")]
    RasterBytes {
        /// Width, pixels.
        width: u32,
        /// Height, pixels.
        height: u32,
        /// Number of bytes needed.
        expected: usize,
        /// Number of bytes given.
        found: usize,
    },
}

/// What an SVG's `<metadata>` carries: written by [`write`], read back by
/// [`read_metadata`]. The body is the caller's own text (usually JSON it
/// serialised itself): this module never parses it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metadata {
    /// The element's name inside `<metadata>`: `target-view`, `table-schema`...
    pub element: String,
    /// Its `xmlns`: `urn:energyml:target-view`.
    pub namespace: String,
    /// The format of the body, which the caller checks on reading.
    pub format: u32,
    /// The version of the crate that wrote it.
    pub version: String,
    /// The body, the caller's own text; this module never parses it.
    pub body: String,
}

/// Why an SVG's metadata could not be read back.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum MetadataError {
    /// The SVG has no matching `<metadata>` element.
    #[error(
        "no <{element}> metadata in this SVG - it was not written by this view, or its <metadata> was removed"
    )]
    Missing {
        /// The element name looked for.
        element: String,
    },
    /// The `<metadata>` element is present but does not read.
    #[error("the <{element}> metadata does not read: {why}")]
    Unreadable {
        /// The element name read.
        element: String,
        /// Why it does not read.
        why: String,
    },
}

/// A number, `decimals` decimals at most, no trailing zeros, no "-0", never `NaN`.
pub(crate) fn num(v: f64, decimals: usize) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    let text = format!("{v:.decimals$}");
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        text.as_str()
    };
    if text == "-0" || text.is_empty() {
        "0".to_string()
    } else {
        text.to_string()
    }
}

/// Text made safe for XML content and attribute values: the five characters
/// that mean something are replaced, and the control characters XML 1.0
/// cannot carry at all are dropped (a title from a file may hold anything).
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(ch),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

/// Writes `list` as an SVG document, with `meta` in its `<metadata>` if given.
pub fn write(list: &DisplayList, meta: Option<&Metadata>) -> Result<String, DrawError> {
    list.validate()?;
    if let Some(meta) = meta {
        if !valid_name(&meta.element) {
            return Err(DrawError::BadName {
                what: "metadata element",
                name: meta.element.clone(),
            });
        }
        if meta.namespace.is_empty() {
            return Err(DrawError::BadName {
                what: "metadata namespace",
                name: meta.namespace.clone(),
            });
        }
    }
    let d = match list.unit {
        PageUnit::Pixel => 2,
        PageUnit::Millimetre => 3,
    };
    let suffix = match list.unit {
        PageUnit::Pixel => "",
        PageUnit::Millimetre => "mm",
    };
    let [w, h] = list.size;
    let has_image = list
        .layers
        .iter()
        .flat_map(|l| l.elements.iter())
        .flat_map(|e| e.items.iter())
        .any(|i| matches!(i, Item::Image { .. }));

    let mut out = String::new();
    out.push_str("<svg xmlns=\"http://www.w3.org/2000/svg\"");
    if has_image {
        out.push_str(" xmlns:xlink=\"http://www.w3.org/1999/xlink\"");
    }
    let _ = writeln!(
        out,
        " width=\"{}{suffix}\" height=\"{}{suffix}\" viewBox=\"0 0 {} {}\" font-family=\"{}\">",
        num(w, d),
        num(h, d),
        num(w, d),
        num(h, d),
        escape(FONT_STACK)
    );
    if !list.title.is_empty() {
        let _ = writeln!(out, "<title>{}</title>", escape(&list.title));
    }
    if let Some(meta) = meta {
        let _ = writeln!(
            out,
            "<metadata><{el} xmlns=\"{ns}\" format=\"{format}\" version=\"{version}\">{body}</{el}></metadata>",
            el = meta.element,
            ns = escape(&meta.namespace),
            format = meta.format,
            version = escape(&meta.version),
            body = escape(&meta.body),
        );
    }

    // One pattern per distinct hatch style, in order of first meeting.
    let mut hatches: Vec<Hatch> = Vec::new();
    for layer in &list.layers {
        for element in &layer.elements {
            for item in &element.items {
                if let Some(Paint::Hatch(hatch)) = fill_of(item)
                    && !hatches.contains(&hatch)
                {
                    hatches.push(hatch);
                }
            }
        }
    }
    if !hatches.is_empty() || list.layers.iter().any(|l| l.clip.is_some()) {
        out.push_str("<defs>\n");
        for (i, layer) in list.layers.iter().enumerate() {
            if let Some(clip) = &layer.clip {
                let _ = writeln!(
                    out,
                    "<clipPath id=\"clip-{i}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath>",
                    num(clip.x, d),
                    num(clip.y, d),
                    num(clip.w, d),
                    num(clip.h, d)
                );
            }
        }
        for (n, hatch) in hatches.iter().enumerate() {
            let s = num(hatch.spacing, d);
            let _ = write!(
                out,
                "<pattern id=\"hatch-{n}\" patternUnits=\"userSpaceOnUse\" width=\"{s}\" height=\"{s}\" patternTransform=\"rotate({})\"><line x1=\"{}\" y1=\"0\" x2=\"{}\" y2=\"{s}\"",
                num(hatch.angle, d),
                num(hatch.spacing / 2.0, d),
                num(hatch.spacing / 2.0, d),
            );
            colour_attr(&mut out, "stroke", hatch.colour);
            let _ = writeln!(out, " stroke-width=\"{}\"/></pattern>", num(hatch.width, d));
        }
        out.push_str("</defs>\n");
    }

    if let Some(background) = list.background {
        let _ = write!(
            out,
            "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\"",
            num(w, d),
            num(h, d)
        );
        colour_attr(&mut out, "fill", background);
        out.push_str("/>\n");
    }

    for (i, layer) in list.layers.iter().enumerate() {
        let _ = write!(out, "<g data-layer=\"{}\"", escape(&layer.name));
        if layer.clip.is_some() {
            let _ = write!(out, " clip-path=\"url(#clip-{i})\"");
        }
        out.push_str(">\n");
        for element in &layer.elements {
            out.push_str("<g");
            if let Some(key) = &element.key {
                let _ = write!(out, " data-key=\"{}\"", escape(&key.replace('\0', "\u{2400}")));
            }
            if !element.kind.is_empty() {
                let _ = write!(out, " data-kind=\"{}\"", escape(&element.kind));
            }
            for (name, value) in &element.data {
                let _ = write!(out, " data-{name}=\"{}\"", escape(value));
            }
            out.push_str(">\n");
            if !element.title.is_empty() {
                let _ = writeln!(out, "<title>{}</title>", escape(&element.title));
            }
            for item in &element.items {
                write_item(&mut out, item, &hatches, d)?;
            }
            out.push_str("</g>\n");
        }
        out.push_str("</g>\n");
    }
    out.push_str("</svg>\n");
    Ok(out)
}

/// The paint of an item's area, if it has an area.
fn fill_of(item: &Item) -> Option<Paint> {
    match item {
        Item::Path { fill, .. }
        | Item::Rect { fill, .. }
        | Item::Polygon { fill, .. }
        | Item::Circle { fill, .. }
        | Item::Sector { fill, .. } => *fill,
        _ => None,
    }
}

/// ` name="#rrggbb"` and, if the colour is not opaque, ` name-opacity="0.502"`.
fn colour_attr(out: &mut String, name: &str, colour: Colour) {
    let _ = write!(out, " {name}=\"{}\"", colour.hex());
    if colour.a < 255 {
        let _ = write!(out, " {name}-opacity=\"{}\"", num(colour.opacity(), 3));
    }
}

fn fill_attrs(out: &mut String, fill: &Option<Paint>, hatches: &[Hatch]) {
    match fill {
        Some(Paint::Solid(colour)) => colour_attr(out, "fill", *colour),
        Some(Paint::Hatch(hatch)) => {
            let n = hatches.iter().position(|h| h == hatch).unwrap_or(0);
            let _ = write!(out, " fill=\"url(#hatch-{n})\"");
        }
        None => out.push_str(" fill=\"none\""),
    }
}

fn stroke_attrs(out: &mut String, stroke: &Option<Stroke>, d: usize) {
    let Some(stroke) = stroke else { return };
    colour_attr(out, "stroke", stroke.colour);
    let _ = write!(out, " stroke-width=\"{}\"", num(stroke.width, d));
    let cap = match stroke.cap {
        LineCap::Butt => "butt",
        LineCap::Round => "round",
        LineCap::Square => "square",
    };
    let join = match stroke.join {
        LineJoin::Miter => "miter",
        LineJoin::Round => "round",
        LineJoin::Bevel => "bevel",
    };
    let _ = write!(out, " stroke-linecap=\"{cap}\" stroke-linejoin=\"{join}\"");
    if !stroke.dash.is_empty() {
        let dash: Vec<String> = stroke.dash.iter().map(|v| num(*v, d)).collect();
        let _ = write!(out, " stroke-dasharray=\"{}\"", dash.join(" "));
    }
}

fn points_attr(points: &[[f64; 2]], d: usize) -> String {
    let parts: Vec<String> = points
        .iter()
        .map(|p| format!("{},{}", num(p[0], d), num(p[1], d)))
        .collect();
    parts.join(" ")
}

fn write_item(out: &mut String, item: &Item, hatches: &[Hatch], d: usize) -> Result<(), DrawError> {
    match item {
        Item::Path { path, fill, stroke } => {
            let _ = write!(out, "<path d=\"{}\"", path.to_commands(d));
            fill_attrs(out, fill, hatches);
            stroke_attrs(out, stroke, d);
            out.push_str("/>\n");
        }
        Item::Polyline { points, stroke } => {
            if points.len() >= 2 {
                let _ = write!(
                    out,
                    "<polyline points=\"{}\" fill=\"none\"",
                    points_attr(points, d)
                );
                stroke_attrs(out, &Some(stroke.clone()), d);
                out.push_str("/>\n");
            }
        }
        Item::Rect {
            rect,
            corner,
            fill,
            stroke,
        } => {
            let _ = write!(
                out,
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"",
                num(rect.x, d),
                num(rect.y, d),
                num(rect.w, d),
                num(rect.h, d)
            );
            if *corner > 0.0 {
                let _ = write!(out, " rx=\"{c}\" ry=\"{c}\"", c = num(*corner, d));
            }
            fill_attrs(out, fill, hatches);
            stroke_attrs(out, stroke, d);
            out.push_str("/>\n");
        }
        Item::Polygon { points, fill, stroke } => {
            if points.len() >= 3 {
                let _ = write!(out, "<polygon points=\"{}\"", points_attr(points, d));
                fill_attrs(out, fill, hatches);
                stroke_attrs(out, stroke, d);
                out.push_str("/>\n");
            }
        }
        Item::Circle {
            centre,
            radius,
            fill,
            stroke,
        } => {
            let _ = write!(
                out,
                "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"",
                num(centre[0], d),
                num(centre[1], d),
                num(*radius, d)
            );
            fill_attrs(out, fill, hatches);
            stroke_attrs(out, stroke, d);
            out.push_str("/>\n");
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
            let whole = end - start >= std::f64::consts::TAU;
            let _ = write!(
                out,
                "<path d=\"{}\"",
                sector_commands(*centre, *inner, *outer, *start, *end, d)
            );
            if whole {
                out.push_str(" fill-rule=\"evenodd\"");
            }
            fill_attrs(out, fill, hatches);
            stroke_attrs(out, stroke, d);
            out.push_str("/>\n");
        }
        Item::Symbol {
            at,
            kind,
            size,
            fill,
            stroke,
        } => write_symbol(out, *at, *kind, *size, *fill, *stroke, d),
        Item::Text { at, text, style } => {
            let anchor = match style.anchor {
                Anchor::Start => "start",
                Anchor::Middle => "middle",
                Anchor::End => "end",
            };
            let _ = write!(
                out,
                "<text x=\"{}\" y=\"{}\" text-anchor=\"{anchor}\" font-size=\"{}\"",
                num(at[0], d),
                num(at[1], d),
                num(style.size, d)
            );
            colour_attr(out, "fill", style.colour);
            if style.rotate != 0.0 {
                let _ = write!(
                    out,
                    " transform=\"rotate({} {} {})\"",
                    num(style.rotate, d),
                    num(at[0], d),
                    num(at[1], d)
                );
            }
            let _ = writeln!(out, ">{}</text>", escape(text));
        }
        Item::Image { rect, raster } => write_image(out, rect, raster, d)?,
    }
    Ok(())
}

#[cfg(feature = "png")]
fn write_image(
    out: &mut String,
    rect: &crate::draw::list::Rect,
    raster: &crate::draw::raster::Raster,
    d: usize,
) -> Result<(), DrawError> {
    let bytes = crate::draw::raster::encode_png(raster)?;
    let _ = writeln!(
        out,
        "<image x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" image-rendering=\"optimizeSpeed\" style=\"image-rendering:pixelated\" xlink:href=\"data:image/png;base64,{}\"/>",
        num(rect.x, d),
        num(rect.y, d),
        num(rect.w, d),
        num(rect.h, d),
        base64(&bytes)
    );
    Ok(())
}

#[cfg(not(feature = "png"))]
fn write_image(
    _out: &mut String,
    _rect: &crate::draw::list::Rect,
    _raster: &crate::draw::raster::Raster,
    _d: usize,
) -> Result<(), DrawError> {
    Err(DrawError::FeatureNotEnabled {
        feature: "png",
        what: "write a raster image into an SVG",
    })
}

/// A marker's symbol: one group, centred on `at`, every shape a path or a
/// circle. 3D shapes are drawn as what they look like from the side.
fn write_symbol(
    out: &mut String,
    at: [f64; 2],
    kind: SymbolKind,
    size: f64,
    fill: Option<Colour>,
    stroke: Colour,
    d: usize,
) {
    let [x, y] = at;
    let r = (size / 2.0).max(0.5);
    let path = |pts: &[[f64; 2]], closed: bool| -> String {
        let mut data = String::new();
        for (i, p) in pts.iter().enumerate() {
            // A closed outline is one run of segments; an open one is pairs (a stroke each).
            let start = if closed { i == 0 } else { i % 2 == 0 };
            if i > 0 {
                data.push(' ');
            }
            let _ = write!(
                data,
                "{}{},{}",
                if start { "M" } else { "L" },
                num(x + p[0] * r, d),
                num(y + p[1] * r, d)
            );
        }
        if closed {
            data.push_str(" Z");
        }
        format!("<path d=\"{data}\"")
    };
    let circle = |radius: f64| {
        format!(
            "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"",
            num(x, d),
            num(y, d),
            num(radius, d)
        )
    };
    let body = match kind {
        SymbolKind::Circle => circle(r),
        SymbolKind::Point => circle((r / 2.0).max(1.5)),
        SymbolKind::Square => path(
            &[[-0.85, -0.85], [0.85, -0.85], [0.85, 0.85], [-0.85, 0.85]],
            true,
        ),
        SymbolKind::Diamond => path(&[[0.0, -1.0], [0.8, 0.0], [0.0, 1.0], [-0.8, 0.0]], true),
        SymbolKind::TriangleUp => path(&[[0.0, -1.0], [1.0, 0.85], [-1.0, 0.85]], true),
        SymbolKind::TriangleDown => path(&[[-1.0, -0.85], [1.0, -0.85], [0.0, 1.0]], true),
        SymbolKind::Cross => path(&[[-0.8, -0.8], [0.8, 0.8], [-0.8, 0.8], [0.8, -0.8]], false),
        SymbolKind::Plus => path(&[[-1.0, 0.0], [1.0, 0.0], [0.0, -1.0], [0.0, 1.0]], false),
        SymbolKind::Star => {
            // Five points: ten vertices, alternately on the outer and inner circle.
            let pts: Vec<[f64; 2]> = (0..10)
                .map(|i| {
                    let angle = std::f64::consts::PI * f64::from(i) / 5.0 - std::f64::consts::FRAC_PI_2;
                    let radius = if i % 2 == 0 { 1.0 } else { 0.42 };
                    [radius * angle.cos(), radius * angle.sin()]
                })
                .collect();
            path(&pts, true)
        }
    };
    let _ = write!(out, "<g data-symbol=\"{}\">{body}", kind.id());
    match fill {
        Some(colour) => colour_attr(out, "fill", colour),
        None => out.push_str(" fill=\"none\""),
    }
    colour_attr(out, "stroke", stroke);
    let width = if fill.is_none() { 2.0 } else { 1.0 };
    let _ = writeln!(
        out,
        " stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/></g>",
        num(width, d)
    );
}

/// The `d` of a ring sector (SVG 1.1 section 8.3.8, elliptical arcs: a full
/// circle is two half arcs, since an arc whose end is its start draws nothing).
fn sector_commands(centre: [f64; 2], inner: f64, outer: f64, start: f64, end: f64, d: usize) -> String {
    let p = |radius: f64, angle: f64| {
        format!(
            "{},{}",
            num(centre[0] + radius * angle.cos(), d),
            num(centre[1] + radius * angle.sin(), d)
        )
    };
    let sweep = end - start;
    let large = if sweep > std::f64::consts::PI { 1 } else { 0 };
    let (o, i) = (num(outer, d), num(inner, d));
    if sweep < std::f64::consts::TAU {
        if inner > 0.0 {
            format!(
                "M{} A{o},{o} 0 {large} 1 {} L{} A{i},{i} 0 {large} 0 {} Z",
                p(outer, start),
                p(outer, end),
                p(inner, end),
                p(inner, start)
            )
        } else {
            format!(
                "M{},{} L{} A{o},{o} 0 {large} 1 {} Z",
                num(centre[0], d),
                num(centre[1], d),
                p(outer, start),
                p(outer, end)
            )
        }
    } else {
        let mut text = format!(
            "M{} A{o},{o} 0 0 1 {} A{o},{o} 0 0 1 {} Z",
            p(outer, start),
            p(outer, start + std::f64::consts::PI),
            p(outer, start)
        );
        if inner > 0.0 {
            let _ = write!(
                text,
                " M{} A{i},{i} 0 0 0 {} A{i},{i} 0 0 0 {} Z",
                p(inner, start),
                p(inner, start + std::f64::consts::PI),
                p(inner, start)
            );
        }
        text
    }
}

/// Standard base64 with padding (RFC 4648 section 4).
#[cfg_attr(not(feature = "png"), allow(dead_code))]
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Reads back the `<{element}>` metadata a caller put in an SVG. The format is
/// not checked here: the caller knows which formats it reads.
pub fn read_metadata(svg: &str, element: &str) -> Result<Metadata, MetadataError> {
    let missing = || MetadataError::Missing {
        element: element.to_string(),
    };
    let unreadable = |why: String| MetadataError::Unreadable {
        element: element.to_string(),
        why,
    };
    let open = format!("<{element}");
    // The tag name must end there: `<target-view-x` is not `<target-view`.
    let mut from = 0;
    let start = loop {
        let at = svg[from..].find(&open).ok_or_else(missing)? + from;
        let after = at + open.len();
        match svg[after..].chars().next() {
            Some(c) if c.is_whitespace() || c == '>' || c == '/' => break after,
            Some(_) => from = after,
            None => return Err(missing()),
        }
    };
    let tag_end = svg[start..].find('>').ok_or_else(missing)? + start;
    let self_closing = svg[start..tag_end].ends_with('/');
    let attributes = &svg[start..if self_closing { tag_end - 1 } else { tag_end }];
    let namespace = attribute(attributes, "xmlns").unwrap_or_default();
    let version = attribute(attributes, "version").unwrap_or_default();
    let format = attribute(attributes, "format")
        .and_then(|f| f.trim().parse::<u32>().ok())
        .ok_or_else(|| unreadable("format is not a number".to_string()))?;
    let body = if self_closing {
        String::new()
    } else {
        let close = format!("</{element}>");
        let body_start = tag_end + 1;
        let body_end = svg[body_start..].find(&close).ok_or_else(missing)? + body_start;
        unescape(&svg[body_start..body_end]).map_err(unreadable)?
    };
    Ok(Metadata {
        element: element.to_string(),
        namespace,
        format,
        version,
        body,
    })
}

/// The value of ` name="..."` in a start tag's attributes, unescaped.
fn attribute(attributes: &str, name: &str) -> Option<String> {
    let key = format!(" {name}=\"");
    let from = attributes.find(&key)? + key.len();
    let to = attributes[from..].find('"')? + from;
    unescape(&attributes[from..to]).ok()
}

/// XML 1.0 text, unescaped: the five predefined entities and `&#...;` / `&#x...;`.
fn unescape(text: &str) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let end = after.find(';').ok_or_else(|| {
            format!(
                "an `&` with no `;` near `{}`",
                rest[at..].chars().take(12).collect::<String>()
            )
        })?;
        let name = &after[..end];
        let ch = match name {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ => {
                let code = if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                    u32::from_str_radix(hex, 16).ok()
                } else if let Some(decimal) = name.strip_prefix('#') {
                    decimal.parse().ok()
                } else {
                    None
                };
                code.and_then(char::from_u32)
                    .ok_or_else(|| format!("`&{name};` is not an entity XML 1.0 knows"))?
            }
        };
        out.push(ch);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_vectors_of_rfc_4648_section_10() {
        let cases = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (plain, encoded) in cases {
            assert_eq!(
                base64(plain.as_bytes()),
                encoded,
                "RFC 4648 section 10: {plain:?}"
            );
        }
    }

    #[test]
    fn num_drops_trailing_zeros_and_negative_zero() {
        assert_eq!(num(10.0, 2), "10");
        assert_eq!(num(40.256, 2), "40.26");
        assert_eq!(num(-0.001, 2), "0");
        assert_eq!(num(f64::NAN, 2), "0");
        assert_eq!(num(100.0, 0), "100");
    }
}
