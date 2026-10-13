//! **Fault sticks** — the lines a fault surface is lofted between, drawn from
//! a plane when nobody digitised them, and the orientation of the surface
//! they make.
//!
//! Written for `fault-from-sticks` in `energyml-build` (a synthetic model): two faults are lofted between their sticks, two others
//! are a plane given by dip and azimuth, warped when its trace curves.
//! The loft itself is [`crate::loft`]; this module only says where the sticks
//! of a plane are, whether sticks advance along the strike, and which way the
//! surface they make dips. Nothing here reads a package or knows a frame: the
//! operation says which way is up.
//!
//! # Conventions
//!
//! - an **azimuth** is in degrees clockwise from grid north, the `+y` axis —
//!   the direction `(sin a, cos a)` in plan, the convention of `well_plan` and
//!   of RESQML's `DipDirectionNorthReferenceKind` left absent (*« If not set
//!   (absent), the NorthReferenceKind is Grid North »*, Interpretations.xsd,
//!   `FaultInterpretation`);
//! - a **dip** is in degrees from the horizontal, `[0, 90]`, and a **dip
//!   azimuth** is the direction the surface goes **down**;
//! - `up` says whether the third coordinate grows upward (an elevation) or
//!   downward (a depth). It is the frame's, and it is never guessed here.
//!
//! # Precision far from the origin
//!
//! Lutetia sits at x ≈ 651 000, y ≈ 6 861 000 (Lambert-93). Every difference
//! is taken **relative to a point of the shape** before anything is squared or
//! crossed: `heights` once answered *outside* everywhere on a grid in map
//! coordinates because its tolerance was below the rounding of 6.86e6
//! (a defect once found). The tests of this module are at map
//! coordinates for that reason.

/// A plane's orientation, as a structural geologist states it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orientation {
    /// Degrees from the horizontal, in `[0, 90]`.
    pub dip: f64,
    /// Degrees clockwise from grid north, in `[0, 360)`: the direction the
    /// surface goes **down**. For a vertical surface it is one of two opposite
    /// directions, and which one is not a fact of the surface.
    pub dip_azimuth: f64,
}

/// Why no stick could be drawn from a plane.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PlaneError {
    /// Fewer than two points on the trace.
    #[error("the trace holds {given} point(s): a fault's trace is a line, two points at least")]
    ShortTrace {
        /// How many were given.
        given: usize,
    },
    /// A point of the trace is not a finite position.
    #[error("point {at} of the trace is not a finite position")]
    NotFinite {
        /// Its rank, from 1.
        at: usize,
    },
    /// The dip is outside `]0, 90]`.
    #[error(
        "a dip is in ]0°, 90°] from the horizontal, and {given}° is not: a horizontal plane has no stick going down"
    )]
    Dip {
        /// What was given.
        given: f64,
    },
    /// The dip azimuth is outside `[0, 360]`.
    #[error("a dip azimuth is in [0°, 360°] clockwise from grid north, and {given}° is not")]
    Azimuth {
        /// What was given.
        given: f64,
    },
    /// The vertical extent is not a positive length.
    #[error(
        "the vertical extent is how far the fault goes down below its trace, a length above zero — {given} is not"
    )]
    Extent {
        /// What was given.
        given: f64,
    },
}

/// The sticks of a plane: one per point of the trace, from that point **down
/// the dip** by `extent`, measured vertically.
///
/// Each stick is two points — the trace point, and the point `extent` below it
/// and `extent / tan(dip)` further along the dip azimuth. A straight trace
/// makes a plane; a curved one a surface whose every stick dips the same way,
/// which is what *warped* means here. A vertical plane (`dip = 90`)
/// drops straight down, with **no** horizontal offset — `tan(90°)` is not
/// infinite in floating point, and an offset of 1e-14 m is a number nobody
/// stated.
///
/// Nothing checks that the trace runs along the strike: a trace oblique to the
/// dip direction makes a surface steeper than the dip given, and the caller's
/// [`orientation`] of the lofted surface is what shows it.
pub fn sticks_from_plane(
    trace: &[[f64; 3]],
    dip: f64,
    dip_azimuth: f64,
    extent: f64,
    up: bool,
) -> Result<Vec<Vec<[f64; 3]>>, PlaneError> {
    if trace.len() < 2 {
        return Err(PlaneError::ShortTrace { given: trace.len() });
    }
    if let Some(at) = trace.iter().position(|p| !p.iter().all(|v| v.is_finite())) {
        return Err(PlaneError::NotFinite { at: at + 1 });
    }
    if !(dip.is_finite() && dip > 0.0 && dip <= 90.0) {
        return Err(PlaneError::Dip { given: dip });
    }
    if !(dip_azimuth.is_finite() && (0.0..=360.0).contains(&dip_azimuth)) {
        return Err(PlaneError::Azimuth { given: dip_azimuth });
    }
    if !(extent.is_finite() && extent > 0.0) {
        return Err(PlaneError::Extent { given: extent });
    }
    let across = if dip == 90.0 {
        0.0
    } else {
        extent / dip.to_radians().tan()
    };
    let (east, north) = (dip_azimuth.to_radians().sin(), dip_azimuth.to_radians().cos());
    let down = if up { -extent } else { extent };
    Ok(trace
        .iter()
        .map(|&[x, y, z]| vec![[x, y, z], [x + across * east, y + across * north, z + down]])
        .collect())
}

