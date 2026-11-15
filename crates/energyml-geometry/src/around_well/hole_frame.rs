//! The physical frame of the hole at one measured depth: tangent, high side, right, inclination.
//!
//! Computed in the placed frame (metres, z up), after `PlacedPath` has turned the line's direction through the
//! CRS, so a CRS written z down is already corrected. The right is the **physical** right of an observer looking
//! down the hole, even in an indirect placed frame (northing then easting): `right = s (tangent x high_side)`,
//! `s = +1` in a direct frame and `-1` in an indirect one.
//!
//! This is not the transported frame of `tube.rs` (minimal rotation, arbitrary origin): that one has no
//! meaning for "up" or "right".

use energyml_domain::crs::{CompassAxes, Handedness};

use crate::points::Point;
use crate::vector::{add, cross, dot, norm, scale, sub, unit};
use crate::well::{MdRange, PlacedPath};

/// S4: below this inclination the high side does not exist and a frame is refused at that node.
pub const MIN_INCLINATION_DEGREES: f64 = 1.0;

/// The physical frame of the hole at one measured depth, placed in metres, z up.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct HoleFrame {
    /// The measured depth, in the path's MD unit.
    pub md: f64,
    /// The centre of the hole there.
    pub at: Point,
    /// Unit, towards increasing MD.
    pub tangent: Point,
    /// Unit, perpendicular to the tangent, in the vertical plane holding it, pointing up.
    pub high_side: Point,
    /// Unit: the physical right of someone looking down the hole, in placed coordinates
    /// (tangent x high_side in a direct frame, its opposite in an indirect one).
    pub right: Point,
    /// Radians from vertical down.
    pub inclination: f64,
}

/// Why a frame cannot be given, by name.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum FrameRefusal {
    /// The direction or the centre is not a finite vector.
    #[error("at MD {md}: the direction of the hole is not a finite vector")]
    NotFinite {
        /// The measured depth.
        md: f64,
    },
    /// The hole is too close to vertical for a high side to exist (S4).
    #[error("at MD {md}: the hole is {inclination} rad from vertical, under the threshold - it has no high side")]
    HighSideUndefined {
        /// The measured depth.
        md: f64,
        /// Radians from vertical down.
        inclination: f64,
    },
    /// A frame is never extrapolated past the depths a path states.
    #[error("MD {md} is outside the path's depths [{first}, {last}]: a frame is never extrapolated")]
    MdOutside {
        /// The depth asked.
        md: f64,
        /// The first depth the path states.
        first: f64,
        /// The last depth the path states.
        last: f64,
    },
    /// The line has no direction there (a degenerate interval).
    #[error("at MD {md}: the line has no direction")]
    NoDirection {
        /// The measured depth.
        md: f64,
    },
    /// Neither the CRS nor the caller states the order of the horizontal axes.
    #[error("the CRS states no usable order of its horizontal axes, and none was declared: which side is right is not known")]
    AxisOrderUnknown,
    /// A declaration against what the file states.
    #[error("the file states a {stated:?}-handed frame and the declaration says {declared:?}: a declaration never overrides the file")]
    DeclaredAgainstFile {
        /// What the file states.
        stated: Handedness,
        /// What the caller declared.
        declared: Handedness,
    },
    /// Grid north runs along the hole.
    #[error("at MD {md}: north runs along the hole - it has no direction in the plane across it")]
    NorthAlongHole {
        /// The measured depth.
        md: f64,
    },
}

/// What a frame needs that the file may not say.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FrameOptions {
    /// Radians. Defaults to [`MIN_INCLINATION_DEGREES`].
    pub min_inclination: f64,
    /// The handedness the caller declares when the CRS states none (S2). Never applied over the file's.
    pub handedness: Option<Handedness>,
}

impl Default for FrameOptions {
    fn default() -> Self {
        FrameOptions {
            min_inclination: MIN_INCLINATION_DEGREES.to_radians(),
            handedness: None,
        }
    }
}

/// One outcome per measured depth asked: a refused node is a named hole, not the end of the well.
#[derive(Clone, Debug, PartialEq)]
pub struct Frames {
    /// In the order of the depths asked.
    pub frames: Vec<Result<HoleFrame, FrameRefusal>>,
}

impl Frames {
    /// The frames given, skipping the refused ones.
    pub fn given(&self) -> impl Iterator<Item = &HoleFrame> {
        self.frames.iter().filter_map(|frame| frame.as_ref().ok())
    }

    /// How many were refused.
    pub fn refused(&self) -> usize {
        self.frames.iter().filter(|frame| frame.is_err()).count()
    }
}

