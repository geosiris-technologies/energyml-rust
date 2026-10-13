//! A recipe played in memory — `energyml_build::recipe`,
//! the reference model's recipe.
//!
//! No HDF5: the recipes here are made of steps that write no array — `crs`,
//! and an operation declared **in this file** that names what an earlier step
//! wrote. A test operation in the shipped registry would be an entry the front end
//! offers somebody (`tests/cardinality.rs` says why); here it is registered
//! for the run and nowhere else.
//!
//! # The form of each assertion comes from the schema, not from what we write
//!
//! `Activity.Parent` 0..1, *“An optional parent activity, when the activity is
//! a sub-activity”* (common TRG 2.3 §3.3 p. 42; `Activities.xsd` l. 76); a
//! parameter of kind `subActivity` in the template (l. 24–33); the parent
//! naming each child by a `DataObjectParameter` whose `Title` *“must have an
//! equivalent in the activity descriptor parameters”* (l. 38–40).
//!
//! # What would make them fail
//!
//! A clock or a random draw anywhere in the identities (two runs differ); a
//! step's name out of its activity's seed (two `crs` steps of one seed write
//! one activity); `Parent` written under a key the reader does not know (it
//! is dropped without a word — which is why the activities are read **back
//! from the package**); a refusal that skips its step instead of stopping; a
//! malformed recipe that plans its first steps before being refused.

use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};

use energyml_build::entries::Reader;
use energyml_build::operation::{AnyOperation, Choices, Error, Fitness, Identity, Need, NewObject, Note, Plan, Registry, Slot, When};
use energyml_build::pieces::{Pieces, Source};
use energyml_build::recipe::{self, Mode, Recipe, StoppedAt};
use energyml_build::source::Source as Where;
use energyml_storage::EpcStore;

/// An operation that **names** what an earlier step wrote, and nothing more:
/// a setting `frame` (an object of the package) and whatever input it is
/// given. It writes one `BoundaryFeature`, so that the plan is not empty, and
/// says in a note which object pieces reached it.
///
/// It records under `crs`'s canonical template — the only way to write an
/// activity without a line in `rc/activityTemplates.json`, which would ship.
struct NamesAFrame;

impl AnyOperation for NamesAFrame {
    fn name(&self) -> &'static str {
        "names-a-frame"
    }
    fn describe(&self) -> &'static str {
        "names an object an earlier step wrote — a test operation"
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
        vec![
            Slot::optional("frame", Need::Object { types: &["eml23.VerticalCrs"] }, "the frame to name"),
            Slot::required("title", Need::Title, "the feature's title"),
        ]
    }
    fn plan(&self, pieces: &Pieces, choices: &Choices, identity: &Identity, when: &When) -> Result<Plan, Error> {
        let title = choices.text("title").unwrap_or("untitled").to_string();
        let uuid = identity.uuid(&format!("names-a-frame:{title}"), 0)?;
        // The object pieces it was given — the inputs, not the companions.
        let inputs: Vec<String> = pieces
            .iter()
            .filter(|(name, _)| !name.ends_with(" (object)"))
            .filter_map(|(name, source)| match source {
                Source::Object(known) => known.reference.as_ref().map(|one| format!("{name}={}", one.uuid)),
                _ => None,
            })
            .collect();
        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            objects: vec![NewObject {
                uuid: uuid.clone(),
                qualified_type: "resqml22.BoundaryFeature".to_string(),
                role: "feature".to_string(),
                title: title.clone(),
                json: json!({
                    "$type": "resqml22.BoundaryFeature",
                    "uuid": uuid,
                    "schemaVersion": "2.2",
                    "Citation": { "$type": "eml23.Citation", "Title": title, "Originator": when.originator, "Creation": when.creation },
                    "IsWellKnown": false,
                }),
            }],
            datasets: Vec::new(),
            activity: energyml_build::operation::ActivityDraft {
                recipe: "crs",
                title: format!("names-a-frame → {title}"),
                seed: title.clone(),
                parameters: Vec::new(),
            },
            says: vec![Note { key: "inputs", detail: inputs.join(",") }],
            dropped: Vec::new(),
            shapes: Vec::new(),
        })
    }
}

