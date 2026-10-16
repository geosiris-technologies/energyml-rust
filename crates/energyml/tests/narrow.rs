//! **The narrow builds** (plan n°117): the façade compiled with ONE RESQML
//! release, reading a part of that release and of the other one.
//!
//! This file can only run in a reduced build, because a workspace build unifies
//! every schema feature on (and the whole file is then compiled out):
//!
//! ```text
//! cargo test -p energyml --no-default-features --features resqml20 --test narrow
//! cargo test -p energyml --no-default-features --features resqml22 --test narrow
//! ```
//!
//! CI runs both (the `features` job). The packages are fabricated in memory and
//! nothing outside the repository is read.
//!
//! * `resqml20` alone: `2.0`, `2.0.1`, `resqml20.` and `resqml201.` are four
//!   spellings of one release, and all four are read as the 2.0.1 crate;
//! * `resqml22` alone: a 2.0.1 part is **refused**, by a message naming the
//!   `resqml20` feature, and is not read as 2.2 - a half read that looks right.
//!
//! What to break: in the first module, drop the `201` from `KnownVersion::compact`
//! of the 2.0.1 row (`resqml201.` stops being read); in the second, remove the
//! `declare_absent_versions` call from `energyml_schemas::all` (the part comes
//! back as RESQML 2.2).

#![cfg(any(
    all(feature = "resqml20", not(feature = "resqml22")),
    all(feature = "resqml22", not(feature = "resqml20"))
))]

use std::io::{Cursor, Write};
use std::sync::Arc;

use energyml::storage::EpcStore;

const UUID: &str = "5d2e8a07-91c4-4b3f-a6e1-0c7f3b9d4a58";
const RESQML: &str = "http://www.energistics.org/energyml/data/resqmlv2";
const EML: &str = "http://www.energistics.org/energyml/data/commonv2";

/// A package holding one RESQML 2.0.1 `BoundaryFeature`, written the way a
/// 2.0.1 producer writes it, declaring `schema_version` and (when given) the
/// content type `content_type`.
fn package_2_0_1(schema_version: &str, content_type: Option<&str>) -> Vec<u8> {
    let options: zip::write::SimpleFileOptions =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let part = format!("obj_BoundaryFeature_{UUID}.xml");
    let overrides = content_type
        .map(|ct| format!("<Override PartName=\"/{part}\" ContentType=\"{ct}\"/>"))
        .unwrap_or_default();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
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
    zip.write_all(
        format!(
            "<resqml:obj_BoundaryFeature xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" uuid=\"{UUID}\" \
             schemaVersion=\"{schema_version}\">\
             <eml:Citation><eml:Title>a boundary</eml:Title><eml:Originator>test</eml:Originator>\
             <eml:Creation>2026-10-13T00:00:00Z</eml:Creation><eml:Format>test</eml:Format></eml:Citation>\
             </resqml:obj_BoundaryFeature>"
        )
        .as_bytes(),
    )
    .unwrap();
    zip.finish().unwrap().into_inner()
}

fn open(bytes: Vec<u8>) -> EpcStore {
    EpcStore::from_reader(
        Box::new(Cursor::new(bytes)),
        "narrow.epc",
        Arc::new(energyml::schemas::all()),
    )
    .expect("the package opens")
}

fn uri_of(store: &EpcStore) -> String {
    store
        .resources()
        .into_iter()
        .find(|summary| summary.uuid == UUID)
        .expect("listed, whether or not this build can read it")
        .uri
}

/// Only RESQML 2.0.1 is compiled in.
#[cfg(all(feature = "resqml20", not(feature = "resqml22")))]
mod resqml20_alone {
    use super::*;

    /// A part declaring `schemaVersion="2.0"` and one declaring `2.0.1`, each
    /// with the content type spelled the same way, and one with no content type
    /// at all: all read, all as the 2.0.1 crate.
    #[test]
    fn a_part_in_either_spelling_of_the_version_is_read_as_2_0_1() {
        let cases = [
            (
                "2.0",
                Some("application/x-resqml+xml;version=2.0;type=obj_BoundaryFeature"),
            ),
            (
                "2.0.1",
                Some("application/x-resqml+xml;version=2.0.1;type=obj_BoundaryFeature"),
            ),
            ("2.0", None),
            ("2.0.1", None),
        ];
        for (schema_version, content_type) in cases {
            let store = open(package_2_0_1(schema_version, content_type));
            let object = store
                .get_object(&uri_of(&store))
                .unwrap_or_else(|e| panic!("`{schema_version}` / {content_type:?}: {e}"));
            let info = object.type_info();
            assert_eq!(
                (info.package, info.version),
                ("resqml", "2.0.1"),
                "`{schema_version}` / {content_type:?}"
            );
        }
    }

