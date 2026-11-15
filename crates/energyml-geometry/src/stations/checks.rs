//! The checks a survey goes through, in the order they run, and the typed bilan of them (`diagnose`).
//!
//! `read` and `diagnose` follow **the same run**: `read` stops at the first failure and gives its
//! sentence, `diagnose` goes on wherever the next check does not depend on the failed one and gives
//! every outcome. No text is written for a face here: a [`Finding`] is a name and its facts, and the
//! sentence of the library (the error `read` returns) is built from it at the point of the failure.

use std::fmt;

use energyml_core::value::Reflect;

use super::north::{self, Frame, Horizontal, NorthSource};
use super::place::{self, World};
use super::{
    Facts, Kept, MINIMUM_CURVATURE, NorthReference, RejectedStations, SurveyDeclarations, SurveyKind,
    UnitFault, keep, read_parts,
};
use crate::error::Error;
use crate::min_curvature::{self, Measured, StationsError};
use crate::source::ValueSource;

/// The checks, in the order they run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CheckKind {
    /// The object is a survey, with its stations (and, for a deviation survey, its three arrays).
    Stations,
    /// The method stated is minimum curvature, or none is stated.
    Method,
    /// No station is `rejected`, or the caller leaves them out.
    Status,
    /// Every depth and every angle has a number and a unit this build converts.
    Units,
    /// Every station counts its depths from one datum.
    Datum,
    /// The first station kept is at the datum (MD 0).
    StartsAtDatum,
    /// The stations join: the path of the minimum curvature method exists.
    Path,
    /// The file says a north that can be applied, or the caller declares one, or none is needed.
    North,
    /// The datum is placed, in a CRS whose units are read.
    Placement,
    /// The north can be laid in the frame of the datum.
    NorthInFrame,
}

impl CheckKind {
    /// Every check, in order.
    pub(crate) const ALL: [CheckKind; 10] = [
        CheckKind::Stations,
        CheckKind::Method,
        CheckKind::Status,
        CheckKind::Units,
        CheckKind::Datum,
        CheckKind::StartsAtDatum,
        CheckKind::Path,
        CheckKind::North,
        CheckKind::Placement,
        CheckKind::NorthInFrame,
    ];
}

/// What happened to a check.
#[derive(Debug, Clone, PartialEq)]
pub enum CheckOutcome {
    /// Passed.
    Passed,
    /// Failed, by name.
    Failed(Finding),
    /// Not run, because a check it depends on failed.
    NotReached,
}

/// One check and its outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct SurveyCheck {
    /// Which check.
    pub kind: CheckKind,
    /// What happened to it.
    pub outcome: CheckOutcome,
}

/// Every check, in order - always ten entries.
#[derive(Debug, Clone, PartialEq)]
pub struct SurveyDiagnosis {
    /// The ten checks, in the order of [`CheckKind`].
    pub checks: Vec<SurveyCheck>,
}

impl SurveyDiagnosis {
    /// The outcome of one check.
    pub fn outcome(&self, kind: CheckKind) -> &CheckOutcome {
        &self.checks[kind as usize].outcome
    }

    /// The failures, in order.
    pub fn failures(&self) -> impl Iterator<Item = (CheckKind, &Finding)> {
        self.checks.iter().filter_map(|check| match &check.outcome {
            CheckOutcome::Failed(finding) => Some((check.kind, finding)),
            _ => None,
        })
    }

    /// No check failed (some may be `NotReached` only if an earlier one failed, so: all passed).
    pub fn passed(&self) -> bool {
        self.checks.iter().all(|check| check.outcome == CheckOutcome::Passed)
    }
}

