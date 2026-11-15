//! A recipe: the ordered steps that build a package, played through the one
//! door.
//!
//! A recipe can be corrected or extended later, and the code can replay the
//! generation. Chosen: **a versioned recipe, a runner in the
//! library, a command** - neither a session script nor a program written for
//! one model.
//!
//! # What a recipe is
//!
//! A TOML file (the reference model's recipe is one): a header - the package's name,
//! the instant and the author every object is stamped with, the HDF5 file, the
//! title of the workflow template - and `[[etape]]`s, in order. A step names
//! an operation of the registry, its answers (`set`, typed as TOML types them),
//! its inputs (`entrees`), the template of the tables it reads, and the use
//! cases and objects of the model it serves (`sert`, recopied, never read).
//!
//! An input is **either** a file relative to the recipe **or** what an earlier
//! step wrote, named by that step's name and narrowed by type, title or role -
//! never a uuid copied from a previous run. A setting that takes an object of
//! the package (`crs`, `interpretation`, …) names it the same way:
//! `crs = { etape = "reperes", type = "eml23.LocalEngineeringCompoundCrs" }`.
//!
//! # How it is played
//!
//! Every step goes through the path `energyml create` takes, and no other:
//! [`crate::entries`] reads its files, [`crate::read::every`] fills what it
//! leaves empty, [`crate::operation::Registry::plan`] plans with
//! `Identity::derived()` and the recipe's instant, [`crate::apply`] writes into
//! **one** in-memory package for the whole run. The first refusal stops the run
//! and names the step - a step is never skipped in silence, and a package half
//! made is never saved.
//!
//! A step can be **held** (`attente = "why"`): it is checked like the others,
//! reported as held with its reason, and not played. A step that takes what a
//! held step would write is refused before anything runs, unless it is held
//! too - an input that is not there is not an empty input.
//!
//! # The recipe is the workflow
//!
//! Played, a recipe writes an `ActivityTemplate` of its own - one `subActivity`
//! parameter per step, whose `Constraint` names the operation's canonical
//! template (a template cannot name the template of its sub-activities in any
//! other way, `Activities.xsd` l. 155-211) - and a root `Activity`. **Every
//! operation's activity carries `Parent` → that root from the moment it is
//! written**: the root's uuid is derived from the recipe before the first step,
//! which the workflow structure requires. The root names each step's activity in a
//! `DataObjectParameter` titled after the step, so the tree is stated both ways.
//!
//! # What makes two runs write one package
//!
//! The instant and the author are the recipe's; every object uuid is derived
//! from content (the operations' rule); the workflow's are derived from the
//! recipe. One thing is added to what an operation derives: **the step's name
//! enters its activity's seed**. Two steps of one operation over the same seed -
//! the depth and the time frames of `crs`, over the same EPSG codes - would
//! otherwise write one activity twice, and the second would replace the first:
//! a step of the workflow gone without a word.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use energyml_domain::activity::{
    ActivityParameter, ActivityRecord, ActivityRole, DeclaredReference, ParameterDirection, ParameterKind,
    ParameterTemplateRecord, ParameterValue, RoleBasis, RoleRules, TemplateRecord,
};
use energyml_domain::package_index::PackageIndex;
use energyml_domain::provenance_graph::{InputFileReader, StepConstraint, StepConstraintReader};
use energyml_eml_2_3 as eml;
use energyml_storage::EpcStore;

use crate::apply::Made;
use crate::entries::{self, Numbering, Reader};
use crate::operation::{AnyOperation, Choices, Dropped, Identity, Note, Registry, When};
use crate::pieces::{Known, Pieces, Ref, Source};
use crate::source::{Source as Where, name_of};

/// How a missing table template is to be given, in a refusal.
const GIVE_TEMPLATE: &str = "`template = \"<file.json>\"` on its step";

/// A recipe, as written.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    /// The package's name - and the root activity's, unless `activite` says
    /// otherwise.
    #[serde(rename = "titre")]
    pub title: String,
    /// `YYYY-MM-DDThh:mm:ssZ`, stamped on every object: **stated**, so that
    /// two runs of one recipe write one package.
    pub creation: String,
    /// Who is writing, for every `Citation`.
    #[serde(rename = "auteur")]
    pub originator: String,
    /// The HDF5 file every step writes its arrays into, unless its `set`
    /// names another `dataFile`.
    #[serde(rename = "donnees", default = "data_h5")]
    pub data_file: String,
    /// The title of the workflow's `ActivityTemplate` - `workflow:lutetia`.
    pub workflow: String,
    /// The title of the root `Activity`; left out, [`Recipe::title`].
    #[serde(rename = "activite", default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<String>,
    /// What the workflow is, for its `Citation/Description`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The steps, in the order they are played.
    #[serde(rename = "etape", default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<Step>,
}

fn data_h5() -> String {
    "data.h5".to_string()
}

/// One step: an operation, its answers, its inputs.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// Its name - unique, and how a later step names what it wrote.
    #[serde(rename = "nom")]
    pub name: String,
    /// The operation, as `energyml create --list` names it.
    pub operation: String,
    /// The answers, by slot name, typed as TOML types them - a number, a flag,
    /// a list, text - or a reference to what an earlier step wrote.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub set: BTreeMap<String, Value>,
    /// The inputs.
    #[serde(rename = "entrees", default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<Entry>,
    /// What the columns of its tables mean - relative to the recipe, like
    /// `energyml create --template`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    /// The use cases and objects of the model it serves - recopied into the
    /// report and the workflow, never read.
    #[serde(rename = "sert", default, skip_serializing_if = "Vec::is_empty")]
    pub serves: Vec<String>,
    /// Why it waits, when it does: checked, reported, not played.
    #[serde(rename = "attente", default, skip_serializing_if = "Option::is_none")]
    pub held: Option<String>,
    /// The group it belongs to, when a variant of the package may leave it out -
    /// `erreur` for the steps of Lutetia's `[ERROR]` model. See
    /// [`Recipe::without`].
    #[serde(rename = "groupe", default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

/// One input of a step: a file, or what an earlier step wrote.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// The slot it goes into; left out, the piece is named after the file or
    /// the object, as `--from` without `slot=` does.
    #[serde(rename = "creneau", default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<String>,
    /// A file, relative to the recipe's folder.
    #[serde(rename = "fichier", default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// The earlier step whose objects are taken.
    #[serde(rename = "etape", default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    /// Only its objects of this type - `eml23.VerticalCrs`, or the bare class.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub qualified_type: Option<String>,
    /// Only its objects of this exact title.
    #[serde(rename = "titre", default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Only its objects of this role in the plan - `representation`, `crs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

/// Plan and write, or plan only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Every step is planned and applied, arrays included.
    Write,
    /// Every step is planned and applied **without its arrays** into a package
    /// that is never saved - what a quote can say. A step that reads the
    /// geometry an earlier one would write stops there, and says why.
    Plan,
}

/// An object a step wrote, as a later step can name it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    /// Its role in the plan - `representation`, `crs`, `feature`.
    pub role: String,
    /// `resqml22.PolylineSetRepresentation`.
    pub qualified_type: String,
    /// Its uuid.
    pub uuid: String,
    /// Its title.
    pub title: String,
    /// **Its address** - the full uri, the version it declares included
    /// (ADR-0003). What a later step reads it by: a step may write a
    /// second version of an object beside the first, and both have one uuid.
    pub uri: String,
}

impl Object {
    /// The object a step wrote, from the reference naming it.
    fn of_reference(role: &str, reference: &Ref) -> Self {
        Self {
            role: role.to_string(),
            qualified_type: reference.qualified_type.clone(),
            uuid: reference.uuid.clone(),
            title: reference.title.clone(),
            uri: reference.uri(),
        }
    }

    /// The reference to it - uuid, type, title **and version** (the uri's).
    fn reference(&self) -> Ref {
        Ref::of_uri(&self.uri, self.title.clone())
            .unwrap_or_else(|| Ref::new(self.uuid.clone(), self.qualified_type.clone(), self.title.clone()))
    }
}

/// What one step did.
#[derive(Debug, Clone)]
pub struct Done {
    /// The step's name.
    pub step: String,
    /// Its operation.
    pub operation: String,
    /// What it serves, as the recipe says.
    pub serves: Vec<String>,
    /// Why it was not played, when it was held.
    pub held: Option<String>,
    /// Its activity's title - empty when held.
    pub title: String,
    /// The objects its plan made, in order.
    pub objects: Vec<Object>,
    /// How many arrays its plan holds.
    pub arrays: usize,
    /// What the plan wanted said.
    pub says: Vec<Note>,
    /// What the plan set aside, counted.
    pub dropped: Vec<Dropped>,
    /// What reading its inputs set aside - a feature of a GeoJSON that did
    /// not read, named.
    pub set_aside: Vec<String>,
    /// What applying it did - written, replaced, versioned. `None` when held.
    pub made: Option<Made>,
    /// Its activity, as the workflow names it. `None` when held.
    pub activity: Option<Ref>,
    /// What it wrote, as the run manifest says it - `None` when held.
    pub manifest: Option<crate::manifest::Step>,
    /// The external files its plan wrote arrays into.
    pub data_files: Vec<String>,
}

