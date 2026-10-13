//! **The heights of a surface under a plan position** — a vertical line
//! dropped on it, read exactly: a triangle is linear, a quadrilateral bilinear,
//! and nothing else is assumed.
//!
//! Moved out of `thickness` when `project` needed the same question
//! answered — *what is the height of this surface under this (x, y)?* — and a
//! second answer to it would have been a second place for the edges, the
//! folds and the degenerate faces to be read differently.
//!
//! # Three answers, and they are not the same news
//!
//! - **at** a height;
//! - **outside**: no face lies under the position. Extrapolating past the edge
//!   would put a height where nobody picked a surface;
//! - **folded**: several faces lie under it and disagree — a reverse fault, an
//!   overturned fold. Which one is *the* surface there is a geological
//!   decision, not a numerical one.
//!
//! Faces that cannot be interpolated — a corner that is not a finite point, no
//! area in plan (a vertical face says nothing about *one* height), a
//! quadrilateral that is not convex in plan — are **set aside** and counted.

use crate::Error;

/// A surface as flat arrays: `[x, y, z, …]`, and its faces as node indices —
/// triangles by three, quadrilaterals by four, in ring order.
///
/// Both, because a 2D grid's cells are quadrilaterals and halving them would
/// be a decision: the height at the centre of a cell is the mean of its four
/// corners bilinearly, and one of two different values when cut in two.
#[derive(Clone, Copy, Debug)]
pub struct Surface<'a> {
    pub points: &'a [f64],
    pub triangles: &'a [u32],
    pub quads: &'a [u32],
}

/// How far outside a face, in its own coordinates, still counts as on it —
/// what lets a node on an edge shared by two faces be found by either.
const ON_EDGE: f64 = 1e-9;

/// The height under a plan position — see the module documentation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Height {
    /// No face lies under it.
    Outside,
    /// Several faces lie under it, and disagree.
    Folded,
    /// The height there.
    At(f64),
}

/// A surface, indexed to answer [`Height`]s.
pub struct Heights<'a> {
    points: &'a [f64],
    index: Index,
}

