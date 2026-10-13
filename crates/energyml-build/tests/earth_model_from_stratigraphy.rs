//! `earth-model-from-stratigraphy` — an earth model made from a unit or a whole
//! column.
//!
//! The package under test is written by `stratigraphy-from-geolts` from the
//! chart extract of `support/geolts_extract.rs`, then read back as a person's
//! package is — the unit reaches the operation through `read::known_of_object`,
//! the stratigraphy with it.
//!
//! **The nesting is confronted with the register.** Which stages lie under the
//! Eocene is computed by `energyml_domain::stratigraphy::subtree` from the ages
//! the units state; the chart states the same fact another way — the `broader`
//! links of the BRGM's SKOS register, which the subtree never reads. The two
//! must name the same units.

use std::sync::Arc;

use serde_json::{Value, json};

use energyml_build::operation::{Choices, Identity, NewObject, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_domain::geolts;
use energyml_domain::stratigraphy::{self, ColumnRank, ColumnUnit, Columns, Named, StratigraphicColumn};
use energyml_storage::EpcStore;

#[path = "support/geolts_extract.rs"]
mod geolts_extract;
use geolts_extract::extract;

const OPERATION: &str = "earth-model-from-stratigraphy";

fn when() -> When {
    When {
        creation: "2026-09-25T00:00:00Z".into(),
        originator: "test".into(),
    }
}

fn choices(answers: &Value) -> Choices {
    let mut choices = Choices::new();
    for (name, value) in answers.as_object().expect("an object") {
        choices = choices.set(name, value.clone());
    }
    choices
}

fn plan(operation: &str, pieces: &Pieces, answers: Value) -> Result<Plan, String> {
    extract();
    Registry::new()
        .plan(
            operation,
            pieces,
            &choices(&answers),
            &Identity::derived(),
            &when(),
        )
        .map_err(|why| why.to_string())
}

/// A package holding the Paleogene of the extract, with the chain above it.
fn paleogene() -> EpcStore {
    let made = plan(
        "stratigraphy-from-geolts",
        &Pieces::empty(),
        json!({ "units": ["Paléogène"] }),
    )
    .expect("a column");
    let mut store = energyml_build::epc::open(None, "model.epc", Arc::new(energyml_schemas::all()))
        .expect("a new package");
    let applied = energyml_build::apply::apply(&made, &mut store, &when());
    assert!(applied.refused.is_empty(), "{:?}", applied.refused);
    store
}

/// The uuid of the object of a type and title.
fn uuid_of(store: &EpcStore, kind: &str, title: &str) -> String {
    store
        .resources()
        .iter()
        .find(|one| {
            one.qualified_type.ends_with(kind)
                && store
                    .get_object(&one.uri)
                    .ok()
                    .and_then(|o| {
                        energyml_core::identity::title_of_reflect(o.as_reflect()).map(str::to_string)
                    })
                    .as_deref()
                    == Some(title)
        })
        .unwrap_or_else(|| panic!("the package holds a {kind} titled {title}"))
        .uuid
        .clone()
}

/// The bag a person makes by pressing the card of `title`: the object read
/// back from the package, and what the empty `existing` slot takes by default.
fn bag(store: &EpcStore, kind: &str, title: &str) -> Pieces {
    let known = energyml_build::read::known_of_object(store, &uuid_of(store, kind, title)).expect("it reads");
    let pieces = Pieces::empty().with("from", Source::Object(known));
    let takes = Registry::new().get(OPERATION).expect("registered").takes();
    energyml_build::read::every(store, &takes, pieces, energyml_geometry::Placement::Local)
        .expect("the defaults fill")
}

fn of_type<'a>(made: &'a Plan, kind: &str) -> Vec<&'a NewObject> {
    made.objects
        .iter()
        .filter(|one| one.qualified_type == kind)
        .collect()
}

fn titles(made: &Plan, kind: &str) -> Vec<String> {
    of_type(made, kind).iter().map(|one| one.title.clone()).collect()
}

