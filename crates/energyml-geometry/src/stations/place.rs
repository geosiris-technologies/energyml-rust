//! A survey placed: its datum in the world, its stations in the frame and the units of the CRS its
//! datum gives it, and the path between its stations.
//!
//! The units are the CRS's (**one rule**): the points a survey gives are in the frame and the units of
//! the CRS `energyml_domain::crs::of` gives the same object - the CRS the scene applies to them - so
//! `crs.to_metres(points[k])` is the station placed, in metres; with no CRS, metres already. The
//! conversions go through `units::metres`, `Crs::to_metres` and `Crs::from_projected`, never a table
//! of this module.

use energyml_core::value::Reflect;
use energyml_domain::Crs;
use energyml_domain::units;

use super::checks::{CheckKind, Fault, Finding, Mode, Placed, Scope, SurveyRefusal, renumbered, run};
use super::north::{Horizontal, NorthSource};
use super::{SurveyDeclarations, SurveyKind};
use crate::error::Error;
use crate::min_curvature::{self, Measured, StationsError};
use crate::parametric::{Path, Vertical};
use crate::points::Point;
use crate::source::ValueSource;
use crate::well::{PathRefusal, PlacedDatum, PlacedPath, SurveyPlacement, place_datum};

/// How a survey runs between two stations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BetweenStations {
    /// The arc of the minimum curvature method (`Path` kind 5): the method itself.
    #[default]
    Arc,
    /// The straight segment (`Path` kind 1): what a polyline through the stations draws.
    Straight,
}

/// The survey as a line evaluable at any MD, in the datum frame (metres, x east, y north of the
/// survey's north, z up, origin at the first station): the stations' positions as control points,
/// their directions as tangents, the MD as parameter. **The one constructor of a survey's arc.**
pub fn survey_path(stations: &[Measured], between: BetweenStations) -> Result<Path, StationsError> {
    let (located, directions) = min_curvature::positions_with_directions(stations)?;
    let mut points = Vec::with_capacity(stations.len());
    let mut tangents = Vec::with_capacity(stations.len());
    let mut mds = Vec::with_capacity(stations.len());
    for ((station, at), t) in stations.iter().zip(&located).zip(&directions) {
        points.push([at.east, at.north, -at.tvd]);
        // North, east, down -> east, north, up.
        tangents.push([t[1], t[0], -t[2]]);
        mds.push(station.md);
    }
    let path = match between {
        BetweenStations::Arc => Path::new(5, points, mds, Some(tangents), Vertical::Up),
        BetweenStations::Straight => Path::new(1, points, mds, None, Vertical::Up),
    };
    Ok(path.expect("kinds 5 and 1 are known and every station has a tangent"))
}

/// A survey placed in the world.
#[derive(Debug, Clone)]
pub struct PlacedSurvey {
    /// One per kept station, in the frame and units of the CRS `energyml_domain::crs::of` gives the
    /// survey - what `read` draws and the scene places: `crs.to_metres(points[k])` is the station placed,
    /// in metres; with no CRS, metres already.
    pub points: Vec<[f64; 3]>,
    /// The stations placed (from 1, file order).
    pub kept: Vec<usize>,
    /// Where its north comes from.
    pub north: NorthSource,
    /// The placement used, in metres.
    pub placement: SurveyPlacement,
    /// The survey's path in the datum frame, in its placed north (corrected azimuths when `Corrected`).
    pub local: Path,
}

impl PlacedSurvey {
    /// The placed path, evaluable at any MD by the arc.
    pub fn placed_path(&self) -> Result<PlacedPath, PathRefusal> {
        PlacedPath::from_local(self.local.clone(), &self.placement)
    }
}

/// The datum placed, and the units of the CRS its depths are counted in.
pub(crate) struct World {
    pub(crate) placed: PlacedDatum,
    /// The CRS `energyml_domain::crs::of` gives the survey, when it gives one.
    pub(crate) crs: Option<Crs>,
    /// Metres per unit of the CRS's horizontal axes (1 when it states none, or there is no CRS).
    pub(crate) h: f64,
    /// Metres per unit of the CRS's vertical axis.
    pub(crate) v: f64,
    /// The datum, in metres.
    pub(crate) datum: Point,
}

