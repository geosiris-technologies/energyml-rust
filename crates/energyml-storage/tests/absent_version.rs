//! **A version the project ships and this build does not carry is a named
//! refusal, never a half read** (plan n°113).
//!
//! The workspace build has every schema feature on, so a narrow build is
//! simulated by hand: a registry holding RESQML 2.2 and EML 2.3 only, with
//! RESQML 2.0.1 declared absent (`Registry::declare_absent`) the way
//! `energyml_schemas::all()` does for a feature that is off. The real narrow
//! builds are run by `crates/energyml/tests/narrow.rs`.
//!
//! The package is fabricated in memory: one `BoundaryFeature` written the way a
//! RESQML 2.0.1 producer writes it (`obj_` root, `schemaVersion="2.0"`). It is
//! the case that hurts: RESQML 2.2 *can* read it — its one extra required
//! element is defaulted by a tolerant read — so without the refusal the part
//! comes back as a plausible 2.2 object.

use std::io::{Cursor, Write};
use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_storage::{EpcStore, Error};

const UUID: &str = "3c9a41d2-7b05-4e8a-9d17-5f20c8b6a914";
const RESQML: &str = "http://www.energistics.org/energyml/data/resqmlv2";
const EML: &str = "http://www.energistics.org/energyml/data/commonv2";
const WHY: &str = "RESQML 2.0.1 is not compiled in: enable the `resqml20` feature of energyml-schemas (or of energyml)";

/// RESQML 2.2 and EML 2.3 only; RESQML 2.0.1 declared absent when asked.
fn registry(declare_absent: bool) -> Arc<Registry> {
    let mut registry = Registry::new();
    energyml_eml_2_3::register(&mut registry);
    energyml_resqml_2_2::register(&mut registry);
    if declare_absent {
        registry.declare_absent("resqml", "2.0.1", &["20", "201"], WHY);
    }
    Arc::new(registry)
}

/// One part, as a 2.0.1 producer writes it, declared by `content_type` when
/// given (an `Override`), left undeclared when `None`.
fn package(root: &str, schema_version: &str, content_type: Option<&str>) -> Vec<u8> {
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let part = format!("{root}_{UUID}.xml");
    let overrides = content_type
        .map(|ct| format!("<Override PartName=\"/{part}\" ContentType=\"{ct}\"/>"))
        .unwrap_or_default();
    zip.start_file("[Content_Types].xml", options).unwrap();
    zip.write_all(
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
             <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
             <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
             {overrides}</Types>"
        )
        .as_bytes(),
    )
    .unwrap();
    zip.start_file(part.clone(), options).unwrap();
    // An empty `schema_version` leaves the attribute out: a document that
    // declares no version.
    let schema_attribute =
        if schema_version.is_empty() { String::new() } else { format!(" schemaVersion=\"{schema_version}\"") };
    zip.write_all(
        format!(
            "<resqml:{root} xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" uuid=\"{UUID}\"\
             {schema_attribute}>\
             <eml:Citation><eml:Title>a boundary</eml:Title><eml:Originator>test</eml:Originator>\
             <eml:Creation>2026-10-13T00:00:00Z</eml:Creation><eml:Format>test</eml:Format></eml:Citation>\
             </resqml:{root}>"
        )
        .as_bytes(),
    )
    .unwrap();
    zip.finish().unwrap().into_inner()
}

fn open(bytes: Vec<u8>, registry: Arc<Registry>) -> EpcStore {
    EpcStore::from_reader(Box::new(Cursor::new(bytes)), "absent.epc", registry).expect("the package opens")
}

fn uri_of(store: &EpcStore) -> String {
    store
        .resources()
        .into_iter()
        .find(|s| s.uuid == UUID)
        .expect("a refused part is still listed: a listing that hides rows lies about the file")
        .uri
}

fn assert_refused(store: &EpcStore, what: &str) {
    match store.get_object(&uri_of(store)) {
        Err(Error::Core(energyml_core::error::Error::VersionAbsent(message))) => {
            assert_eq!(message, WHY, "{what}");
            assert!(message.contains("resqml20"), "the message names the feature: {message}");
        }
        Err(other) => panic!("{what}: refused, but not by name: {other}"),
        Ok(object) => panic!("{what}: read as `{}` {}, a half read", object.type_info().package, object.type_info().version),
    }
    assert!(
        store.diagnostics().iter().any(|d| d.message.contains("resqml20")),
        "{what}: the refusal is also recorded on the part"
    );
}

/// Declared by content type, in both spellings of the version: refused, listed,
/// and not read as RESQML 2.2.
///
/// What to break: delete the `refusal` computation in `classify` (the content
/// type path) and the substitution reads the part as 2.2 — `Ok` instead of the
/// refusal.
#[test]
fn a_part_of_an_absent_version_is_refused_by_name_not_read_as_another() {
    for version in ["2.0", "2.0.1"] {
        let content_type = format!("application/x-resqml+xml;version={version};type=obj_BoundaryFeature");
        let store = open(package("obj_BoundaryFeature", "2.0", Some(&content_type)), registry(true));
        assert_refused(&store, &content_type);
    }
}

