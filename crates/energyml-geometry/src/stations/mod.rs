//! A survey, read apart from being placed, judged check by check, and drawn: a WITSML
//! `Trajectory` or a RESQML 2.0.1 `DeviationSurveyRepresentation`, its stations placed by the
//! minimum curvature method from the datum its measured depths are counted from.
//!
//! A RESQML trajectory states a **line** - control points, a parametric
//! spline - and [`crate::parametric`] evaluates it. A WITSML 2.1 trajectory
//! states **measurements**: at each `TrajectoryStation` a measured depth, an
//! inclination *« measured from vertical »* and an azimuth *« Corrected to
//! wells azimuth reference »* (WITSML TRG 2.1 §16.10 p. 356-358). Where the
//! hole is follows from them by a method, and the method is
//! [`crate::min_curvature`] - the minimum curvature method run forward
//! (WITSML/PRODML business views). This module is
//! what it takes to hand that function numbers it can trust, and to put its
//! answer in the world.
//!
//! # Read, judged, placed
//!
//! - [`stations_of`] **reads** the stations, in the file's units and the file's own north, and
//!   places nothing and judges nothing: it refuses only what makes the stations unreadable (no
//!   station, a depth or an angle with no number or no usable unit, an array that cannot be read).
//!   It also reads the values the file states at its stations (`Tvd`, `DispNs`, `Dls`, ...), the
//!   corrections it says it used and the vertical section it states, for a triangulation
//!   (`energyml_analysis::survey_qc`), never to place.
//! - [`diagnose`] **judges**: ten checks in a written order ([`CheckKind`]), each passed, failed with
//!   a typed cause ([`Finding`]) or not reached.
//! - [`read_declaring`] **places**: it runs the same ten checks and stops at the first failure,
//!   with the same sentences as ever, then places the stations. `read` (the registered reader) is
//!   `read_declaring` with nothing declared.
//!
//! The order of the checks is: the stations, the method, the status, the units, the datum, the
//! first station at the datum, the path, **the north** (before the datum is placed: it depends on the
//! stations alone), the placement of the datum, and the north in the frame.
//!
//! # Placed by the trajectory's datum, not the well's
//!
//! The WITSML 2.1 `Well` carries no datum. The trajectory names
//! the one its depths count from - `DefaultMdDatum`, an `AbstractReferencePoint`
//! (§16.6 p. 350) - and a station may name its own in `Md.Datum`. The two must
//! agree, and so must `MdInterval.Datum` when it is stated, because that is the
//! one `energyml_domain::crs::of` reads to give the trajectory its CRS: a
//! trajectory drawn from one datum and labelled with another is placed twice
//! or not at all. The datum is placed by `crate::well::place_datum`, the
//! reading a RESQML well's datum goes through - never a second one.
//!
//! **MD 0 is the datum**, by definition of a measured depth, so the first
//! station must be there. One that starts below it - a survey tied on to
//! another (`MdTieOn`, `ParentTrajectory`) - does not say where the hole runs
//! above it, and is refused rather than hung from the datum.
//!
//! # Which north
//!
//! *« Specifies the definition of north. While this is optional because of
//! legacy data, it is strongly recommended that this always be specified »*
//! (`AziRef`, §16.7 p. 352). Honoured, or refused:
//!
//! - **grid north** - the north of the projected system the datum is placed in:
//!   the displacement is added to the placed datum as it stands, east along the
//!   first projected axis and north along the second;
//! - **plant north** - *« a direction in a local engineering coordinate
//!   reference system »* (eml 2.3 `NorthReferenceKind`): the second axis of the
//!   compound the datum is written in, so the displacement is turned by that
//!   compound's azimuth - through `Crs::to_projected`, the placement every
//!   geometry of the compound goes through. A datum written in no compound
//!   names no plant, and is refused;
//! - **true north** and **magnetic north** - turned to grid north by the corrections **the file
//!   states** (`GridConUsed`, `MagDeclUsed`, or a station's own `StnGridConUsed`, `StnMagDeclUsed`):
//!   the convergence is subtracted, and from magnetic north the declination is added first. A
//!   correction the file does not state is refused by name, never taken from elsewhere;
//! - **astronomic, compass north**, or a value outside the list - no field corrects them: refused;
//! - **none** - refused, unless no station leaves the vertical below the
//!   datum, in which case no azimuth moves anything and none is asked; or unless
//!   the caller **declares** grid or plant north ([`SurveyDeclarations`]), which is carried in the
//!   result ([`NorthSource::Declared`]), never written into the package, and never replaces a north
//!   the file states.
//!
//! An axis order the file states other than east then north is refused where
//! it would turn the hole: the compound's `HorizontalAxes` for plant north,
//! the projected system's `AxisOrder` for grid north.
//!
//! # Decisions, stated
//!
//! **No algorithm stated is read as minimum curvature.** A station's
//! `CalcAlgorithm` and the trajectory's `NominalCalcAlgorithm` are optional,
//! and absent on most surveys; the angles are measurements and some method must
//! join them. Minimum curvature is the one API D20 and the industry settle on,
//! and the one the design names. A method the file **states** otherwise -
//! tangential, radius of curvature, … - is refused: drawing it by another would
//! move the hole away from where its producer computed it.
//!
//! **A rejected station** (`StatusTrajStation` = `rejected`) refuses the survey, unless the caller
//! declares that rejected stations are left out ([`RejectedStations::LeaveOut`]): the positions
//! then run over the other stations, and a refusal still names a station by its number in the file.
//!
//! **A RESQML 2.0.1 deviation survey** states its azimuths relative to the projected CRS north and
//! its inclinations from the vertical, and no method: grid north, minimum curvature. Its
//! `FirstStationLocation` is read and shown, never used to place: it is placed from its `MdDatum`,
//! and refused by name where that datum names no projected CRS to turn from.
//!
//! # What comes out
//!
//! One polyline, **one node per station** - the stations are what the object
//! states, and what a log indexed on the trajectory hangs on - **in the frame and the units of the
//! CRS** `energyml_domain::crs::of` gives the object: `crs.to_metres(points[k])` is the station
//! placed, in metres; with no CRS, metres already. The CRS carries the identity of the system and
//! none of its placement, so the two read together place the hole once.

