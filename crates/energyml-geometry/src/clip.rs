//! Cutting a surface with a ring seen from above — removing parts:
//! the user draws a ring (outer or
//! inner), and what falls inside — or outside — becomes a **new**
//! representation. Never a modification of the original: it stays what the
//! producer wrote, and the provenance says which came from which.
//!
//! # The rules
//!
//! - **A triangle goes to the side its centre is on.** A triangle the ring
//!   crosses is not cut in two: v1 classifies it by its centroid, and says so
//!   — cutting cleanly would mean inserting vertices on the boundary, which
//!   will come if a real need measures it. The boundary is therefore jagged at
//!   the triangle's own scale, which is visible and honest, where an
//!   approximate cut would be smooth and wrong.
//! - **On the edge counts as inside.** The even-odd test puts boundary points
//!   on one side or the other according to rounding; counting them inside
//!   makes the choice deterministic and stated — a ring drawn exactly along an
//!   edge does not make the triangle flicker.
//! - **The result is compacted.** Nodes no triangle references any more are
//!   dropped and the indices remapped: a new representation does not carry
//!   another one's points.
//! - **Empty is an answer.** A ring that catches nothing yields zero
//!   triangles, and the caller says so with the counts — no empty object
//!   created in silence.

/// One half of a cut: its compacted points, its remapped triangles.
pub type Cutout = (Vec<[f64; 3]>, Vec<[u32; 3]>);

/// Is an (x, y) point inside the ring (even-odd, boundary included)?
pub fn inside_ring(point: [f64; 2], ring: &[[f64; 2]]) -> bool {
    if ring.len() < 3 {
        return false;
    }
    let [x, y] = point;
    let mut inside = false;
    let mut previous = ring[ring.len() - 1];
    for &current in ring {
        let [x1, y1] = previous;
        let [x2, y2] = current;
        // The edge counts as inside: a point on the segment is accepted before
        // the even-odd test, which would put it on either side as rounding
        // decided.
        let cross = (x2 - x1) * (y - y1) - (y2 - y1) * (x - x1);
        if cross.abs() < 1e-12
            && x >= x1.min(x2) - 1e-12
            && x <= x1.max(x2) + 1e-12
            && y >= y1.min(y2) - 1e-12
            && y <= y1.max(y2) + 1e-12
        {
            return true;
        }
        if (y1 > y) != (y2 > y) {
            let at = (x2 - x1) * (y - y1) / (y2 - y1) + x1;
            if x < at {
                inside = !inside;
            }
        }
        previous = current;
    }
    inside
}

/// The trimmed surface: the triangles kept (on the requested side), compacted.
///
/// `keep_inside` says which side survives — keeping the inside cuts out a
/// patch, keeping the outside punches a hole. A triangle with a `NaN` corner is
/// dropped (no side is its own), and so is one with an invalid index.
pub fn trim(points: &[[f64; 3]], triangles: &[[u32; 3]], ring: &[[f64; 2]], keep_inside: bool) -> Cutout {
    let mut kept: Vec<[u32; 3]> = Vec::new();
    for triangle in triangles {
        let corners: Vec<&[f64; 3]> = triangle
            .iter()
            .filter_map(|&index| points.get(index as usize))
            .collect();
        if corners.len() != 3 || corners.iter().any(|c| c.iter().any(|v| !v.is_finite())) {
            continue;
        }
        let centre = [
            (corners[0][0] + corners[1][0] + corners[2][0]) / 3.0,
            (corners[0][1] + corners[1][1] + corners[2][1]) / 3.0,
        ];
        if inside_ring(centre, ring) == keep_inside {
            kept.push(*triangle);
        }
    }
    compact(points, &kept)
}

/// Which side of a polyline in plan a point falls on.
///
/// The sign of the cross product against the **nearest** segment — measuring
/// against a distant one would give the side of a bend the point does not run
/// along. Positive to the left of the direction of travel, negative to the
/// right; a point exactly on the line yields 0 and `split_by_line` files it on
/// the left — deterministic, like a ring's edge.
pub fn side_of_line(point: [f64; 2], line: &[[f64; 2]]) -> f64 {
    let [x, y] = point;
    let mut best = f64::MAX;
    let mut side = 0.0;
    for pair in line.windows(2) {
        let [a, b] = [pair[0], pair[1]];
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let length2 = dx * dx + dy * dy;
        if length2 < 1e-24 {
            continue;
        }
        let t = (((x - a[0]) * dx + (y - a[1]) * dy) / length2).clamp(0.0, 1.0);
        let (px, py) = (a[0] + dx * t, a[1] + dy * t);
        let d2 = (x - px) * (x - px) + (y - py) * (y - py);
        if d2 < best {
            best = d2;
            side = dx * (y - a[1]) - dy * (x - a[0]);
        }
    }
    side
}

