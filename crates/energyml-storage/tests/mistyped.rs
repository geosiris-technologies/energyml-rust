//! Packages whose `[Content_Types].xml` does not tell the truth.
//!
//! The index is metadata *about* the parts; the root element is the part. When
//! they disagree, one of them is a typing error made by a producer and the
//! other is the object. The audit of 01/08/2026 found the reader believing the
//! index without ever looking: a package that typed a
//! `TriangulatedSetRepresentation` as a `PointSetRepresentation` came out with
//! an object of the wrong type, plausible and silent - the only place in the
//! whole reader where bad input produced a **wrong answer** rather than a named
//! refusal.
//!
//! Everything here is built in the test rather than taken from `testdata/`,
//! because a corpus of deliberately broken packages is exactly what nobody
//! wants to keep in a repository.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_storage::EpcStore;

const UUID: &str = "1a4112fa-c4ef-4c8d-aed0-47d9273bebc5";

fn registry() -> Arc<Registry> {
    let mut registry = Registry::new();
    energyml_eml_2_3::register(&mut registry);
    energyml_resqml_2_2::register(&mut registry);
    Arc::new(registry)
}

/// A `TriangulatedSetRepresentation`, minimal but valid.
fn object(root: &str) -> String {
    format!(
        "<resqml:{root} \
xmlns:resqml=\"http://www.energistics.org/energyml/data/resqmlv2\" \
xmlns:eml=\"http://www.energistics.org/energyml/data/commonv2\" \
uuid=\"{UUID}\" schemaVersion=\"2.2\">\
<eml:Citation><eml:Title>Fault1 Interp1 TriRep</eml:Title>\
<eml:Originator>test</eml:Originator>\
<eml:Creation>2026-08-01T00:00:00Z</eml:Creation>\
<eml:Format>test</eml:Format></eml:Citation>\
<resqml:SurfaceRole>map</resqml:SurfaceRole>\
</resqml:{root}>"
    )
}

fn content_types(part: &str, declared_type: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Override PartName=\"/{part}\" \
ContentType=\"application/x-resqml+xml;version=2.2;type={declared_type}\"/>\
</Types>"
    )
}

/// Write a one-object package with the content type and root element asked for.
fn package(name: &str, part: &str, declared_type: &str, root: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("energyml-mistyped-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a temporary folder");
    let path = dir.join(format!("{name}.epc"));

    let file = std::fs::File::create(&path).expect("the package is creatable");
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("[Content_Types].xml", options).unwrap();
    zip.write_all(content_types(part, declared_type).as_bytes())
        .unwrap();
    zip.start_file(part, options).unwrap();
    zip.write_all(object(root).as_bytes()).unwrap();
    zip.finish().expect("the package is written");
    path
}

#[test]
fn the_root_element_beats_a_lying_content_type() {
    let path = package(
        "lies",
        &format!("TriangulatedSetRepresentation_{UUID}.xml"),
        // The lie: same domain, same version, a type that also exists and whose
        // fields overlap enough that nothing downstream would look twice.
        "PointSetRepresentation",
        "TriangulatedSetRepresentation",
    );
    let store = EpcStore::open(&path, registry()).expect("the package opens");

    let summary = store
        .resources()
        .into_iter()
        .find(|s| s.uuid == UUID)
        .expect("the object is listed");
    assert_eq!(
        summary.qualified_type, "resqml22.TriangulatedSetRepresentation",
        "the document's own root element decides"
    );

    // Not just the listing: the object that comes out is of that type.
    let object = store.get_object(&summary.uri).expect("it deserializes");
    assert_eq!(object.type_info().name, "TriangulatedSetRepresentation");

    assert!(
        store
            .diagnostics()
            .iter()
            .any(|d| d.message.contains("the root element wins")),
        "and the discrepancy is said out loud: {:?}",
        store.diagnostics()
    );
}

#[test]
fn a_root_element_this_build_cannot_type_leaves_the_declared_type_alone() {
    // The other half of the rule. A root element from a version this build does
    // not generate is not evidence: overriding on it would throw away the only
    // typing there is and drop the part out of the inventory altogether. The
    // declared type stands, and the part stays listed.
    let path = package(
        "unknown-root",
        &format!("TriangulatedSetRepresentation_{UUID}.xml"),
        "TriangulatedSetRepresentation",
        "SomethingFromAFutureVersion",
    );
    let store = EpcStore::open(&path, registry()).expect("the package opens");

    let summary = store
        .resources()
        .into_iter()
        .find(|s| s.uuid == UUID)
        .expect("the object is still listed");
    assert_eq!(summary.qualified_type, "resqml22.TriangulatedSetRepresentation");
}

#[test]
fn the_two_spellings_of_a_root_are_not_a_disagreement() {
    // The reference package writes `obj_DiscreteProperty` in the document where
    // the content type says `DiscreteProperty`. That is one name, not two, and
    // treating it as a contradiction would put a warning on every object of
    // every RESQML 2.0.1 package in the world.
    let path = package(
        "obj-prefix",
        &format!("obj_TriangulatedSetRepresentation_{UUID}.xml"),
        "obj_TriangulatedSetRepresentation",
        "TriangulatedSetRepresentation",
    );
    let store = EpcStore::open(&path, registry()).expect("the package opens");
    assert!(
        !store
            .diagnostics()
            .iter()
            .any(|d| d.message.contains("the root element wins")),
        "{:?}",
        store.diagnostics()
    );
}
