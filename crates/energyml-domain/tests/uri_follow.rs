//! The readers of the domain follow a reference **by its full uri** — the
//! version it names is the version read (ADR-0003).
//!
//! Internally, full uris only (ADR-0003), which avoids sources of bugs. A package may hold one uuid
//! twice, as two `objectVersion`s; following a reference by its uuid alone then
//! reads the wrong one, or fails because the store finds the uuid ambiguous.
//!
//! The package is fabricated here: a `LocalDepth3dCrs` in two versions (two EPSG
//! codes), a grid naming each, and a property hanging on each. Each test names
//! what would have to break to make it fail: a reader asking the source for the
//! bare uuid — the store holds two objects under it and answers none.

use std::io::Cursor;
use std::sync::Arc;

use energyml_core::identity::object_version_of_reflect;
use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::source::ObjectSource;
use energyml_domain::{crs, metadata, property, series};
use energyml_eml_2_0 as eml20;
use energyml_resqml_2_0_1 as resqml20;
use energyml_storage::EpcStore;

const CRS: &str = "bbbbbbbb-0000-4000-8000-000000000001";
const GRID_ON_1: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const GRID_ON_2: &str = "aaaaaaaa-0000-4000-8000-000000000002";
const PROPERTY_ON_1: &str = "cccccccc-0000-4000-8000-000000000001";
const PROPERTY_ON_2: &str = "cccccccc-0000-4000-8000-000000000002";
const EPSG_V1: u64 = 23031;
const EPSG_V2: u64 = 32631;

fn citation(title: &str, version: &str) -> eml20::Citation {
    eml20::Citation {
        title: title.into(),
        originator: "test".into(),
        creation: "2026-10-06T00:00:00Z".into(),
        format: "test".into(),
        version_string: Some(version.into()),
        ..Default::default()
    }
}

fn crs_in(version: &str, epsg: u64) -> Box<dyn DynObject> {
    Box::new(resqml20::LocalDepth3dCrs {
        uuid: CRS.into(),
        schema_version: "2.0".into(),
        object_version: Some(version.into()),
        citation: citation(&format!("crs v{version}"), version),
        projected_crs: Some(eml20::AbstractProjectedCrs::ProjectedCrsEpsgCode(
            eml20::ProjectedCrsEpsgCode { epsg_code: epsg },
        )),
        ..Default::default()
    })
}

/// A reference to the CRS naming version `version`.
fn crs_reference(version: &str) -> eml20::DataObjectReference {
    eml20::DataObjectReference {
        uuid: CRS.into(),
        title: format!("crs v{version}"),
        content_type: "application/x-resqml+xml;version=2.0;type=obj_LocalDepth3dCrs".into(),
        version_string: Some(version.into()),
        ..Default::default()
    }
}