/// The same part with no entry in `[Content_Types].xml`: the version comes from
/// the document (its namespace and `schemaVersion`), and the refusal holds.
///
/// What to break: delete the `absent_declaration` call in the root-element path
/// of `classify`; the best candidate is then 2.2.
#[test]
fn an_undeclared_part_of_an_absent_version_is_refused_too() {
    let store = open(package("obj_BoundaryFeature", "2.0", None), registry(true));
    assert_refused(&store, "undeclared part");
}

/// The control, and the reason the test above means something: the very same
/// package, with nothing declared absent, **is** read as RESQML 2.2 (the
/// substitution built for versions the project does not ship). If this ever
/// refuses, the first test no longer proves the refusal comes from the table.
///
/// What to break: make `Registry::absent_content_type` answer `Some` always,
/// and this one fails.
#[test]
fn without_the_declaration_the_same_part_is_substituted() {
    let content_type = "application/x-resqml+xml;version=2.0;type=obj_BoundaryFeature";
    let store = open(package("obj_BoundaryFeature", "2.0", Some(content_type)), registry(false));
    let object = store.get_object(&uri_of(&store)).expect("read as another release of the package");
    assert_eq!(object.type_info().version, "2.2");
    assert!(
        store.diagnostics().iter().any(|d| d.message.contains("not registered in this build")),
        "the substitution is said out loud"
    );
}

/// A version the project does not know at all keeps the old path even when
/// another version is declared absent: the change is not a blanket refusal.
///
/// What to break: make `Registry::absent` ignore the version (match on the
/// package alone) and `2.0.9` is refused as RESQML 2.0.1.
#[test]
fn a_version_nobody_declared_still_takes_the_substitution() {
    let content_type = "application/x-resqml+xml;version=2.0.9;type=BoundaryFeature";
    let store = open(package("BoundaryFeature", "2.0.9", Some(content_type)), registry(true));
    let object = store.get_object(&uri_of(&store)).expect("an unknown version is substituted, as before");
    assert_eq!(object.type_info().version, "2.2");
}

/// Plan n°119: a content type that is **registered** (`version=2.2`) over a
/// document that declares the absent `schemaVersion="2.0"` is a contradiction
/// toward an absent release. The document is the part: refused by name, listed,
/// and never read as 2.2.
///
/// What to break: delete the `absent_declaration` block that follows the
/// `refusal` computation in the content-type path of `classify`; the index wins
/// and the part comes back as a RESQML 2.2 object (`Ok`).
#[test]
fn a_registered_content_type_over_an_absent_document_version_is_refused() {
    for version in ["2.0", "2.0.1"] {
        let content_type = "application/x-resqml+xml;version=2.2;type=BoundaryFeature";
        let store = open(package("BoundaryFeature", version, Some(content_type)), registry(true));
        assert_refused(&store, &format!("2.2 index over schemaVersion `{version}`"));
        let listed = store.resources().into_iter().find(|s| s.uuid == UUID).unwrap();
        assert!(
            listed.qualified_type.starts_with("resqml20"),
            "the listing names what the document is, not what the index said: {}",
            listed.qualified_type
        );
    }
}

/// The control for the test above: the same 2.2 content type over a document
/// declaring **no** version still follows the index, and a 2.2 document under
/// a 2.2 content type reads. Without it the refusal could be a blanket one.
///
/// Today the first reads as RESQML 2.2 (the index is trusted, nothing
/// contradicts it).
///
/// What to break: drop the `peeked.schema_version` requirement from
/// `absent_declaration` (answer for a bare namespace) and the first assertion
/// fails.
#[test]
fn a_registered_content_type_over_a_silent_or_matching_document_still_reads() {
    let content_type = "application/x-resqml+xml;version=2.2;type=BoundaryFeature";
    let silent = open(package("BoundaryFeature", "", Some(content_type)), registry(true));
    let object = silent.get_object(&uri_of(&silent)).expect("no declaration, the index stands");
    assert_eq!(object.type_info().version, "2.2");

    let matching = open(package("BoundaryFeature", "2.2", Some(content_type)), registry(true));
    let object = matching.get_object(&uri_of(&matching)).expect("a 2.2 document under a 2.2 content type");
    assert_eq!(object.type_info().version, "2.2");
}

/// The registered version is untouched by the declaration: a RESQML 2.2 part
/// still reads as RESQML 2.2 in the registry that declares 2.0.1 absent.
///
/// What to break: declare the wrong row (e.g. `"2.2"`) in `registry(true)`.
#[test]
fn a_registered_version_is_read_as_itself() {
    let content_type = "application/x-resqml+xml;version=2.2;type=BoundaryFeature";
    let store = open(package("BoundaryFeature", "2.2", Some(content_type)), registry(true));
    let object = store.get_object(&uri_of(&store)).expect("registered");
    assert_eq!(object.type_info().version, "2.2");
}
