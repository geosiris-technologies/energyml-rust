//! The run manifest — `energyml_build::manifest`,
//! (ADR 0017).
//!
//! # What is confronted with what
//!
//! The manifest is built from the **plan** and from what `apply` **returned**;
//! asserting it against those alone would ask the builder to vouch for itself.
//! Every claim is therefore read **back from the package**: each uri must
//! resolve in the store, the objects of a role in the store must be exactly the
//! role's uris, and the `collection` is a `DataobjectCollection` the store
//! holds, whose members are named by the `CollectionsToDataobjectsAssociationSet`
//! the store holds (`Collection.xsd`: `SingleCollectionAssociation` has
//! `Collection` 1..1 and `Dataobject` 1..∞) — read from the store, not from
//! `apply`'s own return.
//!
//! The operations are declared **in this file**: a fake one in the shipped
//! registry would be an entry the front end offers somebody.
//!
//! # What would make them fail
//!
//! A uri built from the uuid alone (it would not resolve once a version is
//! declared); a collection uri derived twice and differently; a role listed from
//! the declaration but not from the plan, or the reverse; a refused object still
//! listed; `declared` read from the types instead of the role.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};

use energyml_build::apply::{self, apply};
use energyml_build::entries::Reader;
use energyml_build::manifest::{self, FORMAT, Manifest};
use energyml_build::operation::{
    ActivityDraft, AnyOperation, Choices, Error, Fitness, Identity, NewObject, Output, Plan, Registry, Slot,
    When,
};
use energyml_build::pieces::Pieces;
use energyml_build::recipe::{self, Mode, Recipe};
use energyml_build::source::Source as Where;
use energyml_storage::EpcStore;

fn when() -> When {
    When {
        creation: "2026-10-07T00:00:00Z".into(),
        originator: "tests/manifest.rs".into(),
    }
}

fn empty_store() -> EpcStore {
    energyml_build::epc::open(None, "manifest.epc", Arc::new(energyml_schemas::all()))
        .expect("an empty package")
}

/// Writes `items` features of role `item`, then one of role `other`; declares
/// `item` and a role `ghost` it never writes — or nothing, when `declared` is off.
struct Fake {
    name: &'static str,
    items: usize,
    declared: bool,
}

impl AnyOperation for Fake {
    fn name(&self) -> &'static str {
        self.name
    }
    fn describe(&self) -> &'static str {
        "a test operation"
    }
    fn makes(&self) -> &'static [&'static str] {
        &["resqml22.BoundaryFeature"]
    }
    fn recipe(&self) -> &'static str {
        "crs"
    }
    fn fitness(&self, _pieces: &Pieces) -> Fitness {
        Fitness::Fit
    }
    fn needs(&self, _pieces: &Pieces) -> Vec<Slot> {
        Vec::new()
    }
    fn gives(&self) -> Vec<Output> {
        if !self.declared {
            return Vec::new();
        }
        vec![
            Output {
                role: "item",
                types: &["resqml22.BoundaryFeature"],
                least: 1,
                most: None,
                describe: "the items",
            },
            Output {
                role: "ghost",
                types: &["resqml22.HorizonInterpretation"],
                least: 0,
                most: Some(1),
                describe: "never written",
            },
        ]
    }
    fn plan(
        &self,
        _pieces: &Pieces,
        _choices: &Choices,
        identity: &Identity,
        when: &When,
    ) -> Result<Plan, Error> {
        let object = |role: &str, title: String, seed: String| -> Result<NewObject, Error> {
            let uuid = identity.uuid(&seed, 0)?;
            Ok(NewObject {
                uuid: uuid.clone(),
                qualified_type: "resqml22.BoundaryFeature".to_string(),
                role: role.to_string(),
                json: json!({
                    "$type": "resqml22.BoundaryFeature",
                    "uuid": uuid,
                    "schemaVersion": "2.2",
                    "Citation": { "$type": "eml23.Citation", "Title": title, "Originator": when.originator, "Creation": when.creation },
                    "IsWellKnown": false,
                }),
                title,
            })
        };
        let mut objects = Vec::new();
        for at in 0..self.items {
            objects.push(object(
                "item",
                format!("item {at}"),
                format!("{}:item:{at}", self.name),
            )?);
        }
        objects.push(object(
            "other",
            "the other".to_string(),
            format!("{}:other", self.name),
        )?);
        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            objects,
            datasets: Vec::new(),
            activity: ActivityDraft {
                recipe: "crs",
                title: format!("{} run", self.name),
                seed: self.name.to_string(),
                parameters: Vec::new(),
            },
            says: Vec::new(),
            dropped: Vec::new(),
            shapes: Vec::new(),
        })
    }
}

