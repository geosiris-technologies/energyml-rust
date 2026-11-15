//! `combine`: a property derived from two others, value by value, written with
//! the shape of its parents and an activity that names both.
//!
//! The form of the assertions comes from the standard and from arithmetic: a
//! ratio of two quantities is unitless (`Euc`), the shape of a derived
//! property is its parents' (RESQML TRG 2.2 section 7.1 p. 222), and the
//! dataset is `[elements, d1, d2]` (section 7 p. 221).
//!
//! What would make them fail: the result written as a scalar, a unit replaced
//! in silence, two supports combined, the parents missing from the activity, a
//! typo in the id of a combination, an array of NaN written for nothing.

use energyml_analysis::combine::Combines;
use energyml_build::operation::{Choices, Identity, Numbers, Plan, Registry, When};
use energyml_build::ops::combine::CombineChoice;
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};
use serde_json::{Value, json};

const FRAME: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const PARENT_A: &str = "dddddddd-0000-4000-8000-00000000000a";
const PARENT_B: &str = "dddddddd-0000-4000-8000-00000000000b";

fn when() -> When {
    When {
        creation: "2026-11-15T00:00:00Z".into(),
        originator: "tests/combine.rs".into(),
    }
}

fn dor(uuid: &str, qualified: &str, title: &str) -> Value {
    json!({ "$type": "eml23.DataObjectReference", "Uuid": uuid, "QualifiedType": qualified, "Title": title })
}

/// Two fields over a frame of two nodes, the frame as the piece they lie on.
fn bag(a: Vec<f64>, b: Vec<f64>, over_b: &str) -> Pieces {
    let geometry = Geometry {
        kind: "x",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            topology: Topology::Points,
            closed: Vec::new(),
        }],
    };
    let field = |values: Vec<f64>, over: &str| Source::Field {
        over: over.to_string(),
        on: "nodes".to_string(),
        values,
        uom: Some("ohm.m".to_string()),
    };
    let parent = |uuid: &str, title: &str| {
        Source::Object(Known {
            reference: Some(Ref::new(uuid, "resqml22.ContinuousProperty", title)),
            ..Default::default()
        })
    };
    Pieces::empty()
        .with(
            "frame",
            Source::Shape {
                geometry,
                closed: None,
            },
        )
        .with(
            "frame (object)",
            Source::Object(Known {
                reference: Some(Ref::new(
                    FRAME,
                    "resqml22.WellboreFrameRepresentation",
                    "the frame",
                )),
                node_count: Some(2),
                ..Default::default()
            }),
        )
        .with("a", field(a, "frame"))
        .with("b", field(b, over_b))
        .with("parents", parent(PARENT_A, "Rh"))
        .with("parents 2", parent(PARENT_B, "Rv"))
}

fn choices(operation: &str, uom: &str) -> Choices {
    Choices::new()
        .set("operation", operation)
        .set("valuesPerElement", json!([2, 2]))
        .set(
            "representation",
            dor(FRAME, "resqml22.WellboreFrameRepresentation", "the frame"),
        )
        .set(
            "propertyKind",
            dor(
                "eeeeeeee-0000-4000-8000-000000000001",
                "eml23.PropertyKind",
                "anisotropy",
            ),
        )
        .set("uom", uom)
        .set("title", "Rv over Rh")
        .set("dataFile", "data.h5")
}

fn planned(pieces: &Pieces, choices: Choices) -> Result<Plan, String> {
    Registry::new()
        .plan("combine", pieces, &choices, &Identity::derived(), &when())
        .map_err(|why| why.to_string())
}

fn a() -> Vec<f64> {
    vec![2.0, 9.0, 8.0, 6.0, 10.0, 12.0, 14.0, 3.0]
}

fn b() -> Vec<f64> {
    vec![1.0, 3.0, 4.0, 2.0, 5.0, 4.0, 7.0, 1.0]
}

#[test]
fn a_ratio_is_written_unitless_with_the_shape_of_its_parents() {
    let plan = planned(&bag(a(), b(), "frame"), choices("ratio", "Euc")).expect("planned");
    let written = plan
        .objects
        .iter()
        .find(|one| one.qualified_type == "resqml22.ContinuousProperty")
        .expect("a property");
    assert_eq!(written.json["ValueCountPerIndexableElement"], json!([2, 2]));
    assert_eq!(written.json["Uom"], json!("Euc"));
    assert_eq!(plan.datasets[0].dimensions, vec![2, 2, 2]);
    match &plan.datasets[0].values {
        Numbers::F64(values) => assert_eq!(values, &vec![2.0, 3.0, 2.0, 3.0, 2.0, 3.0, 2.0, 3.0]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_unit_given_against_the_computed_one_is_refused() {
    let why = planned(&bag(a(), b(), "frame"), choices("ratio", "ohm.m")).expect_err("refused");
    assert!(why.contains("`uom`"), "{why}");
    assert!(why.contains("Euc"), "it says the computed unit: {why}");
}

#[test]
fn fields_over_two_supports_are_refused() {
    let why = planned(&bag(a(), b(), "another frame"), choices("ratio", "Euc")).expect_err("refused");
    assert!(why.contains("`b`") && why.contains("lies over"), "{why}");
}

#[test]
fn the_activity_records_the_operation_and_both_parents() {
    let plan = planned(&bag(a(), b(), "frame"), choices("ratio", "Euc")).expect("planned");
    assert_eq!(plan.activity.recipe, "combine");
    let inputs: Vec<&str> = plan
        .activity
        .parameters
        .iter()
        .filter_map(|p| match p {
            Parameter::Object { title, reference } if title == "input" => reference["Uuid"].as_str(),
            _ => None,
        })
        .collect();
    // The support first, recorded by the property writer as every property's
    // input, then the two parents.
    assert_eq!(inputs, vec![FRAME, PARENT_A, PARENT_B]);
    assert!(plan.activity.parameters.iter().any(|p| matches!(
        p,
        Parameter::Text { title, value } if title == "operation" && value == "ratio"
    )));
}

#[test]
fn the_choice_ids_are_the_registrys() {
    let registry = Combines::builtin();
    for choice in [
        CombineChoice::Ratio,
        CombineChoice::Difference,
        CombineChoice::LogRatio,
    ] {
        assert!(registry.get(choice.id()).is_some(), "{}", choice.id());
        // And the word a form says is the id: one spelling.
        assert_eq!(choice.word(), choice.id());
    }
}

#[test]
fn nothing_defined_is_nothing_written() {
    let why = planned(&bag(a(), vec![0.0; 8], "frame"), choices("ratio", "Euc")).expect_err("refused");
    assert!(why.contains("`a`") && why.contains("nothing to write"), "{why}");
}
