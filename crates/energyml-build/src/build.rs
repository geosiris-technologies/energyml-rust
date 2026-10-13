//! From features and units to RESQML 2.2 objects.
//!
//! Three objects per group, and the chain between them is what makes the
//! package readable rather than a bag of points:
//!
//! ```text
//! RockVolumeFeature            « Marnes et Caillasses » — the rock body itself
//!   ↑ InterpretedFeature
//! StratigraphicUnitInterpretation   its ages, its role, one reading of it
//!   ↑ RepresentedObject
//! PointSetRepresentation       where boreholes met it
//! ```
//!
//! # Why a feature *and* an interpretation, when the file gives one thing
//!
//! Because RESQML separates *what exists* from *what somebody decided about
//! it*, and a SIGEOL export is squarely the second: `observationMethod` on every
//! feature reads « Dire d'expert ou jugement de l'ingénieur ». Collapsing the
//! pair — putting the ages on the feature — would state as a property of the
//! rock what is in fact one expert's reading, and the next import from another
//! source could not disagree with it without contradicting the geology.
//!
//! # What is not built here
//!
//! No store, no HDF5, no uuid drawn. This module takes the groups and the uuids
//! it is given and returns objects; writing them and putting the coordinates in
//! a file is [`crate::epc`]'s business. That is the rule the workspace follows
//! everywhere — `energyml_storage::graph`, `energyml_domain::provenance` — and
//! it is what lets these be tested without a package.

use energyml_eml_2_3 as eml;
use energyml_resqml_2_2 as resqml;

use crate::geojson::Position;
use crate::sigeol::{Side, Unit};

/// Milliseconds of the reference instant every geologic age is measured from.
///
/// EML 2.3 has no way to say « 41.2 Ma » on its own: a `GeologicTime` is a
/// `DateTime` **plus** an offset in years, positive into the past. So the pair
/// only means something against a stated instant, and this is it — the epoch,
/// written down rather than left to whoever reads the file.
pub const AGE_REFERENCE: &str = "1970-01-01T00:00:00Z";

/// One surface's worth of points, and what they are about.
#[derive(Debug, Clone)]
pub struct Group {
    /// The unit these points met.
    pub unit: Unit,
    /// Which of its surfaces.
    pub side: Side,
    /// The stage that surface lies in.
    pub stage: String,
    /// The positions, **already deduplicated** by the caller.
    pub points: Vec<Position>,
    /// How many source features fed this group, before deduplication.
    ///
    /// Carried so the activity can state the gap. A SIGEOL export writes one
    /// feature per borehole *pass*, not per place — 10 374 features for 1 291
    /// points on the Bartonien — and a package that showed only the second
    /// number would look like a much thinner survey than it is.
    pub features: usize,
}

/// What one group becomes.
#[derive(Debug, Clone)]
pub struct Built {
    /// The rock body.
    pub feature: resqml::RockVolumeFeature,
    /// One reading of it: ages, role.
    pub interpretation: resqml::StratigraphicUnitInterpretation,
    /// Where boreholes met it.
    pub representation: resqml::PointSetRepresentation,
}

/// Where a group's coordinates will live, once somebody writes them.
///
/// Named by the caller because it owns the file: this module states the shape
/// of the reference, not where a package keeps its numbers.
#[derive(Debug, Clone)]
pub struct Array {
    /// The external file's name, as the package will refer to it.
    pub file: String,
    /// The dataset's path inside it.
    ///
    /// **Opaque, and never parsed** — the schema says `PathInExternalFile` is a
    /// string and nothing more. `/resqml22/<uuid>/points` is what fesapi writes,
    /// a convention and not a rule.
    pub path: String,
    /// The MIME type of the external file.
    pub mime: String,
}

