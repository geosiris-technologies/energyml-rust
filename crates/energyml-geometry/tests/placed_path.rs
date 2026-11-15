//! `PlacedPath`: a trajectory placed in metres, z up, evaluable at any measured depth (WP04 F3).
//!
//! Built by hand, no file read. The references: RESQML TRG 2.0.1 §2.1 p. 10-11 (the offsets of a
//! local CRS, `ZIncreasingDownward`, `ProjectedAxisOrder`); RESQML 2.0.1 `Geometry.xsd` (a
//! parametric line is unbounded); Sawaryn and Thorogood, SPE 84246 (minimum curvature, cited by
//! `min_curvature.rs`); the decisions S2 (a missing fact is a named refusal) and S6 (the TVD is
//! counted below the MD datum) of the specification of the lot.

use energyml_core::registry::DynObject;
use energyml_domain::crs::{AxisOrder, AxisReading, CompassAxes, Handedness};
use energyml_domain::source::{ArraySource, ObjectSource};
use energyml_eml_2_3::{AbstractFloatingPointArray, DataObjectReference, FloatingPointXmlArray, MdInterval};
use energyml_geometry::min_curvature::Measured;
use energyml_geometry::parametric::{Path, Vertical};
use energyml_geometry::well::{self, MdRange, PathRefusal, PlacedPath, SurveyPlacement};
use energyml_geometry::{Crs, CrsKind, VerticalDirection, VerticalQuantity};
use energyml_resqml_2_2::{
    AbstractParametricLineGeometry, AbstractPoint3dArray, ParametricLineGeometry, Point3d,
    Point3dLatticeArray, Point3dLatticeDimension, WellboreTrajectoryRepresentation,
};

const CRS: &str = "55555555-5555-4555-8555-555555555555";
const TRAJECTORY: &str = "66666666-6666-4666-8666-666666666666";

fn crs(direction: VerticalDirection, order: Option<&str>, horizontal: &str, vertical: &str) -> Crs {
    Crs {
        uuid: "test".into(),
        kind: "LocalDepth3dCrs",
        x_offset: 0.0,
        y_offset: 0.0,
        z_offset: 0.0,
        areal_rotation: 0.0,
        vertical_direction: direction,
        vertical_quantity: VerticalQuantity::Length,
        horizontal_uom: Some(horizontal.into()),
        vertical_uom: Some(vertical.into()),
        time_uom: None,
        projected: CrsKind::Absent,
        vertical: CrsKind::Absent,
        axes: order
            .and_then(AxisOrder::from_order)
            .map_or(AxisReading::NotStated, AxisReading::Stated),
    }
}

fn close(a: [f64; 3], b: [f64; 3], tolerance: f64) -> bool {
    (0..3).all(|axis| (a[axis] - b[axis]).abs() < tolerance)
}

fn unit(v: [f64; 3]) -> [f64; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / length, v[1] / length, v[2] / length]
}

fn dor(uuid: &str) -> DataObjectReference {
    DataObjectReference {
        uuid: uuid.to_string(),
        ..Default::default()
    }
}

/// A trajectory whose geometry is the line (0, 0, 0) to (0, 0, 100) over MD 0 to 100, in the CRS `local_crs`.
fn trajectory(local_crs: DataObjectReference) -> WellboreTrajectoryRepresentation {
    let array = |values: Vec<f64>| {
        AbstractFloatingPointArray::FloatingPointXmlArray(FloatingPointXmlArray {
            count_per_value: 1,
            values,
            ..Default::default()
        })
    };
    WellboreTrajectoryRepresentation {
        uuid: TRAJECTORY.to_string(),
        md_interval: MdInterval {
            md_min: 0.0,
            md_max: 100.0,
            uom: energyml_eml_2_3::LengthUomExt::M,
            datum: None,
            ..Default::default()
        },
        geometry: Some(AbstractParametricLineGeometry::ParametricLineGeometry(
            ParametricLineGeometry {
                local_crs,
                control_point_parameters: Some(array(vec![0.0, 100.0])),
                control_points: Some(AbstractPoint3dArray::Point3dLatticeArray(Point3dLatticeArray {
                    all_dimensions_are_orthogonal: None,
                    origin: Point3d {
                        coordinate1: 0.0,
                        coordinate2: 0.0,
                        coordinate3: 0.0,
                    },
                    dimension: vec![Point3dLatticeDimension {
                        direction: Point3d {
                            coordinate1: 0.0,
                            coordinate2: 0.0,
                            coordinate3: 100.0,
                        },
                        spacing: Some(array(vec![1.0])),
                    }],
                })),
                knot_count: 2,
                line_kind_index: 1,
                ..Default::default()
            },
        )),
        ..Default::default()
    }
}