/// The surface split by a cut line in plan, into patches:
/// left then right of the direction of travel,
/// each compacted. Same rule as [`trim`]: a triangle goes to the side its
/// centroid is on, the boundary is jagged at the triangle's scale, and a `NaN`
/// corner drops its triangle from both sides.
pub fn split_by_line(points: &[[f64; 3]], triangles: &[[u32; 3]], line: &[[f64; 2]]) -> [Cutout; 2] {
    let mut left: Vec<[u32; 3]> = Vec::new();
    let mut right: Vec<[u32; 3]> = Vec::new();
    for triangle in triangles {
        let corners: Vec<&[f64; 3]> = triangle
            .iter()
            .filter_map(|&index| points.get(index as usize))
            .collect();
        if corners.len() != 3 || corners.iter().any(|c| c.iter().any(|v| !v.is_finite())) {
            continue;
        }
        let centre = [
            (corners[0][0] + corners[1][0] + corners[2][0]) / 3.0,
            (corners[0][1] + corners[1][1] + corners[2][1]) / 3.0,
        ];
        if side_of_line(centre, line) >= 0.0 {
            left.push(*triangle);
        } else {
            right.push(*triangle);
        }
    }
    [compact(points, &left), compact(points, &right)]
}

/// The shared compaction: orphaned nodes go, the indices follow.
fn compact(points: &[[f64; 3]], kept: &[[u32; 3]]) -> Cutout {
    let mut remap: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    let mut out_points: Vec<[f64; 3]> = Vec::new();
    let mut out_triangles: Vec<[u32; 3]> = Vec::with_capacity(kept.len());
    for triangle in kept {
        let mut mapped = [0u32; 3];
        for (slot, &index) in triangle.iter().enumerate() {
            let next = *remap.entry(index).or_insert_with(|| {
                out_points.push(points[index as usize]);
                (out_points.len() - 1) as u32
            });
            mapped[slot] = next;
        }
        out_triangles.push(mapped);
    }
    (out_points, out_triangles)
}

