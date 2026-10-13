//! 2D Delaunay for the `triangulate` operation — the heavy computation
//! done in Rust because this is where a test reaches it and because `spade` has **exact** predicates: a
//! triangulation on floating-point predicates yields, over nearly collinear
//! points — a set of boreholes always is — triangles that overlap without any
//! simple assertion seeing it.
//!
//! The surface is 2.5D: the triangulation runs on (x, y) and each point's z is
//! kept as it stands — that is the definition of a surface with one value per
//! position, a horizon's. Two points at the same (x, y) are **one** vertex for
//! the triangulation; the triangles returned index the first of the two, and
//! the duplicate is simply unused — refusing it would rule out real point
//! clouds, which have them.
//!
//! # Two entry points, one algorithm
//!
//! [`triangulate_xy`] is the plain Delaunay: the triangles index the caller's
//! own buffer, and no point is added. [`triangulate_xy_constrained`] takes
//! fault traces as **constraint edges** — the constrained Delaunay of `spade`
//! — so that no triangle straddles a fault: the trace becomes a chain of
//! triangle edges, and the two blocks are triangulated on each side of it.
//! That adds points (the trace's own, and one at every place two traces
//! cross), so it hands back the **whole** point list rather than indices into
//! the caller's; the caller writes what it is given.
//!
//! What a constraint is not: a barrier. The triangles on the two sides share
//! the trace's vertices, so the surface stays one sheet, folded along the
//! fault rather than cut — and the z of a trace vertex is the trace's own,
//! which is why a fault handed to this function must carry one.
//!
//! TODO(reference): verify — Delaunay triangulation is delegated to the `spade` crate; cite the underlying references (as documented by that crate) once checked.

use spade::handles::FixedVertexHandle;
use spade::{
    ConstrainedDelaunayTriangulation, DelaunayTriangulation, HasPosition, HierarchyHintGenerator, Point2,
    Triangulation,
};

/// **Where an insertion starts looking** — a hierarchy of coarser
/// triangulations, so that locating a point costs `log n`.
///
/// `spade`'s default starts from the last vertex inserted and walks: cheap for
/// points that arrive in order, `√n` per point for a cloud that does not — and
/// clouds do not. The speed report measured an exponent of 1.84 and a million
/// points in 106 s. The order of
/// insertion is unchanged, and so is the triangulation: only the search that
/// finds each point's triangle is.
type Hint = HierarchyHintGenerator<f64>;

/// Triangulate an `[x, y, z, …]` cloud on (x, y) and return index triples into
/// the original buffer, wound anticlockwise in the plane.
///
/// Refusals are named, never empty results: fewer than three points, a
/// non-finite coordinate (with its position), and the case that most resembles
/// a success — points all collinear, whose triangulation is empty. "A
/// successful evaluation that yields nothing is a bug, not a result."
pub fn triangulate_xy(points: &[f64]) -> Result<Vec<u32>, String> {
    let count = check_triples(points)?;

    let mut triangulation: DelaunayTriangulation<Point2<f64>, (), (), (), Hint> =
        DelaunayTriangulation::new();
    // Each triangulation vertex's original index. A duplicate (same x, y)
    // returns the handle of a vertex already there: nothing to push.
    let mut original: Vec<u32> = Vec::with_capacity(count);
    for at in 0..count {
        let (x, y) = (points[at * 3], points[at * 3 + 1]);
        let handle = triangulation
            .insert(Point2::new(x, y))
            .map_err(|_| format!("point {at} has a non-finite coordinate ({x}, {y})"))?;
        if handle.index() == original.len() {
            original.push(at as u32);
        }
    }

    let mut triangles = Vec::with_capacity(triangulation.num_inner_faces() * 3);
    for face in triangulation.inner_faces() {
        for vertex in face.vertices() {
            triangles.push(original[vertex.fix().index()]);
        }
    }
    if triangles.is_empty() {
        return Err(format!("the {count} points are collinear — no triangle exists"));
    }
    Ok(triangles)
}

/// What a constrained triangulation hands back: the points it triangulated,
/// which are more than it was given.
#[derive(Debug, Clone, PartialEq)]
pub struct Triangulated {
    /// Every vertex, `[x, y, z]`: the cloud's own first (duplicates dropped),
    /// then the traces' vertices, then the points where traces cross.
    pub points: Vec<[f64; 3]>,
    /// Index triples into `points`, wound anticlockwise in the plane.
    pub triangles: Vec<u32>,
    /// How many of `points` were not in the cloud — trace vertices and
    /// crossings. Said, because a surface with more nodes than its samples
    /// is a surface somebody has to be told about.
    pub added: usize,
    /// How many cloud points were dropped as duplicates of an earlier (x, y).
    pub duplicates: usize,
    /// How many constraint edges the traces made, after splitting.
    pub constraints: usize,
}

/// A vertex that carries its z through the triangulation.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Vertex {
    at: Point2<f64>,
    z: f64,
}

