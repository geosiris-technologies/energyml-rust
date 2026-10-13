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
//! and is dropped when it lies within `tolerance` of the chord - `0` keeps
//! everything but the exactly collinear. Measured in the shape's own units, in
//! 3D: a point that is collinear in plan and steps in Z is a corner.
//!
//! TODO(reference): verify - Andrew's monotone chain is named without a full citation; add the original reference once checked.

use std::collections::BTreeMap;

use crate::Error;

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

/// The same loop without the nodes that do not turn: a node within
/// `tolerance` of the chord between its neighbours is dropped. Never below
/// three nodes - a ring of two is an edge, not an outline.
pub fn simplify(points: &[[f64; 3]], ring: &[u32], tolerance: f64) -> Loop {
    let tolerance = if tolerance.is_finite() && tolerance >= 0.0 {
        tolerance
    } else {
        0.0
    };
    let mut kept: Vec<u32> = ring.to_vec();
    // Repeated until nothing is dropped: removing one node changes its
    // neighbours' chords.
    loop {
        if kept.len() <= 3 {
            return kept;
        }
        let n = kept.len();
        let mut next = Vec::with_capacity(n);
        let mut dropped = false;
        let mut i = 0;
        while i < n {
            let a = points[kept[(i + n - 1) % n] as usize];
            let b = points[kept[i] as usize];
            let c = points[kept[(i + 1) % n] as usize];
            if !dropped_this_pass(
                &mut dropped,
                distance_to_chord(a, b, c) <= tolerance,
                next.len() + (n - i) > 3,
            ) {
                next.push(kept[i]);
            }
            i += 1;
        }
        if !dropped {
            return next;
        }
        kept = next;
    }
}

/// Whether to drop this node: only if it lies on the chord, and only while
/// three would remain. One drop per pass at most in a row is not needed -
/// dropping two collinear neighbours in one pass is still correct, since each
/// was tested against its original neighbours, and the loop repeats anyway.
fn dropped_this_pass(dropped: &mut bool, on_chord: bool, room: bool) -> bool {
    if on_chord && room {
        *dropped = true;
        true
    } else {
        false
    }
}

/// The distance from `b` to the line through `a` and `c`, in 3D. Coincident
/// `a` and `c` make the chord a point; the distance is then to that point.
fn distance_to_chord(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let ac_len = (ac[0] * ac[0] + ac[1] * ac[1] + ac[2] * ac[2]).sqrt();
    if ac_len == 0.0 || !ac_len.is_finite() {
        return (ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2]).sqrt();
    }
    let cross = [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ];
    (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt() / ac_len
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