impl<'a> Heights<'a> {
    /// The surface indexed — refused when its arrays are malformed, or when it
    /// has no face to read a height from; `which` names the surface and `how`
    /// what its heights were wanted for, in that refusal.
    pub fn of(surface: Surface<'a>, which: &'static str, how: &'static str) -> Result<Heights<'a>, Error> {
        check(&surface)?;
        let index = Index::of(&surface);
        if index.faces.is_empty() {
            // Faces that are all set aside are not *no face*: the answer would
            // be a hole everywhere, which reads as a position outside it.
            return Err(match index.set_aside {
                0 => Error::NoFace { which, how },
                set_aside => Error::NoUsableFace { which, set_aside },
            });
        }
        Ok(Heights {
            points: surface.points,
            index,
        })
    }

    /// The height under `(x, y)`.
    ///
    /// Faces that agree within a billionth of the surface's span are one: the
    /// same height read from both sides of a shared edge is not a fold.
    pub fn height(&self, x: f64, y: f64) -> Height {
        let heights = self.index.heights_at(self.points, x, y);
        let low = heights.iter().copied().fold(f64::INFINITY, f64::min);
        let high = heights.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if heights.is_empty() {
            Height::Outside
        } else if high - low > 1e-9 * self.index.scale {
            Height::Folded
        } else {
            Height::At(low)
        }
    }

    /// The faces set aside.
    pub fn set_aside(&self) -> usize {
        self.index.set_aside
    }
}

/// The shape of the arrays, before anything is read from them.
pub(crate) fn check(surface: &Surface) -> Result<(), Error> {
    if !surface.points.len().is_multiple_of(3) {
        return Err(Error::NotTriples {
            given: surface.points.len(),
        });
    }
    if !surface.triangles.len().is_multiple_of(3) {
        return Err(Error::NotTriangles {
            given: surface.triangles.len(),
        });
    }
    if !surface.quads.len().is_multiple_of(4) {
        return Err(Error::NotQuads {
            given: surface.quads.len(),
        });
    }
    let nodes = surface.points.len() / 3;
    if let Some(&at) = surface
        .triangles
        .iter()
        .chain(surface.quads)
        .find(|&&i| i as usize >= nodes)
    {
        return Err(Error::NodeOutOfRange {
            at: at as usize,
            nodes,
        });
    }
    Ok(())
}

/// A face of the base that can be interpolated.
enum Face {
    Triangle([usize; 3]),
    Quad([usize; 4]),
}

/// The base's faces, bucketed by their plan-view boxes on a regular grid.
struct Index {
    faces: Vec<Face>,
    /// Face numbers per cell, row after row.
    cells: Vec<Vec<usize>>,
    min: [f64; 2],
    size: [f64; 2],
    counts: [usize; 2],
    /// The largest span of the base on any axis, at least one — what the
    /// agreement between two faces is measured against.
    scale: f64,
    set_aside: usize,
}

impl Index {
    fn of(base: &Surface) -> Index {
        let point = |i: usize| [base.points[3 * i], base.points[3 * i + 1], base.points[3 * i + 2]];
        let finite = |corners: &[usize]| corners.iter().all(|&i| point(i).iter().all(|c| c.is_finite()));
        let mut faces = Vec::new();
        let mut set_aside = 0usize;
        for t in base.triangles.chunks_exact(3) {
            let corners = [t[0] as usize, t[1] as usize, t[2] as usize];
            let [a, b, c] = corners.map(point);
            if finite(&corners) && cross(a, b, c) != 0.0 {
                faces.push(Face::Triangle(corners));
            } else {
                set_aside += 1;
            }
        }
        for q in base.quads.chunks_exact(4) {
            let corners = [q[0] as usize, q[1] as usize, q[2] as usize, q[3] as usize];
            let [a, b, c, d] = corners.map(point);
            // Convex in plan, turning one way: the bilinear map is then one to
            // one over the cell, and its inverse is the height's only answer.
            let turns = [cross(a, b, c), cross(b, c, d), cross(c, d, a), cross(d, a, b)];
            let convex = turns.iter().all(|t| *t > 0.0) || turns.iter().all(|t| *t < 0.0);
            if finite(&corners) && convex {
                faces.push(Face::Quad(corners));
            } else {
                set_aside += 1;
            }
        }

        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        let mut boxes = Vec::with_capacity(faces.len());
        for face in &faces {
            let corners: &[usize] = match face {
                Face::Triangle(c) => c,
                Face::Quad(c) => c,
            };
            let mut low = [f64::INFINITY; 2];
            let mut high = [f64::NEG_INFINITY; 2];
            for &i in corners {
                let p = point(i);
                for axis in 0..3 {
                    min[axis] = min[axis].min(p[axis]);
                    max[axis] = max[axis].max(p[axis]);
                }
                for axis in 0..2 {
                    low[axis] = low[axis].min(p[axis]);
                    high[axis] = high[axis].max(p[axis]);
                }
            }
            boxes.push((low, high));
        }
        let scale = (0..3)
            .map(|a| max[a] - min[a])
            .filter(|s| s.is_finite())
            .fold(1.0f64, f64::max);
        if faces.is_empty() {
            return Index {
                faces,
                cells: Vec::new(),
                min: [0.0; 2],
                size: [1.0; 2],
                counts: [0; 2],
                scale,
                set_aside,
            };
        }
        // About one face per cell, the cells as square as the extent allows.
        let span = [
            (max[0] - min[0]).max(scale * 1e-12),
            (max[1] - min[1]).max(scale * 1e-12),
        ];
        let per_axis = |mine: f64, other: f64| {
            ((faces.len() as f64 * mine / other).sqrt().ceil() as usize).clamp(1, 1024)
        };
        let counts = [per_axis(span[0], span[1]), per_axis(span[1], span[0])];
        let size = [span[0] / counts[0] as f64, span[1] / counts[1] as f64];
        let mut index = Index {
            faces,
            cells: vec![Vec::new(); counts[0] * counts[1]],
            min: [min[0], min[1]],
            size,
            counts,
            scale,
            set_aside,
        };
        for (at, (low, high)) in boxes.into_iter().enumerate() {
            let (i0, j0) = index.cell_of(low[0], low[1]);
            let (i1, j1) = index.cell_of(high[0], high[1]);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    index.cells[j * counts[0] + i].push(at);
                }
            }
        }
        index
    }

    /// The cell a plan position falls in, clamped to the grid.
    fn cell_of(&self, x: f64, y: f64) -> (usize, usize) {
        let at = |v: f64, axis: usize| {
            (((v - self.min[axis]) / self.size[axis]).floor().max(0.0) as usize).min(self.counts[axis] - 1)
        };
        (at(x, 0), at(y, 1))
    }

    /// The heights of every face lying under `(x, y)`.
    fn heights_at(&self, points: &[f64], x: f64, y: f64) -> Vec<f64> {
        if self.faces.is_empty() {
            return Vec::new();
        }
        // A position past the grid lands in its edge cell, and the faces
        // there say no: the face test is the only one that decides.
        let point = |i: usize| [points[3 * i], points[3 * i + 1], points[3 * i + 2]];
        let (i, j) = self.cell_of(x, y);
        self.cells[j * self.counts[0] + i]
            .iter()
            .filter_map(|&at| match &self.faces[at] {
                Face::Triangle(c) => in_triangle(c.map(point), x, y),
                Face::Quad(c) => in_quad(c.map(point), x, y),
            })
            .collect()
    }
}

