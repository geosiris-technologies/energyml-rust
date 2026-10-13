//! The intersection of two triangulated surfaces - a fault against a horizon,
//! as polylines.
//!
//! The need: a fault is often a **surface** cutting
//! several horizons, and neither a barrier nor a constrained triangulation can
//! take a dipping surface directly - both are rules on a plan-view *line*,
//! and the line depends on which horizon one stands on. The intersection is
//! that line. Run once against every horizon of a stratigraphic unit set, it
//! gives one trace per horizon, which is exactly the "horizontal polyline at
//! the horizon's height" the other RESQML convention writes by hand.
//!
//! # How a segment is found, and why its ends are exact
//!
//! Two triangles meet, when they do, along a piece of the line where their
//! planes cross. The piece is the **overlap** of two segments on that line:
//! `T1` cut by the plane of `T2`, and `T2` cut by the plane of `T1`. Each cut
//! end lies on an edge of one triangle and is computed from that edge's two
//! corners - **taken in a fixed order** - and the other triangle's plane. Two
//! neighbouring triangles sharing that edge therefore compute the same end,
//! bit for bit, which is what lets [`chain`] weld on exact equality with no
//! threshold: the rule of `energyml_geometry::section`, kept as it stands.
//!
//! # What it does not do
//!
//! It does not decide which side is the hanging wall, does not measure a
//! throw, and does not join two traces across a gap - a horizon torn by the
//! fault yields two separate traces, one per lip, and they stay two. That is
//! left to a later operation, which starts from these lines.

use std::collections::HashMap;

/// One intersection segment, in the frame of the input points.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub ends: [[f64; 3]; 2],
}

/// A chained polyline of intersection, in walking order.
#[derive(Clone, Debug, PartialEq)]
pub struct Trace {
    pub points: Vec<[f64; 3]>,
    /// The two ends meet: a ring.
    pub closed: bool,
}

/// What an intersection answers.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Intersection {
    pub traces: Vec<Trace>,
    /// Triangles dropped for a non-finite corner, on either side.
    pub dropped: usize,
}

type Point = [f64; 3];

/// A triangle of B as the grid holds it: corners, indices, and its box.
type Boxed = ([Point; 3], [u32; 3], [f64; 3], [f64; 3]);

/// A triangle's plane: unit normal and offset, or `None` if degenerate.
fn plane(t: &[Point; 3]) -> Option<([f64; 3], f64)> {
    let u = [t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]];
    let v = [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]];
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len < 1e-18 {
        return None;
    }
    let n = [n[0] / len, n[1] / len, n[2] / len];
    Some((n, -(n[0] * t[0][0] + n[1] * t[0][1] + n[2] * t[0][2])))
}

fn signed(n: &[f64; 3], d: f64, p: &Point) -> f64 {
    n[0] * p[0] + n[1] * p[1] + n[2] * p[2] + d
}

/// Where a triangle's edges cross a plane - zero, one (a touch) or two points.
///
/// The corners of each edge are taken in **index order** so that the same edge
/// seen from the neighbouring triangle yields the same bits.
fn cut_by(corners: &[Point; 3], index: &[u32; 3], n: &[f64; 3], d: f64) -> Vec<Point> {
    let side: Vec<f64> = corners.iter().map(|c| signed(n, d, c)).collect();
    let mut hits: Vec<Point> = Vec::with_capacity(2);
    for (a, b) in [(0usize, 1usize), (1, 2), (2, 0)] {
        let (a, b) = if index[a] <= index[b] { (a, b) } else { (b, a) };
        let (da, db) = (side[a], side[b]);
        if (da > 0.0 && db > 0.0) || (da < 0.0 && db < 0.0) {
            continue;
        }
        let span = da - db;
        if span.abs() < 1e-300 {
            continue;
        }
        let t = da / span;
        let hit = [
            corners[a][0] + (corners[b][0] - corners[a][0]) * t,
            corners[a][1] + (corners[b][1] - corners[a][1]) * t,
            corners[a][2] + (corners[b][2] - corners[a][2]) * t,
        ];
        if !hits.iter().any(|seen| seen == &hit) {
            hits.push(hit);
        }
    }
    hits
}