impl Done {
    fn held(step: &Step, why: &str) -> Done {
        Done {
            step: step.name.clone(),
            operation: step.operation.clone(),
            serves: step.serves.clone(),
            held: Some(why.to_string()),
            title: String::new(),
            objects: Vec::new(),
            arrays: 0,
            says: Vec::new(),
            dropped: Vec::new(),
            set_aside: Vec::new(),
            made: None,
            activity: None,
            manifest: None,
            data_files: Vec::new(),
        }
    }
}

/// The workflow a recipe writes: its template and its root activity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workflow {
    /// The `ActivityTemplate` - one `subActivity` parameter per step.
    pub template: Ref,
    /// The root `Activity`, every step's `Parent`.
    pub activity: Ref,
}

/// What a recipe played.
#[derive(Debug, Clone)]
pub struct Played {
    /// Each step, in order, held ones included.
    pub steps: Vec<Done>,
    /// The workflow written.
    pub workflow: Workflow,
}

impl Played {
    /// **The run manifest** of what was played: one step per step played (a held
    /// step wrote nothing and has none), `recipe` the recipe's title, the data
    /// files the plans wrote, first met first. `epc` is the package's file name.
    pub fn manifest(&self, epc: &str, recipe: &str) -> crate::manifest::Manifest {
        let mut files: Vec<String> = Vec::new();
        for done in &self.steps {
            for file in &done.data_files {
                if !files.contains(file) {
                    files.push(file.clone());
                }
            }
        }
        let steps = self
            .steps
            .iter()
            .filter_map(|done| done.manifest.clone())
            .collect();
        crate::manifest::Manifest::new(epc, files, Some(recipe.to_string()), steps)
    }
}

/// Where a run stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoppedAt {
    /// The recipe itself: it does not read, or it does not hold together.
    Recipe,
    /// A step refused.
    Step {
        /// Its name.
        name: String,
        /// Its operation.
        operation: String,
    },
    /// The workflow, written after every step.
    Workflow,
    /// The package, saved at the end.
    Package,
}

/// Why a run did not reach its end - where, and every refusal found there.
#[derive(Debug, Clone)]
pub struct Stop {
    /// Where.
    pub at: StoppedAt,
    /// What refused, in the order found. Never empty.
    pub refusals: Vec<String>,
    /// What the steps before it did. **Nothing was saved.**
    pub done: Vec<Done>,
}

impl Stop {
    fn recipe(refusals: Vec<String>) -> Stop {
        Stop {
            at: StoppedAt::Recipe,
            refusals,
            done: Vec::new(),
        }
    }
}

impl std::fmt::Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let said = self.refusals.join("; ");
        match &self.at {
            StoppedAt::Recipe => write!(f, "the recipe is refused before any step: {said}"),
            StoppedAt::Step { name, operation } => write!(
                f,
                "step `{name}` (`{operation}`) refused, and the run stops there: {said}"
            ),
            StoppedAt::Workflow => write!(f, "the workflow could not be written: {said}"),
            StoppedAt::Package => write!(f, "the package could not be saved: {said}"),
        }
    }
}

impl std::error::Error for Stop {}

impl Recipe {
    /// Read a recipe. A key it does not know is refused by name - `entree`
    /// for `entrees` would otherwise be a step with no input, which reads like
    /// one that needs none.
    pub fn parse(text: &str) -> Result<Recipe, Stop> {
        toml::from_str(text).map_err(|why| Stop::recipe(vec![format!("it does not read: {why}")]))
    }

    /// The root activity's title.
    pub fn activity_title(&self) -> &str {
        self.activity.as_deref().unwrap_or(&self.title)
    }

