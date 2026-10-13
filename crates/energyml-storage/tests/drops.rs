//! A **development drop** beside the release it precedes.
//!
//! Energistics ships `resqml 2.2_dev3` next to `resqml 2.2`, and files written
//! before 2.2 was final are out there. The awkward part is that the drop's own
//! schemas declare the *release's* version - `v2.2_dev3/ResqmlAllObjects.xsd`
//! says `version="2.2"` - so a drop document and a released 2.2 document carry
//! the same namespace, the same `schemaVersion`, the same content type and the
//! same qualified type. Nothing on the wire separates them.
//!
//! That leaves the answer the releases already use: return every candidate,
//! best first, and let the parser settle it by *refusing* the wrong one. What
//! the drop adds is an order - the release is tried before its drops, a
//! document that names a drop gets that drop first - and a rule in
//! `Registry::register` so the shared qualified type keeps resolving to the
//! release.
//!
//! `BoundaryFeature` is the discriminator throughout: RESQML 2.2 requires an
//! `IsWellKnown` element and the drop has no such field, so a document without
//! it refuses as 2.2 and reads as the drop. With it, **both** parse - which is
//! what makes the ordering tests mean something, since then only the order can
//! decide.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_storage::EpcStore;

const UUID: &str = "7b2f19a4-1c33-4d55-9f01-2ab7c6e40188";
const RESQML: &str = "http://www.energistics.org/energyml/data/resqmlv2";
const EML: &str = "http://www.energistics.org/energyml/data/commonv2";

/// The release and its drop, both registered - the situation every test here
/// is about. The drop is registered *first*, as `energyml_schemas::all` does.
fn registry() -> Arc<Registry> {
    let mut registry = Registry::new();
    energyml_eml_2_2::register(&mut registry);
    energyml_eml_2_3::register(&mut registry);
    energyml_resqml_2_2_dev3::register(&mut registry);
    energyml_resqml_2_2::register(&mut registry);
    Arc::new(registry)
}

/// One `BoundaryFeature`, with the two signals these tests vary: what its
/// `schemaVersion` says, and whether the element RESQML 2.2 requires is there.
fn xml(schema: Option<&str>, well_known: bool) -> String {
    let schema = schema
        .map(|v| format!(" schemaVersion=\"{v}\""))
        .unwrap_or_default();
    // Required by 2.2, absent from the drop's schema entirely. Unknown children
    // are skipped rather than rejected, so its presence never stops the drop
    // reading - it only stops the *release* refusing.
    let flag = if well_known {
        "<resqml:IsWellKnown>false</resqml:IsWellKnown>"
    } else {
        ""
    };
    format!(
        "<resqml:BoundaryFeature xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" \
         uuid=\"{UUID}\"{schema}>\
         <eml:Citation><eml:Title>a boundary</eml:Title>\
         <eml:Originator>test</eml:Originator>\
         <eml:Creation>2026-08-31T00:00:00Z</eml:Creation>\
         <eml:Format>test</eml:Format></eml:Citation>\
         {flag}</resqml:BoundaryFeature>"
    )
}

/// A package holding that one part, **not** declared in
/// `[Content_Types].xml`, so the identification has to come from the document.
fn package(name: &str, schema: Option<&str>, well_known: bool) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("energyml-drops-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a temporary folder");
    let path = dir.join(format!("{name}.epc"));

    let file = std::fs::File::create(&path).expect("creatable");
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::FileOptions<'_, ()> =
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
    zip.start_file(format!("Part_{UUID}.xml"), options).unwrap();
    zip.write_all(xml(schema, well_known).as_bytes()).unwrap();
    zip.finish().expect("written");
    path
}

