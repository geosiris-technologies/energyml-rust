//! Wells whose only spatial anchor is a reference point.
//!
//! RESQML 2.2 makes `Geometry` optional on a `WellboreTrajectoryRepresentation`
//! and puts the obligation elsewhere - *"BUSINESS RULE: For purposes of the
//! trajectory the MdDatum within the MdInterval is mandatory"* (Technical
//! Reference Guide §12.17). Real packages use exactly that: the BRGM export
//! carries 120 trajectories that are nothing but an `MdInterval` and a
//! [`Datum`] pointing at a fully georeferenced `ReferencePointInACrs`, plus a
//! `WellboreFrameRepresentation` per well holding the measured depths its logs
//! hang on.
//!
//! # The assumption, stated
//!
//! A trajectory with a measured-depth interval and a wellhead, and **no other
//! spatial information anywhere**, is evaluated as a **vertical** well: down
//! from the reference point, one metre of depth per metre of measured depth.
//! That is an assumption, not arithmetic - a deviated well whose geometry was
//! simply not exported comes out wrong. It is taken because (a) it is the only
//! evaluation the data permits, (b) the producers that write this shape write
//! it for shallow vertical boreholes, and (c) the alternative - refusing 120
//! wells that carry everything needed to place them - helps nobody. A
//! trajectory that hints at a real shape elsewhere (`WitsmlTrajectory`,
//! `ParentIntersection`) is refused by name instead.
//!
//! # Two conventions, and why they do not clash
//!
//! A [`WellboreFrameRepresentation`] takes whichever of the two shapes its
//! trajectory has. When the trajectory carries a `Geometry`, the frame's nodes
//! are that parametric line evaluated at the frame's measured depths, and they
//! come out in the line's **local CRS as stored**, like every other
//! representation in this crate. When it carries none, both the trajectory and
//! the frame go through `Anchor`, which normalises to **z-up** because a
//! `ReferencePointInACrs` has no local CRS to defer to - and which **places** a
//! `ReferencePointInALocalEngineeringCompoundCrs` through the compound it is
//! written in, so that the two eml 2.3 datums come out in the same
//! convention: projected x and y, an elevation in metres.
//!
//! So z means different things in the two cases - but never within one
//! package, and never between a frame and the trajectory it hangs on, which is
//! the only pairing that has to line up on screen.
//!
//! [`WellboreFrameRepresentation`]: https://docs.energistics.org
//!
//! [`Datum`]: https://docs.energistics.org

use energyml_core::value::Reflect;

use crate::error::{Error, Result};
use crate::geometry::{Patch, Topology};
use crate::points::Point;
use crate::source::ValueSource;
use crate::values::{self, scalar_f64};
use energyml_domain::crs::CrsKind;
use energyml_domain::units;

/// A no-`Geometry` trajectory: the vertical segment from `MdMin` to `MdMax`
/// below the reference point.
pub(crate) fn datum_trajectory(
    representation: &dyn Reflect,
    source: &dyn ValueSource,
    at: &str,
) -> Result<Vec<Patch>> {
    // Anything that says the shape lives elsewhere makes "vertical" a guess
    // rather than the only reading, so those are refused by name.
    for field in ["WitsmlTrajectory", "ParentIntersection"] {
        if representation
            .field(field)
            .is_some_and(|value| value.as_object().is_some())
        {
            return Err(Error::unsupported(
                "a wellbore trajectory whose shape is deferred to another object",
                format!("{at}.{field}"),
            ));
        }
    }
    let anchor = Anchor::of(representation, source, at)?;
    let top = anchor.at(anchor.md_min);
    let bottom = anchor.at(anchor.md_max);
    Ok(vec![Patch {
        index: 0,
        node_offset: 0,
        points: vec![top, bottom],
        topology: Topology::Polylines(vec![2]),
        closed: Vec::new(),
    }])
}

/// Where a trajectory is collared: `[x, y, elevation]` of the datum its
/// `MdInterval` names, the elevation **z-up** whatever the file's vertical CRS
/// says - in the coordinates of that datum's CRS, and for a datum written in a
/// local engineering compound, placed through that compound into its
/// projected system, the elevation in metres.
///
/// The reading that draws the well, made public for what tabulates it
/// (`crate::wells`): a second reader of the datum would be a second
/// answer to *where is this well*, in the two shapes the releases give it.
pub fn head_of(trajectory: &dyn Reflect, source: &dyn ValueSource) -> Result<[f64; 3]> {
    let anchor = Anchor::of(trajectory, source, "WellboreTrajectoryRepresentation")?;
    Ok([anchor.x, anchor.y, anchor.elevation])
}

/// **A trajectory's own line, evaluable at any measured depth** - the reading
/// `frame` places a frame's nodes with, for what needs a depth between two
/// stations (a computed top).
///
/// Two refusals, and both are the file's silence rather than a gap here:
///
/// - **no `Geometry`**: the only placement left is the vertical reading from
///   the datum this module documents - an assumption, and one written in
///   placed, z-up coordinates where a geometry's line is local to its CRS. A
///   number computed on it would carry the assumption without saying so;
/// - **no `ControlPointParameters`**: the knots are then 0, 1, 2 … (what the
///   schema says to assume), and no parameter along the line is a measured
///   depth. The contract between a trajectory and its frames is that the
///   parameter *is* the MD (the contract `energyml-build` relies on).
pub fn path_of(trajectory: &dyn Reflect, source: &dyn ValueSource) -> Result<crate::parametric::Path> {
    let at = "WellboreTrajectoryRepresentation";
    let Some(geometry) = crate::geometry::field_object(trajectory, "Geometry") else {
        return Err(Error::unsupported(
            "a wellbore trajectory with no `Geometry` - its only placement is the vertical reading from its datum, an assumption",
            at,
        ));
    };
    let path = crate::parametric::Path::of(geometry, source)?;
    if path.knots().is_none() {
        return Err(Error::malformed(
            at,
            "a line that states no `ControlPointParameters`: its knots are then 0, 1, 2 …, and no parameter along it is a measured depth",
        ));
    }
    Ok(path)
}

/// Where a measured depth falls against the depths a path states.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum MdRange {
    /// Between the first and the last knot, both included.
    Within,
    /// Above the first knot: the point is the first interval extended (`Path::at`), and says so.
    BeforeFirst {
        /// The first knot.
        first: f64,
    },
    /// Below the last knot: the last interval extended.
    AfterLast {
        /// The last knot.
        last: f64,
    },
    /// The depth asked is not a number.
    NotANumber,
}