    /// **The same recipe, with the steps of these groups left out** - held,
    /// with the reason said, so that they are checked, reported and declared
    /// in the workflow's template like any held step, and not played.
    ///
    /// The use case: a variant that opens the model without the objects in
    /// error. A variant is the recipe played without a group,
    /// never a second recipe: two copies of eighty steps would part company at
    /// the first edit of one. Held, the steps keep what `check` already says of
    /// held steps: a step left in that takes what a left-out one writes is
    /// refused by name, rather than played on an empty slot.
    ///
    /// A group no step belongs to is refused: a misspelt `--without` leaving
    /// everything in would write the full package under a name that promises
    /// otherwise.
    pub fn without(&self, groups: &[&str]) -> Result<Recipe, String> {
        for group in groups {
            if !self
                .steps
                .iter()
                .any(|step| step.group.as_deref() == Some(*group))
            {
                let known: std::collections::BTreeSet<&str> = self
                    .steps
                    .iter()
                    .filter_map(|step| step.group.as_deref())
                    .collect();
                return Err(format!(
                    "no step of this recipe is in the group `{group}` - its groups are {}",
                    if known.is_empty() {
                        "none".to_string()
                    } else {
                        known
                            .iter()
                            .map(|one| format!("`{one}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                ));
            }
        }
        let mut out = self.clone();
        for step in &mut out.steps {
            if let Some(group) = step.group.as_deref()
                && groups.contains(&group)
                && step.held.is_none()
            {
                step.held = Some(format!(
                    "left out of this variant of the package: group `{group}`"
                ));
            }
        }
        Ok(out)
    }

    /// Whether the recipe holds together - **before any plan**, every fault at
    /// once: an unknown operation, a name used twice, an input from a step
    /// that comes later, from no step, or from a held one.
    pub fn check(&self, operations: &Registry) -> Result<(), Stop> {
        let mut wrong: Vec<String> = Vec::new();
        if self.title.trim().is_empty() {
            wrong.push("`titre` is empty: the package and its workflow are named after it".to_string());
        }
        if !is_instant(&self.creation) {
            wrong.push(format!(
                "`creation` is `{}`: expected `YYYY-MM-DDThh:mm:ssZ` - the instant every object is stamped with, stated so that two runs write one package",
                self.creation
            ));
        }
        if self.originator.trim().is_empty() {
            wrong.push("`auteur` is empty: every `Citation` names who wrote it".to_string());
        }
        if self.workflow.trim().is_empty() {
            wrong.push("`workflow` is empty: it titles the template the recipe writes".to_string());
        }
        if self.data_file.trim().is_empty() {
            wrong.push("`donnees` is empty: it names the file the arrays go into".to_string());
        }
        if self.steps.is_empty() {
            wrong.push("the recipe has no `[[etape]]`: there is nothing to play".to_string());
        } else if self.steps.iter().all(|step| step.held.is_some()) {
            wrong.push("every step is held (`attente`): nothing would be written".to_string());
        }

        // The names seen so far, and whether each is held: what a step may
        // name is what came before it.
        let mut earlier: Vec<(&str, Option<&str>)> = Vec::new();
        for (at, step) in self.steps.iter().enumerate() {
            let named = format!("step {} `{}`", at + 1, step.name);
            if step.name.trim().is_empty() {
                wrong.push(format!(
                    "step {} has no `nom`: a later step names what it wrote by it",
                    at + 1
                ));
            } else if let Some(first) = self.steps[..at].iter().position(|one| one.name == step.name) {
                wrong.push(format!(
                    "{named}: the name is already step {}'s - a step is named to be referenced, and two of one name are one ambiguity",
                    first + 1
                ));
            }
            match operations.get(&step.operation) {
                None => wrong.push(format!(
                    "{named}: no operation is called `{}` - `energyml create --list` names them",
                    step.operation
                )),
                // **A setting the operation does not declare is refused**, not
                // carried: `azimuht` for `azimuth` would otherwise be a frame
                // written without its rotation, which reads like one that has
                // none. `needs` answers on the empty bag - it is what the
                // template is projected from - so every slot is there, the
                // conditional ones included.
                Some(operation) => {
                    let declared: Vec<&'static str> = operation
                        .needs(&Pieces::empty())
                        .into_iter()
                        .map(|slot| slot.name)
                        .collect();
                    for key in step.set.keys().filter(|key| !declared.contains(&key.as_str())) {
                        wrong.push(format!(
                            "{named}: `{}` has no setting `{key}` - it takes {}",
                            step.operation,
                            declared
                                .iter()
                                .map(|one| format!("`{one}`"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    }
                }
            }
            if step.held.as_deref().is_some_and(|why| why.trim().is_empty()) {
                wrong.push(format!(
                    "{named}: `attente` is empty - a held step says why it waits"
                ));
            }

            let mut targets: Vec<String> = Vec::new();
            for (k, entry) in step.inputs.iter().enumerate() {
                let entry_named = format!("{named}, input {}", k + 1);
                match (&entry.file, &entry.step) {
                    (Some(_), Some(_)) => wrong.push(format!(
                        "{entry_named}: it names a file **and** a step - an input is one or the other"
                    )),
                    (None, None) => wrong.push(format!(
                        "{entry_named}: it names neither a `fichier` nor an `etape`"
                    )),
                    (Some(file), None) => {
                        if entry.qualified_type.is_some() || entry.title.is_some() || entry.role.is_some() {
                            wrong.push(format!(
                                "{entry_named}: `type`, `titre` and `role` narrow what a step wrote - a file has nothing of the kind to narrow"
                            ));
                        }
                        // A recipe names objects by the step that wrote them,
                        // never by a uuid copied out of another run.
                        if file.contains('#') {
                            wrong.push(format!(
                                "{entry_named}: `{file}` names an object by its uuid - a recipe names what an earlier step wrote with `etape`, never a uuid copied from a run"
                            ));
                        }
                    }
                    (None, Some(target)) => targets.push(target.clone()),
                }
            }
            for (key, value) in &step.set {
                let mut picks: Vec<Result<Pick, String>> = Vec::new();
                picks_in(value, &mut picks);
                for pick in picks {
                    match pick {
                        Ok(pick) => targets.push(pick.step),
                        Err(why) => wrong.push(format!("{named}, `set.{key}`: {why}")),
                    }
                }
            }
            for target in targets {
                if target == step.name {
                    wrong.push(format!(
                        "{named}: it takes what it writes itself - a step reads what earlier steps wrote"
                    ));
                } else if let Some((_, held)) = earlier.iter().find(|(name, _)| *name == target) {
                    if let (Some(why), None) = (held, &step.held) {
                        wrong.push(format!(
                            "{named}: it takes what `{target}` writes, and `{target}` is held ({why}) - hold this step too, or release that one"
                        ));
                    }
                } else if self.steps[at + 1..].iter().any(|later| later.name == target) {
                    wrong.push(format!(
                        "{named}: it takes what `{target}` writes, and `{target}` comes after it - a step reads what earlier steps wrote"
                    ));
                } else {
                    wrong.push(format!(
                        "{named}: it takes what `{target}` writes, and no step is called `{target}`"
                    ));
                }
            }
            earlier.push((step.name.as_str(), step.held.as_deref()));
        }
        if wrong.is_empty() {
            Ok(())
        } else {
            Err(Stop::recipe(wrong))
        }
    }

    /// The workflow's template and root activity, **derived from the recipe
    /// before any step runs** - which is what lets every step's activity name
    /// its `Parent` as it is written.
    ///
    /// The template from what it states: each step's name, operation, what it
    /// serves and whether it is held. The root activity from the template, the
    /// title and the instant: a run of the recipe is an event.
    pub fn workflow(&self) -> Workflow {
        let structure: Vec<String> = self
            .steps
            .iter()
            .map(|step| {
                format!(
                    "{}={}[{}]{}",
                    step.name,
                    step.operation,
                    step.serves.join(","),
                    if step.held.is_some() { "?" } else { "" }
                )
            })
            .collect();
        let template = derived(&format!(
            "recipe-workflow-template:{}:{}",
            self.workflow,
            structure.join(";")
        ));
        let activity = derived(&format!(
            "recipe-workflow-activity:{}:{template}:{}",
            self.activity_title(),
            self.creation
        ));
        Workflow {
            template: Ref::new(template, "eml23.ActivityTemplate", self.workflow.clone()),
            activity: Ref::new(activity, "eml23.Activity", self.activity_title().to_string()),
        }
    }
}

/// A uuid v5 in the crate's namespace - the one every derived identity hangs
/// from.
fn derived(name: &str) -> String {
    uuid::Uuid::new_v5(&crate::naming::NAMESPACE, name.as_bytes()).to_string()
}

/// `YYYY-MM-DDThh:mm:ssZ`, digits where digits go.
fn is_instant(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 20
        && bytes.iter().enumerate().all(|(at, byte)| match at {
            4 | 7 => *byte == b'-',
            10 => *byte == b'T',
            13 | 16 => *byte == b':',
            19 => *byte == b'Z',
            _ => byte.is_ascii_digit(),
        })
}

/// A reference to what an earlier step wrote, and what narrows it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pick {
    step: String,
    qualified_type: Option<String>,
    title: Option<String>,
    role: Option<String>,
}

impl Pick {
    fn of_entry(entry: &Entry) -> Option<Pick> {
        Some(Pick {
            step: entry.step.clone()?,
            qualified_type: entry.qualified_type.clone(),
            title: entry.title.clone(),
            role: entry.role.clone(),
        })
    }

    /// A reference written as a setting: a table whose `etape` names the
    /// step. Any other table is left as it is - `Ok(None)`.
    fn of_value(value: &Value) -> Result<Option<Pick>, String> {
        let Value::Object(map) = value else {
            return Ok(None);
        };
        let Some(step) = map.get("etape") else {
            return Ok(None);
        };
        let Value::String(step) = step else {
            return Err(format!(
                "`etape` is {step}, and it names a step by its name, in text"
            ));
        };
        if let Some(key) = map
            .keys()
            .find(|key| !["etape", "type", "titre", "role"].contains(&key.as_str()))
        {
            return Err(format!(
                "`{key}` is no part of a reference to a step - `etape`, and to narrow it `type`, `titre`, `role`"
            ));
        }
        let text = |key: &str| -> Result<Option<String>, String> {
            match map.get(key) {
                None => Ok(None),
                Some(Value::String(text)) => Ok(Some(text.clone())),
                Some(other) => Err(format!("`{key}` is {other}, and it narrows a reference in text")),
            }
        };
        Ok(Some(Pick {
            step: step.clone(),
            qualified_type: text("type")?,
            title: text("titre")?,
            role: text("role")?,
        }))
    }

    fn keeps(&self, object: &Object) -> bool {
        self.qualified_type
            .as_deref()
            .is_none_or(|wanted| same_type(wanted, &object.qualified_type))
            && self.title.as_deref().is_none_or(|wanted| wanted == object.title)
            && self.role.as_deref().is_none_or(|wanted| wanted == object.role)
    }

    /// The reference in words, for a refusal.
    fn said(&self) -> String {
        let mut out = format!("`etape = {}`", self.step);
        if let Some(kind) = &self.qualified_type {
            out.push_str(&format!(", `type = {kind}`"));
        }
        if let Some(title) = &self.title {
            out.push_str(&format!(", `titre = {title}`"));
        }
        if let Some(role) = &self.role {
            out.push_str(&format!(", `role = {role}`"));
        }
        out
    }
}

/// Every reference a setting holds - itself, or the items of its list.
fn picks_in(value: &Value, out: &mut Vec<Result<Pick, String>>) {
    match value {
        Value::Array(items) => items.iter().for_each(|item| picks_in(item, out)),
        Value::Object(_) => {
            if let Some(found) = Pick::of_value(value).transpose() {
                out.push(found);
            }
        }
        _ => {}
    }
}

/// Whether an object of type `qualified` is of the type `wanted` names - the
/// qualified type exactly when it has a package, the bare class otherwise,
/// case and RESQML 2.0.1's `obj_` ignored, as [`crate::operation::Takes::accepts`]
/// compares.
fn same_type(wanted: &str, qualified: &str) -> bool {
    if wanted.contains('.') {
        return wanted.eq_ignore_ascii_case(qualified);
    }
    let class = qualified
        .rsplit('.')
        .next()
        .unwrap_or(qualified)
        .to_ascii_lowercase();
    let class = class.strip_prefix("obj_").map(str::to_string).unwrap_or(class);
    class == wanted.to_ascii_lowercase()
}

/// A few objects in words, for a refusal that says what *was* there.
fn listed(objects: &[Object]) -> String {
    const SHOWN: usize = 10;
    if objects.is_empty() {
        return "nothing".to_string();
    }
    let mut out: Vec<String> = objects
        .iter()
        .take(SHOWN)
        .map(|one| format!("`{}` ({}, {})", one.title, one.qualified_type, one.role))
        .collect();
    if objects.len() > SHOWN {
        out.push(format!("and {} more", objects.len() - SHOWN));
    }
    out.join(", ")
}

/// What an earlier step wrote that a reference keeps - at least one.
fn picked<'a>(pick: &Pick, written: &'a BTreeMap<String, Vec<Object>>) -> Result<Vec<&'a Object>, String> {
    let objects = written.get(&pick.step).map(Vec::as_slice).unwrap_or_default();
    let kept: Vec<&Object> = objects.iter().filter(|one| pick.keeps(one)).collect();
    if kept.is_empty() {
        return Err(format!(
            "{} names nothing: that step wrote {}",
            pick.said(),
            listed(objects)
        ));
    }
    Ok(kept)
}

/// A file of the recipe, as a source the caller's reader can read.
fn file_source(base: &Path, file: &str) -> Where {
    Where::File(base.join(file).to_string_lossy().into_owned())
}

/// What an earlier step wrote, as pieces: a representation as its shape and
/// its companion, anything else as what is known of it - the line
/// [`crate::read::every`] and the front end draw, **on the type**.
fn pieces_of_written(
    store: &EpcStore,
    object: &Object,
    slot: Option<&str>,
    mode: Mode,
) -> Result<Vec<(String, Source)>, String> {
    // **A representation this build cannot draw has no shape to offer**, and
    // is an object like any other - what a collection's members take. Decided
    // on the type (`energyml_geometry::geometry::is_supported`), never on the
    // words of a failed read: a framework is written by `surface-framework`
    // and gathered by the project, and nothing reads its geometry yet.
    //
    // Everything below reads the object **by its uri**: a step may have written
    // a second version beside the first, and a uuid would name them both.
    let drawable = store
        .get_object(&object.uri)
        .ok()
        .is_none_or(|read| energyml_geometry::geometry::is_supported(read.as_reflect()));
    if !drawable {
        let known = crate::read::known_of_object(store, &object.uri)?;
        let name = slot.map(str::to_string).unwrap_or_else(|| object.title.clone());
        return Ok(vec![(name, Source::Object(known))]);
    }
    match crate::read::pieces_of_object(store, &object.uri) {
        Ok(read) => Ok(match slot {
            Some(slot) => crate::read::in_slot(slot, read),
            None => read,
        }),
        Err(why) if object.qualified_type.ends_with("Representation") => Err(match mode {
            Mode::Write => format!("`{}` ({}): {why}", object.title, object.qualified_type),
            // **Said, not passed off as a fault of the data**: a plan writes no
            // array, so a geometry an earlier step planned is not there to be
            // read back.
            Mode::Plan => format!(
                "`{}` ({}) cannot be read back: a plan writes no array, so the geometry an earlier step would write is not there - play the recipe with an output to go further ({why})",
                object.title, object.qualified_type
            ),
        }),
        Err(_) => {
            let known = crate::read::known_of_object(store, &object.uri)?;
            let name = slot.map(str::to_string).unwrap_or_else(|| object.title.clone());
            Ok(vec![(name, Source::Object(known))])
        }
    }
}

/// A setting, its references to earlier steps replaced by the object they
/// name - **one** object each, a setting taking one - and those objects kept
/// aside so that what is known of them can come back into the bag.
fn resolved(
    value: &Value,
    written: &BTreeMap<String, Vec<Object>>,
    answered: &mut Vec<Object>,
) -> Result<Value, String> {
    match value {
        Value::Array(items) => items
            .iter()
            .map(|item| resolved(item, written, answered))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(_) => match Pick::of_value(value)? {
            None => Ok(value.clone()),
            Some(pick) => {
                let kept = picked(&pick, written)?;
                let [one] = kept.as_slice() else {
                    let owned: Vec<Object> = kept.iter().copied().cloned().collect();
                    return Err(format!(
                        "{} names {} objects - {}; a setting takes one: narrow it with `type`, `titre` or `role`",
                        pick.said(),
                        owned.len(),
                        listed(&owned)
                    ));
                };
                answered.push((*one).clone());
                Ok(one.reference().dor())
            }
        },
        other => Ok(other.clone()),
    }
}

/// Play one step into the store - the path `energyml create` takes.
#[allow(clippy::too_many_arguments)]
fn play_step(
    step: &Step,
    recipe: &Recipe,
    base: &Path,
    operations: &Registry,
    store: &mut EpcStore,
    reader: &mut dyn Reader,
    mode: Mode,
    when: &When,
    workflow: &Workflow,
    written: &BTreeMap<String, Vec<Object>>,
) -> Result<Done, Vec<String>> {
    let _step = tracing::info_span!(
        "recipe.step",
        step = step.name.as_str(),
        operation = step.operation.as_str()
    )
    .entered();
    let one = |why: String| vec![why];
    let operation = operations
        .get(&step.operation)
        .ok_or_else(|| one(format!("no operation is called `{}`", step.operation)))?;
    let mut set_aside: Vec<String> = Vec::new();

    // The template of its tables, read once for the step - `--template`.
    let template = match &step.template {
        None => None,
        Some(file) => Some(entries::template(reader, &file_source(base, file)).map_err(one)?),
    };

    let mut bag = Pieces::empty();
    let mut numbering = Numbering::default();
    let reading = tracing::info_span!("recipe.read_inputs", inputs = step.inputs.len()).entered();
    for entry in &step.inputs {
        if let Some(file) = &entry.file {
            let source = file_source(base, file);
            // The piece takes the slot's name; the collection keeps the
            // file's, which is what an operation reads a stage or a date from.
            let name = name_of(&source);
            let piece = entry
                .slot
                .as_deref()
                .map(|slot| numbering.next(slot))
                .unwrap_or_else(|| name.clone());
            // A file for a slot that takes a **document** is carried as bytes
            // - the operation's declaration decides, never the file's name.
            if entries::takes_document(&operation.takes(), entry.slot.as_deref()) {
                let (bytes, declared) = reader.bytes(&source).map_err(one)?;
                bag = bag.with(piece, entries::read_document(&name, bytes, declared.as_deref()));
                continue;
            }
            let (text, declared) = reader.text(&source).map_err(one)?;
            let read = entries::read_text(
                &name,
                &text,
                declared.as_deref(),
                template.as_ref(),
                file,
                &mut set_aside,
            )
            .map_err(|why| one(why.said(GIVE_TEMPLATE)))?;
            bag = bag.with(piece, read);
        } else if let Some(pick) = Pick::of_entry(entry) {
            for object in picked(&pick, written).map_err(one)? {
                let slot = entry.slot.as_deref().map(|slot| numbering.next(slot));
                for (name, source) in pieces_of_written(store, object, slot.as_deref(), mode).map_err(one)? {
                    bag = bag.with(name, source);
                }
            }
        }
    }

    drop(reading);
    // The answers, references resolved. What a reference named comes back into
    // the bag as a **companion** - `crs (object)` - **after** the inputs: an
    // answer is an input (`read::known_of_object`), and the frame of a CRS
    // named here gives its vertical half this way, as it does in the front end. A
    // companion binds to no slot, so it cannot be taken for an input.
    let mut choices = Choices::new();
    for (key, value) in &step.set {
        let mut answered: Vec<Object> = Vec::new();
        let value =
            resolved(value, written, &mut answered).map_err(|why| one(format!("`set.{key}`: {why}")))?;
        choices = choices.set(key, value);
        let mut named = Numbering::default();
        for object in answered {
            let known: Known = crate::read::known_of_object(store, &object.uri).map_err(one)?;
            bag = bag.with(format!("{} (object)", named.next(key)), Source::Object(known));
        }
    }
    // The recipe's HDF5 file, for an operation that writes arrays and was not
    // told another.
    if choices.get("dataFile").is_none() && operation.needs(&bag).iter().any(|slot| slot.name == "dataFile") {
        choices = choices.set("dataFile", recipe.data_file.clone());
    }
    // **What a slot takes when left empty, from the package being built** -
    // `Otherwise::Every`, the function `--into` and the front end call.
    let bag = crate::read::every(
        store,
        &operation.takes(),
        bag,
        energyml_geometry::Placement::Local,
    )
    .map_err(|why| one(why.to_string()))?;

    let mut plan = operations
        .plan(&step.operation, &bag, &choices, &Identity::derived(), when)
        .map_err(|why| {
            why.refusals()
                .iter()
                .map(|refusal| refusal.message.clone())
                .collect::<Vec<_>>()
        })?;
    // The step's name in its activity's seed - see the module.
    plan.activity.seed = format!("{} - step {}", plan.activity.seed, step.name);
    // **The files this step read**, as its activity records them, in the
    // recipe's order: what the object inputs cannot say (common TRG 2.3
    // section 3.1, p. 40: the title has an equivalent in the template, which
    // declares `inputFile`).
    for entry in &step.inputs {
        if let Some(file) = &entry.file {
            plan.activity
                .parameters
                .push(input_file_parameter(entry.slot.as_deref(), file));
        }
    }

    let made = {
        let _span = tracing::info_span!(
            "recipe.apply",
            objects = plan.objects.len(),
            datasets = plan.datasets.len()
        )
        .entered();
        crate::apply::apply_under(&plan, store, when, mode == Mode::Write, Some(&workflow.activity))
    };
    if !made.refused.is_empty() {
        return Err(made.refused);
    }
    let activity = made
        .activity
        .as_deref()
        // The uri `apply` wrote, read by the one parser: a reference to the
        // activity as it is, version included.
        .and_then(|uri| Ref::of_uri(uri, plan.activity.title.clone()));
    let manifest = crate::manifest::step(&step.name, operation, &plan, &made);
    Ok(Done {
        step: step.name.clone(),
        operation: step.operation.clone(),
        serves: step.serves.clone(),
        held: None,
        title: plan.activity.title.clone(),
        objects: plan
            .objects
            .iter()
            .map(|one| Object::of_reference(&one.role, &one.reference()))
            .collect(),
        arrays: plan.datasets.len(),
        says: plan.says.clone(),
        dropped: plan.dropped.clone(),
        set_aside,
        made: Some(made),
        activity,
        manifest: Some(manifest),
        data_files: crate::manifest::data_files(&plan),
    })
}

/// A file a step read, as its activity records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputFile {
    /// The slot the recipe named (`creneau`), when it named one.
    pub slot: Option<String>,
    /// The path, relative to the recipe, as the recipe wrote it.
    pub path: String,
}

/// The `inputFile` parameter of one file entry: `<slot>: <path>`, or `<path>`
/// when the recipe named no slot.
///
/// The path is written **as the recipe writes it**: no normalisation, no
/// `canonicalize`. A `StringParameter` has only a title and a value, and the
/// slot is needed to make the entry again, so it is part of the value; the
/// `Constraint` of the template's `inputFile` declares that form
/// ([`crate::template::INPUT_FILE_CONSTRAINT`]).
pub fn input_file_parameter(slot: Option<&str>, path: &str) -> energyml_domain::provenance::Parameter {
    let value = match slot {
        Some(slot) if !slot.is_empty() => format!("{slot}: {path}"),
        _ => path.to_string(),
    };
    energyml_domain::provenance::Parameter::Text {
        title: "inputFile".to_string(),
        value,
    }
}

/// Read an `inputFile` value back - the reader beside its writer. `None` for
/// an empty value.
///
/// The slot is what comes before the **first** `": "`, when that is a name
/// (`[A-Za-z0-9_-]`, not empty); anything else is a path as it stands, so a
/// Windows `C:\x` (no space after the colon) stays whole.
pub fn read_input_file(value: &str) -> Option<InputFile> {
    if value.trim().is_empty() {
        return None;
    }
    if let Some((before, after)) = value.split_once(": ")
        && !before.is_empty()
        && before
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Some(InputFile {
            slot: Some(before.to_string()),
            path: after.to_string(),
        });
    }
    Some(InputFile {
        slot: None,
        path: value.to_string(),
    })
}

/// The workflow: its template, one `subActivity` parameter per step, and its
/// root activity, naming each step's activity.
fn write_workflow(
    recipe: &Recipe,
    workflow: &Workflow,
    done: &[Done],
    operations: &Registry,
    store: &mut EpcStore,
) -> Result<(), Vec<String>> {
    let citation = |title: &str| eml::Citation {
        title: title.to_string(),
        originator: recipe.originator.clone(),
        creation: recipe.creation.clone(),
        format: energyml_core::CITATION_FORMAT.to_string(),
        description: recipe.description.clone(),
        ..Default::default()
    };
    let reference = |one: &Ref| eml::DataObjectReference {
        uuid: one.uuid.clone(),
        qualified_type: one.qualified_type.clone(),
        title: one.title.clone(),
        ..Default::default()
    };

    let mut parameters: Vec<eml::ParameterTemplate> = Vec::new();
    for step in done {
        // **A template cannot name the template of its sub-activities**:
        // `ParameterTemplate` has no reference to another template, and
        // `DataObjectContentType` is for a `dataObject` only (Activities.xsd
        // l. 155-211). `Constraint` - free text - is the one place the schema
        // leaves for it, and it says so itself: *« it will be better to have a
        // formal description of the constraint »*.
        let maker: &str = operations
            .get(&step.operation)
            .map(|operation| operation.recipe())
            .unwrap_or(step.operation.as_str());
        let mut constraint = match energyml_domain::provenance::template_uuid(maker) {
            Ok(uuid) => format!("follows fabricant:{maker} ({uuid})"),
            Err(_) => format!("follows fabricant:{maker}"),
        };
        if !step.serves.is_empty() {
            constraint.push_str(&format!("; serves {}", step.serves.join(", ")));
        }
        if let Some(why) = &step.held {
            constraint.push_str(&format!("; held: {why}"));
        }
        parameters.push(eml::ParameterTemplate {
            allowed_kind: vec![eml::ActivityParameterKind::SubActivity],
            // **Neither**: a step is a part of the workflow, not what it
            // consumes nor what it produces. The schema requires both flags
            // and has no third word for this.
            is_input: false,
            is_output: false,
            title: step.step.clone(),
            max_occurs: 1,
            // A held step is declared - it is part of the workflow - and
            // absent from the run.
            min_occurs: if step.held.is_some() { 0 } else { 1 },
            // `String2000`, the schema's bound.
            constraint: Some(constraint.chars().take(2_000).collect()),
            ..Default::default()
        });
    }
    let template = eml::ActivityTemplate {
        uuid: workflow.template.uuid.clone(),
        schema_version: "2.3".to_string(),
        citation: citation(&workflow.template.title),
        parameter: parameters,
        ..Default::default()
    };

    // **The parent names its children** - the other half of `Parent`, so the
    // tree reads from either end (the parent names its children by its
    // parameters, each child names its parent by `Parent`).
    let children: Vec<eml::AbstractActivityParameter> = done
        .iter()
        .filter_map(|step| {
            let activity = step.activity.as_ref()?;
            Some(eml::AbstractActivityParameter::DataObjectParameter(
                eml::DataObjectParameter {
                    title: step.step.clone(),
                    is_uncertain: false,
                    data_object: reference(activity),
                    ..Default::default()
                },
            ))
        })
        .collect();
    if children.is_empty() {
        // `Activity.Parameter` is `1..∞`: an activity naming no step would not
        // be one, and `check` refuses a recipe where every step is held.
        return Err(vec![
            "no step wrote an activity, so the workflow has nothing to name".to_string(),
        ]);
    }
    let activity = eml::Activity {
        uuid: workflow.activity.uuid.clone(),
        schema_version: "2.3".to_string(),
        citation: citation(&workflow.activity.title),
        activity_descriptor: reference(&workflow.template),
        parameter: children,
        ..Default::default()
    };
    store
        .put(Box::new(template))
        .map_err(|why| vec![format!("its template: {why}")])?;
    store
        .put(Box::new(activity))
        .map_err(|why| vec![format!("its activity: {why}")])?;
    Ok(())
}

/// Play a recipe into `store` - every step, then the workflow. **Nothing is
/// saved**: the store is the caller's.
///
/// `base` is the recipe's folder, which its files are relative to; `reader` is
/// the caller's I/O. The recipe is checked first, and a malformed one is
/// refused before any plan.
pub fn play(
    recipe: &Recipe,
    base: &Path,
    operations: &Registry,
    store: &mut EpcStore,
    reader: &mut dyn Reader,
    mode: Mode,
) -> Result<Played, Stop> {
    let _play = tracing::info_span!(
        "recipe.play",
        recipe = recipe.title.as_str(),
        steps = recipe.steps.len()
    )
    .entered();
    recipe.check(operations)?;
    let when = When {
        creation: recipe.creation.clone(),
        originator: recipe.originator.clone(),
    };
    let workflow = recipe.workflow();
    let mut done: Vec<Done> = Vec::new();
    let mut written: BTreeMap<String, Vec<Object>> = BTreeMap::new();
    for step in &recipe.steps {
        if let Some(why) = &step.held {
            done.push(Done::held(step, why));
            continue;
        }
        match play_step(
            step, recipe, base, operations, store, reader, mode, &when, &workflow, &written,
        ) {
            Ok(one) => {
                // **What a step wrote is what `apply` wrote**, not only the
                // plan's objects: the template it added (`template`), the
                // collections grouping a plan's many objects (`grouping`), and
                // its activity (`activity`). A project collection holds every object of the
                // package, so an entry naming a step without a `type` must
                // bring them all - found by `tests/lutetia.rs`, which set the
                // project against the package; narrowed by `type`, they stay
                // out.
                let mut objects = one.objects.clone();
                if let Some(made) = &one.made {
                    for uri in made
                        .written
                        .iter()
                        .filter(|uri| Some(*uri) != made.activity.as_ref())
                    {
                        // The uri is read by `EtpUri` (through `Ref::of_uri`) and
                        // compared **whole**: a second version written by the step
                        // is a second object, not one already seen.
                        let Some(written) = Ref::of_uri(uri, "") else {
                            continue;
                        };
                        if objects.iter().any(|seen| seen.uri == written.uri()) {
                            continue;
                        }
                        let role = if written.qualified_type.ends_with("ActivityTemplate") {
                            "template"
                        } else {
                            "grouping"
                        };
                        let title = store
                            .get_object(uri)
                            .ok()
                            .and_then(|object| {
                                energyml_core::json::write_value(object.as_reflect())["Citation"]["Title"]
                                    .as_str()
                                    .map(str::to_string)
                            })
                            .unwrap_or_default();
                        objects.push(Object::of_reference(role, &Ref { title, ..written }));
                    }
                }
                if let Some(activity) = &one.activity {
                    objects.push(Object::of_reference("activity", activity));
                }
                written.insert(step.name.clone(), objects);
                done.push(one);
            }
            Err(refusals) => {
                return Err(Stop {
                    at: StoppedAt::Step {
                        name: step.name.clone(),
                        operation: step.operation.clone(),
                    },
                    refusals,
                    done,
                });
            }
        }
    }
    let workflow_span = tracing::info_span!("recipe.workflow").entered();
    let workflow_written = write_workflow(recipe, &workflow, &done, operations, store);
    drop(workflow_span);
    if let Err(refusals) = workflow_written {
        return Err(Stop {
            at: StoppedAt::Workflow,
            refusals,
            done,
        });
    }
    Ok(Played {
        steps: done,
        workflow,
    })
}

/// Play a recipe and save the package to `out` - the whole of
/// `energyml create --recipe … --out …`.
///
/// One in-memory package for the run, saved **only** when every step and the
/// workflow went through: a package half made is never written.
pub fn run(
    recipe: &Recipe,
    base: &Path,
    operations: &Registry,
    schemas: Arc<energyml_core::registry::Registry>,
    out: &Path,
    reader: &mut dyn Reader,
) -> Result<Played, Stop> {
    let name = out
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| out.display().to_string());
    // **The package's own date is the recipe's too.** `docProps/core.xml`
    // carries a `created`, and taken from the clock it was the one part two
    // runs of the same recipe wrote differently (measured: 412 parts of
    // 413 identical, byte for byte, and this one not). `SaveOptions::created`
    // is there for exactly this; a `creation` that does not read as a UTC
    // date-time is refused, never replaced by the clock.
    if energyml_storage::epc::core_props::unix_seconds(&recipe.creation).is_none() {
        return Err(Stop {
            at: StoppedAt::Package,
            refusals: vec![format!(
                "`creation = \"{}\"` is not a UTC date-time as `2026-09-24T00:00:00Z`: the package's `created` is taken from it",
                recipe.creation
            )],
            done: Vec::new(),
        });
    }
    let _run = tracing::info_span!("recipe.run", out = %out.display()).entered();
    let mut store = crate::epc::open(None, &name, schemas).map_err(|why| Stop {
        at: StoppedAt::Package,
        refusals: vec![format!("making an empty package: {why}")],
        done: Vec::new(),
    })?;
    // **A bare name is in the current folder**, and the store must be told:
    // without a root, `--out package.epc` wrote the package and refused each
    // of its arrays (« `data.h5`: file not found »), two thousand lines of
    // refusals for a file that looks as if it came out - the trap that
    // `energyml-cli/src/create.rs` comments on, set here the same way.
    let parent = out
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    store.external_files_mut().set_root(parent);
    let played = play(recipe, base, operations, &mut store, reader, Mode::Write)?;
    let options = energyml_storage::SaveOptions {
        created: Some(recipe.creation.clone()),
        ..Default::default()
    };
    let saved = {
        let _span =
            tracing::info_span!("recipe.save", path = %out.display(), steps = played.steps.len()).entered();
        store.save_as(out, &options)
    };
    if let Err(why) = saved {
        return Err(Stop {
            at: StoppedAt::Package,
            refusals: vec![format!("writing {}: {why}", out.display())],
            done: played.steps,
        });
    }
    Ok(played)
}

/// Plan a recipe - every step planned and applied without arrays into a
/// package that is never saved ([`Mode::Plan`]).
pub fn plan(
    recipe: &Recipe,
    base: &Path,
    operations: &Registry,
    schemas: Arc<energyml_core::registry::Registry>,
    reader: &mut dyn Reader,
) -> Result<Played, Stop> {
    let mut store =
        crate::epc::open(None, &format!("{}.epc", recipe.title), schemas).map_err(|why| Stop {
            at: StoppedAt::Package,
            refusals: vec![format!("making an empty package: {why}")],
            done: Vec::new(),
        })?;
    play(recipe, base, operations, &mut store, reader, Mode::Plan)
}

// ---------------------------------------------------------------------------
// Reading a workflow back: the constraint of a step, and the draft of a recipe
// ---------------------------------------------------------------------------

/// Read a constraint [`write_workflow`] wrote: `follows fabricant:<op> (<uuid>)`,
/// then `; serves <a>, <b>`, then `; held: <why>`. Anything else: `None`.
///
/// The reader lives beside its writer (the constraint is a convention of this
/// crate; `energyml-domain` knows none and reaches this one through
/// [`StepConstraintReader`]).
///
/// The reason a step is held may itself contain `;`, so `held` is everything
/// after the **first** `; held: `. A usage that contains `, ` is not read back
/// as one (the writer does not escape it).
pub fn read_step_constraint(text: &str) -> Option<StepConstraint> {
    let rest = text.strip_prefix("follows fabricant:")?;
    let (head, held) = match rest.split_once("; held: ") {
        Some((head, why)) => (head, Some(why.to_string())),
        None => (rest, None),
    };
    let (head, serves) = match head.split_once("; serves ") {
        Some((head, serves)) => (
            head,
            serves
                .split(", ")
                .filter(|one| !one.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>(),
        ),
        None => (head, Vec::new()),
    };
    let (operation, template_uuid) = match head.split_once(" (") {
        Some((operation, inside)) => {
            let uuid = inside.strip_suffix(')')?;
            if !energyml_core::identifiers::is_uuid(uuid) {
                return None;
            }
            (operation, Some(uuid.to_string()))
        }
        None => (head, None),
    };
    if operation.is_empty() || operation.contains(char::is_whitespace) {
        return None;
    }
    Some(StepConstraint {
        operation: operation.to_string(),
        template_uuid,
        serves,
        held,
    })
}

/// The recipe's constraint convention, for
/// [`energyml_domain::provenance_graph::workflow_steps`].
pub struct RecipeConstraints;

impl StepConstraintReader for RecipeConstraints {
    fn read(&self, constraint: &str) -> Option<StepConstraint> {
        read_step_constraint(constraint)
    }
}

/// The recipe's `inputFile` convention, for
/// [`energyml_domain::provenance_graph::activity_edges`]: a text parameter
/// titled `inputFile` of an activity whose originator is `energyml-rust` names
/// the file [`read_input_file`] finds in its value.
pub struct RecipeInputFiles;

impl InputFileReader for RecipeInputFiles {
    fn id(&self) -> &'static str {
        "energyml-build:input-file"
    }

    fn file(&self, activity: &ActivityRecord, parameter: &ActivityParameter) -> Option<String> {
        if activity.originator.as_deref() != Some("energyml-rust") || parameter.title != "inputFile" {
            return None;
        }
        let ParameterValue::Text(value) = &parameter.value else {
            return None;
        };
        read_input_file(value).map(|file| file.path)
    }
}

/// Why a workflow cannot be drafted back into a recipe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayRefusal {
    /// The root's template declares no `subActivity`.
    NotAWorkflow,
    /// The step's activity names a template that is no canonical template, or
    /// no operation records under it.
    OperationUnknown {
        /// The step's name.
        step: String,
        /// The template the activity names.
        template: String,
    },
    /// Several operations record under the step's template.
    OperationAmbiguous {
        /// The step's name.
        step: String,
        /// The template the activity names.
        template: String,
        /// The operations, sorted.
        operations: Vec<String>,
    },
    /// The step is declared, not held, and no activity of the root plays it.
    ChildMissing {
        /// The step's name.
        step: String,
    },
    /// The operation takes an input and its activity records neither an object
    /// nor a file.
    InputFileNotRecorded {
        /// The step's name.
        step: String,
    },
    /// An object no earlier step wrote: a recipe never copies a uuid.
    ObjectFromOutside {
        /// The step's name.
        step: String,
        /// The object's address.
        uri: String,
    },
    /// An input the template declares and no slot of the operation takes.
    ParameterNotASlot {
        /// The step's name.
        step: String,
        /// The parameter's title.
        title: String,
    },
}

impl std::fmt::Display for ReplayRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReplayRefusal::NotAWorkflow => {
                write!(
                    f,
                    "the root's template declares no subActivity: it is not a workflow"
                )
            }
            ReplayRefusal::OperationUnknown { step, template } => write!(
                f,
                "step `{step}`: its template {template} is no canonical template, or no operation records under it"
            ),
            ReplayRefusal::OperationAmbiguous {
                step,
                template,
                operations,
            } => write!(
                f,
                "step `{step}`: several operations record under the template {template}: {operations:?}"
            ),
            ReplayRefusal::ChildMissing { step } => write!(
                f,
                "step `{step}` is declared, not held, and no activity of the root plays it"
            ),
            ReplayRefusal::InputFileNotRecorded { step } => write!(
                f,
                "step `{step}`: the operation takes an input, and its activity records neither an object nor a file"
            ),
            ReplayRefusal::ObjectFromOutside { step, uri } => write!(
                f,
                "step `{step}`: {uri} was not written by an earlier step, and a recipe never copies a uuid"
            ),
            ReplayRefusal::ParameterNotASlot { step, title } => write!(
                f,
                "step `{step}`: `{title}` is an input the template declares and no slot of the operation takes"
            ),
        }
    }
}