/// Which package version actually parsed it.
///
/// The qualified type cannot answer: the drop and the release both compute
/// `resqml22.BoundaryFeature`. The object's own `TypeInfo` can.
fn read_as(name: &str, schema: Option<&str>, well_known: bool) -> String {
    let store = EpcStore::open(package(name, schema, well_known), registry()).expect("the package opens");
    let summary = store
        .resources()
        .into_iter()
        .find(|s| s.uuid == UUID)
        .unwrap_or_else(|| panic!("{name}: the object is not listed at all"));
    let object = store
        .get_object(&summary.uri)
        .unwrap_or_else(|e| panic!("{name}: nothing reads it: {e}"));
    object.type_info().version.to_string()
}

// ---------------------------------------------------------------------------

/// A document saying `2.2` is a released 2.2, and is read as one.
///
/// `IsWellKnown` is present, so the drop would parse it too - the drop skips
/// the element it does not know. Only the order decides, which is the point.
#[test]
fn a_document_saying_the_release_is_read_as_the_release() {
    assert_eq!(read_as("release", Some("2.2"), true), "2.2");
}

/// …and when the release refuses, the drop gets its turn.
///
/// This is the walk the ladder exists for: `IsWellKnown` is absent, RESQML 2.2
/// requires it, so the first candidate fails on the strict pass and the second
/// answers. Nothing here guesses - the drop is reached only because the release
/// said no.
#[test]
fn a_document_the_release_refuses_falls_through_to_the_drop() {
    let path = package("fallback", Some("2.2"), false);
    let store = EpcStore::open(&path, registry()).expect("the package opens");
    let summary = store
        .resources()
        .into_iter()
        .find(|s| s.uuid == UUID)
        .expect("listed");
    let object = store.get_object(&summary.uri).expect("something reads it");
    assert_eq!(
        object.type_info().version,
        "2.2_dev3",
        "the release requires IsWellKnown and this document has none"
    );
    assert!(
        !store.diagnostics().is_empty(),
        "a version this build fell back to is said out loud, never silently substituted"
    );
}

/// A document that **names** the drop is read as the drop, first.
///
/// `IsWellKnown` is present here, so the release would parse it happily. If the
/// declaration were ignored this would come back `2.2`; that it comes back the
/// drop is the whole of what "explicitly 2.2dev3 tries the drop first" means.
#[test]
fn a_document_naming_the_drop_is_read_as_the_drop() {
    assert_eq!(read_as("named", Some("2.2_dev3"), true), "2.2_dev3");
}

/// However the drop is spelled.
///
/// The directory writes `2.2_dev3` and nothing obliges a producer to agree, so
/// the separator and the case are ignored. A spelling that missed would send a
/// drop document to the release - which, on a document the release can parse,
/// fails nothing and returns the wrong object.
#[test]
fn the_drop_is_recognised_however_it_is_spelled() {
    for (name, spelling) in [
        ("underscore", "2.2_dev3"),
        ("bare", "2.2dev3"),
        ("hyphen", "2.2-dev3"),
        ("upper", "2.2_DEV3"),
        ("labelled", "Resqml V2.2_dev3"),
    ] {
        assert_eq!(
            read_as(name, Some(spelling), true),
            "2.2_dev3",
            "`{spelling}` names the drop"
        );
    }
}

/// A different drop of the same release is not this one.
///
/// `dev2` is not registered, so the release answers - and that is the right
/// answer, not a silent promotion of `dev3` to "close enough".
#[test]
fn an_unknown_drop_falls_back_to_the_release_not_to_another_drop() {
    assert_eq!(read_as("dev2", Some("2.2_dev2"), true), "2.2");
}

