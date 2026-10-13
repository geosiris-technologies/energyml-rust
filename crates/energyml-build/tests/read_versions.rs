//! **Reading an object of the package is reading *that entry*** (ADR 0003).
//!
//! `read::every` held `summary.uri` and passed `summary.uuid`, so a package
//! with an object in two versions read the second as the first (Lutetia's
//! “H3 triangulated”, held twice). The rule (ADR 0003): internally only
//! the full uri is used. It prevents sources of
//! bugs.
//!
//! The package is fabricated in memory — never a file — with `X` in versions 1
//! and 2, **titled differently**, so that reading the wrong entry shows. A
//! boundary feature is no representation: `every` then reads what is known of
//! it, which is what carries the title and the whole object (`Known::json`).
//!
//! What would have to break: putting `summary.uuid` back in `every` — both
//! entries then answer with the title of the same one, and the set of titles
//! has one member.

use std::collections::BTreeSet;
use std::io::Cursor;
use std::sync::Arc;

use energyml_build::operation::{Need, Otherwise, Slot, Takes};
use energyml_build::pieces::{Pieces, Source};
use energyml_core::registry::{DynObject, ReadMode};
use energyml_storage::EpcStore;

const RESQML: &str = "http://www.energistics.org/energyml/data/resqmlv2";
const EML: &str = "http://www.energistics.org/energyml/data/commonv2";
const X: &str = "7e100000-0000-4000-8000-000000000001";
const CT: &str = "application/x-resqml+xml;version=2.2;type=BoundaryFeature";

fn feature(version: &str, title: &str) -> Box<dyn DynObject> {
    let xml = format!(
        "<resqml:BoundaryFeature xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" uuid=\"{X}\" \
         schemaVersion=\"2.2\" objectVersion=\"{version}\">\
         <eml:Citation><eml:Title>{title}</eml:Title>\
         <eml:Originator>test</eml:Originator>\
         <eml:Creation>2026-10-05T00:00:00Z</eml:Creation>\
         <eml:Format>test</eml:Format></eml:Citation>\
         <resqml:IsWellKnown>false</resqml:IsWellKnown></resqml:BoundaryFeature>"
    );
    energyml_schemas::all().read_xml_as(CT, xml.as_bytes(), ReadMode::Tolerant).expect("readable").0
}

/// `X` in two versions, titled apart.
fn two_versions() -> EpcStore {
    let empty = energyml_storage::extract::empty_package(Some(0)).expect("an empty package");
    let mut store =
        EpcStore::from_reader(Box::new(Cursor::new(empty)), "versions.epc", Arc::new(energyml_schemas::all())).expect("opens");
    store.put(feature("1", "X, first")).expect("the first");
    store.put_as_new_version(feature("2", "X, second")).expect("the second");
    store
}

fn uris(store: &EpcStore) -> Vec<String> {
    let mut uris: Vec<String> = store.resources().into_iter().map(|one| one.uri).collect();
    uris.sort();
    uris
}

#[test]
fn every_reads_each_version_as_itself() {
    let store = two_versions();
    assert_eq!(uris(&store).len(), 2, "two entries under one uuid");

    let slots = vec![Slot::many("objects", Need::Input(Takes::object(&["resqml22.BoundaryFeature"])), "boundary features", 0)
        .otherwise(Otherwise::Every)];
    let filled =
        energyml_build::read::every(&store, &slots, Pieces::empty(), energyml_geometry::Placement::Local).expect("the objects fill");

    let mut titles = BTreeSet::new();
    let mut seen = 0;
    for (_, source) in filled.iter() {
        let Source::Object(known) = source else { continue };
        seen += 1;
        let json = known.json.as_ref().expect("the object whole").to_string();
        for title in ["X, first", "X, second"] {
            if json.contains(title) {
                titles.insert(title);
            }
        }
        // And the reference it carries is the title of **this** entry.
        let named = known.reference.as_ref().map(|one| one.title.as_str()).expect("a reference");
        assert!(json.contains(named), "the reference ({named}) and the object read ({json}) are the same entry");
    }
    assert_eq!(seen, 2, "one piece per entry");
    assert_eq!(
        titles.into_iter().collect::<Vec<_>>(),
        vec!["X, first", "X, second"],
        "each entry is read as itself — the second is not the first read twice"
    );
}

#[test]
fn a_uri_reads_its_version_and_a_bare_uuid_that_names_two_is_refused() {
    let store = two_versions();
    let all = uris(&store);
    let second = all.iter().find(|one| one.contains("version='2'")).expect("the second's uri").clone();

    let known = energyml_build::read::known_of_object(&store, &second).expect("read by its uri");
    assert_eq!(known.reference.as_ref().map(|one| one.title.as_str()), Some("X, second"));
    let json = known.json.as_ref().expect("the object whole").to_string();
    assert!(json.contains("X, second") && !json.contains("X, first"), "{json}");

    // The uuid alone does not say which: the refusal names both candidates.
    let refused = energyml_build::read::known_of_object(&store, X).expect_err("a bare uuid is ambiguous here");
    for uri in &all {
        assert!(refused.contains(uri.as_str()), "{refused} names {uri}");
    }
}
