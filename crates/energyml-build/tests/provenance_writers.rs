//! What the library writes about where an object came from: integers as
//! integers, the files a step read, and templates that declare both.
//!
//! # Where the forms come from
//!
//! - common TRG 2.3 section 3.1, p. 40: *"Name of the parameter, used to
//!   identify it in the activity. Must have an equivalent in the activity
//!   descriptor parameters."* - so a parameter the writers produce under `input`
//!   or `inputFile` has its declaration in every template.
//! - common TRG 2.3 section 3.8, p. 47: `IntegerQuantityParameter` holds a
//!   `Value: long` and no unit.
//! - common TRG 2.3 section 3.10, p. 49: `AllowedKind` is the kind a parameter
//!   is written as.
//!
//! The operations used here are declared **in this file**, and registered for
//! the run: a test operation in the shipped registry would be an entry the
//! front end offers somebody. They record under canonical templates that exist
//! already (`triangulate`, `crs`), the only way to write an activity without a
//! line in `data/activityTemplates.json`, which would ship.

use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};

use energyml_build::entries::Reader;
use energyml_build::operation::{
    ActivityDraft, AnyOperation, Choices, Error, Fitness, Identity, Need, NewObject, Plan, Registry, Slot,
    When,
};
use energyml_build::pieces::Pieces;
use energyml_build::recipe::{self, InputFile, Mode, Recipe};
use energyml_build::source::Source as Where;
use energyml_build::template::{INPUT_FILE_CONSTRAINT, template_of};
use energyml_domain::provenance::Parameter;
use energyml_storage::EpcStore;

fn when() -> When {
    When {
        creation: "2026-11-14T00:00:00Z".into(),
        originator: "tests/provenance_writers.rs".into(),
    }
}

/// An operation that writes one feature and the parameters it is given, under
/// the canonical template named by `recipe`. It declares one integer setting,
/// `count`.
struct Writes {
    name: &'static str,
    recipe: &'static str,
    own: Vec<Parameter>,
}

impl AnyOperation for Writes {
    fn name(&self) -> &'static str {
        self.name
    }
    fn describe(&self) -> &'static str {
        "writes one feature and the parameters it was given - a test operation"
    }
    fn makes(&self) -> &'static [&'static str] {
        &["resqml22.BoundaryFeature"]
    }
    fn recipe(&self) -> &'static str {
        self.recipe
    }
    fn fitness(&self, _pieces: &Pieces) -> Fitness {
        Fitness::Fit
    }
    fn needs(&self, _pieces: &Pieces) -> Vec<Slot> {
        vec![
            Slot::optional(
                "count",
                Need::Integer {
                    least: None,
                    most: None,
                },
                "a whole number",
            ),
            Slot::optional("title", Need::Title, "the feature's title"),
        ]
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
                recipe: self.recipe,
                title: format!("{} → {title}", self.name),
                seed: title.clone(),
                parameters: self.own.clone(),
            },
            says: Vec::new(),
            dropped: Vec::new(),
            shapes: Vec::new(),
        })
    }
}

fn registry_with(operation: Writes) -> Registry {
    let mut registry = Registry::new();
    registry.register(Box::new(operation));
    registry
}

fn plan_of(registry: &Registry, name: &str, choices: &Choices) -> Plan {
    registry
        .plan(name, &Pieces::empty(), choices, &Identity::derived(), &when())
        .unwrap_or_else(|why| panic!("planned: {why:?}"))
}

fn recorded<'a>(plan: &'a Plan, title: &str) -> Vec<&'a Parameter> {
    plan.activity.parameters.iter().filter(|p| p.title() == title).collect()
}

/// An integer setting is written as an integer, not as a double.
///
/// Spec: common TRG 2.3 section 3.8, p. 47. To break it: put `Need::Integer`
/// back in the `Double` branch of `record_choices`.
#[test]
fn an_integer_setting_is_recorded_as_an_integer() {
    let registry = registry_with(Writes {
        name: "writes-a-count",
        recipe: "triangulate",
        own: Vec::new(),
    });
    let plan = plan_of(&registry, "writes-a-count", &Choices::new().set("count", 4));
    match recorded(&plan, "count").as_slice() {
        [Parameter::Integer { value, .. }] => assert_eq!(*value, 4),
        other => panic!("`count` is recorded as {other:?}"),
    }
}

/// A whole number the operation wrote as a double, under a title the canonical
/// template declares `integer`, is retyped; one that is not whole stays a
/// double (the conformance will report it).
///
/// Spec: common TRG 2.3 section 3.10, p. 49 (`AllowedKind`). To break it:
/// remove the call to `conform_integer_kinds` in `Registry::plan_in`.
///
/// `TODO(verify)`: that `triangulate` declares `faultCount` and `nodesAdded` as
/// `integer` (true in `data/activityTemplates.json` on 14/11).
#[test]
fn a_whole_number_the_operation_wrote_as_a_double_is_retyped() {
    let registry = registry_with(Writes {
        name: "writes-doubles",
        recipe: "triangulate",
        own: vec![
            Parameter::Double {
                title: "faultCount".into(),
                value: 2.0,
                uom: None,
            },
            Parameter::Double {
                title: "nodesAdded".into(),
                value: 2.5,
                uom: None,
            },
        ],
    });
    let plan = plan_of(&registry, "writes-doubles", &Choices::new());
    match recorded(&plan, "faultCount").as_slice() {
        [Parameter::Integer { value, .. }] => assert_eq!(*value, 2),
        other => panic!("`faultCount` is recorded as {other:?}"),
    }
    match recorded(&plan, "nodesAdded").as_slice() {
        [Parameter::Double { value, .. }] => assert_eq!(*value, 2.5),
        other => panic!("`nodesAdded` is recorded as {other:?}"),
    }
}

