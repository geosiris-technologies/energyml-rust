//! Polar vectors laid in the plane across the hole, under a convention the caller declares.
//!
//! # The canonical plane
//!
//! Coordinates `(r, h)`: `r` along the `right` of a [`HoleFrame`], `h` along its `high_side`, as an observer
//! standing uphole and looking towards increasing MD sees them: `right` on the right of the screen and
//! `high_side` up. A clockwise rotation **in this plane** goes from the top to the right.
//!
//! What the file does not state (where angle zero points, which way angles grow, from where they are seen) is
//! an [`AngleConvention`] the caller declares; none declared is refused by name (S2).

use std::f64::consts::TAU;

use crate::around_well::hole_frame::HoleFrame;
use crate::points::Point;
use crate::vector::{add, scale};

/// Where angle zero points.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AngleOrigin {
    /// The high side of the hole.
    HighSide,
    /// The low side (opposite the high side).
    LowSide,
    /// The right of an observer looking down the hole.
    Right,
    /// The left of an observer looking down the hole.
    Left,
    /// Grid north, as seen in the plane across the hole (`hole_frame::north_in_plane`).
    North,
}

/// Which way angles grow.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AngleSense {
    /// Clockwise as seen from `ViewedFrom`.
    Clockwise,
    /// Anticlockwise as seen from `ViewedFrom`.
    Anticlockwise,
}

/// From where the sense is seen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewedFrom {
    /// Standing uphole, looking towards increasing MD: the canonical plane, no mirror.
    LookingDownhole,
    /// Standing downhole, looking back up: left and right mirrored.
    LookingUphole,
}

/// A convention the file does not state and the caller declares (S2).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct AngleConvention {
    /// Where angle zero points.
    pub origin: AngleOrigin,
    /// Which way angles grow.
    pub sense: AngleSense,
    /// From where the sense is seen.
    pub viewed_from: ViewedFrom,
    /// Radians, added to every angle before it is placed: where the file's zero sits from `origin`.
    pub offset: f64,
}

/// Why a polar vector cannot be placed.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum PolarRefusal {
    /// No convention was declared.
    #[error(
        "no angle convention is declared: the origin and the sense of the angles are not stated by the file"
    )]
    ConventionUndeclared,
    /// Radii and angles differ in number.
    #[error("{radii} radii for {angles} angles")]
    CountMismatch {
        /// The number of radii.
        radii: usize,
        /// The number of angles.
        angles: usize,
    },
    /// The origin is north and the caller gave no north in the plane.
    #[error("the angles count from north, and north has no direction across the hole here")]
    NorthUnavailable,
    /// Fewer than three sectors.
    #[error("{n} sectors: three are the fewest that close a ring")]
    TooFewSectors {
        /// The number of sectors given.
        n: usize,
    },
    /// An angle that is not a number.
    #[error("angle {at} is not a number")]
    NotFinite {
        /// The index of the angle.
        at: usize,
    },
    /// Angles that do not turn one way within one turn.
    #[error("the angles do not turn one way within one turn")]
    AnglesNotMonotonic,
}

/// A ring placed in the canonical plane; a missing radius is a counted hole, never filled.
#[derive(Clone, Debug, PartialEq)]
pub struct PolarPoints {
    /// (right, high side), one per radius, `None` for a hole.
    pub points: Vec<Option<[f64; 2]>>,
    /// How many are `None`.
    pub holes: usize,
    /// Of which negative radii (shown as holes, with a note: detect and signal).
    pub negative: usize,
}

/// The unit direction of one angle in the canonical plane.
///
/// `north` is the output of `hole_frame::north_in_plane`; it is read only when the origin is north.
pub fn plane_direction(
    angle: f64,
    convention: &AngleConvention,
    north: Option<[f64; 2]>,
) -> Result<[f64; 2], PolarRefusal> {
    let theta = angle + convention.offset;
    if !theta.is_finite() {
        return Err(PolarRefusal::NotFinite { at: 0 });
    }
    let origin = match convention.origin {
        AngleOrigin::HighSide => [0.0, 1.0],
        AngleOrigin::LowSide => [0.0, -1.0],
        AngleOrigin::Right => [1.0, 0.0],
        AngleOrigin::Left => [-1.0, 0.0],
        AngleOrigin::North => {
            let north = north.ok_or(PolarRefusal::NorthUnavailable)?;
            let length = north[0].hypot(north[1]);
            if !length.is_finite() || length < 1e-12 {
                return Err(PolarRefusal::NorthUnavailable);
            }
            [north[0] / length, north[1] / length]
        }
    };
    let clockwise = (convention.sense == AngleSense::Clockwise)
        == (convention.viewed_from == ViewedFrom::LookingDownhole);
    let (sin, cos) = theta.sin_cos();
    Ok(if clockwise {
        [
            origin[0] * cos + origin[1] * sin,
            -origin[0] * sin + origin[1] * cos,
        ]
    } else {
        [
            origin[0] * cos - origin[1] * sin,
            origin[0] * sin + origin[1] * cos,
        ]
    })
}