impl std::error::Error for ReplayRefusal {}

/// A recipe drafted from a workflow's activities, and what it left out.
///
/// **With loss, said so**: it finds a lost recipe or compares two, it never
/// replaces the versioned recipe (see the module).
#[derive(Debug, Clone)]
pub struct RecipeDraft {
    /// The drafted recipe.
    pub recipe: Recipe,
    /// One sentence per parameter left out (a count, a title no slot or
    /// template names).
    pub left_out: Vec<String>,
}

impl RecipeDraft {
    /// The TOML text; [`Recipe::parse`] reads it back.
    pub fn to_toml(&self) -> Result<String, String> {
        toml::to_string(&self.recipe).map_err(|why| why.to_string())
    }
}

/// An object an earlier step wrote, as a later activity can name it.
struct Written {
    step: String,
    address: String,
}

/// Whether two addresses name one object: same uuid, versions that agree
/// (the library's one reading of an address).
fn same_object(a: &str, b: &str) -> bool {
    let (uuid_a, version_a) = energyml_core::identity::parse_address(a);
    let (uuid_b, version_b) = energyml_core::identity::parse_address(b);
    uuid_a == uuid_b && energyml_core::identity::versions_agree(version_a.as_deref(), version_b.as_deref())
}

/// A reference as the JSON `exemplar` reads (`DataObject`).
fn reference_json(reference: &DeclaredReference) -> Value {
    let mut out = serde_json::Map::new();
    out.insert("Uuid".to_string(), Value::from(reference.uuid.clone()));
    if let Some(kind) = &reference.qualified_type {
        out.insert("QualifiedType".to_string(), Value::from(kind.clone()));
    }
    if let Some(title) = &reference.title {
        out.insert("Title".to_string(), Value::from(title.clone()));
    }
    if let Some(version) = &reference.version {
        out.insert("ObjectVersion".to_string(), Value::from(version.clone()));
    }
    Value::Object(out)
}

