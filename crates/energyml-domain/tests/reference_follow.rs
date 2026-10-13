//! Following a reference reads **the version it names** — `ObjectSource::object_of`,
//! the one way a reader of the library goes from a `DataObjectReference` to its
//! target (ADR-0003).
//!
//! The package holds a feature in two versions; one horizon names version 2,
//! another names no version. The first must come back at version 2; the second
//! names an object the package holds twice, so it is refused rather than
//! guessed — the rule being: accept the short uri when there is no conflict,
//! with a version when there is one.
//!
//! What would have to break: `object_of` asking the store for the reference's
//! uuid alone — the store holds two objects under it, so the horizon naming
//! version 2 would come back with nothing.

use std::io::Cursor;
use std::sync::Arc;

use energyml_core::identity::{object_version_of_reflect, uri_of_reflect};
use energyml_core::registry::{DynObject, ReadMode};
use energyml_domain::source::ObjectSource;
use energyml_storage::EpcStore;

const RESQML: &str = "http://www.energistics.org/energyml/data/resqmlv2";
const EML: &str = "http://www.energistics.org/energyml/data/commonv2";
const S: &str = "7f000000-0000-4000-8000-000000000101";
const NAMES_TWO: &str = "7f000000-0000-4000-8000-0000000001a1";
const NAMES_NONE: &str = "7f000000-0000-4000-8000-0000000001a2";
const CT: &str = "application/x-resqml+xml;version=2.2;type=BoundaryFeature";
const CT_H: &str = "application/x-resqml+xml;version=2.2;type=HorizonInterpretation";

fn feature(version: Option<&str>) -> String {
    let version = version.map(|v| format!(" objectVersion=\"{v}\"")).unwrap_or_default();
    format!(
        "<resqml:BoundaryFeature xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" uuid=\"{S}\" \
         schemaVersion=\"2.2\"{version}><eml:Citation><eml:Title>a surface</eml:Title>\
         <eml:Originator>test</eml:Originator><eml:Creation>2026-10-06T00:00:00Z</eml:Creation>\
         <eml:Format>test</eml:Format></eml:Citation><resqml:IsWellKnown>false</resqml:IsWellKnown>\
         </resqml:BoundaryFeature>"
    )
}

fn horizon(uuid: &str, version: Option<&str>) -> String {
    let version = version.map(|v| format!("<eml:ObjectVersion>{v}</eml:ObjectVersion>")).unwrap_or_default();
    format!(
        "<resqml:HorizonInterpretation xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" uuid=\"{uuid}\" \
         schemaVersion=\"2.2\"><eml:Citation><eml:Title>a horizon</eml:Title>\
         <eml:Originator>test</eml:Originator><eml:Creation>2026-10-06T00:00:00Z</eml:Creation>\
         <eml:Format>test</eml:Format></eml:Citation><resqml:Domain>depth</resqml:Domain>\
         <resqml:InterpretedFeature><eml:Uuid>{S}</eml:Uuid>{version}\
         <eml:QualifiedType>resqml22.BoundaryFeature</eml:QualifiedType><eml:Title>a surface</eml:Title>\
         </resqml:InterpretedFeature></resqml:HorizonInterpretation>"
    )
}

fn object(content_type: &str, xml: String) -> Box<dyn DynObject> {
    energyml_schemas::all().read_xml_as(content_type, xml.as_bytes(), ReadMode::Tolerant).expect("readable").0
}

fn package() -> EpcStore {
    let registry = Arc::new(energyml_schemas::all());
    let empty = energyml_storage::extract::empty_package(Some(0)).expect("an empty package");
    let mut store = EpcStore::from_reader(Box::new(Cursor::new(empty)), "follow.epc", registry).expect("opens");
    store.put(object(CT, feature(Some("1")))).expect("version 1");
    store.put_as_new_version(object(CT, feature(Some("2")))).expect("version 2");
    store.put(object(CT_H, horizon(NAMES_TWO, Some("2")))).expect("a horizon naming version 2");
    store.put(object(CT_H, horizon(NAMES_NONE, None))).expect("a horizon naming no version");
    store
}

/// The `InterpretedFeature` reference of a horizon, handed to `check`.
fn with_reference(store: &EpcStore, horizon: &str, check: impl FnOnce(&dyn energyml_core::value::Reflect)) {
    let object = store.object(horizon).expect("the horizon");
    let reference = object.as_reflect().field("InterpretedFeature").expect("the field");
    check(reference.as_object().expect("a reference"));
}

#[test]
fn a_reference_that_names_a_version_reads_that_version() {
    let store = package();
    with_reference(&store, NAMES_TWO, |reference| {
        let uri = uri_of_reflect(reference).expect("a uri");
        assert!(uri.contains("version='2'"), "the uri carries the version the reference names: {uri}");
        let target = store.object_of(reference).expect("the target");
        assert_eq!(object_version_of_reflect(target.as_reflect()), Some("2"), "version 2, never version 1");
    });
}

#[test]
fn a_reference_without_a_version_to_an_object_held_twice_is_refused_not_guessed() {
    let store = package();
    with_reference(&store, NAMES_NONE, |reference| {
        assert!(store.object_of(reference).is_none(), "two candidates: no guess");
        let why = store.read_object_of(reference).err().expect("a reason");
        assert!(!why.is_empty(), "the refusal says why");
    });
}