/// Why a check failed, by name. Stations are numbered from 1, in the file's order.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Finding {
    // -- Stations
    /// The object is neither a WITSML `Trajectory` nor a RESQML `DeviationSurveyRepresentation`.
    NotASurvey {
        /// The concept met.
        type_key: String,
    },
    /// No station.
    NoStation,
    /// An array of a deviation survey cannot be read.
    ArrayUnreadable {
        /// The array.
        field: &'static str,
        /// Why.
        reason: String,
    },
    /// An array of a deviation survey has not one value per station.
    ArrayLength {
        /// The array.
        field: &'static str,
        /// How many values it holds.
        found: usize,
        /// How many stations the survey states.
        stations: usize,
    },
    // -- Method
    /// A method other than minimum curvature is stated.
    OtherMethod {
        /// The station, or `None` for the nominal method.
        station: Option<usize>,
        /// The field.
        field: &'static str,
        /// The method, as written.
        method: String,
    },
    // -- Status
    /// A rejected station, and the caller does not leave it out.
    RejectedStation {
        /// The station.
        station: usize,
    },
    // -- Units
    /// A station with no `Md`.
    NoMd {
        /// The station.
        station: usize,
    },
    /// A value that holds no number.
    NoNumber {
        /// The station, or `None` for a unit of the whole survey.
        station: Option<usize>,
        /// The field.
        field: &'static str,
    },
    /// A value with no unit.
    UnitMissing {
        /// The station, or `None`.
        station: Option<usize>,
        /// The field.
        field: &'static str,
    },
    /// A unit this build does not convert.
    UnitRefused {
        /// The station, or `None`.
        station: Option<usize>,
        /// The field.
        field: &'static str,
        /// The unit as written.
        unit: String,
        /// The domain's refusal.
        reason: String,
    },
    // -- Datum
    /// A station whose depths name no datum.
    NoDatum {
        /// The station.
        station: usize,
    },
    /// Stations that count from two datums.
    TwoDatums {
        /// The first station that counts from another.
        station: usize,
        /// The datum of the stations before.
        first: String,
        /// This station's.
        other: String,
    },
    /// `MdInterval.Datum` names another datum than the stations.
    IntervalDatumDiffers {
        /// What the interval names.
        stated: String,
        /// What the stations name.
        stations: String,
    },
    // -- StartsAtDatum
    /// The first station kept is not at MD 0.
    StartsBelowDatum {
        /// The station.
        station: usize,
        /// Its measured depth, metres.
        md: f64,
    },
    // -- Path
    /// The stations do not join.
    Stations(StationsError),
    // -- North
    /// An inclined survey that states no north, and none declared.
    NorthNotStated {
        /// The first station off the vertical.
        station: usize,
        /// Its measured depth.
        md: f64,
        /// Its inclination, degrees.
        inclination: f64,
    },
    /// A north declared over a survey that states its own.
    NorthAlreadyStated {
        /// What the file states.
        stated: NorthReference,
    },
    /// A true or magnetic north whose corrections the file does not state.
    NorthNotApplied {
        /// The north stated.
        north: NorthReference,
        /// What is missing.
        missing: MissingCorrection,
    },
    // -- Placement
    /// The datum cannot be placed.
    DatumNotPlaced {
        /// The datum's uri.
        datum: String,
        /// Why.
        reason: String,
    },
    /// The CRS `energyml_domain::crs::of` gives the survey cannot be read.
    CrsUnreadable {
        /// Why.
        reason: String,
    },
    /// The unit of the CRS's vertical axis is refused.
    VerticalUnitRefused {
        /// The unit as written.
        unit: String,
        /// The domain's refusal.
        reason: String,
    },
    // -- NorthInFrame
    /// A grid-north survey whose datum is placed in no projected CRS.
    GridWithoutProjected,
    /// A projected CRS that states no axis order.
    AxisOrderUnstated,
    /// A projected CRS whose axes are not easting then northing.
    AxisOrder {
        /// The order as written.
        order: String,
    },
    /// A plant-north survey whose datum is written in no compound.
    PlantWithoutCompound,
    /// A compound whose axes are not east then north.
    PlantAxes {
        /// The first axis.
        first: String,
        /// The second.
        second: String,
    },
    /// A compound whose horizontal axes cannot be read.
    PlantAxesUnreadable,
    /// A projected system with no unit.
    HorizontalUnitUnstated,
    /// A projected system whose unit is refused.
    HorizontalUnitRefused {
        /// The unit as written.
        unit: String,
        /// The domain's refusal.
        reason: String,
    },
}

/// What a true- or magnetic-north survey lacks to be turned to grid north.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MissingCorrection {
    /// A station with an azimuth has no convergence (neither its own nor the trajectory's).
    Convergence {
        /// The station.
        station: usize,
    },
    /// A magnetic-north station with an azimuth has no declination.
    Declination {
        /// The station.
        station: usize,
    },
    /// A correction is stated but its unit cannot be read.
    Unreadable {
        /// The value and its unit.
        fault: UnitFault,
    },
    /// Astronomic, compass, or a value outside the list: the file has no field that corrects it.
    NoRule,
}