/// The canonical recipe whose template has this uuid: a key of
/// `data/activityTemplates.json`, and the template as a record.
fn canonical_by_uuid(uuid: &str) -> Option<(String, TemplateRecord)> {
    let all = energyml_domain::provenance::templates().as_object()?;
    all.iter()
        .filter(|(key, _)| !key.starts_with("__"))
        .find(|(_, template)| {
            template["uuid"]
                .as_str()
                .is_some_and(|held| held.eq_ignore_ascii_case(uuid))
        })
        .map(|(recipe, template)| (recipe.clone(), canonical_record(template)))
}

/// A canonical template as a [`TemplateRecord`], read from the crate's own
/// JSON (an object nobody wrote into a package: there is nothing to read back
/// through the reflection, and `energyml-schemas` is not a dependency here).
fn canonical_record(template: &Value) -> TemplateRecord {
    let parameters = template["Parameter"]
        .as_array()
        .map(|declared| {
            declared
                .iter()
                .enumerate()
                .map(|(position, one)| ParameterTemplateRecord {
                    position,
                    title: one["Title"].as_str().unwrap_or_default().to_string(),
                    allowed: one["AllowedKind"]
                        .as_array()
                        .map(|kinds| {
                            kinds
                                .iter()
                                .filter_map(Value::as_str)
                                .map(ParameterKind::from_word)
                                .collect()
                        })
                        .unwrap_or_default(),
                    direction: match (
                        one["IsInput"].as_bool().unwrap_or(false),
                        one["IsOutput"].as_bool().unwrap_or(false),
                    ) {
                        (true, false) => ParameterDirection::Input,
                        (false, true) => ParameterDirection::Output,
                        (true, true) => ParameterDirection::Both,
                        (false, false) => ParameterDirection::Neither,
                    },
                    min_occurs: one["MinOccurs"].as_i64(),
                    max_occurs: one["MaxOccurs"].as_i64(),
                    constraint: one["Constraint"].as_str().map(str::to_string),
                    content_type: one["DataObjectContentType"].as_str().map(str::to_string),
                    key_constraints: Vec::new(),
                    defaults: Vec::new(),
                })
                .collect()
        })
        .unwrap_or_default();
    TemplateRecord {
        uri: format!(
            "eml:///eml23.ActivityTemplate({})",
            template["uuid"].as_str().unwrap_or_default()
        ),
        title: template["Citation"]["Title"].as_str().map(str::to_string),
        parameters,
    }
}

