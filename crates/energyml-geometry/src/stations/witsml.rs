//! A WITSML `Trajectory`, read: its stations, the datums they count from, the methods, the north and
//! the corrections it states, and the values it states at each station.

use energyml_core::measure;
use energyml_core::value::Reflect;
use energyml_domain::units;

use super::checks::{CheckKind, Fault, Finding};
use super::{
    DatumFacts, Facts, MethodFacts, NorthCorrections, NorthReference, Reading, StatedSection, StatedStation,
    StatedTvd, StationCorrections, SurveyKind, TvdReference, UnitFault,
};
use crate::error::Error;
use crate::geometry::{field_list, field_object};
use crate::min_curvature::Measured;
use crate::source::ValueSource;
use crate::well::reference_of;

const KIND: SurveyKind = SurveyKind::WitsmlTrajectory;

/// A `Trajectory`: the facts, and the measured values apart.
pub(super) fn read(survey: &dyn Reflect, _source: &dyn ValueSource) -> Reading {
    let listed = field_list(survey, "TrajectoryStation");
    if listed.is_empty() {
        return Err(Fault::new(CheckKind::Stations, Finding::NoStation, KIND));
    }

    // -- what the file says, with no judgement --------------------------------
    let methods = MethodFacts {
        nominal: measure::enum_value(survey, "NominalCalcAlgorithm"),
        per_station: listed
            .iter()
            .map(|station| measure::enum_value(*station, "CalcAlgorithm"))
            .collect(),
    };
    let datums = DatumFacts {
        default: reference_of(survey, "DefaultMdDatum"),
        per_station: listed
            .iter()
            .map(|station| field_object(*station, "Md").and_then(|md| reference_of(md, "Datum")))
            .collect(),
        interval: field_object(survey, "MdInterval").and_then(|interval| reference_of(interval, "Datum")),
    };
    let rejected: Vec<usize> = listed
        .iter()
        .enumerate()
        .filter(|(_, station)| {
            measure::enum_value(**station, "StatusTrajStation").as_deref() == Some("rejected")
        })
        .map(|(at, _)| at + 1)
        .collect();
    let north = measure::enum_value(survey, "AziRef").map(|text| NorthReference::from_lexical(&text));
    let corrections = NorthCorrections {
        convergence: read_stated(survey, "GridConUsed", to_degrees),
        declination: read_stated(survey, "MagDeclUsed", to_degrees),
        per_station: listed
            .iter()
            .map(|station| {
                let used = field_object(*station, "CorUsed");
                StationCorrections {
                    convergence: used.and_then(|used| read_stated(used, "StnGridConUsed", to_degrees)),
                    declination: used.and_then(|used| read_stated(used, "StnMagDeclUsed", to_degrees)),
                }
            })
            .collect(),
        grid_scale_factor_stated: field_object(survey, "GridScaleFactorUsed").is_some(),
    };
    let stated: Vec<StatedStation> = listed
        .iter()
        .enumerate()
        .map(|(at, station)| StatedStation {
            station: at + 1,
            tvd: read_tvd(*station),
            disp_ns: read_stated(*station, "DispNs", units::metres),
            disp_ew: read_stated(*station, "DispEw", units::metres),
            vert_sect: read_stated(*station, "VertSect", units::metres),
            dls: read_stated(*station, "Dls", to_radians_per_metre),
            rate_build: read_stated(*station, "RateBuild", to_radians_per_metre),
            rate_turn: read_stated(*station, "RateTurn", to_radians_per_metre),
        })
        .collect();
    let section = StatedSection {
        azimuth: read_stated(survey, "AziVertSect", to_degrees),
        origin_north: read_stated(survey, "DispNsVertSectOrig", units::metres),
        origin_east: read_stated(survey, "DispEwVertSectOrig", units::metres),
    };

    let facts = Facts {
        kind: KIND,
        count: listed.len(),
        rejected,
        datums,
        methods,
        north,
        corrections,
        stated,
        section,
        first_station_location: None,
    };

    // -- the measured values, in the file's order: a failure here is the check `Units` ---
    let measured = listed
        .iter()
        .enumerate()
        .map(|(at, station)| -> Result<Measured, Fault> {
            let number = at + 1;
            let holder = field_object(*station, "Md")
                .ok_or_else(|| Fault::new(CheckKind::Units, Finding::NoMd { station: number }, KIND))?;
            Ok(Measured {
                md: measured_depth(holder, number)?,
                inclination: angle(*station, "Incl", number)?,
                azimuth: angle(*station, "Azi", number)?,
            })
        })
        .collect::<Result<Vec<_>, Fault>>();
    Ok((facts, measured))
}