/// The high side and the inclination of a tangent (S4). The one place the high side is computed.
///
/// The high side is the upward vertical projected on the plane normal to the tangent. `min_inclination` is in
/// radians: a hole within it of vertical (down or up) is refused. TODO(verify): the TRG WITSML 2.1 defines no
/// high side (only the gravity toolface `Gtf`, section 16.10 p. 358); the definition used is the one of
/// Sawaryn and Thorogood, SPE 84246.
pub fn high_side_of(
    md: f64,
    tangent: Point,
    min_inclination: f64,
) -> Result<(Point, f64), FrameRefusal> {
    let t = unit(tangent).ok_or(FrameRefusal::NotFinite { md })?;
    let horizontal = t[0].hypot(t[1]);
    let inclination = horizontal.atan2(-t[2]);
    if horizontal.is_nan() || horizontal < min_inclination.sin() {
        return Err(FrameRefusal::HighSideUndefined { md, inclination });
    }
    let up = [0.0, 0.0, 1.0];
    let high = unit(sub(up, scale(t, t[2]))).ok_or(FrameRefusal::NotFinite { md })?;
    Ok((high, inclination))
}

/// The frame from a centre and a tangent, pure.
pub fn hole_frame(
    md: f64,
    at: Point,
    tangent: Point,
    handedness: Handedness,
    min_inclination: f64,
) -> Result<HoleFrame, FrameRefusal> {
    if !at.iter().all(|c| c.is_finite()) {
        return Err(FrameRefusal::NotFinite { md });
    }
    let t = unit(tangent).ok_or(FrameRefusal::NotFinite { md })?;
    let (high_side, inclination) = high_side_of(md, t, min_inclination)?;
    let sign = match handedness {
        Handedness::Right => 1.0,
        Handedness::Left => -1.0,
    };
    let right = scale(cross(t, high_side), sign);
    Ok(HoleFrame {
        md,
        at,
        tangent: t,
        high_side,
        right,
        inclination,
    })
}

/// The frames at `mds` along a placed path.
///
/// The handedness is the file's, or the caller's when the file states none; both stated and different is
/// refused as a whole, and so is neither. A depth outside the path, or where the line has no direction, or too
/// close to vertical, is a refused frame at that depth only.
pub fn frames_along(
    path: &PlacedPath,
    mds: &[f64],
    options: &FrameOptions,
) -> Result<Frames, FrameRefusal> {
    let handedness = match (path.handedness(), options.handedness) {
        (Some(stated), Some(declared)) if stated != declared => {
            return Err(FrameRefusal::DeclaredAgainstFile { stated, declared });
        }
        (Some(stated), _) => stated,
        (None, Some(declared)) => declared,
        (None, None) => return Err(FrameRefusal::AxisOrderUnknown),
    };
    let [first, last] = path.md_extent();
    let frames = mds
        .iter()
        .map(|&md| {
            if path.range_of(md) != MdRange::Within {
                return Err(FrameRefusal::MdOutside { md, first, last });
            }
            let tangent = path
                .direction_at(md)
                .ok_or(FrameRefusal::NoDirection { md })?;
            hole_frame(
                md,
                path.at(md).point,
                tangent,
                handedness,
                options.min_inclination,
            )
        })
        .collect();
    Ok(Frames { frames })
}

/// The one 3D pose of a point off the hole: `P + dn h`, `P = path.at(md).point`, `h` the high side
/// there (S4 threshold `MIN_INCLINATION_DEGREES`). `dn == 0` needs no high side and gives `P`.
pub fn offset_point(path: &PlacedPath, md: f64, dn: f64) -> Result<Point, FrameRefusal> {
    if path.range_of(md) != MdRange::Within {
        let [first, last] = path.md_extent();
        return Err(FrameRefusal::MdOutside { md, first, last });
    }
    let p = path.at(md).point;
    if dn == 0.0 {
        return Ok(p);
    }
    if !dn.is_finite() {
        return Err(FrameRefusal::NotFinite { md });
    }
    let t = path
        .direction_at(md)
        .ok_or(FrameRefusal::NoDirection { md })?;
    let (h, _) = high_side_of(md, t, MIN_INCLINATION_DEGREES.to_radians())?;
    Ok(add(p, scale(h, dn)))
}

/// Grid north seen in the plane across the hole, as (right, high side), unit.
///
/// This is the north of the grid (the second axis of the projected system turned by its axis order), not the
/// true north, which needs a convergence this crate does not have.
pub fn north_in_plane(frame: &HoleFrame, compass: &CompassAxes) -> Result<[f64; 2], FrameRefusal> {
    let n = [compass.north[0], compass.north[1], 0.0];
    let p = sub(n, scale(frame.tangent, dot(n, frame.tangent)));
    if norm(p).is_nan() || norm(p) < 1e-9 {
        return Err(FrameRefusal::NorthAlongHole { md: frame.md });
    }
    let p = unit(p).ok_or(FrameRefusal::NorthAlongHole { md: frame.md })?;
    Ok([dot(p, frame.right), dot(p, frame.high_side)])
}