/// The name of the one operation that records under `recipe`, else the recipe's
/// own name (a held step has no activity to say more).
fn operation_named_by(operations: &Registry, recipe: &str) -> String {
    let mut names = operations
        .iter()
        .filter(|operation| operation.recipe() == recipe)
        .map(|operation| operation.name());
    match (names.next(), names.next()) {
        (Some(only), None) => only.to_string(),
        _ => recipe.to_string(),
    }
}

/// Draft a recipe from a workflow's root activity, its template and the
/// activities of its steps (08 section 5.7, B).
///
/// `children` are the activities of the root's steps, as the caller read them;
/// `operations` finds each step's operation and its slots; `index` recognises
/// the members of a collection an earlier step wrote (past 8 objects of a role
/// the activity names only the collection).
///
/// Every refusal is collected, none stops the others; at least one is an `Err`.
/// What is left out (a count, a measure no slot takes) is in
/// [`RecipeDraft::left_out`].
pub fn draft_from_workflow(
    root: &ActivityRecord,
    template: &TemplateRecord,
    children: &[ActivityRecord],
    operations: &Registry,
    index: &PackageIndex,
) -> Result<RecipeDraft, Vec<ReplayRefusal>> {
    let declared = energyml_domain::provenance_graph::workflow_steps(template, &RecipeConstraints);
    if declared.is_empty() {
        return Err(vec![ReplayRefusal::NotAWorkflow]);
    }
    // The root names each step's activity by a parameter titled after the step.
    let named: Vec<(&str, &ActivityRecord)> = root
        .parameters
        .iter()
        .filter_map(|parameter| match &parameter.value {
            ParameterValue::Object(reference) => children
                .iter()
                .find(|child| same_object(&reference.address, &child.uri))
                .map(|child| (parameter.title.as_str(), child)),
            _ => None,
        })
        .collect();

    let mut refusals: Vec<ReplayRefusal> = Vec::new();
    let mut left_out: Vec<String> = Vec::new();
    let mut steps: Vec<Step> = Vec::new();
    let mut written: Vec<Written> = Vec::new();
    for step in &declared {
        let serves = step
            .parsed
            .as_ref()
            .map(|parsed| parsed.serves.clone())
            .unwrap_or_default();
        if let Some(reason) = step.held() {
            let operation = step
                .parsed
                .as_ref()
                .map(|parsed| operation_named_by(operations, &parsed.operation))
                .unwrap_or_default();
            steps.push(Step {
                name: step.title.clone(),
                operation,
                set: BTreeMap::new(),
                inputs: Vec::new(),
                template: None,
                serves,
                held: Some(reason.to_string()),
                group: None,
            });
            continue;
        }
        let Some(child) = named
            .iter()
            .find(|(title, _)| *title == step.title)
            .map(|(_, child)| *child)
        else {
            refusals.push(ReplayRefusal::ChildMissing {
                step: step.title.clone(),
            });
            continue;
        };
        let (drafted, outputs) = draft_step(
            &step.title,
            serves,
            child,
            operations,
            index,
            &written,
            &mut refusals,
            &mut left_out,
        );
        steps.extend(drafted);
        written.extend(outputs);
    }
    if !refusals.is_empty() {
        return Err(refusals);
    }
    Ok(RecipeDraft {
        recipe: Recipe {
            title: root
                .title
                .clone()
                .or_else(|| template.title.clone())
                .unwrap_or_default(),
            creation: root.creation.clone().unwrap_or_default(),
            originator: root.originator.clone().unwrap_or_default(),
            data_file: data_h5(),
            workflow: template.title.clone().unwrap_or_default(),
            activity: None,
            description: None,
            steps,
        },
        left_out,
    })
}