/// Which way a triangulated surface dips, **as a whole**: the area-weighted
/// normal of its triangles, turned up.
///
/// Each triangle's normal is turned to agree with the first one's before they
/// are summed, so a surface whose triangles are wound inconsistently reads the
/// same as one wound well; the sum is then turned to point up, and its
/// horizontal part points down the dip. `None` when the surface has no area,
/// or a triangle names a node it does not have.
///
/// This is a **measure** of the surface, not what a producer states: RESQML's
/// `MeanDip` and `MeanAzimuth` are *« "mean" means "representative"; it is not
/// a mathematically derived mean »* (Interpretations.xsd, `FaultInterpretation`).
/// The caller confronts the two, it does not replace one by the other.
pub fn orientation(points: &[[f64; 3]], triangles: &[[u32; 3]], up: bool) -> Option<Orientation> {
    let sign = if up { 1.0 } else { -1.0 };
    let mut reference: Option<[f64; 3]> = None;
    let mut sum = [0.0f64; 3];
    for triangle in triangles {
        let corner = |i: u32| points.get(i as usize).copied();
        let (Some(a), Some(b), Some(c)) = (corner(triangle[0]), corner(triangle[1]), corner(triangle[2]))
        else {
            return None;
        };
        // Relative to the triangle's first corner, and with the vertical turned
        // to grow upward: a depth frame measures the same dip.
        let u = [b[0] - a[0], b[1] - a[1], sign * (b[2] - a[2])];
        let v = [c[0] - a[0], c[1] - a[1], sign * (c[2] - a[2])];
        let mut n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        if !n.iter().all(|x| x.is_finite()) || n.iter().all(|x| *x == 0.0) {
            continue;
        }
        match reference {
            None => reference = Some(n),
            Some(r) => {
                if n[0] * r[0] + n[1] * r[1] + n[2] * r[2] < 0.0 {
                    n = [-n[0], -n[1], -n[2]];
                }
            }
        }
        for (total, part) in sum.iter_mut().zip(n) {
            *total += part;
        }
    }
    if sum.iter().all(|x| *x == 0.0) {
        return None;
    }
    if sum[2] < 0.0 {
        sum = [-sum[0], -sum[1], -sum[2]];
    }
    let horizontal = (sum[0] * sum[0] + sum[1] * sum[1]).sqrt();
    let dip = horizontal.atan2(sum[2]).to_degrees();
    let mut dip_azimuth = sum[0].atan2(sum[1]).to_degrees();
    if dip_azimuth < 0.0 {
        dip_azimuth += 360.0;
    }
    if dip_azimuth >= 360.0 {
        dip_azimuth -= 360.0;
    }
    Some(Orientation { dip, dip_azimuth })
}

