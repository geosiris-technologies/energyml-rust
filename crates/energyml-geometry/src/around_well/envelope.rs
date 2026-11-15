//! Meshes laid around the well: the envelope (a section of variable radius, one radius per sector and per
//! node) and the ribbon (one column of a cube, laid along the well).
//!
//! `tube::sweep` stays apart: its frame is transported (minimal rotation, arbitrary origin), and an angle
//! counted from the high side has no meaning in it.

use energyml_domain::crs::CompassAxes;

use crate::around_well::hole_frame::{Frames, HoleFrame, north_in_plane};
use crate::around_well::polar::{
    AngleConvention, AngleOrigin, PolarRefusal, polar_points, regular_sectors, to_space, turns_once,
};
use crate::around_well::transverse::{RibbonPlane, TransverseGrid};
use crate::points::Point;
use crate::vector::{cross, dot, sub};

/// Where each sector's angle comes from - a choice the caller makes, never a default (S2).
#[derive(Clone, Copy, Debug)]
pub enum Angles<'a> {
    /// nodes x sectors angles, node slowest.
    PerNode(&'a [f64]),
    /// One angle per sector, the same at every node.
    Shared(&'a [f64]),
    /// Evenly spread, the first sector centred on this angle (radians).
    Regular {
        /// The angle of the first sector's centre.
        first_centre: f64,
    },
}

/// The radii around each node.
#[derive(Clone, Copy, Debug)]
pub struct PolarSamples<'a> {
    /// The number of sectors around each node.
    pub sectors: usize,
    /// nodes x sectors radii in metres, node slowest (TRG 2.2 section 7.1).
    pub radii: &'a [f64],
    /// Where the angles come from.
    pub angles: Angles<'a>,
}

/// A mesh laid around the well: the envelope and the ribbon both.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct EnvelopeMesh {
    /// The points of the mesh.
    pub points: Vec<Point>,
    /// Triangles, as indices into `points`.
    pub triangles: Vec<[u32; 3]>,
    /// For each point, the index of its value in the property's own array.
    pub sample_of: Vec<u32>,
    /// For each triangle, (node, sector) for the envelope, (node, row) for the ribbon, of its first corner.
    pub cell_of: Vec<[u32; 2]>,
    /// Quads not made because a corner had no radius or no frame. Counted, never bridged.
    pub holes: usize,
}

/// Why a mesh cannot be laid, by name.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum EnvelopeRefusal {
    /// Fewer than three sectors.
    #[error("{k} sectors: three are the fewest that close a ring")]
    TooFewSectors {
        /// The number of sectors given.
        k: usize,
    },
    /// The radii do not fill nodes x sectors.
    #[error("{values} radii for {nodes} nodes of {sectors} sectors")]
    CountMismatch {
        /// The number of nodes (frames).
        nodes: usize,
        /// The number of sectors.
        sectors: usize,
        /// The number of radii given.
        values: usize,
    },
    /// The angles do not fit the sectors.
    #[error("{given} angles where {expected} are needed")]
    AnglesCount {
        /// How many are needed.
        expected: usize,
        /// How many were given.
        given: usize,
    },
    /// No angle convention was declared.
    #[error(
        "no angle convention is declared: the origin and the sense of the angles are not stated by the file"
    )]
    ConventionUndeclared,
    /// The angles of a node do not turn one way within one turn.
    #[error("the angles of node {node} do not turn one way within one turn")]
    AnglesNotMonotonic {
        /// The index of the node.
        node: usize,
    },
    /// A radial factor that is not a positive number.
    #[error("a radial factor of {factor} draws nothing true")]
    BadRadialFactor {
        /// The factor given.
        factor: f64,
    },
    /// The index of the fixed dimension of a ribbon is out of range.
    #[error("index {index} is outside the {count} of the fixed dimension")]
    RibbonIndexOutside {
        /// The index asked.
        index: usize,
        /// The count of the fixed dimension.
        count: usize,
    },
    /// A refusal of the polar layer.
    #[error(transparent)]
    Polar(#[from] PolarRefusal),
}