/// The check `Placement`: the datum placed by `place_datum`, then the CRS of the survey and its units.
pub(super) fn place_stage(
    survey: &dyn Reflect,
    source: &dyn ValueSource,
    datum_uri: &str,
    kind: SurveyKind,
) -> Result<World, Fault> {
    let placed = place_datum(datum_uri, source, kind.at()).map_err(|error| {
        Fault::with_error(
            CheckKind::Placement,
            Finding::DatumNotPlaced {
                datum: datum_uri.to_string(),
                reason: error.to_string(),
            },
            error,
        )
    })?;
    let crs = energyml_domain::crs::of(survey, source).map_err(|why| {
        Fault::with_error(
            CheckKind::Placement,
            Finding::CrsUnreadable {
                reason: why.to_string(),
            },
            Error::Domain(why),
        )
    })?;
    let h = match crs.as_ref().and_then(|crs| crs.horizontal_uom.as_deref()) {
        Some(unit) => units::metres(1.0, unit).map_err(|why| {
            Fault::with_error(
                CheckKind::Placement,
                Finding::HorizontalUnitRefused {
                    unit: unit.to_string(),
                    reason: why.to_string(),
                },
                Error::Domain(why),
            )
        })?,
        None => 1.0,
    };
    let v = match crs.as_ref().and_then(|crs| crs.vertical_axis_uom()) {
        Some(unit) => units::metres(1.0, unit).map_err(|why| {
            Fault::with_error(
                CheckKind::Placement,
                Finding::VerticalUnitRefused {
                    unit: unit.to_string(),
                    reason: why.to_string(),
                },
                Error::Domain(why),
            )
        })?,
        None => 1.0,
    };
    let datum = match &crs {
        Some(crs) => crs.to_metres(placed.written).map_err(|why| {
            Fault::with_error(
                CheckKind::Placement,
                Finding::CrsUnreadable {
                    reason: why.to_string(),
                },
                Error::Domain(why),
            )
        })?,
        None => placed.written,
    };
    Ok(World {
        placed,
        crs,
        h,
        v,
        datum,
    })
}

/// A point of the datum frame placed in metres, in the frame and units of the CRS.
fn to_crs_frame(crs: Option<&Crs>, metres: Point, h: f64, v: f64) -> Point {
    match crs {
        Some(crs) => crs.from_projected([metres[0] / h, metres[1] / h, metres[2] / v]),
        None => metres,
    }
}

/// The placement in metres: the datum, and where azimuth 90 and azimuth 0 point in the placed plane.
fn placement_of(world: &World, horizontal: Option<&Horizontal>) -> SurveyPlacement {
    let mut datum = world.datum;
    let axes = match horizontal {
        Some(horizontal) => {
            let f = horizontal.metres_per_unit();
            // With no CRS the datum is as written, in the unit of its system: metres, like the rest.
            if world.crs.is_none() {
                datum[0] *= f;
                datum[1] *= f;
            }
            let east = horizontal.offset(1.0, 0.0);
            let north = horizontal.offset(0.0, 1.0);
            energyml_domain::crs::CompassAxes {
                east: [east.0 * f, east.1 * f],
                north: [north.0 * f, north.1 * f],
            }
        }
        // No azimuth moves anything: the choice is without effect.
        None => energyml_domain::crs::CompassAxes {
            east: [1.0, 0.0],
            north: [0.0, 1.0],
        },
    };
    SurveyPlacement { datum, axes }
}

/// The survey placed with what the caller declares - `SurveyDeclarations::default()` is `read`.
pub fn read_declaring(
    survey: &dyn Reflect,
    source: &dyn ValueSource,
    declared: &SurveyDeclarations,
) -> Result<PlacedSurvey, SurveyRefusal> {
    let placed = run(survey, source, declared, Mode::StopAtFirst, Scope::Full)
        .into_result()
        .map_err(SurveyRefusal::from)?;
    let Placed {
        facts,
        kept,
        north,
        corrected,
        world,
        horizontal,
    } = placed;
    let placement = placement_of(&world, horizontal.as_ref());
    let path_fault = |error: StationsError| {
        SurveyRefusal::from(Fault::new(
            CheckKind::Path,
            Finding::Stations(renumbered(error, &kept.stations)),
            facts.kind,
        ))
    };
    let located = min_curvature::positions(&corrected).map_err(path_fault)?;
    let points = located
        .iter()
        .map(|one| {
            to_crs_frame(
                world.crs.as_ref(),
                placement.place([one.east, one.north, -one.tvd]),
                world.h,
                world.v,
            )
        })
        .collect();
    let local = survey_path(&corrected, BetweenStations::Arc).map_err(path_fault)?;
    Ok(PlacedSurvey {
        points,
        kept: kept.stations.clone(),
        north,
        placement,
        local,
    })
}

/// The datum placed and the grid axes of its system, with no north asked: the frame a survey whose
/// north is unknown is compared in (`energyml_analysis::survey_qc::compare`, `Frames::TurnedAboutVertical`).
pub fn placement_without_north(
    survey: &dyn Reflect,
    source: &dyn ValueSource,
) -> Result<SurveyPlacement, SurveyRefusal> {
    let placed = run(
        survey,
        source,
        &SurveyDeclarations::default(),
        Mode::StopAtFirst,
        Scope::FrameOnly,
    )
    .into_result()
    .map_err(SurveyRefusal::from)?;
    Ok(placement_of(&placed.world, placed.horizontal.as_ref()))
}
