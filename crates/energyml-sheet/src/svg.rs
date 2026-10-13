//! Writing a [`crate::layout::Layout`] as SVG — the sheet's one point of
//! output: a web page inserts these bytes unchanged,
//! the CLI writes them unchanged, and a PNG or PDF export (not written
//! yet) would read *this* string rather than the layout a second time.
//!
//! # Units, and what carries what
//!
//! The document is sized in millimetres — `width="…mm"` and a `viewBox`
//! whose numbers are the same magnitude, so one user unit is one
//! millimetre — and every length this module computes (a column's width, a
//! section's height) is already in that unit, straight out of
//! [`crate::layout`]. **The font size too**: it is written in user units,
//! that is in millimetres (`text_size_pt × 25.4 / 72`), never as `…pt`. An
//! SVG converts `pt` into user units as if one unit were one CSS pixel, *then*
//! scales by the `viewBox` like everything else — so under a millimetre
//! `viewBox`, `font-size="8pt"` came out 10.7 mm tall, four times the 2.8 mm
//! the layout had reserved, and every label ran over its neighbour (the first
//! sheet drawn from Lutetia). `tests/svg.rs` holds the conversion.
//!
//! # What is this module's own choice, not the specification's
//!
//! The **horizontal** layout — how wide each column is on the page — is not
//! decided anywhere in the specification; only a *text* column's width is
//! (`max_chars`). `column_widths_mm` is a heuristic: a text column
//! is `max_chars` wide, a fill-only column and the MD column get a fixed
//! share. The row is **stretched** to [`crate::layout::Layout::page_width_mm`]
//! when it is narrower, and the page **widens** when it is wider — a column is
//! never squeezed below its natural width. The first version squeezed every
//! row into 210 mm: a text column cut for 32 characters (≈ 48 mm) was drawn
//! 16 mm wide, and every description ran over its neighbour (the first sheet
//! drawn from Lutetia). The sheet already grows downward rather than
//! truncate; it grows sideways for the same reason. [`page_width_of`]
//! is that one decision, and `tests/svg.rs` holds it.
//!
//! # The two groups
//!
//! `<g id="header">` carries the title, the mode note (written on the
//! sheet), and every column's own header — a sticky table header, so a page
//! that scrolls the body can leave it in place. `<g id="body">` carries
//! everything positioned by MD: the section separators, the ruler when the
//! mode is deformed, and every column's cells.
//!
//! # `data-*`
//!
//! Every cell carries `data-md-top` and `data-md-base` always, and
//! `data-uri` / `data-row` when the interval it was drawn from named one —
//! the hooks a page's own script hangs a hover or a click on, so the SVG
//! never needs one embedded in it.

use crate::layout::{LaidCell, LaidTrack, Layout};
use crate::text;

/// The MD column's width, in characters — this module's own guess (see the
/// module doc): nothing in the specification sizes it.
const MD_COLUMN_CHARS: f64 = 10.0;
/// A fill-only column's width, in millimetres — likewise a guess.
const FILL_ONLY_COLUMN_MM: f64 = 15.0;
/// No column is drawn narrower than this, whatever its render says.
const MIN_COLUMN_MM: f64 = 8.0;
/// The margin ruler's own width.
const RULER_WIDTH_MM: f64 = 6.0;
/// Left padding inside a cell, before its text starts.
const CELL_PAD_MM: f64 = 1.0;
/// Every stroked line's width, in millimetres — thin, on purpose: a paper
/// sheet's rule lines are not the data.
const STROKE_WIDTH_MM: f64 = 0.1;

/// Write `layout` as a complete SVG document.
pub fn write(layout: &Layout) -> String {
    let line_height = crate::layout::line_height_mm(layout.text_size_pt);

    let natural_widths = column_widths_mm(layout);
    let ruler_natural = if layout.ruler.is_some() {
        RULER_WIDTH_MM
    } else {
        0.0
    };
    let natural_total: f64 = ruler_natural + natural_widths.iter().sum::<f64>();
    let page_width_mm = page_width_of(layout.page_width_mm, natural_total);
    let scale = if natural_total > 0.0 {
        page_width_mm / natural_total
    } else {
        1.0
    };

    // `x[i]` is where track `i` starts; `x[tracks.len()]` is the row's own
    // right edge. `x[0]` is the ruler's width, scaled — the ruler sits to
    // the left of every track, in the margin the deformed modes ask for.
    let mut x: Vec<f64> = Vec::with_capacity(layout.tracks.len() + 1);
    x.push(ruler_natural * scale);
    for width in &natural_widths {
        x.push(x.last().copied().unwrap_or(0.0) + width * scale);
    }

    let page_height_mm = layout.header_height_mm + layout.body_height_mm;

    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}mm\" height=\"{}mm\" viewBox=\"0 0 {} {}\" \
         font-family=\"Liberation Sans, Arial, Helvetica, sans-serif\" font-size=\"{}\">\n",
        fmt(page_width_mm),
        fmt(page_height_mm),
        fmt(page_width_mm),
        fmt(page_height_mm),
        // In user units — millimetres — see the module doc.
        fmt(layout.text_size_pt * crate::layout::PT_TO_MM),
    ));
    out.push_str(&format!(
        "<title>{}</title>\n",
        escape(layout.title.as_deref().unwrap_or("Drilling sheet"))
    ));

    write_header(&mut out, layout, &x, line_height);
    write_body(&mut out, layout, &x, line_height);

    out.push_str("</svg>\n");
    out
}

