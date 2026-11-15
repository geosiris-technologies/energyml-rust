//! Markers on the hole, and the dip they show in the plane of the section.
//!
//! No marker reader exists in the workspace (`well_strata` refuses a
//! `WellboreMarkerSet` by name; neither the RESQML marker frame nor the
//! WITSML 2.2 interval set has one): the pure function is written and tested
//! for a caller that holds markers, and the model refuses every marker uri by
//! name ([`MarkerRefusal::NoReader`]) without failing the section.

use energyml_geometry::around_well::projection::{WellAxis, WellProjection};
use energyml_geometry::well::PlacedPath;

/// A marker on the hole.
#[derive(Clone, Debug, PartialEq)]
pub struct WellMarker {
    /// The uri of the marker object.
    pub uri: String,
    /// Its measured depth.
    pub md: f64,
    /// Its dip, when it states one.
    pub dip: Option<Dip>,
}

/// A dip as a WITSML 2.1 `WellboreMarker` states it (`DipAngle`, `DipDirection`,
/// `DipDirectionRef`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dip {
    /// Radians from horizontal.
    pub angle: f64,
    /// Radians, clockwise from the reference north.
    pub direction: f64,
    /// The north the direction is counted from.
    pub reference: DipReference,
}

/// The north a dip direction is counted from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DipReference {
    /// The north of the grid.
    GridNorth,
    /// The true north.
    TrueNorth,
    /// The magnetic north.
    MagneticNorth,
}

/// A marker, placed on the section.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkerTick {
    /// The uri of the marker object.
    pub uri: String,
    /// Its measured depth.
    pub md: f64,
    /// (md, dn = 0) placed; not a number when the projection refused it.
    pub at: [f64; 2],
    /// Apparent dip in the plane of the section, radians; `None`: drawn level, `note` says why.
    pub apparent_dip: Option<f64>,
    /// Why the tick is drawn level, or how its slope was bent.
    pub note: Option<MarkerNote>,
}

/// What a marker tick says beside its slope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerNote {
    /// The marker states no dip.
    NoDip,
    /// A dip direction against a north this section cannot turn to grid north without a declared angle.
    NorthNotDeclared,
    /// Against MD: the slope is multiplied by sin(I).
    SlopeAgainstMd,
    /// The projection refused this MD (outside the axis, no high side).
    NotPlaced,
}

/// A marker the model could not read.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum MarkerRefusal {
    /// There is no reader for this type of marker.
    #[error("`{uri}` is a {qualified_type}, and this build has no reader of markers")]
    NoReader {
        /// The uri asked.
        uri: String,
        /// Its qualified type, as the package holds it.
        qualified_type: String,
    },
}

/// The ticks of the markers on the section.
///
/// The apparent dip is `tan(apparent) = tan(dip) cos(direction - azimuth)`,
/// the azimuth being that of the hole at the marker's depth, in grid north
/// (`PlacedPath::compass`). `to_grid` is the angle (radians, clockwise) the
/// caller declares between the marker's north and grid north, for
/// `TrueNorth` and `MagneticNorth` (S2): without it such a dip is drawn level
/// and said, never guessed. Against the MD (`abscissa_is_length` false) the
/// slope is multiplied by `sin(I)`.
pub fn marker_ticks(
    path: &PlacedPath,
    axis: &WellAxis,
    markers: &[WellMarker],
    projection: &dyn WellProjection,
    to_grid: Option<f64>,
) -> Vec<MarkerTick> {
    markers
        .iter()
        .map(|marker| tick(path, axis, marker, projection, to_grid))
        .collect()
}

fn tick(
    path: &PlacedPath,
    axis: &WellAxis,
    marker: &WellMarker,
    projection: &dyn WellProjection,
    to_grid: Option<f64>,
) -> MarkerTick {
    let level = |at: [f64; 2], note: MarkerNote| MarkerTick {
        uri: marker.uri.clone(),
        md: marker.md,
        at,
        apparent_dip: None,
        note: Some(note),
    };
    let at = match projection.place(axis, marker.md, 0.0) {
        Ok(at) => at,
        Err(_) => return level([f64::NAN; 2], MarkerNote::NotPlaced),
    };
    let Some(dip) = marker.dip else {
        return level(at, MarkerNote::NoDip);
    };
    let direction = match (dip.reference, to_grid) {
        (DipReference::GridNorth, _) => dip.direction,
        (_, Some(angle)) => dip.direction + angle,
        (_, None) => return level(at, MarkerNote::NorthNotDeclared),
    };
    let (Some(compass), Some(tangent)) = (path.compass(), path.direction_at(marker.md)) else {
        return level(at, MarkerNote::NorthNotDeclared);
    };
    let east = tangent[0] * compass.east[0] + tangent[1] * compass.east[1];
    let north = tangent[0] * compass.north[0] + tangent[1] * compass.north[1];
    let azimuth = east.atan2(north);
    let mut slope = dip.angle.tan() * (direction - azimuth).cos();
    let mut note = None;
    if !projection.abscissa_is_length() {
        slope *= tangent[0].hypot(tangent[1]);
        note = Some(MarkerNote::SlopeAgainstMd);
    }
    MarkerTick {
        uri: marker.uri.clone(),
        md: marker.md,
        at,
        apparent_dip: Some(slope.atan()),
        note,
    }
}