// A failed check carries its typed cause, the library's sentence and where it happened: it is the rare
// path of every function here, and boxing it would only hide the shape.
#![allow(clippy::result_large_err)]

mod checks;
mod deviation;
mod north;
mod place;
mod witsml;

use energyml_core::value::Reflect;

use crate::error::Error;
use crate::geometry::{Patch, Topology, field_list};
use crate::min_curvature::Measured;
use crate::source::ValueSource;

pub use checks::{
    CheckKind, CheckOutcome, Finding, MissingCorrection, SurveyCheck, SurveyDiagnosis, SurveyRefusal,
    diagnose,
};
pub use north::NorthSource;
pub use place::{BetweenStations, PlacedSurvey, placement_without_north, read_declaring, survey_path};

pub(crate) use checks::Fault;

/// The one calculation method the reader applies, as WITSML spells it
/// (`TrajStnCalcAlgorithm`, WITSML TRG 2.1 §16.15).
pub(crate) const MINIMUM_CURVATURE: &str = "minimum curvature";

/// Which object the stations were read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurveyKind {
    /// A WITSML `Trajectory` (2.1; a 2.0 one is read, and refused by its datum).
    WitsmlTrajectory,
    /// A RESQML 2.0.1 `DeviationSurveyRepresentation`.
    DeviationSurvey,
}

impl SurveyKind {
    /// Where a refusal is, in words: the name of the concept.
    pub(crate) fn at(self) -> &'static str {
        match self {
            SurveyKind::WitsmlTrajectory => "Trajectory",
            SurveyKind::DeviationSurvey => "DeviationSurveyRepresentation",
        }
    }

    /// Where station `station` (from 1) is, in words.
    pub(crate) fn here(self, station: usize) -> String {
        match self {
            SurveyKind::WitsmlTrajectory => format!("Trajectory.TrajectoryStation[{station}]"),
            SurveyKind::DeviationSurvey => format!("DeviationSurveyRepresentation.station[{station}]"),
        }
    }

    /// One of two phrases, by kind.
    pub(crate) fn pick(self, witsml: &'static str, deviation: &'static str) -> &'static str {
        match self {
            SurveyKind::WitsmlTrajectory => witsml,
            SurveyKind::DeviationSurvey => deviation,
        }
    }
}

