//! The containment contract (`energyml_storage::contract_witsml`) played on an EPC.
//!
//! The package is built in memory from the WITSML model of the contract
//! ([`witsml::model_witsml`]: a Log holding by value two ChannelSets that hold
//! Channels, one of them in both) and opened with [`EpcStore::from_reader`]. The
//! groups read the capabilities of the store: an EPC that does not know
//! containment (`contained_objects` off) is held to the refusals, one that does
//! to the behaviour.
//!
//! The suite must be able to fail, so two tests build a package whose
//! containment is not the model's and expect the groups that read it to say so:
//!
//! - `a_content_moved_to_another_container_is_caught`: a Channel is moved from
//!   one ChannelSet to the other, so the direct containers of the model are not
//!   the package's;
//! - `a_content_moved_is_caught_by_the_edges_group`: same package, the edges
//!   are not the model's.
//!
//! `containment_cost` reads the scan counter of the store, `EpcStore::scans()`
//! (design note section 5.1, `#[doc(hidden)]`): the one line to adapt if the
//! counter is named otherwise.

use std::io::{Cursor, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_storage::contract::{self, ModelObject};
use energyml_storage::contract_witsml::{self as witsml, CHANNEL_ONLY_A, CHANNEL_ONLY_B, CHANNEL_SHARED};
use energyml_storage::{EnergymlStorage, EpcStore};

fn run<F: std::future::Future>(future: F) -> F::Output {
    futures_executor::block_on(future)
}

fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

/// A package in memory holding exactly these parts.
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
    EpcStore::from_reader(Box::new(Cursor::new(bytes)), "contract-witsml.epc", registry()).expect("opens")
}

/// The store the groups are played on: the WITSML model, nothing else.
fn package() -> EpcStore {
    package_of(&witsml::model_witsml().parts)
}

/// The same with a standalone Channel part of the uuid of a contained Channel.
fn package_with_part() -> EpcStore {
    package_of(&witsml::model_witsml_with_part().parts)
}

/// A dataspace with no container type: the RESQML model of the first contract.
fn plain_package() -> EpcStore {
    package_of(&contract::model().objects)
}

fn scans(store: &EpcStore) -> u64 {
    store.scans()
}

#[test]
fn the_model_parts_are_read_by_the_registry_as_what_they_say_they_are() {
    let registry = registry();
    for object in witsml::model_witsml_with_part().parts {
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
fn the_model_describes_what_it_contains() {
    let model = witsml::model_witsml();
    // Three parts, five objects by value, a channel in two containers.
    assert_eq!(model.parts.len(), 3);
    assert_eq!(model.contents.len(), 5);
    assert_eq!(model.containers_of(CHANNEL_SHARED).len(), 2);
    assert_eq!(model.containers_of(CHANNEL_ONLY_A), vec![witsml::SET_A]);
    // The Log holds the channels through the sets, not directly.
    assert!(
        !model
            .containment()
            .contains(&(CHANNEL_SHARED.to_string(), witsml::LOG.to_string()))
    );
}

#[test]
fn contents() {
    run(witsml::contents(&package));
}

#[test]
fn containment_edges() {
    run(witsml::containment_edges(&package));
}

#[test]
fn contained_reads() {
    run(witsml::contained_reads(&package));
}

#[test]
fn contained_references() {
    run(witsml::contained_references(&package));
}

#[test]
fn contained_part() {
    run(witsml::contained_part(&package_with_part));
}

#[test]
fn containment_cost() {
    run(witsml::contained_cost(&package, &plain_package, &scans));
}

#[test]
fn without_containment() {
    run(witsml::contained_unaware(&package));
}

#[test]
fn writes_that_need_containment() {
    run(witsml::contained_writes(&package, &registry()));
}

#[test]
fn every_group() {
    run(witsml::witsml(
        &package,
        &package_with_part,
        &plain_package,
        &registry(),
        &scans,
    ));
}

/// A package like the model, but whose first channel sits in the other
/// ChannelSet: the direct containers are not the model's.
fn departing_package() -> EpcStore {
    let mut parts = witsml::model_witsml().parts;
    let log = parts
        .iter_mut()
        .find(|part| part.uuid == witsml::LOG)
        .expect("the model holds the log");
    *log = witsml::log_with(
        &[CHANNEL_SHARED],
        &[CHANNEL_ONLY_A, CHANNEL_ONLY_B, CHANNEL_SHARED],
    );
    package_of(&parts)
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

/// The departure can only be seen by a store that knows containment: the test
/// says so rather than passing on one that does not.
fn knows_containment() -> bool {
    departing_package().capabilities().contained_objects
}

#[test]
fn a_content_moved_to_another_container_is_caught() {
    assert!(
        knows_containment(),
        "the EPC store does not state contained_objects: the departure cannot be seen"
    );
    let message = breaks(witsml::contents(&departing_package));
    assert!(
        message.contains("contract rule CONT-"),
        "a containment rule should have named the departure: {message}"
    );
}

#[test]
fn a_content_moved_is_caught_by_the_edges_group() {
    assert!(
        knows_containment(),
        "the EPC store does not state contained_objects: the departure cannot be seen"
    );
    let message = breaks(witsml::containment_edges(&departing_package));
    assert!(
        message.contains("contract rule CONT-"),
        "a containment edge rule should have named the departure: {message}"
    );
}
