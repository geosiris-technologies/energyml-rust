//! A [`Layout`] translated into a [`DisplayList`]: the sheet's one way to a picture.
//!
//! [`of_layout`] turns the page layout (sections, ticks, cells, ruler, already placed in millimetres by
//! [`crate::layout`]) into keyed elements in ordered layers. [`crate::svg::write`] is that list through
//! [`crate::draw::svg::write`]: **one SVG writer** for every picture, the sheet's included, so that a PNG, a hit
//! test or a window reads the same list. This module decides nothing about the page except the horizontal layout
//! (below); every length it does not compute is the layout's.
//!
//! It sits outside [`crate::draw`] on purpose: it names [`crate::layout`] (and so `energyml_domain`'s colour),
//! which the drawing module must never do (`tests/draw/no_format.rs`).
//!
//! # Units
//!
//! The list is in millimetres ([`PageUnit::Millimetre`]) and so is every text size, **in user units**:
//! `text_size_pt × 25.4 / 72`, never points. An SVG converts `pt` into user units as if one unit were one CSS pixel,
//! then scales by the `viewBox`, so under a millimetre `viewBox` a `font-size="8pt"` came out 10.7 mm tall, four
//! times the 2.8 mm the layout had reserved, and every label ran over its neighbour (the first sheet drawn from
//! Lutetia). `tests/svg.rs` holds the conversion.
//!
//! # What is this module's own choice, not the specification's
//!
//! The **horizontal** layout - how wide each column is on the page - is not decided anywhere in the
//! specification; only a *text* column's width is (`max_chars`). `column_widths_mm` is a heuristic: a text column
//! is `max_chars` wide, a fill-only column and the MD column get a fixed share. The row is **stretched** to
//! [`Layout::page_width_mm`] when it is narrower, and the page **widens** when it is wider: a column is never
//! squeezed below its natural width ([`page_width_of`]). The first version squeezed every row into 210 mm: a text
//! column cut for 32 characters (about 48 mm) was drawn 16 mm wide, and every description ran over its neighbour.
//!
//! # The layers, back to front
//!
//! | Layer | Holds |
//! |---|---|
//! | `header` | the title, the mode note and every column's own header (a sticky table header: a page that scrolls the body can leave it in place) |
//! | `sections` | one line per section boundary, across the whole row |
//! | `ruler` | the margin ruler and its oblique connectors, only when the mode is deformed |
//! | `tracks` | the MD column's ticks and every interval column's cells |
//!
//! # Keys
//!
//! An element a user can point at carries a key ([`Element::key`]), built on [`PictureStyle::object`] (the frame's
//! full uri) when the caller names one:
//!
//! - a depth label (`tick`): `<object>#md=<md>`;
//! - a cell (`cell`): `<uri>#col=<track>&md=<md_top>` and, when the interval came from a table row,
//!   `&row=<row>`, where `<uri>` is the uri of the interval the cell was drawn from, else the object. A cell with
//!   neither is a decoration, with no key.
//!
//! Every cell also carries `md-top`, `md-base`, `uri` and `row` (when it has them) and `blank` (when the interval
//! states nothing) as `data-*` attributes: the hooks a page's own script hangs a hover or a click on.
//!
//! # What the list cannot say
//!
//! A [`TextStyle`] has a size, a colour, an anchor, a rotation and a weight, and no slant: the title and the column
//! headers are bold, the mode note, once italic, is upright.

use energyml_domain::color::Rgba;

use crate::draw::{
    Anchor, Colour, DisplayList, Element, FontWeight, Item, Layer, LineCap, PageUnit, Paint, Rect, Stroke,
    TextStyle,
};
use crate::layout::{LaidCell, LaidTrack, Layout, PT_TO_MM, line_height_mm};
use crate::text;

/// The MD column's width, in characters - this module's own guess: nothing in the specification sizes it.
const MD_COLUMN_CHARS: f64 = 10.0;
/// A fill-only column's width, in millimetres - likewise a guess.
const FILL_ONLY_COLUMN_MM: f64 = 15.0;
/// No column is drawn narrower than this, whatever its render says.
const MIN_COLUMN_MM: f64 = 8.0;
/// The margin ruler's own width.
const RULER_WIDTH_MM: f64 = 6.0;
/// Left padding inside a cell, before its text starts.
const CELL_PAD_MM: f64 = 1.0;
/// The title a sheet without one carries in its `<title>`.
const UNTITLED: &str = "Drilling sheet";

