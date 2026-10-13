//! **The wells table reads the frame it lists, not the frame its uuid names
//! first** (ADR-0003: identity is the full URI).
//!
//! `wells::of_store` held each summary and read it by `summary.uuid`: a frame
//! held in two versions was read twice as the same one. The package is
//! fabricated in memory, the frame in versions 1 and 2, **titled apart**; the
//! `wanted` filter selects by title, so which entry was read decides which
//! frames go on. These frames carry no trajectory, so each one that goes on is
//! *noted* as unreadable - and the note names the entry by its full uri.
//!
//! What would have to break: reading by `summary.uuid` again. Both entries are
//! then read as the first, titled « F, first », and nothing matches
//! « F, second ».

use std::io::Cursor;
use std::sync::Arc;

use energyml_core::registry::{DynObject, ReadMode};
use energyml_storage::EpcStore;

const RESQML: &str = "http://www.energistics.org/energyml/data/resqmlv2";
const EML: &str = "http://www.energistics.org/energyml/data/commonv2";
const F: &str = "7e200000-0000-4000-8000-000000000001";
const CT: &str = "application/x-resqml+xml;version=2.2;type=WellboreFrameRepresentation";

fn frame(version: &str, title: &str) -> Box<dyn DynObject> {
    let xml = format!(
        "<resqml:WellboreFrameRepresentation xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" uuid=\"{F}\" \
         schemaVersion=\"2.2\" objectVersion=\"{version}\">\
         <eml:Citation><eml:Title>{title}</eml:Title>\
         <eml:Originator>test</eml:Originator>\
         <eml:Creation>2026-10-05T00:00:00Z</eml:Creation>\
         <eml:Format>test</eml:Format></eml:Citation>\
         </resqml:WellboreFrameRepresentation>"
    );
    energyml_schemas::all()
        .read_xml_as(CT, xml.as_bytes(), ReadMode::Tolerant)
        .expect("readable")
        .0
}

fn two_frames() -> EpcStore {
    let empty = energyml_storage::extract::empty_package(Some(0)).expect("an empty package");
    let mut store = EpcStore::from_reader(
        Box::new(Cursor::new(empty)),
        "frames.epc",
        Arc::new(energyml_schemas::all()),
    )
    .expect("opens");
    store.put(frame("1", "F, first")).expect("the first");
    store
        .put_as_new_version(frame("2", "F, second"))
        .expect("the second");
    store
}

#[test]
fn a_title_selects_the_version_that_carries_it() {
    let store = two_frames();
    assert_eq!(store.resources().len(), 2, "one uuid, two entries");

    let second = energyml_geometry::wells::of_store(&store.resources(), &store, &["F, second".to_string()]);
    assert_eq!(
        second.notes.len(),
        1,
        "exactly the second goes on: {:?}",
        second.notes
    );
    assert!(
        second.notes[0].contains("version='2'"),
        "and the note names it by its uri: {:?}",
        second.notes
    );

    let first = energyml_geometry::wells::of_store(&store.resources(), &store, &["F, first".to_string()]);
    assert_eq!(
        first.notes.len(),
        1,
        "exactly the first goes on: {:?}",
        first.notes
    );
    assert!(first.notes[0].contains("version='1'"), "{:?}", first.notes);

    let both = energyml_geometry::wells::of_store(&store.resources(), &store, &[]);
    assert_eq!(
        both.notes.len(),
        2,
        "no filter: both entries are read, each as itself: {:?}",
        both.notes
    );
}