/// A placed point at a measured depth, with where that depth falls.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PointAtMd {
    /// The measured depth asked, in the path's MD unit.
    pub md: f64,
    /// Placed, in metres, z up.
    pub point: Point,
    /// Within the stated depths, or extended past them.
    pub range: MdRange,
}

/// Where a WITSML survey stands: what `stations` reads from the file or the caller declares.
/// The one placement type of a survey.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SurveyPlacement {
    /// MD 0 - the datum - placed: first and second projected coordinates and the elevation, all in metres.
    pub datum: Point,
    /// Where the survey's azimuth 90 degrees and 0 degrees point in the placed plane (grid, plant or declared north).
    pub axes: energyml_domain::crs::CompassAxes,
}

impl SurveyPlacement {
    /// From the datum frame (metres, x east and y north of the survey's north, z up, origin at the
    /// datum) to the placed frame: `datum + x * east + y * north` horizontally, `datum.z + z` vertically.
    pub fn place(&self, local: Point) -> Point {
        let (east, north) = (self.axes.east, self.axes.north);
        [
            self.datum[0] + local[0] * east[0] + local[1] * north[0],
            self.datum[1] + local[0] * east[1] + local[1] * north[1],
            self.datum[2] + local[2],
        ]
    }

    /// The inverse; `None` when east and north are parallel (never for a placement `from_local`
    /// accepts, checked all the same).
    pub fn to_local(&self, placed: Point) -> Option<Point> {
        let (e, n) = (self.axes.east, self.axes.north);
        let det = e[0] * n[1] - n[0] * e[1];
        if !det.is_finite() || det.abs() < 1e-12 {
            return None;
        }
        let (dx, dy) = (placed[0] - self.datum[0], placed[1] - self.datum[1]);
        // [[e0, n0], [e1, n1]] [x, y] = [dx, dy], by Cramer's rule.
        let x = (dx * n[1] - n[0] * dy) / det;
        let y = (e[0] * dy - e[1] * dx) / det;
        Some([x, y, placed[2] - self.datum[2]])
    }
}

/// Why a path cannot be placed. Each variant names what is missing.
#[derive(Debug, thiserror::Error)]
pub enum PathRefusal {
    /// `path_of` refused, or the CRS could not be read.
    #[error("the trajectory's line cannot be read: {0}")]
    Read(#[from] Error),
    /// The trajectory names no CRS.
    #[error(
        "the trajectory names no coordinate system: which way is up and where the well stands are not stated - declare one (`PlacedPath::new`)"
    )]
    NoCrs,
    /// A time CRS.
    #[error("the trajectory is written in the time CRS {crs}: a measured depth is a length")]
    TimeCrs {
        /// The uuid of the CRS.
        crs: String,
    },
    /// A unit of the CRS that is not a length this build converts.
    #[error("a unit of the CRS {crs} is not a length this build converts to metres: {source}")]
    Unit {
        /// The uuid of the CRS.
        crs: String,
        /// The refusal of `units::metres`.
        source: energyml_domain::Error,
    },
    /// No parameters along the line.
    #[error("the line states no parameters: no parameter along it is a measured depth")]
    NotParametrised,
    /// The survey's stations.
    #[error("the survey's stations cannot be joined: {0}")]
    Stations(#[from] crate::min_curvature::StationsError),
    /// A survey that does not start at its datum.
    #[error("the first station is at MD {md}, not at the datum: where the hole runs above it is not stated")]
    FirstStationNotAtDatum {
        /// The first station's MD.
        md: f64,
    },
    /// A placement that is no placement.
    #[error("the survey placement cannot be used: {why}")]
    BadPlacement {
        /// What is wrong with it.
        why: &'static str,
    },
}

/// How a path is placed in metres, z up.
#[derive(Clone, Debug)]
// One placing per placed path, and a path per well: the size of the larger variant costs nothing, and boxing
// the CRS would add an indirection to every evaluation.
#[allow(clippy::large_enum_variant)]
enum Placing {
    /// A RESQML line, local to its CRS: placed at each evaluation, `to_projected` then the unit factors.
    Crs {
        crs: energyml_domain::Crs,
        horizontal: f64,
        vertical: f64,
    },
    /// A line in a survey's datum frame (metres, x east, y north, z up), placed by
    /// `SurveyPlacement::place` at each evaluation.
    Survey(SurveyPlacement),
}

/// A trajectory placed in metres, z up, evaluable at any measured depth: the one door from a
/// RESQML trajectory or a WITSML survey to where the hole is and where it points.
///
/// Positions, directions and offsets are in the placed frame of [`energyml_domain::Crs::to_projected`]
/// converted to metres. That frame may be left-handed (see [`PlacedPath::handedness`]): nothing here
/// computes a cross product on its coordinates.
#[derive(Clone, Debug)]
pub struct PlacedPath {
    path: crate::parametric::Path,
    placing: Placing,
    knots: Vec<f64>,
    md_uom: String,
    axes: energyml_domain::crs::AxisReading,
    compass: Option<energyml_domain::crs::CompassAxes>,
    handedness: Option<energyml_domain::crs::Handedness>,
    datum: Option<Point>,
}

impl PlacedPath {
    /// A RESQML `WellboreTrajectoryRepresentation`, placed through the CRS its geometry names.
    pub fn of(
        trajectory: &dyn Reflect,
        source: &dyn ValueSource,
    ) -> std::result::Result<PlacedPath, PathRefusal> {
        let path = path_of(trajectory, source)?;
        let crs = match energyml_domain::crs::of(trajectory, source) {
            Err(error) => return Err(PathRefusal::Read(Error::Domain(error))),
            Ok(None) => return Err(PathRefusal::NoCrs),
            Ok(Some(crs)) => crs,
        };
        PlacedPath::new(path, crs, &md_unit(trajectory))
    }

    /// A line and the CRS it is written in - the file's, or one the caller declares (S2): the line's
    /// vertical reading is set to that CRS's (`Path::with_vertical`).
    pub fn new(
        path: crate::parametric::Path,
        crs: energyml_domain::Crs,
        md_uom: &str,
    ) -> std::result::Result<PlacedPath, PathRefusal> {
        if crs.is_time() {
            return Err(PathRefusal::TimeCrs {
                crs: crs.uuid.clone(),
            });
        }
        let factor = |unit: &Option<String>| -> std::result::Result<f64, PathRefusal> {
            match unit {
                Some(unit) => units::metres(1.0, unit).map_err(|source| PathRefusal::Unit {
                    crs: crs.uuid.clone(),
                    source,
                }),
                None => Ok(1.0),
            }
        };
        let horizontal = factor(&crs.horizontal_uom)?;
        let vertical = factor(&crs.vertical_uom)?;
        let knots = match path.knots() {
            Some(knots) if !knots.is_empty() => knots.to_vec(),
            _ => return Err(PathRefusal::NotParametrised),
        };
        let path = path.with_vertical(crate::parametric::Vertical::reading(Some(&crs)));
        let axes = crs.axes.clone();
        let compass = axes.stated().map(energyml_domain::crs::AxisOrder::compass);
        let handedness = axes.stated().map(energyml_domain::crs::AxisOrder::handedness);
        let mut placed = PlacedPath {
            path,
            placing: Placing::Crs {
                crs,
                horizontal,
                vertical,
            },
            knots,
            md_uom: md_uom.to_string(),
            axes,
            compass,
            handedness,
            datum: None,
        };
        // A trajectory that starts below MD 0 has no datum readable here: the projections in TVD
        // refuse `NoDatum`, never an extrapolation.
        if placed.knots[0] <= 0.0 && 0.0 <= placed.knots[placed.knots.len() - 1] {
            placed.datum = Some(placed.at(0.0).point);
        }
        Ok(placed)
    }