/// Build the three objects of one group.
///
/// `citation` makes a `Citation` from a title, so the originator, the format and
/// the creation instant are decided once by the caller — a clock is an input.
pub fn group(
    group: &Group,
    array: &Array,
    crs: &eml::DataObjectReference,
    citation: &dyn Fn(&str) -> eml::Citation,
) -> Built {
    let key = group.unit.key(group.side).0;
    let name = group.unit.name.clone().unwrap_or_else(|| key.clone());

    let feature = resqml::RockVolumeFeature {
        uuid: crate::naming::feature(&key),
        schema_version: "2.2".to_string(),
        citation: citation(&name),
        // The BRGM lithostratigraphic referential is a published vocabulary,
        // but `IsWellKnown` in RESQML means *this feature is one of the
        // standard's own well-known bodies* — which these are not. Saying
        // otherwise would claim an authority the standard has not granted.
        is_well_known: false,
        ..Default::default()
    };

    let interpretation = resqml::StratigraphicUnitInterpretation {
        uuid: crate::naming::interpretation(&key),
        schema_version: "2.2".to_string(),
        citation: citation(&name),
        // Depth: the third ordinate is an elevation, not a travel time. A
        // package that said `Time` here would place every point on an axis
        // nothing in the file describes.
        domain: Some(resqml::Domain::Depth),
        has_occurred_during: occurred_during(&group.unit),
        interpreted_feature: reference(
            &feature.uuid,
            "resqml22.RockVolumeFeature",
            &feature.citation.title,
        ),
        // Lithostratigraphic, which is what the export says of itself:
        // `geologicUnitType` is « unité lithostratigraphique » on every feature.
        stratigraphic_role: Some(resqml::StratigraphicRole::Lithostratigraphic),
        ..Default::default()
    };

    // What somebody reads in a list of forty objects: the unit alone would give
    // two objects the same name the day a second surface is imported.
    let title = format!("{name} — {} {}", group.stage, group.side.label());
    let representation = resqml::PointSetRepresentation {
        uuid: crate::naming::point_set(&key, &group.stage, group.side.label()),
        schema_version: "2.2".to_string(),
        citation: citation(&title),
        represented_object: Some(reference(
            &interpretation.uuid,
            "resqml22.StratigraphicUnitInterpretation",
            &interpretation.citation.title,
        )),
        node_patch_geometry: vec![resqml::AnyPointGeometry::PointGeometry(resqml::PointGeometry {
            local_crs: crs.clone(),
            points: Some(resqml::AbstractPoint3dArray::Point3dExternalArray(
                resqml::Point3dExternalArray {
                    coordinates: eml::ExternalDataArray {
                        external_data_array_part: vec![eml::ExternalDataArrayPart {
                            // Rows of three: the shape a `Point3d` array has,
                            // and what a reader will slice on.
                            count: vec![group.points.len() as i64, 3],
                            path_in_external_file: array.path.clone(),
                            start_index: vec![0, 0],
                            uri: array.file.clone(),
                            mime_type: Some(array.mime.clone()),
                        }],
                    },
                },
            )),
            ..Default::default()
        })],
        ..Default::default()
    };

    Built {
        feature,
        interpretation,
        representation,
    }
}

/// The unit's age span, as EML 2.3 can state it.
///
/// `None` when the file gave no numeric bounds: an interval with an invented
/// end would be a claim about geology that nothing in the document supports,
/// and `HasOccurredDuring` is optional precisely so it can be left unsaid.
fn occurred_during(unit: &Unit) -> Option<resqml::AbstractTimeInterval> {
    let (young, old) = (unit.young.ma?, unit.old.ma?);
    Some(resqml::AbstractTimeInterval::GeologicTimeBasedTimeInterval(
        resqml::GeologicTimeBasedTimeInterval {
            // **Start is the old end and end is the young one.** An interval
            // runs forwards in time; writing them the other way round would
            // describe a unit that stops before it begins, and no validator
            // would catch it — both are well-formed dates with well-formed
            // offsets.
            start: geologic_time(old),
            end: geologic_time(young),
        },
    ))
}

/// Millions of years, as a `DateTime` plus an offset in years.
///
/// The offset is **positive into the past**, which the schema states in so many
/// words. Rounded to the year because that is the unit the field carries; the
/// Ma value keeps its own precision in the source, which the package still
/// holds.
fn geologic_time(ma: f64) -> eml::GeologicTime {
    eml::GeologicTime {
        date_time: AGE_REFERENCE.to_string(),
        age_offset_attribute: Some((ma * 1_000_000.0).round() as i64),
    }
}