/// A store holding one RESQML 2.0.1 CRS and one trajectory.
struct Site(WellboreTrajectoryRepresentation);

impl ObjectSource for Site {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        match uuid {
            CRS => Some(Box::new(depth_crs())),
            TRAJECTORY => Some(Box::new(self.0.clone())),
            _ => None,
        }
    }
}

impl ArraySource for Site {
    fn locate(&self, _object: &dyn energyml_core::value::Reflect) -> Vec<energyml_storage::ArrayLocation> {
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

/// RESQML TRG 2.0.1 §2.1 p. 10-11: offset (1000, 2000), z counting downward, metres, easting northing.
fn depth_crs() -> energyml_resqml_2_0_1::LocalDepth3dCrs {
    use energyml_eml_2_0 as eml20;
    energyml_resqml_2_0_1::LocalDepth3dCrs {
        uuid: CRS.to_string(),
        schema_version: "2.0".into(),
        x_offset: 1000.0,
        y_offset: 2000.0,
        z_offset: 0.0,
        areal_rotation: eml20::PlaneAngleMeasure {
            value: 0.0,
            uom: eml20::PlaneAngleUom::Dega,
        },
        projected_uom: eml20::LengthUom::M,
        vertical_uom: eml20::LengthUom::M,
        z_increasing_downward: true,
        projected_axis_order: eml20::AxisOrder2d::EastingNorthing,
        projected_crs: Some(eml20::AbstractProjectedCrs::ProjectedCrsEpsgCode(
            eml20::ProjectedCrsEpsgCode { epsg_code: 2154 },
        )),
        vertical_crs: Some(eml20::AbstractVerticalCrs::VerticalCrsEpsgCode(
            eml20::VerticalCrsEpsgCode { epsg_code: 5720 },
        )),
        ..Default::default()
    }
}

#[test]
fn a_trajectory_is_placed_through_its_own_crs() {
    // RESQML TRG 2.0.1 §2.1 p. 10-11; S6. Break it by not applying `to_projected`.
    let t = trajectory(dor(CRS));
    let site = Site(t.clone());
    let placed = PlacedPath::of(&t, &site).expect("a trajectory with its CRS is placed");
    assert!(close(placed.at(50.0).point, [1000.0, 2000.0, -50.0], 1e-9));
    assert!(close(placed.direction_at(50.0).unwrap(), [0.0, 0.0, -1.0], 1e-9));
    assert_eq!(placed.handedness(), Some(Handedness::Right));
    assert_eq!(placed.tvd(50.0), Some(50.0));
    assert_eq!(placed.md_extent(), [0.0, 100.0]);
    assert_eq!(placed.md_uom(), "m");
}

#[test]
fn a_trajectory_without_a_crs_is_refused_until_one_is_declared() {
    // S2: the missing fact is a named refusal, and the declaration is a parameter.
    // Break it by returning `Ok` without a CRS.
    let t = trajectory(DataObjectReference::default());
    let site = Site(t.clone());
    assert!(matches!(PlacedPath::of(&t, &site), Err(PathRefusal::NoCrs)));
    let path = well::path_of(&t, &site).expect("the line itself reads");
    let declared = PlacedPath::new(path, crs(VerticalDirection::Up, Some("easting northing"), "m", "m"), "m");
    assert!(declared.is_ok());
}

#[test]
fn feet_are_placed_as_to_metres_places_them() {
    // Triangulation with `Crs::to_metres`. Break it by forgetting the vertical factor.
    let path = Path::new(
        1,
        vec![[0.0, 0.0, 0.0], [100.0, 0.0, -100.0]],
        vec![0.0, 141.42],
        None,
        Vertical::Up,
    )
    .unwrap();
    let feet = crs(VerticalDirection::Up, Some("easting northing"), "ft", "ft");
    let placed = PlacedPath::new(path.clone(), feet.clone(), "ft").unwrap();
    for md in [0.0, 70.0, 141.42] {
        let expected = feet.to_metres(path.at(md)).unwrap();
        assert!(close(placed.at(md).point, expected, 1e-9), "md {md}");
    }
}

#[test]
fn feet_across_and_metres_down_tilt_the_direction() {
    // Two doors: the scaled direction, and a difference of placed points.
    // Break it by normalising before scaling.
    let path = Path::new(
        1,
        vec![[0.0, 0.0, 0.0], [100.0, 0.0, -100.0]],
        vec![0.0, 141.42],
        None,
        Vertical::Up,
    )
    .unwrap();
    let placed = PlacedPath::new(path, crs(VerticalDirection::Up, Some("easting northing"), "ft", "m"), "ft").unwrap();
    let direction = placed.direction_at(70.0).unwrap();
    assert!(close(direction, unit([0.3048, 0.0, -1.0]), 1e-9), "{direction:?}");
    let h = 1e-4;
    let (before, after) = (placed.at(70.0 - h).point, placed.at(70.0 + h).point);
    let numeric = unit([after[0] - before[0], after[1] - before[1], after[2] - before[2]]);
    assert!(close(direction, numeric, 1e-6));
}

#[test]
fn an_md_past_the_path_is_extended_and_said() {
    // RESQML 2.0.1 `Geometry.xsd`: the line is unbounded. Break it by answering `Within` everywhere.
    let path = Path::new(
        1,
        vec![[0.0, 0.0, 0.0], [0.0, 0.0, -100.0]],
        vec![0.0, 100.0],
        None,
        Vertical::Up,
    )
    .unwrap();
    let placed = PlacedPath::new(path, crs(VerticalDirection::Up, Some("easting northing"), "m", "m"), "m").unwrap();
    let after = placed.at(150.0);
    assert_eq!(after.range, MdRange::AfterLast { last: 100.0 });
    assert!(close(after.point, [0.0, 0.0, -150.0], 1e-9));
    assert_eq!(placed.at(-10.0).range, MdRange::BeforeFirst { first: 0.0 });
    assert_eq!(placed.at(f64::NAN).range, MdRange::NotANumber);
    assert_eq!(placed.at(100.0).range, MdRange::Within);
}

#[test]
fn a_survey_is_the_arc_of_its_stations() {
    // Sawaryn and Thorogood SPE 84246. Break it by building a kind-1 line (the chord): the point
    // then falls at (500500, 6000000, -400).
    let quarter = 500.0 * std::f64::consts::PI;
    let stations = [
        Measured {
            md: 0.0,
            inclination: Some(0.0),
            azimuth: None,
        },
        Measured {
            md: quarter,
            inclination: Some(90.0),
            azimuth: Some(90.0),
        },
    ];
    let placement = SurveyPlacement {
        datum: [500_000.0, 6_000_000.0, 100.0],
        axes: CompassAxes {
            east: [1.0, 0.0],
            north: [0.0, 1.0],
        },
    };
    let placed = PlacedPath::from_stations(&stations, &placement).unwrap();
    let half = std::f64::consts::FRAC_1_SQRT_2;
    let md = quarter / 2.0;
    assert!(
        close(
            placed.at(md).point,
            [500_000.0 + 1000.0 * (1.0 - half), 6_000_000.0, 100.0 - 1000.0 * half],
            1e-6
        ),
        "{:?}",
        placed.at(md).point
    );
    assert!(close(placed.direction_at(md).unwrap(), [half, 0.0, -half], 1e-9));
    assert_eq!(placed.datum(), Some([500_000.0, 6_000_000.0, 100.0]));
}

#[test]
fn a_survey_whose_first_station_is_below_the_datum_is_refused() {
    // `stations.rs`: a survey starts at its datum. Break it by removing the check.
    let stations = [
        Measured {
            md: 30.0,
            inclination: Some(0.0),
            azimuth: None,
        },
        Measured {
            md: 60.0,
            inclination: Some(0.0),
            azimuth: None,
        },
    ];
    let placement = SurveyPlacement {
        datum: [0.0, 0.0, 0.0],
        axes: CompassAxes {
            east: [1.0, 0.0],
            north: [0.0, 1.0],
        },
    };
    assert!(matches!(
        PlacedPath::from_stations(&stations, &placement),
        Err(PathRefusal::FirstStationNotAtDatum { md }) if md == 30.0
    ));
}

#[test]
fn a_placement_with_skewed_axes_is_refused() {
    // The axes of a placement are unit vectors at right angles. Break it by removing the check.
    let path = Path::new(
        1,
        vec![[0.0, 0.0, 0.0], [0.0, 0.0, -10.0]],
        vec![0.0, 10.0],
        None,
        Vertical::Up,
    )
    .unwrap();
    let placement = SurveyPlacement {
        datum: [0.0, 0.0, 0.0],
        axes: CompassAxes {
            east: [1.0, 0.0],
            north: [0.6, 0.8],
        },
    };
    assert!(matches!(
        PlacedPath::from_local(path, &placement),
        Err(PathRefusal::BadPlacement { .. })
    ));
}

#[test]
fn a_survey_placed_northing_easting_is_left_handed() {
    // Common TRG 2.3 §12.7 p. 309 (northing easting is indirect). Break it by taking a default hand.
    let stations = [
        Measured {
            md: 0.0,
            inclination: Some(0.0),
            azimuth: None,
        },
        Measured {
            md: 100.0,
            inclination: Some(0.0),
            azimuth: None,
        },
    ];
    let placement = SurveyPlacement {
        datum: [0.0, 0.0, 0.0],
        axes: CompassAxes {
            east: [0.0, 1.0],
            north: [1.0, 0.0],
        },
    };
    let placed = PlacedPath::from_stations(&stations, &placement).unwrap();
    assert_eq!(placed.handedness(), Some(Handedness::Left));
}

#[test]
fn a_local_path_is_placed_by_its_survey_placement() {
    // Triangulation: `place` against arithmetic written by hand. Break it by forgetting the
    // translation of the datum.
    let path = Path::new(
        1,
        vec![[0.0, 0.0, 0.0], [30.0, 40.0, -100.0]],
        vec![0.0, 111.8],
        None,
        Vertical::Up,
    )
    .unwrap();
    let placement = SurveyPlacement {
        datum: [10.0, 20.0, 5.0],
        axes: CompassAxes {
            east: [0.0, 1.0],
            north: [-1.0, 0.0],
        },
    };
    let placed = PlacedPath::from_local(path, &placement).unwrap();
    let expected = placement.place([30.0, 40.0, -100.0]);
    assert!(close(expected, [-30.0, 50.0, -95.0], 1e-12));
    assert!(close(placed.at(111.8).point, expected, 1e-9));
    for local in [[0.0, 0.0, 0.0], [30.0, 40.0, -100.0], [-7.5, 12.25, 3.0]] {
        let back = placement.to_local(placement.place(local)).unwrap();
        assert!(close(back, local, 1e-12), "{local:?} -> {back:?}");
    }
}
