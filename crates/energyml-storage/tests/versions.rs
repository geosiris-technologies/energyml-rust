//! Which **version** a part is, when the namespace cannot say.
//!
//! Energistics gave every version of a package the same namespace:
//! `resqmlv2` is RESQML 2.0.1 *and* RESQML 2.2, `witsmlv2` is WITSML 2.0 *and*
//! 2.1. So the namespace answers "which package" - worth something, since
//! `Activity` exists in both `resqml20` and `eml23` under different namespaces -
//! and answers nothing about the release.
//!
//! The release comes from `schemaVersion`, and that is thin: the attribute is
//! optional in practice, producers write `2.0` where the release is 2.0.1, and
//! a file may name a version this build does not carry. Until 02/08 an unusable
//! `schemaVersion` fell through to whichever candidate the registry happened to
//! hold first - silently. Reading a part as the wrong version does not fail: it
//! yields a plausible object carrying the fields the two releases share and
//! nothing else, which is the one outcome this reader exists to prevent.
//!
//! So the identification returns **every** candidate, best first, and the read
//! walks the list until one parses. These tests pin the order and the walk.
//!
//! `FaultInterpretation` is the discriminator throughout: RESQML 2.0.1 requires
//! a `Domain` element and 2.2 does not, so one document without `Domain` reads
//! as 2.2 and refuses as 2.0.1. That asymmetry is the only way a parser can
//! settle a version question, and it is why the fallback is a *walk* rather
//! than a better guess.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_storage::EpcStore;

const UUID: &str = "1a4112fa-c4ef-4c8d-aed0-47d9273bebc5";
const RESQML: &str = "http://www.energistics.org/energyml/data/resqmlv2";
const COMMON: &str = "http://www.energistics.org/energyml/data/commonv2";

fn registry() -> Arc<Registry> {
    let mut registry = Registry::new();
    energyml_eml_2_0::register(&mut registry);
    energyml_eml_2_3::register(&mut registry);
    energyml_resqml_2_0_1::register(&mut registry);
    energyml_resqml_2_2::register(&mut registry);
    Arc::new(registry)
}

/// One object, with exactly the version signals the test wants to give.
///
/// `root` may carry the `obj_` prefix; `schema` may be absent; `namespace`
/// decides the package; `domain` decides whether RESQML 2.0.1 can read it.
struct Doc {
    root: &'static str,
    namespace: &'static str,
    schema: Option<&'static str>,
    domain: bool,
}

impl Doc {
    fn xml(&self) -> String {
        let schema = match self.schema {
            Some(version) => format!(" schemaVersion=\"{version}\""),
            None => String::new(),
        };
        let domain = if self.domain {
            "<resqml:Domain>depth</resqml:Domain>"
        } else {
            ""
        };
        // Required by *both* releases, so it is always there and never the
        // thing that tells them apart. Written with the reference spelling of
        // each - eml 2.0 wants `ContentType`, eml 2.3 wants `QualifiedType` -
        // because a reader skips the element it does not know and would
        // otherwise fail for a reason this test is not about.
        let feature = "<resqml:InterpretedFeature>\
<eml:ContentType>application/x-resqml+xml;version=2.0;type=obj_TectonicBoundaryFeature</eml:ContentType>\
<eml:QualifiedType>resqml22.TectonicBoundaryFeature</eml:QualifiedType>\
<eml:Title>a fault</eml:Title>\
<eml:UUID>3d5f8c21-0000-4000-8000-000000000001</eml:UUID>\
<eml:Uuid>3d5f8c21-0000-4000-8000-000000000001</eml:Uuid>\
</resqml:InterpretedFeature>";
        format!(
            "<resqml:{root} xmlns:resqml=\"{ns}\" \
xmlns:eml=\"http://www.energistics.org/energyml/data/commonv2\" \
uuid=\"{UUID}\"{schema}>\
<eml:Citation><eml:Title>a fault</eml:Title>\
<eml:Originator>test</eml:Originator>\
<eml:Creation>2026-08-02T00:00:00Z</eml:Creation>\
<eml:Format>test</eml:Format></eml:Citation>\
{feature}{domain}\
</resqml:{root}>",
            root = self.root,
            ns = self.namespace,
            feature = if self.root.ends_with("FaultInterpretation") {
                feature
            } else {
                ""
            },
        )
    }
}

/// A package holding one part, **not** declared in `[Content_Types].xml`, so
/// the identification has to come from the document itself.
fn package(name: &str, doc: &Doc) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("energyml-versions-{}-{}", name, std::process::id()));
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
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
</Types>",
    )
    .unwrap();
    zip.start_file(format!("Part_{UUID}.xml"), options).unwrap();
    zip.write_all(doc.xml().as_bytes()).unwrap();
    zip.finish().expect("written");
    path
}