/// The plan-view cross product of `b − a` and `c − a` — twice the signed area.
fn cross(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// The height of the triangle at `(x, y)`, if it lies over that position.
fn in_triangle([a, b, c]: [[f64; 3]; 3], x: f64, y: f64) -> Option<f64> {
    let q = [x, y, 0.0];
    let whole = cross(a, b, c);
    let la = cross(q, b, c) / whole;
    let lb = cross(a, q, c) / whole;
    let lc = 1.0 - la - lb;
    (la >= -ON_EDGE && lb >= -ON_EDGE && lc >= -ON_EDGE).then(|| la * a[2] + lb * b[2] + lc * c[2])
}

/// The height of the quadrilateral at `(x, y)` — bilinear in its own `(u, v)`
/// — if it lies over that position.
///
/// The inverse of the bilinear map by Newton from the cell's centre. For a
/// convex cell the map is one to one and the iteration settles in a few
/// steps; one that does not settle is not claimed as a hit.
///
/// **Relative to the cell's first corner.** In map coordinates — a northing of
/// 6.86e6 m — the rounding of the map alone is ~1e-9 m, a step that never
/// settles under 1e-13 of a cell, and every position read as outside
/// (`a_cell_far_from_the_origin_answers_its_height`). Moved to the corner, the
/// cell's own size is what the rounding is measured against.
fn in_quad([a, b, c, d]: [[f64; 3]; 4], x: f64, y: f64) -> Option<f64> {
    let origin = [a[0], a[1], 0.0];
    let local = |p: [f64; 3]| [p[0] - origin[0], p[1] - origin[1], p[2]];
    let (a, b, c, d) = (local(a), local(b), local(c), local(d));
    let (x, y) = (x - origin[0], y - origin[1]);
    let at = |u: f64, v: f64, k: usize| {
        (1.0 - u) * (1.0 - v) * a[k] + u * (1.0 - v) * b[k] + u * v * c[k] + (1.0 - u) * v * d[k]
    };
    let (mut u, mut v) = (0.5, 0.5);
    for _ in 0..30 {
        let f = [at(u, v, 0) - x, at(u, v, 1) - y];
        let du = [
            (1.0 - v) * (b[0] - a[0]) + v * (c[0] - d[0]),
            (1.0 - v) * (b[1] - a[1]) + v * (c[1] - d[1]),
        ];
        let dv = [
            (1.0 - u) * (d[0] - a[0]) + u * (c[0] - b[0]),
            (1.0 - u) * (d[1] - a[1]) + u * (c[1] - b[1]),
        ];
        let det = du[0] * dv[1] - du[1] * dv[0];
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        let step_u = (f[0] * dv[1] - f[1] * dv[0]) / det;
        let step_v = (du[0] * f[1] - du[1] * f[0]) / det;
        u -= step_u;
        v -= step_v;
        if step_u.abs() < 1e-13 && step_v.abs() < 1e-13 {
            let inside = (-ON_EDGE..=1.0 + ON_EDGE).contains(&u) && (-ON_EDGE..=1.0 + ON_EDGE).contains(&v);
            return inside.then(|| at(u, v, 2));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A cell read at the coordinates a real grid has.** Every test of this
    /// module ran near the origin, where the inverse of a cell's bilinear map
    /// settles to 1e-13 of a cell. In Lambert-93 a northing is 6.86e6 m, and
    /// the rounding of the map alone is ~1e-9 m — a step of ~1e-11 of a 25 m
    /// cell that never goes under the threshold: every position of the grid
    /// read as *outside*. Found when `well-from-plan` read the
    /// ground of Lutetia under a head squarely
    /// inside it; `project` and `thickness` read heights the same way.
    #[test]
    fn a_cell_far_from_the_origin_answers_its_height() {
        let (x0, y0) = (650_266.553_564_973_3, 6_860_917.669_104_679_5);
        let points = [
            x0,
            y0,
            10.0, //
            x0 + 25.0,
            y0,
            20.0, //
            x0 + 25.0,
            y0 + 25.0,
            30.0, //
            x0,
            y0 + 25.0,
            20.0,
        ];
        let quads = [0u32, 1, 2, 3];
        let surface = Surface {
            points: &points,
            triangles: &[],
            quads: &quads,
        };
        let heights = Heights::of(surface, "surface", "in a test").expect("one cell");
        match heights.height(x0 + 12.5, y0 + 12.5) {
            Height::At(z) => assert!((z - 20.0).abs() < 1e-6, "{z}"),
            other => panic!("the centre of the cell is not outside it: {other:?}"),
        }
        match heights.height(x0 + 5.0, y0 + 20.0) {
            Height::At(z) => assert!(
                (z - (10.0 + 0.2 * 10.0 + 0.8 * 10.0 + 0.2 * 0.8 * 0.0)).abs() < 1e-6,
                "{z}"
            ),
            other => panic!("inside the cell: {other:?}"),
        }
        assert!(matches!(heights.height(x0 + 30.0, y0 + 12.5), Height::Outside));
    }
}