/// A north, as the file states it (`AziRef`, eml 2.3 `NorthReferenceKind`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NorthReference {
    /// `grid north`.
    Grid,
    /// `plant north`.
    Plant,
    /// `true north`.
    True,
    /// `magnetic north`.
    Magnetic,
    /// `astronomic north`.
    Astronomic,
    /// `compass north`.
    Compass,
    /// Anything else, kept as written.
    Other(String),
}

impl NorthReference {
    /// From the lexical value: "grid north", "plant north", "true north", "magnetic north",
    /// "astronomic north", "compass north"; anything else is `Other`, kept as written.
    pub fn from_lexical(value: &str) -> NorthReference {
        match value {
            "grid north" => NorthReference::Grid,
            "plant north" => NorthReference::Plant,
            "true north" => NorthReference::True,
            "magnetic north" => NorthReference::Magnetic,
            "astronomic north" => NorthReference::Astronomic,
            "compass north" => NorthReference::Compass,
            other => NorthReference::Other(other.to_string()),
        }
    }

    /// The lexical value, as the schema spells it (`Other` gives back its text).
    pub fn lexical(&self) -> &str {
        match self {
            NorthReference::Grid => "grid north",
            NorthReference::Plant => "plant north",
            NorthReference::True => "true north",
            NorthReference::Magnetic => "magnetic north",
            NorthReference::Astronomic => "astronomic north",
            NorthReference::Compass => "compass north",
            NorthReference::Other(text) => text,
        }
    }
}

/// A north the caller declares for a survey that states none. Never a default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclaredNorth {
    /// The north of the projected system the datum is placed in.
    Grid,
    /// The north of the local engineering compound the datum is written in.
    Plant,
}

/// What happens to a station whose `StatusTrajStation` is `rejected`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RejectedStations {
    /// The survey is refused (`Finding::RejectedStation`): the default.
    #[default]
    Refuse,
    /// Left out, because the caller declares so: the positions run over the other stations.
    LeaveOut,
}

/// Everything the caller declares, never written into the package by this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SurveyDeclarations {
    /// The north of a survey that states none.
    pub north: Option<DeclaredNorth>,
    /// What to do with a rejected station.
    pub rejected: RejectedStations,
}

/// A value the file states, whose unit could not be read: `unit` is `None` when it states none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitFault {
    /// The field, as the schema spells it.
    pub field: &'static str,
    /// The unit as written, when there is one.
    pub unit: Option<String>,
}

/// Where the depths count from, as the object states it - read, never judged (`diagnose` judges).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DatumFacts {
    /// `DefaultMdDatum` (WITSML) or `MdDatum` (RESQML 2.0.1), as a full uri.
    pub default: Option<String>,
    /// Each station's `Md.Datum`, as a full uri (always `None` for a deviation survey).
    pub per_station: Vec<Option<String>>,
    /// `MdInterval.Datum` (WITSML), as a full uri.
    pub interval: Option<String>,
}

/// The calculation methods the object states (`NominalCalcAlgorithm`, each `CalcAlgorithm`), as written.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MethodFacts {
    /// `NominalCalcAlgorithm`.
    pub nominal: Option<String>,
    /// Each station's `CalcAlgorithm`.
    pub per_station: Vec<Option<String>>,
}

/// The corrections the file says it used, in degrees (`GridConUsed`, `MagDeclUsed`, and per station
/// `CorUsed.StnGridConUsed`, `CorUsed.StnMagDeclUsed`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NorthCorrections {
    /// `GridConUsed`: the convergence.
    pub convergence: Option<Result<f64, UnitFault>>,
    /// `MagDeclUsed`: the declination.
    pub declination: Option<Result<f64, UnitFault>>,
    /// One per station, file order.
    pub per_station: Vec<StationCorrections>,
    /// `GridScaleFactorUsed` is stated (its value is not read: no unit family holds it).
    pub grid_scale_factor_stated: bool,
}

/// The corrections a station states for itself.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StationCorrections {
    /// `StnGridConUsed`.
    pub convergence: Option<Result<f64, UnitFault>>,
    /// `StnMagDeclUsed`.
    pub declination: Option<Result<f64, UnitFault>>,
}

/// What a station's `Tvd` is counted from (eml 2.3 `AbstractVerticalDepth`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TvdReference {
    /// `ReferencePointVerticalDepth.ReferencePoint`, a full uri.
    Point(String),
    /// `DatumVerticalDepth.Datum`, a vertical CRS (full uri), or none named (EML TRG 2.3 §6.26 p. 102:
    /// « Required when there is no default TVD datum associated with the data object »).
    VerticalCrs(Option<String>),
}

