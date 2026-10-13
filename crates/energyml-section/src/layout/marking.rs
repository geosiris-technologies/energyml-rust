//! Markers, laid out (ADR-0015).
//!
//! Two steps, because the legend takes its room on the right **before** the
//! frame is known, and a symbol needs the frame to find its pixel:
//!
//! 1. [`resolve`] works in the drawing's units, before the frame: which markers
//!    can be drawn at all, where each of their objects carries them, and one
//!    note per cause for what cannot. The legend lists what this step kept —
//!    **what is drawn is what the legend says**, from one list.
//! 2. [`Resolved::contours`], [`Resolved::symbols`] and [`Resolved::legend`] put
//!    that list on the map.
//!
//! A marker is drawn on the objects the drawing holds: a well (its head, its
//! bottom, a measured depth) or a surface's trace (the first or last point of
//! its single span). A key the drawing does not hold — hidden, outside the
//! portion, or not a well or a surface — is counted and said, never drawn
//! elsewhere. A shape the drawer does not know is refused by its name.

use std::collections::HashSet;

use super::{
    Anchor, Attr, FONT_PX, INK, KeptTrace, LEGEND_MAX_PX, LEGEND_MIN_PX, LINE_PX, LaidObject, Map, SMALL_PX, Shape, labels, width_of,
};
use crate::markers::{self, CONTOUR};
use crate::model::{MarkerPlace, View, WellItem};

/// The largest symbol a marker may ask for, pixels.
const MAX_SIZE_PX: f64 = 64.0;
/// How much wider than the object a contour is: a halo of this many pixels.
const CONTOUR_HALO_PX: f64 = 6.0;
/// The symbol in the legend, pixels.
const LEGEND_SYMBOL_PX: f64 = 9.0;

/// What the legend lists: a marker that is drawn at least once.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Entry {
    pub name: String,
    pub shape: String,
    pub colour: String,
}

enum Spot {
    /// A symbol at a point of the drawing's units.
    Point([f64; 2]),
    /// A contour along these lines, with the width of the object under it.
    Line(Vec<Vec<[f64; 2]>>, f64),
}

struct Placed {
    entry: usize,
    key: String,
    title: String,
    size: f64,
    spot: Spot,
}

/// The markers that can be drawn, and where.
pub(super) struct Resolved {
    pub entries: Vec<Entry>,
    placed: Vec<Placed>,
}

