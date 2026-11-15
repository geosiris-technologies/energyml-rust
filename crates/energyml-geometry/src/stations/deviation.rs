//! A RESQML 2.0.1 `DeviationSurveyRepresentation`, read into the same stations as a WITSML trajectory.
//!
//! RESQML TRG 2.0.1 §7.5.7 p. 139-140: one measured depth, one azimuth and one inclination per
//! station, in three arrays of `StationCount` values; the azimuths are relative to the projected CRS
//! north (grid north) and the inclinations are measured from the vertical. TUG 2.0.1 §12.2.3.3 p. 204.

use energyml_core::measure;
use energyml_core::value::{FieldValue, Reflect};
use energyml_domain::units;

use super::checks::{CheckKind, Fault, Finding};
use super::witsml::unit_refused;
use super::{
    DatumFacts, Facts, MethodFacts, NorthCorrections, NorthReference, Reading, StatedSection, StatedStation,
    StationCorrections, SurveyKind,
};
use crate::error::Error;
use crate::geometry::field_object;
use crate::min_curvature::Measured;
use crate::source::ValueSource;
use crate::values::{read_doubles, scalar_f64, scalar_i64};
use crate::well::reference_of;

const KIND: SurveyKind = SurveyKind::DeviationSurvey;
const AT: &str = "DeviationSurveyRepresentation";

/// A `DeviationSurveyRepresentation`: the facts, and the measured values apart.
pub(super) fn read(survey: &dyn Reflect, source: &dyn ValueSource) -> Reading {
    let count = match scalar_i64(survey, "StationCount") {
        Some(count) if count > 0 => count as usize,
        _ => return Err(Fault::new(CheckKind::Stations, Finding::NoStation, KIND)),
    };

    // -- the three arrays, one value per station (RESQML TRG 2.0.1 §7.5.7 p. 139) ------
    let mut arrays: Vec<Vec<f64>> = Vec::with_capacity(3);
    for field in ["Mds", "Inclinations", "Azimuths"] {
        let at = format!("{AT}.{field}");
        let Some(array) = field_object(survey, field) else {
            return Err(Fault::with_error(
                CheckKind::Stations,
                Finding::ArrayUnreadable {
                    field,
                    reason: "no array".into(),
                },
                Error::malformed(
                    &at,
                    "no array: a deviation survey states one value per station",
                ),
            ));
        };
        let values = match read_doubles(array, source, &at) {
            Ok(values) => values,
            Err(error) => {
                return Err(Fault::with_error(
                    CheckKind::Stations,
                    Finding::ArrayUnreadable {
                        field,
                        reason: error.to_string(),
                    },
                    error,
                ));
            }
        };
        if values.len() != count {
            let found = values.len();
            return Err(Fault::with_error(
                CheckKind::Stations,
                Finding::ArrayLength {
                    field,
                    found,
                    stations: count,
                },
                Error::malformed(
                    &at,
                    format!(
                        "{found} value(s) for {count} station(s) - RESQML TRG 2.0.1 §7.5.7 p. 139 asks one per station"
                    ),
                ),
            ));
        }
        arrays.push(values);
    }
    let (mds, inclinations, azimuths) = (&arrays[0], &arrays[1], &arrays[2]);

    // -- the units: a failure is the check `Units`, never a refusal of the facts ------
    let measured = measured_of(survey, mds, inclinations, azimuths);

    let facts = Facts {
        kind: KIND,
        count,
        rejected: Vec::new(),
        datums: DatumFacts {
            default: reference_of(survey, "MdDatum"),
            per_station: vec![None; count],
            interval: None,
        },
        methods: MethodFacts::default(),
        north: Some(NorthReference::Grid),
        corrections: NorthCorrections {
            per_station: vec![StationCorrections::default(); count],
            ..Default::default()
        },
        stated: (1..=count)
            .map(|station| StatedStation {
                station,
                ..Default::default()
            })
            .collect(),
        section: StatedSection::default(),
        first_station_location: first_station_location(survey),
    };
    Ok((facts, measured))
}

/// The stations in metres and degrees, or the unit that cannot be read.
fn measured_of(
    survey: &dyn Reflect,
    mds: &[f64],
    inclinations: &[f64],
    azimuths: &[f64],
) -> Result<Vec<Measured>, Fault> {
    let unit_of = |field: &'static str| -> Result<String, Fault> {
        measure::enum_value(survey, field).ok_or_else(|| {
            Fault::new(
                CheckKind::Units,
                Finding::UnitMissing { station: None, field },
                KIND,
            )
        })
    };
    let md_unit = unit_of("MdUom")?;
    let angle_unit = unit_of("AngleUom")?;
    (0..mds.len())
        .map(|k| -> Result<Measured, Fault> {
            Ok(Measured {
                md: units::metres(mds[k], &md_unit).map_err(|why| unit_refused(None, "MdUom", &md_unit, why))?,
                // A `NaN` in an array stays a `NaN`: `checked_directions` refuses it, nothing fills it in.
                inclination: Some(
                    units::convert(inclinations[k], &angle_unit, "dega")
                        .map_err(|why| unit_refused(None, "AngleUom", &angle_unit, why))?,
                ),
                azimuth: Some(
                    units::convert(azimuths[k], &angle_unit, "dega")
                        .map_err(|why| unit_refused(None, "AngleUom", &angle_unit, why))?,
                ),
            })
        })
        .collect()
}

/// `FirstStationLocation`, as written: shown, never used to place - its CRS is not stated. A missing
/// coordinate gives `None`.
fn first_station_location(survey: &dyn Reflect) -> Option<[f64; 3]> {
    let point = match survey.field("FirstStationLocation")? {
        FieldValue::Object(point) => point,
        _ => return None,
    };
    Some([
        scalar_f64(point, "Coordinate1")?,
        scalar_f64(point, "Coordinate2")?,
        scalar_f64(point, "Coordinate3")?,
    ])
}
