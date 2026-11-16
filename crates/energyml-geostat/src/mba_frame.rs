//! The tilted local frame for a surface MBA cannot fit in world coordinates -
//! a mean plane, a rotation to put it horizontal, MBA in that frame, the
//! inverse rotation back. The need: do the same for tilted surfaces (such as
//! faults) by finding a mean plane, rotating it to horizontal, applying MBA,
//! then applying the inverse rotation.
//!
//! # Why a fault needs this and a gentle horizon does not
//!
//! [`crate::mba::Mba`] approximates a **function** `w = f(u, v)`: one height
//! per plan-view position. A steep fault is not one in the world frame - seen
//! from above, it collapses into a narrow band where two points almost
//! coincident in `(x, y)` carry heights hundreds of metres apart. Fitted in
//! `(u, v)`, the same fault is close to flat and is a function again.
//!
//! # What this module is, and what it deliberately is not
//!
//! [`crate::mba::Mba`] itself does not change: it fits and evaluates whatever
//! 2D coordinates it is given, and knows nothing of a "frame" - see its
//! module doc. This module only builds the `(u, v, w)` basis and the two
//! transforms; a caller runs `Mba::interpolate` on the samples' *local*
//! coordinates and rotates the answer back. That caller is
//! `crates/energyml-build`'s MBA operations, not this crate - this module
//! stays as free of package knowledge as `Mba` itself.
//!
//! # The frame
//!
//! 1. **The mean plane**: [`crate::extend::fit_plane`], already written for
//!    `extend` (weighted PCA, Huber reweighting, named outliers). Reused
//!    rather than refitted: a second plane fit in the repository would be a
//!    second answer to the same question. It gives a centre `c` and a normal
//!    `n` (the least-variance axis).
//! 2. **The orthonormal local basis** `(u, v, w)`, origin `c`:
//!    - `w = n`, oriented up (`n · z ≥ 0`) - otherwise the same fault, fitted
//!      twice, could come out flipped;
//!    - `u` = the plane's own strike direction (horizontal: `z × n`,
//!      normalised), `v = w × u` = the line of steepest dip. Not arbitrary:
//!      it keeps "along the strike" and "along the dip" - `extend`'s own
//!      vocabulary (`Reach::Dip`: `up`, `down`, `sides`) - readable in the
//!      result;
//!    - a **horizontal** plane (`z × n` is zero) has no strike: `u = x`,
//!      `v = y`, `w = z` - the rotation is the identity, and the tilted path
//!      collapses onto the plain world-frame fit (tested below).
//! 3. **The rotation** `R` (rows `u`, `v`, `w`): a point `p` becomes
//!    `R (p - c)`, i.e. `(u, v, w)`.
//! 4. MBA runs on `(u, v) → w` - the untouched core of `mba.rs`.
//! 5. **The inverse rotation**: `p = c + Rᵀ (u, v, w)`. `R` is orthonormal, so
//!    its inverse is its transpose - no numerical inversion.
//!
//! # What is refused
//!
//! - **fewer than three points, or all collinear**: no plane to fit -
//!   [`Error::NoPlane`], which is exactly what [`crate::extend::fit_plane`]
//!   already reports as `None`;
//! - **a surface that is not a function above its own mean plane** - a
//!   recumbent fold, a very curved listric fault: two samples at the same
//!   `(u, v)` with different `w`. No threshold read off a point cloud detects
//!   this reliably; when the input is a triangulated surface, its own face
//!   orientations say it directly - a face whose projected orientation in
//!   `(u, v)` is the opposite of the majority is folded back, and the
//!   projection would overlay it on itself. The count does not depend on the
//!   triangles' winding. [`folded_faces`] counts them, and
//!   [`check_not_folded`] is the named refusal built on that count
//!   (see below).
//!
//! Nothing here decides *when* to use this frame instead of the world one -
//! there is no automatic switch. The caller's `frame` parameter, defaulting to
//! the world frame, carries that choice, never the grid target of a plain
//! interpolation.
//!
//! Also decided: **one mean plane fits the whole shape** -
//! a caller with a listric, strongly-curved fault that [`check_not_folded`]
//! refuses is past what a first version answers; piecewise mean planes
//! stitched together are a later increment, not attempted here.