/// The built-ins, and the test operation ahead of them.
fn operations() -> Registry {
    let mut registry = Registry::new();
    registry.register(Box::new(NamesAFrame));
    registry
}

/// A reader with nothing to read: these recipes name no file.
struct NoFile;

impl Reader for NoFile {
    fn text(&mut self, source: &Where) -> Result<(String, Option<String>), String> {
        Err(format!("reading {}: this test reads no file", source.as_str()))
    }
    fn package(&mut self, path: &str) -> Result<&EpcStore, String> {
        Err(format!("opening {path}: this test opens no package"))
    }
}

fn empty_store() -> EpcStore {
    energyml_build::epc::open(None, "recipe.epc", Arc::new(energyml_schemas::all())).expect("an empty package")
}

/// Three steps: two frames of one seed, then a step that names the vertical
/// system the first one wrote — by the step's name, never by a uuid — and
/// takes all its objects as inputs.
const THREE_STEPS: &str = r#"
titre = "Mini"
creation = "2026-09-24T00:00:00Z"
auteur = "tests/recipe.rs"
workflow = "workflow:mini"

[[etape]]
nom = "reperes"
operation = "crs"
set = { epsg = 2154, vertical = 5720 }
sert = ["O_001", "O_002"]

[[etape]]
nom = "reperes-encore"
operation = "crs"
set = { epsg = 2154, vertical = 5720 }

[[etape]]
nom = "nomme"
operation = "names-a-frame"
set = { frame = { etape = "reperes", type = "VerticalCrs" }, title = "a limit" }
entrees = [ { creneau = "seen", etape = "reperes" } ]
sert = ["CU_00"]
"#;

fn played(text: &str) -> (recipe::Played, EpcStore) {
    let recipe = Recipe::parse(text).expect("the recipe reads");
    let mut store = empty_store();
    let played = recipe::play(&recipe, Path::new("."), &operations(), &mut store, &mut NoFile, Mode::Write)
        .unwrap_or_else(|stop| panic!("{stop}"));
    (played, store)
}

/// An object of the package, as the workspace's JSON writes it — read **back**.
fn read_back(store: &EpcStore, uuid: &str) -> Value {
    let summary = store.find_by_uuid(uuid).unwrap_or_else(|| panic!("{uuid} is in the package")).clone();
    let object = store.get_object(&summary.uri).expect("it reads");
    energyml_core::json::write_value(object.as_reflect())
}

fn activities(store: &EpcStore) -> Vec<Value> {
    store
        .resources()
        .into_iter()
        .filter(|one| one.qualified_type == "eml23.Activity")
        .map(|one| read_back(store, &one.uuid))
        .collect()
}

#[test]
fn the_objects_each_step_writes_are_in_the_package() {
    let (played, store) = played(THREE_STEPS);
    assert_eq!(played.steps.len(), 3);
    for done in &played.steps {
        assert!(done.held.is_none(), "{}", done.step);
        assert!(!done.objects.is_empty(), "{} wrote nothing", done.step);
        for object in &done.objects {
            assert!(store.find_by_uuid(&object.uuid).is_some(), "{}: {} is missing", done.step, object.title);
        }
        let made = done.made.as_ref().expect("applied");
        assert!(made.refused.is_empty(), "{}: {:?}", done.step, made.refused);
    }
    let first = &played.steps[0];
    assert!(first.objects.iter().any(|one| one.qualified_type == "eml23.VerticalCrs"), "{:?}", first.objects);
    assert_eq!(first.serves, ["O_001", "O_002"]);
    // The second `crs` step writes the same systems — replaced, and said.
    let again = played.steps[1].made.as_ref().expect("applied");
    assert!(!again.replaced.is_empty(), "the same systems a second time are replacements, and counted");
}

