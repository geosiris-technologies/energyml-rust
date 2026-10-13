//! What the boundary knows of a well whose datum is written in a compound -
//! `read::wellhead_of` and the search for the vertical half
//!.
//!
//! `ReferencePointInALocalEngineeringCompoundCrs` (common TRG 2.3 §13.25
//! p. 371) holds **local** coordinates of the compound its `Crs` names, and
//! names no `VerticalCrs` of its own. Before 24/09 both readers here took it
//! at face value: the collar came back raw - the compound's origin, azimuth
//! and vertical sense all missing - and no vertical half was found, so
//! `well-from-polyline` refused to write a datum over a well of this shape.
//!
//! The system is built by hand, in an empty package; no array is read, so this
//! runs in every build.

use energyml_build::pieces::Source;
use energyml_eml_2_3 as eml;
use energyml_resqml_2_2 as resqml;

const PLANE: &str = "30000000-0000-4000-8000-000000000001";
const VERTICAL: &str = "30000000-0000-4000-8000-000000000002";
const COMPOUND: &str = "30000000-0000-4000-8000-000000000003";
const POINT: &str = "30000000-0000-4000-8000-000000000004";
const TRAJECTORY: &str = "30000000-0000-4000-8000-000000000005";

fn dor(uuid: &str, qualified_type: &str, title: &str) -> eml::DataObjectReference {
    eml::DataObjectReference {
        uuid: uuid.to_string(),
        qualified_type: qualified_type.to_string(),
        title: title.to_string(),
        ..Default::default()
    }
}

fn citation(title: &str) -> eml::Citation {
    eml::Citation {
        title: title.to_string(),
        originator: "test".to_string(),
        creation: "2026-09-24T00:00:00Z".to_string(),
        format: "test".to_string(),
        ..Default::default()
    }
}

/// A package holding a well anchored in a compound: Lambert-93 turned 30°
/// clockwise about (1000, 2000), depths from an origin 35 m above NGF-IGN69
/// (which counts up), and a collar 10 m along the local x axis, 5 m deep.
fn package() -> energyml_storage::EpcStore {
    let registry = std::sync::Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "datum.epc", registry).expect("an empty package");
    store
        .put(Box::new(eml::LocalEngineering2dCrs {
            uuid: PLANE.to_string(),
            schema_version: "2.3".to_string(),
            citation: citation("plane"),
            azimuth: eml::PlaneAngleMeasureExt {
                value: 30.0,
                uom: eml::PlaneAngleUomExt::Dega,
            },
            azimuth_reference: eml::NorthReferenceKind::GridNorth,
            origin_projected_coordinate1: 1000.0,
            origin_projected_coordinate2: 2000.0,
            horizontal_axes: eml::HorizontalAxes {
                direction1: eml::AxisDirectionKind::East,
                direction2: eml::AxisDirectionKind::North,
                uom: eml::LengthAndTimeUomExt::M,
                is_time: false,
            },
            origin_projected_crs: eml::ProjectedCrs {
                abstract_projected_crs: Some(eml::AbstractProjectedCrs::ProjectedEpsgCrs(
                    eml::ProjectedEpsgCrs { epsg_code: 2154 },
                )),
                ..Default::default()
            },
            ..Default::default()
        }))
        .expect("the plane is a part");
    store
        .put(Box::new(eml::VerticalCrs {
            uuid: VERTICAL.to_string(),
            schema_version: "2.3".to_string(),
            citation: citation("NGF-IGN69"),
            direction: eml::VerticalDirection::Up,
            uom: eml::LengthUomExt::M,
            abstract_vertical_crs: Some(eml::AbstractVerticalCrs::VerticalEpsgCrs(eml::VerticalEpsgCrs {
                epsg_code: 5720,
            })),
            ..Default::default()
        }))
        .expect("the datum is a part");
    store
        .put(Box::new(eml::LocalEngineeringCompoundCrs {
            uuid: COMPOUND.to_string(),
            schema_version: "2.3".to_string(),
            citation: citation("depths"),
            vertical_crs: dor(VERTICAL, "eml23.VerticalCrs", "NGF-IGN69"),
            origin_vertical_coordinate: 35.0,
            vertical_axis: eml::VerticalAxis {
                direction: eml::VerticalDirection::Down,
                uom: eml::LengthAndTimeUomExt::M,
                is_time: false,
            },
            local_engineering2d_crs: dor(PLANE, "eml23.LocalEngineering2dCrs", "plane"),
            ..Default::default()
        }))
        .expect("the compound is a part");
    store
        .put(Box::new(eml::ReferencePointInALocalEngineeringCompoundCrs {
            uuid: POINT.to_string(),
            schema_version: "2.3".to_string(),
            citation: citation("collar"),
            horizontal_coordinates: Some(eml::HorizontalCoordinates {
                coordinate1: 10.0,
                coordinate2: 0.0,
            }),
            vertical_coordinate: Some(5.0),
            crs: dor(COMPOUND, "eml23.LocalEngineeringCompoundCrs", "depths"),
            ..Default::default()
        }))
        .expect("the collar is a part");
    store
        .put(Box::new(resqml::WellboreTrajectoryRepresentation {
            uuid: TRAJECTORY.to_string(),
            schema_version: "2.2".to_string(),
            citation: citation("the well"),
            md_interval: eml::MdInterval {
                md_min: 0.0,
                md_max: 100.0,
                uom: eml::LengthUomExt::M,
                datum: Some(dor(
                    POINT,
                    "eml23.ReferencePointInALocalEngineeringCompoundCrs",
                    "collar",
                )),
                ..Default::default()
            },
            ..Default::default()
        }))
        .expect("the trajectory is a part");
    store
}

/// The collar is where the compound places it, and the vertical half is
/// found through the compound - both read off the trajectory, the way a slot
/// receives a well.
#[test]
fn a_well_anchored_in_a_compound_is_collared_where_the_compound_places_it() {
    let store = package();
    let known = energyml_build::read::pieces_of_object(&store, TRAJECTORY)
        .expect("the trajectory reads")
        .into_iter()
        .find_map(|(_, source)| match source {
            Source::Object(known) => Some(known),
            _ => None,
        })
        .expect("what is known");

    // Worked from the citations: x along the local axis turned 30° clockwise
    // from grid north, (cos 30°, −sin 30°); a local depth of 5 under an origin
    // 35 m above the datum, CRS.xsd l. 517-520.
    let (sin, cos) = 30f64.to_radians().sin_cos();
    let expected = [1000.0 + 10.0 * cos, 2000.0 - 10.0 * sin, 30.0];
    let head = known.wellhead.expect("the collar is stated");
    for axis in 0..3 {
        assert!(
            (head[axis] - expected[axis]).abs() < 1e-9,
            "collar {head:?}, expected {expected:?}"
        );
    }

    let vertical = known
        .vertical_crs
        .expect("the vertical half, through the compound");
    assert_eq!(vertical.uuid, VERTICAL);
    assert_eq!(known.vertical_epsg, Some(5720));
}

/// The datum itself, given to a slot: its vertical half is its compound's.
#[test]
fn a_point_in_a_compound_names_its_compounds_vertical_half() {
    let store = package();
    let known = energyml_build::read::known_of_object(&store, POINT).expect("what is known");
    let vertical = known.vertical_crs.expect("the vertical half, through `Crs`");
    assert_eq!(vertical.uuid, VERTICAL);
}