/// A reference to an object this crate builds.
fn reference(uuid: &str, qualified_type: &str, title: &str) -> eml::DataObjectReference {
    eml::DataObjectReference {
        uuid: uuid.to_string(),
        qualified_type: qualified_type.to_string(),
        title: title.to_string(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn citation(title: &str) -> eml::Citation {
        eml::Citation {
            title: title.to_string(),
            originator: "BRGM".to_string(),
            creation: "2026-09-08T00:00:00Z".to_string(),
            format: energyml_core::CITATION_FORMAT.to_string(),
            ..Default::default()
        }
    }

    fn unit() -> Unit {
        crate::sigeol::unit_of(&json!({
            "composition": [{ "material": { "StratiRockMaterial": { "lithostratigraphy": {
                "@href": "https://referentiel-lithostrat.brgm-rec.fr/#/lithostrat/search/details/67475488-34dc-464e-8268-2757a3e7b1c8",
                "@title": "Marnes et Caillasses"
            }}}}],
            "geologicHistory": [{
                "olderNamedAge": { "@title": "Bartonien" },
                "youngerNamedAge": { "@title": "Lutétien" },
                "numericAge": { "NumericAgeRange": {
                    "olderBoundDate": { "Quantity": { "value": 37.8 } },
                    "youngerBoundDate": { "Quantity": { "value": 47.8 } }
                }}
            }]
        }))
        .expect("a unit")
    }

    fn built() -> Built {
        let made = Group {
            unit: unit(),
            side: Side::Top,
            stage: "Bartonien".to_string(),
            points: vec![
                Position {
                    lon: 2.29,
                    lat: 48.87,
                    z: Some(58.0),
                },
                Position {
                    lon: 2.40,
                    lat: 48.90,
                    z: Some(50.0),
                },
            ],
            features: 10_354,
        };
        let array = Array {
            file: "import.h5".to_string(),
            path: "/resqml22/x/points".to_string(),
            mime: "application/x-hdf5".to_string(),
        };
        let crs = reference("crs-uuid", "eml23.GeographicCompoundCrs", "EPSG:4326");
        group(&made, &array, &crs, &citation)
    }

    #[test]
    fn the_chain_holds_from_the_points_up_to_the_rock() {
        // What makes the package readable rather than a bag of points. Each
        // link is a uuid, and a broken one would still validate.
        let made = built();
        assert_eq!(made.interpretation.interpreted_feature.uuid, made.feature.uuid);
        assert_eq!(
            made.representation.represented_object.as_ref().unwrap().uuid,
            made.interpretation.uuid
        );
        assert_eq!(
            made.interpretation.interpreted_feature.qualified_type,
            "resqml22.RockVolumeFeature"
        );
    }

    #[test]
    fn an_interval_runs_forwards_and_the_offsets_are_positive_into_the_past() {
        // The trap: `start` is the **old** end. Written the other way round it
        // describes a unit that stops before it begins, and nothing complains —
        // both are well-formed dates with well-formed offsets.
        let made = built();
        let Some(resqml::AbstractTimeInterval::GeologicTimeBasedTimeInterval(interval)) =
            &made.interpretation.has_occurred_during
        else {
            panic!("expected a geologic interval")
        };
        assert_eq!(interval.start.age_offset_attribute, Some(47_800_000));
        assert_eq!(interval.end.age_offset_attribute, Some(37_800_000));
        assert!(interval.start.age_offset_attribute > interval.end.age_offset_attribute);
        // And both name the instant they are measured from, or the pair means
        // nothing.
        assert_eq!(interval.start.date_time, AGE_REFERENCE);
    }

    #[test]
    fn a_unit_with_no_numeric_age_states_no_interval_rather_than_an_invented_one() {
        let mut bare = unit();
        bare.young.ma = None;
        let made = Group {
            unit: bare,
            side: Side::Top,
            stage: "Bartonien".to_string(),
            points: vec![Position {
                lon: 2.29,
                lat: 48.87,
                z: None,
            }],
            features: 1,
        };
        let array = Array {
            file: "x.h5".into(),
            path: "/p".into(),
            mime: "application/x-hdf5".into(),
        };
        let crs = reference("c", "eml23.GeographicCompoundCrs", "c");
        let out = group(&made, &array, &crs, &citation);
        assert!(out.interpretation.has_occurred_during.is_none());
    }

    #[test]
    fn the_array_is_declared_as_rows_of_three_and_the_path_is_left_alone() {
        let made = built();
        let resqml::AnyPointGeometry::PointGeometry(geometry) = &made.representation.node_patch_geometry[0]
        else {
            panic!("expected a point geometry")
        };
        let Some(resqml::AbstractPoint3dArray::Point3dExternalArray(array)) = &geometry.points else {
            panic!("expected an external array")
        };
        let part = &array.coordinates.external_data_array_part[0];
        assert_eq!(part.count, vec![2, 3]);
        assert_eq!(part.start_index, vec![0, 0]);
        // Opaque: the schema says a string, and `/resqml22/<uuid>/points` is
        // fesapi's convention rather than a rule.
        assert_eq!(part.path_in_external_file, "/resqml22/x/points");
        assert_eq!(part.mime_type.as_deref(), Some("application/x-hdf5"));
        assert_eq!(geometry.local_crs.uuid, "crs-uuid");
    }

    #[test]
    fn the_role_and_the_domain_say_what_the_export_says_of_itself() {
        let made = built();
        assert_eq!(
            made.interpretation.stratigraphic_role,
            Some(resqml::StratigraphicRole::Lithostratigraphic)
        );
        // Depth and not Time: an elevation is not a travel time, and a package
        // that said otherwise would place every point on an axis nothing in the
        // file describes.
        assert_eq!(made.interpretation.domain, Some(resqml::Domain::Depth));
        // And the rock body does not claim to be one of the standard's own.
        assert!(!made.feature.is_well_known);
    }

    #[test]
    fn the_title_says_the_unit_the_stage_and_the_side() {
        // What somebody reads in a list of forty objects. The unit alone would
        // give two objects the same name the day a second surface is imported.
        let made = built();
        assert_eq!(
            made.representation.citation.title,
            "Marnes et Caillasses — Bartonien Top"
        );
        assert_eq!(made.feature.citation.title, "Marnes et Caillasses");
    }

    #[test]
    fn a_second_run_names_the_same_objects() {
        let a = built();
        let b = built();
        assert_eq!(a.feature.uuid, b.feature.uuid);
        assert_eq!(a.representation.uuid, b.representation.uuid);
        // And the three are distinct.
        assert_ne!(a.feature.uuid, a.interpretation.uuid);
        assert_ne!(a.interpretation.uuid, a.representation.uuid);
    }
}
