//! A workflow read back: the constraint of each step, and the draft of a
//! recipe made from the activities a played recipe wrote.
//!
//! # Where the forms come from
//!
//! - common TRG 2.3 section 3.3, p. 42: `Activity.Parent` and the parent's
//!   sub-activity parameters; section 3.10, p. 49: the `Constraint` of a
//!   `ParameterTemplate` is free text (*"it will be better to have a formal
//!   description of the constraint"*), which is why the recipe's convention
//!   (`follows fabricant:<op> (<uuid>); serves ...; held: ...`) has a reader
//!   beside its writer.
//! - the draft is **with loss** and says so (`RecipeDraft::left_out`): it finds
//!   a lost recipe or compares two, it never replaces the versioned recipe.
//!
//! Test operations are declared in this file and registered for the run, as in
//! `tests/recipe.rs`. A registry that must name exactly one operation per
//! canonical template is `Registry::bare()` plus the test operation.

use std::path::Path;
use std::sync::Arc;

use energyml_build::entries::Reader;
use energyml_build::operation::{
    ActivityDraft, AnyOperation, Choices, Error, Fitness, Identity, Need, NewObject, Plan, Registry, Slot,
    Takes, When,
};
use energyml_build::pieces::Pieces;
use energyml_build::recipe::{self, Mode, Recipe, ReplayRefusal, read_step_constraint};
use energyml_build::source::Source as Where;
use energyml_domain::activity::{
    ActivityParameter, ActivityRecord, DeclaredReference, ParameterDirection, ParameterKind,
    ParameterTemplateRecord, ParameterValue, TemplateRecord,
};
use energyml_domain::package_index::PackageIndex;
use energyml_storage::EpcStore;
use serde_json::json;

const CRS_UUID_PLACEHOLDER: &str = "6f8d3a10-1a2b-4c3d-8e4f-a0b1c2d3e40f";

