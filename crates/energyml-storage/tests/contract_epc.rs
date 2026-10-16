//! The storage contract (`energyml_storage::contract`) played on an EPC.
//!
//! The package is built in memory from the contract's own model
//! ([`contract::model`]) and opened with [`EpcStore::from_reader`]; each group of
//! the contract is a test, so a failure names the group and, in its panic
//! message, the rule, its text and its source (ETP 1.2 section, design note).
//!
//! Needs the `contract` feature of this crate (a dev-dependency on itself
//! switches it on for the tests).
//!
//! The suite must be able to fail, so two tests build a package whose references
//! are not the model's and expect the groups that read them to say so:
//!
//! - `a_package_that_departs_from_the_model_is_caught_by_discovery`: the second
//!   horizon points at the absent feature, so the sources of the feature are
//!   not the model's;
//! - `the_references_group_names_a_departure_from_the_model`: same package, the
//!   references written are not the model's.
//!
//! To make the groups fail on the EPC itself: let `delete_objects` answer an
//! empty list, let `edges()` keep the references to absent objects, or let
//! `resources_in` follow `Sources` the other way.

use std::io::{Cursor, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_storage::EpcStore;
use energyml_storage::contract::{self, ABSENT, HORIZON_2, ModelObject};

fn run<F: std::future::Future>(future: F) -> F::Output {
    futures_executor::block_on(future)
}

fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

/// A package in memory holding exactly these objects.
fn package_of(objects: &[ModelObject]) -> EpcStore {
    let mut bytes = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(Cursor::new(&mut bytes));
        let options: zip::write::SimpleFileOptions = zip::write::FileOptions::default();
        zip.start_file("[Content_Types].xml", options).unwrap();
        zip.write_all(
            b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
              <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
              <Default Extension=\"rels\" \
              ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
              </Types>",
        )
        .unwrap();
        for object in objects {
            zip.start_file(object.part_name(), options).unwrap();
            zip.write_all(object.xml.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }
    EpcStore::from_reader(Box::new(Cursor::new(bytes)), "contract.epc", registry()).expect("opens")
}

/// The store every group is played on: the model, nothing else.
fn package() -> EpcStore {
    package_of(&contract::model().objects)
}

#[test]
fn the_model_is_read_by_the_registry_as_what_it_says_it_is() {
    let registry = registry();
    for object in contract::model().objects {
        let read = object.read(&registry);
        let info = { read.as_reflect().type_info() };
        assert_eq!(
            info.qualified_type,
            Some(object.qualified_type),
            "{} is not read as {}",
            object.uuid,
            object.qualified_type
        );
    }
}

#[test]
fn objects() {
    run(contract::objects(&package, &registry()));
}

#[test]
fn containment() {
    run(contract::containment(&package, &registry()));
}

#[test]
fn transactions() {
    run(contract::transactions(&package, &registry()));
}

#[test]
fn raw_objects() {
    run(contract::raw_objects(&package));
}

#[test]
fn discovery() {
    run(contract::discovery(&package));
}

#[test]
fn references_and_edges() {
    run(contract::references(&package));
}

#[test]
fn capabilities_agree_with_behaviour() {
    run(contract::capabilities(&package, &registry()));
}

#[test]
fn every_group() {
    run(contract::all(&package, &registry()));
}

/// A package like the model, but whose second horizon references the absent
/// feature instead of the feature the model holds.
fn departing_package() -> EpcStore {
    let mut objects = contract::model().objects;
    let second = objects
        .iter_mut()
        .find(|object| object.uuid == HORIZON_2)
        .expect("the model holds the second horizon");
    *second = contract::horizon(HORIZON_2, "another horizon", ABSENT);
    package_of(&objects)
}

/// The panic message of a group that must break.
fn breaks<F: std::future::Future<Output = ()>>(group: F) -> String {
    let outcome = catch_unwind(AssertUnwindSafe(|| run(group)));
    let payload = outcome.expect_err("the group should have broken a rule");
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default()
}

#[test]
fn a_package_that_departs_from_the_model_is_caught_by_discovery() {
    let message = breaks(contract::discovery(&departing_package));
    assert!(
        message.contains("contract rule DISC-"),
        "a discovery rule should have named the departure: {message}"
    );
}

#[test]
fn the_references_group_names_a_departure_from_the_model() {
    let message = breaks(contract::references(&departing_package));
    assert!(
        message.contains("contract rule REF-"),
        "a references rule should have named the departure: {message}"
    );
}