/// The envelope: one ring per node of `frames`, joined node to node, faces looking outward.
///
/// A node whose frame is refused (or whose north is unavailable under a north origin) gives no point; the quads
/// that would touch it, and the ones touching a missing radius, are counted in `holes`, never bridged.
/// `radial_factor` exaggerates the radii (1 for true scale).
pub fn envelope(
    frames: &Frames,
    samples: &PolarSamples,
    convention: Option<&AngleConvention>,
    compass: Option<&CompassAxes>,
    radial_factor: f64,
) -> Result<EnvelopeMesh, EnvelopeRefusal> {
    let k = samples.sectors;
    if k < 3 {
        return Err(EnvelopeRefusal::TooFewSectors { k });
    }
    let n = frames.frames.len();
    if samples.radii.len() != n * k {
        return Err(EnvelopeRefusal::CountMismatch {
            nodes: n,
            sectors: k,
            values: samples.radii.len(),
        });
    }
    let convention = convention.ok_or(EnvelopeRefusal::ConventionUndeclared)?;
    if !radial_factor.is_finite() || radial_factor <= 0.0 {
        return Err(EnvelopeRefusal::BadRadialFactor {
            factor: radial_factor,
        });
    }

    // The angles of every node, checked before anything is laid.
    let regular;
    let angles_of: Vec<&[f64]> = match samples.angles {
        Angles::PerNode(all) => {
            if all.len() != n * k {
                return Err(EnvelopeRefusal::AnglesCount {
                    expected: n * k,
                    given: all.len(),
                });
            }
            (0..n).map(|i| &all[i * k..(i + 1) * k]).collect()
        }
        Angles::Shared(shared) => {
            if shared.len() != k {
                return Err(EnvelopeRefusal::AnglesCount {
                    expected: k,
                    given: shared.len(),
                });
            }
            vec![shared; n]
        }
        Angles::Regular { first_centre } => {
            regular = regular_sectors(k, first_centre);
            vec![regular.as_slice(); n]
        }
    };
    for (node, angles) in angles_of.iter().enumerate() {
        if !turns_once(angles) {
            return Err(EnvelopeRefusal::AnglesNotMonotonic { node });
        }
    }

    let mut mesh = EnvelopeMesh::default();
    // `index[i * k + s]`: the index of the point of (node i, sector s), when there is one.
    let mut index: Vec<Option<u32>> = vec![None; n * k];
    for (i, frame) in frames.frames.iter().enumerate() {
        let Ok(frame) = frame else { continue };
        let north = if convention.origin == AngleOrigin::North {
            match compass.map(|c| north_in_plane(frame, c)) {
                Some(Ok(north)) => Some(north),
                _ => continue,
            }
        } else {
            None
        };
        let ring = polar_points(
            &samples.radii[i * k..(i + 1) * k],
            angles_of[i],
            Some(convention),
            north,
        )?;
        for (s, plane) in ring.points.iter().enumerate() {
            if let Some(plane) = plane {
                let point = to_space(frame, [plane[0] * radial_factor, plane[1] * radial_factor]);
                index[i * k + s] = Some(mesh.points.len() as u32);
                mesh.points.push(point);
                mesh.sample_of.push((i * k + s) as u32);
            }
        }
    }

    for i in 0..n.saturating_sub(1) {
        let centre = frames.frames[i].as_ref().ok();
        for s in 0..k {
            let s1 = (s + 1) % k;
            let corners = [
                index[i * k + s],
                index[i * k + s1],
                index[(i + 1) * k + s1],
                index[(i + 1) * k + s],
            ];
            let (Some(a), Some(b), Some(c), Some(d), Some(centre)) =
                (corners[0], corners[1], corners[2], corners[3], centre)
            else {
                mesh.holes += 1;
                continue;
            };
            push_outward(&mut mesh, [a, b, c, d], centre, [i as u32, s as u32]);
        }
    }
    Ok(mesh)
}

/// Two triangles for the quad `[a, b, c, d]`, wound so that the faces look away from `frame`'s centre
/// whichever way the angles turn. The test is made on the coordinates, as the renderer reads them: it does not
/// depend on the handedness of the placed frame.
fn push_outward(mesh: &mut EnvelopeMesh, quad: [u32; 4], frame: &HoleFrame, cell: [u32; 2]) {
    let [a, b, c, d] = quad;
    let (pa, pb, pc) = (
        mesh.points[a as usize],
        mesh.points[b as usize],
        mesh.points[c as usize],
    );
    let normal = cross(sub(pb, pa), sub(pc, pa));
    if dot(normal, sub(pa, frame.at)) < 0.0 {
        mesh.triangles.push([a, c, b]);
        mesh.triangles.push([a, d, c]);
    } else {
        mesh.triangles.push([a, b, c]);
        mesh.triangles.push([a, c, d]);
    }
    mesh.cell_of.push(cell);
    mesh.cell_of.push(cell);
}

/// The ribbon: one column of the cube per node, laid along the well, seen from both faces (no winding rule).
///
/// `sample_of` indexes the cube's array: node, then the slow dimension, then the fast one (TRG 2.2 section
/// 7.1). A refused node cuts the ribbon; its quads are counted in `holes`.
pub fn ribbon(
    frames: &Frames,
    grid: &TransverseGrid,
    plane: RibbonPlane,
) -> Result<EnvelopeMesh, EnvelopeRefusal> {
    let [m, n] = grid.counts();
    let (rows, fixed, fixed_count) = match plane {
        RibbonPlane::FixedFast(k0) => (m, k0, n),
        RibbonPlane::FixedSlow(j0) => (n, j0, m),
    };
    if fixed >= fixed_count {
        return Err(EnvelopeRefusal::RibbonIndexOutside {
            index: fixed,
            count: fixed_count,
        });
    }
    let nodes = frames.frames.len();
    let mut mesh = EnvelopeMesh::default();
    // `index[i * rows + r]`: the point of (node i, row r).
    let mut index: Vec<Option<u32>> = vec![None; nodes * rows];
    for (i, frame) in frames.frames.iter().enumerate() {
        let Ok(frame) = frame else { continue };
        for r in 0..rows {
            let (j, k) = match plane {
                RibbonPlane::FixedFast(k0) => (r, k0),
                RibbonPlane::FixedSlow(j0) => (j0, r),
            };
            if let Some(point) = grid.lattice_point(frame, j, k) {
                index[i * rows + r] = Some(mesh.points.len() as u32);
                mesh.points.push(point);
                mesh.sample_of.push((i * m * n + j * n + k) as u32);
            }
        }
    }
    for i in 0..nodes.saturating_sub(1) {
        for r in 0..rows.saturating_sub(1) {
            let corners = [
                index[i * rows + r],
                index[i * rows + r + 1],
                index[(i + 1) * rows + r + 1],
                index[(i + 1) * rows + r],
            ];
            let (Some(a), Some(b), Some(c), Some(d)) = (corners[0], corners[1], corners[2], corners[3])
            else {
                mesh.holes += 1;
                continue;
            };
            mesh.triangles.push([a, b, c]);
            mesh.triangles.push([a, c, d]);
            mesh.cell_of.push([i as u32, r as u32]);
            mesh.cell_of.push([i as u32, r as u32]);
        }
    }
    Ok(mesh)
}
