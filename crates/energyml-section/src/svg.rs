//! A layout written as SVG (ADR-0015).
//!
//! A pure writer: every number it prints was decided by [`crate::layout`], so
//! the front end, the CLI and the export draw the same bytes (WYSIWYG by
//! construction). Four rules:
//!
//! * **grouped and annotated** — one `<g>` per object, with `data-key` and
//!   `data-kind`; a well has one `<g>` per interval, with its unit
//!   (`data-unit`, `data-md-from`, `data-md-to`). That is how the front end knows
//!   what is under the pointer without redoing any geometry;
//! * **colours are attributes**, never classes (the front end's
//!   `bakeComputedPaint` trap: a class is lost the moment the SVG leaves the
//!   front end's stylesheet — an export, a standalone file);
//! * **everything is escaped** — a package's title is someone else's text and
//!   the front end inserts this string into its DOM. [`escape`] also drops the
//!   characters XML 1.0 cannot carry at all;
//! * **the metadata** — `<metadata>` holds the format, this crate's
//!   version, the inputs and the view as JSON, so `--from-svg` can redraw.
//!
//! # `data-key` and the NUL
//!
//! The front end's key of an object is its store and its uri **joined by a NUL**
//! (`model::object_key`), and a NUL cannot be written in XML 1.0 — not raw, not
//! as `&#0;`; an HTML parser turns it into U+FFFD. So `data-key` carries the
//! key with the NUL spelled U+2400 (`␀`, *symbol for null*), and the two halves
//! are also written whole as `data-store` and `data-uri`: the front end rebuilds its
//! key from those, with no guess about a separator.

use std::fmt::Write as _;

use crate::layout::{Anchor, Attr, LaidInterval, LaidObject, Layout, MARKERS_LAYER, Rect, STRATA_LAYER, Shape};

/// The script an SVG carries when `View::zoom_script` is on (it has to work
/// outside a front end, when the user opens the SVG in a browser). Plain ES2015 in `zoom.js`; it reads the root's
/// `data-frame`, `data-window` and `data-scale` and the `data-zoom*`
/// attributes this writer puts on what it moves. Where scripts do not run
/// (`<img>`, Inkscape, Office) the drawing is the same, static.
const ZOOM_SCRIPT: &str = include_str!("zoom.js");

/// What a piece of the drawing does when the script zooms it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Zoom {
    /// No script: every byte as it was before the script existed.
    Off,
    /// Inside the plot group, which the script scales: strokes are
    /// `non-scaling`, and dots and texts are counter-scaled around their anchor.
    Plot,
    /// The labels layer: outside the plot group (it reaches above the frame), so
    /// the script moves each label to where its anchor went, at its own size.
    Follow,
    /// The legend and the like: never moves, but strokes stay as they were.
    Still,
}

impl Zoom {
    fn on(self) -> bool {
        self != Zoom::Off
    }
}

/// The font the labels are measured against (`energyml_sheet::text`'s table is
/// Liberation Sans's, and the sheet writes the same stack).
const FONT_FAMILY: &str = "Liberation Sans, Arial, Helvetica, sans-serif";