use crate::vector::{add, cross, dot, norm, scale, sub, unit_above};

/// `a`, normalised - `None` when it is too close to zero to have a direction.
fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    unit_above(a, 1e-12)
}

/// What this module refuses, by name.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum Error {
    #[error(
        "no plane could be fitted: {given} point(s), and a mean plane needs at least 3 that are not all on one line"
    )]
    NoPlane { given: usize },
    #[error(
        "{folded} of {total} face(s) are folded back under the mean plane - the surface is not a function of (u, v) there, so a single mean plane cannot resurface it"
    )]
    Folded { folded: usize, total: usize },
}

/// An orthonormal local frame - a mean plane's own `(u, v, w)`, with `w` the
/// plane's normal oriented up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// The plane's weighted centre, in world coordinates - the frame's origin.
    pub centre: [f64; 3],
    /// The strike direction - horizontal, `z × w` normalised.
    pub u: [f64; 3],
    /// The dip direction - `w × u`.
    pub v: [f64; 3],
    /// The plane's normal, oriented so that `w · z ≥ 0`.
    pub w: [f64; 3],
}

impl Frame {
    /// The identity frame: `u = x`, `v = y`, `w = z`, centred on the origin -
    /// what a horizontal mean plane collapses onto (see the module docs)
    /// and the `world` frame parameter's own basis.
    pub fn horizontal() -> Self {
        Frame {
            centre: [0.0, 0.0, 0.0],
            u: [1.0, 0.0, 0.0],
            v: [0.0, 1.0, 0.0],
            w: [0.0, 0.0, 1.0],
        }
    }

    /// The mean-plane frame of `points` - [`crate::extend::fit_plane`],
    /// reused rather than refitted (module doc).
    pub fn fit(points: &[[f64; 3]]) -> Result<Self, Error> {
        let Some(fitted) = crate::extend::fit_plane(points) else {
            return Err(Error::NoPlane { given: points.len() });
        };
        // Oriented up: a fault fitted twice (its points given in the reverse
        // order, say) must come out the same way, never flipped.
        let mut w = fitted.axis;
        // A (near) vertical plane has `w.z` at the noise of the eigen-solver,
        // so its sign decides nothing: the first horizontal component above
        // the noise is made positive instead, a tie-break that depends on the
        // geometry alone and gives the same frame on every fit of the same
        // fault.
        let flip = if w[2].abs() < VERTICAL {
            match [w[0], w[1]].into_iter().find(|c| c.abs() > VERTICAL) {
                Some(first) => first < 0.0,
                None => w[2] < 0.0,
            }
        } else {
            w[2] < 0.0
        };
        if flip {
            w = scale(w, -1.0);
        }
        let z_axis = [0.0, 0.0, 1.0];
        let cross_zn = cross(z_axis, w);
        // A dedicated, looser tolerance than `unit`'s generic zero-vector
        // guard: `w` comes out of an eigendecomposition, whose numerical
        // noise on a plane that is *meant* to be exactly horizontal can sit
        // a few orders of magnitude above `unit`'s 1e-12, and a stray, tiny
        // `u` derived from that noise would rotate the grid for no geometric
        // reason (the horizontal test below is precisely what would catch
        // that regression).
        let (u, v) = if norm(cross_zn) < 1e-9 {
            // A horizontal plane (`z × w` is the zero vector) has no strike:
            // the identity, so the tilted path collapses onto the world one.
            ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
        } else {
            let u = unit(cross_zn).expect("checked non-zero above");
            (u, cross(w, u))
        };
        Ok(Frame {
            centre: fitted.centre,
            u,
            v,
            w,
        })
    }

    /// `p`, expressed as `(u, v, w)` about the frame's centre.
    pub fn to_local(&self, p: [f64; 3]) -> [f64; 3] {
        let d = sub(p, self.centre);
        [dot(d, self.u), dot(d, self.v), dot(d, self.w)]
    }