/// The intersection segment of two triangles, if any.
fn meet(t1: &[Point; 3], i1: &[u32; 3], t2: &[Point; 3], i2: &[u32; 3]) -> Option<Piece> {
    let (n1, d1) = plane(t1)?;
    let (n2, d2) = plane(t2)?;
    let s1 = cut_by(t1, i1, &n2, d2);
    let s2 = cut_by(t2, i2, &n1, d1);
    if s1.len() != 2 || s2.len() != 2 {
        return None;
    }
    // Both segments lie on the same line: order them along its direction and
    // keep the overlap. The direction is the cross product of the normals.
    let dir = [
        n1[1] * n2[2] - n1[2] * n2[1],
        n1[2] * n2[0] - n1[0] * n2[2],
        n1[0] * n2[1] - n1[1] * n2[0],
    ];
    let along = |p: &Point| p[0] * dir[0] + p[1] * dir[1] + p[2] * dir[2];
    let order = |s: &[Point]| -> (Point, Point, f64, f64) {
        let (a, b) = (s[0], s[1]);
        let (ta, tb) = (along(&a), along(&b));
        if ta <= tb { (a, b, ta, tb) } else { (b, a, tb, ta) }
    };
    let (a1, b1, ta1, tb1) = order(&s1);
    let (a2, b2, ta2, tb2) = order(&s2);
    let (start, ts) = if ta1 >= ta2 { (a1, ta1) } else { (a2, ta2) };
    let (end, te) = if tb1 <= tb2 { (b1, tb1) } else { (b2, tb2) };
    if te - ts <= 0.0 {
        return None;
    }
    Some(Piece { ends: [start, end] })
}

/// The axis-aligned box of a triangle.
fn aabb(t: &[Point; 3]) -> ([f64; 3], [f64; 3]) {
    let mut lo = t[0];
    let mut hi = t[0];
    for c in &t[1..] {
        for k in 0..3 {
            lo[k] = lo[k].min(c[k]);
            hi[k] = hi[k].max(c[k]);
        }
    }
    (lo, hi)
}

/// Every intersection segment between two surfaces, in no particular order.
///
/// A uniform grid over the second surface keeps this from being every
/// triangle against every triangle - a horizon is a hundred thousand
/// triangles, and the product of two of them is not a number to iterate.
pub fn pieces(
    points_a: &[Point],
    triangles_a: &[[u32; 3]],
    points_b: &[Point],
    triangles_b: &[[u32; 3]],
) -> (Vec<Piece>, usize) {
    let corners = |points: &[Point], t: &[u32; 3]| -> Option<[Point; 3]> {
        let c = [
            *points.get(t[0] as usize)?,
            *points.get(t[1] as usize)?,
            *points.get(t[2] as usize)?,
        ];
        if c.iter().any(|p| p.iter().any(|v| !v.is_finite())) {
            None
        } else {
            Some(c)
        }
    };
    let mut dropped = 0usize;
    // Bucket B by AABB into a grid sized by B's own extent and count.
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut boxed_b: Vec<Boxed> = Vec::with_capacity(triangles_b.len());
    for t in triangles_b {
        let Some(c) = corners(points_b, t) else {
            dropped += 1;
            continue;
        };
        let (l, h) = aabb(&c);
        for k in 0..3 {
            lo[k] = lo[k].min(l[k]);
            hi[k] = hi[k].max(h[k]);
        }
        boxed_b.push((c, *t, l, h));
    }
    if boxed_b.is_empty() {
        return (Vec::new(), dropped);
    }
    let per_axis = ((boxed_b.len() as f64).cbrt().ceil() as usize).clamp(1, 256);
    let size = [0, 1, 2].map(|k| ((hi[k] - lo[k]) / per_axis as f64).max(1e-9));
    let cell =
        |v: f64, k: usize| (((v - lo[k]) / size[k]).floor() as i64).clamp(0, per_axis as i64 - 1) as usize;
    let mut grid: HashMap<(usize, usize, usize), Vec<usize>> = HashMap::new();
    for (at, (_, _, l, h)) in boxed_b.iter().enumerate() {
        for x in cell(l[0], 0)..=cell(h[0], 0) {
            for y in cell(l[1], 1)..=cell(h[1], 1) {
                for z in cell(l[2], 2)..=cell(h[2], 2) {
                    grid.entry((x, y, z)).or_default().push(at);
                }
            }
        }
    }

    let mut out = Vec::new();
    let mut seen: Vec<usize> = Vec::new();
    for t in triangles_a {
        let Some(ca) = corners(points_a, t) else {
            dropped += 1;
            continue;
        };
        let (l, h) = aabb(&ca);
        if (0..3).any(|k| h[k] < lo[k] || l[k] > hi[k]) {
            continue;
        }
        seen.clear();
        for x in cell(l[0], 0)..=cell(h[0], 0) {
            for y in cell(l[1], 1)..=cell(h[1], 1) {
                for z in cell(l[2], 2)..=cell(h[2], 2) {
                    if let Some(list) = grid.get(&(x, y, z)) {
                        seen.extend_from_slice(list);
                    }
                }
            }
        }
        seen.sort_unstable();
        seen.dedup();
        for &at in &seen {
            let (cb, ib, lb, hb) = &boxed_b[at];
            if (0..3).any(|k| h[k] < lb[k] || l[k] > hb[k]) {
                continue;
            }
            if let Some(piece) = meet(&ca, t, cb, ib) {
                out.push(piece);
            }
        }
    }
    (out, dropped)
}

