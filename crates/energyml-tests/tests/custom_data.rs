//! `xsd:any` — the vendor extension point — end to end.
//!
//! `CustomData` is the only `xsd:any` in the whole schema set, and it hangs off
//! **every** top-level object of every eml version. Until the audit of
//! 01/08/2026 found it, the generator had no arm for `xsd:any` at all: the
//! struct came out empty, and every byte a client put in there was read as
//! nothing and written back as an empty element. Silent data loss on the one
//! construct whose entire purpose is carrying data this library does not know
//! about.

use energyml_core::ns;
use energyml_core::xml::{self, XmlWriteOptions};
use energyml_eml_2_3 as eml;
use energyml_resqml_2_2 as resqml;

/// A vendor subtree whose prefix is declared on the **root**, as a real
/// document does — the subtree does not carry the declaration, so it has to be
/// remembered and re-emitted or the output binds `geo:` to nothing.
const SOURCE: &str = "\
<resqml:TriangulatedSetRepresentation \
xmlns:resqml=\"http://www.energistics.org/energyml/data/resqmlv2\" \
xmlns:eml=\"http://www.energistics.org/energyml/data/commonv2\" \
xmlns:geo=\"http://geosiris.com/vendor\" \
uuid=\"a1b2c3d4-e5f6-7890-abcd-ef1234567890\" schemaVersion=\"2.2\">\
<eml:Citation><eml:Title>with vendor data</eml:Title>\
<eml:Originator>test</eml:Originator>\
<eml:Creation>2026-08-01T00:00:00Z</eml:Creation>\
<eml:Format>test</eml:Format></eml:Citation>\
<eml:CustomData>\
<geo:Survey id=\"S_42\" quality=\"good\">\
<geo:Crew>Bernard</geo:Crew>\
<geo:Note>relev&#233; &amp; contr&#244;l&#233;</geo:Note>\
</geo:Survey>\
<geo:Flag>true</geo:Flag>\
</eml:CustomData>\
<resqml:SurfaceRole>map</resqml:SurfaceRole>\
</resqml:TriangulatedSetRepresentation>";

fn to_xml(value: &resqml::TriangulatedSetRepresentation) -> String {
    xml::to_xml_string(
        value,
        ns::RESQML,
        "TriangulatedSetRepresentation",
        &[ns::RESQML, ns::EML, ns::XSI],
        XmlWriteOptions::COMPACT,
    )
    .expect("serializes")
}

#[test]
fn a_vendor_extension_survives_a_round_trip() {
    let object: resqml::TriangulatedSetRepresentation =
        xml::from_xml_str(SOURCE).expect("the object reads");

    let custom = object
        .custom_data
        .as_ref()
        .expect("CustomData is present")
        .any
        .as_ref()
        .expect("the wildcard captured the vendor elements");

    // Element names, attributes, nesting and text all kept.
    assert!(custom.content.contains("<geo:Survey"), "{}", custom.content);
    assert!(custom.content.contains("id=\"S_42\""), "{}", custom.content);
    assert!(
        custom.content.contains("<geo:Crew>Bernard</geo:Crew>"),
        "{}",
        custom.content
    );
    assert!(
        custom.content.contains("<geo:Flag>true</geo:Flag>"),
        "{}",
        custom.content
    );
    // Character references resolved on the way in and re-escaped on the way
    // out — kept, not lost, and not double-escaped.
    assert!(
        custom.content.contains("relevé &amp; contrôlé"),
        "{}",
        custom.content
    );
    // The prefix was declared on an ancestor: remembering it is what makes the
    // written-back subtree parseable at all.
    assert!(
        custom
            .namespaces
            .iter()
            .any(|(p, uri)| p == "geo" && uri == "http://geosiris.com/vendor"),
        "{:?}",
        custom.namespaces
    );

    // The sibling fields still read: a wildcard takes the leftovers, not the
    // elements the schema names.
    assert_eq!(object.citation.title, "with vendor data");
    assert!(matches!(object.surface_role, resqml::SurfaceRole::Map));

    let written = to_xml(&object);
    assert!(written.contains("<geo:Survey"), "{written}");
    assert!(written.contains("Bernard"), "{written}");
    assert!(
        written.contains("xmlns:geo=\"http://geosiris.com/vendor\""),
        "{written}"
    );

    // And what came out reads back **equal** — the real test of a round trip.
    let again: resqml::TriangulatedSetRepresentation =
        xml::from_xml_str(&written).expect("the output reads back");
    assert_eq!(again, object);
}

#[test]
fn an_object_without_vendor_data_writes_no_wildcard() {
    // The empty case must stay empty: an absent `CustomData` must not become an
    // element, and a present-but-empty one must not gain content.
    let plain = resqml::TriangulatedSetRepresentation {
        uuid: "a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_string(),
        schema_version: "2.2".to_string(),
        surface_role: resqml::SurfaceRole::Map,
        ..Default::default()
    };
    let written = to_xml(&plain);
    assert!(!written.contains("CustomData"), "{written}");

    let empty = resqml::TriangulatedSetRepresentation {
        custom_data: Some(eml::CustomData::default()),
        ..plain
    };
    let written = to_xml(&empty);
    assert!(written.contains("CustomData"), "{written}");
    assert!(!written.contains("geo:"), "{written}");
}

#[test]
fn an_unknown_child_is_still_skipped_where_no_wildcard_was_declared() {
    // `Citation` declares no `xsd:any`, so a child from a future minor version
    // is skipped as before — the wildcard must not become a catch-all
    // everywhere.
    let source = "\
<eml:Citation xmlns:eml=\"http://www.energistics.org/energyml/data/commonv2\">\
<eml:Title>anything</eml:Title>\
<eml:Originator>test</eml:Originator>\
<eml:Creation>2026-08-01T00:00:00Z</eml:Creation>\
<eml:Format>test</eml:Format>\
<eml:FromAFutureVersion>ignored</eml:FromAFutureVersion>\
</eml:Citation>";

    let citation: eml::Citation = xml::from_xml_str(source).expect("the citation reads");
    assert_eq!(citation.title, "anything");
}
