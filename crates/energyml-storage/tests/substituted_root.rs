//! A part whose type has **no global element of its own**, written under its
//! ancestor's element with `xsi:type`.
//!
//! PRODML 2.3 declares an element for `FlowTestActivity` and for none of its
//! subtypes, though its documentation says *"This object is abstract; you must
//! choose one of the available types"* (FlowTestActivity.xsd l. 186–190). So an
//! injection test can only be written
//! `<prodml:FlowTestActivity xsi:type="prodml:InjectionFlowTest">` — the form of
//! Energistics' own example (`xml_examples/FlowTest/FlowTestActivity.xml`
//! l. 2). The part's identity is the concrete type — content type
//! `type=InjectionFlowTest`, qualified type `prodml23.InjectionFlowTest`:
//! the specifications being silent on it, this is the library's decision.

use std::sync::Arc;

use energyml_core::registry::{Registry, to_part};
use energyml_core::value::{FieldValue, Scalar};
use energyml_core::xml::XmlWriteOptions;
use energyml_storage::extract::empty_package;
use energyml_storage::{EpcStore, SaveOptions};

const UUID: &str = "0b000000-0000-4000-8000-0000000000a1";
const CONCRETE: &str = "application/x-prodml+xml;version=2.3;type=InjectionFlowTest";
const ELEMENT: &str = "application/x-prodml+xml;version=2.3;type=FlowTestActivity";

/// An injection test as a document writes it: the ancestor's element, and
/// `xsi:type` saying what it is.
fn document(xsi_type: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<prodml:FlowTestActivity xmlns:prodml="http://www.energistics.org/energyml/data/prodmlv2" xmlns:eml="http://www.energistics.org/energyml/data/commonv2" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="{xsi_type}" uuid="{UUID}" schemaVersion="2.3">
  <eml:Citation>
    <eml:Title>GTH-2 — injection test</eml:Title>
    <eml:Originator>test</eml:Originator>
    <eml:Creation>2026-09-27T00:00:00Z</eml:Creation>
    <eml:Format>test</eml:Format>
  </eml:Citation>
  <prodml:WellTestMethod>step rate</prodml:WellTestMethod>
  <prodml:IntervalMeasurementSet/>
</prodml:FlowTestActivity>"#
    )
}

fn method(object: &dyn energyml_core::value::Reflect) -> Option<String> {
    match object.field("WellTestMethod")? {
        FieldValue::Scalar(Scalar::Str(text)) => Some(text.to_string()),
        _ => None,
    }
}

fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

#[test]
fn declared_as_its_element_the_part_is_read_as_what_its_xsi_type_says() {
    // Read as the empty base, everything the test says would be lost — and an
    // object with every field at its default reads exactly like a right one.
    let object = registry()
        .read_xml(ELEMENT, document("prodml:InjectionFlowTest").as_bytes())
        .expect("it reads");
    assert_eq!(object.as_reflect().type_info().name, "InjectionFlowTest");
    assert_eq!(method(object.as_reflect()).as_deref(), Some("step rate"));
}

#[test]
fn declared_as_the_concrete_type_it_reads_too() {
    let object = registry()
        .read_xml(CONCRETE, document("prodml:InjectionFlowTest").as_bytes())
        .expect("it reads");
    assert_eq!(object.as_reflect().type_info().name, "InjectionFlowTest");
    assert_eq!(method(object.as_reflect()).as_deref(), Some("step rate"));
}

#[test]
fn an_xsi_type_that_names_no_subtype_is_refused_by_name() {
    let Err(refused) = registry().read_xml(ELEMENT, document("prodml:NoSuchTest").as_bytes()) else {
        panic!("an `xsi:type` naming no subtype was read")
    };
    assert!(refused.to_string().contains("NoSuchTest"), "{refused}");
}

