//! What an operation declares, as the one JSON document the front end, the command
//! line and a workflow server read (`energyml_build::describe`, 07/10/2026).
//!
//! The values asserted below are the ones the **declaration code** states - the
//! `#[param(default = …)]` lines of `ops/wells_and_properties.rs` and the
//! `Output` lists of the six chain operations - not what a run printed.

use energyml_build::describe;
use energyml_build::operation::Registry;
use serde_json::Value;

/// The six operations that declare `gives`.
const CHAIN: [&str; 6] = [
    "wells-and-properties-from-table",
    "wells-from-table",
    "properties-from-table",
    "contacts-from-frames",
    "pointset-from-wellheads",
    "stratigraphy-from-geolts",
];

fn slot<'a>(declaration: &'a Value, list: &str, name: &str) -> &'a Value {
    declaration[list]
        .as_array()
        .unwrap_or_else(|| panic!("`{list}` is a list"))
        .iter()
        .find(|slot| slot["name"] == name)
        .unwrap_or_else(|| panic!("no slot `{name}` in `{list}`"))
}

#[test]
fn every_built_in_operation_has_a_declaration_that_serialises() {
    let registry = Registry::new();
    let all = describe::declarations(&registry);
    let list = all.as_array().expect("an array");
    assert_eq!(list.len(), registry.iter().count(), "one entry per operation");
    for (one, operation) in list.iter().zip(registry.iter()) {
        // The string form is what a script parses.
        let text = serde_json::to_string(one).expect("serialises");
        let back: Value = serde_json::from_str(&text).expect("parses back");
        assert_eq!(&back, one);
        assert_eq!(one["name"], operation.name());
        for key in [
            "describe", "makes", "recipe", "category", "also", "needs", "takes", "reports", "gives",
        ] {
            assert!(!one[key].is_null(), "{}: `{key}` is missing", operation.name());
        }
        assert_eq!(one["gives"].as_array().unwrap().len(), operation.gives().len());
    }
}

#[test]
fn the_six_chain_operations_carry_the_roles_they_give() {
    let registry = Registry::new();
    for name in CHAIN {
        let one = describe::declaration_of(&registry, name).unwrap_or_else(|| panic!("{name} is registered"));
        let gives = one["gives"].as_array().unwrap();
        assert!(!gives.is_empty(), "{name} declares what it gives");
        for output in gives {
            assert!(
                output["role"].is_string() && output["describe"].is_string(),
                "{name}: {output}"
            );
            // `types` may be empty: *the caller's choice* (`pointset-from-wellheads`
            // writes the feature of the type `featureType` names).
            assert!(output["types"].is_array(), "{name}: a role lists its types");
            assert!(output["least"].is_u64(), "{name}: `least` is a count");
        }
    }
    // `contacts-from-frames` writes its point sets under `representation`,
    // at least one (`ops/contacts.rs`, `gives`).
    let contacts = describe::declaration_of(&registry, "contacts-from-frames").unwrap();
    let representation = contacts["gives"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["role"] == "representation")
        .expect("a representation role");
    assert_eq!(representation["types"][0], "resqml22.PointSetRepresentation");
    assert_eq!(representation["least"], 1);
    assert!(representation["most"].is_null(), "unbounded is null");
}

#[test]
fn an_unknown_operation_has_no_declaration() {
    assert!(describe::declaration_of(&Registry::new(), "no-such-operation").is_none());
}

#[test]
fn wells_and_properties_names_its_slots_with_their_defaults_and_choices() {
    let one = describe::declaration_of(&Registry::new(), "wells-and-properties-from-table").unwrap();
    for name in [
        "units",
        "unitLabels",
        "sourceEpsg",
        "project",
        "textColumns",
        "language",
    ] {
        slot(&one, "needs", name);
    }
    // `#[param(default = Language::Fr, optional)]`: the default is a suggestion,
    // the choices are the wire words of the enum.
    let language = slot(&one, "needs", "language");
    assert_eq!(language["suggested"], "fr");
    assert_eq!(language["need"]["kind"], "choice");
    assert_eq!(language["need"]["choices"], serde_json::json!(["fr", "en"]));
    // `#[param(default = TextColumns::Separate, optional)]`.
    let text_columns = slot(&one, "needs", "textColumns");
    assert_eq!(text_columns["suggested"], "separate");
    assert_eq!(
        text_columns["need"]["choices"],
        serde_json::json!(["separate", "shared-table", "table-per-well"])
    );
    // `Option<Epsg>` and `Option<String>`: optional, no default, and typed.
    for (name, kind) in [
        ("sourceEpsg", "epsg"),
        ("project", "epsg"),
        ("units", "text"),
        ("unitLabels", "text"),
    ] {
        let one = slot(&one, "needs", name);
        assert_eq!(one["need"]["kind"], kind, "{name}");
        assert_eq!(one["required"], false, "{name}");
        assert!(one["suggested"].is_null(), "{name} has no default");
    }
}

#[test]
fn what_it_takes_is_declared_with_its_slots() {
    let one = describe::declaration_of(&Registry::new(), "contacts-from-frames").unwrap();
    let takes = one["takes"].as_array().unwrap();
    assert!(!takes.is_empty(), "contacts-from-frames takes frames");
    for taken in takes {
        assert_eq!(taken["need"]["kind"], "input");
        for key in ["name", "about", "least", "most", "takesMany", "applies", "group"] {
            assert!(taken.get(key).is_some(), "a slot says `{key}`");
        }
    }
}