/// The SVG of a layout, as a string.
pub fn write(layout: &Layout) -> String {
    let mut out = String::with_capacity(16 * 1024);
    let [w, h] = layout.size;
    // The map a front end's overlay reads a pointer through: the plot's
    // frame in pixels, the window it shows in the drawing's units, and the
    // pixels per unit — `x = frame.x + (s − window.x0)·scale.x`,
    // `y = frame.y + frame.h − (z − window.y0)·scale.y`. Written in full, not
    // rounded like the drawing's coordinates: a reading in metres must not
    // inherit a hundredth of a pixel's error times the scale. `data-whole` is
    // the window with none asked — the same value however the drawing was
    // zoomed — so a zoom can be clamped to it from any drawing.
    let (f, win, scale) = (layout.frame, layout.window, layout.scale);
    // Inkscape's namespace, declared only when a group uses it: a drawing
    // without the strata legend is the bytes it always was.
    let has_strata =
        layout.layers.iter().any(|l| (l.name == STRATA_LAYER || l.name == MARKERS_LAYER) && !l.objects.is_empty());
    let inkscape = if has_strata { " xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\"" } else { "" };
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\"{} width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\" \
         font-family=\"{}\" font-size=\"11\" data-frame=\"{} {} {} {}\" data-window=\"{} {} {} {}\" data-whole=\"{} {} {} {}\" data-scale=\"{} {}\">",
        inkscape,
        fmt(w),
        fmt(h),
        fmt(w),
        fmt(h),
        FONT_FAMILY,
        f.x,
        f.y,
        f.w,
        f.h,
        win.x[0],
        win.x[1],
        win.y[0],
        win.y[1],
        layout.whole.x[0],
        layout.whole.x[1],
        layout.whole.y[0],
        layout.whole.y[1],
        scale[0],
        scale[1]
    );
    let _ = writeln!(out, "<title>{}</title>", escape(&layout.title));
    write_metadata(&mut out, layout);
    let f = layout.frame;
    let _ = writeln!(
        out,
        "<defs><clipPath id=\"plot\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath>{}</defs>",
        fmt(f.x),
        fmt(f.y),
        fmt(f.w),
        fmt(f.h),
        hatch_pattern(layout)
    );
    let _ = writeln!(out, "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"{}\"/>", fmt(w), fmt(h), escape(&layout.background));
    let zoomable = layout.metadata.view.zoom_script;
    write_axes(&mut out, layout, zoomable);

    // With the script, the clip moves to a wrapper of its own: a `clip-path` on
    // the group the script transforms would be scaled with it, and the plot
    // would stop being clipped to its frame. One wrapper per run of clipped
    // layers (they are contiguous today; `querySelectorAll` does not care).
    let mut in_plot = false;
    for layer in &layout.layers {
        if layer.objects.is_empty() {
            continue;
        }
        let zoom = match (zoomable, layer.clipped, layer.name) {
            (false, ..) => Zoom::Off,
            (true, true, _) => Zoom::Plot,
            (true, false, "labels") => Zoom::Follow,
            (true, false, _) => Zoom::Still,
        };
        if zoom == Zoom::Plot && !in_plot {
            out.push_str("<g clip-path=\"url(#plot)\"><g data-zoom=\"plot\">\n");
            in_plot = true;
        } else if zoom != Zoom::Plot && in_plot {
            out.push_str("</g></g>\n");
            in_plot = false;
        }
        let clip = if layer.clipped && !zoomable { " clip-path=\"url(#plot)\"" } else { "" };
        if layer.name == STRATA_LAYER {
            // One group, outside the zoomed plot, that an editor can move whole:
            // an Inkscape layer with a name and an id of its own.
            out.push_str("<g id=\"strata-legend\" data-legend=\"strata\" inkscape:groupmode=\"layer\" inkscape:label=\"Strata legend\">\n");
        } else if layer.name == MARKERS_LAYER {
            out.push_str("<g id=\"markers-legend\" data-legend=\"markers\" inkscape:groupmode=\"layer\" inkscape:label=\"Markers legend\">\n");
        } else {
            let _ = writeln!(out, "<g data-layer=\"{}\"{}>", layer.name, clip);
        }
        for object in &layer.objects {
            write_object(&mut out, object, zoom);
        }
        out.push_str("</g>\n");
    }
    if in_plot {
        out.push_str("</g></g>\n");
    }

    // What the drawing says about itself (and the warnings).
    let _ = writeln!(
        out,
        "<text x=\"{}\" y=\"16\" font-size=\"14\" font-weight=\"bold\" fill=\"#222222\">{}</text>",
        fmt(f.x),
        escape(&layout.title)
    );
    let _ = writeln!(
        out,
        "<text data-role=\"exaggeration\" x=\"{}\" y=\"16\" text-anchor=\"end\" fill=\"#222222\">{}</text>",
        fmt(f.x + f.w),
        escape(&layout.exaggeration_text)
    );
    if !layout.origin_text.is_empty() {
        // The layout measured the free space and chose where the text starts.
        let _ = writeln!(
            out,
            "<text data-role=\"origin\" x=\"{}\" y=\"16\" fill=\"#444444\">{}</text>",
            fmt(layout.origin_x),
            escape(&layout.origin_text)
        );
    }
    let base = f.y + f.h + 46.0;
    for (i, line) in layout.written_warnings().iter().enumerate() {
        let _ = writeln!(
            out,
            "<text data-role=\"warning\" x=\"{}\" y=\"{}\" fill=\"#b71c1c\">{}</text>",
            fmt(f.x),
            fmt(base + 14.0 * i as f64),
            escape(line)
        );
    }
    if zoomable {
        // `]]>` would end the CDATA and `</script>` the element: the script has
        // neither (`tests/zoom.rs`), and a CDATA needs no escaping otherwise.
        let _ = writeln!(out, "<script><![CDATA[\n{ZOOM_SCRIPT}\n]]></script>");
    }
    out.push_str("</svg>\n");
    out
}

