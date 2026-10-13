//! Areas, lengths and volumes of shapes.
//!
//! What a shape *measures*, per element: the area of each triangle or grid
//! cell, the length of each polyline, the volume of each cell of an IJK grid.
//! The totals are the caller's sum of these — [`total`] — so that a total and
//! the per-element values can never disagree on what they mean.
//!
//! # Two areas and two lengths, both said
//!
//! A surface has an area **along itself** — in three dimensions — and an area
//! **in plan**, its projection on the horizontal. They are different facts: a
//! fault plane dipping at 60° has twice the area along it that it covers on a
//! map, and a vertical one covers none. Every function here returns both, and
//! the caller writes the one it was asked for, never one presented as the
//! other.
//!
//! # A grid cell is bilinear, never cut in two
//!
//! A cell of a 2D grid is the bilinear patch through its four corners — the
//! reading `thickness` and `project` already make of it. Cutting it into two
//! triangles would give an area that depends on a diagonal the file never
//! chose. So:
//!
//! - **in plan the area is exact**: the plan projection of a bilinear patch
//!   is a bilinear map of the plane, whose Jacobian is linear in each
//!   parameter, so its integral is the shoelace area of the four corners. A
//!   cell whose projection **folds** — the Jacobian changes sign between two
//!   corners, a bow-tie in plan — has no single plan area: `NaN`, counted;
//! - **along the surface the area is integrated numerically** — the norm of
//!   the Jacobian is a square root, with no closed form for a warped patch —
//!   by a 4 × 4 Gauss–Legendre rule. Exact for a planar cell (the integrand is
//!   then bilinear), and within 1e-9 relative on the warped patches the tests
//!   measure against an independent integration.
//!
//! # A hexahedron is trilinear, and its volume is exact
//!
//! An IJK cell is the trilinear map of the unit cube through its eight
//! corners. The determinant of its Jacobian has degree at most two in each
//! parameter, so the 2 × 2 × 2 Gauss–Legendre rule, exact up to degree three,
//! gives its volume **exactly** — non-planar faces included, which a
//! decomposition into tetrahedra would get wrong by a choice of diagonals.
//!
//! The sign of the determinant depends on the handedness of the corner order
//! and of the frame (a depth axis mirrors it), neither of which a volume
//! should depend on: the magnitude is kept. What is refused is a cell whose
//! determinant **changes sign inside it** — a twisted cell, whose integral
//! subtracts the overlap from the rest: `NaN`, counted. A collapsed cell — a
//! pinch-out, zero thickness — has a volume of zero, which is an answer, and
//! is counted apart so that a grid of flat cells does not pass for a grid of
//! small ones.
//!
//! # What is not a number is not measured
//!
//! A corner that is not a finite point makes its element `NaN`, counted as
//! `unplaced` — never a zero, which would read as an element that measures
//! nothing.

use crate::Error;

/// A point, `[x, y, z]`.
pub type Point = [f64; 3];

/// Per-element areas of a surface, both ways, in the square of the
/// coordinates' unit.
#[derive(Debug, Clone, PartialEq)]
pub struct Areas {
    /// The area along the surface, in three dimensions, per element.
    pub along: Vec<f64>,
    /// The area of its projection on the horizontal, per element.
    pub plan: Vec<f64>,
    /// Elements with a corner that is not a finite point: `NaN` both ways.
    pub unplaced: usize,
    /// Grid cells whose projection folds over itself: `NaN` in plan only.
    pub folded: usize,
}

/// Per-polyline lengths, both ways, in the coordinates' unit.
#[derive(Debug, Clone, PartialEq)]
pub struct Lengths {
    /// Along the line, in three dimensions.
    pub along: Vec<f64>,
    /// Of its projection on the horizontal.
    pub plan: Vec<f64>,
    /// Lines with a node that is not a finite point: `NaN` both ways.
    pub unplaced: usize,
}

/// Per-cell volumes, in the cube of the coordinates' unit.
#[derive(Debug, Clone, PartialEq)]
pub struct Volumes {
    /// One per cell, in the order given.
    pub values: Vec<f64>,
    /// Cells with a corner that is not a finite point: `NaN`.
    pub unplaced: usize,
    /// Cells whose Jacobian changes sign inside them: `NaN`.
    pub twisted: usize,
    /// Cells of zero volume — collapsed, a pinch-out. Kept at zero, and
    /// counted.
    pub flat: usize,
}

