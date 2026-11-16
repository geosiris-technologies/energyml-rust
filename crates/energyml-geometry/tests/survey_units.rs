//! A WITSML survey placed in the units of the CRS its datum gives it - `src/stations/place.rs`.
//!
//! **The rule** (one, from the design): the points a survey gives are in the frame and the units of the
//! CRS `energyml_domain::crs::of` gives the same object - the CRS the scene applies to them - so
//! `crs.to_metres(points[k])` is the station placed, in metres.
//!
//! **The form of the assertions comes from the standards**, cited where used: the depths of a trajectory
//! count from the datum `DefaultMdDatum` (WITSML TRG 2.1 §16.6 p. 350), a point in a projected and a
//! vertical CRS is written in the units of those two systems, and 1 ft is 0.3048 m exactly. The points in
//! metres are read through the conversion of the scene (`SceneObject::points_in`), never through a
//! conversion written here.
//!
//! **The numbers are worked by hand**: the hole is vertical to MD 100, then one arc of 90 degrees over
//! 900 m of hole, a circle of radius R = 572.957795 m, toward the east.

use std::collections::HashMap;

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::source::{ArraySource, ObjectSource};
use energyml_eml_2_3 as eml;
use energyml_geometry::stations::{self, SurveyDeclarations};
use energyml_geometry::{Placement, geometry, scene};
use energyml_witsml_2_1 as witsml;

const PLANE: &str = "41000000-0000-4000-8000-000000000001";
const VERTICAL: &str = "41000000-0000-4000-8000-000000000002";
const COMPOUND: &str = "41000000-0000-4000-8000-000000000003";
const COLLAR: &str = "41000000-0000-4000-8000-000000000004";
const TRAJECTORY: &str = "41000000-0000-4000-8000-000000000006";
const PROJECTED: &str = "41000000-0000-4000-8000-000000000007";
const POINT: &str = "41000000-0000-4000-8000-000000000008";
const RESQML: &str = "41000000-0000-4000-8000-000000000009";

/// The radius of 90 degrees over 900 m, by hand: 180/pi x 10.
const R: f64 = 572.957_795_130_823_2;

const FOOT: f64 = 0.3048;

fn dor(uuid: &str, qualified_type: &str) -> eml::DataObjectReference {
    eml::DataObjectReference {
        uuid: uuid.into(),
        qualified_type: qualified_type.into(),
        ..Default::default()
    }
}

/// Objects by uuid, handed out as a store would.
struct Site(HashMap<String, Box<dyn DynObject>>);

impl Site {
    fn with(mut self, uuid: &str, object: Box<dyn DynObject>) -> Self {
        self.0.insert(uuid.to_string(), object);
        self
    }
}

impl ObjectSource for Site {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        self.0.get(uuid).map(|object| object.clone_box())
    }
}