/// The page's width: the one asked for, or the row's natural width when that
/// is wider — a column is never squeezed below the width its text was cut for
/// (module doc, *What is this module's own choice*).
pub fn page_width_of(requested_mm: f64, natural_mm: f64) -> f64 {
    requested_mm.max(natural_mm)
}

/// This module's guess at each track's width, before the whole row is
/// scaled to fit the page — see the module doc.
fn column_widths_mm(layout: &Layout) -> Vec<f64> {
    layout
        .tracks
        .iter()
        .map(|track| match track {
            LaidTrack::Md { .. } => {
                MD_COLUMN_CHARS * text::average_char_width_pt(layout.text_size_pt) * crate::layout::PT_TO_MM
            }
            LaidTrack::Intervals {
                max_chars, renders, ..
            } => {
                if renders.iter().any(|render| render == "text") {
                    *max_chars as f64
                        * text::average_char_width_pt(layout.text_size_pt)
                        * crate::layout::PT_TO_MM
                } else if renders.iter().any(|render| render == "fill") {
                    FILL_ONLY_COLUMN_MM
                } else {
                    MIN_COLUMN_MM
                }
            }
        })
        .map(|width: f64| width.max(MIN_COLUMN_MM))
        .collect()
}

/// `<g id="header">` — the title, the mode note, and every column's own
/// header, sticky-table-header fashion (module doc).
fn write_header(out: &mut String, layout: &Layout, x: &[f64], line_height: f64) {
    out.push_str("<g id=\"header\">\n");
    let mut row_y = line_height;
    if let Some(title) = &layout.title {
        out.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-weight=\"bold\">{}</text>\n",
            fmt(x.last().copied().unwrap_or(layout.page_width_mm) / 2.0),
            fmt(row_y),
            escape(title)
        ));
        row_y += line_height;
    }
    out.push_str(&format!(
        "<text x=\"0\" y=\"{}\" font-style=\"italic\">{}</text>\n",
        fmt(row_y),
        escape(&layout.mode_note)
    ));
    row_y += line_height;

    for (index, track) in layout.tracks.iter().enumerate() {
        let x0 = x[index] + CELL_PAD_MM;
        let lines: &[String] = match track {
            LaidTrack::Md { header, .. } => std::slice::from_ref(header),
            LaidTrack::Intervals { header_lines, .. } => header_lines.as_slice(),
        };
        for (line_index, line) in lines.iter().enumerate() {
            out.push_str(&format!(
                "<text x=\"{}\" y=\"{}\" font-weight=\"bold\">{}</text>\n",
                fmt(x0),
                // `row_y` is already the next baseline, the mode note's line
                // having advanced it: adding one more put every column header
                // a line below the height `layout` reserved for the header,
                // on top of the body's first row (the first sheet).
                fmt(row_y + line_height * line_index as f64),
                escape(line)
            ));
        }
    }
    out.push_str("</g>\n");
}