    /// A line already in a survey's datum frame (metres, x east, y north, z up, MD as parameter,
    /// MD 0 at the datum), placed by `placement`.
    pub fn from_local(
        local: crate::parametric::Path,
        placement: &SurveyPlacement,
    ) -> std::result::Result<PlacedPath, PathRefusal> {
        if !placement.datum.iter().all(|c| c.is_finite()) {
            return Err(PathRefusal::BadPlacement {
                why: "the datum is not a finite point",
            });
        }
        let Some(handedness) = placement.axes.handedness() else {
            return Err(PathRefusal::BadPlacement {
                why: "east and north must be unit vectors at right angles",
            });
        };
        let knots = match local.knots() {
            Some(knots) if !knots.is_empty() => knots.to_vec(),
            _ => return Err(PathRefusal::NotParametrised),
        };
        if knots[0] != 0.0 {
            return Err(PathRefusal::FirstStationNotAtDatum { md: knots[0] });
        }
        Ok(PlacedPath {
            path: local,
            placing: Placing::Survey(*placement),
            knots,
            // The MDs of `Measured` are in metres.
            md_uom: "m".to_string(),
            axes: energyml_domain::crs::AxisReading::NotStated,
            compass: Some(placement.axes),
            handedness: Some(handedness),
            datum: Some(placement.datum),
        })
    }

    /// A WITSML survey: its stations (MD in metres, angles in degrees) joined by the minimum-curvature
    /// arc (`Path::new` kind 5), placed by `placement`: `from_local` of the survey's arc.
    pub fn from_stations(
        stations: &[crate::min_curvature::Measured],
        placement: &SurveyPlacement,
    ) -> std::result::Result<PlacedPath, PathRefusal> {
        PlacedPath::from_local(arc_of(stations)?, placement)
    }

    /// The placed point at `md`, and where `md` falls.
    pub fn at(&self, md: f64) -> PointAtMd {
        PointAtMd {
            md,
            point: self.place(self.path.at(md)),
            range: self.range_of(md),
        }
    }

    /// The unit direction of the hole at `md`, placed, towards increasing MD. `None` where the line has none.
    pub fn direction_at(&self, md: f64) -> Option<[f64; 3]> {
        let local = self.path.direction_at(md)?;
        let scaled = match &self.placing {
            Placing::Crs {
                crs,
                horizontal,
                vertical,
            } => {
                // Scaled before it is normalised: a CRS in feet across and metres down tilts it.
                let v = crs.direction_to_projected(local);
                [v[0] * horizontal, v[1] * horizontal, v[2] * vertical]
            }
            Placing::Survey(placement) => {
                let (e, n) = (placement.axes.east, placement.axes.north);
                [
                    local[0] * e[0] + local[1] * n[0],
                    local[0] * e[1] + local[1] * n[1],
                    local[2],
                ]
            }
        };
        crate::vector::unit(scaled)
    }

    /// Where `md` falls against the knots.
    pub fn range_of(&self, md: f64) -> MdRange {
        let (first, last) = (self.knots[0], self.knots[self.knots.len() - 1]);
        if md.is_nan() {
            MdRange::NotANumber
        } else if md < first {
            MdRange::BeforeFirst { first }
        } else if md > last {
            MdRange::AfterLast { last }
        } else {
            MdRange::Within
        }
    }

    /// The knots, ascending.
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }

    /// `[first knot, last knot]`.
    pub fn md_extent(&self) -> [f64; 2] {
        [self.knots[0], self.knots[self.knots.len() - 1]]
    }

    /// The unit the MDs are in, as the file spells it (`m` for a survey).
    pub fn md_uom(&self) -> &str {
        &self.md_uom
    }

    /// What the CRS says of its horizontal axes (`NotStated` for a survey: see [`PlacedPath::compass`]).
    pub fn axes(&self) -> &energyml_domain::crs::AxisReading {
        &self.axes
    }

    /// East and north in the placed plane, when known.
    pub fn compass(&self) -> Option<energyml_domain::crs::CompassAxes> {
        self.compass
    }

    /// Whether the placed frame is direct, when known.
    pub fn handedness(&self) -> Option<energyml_domain::crs::Handedness> {
        self.handedness
    }

    /// MD 0 placed, when the path covers it.
    pub fn datum(&self) -> Option<Point> {
        self.datum
    }

    /// True vertical depth below the MD datum (S6): `datum.z - at(md).z`. `None` without a datum, or
    /// where the point has no position.
    pub fn tvd(&self, md: f64) -> Option<f64> {
        let datum = self.datum?;
        Some(datum[2] - self.at(md).point[2]).filter(|tvd| tvd.is_finite())
    }

    /// The line itself (local to its CRS for a RESQML path).
    pub fn path(&self) -> &crate::parametric::Path {
        &self.path
    }

    /// The CRS, for a RESQML path.
    pub fn crs(&self) -> Option<&energyml_domain::Crs> {
        match &self.placing {
            Placing::Crs { crs, .. } => Some(crs),
            Placing::Survey(_) => None,
        }
    }

    /// A local point of the line, placed in metres, z up.
    fn place(&self, local: Point) -> Point {
        match &self.placing {
            Placing::Crs {
                crs,
                horizontal,
                vertical,
            } => {
                let [x, y, z] = crs.to_projected(local);
                [x * horizontal, y * horizontal, z * vertical]
            }
            Placing::Survey(placement) => placement.place(local),
        }
    }
}