/// How a sheet is inked, and what its keys are built on.
#[derive(Clone, Debug, PartialEq)]
pub struct PictureStyle {
    /// The ink of texts, rule lines and cell outlines.
    pub ink: Colour,
    /// The ink of the ruler's oblique connectors: a guide, not the data.
    pub guide: Colour,
    /// Every stroked line's width, in millimetres - thin on purpose: a paper sheet's rule lines are not the data.
    pub stroke_mm: f64,
    /// Under everything; `None`: transparent.
    pub background: Option<Colour>,
    /// The full uri of the object the sheet is of (its frame): the base of the keys of the depth labels, and of
    /// the cells that name no uri of their own. `None`: those carry no key.
    pub object: Option<String>,
}

impl Default for PictureStyle {
    /// Black on transparent, hairlines, no object.
    fn default() -> Self {
        PictureStyle {
            ink: Colour::rgb(0, 0, 0),
            guide: Colour::rgb(0x99, 0x99, 0x99),
            stroke_mm: 0.1,
            background: None,
            object: None,
        }
    }
}

/// The page's width: the one asked for, or the row's natural width when that is wider - a column is never
/// squeezed below the width its text was cut for (module doc, *What is this module's own choice*).
pub fn page_width_of(requested_mm: f64, natural_mm: f64) -> f64 {
    requested_mm.max(natural_mm)
}

