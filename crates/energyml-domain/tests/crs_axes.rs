//! The order of the horizontal axes of a CRS, as the file states it (WP04 F1).
//!
//! Every system is built by hand from the documents. The citations each test rests on:
//!
//! - RESQML TRG 2.0.1 §2.1 p. 10: `ProjectedAxisOrder` *"Defines the coordinate system axis order of
//!   the global projected CRS"*;
//! - common TRG 2.3 §12.7 p. 309: `AxisOrder2d`, carried by `ProjectedCrs.AxisOrder`;
//! - common TRG 2.3 §12.18 p. 320: `HorizontalAxes.Direction1/Direction2`; §12.19 p. 321, the local
//!   engineering 2d CRS.

use energyml_core::registry::DynObject;
use energyml_domain::crs::{self, AxisOrder, AxisReading, Handedness};
use energyml_domain::source::ObjectSource;
use energyml_eml_2_0 as eml20;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_0_1 as resqml20;

const PLANE: &str = "10000000-0000-4000-8000-000000000001";
const DATUM: &str = "10000000-0000-4000-8000-000000000002";
const COMPOUND: &str = "10000000-0000-4000-8000-000000000003";

/// Every object of a test, found by its uuid.
struct Store(Vec<Box<dyn DynObject>>);

impl ObjectSource for Store {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        self.0
            .iter()
            .find(|object| energyml_core::identity::uuid_of_reflect(object.as_reflect()) == Some(uuid))
            .map(|object| object.clone_box())
    }
}

fn dor(uuid: &str, qualified_type: &str) -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: uuid.into(),
        qualified_type: qualified_type.into(),
        ..Default::default()
    }
}

