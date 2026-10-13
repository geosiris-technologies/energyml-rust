//! The thickness between two horizons, at every node of the upper one.
//!
//! # What is measured
//!
//! **Vertical** (the isochore, TVT): under each node of the top, the base is
//! found in plan view and its height read there; the thickness is the
//! difference, turned into *top above base* by the caller's [`Up`]. Exact for
//! the surfaces as stated — a triangle is linear, a quadrilateral bilinear —
//! and nothing else is assumed.
//!
//! **Stratigraphic** (the isopach, TST): the vertical thickness times the
//! cosine of the top's dip. That is the **parallel-beds** formula: exact when
//! the base runs parallel to the top, an approximation otherwise. The exact
//! measure — along the top's normal until it meets the base — is a ray cast,
//! and a ray caster is not part of this crate. Writing one here would be a
//! second one to replace later; the name of the method travels with the values
//! instead, so nobody reads the approximation as the exact thing.
//!
//! # What has no value, and why it is counted
//!
//! Every one of these is a `NaN` in `values` and a count beside it — the house
//! rule: a hole that is not counted reads as a surface that is complete.
//!
//! - **outside**: no face of the base lies under the node. Extrapolating the
//!   base past its edge would draw a thickness where nobody picked a horizon;
//! - **folded**: several faces lie under the node and disagree on its height —
//!   a reverse fault, an overturned fold, a surface that crosses itself. Which
//!   one is *the* base there is a geological decision, not a numerical one;
//! - **unplaced**: the node itself has no finite position — a hole of a grid;
//! - **undipped** (stratigraphic only): the top has no normal at the node.
//!
//! And one that **keeps** its value: **crossed**, where the base lies above
//! the top. A negative thickness is a finding — two picks that cross — and
//! setting it to zero or `NaN` would hide exactly where the interpretation
//! needs a second look.
//!
//! Base faces that cannot be interpolated — a corner that is not a finite
//! point, no area in plan (a vertical face says nothing about *one* height), a
//! quadrilateral that is not convex in plan — are **set aside** and counted.

use crate::Error;
use crate::heights::{Height, Heights, check};
use crate::surface::{self, Up};

pub use crate::heights::Surface;

/// Which thickness.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Measure {
    /// The isochore — true vertical thickness.
    Vertical,
    /// The isopach — true stratigraphic thickness, by the parallel-beds
    /// formula. See the module documentation.
    Stratigraphic,
}

/// The thickness at each node of the top, and what could not be measured.
#[derive(Clone, Debug, PartialEq)]
pub struct Thickness {
    /// One per node of the top, in its order; `NaN` where there is none.
    pub values: Vec<f64>,
    /// Nodes over no face of the base.
    pub outside: usize,
    /// Nodes over several faces of the base that disagree on its height.
    pub folded: usize,
    /// Nodes whose own position is not finite.
    pub unplaced: usize,
    /// Nodes where the top has no normal (stratigraphic only).
    pub undipped: usize,
    /// Nodes where the base lies above the top — **kept**, negative.
    pub crossed: usize,
    /// Base faces that cannot be interpolated.
    pub set_aside: usize,
}

