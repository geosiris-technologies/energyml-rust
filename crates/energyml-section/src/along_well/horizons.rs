//! Horizons cut by the curtain, and the tops of the hole itself.
//!
//! No solver is written here: a column of the curtain is a line (`P + dn h`),
//! and `energyml_analysis::tops::tops` finds where a line crosses a surface,
//! `dn` playing the part of the measured depth.

use energyml_analysis::heights::{Heights, Surface};
use energyml_analysis::tops::{Side, Tops, TopsError, tops};
use energyml_core::value::Reflect;
use energyml_domain::ArraySource;
use energyml_geometry::Topology;
use energyml_geometry::around_well::hole_frame::{MIN_INCLINATION_DEGREES, high_side_of, offset_point};
use energyml_geometry::around_well::projection::{WellAxis, WellProjection};
use energyml_geometry::scene::{self, Placement};
use energyml_geometry::well::PlacedPath;

use super::model::AlongWellRefusal;

/// A surface read and placed like the path (metres, z up): the caller's, or [`PlacedMesh::read`].
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedMesh {
    /// The uri of the surface.
    pub uri: String,
    /// `[x, y, z, ...]`, placed.
    pub points: Vec<f64>,
    /// Triangles, by three indices into the points.
    pub triangles: Vec<u32>,
    /// Quadrilaterals, by four indices into the points.
    pub quads: Vec<u32>,
}

impl PlacedMesh {
    /// Read and placed by the one door of the geometry: `scene::object_of`, then
    /// each patch's points through `SceneObject::geometry_in` with
    /// `Placement::Metres`; triangles and quadrilaterals kept as the file has
    /// them (a 2D grid's cells are not halved: [`PlacedMesh::heights`] reads
    /// them bilinearly).
    ///
    /// # Errors
    ///
    /// [`AlongWellRefusal::NotInPackage`], [`AlongWellRefusal::Read`] when the
    /// shape cannot be read or placed in metres, [`AlongWellRefusal::NoFaces`]
    /// for a shape with neither triangle nor quadrilateral.
    pub fn read(source: &dyn ArraySource, uri: &str) -> Result<PlacedMesh, AlongWellRefusal> {
        let object = source
            .object_at(uri)
            .ok_or_else(|| AlongWellRefusal::NotInPackage { uri: uri.to_string() })?;
        let reflect: &dyn Reflect = object.as_reflect();
        let read = |why: String| AlongWellRefusal::Read {
            uri: uri.to_string(),
            why,
        };
        let shape = scene::object_of(reflect, source, uri).map_err(|why| read(why.to_string()))?;
        let placed = shape
            .geometry_in(Placement::Metres)
            .map_err(|why| read(why.to_string()))?;
        let mut points: Vec<f64> = Vec::new();
        let mut triangles: Vec<u32> = Vec::new();
        let mut quads: Vec<u32> = Vec::new();
        for patch in &placed.patches {
            let offset = (points.len() / 3) as u32;
            for point in &patch.points {
                points.extend_from_slice(point);
            }
            match &patch.topology {
                Topology::Triangles(faces) => {
                    for face in faces {
                        triangles.extend(face.iter().map(|index| index + offset));
                    }
                }
                Topology::Quads(faces) => {
                    for face in faces {
                        quads.extend(face.iter().map(|index| index + offset));
                    }
                }
                _ => {}
            }
        }
        if triangles.is_empty() && quads.is_empty() {
            return Err(AlongWellRefusal::NoFaces { uri: uri.to_string() });
        }
        Ok(PlacedMesh {
            uri: uri.to_string(),
            points,
            triangles,
            quads,
        })
    }

    /// The surface indexed to answer heights.
    ///
    /// # Errors
    ///
    /// The refusal of `Heights::of` (malformed arrays, no face).
    pub fn heights(&self) -> Result<Heights<'_>, energyml_analysis::Error> {
        Heights::of(
            Surface {
                points: &self.points,
                triangles: &self.triangles,
                quads: &self.quads,
            },
            "the horizon",
            "a horizon cut by the curtain",
        )
    }
}

/// One point of a horizon in the curtain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HorizonPoint {
    /// The measured depth of the column.
    pub md: f64,
    /// The offset along the high side where the column crosses the horizon.
    pub dn: f64,
    /// Placed by the projection: `[abscissa, tvd]`.
    pub at: [f64; 2],
    /// The side the column comes from, going up (`tops::Side`).
    pub from: Side,
}