/// Relabelling a mislabelled part is what makes it readable as the drop.
///
/// The loop this closes, end to end. A part written against the drop but
/// declaring the release is read as the *release* - and the release succeeds,
/// skipping what it does not know, so nothing refuses and nothing falls back.
/// `relabel::set_schema_version` edits the declaration without parsing the
/// document, and the same bytes then read as the drop.
///
/// `IsWellKnown` is present throughout, so the release can parse it at every
/// step: this test is about the declaration deciding, not about a refusal.
#[test]
fn relabelling_the_declaration_is_what_sends_a_part_to_the_drop() {
    use energyml_storage::relabel::{Relabel, set_schema_version};

    let before = xml(Some("2.2"), true);
    let (after, what) =
        set_schema_version(before.as_bytes(), "2.2_dev3").expect("the declaration is rewritten");
    assert_eq!(
        what,
        Relabel::Replaced {
            was: "2.2".to_string()
        }
    );

    // The edit touched the declaration and nothing else.
    assert_eq!(
        String::from_utf8(after.clone())
            .expect("utf-8")
            .replace("2.2_dev3", "2.2"),
        before,
        "only the schemaVersion changed"
    );

    // And that one attribute is the whole difference in what gets read.
    assert_eq!(
        read_as("before-relabel", Some("2.2"), true),
        "2.2",
        "declaring the release sends it to the release"
    );
    assert_eq!(
        read_as("after-relabel", Some("2.2_dev3"), true),
        "2.2_dev3",
        "declaring the drop sends it to the drop"
    );
}

/// A package that **declares** its content types, which is the ordinary shape.
///
/// `package` above deliberately declares none, so identification has to come
/// from the document. Here the index is present and complete - and that is
/// exactly the case where a drop used to be lost.
fn package_declaring(name: &str, schema: Option<&str>, well_known: bool) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("energyml-drops-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a temporary folder");
    let path = dir.join(format!("{name}.epc"));

    let file = std::fs::File::create(&path).expect("creatable");
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let part = format!("BoundaryFeature_{UUID}.xml");
    zip.start_file("[Content_Types].xml", options).unwrap();
    zip.write_all(
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>             <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">             <Default Extension=\"rels\"              ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>             <Override PartName=\"/{part}\"              ContentType=\"application/x-resqml+xml;version=2.2;type=BoundaryFeature\"/>             </Types>"
        )
        .as_bytes(),
    )
    .unwrap();
    zip.start_file(&part, options).unwrap();
    zip.write_all(xml(schema, well_known).as_bytes()).unwrap();
    zip.finish().expect("written");
    path
}

/// **A content type cannot name a drop, so it must not settle the version.**
///
/// `application/x-resqml+xml;version=2.2;type=BoundaryFeature` is what the
/// released 2.2 *and* `2.2_dev3` both write - the drop's schemas declare the
/// release's own version. So when the index resolves, it resolves to the
/// release, and until this was fixed the part's own `schemaVersion` was never
/// consulted: the candidate walk had one entry and the drop never got a turn.
///
/// Measured on a real 1344-object package before the fix: 2749 validation
/// errors, all of them `QualifiedType` failing its pattern on an empty string,
/// because eml 2.2 references were being read as eml 2.3. Afterwards: none.
///
/// `IsWellKnown` is the discriminator and it is **absent** here - the release
/// requires it, the drop has no such field. Read as the release it comes back
/// invented; read as the drop it does not exist.
#[test]
fn a_declared_content_type_does_not_override_a_drop_the_document_names() {
    let path = package_declaring("declared-drop", Some("2.2_dev3"), false);
    let store = EpcStore::open(&path, registry()).expect("the package opens");
    let summary = store
        .resources()
        .into_iter()
        .find(|s| s.uuid == UUID)
        .expect("listed");
    let object = store.get_object(&summary.uri).expect("something reads it");
    assert_eq!(
        object.type_info().version,
        "2.2_dev3",
        "the index says 2.2 because it cannot say anything else; the document says the drop"
    );
}

/// …and a package that names no drop is still decided by its index.
///
/// The guard above must not fire on the ordinary case, or every released 2.2
/// part in every package would start consulting candidates it does not need.
#[test]
fn a_declared_content_type_still_decides_when_no_drop_is_named() {
    assert_eq!(
        {
            let path = package_declaring("declared-release", Some("2.2"), true);
            let store = EpcStore::open(&path, registry()).expect("opens");
            let summary = store
                .resources()
                .into_iter()
                .find(|s| s.uuid == UUID)
                .expect("listed");
            store.get_object(&summary.uri).expect("reads").type_info().version
        },
        "2.2"
    );
}
