//! A well anchored by a point written in a local engineering compound CRS.
//!
//! `ReferencePointInALocalEngineeringCompoundCrs` — common TRG 2.3 §13.25
//! p. 371: *"A reference point which is defined in the context of a compound
//! (2D horizontal + 1D vertical) CRS"*, its `Crs` going *"To:
//! LocalEngineeringCompoundCrs"*. The form an external translation tool writes for a
//! translated `obj_MdDatum`, and the one chosen for a datum under
//! a compound CRS.
//!
//! Its coordinates are **local** ones of that compound, so the collar is where
//! the compound places them: through its origin, its azimuth, and a vertical
//! axis that may count down from a datum that counts up. The system here is
//! built by hand to exercise all three at once — the corpus has none of them.

use energyml_core::registry::DynObject;
use energyml_domain::source::ObjectSource;
use energyml_eml_2_3 as eml23;
use energyml_geometry::{Topology, geometry};
use energyml_resqml_2_2::WellboreTrajectoryRepresentation;

const PLANE: &str = "20000000-0000-4000-8000-000000000001";
const DATUM: &str = "20000000-0000-4000-8000-000000000002";
const COMPOUND: &str = "20000000-0000-4000-8000-000000000003";
const POINT: &str = "20000000-0000-4000-8000-000000000004";
const TRAJECTORY: &str = "20000000-0000-4000-8000-000000000005";

fn dor(uuid: &str, qualified_type: &str) -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: uuid.into(),
        qualified_type: qualified_type.into(),
        ..Default::default()
    }
}