/// What a column left undecided: `Tops`'s own answers, kept.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Undecided {
    /// A crossing is somewhere in this span of `dn`, over a fold of the surface.
    Folded {
        /// The measured depth of the column.
        md: f64,
        /// `[from, to]` in `dn`.
        dn: [f64; 2],
    },
    /// A crossing was bracketed, the bisection met no single height.
    Unresolved {
        /// The measured depth of the column.
        md: f64,
        /// `[from, to]` in `dn`.
        dn: [f64; 2],
    },
    /// The column changes side where the surface is not.
    Gap {
        /// The measured depth of the column.
        md: f64,
        /// `[from, to]` in `dn`.
        dn: [f64; 2],
    },
    /// The column meets the surface and leaves on the side it came from.
    Touch {
        /// The measured depth of the column.
        md: f64,
        /// The offset of the touch.
        dn: f64,
    },
}

/// A horizon as the curtain sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct HorizonTrace {
    /// The uri of the surface.
    pub surface: String,
    /// Runs of crossings joined column to column.
    pub spans: Vec<Vec<HorizonPoint>>,
    /// What the columns left undecided.
    pub undecided: Vec<Undecided>,
    /// Columns with no high side (S4): not looked at, counted.
    pub columns_without_frame: usize,
    /// Places where two neighbouring columns had crossings that could not be paired.
    pub breaks: usize,
}

/// The horizon in the plane of the curtain: for each column, the line `P + dn h`
/// over `dn_range` is crossed with the surface by `tops::tops`, each crossing
/// placed by `projection`, and the crossings of neighbouring columns joined
/// when they pair rank by rank (same number of crossings, same side).
///
/// # Errors
///
/// The refusals of `tops::tops` (a step that is not positive, a range that is
/// not a stretch, too many samples).
#[allow(clippy::too_many_arguments)]
pub fn horizon_in_curtain(
    path: &PlacedPath,
    axis: &WellAxis,
    columns_md: &[f64],
    dn_range: [f64; 2],
    step: f64,
    surface: &Heights,
    uri: &str,
    projection: &dyn WellProjection,
) -> Result<HorizonTrace, TopsError> {
    let mut trace = HorizonTrace {
        surface: uri.to_string(),
        spans: Vec::new(),
        undecided: Vec::new(),
        columns_without_frame: 0,
        breaks: 0,
    };
    let min_inclination = MIN_INCLINATION_DEGREES.to_radians();
    // The runs open at the previous column, one per crossing, and their sides.
    let mut open: Vec<Vec<HorizonPoint>> = Vec::new();
    for &md in columns_md {
        let has_frame = path
            .direction_at(md)
            .is_some_and(|tangent| high_side_of(md, tangent, min_inclination).is_ok());
        if !has_frame {
            trace.columns_without_frame += 1;
            trace.spans.append(&mut open);
            continue;
        }
        let column = |dn: f64| offset_point(path, md, dn).unwrap_or([f64::NAN; 3]);
        let found = tops(&column, &[dn_range[0], dn_range[1]], step, surface)?;

        let mut points: Vec<HorizonPoint> = Vec::new();
        let mut all_placed = true;
        for crossing in &found.crossings {
            match projection.place(axis, md, crossing.md) {
                Ok(at) => points.push(HorizonPoint {
                    md,
                    dn: crossing.md,
                    at,
                    from: crossing.from,
                }),
                Err(_) => all_placed = false,
            }
        }
        for span in &found.folded {
            trace.undecided.push(Undecided::Folded { md, dn: *span });
        }
        for span in &found.unresolved {
            trace.undecided.push(Undecided::Unresolved { md, dn: *span });
        }
        for span in &found.gaps {
            trace.undecided.push(Undecided::Gap { md, dn: *span });
        }
        for &dn in &found.touches {
            trace.undecided.push(Undecided::Touch { md, dn });
        }

        let pairs = all_placed
            && !open.is_empty()
            && open.len() == points.len()
            && open
                .iter()
                .zip(&points)
                .all(|(run, point)| run.last().is_some_and(|last| last.from == point.from));
        if pairs {
            for (run, point) in open.iter_mut().zip(points) {
                run.push(point);
            }
        } else {
            if !open.is_empty() && !points.is_empty() {
                trace.breaks += 1;
            }
            trace.spans.append(&mut open);
            open = points.into_iter().map(|point| vec![point]).collect();
        }
    }
    trace.spans.append(&mut open);
    Ok(trace)
}

/// Where the hole itself crosses a surface: `tops::tops` over the placed path,
/// reused as it is, from the first to the last knot inside `md_range`.
///
/// # Errors
///
/// The refusals of `tops::tops`.
pub fn computed_tops(
    path: &PlacedPath,
    md_range: [f64; 2],
    step: f64,
    surface: &Heights,
) -> Result<Tops, TopsError> {
    let mut knots = vec![md_range[0]];
    knots.extend(
        path.knots()
            .iter()
            .copied()
            .filter(|&knot| knot > md_range[0] && knot < md_range[1]),
    );
    knots.push(md_range[1]);
    tops(&|md| path.at(md).point, &knots, step, surface)
}