/// A refusal: the check, its typed cause, and the library's sentence (the error `read` returns).
#[derive(Debug)]
pub struct SurveyRefusal {
    /// The check that failed.
    pub check: CheckKind,
    /// Why, by name.
    pub finding: Finding,
    /// The library's sentence.
    pub error: Error,
}

impl fmt::Display for SurveyRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.error)
    }
}

impl std::error::Error for SurveyRefusal {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

impl From<SurveyRefusal> for Error {
    fn from(refusal: SurveyRefusal) -> Error {
        refusal.error
    }
}

/// A failed check inside the run: where, why, and the sentence, built at the point of the failure.
#[derive(Debug)]
pub(crate) struct Fault {
    pub(crate) kind: CheckKind,
    pub(crate) finding: Finding,
    pub(crate) error: Error,
}

impl Fault {
    /// A fault whose sentence is the one of `sentence`.
    pub(crate) fn new(kind: CheckKind, finding: Finding, survey: SurveyKind) -> Fault {
        let error = sentence(survey, &finding, survey.at());
        Fault { kind, finding, error }
    }

    /// A fault whose sentence mentions `at`, the place the north is read from.
    pub(crate) fn at(kind: CheckKind, finding: Finding, survey: SurveyKind, at: &str) -> Fault {
        let error = sentence(survey, &finding, at);
        Fault { kind, finding, error }
    }

