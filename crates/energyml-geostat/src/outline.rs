//! The outline of a shape: the boundary of a triangulated surface, or the
//! convex hull of whatever else.
//!
//! The need: the borders of the starting surface are not marked; build a polyline
//! (simplifying points when they are collinear) that outlines the object. An
//! operation of its own, **and** an option of the contouring that composes
//! with it. This module is the computation; both doors call it.
//!
//! # Two answers, and each says which it is
//!
//! - a **triangulated surface** has an exact boundary: the edges that belong
//!   to one triangle only, chained into closed loops. A surface with a hole
//!   has two loops, and both come out - which is the outer one is a
//!   question of area, answered by the caller if it cares;
//! - anything else - a point cloud, a set of polylines - has no boundary of
//!   its own, and what can honestly be drawn around it is its **convex hull**
//!   in plan view. It is not the same thing, and the caller is told which it
//!   got so it can say so.
//!
//! # A boundary that is not a set of simple loops is refused
//!
//! A node with three boundary edges - two surfaces touching at a corner, a
//! sliver folded on itself - has no unambiguous loop through it. Choosing one
//! would draw a contour that looks right and is not; the refusal names the
//! node so that somebody can look at it.
//!
//! # Simplification drops what does not turn
//!
//! A node strictly between its neighbours contributes nothing to the outline
//! and is dropped when it lies within `tolerance` of the outline that replaces
//! it - `0` keeps everything but the exactly collinear. The bound holds for
//! every dropped node against the **original** nodes (Douglas-Peucker), not
//! pass by pass: see [`simplify`]. Measured in the shape's own units, in
//! 3D: a point that is collinear in plan and steps in Z is a corner.
//!
//! TODO(reference): verify - Andrew's monotone chain is named without a full citation; add the original reference once checked.

use std::collections::BTreeMap;

use crate::Error;
use crate::vector::distance_to_segment;

/// A closed loop of node indices - the first node is not repeated.
pub type Loop = Vec<u32>;

/// The boundary loops of a triangulated surface, each closed, in the order
/// they were found. Empty for a closed surface (a sphere has no boundary).
pub fn boundary_loops(triangles: &[[u32; 3]]) -> Result<Vec<Loop>, Error> {
    if triangles.is_empty() {
        return Err(Error::NoTriangle);
    }
    // An edge is keyed on its unordered pair; a boundary edge is seen once.
    let mut count: BTreeMap<(u32, u32), (u32, u32)> = BTreeMap::new();
    let mut seen: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    for triangle in triangles {
        for k in 0..3 {
            let a = triangle[k];
            let b = triangle[(k + 1) % 3];
            let key = if a < b { (a, b) } else { (b, a) };
            *seen.entry(key).or_insert(0) += 1;
            // Kept in the triangle's own direction, so the loops come out
            // wound the way the surface is.
            count.entry(key).or_insert((a, b));
        }
    }
    let mut next: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for (key, n) in &seen {
        if *n == 1 {
            let (a, b) = count[key];
            next.entry(a).or_default().push(b);
            next.entry(b).or_default().push(a);
        }
    }
    if let Some((node, _)) = next.iter().find(|(_, neighbours)| neighbours.len() != 2) {
        return Err(Error::NotSimple { node: *node });
    }
    // Walk each loop from its smallest node, following the triangle's own
    // direction where it is known.
    let mut visited: BTreeMap<u32, bool> = next.keys().map(|k| (*k, false)).collect();
    let mut loops = Vec::new();
    for start in next.keys().copied().collect::<Vec<_>>() {
        if visited[&start] {
            continue;
        }
        let mut ring = vec![start];
        visited.insert(start, true);
        let mut previous = start;
        // The forward neighbour: the one this node's directed boundary edge
        // points at, when the direction was kept; the first otherwise.
        let mut current = forward(&count, start, &next[&start]);
        while current != start {
            ring.push(current);
            visited.insert(current, true);
            let candidates = &next[&current];
            let onward = if candidates[0] == previous {
                candidates[1]
            } else {
                candidates[0]
            };
            previous = current;
            current = onward;
        }
        loops.push(ring);
    }
    Ok(loops)
}