fn operations() -> Registry {
    let mut registry = Registry::new();
    registry.register(Box::new(Fake {
        name: "fake-declared",
        items: 3,
        declared: true,
    }));
    registry.register(Box::new(Fake {
        name: "fake-many",
        items: 9,
        declared: true,
    }));
    registry.register(Box::new(Fake {
        name: "fake-undeclared",
        items: 3,
        declared: false,
    }));
    registry
}

/// Plan and apply one fake operation into a fresh package.
fn run(name: &str) -> (manifest::Step, Plan, apply::Made, EpcStore) {
    let registry = operations();
    let plan = registry
        .plan(
            name,
            &Pieces::empty(),
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .expect("planned");
    let mut store = empty_store();
    let made = apply(&plan, &mut store, &when());
    assert!(made.refused.is_empty(), "{:?}", made.refused);
    let step = manifest::step(name, registry.get(name).expect("registered"), &plan, &made);
    (step, plan, made, store)
}

fn read_back(store: &EpcStore, uri: &str) -> Value {
    let object = store
        .get_object(uri)
        .unwrap_or_else(|why| panic!("{uri} does not resolve in the package: {why}"));
    energyml_core::json::write_value(object.as_reflect())
}

/// Every `BoundaryFeature` the package holds: its uri and its title.
fn features_in(store: &EpcStore) -> Vec<(String, String)> {
    store
        .resources()
        .into_iter()
        .filter(|one| one.qualified_type == "resqml22.BoundaryFeature")
        .map(|one| {
            let title = read_back(store, &one.uri)["Citation"]["Title"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            (one.uri, title)
        })
        .collect()
}

#[test]
fn the_uris_of_a_role_are_what_the_package_holds_under_it() {
    let (step, _plan, made, store) = run("fake-declared");
    let item = step
        .outputs
        .iter()
        .find(|one| one.role == "item")
        .expect("the item role");
    let other = step
        .outputs
        .iter()
        .find(|one| one.role == "other")
        .expect("the other role");
    // Confronted with the package: the titles say which role an object was.
    let held = features_in(&store);
    let held_items: BTreeSet<&str> = held
        .iter()
        .filter(|(_, title)| title.starts_with("item "))
        .map(|(uri, _)| uri.as_str())
        .collect();
    let held_other: BTreeSet<&str> = held
        .iter()
        .filter(|(_, title)| title == "the other")
        .map(|(uri, _)| uri.as_str())
        .collect();
    assert_eq!(
        item.uris.iter().map(String::as_str).collect::<BTreeSet<_>>(),
        held_items
    );
    assert_eq!(
        other.uris.iter().map(String::as_str).collect::<BTreeSet<_>>(),
        held_other
    );
    assert_eq!((item.count, other.count), (3, 1));
    // Plan order: `item 0`, `item 1`, `item 2`.
    let titles: Vec<String> = item
        .uris
        .iter()
        .map(|uri| {
            read_back(&store, uri)["Citation"]["Title"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(titles, ["item 0", "item 1", "item 2"]);
    // The activity is the one `apply` wrote, and it is in the package.
    let activity = step.activity.as_deref().expect("an activity");
    assert_eq!(Some(activity), made.activity.as_deref());
    assert_eq!(read_back(&store, activity)["$type"], "eml23.Activity");
    assert_eq!(
        (step.name.as_str(), step.operation.as_str(), step.refused),
        ("fake-declared", "fake-declared", 0)
    );
}

#[test]
fn roles_come_in_plan_order_then_the_declared_ones_not_written() {
    let (step, ..) = run("fake-declared");
    let roles: Vec<(&str, bool, usize)> = step
        .outputs
        .iter()
        .map(|one| (one.role.as_str(), one.declared, one.count))
        .collect();
    assert_eq!(
        roles,
        [("item", true, 3), ("other", false, 1), ("ghost", true, 0)]
    );
    let ghost = &step.outputs[2];
    assert!(ghost.uris.is_empty() && ghost.collection.is_none());
    assert_eq!(ghost.types, ["resqml22.HorizonInterpretation"]);
    assert_eq!(ghost.describe, "never written");
    // A declared role takes the declaration's types; an undeclared one, the written ones.
    assert_eq!(step.outputs[0].types, ["resqml22.BoundaryFeature"]);
    assert_eq!(step.outputs[1].types, ["resqml22.BoundaryFeature"]);
    assert_eq!(step.outputs[1].describe, "");
}

#[test]
fn an_operation_that_declares_nothing_has_every_role_undeclared() {
    let (step, ..) = run("fake-undeclared");
    assert_eq!(step.outputs.len(), 2, "no declared role to add");
    assert!(step.outputs.iter().all(|one| !one.declared), "{:?}", step.outputs);
    assert_eq!(step.outputs[0].types, ["resqml22.BoundaryFeature"]);
    assert_eq!(step.outputs[0].count, 3);
}

/// The independent check: the collection and its members are read from the
/// package, not from `apply`'s return value.
#[test]
fn the_collection_of_a_role_holds_exactly_the_uris_of_that_role() {
    let (step, _plan, _made, store) = run("fake-many");
    let item = step.outputs.iter().find(|one| one.role == "item").unwrap();
    assert_eq!(item.count, 9);
    let collection = item
        .collection
        .as_deref()
        .expect("nine objects of a role are grouped");
    let json = read_back(&store, collection);
    assert_eq!(json["$type"], "eml23.DataobjectCollection");
    let collection_uuid = json["uuid"].as_str().expect("a uuid").to_string();

    let set = store
        .resources()
        .into_iter()
        .find(|one| one.qualified_type == "eml23.CollectionsToDataobjectsAssociationSet")
        .expect("the run's association set");
    let set = read_back(&store, &set.uri);
    let associations = match &set["SingleCollectionAssociation"] {
        Value::Array(many) => many.clone(),
        one => vec![one.clone()],
    };
    let mine = associations
        .iter()
        .find(|one| one["Collection"]["Uuid"] == json!(collection_uuid))
        .expect("an association for it");
    let members: BTreeSet<String> = match &mine["Dataobject"] {
        Value::Array(many) => many
            .iter()
            .map(|one| one["Uuid"].as_str().unwrap().to_string())
            .collect(),
        one => BTreeSet::from([one["Uuid"].as_str().unwrap().to_string()]),
    };
    let expected: BTreeSet<String> = item
        .uris
        .iter()
        .map(|uri| read_back(&store, uri)["uuid"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(members, expected);
    assert_eq!(members.len(), 9);
    // The lone `other` object is not grouped.
    assert!(
        step.outputs
            .iter()
            .find(|one| one.role == "other")
            .unwrap()
            .collection
            .is_none()
    );
}

#[test]
fn a_few_objects_have_no_collection() {
    let (step, ..) = run("fake-declared");
    assert!(
        step.outputs.iter().all(|one| one.collection.is_none()),
        "{:?}",
        step.outputs
    );
}

/// A refusal is simulated by dropping one uri from what `apply` reports as
/// written: the manifest lists written objects only, and counts the refusal.
#[test]
fn an_object_apply_did_not_write_is_not_an_output() {
    let registry = operations();
    let plan = registry
        .plan(
            "fake-declared",
            &Pieces::empty(),
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let mut store = empty_store();
    let mut made = apply(&plan, &mut store, &when());
    let lost = plan.objects[1].uri();
    made.written.retain(|uri| *uri != lost);
    made.refused.push("item 1: refused".to_string());
    let step = manifest::step("s", registry.get("fake-declared").unwrap(), &plan, &made);
    let item = &step.outputs[0];
    assert_eq!(item.count, 2);
    assert!(!item.uris.contains(&lost));
    assert_eq!(step.refused, 1);
}

#[test]
fn the_json_round_trips_and_names_its_format() {
    let (step, plan, ..) = run("fake-declared");
    let manifest = Manifest::new("out.epc", manifest::data_files(&plan), None, vec![step]);
    assert_eq!(FORMAT, "energyml-run-outputs/1");
    assert_eq!(manifest.format, "energyml-run-outputs/1");
    let text = manifest.to_json();
    let back: Manifest = serde_json::from_str(&text).expect("it reads back");
    assert_eq!(back, manifest);
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["format"], "energyml-run-outputs/1");
    assert_eq!(value["package"]["epc"], "out.epc");
    assert_eq!(value["package"]["dataFiles"], json!([]));
    assert!(value["recipe"].is_null());
    let output = &value["steps"][0]["outputs"][0];
    for key in [
        "role",
        "declared",
        "describe",
        "types",
        "count",
        "collection",
        "uris",
    ] {
        assert!(output.get(key).is_some(), "`{key}` is missing from {output}");
    }
    assert!(output["collection"].is_null());
    assert!(value["steps"][0].get("refused").is_some());
}

struct NoFile;

impl Reader for NoFile {
    fn text(&mut self, source: &Where) -> Result<(String, Option<String>), String> {
        Err(format!("reading {}: this test reads no file", source.as_str()))
    }
    fn package(&mut self, path: &str) -> Result<&EpcStore, String> {
        Err(format!("opening {path}: this test opens no package"))
    }
}

#[test]
fn a_recipe_of_two_steps_gives_two_steps() {
    let text = r#"
titre = "Mini"
creation = "2026-10-07T00:00:00Z"
auteur = "tests/manifest.rs"
workflow = "workflow:mini"

[[etape]]
nom = "premier"
operation = "fake-declared"

[[etape]]
nom = "attend"
operation = "fake-declared"
attente = "held: wrote nothing"

[[etape]]
nom = "second"
operation = "fake-undeclared"
"#;
    let recipe = Recipe::parse(text).expect("the recipe reads");
    let mut store = empty_store();
    let played = recipe::play(
        &recipe,
        Path::new("."),
        &operations(),
        &mut store,
        &mut NoFile,
        Mode::Write,
    )
    .unwrap_or_else(|stop| panic!("{stop}"));
    let manifest = played.manifest("mini.epc", &recipe.title);
    assert_eq!(manifest.recipe.as_deref(), Some("Mini"));
    assert_eq!(manifest.package.epc, "mini.epc");
    let names: Vec<&str> = manifest.steps.iter().map(|one| one.name.as_str()).collect();
    assert_eq!(
        names,
        ["premier", "second"],
        "a held step wrote nothing and is not listed"
    );
    assert_eq!(manifest.steps[0].operation, "fake-declared");
    assert_eq!(manifest.steps[1].operation, "fake-undeclared");
    assert!(manifest.steps[0].outputs[0].declared && !manifest.steps[1].outputs[0].declared);
    // Every uri of every step resolves in the package the recipe wrote.
    for step in &manifest.steps {
        assert!(step.activity.is_some());
        for output in &step.outputs {
            for uri in &output.uris {
                assert_eq!(read_back(&store, uri)["$type"], "resqml22.BoundaryFeature");
            }
        }
    }
    // The two steps wrote different objects (the operation's name is in the seed).
    let first: BTreeSet<&String> = manifest.steps[0]
        .outputs
        .iter()
        .flat_map(|one| &one.uris)
        .collect();
    let second: BTreeSet<&String> = manifest.steps[1]
        .outputs
        .iter()
        .flat_map(|one| &one.uris)
        .collect();
    assert_eq!(first.len(), 4);
    assert!(first.is_disjoint(&second));
}
