//! The limits of a depth-of-detection (DoD) property: rings of radii at
//! angles, read **by the value of the angle**, never by its rank.

use std::f64::consts::PI;

use energyml_geometry::around_well::projection::{WellAxis, WellProjection};

/// How the boundaries are taken from the rings of radii.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BoundaryMode {
    /// The angles 0 (above) and pi (below) only: the points in the plane of the section.
    InPlane,
    /// Every angle, at `r cos(theta)` on the high-side axis (even in theta: the
    /// sense of the angles is not needed).
    Projected,
}

/// The tolerance an angle is read as 0 or pi with, in radians. The angles are
/// written, not measured; a conversion from degrees moves them by a few ULPs.
pub const ANGLE_TOLERANCE: f64 = 1e-6;

/// One point of a boundary, placed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundaryPoint {
    /// The measured depth of the node.
    pub md: f64,
    /// Along the high side, positive up: `+r` above, `-r` below.
    pub dn: f64,
    /// Placed by the projection: `[abscissa, tvd]`.
    pub at: [f64; 2],
}

/// The boundaries of every node.
#[derive(Clone, Debug, PartialEq)]
pub struct Boundaries {
    /// One entry per node.
    pub above: Vec<Option<BoundaryPoint>>,
    /// One entry per node.
    pub below: Vec<Option<BoundaryPoint>>,
    /// `Projected` mode only: every finite radius of each node.
    pub projected: Vec<Vec<BoundaryPoint>>,
    /// Radii that are NaN or negative: no point, counted.
    pub absent: usize,
    /// Nodes the projection refused (S4 under 1 degree): no point, counted apart.
    pub refused: usize,
}

/// What a boundary refuses, by name.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum BoundaryRefusal {
    /// The two properties do not lie on the same rings.
    #[error(
        "{radii} radius value(s) and {angles} angle value(s) for {nodes} node(s) of {per_node}: the two properties do not lie on the same rings"
    )]
    DodShapeMismatch {
        /// Radius values given.
        radii: usize,
        /// Angle values given.
        angles: usize,
        /// Nodes of the frame.
        nodes: usize,
        /// Values per node.
        per_node: usize,
    },
    /// No angle at 0 or pi on any node.
    #[error("no angle is at 0 or pi (within {tolerance} rad): nothing falls in the plane of the section")]
    NoAngleInPlane {
        /// The tolerance used.
        tolerance: f64,
    },
    /// Two angles on one side at one node.
    #[error("node {node} has two angles at {side}: which radius is the boundary is not decided here")]
    TwoAnglesInPlane {
        /// The node.
        node: usize,
        /// `0` (above) or `pi` (below).
        side: &'static str,
    },
}

/// The angle in `(-pi, pi]`.
fn wrap(theta: f64) -> f64 {
    let two_pi = 2.0 * PI;
    let mut t = theta % two_pi;
    if t > PI {
        t -= two_pi;
    } else if t <= -PI {
        t += two_pi;
    }
    t
}

/// The boundaries of the rings of radii, placed.
///
/// `radius_m.len() == nodes * per_node`; `angle_rad.len()` is `nodes *
/// per_node`, or `per_node` (the same angles at every node: the UDAR case).
/// The angles are **searched by value**: within [`ANGLE_TOLERANCE`] of 0 is
/// above, of pi is below. A boundary point is `projection.place(axis, md, +-r)`,
/// the very call that places the centre of the curtain's row at that offset.
///
/// # Errors
///
/// [`BoundaryRefusal::DodShapeMismatch`], [`BoundaryRefusal::TwoAnglesInPlane`],
/// and, in [`BoundaryMode::InPlane`] only, [`BoundaryRefusal::NoAngleInPlane`].
pub fn boundaries(
    axis: &WellAxis,
    node_md: &[f64],
    radius_m: &[f64],
    angle_rad: &[f64],
    per_node: usize,
    mode: BoundaryMode,
    projection: &dyn WellProjection,
) -> Result<Boundaries, BoundaryRefusal> {
    let nodes = node_md.len();
    let shared = angle_rad.len() == per_node;
    if radius_m.len() != nodes * per_node || !(shared || angle_rad.len() == nodes * per_node) {
        return Err(BoundaryRefusal::DodShapeMismatch {
            radii: radius_m.len(),
            angles: angle_rad.len(),
            nodes,
            per_node,
        });
    }
    let mut out = Boundaries {
        above: vec![None; nodes],
        below: vec![None; nodes],
        projected: Vec::new(),
        absent: 0,
        refused: 0,
    };
    let mut any_in_plane = false;
    for n in 0..nodes {
        let md = node_md[n];
        let mut node_refused = false;
        let mut place = |dn: f64| -> Option<BoundaryPoint> {
            match projection.place(axis, md, dn) {
                Ok(at) => Some(BoundaryPoint { md, dn, at }),
                Err(_) => {
                    node_refused = true;
                    None
                }
            }
        };
        let mut above_seen = false;
        let mut below_seen = false;
        let mut projected = Vec::new();
        for k in 0..per_node {
            let theta = angle_rad[if shared { k } else { n * per_node + k }];
            let radius = radius_m[n * per_node + k];
            let usable = radius.is_finite() && radius >= 0.0;
            let side = if wrap(theta).abs() <= ANGLE_TOLERANCE {
                Some(true)
            } else if wrap(theta - PI).abs() <= ANGLE_TOLERANCE {
                Some(false)
            } else {
                None
            };
            if let Some(above) = side {
                any_in_plane = true;
                let seen = if above { &mut above_seen } else { &mut below_seen };
                if *seen {
                    return Err(BoundaryRefusal::TwoAnglesInPlane {
                        node: n,
                        side: if above { "0" } else { "pi" },
                    });
                }
                *seen = true;
                if usable {
                    let point = place(if above { radius } else { -radius });
                    if above {
                        out.above[n] = point;
                    } else {
                        out.below[n] = point;
                    }
                } else {
                    out.absent += 1;
                }
            }
            if mode == BoundaryMode::Projected {
                if usable {
                    if let Some(point) = place(radius * theta.cos()) {
                        projected.push(point);
                    }
                } else if side.is_none() {
                    // A radius on the plane is already counted above.
                    out.absent += 1;
                }
            }
        }
        if mode == BoundaryMode::Projected {
            out.projected.push(projected);
        }
        if node_refused {
            out.refused += 1;
        }
    }
    if mode == BoundaryMode::InPlane && !any_in_plane {
        return Err(BoundaryRefusal::NoAngleInPlane {
            tolerance: ANGLE_TOLERANCE,
        });
    }
    Ok(out)
}