fn forward(directed: &BTreeMap<(u32, u32), (u32, u32)>, from: u32, neighbours: &[u32]) -> u32 {
    for &to in neighbours {
        let key = if from < to { (from, to) } else { (to, from) };
        if directed.get(&key) == Some(&(from, to)) {
            return to;
        }
    }
    neighbours[0]
}

/// The convex hull of the points in plan view (XY), counterclockwise, as
/// indices into `points`. Andrew's monotone chain. Z is the contributing
/// point's own - a hull is a plan view, and its corners have no depth of
/// their own.
pub fn hull_xy(points: &[[f64; 3]]) -> Result<Loop, Error> {
    let mut order: Vec<usize> = (0..points.len())
        .filter(|&i| points[i][0].is_finite() && points[i][1].is_finite())
        .collect();
    if order.len() < 3 {
        return Err(Error::TooFewPoints { given: order.len() });
    }
    order.sort_by(|&a, &b| {
        points[a][0]
            .partial_cmp(&points[b][0])
            .unwrap()
            .then(points[a][1].partial_cmp(&points[b][1]).unwrap())
    });
    let cross = |o: usize, a: usize, b: usize| {
        (points[a][0] - points[o][0]) * (points[b][1] - points[o][1])
            - (points[a][1] - points[o][1]) * (points[b][0] - points[o][0])
    };
    let mut lower: Vec<usize> = Vec::new();
    for &p in &order {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<usize> = Vec::new();
    for &p in order.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    if lower.len() < 3 {
        return Err(Error::TooFewPoints { given: lower.len() });
    }
    Ok(lower.into_iter().map(|i| i as u32).collect())
}

/// The same loop without the nodes that do not turn: Douglas-Peucker on the
/// ring, every dropped node measured against the **original** outline.
///
/// The bound: every node of `ring` is within `tolerance` (3D distance to a
/// segment, in the shape's own units) of the segment between the two kept
/// nodes that enclose it, so the answer never strays from the original nodes by
/// more than `tolerance` - however many nodes were dropped, and however curved
/// the outline. (The previous version dropped nodes by their neighbours'
/// chords, pass after pass, and a smooth arc was flattened far past the
/// tolerance.) `0` keeps everything but the exactly collinear.
///
/// The ring is split at two anchors: its lexicographically smallest node, an
/// extreme node that no chord of the ring can cross, and the node farthest from
/// it. Never below three nodes - a ring of two is an edge, not an outline: the
/// dropped node deviating most is put back when fewer remain. A tolerance that
/// is not a finite number >= 0 is `0`; a node whose distance is not a number
/// is kept.
pub fn simplify(points: &[[f64; 3]], ring: &[u32], tolerance: f64) -> Loop {
    let tolerance = if tolerance.is_finite() && tolerance >= 0.0 {
        tolerance
    } else {
        0.0
    };
    let n = ring.len();
    if n <= 3 {
        return ring.to_vec();
    }
    let at = |k: usize| points[ring[k % n] as usize];
    // The first anchor: the smallest node, so a rotation of the ring does not
    // change the answer.
    let first = (0..n)
        .min_by(|&a, &b| {
            let (p, q) = (at(a), at(b));
            p[0].total_cmp(&q[0])
                .then(p[1].total_cmp(&q[1]))
                .then(p[2].total_cmp(&q[2]))
        })
        .unwrap_or(0);
    // The second: the farthest from the first (the first of equals).
    let mut far = 1;
    let mut far_distance = f64::NEG_INFINITY;
    for k in 1..n {
        let d = crate::vector::distance(at(first), at(first + k));
        if d > far_distance {
            far = k;
            far_distance = d;
        }
    }
    // Offsets from `first`; `keep[k]` is for the node at `first + k`.
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[far] = true;
    let mut stack = vec![(0usize, far), (far, n)];
    let mut deviation = vec![0.0f64; n];
    while let Some((lo, hi)) = stack.pop() {
        if hi - lo < 2 {
            continue;
        }
        let (a, b) = (at(first + lo), at(first + hi));
        let mut worst = lo + 1;
        let mut worst_distance = f64::NEG_INFINITY;
        for k in lo + 1..hi {
            let d = distance_to_segment(at(first + k), a, b);
            let d = if d.is_nan() { f64::INFINITY } else { d };
            deviation[k] = d;
            if d > worst_distance {
                worst = k;
                worst_distance = d;
            }
        }
        if worst_distance > tolerance {
            keep[worst] = true;
            stack.push((lo, worst));
            stack.push((worst, hi));
        }
    }
    // A ring of two is an edge: put back the node that deviated most.
    if keep.iter().filter(|&&k| k).count() < 3 {
        let mut back = None;
        for k in 1..n {
            if !keep[k] && back.is_none_or(|b: usize| deviation[k] > deviation[b]) {
                back = Some(k);
            }
        }
        if let Some(k) = back {
            keep[k] = true;
        }
    }
    // The ring's own order and start, whatever the anchors were.
    (0..n)
        .filter(|&p| keep[(p + n - first) % n])
        .map(|p| ring[p])
        .collect()
}

/// The signed area of a loop in plan view - positive counterclockwise. What
/// tells an outer loop (the largest magnitude) from a hole.
pub fn area_xy(points: &[[f64; 3]], ring: &[u32]) -> f64 {
    let n = ring.len();
    let mut twice = 0.0;
    for i in 0..n {
        let p = points[ring[i] as usize];
        let q = points[ring[(i + 1) % n] as usize];
        twice += p[0] * q[1] - q[0] * p[1];
    }
    twice / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 3 × 3 grid of nodes, two triangles per cell: eight boundary edges.
    fn grid() -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
        let points: Vec<[f64; 3]> = (0..3)
            .flat_map(|j| (0..3).map(move |i| [i as f64, j as f64, 0.0]))
            .collect();
        let mut triangles = Vec::new();
        for j in 0..2u32 {
            for i in 0..2u32 {
                let a = j * 3 + i;
                triangles.push([a, a + 1, a + 4]);
                triangles.push([a, a + 4, a + 3]);
            }
        }
        (points, triangles)
    }

    #[test]
    fn the_boundary_of_a_grid_is_one_loop_of_its_eight_rim_nodes() {
        // To break: an interior edge counted as boundary, or a loop that stops
        // before closing.
        let (_, triangles) = grid();
        let loops = boundary_loops(&triangles).unwrap();
        assert_eq!(loops.len(), 1);
        let mut rim = loops[0].clone();
        rim.sort_unstable();
        assert_eq!(rim, vec![0, 1, 2, 3, 5, 6, 7, 8]);
        assert_eq!(loops[0].len(), 8, "no node twice, none repeated at the end");
    }

    #[test]
    fn the_loop_follows_the_triangles_winding() {
        // The grid's triangles turn counterclockwise; so does its rim.
        let (points, triangles) = grid();
        let loops = boundary_loops(&triangles).unwrap();
        assert!(area_xy(&points, &loops[0]) > 0.0);
    }

    #[test]
    fn a_hole_is_a_second_loop() {
        // The centre cell taken out: the rim, and the hole's four nodes.
        let (_, mut triangles) = grid();
        // Cell (1,1) is triangles 6 and 7 (a = 4): remove them and the centre node is unused.
        // Take instead the cell at a = 4 replaced by a ring around node 4: drop the four
        // triangles touching node 4 in a 3×3 mesh would leave no interior; so build a
        // 4×4 grid instead.
        triangles.clear();
        let n = 4u32;
        for j in 0..3u32 {
            for i in 0..3u32 {
                if i == 1 && j == 1 {
                    continue;
                }
                let a = j * n + i;
                triangles.push([a, a + 1, a + n + 1]);
                triangles.push([a, a + n + 1, a + n]);
            }
        }
        let loops = boundary_loops(&triangles).unwrap();
        assert_eq!(loops.len(), 2);
        let sizes: Vec<usize> = loops.iter().map(|l| l.len()).collect();
        assert!(sizes.contains(&12) && sizes.contains(&4), "{sizes:?}");
    }

    #[test]
    fn a_node_with_three_boundary_edges_is_refused_by_name() {
        // Two triangles sharing only node 0: a bow tie.
        let triangles = vec![[0, 1, 2], [0, 3, 4]];
        match boundary_loops(&triangles) {
            Err(Error::NotSimple { node }) => assert_eq!(node, 0),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_closed_surface_has_no_boundary_and_no_triangle_is_refused() {
        // A tetrahedron: every edge is shared by two faces.
        let tetra = vec![[0, 1, 2], [0, 3, 1], [1, 3, 2], [2, 3, 0]];
        assert_eq!(boundary_loops(&tetra).unwrap(), Vec::<Loop>::new());
        assert!(matches!(boundary_loops(&[]), Err(Error::NoTriangle)));
    }

    #[test]
    fn simplifying_drops_the_nodes_that_do_not_turn() {
        // The grid's rim has a node in the middle of each side: four corners remain.
        let (points, triangles) = grid();
        let loops = boundary_loops(&triangles).unwrap();
        let mut corners = simplify(&points, &loops[0], 0.0);
        corners.sort_unstable();
        assert_eq!(corners, vec![0, 2, 6, 8]);
    }

    #[test]
    fn a_step_in_z_is_a_corner_in_3d_and_a_tolerance_forgives_it() {
        let points = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.5],
            [2.0, 0.0, 0.0],
            [2.0, 2.0, 0.0],
            [0.0, 2.0, 0.0],
        ];
        let ring = vec![0u32, 1, 2, 3, 4];
        assert_eq!(simplify(&points, &ring, 0.0).len(), 5);
        assert_eq!(simplify(&points, &ring, 0.6).len(), 4);
    }

    /// [geostat-proj-roundtrip S5] A smooth arc simplified with a tolerance
    /// stays within it: the largest distance of an original node to the
    /// simplified outline is measured. The previous passes dropped every node
    /// within the tolerance of its neighbours' chord, so the whole circle
    /// collapsed to a triangle 50 units from its nodes.
    #[test]
    fn simplifying_a_circle_never_strays_past_the_tolerance() {
        let points: Vec<[f64; 3]> = (0..360)
            .map(|k| {
                let a = (k as f64).to_radians();
                [100.0 * a.cos(), 100.0 * a.sin(), 0.0]
            })
            .collect();
        let ring: Vec<u32> = (0..360).collect();
        for tolerance in [0.05, 0.5, 2.0] {
            let kept = simplify(&points, &ring, tolerance);
            assert!(kept.len() >= 3 && kept.len() < 360, "{} nodes", kept.len());
            let mut worst = 0.0f64;
            for p in &points {
                let near = (0..kept.len())
                    .map(|i| {
                        let a = points[kept[i] as usize];
                        let b = points[kept[(i + 1) % kept.len()] as usize];
                        distance_to_segment(*p, a, b)
                    })
                    .fold(f64::INFINITY, f64::min);
                worst = worst.max(near);
            }
            assert!(
                worst <= tolerance + 1e-9,
                "tolerance {tolerance}: an original node is {worst} from the outline"
            );
        }
        // Break it: drop each node by the chord of its two neighbours, pass after pass, as before.
    }

    #[test]
    fn simplifying_never_goes_below_three_nodes() {
        let points = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [3.0, 0.0, 0.0]];
        assert_eq!(simplify(&points, &[0, 1, 2, 3], 0.0).len(), 3);
    }

    #[test]
    fn the_hull_of_a_cloud_is_its_outer_corners_counterclockwise() {
        // To break: an inner point kept, or the winding reversed.
        let points = vec![
            [0.0, 0.0, 1.0],
            [2.0, 0.0, 2.0],
            [2.0, 2.0, 3.0],
            [0.0, 2.0, 4.0],
            [1.0, 1.0, 9.0],
            [1.0, 0.5, 9.0],
        ];
        let hull = hull_xy(&points).unwrap();
        let mut sorted = hull.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, vec![0, 1, 2, 3]);
        assert!(area_xy(&points, &hull) > 0.0);
        assert!(matches!(
            hull_xy(&points[..2]),
            Err(Error::TooFewPoints { given: 2 })
        ));
    }

    #[test]
    fn a_hull_ignores_positions_without_a_plan_view() {
        let points = vec![
            [0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [f64::NAN, 1.0, 0.0],
            [2.0, 2.0, 0.0],
            [0.0, 2.0, 0.0],
        ];
        let hull = hull_xy(&points).unwrap();
        assert!(!hull.contains(&2));
        assert_eq!(hull.len(), 4);
    }
}
