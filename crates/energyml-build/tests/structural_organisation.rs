//! `structural-organisation` — the boundaries of a structural scenario in the
//! order it states, its frontiers and its contacts (step 7 of the reference model).
//!
//! The form of the assertions comes from the schema: one
//! `OrderedBoundaryFeatureInterpretation` whose position is the order, and an
//! `AscendingOrderingCriteria` (Interpretations.xsd l. 976–1012); a contact is
//! *subject — verb — direct object*, each side a `ContactElement` with its
//! qualifiers (l. 533–566); a frontier is a cultural feature or a boundary
//! interpretation (l. 994–1008); the feature an organisation interprets is the
//! reference model's `Model`, the one `earth-model` writes. The uuids and
//! titles are the tests' own, named like Lutetia's.
//!
//! # What would have to break to make it turn red
//!
//! Sorting the list instead of keeping it; writing another criterion than the one stated;
//! writing its own `Model` instead of the one of `earth-model`; accepting both
//! `model` and `modelTitle`; taking `F1` for `F10`; inverting subject and
//! object of a contact, or losing a side; accepting a generic
//! interpretation as a frontier or as a named member; keeping a
//! limit twice; naming the organisation after something other than what it says.

use serde_json::json;

use energyml_build::operation::{Choices, Identity, NewObject, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_domain::provenance::Parameter;

fn when() -> When {
    When {
        creation: "2026-09-24T00:00:00Z".into(),
        originator: "test".into(),
    }
}

fn object(uuid: &str, qualified_type: &str, title: &str) -> Source {
    Source::Object(Known {
        reference: Some(Ref::new(uuid, qualified_type, title)),
        ..Default::default()
    })
}

const MODEL: &str = "0a000000-0000-4000-8000-000000000100";
const F1: &str = "0f000000-0000-4000-8000-000000000201";
const F2: &str = "0f000000-0000-4000-8000-000000000202";
const F3: &str = "0f000000-0000-4000-8000-000000000203";
const F4: &str = "0f000000-0000-4000-8000-000000000204";
const H6: &str = "0b000000-0000-4000-8000-000000000135";
const GROUND: &str = "0b000000-0000-4000-8000-000000000138";
const FRONTIER: &str = "0c000000-0000-4000-8000-000000000050";

fn fault(uuid: &str, name: &str) -> Source {
    object(
        uuid,
        "resqml22.FaultInterpretation",
        &format!("{name} — interpretation"),
    )
}

/// Scenario A as it can be written today, H6 standing for the horizons: the
/// model, four boundaries in the order stated, the ground above and the
/// model's frontier around.
fn scenario() -> Pieces {
    Pieces::empty()
        .with("model", object(MODEL, "resqml22.Model", "Lutetia"))
        .with(
            "boundaries",
            object(
                H6,
                "resqml22.HorizonInterpretation",
                "H6 — top of the Chalk — interpretation",
            ),
        )
        .with("boundaries 2", fault(F2, "F2"))
        .with("boundaries 3", fault(F1, "F1"))
        .with("boundaries 4", fault(F3, "F3"))
        .with(
            "top",
            object(
                GROUND,
                "resqml22.BoundaryFeatureInterpretation",
                "Lutetia — sol — interpretation",
            ),
        )
        .with(
            "sides",
            object(
                FRONTIER,
                "resqml22.CulturalFeature",
                "Lutetia — emprise — feature",
            ),
        )
}

fn answers() -> Choices {
    Choices::new()
        .set("title", "Lutetia — scenario A")
        .set("ordering", "age")
}

fn planned(pieces: &Pieces, choices: &Choices) -> Result<Plan, energyml_build::operation::Error> {
    Registry::new().plan(
        "structural-organisation",
        pieces,
        choices,
        &Identity::derived(),
        &when(),
    )
}

fn organisation(plan: &Plan) -> &NewObject {
    plan.objects
        .iter()
        .find(|o| o.qualified_type == "resqml22.StructuralOrganizationInterpretation")
        .expect("an organisation")
}

fn members(plan: &Plan) -> Vec<String> {
    organisation(plan).json["OrderedBoundaryFeatureInterpretation"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|entry| {
            entry["BoundaryFeatureInterpretation"]["Uuid"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}

#[test]
fn the_organisation_interprets_the_model_and_keeps_its_boundaries_in_the_order_given() {
    let plan = planned(&scenario(), &answers()).expect("planned");
    assert_eq!(
        plan.objects.len(),
        1,
        "the model is given: only the organisation is written"
    );
    let written = &organisation(&plan).json;
    assert_eq!(written["InterpretedFeature"]["Uuid"], json!(MODEL));
    assert_eq!(
        written["InterpretedFeature"]["QualifiedType"],
        json!("resqml22.Model")
    );
    assert_eq!(members(&plan), vec![H6, F2, F1, F3]);
    assert_eq!(written["AscendingOrderingCriteria"], json!("age"));
    assert_eq!(written["TopFrontier"][0]["Uuid"], json!(GROUND));
    assert_eq!(written["Sides"][0]["Uuid"], json!(FRONTIER));
    assert!(
        written.get("BottomFrontier").is_none(),
        "none given, none written: {written}"
    );

    // Another order is another statement, written as it is.
    let reversed = Pieces::empty()
        .with("model", object(MODEL, "resqml22.Model", "Lutetia"))
        .with("boundaries", fault(F3, "F3"))
        .with("boundaries 2", fault(F1, "F1"))
        .with("boundaries 3", fault(F2, "F2"))
        .with(
            "boundaries 4",
            object(
                H6,
                "resqml22.HorizonInterpretation",
                "H6 — top of the Chalk — interpretation",
            ),
        );
    let plan = planned(&reversed, &answers()).expect("planned");
    assert_eq!(members(&plan), vec![F3, F1, F2, H6]);
}

#[test]
fn the_criterion_is_written_as_said_and_has_no_default() {
    let plan = planned(&scenario(), &answers().set("ordering", "apparent depth")).expect("planned");
    assert_eq!(
        organisation(&plan).json["AscendingOrderingCriteria"],
        json!("apparent depth")
    );
    let why = planned(&scenario(), &Choices::new().set("title", "Without order")).expect_err("no criterion");
    assert!(
        why.refusals()
            .iter()
            .any(|refusal| refusal.slot.as_deref() == Some("ordering")),
        "{why}"
    );
}

/// Written before the earth model that gathers it, the organisation names the
/// model by its title — and it is the **same** object `earth-model` writes.
#[test]
fn a_model_named_by_its_title_is_the_one_earth_model_writes() {
    let mut bag = Pieces::empty();
    for (name, source) in scenario().iter().filter(|(name, _)| *name != "model") {
        bag = bag.with(name, source.clone());
    }
    let plan = planned(&bag, &answers().set("modelTitle", "Lutetia")).expect("planned");
    let model = plan
        .objects
        .iter()
        .find(|o| o.qualified_type == "resqml22.Model")
        .expect("the model, written here");
    let earth = Registry::new()
        .plan(
            "earth-model",
            &Pieces::empty(),
            &Choices::new().set("title", "Lutetia"),
            &Identity::derived(),
            &when(),
        )
        .expect("an earth model");
    let theirs = earth
        .objects
        .iter()
        .find(|o| o.qualified_type == "resqml22.Model")
        .expect("its model");
    assert_eq!(model.uuid, theirs.uuid, "one model, whichever writes it first");
    assert_eq!(model.json, theirs.json, "and the same object, byte for byte");
    assert_eq!(
        organisation(&plan).json["InterpretedFeature"]["Uuid"],
        json!(theirs.uuid)
    );
}

#[test]
fn the_model_is_given_one_way_and_a_title_names_it_only_under_derived_uuids() {
    let why = planned(&scenario(), &answers().set("modelTitle", "Lutetia")).expect_err("both");
    assert_eq!(why.refusals()[0].slot.as_deref(), Some("modelTitle"), "{why}");
    let mut bag = Pieces::empty();
    for (name, source) in scenario().iter().filter(|(name, _)| *name != "model") {
        bag = bag.with(name, source.clone());
    }
    let why = planned(&bag, &answers()).expect_err("neither");
    assert_eq!(why.refusals()[0].slot.as_deref(), Some("model"), "{why}");
    let given = Identity::Given(
        (0..4)
            .map(|n| format!("00000000-0000-4000-8000-{n:012}"))
            .collect(),
    );
    let why = Registry::new()
        .plan(
            "structural-organisation",
            &bag,
            &answers().set("modelTitle", "Lutetia"),
            &given,
            &when(),
        )
        .expect_err("a title under given uuids");
    assert_eq!(why.refusals()[0].slot.as_deref(), Some("modelTitle"), "{why}");
    // A model that is no `Model`: refused by its type.
    let mut wrong = Pieces::empty().with(
        "model",
        object(
            "0d000000-0000-4000-8000-000000000001",
            "resqml22.BoundaryFeature",
            "Not a model",
        ),
    );
    for (name, source) in scenario().iter().filter(|(name, _)| *name != "model") {
        wrong = wrong.with(name, source.clone());
    }
    let why = planned(&wrong, &answers()).expect_err("no model");
    assert_eq!(why.refusals()[0].slot.as_deref(), Some("model"), "{why}");
}

#[test]
fn a_rank_lands_on_the_boundary_its_name_designates_and_f1_is_never_f10() {
    let plan = planned(&scenario(), &answers().set("ranks", json!(["H6 = 4"]))).expect("planned");
    let list = &organisation(&plan).json["OrderedBoundaryFeatureInterpretation"];
    assert_eq!(list[0]["StratigraphicRank"], json!(4), "{list}");
    assert!(
        list[1].get("StratigraphicRank").is_none(),
        "a fault has no rank unless one is said: {list}"
    );

    let bag = scenario().with(
        "boundaries 5",
        fault("0f000000-0000-4000-8000-000000000210", "F10"),
    );
    let plan = planned(&bag, &answers().set("ranks", json!(["F1 = 2"]))).expect("planned");
    let list = &organisation(&plan).json["OrderedBoundaryFeatureInterpretation"];
    assert_eq!(list[2]["BoundaryFeatureInterpretation"]["Uuid"], json!(F1));
    assert_eq!(list[2]["StratigraphicRank"], json!(2), "{list}");
    assert!(
        list[4].get("StratigraphicRank").is_none(),
        "`F1` does not name `F10`: {list}"
    );

    // Two titles a name begins: refused, never chosen.
    let bag = scenario().with(
        "boundaries 5",
        object(
            "0f000000-0000-4000-8000-000000000211",
            "resqml22.FaultInterpretation",
            "F1 — interpretation (B)",
        ),
    );
    let why = planned(&bag, &answers().set("ranks", json!(["F1 = 2"]))).expect_err("ambiguous");
    assert!(why.message().contains("could be"), "{why}");
    let why = planned(&scenario(), &answers().set("ranks", json!(["F9 = 1"]))).expect_err("unknown");
    assert!(why.message().contains("names none"), "{why}");
    let why = planned(&scenario(), &answers().set("ranks", json!(["H6 = -1"]))).expect_err("negative");
    assert!(why.message().contains("not a rank"), "{why}");
}

/// Three cases of the reference model: a fault crossing a fault, an
/// erosion stopping a fault, the frontier stopping a horizon.
#[test]
fn a_contact_is_the_sentence_subject_verb_object_with_its_sides() {
    let contacts = json!([
        "F3 {east} crosses F1 {west}",
        "H6 {older} stops F2 {both}",
        "Lutetia — emprise stops H6 {both}"
    ]);
    let plan = planned(&scenario(), &answers().set("contacts", contacts)).expect("planned");
    let written = &organisation(&plan).json["ContactInterpretation"];
    assert_eq!(written.as_array().map(Vec::len), Some(3), "{written}");
    let k05 = &written[0];
    assert_eq!(k05["$type"], json!("resqml22.BinaryContactInterpretationPart"));
    assert_eq!(k05["Subject"]["Uuid"], json!(F3));
    assert_eq!(k05["Subject"]["Qualifier"], json!("east"));
    assert_eq!(k05["Verb"], json!("crosses"));
    assert_eq!(k05["DirectObject"]["Uuid"], json!(F1));
    assert_eq!(k05["DirectObject"]["Qualifier"], json!("west"));
    let k04 = &written[1];
    assert_eq!(k04["Subject"]["Uuid"], json!(H6));
    assert_eq!(k04["Subject"]["Qualifier"], json!("older"));
    assert_eq!(k04["Verb"], json!("stops"));
    assert_eq!(k04["DirectObject"]["Uuid"], json!(F2));
    assert_eq!(k04["DirectObject"]["Qualifier"], json!("both"));
    let k07 = &written[2];
    assert_eq!(
        k07["Subject"]["Uuid"],
        json!(FRONTIER),
        "a frontier feature may be a subject (TRG 2.2 §6.9)"
    );
    assert_eq!(k07["Subject"]["QualifiedType"], json!("resqml22.CulturalFeature"));
    assert!(
        k07["Subject"].get("Qualifier").is_none(),
        "no side said, none written: {k07}"
    );
    assert!(
        !plan
            .says
            .iter()
            .any(|note| note.key == "contact-side" || note.key == "contact-mode"),
        "{:?}",
        plan.says
    );
}

#[test]
fn a_contact_that_names_nothing_no_verb_or_no_side_is_refused() {
    for (contact, said) in [
        ("F9 {east} crosses F1", "names none"),
        ("F3 {east} meets F1", "names no verb"),
        ("F3 {sideways} crosses F1", "neither a side"),
        ("F3 crosses F3", "one object"),
        ("F3 {east, west} crosses F1", "two sides"),
    ] {
        let why = planned(&scenario(), &answers().set("contacts", json!([contact]))).expect_err(contact);
        assert_eq!(
            why.refusals()[0].slot.as_deref(),
            Some("contacts"),
            "{contact}: {why}"
        );
        assert!(why.message().contains(said), "{contact}: {why}");
    }
}

/// What the usage guide says and the schema does not is said, never refused.
#[test]
fn a_mode_on_a_boundary_and_a_fault_s_side_on_a_horizon_are_said_and_written() {
    let contacts = json!([
        "H6 {older, unconformable} stops F2 {both}",
        "F2 {hanging wall} splits H6 {footwall}"
    ]);
    let plan = planned(&scenario(), &answers().set("contacts", contacts)).expect("planned");
    assert!(
        plan.says
            .iter()
            .any(|note| note.key == "contact-mode" && note.detail.contains("unconformable")),
        "{:?}",
        plan.says
    );
    assert!(
        plan.says
            .iter()
            .any(|note| note.key == "contact-side" && note.detail.contains("footwall")),
        "{:?}",
        plan.says
    );
    let written = &organisation(&plan).json["ContactInterpretation"];
    assert_eq!(
        written[0]["Subject"]["SecondaryQualifier"],
        json!("unconformable")
    );
    assert_eq!(written[1]["DirectObject"]["Qualifier"], json!("footwall"));
}

#[test]
fn a_frontier_is_a_cultural_feature_or_a_boundary_interpretation() {
    let generic = scenario().with(
        "bottom",
        object(
            "0c000000-0000-4000-8000-000000000051",
            "resqml22.GenericFeatureInterpretation",
            "Lutetia — emprise — interpretation",
        ),
    );
    let why = planned(&generic, &answers()).expect_err("a generic interpretation");
    assert_eq!(why.refusals()[0].slot.as_deref(), Some("bottom"), "{why}");
    assert!(why.message().contains("l. 994–1008"), "{why}");
    // The cultural feature is taken, and what cannot be checked of it is said.
    let plan = planned(&scenario(), &answers()).expect("planned");
    assert!(
        plan.says.iter().any(|note| note.key == "frontier-kind"),
        "{:?}",
        plan.says
    );
}

#[test]
fn a_member_twice_or_named_and_no_boundary_is_refused_and_an_unnamed_one_is_passed_over() {
    let twice = scenario().with("boundaries 5", fault(F1, "F1"));
    let why = planned(&twice, &answers()).expect_err("twice");
    assert!(why.message().contains("given twice"), "{why}");

    let named = scenario().with(
        "boundaries 5",
        object(
            "0c000000-0000-4000-8000-000000000051",
            "resqml22.GenericFeatureInterpretation",
            "Lutetia — emprise — interpretation",
        ),
    );
    let why = planned(&named, &answers()).expect_err("no boundary");
    assert!(why.message().contains("l. 593–598"), "{why}");

    // An object piece nobody named goes to the one required object slot —
    // how a form's answer arrives — and is passed over, counted.
    let unnamed = scenario().with(
        "an answer",
        object(
            "0c000000-0000-4000-8000-000000000051",
            "resqml22.GenericFeatureInterpretation",
            "Lutetia — emprise — interpretation",
        ),
    );
    let plan = planned(&unnamed, &answers()).expect("planned");
    assert_eq!(members(&plan), vec![H6, F2, F1, F3]);
    assert_eq!(
        plan.dropped.iter().map(|one| one.count).sum::<usize>(),
        1,
        "{:?}",
        plan.dropped
    );
}

#[test]
fn the_same_content_is_the_same_organisation_and_anything_said_otherwise_is_another() {
    let uuid = |pieces: &Pieces, choices: &Choices| {
        organisation(&planned(pieces, choices).expect("planned"))
            .uuid
            .clone()
    };
    let first = uuid(&scenario(), &answers());
    assert_eq!(first, uuid(&scenario(), &answers()), "same content, same object");
    let reordered = Pieces::empty()
        .with("model", object(MODEL, "resqml22.Model", "Lutetia"))
        .with(
            "boundaries",
            object(
                H6,
                "resqml22.HorizonInterpretation",
                "H6 — top of the Chalk — interpretation",
            ),
        )
        .with("boundaries 2", fault(F1, "F1"))
        .with("boundaries 3", fault(F2, "F2"))
        .with("boundaries 4", fault(F3, "F3"))
        .with(
            "top",
            object(
                GROUND,
                "resqml22.BoundaryFeatureInterpretation",
                "Lutetia — sol — interpretation",
            ),
        )
        .with(
            "sides",
            object(
                FRONTIER,
                "resqml22.CulturalFeature",
                "Lutetia — emprise — feature",
            ),
        );
    assert_ne!(first, uuid(&reordered, &answers()), "an order is content");
    assert_ne!(
        first,
        uuid(
            &scenario(),
            &answers().set("contacts", json!(["F3 {east} crosses F1 {west}"]))
        ),
        "a contact is content"
    );
    assert_ne!(
        first,
        uuid(&scenario(), &answers().set("ranks", json!(["H6 = 4"]))),
        "a rank is content"
    );
    assert_ne!(
        first,
        uuid(&scenario(), &answers().set("ordering", "apparent depth")),
        "a criterion is content"
    );
    assert_ne!(
        first,
        uuid(&scenario(), &answers().set("title", "Lutetia — scenario B")),
        "two scenarios are two"
    );
    assert_ne!(
        first,
        uuid(&scenario().with("boundaries 5", fault(F4, "F4")), &answers()),
        "a boundary added is another snapshot"
    );
}

#[test]
fn an_organisation_of_faults_only_says_so() {
    let b = Pieces::empty()
        .with("model", object(MODEL, "resqml22.Model", "Lutetia"))
        .with("boundaries", fault(F1, "F1"))
        .with("boundaries 2", fault(F2, "F2"))
        .with("boundaries 3", fault(F3, "F3"))
        .with("boundaries 4", fault(F4, "F4"));
    let plan = planned(
        &b,
        &answers()
            .set("title", "Lutetia — scenario B")
            .set("contacts", json!(["F4 {footwall} stops F1 {both}"])),
    )
    .expect("planned");
    assert!(
        plan.says.iter().any(|note| note.key == "faults-only"),
        "{:?}",
        plan.says
    );
    let with_horizon = planned(&scenario(), &answers()).expect("planned");
    assert!(
        !with_horizon.says.iter().any(|note| note.key == "faults-only"),
        "{:?}",
        with_horizon.says
    );
}

#[test]
fn the_activity_records_the_lists_it_was_told_and_what_it_counted() {
    let plan = planned(
        &scenario(),
        &answers()
            .set("ranks", json!(["H6 = 4"]))
            .set("contacts", json!(["F3 {east} crosses F1 {west}"])),
    )
    .expect("planned");
    let texts = |title: &str| -> Vec<String> {
        plan.activity
            .parameters
            .iter()
            .filter_map(|one| match one {
                Parameter::Text { title: t, value } if t == title => Some(value.clone()),
                _ => None,
            })
            .collect()
    };
    assert_eq!(texts("ranks"), vec!["H6 = 4".to_string()]);
    assert_eq!(texts("contacts"), vec!["F3 {east} crosses F1 {west}".to_string()]);
    let count = |title: &str| {
        plan.activity.parameters.iter().find_map(|one| match one {
            Parameter::Double { title: t, value, .. } if t == title => Some(*value),
            _ => None,
        })
    };
    assert_eq!(count("memberCount"), Some(4.0));
    assert_eq!(count("faultCount"), Some(3.0));
    assert_eq!(count("contactCount"), Some(1.0));
    assert_eq!(plan.activity.recipe, "structural-organisation");
}

/// The order of `boundaries` is declared, so that a form renders a list to
/// reorder and not boxes to tick.
#[test]
fn the_boundaries_are_declared_in_order() {
    let registry = Registry::new();
    let operation = registry.get("structural-organisation").expect("registered");
    let takes = operation.takes();
    let boundaries = takes
        .iter()
        .find(|slot| slot.name == "boundaries")
        .expect("a boundaries slot");
    assert!(boundaries.ordered && boundaries.least == 1, "{boundaries:?}");
}
