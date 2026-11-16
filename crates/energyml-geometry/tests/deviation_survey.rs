//! A RESQML 2.0.1 `DeviationSurveyRepresentation`, read and drawn - `src/stations/deviation.rs`.
//!
//! **The form of the assertions comes from the standards**: one measured depth, one azimuth and one
//! inclination per station, in three arrays of `StationCount` values; the azimuths relative to the
//! projected CRS north (grid north) and the inclinations from the vertical, with no method stated
//! (RESQML TRG 2.0.1 §7.5.7 p. 139-140; TUG 2.0.1 §12.2.3.3 p. 204).
//!
//! **The numbers are worked by hand**: 11 stations every 30 m, the inclination growing by 3 degrees per
//! station toward azimuth 90 - a build of 3 degrees per 30 m.

use std::collections::HashMap;

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::source::{ArraySource, ObjectSource};
use energyml_eml_2_0::{
    AbstractProjectedCrs, AbstractVerticalCrs, AxisOrder2d, DataObjectReference, LengthUom,
    PlaneAngleMeasure, PlaneAngleUom, ProjectedCrsEpsgCode, VerticalCrsEpsgCode,
};
use energyml_geometry::geometry;
use energyml_geometry::min_curvature::courses;
use energyml_geometry::scene::{self, Placement};
use energyml_geometry::stations::{
    self, CheckKind, CheckOutcome, Finding, MethodFacts, NorthReference, SurveyDeclarations,
};
use energyml_resqml_2_0_1::{
    AbstractDoubleArray, DeviationSurveyRepresentation, DoubleConstantArray, DoubleLatticeArray,
    LocalDepth3dCrs, MdDatum, MdReference, Point3d,
};

const CRS: &str = "50000000-0000-4000-8000-000000000001";
const DATUM: &str = "50000000-0000-4000-8000-000000000002";
const SURVEY: &str = "50000000-0000-4000-8000-000000000003";

fn dor(uuid: &str, content_type: &str) -> DataObjectReference {
    DataObjectReference {
        uuid: uuid.to_string(),
        content_type: content_type.to_string(),
        ..Default::default()
    }
}

/// Objects by uuid, handed out as a store would.
struct Site(HashMap<String, Box<dyn DynObject>>);

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

/// A local depth CRS: origin (1000, 2000), z counting downward, metres horizontally.
fn local_crs(vertical_uom: LengthUom) -> LocalDepth3dCrs {
    LocalDepth3dCrs {
        uuid: CRS.to_string(),
        schema_version: "2.0".into(),
        x_offset: 1000.0,
        y_offset: 2000.0,
        z_offset: 0.0,
        areal_rotation: PlaneAngleMeasure {
            value: 0.0,
            uom: PlaneAngleUom::Dega,
        },
        projected_uom: LengthUom::M,
        vertical_uom,
        z_increasing_downward: true,
        projected_axis_order: AxisOrder2d::EastingNorthing,
        projected_crs: Some(AbstractProjectedCrs::ProjectedCrsEpsgCode(ProjectedCrsEpsgCode {
            epsg_code: 2154,
        })),
        vertical_crs: Some(AbstractVerticalCrs::VerticalCrsEpsgCode(VerticalCrsEpsgCode {
            epsg_code: 5720,
        })),
        ..Default::default()
    }
}

/// The datum of the depths: a point written at `(0, 0, z)` in the CRS above.
fn md_datum(z: f64) -> MdDatum {
    MdDatum {
        uuid: DATUM.to_string(),
        location: Point3d {
            coordinate1: 0.0,
            coordinate2: 0.0,
            coordinate3: z,
        },
        md_reference: MdReference::KellyBushing,
        local_crs: dor(
            CRS,
            "application/x-resqml+xml;version=2.0;type=obj_LocalDepth3dCrs",
        ),
        ..Default::default()
    }
}

fn site(crs: LocalDepth3dCrs, datum: MdDatum) -> Site {
    let mut objects: HashMap<String, Box<dyn DynObject>> = HashMap::new();
    objects.insert(CRS.to_string(), Box::new(crs));
    objects.insert(DATUM.to_string(), Box::new(datum));
    Site(objects)
}

fn lattice(start: f64, step: f64, count: u64) -> AbstractDoubleArray {
    AbstractDoubleArray::DoubleLatticeArray(DoubleLatticeArray {
        start_value: start,
        offset: vec![DoubleConstantArray { value: step, count }],
    })
}