/// A step constraint as `write_workflow` writes it, read back.
///
/// To break it: cut on every `;` without looking at the prefixes.
#[test]
fn a_step_constraint_reads_back_what_write_workflow_wrote() {
    let read = read_step_constraint(&format!(
        "follows fabricant:crs ({CRS_UUID_PLACEHOLDER}); serves a, b; held: why; still why"
    ))
    .expect("a constraint of the recipe");
    assert_eq!(read.operation, "crs");
    assert_eq!(read.template_uuid.as_deref(), Some(CRS_UUID_PLACEHOLDER));
    assert_eq!(read.serves, ["a", "b"]);
    assert_eq!(read.held.as_deref(), Some("why; still why"));

    let bare = read_step_constraint("follows fabricant:crs").expect("a bare constraint");
    assert_eq!(bare.operation, "crs");
    assert!(bare.template_uuid.is_none() && bare.serves.is_empty() && bare.held.is_none());

    assert!(read_step_constraint("one of: a, b").is_none(), "a constraint of another writer");
    assert!(
        read_step_constraint("follows fabricant:crs (not-a-uuid)").is_none(),
        "what is in the brackets must be a uuid"
    );
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

/// A reader that has one file.
struct OneFile;

impl Reader for OneFile {
    fn text(&mut self, source: &Where) -> Result<(String, Option<String>), String> {
        if source.as_str().ends_with("a.geojson") {
            Ok((
                r#"{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[1,2]},"properties":{}}]}"#
                    .to_string(),
                None,
            ))
        } else {
            Err(format!("reading {}: this test has one file", source.as_str()))
        }
    }
    fn package(&mut self, path: &str) -> Result<&EpcStore, String> {
        Err(format!("opening {path}: this test opens no package"))
    }
}

fn empty_store() -> EpcStore {
    energyml_build::epc::open(None, "draft.epc", Arc::new(energyml_schemas::all())).expect("an empty package")
}

/// Two `crs` steps of one seed; the first serves two use cases.
const THREE_STEPS: &str = r#"
titre = "Mini"
creation = "2026-11-14T00:00:00Z"
auteur = "tests/recipe_draft.rs"
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
"#;

fn played_with(text: &str, registry: &Registry, reader: &mut dyn Reader) -> (recipe::Played, EpcStore) {
    let recipe = Recipe::parse(text).expect("the recipe reads");
    let mut store = empty_store();
    let played = recipe::play(&recipe, Path::new("."), registry, &mut store, reader, Mode::Write)
        .unwrap_or_else(|stop| panic!("{stop}"));
    (played, store)
}

fn activity_of(store: &EpcStore, uuid: &str) -> ActivityRecord {
    let summary = store
        .find_by_uuid(uuid)
        .unwrap_or_else(|| panic!("{uuid} is in the package"))
        .clone();
    let object = store.get_object(&summary.uri).expect("it reads");
    energyml_domain::activity::read_activity(object.as_reflect()).expect("an activity")
}

fn template_of(store: &EpcStore, uuid: &str) -> TemplateRecord {
    let summary = store
        .find_by_uuid(uuid)
        .unwrap_or_else(|| panic!("{uuid} is in the package"))
        .clone();
    let object = store.get_object(&summary.uri).expect("it reads");
    energyml_domain::activity::read_template(object.as_reflect()).expect("a template")
}

/// What a played recipe left: the root, its template, the activities of the
/// steps that ran, and the index of the package.
struct Workflow {
    root: ActivityRecord,
    template: TemplateRecord,
    children: Vec<ActivityRecord>,
    index: PackageIndex,
}

fn workflow_of(played: &recipe::Played, store: &EpcStore) -> Workflow {
    Workflow {
        root: activity_of(store, &played.workflow.activity.uuid),
        template: template_of(store, &played.workflow.template.uuid),
        children: played
            .steps
            .iter()
            .filter_map(|done| done.activity.as_ref())
            .map(|one| activity_of(store, &one.uuid))
            .collect(),
        index: PackageIndex::build(store, store.revision()),
    }
}

/// **Triangulation**: the constraint the recipe's writer wrote is read back by
/// the reader beside it, from the template a played recipe left in a package.
///
/// To break it: read the constraint with another format than the writer's.
#[test]
fn the_constraint_of_a_played_recipe_reads_back() {
    let registry = Registry::new();
    let (played, store) = played_with(THREE_STEPS, &registry, &mut NoFile);
    let template = template_of(&store, &played.workflow.template.uuid);
    let steps = energyml_domain::provenance_graph::workflow_steps(&template, &recipe::RecipeConstraints);
    assert_eq!(steps.len(), 2, "{steps:?}");
    let crs = energyml_domain::provenance::template_uuid("crs").expect("the canonical `crs`");
    for step in &steps {
        let parsed = step.parsed.as_ref().expect("the writer's constraint reads");
        assert_eq!(parsed.operation, "crs");
        assert_eq!(parsed.template_uuid.as_deref(), Some(crs));
    }
    assert_eq!(steps[0].parsed.as_ref().unwrap().serves, ["O_001", "O_002"]);
    assert!(steps[1].parsed.as_ref().unwrap().serves.is_empty());
}

/// A recipe played, read back from its activities, drafts into a recipe that
/// parses and names the same steps.
///
/// To break it: lose the order of the template's steps.
#[test]
fn a_played_recipe_drafts_back_into_a_recipe_that_parses() {
    let registry = Registry::new();
    let (played, store) = played_with(THREE_STEPS, &registry, &mut NoFile);
    let flow = workflow_of(&played, &store);
    let draft = recipe::draft_from_workflow(&flow.root, &flow.template, &flow.children, &registry, &flow.index)
        .unwrap_or_else(|refused| panic!("{refused:?}"));
    let names: Vec<&str> = draft.recipe.steps.iter().map(|step| step.name.as_str()).collect();
    assert_eq!(names, ["reperes", "reperes-encore"]);
    assert!(draft.recipe.steps.iter().all(|step| step.operation == "crs"));
    assert_eq!(draft.recipe.steps[0].set.get("epsg"), Some(&json!(2154)));
    assert_eq!(draft.recipe.steps[0].serves, ["O_001", "O_002"]);

    let text = draft.to_toml().expect("it writes TOML");
    let again = Recipe::parse(&text).unwrap_or_else(|stop| panic!("{stop}\n{text}"));
    let again_names: Vec<&str> = again.steps.iter().map(|step| step.name.as_str()).collect();
    assert_eq!(again_names, names);
}

/// A step declared held and never played is drafted held, with its reason.
///
/// To break it: do not read `held:` in `read_step_constraint`.
#[test]
fn a_held_step_is_drafted_held_with_its_reason() {
    let text = format!(
        "{THREE_STEPS}\n[[etape]]\nnom = \"plus-tard\"\noperation = \"crs\"\nattente = \"later\"\n"
    );
    let registry = Registry::new();
    let (played, store) = played_with(&text, &registry, &mut NoFile);
    let flow = workflow_of(&played, &store);
    let draft = recipe::draft_from_workflow(&flow.root, &flow.template, &flow.children, &registry, &flow.index)
        .unwrap_or_else(|refused| panic!("{refused:?}"));
    assert_eq!(draft.recipe.steps.len(), 3);
    assert_eq!(draft.recipe.steps[2].name, "plus-tard");
    assert_eq!(draft.recipe.steps[2].held.as_deref(), Some("later"));
    assert!(draft.recipe.steps[2].inputs.is_empty() && draft.recipe.steps[2].set.is_empty());
}

/// An operation declared in this file: it records under the canonical template
/// `crs`, and may declare a required input.
struct TestOp {
    name: &'static str,
    takes_input: bool,
}

impl AnyOperation for TestOp {
    fn name(&self) -> &'static str {
        self.name
    }
    fn describe(&self) -> &'static str {
        "writes one feature - a test operation"
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
        vec![Slot::required("title", Need::Title, "the feature's title")]
    }
    fn takes(&self) -> Vec<Slot> {
        if self.takes_input {
            vec![Slot::required("features", Need::Input(Takes::piece("features")), "")]
        } else {
            Vec::new()
        }
    }
    fn plan(
        &self,
        _pieces: &Pieces,
        choices: &Choices,
        identity: &Identity,
        when: &When,
    ) -> Result<Plan, Error> {
        let title = choices.text("title").unwrap_or("untitled").to_string();
        let uuid = identity.uuid(&format!("{}:{title}", self.name), 0)?;
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
            activity: ActivityDraft {
                recipe: "crs",
                title: format!("{} → {title}", self.name),
                seed: title.clone(),
                parameters: Vec::new(),
            },
            says: Vec::new(),
            dropped: Vec::new(),
            shapes: Vec::new(),
        })
    }
}