    /// A fault with the error of the domain (or of an array read) as its sentence, as it came.
    pub(crate) fn with_error(kind: CheckKind, finding: Finding, error: Error) -> Fault {
        Fault { kind, finding, error }
    }
}

impl From<Fault> for SurveyRefusal {
    fn from(fault: Fault) -> SurveyRefusal {
        SurveyRefusal {
            check: fault.kind,
            finding: fault.finding,
            error: fault.error,
        }
    }
}

/// What a true- or magnetic-north survey lacks (WITSML TRG 2.1 §16.7 p. 352-353).
const CORRECTION_NOT_STATED: &str = "a true- or magnetic-north survey whose file does not state the correction that turns it to grid north - the convergence `GridConUsed` (or `StnGridConUsed`), and from magnetic north the declination `MagDeclUsed` (or `StnMagDeclUsed`) (WITSML TRG 2.1 §16.7 p. 352-353)";

/// The sentence of a finding: the one place the library's phrases are written. A finding that carries
/// the refusal of the domain (`UnitRefused`, `DatumNotPlaced`, `CrsUnreadable`, `VerticalUnitRefused`,
/// `HorizontalUnitRefused`) is built with `Fault::with_error` instead, and only has a fallback here.
/// `at` is where the north of the finding is read from (the checks `North` and `NorthInFrame`).
fn sentence(survey: SurveyKind, finding: &Finding, at: &str) -> Error {
    let object = survey.at();
    match finding {
        Finding::NotASurvey { type_key } => Error::unsupported(
            "a survey reader met an object that is neither a WITSML `Trajectory` nor a RESQML `DeviationSurveyRepresentation`",
            type_key,
        ),
        Finding::NoStation => Error::malformed(
            object,
            survey.pick(
                "a trajectory with no `TrajectoryStation`: nothing states where the hole is",
                "a deviation survey with no station: nothing states where the hole is",
            ),
        ),
        Finding::ArrayUnreadable { field, reason } => Error::malformed(format!("{object}.{field}"), reason),
        Finding::ArrayLength { field, found, stations } => Error::malformed(
            format!("{object}.{field}"),
            format!(
                "{found} value(s) for {stations} station(s) - RESQML TRG 2.0.1 §7.5.7 p. 139 asks one per station"
            ),
        ),
        Finding::OtherMethod { station, field, method } => {
            let place = station.map_or_else(|| object.to_string(), |station| survey.here(station));
            Error::unsupported(
                "a survey computed by a method other than minimum curvature - drawing it by minimum curvature would move the hole from where its producer put it",
                format!("{place}.{field} = `{method}`"),
            )
        }
        Finding::RejectedStation { station } => Error::unsupported(
            "a trajectory holding a rejected station - it is left out only when the caller declares so",
            survey.here(*station),
        ),
        Finding::NoMd { station } => Error::malformed(survey.here(*station), "a station with no `Md`"),
        Finding::NoNumber { station, field } => match station {
            Some(station) if *field == "Md" => {
                Error::malformed(survey.here(*station), "an `Md` holding no number")
            }
            Some(station) => Error::malformed(format!("{}.{field}", survey.here(*station)), "no number"),
            None => Error::malformed(format!("{object}.{field}"), "no number"),
        },
        Finding::UnitMissing { station, field } => match station {
            Some(station) if *field == "Md" => Error::malformed(
                survey.here(*station),
                "an `Md` with no unit: a length in nothing is not a depth",
            ),
            Some(station) => Error::malformed(
                format!("{}.{field}", survey.here(*station)),
                "an angle with no unit - degrees or radians is not a guess to make",
            ),
            None => Error::malformed(
                format!("{object}.{field}"),
                "no unit stated - a length or an angle in nothing is not a guess to make",
            ),
        },
        Finding::UnitRefused { field, reason, .. } => Error::malformed(format!("{object}.{field}"), reason),
        Finding::NoDatum { station } => Error::unsupported(
            survey.pick(
                "a WITSML trajectory whose measured depths name no datum - neither `DefaultMdDatum` nor the station's `Md.Datum`",
                "a deviation survey whose measured depths name no datum - it states no `MdDatum`",
            ),
            survey.here(*station),
        ),
        Finding::TwoDatums { station, first, other } => Error::unsupported(
            "a WITSML trajectory whose stations count their depths from two datums",
            format!(
                "{}: {other}, where the stations before count from {first}",
                survey.here(*station)
            ),
        ),
        Finding::IntervalDatumDiffers { stated, stations } => Error::malformed(
            object,
            format!(
                "`MdInterval.Datum` names {stated} and the stations count from {stations}: the CRS the trajectory is given is read from the first, the hole would be drawn from the second"
            ),
        ),
        Finding::StartsBelowDatum { station, md } => Error::unsupported(
            survey.pick(
                "a WITSML trajectory whose first station is not at its datum - where the hole runs above it is not stated",
                "a deviation survey whose first station is not at its datum - where the hole runs above it is not stated",
            ),
            format!("{}: MD {md} m", survey.here(*station)),
        ),
        Finding::Stations(error) => Error::malformed(object, error),
        Finding::NorthNotStated { .. } => Error::unsupported(
            survey.pick(
                "a WITSML trajectory that leaves the vertical and states no `AziRef` - which north its azimuths turn from is not said (WITSML TRG 2.1 §16.7 p. 352)",
                "a deviation survey that leaves the vertical and states no north - which north its azimuths turn from is not said",
            ),
            format!("{object}.AziRef"),
        ),
        Finding::NorthAlreadyStated { stated } => Error::unsupported(
            "a north declared by the caller over a survey that states its own `AziRef` - a declaration never replaces what the file says",
            format!("{object}.AziRef = `{}`", stated.lexical()),
        ),
        Finding::NorthNotApplied { north, missing } => match missing {
            MissingCorrection::NoRule => Error::unsupported(
                "an azimuth reference other than grid or plant north - it needs a convergence or a declination this build does not apply",
                format!("{object}.AziRef = `{}`", north.lexical()),
            ),
            MissingCorrection::Convergence { station } => Error::unsupported(
                CORRECTION_NOT_STATED,
                format!(
                    "{object}.AziRef = `{}`: station {station} has no convergence",
                    north.lexical()
                ),
            ),
            MissingCorrection::Declination { station } => Error::unsupported(
                CORRECTION_NOT_STATED,
                format!(
                    "{object}.AziRef = `{}`: station {station} has no declination",
                    north.lexical()
                ),
            ),
            MissingCorrection::Unreadable { fault } => Error::unsupported(
                CORRECTION_NOT_STATED,
                format!(
                    "{object}.AziRef = `{}`: {object}.{}: no readable unit",
                    north.lexical(),
                    fault.field
                ),
            ),
        },
        Finding::DatumNotPlaced { datum, reason } => Error::malformed(datum, reason),
        Finding::CrsUnreadable { reason } => Error::malformed(object, reason),
        Finding::VerticalUnitRefused { unit, reason } => {
            Error::malformed(format!("{object} (vertical unit `{unit}`)"), reason)
        }
        Finding::GridWithoutProjected => Error::unsupported(
            "a grid-north survey whose datum is not placed in a projected CRS - the grid it turns from is not named",
            at,
        ),
        Finding::AxisOrderUnstated => Error::unsupported("a projected CRS that states no axis order", at),
        Finding::AxisOrder { order } => Error::unsupported(
            "a grid-north survey in a projected system whose axes are not easting then northing",
            format!("{at}: AxisOrder `{order}`"),
        ),
        Finding::PlantWithoutCompound => Error::unsupported(
            "a plant-north survey whose datum is written in no local engineering compound - no plant is named to turn from",
            at,
        ),
        Finding::PlantAxes { first, second } => Error::unsupported(
            "a plant-north survey in a compound whose axes are not east then north",
            format!("{at}: `{first}`, `{second}`"),
        ),
        Finding::PlantAxesUnreadable => Error::unsupported(
            "a plant-north survey whose compound's horizontal axes cannot be read - which way its north points is not known",
            at,
        ),
        Finding::HorizontalUnitUnstated => Error::unsupported(
            "a projected system that states no unit - a displacement in metres cannot be added to it",
            at,
        ),
        Finding::HorizontalUnitRefused { unit, reason } => {
            Error::malformed(format!("{at} (horizontal unit `{unit}`)"), reason)
        }
    }
}

/// The station numbers of an error, from the rank among the kept stations to the number in the file.
pub(crate) fn renumbered(error: StationsError, kept: &[usize]) -> StationsError {
    let number = |station: usize| kept.get(station.wrapping_sub(1)).copied().unwrap_or(station);
    match error {
        StationsError::TooFewStations { .. } => error,
        StationsError::NotANumber { station, what, given } => StationsError::NotANumber {
            station: number(station),
            what,
            given,
        },
        StationsError::NotIncreasing { station, md, previous } => StationsError::NotIncreasing {
            station: number(station),
            md,
            previous,
        },
        StationsError::NoInclination { station } => StationsError::NoInclination {
            station: number(station),
        },
        StationsError::NoAzimuth { station, inclination } => StationsError::NoAzimuth {
            station: number(station),
            inclination,
        },
        StationsError::BadInclination { station, given } => StationsError::BadInclination {
            station: number(station),
            given,
        },
        StationsError::BadAzimuth { station, given } => StationsError::BadAzimuth {
            station: number(station),
            given,
        },
        StationsError::Reversal { station } => StationsError::Reversal {
            station: number(station),
        },
    }
}

// -- The individual checks ------------------------------------------------------------------

/// `Method`: the nominal method, then each station's, must be minimum curvature when stated.
fn check_methods(facts: &Facts) -> Result<(), Fault> {
    let not_minimum = |method: &str| !method.eq_ignore_ascii_case(MINIMUM_CURVATURE);
    if let Some(method) = &facts.methods.nominal
        && not_minimum(method)
    {
        return Err(Fault::new(
            CheckKind::Method,
            Finding::OtherMethod {
                station: None,
                field: "NominalCalcAlgorithm",
                method: method.clone(),
            },
            facts.kind,
        ));
    }
    for (at, method) in facts.methods.per_station.iter().enumerate() {
        if let Some(method) = method
            && not_minimum(method)
        {
            return Err(Fault::new(
                CheckKind::Method,
                Finding::OtherMethod {
                    station: Some(at + 1),
                    field: "CalcAlgorithm",
                    method: method.clone(),
                },
                facts.kind,
            ));
        }
    }
    Ok(())
}

/// `Status`: a rejected station refuses the survey unless the caller leaves it out.
fn check_status(facts: &Facts, policy: RejectedStations) -> Result<(), Fault> {
    match (policy, facts.rejected.first()) {
        (RejectedStations::Refuse, Some(&station)) => Err(Fault::new(
            CheckKind::Status,
            Finding::RejectedStation { station },
            facts.kind,
        )),
        _ => Ok(()),
    }
}

/// `Datum`: every station counts from one datum, the one `MdInterval.Datum` names when it is stated.
/// Gives the datum's uri.
fn check_datums(facts: &Facts) -> Result<String, Fault> {
    let mut datum: Option<String> = None;
    for at in 0..facts.count {
        let counted_from = facts
            .datums
            .per_station
            .get(at)
            .cloned()
            .flatten()
            .or_else(|| facts.datums.default.clone())
            .ok_or_else(|| {
                Fault::new(CheckKind::Datum, Finding::NoDatum { station: at + 1 }, facts.kind)
            })?;
        match &datum {
            None => datum = Some(counted_from),
            Some(first) if crate::well::same_uri(first, &counted_from) => {}
            Some(first) => {
                return Err(Fault::new(
                    CheckKind::Datum,
                    Finding::TwoDatums {
                        station: at + 1,
                        first: first.clone(),
                        other: counted_from,
                    },
                    facts.kind,
                ));
            }
        }
    }
    let datum = datum.expect("one station at least, each with a datum");
    if let Some(stated) = &facts.datums.interval
        && !crate::well::same_uri(stated, &datum)
    {
        return Err(Fault::new(
            CheckKind::Datum,
            Finding::IntervalDatumDiffers {
                stated: stated.clone(),
                stations: datum,
            },
            facts.kind,
        ));
    }
    Ok(datum)
}

/// `StartsAtDatum`: MD 0 is the datum, so the first station kept must be there.
fn check_start(kept: &Kept, survey: SurveyKind) -> Result<(), Fault> {
    match (kept.measured.first(), kept.stations.first()) {
        (Some(first), Some(&station)) if first.md != 0.0 => Err(Fault::new(
            CheckKind::StartsAtDatum,
            Finding::StartsBelowDatum { station, md: first.md },
            survey,
        )),
        _ => Ok(()),
    }
}

/// `Path`: the stations kept join, on the azimuths as the file states them.
fn check_path(kept: &Kept, survey: SurveyKind) -> Result<(), Fault> {
    min_curvature::positions(&kept.measured)
        .map(|_| ())
        .map_err(|error| {
            Fault::new(
                CheckKind::Path,
                Finding::Stations(renumbered(error, &kept.stations)),
                survey,
            )
        })
}

// -- The run ----------------------------------------------------------------------------------

/// How far a run goes after a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Stop at the first failure: what `read` does.
    StopAtFirst,
    /// Go on wherever the next check does not depend on the failed one: what `diagnose` does.
    All,
}

