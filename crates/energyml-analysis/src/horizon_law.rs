//! **A horizon laid by a law** over a reference surface — how the synthetic
//! model Lutetia states the horizons no borehole measures
//! (`horizon-from-law`; two of them are declared synthetic).
//!
//! # What a law is here
//!
//! A **thickness** between the reference and the new horizon, and the side of
//! the reference the new horizon lies on:
//!
//! - [`Law::Parallel`] — the same thickness everywhere: H7 under H6 at the
//!   thickness of the Campanian, H2 over the base of the Lutetian at the
//!   thickness of the Lutetian;
//! - [`Law::Linear`] — a thickness stated at an anchor, changing by `gradient`
//!   per unit of horizontal distance along `azimuth` (degrees clockwise from
//!   grid north, the convention of the `crs` operation's azimuth): a wedge,
//!   the way a formation thickens toward the centre of a basin.
//!
//! The thickness is laid **vertically** — an isochore, the measure the
//! `thickness` operation writes as `true vertical thickness` — because a node
//! of a grid carries one height per (x, y). Where the reference dips, the
//! thickness measured across the beds is the vertical one times the cosine of
//! the dip: the parallel-beds formula of [`crate::thickness()`], read the other
//! way. The caller says so; nothing here hides it.
//!
//! # Truncation
//!
//! A **cut** is an erosion surface the new horizon may not rise above: where
//! the laid height is above the cut, the horizon was eroded and **does not
//! exist** there. The form is the usage guide's, which the synthetic model follows for its
//! erosions (RESQML Technical
//! Usage Guide 2.2, draft, §7.2.2.2, table 5 p. 84): *« Horizon 2 | older side
//! | stops | Horizon 1 | both sides »* — the older horizon **stops** at the
//! erosion surface, it does not follow it.
//!
//! So an eroded node is a **hole**, never clamped onto the cut: a height taken
//! from the cut would be the erosion surface drawn a second time under the
//! older horizon's name — two picks of one surface, and one of them a lie.
//!
//! "Never rises above" is checked **at the nodes** of the new horizon, which is
//! what a grid states; between two nodes the two surfaces are each read their
//! own way (bilinear cells, linear triangles), and no claim is made there.
//!
//! # What each node answers, and what is counted
//!
//! Every hole is a `NaN` **and a count** — the house rule: a hole that is not
//! counted reads as a surface that is complete.
//!
//! - **laid**: the reference's height moved by the thickness, toward the side;
//! - **hole**: the reference has no finite position or height there — a `NaN`
//!   node of its grid. The law has nothing to stand on;
//! - **eroded**: a cut lies below the laid height — a hole;
//! - **folded**: a cut is folded over the node — several of its faces there
//!   disagree. Which sheet erodes is a geological decision, not a numerical
//!   one — a hole, counted apart from the eroded ones;
//! - and one that **keeps** its height and is counted: **beyond** the cuts —
//!   no cut has a face over or under the node, so the erosion surface says
//!   nothing there and nothing erodes. H1, the base of the alluvium, exists in
//!   the valley only; a node outside it is not eroded by it.
//!
//! # Refused by name
//!
//! A thickness that is not a finite number, 0 or more; a parallel law at a
//! thickness of 0 — the reference again, under another name; a linear law
//! whose thickness turns **negative** on some node (past a nanometre of
//! rounding, `ROUNDING`) — the new horizon would
//! cross its reference, on the wrong side of it, while looking like a horizon
//! laid as asked. It is refused with the count and the first node, rather than
//! clamped at zero: a pinch-out is a statement about the geology, and nobody
//! made it.

use crate::heights::{Height, Heights};
use crate::surface::Up;

pub use crate::heights::Surface;

/// What this module refuses, by name.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum LawError {
    #[error("the thickness must be a finite number, 0 or more — {given} given")]
    BadThickness { given: f64 },
    #[error("a thickness of 0 everywhere lays the reference again, under another name")]
    ZeroThickness,
    #[error("the {what} of the law must be a finite number — {given} given")]
    NotFinite { what: &'static str, given: f64 },
    #[error(
        "the law gives a negative thickness at {count} node(s) — {value} at ({x}, {y}) first: the horizon would cross its reference; bound the law (a smaller gradient, another anchor), or cut the horizon where it thins out"
    )]
    Negative {
        count: usize,
        x: f64,
        y: f64,
        value: f64,
    },
    #[error("cut {at}: {why}")]
    Cut { at: usize, why: crate::Error },
}