/// Weld pieces into traces on **exactly** shared ends - the rule of
/// `energyml_geometry::section::chain`, without the profile.
pub fn chain(pieces: &[Piece]) -> Vec<Trace> {
    let key = |p: &Point| -> [u64; 3] { [p[0].to_bits(), p[1].to_bits(), p[2].to_bits()] };
    let mut by_end: HashMap<[u64; 3], Vec<usize>> = HashMap::new();
    for (index, one) in pieces.iter().enumerate() {
        for end in &one.ends {
            by_end.entry(key(end)).or_default().push(index);
        }
    }
    let mut used = vec![false; pieces.len()];
    let mut out = Vec::new();
    for start in 0..pieces.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let mut points = vec![pieces[start].ends[0], pieces[start].ends[1]];
        for forward in [true, false] {
            loop {
                let tip = if forward {
                    *points.last().unwrap()
                } else {
                    points[0]
                };
                let at_tip = by_end.get(&key(&tip)).map(Vec::as_slice).unwrap_or(&[]);
                // Degree two or nothing: a junction is a stop.
                if at_tip.len() != 2 {
                    break;
                }
                let Some(&next) = at_tip.iter().find(|&&i| !used[i]) else {
                    break;
                };
                used[next] = true;
                let [a, b] = pieces[next].ends;
                let extend = if key(&a) == key(&tip) { b } else { a };
                if forward {
                    points.push(extend)
                } else {
                    points.insert(0, extend)
                }
            }
        }
        let closed = points.len() > 2 && key(points.first().unwrap()) == key(points.last().unwrap());
        if closed {
            points.pop();
        }
        out.push(Trace { points, closed });
    }
    out
}