fn sub(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: Point, b: Point) -> Point {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: Point, b: Point) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(a: Point) -> f64 {
    dot(a, a).sqrt()
}

fn finite(p: &Point) -> bool {
    p.iter().all(|c| c.is_finite())
}

/// The nodes an element names, or the refusal naming the one out of range.
fn corners<const N: usize>(
    points: &[Point],
    indices: &[u32; N],
    what: &'static str,
    element: usize,
) -> Result<[Point; N], Error> {
    let mut out = [[0.0; 3]; N];
    for (slot, &at) in indices.iter().enumerate() {
        out[slot] = *points.get(at as usize).ok_or(Error::ElementOutOfRange {
            what,
            element,
            at: at as usize,
            nodes: points.len(),
        })?;
    }
    Ok(out)
}

/// The area of each triangle, along it and in plan.
pub fn triangle_areas(points: &[Point], triangles: &[[u32; 3]]) -> Result<Areas, Error> {
    let mut out = Areas {
        along: Vec::with_capacity(triangles.len()),
        plan: Vec::with_capacity(triangles.len()),
        unplaced: 0,
        folded: 0,
    };
    for (element, triangle) in triangles.iter().enumerate() {
        let [a, b, c] = corners(points, triangle, "triangle", element)?;
        if ![a, b, c].iter().all(finite) {
            out.along.push(f64::NAN);
            out.plan.push(f64::NAN);
            out.unplaced += 1;
            continue;
        }
        let normal = cross(sub(b, a), sub(c, a));
        out.along.push(0.5 * norm(normal));
        out.plan.push(0.5 * normal[2].abs());
    }
    Ok(out)
}

/// The Gauss–Legendre rule of four points on `[0, 1]`: abscissae and weights.
const GAUSS_4: [(f64, f64); 4] = [
    (0.069_431_844_202_973_71, 0.173_927_422_568_726_93),
    (0.330_009_478_207_571_87, 0.326_072_577_431_273_07),
    (0.669_990_521_792_428_1, 0.326_072_577_431_273_07),
    (0.930_568_155_797_026_3, 0.173_927_422_568_726_93),
];

/// The two-point rule on `[0, 1]`: `(1 ∓ 1/√3) / 2`, each weighing a half.
const GAUSS_2: [f64; 2] = [0.211_324_865_405_187_1, 0.788_675_134_594_812_9];

/// The area of each quadrilateral — the bilinear patch through `a, b, c, d`,
/// **in ring order** — along it and in plan.
pub fn quad_areas(points: &[Point], quads: &[[u32; 4]]) -> Result<Areas, Error> {
    let mut out = Areas {
        along: Vec::with_capacity(quads.len()),
        plan: Vec::with_capacity(quads.len()),
        unplaced: 0,
        folded: 0,
    };
    for (element, quad) in quads.iter().enumerate() {
        let [a, b, c, d] = corners(points, quad, "cell", element)?;
        if ![a, b, c, d].iter().all(finite) {
            out.along.push(f64::NAN);
            out.plan.push(f64::NAN);
            out.unplaced += 1;
            continue;
        }
        // X(u, v) = (1-u)(1-v) a + u(1-v) b + u v c + (1-u) v d.
        let tangents = |u: f64, v: f64| -> (Point, Point) {
            let du = [0, 1, 2].map(|k| (1.0 - v) * (b[k] - a[k]) + v * (c[k] - d[k]));
            let dv = [0, 1, 2].map(|k| (1.0 - u) * (d[k] - a[k]) + u * (c[k] - b[k]));
            (du, dv)
        };
        let mut along = 0.0;
        for &(u, wu) in &GAUSS_4 {
            for &(v, wv) in &GAUSS_4 {
                let (du, dv) = tangents(u, v);
                along += wu * wv * norm(cross(du, dv));
            }
        }
        out.along.push(along);
        // In plan the Jacobian is bilinear in (u, v), so its extremes are at
        // the corners: a sign change there is a fold, and nowhere else.
        let jz = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].map(|(u, v)| {
            let (du, dv) = tangents(u, v);
            du[0] * dv[1] - du[1] * dv[0]
        });
        let scale = jz.iter().fold(0.0f64, |m, j| m.max(j.abs()));
        let tolerance = 1e-12 * scale;
        let positive = jz.iter().any(|j| *j > tolerance);
        let negative = jz.iter().any(|j| *j < -tolerance);
        if positive && negative {
            out.plan.push(f64::NAN);
            out.folded += 1;
        } else {
            // The shoelace area of the ring, which is the exact integral of
            // that Jacobian.
            let ring = [a, b, c, d];
            let twice: f64 = (0..4)
                .map(|k| {
                    let (p, q) = (ring[k], ring[(k + 1) % 4]);
                    p[0] * q[1] - q[0] * p[1]
                })
                .sum();
            out.plan.push(0.5 * twice.abs());
        }
    }
    Ok(out)
}