/// One point per radius at its angle. A radius or an angle that is not a number, or a negative radius, is a
/// counted hole and is never filled.
pub fn polar_points(
    radii: &[f64],
    angles: &[f64],
    convention: Option<&AngleConvention>,
    north: Option<[f64; 2]>,
) -> Result<PolarPoints, PolarRefusal> {
    let convention = convention.ok_or(PolarRefusal::ConventionUndeclared)?;
    if radii.len() != angles.len() {
        return Err(PolarRefusal::CountMismatch {
            radii: radii.len(),
            angles: angles.len(),
        });
    }
    if convention.origin == AngleOrigin::North && north.is_none() {
        return Err(PolarRefusal::NorthUnavailable);
    }
    let mut points = Vec::with_capacity(radii.len());
    let mut holes = 0;
    let mut negative = 0;
    for (&radius, &angle) in radii.iter().zip(angles) {
        if !radius.is_finite() || !angle.is_finite() || radius < 0.0 {
            holes += 1;
            if radius.is_finite() && radius < 0.0 {
                negative += 1;
            }
            points.push(None);
            continue;
        }
        let direction = plane_direction(angle, convention, north)?;
        points.push(Some([direction[0] * radius, direction[1] * radius]));
    }
    Ok(PolarPoints {
        points,
        holes,
        negative,
    })
}

/// The bounds of each sector, halfway to its neighbours, in the sense the angles turn: `[from, to]` for each.
pub fn sector_bounds(angles: &[f64]) -> Result<Vec<[f64; 2]>, PolarRefusal> {
    let n = angles.len();
    if n < 3 {
        return Err(PolarRefusal::TooFewSectors { n });
    }
    if let Some(at) = angles.iter().position(|a| !a.is_finite()) {
        return Err(PolarRefusal::NotFinite { at });
    }
    if !turns_once(angles) {
        return Err(PolarRefusal::AnglesNotMonotonic);
    }
    let increasing = angles[1] > angles[0];
    let (before_first, after_last) = if increasing {
        (angles[n - 1] - TAU, angles[0] + TAU)
    } else {
        (angles[n - 1] + TAU, angles[0] - TAU)
    };
    Ok((0..n)
        .map(|k| {
            let previous = if k == 0 { before_first } else { angles[k - 1] };
            let next = if k == n - 1 { after_last } else { angles[k + 1] };
            [(previous + angles[k]) / 2.0, (angles[k] + next) / 2.0]
        })
        .collect())
}

/// `n` sectors evenly spread, the first centred on `first_centre`.
pub fn regular_sectors(n: usize, first_centre: f64) -> Vec<f64> {
    (0..n).map(|k| first_centre + k as f64 * TAU / n as f64).collect()
}

/// A point of the canonical plane carried into space by the frame.
///
/// A radial exaggeration is applied by the caller to the plane point, never hidden here.
pub fn to_space(frame: &HoleFrame, plane: [f64; 2]) -> Point {
    add(
        add(frame.at, scale(frame.right, plane[0])),
        scale(frame.high_side, plane[1]),
    )
}

/// Whether `angles` turn strictly one way within less than one turn (shared with `envelope`).
pub(crate) fn turns_once(angles: &[f64]) -> bool {
    if angles.len() < 2 {
        return true;
    }
    let first = angles[1] - angles[0];
    let one_way = angles.windows(2).all(|pair| {
        let step = pair[1] - pair[0];
        (step > 0.0 && first > 0.0) || (step < 0.0 && first < 0.0)
    });
    one_way && (angles[angles.len() - 1] - angles[0]).abs() < TAU
}