/// Which side of the reference the new horizon lies on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// Below it: older, deeper — H7 under H6.
    Below,
    /// Above it: younger, shallower — H2 over the base of the Lutetian.
    Above,
}

impl Side {
    /// `+1` for above, `−1` for below — times `Up::sign`, the sign of the
    /// offset on the third coordinate.
    fn sign(self) -> f64 {
        match self {
            Side::Below => -1.0,
            Side::Above => 1.0,
        }
    }
}

/// The thickness between the reference and the new horizon, measured
/// vertically, in the unit of the reference's heights.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Law {
    /// The same thickness everywhere.
    Parallel {
        /// The thickness.
        thickness: f64,
    },
    /// A thickness changing linearly along a direction.
    Linear {
        /// The thickness at `anchor`.
        thickness: f64,
        /// Where the thickness is `thickness`: `[x, y]`, in the reference's
        /// frame.
        anchor: [f64; 2],
        /// The direction the gradient runs along, in degrees clockwise from
        /// grid north (+y); east is 90.
        azimuth: f64,
        /// The change of thickness per unit of horizontal distance along
        /// `azimuth` — positive thickens that way, negative thins.
        gradient: f64,
    },
}

impl Law {
    /// Whether the law can be laid at all — before a node is read.
    pub fn check(&self) -> Result<(), LawError> {
        match *self {
            Law::Parallel { thickness } => {
                if !thickness.is_finite() || thickness < 0.0 {
                    return Err(LawError::BadThickness { given: thickness });
                }
                if thickness == 0.0 {
                    return Err(LawError::ZeroThickness);
                }
            }
            Law::Linear {
                thickness,
                anchor,
                azimuth,
                gradient,
            } => {
                if !thickness.is_finite() || thickness < 0.0 {
                    return Err(LawError::BadThickness { given: thickness });
                }
                for (what, given) in [
                    ("gradient", gradient),
                    ("azimuth", azimuth),
                    ("anchor x", anchor[0]),
                    ("anchor y", anchor[1]),
                ] {
                    if !given.is_finite() {
                        return Err(LawError::NotFinite { what, given });
                    }
                }
                // Zero at the anchor and nowhere else different: the
                // parallel law's zero, reached by another road.
                if thickness == 0.0 && gradient == 0.0 {
                    return Err(LawError::ZeroThickness);
                }
            }
        }
        Ok(())
    }

    /// The thickness the law states under a plan position.
    pub fn thickness_at(&self, x: f64, y: f64) -> f64 {
        match *self {
            Law::Parallel { thickness } => thickness,
            Law::Linear {
                thickness,
                anchor,
                azimuth,
                gradient,
            } => {
                // The distance along the azimuth: north is +y, east is +x,
                // the angle turning clockwise from north.
                let (sin, cos) = sin_cos_degrees(azimuth);
                let along = (x - anchor[0]) * sin + (y - anchor[1]) * cos;
                thickness + gradient * along
            }
        }
    }
}

/// The sine and cosine of an angle in degrees, **exact at the four cardinal
/// directions**: `cos(90°)` computed through radians is 6e-17, not 0, and a
/// wedge running east would then thin by a rounding error along the north —
/// enough to make a zero thickness negative on the line through its anchor.
fn sin_cos_degrees(degrees: f64) -> (f64, f64) {
    let turned = degrees.rem_euclid(360.0);
    if turned == 0.0 {
        (0.0, 1.0)
    } else if turned == 90.0 {
        (1.0, 0.0)
    } else if turned == 180.0 {
        (0.0, -1.0)
    } else if turned == 270.0 {
        (-1.0, 0.0)
    } else {
        let radians = turned.to_radians();
        (radians.sin(), radians.cos())
    }
}

/// How far below zero a thickness may fall and still be read as zero — a
/// rounding of the arithmetic, in the unit of the heights. A nanometre of any
/// length unit is not a statement about the geology; anything more is.
const ROUNDING: f64 = 1e-9;

/// A horizon laid, node by node, and what could not be.
#[derive(Clone, Debug, PartialEq)]
pub struct Laid {
    /// One height per node of the reference, in its order; `NaN` where there
    /// is none.
    pub z: Vec<f64>,
    /// Nodes of the reference with no finite position or height.
    pub holes: usize,
    /// Nodes a cut lies below — eroded.
    pub eroded: usize,
    /// Nodes under a folded cut — undecided.
    pub folded: usize,
    /// Nodes no cut has a face over or under — kept; `0` without cuts.
    pub beyond: usize,
    /// Faces of the cuts that cannot be read — a corner with no height (a
    /// hole of a cut's grid), no area in plan, a cell that is not convex.
    pub set_aside: usize,
    /// The thinnest and the thickest thickness laid, over the nodes kept —
    /// `None` when none is.
    pub range: Option<[f64; 2]>,
}

