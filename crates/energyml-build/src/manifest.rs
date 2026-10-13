//! **The run manifest** — what a run wrote, by step and by role, as JSON.
//!
//! Chaining operations in a workflow requires that operations expose the
//! objects they created. The manifest is
//! written by the library from what [`crate::apply`] **wrote** — the same source
//! as the activity — and a face (the CLI's `--manifest`, a Galaxy tool) only
//! carries it.
//!
//! # What it says, and what it does not guess
//!
//! - one [`Step`] per operation played; roles in the order the plan first meets
//!   them, then the roles the operation declares ([`AnyOperation::gives`]) that
//!   the run did not write, with `count: 0`;
//! - `declared` is `false` for a role the plan wrote and `gives()` does not name
//!   — said, never silent; an operation whose `gives()` is empty has every role
//!   undeclared;
//! - `uris` are full uris ([`NewObject::uri`]), in the plan's order, **only the
//!   ones `apply` wrote** — an object refused is not in the manifest, and the
//!   step's `refused` counts the refusals;
//! - `collection` is the uri `apply` wrote for that role
//!   ([`Made::collections`]), `null` when it wrote none.

use serde::{Deserialize, Serialize};

use crate::apply::Made;
use crate::operation::{AnyOperation, Plan};

/// The `format` of a manifest — versioned, so that a reader can refuse one it
/// does not know.
pub const FORMAT: &str = "energyml-run-outputs/1";

/// What a run wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// Always [`FORMAT`].
    pub format: String,
    /// The files of the package.
    pub package: Package,
    /// The recipe's title, when a recipe was played.
    pub recipe: Option<String>,
    /// One per operation played, in order.
    pub steps: Vec<Step>,
}

/// The files a run wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Package {
    /// The EPC's file name.
    pub epc: String,
    /// The external data files the plans wrote arrays into, first met first.
    pub data_files: Vec<String>,
}

/// One operation played.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    /// The step's name — the recipe's, or the operation's for a `create`.
    pub name: String,
    /// The operation, as `energyml create --list` names it.
    pub operation: String,
    /// Its activity's uri, when it was written.
    pub activity: Option<String>,
    /// What it wrote, by role.
    pub outputs: Vec<Outputs>,
    /// How many things `apply` refused.
    pub refused: usize,
}

/// The objects of one role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Outputs {
    /// `NewObject::role`.
    pub role: String,
    /// Does the operation's `gives()` name this role?
    pub declared: bool,
    /// What the role is, in one line — the declaration's; empty when undeclared.
    pub describe: String,
    /// The qualified types: the declared ones, or the ones written when the role
    /// is not declared.
    pub types: Vec<String>,
    /// How many uris — what `apply` wrote.
    pub count: usize,
    /// The `DataobjectCollection` `apply` wrote for the role, if any.
    pub collection: Option<String>,
    /// The full uris, in the plan's order.
    pub uris: Vec<String>,
}

impl Manifest {
    /// A manifest of `steps`.
    pub fn new(
        epc: impl Into<String>,
        data_files: Vec<String>,
        recipe: Option<String>,
        steps: Vec<Step>,
    ) -> Manifest {
        Manifest {
            format: FORMAT.to_string(),
            package: Package {
                epc: epc.into(),
                data_files,
            },
            recipe,
            steps,
        }
    }

    /// The JSON, pretty-printed.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a manifest is plain data")
    }
}

/// The external files a plan's arrays go into, first met first.
pub fn data_files(plan: &Plan) -> Vec<String> {
    let mut files: Vec<String> = Vec::new();
    for dataset in &plan.datasets {
        if !files.contains(&dataset.file) {
            files.push(dataset.file.clone());
        }
    }
    files
}

/// The step an operation's run makes: its `plan`, and what `apply` returned for it.
pub fn step(name: &str, operation: &dyn AnyOperation, plan: &Plan, made: &Made) -> Step {
    let declared = operation.gives();
    let mut outputs: Vec<Outputs> = Vec::new();
    // The plan's roles, in the order it first meets them.
    for object in &plan.objects {
        let at = match outputs.iter().position(|one| one.role == object.role) {
            Some(at) => at,
            None => {
                let named = declared.iter().find(|one| one.role == object.role);
                outputs.push(Outputs {
                    role: object.role.clone(),
                    declared: named.is_some(),
                    describe: named.map(|one| one.describe.to_string()).unwrap_or_default(),
                    types: named
                        .map(|one| one.types.iter().map(|one| one.to_string()).collect())
                        .unwrap_or_default(),
                    count: 0,
                    collection: made
                        .collections
                        .iter()
                        .find(|(role, _)| *role == object.role)
                        .map(|(_, uri)| uri.clone()),
                    uris: Vec::new(),
                });
                outputs.len() - 1
            }
        };
        let one = &mut outputs[at];
        let uri = object.uri();
        // Written, not planned: an object `apply` refused is not an output.
        if made.written.contains(&uri) {
            one.uris.push(uri);
            one.count += 1;
        }
        if !one.declared && !one.types.contains(&object.qualified_type) {
            one.types.push(object.qualified_type.clone());
        }
    }
    // The roles it declares and did not write.
    for named in &declared {
        if outputs.iter().all(|one| one.role != named.role) {
            outputs.push(Outputs {
                role: named.role.to_string(),
                declared: true,
                describe: named.describe.to_string(),
                types: named.types.iter().map(|one| one.to_string()).collect(),
                count: 0,
                collection: None,
                uris: Vec::new(),
            });
        }
    }
    Step {
        name: name.to_string(),
        operation: operation.name().to_string(),
        activity: made.activity.clone(),
        outputs,
        refused: made.refused.len(),
    }
}