/// One played step, drafted: its [`Step`] (when nothing refused) and the
/// objects its activity says it wrote.
#[allow(clippy::too_many_arguments)]
fn draft_step(
    name: &str,
    serves: Vec<String>,
    child: &ActivityRecord,
    operations: &Registry,
    index: &PackageIndex,
    written: &[Written],
    refusals: &mut Vec<ReplayRefusal>,
    left_out: &mut Vec<String>,
) -> (Option<Step>, Vec<Written>) {
    let step = name.to_string();
    // The template the activity names, and the operation that records under it.
    let descriptor = child
        .descriptor
        .as_ref()
        .map(|descriptor| descriptor.address.clone())
        .unwrap_or_default();
    let uuid = energyml_core::identity::parse_address(&descriptor).0;
    let Some((recipe, record)) = canonical_by_uuid(&uuid) else {
        refusals.push(ReplayRefusal::OperationUnknown {
            step,
            template: descriptor,
        });
        return (None, Vec::new());
    };
    let mut candidates: Vec<&dyn AnyOperation> = operations
        .iter()
        .filter(|operation| operation.recipe() == recipe)
        .collect();
    let operation = match candidates.len() {
        1 => candidates.remove(0),
        0 => {
            refusals.push(ReplayRefusal::OperationUnknown {
                step,
                template: descriptor,
            });
            return (None, Vec::new());
        }
        _ => {
            let mut names: Vec<String> = candidates.iter().map(|one| one.name().to_string()).collect();
            names.sort();
            refusals.push(ReplayRefusal::OperationAmbiguous {
                step,
                template: descriptor,
                operations: names,
            });
            return (None, Vec::new());
        }
    };
    let roles = energyml_domain::activity::roles(child, Some(&record), &RoleRules::new());
    let role_of = |position: usize| roles.iter().find(|one| one.position == position);
    let slots = operation.needs(&Pieces::empty());
    let reports = operation.reports();

    // What the activity says it wrote: the objects of its output parameters,
    // and the members of a collection among them.
    let mut outputs: Vec<Written> = Vec::new();
    for parameter in &child.parameters {
        let ParameterValue::Object(reference) = &parameter.value else {
            continue;
        };
        if !matches!(
            role_of(parameter.position).map(|one| one.role),
            Some(ActivityRole::Output | ActivityRole::Both)
        ) {
            continue;
        }
        outputs.push(Written {
            step: step.clone(),
            address: reference.address.clone(),
        });
        if index
            .node(&reference.address)
            .is_some_and(|node| &*node.type_key == "DataobjectCollection")
            && let Ok(members) = index.expand_collection(&reference.address, usize::MAX)
        {
            outputs.extend(members.into_iter().map(|address| Written {
                step: step.clone(),
                address,
            }));
        }
    }

    let before = refusals.len();
    let mut inputs: Vec<Entry> = Vec::new();
    let mut parameters: Vec<Value> = Vec::new();
    let mut references: Vec<(&str, &DeclaredReference)> = Vec::new();
    let mut titles: Vec<&str> = Vec::new();
    for parameter in &child.parameters {
        let title = parameter.title.as_str();
        let role = role_of(parameter.position);
        if title == "output" {
            continue;
        }
        if title == "inputFile" {
            if let ParameterValue::Text(value) = &parameter.value
                && let Some(file) = read_input_file(value)
            {
                inputs.push(Entry {
                    slot: file.slot,
                    file: Some(file.path),
                    step: None,
                    qualified_type: None,
                    title: None,
                    role: None,
                });
            }
            continue;
        }
        let is_slot = slots.iter().any(|slot| slot.name == title);
        match &parameter.value {
            ParameterValue::Object(reference) => {
                if is_slot {
                    parameters
                        .push(serde_json::json!({ "Title": title, "DataObject": reference_json(reference) }));
                    references.push((title, reference));
                    titles.push(title);
                    continue;
                }
                if !matches!(
                    role.map(|one| one.role),
                    Some(ActivityRole::Input | ActivityRole::Both)
                ) {
                    continue;
                }
                match written
                    .iter()
                    .find(|one| same_object(&one.address, &reference.address))
                {
                    Some(found) => {
                        let already = inputs.iter().any(|entry| {
                            entry.step.as_deref() == Some(found.step.as_str())
                                && entry.qualified_type == reference.qualified_type
                                && entry.title == reference.title
                        });
                        if !already {
                            inputs.push(Entry {
                                slot: None,
                                file: None,
                                step: Some(found.step.clone()),
                                qualified_type: reference.qualified_type.clone(),
                                title: reference.title.clone(),
                                role: None,
                            });
                        }
                    }
                    None => refusals.push(ReplayRefusal::ObjectFromOutside {
                        step: step.clone(),
                        uri: reference.address.clone(),
                    }),
                }
            }
            ParameterValue::Double { .. } | ParameterValue::Integer { .. } | ParameterValue::Text(_) => {
                if is_slot {
                    let value = match &parameter.value {
                        ParameterValue::Double { value, .. } => Value::from(*value),
                        ParameterValue::Integer { value } => Value::from(*value),
                        ParameterValue::Text(value) => Value::from(value.clone()),
                        _ => continue,
                    };
                    parameters.push(serde_json::json!({ "Title": title, "Value": value }));
                    titles.push(title);
                } else if matches!(
                    role.map(|one| (one.role, one.basis)),
                    Some((ActivityRole::Input | ActivityRole::Both, RoleBasis::Template))
                ) {
                    refusals.push(ReplayRefusal::ParameterNotASlot {
                        step: step.clone(),
                        title: title.to_string(),
                    });
                } else if reports.iter().any(|slot| slot.name == title) {
                    left_out.push(format!(
                        "step `{name}`: `{title}` is measured by the operation, it is not a setting"
                    ));
                } else {
                    left_out.push(format!(
                        "step `{name}`: `{title}` is named by no slot of the operation and by no direction of its template"
                    ));
                }
            }
            ParameterValue::TimeIndex { .. } | ParameterValue::Unreadable { .. } => {
                left_out.push(format!(
                    "step `{name}`: `{title}` is not a value a recipe can state"
                ));
            }
        }
    }

    // The settings: read back the way a form replays an activity.
    let mut set: BTreeMap<String, Value> = BTreeMap::new();
    let copied = crate::exemplar::from_activity(&slots, &serde_json::json!({ "Parameter": parameters }));
    for one in &copied {
        if let Some((_, reference)) = references.iter().find(|(title, _)| *title == one.slot) {
            match written
                .iter()
                .find(|held| same_object(&held.address, &reference.address))
            {
                Some(found) => {
                    let mut pick = serde_json::Map::new();
                    pick.insert("etape".to_string(), Value::from(found.step.clone()));
                    if let Some(kind) = &reference.qualified_type {
                        pick.insert("type".to_string(), Value::from(kind.clone()));
                    }
                    if let Some(title) = &reference.title {
                        pick.insert("titre".to_string(), Value::from(title.clone()));
                    }
                    set.insert(one.slot.clone(), Value::Object(pick));
                }
                None => refusals.push(ReplayRefusal::ObjectFromOutside {
                    step: step.clone(),
                    uri: reference.address.clone(),
                }),
            }
        } else if !one.value.is_null() {
            set.insert(one.slot.clone(), one.value.clone());
        }
    }
    for title in &titles {
        if !copied.iter().any(|one| one.slot == *title) {
            left_out.push(format!(
                "step `{name}`: the value of `{title}` does not read back as a setting of the operation"
            ));
        }
    }

    if operation.takes().iter().any(|slot| slot.least >= 1) && inputs.is_empty() {
        refusals.push(ReplayRefusal::InputFileNotRecorded { step: step.clone() });
    }
    if refusals.len() > before {
        return (None, outputs);
    }
    (
        Some(Step {
            name: step,
            operation: operation.name().to_string(),
            set,
            inputs,
            template: None,
            serves,
            held: None,
            group: None,
        }),
        outputs,
    )
}