/// **By the step's name.** The setting `frame` arrives as the reference to the
/// one `VerticalCrs` step `reperes` wrote, and the activity records it; the
/// input takes every object of that step, numbered after its slot.
#[test]
fn a_step_names_what_an_earlier_step_wrote_by_its_name() {
    let (played, store) = played(THREE_STEPS);
    let vertical = played.steps[0]
        .objects
        .iter()
        .find(|one| one.qualified_type == "eml23.VerticalCrs")
        .expect("step `reperes` wrote a vertical system");
    let naming = &played.steps[2];
    let activity = read_back(&store, &naming.activity.as_ref().expect("an activity").uuid);
    let frame = activity["Parameter"]
        .as_array()
        .expect("parameters")
        .iter()
        .find(|one| one["Title"] == "frame")
        .unwrap_or_else(|| panic!("the setting is recorded: {activity}"));
    assert_eq!(frame["DataObject"]["Uuid"], json!(vertical.uuid));
    assert_eq!(frame["DataObject"]["QualifiedType"], "eml23.VerticalCrs");

    // Every object of `reperes`, each a piece of its own, `seen` then `seen 2`
    // — **then what `apply` wrote beside them, its activity last**: the
    // template it added, and the activity. A step wrote them too, and a project
    // collection holds them.
    let note = naming.says.iter().find(|one| one.key == "inputs").expect("the test operation says what it got");
    let got: Vec<&str> = note.detail.split(',').collect();
    let frames_step = &played.steps[0];
    let made = frames_step.made.as_ref().expect("the step was applied");
    let beside: Vec<String> = made
        .written
        .iter()
        .filter(|uri| Some(*uri) != made.activity.as_ref())
        .map(|uri| uri.rsplit_once('(').map(|(_, inside)| inside.trim_end_matches(')')).unwrap_or(uri).to_string())
        .filter(|uuid| !frames_step.objects.iter().any(|one| &one.uuid == uuid))
        .collect();
    assert!(!beside.is_empty(), "the `crs` template was added by this step: {:?}", made.written);
    let wanted: Vec<String> = frames_step
        .objects
        .iter()
        .map(|one| one.uuid.clone())
        .chain(beside.iter().cloned())
        .chain(frames_step.activity.as_ref().map(|activity| activity.uuid.clone()))
        .enumerate()
        .map(|(at, uuid)| if at == 0 { format!("seen={uuid}") } else { format!("seen {}={uuid}", at + 1) })
        .collect();
    assert_eq!(got, wanted);
    assert!(frames_step.activity.is_some() && got.len() == frames_step.objects.len() + beside.len() + 1, "{got:?}");
}

/// **Two runs, one package.** Every uuid — the objects', the activities', the
/// workflow's — comes out the same: nothing is drawn, the instant is stated.
#[test]
fn two_runs_of_one_recipe_write_the_same_uuids() {
    let uuids = |store: &EpcStore| {
        let mut all: Vec<String> = store.resources().into_iter().map(|one| one.uuid).collect();
        all.sort();
        all
    };
    let (first, one) = played(THREE_STEPS);
    let (second, two) = played(THREE_STEPS);
    assert!(uuids(&one).len() > 5, "{:?}", uuids(&one));
    assert_eq!(uuids(&one), uuids(&two));
    assert_eq!(first.workflow, second.workflow);
    assert!(uuids(&one).contains(&first.workflow.activity.uuid));
    assert!(uuids(&one).contains(&first.workflow.template.uuid));
}

/// **Each operation's activity carries `Parent` → the workflow's**, read back
/// from the package — and the two `crs` steps, one seed, are two activities.
#[test]
fn every_operation_activity_has_the_workflow_as_its_parent() {
    let (played, store) = played(THREE_STEPS);
    let root = &played.workflow.activity.uuid;
    let children: Vec<Value> = activities(&store).into_iter().filter(|one| one["uuid"] != json!(root)).collect();
    // One per step played: were the step's name out of the seed, the two
    // `crs` steps would be one activity written twice.
    assert_eq!(children.len(), 3, "{children:?}");
    for child in &children {
        assert_eq!(child["Parent"]["Uuid"], json!(root), "{child}");
        assert_eq!(child["Parent"]["QualifiedType"], "eml23.Activity");
    }
    let named: Vec<&str> = played.steps.iter().filter_map(|done| done.activity.as_ref()).map(|one| one.uuid.as_str()).collect();
    for uuid in &named {
        assert!(children.iter().any(|child| child["uuid"] == json!(uuid)), "{uuid}");
    }
}

