//! The nil UUID is **reported**, never refused.
//!
//! Energistics Identifier Specification v5.0, section 2.3.1 (p. 9): an
//! instance of a business object is identified by a UUID as defined by
//! RFC 4122. RFC 4122 section 4.1.7 defines the nil UUID ("all 128 bits set to
//! zero"), which names no instance: two objects of one type carrying it cannot
//! be told apart. The `UuidString` pattern of the schemas accepts it, so the
//! package is read as before and the departure is said as a warning.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_storage::{EpcStore, Severity};

const NIL: &str = "00000000-0000-0000-0000-000000000000";
const ORDINARY: &str = "7b2f19a4-1c33-4d55-9f01-2ab7c6e40188";
const RESQML: &str = "http://www.energistics.org/energyml/data/resqmlv2";
const EML: &str = "http://www.energistics.org/energyml/data/commonv2";

fn registry() -> Arc<Registry> {
    let mut registry = Registry::new();
    energyml_eml_2_3::register(&mut registry);
    energyml_resqml_2_2::register(&mut registry);
    Arc::new(registry)
}

/// A package holding one `BoundaryFeature` with the uuid asked for.
fn package(name: &str, uuid: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("energyml-nil-uuid-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a temporary folder");
    let path = dir.join(format!("{name}.epc"));

    let file = std::fs::File::create(&path).expect("creatable");
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("[Content_Types].xml", options).unwrap();
    zip.write_all(
        b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
          <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
          <Default Extension=\"rels\" \
          ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
          </Types>",
    )
    .unwrap();
    zip.start_file(format!("Part_{uuid}.xml"), options).unwrap();
    let xml = format!(
        "<resqml:BoundaryFeature xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" \
         uuid=\"{uuid}\" schemaVersion=\"2.2\">\
         <eml:Citation><eml:Title>a boundary</eml:Title>\
         <eml:Originator>test</eml:Originator>\
         <eml:Creation>2026-08-31T00:00:00Z</eml:Creation>\
         <eml:Format>test</eml:Format></eml:Citation>\
         <resqml:IsWellKnown>false</resqml:IsWellKnown></resqml:BoundaryFeature>"
    );
    zip.write_all(xml.as_bytes()).unwrap();
    zip.finish().expect("written");
    path
}

fn nil_warnings(store: &EpcStore) -> Vec<energyml_storage::Diagnostic> {
    store
        .diagnostics()
        .into_iter()
        .filter(|d| d.message.contains("nil UUID"))
        .collect()
}

/// A part with the nil UUID opens, is listed and read; one warning names the part.
///
/// Break it: drop the `is_nil_uuid` check in `build_inventory`.
#[test]
fn a_part_with_the_nil_uuid_is_read_and_warned_about() {
    let store = EpcStore::open(package("nil", NIL), registry()).expect("the package opens");
    let summary = store
        .resources()
        .into_iter()
        .find(|s| s.uuid == NIL)
        .expect("the object is listed");
    store.get_object(&summary.uri).expect("the object is read");

    let warnings = nil_warnings(&store);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(warnings[0].severity, Severity::Warning);
    assert_eq!(warnings[0].part, format!("Part_{NIL}.xml"));
}

/// An ordinary uuid is not mentioned.
#[test]
fn an_ordinary_uuid_produces_no_nil_warning() {
    let store = EpcStore::open(package("ordinary", ORDINARY), registry()).expect("the package opens");
    assert!(nil_warnings(&store).is_empty());
}
