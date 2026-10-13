//! What a lenient read repairs, and what it still refuses.
//!
//! `--lenient` started as a rule about **cardinality**: a required item that
//! is absent gets its default and is reported. This suite is about the level
//! above — values that are *there* and cannot be read:
//!
//! | in the document | strict | lenient |
//! |---|---|---|
//! | `<SurfaceRole>banana</SurfaceRole>` on a closed enumeration | refuses | first variant, and the string is recorded |
//! | `<Count>abc</Count>` where a number belongs | refuses | `0`, recorded |
//! | `<IsClosed>yes</IsClosed>` | refuses | `false`, recorded |
//! | `&nbsp;` — an entity nothing defines | refuses | U+FFFD, recorded |
//! | `xsi:type` naming an unknown subtype **of a concrete base** | refuses | the base, recorded |
//! | `xsi:type` naming an unknown subtype of a purely abstract one | refuses | **refuses** |
//!
//! The last row is the line, and it is deliberate: with no concrete base there
//! is nothing to fall back *to*, and inventing one would put an object in
//! memory that the document does not describe.
//!
//! Every repair carries the string that was in the file. That string is the
//! only record of what the producer wrote, and a repair tool cannot put back
//! what it was never told.

use energyml_core::registry::{Departures, Registry};
use energyml_core::xml;

fn registry() -> Registry {
    let mut registry = Registry::new();
    energyml_eml_2_3::register(&mut registry);
    energyml_resqml_2_2::register(&mut registry);
    registry
}

/// A `TriangulatedSetRepresentation` part, with one thing wrong in it.
fn part(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<resqml:TriangulatedSetRepresentation
  xmlns:resqml="http://www.energistics.org/energyml/data/resqmlv2"
  xmlns:eml="http://www.energistics.org/energyml/data/commonv2"
  xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
  uuid="1a4112fa-c4ef-4c8d-aed0-47d9273bebc5" schemaVersion="2.2">
  <eml:Citation>
    <eml:Title>a surface with a defect</eml:Title>
    <eml:Originator>test</eml:Originator>
    <eml:Creation>2026-08-01T00:00:00Z</eml:Creation>
    <eml:Format>test</eml:Format>
  </eml:Citation>
  {body}
</resqml:TriangulatedSetRepresentation>"#
    )
}

const CONTENT_TYPE: &str = "application/x-resqml+xml;version=2.2;type=TriangulatedSetRepresentation";

