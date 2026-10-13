//! An eml 2.3 local engineering compound CRS, read the way the schema writes
//! it (ADR-0018).
//!
//! Every system here is built by hand from the documents, never taken from a
//! file: in the five RESQML 2.2 packages of the corpus the vertical axis and
//! its datum count the same way, the origins are zero and nothing is rotated,
//! so a reader that confused the two statements, or turned the wrong way,
//! passed every corpus test. The citations each test rests on:
//!
//! - `LocalEngineeringCompoundCrs.OriginVerticalCoordinate` — *"Vertical
//!   coordinate of the origin of the local engineering CRS in the base vertical
//!   CRS (consequently in the uom of the base vertical CRS)"*: CRS.xsd
//!   l. 517–520, and common TRG 2.3 §12.20 p. 322;
//! - `VerticalAxis` — `Direction`, `Uom`, `IsTime`: the local axis, common TRG
//!   2.3 §12.34 p. 338; `VerticalCrs` — its own `Direction` and `uom`: the
//!   datum, §12.35 p. 339;
//! - `ArealRotation` — *"The rotation of the local Y axis relative to the
//!   projected Y axis. A positive value indicates a clockwise rotation from the
//!   projected Y axis"*: RESQML TRG 2.0.1 §2.1 p. 10. `Azimuth` is documented
//!   nowhere in 2.3 (common TRG 2.3 §12.19 p. 321 has no notes), and is read as
//!   the same clockwise angle from grid north — see `crs::read_with`;
//! - `ReferencePointInALocalEngineeringCompoundCrs.Crs` — *"To:
//!   LocalEngineeringCompoundCrs"*: common TRG 2.3 §13.25 p. 371.

use energyml_core::registry::DynObject;
use energyml_domain::crs::{self, CrsKind, VerticalDirection};
use energyml_domain::source::ObjectSource;
use energyml_eml_2_3 as eml23;

const PLANE: &str = "10000000-0000-4000-8000-000000000001";
const DATUM: &str = "10000000-0000-4000-8000-000000000002";
const COMPOUND: &str = "10000000-0000-4000-8000-000000000003";
const POINT: &str = "10000000-0000-4000-8000-000000000004";
const TRAJECTORY: &str = "10000000-0000-4000-8000-000000000005";

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

