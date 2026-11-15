//! What is under a point: the element a face finds by pointing, so that no
//! face redoes the geometry.
//!
//! Layers are tried from the last to the first, elements from the last to the
//! first, items from the last to the first: what is drawn last is on top. An
//! element without a key is a decoration: it is never hit and hides nothing.
//! Reading the data under the point (which depth, which value) stays with each
//! view; [`hit`] gives the key and, on an image, the pixel.

use crate::draw::list::{Anchor, DisplayList, Item, Rect, Stroke};
use crate::draw::text::{ASCENT_PER_EM, DESCENT_PER_EM, string_width_pt};
use energyml_geostat::vector::plan::{distance, distance_to_segment};

/// How far from a line, in user units, a point still hits it.
pub const HIT_SLOP: f64 = 3.0;

/// The element under a point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit<'a> {
    /// Index of the layer in `DisplayList::layers`.
    pub layer: usize,
    /// Index of the element in its layer.
    pub element: usize,
    /// The key of the element hit (`Element::key`).
    pub key: &'a str,
    /// On an `Item::Image`: the raster's column and row under the point.
    pub pixel: Option<[u32; 2]>,
}

/// The keyed element under `(x, y)`, the topmost one; `None` if there is none
/// or if a coordinate is not finite.
pub fn hit(list: &DisplayList, x: f64, y: f64) -> Option<Hit<'_>> {
    if !(x.is_finite() && y.is_finite()) {
        return None;
    }
    let p = [x, y];
    for (layer_index, layer) in list.layers.iter().enumerate().rev() {
        if let Some(clip) = &layer.clip
            && !clip.contains(x, y)
        {
            continue;
        }
        for (element_index, element) in layer.elements.iter().enumerate().rev() {
            let Some(key) = element.key.as_deref() else {
                continue;
            };
            for item in element.items.iter().rev() {
                if let Some(pixel) = item_hit(item, p) {
                    return Some(Hit {
                        layer: layer_index,
                        element: element_index,
                        key,
                        pixel,
                    });
                }
            }
        }
    }
    None
}