/// A double under a title the template declares `double` is left alone.
///
/// To break it: retype every whole double without looking at the template.
#[test]
fn a_double_under_a_double_title_is_left_alone() {
    let registry = registry_with(Writes {
        name: "writes-an-epsg",
        recipe: "crs",
        own: vec![Parameter::Double {
            title: "epsg".into(),
            value: 2154.0,
            uom: None,
        }],
    });
    let plan = plan_of(&registry, "writes-an-epsg", &Choices::new());
    match recorded(&plan, "epsg").as_slice() {
        [Parameter::Double { value, .. }] => assert_eq!(*value, 2154.0),
        other => panic!("`epsg` is recorded as {other:?}"),
    }
}

/// The projection of an operation declares `input` and `inputFile` before
/// `output`, and says the same words as the canonical file.
///
/// Spec: common TRG 2.3 section 3.1, p. 40. To break it: change the text of
/// `INPUT_FILE_CONSTRAINT` on one side only.
#[test]
fn the_projection_declares_input_and_input_file_before_output() {
    let registry = Registry::new();
    let operation = registry.get("crs").expect("the shipped `crs` operation");
    let projected = template_of(operation);
    let parameters = projected["Parameter"].as_array().expect("parameters");
    let titles: Vec<&str> = parameters.iter().filter_map(|p| p["Title"].as_str()).collect();
    let n = titles.len();
    assert_eq!(&titles[n - 3..], ["input", "inputFile", "output"], "{titles:?}");
    assert_eq!(
        parameters[n - 2]["Constraint"].as_str(),
        Some(INPUT_FILE_CONSTRAINT),
        "the projection's constraint"
    );
    let canonical = energyml_domain::provenance::template("crs").expect("the canonical `crs`");
    let held = canonical["Parameter"]
        .as_array()
        .expect("parameters")
        .iter()
        .find(|p| p["Title"] == "inputFile")
        .expect("the canonical file declares `inputFile`");
    assert_eq!(
        held["Constraint"].as_str(),
        Some(INPUT_FILE_CONSTRAINT),
        "the file's constraint and the projection's are one sentence"
    );
}

/// What `input_file_parameter` writes, `read_input_file` reads back.
///
/// To break it: cut on the last `": "` instead of the first.
#[test]
fn an_input_file_reads_back_as_it_was_written() {
    for (slot, path) in [
        (Some("features"), "donnees/emprise.geojson"),
        (None, "a b/c.csv"),
        (Some("svg"), "x: y.svg"),
    ] {
        let Parameter::Text { title, value } = recipe::input_file_parameter(slot, path) else {
            panic!("`inputFile` is a text parameter");
        };
        assert_eq!(title, "inputFile");
        assert_eq!(
            recipe::read_input_file(&value),
            Some(InputFile {
                slot: slot.map(str::to_string),
                path: path.to_string()
            }),
            "{value}"
        );
    }
    assert_eq!(recipe::read_input_file(""), None);
    // A Windows path has no ": " (no space after the colon): it is a path.
    assert_eq!(
        recipe::read_input_file("C:\\data\\a.csv"),
        Some(InputFile {
            slot: None,
            path: "C:\\data\\a.csv".to_string()
        })
    );
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

const ONE_STEP: &str = r#"
titre = "Mini"
creation = "2026-11-14T00:00:00Z"
auteur = "tests/provenance_writers.rs"
workflow = "workflow:mini"

[[etape]]
nom = "importe"
operation = "writes-a-count"
set = { title = "a feature" }
entrees = [ { creneau = "features", fichier = "donnees/a.geojson" } ]
"#;

/// A file a step read is in its activity, as a `StringParameter` titled
/// `inputFile`.
///
/// To break it: do not push the parameter in `play_step`.
///
/// `TODO(verify)`: that `entries::read_text` accepts this one-point GeoJSON
/// without a column template; otherwise take the simplest file form it reads.
#[test]
fn a_file_a_step_read_is_in_its_activity() {
    let registry = registry_with(Writes {
        name: "writes-a-count",
        recipe: "crs",
        own: Vec::new(),
    });
    let recipe = Recipe::parse(ONE_STEP).expect("the recipe reads");
    let mut store = energyml_build::epc::open(None, "w.epc", Arc::new(energyml_schemas::all()))
        .expect("an empty package");
    let played = recipe::play(&recipe, Path::new("."), &registry, &mut store, &mut OneFile, Mode::Write)
        .unwrap_or_else(|stop| panic!("{stop}"));
    let activity = played.steps[0].activity.as_ref().expect("an activity");
    let summary = store.find_by_uuid(&activity.uuid).expect("it is in the package").clone();
    let object = store.get_object(&summary.uri).expect("it reads");
    let written: Value = energyml_core::json::write_value(object.as_reflect());
    let files: Vec<&Value> = written["Parameter"]
        .as_array()
        .expect("parameters")
        .iter()
        .filter(|p| p["Title"] == "inputFile")
        .collect();
    assert_eq!(files.len(), 1, "{written}");
    assert_eq!(files[0]["Value"], "features: donnees/a.geojson");
    assert_eq!(files[0]["$type"], "eml23.StringParameter");
}
