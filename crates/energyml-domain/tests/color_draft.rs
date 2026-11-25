//! A colour information that names its colour map by an element of the draft of RESQML 2.2.
//!
//! The released 2.2 schema has `ColorMap`; the draft `v2.2_dev3` has `DiscreteColorMap` and `ContinuousColorMap`. The map is
//! read whichever holds it, and the validation **warns** - never an error - unless the package declares the draft. Every object
//! is built here.
//!
//! What it would take to make each one fail: no longer reading the draft element, warning for `ColorMap` too, or warning a
//! package that declares the draft.

use energyml_core::any_xml::AnyXml;
use energyml_core::registry::DynObject;
use energyml_core::validate::{self, Severity};
use energyml_domain::check;
use energyml_domain::color::{GraphicalSet, Information};
use energyml_eml_2_3 as eml23;

const TABLE: &str = "e1000000-0000-4000-8000-000000000001";
const MAP: &str = "e1000000-0000-4000-8000-000000000002";

/// A set of one entry whose colour map is named by an element called `element`, in a set declaring `schema_version`.
fn set_naming_by(element: &str, schema_version: &str) -> eml23::GraphicalInformationSet {
    let content = format!(
        "<eml23:TargetObject><eml23:Uuid>{TABLE}</eml23:Uuid>\
         <eml23:QualifiedType>eml23.ColumnBasedTable</eml23:QualifiedType><eml23:Title>t</eml23:Title></eml23:TargetObject>\
         <resqml22:UseLogarithmicMapping>false</resqml22:UseLogarithmicMapping>\
         <resqml22:UseReverseMapping>false</resqml22:UseReverseMapping>\
         <resqml22:ValueVectorIndex>1</resqml22:ValueVectorIndex>\
         <resqml22:{element}><eml23:Uuid>{MAP}</eml23:Uuid>\
         <eml23:QualifiedType>resqml22.DiscreteColorMap</eml23:QualifiedType><eml23:Title>m</eml23:Title></resqml22:{element}>"
    );
    eml23::GraphicalInformationSet {
        uuid: "e1000000-0000-4000-8000-0000000000f1".to_string(),
        schema_version: schema_version.to_string(),
        citation: eml23::Citation {
            title: "a set".to_string(),
            ..Default::default()
        },
        graphical_information: vec![AnyXml {
            attributes: vec![("xsi:type".to_string(), "resqml22:ColorInformation".to_string())],
            content,
            namespaces: Vec::new(),
        }],
        ..Default::default()
    }
}

fn warned(set: eml23::GraphicalInformationSet) -> Vec<validate::Issue> {
    let rules = check::documented_rules();
    let object: Box<dyn DynObject> = Box::new(set);
    validate::validate_with(object.as_validate(), &rules)
        .issues
        .into_iter()
        .filter(|issue| issue.code == "colour-map-draft-element")
        .collect()
}

fn map_of(set: &eml23::GraphicalInformationSet) -> Option<String> {
    let read = GraphicalSet::read(set.as_reflect());
    match &read.entries.first()?.information {
        Information::Color { map, .. } => map.clone(),
        _ => None,
    }
}

/// A published 2.2 package whose colour information names its map by `DiscreteColorMap` is **warned**, once, in words that say
/// the element is not in the declared schema and that a strict reader will not find the map - and the map is read all the same.
///
/// Break it: in `read_raw_entry`, look for the map under `ColorMap` alone (nothing is read), or drop the note.
#[test]
fn a_draft_element_in_a_published_package_is_warned_and_read() {
    let set = set_naming_by("DiscreteColorMap", "2.3");
    assert_eq!(map_of(&set).as_deref(), Some(MAP), "the tolerant reading stays");
    let issues = warned(set);
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert_eq!(issues[0].severity, Severity::Warning);
    assert!(
        issues[0]
            .message
            .contains("not in the schema the package declares")
            && issues[0].message.contains("strict reader"),
        "{}",
        issues[0].message
    );
}

/// The released element, `ColorMap`, gives nothing to say.
///
/// Break it: in `read_raw_entry`, push the note whatever the element's name.
#[test]
fn the_released_element_is_not_warned() {
    let set = set_naming_by("ColorMap", "2.3");
    assert_eq!(map_of(&set).as_deref(), Some(MAP));
    assert!(warned(set).is_empty());
}

/// A package that declares the draft (`schemaVersion` with the `_dev3` suffix) uses its own schema's element: nothing to say.
///
/// Break it: in `ColourMapDraftElement::check`, leave out the test of the declared version.
#[test]
fn a_package_that_declares_the_draft_is_not_warned() {
    let set = set_naming_by("DiscreteColorMap", "2.2_dev3");
    assert_eq!(map_of(&set).as_deref(), Some(MAP));
    assert!(warned(set).is_empty());
}
