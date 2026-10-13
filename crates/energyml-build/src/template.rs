//! The form and the activity template are the same declaration.
//!
//! The ideal is one activity template per operation. There are canonical
//! templates in `rc/activityTemplates.json`,
//! written by hand next to the code that honours them — two places for one
//! fact. The rule here: **the [`Slot`] is the source, the `ParameterTemplate`
//! is a projection of it**, and a test replays the projection against the file
//! the way `generate --all --check` replays the generator against the committed
//! crates. The canonical uuid stays a constant of the file — never recomputed,
//! never changed.
//!
//! What EML 2.3 can say is most of a form: `AllowedKind`, `MinOccurs`,
//! `MaxOccurs`, `DataObjectContentType`. What it cannot — a closed list, a
//! bound, a unit — goes into `Constraint`, which is free text meant for
//! exactly that.

use serde_json::{Value, json};

use crate::operation::{AnyOperation, Need, Slot};
use crate::pieces::Pieces;

/// The `eml23.ParameterTemplate` a slot projects to.
pub fn parameter_template(slot: &Slot, is_input: bool) -> Value {
    let mut out = json!({
        "$type": "eml23.ParameterTemplate",
        "AllowedKind": [slot.need.parameter_kind()],
        "IsInput": is_input,
        "IsOutput": !is_input,
        "Title": slot.name,
        // A slot that acts only under a condition may be absent from a run
        // that did not meet it, whatever it requires when it does act.
        "MinOccurs": if slot.applies.is_conditional() { 0 } else { slot.least },
        "MaxOccurs": slot.most.map(i64::from).unwrap_or(-1),
    });
    if let Need::Object { types: [one] } = slot.need {
        out["DataObjectContentType"] = json!(one);
    }
    let mut constraint: Vec<String> = constraint_of(&slot.need).into_iter().collect();
    // The condition of a method's sub-parameter, said: a reader of the
    // template sees why `kernel` is missing from an IDW run.
    if let Some(words) = condition_words(&slot.applies) {
        constraint.push(format!("only when {words}"));
    }
    if !constraint.is_empty() {
        out["Constraint"] = json!(constraint.join("; "));
    }
    out
}

/// A `When` or an `Any` of them, in the template's plain words — no backticks.
fn condition_words(applies: &crate::operation::Applies) -> Option<String> {
    use crate::operation::Applies;
    match applies {
        Applies::When { slot, is } => Some(format!("{slot} is {}", is.join(" or "))),
        Applies::Any(all) => Some(
            all.iter()
                .filter_map(condition_words)
                .collect::<Vec<_>>()
                .join(", or "),
        ),
        _ => None,
    }
}

/// What EML cannot type, said in words.
fn constraint_of(need: &Need) -> Option<String> {
    match need {
        Need::Choice(allowed) => Some(format!("one of: {}", allowed.join(", "))),
        Need::Number { uom, least, most } => {
            let mut parts = Vec::new();
            if let Some(uom) = uom {
                parts.push(format!("uom {uom}"));
            }
            if least.is_some() || most.is_some() {
                parts.push(format!("in [{}, {}]", bound(*least), bound(*most)));
            }
            (!parts.is_empty()).then(|| parts.join(", "))
        }
        Need::Integer { least, most } if least.is_some() || most.is_some() => {
            Some(format!("in [{}, {}]", bound(*least), bound(*most)))
        }
        Need::Flag => Some("true or false".to_string()),
        Need::Epsg => Some("an EPSG code".to_string()),
        // What EML cannot type at all: a type name, and the two axes an
        // operation may pin. Said in words so that a reader of the template
        // knows what the form greys out and why.
        Need::Type(choice) => {
            let mut parts = vec![format!(
                "a qualified type, listed on `{}` by default",
                choice.filter
            )];
            if choice.package.is_fixed() {
                parts.push(format!("package {}", choice.package.named().join(" or ")));
            }
            if choice.version.is_fixed() {
                parts.push(format!("version {}", choice.version.named().join(" or ")));
            }
            Some(parts.join(", "))
        }
        Need::Crs => Some("a local CRS".to_string()),
        Need::Interpretation => Some("an interpretation".to_string()),
        Need::PropertyKind => Some("a property kind".to_string()),
        Need::Object { types } if types.len() > 1 => Some(format!("one of: {}", types.join(", "))),
        _ => None,
    }
}