/// A measured depth, in metres: `Md.MeasuredDepth` (eml 2.3), or `Md` itself
/// where it is the measure (WITSML 2.0) - the field present, never the release.
fn measured_depth(holder: &dyn Reflect, station: usize) -> Result<f64, Fault> {
    let found = measure::read(holder)
        .or_else(|| field_object(holder, "MeasuredDepth").and_then(measure::read))
        .ok_or_else(|| {
            Fault::new(
                CheckKind::Units,
                Finding::NoNumber {
                    station: Some(station),
                    field: "Md",
                },
                KIND,
            )
        })?;
    let unit = found.uom.ok_or_else(|| {
        Fault::new(
            CheckKind::Units,
            Finding::UnitMissing {
                station: Some(station),
                field: "Md",
            },
            KIND,
        )
    })?;
    units::metres(found.value, &unit).map_err(|why| unit_refused(Some(station), "Md", &unit, why))
}

/// An angle, in degrees - `None` when the station states none. A unit that is
/// not an angle is refused by `energyml_domain::units`, by name.
fn angle(station: &dyn Reflect, field: &'static str, number: usize) -> Result<Option<f64>, Fault> {
    let Some(holder) = field_object(station, field) else {
        return Ok(None);
    };
    let found = measure::read(holder).ok_or_else(|| {
        Fault::new(
            CheckKind::Units,
            Finding::NoNumber {
                station: Some(number),
                field,
            },
            KIND,
        )
    })?;
    let unit = found.uom.ok_or_else(|| {
        Fault::new(
            CheckKind::Units,
            Finding::UnitMissing {
                station: Some(number),
                field,
            },
            KIND,
        )
    })?;
    // Into degrees directly: `convert` leaves a value in `dega` untouched, so
    // 360 stays 360 and is not refused as 360.00000000000006.
    units::convert(found.value, &unit, "dega")
        .map(Some)
        .map_err(|why| unit_refused(Some(number), field, &unit, why))
}

/// A unit the domain refuses: the finding, and the domain's own error as the sentence.
pub(super) fn unit_refused(
    station: Option<usize>,
    field: &'static str,
    unit: &str,
    why: energyml_domain::Error,
) -> Fault {
    Fault::with_error(
        CheckKind::Units,
        Finding::UnitRefused {
            station,
            field,
            unit: unit.to_string(),
            reason: why.to_string(),
        },
        Error::Domain(why),
    )
}

fn to_degrees(value: f64, unit: &str) -> energyml_domain::Result<f64> {
    units::convert(value, unit, "dega")
}

fn to_radians_per_metre(value: f64, unit: &str) -> energyml_domain::Result<f64> {
    units::convert(value, unit, "rad/m")
}

/// A value the file states in the field `field` of `object`, converted by `to`: `None` when the field
/// is absent; `Err(UnitFault)` when its number or its unit cannot be read (the reading is never refused
/// on that account: nothing stated here is used to place).
pub(super) fn read_stated(
    object: &dyn Reflect,
    field: &'static str,
    to: impl Fn(f64, &str) -> energyml_domain::Result<f64>,
) -> Option<Result<f64, UnitFault>> {
    let holder = field_object(object, field)?;
    let Some(found) = measure::read(holder) else {
        return Some(Err(UnitFault { field, unit: None }));
    };
    let Some(unit) = found.uom else {
        return Some(Err(UnitFault { field, unit: None }));
    };
    Some(to(found.value, &unit).map_err(|_| UnitFault {
        field,
        unit: Some(unit),
    }))
}

/// A station's `Tvd` (an eml 2.3 `AbstractVerticalDepth`): the depth in metres, and what it counts from,
/// by the field present - `ReferencePoint` (a reference point) or `Datum` (a vertical CRS), never by the
/// name of the version.
// TODO(verify): that an `AbstractVerticalDepth` choice reads through `field_object` as its variant.
fn read_tvd(station: &dyn Reflect) -> Option<Result<StatedTvd, UnitFault>> {
    let tvd = field_object(station, "Tvd")?;
    let depth = match read_stated(tvd, "VerticalDepth", units::metres) {
        Some(Ok(depth)) => depth,
        Some(Err(fault)) => {
            return Some(Err(UnitFault {
                field: "Tvd",
                unit: fault.unit,
            }));
        }
        None => {
            return Some(Err(UnitFault {
                field: "Tvd",
                unit: None,
            }));
        }
    };
    let from = if tvd.field("ReferencePoint").is_some() {
        TvdReference::Point(reference_of(tvd, "ReferencePoint").unwrap_or_default())
    } else {
        TvdReference::VerticalCrs(reference_of(tvd, "Datum"))
    };
    Some(Ok(StatedTvd { metres: depth, from }))
}