/// The arc of a survey's stations in the datum frame (metres, x east, y north, z up): the stations
/// joined by `Path::new` kind 5, every check of `min_curvature::positions` made first.
fn arc_of(
    stations: &[crate::min_curvature::Measured],
) -> std::result::Result<crate::parametric::Path, PathRefusal> {
    let located = crate::min_curvature::positions(stations)?;
    let mut points = Vec::with_capacity(stations.len());
    let mut tangents = Vec::with_capacity(stations.len());
    let mut mds = Vec::with_capacity(stations.len());
    for (station, at) in stations.iter().zip(&located) {
        // `positions` has refused an inclined station with no azimuth: `unwrap_or(0.0)` only meets
        // vertical stations, where the azimuth moves nothing.
        let t = crate::min_curvature::direction(
            station.inclination.unwrap_or(0.0),
            station.azimuth.unwrap_or(0.0),
        );
        points.push([at.east, at.north, -at.tvd]);
        tangents.push([t[1], t[0], -t[2]]);
        mds.push(station.md);
    }
    Ok(crate::parametric::Path::new(
        5,
        points,
        mds,
        Some(tangents),
        crate::parametric::Vertical::Up,
    )?)
}

/// The unit the trajectory's measured depths are in: `MdInterval.Uom` (2.2) or `MdUom` (2.0.1 and the
/// drops), `m` when neither is written - the reading `Anchor::of` has always made.
fn md_unit(trajectory: &dyn Reflect) -> String {
    let interval = crate::geometry::field_object(trajectory, MD_INTERVAL).unwrap_or(trajectory);
    string_any(interval, MD_UOM).unwrap_or_else(|| "m".to_string())
}

/// A `WellboreFrameRepresentation`: its measured depths, placed on the well.
///
/// The frame is where the logs hang - one property value per node - so
/// evaluating it is what makes well data drawable at all. The nodes come out
/// as one polyline, top to bottom, in the order the frame stores them.
pub(crate) fn frame(representation: &dyn Reflect, source: &dyn ValueSource) -> Result<Vec<Patch>> {
    let at = "WellboreFrameRepresentation";
    let Some(node_md) = crate::geometry::field_object(representation, "NodeMd") else {
        return Err(Error::malformed(at, "a frame with no `NodeMd`"));
    };
    let mds = values::read_doubles(node_md, source, &format!("{at}.NodeMd"))?;
    if mds.is_empty() {
        return Err(Error::malformed(at, "a frame with no measured depth"));
    }

    let Some(trajectory_uri) = reference_of(representation, "Trajectory") else {
        return Err(Error::malformed(at, "a frame that names no trajectory"));
    };
    let Some(trajectory) = source.object_at(&trajectory_uri) else {
        return Err(Error::Domain(energyml_domain::Error::Missing(format!(
            "the trajectory {trajectory_uri}"
        ))));
    };
    let trajectory = trajectory.as_reflect();

    // Two ways to place a measured depth, and which one applies is decided by
    // the trajectory, not by the frame.
    let points: Vec<Point> = match crate::geometry::field_object(trajectory, "Geometry") {
        // The trajectory has a shape of its own. The parameter of its
        // parametric line **is** the measured depth - RESQML says so, and the
        // corpus confirms it: `acd2cdcf`'s parameters run 0…1000 and its
        // frames sample 0, 200 … 1000 of the same well. So the frame's nodes
        // are the line evaluated at its own MDs, in the line's local CRS,
        // which is the space the trajectory itself comes out in - a log has to
        // land *on* the well path, and that is the property that matters.
        //
        // The frame states no unit for `NodeMd`; the only MD unit in reach is
        // the trajectory's, and both describe depths along the same wellbore,
        // so they are read as being in it. A frame that meant something else
        // would have no way to say so.
        Some(geometry) => {
            let line = crate::parametric::Line::single(geometry, source, at)?;
            mds.iter().map(|md| line.at(*md)).collect()
        }
        // No shape anywhere: the vertical reading this module documents, from
        // the wellhead down. Its points are z-up by construction, matching
        // `datum_trajectory`, so frame and well still agree with each other.
        None => {
            let anchor = Anchor::of(trajectory, source, at)?;
            mds.iter().map(|md| anchor.at(*md)).collect()
        }
    };
    let count = points.len() as u32;
    Ok(vec![Patch {
        index: 0,
        node_offset: 0,
        points,
        topology: Topology::Polylines(vec![count]),
        closed: Vec::new(),
    }])
}

/// How many points sample a deviated trajectory's head portion.
///
/// Enough to show a bend without pretending to a precision the segment does
/// not have - this portion is already drawn differently from the frame's own
/// nodes (see the front end), so a coarse sample is honest rather than a shortcut.
const HEAD_SAMPLES: usize = 17;

/// The portion of a frame's trajectory from MD 0 down to its first node, in
/// the frame the frame's own nodes are written in, and whether the package
/// states it.
#[derive(Clone, PartialEq, Debug)]
pub enum HeadPortion {
    /// The trajectory's own line covers MD 0 to the first node (`MdMin` at or
    /// above 0): points of that line, sampled like `frame` samples its nodes -
    /// stated by the package.
    Described(Vec<Point>),
    /// Something in the portion is stated by no object. With no `Geometry`,
    /// the straight vertical segment below the datum this module documents for
    /// the whole well. With a line that starts below MD 0, the straight chord
    /// from the datum, brought into the line's frame, to where the line
    /// starts, then the line itself down to the first node. The first point is
    /// MD 0, the last is the frame's first node.
    Assumed(Vec<Point>),
}

impl HeadPortion {
    /// The points, whichever the case.
    pub fn points(&self) -> &[Point] {
        match self {
            HeadPortion::Described(points) | HeadPortion::Assumed(points) => points,
        }
    }

    /// The points, owned.
    pub fn into_points(self) -> Vec<Point> {
        match self {
            HeadPortion::Described(points) | HeadPortion::Assumed(points) => points,
        }
    }

    /// Whether the package states this portion.
    pub fn is_described(&self) -> bool {
        matches!(self, HeadPortion::Described(_))
    }
}

/// `HEAD_SAMPLES` points of `line` from parameter `from` to parameter `to`,
/// the last one being `to` exactly: `from + (to - from)` can differ from `to`
/// in floating point as soon as `from` is not 0, and the last point has to be
/// the frame's first node to the bit.
fn samples(line: &crate::parametric::Line, from: f64, to: f64) -> Vec<Point> {
    let last = HEAD_SAMPLES - 1;
    (0..HEAD_SAMPLES)
        .map(|i| {
            if i == last {
                to
            } else {
                from + (to - from) * (i as f64) / (last as f64)
            }
        })
        .map(|md| line.at(md))
        .collect()
}