/// `Some(pixel)` if the item is hit (the pixel only for an image).
fn item_hit(item: &Item, p: [f64; 2]) -> Option<Option<[u32; 2]>> {
    let near = |distance: f64, stroke: &Option<Stroke>, extra: f64| {
        distance <= stroke.as_ref().map_or(0.0, |s| s.width) / 2.0 + extra
    };
    let hit = match item {
        Item::Polyline { points, stroke } => distance_to_polyline(p, points) <= stroke.width / 2.0 + HIT_SLOP,
        Item::Path { path, fill, stroke } => {
            let subpaths = path.flatten();
            let filled = fill.is_some() && winding_sum(p, &subpaths) != 0;
            let stroked = subpaths.iter().any(|line| {
                near(
                    distance_to_polyline(p, line),
                    stroke,
                    if fill.is_some() { 0.0 } else { HIT_SLOP },
                )
            }) && (stroke.is_some() || fill.is_none());
            filled || stroked
        }
        Item::Polygon { points, fill, stroke } => {
            if fill.is_some() {
                winding(p, points) != 0
                    || (stroke.is_some() && near(distance_to_ring(p, points), stroke, 0.0))
            } else {
                near(distance_to_ring(p, points), stroke, HIT_SLOP)
            }
        }
        Item::Rect {
            rect, fill, stroke, ..
        } => {
            let ring = rect_ring(rect);
            // A rectangle with neither fill nor stroke draws nothing: it can
            // only be a hit area (a row, a column), so its whole interior
            // hits, not its outline (`[views-sheet C1]`). One with a stroke
            // and no fill is an outline and stays one.
            if fill.is_some() || stroke.is_none() {
                rect.contains(p[0], p[1])
                    || (stroke.is_some() && near(distance_to_ring(p, &ring), stroke, 0.0))
            } else {
                near(distance_to_ring(p, &ring), stroke, HIT_SLOP)
            }
        }
        Item::Circle {
            centre,
            radius,
            fill,
            stroke,
        } => {
            let d = distance(p, *centre);
            if fill.is_some() {
                d <= *radius || (stroke.is_some() && near((d - radius).abs(), stroke, 0.0))
            } else {
                near((d - radius).abs(), stroke, HIT_SLOP)
            }
        }
        Item::Sector {
            centre,
            inner,
            outer,
            start,
            end,
            ..
        } => {
            let dx = p[0] - centre[0];
            let dy = p[1] - centre[1];
            let r = dx.hypot(dy);
            if r < inner - HIT_SLOP || r > outer + HIT_SLOP {
                false
            } else if end - start >= std::f64::consts::TAU {
                true
            } else {
                let angle = dy.atan2(dx);
                (angle - start).rem_euclid(std::f64::consts::TAU) <= end - start
            }
        }
        Item::Symbol { at, size, .. } => {
            let half = size / 2.0 + HIT_SLOP;
            (p[0] - at[0]).abs() <= half && (p[1] - at[1]).abs() <= half
        }
        Item::Text { at, text, style } => {
            // Bring the point into the text's own frame: rotate by -rotate about `at`.
            let theta = (-style.rotate).to_radians();
            let (sin, cos) = theta.sin_cos();
            let (dx, dy) = (p[0] - at[0], p[1] - at[1]);
            let local = [at[0] + dx * cos - dy * sin, at[1] + dx * sin + dy * cos];
            let width = string_width_pt(text, style.size);
            let left = match style.anchor {
                Anchor::Start => at[0],
                Anchor::Middle => at[0] - width / 2.0,
                Anchor::End => at[0] - width,
            };
            local[0] >= left
                && local[0] <= left + width
                && local[1] >= at[1] - ASCENT_PER_EM * style.size
                && local[1] <= at[1] + DESCENT_PER_EM * style.size
        }
        Item::Image { rect, raster } => {
            if rect.contains(p[0], p[1]) {
                let (w, h) = (raster.width(), raster.height());
                let column = (((p[0] - rect.x) / rect.w * f64::from(w)).floor().max(0.0) as u32)
                    .min(w.saturating_sub(1));
                let row = (((p[1] - rect.y) / rect.h * f64::from(h)).floor().max(0.0) as u32)
                    .min(h.saturating_sub(1));
                return Some(Some([column, row]));
            }
            false
        }
    };
    if hit { Some(None) } else { None }
}

fn rect_ring(rect: &Rect) -> Vec<[f64; 2]> {
    vec![
        [rect.x, rect.y],
        [rect.x + rect.w, rect.y],
        [rect.x + rect.w, rect.y + rect.h],
        [rect.x, rect.y + rect.h],
    ]
}

fn distance_to_polyline(p: [f64; 2], points: &[[f64; 2]]) -> f64 {
    match points {
        [] => f64::INFINITY,
        [only] => distance(p, *only),
        _ => points
            .windows(2)
            .map(|w| distance_to_segment(p, w[0], w[1]))
            .fold(f64::INFINITY, f64::min),
    }
}

/// The distance to a closed outline (the last point joins the first).
fn distance_to_ring(p: [f64; 2], points: &[[f64; 2]]) -> f64 {
    let mut best = distance_to_polyline(p, points);
    if points.len() > 2 {
        best = best.min(distance_to_segment(p, points[points.len() - 1], points[0]));
    }
    best
}

/// The winding number of a closed outline around `p` (non-zero rule).
fn winding(p: [f64; 2], points: &[[f64; 2]]) -> i32 {
    if points.len() < 3 {
        return 0;
    }
    let mut total = 0;
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        let cross = (b[0] - a[0]) * (p[1] - a[1]) - (p[0] - a[0]) * (b[1] - a[1]);
        if a[1] <= p[1] {
            if b[1] > p[1] && cross > 0.0 {
                total += 1;
            }
        } else if b[1] <= p[1] && cross < 0.0 {
            total -= 1;
        }
    }
    total
}

/// The winding numbers of every subpath, summed (each subpath taken as closed).
fn winding_sum(p: [f64; 2], subpaths: &[Vec<[f64; 2]>]) -> i32 {
    subpaths.iter().map(|line| winding(p, line)).sum()
}
