//! What the command line prints of an object that declares a version (identity
//! lot I2).
//!
//! A rule of the repository: the views of objects that show the type and the
//! title also show the version when it is neither empty nor null.
//!
//! The package is **fabricated here**: one feature in versions `1` and `2`, and
//! one that declares none. `list` must tell the two versions apart, and print
//! nothing extra for the object without one. The wording is
//! `ObjectHeader::titled` / `ObjectId::label` - the CLI only calls it, and
//! that is what this test confronts.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use energyml_storage::{EpcStore, SaveOptions};
use serde_json::json;

const X: &str = "7e000000-0000-4000-8000-000000000001";
const PLAIN: &str = "7e000000-0000-4000-8000-000000000002";

fn feature(uuid: &str, version: Option<&str>, title: &str) -> String {
    let mut object = json!({
        "$type": "resqml22.BoundaryFeature",
        "uuid": uuid,
        "schemaVersion": "2.2",
        "Citation": {
            "$type": "eml23.Citation",
            "Title": title,
            "Originator": "test",
            "Creation": "2026-10-05T00:00:00Z",
            "Format": "test",
        },
    });
    if let Some(version) = version {
        object["objectVersion"] = json!(version);
    }
    object.to_string()
}

fn package(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("energyml-cli-versions-{name}"));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    let start = directory.join("start.epc");
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
    let second = read(&store, &feature(X, Some("2"), "X"));
    store.put_as_new_version(second).expect("version 2");
    let plain = read(&store, &feature(PLAIN, None, "plain"));
    store.put(plain).expect("a feature without a version");
    let saved = directory.join("versions.epc");
    store.save_as(&saved, &SaveOptions::default()).expect("it saves");
    saved
}

fn run(args: &[&str], file: &PathBuf) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_energyml"))
        .args(args)
        .arg(file)
        .output()
        .expect("the binary runs");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn list_tells_two_versions_apart_and_prints_nothing_extra_for_an_unversioned_object() {
    let file = package("list");
    let out = run(&["list"], &file);
    let lines: Vec<&str> = out
        .lines()
        .filter(|line| line.contains("BoundaryFeature"))
        .collect();
    assert_eq!(lines.len(), 3, "{out}");
    assert!(lines.iter().any(|line| line.ends_with("X (version 1)")), "{out}");
    assert!(lines.iter().any(|line| line.ends_with("X (version 2)")), "{out}");
    let plain = lines
        .iter()
        .find(|line| line.contains(PLAIN))
        .expect("the plain feature");
    assert!(plain.ends_with("plain"), "no version, nothing added: {plain}");
}