fn constant(value: f64, count: u64) -> AbstractDoubleArray {
    AbstractDoubleArray::DoubleConstantArray(DoubleConstantArray { value, count })
}

/// 11 stations, every 30 m, the inclination up by 3 degrees per station, toward azimuth 90. The default
/// units of the generated code are « 0.1 ft » and « 0.001 seca »: written always.
fn survey() -> DeviationSurveyRepresentation {
    DeviationSurveyRepresentation {
        uuid: SURVEY.to_string(),
        schema_version: "2.0".into(),
        station_count: 11,
        md_uom: LengthUom::M,
        angle_uom: PlaneAngleUom::Dega,
        mds: Some(lattice(0.0, 30.0, 10)),
        inclinations: Some(lattice(0.0, 3.0, 10)),
        azimuths: Some(constant(90.0, 11)),
        md_datum: dor(DATUM, "application/x-resqml+xml;version=2.0;type=obj_MdDatum"),
        ..Default::default()
    }
}

/// The same survey, vertical: every inclination 0, so no azimuth moves anything.
fn vertical_survey() -> DeviationSurveyRepresentation {
    let mut vertical = survey();
    vertical.station_count = 2;
    vertical.mds = Some(lattice(0.0, 100.0, 1));
    vertical.inclinations = Some(constant(0.0, 2));
    vertical.azimuths = Some(constant(0.0, 2));
    vertical
}

fn close(found: [f64; 3], expected: [f64; 3], tolerance: f64) -> bool {
    (0..3).all(|axis| (found[axis] - expected[axis]).abs() <= tolerance)
}

#[test]
fn a_deviation_survey_is_read_into_the_same_stations() {
    // RESQML TRG 2.0.1 §7.5.7 p. 139-140. To break: read `Azimuths` in `Inclinations`.
    let read = stations::stations_of(&survey(), &site(local_crs(LengthUom::M), md_datum(0.0)))
        .expect("a deviation survey is read");
    assert_eq!(read.measured.len(), 11);
    for (k, station) in read.measured.iter().enumerate() {
        assert!((station.md - 30.0 * k as f64).abs() < 1e-12, "{station:?}");
        assert!(
            (station.inclination.unwrap() - 3.0 * k as f64).abs() < 1e-12,
            "{station:?}"
        );
        assert_eq!(station.azimuth, Some(90.0));
    }
    assert_eq!(read.north, Some(NorthReference::Grid));
    assert!(
        read.datums
            .default
            .as_deref()
            .is_some_and(|datum| datum.contains(DATUM)),
        "{:?}",
        read.datums
    );
    let rate = 3f64.to_radians() / 30.0;
    for course in courses(&read.measured).expect("the stations join") {
        assert!((course.severity() - rate).abs() < 1e-12, "{course:?}");
    }
}

#[test]
fn a_deviation_survey_in_radians_is_read_in_degrees() {
    // RESQML TRG 2.0.1 §7.5.7 p. 139: `AngleUom` defines the unit of the azimuths and the inclinations.
    // To break: read the angles as degrees whatever the unit.
    let mut radians = survey();
    radians.angle_uom = PlaneAngleUom::Rad;
    radians.inclinations = Some(lattice(0.0, 3f64.to_radians(), 10));
    radians.azimuths = Some(constant(std::f64::consts::FRAC_PI_2, 11));
    let read = stations::stations_of(&radians, &site(local_crs(LengthUom::M), md_datum(0.0))).expect("read");
    for (k, station) in read.measured.iter().enumerate() {
        assert!(
            (station.inclination.unwrap() - 3.0 * k as f64).abs() < 1e-9,
            "{station:?}"
        );
        assert!((station.azimuth.unwrap() - 90.0).abs() < 1e-9, "{station:?}");
    }
}

#[test]
fn a_deviation_survey_whose_arrays_disagree_is_refused() {
    // RESQML TRG 2.0.1 §7.5.7 p. 139: one value per station, in each of the three arrays.
    // To break: compare only `Mds` with `StationCount`.
    let mut short = survey();
    short.inclinations = Some(lattice(0.0, 3.0, 9));
    let refusal = stations::stations_of(&short, &site(local_crs(LengthUom::M), md_datum(0.0))).unwrap_err();
    assert_eq!(
        refusal.finding,
        Finding::ArrayLength {
            field: "Inclinations",
            found: 10,
            stations: 11
        }
    );
    assert_eq!(refusal.check, CheckKind::Stations);
}