fn only(name: &'static str, takes_input: bool) -> Registry {
    let mut registry = Registry::bare();
    registry.register(Box::new(TestOp { name, takes_input }));
    registry
}

fn declared(address: &str, qualified_type: &str, title: &str) -> DeclaredReference {
    let uuid = address
        .rsplit_once('(')
        .map(|(_, inside)| inside.trim_end_matches(')').to_string())
        .unwrap_or_else(|| address.to_string());
    DeclaredReference {
        address: address.to_string(),
        uuid,
        qualified_type: Some(qualified_type.to_string()),
        title: Some(title.to_string()),
        version: None,
    }
}

fn parameter(position: usize, title: &str, value: ParameterValue) -> ActivityParameter {
    ActivityParameter {
        position,
        title: title.to_string(),
        index: None,
        selection: None,
        is_uncertain: Some(false),
        keys: Vec::new(),
        value,
        class: String::new(),
    }
}

/// A workflow of one `crs` step, fabricated: the root names `step`, the child
/// is an activity of the canonical `crs` template with the given parameters.
fn fabricated(parameters: Vec<ActivityParameter>) -> Workflow {
    let crs = energyml_domain::provenance::template_uuid("crs").expect("the canonical `crs`");
    let child_uuid = "11111111-1111-4111-8111-111111111111";
    let child_uri = format!("eml:///eml23.Activity({child_uuid})");
    let child = ActivityRecord {
        uri: child_uri.clone(),
        title: Some("the step".to_string()),
        creation: Some("2026-11-14T00:00:00Z".to_string()),
        last_update: None,
        originator: Some("energyml-rust".to_string()),
        format: None,
        descriptor: Some(declared(
            &format!("eml:///eml23.ActivityTemplate({crs})"),
            "eml23.ActivityTemplate",
            "fabricant:crs",
        )),
        parent: None,
        parameters,
    };
    let root = ActivityRecord {
        uri: "eml:///eml23.Activity(22222222-2222-4222-8222-222222222222)".to_string(),
        title: Some("the workflow".to_string()),
        creation: Some("2026-11-14T00:00:00Z".to_string()),
        last_update: None,
        originator: Some("energyml-rust".to_string()),
        format: None,
        descriptor: None,
        parent: None,
        parameters: vec![parameter(
            0,
            "step",
            ParameterValue::Object(declared(&child_uri, "eml23.Activity", "the step")),
        )],
    };
    let template = TemplateRecord {
        uri: "eml:///eml23.ActivityTemplate(33333333-3333-4333-8333-333333333333)".to_string(),
        title: Some("workflow:fabricated".to_string()),
        parameters: vec![ParameterTemplateRecord {
            position: 0,
            title: "step".to_string(),
            allowed: vec![ParameterKind::SubActivity],
            direction: ParameterDirection::Neither,
            min_occurs: Some(1),
            max_occurs: Some(1),
            constraint: Some(format!("follows fabricant:crs ({crs})")),
            content_type: None,
            key_constraints: Vec::new(),
            defaults: Vec::new(),
        }],
    };
    Workflow {
        root,
        template,
        children: vec![child],
        index: PackageIndex::build(&empty_store(), 0),
    }
}