impl ArraySource for Site {
    fn locate(&self, _object: &dyn Reflect) -> Vec<energyml_storage::ArrayLocation> {
        Vec::new()
    }
    fn metadata(
        &self,
        location: &energyml_storage::ArrayLocation,
    ) -> energyml_domain::Result<energyml_storage::ArrayMetadata> {
        Err(energyml_domain::Error::Missing(location.field_path.clone()))
    }
    fn read(
        &self,
        location: &energyml_storage::ArrayLocation,
    ) -> energyml_domain::Result<energyml_storage::ArrayData> {
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

/// Lambert-93, origin (1000, 2000), turned 30 degrees clockwise, its horizontal axes in `unit`; a depth
/// axis from 35 m above NGF-IGN69; the collar 10 along x, 5 m deep.
fn compound_site(unit: eml::LengthAndTimeUomExt) -> Site {
    let plane = eml::LocalEngineering2dCrs {
        uuid: PLANE.into(),
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
            uom: unit,
            is_time: false,
        },
        origin_projected_crs: eml::ProjectedCrs {
            abstract_projected_crs: Some(eml::AbstractProjectedCrs::ProjectedEpsgCrs(
                eml::ProjectedEpsgCrs { epsg_code: 2154 },
            )),
            ..Default::default()
        },
        ..Default::default()
    };
    let vertical = eml::VerticalCrs {
        uuid: VERTICAL.into(),
        direction: eml::VerticalDirection::Up,
        uom: eml::LengthUomExt::M,
        abstract_vertical_crs: Some(eml::AbstractVerticalCrs::VerticalEpsgCrs(eml::VerticalEpsgCrs {
            epsg_code: 5720,
        })),
        ..Default::default()
    };
    let compound = eml::LocalEngineeringCompoundCrs {
        uuid: COMPOUND.into(),
        vertical_crs: dor(VERTICAL, "eml23.VerticalCrs"),
        origin_vertical_coordinate: 35.0,
        vertical_axis: eml::VerticalAxis {
            direction: eml::VerticalDirection::Down,
            uom: eml::LengthAndTimeUomExt::M,
            is_time: false,
        },
        local_engineering2d_crs: dor(PLANE, "eml23.LocalEngineering2dCrs"),
        ..Default::default()
    };
    let collar = eml::ReferencePointInALocalEngineeringCompoundCrs {
        uuid: COLLAR.into(),
        horizontal_coordinates: Some(eml::HorizontalCoordinates {
            coordinate1: 10.0,
            coordinate2: 0.0,
        }),
        vertical_coordinate: Some(5.0),
        crs: dor(COMPOUND, "eml23.LocalEngineeringCompoundCrs"),
        ..Default::default()
    };
    Site(HashMap::new())
        .with(PLANE, Box::new(plane))
        .with(VERTICAL, Box::new(vertical))
        .with(COMPOUND, Box::new(compound))
        .with(COLLAR, Box::new(collar))
}

/// A `ReferencePointInACrs` at (500 000, 6 000 000), `elevation` up an upward vertical CRS written in
/// `vertical_unit` (no EPSG code, so its `Direction` is read), in a Lambert-93 whose order and unit are as given.
fn projected_site(
    order: eml::AxisOrder2d,
    uom: Option<eml::LengthUomExt>,
    vertical_unit: eml::LengthUomExt,
    elevation: f64,
) -> Site {
    let projected = eml::ProjectedCrs {
        uuid: PROJECTED.into(),
        axis_order: order,
        uom,
        abstract_projected_crs: Some(eml::AbstractProjectedCrs::ProjectedEpsgCrs(
            eml::ProjectedEpsgCrs { epsg_code: 2154 },
        )),
        ..Default::default()
    };
    let vertical = eml::VerticalCrs {
        uuid: VERTICAL.into(),
        direction: eml::VerticalDirection::Up,
        uom: vertical_unit,
        abstract_vertical_crs: None,
        ..Default::default()
    };
    let point = eml::ReferencePointInACrs {
        uuid: POINT.into(),
        horizontal_coordinates: Some(eml::AbstractHorizontalCoordinates::ProjectedCoordinates(
            eml::ProjectedCoordinates {
                coordinate1: 500_000.0,
                coordinate2: 6_000_000.0,
                crs: dor(PROJECTED, "eml23.ProjectedCrs"),
            },
        )),
        vertical_coordinate: Some(elevation),
        vertical_crs: Some(dor(VERTICAL, "eml23.VerticalCrs")),
        ..Default::default()
    };
    Site(HashMap::new())
        .with(PROJECTED, Box::new(projected))
        .with(VERTICAL, Box::new(vertical))
        .with(POINT, Box::new(point))
}

fn degrees(value: f64) -> eml::PlaneAngleMeasure {
    eml::PlaneAngleMeasure {
        value,
        uom: eml::PlaneAngleUom::Dega,
    }
}

fn station(at: f64, incl: Option<f64>, azi: Option<f64>) -> witsml::TrajectoryStation {
    witsml::TrajectoryStation {
        uid: format!("s{at}"),
        md: eml::MeasuredDepth {
            measured_depth: eml::LengthMeasureExt {
                value: at,
                uom: eml::LengthUomExt::M,
            },
            datum: None,
        },
        type_traj_station: witsml::TrajStationType::MDINCLAndAZI,
        incl: incl.map(degrees),
        azi: azi.map(degrees),
        ..Default::default()
    }
}

/// Vertical to MD 100, then one arc to horizontal toward azimuth 90 at MD 1000, from the datum `uuid`
/// of the given type, north as asked.
fn trajectory(datum: &str, datum_type: &str, north: Option<eml::NorthReferenceKind>) -> witsml::Trajectory {
    witsml::Trajectory {
        uuid: TRAJECTORY.into(),
        schema_version: "2.1".into(),
        md_interval: Some(eml::MdInterval {
            md_min: 0.0,
            md_max: 1000.0,
            uom: eml::LengthUomExt::M,
            datum: Some(dor(datum, datum_type)),
            ..Default::default()
        }),
        default_md_datum: Some(dor(datum, datum_type)),
        trajectory_station: vec![
            station(0.0, Some(0.0), None),
            station(100.0, Some(0.0), None),
            station(1000.0, Some(90.0), Some(90.0)),
        ],
        azi_ref: north,
        ..Default::default()
    }
}

/// A hole from a point in a projected CRS.
fn from_point(north: eml::NorthReferenceKind) -> witsml::Trajectory {
    trajectory(POINT, "eml23.ReferencePointInACrs", Some(north))
}

/// Two vertical stations, MD 0 and 100 m, from the point: no azimuth, no north.
fn vertical_from_point() -> witsml::Trajectory {
    let mut vertical = trajectory(POINT, "eml23.ReferencePointInACrs", None);
    vertical.trajectory_station = vec![station(0.0, Some(0.0), None), station(100.0, Some(0.0), None)];
    vertical
}

fn close(found: [f64; 3], expected: [f64; 3], tolerance: f64) -> bool {
    (0..3).all(|axis| (found[axis] - expected[axis]).abs() <= tolerance)
}

/// The points of the survey as the scene gives them in the given placement.
fn points_in(trajectory: &witsml::Trajectory, site: &Site, placement: Placement) -> Vec<[f64; 3]> {
    scene::object_of(trajectory, site, "eml:///test")
        .expect("the survey is a scene object")
        .points_in(placement)
        .expect("placed")
}

/// A compound whose horizontal axes are in feet already places the survey in metres: the displacement is
/// in metres, the datum is converted by the CRS. True before and after the lot (the guard of the rule).
#[test]
fn a_compound_in_feet_already_lands_in_metres() {
    // To break: take `h = 1.0` in `to_crs_frame`.
    let site = compound_site(eml::LengthAndTimeUomExt::Ft);
    let grid = trajectory(
        COLLAR,
        "eml23.ReferencePointInALocalEngineeringCompoundCrs",
        Some(eml::NorthReferenceKind::GridNorth),
    );
    let found = points_in(&grid, &site, Placement::Metres);
    let (sin, cos) = 30f64.to_radians().sin_cos();
    let expected = [
        (1000.0 + 10.0 * cos) * FOOT + R,
        (2000.0 - 10.0 * sin) * FOOT,
        30.0 - 100.0 - R,
    ];
    assert!(
        close(found[2], expected, 1e-6),
        "{:?} against {expected:?}",
        found[2]
    );
}

/// A point in a projected CRS in feet: the station is placed in metres (WITSML TRG 2.1 §16.6 p. 350).
#[test]
fn a_point_in_a_feet_projected_crs_is_placed_in_metres() {
    // To break: put `horizontal_uom: None` back in `crs::of`.
    let site = projected_site(
        eml::AxisOrder2d::EastingNorthing,
        Some(eml::LengthUomExt::Ft),
        eml::LengthUomExt::M,
        30.0,
    );
    let grid = from_point(eml::NorthReferenceKind::GridNorth);
    let metres = points_in(&grid, &site, Placement::Metres);
    let expected = [500_000.0 * FOOT + R, 6_000_000.0 * FOOT, -70.0 - R];
    assert!(
        close(metres[2], expected, 1e-6),
        "{:?} against {expected:?}",
        metres[2]
    );
    let projected = points_in(&grid, &site, Placement::Projected);
    let expected = [500_000.0 + R / FOOT, 6_000_000.0, -70.0 - R];
    assert!(
        close(projected[2], expected, 1e-6),
        "{:?} against {expected:?}",
        projected[2]
    );
}

/// A point whose elevation is written in feet (its vertical CRS counts in feet): the elevation is in
/// metres once placed, as the measured depths are.
#[test]
fn a_point_in_a_feet_vertical_crs_counts_its_elevation_in_metres() {
    // To break: remove the conversion of the elevation in `place_datum`.
    let site = projected_site(
        eml::AxisOrder2d::EastingNorthing,
        Some(eml::LengthUomExt::M),
        eml::LengthUomExt::Ft,
        100.0,
    );
    let found = points_in(&vertical_from_point(), &site, Placement::Metres);
    assert!(
        close(found[0], [500_000.0, 6_000_000.0, 100.0 * FOOT], 1e-9),
        "{:?}",
        found[0]
    );
    assert!(
        close(found[1], [500_000.0, 6_000_000.0, 100.0 * FOOT - 100.0], 1e-9),
        "{:?}",
        found[1]
    );
}

/// A RESQML trajectory with no geometry, on the same point, hangs from the same elevation: the two
/// readers go through one door, `place_datum`.
#[test]
fn a_resqml_trajectory_on_the_same_point_hangs_from_the_same_elevation() {
    // To break: convert the elevation only for the survey (a second reading of it).
    let site = projected_site(
        eml::AxisOrder2d::EastingNorthing,
        Some(eml::LengthUomExt::M),
        eml::LengthUomExt::Ft,
        100.0,
    );
    let resqml = energyml_resqml_2_2::WellboreTrajectoryRepresentation {
        uuid: RESQML.into(),
        md_interval: eml::MdInterval {
            md_min: 0.0,
            md_max: 100.0,
            uom: eml::LengthUomExt::M,
            datum: Some(dor(POINT, "eml23.ReferencePointInACrs")),
            ..Default::default()
        },
        ..Default::default()
    };
    let found = geometry::of(&resqml, &site).expect("drawn");
    let head = found.patches[0].points[0];
    assert!((head[2] - 100.0 * FOOT).abs() < 1e-9, "{head:?}");
    let in_metres = points_in(&vertical_from_point(), &site, Placement::Metres);
    let resqml_in_metres = scene::object_of(&resqml, &site, "eml:///resqml")
        .expect("a scene object")
        .points_in(Placement::Metres)
        .expect("placed");
    assert!(
        close(resqml_in_metres[0], in_metres[0], 1e-9),
        "{resqml_in_metres:?}"
    );
}

/// The CRS of a point states the units its points are in, and the placed path of WP04 agrees with the
/// scene's conversion (a triangulation: two ways to the same metres).
#[test]
fn the_crs_of_a_point_states_the_units_its_points_are_in() {
    // To break: take `f = 1.0` for the axes.
    let site = projected_site(
        eml::AxisOrder2d::EastingNorthing,
        Some(eml::LengthUomExt::Ft),
        eml::LengthUomExt::M,
        30.0,
    );
    let grid = from_point(eml::NorthReferenceKind::GridNorth);
    let crs = energyml_domain::crs::of(&grid, &site)
        .expect("no error")
        .expect("a CRS through the datum");
    assert_eq!(crs.horizontal_uom.as_deref(), Some("ft"));
    assert_eq!(crs.vertical_uom.as_deref(), Some("m"));
    let placed = stations::read_declaring(&grid, &site, &SurveyDeclarations::default()).expect("placed");
    let path = placed.placed_path().expect("a placed path");
    for (k, md) in [0.0, 100.0, 1000.0].into_iter().enumerate() {
        let from_the_crs = crs.to_metres(placed.points[k]).expect("metres");
        let from_the_path = path.at(md).point;
        assert!(
            close(from_the_path, from_the_crs, 1e-6),
            "MD {md}: {from_the_path:?} against {from_the_crs:?}"
        );
    }
}