fn valid_colour(colour: &str) -> bool {
    colour.strip_prefix('#').is_some_and(|hex| (hex.len() == 6 || hex.len() == 8) && hex.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Why a marker has no point on a well.
enum Miss {
    NoMd,
    Outside,
    Nothing,
}

fn well_point(w: &WellItem, place: MarkerPlace) -> Result<[f64; 2], Miss> {
    match place {
        MarkerPlace::Head => Ok(w.head),
        MarkerPlace::Bottom => w.path.last().copied().ok_or(Miss::Nothing),
        MarkerPlace::Md { value } => {
            let md = w.md.as_ref().filter(|md| md.len() == w.path.len() && md.len() >= 2).ok_or(Miss::NoMd)?;
            for i in 0..md.len() - 1 {
                let (m0, m1) = (md[i], md[i + 1]);
                if value >= m0.min(m1) && value <= m0.max(m1) {
                    let t = if m1 == m0 { 0.0 } else { (value - m0) / (m1 - m0) };
                    let (a, b) = (w.path[i], w.path[i + 1]);
                    return Ok([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
                }
            }
            Err(Miss::Outside)
        }
    }
}

/// Decide, in the drawing's units, what the view's markers draw — and say what
/// they cannot, one note per marker and cause, counted.
pub(super) fn resolve(view: &View, wells: &[&WellItem], traces: &[KeptTrace<'_>], warnings: &mut Vec<String>) -> Resolved {
    let mut out = Resolved { entries: Vec::new(), placed: Vec::new() };
    let mut unnamed = 0;
    for marker in view.markers.iter().filter(|marker| !marker.hidden) {
        let name = marker.name.trim();
        if name.is_empty() {
            unnamed += 1;
            continue;
        }
        let refusal = if !markers::is_drawn(&marker.shape) {
            Some(format!("shape \"{}\" is not drawn by this build", marker.shape))
        } else if !valid_colour(&marker.colour) {
            Some(format!("colour \"{}\" is not #rrggbb or #rrggbbaa", marker.colour))
        } else if !(marker.size_px.is_finite() && marker.size_px > 0.0 && marker.size_px <= MAX_SIZE_PX) {
            Some(format!("size {} px is not above 0 and at most {MAX_SIZE_PX}", marker.size_px))
        } else if matches!(marker.at, MarkerPlace::Md { value } if !value.is_finite()) {
            Some("the measured depth is not a number".to_string())
        } else {
            None
        };
        if let Some(why) = refusal {
            warnings.push(format!("marker \"{name}\" not drawn: {why}"));
            continue;
        }

        let (mut absent, mut no_md, mut outside, mut unplaceable) = (0, 0, 0, 0);
        let mut seen_wells: HashSet<usize> = HashSet::new();
        let mut seen_keys: HashSet<&str> = HashSet::new();
        let entry = out.entries.len();
        let before = out.placed.len();
        for key in marker.keys.iter().filter(|k| seen_keys.insert(k.as_str())) {
            // A wellbore's frame and trajectory are one line: the marker of
            // either lands on it.
            let well = wells.iter().enumerate().find(|(_, w)| w.object.key() == *key || w.merged.iter().any(|m| m.key() == *key));
            if let Some((at, w)) = well {
                if !seen_wells.insert(at) {
                    continue;
                }
                let title = format!("{name} — {}", w.object.title);
                if marker.shape == CONTOUR {
                    out.placed.push(Placed {
                        entry,
                        key: key.clone(),
                        title,
                        size: marker.size_px,
                        spot: Spot::Line(vec![w.path.clone()], width_of(view, &w.object.key(), view.well_width_px.max(1.0))),
                    });
                    continue;
                }
                match well_point(w, marker.at) {
                    Ok(point) => out.placed.push(Placed { entry, key: key.clone(), title, size: marker.size_px, spot: Spot::Point(point) }),
                    Err(Miss::NoMd) => no_md += 1,
                    Err(Miss::Outside) => outside += 1,
                    Err(Miss::Nothing) => unplaceable += 1,
                }
            } else if let Some(t) = traces.iter().find(|t| t.key == *key) {
                let title = format!("{name} — {}", t.item.object.title);
                if marker.shape == CONTOUR {
                    let lines: Vec<Vec<[f64; 2]>> = t.spans.iter().map(|s| s.to_vec()).collect();
                    out.placed.push(Placed {
                        entry,
                        key: key.clone(),
                        title,
                        size: marker.size_px,
                        spot: Spot::Line(lines, width_of(view, &t.key, 1.5)),
                    });
                    continue;
                }
                // A surface has no measured depth, and its first or last point
                // is only one when it is drawn in one piece.
                let point = match (marker.at, t.spans.as_slice()) {
                    (MarkerPlace::Head, [span]) => span.first().copied(),
                    (MarkerPlace::Bottom, [span]) => span.last().copied(),
                    _ => None,
                };
                match point {
                    Some(point) => out.placed.push(Placed { entry, key: key.clone(), title, size: marker.size_px, spot: Spot::Point(point) }),
                    None => unplaceable += 1,
                }
            } else {
                absent += 1;
            }
        }
        let plural = |n: usize, one: &str, many: &str| if n == 1 { format!("1 {one}") } else { format!("{n} {many}") };
        if absent > 0 {
            warnings.push(format!(
                "marker \"{name}\": {} not in this drawing (hidden, outside the portion, or neither a well nor a surface) — nothing marked there",
                plural(absent, "object is", "objects are")
            ));
        }
        if no_md > 0 {
            warnings.push(format!("marker \"{name}\": {} no measured depths — nothing marked there", plural(no_md, "well has", "wells have")));
        }
        if outside > 0 {
            warnings.push(format!(
                "marker \"{name}\": the measured depth is outside {} — nothing marked there",
                plural(outside, "well", "wells")
            ));
        }
        if unplaceable > 0 {
            warnings.push(format!(
                "marker \"{name}\": {} no such place (a surface has no measured depth, and its ends are one point only when drawn in one piece) — nothing marked there",
                plural(unplaceable, "object has", "objects have")
            ));
        }
        if out.placed.len() > before {
            out.entries.push(Entry { name: name.to_string(), shape: marker.shape.clone(), colour: marker.colour.clone() });
        }
    }
    if unnamed > 0 {
        warnings.push(format!("{unnamed} marker(s) without a name not drawn — a marker needs a name for the legend"));
    }
    out
}

/// A symbol as the drawer writes it: outlines (a ring, a cross, a plus) are
/// stroked in the marker's colour; solid shapes are filled with it and edged in
/// white, so that they stay visible over a well of their own colour.
fn symbol(at: [f64; 2], shape: &str, size: f64, colour: &str) -> Shape {
    let outline = matches!(shape, "circle" | "cross" | "plus");
    Shape::Symbol {
        at,
        kind: shape.to_string(),
        size,
        fill: if outline { "none".into() } else { colour.to_string() },
        stroke: if outline { colour.to_string() } else { "#ffffff".into() },
    }
}

impl Resolved {
    /// Nothing to draw, so nothing to list.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The legend's width, pixels — `0` when there is none.
    pub fn width(&self) -> f64 {
        if self.entries.is_empty() {
            return 0.0;
        }
        let widest = self
            .entries
            .iter()
            .map(|e| labels::width_px(&e.name, SMALL_PX))
            .fold(labels::width_px("Markers", FONT_PX), f64::max);
        (widest + 36.0).clamp(LEGEND_MIN_PX, LEGEND_MAX_PX)
    }

    /// The contours, one group per marked object — drawn **under** the lines.
    pub fn contours(&self, map: &Map) -> Vec<LaidObject> {
        self.placed
            .iter()
            .filter_map(|p| {
                let Spot::Line(lines, base) = &p.spot else { return None };
                let entry = &self.entries[p.entry];
                let shapes: Vec<Shape> = lines
                    .iter()
                    .map(|line| map.points(line))
                    .filter(|px| px.len() >= 2)
                    .map(|points| Shape::Polyline { points, stroke: entry.colour.clone(), width: base + CONTOUR_HALO_PX, dash: false })
                    .collect();
                (!shapes.is_empty()).then(|| marked(&entry.name, Some(&p.key), "marker-contour", &p.title, shapes))
            })
            .collect()
    }

    /// The symbols, one group per marked object, over the wells.
    pub fn symbols(&self, map: &Map) -> Vec<LaidObject> {
        self.placed
            .iter()
            .filter_map(|p| {
                let Spot::Point(at) = &p.spot else { return None };
                if !(at[0].is_finite() && at[1].is_finite()) {
                    return None;
                }
                let entry = &self.entries[p.entry];
                Some(marked(&entry.name, Some(&p.key), "marker", &p.title, vec![symbol(map.px(*at), &entry.shape, p.size, &entry.colour)]))
            })
            .collect()
    }

    /// The legend: a box at `x` from `top` down to at most `bottom`, a heading and
    /// one line per marker — symbol, name. What does not fit is said on its last line.
    pub fn legend(&self, x: f64, width: f64, top: f64, bottom: f64) -> Vec<LaidObject> {
        if self.entries.is_empty() {
            return Vec::new();
        }
        let room = (((bottom - top) - LINE_PX - 8.0) / LINE_PX).floor().max(1.0) as usize;
        let shown = if self.entries.len() > room { room.saturating_sub(1) } else { self.entries.len() };
        let height = LINE_PX * (shown + 1 + usize::from(shown < self.entries.len())) as f64 + 10.0;
        let mut frame = vec![
            Shape::Polygon {
                points: vec![[x, top], [x + width, top], [x + width, top + height], [x, top + height]],
                fill: "#ffffff".into(),
                opacity: 0.92,
                stroke: Some("#999999".into()),
                dash: false,
            },
            Shape::Text { at: [x + 8.0, top + LINE_PX], text: "Markers".into(), anchor: Anchor::Start, size: FONT_PX, fill: INK.into() },
        ];
        if shown < self.entries.len() {
            frame.push(Shape::Text {
                at: [x + 8.0, top + LINE_PX * (shown as f64 + 2.0)],
                text: format!("+ {} more markers", self.entries.len() - shown),
                anchor: Anchor::Start,
                size: SMALL_PX,
                fill: INK.into(),
            });
        }
        let mut out = vec![decor("markers-box", frame)];
        for (i, e) in self.entries.iter().take(shown).enumerate() {
            let y = top + LINE_PX * (i as f64 + 2.0);
            let sample = if e.shape == CONTOUR {
                Shape::Polyline { points: vec![[x + 7.0, y - 4.0], [x + 21.0, y - 4.0]], stroke: e.colour.clone(), width: 5.0, dash: false }
            } else {
                symbol([x + 14.0, y - 4.0], &e.shape, LEGEND_SYMBOL_PX, &e.colour)
            };
            let text = Shape::Text {
                at: [x + 28.0, y],
                text: labels::fit(&e.name, SMALL_PX, (width - 28.0 - 6.0).max(20.0)),
                anchor: Anchor::Start,
                size: SMALL_PX,
                fill: INK.into(),
            };
            out.push(marked(&e.name, None, "markers-entry", &e.name, vec![sample, text]));
        }
        out
    }
}

fn marked(name: &str, key: Option<&str>, kind: &str, title: &str, shapes: Vec<Shape>) -> LaidObject {
    LaidObject {
        attr: Attr::Marker { name: name.to_string(), key: key.map(str::to_string) },
        kind: kind.to_string(),
        title: title.to_string(),
        highlighted: false,
        at: None,
        shapes,
        intervals: Vec::new(),
    }
}

fn decor(name: &str, shapes: Vec<Shape>) -> LaidObject {
    LaidObject {
        attr: Attr::Decor(name.into()),
        kind: name.into(),
        title: String::new(),
        highlighted: false,
        at: None,
        shapes,
        intervals: Vec::new(),
    }
}

/// The bottom edge of a legend's box — what the legend below it starts under.
pub(super) fn bottom_of(boxed: &LaidObject) -> Option<f64> {
    match boxed.shapes.first() {
        Some(Shape::Polygon { points, .. }) => points.iter().map(|p| p[1]).reduce(f64::max),
        _ => None,
    }
}