    /// An object whose qualified type is written `resqml20.` (the schema
    /// version's spelling) or `resqml201.` (the release's, which fesapi writes in
    /// some references) is the same type.
    #[test]
    fn both_spellings_of_the_qualified_type_create_the_2_0_1_type() {
        let registry = energyml::schemas::all();
        for qualified_type in ["resqml20.obj_BoundaryFeature", "resqml201.obj_BoundaryFeature"] {
            let object = registry
                .create(qualified_type)
                .unwrap_or_else(|e| panic!("{qualified_type}: {e}"));
            assert_eq!(object.type_info().version, "2.0.1", "{qualified_type}");
        }
        assert!(
            registry
                .by_qualified_type("resqml20.obj_BoundaryFeature")
                .is_some()
        );
        assert!(
            registry
                .by_qualified_type("resqml201.obj_BoundaryFeature")
                .is_some()
        );
    }

    /// The other release is the one that is absent, and it is named so.
    #[test]
    fn resqml_2_2_is_a_named_refusal_here() {
        let registry = energyml::schemas::all();
        match registry.create("resqml22.BoundaryFeature") {
            Err(energyml::core::error::Error::VersionAbsent(why)) => {
                assert!(why.contains("resqml22"), "{why}")
            }
            other => panic!("expected the refusal, got {:?}", other.err()),
        }
    }
}

/// Only RESQML 2.2 (and the EML 2.3 it is built on) is compiled in.
#[cfg(all(feature = "resqml22", not(feature = "resqml20")))]
mod resqml22_alone {
    use super::*;

    /// A 2.0.1 part is refused by a message naming `resqml20`, however it is
    /// declared - and it is **not** returned as an object of the 2.2 crate, which
    /// would read it (its one extra required element is defaulted) and look right.
    #[test]
    fn a_2_0_1_part_is_refused_by_name_not_read_as_2_2() {
        let cases = [
            (
                "2.0",
                Some("application/x-resqml+xml;version=2.0;type=obj_BoundaryFeature"),
            ),
            (
                "2.0.1",
                Some("application/x-resqml+xml;version=2.0.1;type=obj_BoundaryFeature"),
            ),
            ("2.0", None),
            ("2.0.1", None),
        ];
        for (schema_version, content_type) in cases {
            let store = open(package_2_0_1(schema_version, content_type));
            match store.get_object(&uri_of(&store)) {
                Err(energyml::storage::Error::Core(energyml::core::error::Error::VersionAbsent(why))) => {
                    assert!(
                        why.contains("resqml20"),
                        "`{schema_version}` / {content_type:?}: {why}"
                    );
                }
                Err(other) => {
                    panic!("`{schema_version}` / {content_type:?}: refused, but not by name: {other}")
                }
                Ok(object) => panic!(
                    "`{schema_version}` / {content_type:?}: read as {} {}, a half read",
                    object.type_info().package,
                    object.type_info().version
                ),
            }
        }
    }

    /// Plan n°119: the content type is **registered** (`version=2.2`) but the
    /// document declares the absent `schemaVersion` 2.0 / 2.0.1. The document is
    /// the part, so it is refused by the `resqml20` message, not read as 2.2.
    /// The control is the same content type over a 2.2 document, which reads.
    ///
    /// What to break: delete the `absent_declaration` block after the `refusal`
    /// computation in the content-type path of `EpcStore::classify`; the first
    /// loop then gets `Ok` (a 2.2 object).
    #[test]
    fn a_registered_content_type_over_a_2_0_1_document_is_refused() {
        let content_type = "application/x-resqml+xml;version=2.2;type=BoundaryFeature";
        for schema_version in ["2.0", "2.0.1"] {
            let store = open(package_2_0_1(schema_version, Some(content_type)));
            match store.get_object(&uri_of(&store)) {
                Err(energyml::storage::Error::Core(energyml::core::error::Error::VersionAbsent(why))) => {
                    assert!(why.contains("resqml20"), "`{schema_version}`: {why}");
                }
                Err(other) => panic!("`{schema_version}`: refused, but not by name: {other}"),
                Ok(object) => panic!(
                    "`{schema_version}`: followed the 2.2 index and read as {} {}",
                    object.type_info().package,
                    object.type_info().version
                ),
            }
        }
        let store = open(package_2_0_1("2.2", Some(content_type)));
        let object = store
            .get_object(&uri_of(&store))
            .expect("a 2.2 document under a 2.2 content type reads");
        assert_eq!(object.type_info().version, "2.2");
    }

    /// The creation and the content-type lookups carry the refusal too, instead
    /// of "unknown type (not registered)".
    #[test]
    fn the_registry_names_the_missing_feature() {
        let registry = energyml::schemas::all();
        for qualified_type in ["resqml20.obj_BoundaryFeature", "resqml201.obj_BoundaryFeature"] {
            match registry.create(qualified_type) {
                Err(energyml::core::error::Error::VersionAbsent(why)) => {
                    assert!(why.contains("resqml20"), "{why}")
                }
                other => panic!("{qualified_type}: expected the refusal, got {:?}", other.err()),
            }
        }
        let read = registry.read_xml(
            "application/x-resqml+xml;version=2.0.1;type=obj_BoundaryFeature",
            b"<resqml:obj_BoundaryFeature/>",
        );
        match read {
            Err(energyml::core::error::Error::VersionAbsent(why)) => {
                assert!(why.contains("resqml20"), "{why}")
            }
            other => panic!("expected the refusal, got {:?}", other.err()),
        }
    }
}