/// A stated true vertical depth, in metres, and what it is counted from.
#[derive(Debug, Clone, PartialEq)]
pub struct StatedTvd {
    /// The depth, in metres.
    pub metres: f64,
    /// What it counts from.
    pub from: TvdReference,
}

/// The values the file states at one station, converted (lengths in metres, rates in radians per metre).
/// Read for triangulation (`energyml_analysis::survey_qc::stated_against_computed`), never used to place.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StatedStation {
    /// The station, from 1, file order.
    pub station: usize,
    /// `Tvd`.
    pub tvd: Option<Result<StatedTvd, UnitFault>>,
    /// `DispNs`.
    pub disp_ns: Option<Result<f64, UnitFault>>,
    /// `DispEw`.
    pub disp_ew: Option<Result<f64, UnitFault>>,
    /// `VertSect`.
    pub vert_sect: Option<Result<f64, UnitFault>>,
    /// `Dls`.
    pub dls: Option<Result<f64, UnitFault>>,
    /// `RateBuild`.
    pub rate_build: Option<Result<f64, UnitFault>>,
    /// `RateTurn`.
    pub rate_turn: Option<Result<f64, UnitFault>>,
}

/// The vertical section the file states (`AziVertSect` in degrees, in the survey's own north;
/// `DispNsVertSectOrig`, `DispEwVertSectOrig` in metres) - WITSML TRG 2.1 §16.7 p. 352.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StatedSection {
    /// `AziVertSect`, degrees.
    pub azimuth: Option<Result<f64, UnitFault>>,
    /// `DispNsVertSectOrig`, metres.
    pub origin_north: Option<Result<f64, UnitFault>>,
    /// `DispEwVertSectOrig`, metres.
    pub origin_east: Option<Result<f64, UnitFault>>,
}

/// A survey's stations, read and converted, nothing placed and nothing judged.
#[derive(Debug, Clone, PartialEq)]
pub struct SurveyStations {
    /// The kind of object.
    pub kind: SurveyKind,
    /// Every station, file order, rejected ones included: MD in metres, angles in degrees,
    /// azimuths **in the file's own north** (no correction applied).
    pub measured: Vec<Measured>,
    /// The stations (from 1) whose `StatusTrajStation` is `rejected`.
    pub rejected: Vec<usize>,
    /// The datums the depths count from.
    pub datums: DatumFacts,
    /// The methods stated.
    pub methods: MethodFacts,
    /// `AziRef` as stated; a deviation survey: `Some(Grid)`, by its schema.
    pub north: Option<NorthReference>,
    /// The corrections the file says it used.
    pub corrections: NorthCorrections,
    /// One per station, same order as `measured` (all `None` fields for a deviation survey).
    pub stated: Vec<StatedStation>,
    /// The vertical section stated.
    pub section: StatedSection,
    /// A deviation survey's `FirstStationLocation`, as written; its CRS is not stated.
    pub first_station_location: Option<[f64; 3]>,
}

/// The stations kept for a computation, and their numbers in the file.
#[derive(Debug, Clone, PartialEq)]
pub struct Kept {
    /// The stations kept, file order.
    pub measured: Vec<Measured>,
    /// Their numbers in the file, from 1.
    pub stations: Vec<usize>,
}

impl SurveyStations {
    /// The stations a computation runs over: all of them, or without the rejected ones when the
    /// caller declares so. `Err(Finding::RejectedStation { station })` on the first rejected one otherwise.
    pub fn kept(&self, rejected: RejectedStations) -> Result<Kept, Finding> {
        keep(&self.measured, &self.rejected, rejected)
    }
}

/// The stations kept: `rejected` are the numbers (from 1, ascending) of the rejected stations.
pub(crate) fn keep(
    measured: &[Measured],
    rejected: &[usize],
    policy: RejectedStations,
) -> Result<Kept, Finding> {
    if rejected.is_empty() {
        return Ok(Kept {
            measured: measured.to_vec(),
            stations: (1..=measured.len()).collect(),
        });
    }
    match policy {
        RejectedStations::Refuse => Err(Finding::RejectedStation { station: rejected[0] }),
        RejectedStations::LeaveOut => {
            let mut kept = Kept {
                measured: Vec::new(),
                stations: Vec::new(),
            };
            for (at, station) in measured.iter().enumerate() {
                if !rejected.contains(&(at + 1)) {
                    kept.measured.push(*station);
                    kept.stations.push(at + 1);
                }
            }
            Ok(kept)
        }
    }
}

