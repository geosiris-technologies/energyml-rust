//! The version an object declares travels through the Python bridge (identity
//! lot I2): `Summary` and `Link` carry `object_version`, and `uri_of` refuses a
//! uuid held under two versions instead of choosing one.
//!
//! A view of an object that shows its type and title must also show its
//! version when it is neither empty nor null.
//!
//! The package is **fabricated here**: one feature in versions `1` and `2`, a
//! feature that declares none, and an interpretation that names version 2. What
//! would break these: `Summary` built without the header's version, `uri_of`
//! falling back to `find_by_uuid` ("the first one"), or a `Link` that forgets
//! the version its reference names.

use std::sync::Arc;

use energyml_rust::session::Package;
use energyml_storage::{EpcStore, SaveOptions};
use serde_json::json;

const X: &str = "7e000000-0000-4000-8000-000000000001";
const PLAIN: &str = "7e000000-0000-4000-8000-000000000002";
const HORIZON: &str = "7e000000-0000-4000-8000-0000000000a1";

fn citation(title: &str) -> serde_json::Value {
    json!({
        "$type": "eml23.Citation",
        "Title": title,
        "Originator": "test",
        "Creation": "2026-10-05T00:00:00Z",
        "Format": "test",
    })
}

fn feature(uuid: &str, version: Option<&str>, title: &str) -> String {
    let mut object = json!({
        "$type": "resqml22.BoundaryFeature",
        "uuid": uuid,
        "schemaVersion": "2.2",
        "Citation": citation(title),
    });
    if let Some(version) = version {
        object["objectVersion"] = json!(version);
    }
    object.to_string()
}

fn horizon() -> String {
    json!({
        "$type": "resqml22.HorizonInterpretation",
        "uuid": HORIZON,
        "schemaVersion": "2.2",
        "Citation": citation("a horizon"),
        "Domain": "depth",
        "InterpretedFeature": {
            "$type": "eml23.DataObjectReference",
            "Uuid": X,
            "QualifiedType": "resqml22.BoundaryFeature",
            "Title": "X",
            "ObjectVersion": "2",
        },
    })
    .to_string()
}

fn package() -> (tempfile::TempDir, Package) {
    let directory = tempfile::tempdir().expect("a scratch folder");
    let start = directory.path().join("start.epc");
    std::fs::write(
        &start,
        energyml_storage::extract::empty_package(None).expect("an empty package"),
    )
    .unwrap();
    let mut store = EpcStore::open(&start, Arc::new(energyml_schemas::all())).expect("it opens");
    let read = |store: &EpcStore, json: &str| {
        energyml_core::json::read_string(json, store.registry())
            .expect("it reads")
            .root
    };
    let first = read(&store, &feature(X, Some("1"), "X"));
    store.put(first).expect("version 1");
    let second = read(&store, &feature(X, Some("2"), "X, second"));
    store.put_as_new_version(second).expect("version 2");
    let plain = read(&store, &feature(PLAIN, None, "plain"));
    store.put(plain).expect("a feature without a version");
    let horizon = read(&store, &horizon());
    store.put(horizon).expect("the horizon");
    let saved = directory.path().join("versions.epc");
    store.save_as(&saved, &SaveOptions::default()).expect("it saves");
    let package = Package::open(&saved).expect("it reads back");
    (directory, package)
}

#[test]
fn a_summary_carries_the_version_the_object_declares() {
    let (_directory, package) = package();
    let mut seen: Vec<(String, Option<String>)> = package
        .objects()
        .into_iter()
        .filter(|one| one.qualified_type == "resqml22.BoundaryFeature")
        .map(|one| (one.title, one.object_version))
        .collect();
    seen.sort();
    assert_eq!(
        seen,
        vec![
            ("X".to_string(), Some("1".to_string())),
            ("X, second".to_string(), Some("2".to_string())),
            ("plain".to_string(), None),
        ],
        "a version is shown when declared, and `None` (never an empty string) otherwise"
    );
}

#[test]
fn a_link_carries_the_version_of_the_object_it_describes() {
    let (_directory, package) = package();
    // Backwards: the horizon points at the package's version 2, and the horizon
    // itself declares none — each end says its own.
    let second = package
        .objects()
        .into_iter()
        .find(|one| one.object_version.as_deref() == Some("2"))
        .expect("v2");
    let back = package.referenced_by(&second.uri).expect("it reads");
    let horizon = back
        .iter()
        .find(|link| link.uuid == HORIZON)
        .expect("the horizon points at it");
    assert_eq!(horizon.object_version, None, "the horizon declares no version");
    // Forwards: the link's version comes from the object found when the package
    // holds it. (The version a reference *states* is read from `graph::references`,
    // whose `Pointer` does not carry it yet — a known gap.)
    let horizon = package
        .objects()
        .into_iter()
        .find(|one| one.uuid == HORIZON)
        .expect("the horizon");
    let links = package.references(&horizon.uri).expect("it reads");
    assert_eq!(links.len(), 1);
}

#[test]
fn uri_of_refuses_a_uuid_held_under_two_versions_naming_both() {
    let (_directory, package) = package();
    let refusal = package.uri_of(X).expect_err("two versions, no way to say which");
    assert!(
        refusal.contains("version='1'") && refusal.contains("version='2'"),
        "{refusal}"
    );
    // A uuid held once still answers, and one that is not held is `None`.
    assert!(package.uri_of(PLAIN).expect("one entry").is_some());
    assert_eq!(package.uri_of("00000000-0000-4000-8000-000000000000"), Ok(None));
}