    /// The inverse of [`Self::to_local`] - `p = c + Rᵀ (u, v, w)`. `R` is
    /// orthonormal by construction, so this is exact, no numerical inversion.
    pub fn to_world(&self, local: [f64; 3]) -> [f64; 3] {
        add(
            self.centre,
            add(
                scale(self.u, local[0]),
                add(scale(self.v, local[1]), scale(self.w, local[2])),
            ),
        )
    }
}

/// Below this, a component of the fitted normal is eigen-solver noise: the
/// plane is vertical for the purpose of orienting `w`.
const VERTICAL: f64 = 1e-9;

/// The count of triangle faces folded back against the others once projected
/// to `(u, v)` - the fold-back the projection cannot represent as a function
/// (module doc).
///
/// The test does **not** depend on the triangles' winding. Each face's
/// projected signed area has the sign of `normal . frame.w`; the faces with
/// the minority sign are the folded ones (`min(positive, negative)`), so a
/// consistently wound mesh is never folded whichever way it is wound, and a
/// mesh wound both ways is folded by exactly the faces that disagree with the
/// majority. Faces of no area (a collinear triangle) take no side. A
/// non-finite corner, or a corner index past `points`, is skipped rather than
/// counted: it is a different refusal's to name.
pub fn folded_faces(points: &[[f64; 3]], triangles: &[[u32; 3]], frame: &Frame) -> usize {
    let (mut positive, mut negative) = (0usize, 0usize);
    for &[a, b, c] in triangles {
        let (Some(&pa), Some(&pb), Some(&pc)) = (
            points.get(a as usize),
            points.get(b as usize),
            points.get(c as usize),
        ) else {
            continue;
        };
        if [pa, pb, pc].iter().flatten().any(|v| !v.is_finite()) {
            continue;
        }
        let normal = cross(sub(pb, pa), sub(pc, pa));
        let side = dot(normal, frame.w);
        if side > 0.0 {
            positive += 1;
        } else if side < 0.0 {
            negative += 1;
        }
    }
    positive.min(negative)
}