fn plane(
    direction1: eml23::AxisDirectionKind,
    direction2: eml23::AxisDirectionKind,
    projected_order: eml23::AxisOrder2d,
) -> eml23::LocalEngineering2dCrs {
    eml23::LocalEngineering2dCrs {
        uuid: PLANE.into(),
        azimuth: eml23::PlaneAngleMeasureExt {
            value: 0.0,
            uom: eml23::PlaneAngleUomExt::Dega,
        },
        azimuth_reference: eml23::NorthReferenceKind::GridNorth,
        origin_projected_coordinate1: 0.0,
        origin_projected_coordinate2: 0.0,
        horizontal_axes: eml23::HorizontalAxes {
            direction1,
            direction2,
            uom: eml23::LengthAndTimeUomExt::M,
            is_time: false,
        },
        origin_projected_crs: eml23::ProjectedCrs {
            abstract_projected_crs: Some(eml23::AbstractProjectedCrs::ProjectedEpsgCrs(
                eml23::ProjectedEpsgCrs { epsg_code: 2154 },
            )),
            axis_order: projected_order,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn datum() -> eml23::VerticalCrs {
    eml23::VerticalCrs {
        uuid: DATUM.into(),
        direction: eml23::VerticalDirection::Up,
        uom: eml23::LengthUomExt::M,
        abstract_vertical_crs: Some(eml23::AbstractVerticalCrs::VerticalEpsgCrs(
            eml23::VerticalEpsgCrs { epsg_code: 5720 },
        )),
        ..Default::default()
    }
}

/// A compound and its two halves, in one store.
fn system(horizontal: eml23::LocalEngineering2dCrs) -> (eml23::LocalEngineeringCompoundCrs, Store) {
    let system = eml23::LocalEngineeringCompoundCrs {
        uuid: COMPOUND.into(),
        vertical_crs: dor(DATUM, "eml23.VerticalCrs"),
        origin_vertical_coordinate: 0.0,
        vertical_axis: eml23::VerticalAxis {
            direction: eml23::VerticalDirection::Up,
            uom: eml23::LengthAndTimeUomExt::M,
            is_time: false,
        },
        local_engineering2d_crs: dor(PLANE, "eml23.LocalEngineering2dCrs"),
        ..Default::default()
    };
    let store = Store(vec![
        Box::new(horizontal) as Box<dyn DynObject>,
        Box::new(datum()) as Box<dyn DynObject>,
        Box::new(system.clone()) as Box<dyn DynObject>,
    ]);
    (system, store)
}

fn depth_crs_2_0_1(order: eml20::AxisOrder2d) -> resqml20::LocalDepth3dCrs {
    resqml20::LocalDepth3dCrs {
        uuid: "bbbbbbbb-0000-4000-8000-000000000001".into(),
        schema_version: "2.0".into(),
        projected_axis_order: order,
        projected_crs: Some(eml20::AbstractProjectedCrs::ProjectedCrsEpsgCode(
            eml20::ProjectedCrsEpsgCode { epsg_code: 2154 },
        )),
        ..Default::default()
    }
}

#[test]
fn a_2_0_1_crs_states_the_axis_order_of_its_projected_system() {
    // RESQML TRG 2.0.1 §2.1 p. 10.
    let northing_easting = crs::read(&depth_crs_2_0_1(eml20::AxisOrder2d::NorthingEasting)).expect("reads");
    assert_eq!(
        northing_easting.axes,
        AxisReading::Stated(AxisOrder {
            first_degrees: 0.0,
            second_degrees: 90.0
        })
    );
    assert_eq!(
        northing_easting.axes.stated().unwrap().handedness(),
        Handedness::Left
    );

    let easting_northing = crs::read(&depth_crs_2_0_1(eml20::AxisOrder2d::EastingNorthing)).expect("reads");
    assert_eq!(
        easting_northing.axes.stated().unwrap().handedness(),
        Handedness::Right
    );
}

#[test]
fn a_standalone_projected_crs_states_its_own_axis_order() {
    // Common TRG 2.3 §12.7 p. 309.
    let projected = eml23::ProjectedCrs {
        axis_order: eml23::AxisOrder2d::WestingSouthing,
        abstract_projected_crs: Some(eml23::AbstractProjectedCrs::ProjectedEpsgCrs(
            eml23::ProjectedEpsgCrs { epsg_code: 2154 },
        )),
        ..Default::default()
    };
    let read = crs::read(&projected).expect("reads");
    assert_eq!(
        read.axes,
        AxisReading::Stated(AxisOrder {
            first_degrees: 270.0,
            second_degrees: 180.0
        })
    );
    assert_eq!(read.axes.stated().unwrap().handedness(), Handedness::Right);
}

#[test]
fn an_eml_2_3_compound_takes_the_projected_order_and_checks_the_local_axes() {
    // Common TRG 2.3 §12.18 p. 320, §12.19 p. 321.
    let (system, store) = system(plane(
        eml23::AxisDirectionKind::East,
        eml23::AxisDirectionKind::North,
        eml23::AxisOrder2d::EastingNorthing,
    ));
    let read = crs::read_with(&system, &store).expect("the compound reads");
    assert_eq!(
        read.axes,
        AxisReading::Stated(AxisOrder {
            first_degrees: 90.0,
            second_degrees: 0.0
        })
    );
}

#[test]
fn an_eml_2_3_compound_whose_two_statements_disagree_is_contradictory() {
    // Same page: the projected order and the local axes disagree on the hand, and the read still
    // succeeds (the points are still placed).
    let (system, store) = system(plane(
        eml23::AxisDirectionKind::North,
        eml23::AxisDirectionKind::East,
        eml23::AxisOrder2d::EastingNorthing,
    ));
    let read = crs::read_with(&system, &store).expect("the compound still reads");
    assert_eq!(
        read.axes,
        AxisReading::Contradictory {
            projected: AxisOrder {
                first_degrees: 90.0,
                second_degrees: 0.0
            },
            local: AxisOrder {
                first_degrees: 0.0,
                second_degrees: 90.0
            },
        }
    );
    assert_eq!(
        read.to_projected([1.0, 2.0, 0.0])[0],
        1.0,
        "points are placed all the same"
    );
}