/// An object no earlier step wrote cannot become a recipe input: a recipe never
/// copies a uuid.
///
/// To break it: copy the uuid into the recipe instead of refusing.
#[test]
fn an_object_no_earlier_step_wrote_is_refused() {
    let flow = fabricated(vec![parameter(
        0,
        "input",
        ParameterValue::Object(declared(
            "eml:///resqml22.BoundaryFeature(44444444-4444-4444-8444-444444444444)",
            "resqml22.BoundaryFeature",
            "from elsewhere",
        )),
    )]);
    let refused = recipe::draft_from_workflow(
        &flow.root,
        &flow.template,
        &flow.children,
        &only("writes", false),
        &flow.index,
    )
    .expect_err("refused");
    assert!(
        refused
            .iter()
            .any(|one| matches!(one, ReplayRefusal::ObjectFromOutside { step, .. } if step == "step")),
        "{refused:?}"
    );
}

/// An import whose activity records neither an object nor its file is refused:
/// the case of a package written before the file was recorded.
///
/// To break it: draft a step with no input at all.
#[test]
fn an_import_without_its_file_is_refused() {
    let flow = fabricated(vec![parameter(
        0,
        "output",
        ParameterValue::Object(declared(
            "eml:///resqml22.BoundaryFeature(55555555-5555-4555-8555-555555555555)",
            "resqml22.BoundaryFeature",
            "made",
        )),
    )]);
    let refused = recipe::draft_from_workflow(
        &flow.root,
        &flow.template,
        &flow.children,
        &only("imports", true),
        &flow.index,
    )
    .expect_err("refused");
    assert_eq!(refused, vec![ReplayRefusal::InputFileNotRecorded { step: "step".to_string() }]);
}

/// A file recorded in an activity comes back as an entry of the step.
///
/// To break it: ignore the `inputFile` parameter in the draft.
#[test]
fn a_file_recorded_comes_back_as_an_entry() {
    let text = r#"
titre = "Mini"
creation = "2026-11-14T00:00:00Z"
auteur = "tests/recipe_draft.rs"
workflow = "workflow:mini"

[[etape]]
nom = "importe"
operation = "imports"
set = { title = "a feature" }
entrees = [ { creneau = "features", fichier = "donnees/a.geojson" } ]
"#;
    let registry = only("imports", false);
    let (played, store) = played_with(text, &registry, &mut OneFile);
    let flow = workflow_of(&played, &store);
    let draft = recipe::draft_from_workflow(&flow.root, &flow.template, &flow.children, &registry, &flow.index)
        .unwrap_or_else(|refused| panic!("{refused:?}"));
    let step = &draft.recipe.steps[0];
    assert_eq!(step.inputs.len(), 1, "{:?}", step.inputs);
    assert_eq!(step.inputs[0].slot.as_deref(), Some("features"));
    assert_eq!(step.inputs[0].file.as_deref(), Some("donnees/a.geojson"));
}

/// Two operations recording under one template are not guessed between.
///
/// To break it: take the first operation found.
#[test]
fn a_template_two_operations_share_is_ambiguous() {
    let flow = fabricated(vec![parameter(
        0,
        "output",
        ParameterValue::Object(declared(
            "eml:///resqml22.BoundaryFeature(66666666-6666-4666-8666-666666666666)",
            "resqml22.BoundaryFeature",
            "made",
        )),
    )]);
    let mut registry = Registry::new();
    registry.register(Box::new(TestOp {
        name: "names-a-frame",
        takes_input: false,
    }));
    let refused = recipe::draft_from_workflow(&flow.root, &flow.template, &flow.children, &registry, &flow.index)
        .expect_err("refused");
    assert!(
        refused.iter().any(|one| matches!(
            one,
            ReplayRefusal::OperationAmbiguous { operations, .. }
                if operations == &["crs".to_string(), "names-a-frame".to_string()]
        )),
        "{refused:?}"
    );
}
