//! **A reference is followed by its full uri: the version it names is the
//! version read** (ADR-0003: internally, full URIs only). A package can hold one uuid in two `objectVersion`s; a reader
//! that follows a reference by the uuid it carries reads the wrong one.
//!
//! The package is fabricated, and its source is **made to lie to a uuid**: asked
//! `object(uuid)` it answers the *first* version it holds, and only
//! `object_at(uri)` tells the two apart — as a store holding both would. A test
//! that passes therefore proves the reader asked by uri.
//!
//! What would have to break: any reader here going back to `source.object(&uuid)`.
//! The frame's head, the trajectory's datum, the interpretation and the grid of
//! a connection set would then come from version 1 whatever the reference says.

use energyml_core::identifiers::EtpUri;
use energyml_core::identity;
use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::source::{ArraySource, ObjectSource};
use energyml_eml_2_3::{
    AbstractHorizontalCoordinates, AbstractIntegerArray, AbstractProjectedCrs, AbstractVerticalCrs, DataObjectReference,
    IntegerConstantArray, MdInterval, ProjectedCoordinates, ProjectedCrs, ProjectedEpsgCrs, ReferencePointInACrs, VerticalCrs,
    VerticalDirection, VerticalEpsgCrs,
};
use energyml_geometry::{Placement, connection, heads, scene};
use energyml_resqml_2_2::{
    GridConnectionSetRepresentation, HorizonInterpretation, WellboreFrameRepresentation, WellboreTrajectoryRepresentation,
};

const VCRS: &str = "22222222-2222-2222-2222-222222222222";
const PCRS: &str = "aaaaaaaa-0000-0000-0000-00000000000a";
const DATUM: &str = "11111111-1111-1111-1111-111111111111";
const TRAJ: &str = "33333333-3333-3333-3333-333333333333";
const FRAME: &str = "66666666-6666-6666-6666-666666666666";
const HORIZON: &str = "77777777-7777-7777-7777-777777777777";
const SURFACE: &str = "88888888-8888-8888-8888-888888888888";
const GRID: &str = "99999999-9999-9999-9999-999999999999";
const SET: &str = "55555555-5555-5555-5555-555555555555";

fn dor(uuid: &str, version: Option<&str>, qualified_type: &str) -> DataObjectReference {
    DataObjectReference {
        uuid: uuid.to_string(),
        object_version: version.map(str::to_string),
        qualified_type: qualified_type.to_string(),
        ..Default::default()
    }
}

fn datum(version: &str, x: f64, y: f64) -> ReferencePointInACrs {
    ReferencePointInACrs {
        uuid: DATUM.to_string(),
        object_version: Some(version.to_string()),
        vertical_crs: Some(dor(VCRS, None, "eml23.VerticalCrs")),
        vertical_coordinate: Some(37.73),
        horizontal_coordinates: Some(AbstractHorizontalCoordinates::ProjectedCoordinates(ProjectedCoordinates {
            coordinate1: x,
            coordinate2: y,
            crs: dor(PCRS, None, "eml23.ProjectedCrs"),
        })),
        ..Default::default()
    }
}

