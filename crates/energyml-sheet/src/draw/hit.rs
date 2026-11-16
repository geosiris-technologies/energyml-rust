//! What is under a point: the element a face finds by pointing, so that no
//! face redoes the geometry.
//!
//! Layers are tried from the last to the first, elements from the last to the
//! first, items from the last to the first: what is drawn last is on top. An
//! element without a key is a decoration: it is never hit and hides nothing.
//! Reading the data under the point (which depth, which value) stays with each
//! view; [`hit`] gives the key and, on an image, the pixel.
//!
//! [`hit_within`] is the pointer's door (a grab distance: a line, an outline, a disc
//! or a text held within it beats a fill under the point); [`outline`] gives the
//! path of an element, to highlight it over the picture.

use crate::draw::list::{Anchor, DisplayList, Item, PathData, Rect, Segment, Stroke, TextStyle};
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

/// What an item holds of a point: whether it is held by a line-like shape (a
/// line, an outline, a disc, a text, a symbol) or only by a fill, how far the
/// point is, and the pixel on an image.
#[derive(Clone, Copy)]
struct Held {
    precise: bool,
    distance: f64,
    pixel: Option<[u32; 2]>,
}

fn precise(distance: f64) -> Option<Held> {
    Some(Held {
        precise: true,
        distance,
        pixel: None,
    })
}

fn fill_held() -> Option<Held> {
    Some(Held {
        precise: false,
        distance: 0.0,
        pixel: None,
    })
}

/// A shape with an outline and an inside: the outline held within its half
/// width plus `grab` is precise, the inside (when it is drawn filled, or not
/// drawn at all, a hit area) is a fill.
fn ring_held(
    ring_distance: f64,
    inside: bool,
    has_fill: bool,
    stroke: &Option<Stroke>,
    grab: f64,
) -> Option<Held> {
    if let Some(s) = stroke
        && ring_distance <= s.width / 2.0 + grab
    {
        return precise(ring_distance);
    }
    if inside && (has_fill || stroke.is_none()) {
        return fill_held();
    }
    None
}

/// `precise(distance)` when `distance <= limit`.
fn within(distance: f64, limit: f64) -> Option<Held> {
    if distance <= limit {
        precise(distance)
    } else {
        None
    }
}