/// The horizontal half: Lambert-93, an origin and an azimuth from grid north.
fn plane(azimuth_degrees: f64, origin: [f64; 2]) -> eml23::LocalEngineering2dCrs {
    eml23::LocalEngineering2dCrs {
        uuid: PLANE.into(),
        azimuth: eml23::PlaneAngleMeasureExt {
            value: azimuth_degrees,
            uom: eml23::PlaneAngleUomExt::Dega,
        },
        azimuth_reference: eml23::NorthReferenceKind::GridNorth,
        origin_projected_coordinate1: origin[0],
        origin_projected_coordinate2: origin[1],
        horizontal_axes: eml23::HorizontalAxes {
            direction1: eml23::AxisDirectionKind::East,
            direction2: eml23::AxisDirectionKind::North,
            uom: eml23::LengthAndTimeUomExt::M,
            is_time: false,
        },
        origin_projected_crs: eml23::ProjectedCrs {
            abstract_projected_crs: Some(eml23::AbstractProjectedCrs::ProjectedEpsgCrs(
                eml23::ProjectedEpsgCrs { epsg_code: 2154 },
            )),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// The datum: NGF-IGN69 (EPSG 5720), in the direction and unit given.
fn datum(direction: eml23::VerticalDirection, uom: eml23::LengthUomExt) -> eml23::VerticalCrs {
    eml23::VerticalCrs {
        uuid: DATUM.into(),
        direction,
        uom,
        abstract_vertical_crs: Some(eml23::AbstractVerticalCrs::VerticalEpsgCrs(
            eml23::VerticalEpsgCrs { epsg_code: 5720 },
        )),
        ..Default::default()
    }
}

fn axis(
    direction: eml23::VerticalDirection,
    uom: eml23::LengthAndTimeUomExt,
    is_time: bool,
) -> eml23::VerticalAxis {
    eml23::VerticalAxis {
        direction,
        uom,
        is_time,
    }
}

fn compound(origin: f64, axis: eml23::VerticalAxis) -> eml23::LocalEngineeringCompoundCrs {
    eml23::LocalEngineeringCompoundCrs {
        uuid: COMPOUND.into(),
        vertical_crs: dor(DATUM, "eml23.VerticalCrs"),
        origin_vertical_coordinate: origin,
        vertical_axis: axis,
        local_engineering2d_crs: dor(PLANE, "eml23.LocalEngineering2dCrs"),
        ..Default::default()
    }
}

/// A compound and its two halves, in one store.
fn system(
    origin: f64,
    local: eml23::VerticalAxis,
    base: eml23::VerticalCrs,
    horizontal: eml23::LocalEngineering2dCrs,
) -> (eml23::LocalEngineeringCompoundCrs, Store) {
    let system = compound(origin, local);
    let store = Store(vec![
        Box::new(horizontal) as Box<dyn DynObject>,
        Box::new(base) as Box<dyn DynObject>,
        Box::new(system.clone()) as Box<dyn DynObject>,
    ]);
    (system, store)
}

/// A depth axis under a datum that counts up.
///
/// The axis says `down`, NGF-IGN69 says `up`, and the origin is 35 — metres
/// *above* the NGF zero, since the schema writes it in the base vertical CRS.
/// A local depth z is then 35 − z metres of elevation. The reader before
/// 24/09 let the datum's `up` overwrite the axis and applied the origin along
/// the axis: it answered 35 + z.
#[test]
fn the_axis_counts_depths_and_the_origin_is_a_height_on_the_datum() {
    let (system, store) = system(
        35.0,
        axis(
            eml23::VerticalDirection::Down,
            eml23::LengthAndTimeUomExt::M,
            false,
        ),
        datum(eml23::VerticalDirection::Up, eml23::LengthUomExt::M),
        plane(0.0, [0.0, 0.0]),
    );
    let read = crs::read_with(&system, &store).expect("the compound reads");

    assert_eq!(
        read.vertical_direction,
        VerticalDirection::Down,
        "the axis governs the local z"
    );
    assert_eq!(
        read.vertical,
        CrsKind::Epsg(5720),
        "the datum gives the vertical system"
    );
    assert_eq!(read.projected, CrsKind::Epsg(2154));
    for depth in [0.0, 10.0, 35.0, 120.0] {
        assert_eq!(
            read.to_projected([0.0, 0.0, depth])[2],
            35.0 - depth,
            "local depth {depth}"
        );
    }
    // And the way back, which is what a writer stores through this system.
    let local = [12.5, -3.0, 40.0];
    let back = read.from_projected(read.to_projected(local));
    for axis in 0..3 {
        assert!(
            (back[axis] - local[axis]).abs() < 1e-9,
            "{local:?} came back {back:?}"
        );
    }
}

/// When the two count the same way — every package of the corpus — nothing
/// moves: the origin is along the axis as it stands. Pinned so that the
/// conversion above cannot be "fixed" by negating every origin.
#[test]
fn an_axis_and_a_datum_counting_the_same_way_keep_the_origin_as_written() {
    let (depths, store) = system(
        35.0,
        axis(
            eml23::VerticalDirection::Down,
            eml23::LengthAndTimeUomExt::M,
            false,
        ),
        datum(eml23::VerticalDirection::Down, eml23::LengthUomExt::M),
        plane(0.0, [0.0, 0.0]),
    );
    let read = crs::read_with(&depths, &store).expect("the compound reads");
    assert_eq!(read.z_offset, 35.0);
    assert_eq!(
        read.to_projected([0.0, 0.0, 10.0])[2],
        -45.0,
        "35 m below the zero, then 10 m deeper"
    );

    let (heights, store) = system(
        35.0,
        axis(eml23::VerticalDirection::Up, eml23::LengthAndTimeUomExt::M, false),
        datum(eml23::VerticalDirection::Up, eml23::LengthUomExt::M),
        plane(0.0, [0.0, 0.0]),
    );
    let read = crs::read_with(&heights, &store).expect("the compound reads");
    assert_eq!(read.to_projected([0.0, 0.0, 10.0])[2], 45.0);
}

/// The unit follows the axis, and the origin is converted into it.
///
/// An axis in feet over a datum in metres, origin 10 (metres, the datum's):
/// the stored z is in feet, so `vertical_uom` must say `ft` — the datum's `m`
/// would read every depth 3.28 times too short — and the origin must be
/// 10 m expressed in feet before it is added to one.
#[test]
fn the_axis_unit_is_the_unit_of_z_and_the_origin_is_converted_into_it() {
    let (system, store) = system(
        10.0,
        axis(
            eml23::VerticalDirection::Up,
            eml23::LengthAndTimeUomExt::Ft,
            false,
        ),
        datum(eml23::VerticalDirection::Up, eml23::LengthUomExt::M),
        plane(0.0, [0.0, 0.0]),
    );
    let read = crs::read_with(&system, &store).expect("the compound reads");
    assert_eq!(read.vertical_uom.as_deref(), Some("ft"));
    assert!((read.z_offset - 10.0 / 0.3048).abs() < 1e-9, "{}", read.z_offset);
    // 100 ft above an origin 10 m above the datum.
    let placed = read.to_metres([0.0, 0.0, 100.0]).expect("feet convert");
    assert!((placed[2] - (10.0 + 30.48)).abs() < 1e-9, "{placed:?}");
}

/// A time compound: the axis says time and milliseconds, the datum stays a
/// length. `is_time` and the axis unit come from the axis alone; the origin
/// takes the datum's sense but no unit conversion — a length and a time are
/// not joined here (a translation tool names the same gap).
#[test]
fn a_time_axis_is_a_time_system_whatever_its_datum_is() {
    let (system, store) = system(
        15.0,
        axis(
            eml23::VerticalDirection::Down,
            eml23::LengthAndTimeUomExt::Ms,
            true,
        ),
        datum(eml23::VerticalDirection::Up, eml23::LengthUomExt::M),
        plane(0.0, [0.0, 0.0]),
    );
    let read = crs::read_with(&system, &store).expect("the compound reads");
    assert!(read.is_time());
    assert_eq!(read.vertical_axis_uom(), Some("ms"));
    assert_eq!(read.vertical_uom.as_deref(), Some("m"), "the datum's length");
    assert_eq!(read.vertical_direction, VerticalDirection::Down);
    assert_eq!(read.z_offset, -15.0);
}

/// And only the vertical axis says so. `HorizontalAxes.IsTime` is about x and
/// y; until 24/09 a `true` there made the whole compound a time system, with a
/// vertical axis in metres.
#[test]
fn horizontal_axes_in_time_do_not_make_z_a_time() {
    let mut horizontal = plane(0.0, [0.0, 0.0]);
    horizontal.horizontal_axes.is_time = true;
    let (system, store) = system(
        0.0,
        axis(
            eml23::VerticalDirection::Down,
            eml23::LengthAndTimeUomExt::M,
            false,
        ),
        datum(eml23::VerticalDirection::Down, eml23::LengthUomExt::M),
        horizontal,
    );
    let read = crs::read_with(&system, &store).expect("the compound reads");
    assert!(!read.is_time());
}

/// A non-zero origin whose datum cannot be read is refused: its sense is
/// stated by an object the store does not hold, and assuming the axis's is
/// the error this test once caught. A zero origin needs no sense.
#[test]
fn an_origin_whose_datum_is_missing_is_refused_and_a_zero_one_is_not() {
    let local = axis(
        eml23::VerticalDirection::Down,
        eml23::LengthAndTimeUomExt::M,
        false,
    );
    let without_datum = Store(vec![Box::new(plane(0.0, [0.0, 0.0])) as Box<dyn DynObject>]);

    let error =
        crs::read_with(&compound(35.0, local.clone()), &without_datum).expect_err("35 of what sense?");
    assert!(error.to_string().contains("OriginVerticalCoordinate"), "{error}");

    let read = crs::read_with(&compound(0.0, local), &without_datum).expect("a zero origin reads");
    assert_eq!(read.z_offset, 0.0);
}

/// The rotation: 30° clockwise from grid north places the local x axis at
/// (cos 30°, −sin 30°) and the local y axis at (sin 30°, cos 30°) from the
/// origin — a clockwise turn, as the survey azimuths and RESQML 2.0.1's
/// `ArealRotation` both state it.
///
/// Confronted with the 2.0.1 system that says the same thing: `ArealRotation`
/// 30° — which the reader took anticlockwise until 24/09, turning a 2.0.1
/// model and its own 2.2 translation (a translation tool carries the angle
/// unchanged) in opposite directions.
#[test]
fn thirty_degrees_clockwise_in_both_releases() {
    use energyml_resqml_2_0_1 as resqml20;

    let (system, store) = system(
        0.0,
        axis(eml23::VerticalDirection::Up, eml23::LengthAndTimeUomExt::M, false),
        datum(eml23::VerticalDirection::Up, eml23::LengthUomExt::M),
        plane(30.0, [1000.0, 2000.0]),
    );
    let new = crs::read_with(&system, &store).expect("the compound reads");

    let old = resqml20::LocalDepth3dCrs {
        uuid: POINT.into(),
        x_offset: 1000.0,
        y_offset: 2000.0,
        areal_rotation: energyml_eml_2_0::PlaneAngleMeasure {
            value: 30.0,
            uom: energyml_eml_2_0::PlaneAngleUom::Dega,
        },
        projected_uom: energyml_eml_2_0::LengthUom::M,
        vertical_uom: energyml_eml_2_0::LengthUom::M,
        ..Default::default()
    };
    let old = crs::read(&old).expect("the 2.0.1 system reads");

    let (sin, cos) = 30f64.to_radians().sin_cos();
    let expected = [
        ([1.0, 0.0, 0.0], [1000.0 + cos, 2000.0 - sin]),
        ([0.0, 1.0, 0.0], [1000.0 + sin, 2000.0 + cos]),
    ];
    for (read, release) in [(&new, "eml 2.3 Azimuth"), (&old, "RESQML 2.0.1 ArealRotation")] {
        for (local, placed) in expected {
            let found = read.to_projected(local);
            assert!(
                (found[0] - placed[0]).abs() < 1e-9 && (found[1] - placed[1]).abs() < 1e-9,
                "{release}: {local:?} placed at {found:?}, the citation puts it at {placed:?}"
            );
        }
    }
}

/// An azimuth measured from anything but grid north needs a convergence angle
/// this crate does not have: refused, never placed as if it were grid north.
#[test]
fn an_azimuth_from_true_north_is_refused() {
    let mut horizontal = plane(30.0, [0.0, 0.0]);
    horizontal.azimuth_reference = eml23::NorthReferenceKind::TrueNorth;
    let (system, store) = system(
        0.0,
        axis(eml23::VerticalDirection::Up, eml23::LengthAndTimeUomExt::M, false),
        datum(eml23::VerticalDirection::Up, eml23::LengthUomExt::M),
        horizontal,
    );
    assert!(crs::read_with(&system, &store).is_err());
}

// ---------------------------------------------------------------------------
// 1b — the datum written in a compound

fn point_in(
    uuid_of_system: &str,
    qualified_type: &str,
) -> eml23::ReferencePointInALocalEngineeringCompoundCrs {
    eml23::ReferencePointInALocalEngineeringCompoundCrs {
        uuid: POINT.into(),
        horizontal_coordinates: Some(eml23::HorizontalCoordinates {
            coordinate1: 10.0,
            coordinate2: 0.0,
        }),
        vertical_coordinate: Some(5.0),
        crs: dor(uuid_of_system, qualified_type),
        ..Default::default()
    }
}

fn trajectory() -> energyml_resqml_2_2::WellboreTrajectoryRepresentation {
    energyml_resqml_2_2::WellboreTrajectoryRepresentation {
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

/// A trajectory whose only anchor is a point in a compound is georeferenced
/// through it: the compound's EPSG identities, and **none** of its placement —
/// the geometry layer has already placed the datum through the compound, and
/// the offsets and azimuth must not reach the points a second time.
///
/// Before 24/09 the datum's type was not known here, and the trajectory came
/// back with no CRS at all.
#[test]
fn a_well_anchored_in_a_compound_takes_its_identity_not_its_offsets() {
    let (_, store) = system(
        35.0,
        axis(
            eml23::VerticalDirection::Down,
            eml23::LengthAndTimeUomExt::M,
            false,
        ),
        datum(eml23::VerticalDirection::Up, eml23::LengthUomExt::M),
        plane(30.0, [1000.0, 2000.0]),
    );
    let store = Store(
        store
            .0
            .into_iter()
            .chain([
                Box::new(point_in(COMPOUND, "eml23.LocalEngineeringCompoundCrs")) as Box<dyn DynObject>,
                Box::new(trajectory()) as Box<dyn DynObject>,
            ])
            .collect(),
    );

    let read = crs::of(&trajectory(), &store)
        .expect("no error")
        .expect("a CRS, through the datum");
    assert_eq!(read.kind, crs::DATUM_IN_COMPOUND);
    assert_eq!(read.projected, CrsKind::Epsg(2154));
    assert_eq!(read.vertical, CrsKind::Epsg(5720));
    assert_eq!(
        read.vertical_direction,
        VerticalDirection::Up,
        "the geometry layer emits elevations"
    );
    assert!(
        read.is_identity(),
        "the compound's placement is not applied twice: {read:?}"
    );
}

/// The point itself: `compound_of_datum` hands back the compound, and its
/// placement of the point's local coordinates is where the collar is —
/// 10 m along the local x axis turned 30° clockwise, 5 m deep under an origin
/// 35 m above NGF.
#[test]
fn a_point_in_a_compound_is_placed_through_it() {
    let (_, store) = system(
        35.0,
        axis(
            eml23::VerticalDirection::Down,
            eml23::LengthAndTimeUomExt::M,
            false,
        ),
        datum(eml23::VerticalDirection::Up, eml23::LengthUomExt::M),
        plane(30.0, [1000.0, 2000.0]),
    );
    let point = point_in(COMPOUND, "eml23.LocalEngineeringCompoundCrs");
    let compound = crs::compound_of_datum(&point, &store)
        .expect("no error")
        .expect("the point names a compound");
    let placed = compound.to_projected([10.0, 0.0, 5.0]);
    let (sin, cos) = 30f64.to_radians().sin_cos();
    assert!((placed[0] - (1000.0 + 10.0 * cos)).abs() < 1e-9, "{placed:?}");
    assert!((placed[1] - (2000.0 - 10.0 * sin)).abs() < 1e-9, "{placed:?}");
    assert_eq!(placed[2], 30.0);

    // Any other object is not this function's business, and says so by `None`.
    let not_one = eml23::ReferencePointInACrs {
        uuid: POINT.into(),
        ..Default::default()
    };
    assert!(
        crs::compound_of_datum(&not_one, &store)
            .expect("no error")
            .is_none()
    );
}

/// The guide gives `Crs` to a `LocalEngineeringCompoundCrs`; a point whose
/// `Crs` names a `ProjectedCrs` says nothing of which way its vertical
/// coordinate counts, and is refused by name rather than placed `up`.
#[test]
fn a_point_whose_crs_is_not_a_compound_is_refused() {
    let projected = eml23::ProjectedCrs {
        uuid: COMPOUND.into(),
        abstract_projected_crs: Some(eml23::AbstractProjectedCrs::ProjectedEpsgCrs(
            eml23::ProjectedEpsgCrs { epsg_code: 2154 },
        )),
        ..Default::default()
    };
    let store = Store(vec![Box::new(projected) as Box<dyn DynObject>]);
    let point = point_in(COMPOUND, "eml23.ProjectedCrs");
    let error = crs::compound_of_datum(&point, &store).expect_err("half a system");
    assert!(error.to_string().contains("ProjectedCrs"), "{error}");
}

/// A measured depth counted from a point in a time system has nothing to be
/// counted in: refused, not read as metres.
#[test]
fn a_well_anchored_in_a_time_compound_is_refused() {
    let (_, store) = system(
        0.0,
        axis(
            eml23::VerticalDirection::Down,
            eml23::LengthAndTimeUomExt::Ms,
            true,
        ),
        datum(eml23::VerticalDirection::Down, eml23::LengthUomExt::M),
        plane(0.0, [0.0, 0.0]),
    );
    let store = Store(
        store
            .0
            .into_iter()
            .chain([Box::new(point_in(COMPOUND, "eml23.LocalEngineeringCompoundCrs")) as Box<dyn DynObject>])
            .collect(),
    );
    let error = crs::of(&trajectory(), &store).expect_err("a time datum");
    assert!(error.to_string().contains("time"), "{error}");
}