#[test]
fn it_is_written_under_the_ancestors_element_and_says_what_it_is() {
    let object = registry()
        .read_xml(CONCRETE, document("prodml:InjectionFlowTest").as_bytes())
        .expect("it reads");
    let xml = to_part(object.as_ref(), XmlWriteOptions::default()).expect("it writes");
    let root = xml
        .lines()
        .find(|line| line.contains("<prodml:"))
        .expect("a root");
    assert!(
        root.contains("<prodml:FlowTestActivity "),
        "the schema declares no `InjectionFlowTest` element: {root}"
    );
    assert!(root.contains(r#"xsi:type="prodml:InjectionFlowTest""#), "{root}");
    assert!(!xml.contains("<prodml:InjectionFlowTest"), "{xml}");
}

#[test]
fn a_type_with_its_own_element_is_written_as_before() {
    // The rule must not reach a type the schema gives an element: every part of
    // the corpus is one, and an `xsi:type` there is a gratuitous difference.
    let object = registry()
        .create("prodml23.PressureTransientAnalysis")
        .expect("a type");
    let xml = to_part(object.as_ref(), XmlWriteOptions::default()).expect("it writes");
    assert!(xml.contains("<prodml:PressureTransientAnalysis "), "{xml}");
    assert!(
        !xml.contains("xsi:type=\"prodml:PressureTransientAnalysis\""),
        "{xml}"
    );
}

#[test]
fn through_a_package_it_keeps_its_identity_and_its_content() {
    let registry = registry();
    let object = registry
        .read_xml(CONCRETE, document("prodml:InjectionFlowTest").as_bytes())
        .expect("it reads");
    let mut store = EpcStore::from_reader(
        Box::new(std::io::Cursor::new(empty_package(None).unwrap())),
        "flow.epc",
        registry.clone(),
    )
    .expect("an empty package");
    store.put(object).expect("a part");
    let dir = std::env::temp_dir().join(format!("energyml-substituted-root-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("flow.epc");
    store.save_as(&path, &SaveOptions::default()).expect("it saves");

    let reopened = EpcStore::open(&path, registry).expect("it reopens");
    let summary = reopened.find_by_uuid(UUID).expect("listed").clone();
    assert_eq!(summary.qualified_type, "prodml23.InjectionFlowTest");
    assert_eq!(summary.content_type.replace(' ', ""), CONCRETE);
    let raw = String::from_utf8(reopened.raw_part(&summary.locator).expect("the part").to_vec()).unwrap();
    assert!(
        raw.contains(r#"<prodml:FlowTestActivity "#)
            && raw.contains(r#"xsi:type="prodml:InjectionFlowTest""#),
        "{raw}"
    );
    let back = reopened.get_object(&summary.uri).expect("it reads back");
    assert_eq!(back.as_reflect().type_info().name, "InjectionFlowTest");
    assert_eq!(method(back.as_reflect()).as_deref(), Some("step rate"));
    let said: Vec<String> = reopened
        .diagnostics()
        .iter()
        .filter(|one| one.severity >= energyml_storage::Severity::Warning)
        .map(|one| format!("{}: {}", one.part, one.message))
        .collect();
    assert!(said.is_empty(), "{said:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A root whose `xsi:type` names **its own** complex type is no substitution.
///
/// `h+g_PaleoGTM.epc` roots `<resqml2:LocalDepth3dCrs>` with
/// `xsi:type="resqml2:obj_LocalDepth3dCrs"` — RESQML 2.0.1's complex type is
/// `obj_LocalDepth3dCrs`, its element `LocalDepth3dCrs`. Taken for a subtype, the
/// part was refused and dropped out of the graph.
#[test]
fn a_root_typed_by_its_own_complex_type_reads_as_itself() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<resqml2:LocalDepth3dCrs xmlns:resqml2="http://www.energistics.org/energyml/data/resqmlv2" xmlns:eml="http://www.energistics.org/energyml/data/commonv2" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="resqml2:obj_LocalDepth3dCrs" uuid="0b000000-0000-4000-8000-0000000000b1" schemaVersion="2.0">
  <eml:Citation><eml:Title>depth</eml:Title><eml:Originator>test</eml:Originator><eml:Creation>2026-09-27T00:00:00Z</eml:Creation><eml:Format>test</eml:Format></eml:Citation>
</resqml2:LocalDepth3dCrs>"#;
    let object = registry()
        .read_xml(
            "application/x-resqml+xml;version=2.0;type=obj_LocalDepth3dCrs",
            xml.as_bytes(),
        )
        .unwrap_or_else(|why| panic!("it reads as itself: {why}"));
    assert_eq!(object.as_reflect().type_info().object_type, "obj_LocalDepth3dCrs");
    let peeked = energyml_storage::epc::header::peek(xml.as_bytes());
    assert_eq!(peeked.root_name, "LocalDepth3dCrs", "the element stays the name");
    assert_eq!(peeked.root_element, None);
}