/// The new horizon at every node of the reference: the law laid toward
/// `side`, the third coordinate read as `up` says, cut by every one of
/// `cuts`.
///
/// `nodes` are the reference's `[x, y, z]`; `cuts` are in the same frame —
/// the caller checks that, since only it knows the frames.
pub fn lay(
    nodes: &[[f64; 3]],
    law: &Law,
    side: Side,
    up: Up,
    cuts: &[Surface<'_>],
) -> Result<Laid, LawError> {
    law.check()?;
    let heights = cuts
        .iter()
        .enumerate()
        .map(|(at, cut)| {
            Heights::of(*cut, "cut", "under each node of the horizon")
                .map_err(|why| LawError::Cut { at, why })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let set_aside: usize = heights.iter().map(Heights::set_aside).sum();

    // The offset on the third coordinate: toward the side, in the frame's
    // sense — below is down, and down is −z in elevation, +z in depth.
    let toward = side.sign() * up.sign();
    let mut laid = Laid {
        z: Vec::with_capacity(nodes.len()),
        holes: 0,
        eroded: 0,
        folded: 0,
        beyond: 0,
        set_aside,
        range: None,
    };
    let mut negative: Option<(usize, [f64; 3])> = None;
    for &[x, y, z] in nodes {
        if !(x.is_finite() && y.is_finite() && z.is_finite()) {
            laid.holes += 1;
            laid.z.push(f64::NAN);
            continue;
        }
        let thickness = law.thickness_at(x, y);
        if thickness < -ROUNDING {
            negative = Some(match negative {
                None => (1, [x, y, thickness]),
                Some((count, first)) => (count + 1, first),
            });
            laid.z.push(f64::NAN);
            continue;
        }
        let thickness = thickness.max(0.0);
        let height = z + toward * thickness;

        let mut eroded = false;
        let mut folded = false;
        let mut met = false;
        for surface in &heights {
            match surface.height(x, y) {
                Height::At(cut) => {
                    met = true;
                    // Above the cut, in the frame's sense: eroded. On it is
                    // kept — the horizon reaches the erosion surface there.
                    if up.sign() * (height - cut) > 0.0 {
                        eroded = true;
                    }
                }
                Height::Folded => {
                    met = true;
                    folded = true;
                }
                Height::Outside => {}
            }
        }
        if eroded {
            laid.eroded += 1;
            laid.z.push(f64::NAN);
        } else if folded {
            laid.folded += 1;
            laid.z.push(f64::NAN);
        } else {
            if !heights.is_empty() && !met {
                laid.beyond += 1;
            }
            laid.range = Some(match laid.range {
                None => [thickness, thickness],
                Some([low, high]) => [low.min(thickness), high.max(thickness)],
            });
            laid.z.push(height);
        }
    }
    if let Some((count, [x, y, value])) = negative {
        return Err(LawError::Negative { count, x, y, value });
    }
    Ok(laid)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 3 × 3 lattice at 50 from the origin, at height `z`.
    fn lattice(z: f64) -> Vec<[f64; 3]> {
        let mut out = Vec::new();
        for j in 0..3 {
            for i in 0..3 {
                out.push([50.0 * i as f64, 50.0 * j as f64, z]);
            }
        }
        out
    }

    /// A square over `[x0, x1] × [−10, 110]`, split in two triangles, at
    /// `z(x)`.
    fn plate(x0: f64, x1: f64, z: impl Fn(f64) -> f64) -> (Vec<f64>, Vec<u32>) {
        let corners = [[x0, -10.0], [x1, -10.0], [x1, 110.0], [x0, 110.0]];
        (
            corners.iter().flat_map(|&[x, y]| [x, y, z(x)]).collect(),
            vec![0, 1, 2, 0, 2, 3],
        )
    }

    fn surface<'a>(points: &'a [f64], triangles: &'a [u32]) -> Surface<'a> {
        Surface {
            points,
            triangles,
            quads: &[],
        }
    }

    /// **Below is down in the frame's sense**: `z − t` in elevation, `z + t`
    /// in depth — and above is the reverse. Reading the sense wrong would put
    /// H7 above H6 in a depth frame, every number plausible.
    #[test]
    fn a_parallel_horizon_is_the_thickness_away_toward_its_side_in_the_frames_sense() {
        let law = Law::Parallel { thickness: 7.5 };
        let cases = [
            (Side::Below, Up::PlusZ, 92.5),
            (Side::Below, Up::MinusZ, 107.5),
            (Side::Above, Up::PlusZ, 107.5),
            (Side::Above, Up::MinusZ, 92.5),
        ];
        for (side, up, expected) in cases {
            let laid = lay(&lattice(100.0), &law, side, up, &[]).unwrap();
            assert_eq!(laid.z.len(), 9);
            assert!(
                laid.z.iter().all(|z| (z - expected).abs() < 1e-12),
                "{side:?} {up:?}: {:?}",
                laid.z
            );
            assert_eq!((laid.holes, laid.eroded, laid.folded, laid.beyond), (0, 0, 0, 0));
            assert_eq!(laid.range, Some([7.5, 7.5]));
        }
    }

    /// **A hole stays a hole, and is counted**: the law has nothing to stand
    /// on where the reference has no height.
    #[test]
    fn a_hole_of_the_reference_is_a_hole_of_the_horizon_and_counted() {
        let mut nodes = lattice(10.0);
        nodes[4][2] = f64::NAN;
        nodes[8][0] = f64::NAN;
        let laid = lay(
            &nodes,
            &Law::Parallel { thickness: 1.0 },
            Side::Below,
            Up::PlusZ,
            &[],
        )
        .unwrap();
        assert_eq!(laid.holes, 2);
        assert!(laid.z[4].is_nan() && laid.z[8].is_nan());
        assert_eq!(laid.z.iter().filter(|z| z.is_finite()).count(), 7);
    }

    /// **A linear law** is its thickness at the anchor, and changes by the
    /// gradient along the azimuth — east here, so along x only.
    #[test]
    fn a_linear_law_changes_along_its_azimuth_only() {
        let law = Law::Linear {
            thickness: 10.0,
            anchor: [0.0, 0.0],
            azimuth: 90.0,
            gradient: 0.1,
        };
        let laid = lay(&lattice(0.0), &law, Side::Below, Up::PlusZ, &[]).unwrap();
        for (node, z) in lattice(0.0).iter().zip(&laid.z) {
            let expected = -(10.0 + 0.1 * node[0]);
            assert!((z - expected).abs() < 1e-9, "at {node:?}: {z} for {expected}");
        }
        assert_eq!(
            laid.range.map(|[a, b]| [a.round(), b.round()]),
            Some([10.0, 20.0])
        );

        // North: along y only.
        let north = Law::Linear {
            thickness: 10.0,
            anchor: [0.0, 0.0],
            azimuth: 0.0,
            gradient: 0.1,
        };
        let laid = lay(&lattice(0.0), &north, Side::Below, Up::PlusZ, &[]).unwrap();
        for (node, z) in lattice(0.0).iter().zip(&laid.z) {
            assert!((z + 10.0 + 0.1 * node[1]).abs() < 1e-9, "at {node:?}: {z}");
        }
    }

    /// **A law that turns negative is refused with its count**, rather than
    /// laid on the wrong side of its reference or clamped at zero.
    #[test]
    fn a_thickness_that_turns_negative_is_refused_with_its_count() {
        // 5 at x = 0, 0 at x = 50, −5 at x = 100: the column x = 100 crosses.
        let law = Law::Linear {
            thickness: 5.0,
            anchor: [0.0, 0.0],
            azimuth: 90.0,
            gradient: -0.1,
        };
        match lay(&lattice(0.0), &law, Side::Below, Up::PlusZ, &[]) {
            Err(LawError::Negative { count, x, value, .. }) => {
                assert_eq!(count, 3);
                assert_eq!(x, 100.0);
                assert!((value + 5.0).abs() < 1e-9);
            }
            other => panic!("a negative thickness was laid: {other:?}"),
        }
    }

    /// **Nothing that is not a law is laid.**
    #[test]
    fn a_zero_or_non_finite_law_is_refused_by_name() {
        let nodes = lattice(0.0);
        let refused = |law: Law| lay(&nodes, &law, Side::Below, Up::PlusZ, &[]).unwrap_err();
        assert_eq!(refused(Law::Parallel { thickness: 0.0 }), LawError::ZeroThickness);
        assert_eq!(
            refused(Law::Parallel { thickness: -1.0 }),
            LawError::BadThickness { given: -1.0 }
        );
        assert!(matches!(
            refused(Law::Parallel { thickness: f64::NAN }),
            LawError::BadThickness { .. }
        ));
        assert!(matches!(
            refused(Law::Linear {
                thickness: 1.0,
                anchor: [0.0, f64::INFINITY],
                azimuth: 0.0,
                gradient: 0.0
            }),
            LawError::NotFinite { what: "anchor y", .. }
        ));
        assert_eq!(
            refused(Law::Linear {
                thickness: 0.0,
                anchor: [0.0, 0.0],
                azimuth: 0.0,
                gradient: 0.0
            }),
            LawError::ZeroThickness
        );
        // A wedge that is zero at its anchor and thickens away from it is a law.
        let wedge = Law::Linear {
            thickness: 0.0,
            anchor: [0.0, 0.0],
            azimuth: 90.0,
            gradient: 0.1,
        };
        assert!(lay(&nodes, &wedge, Side::Below, Up::PlusZ, &[]).is_ok());
    }

    /// **A cut never lets the horizon rise above it**, in either sense of the
    /// frame; a node above the cut is a hole and counted, a node below it is
    /// kept, a node no cut lies over is kept and counted apart.
    #[test]
    fn a_cut_never_lets_the_horizon_rise_above_it() {
        // In elevation: the horizon laid at 0 − 10 = −10; the cut over
        // x ∈ [−10, 60] at −13 + x/10: −13 at x = 0 (eroded), −8 at x = 50
        // (kept, below it); the column x = 100 lies beyond it.
        let (points, triangles) = plate(-10.0, 60.0, |x| -13.0 + x / 10.0);
        let cut = surface(&points, &triangles);
        let laid = lay(
            &lattice(0.0),
            &Law::Parallel { thickness: 10.0 },
            Side::Below,
            Up::PlusZ,
            &[cut],
        )
        .unwrap();
        for (node, z) in lattice(0.0).iter().zip(&laid.z) {
            let x = node[0];
            if x <= 60.0 {
                let under = -13.0 + x / 10.0;
                assert!(
                    z.is_nan() || *z <= under,
                    "at x = {x}: {z} above the cut at {under}"
                );
            }
        }
        assert_eq!((laid.eroded, laid.beyond, laid.folded), (3, 3, 0), "{laid:?}");
        assert!(
            laid.z[0].is_nan() && laid.z[1] == -10.0 && laid.z[2] == -10.0,
            "{:?}",
            laid.z
        );

        // In depth, the same picture: the horizon at 0 + 10 = 10, the cut at
        // 13 − x/10 — deeper than the horizon at x = 0, so it erodes it there.
        let (points, triangles) = plate(-10.0, 60.0, |x| 13.0 - x / 10.0);
        let cut = surface(&points, &triangles);
        let laid = lay(
            &lattice(0.0),
            &Law::Parallel { thickness: 10.0 },
            Side::Below,
            Up::MinusZ,
            &[cut],
        )
        .unwrap();
        for (node, z) in lattice(0.0).iter().zip(&laid.z) {
            let x = node[0];
            if x <= 60.0 {
                let under = 13.0 - x / 10.0;
                assert!(
                    z.is_nan() || *z >= under,
                    "at x = {x}: {z} above the cut at depth {under}"
                );
            }
        }
        assert_eq!((laid.eroded, laid.beyond), (3, 3), "{laid:?}");
    }

    /// **A folded cut decides nothing**: the node is a hole, counted apart
    /// from the eroded ones.
    #[test]
    fn a_folded_cut_leaves_a_hole_counted_apart() {
        // Two sheets over the same square, at two heights.
        let points = vec![
            -10.0, -10.0, 50.0, 110.0, -10.0, 50.0, 110.0, 110.0, 50.0, -10.0, 110.0, 50.0, //
            -10.0, -10.0, 60.0, 110.0, -10.0, 60.0, 110.0, 110.0, 60.0, -10.0, 110.0, 60.0,
        ];
        let triangles = vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7];
        let cut = surface(&points, &triangles);
        let laid = lay(
            &lattice(0.0),
            &Law::Parallel { thickness: 1.0 },
            Side::Below,
            Up::PlusZ,
            &[cut],
        )
        .unwrap();
        assert_eq!((laid.folded, laid.eroded), (9, 0));
        assert!(laid.z.iter().all(|z| z.is_nan()));
    }

    /// A cut with no face is refused by its rank, not read as no erosion.
    #[test]
    fn a_cut_with_no_face_is_refused_by_its_rank() {
        let points = vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        let cut = surface(&points, &[]);
        let why = lay(
            &lattice(0.0),
            &Law::Parallel { thickness: 1.0 },
            Side::Below,
            Up::PlusZ,
            &[cut],
        )
        .unwrap_err();
        assert!(matches!(why, LawError::Cut { at: 0, .. }), "{why:?}");
    }
}
