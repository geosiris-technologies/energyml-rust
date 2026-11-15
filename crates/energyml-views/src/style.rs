//! What every view draws alike: the inks, the text sizes, the frame of a plot and its graduated axes, the band of
//! notes. One place, so that two views never draw an axis two ways.

use energyml_sheet::draw::ticks::{label, nice_ticks};
use energyml_sheet::draw::{Anchor, Colour, Element, Item, Layer, PageUnit, Rect, Stroke, TextStyle};

use crate::kind::ViewNote;

/// The ink of text and frames.
pub const INK: Colour = Colour::rgb(0x22, 0x22, 0x22);
/// The ink of what is secondary: graduations, axis titles.
pub const MUTED: Colour = Colour::rgb(0x6b, 0x6b, 0x6b);
/// The ink of grid lines.
pub const GRID: Colour = Colour::rgb(0xe3, 0xe3, 0xe3);
/// What is shown but not read from the file: a head portion assumed, an extrapolation.
pub const NEUTRAL: Colour = Colour::rgb(0xa8, 0xa8, 0xa8);
/// The ink of the subject of a view: a well's line.
pub const SUBJECT: Colour = Colour::rgb(0x2b, 0x4c, 0x7e);
/// What a view brings forward: an interval with data, a selection.
pub const HIGHLIGHT: Colour = Colour::rgb(0xe0, 0x8a, 0x1e);
/// The ink of a mark the user placed: a cursor at a depth.
pub const MARK: Colour = Colour::rgb(0xc0, 0x26, 0x26);
/// The ink of the notes: what a view left out.
pub const NOTE: Colour = Colour::rgb(0xb0, 0x30, 0x30);
/// Under everything.
pub const PAPER: Colour = Colour::rgb(0xff, 0xff, 0xff);

/// The size of ordinary text, pixels.
pub const TEXT: f64 = 11.0;
/// The size of a title, pixels.
pub const TITLE: f64 = 14.0;
/// The size of a graduation's label, pixels.
pub const SMALL: f64 = 9.5;

/// A number written short, two decimals at most: [`energyml_sheet::text::short`],
/// the one short formatter, so that the titles and the keys of every view
/// (`#md=...`) write a depth the same way.
pub fn short(value: f64) -> String {
    energyml_sheet::text::short(value, 2)
}

/// A text style: a size, an ink, an anchor, no rotation.
pub fn text(size: f64, colour: Colour, anchor: Anchor) -> TextStyle {
    TextStyle {
        size,
        colour,
        anchor,
        rotate: 0.0,
        weight: Default::default(),
    }
}

/// One line of text as an item.
pub fn label_item(at: [f64; 2], words: impl Into<String>, style: TextStyle) -> Item {
    Item::Text {
        at,
        text: words.into(),
        style,
    }
}

/// A linear map from a data interval to a page interval (either way round: a page's y grows down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    /// The data interval.
    pub data: [f64; 2],
    /// The page interval it lands on.
    pub page: [f64; 2],
}

impl Scale {
    /// The page position of a data value.
    pub fn at(&self, value: f64) -> f64 {
        let span = self.data[1] - self.data[0];
        if span == 0.0 {
            return (self.page[0] + self.page[1]) / 2.0;
        }
        self.page[0] + (value - self.data[0]) / span * (self.page[1] - self.page[0])
    }

    /// The data value at a page position.
    pub fn back(&self, page: f64) -> f64 {
        let span = self.page[1] - self.page[0];
        if span == 0.0 {
            return self.data[0];
        }
        self.data[0] + (page - self.page[0]) / span * (self.data[1] - self.data[0])
    }
}

/// The plot area of a picture and the two scales that land data in it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlotFrame {
    /// The plot area, pixels.
    pub area: Rect,
    /// Data x to page x.
    pub x: Scale,
    /// Data y to page y.
    pub y: Scale,
}

impl PlotFrame {
    /// A frame over `area` showing `x` and `y`; `y_down` when data y grows down the page (a depth).
    /// `equal` keeps one data unit the same length on both axes, the data centred in the area.
    pub fn new(area: Rect, x: [f64; 2], y: [f64; 2], y_down: bool, equal: bool) -> Self {
        let (mut x, mut y) = (widen(x), widen(y));
        if equal {
            let per_x = (x[1] - x[0]) / area.w;
            let per_y = (y[1] - y[0]) / area.h;
            let per = per_x.max(per_y);
            let grow = |range: [f64; 2], length: f64| {
                let middle = (range[0] + range[1]) / 2.0;
                [middle - per * length / 2.0, middle + per * length / 2.0]
            };
            x = grow(x, area.w);
            y = grow(y, area.h);
        }
        let x_scale = Scale {
            data: x,
            page: [area.x, area.x + area.w],
        };
        let y_scale = Scale {
            data: y,
            page: if y_down {
                [area.y, area.y + area.h]
            } else {
                [area.y + area.h, area.y]
            },
        };
        PlotFrame {
            area,
            x: x_scale,
            y: y_scale,
        }
    }

    /// A data point on the page.
    pub fn place(&self, point: [f64; 2]) -> [f64; 2] {
        [self.x.at(point[0]), self.y.at(point[1])]
    }
}