fn strict(xml: &str) -> Result<(), String> {
    registry()
        .read_xml(CONTENT_TYPE, xml.as_bytes())
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn lenient(xml: &str) -> Result<Departures, String> {
    registry()
        .read_xml_repairing(CONTENT_TYPE, xml.as_bytes())
        .map(|(_, departures)| departures)
        .map_err(|e| e.to_string())
}

/// The one repair of `departures`, or a panic saying what it found instead.
fn one(departures: &Departures) -> &xml::Repaired {
    assert_eq!(
        departures.repaired.len(),
        1,
        "expected exactly one repair, got {:?} and {:?}",
        departures.repaired,
        departures.defaulted
    );
    &departures.repaired[0]
}

#[test]
fn a_closed_enumeration_outside_its_list_keeps_the_string_it_could_not_use() {
    let xml = part("<resqml:SurfaceRole>banana</resqml:SurfaceRole>");
    let error = strict(&xml).expect_err("strictly, `banana` is not a surface role");
    assert!(error.contains("banana"), "{error}");

    let departures = lenient(&xml).expect("leniently, it reads");
    let repaired = one(&departures);
    assert_eq!(repaired.kind, "enumeration");
    assert_eq!(repaired.found, "banana");
    // The variant cannot hold the string — a closed enumeration has no
    // `Other(String)` — so the record is the only place it survives.
    assert!(!repaired.used.is_empty());
    assert!(
        repaired.to_string().contains("banana") && repaired.to_string().contains(&repaired.used),
        "{repaired}"
    );
}

#[test]
fn a_number_that_is_not_a_number_becomes_zero_and_says_so() {
    let xml = part(
        "<resqml:SurfaceRole>map</resqml:SurfaceRole>\
         <resqml:TrianglePatch><resqml:NodeCount>abc</resqml:NodeCount>\
         </resqml:TrianglePatch>",
    );
    assert!(strict(&xml).is_err(), "strictly, `abc` is not a number");

    let departures = lenient(&xml).expect("leniently, it reads");
    let repaired = departures
        .repaired
        .iter()
        .find(|r| r.found == "abc")
        .unwrap_or_else(|| panic!("no repair for `abc`: {:?}", departures.repaired));
    assert_eq!(repaired.kind, "number");
    assert_eq!(repaired.used, "0");
}

#[test]
fn a_boolean_spelled_in_words_is_repaired() {
    // XSD allows `true`, `false`, `1` and `0` — and nothing else. `yes` is a
    // producer being helpful in the wrong direction.
    assert!(xml::from_xml_str::<bool>("<v>yes</v>").is_err(), "strictly, `yes` is not a boolean");
    let (value, _) = xml::from_xml_str_lenient::<bool>("<v>yes</v>").expect("leniently, it reads");
    assert!(!value, "the default of a boolean is false");
}

#[test]
fn an_entity_nothing_defines_becomes_the_replacement_character() {
    // This reader ignores DOCTYPE, so every named entity outside the five
    // predefined ones arrives undefined — which is exactly what a document
    // carrying `&nbsp;` in a title does.
    let xml = part("<resqml:SurfaceRole>map</resqml:SurfaceRole>")
        .replace("a surface with a defect", "a surface&nbsp;with a defect");
    assert!(strict(&xml).is_err(), "strictly, `&nbsp;` is undefined");

    let departures = lenient(&xml).expect("leniently, it reads");
    let repaired = one(&departures);
    assert_eq!(repaired.kind, "entity");
    assert_eq!(repaired.found, "&nbsp;");
    assert_eq!(repaired.used, "\u{fffd}");
}

#[test]
fn an_unknown_xsi_type_falls_back_to_the_base_when_there_is_one() {
    // `PointGeometry` is concrete and other geometries extend it, so an
    // element declared with it can be read as it even when the `xsi:type`
    // names something this build does not know: the fields the base declares
    // are still there and still mean what they say.
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<resqml:PointSetRepresentation
  xmlns:resqml="http://www.energistics.org/energyml/data/resqmlv2"
  xmlns:eml="http://www.energistics.org/energyml/data/commonv2"
  xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
  uuid="2a4112fa-c4ef-4c8d-aed0-47d9273bebc5" schemaVersion="2.2">
  <eml:Citation><eml:Title>points</eml:Title><eml:Originator>test</eml:Originator>
    <eml:Creation>2026-08-01T00:00:00Z</eml:Creation><eml:Format>test</eml:Format></eml:Citation>
  <resqml:NodePatchGeometry xsi:type="resqml:GeometryFromTheFuture">
    <resqml:LocalCrs><eml:Uuid>3a4112fa-c4ef-4c8d-aed0-47d9273bebc5</eml:Uuid>
      <eml:QualifiedType>eml23.LocalEngineeringCompoundCrs</eml:QualifiedType></resqml:LocalCrs>
    <resqml:Points xsi:type="resqml:Point3dLatticeArray">
      <resqml:AllDimensionsAreOrthogonal>true</resqml:AllDimensionsAreOrthogonal>
      <resqml:Origin><resqml:Coordinate1>0</resqml:Coordinate1>
        <resqml:Coordinate2>0</resqml:Coordinate2>
        <resqml:Coordinate3>0</resqml:Coordinate3></resqml:Origin>
    </resqml:Points>
  </resqml:NodePatchGeometry>
</resqml:PointSetRepresentation>"#;
    let content_type = "application/x-resqml+xml;version=2.2;type=PointSetRepresentation";

    let strictly = registry().read_xml(content_type, xml.as_bytes());
    let leniently = registry().read_xml_repairing(content_type, xml.as_bytes());

    match (strictly, leniently) {
        (Err(_), Ok((_, departures))) => {
            let repaired = departures
                .repaired
                .iter()
                .find(|r| r.kind == "xsi:type")
                .unwrap_or_else(|| panic!("no xsi:type repair: {:?}", departures.repaired));
            assert!(repaired.found.contains("GeometryFromTheFuture"), "{repaired}");
            assert!(!repaired.used.is_empty(), "the base it fell back to has to be named");
        }
        // The fixture is hand-written against a schema this test does not
        // control; if it stops reaching the branch, say so rather than pass.
        (strictly, leniently) => panic!(
            "the fixture no longer exercises the fallback: strict={:?}, lenient={:?}",
            strictly.map(|_| "ok"),
            leniently.map(|_| "ok")
        ),
    }
}

#[test]
fn an_unknown_xsi_type_on_a_purely_abstract_choice_is_still_refused() {
    // The line leniency does not cross. `AbstractIntegerArray` has no concrete
    // base: every array must name its type, so an unknown one leaves nothing
    // to read. Reading it as *some* array would put numbers in memory that the
    // document does not describe.
    let xml = part(
        r#"<resqml:SurfaceRole>map</resqml:SurfaceRole>
  <resqml:TrianglePatch><resqml:PatchIndex>0</resqml:PatchIndex>
    <resqml:Count>1</resqml:Count><resqml:NodeCount>3</resqml:NodeCount>
    <resqml:Triangles xsi:type="eml:QuantumArray"><eml:Values/></resqml:Triangles>
  </resqml:TrianglePatch>"#,
    );
    let error = lenient(&xml).expect_err("leniently, this is still a refusal");
    assert!(
        error.contains("QuantumArray"),
        "the refusal names the type it could not construct: {error}"
    );
}

#[test]
fn a_conformant_document_repairs_nothing() {
    // The guarantee that makes the rest usable: leniency is invisible on a
    // file that is right.
    let xml = part("<resqml:SurfaceRole>map</resqml:SurfaceRole>");
    let departures = lenient(&xml).expect("it reads");
    assert!(
        departures.is_empty(),
        "a conformant part needed {:?} and {:?}",
        departures.defaulted,
        departures.repaired
    );
}