/// **The portion of a frame's trajectory from measured depth 0 to its first
/// node** - the span a CSV import kept no stratigraphic unit for (on
/// `forages-unites.epc`, 2 654 of 2 699 frames start below ground, median
/// 14.5 m, because the units their MD interval covers do not start at
/// the wellhead).
///
/// The package describes this interval when the trajectory's own line reaches
/// MD 0 ([`HeadPortion::Described`]). Otherwise the datum, which is where MD 0
/// is - by the business rule that makes `MdInterval.Datum` mandatory (RESQML
/// TRG 2.2 §12.17 p. 314, quoted at the top of this module) - is joined to the
/// line by a straight chord that no object states, written in the line's own
/// frame so that a caller places it with the frame's nodes
/// ([`HeadPortion::Assumed`]): the same holds for a trajectory with no
/// `Geometry`, whose portion is the vertical segment this module assumes for
/// the whole well.
///
/// Every point is in the frame the frame's nodes are in: the caller places the
/// portion by the same `place_points` it places the nodes with.
///
/// `Ok(None)` when there is nothing missing - the frame already starts at or
/// below MD 0 - and a named [`Error`] when the datum cannot be expressed in the
/// frame of the line, rather than a guess.
pub fn head_portion(representation: &dyn Reflect, source: &dyn ValueSource) -> Result<Option<HeadPortion>> {
    let at = "WellboreFrameRepresentation";
    let Some(node_md) = crate::geometry::field_object(representation, "NodeMd") else {
        return Err(Error::malformed(at, "a frame with no `NodeMd`"));
    };
    let mds = values::read_doubles(node_md, source, &format!("{at}.NodeMd"))?;
    let Some(&first_md) = mds.first() else {
        return Err(Error::malformed(at, "a frame with no measured depth"));
    };
    if first_md <= 0.0 {
        return Ok(None);
    }

    let Some(trajectory_uri) = reference_of(representation, "Trajectory") else {
        return Err(Error::malformed(at, "a frame that names no trajectory"));
    };
    let Some(trajectory) = source.object_at(&trajectory_uri) else {
        return Err(Error::Domain(energyml_domain::Error::Missing(format!(
            "the trajectory {trajectory_uri}"
        ))));
    };
    let trajectory = trajectory.as_reflect();

    let portion = match crate::geometry::field_object(trajectory, "Geometry") {
        // No shape anywhere: the vertical reading this module documents.
        None => match Anchor::of(trajectory, source, "WellboreTrajectoryRepresentation") {
            Ok(anchor) => HeadPortion::Assumed(vec![anchor.at(0.0), anchor.at(first_md)]),
            Err(_) => {
                return Err(Error::unsupported(
                    "a frame's head portion: the trajectory has no `Geometry` and its datum cannot be read, so where MD 0 is is unknown",
                    at,
                ));
            }
        },
        Some(geometry) => {
            let line = crate::parametric::Line::single(geometry, source, at)?;
            let interval = crate::geometry::field_object(trajectory, MD_INTERVAL).unwrap_or(trajectory);
            let md_min = scalar_any(interval, MD_MIN)
                .filter(|md| md.is_finite())
                .ok_or_else(|| {
                    Error::malformed(
                        "WellboreTrajectoryRepresentation",
                        "a trajectory line with no finite `MdMin` (or `StartMd`): whether it reaches MD 0 is unknown",
                    )
                })?;
            if md_min <= 0.0 {
                // The line reaches MD 0: followed, not guessed at, and the
                // datum is not even read.
                HeadPortion::Described(samples(&line, 0.0, first_md))
            } else {
                let datum = datum_in_line_frame(trajectory, geometry, source).map_err(|why| {
                    Error::unsupported(
                        "a frame's head portion from a datum that cannot be expressed in its line's frame",
                        format!("{at}, MdMin {md_min}: {why}"),
                    )
                })?;
                // Where the line starts - or, when the first node is above
                // that, the first node itself: `frame` places it by extending
                // the first interval, so the portion ends on that point.
                let start = md_min.min(first_md);
                let mut points = vec![datum];
                if start < first_md {
                    points.extend(samples(&line, start, first_md));
                } else {
                    points.push(line.at(first_md));
                }
                HeadPortion::Assumed(points)
            }
        }
    };
    Ok(Some(portion))
}