/// The sticks that do **not** advance along the strike — the rank, from 0, of
/// each stick whose centre in plan is no further along the line from the first
/// stick's centre to the last one's than the stick before it.
///
/// A loft follows the order it is given (`loft.rs`: *« a loft between sticks
/// 1, 3, 2 is a different surface, and one that looks like one »*), so a stick
/// out of order folds the surface back on itself without anything failing.
/// This finds it; deciding whether it is a mistake — a strongly curved fault
/// can turn back — is the caller's, and it is said, never corrected.
///
/// Empty for fewer than three sticks, and for sticks whose first and last
/// centres coincide in plan: there is no strike to advance along.
pub fn backtracking(sticks: &[Vec<[f64; 3]>]) -> Vec<usize> {
    if sticks.len() < 3 || sticks.iter().any(Vec::is_empty) {
        return Vec::new();
    }
    let origin = sticks[0][0];
    // Centres relative to the first stick's first point, in plan.
    let centre = |stick: &[[f64; 3]]| {
        let n = stick.len() as f64;
        let (sx, sy) = stick.iter().fold((0.0, 0.0), |(sx, sy), p| {
            (sx + (p[0] - origin[0]), sy + (p[1] - origin[1]))
        });
        [sx / n, sy / n]
    };
    let centres: Vec<[f64; 2]> = sticks.iter().map(|stick| centre(stick.as_slice())).collect();
    let (first, last) = (centres[0], centres[centres.len() - 1]);
    let axis = [last[0] - first[0], last[1] - first[1]];
    if axis[0] == 0.0 && axis[1] == 0.0 {
        return Vec::new();
    }
    let along: Vec<f64> = centres
        .iter()
        .map(|c| (c[0] - first[0]) * axis[0] + (c[1] - first[1]) * axis[1])
        .collect();
    (1..along.len()).filter(|&k| along[k] <= along[k - 1]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loft::{LoftLine, loft};

    /// Lutetia's GTH-2 wellhead, where F1 is placed: map coordinates, on
    /// purpose.
    const X0: f64 = 651_595.0;
    const Y0: f64 = 6_861_145.0;

    fn close(a: f64, b: f64, tolerance: f64) -> bool {
        (a - b).abs() <= tolerance
    }

    /// A straight trace along the strike of a plane dipping toward `azimuth`:
    /// the strike is the azimuth turned by 90°.
    fn trace(azimuth: f64, z: f64) -> Vec<[f64; 3]> {
        let strike = (azimuth - 90.0).to_radians();
        (0..5)
            .map(|k| {
                [
                    X0 + 100.0 * k as f64 * strike.sin(),
                    Y0 + 100.0 * k as f64 * strike.cos(),
                    z,
                ]
            })
            .collect()
    }

    fn lofted(sticks: &[Vec<[f64; 3]>]) -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
        let names: Vec<String> = (0..sticks.len()).map(|k| format!("stick {}", k + 1)).collect();
        let lines: Vec<LoftLine> = sticks
            .iter()
            .zip(&names)
            .map(|(points, name)| LoftLine {
                name,
                points,
                closed: false,
            })
            .collect();
        let made = loft(&lines).expect("the sticks loft");
        (made.points, made.triangles)
    }

    #[test]
    fn a_vertical_plane_drops_straight_down_with_no_offset() {
        let sticks = sticks_from_plane(&trace(10.0, 30.0), 90.0, 10.0, 180.0, true).unwrap();
        assert_eq!(sticks.len(), 5);
        for stick in &sticks {
            assert_eq!(stick.len(), 2);
            // Bit for bit: a vertical plane has no horizontal offset at all.
            assert_eq!(stick[1][0].to_bits(), stick[0][0].to_bits(), "{stick:?}");
            assert_eq!(stick[1][1].to_bits(), stick[0][1].to_bits(), "{stick:?}");
            assert_eq!(stick[1][2], -150.0, "elevation: 180 below 30");
        }
    }

    #[test]
    fn a_dipping_plane_moves_down_dip_by_the_extent_over_the_tangent() {
        // 45° toward the east: 100 down is 100 east.
        let sticks = sticks_from_plane(&trace(90.0, 20.0), 45.0, 90.0, 100.0, true).unwrap();
        for stick in &sticks {
            assert!(close(stick[1][0] - stick[0][0], 100.0, 1e-9), "{stick:?}");
            assert!(close(stick[1][1] - stick[0][1], 0.0, 1e-9), "{stick:?}");
            assert_eq!(stick[1][2], -80.0);
        }
        // 60° toward 120°: 100 / tan 60° along (sin 120°, cos 120°).
        let sticks = sticks_from_plane(&trace(120.0, 20.0), 60.0, 120.0, 100.0, true).unwrap();
        let across = 100.0 / 60f64.to_radians().tan();
        let stick = &sticks[2];
        assert!(
            close(
                stick[1][0] - stick[0][0],
                across * 120f64.to_radians().sin(),
                1e-9
            ),
            "{stick:?}"
        );
        assert!(
            close(
                stick[1][1] - stick[0][1],
                across * 120f64.to_radians().cos(),
                1e-9
            ),
            "{stick:?}"
        );
    }

    #[test]
    fn in_a_depth_frame_down_is_a_larger_z() {
        let sticks = sticks_from_plane(&trace(90.0, 20.0), 45.0, 90.0, 100.0, false).unwrap();
        assert_eq!(sticks[0][1][2], 120.0, "depth: 100 below 20 is 120");
    }

    #[test]
    fn a_plane_that_cannot_be_drawn_is_refused_by_its_name() {
        let line = trace(90.0, 0.0);
        assert_eq!(
            sticks_from_plane(&line[..1], 60.0, 90.0, 10.0, true),
            Err(PlaneError::ShortTrace { given: 1 })
        );
        assert_eq!(
            sticks_from_plane(&line, 0.0, 90.0, 10.0, true),
            Err(PlaneError::Dip { given: 0.0 })
        );
        assert_eq!(
            sticks_from_plane(&line, 95.0, 90.0, 10.0, true),
            Err(PlaneError::Dip { given: 95.0 })
        );
        assert_eq!(
            sticks_from_plane(&line, 60.0, 400.0, 10.0, true),
            Err(PlaneError::Azimuth { given: 400.0 })
        );
        assert_eq!(
            sticks_from_plane(&line, 60.0, 90.0, 0.0, true),
            Err(PlaneError::Extent { given: 0.0 })
        );
        let mut broken = line.clone();
        broken[2][1] = f64::NAN;
        assert_eq!(
            sticks_from_plane(&broken, 60.0, 90.0, 10.0, true),
            Err(PlaneError::NotFinite { at: 3 })
        );
    }

    /// Two readings of one fact: the plane stated, and the surface lofted
    /// between its sticks, measured. At map coordinates.
    #[test]
    fn the_lofted_plane_measures_the_dip_and_the_azimuth_it_was_drawn_with() {
        for (dip, azimuth) in [(65.0, 120.0), (30.0, 300.0), (55.0, 222.0), (80.0, 5.0)] {
            for up in [true, false] {
                let z = if up { 30.0 } else { 10.0 };
                let sticks = sticks_from_plane(&trace(azimuth, z), dip, azimuth, 180.0, up).unwrap();
                let (points, triangles) = lofted(&sticks);
                let found = orientation(&points, &triangles, up).expect("a surface with area");
                assert!(
                    close(found.dip, dip, 1e-6),
                    "dip {dip} toward {azimuth} (up {up}): measured {found:?}"
                );
                assert!(
                    close(found.dip_azimuth, azimuth, 1e-6),
                    "dip {dip} toward {azimuth} (up {up}): measured {found:?}"
                );
            }
        }
    }

    #[test]
    fn a_vertical_surface_measures_ninety_degrees_and_one_of_two_opposite_azimuths() {
        let sticks = sticks_from_plane(&trace(10.0, 30.0), 90.0, 10.0, 180.0, true).unwrap();
        let (points, triangles) = lofted(&sticks);
        let found = orientation(&points, &triangles, true).unwrap();
        assert!(close(found.dip, 90.0, 1e-9), "{found:?}");
        assert!(
            close(found.dip_azimuth, 10.0, 1e-6) || close(found.dip_azimuth, 190.0, 1e-6),
            "{found:?}"
        );
    }

    #[test]
    fn the_winding_of_a_triangle_does_not_turn_the_answer() {
        let sticks = sticks_from_plane(&trace(120.0, 30.0), 65.0, 120.0, 180.0, true).unwrap();
        let (points, mut triangles) = lofted(&sticks);
        let before = orientation(&points, &triangles, true).unwrap();
        for triangle in triangles.iter_mut().step_by(2) {
            triangle.swap(1, 2);
        }
        let after = orientation(&points, &triangles, true).unwrap();
        assert!(
            close(before.dip, after.dip, 1e-9) && close(before.dip_azimuth, after.dip_azimuth, 1e-9),
            "{before:?} {after:?}"
        );
    }

    #[test]
    fn a_surface_without_area_or_with_a_node_it_does_not_have_has_no_orientation() {
        let flat = [[X0, Y0, 0.0], [X0 + 1.0, Y0, 0.0], [X0 + 2.0, Y0, 0.0]];
        assert_eq!(
            orientation(&flat, &[[0, 1, 2]], true),
            None,
            "three aligned points"
        );
        assert_eq!(
            orientation(&flat, &[[0, 1, 7]], true),
            None,
            "a node out of range"
        );
    }

    #[test]
    fn a_stick_out_of_order_is_found_and_sticks_in_order_are_not() {
        let sticks = sticks_from_plane(&trace(120.0, 30.0), 65.0, 120.0, 180.0, true).unwrap();
        assert!(backtracking(&sticks).is_empty());
        let mut swapped = sticks.clone();
        swapped.swap(2, 3);
        // 1, 2, 4, 3, 5: the fourth given (rank 3) steps back.
        assert_eq!(backtracking(&swapped), vec![3]);
        assert!(
            backtracking(&sticks[..2]).is_empty(),
            "two sticks have no order to break"
        );
    }
}