/// The root names its children, and its template says what each one is: one
/// `subActivity` parameter per step, the operation's canonical template in
/// `Constraint` — the only place `Activities.xsd` leaves for it.
#[test]
fn the_workflow_template_has_a_sub_activity_per_step_and_the_root_names_each_child() {
    let (played, store) = played(THREE_STEPS);
    let template = read_back(&store, &played.workflow.template.uuid);
    let parameters = template["Parameter"].as_array().expect("parameters");
    let titles: Vec<&str> = parameters.iter().filter_map(|one| one["Title"].as_str()).collect();
    assert_eq!(titles, ["reperes", "reperes-encore", "nomme"]);
    for parameter in parameters {
        assert_eq!(parameter["AllowedKind"], json!(["subActivity"]), "{parameter}");
        assert_eq!(parameter["MinOccurs"], json!(1));
        assert_eq!(parameter["MaxOccurs"], json!(1));
    }
    let crs_template = energyml_domain::provenance::template_uuid("crs").expect("canonical");
    assert!(parameters[0]["Constraint"].as_str().is_some_and(|text| text.contains(crs_template)), "{}", parameters[0]);
    assert!(parameters[0]["Constraint"].as_str().is_some_and(|text| text.contains("O_001")), "{}", parameters[0]);

    let root = read_back(&store, &played.workflow.activity.uuid);
    assert_eq!(root["ActivityDescriptor"]["Uuid"], json!(played.workflow.template.uuid));
    assert!(root.get("Parent").is_none_or(Value::is_null), "the root has no parent: {root}");
    let children = root["Parameter"].as_array().expect("the root names its children");
    assert_eq!(children.len(), 3);
    for (child, done) in children.iter().zip(&played.steps) {
        assert_eq!(child["Title"], json!(done.step));
        assert_eq!(child["DataObject"]["Uuid"], json!(done.activity.as_ref().expect("played").uuid));
    }
}

/// **A refusal stops the run, naming the step** — and nothing after it runs.
#[test]
fn a_step_that_refuses_stops_the_run_and_names_the_step() {
    let text = r#"
titre = "Mini"
creation = "2026-09-24T00:00:00Z"
auteur = "tests/recipe.rs"
workflow = "workflow:mini"

[[etape]]
nom = "reperes"
operation = "crs"
set = { epsg = 2154 }

[[etape]]
nom = "sans-code"
operation = "crs"

[[etape]]
nom = "apres"
operation = "names-a-frame"
set = { title = "never" }
"#;
    let recipe = Recipe::parse(text).expect("reads");
    let mut store = empty_store();
    let stop = recipe::play(&recipe, Path::new("."), &operations(), &mut store, &mut NoFile, Mode::Write)
        .expect_err("`epsg` is required");
    assert_eq!(stop.at, StoppedAt::Step { name: "sans-code".to_string(), operation: "crs".to_string() });
    assert!(stop.refusals.iter().any(|why| why.contains("epsg")), "{:?}", stop.refusals);
    assert!(stop.to_string().contains("`sans-code`"), "{stop}");
    // What ran before it is reported; what comes after never ran.
    assert_eq!(stop.done.len(), 1);
    assert_eq!(stop.done[0].step, "reperes");
    assert!(store.resources().iter().all(|one| one.qualified_type != "resqml22.BoundaryFeature"));
}