fn typed_as(name: &str, doc: &Doc) -> String {
    let store = EpcStore::open(package(name, doc), registry()).expect("the package opens");
    store
        .resources()
        .into_iter()
        .find(|s| s.uuid == UUID)
        .unwrap_or_else(|| panic!("{name}: the object is not listed at all"))
        .qualified_type
}

// ---------------------------------------------------------------------------

/// `schemaVersion` decides, in both directions.
#[test]
fn the_schema_version_picks_the_release() {
    let doc = |schema| Doc {
        root: "FaultInterpretation",
        namespace: RESQML,
        schema: Some(schema),
        domain: true,
    };
    assert_eq!(typed_as("v20", &doc("2.0")), "resqml20.obj_FaultInterpretation");
    assert_eq!(typed_as("v22", &doc("2.2")), "resqml22.FaultInterpretation");
}

/// `schemaVersion="2.0.1"` finds RESQML 2.0.1, whose own `schemaVersion` is
/// `2.0`.
///
/// The schema's rule - *"the schema version is the first 2 digits of an ML
/// version"* - means the two forms have to meet, and they meet on the dotted
/// components rather than on the string, so `2.1` must not match `2.10`.
#[test]
fn a_release_number_and_a_schema_version_meet() {
    assert_eq!(
        typed_as(
            "v201",
            &Doc {
                root: "FaultInterpretation",
                namespace: RESQML,
                schema: Some("2.0.1"),
                domain: true,
            }
        ),
        "resqml20.obj_FaultInterpretation"
    );
}

/// The namespace settles the **package**, which is the half `schemaVersion`
/// cannot do: `Activity` exists in RESQML 2.0.1 and in eml 2.3.
#[test]
fn the_namespace_settles_the_package() {
    let doc = |namespace| Doc {
        root: "Activity",
        namespace,
        schema: None,
        domain: false,
    };
    assert_eq!(typed_as("ns-eml", &doc(COMMON)), "eml23.Activity");
    assert!(
        typed_as("ns-resqml", &doc(RESQML)).starts_with("resqml20."),
        "the same element name in the other namespace is the other package"
    );
}

/// With no `schemaVersion` at all, the `obj_` prefix is the only thing the
/// document said about its age - and only the older releases write it.
///
/// A **positive** signal when present and no signal at all when absent:
/// `ALWYN-RESQML.epc` is a RESQML 2.0.1 package whose roots do not carry it,
/// which is why its absence must not be read as "therefore 2.2".
#[test]
fn the_obj_prefix_is_a_signal_when_nothing_else_is() {
    assert_eq!(
        typed_as(
            "obj",
            &Doc {
                root: "obj_FaultInterpretation",
                namespace: RESQML,
                schema: None,
                domain: true,
            }
        ),
        "resqml20.obj_FaultInterpretation"
    );
}

/// The fallback: `schemaVersion` says 2.0, the document is not 2.0, and the
/// read finds that out by trying.
///
/// RESQML 2.0.1 requires `Domain` on a `FaultInterpretation` and 2.2 does not,
/// so this document cannot be what it claims to be. The identification still
/// puts 2.0.1 first - believing the file is the right default - and the read
/// walks past it to the version that parses, saying so.
#[test]
fn a_version_that_will_not_parse_falls_through_to_one_that_will() {
    let path = package(
        "fallback",
        &Doc {
            root: "FaultInterpretation",
            namespace: RESQML,
            schema: Some("2.0"),
            domain: false,
        },
    );
    let store = EpcStore::open(&path, registry()).expect("the package opens");
    let summary = store
        .resources()
        .into_iter()
        .find(|s| s.uuid == UUID)
        .expect("listed");
    assert_eq!(
        summary.qualified_type, "resqml20.obj_FaultInterpretation",
        "the listing reports what the file claims"
    );

    let object = store.get_object(&summary.uri).expect("something reads it");
    assert_eq!(
        object.type_info().qualified_type,
        Some("resqml22.FaultInterpretation"),
        "and the object is the version that actually parses"
    );
    assert!(
        store.diagnostics().iter().any(|d| d
            .message
            .contains("read as `resqml22.FaultInterpretation` instead")),
        "the substitution is said out loud: {:?}",
        store.diagnostics()
    );
}

/// An ambiguous identification says so before anything is read.
///
/// The note is the point: a reader that picked one and returned a plausible
/// object would leave nobody anything to notice.
#[test]
fn an_ambiguous_version_is_reported() {
    let path = package(
        "ambiguous",
        &Doc {
            root: "FaultInterpretation",
            namespace: RESQML,
            schema: None,
            domain: true,
        },
    );
    let store = EpcStore::open(&path, registry()).expect("opens");
    let _ = store.resources();
    assert!(
        store
            .diagnostics()
            .iter()
            .any(|d| d.message.contains("does not identify a single version")
                && d.message.contains("one namespace covers every version")),
        "the ambiguity and its cause are both named: {:?}",
        store.diagnostics()
    );
}