/// MD 0 - the trajectory's datum - in the frame its line is written in, as
/// stored: the frame `crs::of` lends the trajectory's frames, so the one
/// `place_points` a caller runs on a frame's nodes places this point with
/// them. `Err` is the reason, in English, when the datum cannot be expressed
/// in that frame.
fn datum_in_line_frame(
    trajectory: &dyn Reflect,
    geometry: &dyn Reflect,
    source: &dyn ValueSource,
) -> std::result::Result<Point, String> {
    let at = "WellboreTrajectoryRepresentation";
    let Some(line_uri) = reference_of(geometry, "LocalCrs") else {
        return Err("the line names no `LocalCrs`".to_string());
    };
    let interval = crate::geometry::field_object(trajectory, MD_INTERVAL).unwrap_or(trajectory);
    let Some(datum_uri) = reference_any(interval, MD_DATUM) else {
        return Err("the trajectory names no datum".to_string());
    };
    let Some(datum) = source.object_at(&datum_uri) else {
        return Err(format!("the datum {datum_uri} is not in the package"));
    };
    let datum = datum.as_reflect();
    let type_key = datum.type_info().type_key;
    if !DATUM_TYPES.contains(&type_key) {
        return Err(format!("`{type_key}` is not a measured-depth datum"));
    }
    let raw = Anchor::coordinates(datum, at).map_err(|e| e.to_string())?;
    let raw = [raw.0, raw.1, raw.2];

    // The same object: the datum is written in the line's own frame, no
    // calculation. The usual case (one `LocalDepth3dCrs`, or one compound, for
    // the whole well); it asks for no EPSG code and no unit.
    let datum_frame_uri = datum_frame(datum);
    if let Some(uri) = &datum_frame_uri
        && same_uri(uri, &line_uri)
    {
        return Ok(raw);
    }

    // Otherwise both are brought to the projected system, then the datum to
    // the line's frame.
    let Some(line_object) = source.object_at(&line_uri) else {
        return Err(format!("the line's CRS {line_uri} is not in the package"));
    };
    let line_crs =
        energyml_domain::crs::read_with(line_object.as_reflect(), source).map_err(|e| e.to_string())?;
    if line_crs.is_time() {
        return Err("the line's CRS counts time, and a measured depth is a length".to_string());
    }

    // The datum placed (`placed`: projected x and y, elevation up) with the
    // system and units it was written in.
    let placed: Point;
    let datum_projected: CrsKind;
    let datum_vertical: CrsKind;
    let datum_horizontal_uom: Option<String>;
    let datum_vertical_uom: Option<String>;
    let check_horizontal: bool;
    match &datum_frame_uri {
        Some(uri) => {
            let datum_crs = match energyml_domain::crs::compound_of_datum(datum, source)
                .map_err(|e| e.to_string())?
            {
                Some(compound) => compound,
                None => {
                    let Some(object) = source.object_at(uri) else {
                        return Err(format!("the datum's CRS {uri} is not in the package"));
                    };
                    energyml_domain::crs::read_with(object.as_reflect(), source).map_err(|e| e.to_string())?
                }
            };
            if datum_crs.is_time() {
                return Err("the datum's CRS counts time, and a measured depth is a length".to_string());
            }
            placed = datum_crs.to_projected(raw);
            datum_projected = datum_crs.projected.clone();
            datum_vertical = datum_crs.vertical.clone();
            datum_horizontal_uom = datum_crs.horizontal_uom.clone();
            datum_vertical_uom = datum_crs.vertical_uom.clone();
            check_horizontal = true;
        }
        None => {
            // A `ReferencePointInACrs`: two absolute halves.
            let sense = vertical_sense(datum, source, at).map_err(|e| e.to_string())?;
            placed = match sense {
                VerticalSense::Up => raw,
                VerticalSense::Down => [raw[0], raw[1], -raw[2]],
            };
            let horizontal = crate::geometry::field_object(datum, "HorizontalCoordinates")
                .and_then(|h| reference_of(h, "Crs"))
                .and_then(|uri| source.object_at(&uri));
            datum_projected = match horizontal {
                Some(object) => {
                    energyml_domain::crs::read_with(object.as_reflect(), source)
                        .map_err(|e| e.to_string())?
                        .projected
                }
                None => CrsKind::Absent,
            };
            let vertical_object = reference_of(datum, "VerticalCrs").and_then(|uri| source.object_at(&uri));
            match vertical_object {
                Some(object) => {
                    datum_vertical_uom = energyml_core::measure::unit_of(object.as_reflect());
                    datum_vertical = energyml_domain::crs::read_with(object.as_reflect(), source)
                        .map_err(|e| e.to_string())?
                        .vertical;
                }
                None => {
                    datum_vertical_uom = None;
                    datum_vertical = CrsKind::Absent;
                }
            }
            datum_horizontal_uom = None;
            check_horizontal = false;
        }
    }

    // The same system: only what is provably one passes.
    if !same_system(&datum_projected, &line_crs.projected) {
        return Err(format!(
            "the datum sits in projected system {} and the line in {}",
            system_word(&datum_projected),
            system_word(&line_crs.projected)
        ));
    }
    if !same_system(&datum_vertical, &line_crs.vertical) {
        return Err(format!(
            "the datum's height is counted in vertical system {} and the line's in {}",
            system_word(&datum_vertical),
            system_word(&line_crs.vertical)
        ));
    }

    // Horizontal units: no conversion (`to_projected` assumes one unit); two
    // stated units that differ contradict that assumption, and choosing one
    // would be guessing.
    if check_horizontal
        && let (Some(d), Some(l)) = (&datum_horizontal_uom, &line_crs.horizontal_uom)
        && d != l
    {
        return Err(format!(
            "the datum's CRS gives its horizontal unit as `{d}` and the line's as `{l}`, and a placement takes both in the projected system's unit"
        ));
    }

    // Vertical units: the elevation goes from the datum's unit to the line's.
    let mut placed = placed;
    match (&datum_vertical_uom, &line_crs.vertical_uom) {
        (Some(a), Some(b)) if a != b => {
            let metres = energyml_domain::units::metres(placed[2], a).map_err(|e| e.to_string())?;
            let per_unit = energyml_domain::units::metres(1.0, b).map_err(|e| e.to_string())?;
            placed[2] = metres / per_unit;
        }
        (Some(a), None) => {
            return Err(format!(
                "the datum's vertical unit is `{a}` and the line's is not stated"
            ));
        }
        (None, Some(b)) => {
            return Err(format!(
                "the datum's vertical unit is not stated and the line's is `{b}`"
            ));
        }
        _ => {}
    }

    Ok(line_crs.from_projected(placed))
}

/// The uri of the one CRS object a datum's coordinates are written in:
/// `LocalCrs` beside `Location` (a RESQML 2.0.1 `MdDatum`), `Location.LocalCrs`
/// (the 2.2 drops' `MdDatum`), `Crs`
/// (`ReferencePointInALocalEngineeringCompoundCrs`). `None` for a
/// `ReferencePointInACrs`, whose two halves are absolute.
fn datum_frame(datum: &dyn Reflect) -> Option<String> {
    reference_of(datum, "LocalCrs")
        .or_else(|| {
            crate::geometry::field_object(datum, "Location")
                .and_then(|location| reference_of(location, "LocalCrs"))
        })
        .or_else(|| reference_of(datum, "Crs"))
}

/// Whether two systems are provably one: the same EPSG code, the same WKT
/// text, the same authority string. A GML definition, an unknown system and
/// no system prove nothing.
fn same_system(a: &CrsKind, b: &CrsKind) -> bool {
    match (a, b) {
        (CrsKind::Epsg(x), CrsKind::Epsg(y)) => x == y,
        (CrsKind::Wkt(x), CrsKind::Wkt(y)) => x == y,
        (CrsKind::LocalAuthority(x), CrsKind::LocalAuthority(y)) => x == y,
        _ => false,
    }
}

/// A system named in a sentence: `EPSG:2154`, `a WKT definition`,
/// `a GML definition`, `the authority <name>`, `an unknown system`, `no system`.
fn system_word(kind: &CrsKind) -> String {
    match kind {
        CrsKind::Epsg(code) => format!("EPSG:{code}"),
        CrsKind::Wkt(_) => "a WKT definition".to_string(),
        CrsKind::Gml => "a GML definition".to_string(),
        CrsKind::LocalAuthority(name) => format!("the authority {name}"),
        CrsKind::Unknown => "an unknown system".to_string(),
        CrsKind::Absent => "no system".to_string(),
    }
}

/// The measured-depth bounds, in the two shapes they have worn.
///
/// The released RESQML 2.2 groups them into an `MdInterval`; **RESQML 2.0.1 and
/// the 2.2 development drops put the same four values flat on the trajectory** -
/// `StartMd`, `FinishMd`, `MdUom`, `MdDatum`. One concept, two spellings,
/// handled the way this workspace handles every other respelling: a list of
/// names beside the code that reads them, never a match on version.
const MD_INTERVAL: &str = "MdInterval";
const MD_MIN: &[&str] = &["MdMin", "StartMd"];
const MD_MAX: &[&str] = &["MdMax", "FinishMd"];
const MD_UOM: &[&str] = &["Uom", "MdUom"];
const MD_DATUM: &[&str] = &["Datum", "MdDatum"];