/// This module's guess at each track's width, before the whole row is scaled to fit the page - see the module doc.
fn column_widths_mm(layout: &Layout) -> Vec<f64> {
    layout
        .tracks
        .iter()
        .map(|track| match track {
            LaidTrack::Md { .. } => {
                MD_COLUMN_CHARS * text::average_char_width_pt(layout.text_size_pt) * PT_TO_MM
            }
            LaidTrack::Intervals {
                max_chars, renders, ..
            } => {
                if renders.iter().any(|render| render == "text") {
                    *max_chars as f64 * text::average_char_width_pt(layout.text_size_pt) * PT_TO_MM
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

/// A colour of the layout as a colour of the list.
fn colour_of(colour: Rgba) -> Colour {
    let [r, g, b, a] = colour.to_bytes();
    Colour::rgba(r, g, b, a)
}

/// A rule line: flat ends, so that two boundaries meet the row's edges exactly.
fn rule(colour: Colour, width: f64) -> Stroke {
    let mut stroke = Stroke::solid(colour, width);
    stroke.cap = LineCap::Butt;
    stroke
}

fn text_item(at: [f64; 2], words: &str, size: f64, ink: Colour, anchor: Anchor) -> Item {
    Item::Text {
        at,
        text: words.to_string(),
        style: TextStyle {
            size,
            colour: ink,
            anchor,
            rotate: 0.0,
            weight: Default::default(),
        },
    }
}

/// A text item in bold: the title and the column headers.
fn bold_item(at: [f64; 2], words: &str, size: f64, ink: Colour, anchor: Anchor) -> Item {
    let mut item = text_item(at, words, size, ink, anchor);
    if let Item::Text { style, .. } = &mut item {
        style.weight = FontWeight::Bold;
    }
    item
}

fn element(
    key: Option<String>,
    kind: &str,
    title: &str,
    data: Vec<(&str, String)>,
    items: Vec<Item>,
) -> Element {
    Element {
        key,
        kind: kind.to_string(),
        title: title.to_string(),
        data: data
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect(),
        items,
    }
}

/// A length printed as the shortest text that reads back the same (no exponent, `10` for ten).
fn plain(value: f64) -> String {
    format!("{value}")
}

/// A depth as an attribute: three decimals, as the layout's lengths are written.
fn fixed(value: f64) -> String {
    format!("{value:.3}")
}

/// The key of one cell, if it has anything to be found by (module doc, *Keys*).
fn cell_key(style: &PictureStyle, cell: &LaidCell, track: usize, md_top: f64) -> Option<String> {
    let base = cell.uri.as_deref().or(style.object.as_deref())?;
    let mut key = format!("{base}#col={track}&md={}", plain(md_top));
    if let Some(row) = &cell.row {
        key.push_str(&format!("&row={row}"));
    }
    Some(key)
}

/// Translate `layout` into a display list, in millimetres, styled by `style`.
///
/// The list is only as valid as the layout is finite and non-empty: a sheet with no section has no height, and
/// [`DisplayList::validate`] (so [`crate::draw::svg::write`]) refuses it by name rather than draw an empty page.
pub fn of_layout(layout: &Layout, style: &PictureStyle) -> DisplayList {
    let line_height = line_height_mm(layout.text_size_pt);
    // In user units - millimetres - see the module doc.
    let text_size = layout.text_size_pt * PT_TO_MM;
    let header_mm = layout.header_height_mm;

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

    // `x[i]` is where track `i` starts; `x[tracks.len()]` is the row's own right edge. `x[0]` is the ruler's
    // width, scaled - the ruler sits to the left of every track, in the margin the deformed modes ask for.
    let mut x: Vec<f64> = Vec::with_capacity(layout.tracks.len() + 1);
    x.push(ruler_natural * scale);
    for width in &natural_widths {
        x.push(x.last().copied().unwrap_or(0.0) + width * scale);
    }
    let row_right = x.last().copied().unwrap_or(layout.page_width_mm);

    let mut list = DisplayList::new(
        [page_width_mm, header_mm + layout.body_height_mm],
        PageUnit::Millimetre,
    );
    list.title = layout.title.clone().unwrap_or_else(|| UNTITLED.to_string());
    list.background = style.background;

    // -- header ------------------------------------------------------------------------------------------------
    let mut header = Layer::new("header");
    let mut row_y = line_height;
    if let Some(title) = &layout.title {
        header.elements.push(element(
            None,
            "title",
            "",
            Vec::new(),
            vec![bold_item(
                [row_right / 2.0, row_y],
                title,
                text_size,
                style.ink,
                Anchor::Middle,
            )],
        ));
        row_y += line_height;
    }
    header.elements.push(element(
        None,
        "mode-note",
        "",
        Vec::new(),
        vec![text_item(
            [0.0, row_y],
            &layout.mode_note,
            text_size,
            style.ink,
            Anchor::Start,
        )],
    ));
    row_y += line_height;
    for (index, track) in layout.tracks.iter().enumerate() {
        let x0 = x[index] + CELL_PAD_MM;
        let lines: &[String] = match track {
            LaidTrack::Md { header, .. } => std::slice::from_ref(header),
            LaidTrack::Intervals { header_lines, .. } => header_lines.as_slice(),
        };
        // `row_y` is already the next baseline, the mode note's line having advanced it: adding one more put every
        // column header a line below the height the layout reserved for the header, on top of the body's first
        // row (the first sheet).
        let items = lines
            .iter()
            .enumerate()
            .map(|(at, line)| {
                bold_item(
                    [x0, row_y + line_height * at as f64],
                    line,
                    text_size,
                    style.ink,
                    Anchor::Start,
                )
            })
            .collect();
        header.elements.push(element(
            None,
            "column-header",
            "",
            vec![("column", index.to_string())],
            items,
        ));
    }
    list.layers.push(header);

    // -- the section boundaries: "at each MD... a black line" spans the whole row, not just the MD column --------
    let mut boundaries: Vec<(f64, f64)> = layout
        .sections
        .iter()
        .map(|section| (section.md_top, section.y_top_mm))
        .collect();
    if let Some(last) = layout.sections.last() {
        boundaries.push((last.md_base, last.y_base_mm));
    }
    let mut sections = Layer::new("sections");
    for (md, y) in &boundaries {
        let at = header_mm + y;
        sections.elements.push(element(
            None,
            "boundary",
            "",
            vec![("md", fixed(*md))],
            vec![Item::Polyline {
                points: vec![[0.0, at], [row_right, at]],
                stroke: rule(style.ink, style.stroke_mm),
            }],
        ));
    }
    list.layers.push(sections);

    // -- the ruler -----------------------------------------------------------------------------------------------
    if let Some(ruler) = &layout.ruler {
        let ruler_right = x.first().copied().unwrap_or(0.0);
        let mut layer = Layer::new("ruler");
        layer.elements.push(element(
            None,
            "ruler",
            "",
            Vec::new(),
            vec![Item::Polyline {
                points: vec![[0.0, header_mm], [0.0, header_mm + ruler.total_mm]],
                stroke: rule(style.ink, style.stroke_mm),
            }],
        ));
        // The oblique connectors: the true position at the ruler's own left edge, the deformed one at the track
        // row's left edge - one line per boundary the ruler and the row share.
        for (y_true, (md, y_deformed)) in ruler.y_true_mm.iter().zip(&boundaries) {
            layer.elements.push(element(
                None,
                "ruler-link",
                "",
                vec![("md", fixed(*md))],
                vec![Item::Polyline {
                    points: vec![[0.0, header_mm + y_true], [ruler_right, header_mm + y_deformed]],
                    stroke: rule(style.guide, style.stroke_mm),
                }],
            ));
        }
        list.layers.push(layer);
    }

    // -- the tracks ----------------------------------------------------------------------------------------------
    let mut tracks = Layer::new("tracks");
    for (index, track) in layout.tracks.iter().enumerate() {
        let x0 = x[index];
        let width = x[index + 1] - x0;
        match track {
            LaidTrack::Md { ticks, .. } => {
                for (tick_index, tick) in ticks.iter().enumerate() {
                    // A label hangs below its line - except the last one, the well's base, whose line is the page's
                    // bottom edge: below it the label would be outside the picture.
                    let baseline = if tick_index + 1 == ticks.len() && tick_index > 0 {
                        tick.y_mm - line_height * 0.25
                    } else {
                        tick.y_mm + line_height * 0.9
                    };
                    let key = style
                        .object
                        .as_ref()
                        .map(|object| format!("{object}#md={}", plain(tick.md)));
                    tracks.elements.push(element(
                        key,
                        "tick",
                        "",
                        vec![("md", fixed(tick.md))],
                        vec![text_item(
                            [x0 + CELL_PAD_MM, header_mm + baseline],
                            &tick.label,
                            text_size,
                            style.ink,
                            Anchor::Start,
                        )],
                    ));
                }
            }
            LaidTrack::Intervals { cells, .. } => {
                for (section_index, cell) in cells.iter().enumerate() {
                    let Some(section) = layout.sections.get(section_index) else {
                        continue;
                    };
                    let y0 = header_mm + section.y_top_mm;
                    let y1 = header_mm + section.y_base_mm;
                    let mut items = vec![Item::Rect {
                        rect: Rect {
                            x: x0,
                            y: y0,
                            w: width,
                            h: y1 - y0,
                        },
                        corner: 0.0,
                        fill: cell.colour.map(|colour| Paint::Solid(colour_of(colour))),
                        stroke: Some(Stroke::solid(style.ink, style.stroke_mm)),
                    }];
                    if let Some(words) = &cell.text {
                        for (line_index, line) in words.lines.iter().enumerate() {
                            items.push(text_item(
                                [x0 + CELL_PAD_MM, y0 + line_height * (line_index as f64 + 1.0)],
                                line,
                                text_size,
                                style.ink,
                                Anchor::Start,
                            ));
                        }
                    }
                    let mut data = vec![
                        ("md-top", fixed(section.md_top)),
                        ("md-base", fixed(section.md_base)),
                    ];
                    if let Some(uri) = &cell.uri {
                        data.push(("uri", uri.clone()));
                    }
                    if let Some(row) = &cell.row {
                        data.push(("row", row.clone()));
                    }
                    if cell.blank {
                        // "An interval without a value is a stated blank" - said in the markup, not just an
                        // absence a reader has to notice on their own.
                        data.push(("blank", "true".to_string()));
                    }
                    let title = cell.text.as_ref().map_or("", |words| words.full_text.as_str());
                    tracks.elements.push(element(
                        cell_key(style, cell, index, section.md_top),
                        "cell",
                        title,
                        data,
                        items,
                    ));
                }
            }
        }
    }
    list.layers.push(tracks);

    list
}