/// The length of each polyline — `runs` consecutive nodes each, the closing
/// segment added where `closed` says so (a missing entry is open) — along it
/// and in plan.
pub fn polyline_lengths(points: &[Point], runs: &[u32], closed: &[bool]) -> Result<Lengths, Error> {
    let total: usize = runs.iter().map(|&run| run as usize).sum();
    if total > points.len() {
        return Err(Error::RunsPastEnd {
            total,
            nodes: points.len(),
        });
    }
    let mut out = Lengths {
        along: Vec::with_capacity(runs.len()),
        plan: Vec::with_capacity(runs.len()),
        unplaced: 0,
    };
    let mut start = 0usize;
    for (line, &run) in runs.iter().enumerate() {
        let nodes = &points[start..start + run as usize];
        start += run as usize;
        if !nodes.iter().all(finite) {
            out.along.push(f64::NAN);
            out.plan.push(f64::NAN);
            out.unplaced += 1;
            continue;
        }
        let mut segments: Vec<(Point, Point)> = nodes.windows(2).map(|pair| (pair[0], pair[1])).collect();
        if closed.get(line).copied().unwrap_or(false) && nodes.len() > 2 {
            segments.push((nodes[nodes.len() - 1], nodes[0]));
        }
        out.along
            .push(segments.iter().map(|(p, q)| norm(sub(*q, *p))).sum());
        out.plan.push(
            segments
                .iter()
                .map(|(p, q)| (q[0] - p[0]).hypot(q[1] - p[1]))
                .sum(),
        );
    }
    Ok(out)
}

/// The volume of each hexahedron — eight corners in VTK order: the bottom
/// ring `0, 1, 2, 3`, then the top ring `4, 5, 6, 7` above it, corner for
/// corner.
pub fn hexahedron_volumes(points: &[Point], cells: &[[u32; 8]]) -> Result<Volumes, Error> {
    let mut out = Volumes {
        values: Vec::with_capacity(cells.len()),
        unplaced: 0,
        twisted: 0,
        flat: 0,
    };
    for (element, cell) in cells.iter().enumerate() {
        let p = corners(points, cell, "cell", element)?;
        if !p.iter().all(finite) {
            out.values.push(f64::NAN);
            out.unplaced += 1;
            continue;
        }
        let determinants = trilinear_determinants(&p);
        let signed: f64 = determinants.iter().sum::<f64>() / 8.0;
        let scale = determinants.iter().fold(0.0f64, |m, j| m.max(j.abs()));
        if scale == 0.0 || signed.abs() <= 1e-12 * scale {
            // Every Gauss point degenerate, or a cancellation that leaves
            // nothing: collapsed if nothing is inverted, twisted otherwise.
            if determinants
                .iter()
                .all(|j| j.abs() <= 1e-9 * scale.max(f64::MIN_POSITIVE))
            {
                out.values.push(0.0);
                out.flat += 1;
            } else {
                out.values.push(f64::NAN);
                out.twisted += 1;
            }
            continue;
        }
        let tolerance = 1e-9 * scale;
        if determinants.iter().any(|j| j * signed.signum() < -tolerance) {
            out.values.push(f64::NAN);
            out.twisted += 1;
            continue;
        }
        out.values.push(signed.abs());
    }
    Ok(out)
}