/// The datum object, under its three names - `energyml_domain::crs`'s list,
/// never a copy of it, so that the CRS a well is given and the point it is
/// drawn from cannot disagree on what a datum is.
///
/// `MdDatum` in 2.0.1 and the drops; `ReferencePointInACrs` and
/// `ReferencePointInALocalEngineeringCompoundCrs` in 2.2. The same thing - a
/// georeferenced point a measured depth is counted from - and they hold their
/// coordinates differently, which [`Anchor::coordinates`] reconciles and
/// [`Anchor::of`] places.
const DATUM_TYPES: &[&str] = energyml_domain::crs::DATUM_TYPES;

/// Where a well is, and how far down it runs.
struct Anchor {
    x: f64,
    y: f64,
    /// The wellhead, as an elevation - positive **up**, whatever the file's
    /// vertical CRS says, so every point this module emits is z-up.
    elevation: f64,
    /// Metres per unit of measured depth, from the interval's `Uom`.
    md_to_metres: f64,
    md_min: f64,
    md_max: f64,
}

impl Anchor {
    /// The datum's position, in the two shapes the releases give it.
    ///
    /// 2.2 splits it - `HorizontalCoordinates{Coordinate1,2}` beside a
    /// `VerticalCoordinate` - while RESQML 2.0.1 keeps a `Location` that is the
    /// `Point3d`, beside the datum's `LocalCrs`, and the 2.2 drops wrap it in
    /// `Location.Point3d`. Same point, and the vertical one is
    /// returned in the file's own sense; [`vertical_sense`] says which way that
    /// is.
    fn coordinates(datum: &dyn Reflect, at: &str) -> Result<(f64, f64, f64)> {
        if let Some(horizontal) = crate::geometry::field_object(datum, "HorizontalCoordinates") {
            let x = scalar_f64(horizontal, "Coordinate1")
                .ok_or_else(|| Error::malformed(at, "a reference point with no `Coordinate1`"))?;
            let y = scalar_f64(horizontal, "Coordinate2")
                .ok_or_else(|| Error::malformed(at, "a reference point with no `Coordinate2`"))?;
            let vertical = scalar_f64(datum, "VerticalCoordinate")
                .ok_or_else(|| Error::malformed(at, "a reference point with no `VerticalCoordinate`"))?;
            return Ok((x, y, vertical));
        }
        // RESQML 2.0.1: `Location` is itself the `Point3d`, beside the datum's
        // `LocalCrs`. The 2.2 drops wrap it in `Location.Point3d`.
        let location = crate::geometry::field_object(datum, "Location");
        let point = location
            .filter(|location| location.field("Coordinate1").is_some())
            .or_else(|| location.and_then(|location| crate::geometry::field_object(location, "Point3d")))
            .ok_or_else(|| {
                Error::malformed(at, "a datum with neither horizontal coordinates nor a `Location`")
            })?;
        let x = scalar_f64(point, "Coordinate1")
            .ok_or_else(|| Error::malformed(at, "a datum location with no `Coordinate1`"))?;
        let y = scalar_f64(point, "Coordinate2")
            .ok_or_else(|| Error::malformed(at, "a datum location with no `Coordinate2`"))?;
        let z = scalar_f64(point, "Coordinate3")
            .ok_or_else(|| Error::malformed(at, "a datum location with no `Coordinate3`"))?;
        Ok((x, y, z))
    }

    /// The point at a measured depth, z-up.
    fn at(&self, md: f64) -> Point {
        [self.x, self.y, self.elevation - md * self.md_to_metres]
    }

    /// Read the anchor of a trajectory: its `MdInterval` and the reference
    /// point the interval's `Datum` names - one of [`DATUM_TYPES`].
    fn of(trajectory: &dyn Reflect, source: &dyn ValueSource, at: &str) -> Result<Self> {
        // Grouped in an `MdInterval` (2.2) or flat on the trajectory itself
        // (2.0.1 and the drops). Falling back to the trajectory rather than
        // refusing is what makes one reader serve both.
        let interval = crate::geometry::field_object(trajectory, MD_INTERVAL).unwrap_or(trajectory);
        let (Some(md_min), Some(md_max)) = (scalar_any(interval, MD_MIN), scalar_any(interval, MD_MAX))
        else {
            // The spec's own business rule makes the datum mandatory, so a
            // trajectory with neither geometry nor depth bounds has no spatial
            // content at all.
            return Err(Error::unsupported(
                "a wellbore trajectory with neither `Geometry` nor measured-depth bounds (`MdInterval`, or `StartMd`/`FinishMd`)",
                at,
            ));
        };
        if md_min > md_max {
            // "The MdMin must be less than the MdMax within the MdInterval" -
            // the spec's rule, and a violated one means the interval cannot be
            // trusted to point down.
            return Err(Error::malformed(
                at,
                format!("MdMin {md_min} is greater than MdMax {md_max}"),
            ));
        }
        let uom = md_unit(trajectory);
        let md_to_metres = units::metres(1.0, &uom).map_err(Error::Domain)?;

        let Some(datum_uri) = reference_any(interval, MD_DATUM) else {
            return Err(Error::malformed(at, "depth bounds with no datum"));
        };
        let placed = place_datum(&datum_uri, source, at)?;
        Ok(Self {
            x: placed.x,
            y: placed.y,
            elevation: placed.elevation,
            md_to_metres,
            md_min,
            md_max,
        })
    }
}

/// A measured-depth datum, placed: projected `x` and `y`, and an elevation in
/// the convention of this module - see [`Anchor`].
pub(crate) struct PlacedDatum {
    pub(crate) x: f64,
    pub(crate) y: f64,
    /// Positive **up**.
    pub(crate) elevation: f64,
    /// The compound the datum is written in, when it is a
    /// `ReferencePointInALocalEngineeringCompoundCrs` - what placed it.
    pub(crate) compound: Option<energyml_domain::Crs>,
}