/// Which checks a run makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    /// All ten.
    Full,
    /// The checks that place the datum and its grid axes, with no north asked (the stations, the
    /// status, the units, the datum, the first station, the placement, the north in the frame).
    FrameOnly,
}

enum Slot {
    NotReached,
    Passed,
    Failed(Fault),
}

/// A survey that passed every check, with what the placement needs.
pub(crate) struct Placed {
    pub(crate) facts: Facts,
    pub(crate) kept: Kept,
    pub(crate) north: NorthSource,
    /// The stations kept, their azimuths turned to the north placed with.
    pub(crate) corrected: Vec<Measured>,
    pub(crate) world: World,
    pub(crate) horizontal: Option<Horizontal>,
}

/// The outcome of a run.
pub(crate) struct Run {
    slots: Vec<Slot>,
    placed: Option<Placed>,
}

impl Run {
    /// The first failure, or the placed survey.
    pub(crate) fn into_result(self) -> Result<Placed, Fault> {
        let Run { slots, placed } = self;
        for slot in slots {
            if let Slot::Failed(fault) = slot {
                return Err(fault);
            }
        }
        Ok(placed.expect("every check passed, so the survey is placed"))
    }

    fn into_diagnosis(self) -> SurveyDiagnosis {
        SurveyDiagnosis {
            checks: self
                .slots
                .into_iter()
                .zip(CheckKind::ALL)
                .map(|(slot, kind)| SurveyCheck {
                    kind,
                    outcome: match slot {
                        Slot::NotReached => CheckOutcome::NotReached,
                        Slot::Passed => CheckOutcome::Passed,
                        Slot::Failed(fault) => CheckOutcome::Failed(fault.finding),
                    },
                })
                .collect(),
        }
    }
}

