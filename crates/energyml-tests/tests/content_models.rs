//! Two XSD constructs the generator used to drop on the floor.
//!
//! Found by the audit of 01/08/2026, in the same function — `read_particles`
//! looked at elements and groups and nothing else, so a container's own
//! cardinality and a type's `mixed` flag never reached the IR.

use energyml_core::xml::{self, XmlWriteOptions, from_xml_str};
use energyml_eml_2_3 as eml;
use energyml_witsml_2_0 as witsml;

/// An optional `<xs:sequence>` makes everything inside it optional.
///
/// `TorqueStatistics` wraps its content in `<xs:sequence minOccurs="0">`, and
/// `Average` is `minOccurs="1"` **inside** it — required *if the group is
/// there*. Reading the container's cardinality as 1 made `Average` a required
/// Rust field, so this perfectly valid document failed with
/// `MissingRequired`. The only place found where the strict reader refused
/// conformant XML.
#[test]
fn a_valid_document_omitting_an_optional_group_reads() {
    let source = "\
<witsml:TorqueStatistics \
xmlns:witsml=\"http://www.energistics.org/energyml/data/witsmlv2\"/>";

    let stats: witsml::TorqueStatistics = from_xml_str(source).expect("the optional group may be absent");
    assert!(stats.average.is_none());
    assert!(stats.channel.is_none());
}

/// And when the group *is* there, its members still read.
#[test]
fn the_members_of_an_optional_group_still_read_when_present() {
    let source = "\
<witsml:TorqueStatistics \
xmlns:witsml=\"http://www.energistics.org/energyml/data/witsmlv2\" \
xmlns:eml=\"http://www.energistics.org/energyml/data/commonv2\">\
<witsml:Average uom=\"N.m\">42.5</witsml:Average>\
</witsml:TorqueStatistics>";

    let stats: witsml::TorqueStatistics = from_xml_str(source).expect("it reads");
    let average = stats.average.expect("Average was written, so it is there");
    assert_eq!(average.value, 42.5);
}

/// `mixed="true"` with an empty content model: the text **is** the value.
///
/// A phone number is written `<Phone type="voice">+33 1 23 45 67 89</Phone>`.
/// Without the `mixed` flag the generated struct had the three attributes and
/// no text field at all — the number itself was read as nothing and written
/// back as an empty element, keeping only its type.
#[test]
fn a_mixed_type_keeps_its_text() {
    let source = "\
<eml:Phone xmlns:eml=\"http://www.energistics.org/energyml/data/commonv2\" \
type=\"voice\" extension=\"204\">+33 1 23 45 67 89</eml:Phone>";

    let phone: eml::PhoneNumberStruct = from_xml_str(source).expect("the phone reads");
    assert_eq!(phone.value, "+33 1 23 45 67 89");
    assert_eq!(phone.extension.as_deref(), Some("204"));

    // And it goes back out, next to the attributes rather than instead of them.
    let written = xml::to_xml_string(
        &phone,
        energyml_core::ns::EML,
        "Phone",
        &[energyml_core::ns::EML],
        XmlWriteOptions::COMPACT,
    )
    .expect("it writes");
    assert!(written.contains("+33 1 23 45 67 89"), "{written}");
    assert!(written.contains("type=\"voice\""), "{written}");

    let again: eml::PhoneNumberStruct = from_xml_str(&written).expect("it reads back");
    assert_eq!(again, phone);
}