/// The traces where surface A meets surface B.
pub fn intersect(
    points_a: &[Point],
    triangles_a: &[[u32; 3]],
    points_b: &[Point],
    triangles_b: &[[u32; 3]],
) -> Intersection {
    let (found, dropped) = pieces(points_a, triangles_a, points_b, triangles_b);
    Intersection {
        traces: chain(&found),
        dropped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A horizontal square at height `z`, split in two triangles.
    fn plate(z: f64) -> (Vec<Point>, Vec<[u32; 3]>) {
        (
            vec![[0.0, 0.0, z], [10.0, 0.0, z], [10.0, 10.0, z], [0.0, 10.0, z]],
            vec![[0, 1, 2], [0, 2, 3]],
        )
    }

    /// A vertical wall at x = 4, from z = -5 to 15, along y.
    fn wall() -> (Vec<Point>, Vec<[u32; 3]>) {
        (
            vec![
                [4.0, -1.0, -5.0],
                [4.0, 11.0, -5.0],
                [4.0, 11.0, 15.0],
                [4.0, -1.0, 15.0],
            ],
            vec![[0, 1, 2], [0, 2, 3]],
        )
    }

    /// A gently dipping horizon: z = 10 + 0.5·x, as a 4×4 grid of triangles.
    fn ramp() -> (Vec<Point>, Vec<[u32; 3]>) {
        let mut points = Vec::new();
        for j in 0..=4 {
            for i in 0..=4 {
                let (x, y) = (i as f64 * 2.5, j as f64 * 2.5);
                points.push([x, y, 10.0 + 0.5 * x]);
            }
        }
        let mut triangles = Vec::new();
        for j in 0..4u32 {
            for i in 0..4u32 {
                let a = j * 5 + i;
                triangles.push([a, a + 1, a + 6]);
                triangles.push([a, a + 6, a + 5]);
            }
        }
        (points, triangles)
    }

    #[test]
    fn a_wall_through_a_plate_gives_one_straight_trace_at_the_plate_height() {
        let (pa, ta) = wall();
        let (pb, tb) = plate(7.0);
        let made = intersect(&pa, &ta, &pb, &tb);
        assert_eq!(made.traces.len(), 1, "{:?}", made.traces);
        let trace = &made.traces[0];
        assert!(!trace.closed);
        for p in &trace.points {
            assert!((p[0] - 4.0).abs() < 1e-9 && (p[2] - 7.0).abs() < 1e-9, "{p:?}");
        }
        let ys: Vec<f64> = trace.points.iter().map(|p| p[1]).collect();
        assert!((ys.iter().cloned().fold(f64::INFINITY, f64::min)).abs() < 1e-9);
        assert!((ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - 10.0).abs() < 1e-9);
    }

    /// **The weld is exact across many triangles.** A wall through a gridded
    /// horizon crosses eight triangles; the pieces must come back as one trace,
    /// not eight - which only holds if neighbouring triangles compute their
    /// shared end bit for bit.
    #[test]
    fn a_gridded_horizon_welds_into_a_single_trace() {
        let (pa, ta) = wall();
        let (pb, tb) = ramp();
        let (found, _) = pieces(&pa, &ta, &pb, &tb);
        assert!(found.len() >= 4, "{} pieces", found.len());
        let traces = chain(&found);
        assert_eq!(traces.len(), 1, "{} traces", traces.len());
        // And the trace sits on the ramp: z = 10 + 0.5·4 = 12 everywhere.
        for p in &traces[0].points {
            assert!((p[2] - 12.0).abs() < 1e-9, "{p:?}");
        }
    }

    #[test]
    fn two_surfaces_that_do_not_meet_give_nothing_and_a_nan_corner_is_dropped() {
        let (pa, ta) = wall();
        let (pb, tb) = plate(40.0);
        assert!(intersect(&pa, &ta, &pb, &tb).traces.is_empty());
        let (mut pc, tc) = plate(7.0);
        pc[0][2] = f64::NAN;
        let made = intersect(&pa, &ta, &pc, &tc);
        assert_eq!(made.dropped, 2, "both triangles share the NaN corner");
    }

    #[test]
    fn a_wall_that_stops_inside_the_plate_gives_a_trace_that_stops_there() {
        // The wall only spans y in [2, 6].
        let pa = vec![
            [4.0, 2.0, -5.0],
            [4.0, 6.0, -5.0],
            [4.0, 6.0, 15.0],
            [4.0, 2.0, 15.0],
        ];
        let ta = vec![[0, 1, 2], [0, 2, 3]];
        let (pb, tb) = plate(7.0);
        let made = intersect(&pa, &ta, &pb, &tb);
        assert_eq!(made.traces.len(), 1);
        let ys: Vec<f64> = made.traces[0].points.iter().map(|p| p[1]).collect();
        assert!(ys.iter().all(|y| *y >= 2.0 - 1e-9 && *y <= 6.0 + 1e-9), "{ys:?}");
    }
}