/// A malformed recipe, refused **by name, before any plan**: an unknown
/// operation, a name used twice, a setting the operation does not declare, an
/// input from a later step, a setting naming no step, a step taking from a held
/// one. All at once, and the package untouched.
#[test]
fn a_malformed_recipe_is_refused_by_name_before_any_plan() {
    let text = r#"
titre = "Mini"
creation = "2026-09-24T00:00:00Z"
auteur = "tests/recipe.rs"
workflow = "workflow:mini"

[[etape]]
nom = "reperes"
operation = "crs"
set = { epsg = 2154 }

[[etape]]
nom = "reperes"
operation = "crs"
set = { epsg = 4326 }

[[etape]]
nom = "inconnue"
operation = "no-such-operation"

[[etape]]
nom = "faute-de-frappe"
operation = "crs"
set = { epsg = 2154, verticale = 5720 }

[[etape]]
nom = "trop-tot"
operation = "names-a-frame"
set = { title = "x", frame = { etape = "nulle-part" } }
entrees = [ { creneau = "seen", etape = "plus-tard" } ]

[[etape]]
nom = "en-attente"
operation = "crs"
set = { epsg = 2154 }
attente = "a table nobody has read yet"

[[etape]]
nom = "depend-de-l-attente"
operation = "names-a-frame"
set = { title = "y" }
entrees = [ { etape = "en-attente" } ]

[[etape]]
nom = "plus-tard"
operation = "crs"
set = { epsg = 2154 }
"#;
    let recipe = Recipe::parse(text).expect("it reads — it does not hold together");
    let mut store = empty_store();
    let before = store.resources().len();
    let stop = recipe::play(&recipe, Path::new("."), &operations(), &mut store, &mut NoFile, Mode::Write)
        .expect_err("refused");
    assert_eq!(stop.at, StoppedAt::Recipe);
    assert!(stop.done.is_empty());
    assert_eq!(store.resources().len(), before, "nothing was planned, so nothing was written");
    let said = stop.refusals.join("\n");
    for expected in [
        "step 2 `reperes`: the name is already step 1's",
        "step 3 `inconnue`: no operation is called `no-such-operation`",
        "step 4 `faute-de-frappe`: `crs` has no setting `verticale`",
        "step 5 `trop-tot`: it takes what `plus-tard` writes, and `plus-tard` comes after it",
        "step 5 `trop-tot`: it takes what `nulle-part` writes, and no step is called `nulle-part`",
        "step 7 `depend-de-l-attente`: it takes what `en-attente` writes, and `en-attente` is held",
    ] {
        assert!(said.contains(expected), "missing “{expected}” in:\n{said}");
    }
}

/// A key the recipe does not know is refused by its name — `entree` for
/// `entrees` would otherwise be a step with no input, which reads like one
/// that needs none.
#[test]
fn an_unknown_key_is_refused_by_its_name() {
    let text = r#"
titre = "Mini"
creation = "2026-09-24T00:00:00Z"
auteur = "tests/recipe.rs"
workflow = "workflow:mini"

[[etape]]
nom = "reperes"
operation = "crs"
entree = [ { fichier = "x.geojson" } ]
"#;
    let stop = Recipe::parse(text).expect_err("`entree` is no key of a step");
    assert!(stop.to_string().contains("entree"), "{stop}");
}

/// A held step is checked, reported with its reason, declared in the template
/// as optional — and not played.
#[test]
fn a_held_step_is_reported_and_declared_optional_but_not_played() {
    let text = r#"
titre = "Mini"
creation = "2026-09-24T00:00:00Z"
auteur = "tests/recipe.rs"
workflow = "workflow:mini"

[[etape]]
nom = "reperes"
operation = "crs"
set = { epsg = 2154 }

[[etape]]
nom = "plus-tard"
operation = "names-a-frame"
set = { title = "attend" }
attente = "the table is not reviewed"
"#;
    let (played, store) = played(text);
    let held = &played.steps[1];
    assert_eq!(held.held.as_deref(), Some("the table is not reviewed"));
    assert!(held.activity.is_none() && held.objects.is_empty());
    assert!(store.resources().iter().all(|one| one.qualified_type != "resqml22.BoundaryFeature"));
    let template = read_back(&store, &played.workflow.template.uuid);
    let parameter = &template["Parameter"][1];
    assert_eq!(parameter["Title"], "plus-tard");
    assert_eq!(parameter["MinOccurs"], json!(0));
    assert!(parameter["Constraint"].as_str().is_some_and(|text| text.contains("held")), "{parameter}");
    // The root names only what ran.
    let root = read_back(&store, &played.workflow.activity.uuid);
    assert_eq!(root["Parameter"].as_array().map(Vec::len), Some(1));
}