/// **The nodes inside a ring drawn in plan** — or outside it: the ring
/// selector, asked for as a shift + mouse-drag gesture drawing a ring that
/// selects a set of points inside it.
///
/// `points` are flat `[x, y, z, …]`, as the scene draws them; `ring` is flat
/// `[x, y, …]`, **closed implicitly** — a drawing whose ends do not meet is
/// joined straight from its end to its start, as asked. The answer is the
/// ranks of the nodes whose (x, y) lies inside the ring by [`inside_ring`] —
/// the reading `trim` makes, the boundary counted inside — or outside it.
/// A node with no finite position is in neither: it has no place to be
/// selected by.
///
/// Refused by name: a ring of fewer than three corners encloses nothing, and
/// arrays that are not pairs and triplets are not a ring and nodes.
pub fn nodes_in_ring(points: &[f64], ring: &[f64], outside: bool) -> Result<Vec<u32>, String> {
    if !points.len().is_multiple_of(3) {
        return Err(format!("{} coordinate(s) — nodes are triplets", points.len()));
    }
    if !ring.len().is_multiple_of(2) {
        return Err(format!("{} coordinate(s) — a ring in plan is pairs", ring.len()));
    }
    let corners: Vec<[f64; 2]> = ring.chunks_exact(2).map(|c| [c[0], c[1]]).collect();
    if corners.len() < 3 {
        return Err(format!(
            "a ring of {} corner(s) encloses nothing — three at least",
            corners.len()
        ));
    }
    Ok(points
        .chunks_exact(3)
        .enumerate()
        .filter(|(_, p)| p.iter().all(|c| c.is_finite()))
        .filter(|(_, p)| inside_ring([p[0], p[1]], &corners) != outside)
        .map(|(at, _)| at as u32)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nodes_inside_a_ring_and_those_outside() {
        // A square ring in plan, drawn open: its last side is implied.
        let ring = [0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0];
        let points = [
            5.0,
            5.0,
            -100.0,
            20.0,
            5.0,
            0.0,
            10.0,
            5.0,
            3.0,
            f64::NAN,
            5.0,
            0.0,
        ];
        // z does not matter; the edge counts inside; a node with no place is in neither.
        assert_eq!(nodes_in_ring(&points, &ring, false).unwrap(), vec![0, 2]);
        assert_eq!(nodes_in_ring(&points, &ring, true).unwrap(), vec![1]);
    }

    #[test]
    fn a_ring_of_two_corners_is_refused() {
        assert!(
            nodes_in_ring(&[0.0; 3], &[0.0, 0.0, 1.0, 1.0], false)
                .unwrap_err()
                .contains("three")
        );
        assert!(nodes_in_ring(&[0.0; 4], &[0.0; 6], false).is_err());
    }

    /// A 4×4 grid of triangles over [0, 4]² at z = 1.
    fn a_meshed_square() -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
        let mut points = Vec::new();
        for j in 0..5 {
            for i in 0..5 {
                points.push([i as f64, j as f64, 1.0]);
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
    fn inside_and_outside_partition_the_surface() {
        let (points, triangles) = a_meshed_square();
        // The ring covers the bottom-left quarter, centres included.
        let ring = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]];
        let (in_points, in_triangles) = trim(&points, &triangles, &ring, true);
        let (out_points, out_triangles) = trim(&points, &triangles, &ring, false);
        // The partition is exact: every triangle goes to one side.
        assert_eq!(in_triangles.len() + out_triangles.len(), triangles.len());
        assert_eq!(in_triangles.len(), 8, "2×2 cells of two triangles each");
        // Compacted: the patch carries only its own nodes.
        assert_eq!(in_points.len(), 9, "the 3×3 lattice of the cut quarter");
        assert!(out_points.len() < points.len());
        // Every kept index points inside its own buffer.
        for triangle in &in_triangles {
            assert!(triangle.iter().all(|&index| (index as usize) < in_points.len()));
        }
    }

    #[test]
    fn the_boundary_counts_as_inside_deterministically() {
        let ring = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]];
        assert!(inside_ring([1.0, 1.0], &ring));
        assert!(
            inside_ring([2.0, 1.0], &ring),
            "on the edge: inside, without flickering"
        );
        assert!(inside_ring([0.0, 0.0], &ring), "a corner too");
        assert!(!inside_ring([3.0, 1.0], &ring));
    }

    #[test]
    fn an_empty_catch_is_an_empty_answer_not_a_panic() {
        let (points, triangles) = a_meshed_square();
        let ring = [[10.0, 10.0], [11.0, 10.0], [11.0, 11.0], [10.0, 11.0]];
        let (kept_points, kept) = trim(&points, &triangles, &ring, true);
        assert!(kept.is_empty());
        assert!(kept_points.is_empty());
    }

    #[test]
    fn a_line_down_the_middle_splits_the_square_in_halves() {
        let (points, triangles) = a_meshed_square();
        // The vertical x = 2, travelled towards +y: the left is x < 2.
        let line = [[2.0, -1.0], [2.0, 5.0]];
        let [(left_points, left), (right_points, right)] = split_by_line(&points, &triangles, &line);
        assert_eq!(left.len() + right.len(), triangles.len());
        assert_eq!(
            left.len(),
            right.len(),
            "a vertical down the middle shares it out evenly"
        );
        // Each half is compacted and consistent.
        for (half_points, half) in [(&left_points, &left), (&right_points, &right)] {
            for triangle in half.iter() {
                assert!(triangle.iter().all(|&index| (index as usize) < half_points.len()));
            }
        }
        // The side is measured against the nearest segment: to the left of the
        // upward vertical, x < 2.
        for point in &left_points {
            assert!(point[0] <= 2.0 + 1e-9);
        }
    }

    #[test]
    fn a_nan_corner_drops_its_triangle_from_both_sides() {
        let (mut points, triangles) = a_meshed_square();
        points[0][2] = f64::NAN;
        let ring = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        let (_, inside) = trim(&points, &triangles, &ring, true);
        let (_, outside) = trim(&points, &triangles, &ring, false);
        // The two triangles touching the NaN node are on neither side.
        assert_eq!(inside.len() + outside.len(), triangles.len() - 2);
    }
}