/// The named refusal built on [`folded_faces`]: `Ok` when none are, the count
/// and the total otherwise.
pub fn check_not_folded(points: &[[f64; 3]], triangles: &[[u32; 3]], frame: &Frame) -> Result<(), Error> {
    let folded = folded_faces(points, triangles, frame);
    if folded > 0 {
        Err(Error::Folded {
            folded,
            total: triangles.len(),
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mba::{Domain, Mba};

    /// A patch of points exactly on the plane through the origin with normal
    /// `normal` - the test fixture for the round-trip and reproduction tests,
    /// not a claim about any real fault's shape.
    fn plane_points(normal: [f64; 3]) -> Vec<[f64; 3]> {
        let n = unit(normal).unwrap();
        let helper = if n[2].abs() < 0.9 {
            [0.0, 0.0, 1.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        let e1 = unit(cross(helper, n)).unwrap();
        let e2 = cross(n, e1);
        let mut points = Vec::new();
        for i in -3..=3 {
            for j in -3..=3 {
                points.push(add(scale(e1, i as f64), scale(e2, j as f64)));
            }
        }
        points
    }

    /// **The rotation round trip**: `Rᵀ R = I` (tested through
    /// `to_world(to_local(p)) == p`) on planes of varied dips and azimuths.
    /// To break: forget the transpose (use `R` instead of `Rᵀ` in
    /// `to_world`) and every non-horizontal case would diverge.
    #[test]
    fn round_trip_returns_the_original_point() {
        for normal in [
            [0.0, 0.0, 1.0],
            [0.3, 0.0, 1.0],
            [0.0, 0.6, 1.0],
            [1.0, 1.0, 0.3],
            [-0.5, 0.8, 0.2],
        ] {
            let points = plane_points(normal);
            let frame = Frame::fit(&points).unwrap();
            for &p in &points {
                let local = frame.to_local(p);
                let back = frame.to_world(local);
                for k in 0..3 {
                    assert!(
                        (back[k] - p[k]).abs() < 1e-9,
                        "normal {normal:?}: {back:?} != {p:?}"
                    );
                }
            }
            // A point off the sample grid, to make sure the round trip is not
            // an artefact of the fitted points themselves.
            let off_grid = add(points[0], [0.37, -1.21, 0.0]);
            let local = frame.to_local(off_grid);
            let back = frame.to_world(local);
            for k in 0..3 {
                assert!(
                    (back[k] - off_grid[k]).abs() < 1e-9,
                    "normal {normal:?}, off-grid point"
                );
            }
        }
    }

    /// **A horizontal surface**: `meanPlane` (this module) and `world` (the
    /// identity) give the same basis, up to floating-point noise.
    /// To break: pick an arbitrary `u` (e.g. the first edge of the cloud) for a
    /// horizontal plane and the grid would turn for no reason.
    #[test]
    fn a_horizontal_plane_collapses_onto_the_world_frame() {
        let points = plane_points([0.0, 0.0, 1.0]);
        let frame = Frame::fit(&points).unwrap();
        let world = Frame::horizontal();
        for (got, want) in [(frame.u, world.u), (frame.v, world.v), (frame.w, world.w)] {
            for k in 0..3 {
                assert!((got[k] - want[k]).abs() < 1e-9, "{got:?} != {want:?}");
            }
        }
    }

    /// **The orientation of `n`**: the same fault, its points given in reverse
    /// order, comes out with the same normal. To break: do not orient `n`
    /// upward (remove the `if w[2] < 0.0` in `Frame::fit`) and the result
    /// would depend on the input order whenever the PCA picks the opposite
    /// sign on the reversed order.
    #[test]
    fn the_normal_is_oriented_the_same_way_whatever_the_points_order() {
        let points = plane_points([0.4, -0.3, 1.0]);
        let forward = Frame::fit(&points).unwrap();
        let reversed: Vec<[f64; 3]> = points.iter().rev().copied().collect();
        let backward = Frame::fit(&reversed).unwrap();
        for k in 0..3 {
            assert!(
                (forward.w[k] - backward.w[k]).abs() < 1e-9,
                "{:?} != {:?}",
                forward.w,
                backward.w
            );
        }
    }

    /// **Fewer than three non-aligned points**: a named refusal, the one
    /// `fit_plane` already returns as `None`.
    #[test]
    fn too_few_or_aligned_points_are_refused() {
        assert_eq!(
            Frame::fit(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]),
            Err(Error::NoPlane { given: 2 })
        );
        // Three points, all on one line: no plane either.
        let aligned = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        assert_eq!(Frame::fit(&aligned), Err(Error::NoPlane { given: 3 }));
    }

    /// **A plane tilted by 60° is reproduced exactly**: in its local frame,
    /// `w = 0` everywhere - MBA reproduces a constant there (`mba.rs`'s own
    /// `a_constant_field_is_reproduced_exactly_even_away_from_samples`) - and,
    /// after the inverse rotation, every evaluated node falls back on the
    /// original plane. To break: drop the inverse rotation (return the local
    /// `(u, v, w)` directly) and every point would collapse onto the world
    /// plane `w = 0` instead of the tilted one.
    #[test]
    fn a_plane_tilted_60_degrees_is_reproduced_through_mba_and_back() {
        // dip 60°: the normal's angle from vertical is 60°, i.e.
        // n_z = cos(60°) = 0.5, horizontal component sin(60°) ≈ 0.866.
        let normal = [60.0f64.to_radians().sin(), 0.0, 60.0f64.to_radians().cos()];
        let points = plane_points(normal);
        let frame = Frame::fit(&points).unwrap();
        let local: Vec<[f64; 3]> = points.iter().map(|&p| frame.to_local(p)).collect();
        for p in &local {
            assert!(
                p[2].abs() < 1e-9,
                "a point of the fitted plane has zero local height: {p:?}"
            );
        }
        let samples: Vec<[f64; 2]> = local.iter().map(|p| [p[0], p[1]]).collect();
        let values: Vec<f64> = local.iter().map(|p| p[2]).collect();
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for s in &samples {
            for k in 0..2 {
                lo[k] = lo[k].min(s[k]);
                hi[k] = hi[k].max(s[k]);
            }
        }
        let domain = Domain {
            origin: [lo[0] - 1.0, lo[1] - 1.0],
            size: [hi[0] - lo[0] + 2.0, hi[1] - lo[1] + 2.0],
        };
        let mba = Mba::new([1, 1]).unwrap();
        let answer = mba.interpolate(&samples, &values, domain, 1.0, &samples).unwrap();
        for (at, &w) in answer.values.iter().enumerate() {
            let world = frame.to_world([samples[at][0], samples[at][1], w]);
            let want = points[at];
            for k in 0..3 {
                assert!((world[k] - want[k]).abs() < 1e-6, "{world:?} != {want:?}");
            }
        }
    }

    /// [geostat-proj-roundtrip S4] The fold count does not depend on the
    /// winding: a mesh wound the same way is never folded, whichever way that
    /// is, and a mixed mesh is folded by its minority only.
    #[test]
    fn the_fold_count_does_not_depend_on_the_winding() {
        let frame = Frame::horizontal();
        let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]];
        let up = [[0u32, 1, 2], [1, 3, 2]];
        let down = [[0u32, 2, 1], [1, 2, 3]];
        assert_eq!(folded_faces(&points, &up, &frame), 0);
        assert_eq!(
            folded_faces(&points, &down, &frame),
            0,
            "all faces wound downward"
        );
        assert_eq!(check_not_folded(&points, &down, &frame), Ok(()));
        // Three up and one down: one folded.
        let points = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
            [2.0, 0.0, 0.0],
        ];
        let mixed = [[0u32, 1, 2], [1, 3, 2], [1, 4, 3], [0, 2, 1]];
        assert_eq!(folded_faces(&points, &mixed, &frame), 1);
        // Break it: count `dot(normal, frame.w) < 0.0` faces instead of the minority side.
    }

    /// [geostat-proj-roundtrip S4] A vertical plane has no up: its normal is
    /// oriented by its first horizontal component, whatever the order of the
    /// points (and so whatever sign the eigen-solver returns).
    #[test]
    fn a_vertical_plane_is_oriented_by_its_first_horizontal_component() {
        for normal in [
            [0.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0],
        ] {
            let points = plane_points(normal);
            let reversed: Vec<[f64; 3]> = points.iter().rev().copied().collect();
            for set in [&points, &reversed] {
                let frame = Frame::fit(set).unwrap();
                let first = [frame.w[0], frame.w[1]]
                    .into_iter()
                    .find(|c| c.abs() > 1e-9)
                    .unwrap();
                assert!(first > 0.0, "normal {normal:?}: w = {:?}", frame.w);
            }
        }
        // Break it: orient `w` by `w[2] < 0.0` alone, as before.
    }

    /// **A folded triangulated surface is refused, naming the number of flipped
    /// faces.** A triangle whose winding gives a normal opposite to `frame.w`
    /// counts as folded - checked by hand (cross product) rather than through
    /// `Frame::fit`, to isolate what `folded_faces` does from what `fit_plane`
    /// returns. To break: turn the `< 0.0` test into `> 0.0` and it would count
    /// the face that is not folded and not the one that is.
    #[test]
    fn a_folded_triangle_is_counted_against_the_mean_plane() {
        let frame = Frame::horizontal(); // w = [0, 0, 1]
        let points = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
        ];
        // Triangle 0: (0, 1, 2) winds counter-clockwise seen from above -
        // normal [0, 0, 1], not folded.
        // Triangle 1: (0, 1, 3) winds clockwise seen from above - normal
        // [0, 0, -1], folded back under the mean plane.
        let triangles = [[0u32, 1, 2], [0, 1, 3]];
        assert_eq!(folded_faces(&points, &triangles, &frame), 1);
        assert_eq!(
            check_not_folded(&points, &triangles, &frame),
            Err(Error::Folded { folded: 1, total: 2 })
        );
        assert_eq!(check_not_folded(&points, &triangles[..1], &frame), Ok(()));
    }
}