fn grid(uuid: &str, names: &str) -> Box<dyn DynObject> {
    Box::new(resqml20::IjkGridRepresentation {
        uuid: uuid.into(),
        schema_version: "2.0".into(),
        citation: citation("a grid", "1"),
        geometry: Some(resqml20::IjkGridGeometry {
            local_crs: crs_reference(names),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// A property hanging on the grid `support`, which it names at `version`.
fn property_on(uuid: &str, support: &str, version: &str) -> Box<dyn DynObject> {
    Box::new(resqml20::ContinuousProperty {
        uuid: uuid.into(),
        schema_version: "2.0".into(),
        citation: citation("a property", "1"),
        count: 1,
        indexable_element: resqml20::IndexableElements::Cells,
        supporting_representation: eml20::DataObjectReference {
            uuid: support.into(),
            title: "a grid".into(),
            content_type: "application/x-resqml+xml;version=2.0;type=obj_IjkGridRepresentation".into(),
            version_string: Some(version.into()),
            ..Default::default()
        },
        ..Default::default()
    })
}

fn package() -> EpcStore {
    let registry = Arc::new(energyml_schemas::all());
    let empty = energyml_storage::extract::empty_package(Some(0)).expect("an empty package");
    let mut store =
        EpcStore::from_reader(Box::new(Cursor::new(empty)), "uri_follow.epc", registry).expect("opens");
    store.put(crs_in("1", EPSG_V1)).expect("version 1 of the CRS");
    store
        .put_as_new_version(crs_in("2", EPSG_V2))
        .expect("version 2 of the CRS");
    store.put(grid(GRID_ON_1, "1")).expect("a grid naming version 1");
    store.put(grid(GRID_ON_2, "2")).expect("a grid naming version 2");
    store
}

fn epsg_of(store: &EpcStore, grid: &str) -> Option<u64> {
    let object = store.object(grid).expect("the grid");
    crs::of(object.as_reflect(), store)
        .expect("the CRS reads")
        .and_then(|read| read.projected.epsg())
}

/// What would have to break: `crs::of` asking for the CRS by its uuid — two
/// objects hold it, so both grids would come back with a *Missing* error.
#[test]
fn a_crs_is_read_at_the_version_the_reference_names() {
    let store = package();
    assert_eq!(
        epsg_of(&store, GRID_ON_1),
        Some(EPSG_V1),
        "the grid naming version 1"
    );
    assert_eq!(
        epsg_of(&store, GRID_ON_2),
        Some(EPSG_V2),
        "the grid naming version 2"
    );
}

#[test]
fn the_uri_of_the_crs_an_object_names_carries_its_version() {
    let store = package();
    let grid = store.object(GRID_ON_2).expect("the grid");
    let uri = crs::reference_uri(grid.as_reflect()).expect("a uri");
    assert!(uri.contains("version='2'"), "{uri}");
    assert_eq!(
        crs::reference(grid.as_reflect()).as_deref(),
        Some(CRS),
        "the uuid adapter is unchanged"
    );
}

/// A path that crosses a reference reads the target the reference names.
#[test]
fn a_metadata_path_crossing_a_reference_reads_the_version_named() {
    let store = package();
    let value = |grid: &str| {
        let object = store.object(grid).expect("the grid");
        metadata::value_at(object.as_reflect(), "Geometry.LocalCrs.Citation.Title", &store)
    };
    assert_eq!(value(GRID_ON_1).as_deref(), Some("crs v1"));
    assert_eq!(value(GRID_ON_2).as_deref(), Some("crs v2"));
}

/// Among objects already in hand, a reference resolves to the version it
/// names; the uuid alone takes the first.
#[test]
fn a_reference_resolves_among_held_objects_at_its_version() {
    let (one, two) = (crs_in("1", EPSG_V1), crs_in("2", EPSG_V2));
    let held: Vec<(&str, &dyn Reflect)> = vec![(CRS, one.as_reflect()), (CRS, two.as_reflect())];
    let reference = crs_reference("2");
    let found = series::resolve_reference(&reference, &held).expect("version 2 is held");
    assert_eq!(object_version_of_reflect(found), Some("2"));
    let named_1 = series::resolve_reference(&crs_reference("1"), &held).expect("version 1 is held");
    assert_eq!(object_version_of_reflect(named_1), Some("1"));
    assert!(
        series::resolve_reference(&crs_reference("3"), &held).is_none(),
        "a version nobody holds is not guessed"
    );
    let by_uri = series::resolve_at(
        &format!("eml:///resqml20.obj_LocalDepth3dCrs({CRS},version='2')"),
        &held,
    )
    .expect("by uri");
    assert_eq!(object_version_of_reflect(by_uri), Some("2"));
}

/// A property keeps the uri of what it hangs on, and the index tells the two
/// versions of a support apart where the uuid cannot.
#[test]
fn a_property_keeps_the_uri_of_its_support_and_the_index_separates_the_versions() {
    let (first, second) = (
        property_on(PROPERTY_ON_1, GRID_ON_1, "1"),
        property_on(PROPERTY_ON_2, GRID_ON_1, "2"),
    );
    let read = property::read(second.as_reflect()).expect("a property");
    assert_eq!(read.supporting_representation.as_deref(), Some(GRID_ON_1));
    let uri = read.supporting_representation_uri.clone().expect("a uri");
    assert!(uri.contains("version='2'"), "{uri}");

    let (index, refused) = property::Index::build([first.as_reflect(), second.as_reflect()]);
    assert!(refused.is_empty(), "{refused:?}");
    assert_eq!(index.of(GRID_ON_1).len(), 2, "by uuid the two versions merge");
    let on_2 = index.of_uri(&uri);
    assert_eq!(on_2.len(), 1, "by uri they do not");
    assert_eq!(on_2[0].uuid, PROPERTY_ON_2);
}