#[test]
fn a_deviation_survey_states_grid_north_and_no_method() {
    // RESQML TRG 2.0.1 §7.5.7 p. 139: the azimuths are relative to the projected CRS north; no method.
    // To break: leave `north` empty.
    let read = stations::stations_of(&survey(), &site(local_crs(LengthUom::M), md_datum(0.0))).expect("read");
    assert_eq!(read.north, Some(NorthReference::Grid));
    assert_eq!(read.methods, MethodFacts::default());
}

#[test]
fn a_deviation_survey_is_never_placed_by_a_guess() {
    // The datum is a point in a local CRS that names no projected CRS to turn the grid-north azimuths
    // from: refused by name at the north in the frame, never placed from `FirstStationLocation`.
    // To break: place the survey from `FirstStationLocation`.
    let diagnosis = stations::diagnose(
        &survey(),
        &site(local_crs(LengthUom::M), md_datum(0.0)),
        &SurveyDeclarations::default(),
    );
    assert_eq!(
        diagnosis.outcome(CheckKind::NorthInFrame),
        &CheckOutcome::Failed(Finding::GridWithoutProjected)
    );
}

#[test]
fn a_deviation_survey_is_drawable_and_states_its_stations() {
    // To break: remove the `reader!` line of `DeviationSurveyRepresentation` in `geometry.rs`.
    let survey = survey();
    assert!(geometry::is_supported(&survey));
    assert!(geometry::supports("DeviationSurveyRepresentation"));
    assert_eq!(geometry::declared_nodes(&survey), Some(11));
}

#[test]
fn a_deviation_survey_that_leaves_the_vertical_is_refused_by_name() {
    // The refusal names the datum's CRS, not an unknown type.
    // To break: place from `FirstStationLocation`.
    let message = geometry::of(&survey(), &site(local_crs(LengthUom::M), md_datum(0.0)))
        .map(|_| ())
        .expect_err("refused")
        .to_string();
    assert!(message.contains("not placed in a projected CRS"), "{message}");
}

#[test]
fn a_vertical_deviation_survey_hangs_below_its_md_datum() {
    // RESQML TRG 2.0.1: `ZIncreasingDownward` makes z a depth under the zero of the local CRS
    // (TODO(verify): the page). The datum at (0, 0, 10): 10 m under the zero of the CRS.
    // To break: take `placed.elevation` instead of `written` (the vertical sense is flipped twice).
    let site = site(local_crs(LengthUom::M), md_datum(10.0));
    let vertical = vertical_survey();
    let metres = scene::object_of(&vertical, &site, "eml:///survey")
        .expect("a scene object")
        .points_in(Placement::Metres)
        .expect("placed");
    assert!(close(metres[0], [1000.0, 2000.0, -10.0], 1e-9), "{:?}", metres[0]);
    assert!(
        close(metres[1], [1000.0, 2000.0, -110.0], 1e-9),
        "{:?}",
        metres[1]
    );
    // The points of the reader are the local ones, which the CRS places.
    let local = geometry::of(&vertical, &site).expect("drawn").patches[0]
        .points
        .clone();
    assert!(close(local[0], [0.0, 0.0, 10.0], 1e-9), "{:?}", local[0]);
    assert!(close(local[1], [0.0, 0.0, 110.0], 1e-9), "{:?}", local[1]);
}

#[test]
fn a_vertical_deviation_survey_in_feet_is_converted_once() {
    // The vertical unit of the CRS is the foot, the point's depth is 10 ft: 3.048 m.
    // To break: take `v = 1.0` in `to_crs_frame`.
    let site = site(local_crs(LengthUom::Ft), md_datum(10.0));
    let metres = scene::object_of(&vertical_survey(), &site, "eml:///survey")
        .expect("a scene object")
        .points_in(Placement::Metres)
        .expect("placed");
    assert!(
        close(metres[0], [1000.0, 2000.0, -3.048], 1e-9),
        "{:?}",
        metres[0]
    );
    assert!(
        close(metres[1], [1000.0, 2000.0, -103.048], 1e-9),
        "{:?}",
        metres[1]
    );
}
