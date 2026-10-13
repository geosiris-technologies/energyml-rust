//! A WITSML 2.1 `Trajectory`, drawn from its stations — `src/stations.rs`.
//!
//! **The form of the assertions comes from the standards**, cited where used:
//!
//! - an inclination is *« measured from vertical »*, an azimuth is *« Corrected
//!   to wells azimuth reference »* and turns clockwise from north — *« 0.00 and
//!   360.00 represent North, 90.00 is East »* (WITSML TRG 2.1 §16.10
//!   p. 356–358);
//! - which north is `AziRef`, *« optional because of legacy data »* (§16.7
//!   p. 352); plant north is *« a direction in a local engineering coordinate
//!   reference system »* (eml 2.3 `NorthReferenceKind`);
//! - the depths count from `DefaultMdDatum` (§16.6 p. 350), a reference point
//!   whose coordinates are local ones of the compound it names (common TRG 2.3
//!   §13.25 p. 371);
//! - between two stations, the arc of the minimum curvature method (API D20;
//!   Sawaryn & Thorogood, SPE 84246, 2005).
//!
//! **The numbers are worked by hand here**: a hold of 100 m, then one arc of
//! 90° over 900 m of hole — a circle of radius R = 180/π × 900/90 =
//! 572.9577951 m — from a collar in a compound turned 30° clockwise about
//! (1000, 2000), 10 m along its x axis and 5 m under an origin 35 m above an
//! upward datum: the collar is at (1000 + 10 cos 30°, 2000 − 10 sin 30°, 30).

use std::collections::HashMap;

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::source::{ArraySource, ObjectSource};
use energyml_eml_2_3 as eml;
use energyml_geometry::{Topology, geometry};
use energyml_witsml_2_1 as witsml;

const PLANE: &str = "40000000-0000-4000-8000-000000000001";
const VERTICAL: &str = "40000000-0000-4000-8000-000000000002";
const COMPOUND: &str = "40000000-0000-4000-8000-000000000003";
const COLLAR: &str = "40000000-0000-4000-8000-000000000004";
const OTHER: &str = "40000000-0000-4000-8000-000000000005";
const TRAJECTORY: &str = "40000000-0000-4000-8000-000000000006";

/// The radius of 90° over 900 m, by hand: 180/π × 10.
const R: f64 = 572.957_795_130_823_2;