fn write_metadata(out: &mut String, layout: &Layout) {
    let json = serde_json::to_string(&layout.metadata).unwrap_or_else(|_| "null".to_string());
    let _ = writeln!(
        out,
        "<metadata><section-view xmlns=\"urn:energyml:section-view\" format=\"{}\" version=\"{}\">{}</section-view></metadata>",
        layout.metadata.format,
        escape(&layout.metadata.version),
        escape(&json)
    );
}

/// Why an SVG's metadata could not be read back.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum MetadataError {
    /// No `<section-view>` element: this SVG was not written by [`write`], or
    /// an editor dropped its `<metadata>`.
    #[error("no section-view metadata in this SVG — it was not written by the section view, or its <metadata> was removed")]
    Missing,
    /// Written by another format of the metadata.
    #[error("the metadata is format {found}, and this build reads format {known}")]
    Format {
        /// What the SVG says.
        found: String,
        /// [`crate::model::FORMAT`].
        known: u32,
    },
    /// The element is there and its JSON does not read.
    #[error("the section-view metadata does not read: {0}")]
    Unreadable(String),
}

/// What [`write`] put in `<metadata>` — the inputs and the view the drawing was
/// made from — read back from the SVG's text.
///
/// The way back of `write_metadata`, and the only one: `--from-svg` and an
/// operation that writes the trace an image was drawn along both read here, so
/// that the trace written and the trace drawn are one fact.
///
/// The text is unescaped as XML 1.0 does: the five entities [`escape`] writes,
/// and character references, which an editor that re-saved the file may have
/// written instead.
pub fn read_metadata(svg: &str) -> Result<crate::layout::Metadata, MetadataError> {
    const OPEN: &str = "<section-view";
    let start = svg.find(OPEN).ok_or(MetadataError::Missing)?;
    let rest = &svg[start + OPEN.len()..];
    let tag_end = rest.find('>').ok_or(MetadataError::Missing)?;
    let attributes = &rest[..tag_end];
    let body = &rest[tag_end + 1..];
    let body = &body[..body.find("</section-view>").ok_or(MetadataError::Missing)?];

    let format = attribute(attributes, "format").unwrap_or_default();
    if format != crate::model::FORMAT.to_string() {
        return Err(MetadataError::Format { found: format, known: crate::model::FORMAT });
    }
    let json = unescape(body).map_err(MetadataError::Unreadable)?;
    serde_json::from_str(&json).map_err(|why| MetadataError::Unreadable(why.to_string()))
}

/// The value of `name="…"` in a start tag's attributes, unescaped.
fn attribute(attributes: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    let from = attributes.find(&key)? + key.len();
    let to = attributes[from..].find('"')? + from;
    unescape(&attributes[from..to]).ok()
}