/// The thickness from `top` down to `base`, at every node of `top`.
///
/// The top's faces are read only for [`Measure::Stratigraphic`], which needs
/// its dip; a vertical thickness is a question about positions, so a top
/// given as bare points is enough there.
pub fn thickness(top: &Surface, base: &Surface, up: Up, measure: Measure) -> Result<Thickness, Error> {
    check(top)?;
    let heights = Heights::of(*base, "base", "under each node of the top")?;
    let sign = up.sign();
    let nodes = top.points.len() / 3;
    let mut out = Thickness {
        values: vec![f64::NAN; nodes],
        outside: 0,
        folded: 0,
        unplaced: 0,
        undipped: 0,
        crossed: 0,
        set_aside: heights.set_aside(),
    };
    for (at, p) in top.points.chunks_exact(3).enumerate() {
        if !p.iter().all(|c| c.is_finite()) {
            out.unplaced += 1;
            continue;
        }
        match heights.height(p[0], p[1]) {
            Height::Outside => out.outside += 1,
            Height::Folded => out.folded += 1,
            Height::At(z) => out.values[at] = (p[2] - z) * sign,
        }
    }

    if measure == Measure::Stratigraphic {
        let mut triangles = top.triangles.to_vec();
        // The dip of a quadrilateral top, read on its two halves: the normal at
        // a node is the area-weighted mean of the faces around it, so which
        // diagonal is cut moves it only where the cell is warped.
        for q in top.quads.chunks_exact(4) {
            triangles.extend_from_slice(&[q[0], q[1], q[2], q[0], q[2], q[3]]);
        }
        if triangles.is_empty() {
            return Err(Error::NoFace { which: "top", how: "for its dip, which a stratigraphic thickness needs" });
        }
        let slope = surface::slope(top.points, &triangles, up)?;
        for (value, dip) in out.values.iter_mut().zip(&slope.dip) {
            if value.is_nan() {
                continue;
            }
            if dip.is_finite() {
                *value *= dip.to_radians().cos();
            } else {
                *value = f64::NAN;
                out.undipped += 1;
            }
        }
    }
    out.crossed = out.values.iter().filter(|v| **v < 0.0).count();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A square of side `side` at the origin, split into two triangles, its
    /// height `z(x, y)`.
    fn plane(side: f64, z: impl Fn(f64, f64) -> f64) -> (Vec<f64>, Vec<u32>) {
        let corners = [[0.0, 0.0], [side, 0.0], [side, side], [0.0, side]];
        let points = corners.iter().flat_map(|&[x, y]| [x, y, z(x, y)]).collect();
        (points, vec![0, 1, 2, 0, 2, 3])
    }

    fn triangles<'a>(points: &'a [f64], triangles: &'a [u32]) -> Surface<'a> {
        Surface { points, triangles, quads: &[] }
    }

    /// A top with a node at each of these plan positions, on `z(x, y)`, and
    /// the fan of triangles of a regular grid over them — for the dip.
    fn grid_top(n: usize, step: f64, z: impl Fn(f64, f64) -> f64) -> (Vec<f64>, Vec<u32>) {
        let mut points = Vec::new();
        for j in 0..n {
            for i in 0..n {
                let (x, y) = (1.0 + i as f64 * step, 1.0 + j as f64 * step);
                points.extend([x, y, z(x, y)]);
            }
        }
        let mut tris = Vec::new();
        for j in 0..n - 1 {
            for i in 0..n - 1 {
                let k = (j * n + i) as u32;
                let n = n as u32;
                tris.extend([k, k + 1, k + n + 1, k, k + n + 1, k + n]);
            }
        }
        (points, tris)
    }

    /// **Parallel planes, dipping**: the isochore is the vertical gap
    /// everywhere, the isopach that gap times the cosine of the dip — the case
    /// where the parallel-beds formula is exact.
    #[test]
    fn two_parallel_planes_give_the_vertical_gap_and_its_cosine() {
        let slope = 0.1;
        let (base_points, base_tris) = plane(100.0, |x, _| 80.0 + slope * x);
        let (top_points, top_tris) = grid_top(6, 15.0, |x, _| 100.0 + slope * x);
        let top = triangles(&top_points, &top_tris);
        let base = triangles(&base_points, &base_tris);

        let vertical = thickness(&top, &base, Up::PlusZ, Measure::Vertical).unwrap();
        assert!(vertical.values.iter().all(|v| (v - 20.0).abs() < 1e-9), "{:?}", vertical.values);
        assert_eq!((vertical.outside, vertical.folded, vertical.crossed), (0, 0, 0));

        let strati = thickness(&top, &base, Up::PlusZ, Measure::Stratigraphic).unwrap();
        let expected = 20.0 * slope.atan().cos();
        assert!(strati.values.iter().all(|v| (v - expected).abs() < 1e-9), "{:?} ≠ {expected}", strati.values);
    }

    /// **Which way is up turns the sign, and a negative is kept.** The same
    /// depths read as elevations put the base above the top: every value is
    /// negative, and every node is counted as crossed rather than blanked.
    #[test]
    fn the_vertical_decides_the_sign_and_a_crossing_is_kept() {
        let (base_points, base_tris) = plane(10.0, |_, _| 1020.0);
        let top_points = vec![5.0, 5.0, 1000.0, 2.0, 7.0, 1000.0];
        let base = triangles(&base_points, &base_tris);
        let top = Surface { points: &top_points, triangles: &[], quads: &[] };
        let depth = thickness(&top, &base, Up::MinusZ, Measure::Vertical).unwrap();
        assert_eq!(depth.values, vec![20.0, 20.0]);
        assert_eq!(depth.crossed, 0);
        let elevation = thickness(&top, &base, Up::PlusZ, Measure::Vertical).unwrap();
        assert_eq!(elevation.values, vec![-20.0, -20.0]);
        assert_eq!(elevation.crossed, 2);
    }

    /// Past the edge of the base, over a fold of it, at a hole of the top:
    /// three `NaN`, three counts, and the node that has a base keeps its value.
    #[test]
    fn outside_the_base_on_a_fold_and_on_a_hole_are_counted() {
        // Two squares, the second over the first's right half and 30 lower.
        let mut base_points = vec![0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 10.0, 10.0, 0.0, 0.0, 10.0, 0.0];
        base_points.extend([5.0, 0.0, -30.0, 10.0, 0.0, -30.0, 10.0, 10.0, -30.0, 5.0, 10.0, -30.0]);
        let base_tris = vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7];
        let top_points = vec![2.0, 5.0, 10.0, 7.0, 5.0, 10.0, 20.0, 5.0, 10.0, f64::NAN, 5.0, 10.0];
        let base = triangles(&base_points, &base_tris);
        let top = Surface { points: &top_points, triangles: &[], quads: &[] };
        let found = thickness(&top, &base, Up::PlusZ, Measure::Vertical).unwrap();
        assert_eq!(found.values[0], 10.0);
        assert!(found.values[1..].iter().all(|v| v.is_nan()));
        assert_eq!((found.folded, found.outside, found.unplaced), (1, 1, 1));
    }

    /// **A node on an edge shared by two faces is not a fold** — both faces
    /// find it, and they agree.
    #[test]
    fn a_node_on_a_shared_edge_is_not_a_fold() {
        let (base_points, base_tris) = plane(10.0, |x, y| 0.3 * x - 0.2 * y);
        let base = triangles(&base_points, &base_tris);
        // On the diagonal the two triangles share, and on a corner.
        let top_points = vec![4.0, 4.0, 50.0, 0.0, 0.0, 50.0];
        let top = Surface { points: &top_points, triangles: &[], quads: &[] };
        let found = thickness(&top, &base, Up::PlusZ, Measure::Vertical).unwrap();
        assert_eq!(found.folded, 0);
        assert!((found.values[0] - (50.0 - 0.4)).abs() < 1e-9 && (found.values[1] - 50.0).abs() < 1e-12);
    }

    /// **A grid cell is bilinear, not two triangles.** Over `z = x·y` on the
    /// unit square, the centre is 0.25 bilinearly — and 0 or 0.5 on either
    /// diagonal cut, which is what a split would have answered.
    #[test]
    fn a_grid_cell_reads_bilinearly() {
        let base_points = vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 1.0, 0.0];
        let base = Surface { points: &base_points, triangles: &[], quads: &[0, 1, 2, 3] };
        let top_points = vec![0.5, 0.5, 1.0, 0.25, 0.75, 1.0];
        let top = Surface { points: &top_points, triangles: &[], quads: &[] };
        let found = thickness(&top, &base, Up::PlusZ, Measure::Vertical).unwrap();
        assert!((found.values[0] - 0.75).abs() < 1e-12, "{:?}", found.values);
        assert!((found.values[1] - (1.0 - 0.25 * 0.75)).abs() < 1e-12, "{:?}", found.values);
    }

    /// A face that says nothing about *one* height — vertical, or with a `NaN`
    /// corner — is set aside and counted; the others still answer.
    #[test]
    fn a_vertical_or_holed_face_is_dropped_and_counted() {
        let mut base_points = vec![0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 10.0, 0.0];
        base_points.extend([0.0, 0.0, 5.0, f64::NAN, 3.0, 0.0]);
        // A wall along x = 0 (no area in plan), a triangle with a NaN corner.
        let base_tris = vec![0, 1, 2, 0, 3, 2, 0, 4, 1];
        let base = triangles(&base_points, &base_tris);
        let top_points = vec![1.0, 1.0, 3.0];
        let top = Surface { points: &top_points, triangles: &[], quads: &[] };
        let found = thickness(&top, &base, Up::PlusZ, Measure::Vertical).unwrap();
        assert_eq!(found.set_aside, 2);
        assert_eq!(found.values, vec![3.0]);
    }

    /// Refused by name: a base of points has nothing to read a height on, and
    /// an isopach of a top of points has no dip.
    #[test]
    fn without_a_face_the_refusal_names_the_surface() {
        let points = vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let bare = Surface { points: &points, triangles: &[], quads: &[] };
        let tris = [0u32, 1, 2];
        let faced = triangles(&points, &tris);
        let why = thickness(&faced, &bare, Up::PlusZ, Measure::Vertical).unwrap_err();
        assert!(why.to_string().contains("base"), "{why}");
        let why = thickness(&bare, &faced, Up::PlusZ, Measure::Stratigraphic).unwrap_err();
        assert!(why.to_string().contains("top"), "{why}");
    }
}