impl HasPosition for Vertex {
    type Scalar = f64;
    fn position(&self) -> Point2<f64> {
        self.at
    }
}

/// Triangulate a cloud on (x, y) with fault traces as constraint edges.
///
/// `faults` are polylines of `[x, y, z]`: a trace's vertices enter the
/// triangulation with their own z, and a point where two traces cross gets
/// its z by linear interpolation along the trace being inserted. A trace with
/// fewer than two positions, or a non-finite coordinate, is refused by name.
///
/// With no fault at all this is [`triangulate_xy`] with the points copied
/// out, which is what lets a caller use one path for both.
pub fn triangulate_xy_constrained(points: &[f64], faults: &[Vec<[f64; 3]>]) -> Result<Triangulated, String> {
    let count = check_triples(points)?;
    for (at, line) in faults.iter().enumerate() {
        if line.len() < 2 {
            return Err(format!(
                "fault {at} has {} position(s), a trace needs two",
                line.len()
            ));
        }
        if let Some(bad) = line.iter().find(|p| !p.iter().all(|c| c.is_finite())) {
            return Err(format!(
                "fault {at} has a non-finite coordinate ({}, {}, {})",
                bad[0], bad[1], bad[2]
            ));
        }
    }

    let mut cdt: ConstrainedDelaunayTriangulation<Vertex, (), (), (), Hint> =
        ConstrainedDelaunayTriangulation::new();
    let mut duplicates = 0usize;
    for at in 0..count {
        let (x, y, z) = (points[at * 3], points[at * 3 + 1], points[at * 3 + 2]);
        let before = cdt.num_vertices();
        cdt.insert(Vertex {
            at: Point2::new(x, y),
            z,
        })
        .map_err(|_| format!("point {at} has a non-finite coordinate ({x}, {y})"))?;
        if cdt.num_vertices() == before {
            duplicates += 1;
        }
    }
    let cloud = cdt.num_vertices();

    // The traces, one constraint per piece. A trace vertex that lands on an
    // existing (x, y) is that vertex — its z is the cloud's, not the trace's,
    // because the cloud was there first and a fault does not move a sample.
    // Looked up rather than inserted: `insert` on a taken position **replaces**
    // the vertex's data, which is exactly the overwrite this guards against.
    let mut constraints = 0usize;
    for line in faults {
        let mut handles: Vec<FixedVertexHandle> = Vec::with_capacity(line.len());
        for p in line {
            let at = Point2::new(p[0], p[1]);
            let handle = match cdt.locate_vertex(at) {
                Some(found) => found.fix(),
                None => cdt
                    .insert(Vertex { at, z: p[2] })
                    .map_err(|_| "a fault vertex could not be inserted".to_string())?,
            };
            handles.push(handle);
        }
        for (pair, ends) in handles.windows(2).zip(line.windows(2)) {
            let (from, to) = (pair[0], pair[1]);
            if from == to {
                continue;
            }
            let (a, b) = (ends[0], ends[1]);
            // Where two traces cross, the new vertex's z is read along the
            // trace being inserted — the only line it is known to lie on.
            let made = cdt.add_constraint_and_split(from, to, |at| {
                let dx = b[0] - a[0];
                let dy = b[1] - a[1];
                let along = dx * dx + dy * dy;
                let t = if along > 0.0 {
                    ((at.x - a[0]) * dx + (at.y - a[1]) * dy) / along
                } else {
                    0.0
                };
                Vertex {
                    at,
                    z: a[2] + t.clamp(0.0, 1.0) * (b[2] - a[2]),
                }
            });
            constraints += made.len();
        }
    }

    let out: Vec<[f64; 3]> = cdt
        .vertices()
        .map(|v| [v.position().x, v.position().y, v.data().z])
        .collect();
    let mut triangles = Vec::with_capacity(cdt.num_inner_faces() * 3);
    for face in cdt.inner_faces() {
        for vertex in face.vertices() {
            triangles.push(vertex.fix().index() as u32);
        }
    }
    if triangles.is_empty() {
        return Err(format!("the {count} points are collinear — no triangle exists"));
    }
    let added = out.len() - cloud;
    Ok(Triangulated {
        points: out,
        triangles,
        added,
        duplicates,
        constraints,
    })
}