/// An interval with some length: a single value is widened by one unit each side.
fn widen(range: [f64; 2]) -> [f64; 2] {
    let (low, high) = (range[0].min(range[1]), range[0].max(range[1]));
    if high - low > 0.0 {
        let pad = (high - low) * 0.04;
        [low - pad, high + pad]
    } else {
        [low - 1.0, high + 1.0]
    }
}

/// One graduation of an axis: where it falls on the page along its axis, and its label.
#[derive(Clone, Debug, PartialEq)]
pub struct Tick {
    /// The page position along the axis (x for a horizontal axis, y for a vertical one).
    pub at: f64,
    /// The label written beside it.
    pub label: String,
}

/// The linear graduations of a scale, about `target` of them: each value [`nice_ticks`] picks, placed by the scale.
/// No graduation when the range gives none.
pub fn linear_ticks(scale: &Scale, target: f64) -> Vec<Tick> {
    let range = [scale.data[0].min(scale.data[1]), scale.data[0].max(scale.data[1])];
    match nice_ticks(range[0], range[1], target) {
        Ok(ticks) => ticks
            .values
            .iter()
            .copied()
            .map(|value| Tick {
                at: scale.at(value),
                label: label(value, ticks.step),
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// The frame of a plot with its grid and its graduations: **the one axis drawing**, whatever scale made the
/// graduations (a linear one by [`linear_ticks`], a logarithmic one by the caller's `Tick`s: the graduations are
/// data here, so a logarithmic axis is drawn the way a linear one is). `x` is the horizontal axis (graduations and
/// title), `None` for a plot with a single graduated axis (a band of bars); `y` the vertical one, its title turned.
pub fn graduated_layer(name: &str, area: Rect, x: Option<(&[Tick], &str)>, y: (&[Tick], &str)) -> Layer {
    let mut layer = Layer::new(name);
    let mut grid = Vec::new();
    let mut marks = Vec::new();
    if let Some((ticks, _)) = x {
        for tick in ticks {
            grid.push(Item::Polyline {
                points: vec![[tick.at, area.y], [tick.at, area.y + area.h]],
                stroke: Stroke::solid(GRID, 1.0),
            });
            marks.push(label_item(
                [tick.at, area.y + area.h + 13.0],
                tick.label.clone(),
                text(SMALL, MUTED, Anchor::Middle),
            ));
        }
    }
    for tick in y.0 {
        grid.push(Item::Polyline {
            points: vec![[area.x, tick.at], [area.x + area.w, tick.at]],
            stroke: Stroke::solid(GRID, 1.0),
        });
        marks.push(label_item(
            [area.x - 5.0, tick.at + 3.5],
            tick.label.clone(),
            text(SMALL, MUTED, Anchor::End),
        ));
    }
    layer.elements.push(Element::new(grid));
    layer.elements.push(Element::new(vec![Item::Rect {
        rect: area,
        corner: 0.0,
        fill: None,
        stroke: Some(Stroke::solid(MUTED, 1.0)),
    }]));
    if let Some((_, title)) = x {
        marks.push(label_item(
            [area.x + area.w / 2.0, area.y + area.h + 30.0],
            title,
            text(TEXT, MUTED, Anchor::Middle),
        ));
    }
    marks.push(Item::Text {
        at: [area.x - 42.0, area.y + area.h / 2.0],
        text: y.1.to_string(),
        style: TextStyle {
            rotate: -90.0,
            ..text(TEXT, MUTED, Anchor::Middle)
        },
    });
    layer.elements.push(Element::new(marks));
    layer
}

/// The frame of a plot, its grid and its graduated axes, with their titles: one layer, `axes`.
pub fn axes_layer(frame: &PlotFrame, x_title: &str, y_title: &str) -> Layer {
    let area = frame.area;
    graduated_layer(
        "axes",
        area,
        Some((&linear_ticks(&frame.x, (area.w / 90.0).max(2.0)), x_title)),
        (&linear_ticks(&frame.y, (area.h / 60.0).max(2.0)), y_title),
    )
}

/// The notes, one line each, in a band added under the picture: the page grows by the band's height. The band is
/// measured in the page's own unit: 11 pixels of text on a page in pixels, 3 mm on a page in millimetres.
pub fn notes_layer(size: &mut [f64; 2], unit: PageUnit, notes: &[ViewNote]) -> Option<Layer> {
    if notes.is_empty() {
        return None;
    }
    let (text_size, margin) = match unit {
        PageUnit::Pixel => (TEXT, 12.0),
        PageUnit::Millimetre => (3.0, 4.0),
    };
    let line = text_size * 1.36;
    let mut layer = Layer::new("notes");
    let top = size[1];
    // A note wider than the page is broken into lines (the sheet's own `wrap`, measured in the page's unit), and
    // every line is a row of the band: the band grows by the lines, not by the notes.
    let room = (size[0] - 2.0 * margin).max(text_size);
    let lines: Vec<String> = notes
        .iter()
        .flat_map(|note| energyml_sheet::draw::text::wrap(&note.says, room, text_size).lines)
        .collect();
    let items = lines
        .iter()
        .enumerate()
        .map(|(at, words)| {
            label_item(
                [margin, top + margin / 3.0 + line * (at as f64 + 1.0)],
                words.clone(),
                text(text_size, NOTE, Anchor::Start),
            )
        })
        .collect();
    layer.elements.push(Element::new(items));
    size[1] = top + margin + line * lines.len() as f64;
    Some(layer)
}