fn json_of(one: &NewObject) -> Value {
    one.json.clone()
}

/// The titles a list of references names.
fn named(list: &Value) -> Vec<String> {
    list.as_array()
        .map(|all| {
            all.iter()
                .map(|one| one["Title"].as_str().unwrap_or_default().to_string())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_model_of_the_eocene_holds_what_the_register_puts_under_it() {
    let mut store = paleogene();
    let made = plan(
        OPERATION,
        &bag(&store, "StratigraphicUnitInterpretation", "Eocène"),
        json!({}),
    )
    .expect("a plan");

    // The model is named after the unit.
    assert_eq!(titles(&made, "resqml22.Model"), ["Eocène"]);
    // Every unit the model's column holds, read back from the package: a rank
    // taken as it is is not in the plan, only referenced.
    let applied = energyml_build::apply::apply(&made, &mut store, &when());
    assert!(applied.refused.is_empty(), "{:?}", applied.refused);
    let emi = of_type(&made, "resqml22.EarthModelInterpretation")[0]
        .uuid
        .to_ascii_lowercase();
    let objects: Vec<_> = store
        .resources()
        .iter()
        .filter_map(|one| store.get_object(&one.uri).ok())
        .collect();
    let columns = stratigraphy::columns(objects.iter().map(|one| one.as_reflect()));
    let model_column = columns
        .columns
        .iter()
        .find(|one| one.models.iter().any(|m| m.uuid == emi))
        .expect("the model's column");
    let mut held: Vec<String> = model_column
        .ranks
        .iter()
        .flat_map(|rank| rank.units.iter().map(|unit| unit.unit.title.clone()))
        .collect();
    held.sort();
    held.dedup();
    held.retain(|one| one != "Eocène");

    // The register's own answer: the descendants of the Eocene by `broader`,
    // among the divisions the package holds.
    let in_package: Vec<String> = store
        .resources()
        .iter()
        .filter(|one| one.qualified_type.ends_with("StratigraphicUnitInterpretation"))
        .filter_map(|one| store.get_object(&one.uri).ok())
        .filter_map(|one| energyml_core::identity::title_of_reflect(one.as_reflect()).map(str::to_string))
        .collect();
    let mut register: Vec<String> = geolts::with_default(|chart| {
        let eocene = chart
            .concepts()
            .iter()
            .find(|one| one.label == "Eocène")
            .expect("the Eocene is in the extract")
            .id;
        chart
            .descendants(eocene)
            .into_iter()
            .filter_map(|id| chart.get(id))
            .map(|one| one.label.clone())
            .collect()
    });
    register.retain(|one| in_package.contains(one) && one != "Eocène");
    register.sort();
    assert!(
        !register.is_empty(),
        "the extract puts something under the Eocene — otherwise this proves nothing"
    );
    assert_eq!(
        held, register,
        "the subtree by age and the register's broader links must name the same units"
    );
}

#[test]
fn the_same_content_gives_the_same_objects_and_a_variant_a_second_snapshot() {
    let store = paleogene();
    let bag = bag(&store, "StratigraphicUnitInterpretation", "Eocène");
    let first = plan(OPERATION, &bag, json!({})).expect("a plan");
    let again = plan(OPERATION, &bag, json!({})).expect("a plan");
    let uuids = |made: &Plan| {
        made.objects
            .iter()
            .map(|one| one.uuid.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        uuids(&first),
        uuids(&again),
        "two clicks on one unit make one model"
    );

    let other = plan(OPERATION, &bag, json!({ "variant": "Peter" })).expect("a plan");
    let emi = |made: &Plan| of_type(made, "resqml22.EarthModelInterpretation")[0].uuid.clone();
    assert_ne!(emi(&first), emi(&other), "a variant is a second snapshot");
    // … of the same model, with the same column.
    assert_eq!(
        of_type(&first, "resqml22.Model")[0].uuid,
        of_type(&other, "resqml22.Model")[0].uuid
    );
}

#[test]
fn applied_the_card_of_the_unit_lists_the_model_made_for_it() {
    let mut store = paleogene();
    let made = plan(
        OPERATION,
        &bag(&store, "StratigraphicUnitInterpretation", "Eocène"),
        json!({}),
    )
    .expect("a plan");
    let applied = energyml_build::apply::apply(&made, &mut store, &when());
    assert!(applied.refused.is_empty(), "{:?}", applied.refused);

    let held: Vec<_> = store
        .resources()
        .iter()
        .filter_map(|one| store.get_object(&one.uri).ok())
        .collect();
    let columns = stratigraphy::columns(held.iter().map(|one| one.as_reflect()));
    let emi = of_type(&made, "resqml22.EarthModelInterpretation")[0]
        .uuid
        .to_ascii_lowercase();
    let cards: Vec<&ColumnUnit> = columns
        .columns
        .iter()
        .filter(|column| column.models.is_empty())
        .flat_map(|column| column.ranks.iter())
        .flat_map(|rank| rank.units.iter())
        .collect();
    let card = |title: &str| {
        cards
            .iter()
            .find(|one| one.unit.title == title)
            .unwrap_or_else(|| panic!("a card for {title}"))
    };
    let eocene = card("Eocène");
    assert_eq!(eocene.models.len(), 1, "{:?}", eocene.models);
    assert_eq!(eocene.models[0].emi.uuid, emi);
    assert!(eocene.models[0].root, "made for the Eocene");
    assert!(eocene.models[0].structure, "the tops and bottoms are organised");
    let bartonian = card("Bartonien");
    assert!(
        bartonian
            .models
            .iter()
            .any(|one| one.emi.uuid == emi && !one.root),
        "contained, not made for it"
    );
    assert!(
        card("Paléocène").models.is_empty(),
        "a sister series is in no model of the Eocene"
    );
    // The model's own column is told apart: it goes to the model's tab.
    assert_eq!(
        columns
            .columns
            .iter()
            .filter(|column| column.models.iter().any(|one| one.uuid == emi))
            .count(),
        1
    );
}

#[test]
fn the_tops_and_bottoms_run_from_the_youngest_down() {
    let store = paleogene();
    let made = plan(
        OPERATION,
        &bag(&store, "StratigraphicUnitInterpretation", "Eocène"),
        json!({}),
    )
    .expect("a plan");
    let organisation = json_of(of_type(&made, "resqml22.StructuralOrganizationInterpretation")[0]);
    let order: Vec<String> = organisation["OrderedBoundaryFeatureInterpretation"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|one| {
            one["BoundaryFeatureInterpretation"]["Title"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert_eq!(order.first().map(String::as_str), Some("Eocène TOP"), "{order:?}");
    assert_eq!(
        order.last().map(String::as_str),
        Some("Eocène BOTTOM"),
        "{order:?}"
    );
    // The Priabonian is the youngest stage: its top comes before the
    // Bartonian's, and its bottom before the Bartonian's top — base of A, then
    // top of B.
    let at = |title: &str| {
        order
            .iter()
            .position(|one| one == title)
            .unwrap_or_else(|| panic!("{title} in {order:?}"))
    };
    assert!(at("Priabonien TOP") < at("Priabonien BOTTOM"));
    assert!(at("Priabonien BOTTOM") < at("Bartonien TOP"), "{order:?}");
    assert_eq!(named(&organisation["TopFrontier"]), ["Eocène TOP"]);
    assert_eq!(named(&organisation["BottomFrontier"]), ["Eocène BOTTOM"]);
    // One feature and one interpretation per top and per bottom.
    assert_eq!(of_type(&made, "resqml22.BoundaryFeature").len(), order.len());
    assert_eq!(
        of_type(&made, "resqml22.HorizonInterpretation").len(),
        order.len()
    );
}

/// The occurrence *“expresses the units issued from the
/// stratigraphicColumn”*. Each occurrence is confronted with the rank it
/// names **as the domain reads that rank back from the package** —
/// `stratigraphy::columns`, which knows nothing of occurrences — so the writer
/// of the occurrence and the writer of the rank are not checked by one another.
#[test]
fn the_model_says_which_units_exist_in_it_one_occurrence_per_rank() {
    let mut store = paleogene();
    let made = plan(
        OPERATION,
        &bag(&store, "StratigraphicUnitInterpretation", "Eocène"),
        json!({}),
    )
    .expect("a plan");
    let applied = energyml_build::apply::apply(&made, &mut store, &when());
    assert!(applied.refused.is_empty(), "{:?}", applied.refused);
    assert!(applied.invalid.is_empty(), "{:?}", applied.invalid);

    let emi_object = of_type(&made, "resqml22.EarthModelInterpretation")[0];
    let emi = emi_object.uuid.to_ascii_lowercase();
    let held: Vec<_> = store
        .resources()
        .iter()
        .filter_map(|one| store.get_object(&one.uri).ok())
        .collect();
    let columns = stratigraphy::columns(held.iter().map(|one| one.as_reflect()));
    let model_column = columns
        .columns
        .iter()
        .find(|one| one.models.iter().any(|m| m.uuid == emi))
        .expect("the model's column");
    assert!(
        model_column.ranks.len() > 1,
        "a model of the Eocene has its series and its stages — otherwise one rank proves little"
    );

    let occurrences = of_type(&made, "resqml22.GeologicUnitOccurrenceInterpretation");
    assert_eq!(
        occurrences.len(),
        model_column.ranks.len(),
        "one occurrence per rank of the model's column"
    );
    let mut named_ranks: Vec<String> = Vec::new();
    for occurrence in &occurrences {
        let json = json_of(occurrence);
        let of = json["IsOccurrenceOf"]["Uuid"]
            .as_str()
            .expect("an occurrence of a rank")
            .to_ascii_lowercase();
        let rank = model_column
            .ranks
            .iter()
            .find(|one| one.rank.uuid == of)
            .unwrap_or_else(|| panic!("{of} is a rank of the model's column"));
        let units: Vec<String> = rank.units.iter().map(|one| one.unit.uuid.clone()).collect();
        let listed: Vec<String> = json["GeologicUnit"]
            .as_array()
            .expect("a list")
            .iter()
            .map(|one| one["Uuid"].as_str().unwrap_or_default().to_ascii_lowercase())
            .collect();
        assert!(!listed.is_empty());
        assert_eq!(listed, units, "every unit of the rank, in the rank's order");
        assert_eq!(json["AscendingOrderingCriteria"], "age");
        assert_eq!(
            json["InterpretedFeature"]["Uuid"],
            of_type(&made, "resqml22.Model")[0].uuid.as_str(),
            "it interprets the model"
        );
        named_ranks.push(of);
    }
    named_ranks.sort();
    named_ranks.dedup();
    assert_eq!(named_ranks.len(), occurrences.len(), "no rank twice");

    // The earth model holds them, and nothing else under that name.
    let mut held_by_emi: Vec<String> = json_of(emi_object)["StratigraphicOccurrences"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|one| one["Uuid"].as_str().unwrap_or_default().to_string())
        .collect();
    let mut written: Vec<String> = occurrences.iter().map(|one| one.uuid.clone()).collect();
    held_by_emi.sort();
    written.sort();
    assert_eq!(held_by_emi, written);
}

#[test]
fn switched_off_no_boundary_is_written() {
    let store = paleogene();
    let made = plan(
        OPERATION,
        &bag(&store, "StratigraphicUnitInterpretation", "Eocène"),
        json!({ "boundaries": false }),
    )
    .expect("a plan");
    assert!(of_type(&made, "resqml22.BoundaryFeature").is_empty());
    assert!(of_type(&made, "resqml22.StructuralOrganizationInterpretation").is_empty());
}

// ---------------------------------------------------------------------------
// Pure plans, from a stratigraphy written by hand.
// ---------------------------------------------------------------------------

fn unit(uuid: &str, title: &str, age: Option<[f64; 2]>) -> ColumnUnit {
    ColumnUnit {
        unit: Named {
            uuid: uuid.into(),
            uri: uuid.into(),
            title: title.into(),
            kind: "StratigraphicUnitInterpretation".into(),
        },
        index: 0,
        role: None,
        resolved: true,
        represented: Vec::new(),
        age,
        models: Vec::new(),
    }
}

fn rank(uuid: &str, number: i64, units: Vec<ColumnUnit>) -> ColumnRank {
    ColumnRank {
        rank: Named {
            uuid: uuid.into(),
            uri: uuid.into(),
            title: uuid.into(),
            kind: "StratigraphicColumnRankInterpretation".into(),
        },
        number: Some(number),
        criteria: "age".into(),
        units,
        contacts: Vec::new(),
        notes: Vec::new(),
    }
}

/// A series rank over a stage rank; dated or not.
fn hand_made(dated: bool) -> Columns {
    let age = |old: f64, young: f64| dated.then_some([old, young]);
    Columns {
        columns: vec![StratigraphicColumn {
            column: Named {
                uuid: "col".into(),
                uri: "col".into(),
                title: "column".into(),
                kind: "StratigraphicColumn".into(),
            },
            ranks: vec![
                rank(
                    "series",
                    5,
                    vec![
                        unit("olig", "Oligocène", age(33.9, 23.0)),
                        unit("eoc", "Eocène", age(56.0, 33.9)),
                    ],
                ),
                rank(
                    "stages",
                    7,
                    vec![
                        unit("rup", "Rupélien", age(33.9, 27.8)),
                        unit("pri", "Priabonien", age(37.7, 33.9)),
                        unit("bar", "Bartonien", age(41.2, 37.7)),
                    ],
                ),
            ],
            models: Vec::new(),
        }],
        loose_ranks: Vec::new(),
        notes: Vec::new(),
    }
}

fn from(columns: Columns) -> Source {
    Source::Object(Known {
        reference: Some(Ref::new(
            "eoc",
            "resqml22.StratigraphicUnitInterpretation",
            "Eocène",
        )),
        stratigraphy: Some(columns),
        ..Known::default()
    })
}

fn object(uuid: &str, kind: &str, title: &str) -> Source {
    Source::Object(Known {
        reference: Some(Ref::new(uuid, kind, title)),
        ..Known::default()
    })
}

#[test]
fn a_top_the_package_already_holds_is_taken_whatever_its_case() {
    let bag = Pieces::empty().with("from", from(hand_made(true))).with(
        "existing",
        object("mine", "resqml22.HorizonInterpretation", "eocène top"),
    );
    let made = plan(OPERATION, &bag, json!({})).expect("a plan");
    assert!(
        !titles(&made, "resqml22.HorizonInterpretation")
            .iter()
            .any(|one| one == "Eocène TOP"),
        "taken, not written"
    );
    let organisation = json_of(of_type(&made, "resqml22.StructuralOrganizationInterpretation")[0]);
    assert_eq!(
        organisation["TopFrontier"][0]["Uuid"], "mine",
        "the package's own top is the model's"
    );
}

#[test]
fn two_objects_of_one_title_are_refused_by_name() {
    let bag = Pieces::empty()
        .with("from", from(hand_made(true)))
        .with(
            "existing",
            object("a", "resqml22.HorizonInterpretation", "Bartonien BOTTOM"),
        )
        .with(
            "existing 2",
            object("b", "resqml22.HorizonInterpretation", "BARTONIEN bottom"),
        );
    let refused = plan(OPERATION, &bag, json!({})).expect_err("which one is not said");
    assert!(
        refused.contains("Bartonien BOTTOM") && refused.contains("(a)") && refused.contains("(b)"),
        "{refused}"
    );
}

#[test]
fn undated_units_get_their_horizons_and_no_organisation() {
    // Without ages the stages are placed by the units the ranks share — none
    // here — so the model holds the Eocene alone; its top and bottom are
    // written, and no order is claimed.
    let bag = Pieces::empty().with("from", from(hand_made(false)));
    let made = plan(OPERATION, &bag, json!({})).expect("a plan");
    assert!(of_type(&made, "resqml22.StructuralOrganizationInterpretation").is_empty());
    assert_eq!(
        titles(&made, "resqml22.HorizonInterpretation"),
        ["Eocène TOP", "Eocène BOTTOM"]
    );
    assert!(
        made.says.iter().any(|note| note.key == "structure-unordered"),
        "{:?}",
        made.says
    );
    assert!(
        made.dropped.iter().any(|one| one.why.contains("shares no unit")),
        "{:?}",
        made.dropped
    );
}

#[test]
fn enriching_keeps_the_uuid_it_is_given_and_asks_for_one() {
    let refused = plan(
        OPERATION,
        &Pieces::empty().with("from", from(hand_made(true))),
        json!({ "mode": "enrich" }),
    )
    .expect_err("nothing to enrich");
    assert!(refused.contains("enrich"), "{refused}");

    let bag = Pieces::empty().with("from", from(hand_made(true))).with(
        "enrich",
        object(
            "kept-uuid",
            "resqml22.EarthModelInterpretation",
            "Eocène — earth model",
        ),
    );
    let made = plan(OPERATION, &bag, json!({ "mode": "enrich" })).expect("a plan");
    assert_eq!(
        of_type(&made, "resqml22.EarthModelInterpretation")[0].uuid,
        "kept-uuid"
    );

    let refused =
        plan(OPERATION, &bag, json!({})).expect_err("an interpretation to enrich, in snapshot mode");
    assert!(refused.contains("snapshot"), "{refused}");
}

#[test]
fn a_whole_column_is_modelled_as_it_is() {
    let columns = hand_made(true);
    let bag = Pieces::empty().with(
        "from",
        Source::Object(Known {
            reference: Some(Ref::new("col", "resqml22.StratigraphicColumn", "column")),
            stratigraphy: Some(columns),
            ..Known::default()
        }),
    );
    let made = plan(OPERATION, &bag, json!({ "boundaries": false })).expect("a plan");
    assert!(
        of_type(&made, "resqml22.StratigraphicColumnRankInterpretation").is_empty(),
        "every rank taken as it is"
    );
    assert!(
        of_type(&made, "resqml22.StratigraphicColumn").is_empty(),
        "and the column too"
    );
    let emi = json_of(of_type(&made, "resqml22.EarthModelInterpretation")[0]);
    assert_eq!(emi["StratigraphicColumn"]["Uuid"], "col");

    // The ranks taken as they are still get their occurrence: what exists in
    // the model is said even when nothing of the column is rewritten.
    let occurrences: Vec<Value> = of_type(&made, "resqml22.GeologicUnitOccurrenceInterpretation")
        .iter()
        .map(|one| json_of(one))
        .collect();
    let of: Vec<&str> = occurrences
        .iter()
        .map(|one| one["IsOccurrenceOf"]["Uuid"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(of, ["series", "stages"]);
    assert_eq!(named(&occurrences[0]["GeologicUnit"]), ["Oligocène", "Eocène"]);
    assert_eq!(
        named(&occurrences[1]["GeologicUnit"]),
        ["Rupélien", "Priabonien", "Bartonien"]
    );
    assert_eq!(emi["StratigraphicOccurrences"].as_array().map(Vec::len), Some(2));
}

#[test]
fn a_unit_of_another_release_is_refused_by_name() {
    let bag = Pieces::empty().with(
        "from",
        Source::Object(Known {
            reference: Some(Ref::new(
                "eoc",
                "resqml20.obj_StratigraphicUnitInterpretation",
                "Eocène",
            )),
            stratigraphy: Some(hand_made(true)),
            ..Known::default()
        }),
    );
    let refused = plan(OPERATION, &bag, json!({})).expect_err("2.0.1");
    assert!(refused.contains("translate the package first"), "{refused}");
}