/// The point count, or the two refusals every entry point shares.
fn check_triples(points: &[f64]) -> Result<usize, String> {
    if !points.len().is_multiple_of(3) {
        return Err(format!("{} values do not make (x, y, z) triples", points.len()));
    }
    let count = points.len() / 3;
    if count < 3 {
        return Err(format!(
            "{count} point(s) — at least three are needed for a triangle"
        ));
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A square with a sample at each corner and one in the middle.
    fn square() -> Vec<f64> {
        vec![
            0.0, 0.0, 10.0, 10.0, 0.0, 20.0, 10.0, 10.0, 30.0, 0.0, 10.0, 40.0, 5.0, 5.0, 25.0,
        ]
    }

    /// Whether some triangle edge crosses the open segment `a`–`b`.
    fn an_edge_crosses(made: &Triangulated, a: [f64; 2], b: [f64; 2]) -> bool {
        let orient = |p: [f64; 2], q: [f64; 2], r: [f64; 2]| {
            (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
        };
        let strict = |p: [f64; 2], q: [f64; 2]| {
            let o1 = orient(a, b, p);
            let o2 = orient(a, b, q);
            let o3 = orient(p, q, a);
            let o4 = orient(p, q, b);
            o1 * o2 < 0.0 && o3 * o4 < 0.0
        };
        made.triangles.chunks_exact(3).any(|t| {
            let p = |i: u32| {
                let v = made.points[i as usize];
                [v[0], v[1]]
            };
            strict(p(t[0]), p(t[1])) || strict(p(t[1]), p(t[2])) || strict(p(t[2]), p(t[0]))
        })
    }

    #[test]
    fn without_a_fault_the_constrained_path_is_the_plain_one() {
        let plain = triangulate_xy(&square()).unwrap();
        let made = triangulate_xy_constrained(&square(), &[]).unwrap();
        assert_eq!(made.added, 0);
        assert_eq!(made.constraints, 0);
        assert_eq!(made.points.len(), 5);
        assert_eq!(made.triangles.len(), plain.len());
    }

    /// **No triangle straddles the fault.** The plain Delaunay of this cloud
    /// has an edge across the fault; the constrained one folds along it.
    #[test]
    fn a_fault_becomes_a_chain_of_edges_that_nothing_crosses() {
        // A fault from the bottom edge to the top, off-centre, carrying z.
        let fault = vec![vec![[3.0, -1.0, 0.0], [3.0, 11.0, 0.0]]];
        let made = triangulate_xy_constrained(&square(), &fault).unwrap();
        assert!(
            !an_edge_crosses(&made, [3.0, -1.0], [3.0, 11.0]),
            "an edge crosses the fault"
        );
        // The two trace vertices were added, with the trace's own z.
        assert_eq!(made.added, 2);
        assert!(
            made.points
                .iter()
                .any(|p| p[0] == 3.0 && p[1] == -1.0 && p[2] == 0.0)
        );
        assert!(made.constraints >= 1);
        // And the plain triangulation, on the same cloud, does cross it —
        // otherwise this test would prove nothing.
        let plain = triangulate_xy(&square()).unwrap();
        let plain_made = Triangulated {
            points: square().chunks_exact(3).map(|p| [p[0], p[1], p[2]]).collect(),
            triangles: plain,
            added: 0,
            duplicates: 0,
            constraints: 0,
        };
        assert!(an_edge_crosses(&plain_made, [3.0, -1.0], [3.0, 11.0]));
    }

    /// Two faults that cross: one vertex is added where they meet, and its z
    /// is read along the trace being inserted.
    #[test]
    fn crossing_faults_add_their_meeting_point_with_an_interpolated_z() {
        let faults = vec![
            vec![[-1.0, 5.0, 100.0], [11.0, 5.0, 100.0]],
            vec![[5.0, -1.0, 0.0], [5.0, 11.0, 12.0]],
        ];
        // The cloud has a sample at (5, 5), which is exactly the crossing:
        // move it so the crossing is a genuinely new point.
        let mut cloud = square();
        cloud[12] = 6.0;
        cloud[13] = 4.0;
        let made = triangulate_xy_constrained(&cloud, &faults).unwrap();
        // 4 trace vertices + 1 crossing.
        assert_eq!(made.added, 5);
        let at = made
            .points
            .iter()
            .find(|p| (p[0] - 5.0).abs() < 1e-9 && (p[1] - 5.0).abs() < 1e-9)
            .expect("the crossing");
        // Along the second trace, (5, 5) is halfway from z = 0 to z = 12.
        assert!((at[2] - 6.0).abs() < 1e-9, "z {}", at[2]);
    }

    #[test]
    fn a_short_or_nan_fault_is_refused_by_name() {
        let why = triangulate_xy_constrained(&square(), &[vec![[1.0, 1.0, 1.0]]]).unwrap_err();
        assert!(why.contains("fault 0"), "{why}");
        let why = triangulate_xy_constrained(&square(), &[vec![[0.0, 0.0, 0.0], [f64::NAN, 1.0, 0.0]]])
            .unwrap_err();
        assert!(why.contains("non-finite"), "{why}");
    }

    #[test]
    fn a_fault_vertex_on_a_sample_is_that_sample_and_keeps_its_z() {
        // The trace starts on the corner (0, 0), whose sample says z = 10.
        let fault = vec![vec![[0.0, 0.0, -999.0], [10.0, 10.0, -999.0]]];
        let made = triangulate_xy_constrained(&square(), &fault).unwrap();
        let corner = made.points.iter().find(|p| p[0] == 0.0 && p[1] == 0.0).unwrap();
        assert_eq!(corner[2], 10.0, "the cloud was there first");
        // Both trace ends sit on samples: nothing added.
        assert_eq!(made.added, 0);
    }
}