struct Board {
    slots: Vec<Slot>,
    mode: Mode,
}

impl Board {
    fn new(mode: Mode) -> Board {
        Board {
            slots: CheckKind::ALL.iter().map(|_| Slot::NotReached).collect(),
            mode,
        }
    }

    fn pass(&mut self, kind: CheckKind) {
        self.slots[kind as usize] = Slot::Passed;
    }

    fn fail(&mut self, fault: Fault) {
        let at = fault.kind as usize;
        self.slots[at] = Slot::Failed(fault);
    }

    /// Records the result of a check, and gives its value when it passed.
    fn settle<T>(&mut self, kind: CheckKind, result: Result<T, Fault>) -> Option<T> {
        match result {
            Ok(value) => {
                self.pass(kind);
                Some(value)
            }
            Err(fault) => {
                self.fail(fault);
                None
            }
        }
    }

    fn any_failed(&self) -> bool {
        self.slots.iter().any(|slot| matches!(slot, Slot::Failed(_)))
    }

    /// Whether the run stops here: a failure, and the mode stops at the first.
    fn halted(&self) -> bool {
        self.mode == Mode::StopAtFirst && self.any_failed()
    }

    fn finish(self, placed: Option<Placed>) -> Run {
        Run {
            slots: self.slots,
            placed,
        }
    }
}