/// The Jacobian determinant of the trilinear map at the eight points of the
/// two-point rule.
fn trilinear_determinants(p: &[Point; 8]) -> [f64; 8] {
    // The corner of each rank on the unit cube, VTK order.
    const AT: [[f64; 3]; 8] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
    ];
    // One factor of a shape function: `t` at the corner's 1, `1 - t` at its 0.
    let f = |corner: f64, t: f64| if corner == 1.0 { t } else { 1.0 - t };
    let df = |corner: f64| if corner == 1.0 { 1.0 } else { -1.0 };
    let mut out = [0.0; 8];
    let mut at = 0;
    for &u in &GAUSS_2 {
        for &v in &GAUSS_2 {
            for &w in &GAUSS_2 {
                let (mut xu, mut xv, mut xw) = ([0.0; 3], [0.0; 3], [0.0; 3]);
                for (corner, point) in AT.iter().zip(p) {
                    let nu = df(corner[0]) * f(corner[1], v) * f(corner[2], w);
                    let nv = f(corner[0], u) * df(corner[1]) * f(corner[2], w);
                    let nw = f(corner[0], u) * f(corner[1], v) * df(corner[2]);
                    for k in 0..3 {
                        xu[k] += nu * point[k];
                        xv[k] += nv * point[k];
                        xw[k] += nw * point[k];
                    }
                }
                out[at] = dot(cross(xu, xv), xw);
                at += 1;
            }
        }
    }
    out
}

/// The sum of the finite values, and how many were not — the total a caller
/// reports beside the per-element values.
pub fn total(values: &[f64]) -> (f64, usize) {
    values.iter().fold((0.0, 0), |(sum, set_aside), v| {
        if v.is_finite() {
            (sum + v, set_aside)
        } else {
            (sum, set_aside + 1)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tolerance: f64) -> bool {
        (a - b).abs() <= tolerance * b.abs().max(1.0)
    }

    /// A unit cube, and one whose top ring is lifted and shifted: a sheared
    /// parallelepiped of base 1 and height 1 has volume 1 whatever the shift.
    #[test]
    fn a_sheared_cube_keeps_its_volume() {
        let cube = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.3, 0.2, 1.0],
            [1.3, 0.2, 1.0],
            [1.3, 1.2, 1.0],
            [0.3, 1.2, 1.0],
        ];
        let found = hexahedron_volumes(&cube, &[[0, 1, 2, 3, 4, 5, 6, 7]]).unwrap();
        assert!(close(found.values[0], 1.0, 1e-12), "{:?}", found.values);
    }

    /// The winding does not change a volume: the same cell listed top first.
    #[test]
    fn a_mirrored_cell_has_the_same_volume() {
        let points = [
            [0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [2.0, 3.0, 0.0],
            [0.0, 3.0, 0.0],
            [0.0, 0.0, 4.0],
            [2.0, 0.0, 4.0],
            [2.0, 3.0, 4.0],
            [0.0, 3.0, 4.0],
        ];
        let up = hexahedron_volumes(&points, &[[0, 1, 2, 3, 4, 5, 6, 7]]).unwrap();
        let down = hexahedron_volumes(&points, &[[4, 5, 6, 7, 0, 1, 2, 3]]).unwrap();
        assert!(close(up.values[0], 24.0, 1e-12));
        assert!(close(down.values[0], 24.0, 1e-12));
    }

    #[test]
    fn a_collapsed_cell_is_flat_and_a_twisted_one_is_refused() {
        let flat = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ];
        let found = hexahedron_volumes(&flat, &[[0, 1, 2, 3, 4, 5, 6, 7]]).unwrap();
        assert_eq!((found.values[0], found.flat, found.twisted), (0.0, 1, 0));
        // The top ring crosses below the bottom at one side: the cell turns
        // inside out halfway.
        let twisted = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, -1.0],
            [1.0, 1.0, -1.0],
            [0.0, 1.0, 1.0],
        ];
        let found = hexahedron_volumes(&twisted, &[[0, 1, 2, 3, 4, 5, 6, 7]]).unwrap();
        assert!(found.values[0].is_nan());
        assert_eq!(found.twisted, 1);
    }

    #[test]
    fn an_element_naming_a_missing_node_is_refused_by_name() {
        let refused = triangle_areas(&[[0.0; 3]; 2], &[[0, 1, 2]]).unwrap_err();
        assert_eq!(
            refused,
            Error::ElementOutOfRange {
                what: "triangle",
                element: 0,
                at: 2,
                nodes: 2
            }
        );
    }

    #[test]
    fn a_bow_tie_in_plan_has_no_plan_area() {
        // The ring crosses itself in plan; along it the patch is a fine surface.
        let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 1.0], [1.0, 1.0, 1.0]];
        let found = quad_areas(&points, &[[0, 1, 2, 3]]).unwrap();
        assert!(found.plan[0].is_nan());
        assert_eq!(found.folded, 1);
        assert!(found.along[0].is_finite());
    }

    #[test]
    fn a_total_sets_the_nans_aside() {
        assert_eq!(total(&[1.0, f64::NAN, 2.5]), (3.5, 1));
    }
}