fn bound<T: std::fmt::Display>(bound: Option<T>) -> String {
    bound.map(|b| b.to_string()).unwrap_or_else(|| "∞".to_string())
}

/// The whole template an operation projects to, with the canonical uuid the
/// file already holds — or `?` when the file has none yet, which the check
/// below reports.
pub fn template_of(operation: &dyn AnyOperation) -> Value {
    let recipe = operation.recipe();
    let uuid = energyml_domain::provenance::template_uuid(recipe).unwrap_or("?");
    let mut parameters: Vec<Value> = operation
        .needs(&Pieces::empty())
        .iter()
        .map(|slot| parameter_template(slot, true))
        .collect();
    // What was measured while planning is an **output** of the activity, like
    // the objects: it did not exist before the run.
    parameters.extend(
        operation
            .reports()
            .iter()
            .map(|slot| parameter_template(slot, false)),
    );
    parameters.push(json!({
        "$type": "eml23.ParameterTemplate",
        "AllowedKind": ["dataObject"],
        "IsInput": false,
        "IsOutput": true,
        "Title": "output",
        "MinOccurs": 1,
        "MaxOccurs": -1,
    }));
    json!({
        "$type": "eml23.ActivityTemplate",
        "uuid": uuid,
        "schemaVersion": "2.3",
        "Citation": {
            "$type": "eml23.Citation",
            "Title": format!("fabricant:{recipe}"),
            "Originator": "energyml-rust",
            "Format": energyml_core::CITATION_FORMAT,
        },
        "Parameter": parameters,
    })
}

/// Where the canonical file and the projection disagree — empty when they
/// agree.
///
/// Compared by parameter title, on what the form is made of: the kind, the
/// direction, the occurrences. The file may carry an `input` parameter the
/// projection does not declare — the objects of the bag are recorded under it
/// by the writer — and nothing else the operation does not declare.
pub fn disagreements(operation: &dyn AnyOperation) -> Vec<String> {
    let recipe = operation.recipe();
    let mut out = Vec::new();
    let canonical = match energyml_domain::provenance::template(recipe) {
        Ok(value) => value,
        Err(why) => return vec![why],
    };
    let projected = template_of(operation);
    let by_title = |template: &Value| -> Vec<(String, Value)> {
        template["Parameter"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|p| p["Title"].as_str().map(|t| (t.to_string(), shape_of(p))))
                    .collect()
            })
            .unwrap_or_default()
    };
    let wanted = by_title(&projected);
    let held = by_title(canonical);
    for (title, shape) in &wanted {
        match held.iter().find(|(t, _)| t == title) {
            None => out.push(format!("`{recipe}`: the file lacks the parameter `{title}`")),
            Some((_, has)) if has != shape => out.push(format!(
                "`{recipe}`: `{title}` is {has} in the file and {shape} in the operation"
            )),
            Some(_) => {}
        }
    }
    for (title, _) in &held {
        if title != "input" && !wanted.iter().any(|(t, _)| t == title) {
            out.push(format!(
                "`{recipe}`: the file declares `{title}`, which the operation does not"
            ));
        }
    }
    out
}

/// The comparable part of a parameter template.
fn shape_of(parameter: &Value) -> Value {
    json!({
        "kind": parameter["AllowedKind"],
        "in": parameter["IsInput"],
        "least": parameter["MinOccurs"],
        "most": parameter["MaxOccurs"],
    })
}