/// XML 1.0 text, unescaped: the five predefined entities and `&#…;` / `&#x…;`.
fn unescape(text: &str) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let end = after.find(';').ok_or_else(|| format!("an `&` with no `;` at `{}`", &rest[at..rest.len().min(at + 12)]))?;
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
                code.and_then(char::from_u32).ok_or_else(|| format!("`&{name};` is not an entity XML 1.0 knows"))?
            }
        };
        out.push(ch);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn write_axes(out: &mut String, layout: &Layout, zoom: bool) {
    let Rect { x, y, w, h } = layout.frame;
    out.push_str("<g data-layer=\"axes\" fill=\"#444444\" stroke=\"none\">\n");
    // The script rewrites the graduations from the window it shows. An axis
    // whose ticks carry a second line (true coordinates) cannot be rewritten —
    // that line is a function of the trace, not of the window — so its ticks
    // are one group each, which the script moves and hides instead.
    let moved = layout.x_axis.ticks.iter().any(|t| t.sublabel.is_some());
    if zoom {
        let _ = writeln!(out, "<g data-ticks=\"x\"{}>", if moved { " data-ticks-mode=\"move\"" } else { "" });
    }
    for t in &layout.x_axis.ticks {
        if t.at < x - 0.5 || t.at > x + w + 0.5 {
            continue;
        }
        if zoom && moved {
            let _ = writeln!(out, "<g data-tick-at=\"{}\">", fmt(t.at));
        }
        let _ = writeln!(
            out,
            "<line x1=\"{a}\" y1=\"{}\" x2=\"{a}\" y2=\"{}\" stroke=\"#e0e0e0\" stroke-width=\"1\"/>",
            fmt(y),
            fmt(y + h),
            a = fmt(t.at)
        );
        let _ = writeln!(out, "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\">{}</text>", fmt(t.at), fmt(y + h + 14.0), escape(&t.label));
        if let Some(sub) = &t.sublabel {
            let _ = writeln!(
                out,
                "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\">{}</text>",
                fmt(t.at),
                fmt(y + h + 26.0),
                escape(sub)
            );
        }
        if zoom && moved {
            out.push_str("</g>
");
        }
    }
    if zoom {
        out.push_str("</g>
<g data-ticks=\"y\">
");
    }
    for t in &layout.y_axis.ticks {
        if t.at < y - 0.5 || t.at > y + h + 0.5 {
            continue;
        }
        let _ = writeln!(
            out,
            "<line x1=\"{}\" y1=\"{b}\" x2=\"{}\" y2=\"{b}\" stroke=\"#e0e0e0\" stroke-width=\"1\"/>",
            fmt(x),
            fmt(x + w),
            b = fmt(t.at)
        );
        let _ = writeln!(out, "<text x=\"{}\" y=\"{}\" text-anchor=\"end\">{}</text>", fmt(x - 5.0), fmt(t.at + 4.0), escape(&t.label));
    }
    if zoom {
        out.push_str("</g>
");
    }
    let _ = writeln!(
        out,
        "<text data-role=\"x-title\" x=\"{}\" y=\"{}\" text-anchor=\"middle\">{}</text>",
        fmt(x + w / 2.0),
        fmt(y + h + if layout.x_axis.ticks.iter().any(|t| t.sublabel.is_some()) { 38.0 } else { 30.0 }),
        escape(&layout.x_axis.title)
    );
    let _ = writeln!(
        out,
        "<text data-role=\"y-title\" x=\"{a}\" y=\"{}\" text-anchor=\"middle\" transform=\"rotate(-90 {a} {})\">{}</text>",
        fmt(y + h / 2.0),
        fmt(y + h / 2.0),
        escape(&layout.y_axis.title),
        a = fmt(12.0 + layout.left_shift)
    );
    let _ = writeln!(
        out,
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"#444444\" stroke-width=\"1\"/>",
        fmt(x),
        fmt(y),
        fmt(w),
        fmt(h)
    );
    out.push_str("</g>\n");
}

/// The `<pattern>` the hatched style is stroked with — written only when a shape
/// refers to it, so a drawing without the style is the bytes it always was. Stripes
/// of the style's colour on nothing, `spacing` apart and rotated by `angle`. Its
/// units are the drawing's: the zoom script's transform magnifies it with the plot
/// (a texture, not a stroke — only strokes keep their pixels).
fn hatch_pattern(layout: &Layout) -> String {
    let reference = crate::layout::hatch_ref();
    let used = layout.layers.iter().flat_map(|l| &l.objects).flat_map(|o| &o.shapes).any(|s| match s {
        Shape::Polyline { stroke, .. } => *stroke == reference,
        Shape::Polygon { fill, .. } => *fill == reference,
        _ => false,
    });
    let crate::model::UnstratifiedStyle::Hatched { colour, spacing, angle } = layout.metadata.view.unstratified_style() else {
        return String::new();
    };
    if !used {
        return String::new();
    }
    format!(
        "<pattern id=\"{}\" patternUnits=\"userSpaceOnUse\" width=\"{}\" height=\"{}\" patternTransform=\"rotate({})\">         <line x1=\"{}\" y1=\"0\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"{}\"/></pattern>",
        crate::layout::HATCH_ID,
        fmt(spacing),
        fmt(spacing),
        fmt(angle),
        fmt(spacing / 2.0),
        fmt(spacing / 2.0),
        fmt(spacing),
        escape(&colour),
        fmt((spacing / 2.0).max(1.0))
    )
}

fn write_object(out: &mut String, o: &LaidObject, zoom: Zoom) {
    out.push_str("<g");
    match &o.attr {
        Attr::Key(key) => {
            let _ = write!(out, " data-key=\"{}\"", escape(&key.replace('\0', "\u{2400}")));
            if let Some((store, uri)) = key.split_once('\0') {
                let _ = write!(out, " data-store=\"{}\" data-uri=\"{}\"", escape(store), escape(uri));
            }
        }
        Attr::Section(s) => {
            let _ = write!(out, " data-section=\"{}\"", escape(s));
        }
        Attr::FillOf(key) => {
            let _ = write!(out, " data-fill-of=\"{}\"", escape(&key.replace('\0', "\u{2400}")));
        }
        Attr::LabelOf(key) => {
            let _ = write!(out, " data-label-of=\"{}\"", escape(&key.replace('\0', "\u{2400}")));
        }
        Attr::Decor(name) => {
            let _ = write!(out, " data-decor=\"{}\"", escape(name));
        }
        Attr::Marker { name, key } => {
            let _ = write!(out, " data-marker=\"{}\"", escape(name));
            if let Some(key) = key {
                let _ = write!(out, " data-key=\"{}\"", escape(&key.replace('\0', "\u{2400}")));
                if let Some((store, uri)) = key.split_once('\0') {
                    let _ = write!(out, " data-store=\"{}\" data-uri=\"{}\"", escape(store), escape(uri));
                }
            }
        }
        Attr::UnitEntry(name, uri, stores) => {
            let _ = write!(out, " data-decor=\"{}\" data-unit-uri=\"{}\"", escape(name), escape(uri));
            // The packages whose wells show the unit in this colour, by the
            // front end's store ids — which never hold a space.
            if !stores.is_empty() {
                let _ = write!(out, " data-unit-stores=\"{}\"", escape(&stores.join(" ")));
            }
        }
    }
    let _ = write!(out, " data-kind=\"{}\"", escape(&o.kind));
    if o.highlighted {
        out.push_str(" data-highlight=\"true\"");
    }
    if zoom == Zoom::Follow {
        // Where the label's first line sits: what the script moves it from.
        if let Some(Shape::Text { at, .. }) = o.shapes.iter().find(|s| matches!(s, Shape::Text { .. })) {
            let _ = write!(out, " data-zoom-anchor=\"{} {}\"", fmt(at[0]), fmt(at[1]));
        }
    }
    out.push_str(">\n");
    if !o.title.is_empty() {
        let _ = writeln!(out, "<title>{}</title>", escape(&o.title));
    }
    for shape in &o.shapes {
        write_shape(out, shape, zoom);
    }
    for iv in &o.intervals {
        write_interval(out, iv, zoom);
    }
    // The boundaries between units, over every interval: a group of their own
    // inside the well's, so that they are one thing to select or delete.
    if o.intervals.iter().any(|iv| iv.tick.is_some()) {
        out.push_str("<g data-part=\"strata-ticks\">\n");
        for [a, b] in o.intervals.iter().filter_map(|iv| iv.tick) {
            // Counter-scaled around its middle, which is on the well: zoomed in,
            // the tick stays a few pixels across the well rather than growing
            // with the drawing — as wide as the well, which keeps its own width.
            let middle = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            let _ = writeln!(
                out,
                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#000000\" stroke-width=\"1\"{}{}/>",
                fmt(a[0]),
                fmt(a[1]),
                fmt(b[0]),
                fmt(b[1]),
                non_scaling(zoom),
                fixed(zoom, middle)
            );
        }
        out.push_str("</g>\n");
    }
    out.push_str("</g>\n");
}

fn write_interval(out: &mut String, iv: &LaidInterval, zoom: Zoom) {
    // The unit's full uri, when the interval names one: what the front end finds the
    // unit by — its legend entry, its colour — and never its title.
    let uri = iv.unit_uri.as_deref().map(|uri| format!(" data-unit-uri=\"{}\"", escape(uri))).unwrap_or_default();
    let _ = writeln!(
        out,
        "<g data-unit=\"{}\"{} data-md-from=\"{}\" data-md-to=\"{}\"><polyline points=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"{}/></g>",
        escape(&iv.unit),
        uri,
        fmt(iv.from),
        fmt(iv.to),
        points(&iv.points),
        escape(&iv.colour),
        fmt(iv.width),
        non_scaling(zoom)
    );
}

/// A stroke that keeps its width in pixels under the script's transform
/// (`vector-effect`, which every browser knows): the line stays as thin as it
/// was drawn however far the plot is magnified, so a zoom pulls objects apart
/// instead of fattening them.
fn non_scaling(zoom: Zoom) -> &'static str {
    if zoom.on() { " vector-effect=\"non-scaling-stroke\"" } else { "" }
}

/// What the script needs to keep a dot or a text at its pixel size: the point it
/// is counter-scaled around, which is also where it is anchored. Only inside the
/// plot group, the one that gets scaled. A dot stays a `<circle>` the script
/// shrinks as the group grows: a zero-length round-capped line would draw the
/// same dot, but the front end and its tests know circles.
fn fixed(zoom: Zoom, at: [f64; 2]) -> String {
    if zoom == Zoom::Plot { format!(" data-zoom-fixed=\"{} {}\"", fmt(at[0]), fmt(at[1])) } else { String::new() }
}

fn write_shape(out: &mut String, shape: &Shape, zoom: Zoom) {
    match shape {
        Shape::Polyline { points: p, stroke, width, dash } => {
            if p.len() < 2 {
                return;
            }
            let _ = writeln!(
                out,
                "<polyline points=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\" stroke-linejoin=\"round\" stroke-linecap=\"round\"{}{}/>",
                points(p),
                escape(stroke),
                fmt(*width),
                non_scaling(zoom),
                if *dash { " stroke-dasharray=\"4 3\"" } else { "" }
            );
        }
        Shape::Dashed { points: p, stroke, width, dash, gap } => {
            if p.len() < 2 {
                return;
            }
            let _ = writeln!(
                out,
                "<polyline points=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\" stroke-linejoin=\"round\" stroke-dasharray=\"{} {}\"{}/>",
                points(p),
                escape(stroke),
                fmt(*width),
                fmt(*dash),
                fmt(*gap),
                non_scaling(zoom)
            );
        }
        Shape::Polygon { points: p, fill, opacity, stroke, dash } => {
            if p.len() < 3 {
                return;
            }
            let _ = write!(out, "<polygon points=\"{}\" fill=\"{}\" fill-opacity=\"{}\"", points(p), escape(fill), fmt(*opacity));
            if let Some(s) = stroke {
                let _ = write!(out, " stroke=\"{}\" stroke-width=\"1\"{}", escape(s), non_scaling(zoom));
                if *dash {
                    out.push_str(" stroke-dasharray=\"4 3\"");
                }
            }
            out.push_str("/>\n");
        }
        Shape::Circle { c, r, fill } => {
            let _ = writeln!(
                out,
                "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\"{}/>",
                fmt(c[0]),
                fmt(c[1]),
                fmt(*r),
                escape(fill),
                fixed(zoom, *c)
            );
        }
        Shape::Symbol { at, kind, size, fill, stroke } => write_symbol(out, *at, kind, *size, fill, stroke, zoom),
        Shape::Text { at, text, anchor, size, fill } => {
            let anchor = match anchor {
                Anchor::Start => "start",
                Anchor::Middle => "middle",
                Anchor::End => "end",
            };
            let _ = writeln!(
                out,
                "<text x=\"{}\" y=\"{}\" text-anchor=\"{}\" font-size=\"{}\" fill=\"{}\"{}>{}</text>",
                fmt(at[0]),
                fmt(at[1]),
                anchor,
                fmt(*size),
                escape(fill),
                fixed(zoom, *at),
                escape(text)
            );
        }
    }
}

/// A marker's symbol: one group, centred on `at`, that the zoom script keeps at
/// its pixel size. Every shape is a path or a circle in pixels — nothing loaded,
/// nothing `cleanSvg` could strip. 3D shapes are drawn as what they look like
/// from the side: a cube a square, a sphere a disc, a pyramid and a tetrahedron
/// a triangle (up, and down, so that the two stay apart).
fn write_symbol(out: &mut String, at: [f64; 2], kind: &str, size: f64, fill: &str, stroke: &str, zoom: Zoom) {
    let [x, y] = at;
    let r = (size / 2.0).max(0.5);
    let path = |pts: &[[f64; 2]], closed: bool| {
        let mut d = String::new();
        for (i, p) in pts.iter().enumerate() {
            // A closed outline is one run of segments; an open one is pairs (a stroke each).
            let start = if closed { i == 0 } else { i % 2 == 0 };
            let _ = write!(d, "{}{},{}", if start { "M" } else { "L" }, fmt(x + p[0] * r), fmt(y + p[1] * r));
        }
        if closed {
            d.push('Z');
        }
        format!("<path d=\"{d}\"")
    };
    let circle = |radius: f64| format!("<circle cx=\"{}\" cy=\"{}\" r=\"{}\"", fmt(x), fmt(y), fmt(radius));
    let body = match kind {
        "circle" | "sphere" => circle(r),
        "point" => circle((r / 2.0).max(1.5)),
        "cube" => path(&[[-0.85, -0.85], [0.85, -0.85], [0.85, 0.85], [-0.85, 0.85]], true),
        "diamond" => path(&[[0.0, -1.0], [0.8, 0.0], [0.0, 1.0], [-0.8, 0.0]], true),
        "pyramid" => path(&[[0.0, -1.0], [1.0, 0.85], [-1.0, 0.85]], true),
        "tetrahedron" => path(&[[-1.0, -0.85], [1.0, -0.85], [0.0, 1.0]], true),
        "cross" => path(&[[-0.8, -0.8], [0.8, 0.8], [-0.8, 0.8], [0.8, -0.8]], false),
        "plus" => path(&[[-1.0, 0.0], [1.0, 0.0], [0.0, -1.0], [0.0, 1.0]], false),
        "star" => {
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
        // The layout only gives shapes the drawer knows ([`crate::markers::is_drawn`]).
        _ => return,
    };
    let width = if fill == "none" { 2.0 } else { 1.0 };
    let _ = writeln!(
        out,
        "<g data-symbol=\"{}\"{}>{} fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/></g>",
        escape(kind),
        fixed(zoom, at),
        body,
        escape(fill),
        escape(stroke),
        fmt(width)
    );
}

fn points(p: &[[f64; 2]]) -> String {
    let mut s = String::with_capacity(p.len() * 12);
    for (i, q) in p.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(&fmt(q[0]));
        s.push(',');
        s.push_str(&fmt(q[1]));
    }
    s
}

/// A number, two decimals at most, no trailing zeros, no "-0", never `NaN`.
fn fmt(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    let t = format!("{v:.2}");
    let t = t.trim_end_matches('0').trim_end_matches('.');
    if t == "-0" || t.is_empty() { "0".to_string() } else { t.to_string() }
}

/// Text made safe for XML content and attribute values: the five characters
/// that mean something, and the control characters XML 1.0 cannot carry at all
/// are dropped (a title from a file may hold anything).
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