fn item_held(item: &Item, p: [f64; 2], grab: f64) -> Option<Held> {
    match item {
        Item::Polyline { points, stroke } => {
            within(distance_to_polyline(p, points), stroke.width / 2.0 + grab)
        }
        Item::Path { path, fill, stroke } => {
            let subpaths = path.flatten();
            let half = stroke.as_ref().map_or(0.0, |s| s.width / 2.0);
            let d = subpaths
                .iter()
                .map(|line| distance_to_polyline(p, line))
                .fold(f64::INFINITY, f64::min);
            if (stroke.is_some() || fill.is_none()) && d <= half + grab {
                precise(d)
            } else if fill.is_some() && winding_sum(p, &subpaths) != 0 {
                fill_held()
            } else {
                None
            }
        }
        Item::Polygon { points, fill, stroke } => ring_held(
            distance_to_ring(p, points),
            winding(p, points) != 0,
            fill.is_some(),
            stroke,
            grab,
        ),
        Item::Rect {
            rect, fill, stroke, ..
        } => ring_held(
            distance_to_ring(p, &rect_ring(rect)),
            rect.contains(p[0], p[1]),
            fill.is_some(),
            stroke,
            grab,
        ),
        Item::Circle {
            centre,
            radius,
            fill,
            stroke,
        } => {
            let d = distance(p, *centre);
            if fill.is_some() {
                within((d - radius).max(0.0), grab)
            } else {
                let half = stroke.as_ref().map_or(0.0, |s| s.width / 2.0);
                within((d - radius).abs(), half + grab)
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
            let inside = r >= inner - grab
                && r <= outer + grab
                && (end - start >= std::f64::consts::TAU
                    || (dy.atan2(dx) - start).rem_euclid(std::f64::consts::TAU) <= end - start);
            if inside { fill_held() } else { None }
        }
        Item::Symbol { at, size, .. } => {
            let half = size / 2.0;
            let dx = ((p[0] - at[0]).abs() - half).max(0.0);
            let dy = ((p[1] - at[1]).abs() - half).max(0.0);
            within(dx.hypot(dy), grab)
        }
        Item::Text { at, text, style } => {
            let theta = (-style.rotate).to_radians();
            let (sin, cos) = theta.sin_cos();
            let (dx, dy) = (p[0] - at[0], p[1] - at[1]);
            let local = [at[0] + dx * cos - dy * sin, at[1] + dx * sin + dy * cos];
            let [left, top, right, bottom] = text_box(*at, text, style);
            let ox = (left - local[0]).max(local[0] - right).max(0.0);
            let oy = (top - local[1]).max(local[1] - bottom).max(0.0);
            within(ox.hypot(oy), grab)
        }
        Item::Image { rect, raster } => {
            if rect.contains(p[0], p[1]) {
                let (w, h) = (raster.width(), raster.height());
                let column = (((p[0] - rect.x) / rect.w * f64::from(w)).floor().max(0.0) as u32)
                    .min(w.saturating_sub(1));
                let row = (((p[1] - rect.y) / rect.h * f64::from(h)).floor().max(0.0) as u32)
                    .min(h.saturating_sub(1));
                Some(Held {
                    precise: false,
                    distance: 0.0,
                    pixel: Some([column, row]),
                })
            } else {
                None
            }
        }
    }
}

/// The box of an unrotated text, `[left, top, right, bottom]`, measured by `draw::text`.
fn text_box(at: [f64; 2], text: &str, style: &TextStyle) -> [f64; 4] {
    let width = string_width_pt(text, style.size);
    let left = match style.anchor {
        Anchor::Start => at[0],
        Anchor::Middle => at[0] - width / 2.0,
        Anchor::End => at[0] - width,
    };
    [
        left,
        at[1] - ASCENT_PER_EM * style.size,
        left + width,
        at[1] + DESCENT_PER_EM * style.size,
    ]
}

/// The keyed element a pointer holds at `(x, y)`, with a grab distance `grab`
/// in units of the page (pixels for a page in pixels). The rule, in order:
///
/// 1. a line, an outline, a disc, a symbol or a text held within `grab` beats
///    a fill that contains the point;
/// 2. then the element in the highest layer;
/// 3. then the nearest (a fill that contains the point is at distance 0);
/// 4. then the one drawn last.
///
/// `None` if nothing is held, if a coordinate is not finite, or if the point is
/// outside the clip of a layer (that layer holds nothing). A `grab` that is
/// negative or not finite counts as 0. [`hit`] stays apart: it keeps the plain
/// order of drawing and its own slop.
pub fn hit_within(list: &DisplayList, x: f64, y: f64, grab: f64) -> Option<Hit<'_>> {
    if !(x.is_finite() && y.is_finite()) {
        return None;
    }
    let grab = if grab.is_finite() && grab > 0.0 { grab } else { 0.0 };
    let p = [x, y];
    // The best so far: its hit, whether it is precise, its distance, its item.
    let mut best: Option<(Hit<'_>, bool, f64, usize)> = None;
    for (layer_index, layer) in list.layers.iter().enumerate() {
        if let Some(clip) = &layer.clip
            && !clip.contains(x, y)
        {
            continue;
        }
        for (element_index, element) in layer.elements.iter().enumerate() {
            let Some(key) = element.key.as_deref() else {
                continue;
            };
            for (item_index, item) in element.items.iter().enumerate() {
                let Some(held) = item_held(item, p, grab) else {
                    continue;
                };
                let better = match &best {
                    None => true,
                    Some((current, was_precise, was_distance, was_item)) => {
                        if held.precise != *was_precise {
                            held.precise
                        } else if layer_index != current.layer {
                            layer_index > current.layer
                        } else if (held.distance - was_distance).abs() > 1e-9 {
                            held.distance < *was_distance
                        } else {
                            (element_index, item_index) >= (current.element, *was_item)
                        }
                    }
                };
                if better {
                    best = Some((
                        Hit {
                            layer: layer_index,
                            element: element_index,
                            key,
                            pixel: held.pixel,
                        },
                        held.precise,
                        held.distance,
                        item_index,
                    ));
                }
            }
        }
    }
    best.map(|(found, ..)| found)
}

/// Segments per full circle in an outline.
const OUTLINE_STEPS: usize = 64;

fn push_closed(points: &[[f64; 2]], path: &mut PathData) {
    for (i, p) in points.iter().enumerate() {
        path.segments.push(if i == 0 {
            Segment::MoveTo(*p)
        } else {
            Segment::LineTo(*p)
        });
    }
    if !points.is_empty() {
        path.segments.push(Segment::Close);
    }
}

fn arc(centre: [f64; 2], radius: f64, start: f64, end: f64) -> Vec<[f64; 2]> {
    let steps = (((end - start).abs() / std::f64::consts::TAU * OUTLINE_STEPS as f64).ceil() as usize).max(1);
    (0..=steps)
        .map(|k| {
            let a = start + (end - start) * k as f64 / steps as f64;
            [centre[0] + radius * a.cos(), centre[1] + radius * a.sin()]
        })
        .collect()
}

/// A full circle as a closed ring of [`OUTLINE_STEPS`] corners.
fn full_ring(centre: [f64; 2], radius: f64) -> Vec<[f64; 2]> {
    let mut ring = arc(centre, radius, 0.0, std::f64::consts::TAU);
    ring.pop();
    ring
}

/// The outline of the element `element` of layer `layer`, as SVG path commands
/// (`M`, `L`, `Z`) in units of the page: a rectangle, a polyline, a polygon, a
/// path (flattened), a disc (in segments, each corner at the radius), a sector,
/// a symbol (its square), an image (its rectangle), a text (its measured box,
/// rotated like the text). A face strokes it over the picture to highlight the
/// element. `None` for an index out of range, or an element with nothing to
/// outline.
pub fn outline(list: &DisplayList, layer: usize, element: usize) -> Option<String> {
    let element = list.layers.get(layer)?.elements.get(element)?;
    let mut path = PathData::default();
    for item in &element.items {
        match item {
            Item::Polyline { points, .. } => {
                if points.len() >= 2 {
                    path.segments.extend(PathData::polyline(points).segments);
                }
            }
            Item::Path { path: data, .. } => {
                for line in data.flatten() {
                    if line.len() >= 2 {
                        path.segments.extend(PathData::polyline(&line).segments);
                    }
                }
            }
            Item::Polygon { points, .. } => {
                if points.len() >= 3 {
                    push_closed(points, &mut path);
                }
            }
            Item::Rect { rect, .. } | Item::Image { rect, .. } => push_closed(&rect_ring(rect), &mut path),
            Item::Circle { centre, radius, .. } => push_closed(&full_ring(*centre, *radius), &mut path),
            Item::Sector {
                centre,
                inner,
                outer,
                start,
                end,
                ..
            } => {
                if end - start >= std::f64::consts::TAU {
                    push_closed(&full_ring(*centre, *outer), &mut path);
                    if *inner > 0.0 {
                        push_closed(&full_ring(*centre, *inner), &mut path);
                    }
                } else {
                    let mut ring = arc(*centre, *outer, *start, *end);
                    let mut back = arc(*centre, *inner, *start, *end);
                    back.reverse();
                    ring.extend(back);
                    push_closed(&ring, &mut path);
                }
            }
            Item::Symbol { at, size, .. } => {
                let h = size / 2.0;
                push_closed(
                    &[
                        [at[0] - h, at[1] - h],
                        [at[0] + h, at[1] - h],
                        [at[0] + h, at[1] + h],
                        [at[0] - h, at[1] + h],
                    ],
                    &mut path,
                );
            }
            Item::Text { at, text, style } => {
                let [l, t, r, b] = text_box(*at, text, style);
                let (sin, cos) = style.rotate.to_radians().sin_cos();
                let corners: Vec<[f64; 2]> = [[l, t], [r, t], [r, b], [l, b]]
                    .iter()
                    .map(|c| {
                        let (dx, dy) = (c[0] - at[0], c[1] - at[1]);
                        [at[0] + dx * cos - dy * sin, at[1] + dx * sin + dy * cos]
                    })
                    .collect();
                push_closed(&corners, &mut path);
            }
        }
    }
    if path.segments.is_empty() {
        None
    } else {
        Some(path.to_commands(2))
    }
}
