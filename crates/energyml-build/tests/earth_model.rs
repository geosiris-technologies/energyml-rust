//! `earth-model`: a `Model`, and the `EarthModelInterpretation` that gathers
//! what the package says of it.
//!
//! The form of each assertion comes from the schema: *“BUSINESS RULE: An
//! earth model Interpretation interprets only a model feature”*
//! (Interpretations.xsd l. 632; RESQML TRG 2.2 §6.19 p. 187), and the
//! references it carries are `StratigraphicColumn` 0..1, `Structure`,
//! `Fluid`, `StratigraphicOccurrences` and `WellboreInterpretationSet` 0..∞
//! (Interpretations.xsd l. 637–641). The uuids and titles are the test's own.

use energyml_build::operation::{Choices, Identity, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use serde_json::{Value, json};

fn when() -> When {
    When {
        creation: "2026-09-24T00:00:00Z".into(),
        originator: "test".into(),
    }
}

fn object(uuid: &str, qualified_type: &str, title: &str) -> Source {
    Source::Object(Known {
        reference: Some(Ref {
            uuid: uuid.into(),
            qualified_type: qualified_type.into(),
            title: title.into(),
            object_version: None,
        }),
        ..Default::default()
    })
}

const COLUMN: &str = "0b7f1a52-3c4d-4e5f-8a9b-0c1d2e3f4a5b";
const STRUCTURE: &str = "1c8e2b63-4d5e-4f60-9bac-1d2e3f4a5b6c";

fn gathered() -> Pieces {
    Pieces::empty()
        .with(
            "column",
            object(COLUMN, "resqml22.StratigraphicColumn", "the column"),
        )
        .with(
            "structures",
            object(
                STRUCTURE,
                "resqml22.StructuralOrganizationInterpretation",
                "faulted",
            ),
        )
}

fn planned(pieces: &Pieces, choices: Choices) -> Result<Plan, String> {
    Registry::new()
        .plan("earth-model", pieces, &choices, &Identity::derived(), &when())
        .map_err(|why| why.to_string())
}

fn of_type<'a>(plan: &'a Plan, qualified_type: &str) -> &'a Value {
    &plan
        .objects
        .iter()
        .find(|one| one.qualified_type == qualified_type)
        .expect(qualified_type)
        .json
}

#[test]
fn the_interpretation_interprets_the_model_written_beside_it() {
    let plan = planned(&gathered(), Choices::new().set("title", "M")).expect("planned");
    assert_eq!(plan.objects.len(), 2);
    let model = &plan
        .objects
        .iter()
        .find(|one| one.qualified_type == "resqml22.Model")
        .unwrap();
    let interpretation = of_type(&plan, "resqml22.EarthModelInterpretation");
    // “An earth model Interpretation interprets only a model feature”.
    assert_eq!(
        interpretation["InterpretedFeature"]["Uuid"],
        json!(model.uuid),
        "{interpretation}"
    );
    assert_eq!(
        interpretation["InterpretedFeature"]["QualifiedType"],
        json!("resqml22.Model"),
        "{interpretation}"
    );
}

#[test]
fn each_reference_lands_in_the_field_its_slot_names() {
    let plan = planned(&gathered(), Choices::new().set("title", "M")).expect("planned");
    let interpretation = of_type(&plan, "resqml22.EarthModelInterpretation");
    assert_eq!(
        interpretation["StratigraphicColumn"]["Uuid"],
        json!(COLUMN),
        "{interpretation}"
    );
    assert_eq!(
        interpretation["Structure"][0]["Uuid"],
        json!(STRUCTURE),
        "{interpretation}"
    );
    assert_eq!(
        interpretation["Structure"][0]["QualifiedType"],
        json!("resqml22.StructuralOrganizationInterpretation"),
        "{interpretation}"
    );
    assert!(
        interpretation
            .get("Fluid")
            .is_none_or(|v| v.as_array().is_none_or(Vec::is_empty)),
        "{interpretation}"
    );
}

#[test]
fn the_same_title_is_the_same_model_and_another_content_another_interpretation() {
    let empty = planned(&Pieces::empty(), Choices::new().set("title", "M")).expect("planned");
    let full = planned(&gathered(), Choices::new().set("title", "M")).expect("planned");
    let model = |plan: &Plan| {
        plan.objects
            .iter()
            .find(|o| o.qualified_type == "resqml22.Model")
            .unwrap()
            .uuid
            .clone()
    };
    let interp = |plan: &Plan| {
        plan.objects
            .iter()
            .find(|o| o.qualified_type == "resqml22.EarthModelInterpretation")
            .unwrap()
            .uuid
            .clone()
    };
    assert_eq!(
        model(&empty),
        model(&full),
        "a second snapshot interprets the same model"
    );
    assert_ne!(interp(&empty), interp(&full), "what it references is what it is");
    let again = planned(&gathered(), Choices::new().set("title", "M")).expect("planned");
    assert_eq!(interp(&full), interp(&again));
}

#[test]
fn an_empty_earth_model_is_written_and_said() {
    let plan = planned(&Pieces::empty(), Choices::new().set("title", "M")).expect("planned");
    assert!(
        plan.says.iter().any(|note| note.key == "empty"),
        "{:?}",
        plan.says
    );
    let full = planned(&gathered(), Choices::new().set("title", "M")).expect("planned");
    assert!(
        !full.says.iter().any(|note| note.key == "empty"),
        "{:?}",
        full.says
    );
}