/// What the stations hold apart from their measured values (private: the measured values are a
/// result of their own, because a unit that cannot be read must not hide the datum).
pub(crate) struct Facts {
    pub(crate) kind: SurveyKind,
    /// How many stations the object holds.
    pub(crate) count: usize,
    pub(crate) rejected: Vec<usize>,
    pub(crate) datums: DatumFacts,
    pub(crate) methods: MethodFacts,
    pub(crate) north: Option<NorthReference>,
    pub(crate) corrections: NorthCorrections,
    pub(crate) stated: Vec<StatedStation>,
    pub(crate) section: StatedSection,
    pub(crate) first_station_location: Option<[f64; 3]>,
}

impl Facts {
    fn into_stations(self, measured: Vec<Measured>) -> SurveyStations {
        SurveyStations {
            kind: self.kind,
            measured,
            rejected: self.rejected,
            datums: self.datums,
            methods: self.methods,
            north: self.north,
            corrections: self.corrections,
            stated: self.stated,
            section: self.section,
            first_station_location: self.first_station_location,
        }
    }
}

/// What a reader gives: the facts (the check `Stations` passed), and the measured values, which
/// may fail on their own (the check `Units`).
pub(crate) type Reading = Result<(Facts, Result<Vec<Measured>, Fault>), Fault>;

type SurveyReader = fn(&dyn Reflect, &dyn ValueSource) -> Reading;

/// The readers, by the concept's name: a registry, not a `match`.
const SURVEY_READERS: &[(&str, SurveyReader)] = &[
    ("Trajectory", witsml::read),
    ("DeviationSurveyRepresentation", deviation::read),
];

/// The facts and the measured values of a survey.
pub(crate) fn read_parts(survey: &dyn Reflect, source: &dyn ValueSource) -> Reading {
    let type_key = survey.type_info().type_key;
    match SURVEY_READERS.iter().find(|(key, _)| *key == type_key) {
        Some((_, read)) => read(survey, source),
        None => Err(Fault::new(
            CheckKind::Stations,
            Finding::NotASurvey {
                type_key: type_key.to_string(),
            },
            SurveyKind::WitsmlTrajectory,
        )),
    }
}

/// The stations of a survey, read and converted - a WITSML `Trajectory` or a RESQML 2.0.1
/// `DeviationSurveyRepresentation`, told apart by the concept's name. Refuses only what makes the
/// stations unreadable: no station, a depth or an angle with no number or no usable unit, an array
/// that cannot be read. Everything else (method, datums, north, status) is read and left to `diagnose`.
pub fn stations_of(survey: &dyn Reflect, source: &dyn ValueSource) -> Result<SurveyStations, SurveyRefusal> {
    let (facts, measured) = read_parts(survey, source).map_err(SurveyRefusal::from)?;
    let measured = measured.map_err(SurveyRefusal::from)?;
    Ok(facts.into_stations(measured))
}

/// Whether two full uris name the same object - `well::same_uri`, the one comparison of this crate,
/// exposed for callers above it: same uuid, object versions that do not contradict.
pub fn same_object(a: &str, b: &str) -> bool {
    crate::well::same_uri(a, b)
}

/// What the document states, by the field present: a deviation survey's `StationCount`, a WITSML
/// trajectory's stations.
pub(crate) fn declared(survey: &dyn Reflect) -> Option<u64> {
    if survey.field("StationCount").is_some() {
        return crate::values::scalar_i64(survey, "StationCount").and_then(|count| u64::try_from(count).ok());
    }
    Some(field_list(survey, "TrajectoryStation").len() as u64)
}

/// The survey, drawn - see the module.
pub(crate) fn read(survey: &dyn Reflect, source: &dyn ValueSource) -> crate::error::Result<Vec<Patch>> {
    let placed: Result<PlacedSurvey, SurveyRefusal> =
        read_declaring(survey, source, &SurveyDeclarations::default());
    let placed = placed.map_err(Error::from)?;
    let count = placed.points.len() as u32;
    Ok(vec![Patch {
        index: 0,
        node_offset: 0,
        points: placed.points,
        topology: Topology::Polylines(vec![count]),
        closed: Vec::new(),
    }])
}