/// Where a datum is, read once for every well that counts its depths from one:
/// a RESQML trajectory's `MdInterval.Datum` ([`Anchor`]) and a WITSML
/// trajectory's `DefaultMdDatum` (`crate::stations`). Two readings of *where is
/// this datum* would be two answers.
pub(crate) fn place_datum(datum_uri: &str, source: &dyn ValueSource, at: &str) -> Result<PlacedDatum> {
    let Some(datum) = source.object_at(datum_uri) else {
        return Err(Error::Domain(energyml_domain::Error::Missing(format!(
            "the datum {datum_uri}"
        ))));
    };
    let datum = datum.as_reflect();
    let type_key = datum.type_info().type_key;
    if !DATUM_TYPES.contains(&type_key) {
        return Err(Error::unsupported(
            crate::geometry::leak(type_key),
            format!("{at}.Datum"),
        ));
    }

    let (x, y, vertical) = Anchor::coordinates(datum, at)?;

    // **A point in a compound is a local point, and is placed.** Its
    // coordinates are counted along the axes of the compound its `Crs`
    // names (common TRG 2.3 §13.25 p. 371), so where the well stands is the
    // compound's placement of them: origin, azimuth, and a vertical axis
    // that may count down from a datum that counts up. The result is what
    // every point of this module is - projected x and y, an elevation - and
    // the elevation goes to metres to meet the measured depths.
    // `energyml_domain::crs::of` gives such a well the compound's identity
    // and none of its offsets, so nothing is placed twice.
    if let Some(compound) = energyml_domain::crs::compound_of_datum(datum, source).map_err(Error::Domain)? {
        if compound.is_time() {
            // A measured depth is a length; a datum in a time system has
            // no depth to count it from without a velocity model.
            return Err(Error::unsupported(
                "a measured-depth datum placed in a time CRS",
                format!("{at}.Datum"),
            ));
        }
        let [x, y, up] = compound.to_projected([x, y, vertical]);
        let elevation = match compound.vertical_uom.as_deref() {
            Some(unit) => units::metres(up, unit).map_err(Error::Domain)?,
            None => up,
        };
        return Ok(PlacedDatum {
            x,
            y,
            elevation,
            compound: Some(compound),
        });
    }

    let elevation = match vertical_sense(datum, source, at)? {
        VerticalSense::Up => vertical,
        VerticalSense::Down => -vertical,
    };
    Ok(PlacedDatum {
        x,
        y,
        elevation,
        compound: None,
    })
}

enum VerticalSense {
    Up,
    Down,
}

/// Which way the datum's `VerticalCoordinate` points.
///
/// Two sources can answer, and the corpus proves they can disagree: the BRGM
/// export names **EPSG:5714** - "MSL height", positive up, and its wellheads
/// sit at +30 to +40 m, which is right for the Paris region - while writing
/// `Direction: down` on the same object, which would put every wellhead under
/// the sea. The EPSG code *is* the convention (5714 = height, 5715 = depth),
/// so when one of those two codes is named it wins; the `Direction` element is
/// only believed when no code says otherwise.
fn vertical_sense(datum: &dyn Reflect, source: &dyn ValueSource, at: &str) -> Result<VerticalSense> {
    // 2.2 names a `VerticalCrs` on the datum itself. 2.0.1 and the drops give
    // the datum a `Location` whose `LocalCrs` carries the sense, as a
    // `ZIncreasingDownward` flag - so that is the second place to look, and
    // looking there is what lets a drop's wells stand the right way up.
    let vertical_crs = reference_of(datum, "VerticalCrs")
        .or_else(|| {
            crate::geometry::field_object(datum, "Location")
                .and_then(|location| reference_of(location, "LocalCrs"))
        })
        .and_then(|uri| source.object_at(&uri));
    let Some(vertical_crs) = vertical_crs else {
        // No CRS to consult: a bare vertical coordinate has no stated sense,
        // and up-vs-down is the whole well.
        return Err(Error::unsupported(
            "a reference point whose vertical CRS is not in the package",
            at,
        ));
    };
    let crs = vertical_crs.as_reflect();

    if let Some(code) = epsg_of(crs) {
        match code {
            5714 => return Ok(VerticalSense::Up),
            5715 => return Ok(VerticalSense::Down),
            _ => {}
        }
    }
    // A local CRS states it as a boolean rather than a word.
    if let Some(energyml_core::value::FieldValue::Scalar(energyml_core::value::Scalar::Bool(down))) =
        crs.field("z_increasing_downward")
    {
        return Ok(if down {
            VerticalSense::Down
        } else {
            VerticalSense::Up
        });
    }
    match string_of(crs, "Direction").as_deref() {
        Some("up") => Ok(VerticalSense::Up),
        Some("down") => Ok(VerticalSense::Down),
        _ => Err(Error::unsupported("a vertical CRS that states no direction", at)),
    }
}

/// The EPSG code a CRS object carries, wherever its version put it.
fn epsg_of(crs: &dyn Reflect) -> Option<i64> {
    fn walk(object: &dyn Reflect, depth: u8) -> Option<i64> {
        if depth > 3 {
            return None;
        }
        for candidate in ["EpsgCode", "Epsg", "Code"] {
            if let Some(code) = values::scalar_i64(object, candidate)
                && code > 0
            {
                return Some(code);
            }
        }
        for field in object.type_info().fields {
            if let Some(energyml_core::value::FieldValue::Object(child)) = object.field(field.name)
                && let Some(code) = walk(child, depth + 1)
            {
                return Some(code);
            }
        }
        None
    }
    walk(crs, 0)
}

/// The uri a DOR-valued field points at - the full identity, version included
/// when the reference declares one - so the object it names is the object read
/// (`ObjectSource::object_at`), never another version sharing its uuid.
/// The first of these fields that holds a number.
fn scalar_any(object: &dyn Reflect, names: &[&str]) -> Option<f64> {
    names.iter().find_map(|name| scalar_f64(object, name))
}

/// The first of these fields that holds a string.
fn string_any(object: &dyn Reflect, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| string_of(object, name))
}

/// The first of these fields that holds a reference.
fn reference_any(object: &dyn Reflect, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| reference_of(object, name))
}

pub(crate) fn reference_of(object: &dyn Reflect, field: &str) -> Option<String> {
    let energyml_core::value::FieldValue::Object(reference) = object.field(field)? else {
        return None;
    };
    crate::follow::address_of(reference)
}

/// Whether two uris name the same object: the same uuid, and object versions
/// that do not contradict each other - an absent version means "the latest",
/// so it matches any (`ObjectKey::same_object`). Text that is no uri is
/// compared as it stands.
pub(crate) fn same_uri(a: &str, b: &str) -> bool {
    use energyml_core::identifiers::EtpUri;
    match (EtpUri::parse(a), EtpUri::parse(b)) {
        (Ok(a), Ok(b)) => {
            a.uuid == b.uuid
                && match (&a.object_version, &b.object_version) {
                    (Some(x), Some(y)) => x == y,
                    _ => true,
                }
        }
        _ => a.eq_ignore_ascii_case(b),
    }
}

/// A scalar string field.
fn string_of(object: &dyn Reflect, field: &str) -> Option<String> {
    match object.field(field)? {
        energyml_core::value::FieldValue::Scalar(scalar) => scalar.as_str().map(|s| s.to_string()),
        _ => None,
    }
}