/// The ten checks, in their order. The one run `read_declaring`, `diagnose` and
/// `placement_without_north` share.
pub(crate) fn run(
    survey: &dyn Reflect,
    source: &dyn ValueSource,
    declared: &SurveyDeclarations,
    mode: Mode,
    scope: Scope,
) -> Run {
    let mut board = Board::new(mode);
    let full = scope == Scope::Full;

    // 1. Stations.
    let (facts, measured) = match read_parts(survey, source) {
        Ok(parts) => parts,
        Err(fault) => {
            board.fail(fault);
            return board.finish(None);
        }
    };
    board.pass(CheckKind::Stations);
    let kind = facts.kind;

    // 2. Method.
    if full {
        board.settle(CheckKind::Method, check_methods(&facts));
        if board.halted() {
            return board.finish(None);
        }
    }
    // 3. Status.
    let status = board.settle(CheckKind::Status, check_status(&facts, declared.rejected));
    if board.halted() {
        return board.finish(None);
    }
    // 4. Units.
    let measured = board.settle(CheckKind::Units, measured);
    if board.halted() {
        return board.finish(None);
    }
    // 5. Datum.
    let datum = board.settle(CheckKind::Datum, check_datums(&facts));
    if board.halted() {
        return board.finish(None);
    }

    // The stations a computation runs over: needs the status and the units.
    let kept = match (status, measured) {
        (Some(()), Some(measured)) => keep(&measured, &facts.rejected, declared.rejected).ok(),
        _ => None,
    };

    // 6. StartsAtDatum.
    let starts = match &kept {
        Some(kept) => board.settle(CheckKind::StartsAtDatum, check_start(kept, kind)).is_some(),
        None => false,
    };
    if board.halted() {
        return board.finish(None);
    }

    // 7. Path, 8. North.
    let mut north = None;
    if full {
        if let Some(kept) = &kept {
            board.settle(CheckKind::Path, check_path(kept, kind));
            if board.halted() {
                return board.finish(None);
            }
            north = board.settle(CheckKind::North, north::north_of(&facts, kept, declared.north));
            if board.halted() {
                return board.finish(None);
            }
        }
    } else if let Some(kept) = &kept {
        north = Some((NorthSource::NotNeeded, Some(Frame::Grid), kept.measured.clone()));
    }

    // 9. Placement.
    let world = match (&datum, starts) {
        (Some(datum), true) => board.settle(CheckKind::Placement, place::place_stage(survey, source, datum, kind)),
        _ => None,
    };
    if board.halted() {
        return board.finish(None);
    }

    // 10. NorthInFrame.
    let (Some(kept), Some((north, frame, corrected)), Some(world), Some(datum)) = (kept, north, world, datum)
    else {
        return board.finish(None);
    };
    let horizontal = match frame {
        // No station leaves the vertical: no azimuth moves anything, nothing is read.
        None => {
            board.pass(CheckKind::NorthInFrame);
            None
        }
        Some(frame) => {
            let at = match (&north, full) {
                (_, false) => format!("{} (frame without a north)", kind.at()),
                (NorthSource::Declared(_), true) => format!("{} (north declared by the caller)", kind.at()),
                _ => format!("{}.AziRef", kind.at()),
            };
            match board.settle(
                CheckKind::NorthInFrame,
                Horizontal::of(frame, &datum, &world.placed, source, &at, kind),
            ) {
                Some(horizontal) => Some(horizontal),
                None => return board.finish(None),
            }
        }
    };

    if board.any_failed() {
        return board.finish(None);
    }
    board.finish(Some(Placed {
        facts,
        kept,
        north,
        corrected,
        world,
        horizontal,
    }))
}

/// Every check of `read`, in its order, each with its outcome - continuing after a failure
/// wherever the next check does not depend on the failed one.
pub fn diagnose(
    survey: &dyn Reflect,
    source: &dyn ValueSource,
    declared: &SurveyDeclarations,
) -> SurveyDiagnosis {
    run(survey, source, declared, Mode::All, Scope::Full).into_diagnosis()
}