/// A setting naming several objects is refused, naming them: a setting takes
/// one, and the first found is not a choice anybody made.
#[test]
fn a_setting_naming_several_objects_is_refused_naming_them() {
    let text = r#"
titre = "Mini"
creation = "2026-09-24T00:00:00Z"
auteur = "tests/recipe.rs"
workflow = "workflow:mini"

[[etape]]
nom = "reperes"
operation = "crs"
set = { epsg = 2154, vertical = 5720 }

[[etape]]
nom = "nomme"
operation = "names-a-frame"
set = { frame = { etape = "reperes" }, title = "x" }
"#;
    let recipe = Recipe::parse(text).expect("reads");
    let mut store = empty_store();
    let stop = recipe::play(&recipe, Path::new("."), &operations(), &mut store, &mut NoFile, Mode::Write)
        .expect_err("two systems for one setting");
    assert_eq!(stop.at, StoppedAt::Step { name: "nomme".to_string(), operation: "names-a-frame".to_string() });
    assert!(stop.refusals.iter().any(|why| why.contains("a setting takes one")), "{:?}", stop.refusals);
}

/// **A variant is the recipe without a group**, never a second recipe.
/// The need: a second one to open lutetia but without the objects in
/// error. The left-out steps are held — said, reported, not
/// played — and a step kept that takes what a left-out one writes is refused
/// by name, as for any held step.
#[test]
fn a_variant_leaves_a_group_out_and_says_so() {
    let text = r#"
titre = "Mini"
creation = "2026-09-24T00:00:00Z"
auteur = "tests/recipe.rs"
workflow = "workflow:mini"

[[etape]]
nom = "reperes"
operation = "crs"
set = { epsg = 2154 }

[[etape]]
nom = "erreur"
operation = "names-a-frame"
set = { title = "a planted anomaly" }
groupe = "erreur"
"#;
    let recipe = Recipe::parse(text).expect("the recipe reads");
    let clean = recipe.without(&["erreur"]).expect("a group of the recipe");
    assert!(clean.steps[1].held.as_deref().is_some_and(|why| why.contains("erreur")), "{:?}", clean.steps[1].held);
    assert!(clean.steps[0].held.is_none(), "the others play");
    let mut store = empty_store();
    let played = recipe::play(&clean, Path::new("."), &operations(), &mut store, &mut NoFile, Mode::Write)
        .unwrap_or_else(|stop| panic!("{stop}"));
    assert!(played.steps[1].objects.is_empty(), "the left-out step writes nothing");
    // The whole recipe is untouched: `without` is a copy.
    assert!(recipe.steps[1].held.is_none());

    // A group no step belongs to is refused, naming the ones there are.
    let why = recipe.without(&["erreurs"]).expect_err("a typo");
    assert!(why.contains("erreurs") && why.contains("`erreur`"), "{why}");

    // A step kept that takes what a left-out one writes: refused by name.
    let leaning = format!("{text}
[[etape]]
nom = \"apres\"
operation = \"names-a-frame\"
set = {{ title = \"x\" }}
entrees = [ {{ creneau = \"seen\", etape = \"erreur\" }} ]
");
    let refused = Recipe::parse(&leaning).expect("reads").without(&["erreur"]).expect("a group").check(&operations());
    assert!(refused.is_err_and(|stop| stop.to_string().contains("apres")), "the step taking from a left-out one is named");
}