fn dor(uuid: &str, qualified_type: &str) -> eml::DataObjectReference {
    eml::DataObjectReference { uuid: uuid.into(), qualified_type: qualified_type.into(), ..Default::default() }
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
    fn metadata(&self, location: &energyml_storage::ArrayLocation) -> energyml_domain::Result<energyml_storage::ArrayMetadata> {
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

/// Lambert-93, origin (1000, 2000), turned 30° clockwise; a depth axis from
/// 35 m above NGF-IGN69; the collar 10 m along x, 5 m deep.
fn site() -> Site {
    let plane = eml::LocalEngineering2dCrs {
        uuid: PLANE.into(),
        azimuth: eml::PlaneAngleMeasureExt { value: 30.0, uom: eml::PlaneAngleUomExt::Dega },
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
            abstract_projected_crs: Some(eml::AbstractProjectedCrs::ProjectedEpsgCrs(eml::ProjectedEpsgCrs { epsg_code: 2154 })),
            ..Default::default()
        },
        ..Default::default()
    };
    let vertical = eml::VerticalCrs {
        uuid: VERTICAL.into(),
        direction: eml::VerticalDirection::Up,
        uom: eml::LengthUomExt::M,
        abstract_vertical_crs: Some(eml::AbstractVerticalCrs::VerticalEpsgCrs(eml::VerticalEpsgCrs { epsg_code: 5720 })),
        ..Default::default()
    };
    let compound = eml::LocalEngineeringCompoundCrs {
        uuid: COMPOUND.into(),
        vertical_crs: dor(VERTICAL, "eml23.VerticalCrs"),
        origin_vertical_coordinate: 35.0,
        vertical_axis: eml::VerticalAxis { direction: eml::VerticalDirection::Down, uom: eml::LengthAndTimeUomExt::M, is_time: false },
        local_engineering2d_crs: dor(PLANE, "eml23.LocalEngineering2dCrs"),
        ..Default::default()
    };
    let collar = |uuid: &str| eml::ReferencePointInALocalEngineeringCompoundCrs {
        uuid: uuid.into(),
        horizontal_coordinates: Some(eml::HorizontalCoordinates { coordinate1: 10.0, coordinate2: 0.0 }),
        vertical_coordinate: Some(5.0),
        crs: dor(COMPOUND, "eml23.LocalEngineeringCompoundCrs"),
        ..Default::default()
    };
    Site(HashMap::new())
        .with(PLANE, Box::new(plane))
        .with(VERTICAL, Box::new(vertical))
        .with(COMPOUND, Box::new(compound))
        .with(COLLAR, Box::new(collar(COLLAR)))
        .with(OTHER, Box::new(collar(OTHER)))
}

/// The collar, worked from the citations: x along the local axis turned 30°
/// clockwise, (cos 30°, −sin 30°); a local depth of 5 under an origin 35 m
/// above the datum.
fn collar() -> [f64; 3] {
    let (sin, cos) = 30f64.to_radians().sin_cos();
    [1000.0 + 10.0 * cos, 2000.0 - 10.0 * sin, 30.0]
}

fn degrees(value: f64) -> eml::PlaneAngleMeasure {
    eml::PlaneAngleMeasure { value, uom: eml::PlaneAngleUom::Dega }
}

fn md(value: f64) -> eml::MeasuredDepth {
    eml::MeasuredDepth { measured_depth: eml::LengthMeasureExt { value, uom: eml::LengthUomExt::M }, datum: None }
}

/// A station: its MD, and its inclination and azimuth when it states them.
fn station(at: f64, incl: Option<f64>, azi: Option<f64>) -> witsml::TrajectoryStation {
    witsml::TrajectoryStation {
        uid: format!("s{at}"),
        md: md(at),
        type_traj_station: witsml::TrajStationType::MDINCLAndAZI,
        incl: incl.map(degrees),
        azi: azi.map(degrees),
        ..Default::default()
    }
}

/// Vertical to MD 100, then one arc to horizontal toward azimuth 90 at MD
/// 1000, from the collar, north as asked.
fn trajectory(north: Option<eml::NorthReferenceKind>) -> witsml::Trajectory {
    witsml::Trajectory {
        uuid: TRAJECTORY.into(),
        schema_version: "2.1".into(),
        md_interval: Some(eml::MdInterval {
            md_min: 0.0,
            md_max: 1000.0,
            uom: eml::LengthUomExt::M,
            datum: Some(dor(COLLAR, "eml23.ReferencePointInALocalEngineeringCompoundCrs")),
            ..Default::default()
        }),
        default_md_datum: Some(dor(COLLAR, "eml23.ReferencePointInALocalEngineeringCompoundCrs")),
        trajectory_station: vec![
            station(0.0, Some(0.0), None),
            station(100.0, Some(0.0), None),
            station(1000.0, Some(90.0), Some(90.0)),
        ],
        azi_ref: north,
        ..Default::default()
    }
}

fn close(found: [f64; 3], expected: [f64; 3], tolerance: f64) -> bool {
    (0..3).all(|axis| (found[axis] - expected[axis]).abs() <= tolerance)
}

fn points(trajectory: &witsml::Trajectory) -> Vec<[f64; 3]> {
    let found = geometry::of(trajectory, &site()).unwrap_or_else(|why| panic!("the trajectory is drawn: {why}"));
    assert_eq!(found.patches.len(), 1);
    let patch = &found.patches[0];
    assert!(matches!(&patch.topology, Topology::Polylines(runs) if runs == &vec![3]), "one line, a node per station");
    patch.points.clone()
}

fn refusal(trajectory: &witsml::Trajectory) -> String {
    geometry::of(trajectory, &site()).map(|_| ()).expect_err("refused").to_string()
}

// ---------------------------------------------------------------------------

/// Grid north: the displacement is along the projected axes, whatever the
/// compound's turn. The arc ends R east of the kick-off and R below it.
#[test]
fn a_grid_north_survey_turns_from_the_projected_north() {
    // To break: the azimuth measured from the compound's axis, east and north
    // swapped, the depth added to the elevation rather than taken from it.
    let [x, y, z] = collar();
    let found = points(&trajectory(Some(eml::NorthReferenceKind::GridNorth)));
    assert!(close(found[0], [x, y, z], 1e-9), "the first station is the datum: {:?}", found[0]);
    assert!(close(found[1], [x, y, z - 100.0], 1e-9), "vertical to MD 100: {:?}", found[1]);
    assert!(close(found[2], [x + R, y, z - 100.0 - R], 1e-6), "{:?}, expected {:?}", found[2], [x + R, y, z - 100.0 - R]);
}

/// Plant north: the compound's second axis, turned 30° clockwise from grid
/// north — so azimuth 90 is its first axis, (cos 30°, −sin 30°) on the map.
#[test]
fn a_plant_north_survey_turns_with_its_compound() {
    // To break: plant north read as grid north (the grid-north answer, 30°
    // off), or the compound's turn applied the wrong way (60° off).
    let [x, y, z] = collar();
    let (sin, cos) = 30f64.to_radians().sin_cos();
    let found = points(&trajectory(Some(eml::NorthReferenceKind::PlantNorth)));
    let expected = [x + R * cos, y - R * sin, z - 100.0 - R];
    assert!(close(found[2], expected, 1e-6), "{:?}, expected {expected:?}", found[2]);
}

/// Placed once: the CRS the trajectory is given through its
/// `MdInterval.Datum` is the compound's identity and none of its placement.
#[test]
fn the_crs_of_a_witsml_trajectory_does_not_place_it_a_second_time() {
    let trajectory = trajectory(Some(eml::NorthReferenceKind::GridNorth));
    let crs = energyml_domain::crs::of(&trajectory, &site()).expect("no error").expect("a CRS through the datum");
    assert_eq!(crs.projected.epsg(), Some(2154));
    assert_eq!(crs.vertical.epsg(), Some(5720));
    for point in points(&trajectory) {
        assert_eq!(crs.to_projected(point), point);
    }
}

/// Listed as drawable, with the count the document states.
#[test]
fn a_witsml_trajectory_is_drawable_and_states_its_stations() {
    let trajectory = trajectory(Some(eml::NorthReferenceKind::GridNorth));
    assert!(geometry::is_supported(&trajectory));
    assert_eq!(geometry::declared_nodes(&trajectory), Some(3));
}

/// A vertical well moves no azimuth, so it needs no north; and a depth in feet
/// is a depth in feet.
#[test]
fn a_vertical_survey_needs_no_north_and_its_feet_are_feet() {
    let mut vertical = trajectory(None);
    vertical.trajectory_station = vec![station(0.0, Some(0.0), None), station(100.0, Some(0.0), None)];
    vertical.trajectory_station[1].md.measured_depth.uom = eml::LengthUomExt::Ft;
    let [x, y, z] = collar();
    let found = geometry::of(&vertical, &site()).expect("a vertical well needs no AziRef").patches[0].points.clone();
    assert!(close(found[1], [x, y, z - 30.48], 1e-9), "100 ft is 30.48 m: {:?}", found[1]);
}

/// An angle in radians is an angle.
#[test]
fn an_angle_in_radians_is_read_as_one() {
    let mut radians = trajectory(Some(eml::NorthReferenceKind::GridNorth));
    for one in &mut radians.trajectory_station {
        for angle in [&mut one.incl, &mut one.azi].into_iter().flatten() {
            angle.value = angle.value.to_radians();
            angle.uom = eml::PlaneAngleUom::Rad;
        }
    }
    let [x, y, z] = collar();
    assert!(close(points(&radians)[2], [x + R, y, z - 100.0 - R], 1e-6));
}

#[test]
fn a_survey_that_does_not_say_which_north_is_refused() {
    let message = refusal(&trajectory(None));
    assert!(message.contains("AziRef"), "{message}");
    let message = refusal(&trajectory(Some(eml::NorthReferenceKind::TrueNorth)));
    assert!(message.contains("convergence") && message.contains("true north"), "{message}");
    let message = refusal(&trajectory(Some(eml::NorthReferenceKind::MagneticNorth)));
    assert!(message.contains("magnetic north"), "{message}");
}

#[test]
fn a_survey_with_no_datum_is_refused_never_drawn_from_the_origin() {
    let mut orphan = trajectory(Some(eml::NorthReferenceKind::GridNorth));
    orphan.default_md_datum = None;
    let message = refusal(&orphan);
    assert!(message.contains("no datum"), "{message}");

    // Named but not in the store: refused as missing, not placed at 0.
    let mut missing = trajectory(Some(eml::NorthReferenceKind::GridNorth));
    let absent = "40000000-0000-4000-8000-0000000000ff";
    missing.default_md_datum = Some(dor(absent, "eml23.ReferencePointInALocalEngineeringCompoundCrs"));
    missing.md_interval.as_mut().unwrap().datum = Some(dor(absent, "eml23.ReferencePointInALocalEngineeringCompoundCrs"));
    let message = refusal(&missing);
    assert!(message.contains(absent), "{message}");
}

#[test]
fn a_survey_whose_datums_disagree_is_refused() {
    // The stations count from one point, the interval (which gives the CRS)
    // names another.
    let mut two = trajectory(Some(eml::NorthReferenceKind::GridNorth));
    two.md_interval.as_mut().unwrap().datum = Some(dor(OTHER, "eml23.ReferencePointInALocalEngineeringCompoundCrs"));
    let message = refusal(&two);
    assert!(message.contains("MdInterval.Datum"), "{message}");

    // One station counts from another datum than the rest.
    let mut split = trajectory(Some(eml::NorthReferenceKind::GridNorth));
    split.trajectory_station[2].md.datum = Some(dor(OTHER, "eml23.ReferencePointInALocalEngineeringCompoundCrs"));
    let message = refusal(&split);
    assert!(message.contains("two datums"), "{message}");
}

#[test]
fn a_survey_that_starts_below_its_datum_is_refused() {
    let mut below = trajectory(Some(eml::NorthReferenceKind::GridNorth));
    below.trajectory_station.remove(0);
    let message = refusal(&below);
    assert!(message.contains("not at its datum"), "{message}");
}

#[test]
fn a_survey_computed_by_another_method_is_refused() {
    let mut tangential = trajectory(Some(eml::NorthReferenceKind::GridNorth));
    tangential.trajectory_station[2].calc_algorithm = Some(witsml::TrajStnCalcAlgorithm::Tangential);
    let message = refusal(&tangential);
    assert!(message.contains("tangential"), "{message}");

    let mut nominal = trajectory(Some(eml::NorthReferenceKind::GridNorth));
    nominal.nominal_calc_algorithm = Some(witsml::TrajStnCalcAlgorithmExt::RadiusOfCurvature);
    let message = refusal(&nominal);
    assert!(message.contains("radius of curvature"), "{message}");

    // Stated as minimum curvature: read.
    let mut stated = trajectory(Some(eml::NorthReferenceKind::GridNorth));
    stated.nominal_calc_algorithm = Some(witsml::TrajStnCalcAlgorithmExt::MinimumCurvature);
    assert_eq!(points(&stated).len(), 3);
}

#[test]
fn a_station_the_survey_cannot_read_is_refused_by_name() {
    // Inclined with no azimuth — `energyml_geometry::min_curvature` names it.
    let mut blind = trajectory(Some(eml::NorthReferenceKind::GridNorth));
    blind.trajectory_station[2].azi = None;
    let message = refusal(&blind);
    assert!(message.contains("station 3") && message.contains("no azimuth"), "{message}");

    // No inclination at all.
    let mut flat = trajectory(Some(eml::NorthReferenceKind::GridNorth));
    flat.trajectory_station[1].incl = None;
    let message = refusal(&flat);
    assert!(message.contains("station 2") && message.contains("no inclination"), "{message}");

    // Depths out of order are not sorted into a survey.
    let mut shuffled = trajectory(Some(eml::NorthReferenceKind::GridNorth));
    shuffled.trajectory_station.swap(1, 2);
    let message = refusal(&shuffled);
    assert!(message.contains("does not come after"), "{message}");

    // An angle in a unit this build cannot convert is not read as degrees.
    let mut centesimal = trajectory(Some(eml::NorthReferenceKind::GridNorth));
    centesimal.trajectory_station[2].incl = Some(eml::PlaneAngleMeasure { value: 90.0, uom: eml::PlaneAngleUom::Ccgr });
    let message = refusal(&centesimal);
    assert!(message.contains("ccgr"), "{message}");

    // A depth whose unit is not a length is not a depth.
    let mut seconds = trajectory(Some(eml::NorthReferenceKind::GridNorth));
    seconds.trajectory_station[2].md.measured_depth.uom = eml::LengthUomExt::Other("s".into());
    let message = refusal(&seconds);
    assert!(message.contains("`s` is a Time unit"), "{message}");

    // A rejected station is not silently dropped nor drawn.
    let mut rejected = trajectory(Some(eml::NorthReferenceKind::GridNorth));
    rejected.trajectory_station[1].status_traj_station = Some(witsml::TrajStationStatus::Rejected);
    let message = refusal(&rejected);
    assert!(message.contains("rejected"), "{message}");
}

/// WITSML 2.0 keeps its datums on the `Well`: the reader, keyed on the
/// concept, meets it and refuses it by name rather than drawing from zero.
#[test]
fn a_witsml_2_0_trajectory_is_refused_by_name() {
    use energyml_witsml_2_0 as w20;
    let old = w20::Trajectory {
        uuid: TRAJECTORY.into(),
        trajectory_station: vec![w20::TrajectoryStation::default(), w20::TrajectoryStation::default()],
        ..Default::default()
    };
    assert!(geometry::is_supported(&old));
    let message = geometry::of(&old, &site()).map(|_| ()).expect_err("no datum").to_string();
    assert!(message.contains("name no datum"), "{message}");
}

// -- A datum placed in a projected CRS ---------------------------------------

const PROJECTED: &str = "40000000-0000-4000-8000-000000000007";
const POINT: &str = "40000000-0000-4000-8000-000000000008";

/// A `ReferencePointInACrs` at (500 000, 6 000 000), 30 m up an upward
/// NGF-IGN69, in a Lambert-93 whose order and unit are as given.
fn projected_site(order: eml::AxisOrder2d, uom: Option<eml::LengthUomExt>) -> Site {
    let projected = eml::ProjectedCrs {
        uuid: PROJECTED.into(),
        axis_order: order,
        uom,
        abstract_projected_crs: Some(eml::AbstractProjectedCrs::ProjectedEpsgCrs(eml::ProjectedEpsgCrs { epsg_code: 2154 })),
        ..Default::default()
    };
    let point = eml::ReferencePointInACrs {
        uuid: POINT.into(),
        horizontal_coordinates: Some(eml::AbstractHorizontalCoordinates::ProjectedCoordinates(eml::ProjectedCoordinates {
            coordinate1: 500_000.0,
            coordinate2: 6_000_000.0,
            crs: dor(PROJECTED, "eml23.ProjectedCrs"),
        })),
        vertical_coordinate: Some(30.0),
        vertical_crs: Some(dor(VERTICAL, "eml23.VerticalCrs")),
        ..Default::default()
    };
    site().with(PROJECTED, Box::new(projected)).with(POINT, Box::new(point))
}

fn from_point(north: eml::NorthReferenceKind) -> witsml::Trajectory {
    let mut trajectory = trajectory(Some(north));
    let datum = Some(dor(POINT, "eml23.ReferencePointInACrs"));
    trajectory.default_md_datum = datum.clone();
    trajectory.md_interval.as_mut().unwrap().datum = datum;
    trajectory
}

/// Grid north from a point in a projected CRS: the projected axes as they
/// stand, once their order and unit are stated.
#[test]
fn a_grid_north_survey_from_a_projected_point_is_drawn_in_that_system() {
    let site = projected_site(eml::AxisOrder2d::EastingNorthing, Some(eml::LengthUomExt::M));
    let found = geometry::of(&from_point(eml::NorthReferenceKind::GridNorth), &site).expect("drawn").patches[0].points.clone();
    let expected = [500_000.0 + R, 6_000_000.0, 30.0 - 100.0 - R];
    assert!(close(found[2], expected, 1e-6), "{:?}, expected {expected:?}", found[2]);
}

#[test]
fn a_projected_point_that_does_not_state_its_axes_or_unit_is_refused() {
    let unitless = projected_site(eml::AxisOrder2d::EastingNorthing, None);
    let message = geometry::of(&from_point(eml::NorthReferenceKind::GridNorth), &unitless).map(|_| ()).expect_err("no unit").to_string();
    assert!(message.contains("states no unit"), "{message}");

    let swapped = projected_site(eml::AxisOrder2d::NorthingEasting, Some(eml::LengthUomExt::M));
    let message = geometry::of(&from_point(eml::NorthReferenceKind::GridNorth), &swapped).map(|_| ()).expect_err("northing first").to_string();
    assert!(message.contains("northing easting"), "{message}");

    // Plant north names a plant, and a point in a projected CRS has none.
    let site = projected_site(eml::AxisOrder2d::EastingNorthing, Some(eml::LengthUomExt::M));
    let message = geometry::of(&from_point(eml::NorthReferenceKind::PlantNorth), &site).map(|_| ()).expect_err("no plant").to_string();
    assert!(message.contains("no local engineering compound"), "{message}");
}