/// `<g id="body">` — the section separators, the margin ruler, and every
/// column's cells, all positioned from the body's own top (the header sits
/// above it, and this group is translated down past it).
fn write_body(out: &mut String, layout: &Layout, x: &[f64], line_height: f64) {
    out.push_str(&format!(
        "<g id=\"body\" transform=\"translate(0, {})\">\n",
        fmt(layout.header_height_mm)
    ));

    // Every section boundary, once — "at each MD... a black line" spans
    // the whole row, not just the MD column (the original request, sheet
    // design).
    let mut boundary_ys: Vec<f64> = layout.sections.iter().map(|section| section.y_top_mm).collect();
    if let Some(last) = layout.sections.last() {
        boundary_ys.push(last.y_base_mm);
    }
    let row_right = x.last().copied().unwrap_or(layout.page_width_mm);
    for boundary_y in &boundary_ys {
        let y = fmt(*boundary_y);
        out.push_str(&format!(
            "<line x1=\"0\" y1=\"{y}\" x2=\"{}\" y2=\"{y}\" stroke=\"#000000\" stroke-width=\"{}\"/>\n",
            fmt(row_right),
            fmt(STROKE_WIDTH_MM),
        ));
    }

    if let Some(ruler) = &layout.ruler {
        let ruler_right = x.first().copied().unwrap_or(0.0);
        out.push_str(&format!(
            "<line x1=\"0\" y1=\"0\" x2=\"0\" y2=\"{}\" stroke=\"#000000\" stroke-width=\"{}\"/>\n",
            fmt(ruler.total_mm),
            fmt(STROKE_WIDTH_MM)
        ));
        // The oblique connectors: the true position at
        // the ruler's own left edge, the deformed one at the track row's
        // left edge — one line per boundary the ruler and the row share.
        for (y_true, y_deformed) in ruler.y_true_mm.iter().zip(&boundary_ys) {
            out.push_str(&format!(
                "<line x1=\"0\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#999999\" stroke-width=\"{}\"/>\n",
                fmt(*y_true),
                fmt(ruler_right),
                fmt(*y_deformed),
                fmt(STROKE_WIDTH_MM)
            ));
        }
    }

    for (index, track) in layout.tracks.iter().enumerate() {
        let x0 = x[index];
        let width = x[index + 1] - x0;
        match track {
            LaidTrack::Md { ticks, .. } => {
                for (tick_index, tick) in ticks.iter().enumerate() {
                    // A label hangs below its line — except the last one, the
                    // well's base, whose line is the page's bottom edge: below
                    // it the label would be outside the picture.
                    let baseline = if tick_index + 1 == ticks.len() && tick_index > 0 {
                        tick.y_mm - line_height * 0.25
                    } else {
                        tick.y_mm + line_height * 0.9
                    };
                    out.push_str(&format!(
                        "<text x=\"{}\" y=\"{}\" data-md=\"{}\">{}</text>\n",
                        fmt(x0 + CELL_PAD_MM),
                        fmt(baseline),
                        fmt(tick.md),
                        escape(&tick.label)
                    ));
                }
            }
            LaidTrack::Intervals { cells, .. } => {
                for (section_index, cell) in cells.iter().enumerate() {
                    let Some(section) = layout.sections.get(section_index) else {
                        continue;
                    };
                    write_cell(
                        out,
                        cell,
                        section.md_top,
                        section.md_base,
                        x0,
                        width,
                        section.y_top_mm,
                        section.y_base_mm,
                        line_height,
                    );
                }
            }
        }
    }

    out.push_str("</g>\n");
}

/// One interval track's one cell: its `<rect>`, its wrapped text if any, and
/// the `data-*` attributes the sheet design asks for.
#[allow(clippy::too_many_arguments)]
fn write_cell(
    out: &mut String,
    cell: &LaidCell,
    md_top: f64,
    md_base: f64,
    x0: f64,
    width: f64,
    y0: f64,
    y1: f64,
    line_height: f64,
) {
    let mut attributes = format!(
        " data-md-top=\"{}\" data-md-base=\"{}\"",
        fmt(md_top),
        fmt(md_base)
    );
    if let Some(uri) = &cell.uri {
        attributes.push_str(&format!(" data-uri=\"{}\"", escape(uri)));
    }
    if let Some(row) = &cell.row {
        attributes.push_str(&format!(" data-row=\"{}\"", escape(row)));
    }
    if cell.blank {
        // "An interval without a value is a stated blank" — said in the
        // markup, not just an absence a reader has to notice on their own.
        attributes.push_str(" data-blank=\"true\"");
    }
    out.push_str(&format!("<g{attributes}>\n"));
    if let Some(text) = &cell.text {
        out.push_str(&format!("<title>{}</title>\n", escape(&text.full_text)));
    }
    let fill = cell
        .colour
        .map(|colour| colour.to_hex())
        .unwrap_or_else(|| "none".to_string());
    out.push_str(&format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\" stroke=\"#000000\" stroke-width=\"{}\"/>\n",
        fmt(x0),
        fmt(y0),
        fmt(width),
        fmt(y1 - y0),
        fill,
        fmt(STROKE_WIDTH_MM)
    ));
    if let Some(text) = &cell.text {
        for (line_index, line) in text.lines.iter().enumerate() {
            out.push_str(&format!(
                "<text x=\"{}\" y=\"{}\">{}</text>\n",
                fmt(x0 + CELL_PAD_MM),
                fmt(y0 + line_height * (line_index as f64 + 1.0)),
                escape(line)
            ));
        }
    }
    out.push_str("</g>\n");
}

/// A length or coordinate, in millimetres, printed without scientific
/// notation and with enough precision that two nearby sections never round
/// to the same line.
fn fmt(value: f64) -> String {
    format!("{value:.3}")
}

/// Escape the five characters XML requires inside text content and
/// double-quoted attribute values.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}