/// Lambert-93, origin (1000, 2000), turned 30° clockwise from grid north.
fn plane() -> eml23::LocalEngineering2dCrs {
    eml23::LocalEngineering2dCrs {
        uuid: PLANE.into(),
        azimuth: eml23::PlaneAngleMeasureExt { value: 30.0, uom: eml23::PlaneAngleUomExt::Dega },
        azimuth_reference: eml23::NorthReferenceKind::GridNorth,
        origin_projected_coordinate1: 1000.0,
        origin_projected_coordinate2: 2000.0,
        horizontal_axes: eml23::HorizontalAxes {
            direction1: eml23::AxisDirectionKind::East,
            direction2: eml23::AxisDirectionKind::North,
            uom: eml23::LengthAndTimeUomExt::M,
            is_time: false,
        },
        origin_projected_crs: eml23::ProjectedCrs {
            abstract_projected_crs: Some(eml23::AbstractProjectedCrs::ProjectedEpsgCrs(eml23::ProjectedEpsgCrs {
                epsg_code: 2154,
            })),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// NGF-IGN69, counting up.
fn datum() -> eml23::VerticalCrs {
    eml23::VerticalCrs {
        uuid: DATUM.into(),
        direction: eml23::VerticalDirection::Up,
        uom: eml23::LengthUomExt::M,
        abstract_vertical_crs: Some(eml23::AbstractVerticalCrs::VerticalEpsgCrs(eml23::VerticalEpsgCrs {
            epsg_code: 5720,
        })),
        ..Default::default()
    }
}

/// Depths, from an origin 35 m above the NGF zero; `is_time` as asked.
fn compound(is_time: bool) -> eml23::LocalEngineeringCompoundCrs {
    eml23::LocalEngineeringCompoundCrs {
        uuid: COMPOUND.into(),
        vertical_crs: dor(DATUM, "eml23.VerticalCrs"),
        origin_vertical_coordinate: 35.0,
        vertical_axis: eml23::VerticalAxis {
            direction: eml23::VerticalDirection::Down,
            uom: if is_time { eml23::LengthAndTimeUomExt::Ms } else { eml23::LengthAndTimeUomExt::M },
            is_time,
        },
        local_engineering2d_crs: dor(PLANE, "eml23.LocalEngineering2dCrs"),
        ..Default::default()
    }
}

/// The collar: 10 m along the local x axis, 5 m deep.
fn point() -> eml23::ReferencePointInALocalEngineeringCompoundCrs {
    eml23::ReferencePointInALocalEngineeringCompoundCrs {
        uuid: POINT.into(),
        horizontal_coordinates: Some(eml23::HorizontalCoordinates { coordinate1: 10.0, coordinate2: 0.0 }),
        vertical_coordinate: Some(5.0),
        crs: dor(COMPOUND, "eml23.LocalEngineeringCompoundCrs"),
        ..Default::default()
    }
}

/// Drilled from MD 0 to MD 100, with nothing but the datum to place it.
fn trajectory() -> WellboreTrajectoryRepresentation {
    WellboreTrajectoryRepresentation {
        uuid: TRAJECTORY.into(),
        md_interval: eml23::MdInterval {
            md_min: 0.0,
            md_max: 100.0,
            uom: eml23::LengthUomExt::M,
            datum: Some(dor(POINT, "eml23.ReferencePointInALocalEngineeringCompoundCrs")),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// A store of exactly the objects a well needs; `time` makes the compound a
/// time system.
struct Wellsite {
    time: bool,
}

impl ObjectSource for Wellsite {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        match uuid {
            PLANE => Some(Box::new(plane())),
            DATUM => Some(Box::new(datum())),
            COMPOUND => Some(Box::new(compound(self.time))),
            POINT => Some(Box::new(point())),
            TRAJECTORY => Some(Box::new(trajectory())),
            _ => None,
        }
    }
}

impl energyml_domain::source::ArraySource for Wellsite {
    fn locate(&self, _object: &dyn energyml_core::value::Reflect) -> Vec<energyml_storage::ArrayLocation> {
        Vec::new()
    }
    fn metadata(
        &self,
        location: &energyml_storage::ArrayLocation,
    ) -> energyml_domain::Result<energyml_storage::ArrayMetadata> {
        Err(energyml_domain::Error::Missing(location.field_path.clone()))
    }
    fn read(&self, location: &energyml_storage::ArrayLocation) -> energyml_domain::Result<energyml_storage::ArrayData> {
        Err(energyml_domain::Error::Missing(location.field_path.clone()))
    }
    fn read_into(
        &self,
        location: &energyml_storage::ArrayLocation,
        _out: &mut energyml_storage::array::ArrayBuffer<'_>,
    ) -> energyml_domain::Result<energyml_storage::ArrayMetadata> {
        Err(energyml_domain::Error::Missing(location.field_path.clone()))
    }
}

/// Where the collar is, worked from the citations rather than read from the
/// code: x along the local axis turned 30° clockwise, (cos 30°, −sin 30°);
/// the local depth 5 under an origin 35 m above the datum, 35 − 5.
fn collar() -> [f64; 3] {
    let (sin, cos) = 30f64.to_radians().sin_cos();
    [1000.0 + 10.0 * cos, 2000.0 - 10.0 * sin, 30.0]
}

fn close(found: [f64; 3], expected: [f64; 3]) -> bool {
    (0..3).all(|axis| (found[axis] - expected[axis]).abs() < 1e-9)
}

/// The well stands at the placed collar and runs down from it.
///
/// Refused before 24/09 — the datum type was not one this reader knew — and
/// placed raw it would stand at (10, 0), 5 m *up*: the compound's origin, its
/// azimuth and its vertical sense all missing, each number reasonable.
#[test]
fn a_well_anchored_in_a_compound_stands_where_the_compound_places_its_collar() {
    let site = Wellsite { time: false };
    let found = geometry::of(&trajectory(), &site).expect("the well is placed");
    let patch = &found.patches[0];
    assert!(matches!(&patch.topology, Topology::Polylines(runs) if runs == &vec![2]));
    let [x, y, z] = collar();
    assert!(close(patch.points[0], [x, y, z]), "top at {:?}, expected {:?}", patch.points[0], [x, y, z]);
    assert!(close(patch.points[1], [x, y, z - 100.0]), "bottom at {:?}", patch.points[1]);

    // The same point through the public reading every tabulation uses.
    let head = energyml_geometry::well::head_of(&trajectory(), &site).expect("a collar");
    assert!(close(head, collar()), "{head:?}");
}

/// **Placed once.** The CRS the well is given carries the compound's EPSG
/// identities and none of its placement, so the placement layer leaves the
/// points where the well reader put them — the two readers composed, the way
/// every export composes them. With the compound's offsets there, the collar
/// would move by (1000, 2000) a second time.
#[test]
fn the_crs_of_such_a_well_does_not_place_it_a_second_time() {
    let site = Wellsite { time: false };
    let found = geometry::of(&trajectory(), &site).expect("the well is placed");
    let crs = energyml_domain::crs::of(&trajectory(), &site).expect("no error").expect("a CRS through the datum");
    assert_eq!(crs.projected.epsg(), Some(2154));
    assert_eq!(crs.vertical.epsg(), Some(5720));
    for point in &found.patches[0].points {
        assert_eq!(crs.to_projected(*point), *point);
    }
}

/// A measured depth is a length; a collar in a time system has none to count
/// it from, and the well is refused rather than drawn in milliseconds.
#[test]
fn a_well_anchored_in_a_time_compound_is_refused() {
    let site = Wellsite { time: true };
    let error = geometry::of(&trajectory(), &site).expect_err("a time datum");
    assert!(error.to_string().contains("time"), "{error}");
}