/// A trajectory collared at the datum **version** it names.
fn trajectory(version: &str, datum_version: &str) -> WellboreTrajectoryRepresentation {
    WellboreTrajectoryRepresentation {
        uuid: TRAJ.to_string(),
        object_version: Some(version.to_string()),
        md_interval: MdInterval {
            md_min: 0.0,
            md_max: 65.0,
            uom: energyml_eml_2_3::LengthUomExt::M,
            datum: Some(dor(DATUM, Some(datum_version), "eml23.ReferencePointInACrs")),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn interpretation(version: &str) -> HorizonInterpretation {
    HorizonInterpretation {
        uuid: HORIZON.to_string(),
        object_version: Some(version.to_string()),
        ..Default::default()
    }
}

/// Holds every object; answers a uuid with the **first** it has, a uri with the
/// one it names, and refuses a uri naming a version it does not hold.
struct Versions(Vec<Box<dyn DynObject>>);

impl ObjectSource for Versions {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        self.0
            .iter()
            .find(|one| identity::uuid_of_reflect(one.as_reflect()).is_some_and(|held| held.eq_ignore_ascii_case(uuid)))
            .map(|one| one.clone_box())
    }

    fn object_at(&self, uri: &str) -> Option<Box<dyn DynObject>> {
        let Ok(parsed) = EtpUri::parse(uri) else { return self.object(uri) };
        self.0
            .iter()
            .find(|one| {
                let held = one.as_reflect();
                identity::uuid_of_reflect(held).map(str::to_ascii_lowercase) == parsed.uuid
                    && identity::object_version_of_reflect(held).map(str::to_string) == parsed.object_version
            })
            .map(|one| one.clone_box())
    }

    fn read_object_at(&self, uri: &str) -> Result<Box<dyn DynObject>, String> {
        self.object_at(uri).ok_or_else(|| format!("no such object: {uri}"))
    }
}

impl ArraySource for Versions {
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

fn systems() -> Vec<Box<dyn DynObject>> {
    vec![
        Box::new(VerticalCrs {
            uuid: VCRS.to_string(),
            direction: VerticalDirection::Up,
            abstract_vertical_crs: Some(AbstractVerticalCrs::VerticalEpsgCrs(VerticalEpsgCrs { epsg_code: 5714 })),
            ..Default::default()
        }),
        Box::new(ProjectedCrs {
            uuid: PCRS.to_string(),
            abstract_projected_crs: Some(AbstractProjectedCrs::ProjectedEpsgCrs(ProjectedEpsgCrs { epsg_code: 2154 })),
            ..Default::default()
        }),
    ]
}

fn ask(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|one| one.to_string()).collect()
}

/// A trajectory names the **version** of its datum, and is collared there.
///
/// Casser : in `well::place_datum`, ask `source.object(datum_uuid)` — the head
/// comes from version 1, at (1000, 2000).
#[test]
fn a_trajectory_is_collared_at_the_datum_version_it_names() {
    let mut held = systems();
    held.push(Box::new(datum("1", 1000.0, 2000.0)));
    held.push(Box::new(datum("2", 7000.0, 8000.0)));
    held.push(Box::new(trajectory("1", "2")));
    let source = Versions(held);

    let found = heads::of_objects(&source, &ask(&[TRAJ]), Placement::Projected).unwrap();
    assert!(found.refused.is_empty(), "{:?}", found.refused);
    assert_eq!(found.heads[0].xy, [7000.0, 8000.0], "the datum of version 2, not the first of its uuid");
}

/// A frame hangs on the **version** of the trajectory it names; the two
/// trajectories are collared at two datums.
///
/// Casser : in `heads::head_of_one`, follow the frame's trajectory by
/// `source.object(uuid)` — the frame is collared at version 1's datum.
#[test]
fn a_frame_hangs_on_the_trajectory_version_it_names() {
    let mut held = systems();
    held.push(Box::new(datum("1", 1000.0, 2000.0)));
    held.push(Box::new(datum("2", 7000.0, 8000.0)));
    held.push(Box::new(trajectory("1", "1")));
    held.push(Box::new(trajectory("2", "2")));
    held.push(Box::new(WellboreFrameRepresentation {
        uuid: FRAME.to_string(),
        trajectory: dor(TRAJ, Some("2"), "resqml22.WellboreTrajectoryRepresentation"),
        ..Default::default()
    }));
    let source = Versions(held);

    let found = heads::of_objects(&source, &ask(&[FRAME]), Placement::Projected).unwrap();
    assert!(found.refused.is_empty(), "{:?}", found.refused);
    assert_eq!(found.heads[0].xy, [7000.0, 8000.0]);

    // A version the package does not hold is refused by its uri, not read as
    // another version of the same uuid.
    let mut held = systems();
    held.push(Box::new(datum("1", 1000.0, 2000.0)));
    held.push(Box::new(trajectory("1", "1")));
    held.push(Box::new(WellboreFrameRepresentation {
        uuid: FRAME.to_string(),
        trajectory: dor(TRAJ, Some("3"), "resqml22.WellboreTrajectoryRepresentation"),
        ..Default::default()
    }));
    let found = heads::of_objects(&Versions(held), &ask(&[FRAME]), Placement::Projected).unwrap();
    assert!(found.heads.is_empty());
    assert!(found.refused[0].why.contains("version='3'"), "{}", found.refused[0].why);
}

/// The interpretation a representation represents is the version it names.
///
/// Casser : in `scene::interpretation_of`, ask `source.object(&uuid)`.
#[test]
fn a_representation_reaches_the_interpretation_version_it_names() {
    let source = Versions(vec![Box::new(interpretation("1")), Box::new(interpretation("2"))]);
    let representation = WellboreFrameRepresentation {
        uuid: SURFACE.to_string(),
        represented_object: Some(dor(HORIZON, Some("2"), "resqml22.HorizonInterpretation")),
        ..Default::default()
    };
    let found = scene::interpretation_of(&representation, &source).expect("the interpretation");
    assert_eq!(identity::object_version_of_reflect(found.as_reflect()), Some("2"));
}

/// A connection set names the **version** of each grid. A version the package
/// does not hold is a named refusal carrying the full uri; it must not be
/// quietly read as the grid's other version.
///
/// Casser : in `connection::read_grids`, ask `source.object(&uuid)` — the grid
/// of version 1 is found and the refusal is another one (no geometry).
#[test]
fn a_connection_set_refuses_a_grid_version_the_package_does_not_hold() {
    let grid = |version: &str| energyml_resqml_2_2::IjkGridRepresentation {
        uuid: GRID.to_string(),
        object_version: Some(version.to_string()),
        ..Default::default()
    };
    let source = Versions(vec![Box::new(grid("1")), Box::new(grid("2"))]);
    let set = GridConnectionSetRepresentation {
        uuid: SET.to_string(),
        count: 1,
        cell_index_pairs: Some(AbstractIntegerArray::IntegerConstantArray(IntegerConstantArray {
            value: 0,
            count: 2,
            ..Default::default()
        })),
        grid: vec![dor(GRID, Some("3"), "resqml22.IjkGridRepresentation")],
        ..Default::default()
    };
    let error = connection::read_with_interpretations(&set, &source).unwrap_err();
    let message = format!("{error}");
    assert!(message.contains("version='3'"), "the refusal names the uri asked: {message}");
}
