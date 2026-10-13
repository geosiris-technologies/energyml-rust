//! `edit-object` - an object of the package with fields set: rewritten under
//! its own uuid, or copied under a new one.
//!
//! What the front end's object editor does, made scriptable:
//! the object arrives whole, in the workspace's JSON encoding
//! ([`crate::pieces::Known::json`], filled at the boundary by `read`), each
//! field named is set to the value given, and the result is written. Nothing
//! else of the object moves.
//!
//! # Why an operation for this
//!
//! Every other operation writes what it **decides**, and refuses what its
//! specification forbids - `fault-from-sticks` refuses `IsListric` on a fault
//! that is not normal, `collection` computes `HomogeneousDatatype` from the
//! members. That is right, and it leaves one thing no operation can do: state
//! a field **as told**. A recipe needs it twice - to correct an object after
//! the step that wrote it, and to write the deliberate anomalies a check must
//! find (the deliberate-anomalies model: objects that are
//! deliberately wrong, each against a written rule, to prove that the
//! check guarding that rule fires). Weakening a writer's refusal with a
//! setting would put the anomaly in the writer; here it is one line of the
//! recipe, naming the field and the value, and the writer stays strict.
//!
//! # What is written, and what is refused
//!
//! - **a path is the workspace's JSON encoding**: element names as the JSON
//!   writes them (`Citation.Description`, `ThrowInterpretation[0].Throw`), `[n]` for
//!   the n-th of a list. Every step but the last must exist; the last may add
//!   a field the object does not hold, and `null` removes one. A path that
//!   leads nowhere is refused by name, never created on the way - a typo would
//!   otherwise write a field no reader looks at;
//! - **`uuid` and `$type` are refused**: the identity is what `copy` changes,
//!   and a type is another object;
//! - **a value is JSON**, and text that is not is refused rather than taken as
//!   a string - `"reverse"` and `reverse` would otherwise be two spellings, one
//!   of which a number would silently change meaning under;
//! - **the object is not validated here**: a plan is pure and holds no schema.
//!   What does not read back as its type is refused by `apply`, which names the
//!   object; each change is **said** in the plan, old value and new, so the
//!   edit is never silent;
//! - **a reference keeps what it is**: `reference` set at `referenceAt`
//!   replaces the `Uuid`, `QualifiedType` and `Title` of the reference there -
//!   a `ContactElement` stays one, with its qualifier -, and only a place that
//!   holds nothing receives the `DataObjectReference` as given;
//! - **the citation says it was edited**: `LastUpdate` and `Editor` in place;
//!   a copy gets its own title, `Creation` and `Originator`. **A title is not
//!   rewritten in place**: every reference to the object holds a copy of it,
//!   and `check` calls each copy that disagrees stale - three warnings, the
//!   first time this ran on Lutetia's `[ERROR]` table.
//!
//! # Identity
//!
//! In place, the uuid is the object's - which is the point: what references it
//! keeps doing so. A copy's uuid comes from the object copied and the copy's
//! title: the same copy is the same object from one run to the next.

use serde_json::{Value, json};

use energyml_domain::provenance::Parameter;

use crate::declare::{Inputs, ObjectIn, ObjectRef, Params};
use crate::operation::{About, ActivityDraft, Category, Context, NewObject, Note, Operation, Plan, Refusal};

/// The operation.
pub struct EditObject;

/// What `edit-object` is told.
#[derive(Debug, Params)]
pub struct EditObjectParams {
    /// the fields to set, each `path = value` - the path in the workspace's JSON encoding (`Citation.Description`, `Key[0]._`), the value in JSON (`"reverse"`, `true`, `null` to remove)
    pub fields: Vec<String>,
    /// the title of a copy - given, the object is copied under a new uuid with this title; left out, it is rewritten under its own
    pub copy: Option<String>,
    /// an object of the package whose reference is set at `referenceAt`
    pub reference: Option<ObjectRef>,
    /// where the reference to `reference` goes - a path, as in `fields`
    pub reference_at: Option<String>,
}

/// What `edit-object` takes.
#[derive(Debug, Inputs)]
pub struct EditObjectInputs {
    /// the object to edit - of any type
    pub object: ObjectIn,
}

/// One step of a path: a field, and the rank in it when it is a list.
#[derive(Debug, Clone, PartialEq)]
struct Step {
    name: String,
    index: Option<usize>,
}

/// `A.B[2].C` - refused by name when a step is empty or an index is no number.
fn parse_path(path: &str) -> Result<Vec<Step>, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("the path is empty".to_string());
    }
    let mut steps = Vec::new();
    for part in path.split('.') {
        let (name, index) = match part.split_once('[') {
            None => (part, None),
            Some((name, rest)) => {
                let inside = rest
                    .strip_suffix(']')
                    .ok_or_else(|| format!("`{part}` opens a `[` it does not close"))?;
                let index = inside
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| format!("`{part}`: `{inside}` is no rank - a list is indexed from 0"))?;
                (name, Some(index))
            }
        };
        let name = name.trim();
        if name.is_empty() {
            return Err(format!("`{path}` has an empty step"));
        }
        if name.contains(['[', ']', ' ']) {
            return Err(format!("`{name}` is no element name"));
        }
        steps.push(Step {
            name: name.to_string(),
            index,
        });
    }
    if let [Step { name, index: None }] = steps.as_slice()
        && (name == "uuid" || name == "$type")
    {
        return Err(format!(
            "`{name}` is not a field to set: the identity changes by `copy`, and another type is another object"
        ));
    }
    Ok(steps)
}

/// `path = value`, split on the first ` = ` and the value read as JSON.
fn parse_field(text: &str) -> Result<(String, Vec<Step>, Value), String> {
    let (path, value) = text
        .split_once('=')
        .ok_or_else(|| format!("`{text}` is not `path = value`"))?;
    let steps = parse_path(path)?;
    let value = value.trim();
    let value: Value = serde_json::from_str(value).map_err(|why| {
        format!(
            "`{value}` is not JSON ({why}) - a text is quoted: `{} = \"{value}\"`",
            path.trim()
        )
    })?;
    Ok((path.trim().to_string(), steps, value))
}

/// A value as a note shows it - short.
fn shown(value: Option<&Value>) -> String {
    match value {
        None => "absent".to_string(),
        Some(value) => {
            let text = value.to_string();
            if text.chars().count() > 120 {
                format!("{}…", text.chars().take(120).collect::<String>())
            } else {
                text
            }
        }
    }
}

/// Set `value` at `steps` in `object`, and answer what was there. `null`
/// removes a field.
fn set(object: &mut Value, path: &str, steps: &[Step], value: Value) -> Result<Option<Value>, String> {
    let (last, before) = steps
        .split_last()
        .ok_or_else(|| "the path is empty".to_string())?;
    let mut here = object;
    let mut walked = String::new();
    for step in before {
        here = descend(here, step, &mut walked, path)?;
    }
    let map = here
        .as_object_mut()
        .ok_or_else(|| format!("`{path}`: `{walked}` holds no fields"))?;
    match last.index {
        None => {
            let old = map.get(&last.name).cloned();
            if value.is_null() {
                map.remove(&last.name);
            } else {
                map.insert(last.name.clone(), value);
            }
            Ok(old)
        }
        Some(at) => {
            let list = map
                .get_mut(&last.name)
                .ok_or_else(|| format!("`{path}`: `{}` is not in the object - a rank can only be set in a list that is there", last.name))?
                .as_array_mut()
                .ok_or_else(|| format!("`{path}`: `{}` is no list", last.name))?;
            let count = list.len();
            let slot = list.get_mut(at).ok_or_else(|| {
                format!(
                    "`{path}`: `{}` holds {count} item(s), and {at} is not one of them",
                    last.name
                )
            })?;
            if value.is_null() {
                return Err(format!(
                    "`{path}`: an item of a list is not removed by `null` - set the whole list"
                ));
            }
            Ok(Some(std::mem::replace(slot, value)))
        }
    }
}

/// One step down, refused by name when it leads nowhere.
fn descend<'a>(
    here: &'a mut Value,
    step: &Step,
    walked: &mut String,
    path: &str,
) -> Result<&'a mut Value, String> {
    let at = if walked.is_empty() {
        "the object".to_string()
    } else {
        format!("`{walked}`")
    };
    let field = here
        .as_object_mut()
        .ok_or_else(|| format!("`{path}`: {at} holds no fields"))?
        .get_mut(&step.name)
        .ok_or_else(|| format!("`{path}`: {at} has no `{}`", step.name))?;
    if !walked.is_empty() {
        walked.push('.');
    }
    walked.push_str(&step.name);
    match step.index {
        None => Ok(field),
        Some(rank) => {
            let list = field
                .as_array_mut()
                .ok_or_else(|| format!("`{path}`: `{walked}` is no list"))?;
            let count = list.len();
            walked.push_str(&format!("[{rank}]"));
            list.get_mut(rank).ok_or_else(|| {
                format!(
                    "`{path}`: `{}` holds {count} item(s), and {rank} is not one of them",
                    step.name
                )
            })
        }
    }
}

impl Operation for EditObject {
    type Params = EditObjectParams;
    type Inputs = EditObjectInputs;

    fn about(&self) -> About {
        About {
            name: "edit-object",
            describe: "an object of the package with fields set as told - rewritten under its own uuid, or copied under a new one",
            category: Category::Create,
            // Any type: the object's own.
            makes: &[],
            recipe: "edit-object",
            represents: None,
        }
    }

    fn plan(
        &self,
        inputs: EditObjectInputs,
        params: EditObjectParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let object = &inputs.object;
        let reference = object.known.reference.as_ref().ok_or_else(|| {
            Refusal::on(
                "object",
                format!("`{}` was given without a reference", object.name),
            )
        })?;
        let mut json = object.known.json.clone().ok_or_else(|| {
            Refusal::on(
                "object",
                format!(
                    "`{}` arrived without its content - an object is edited whole, and only the package's reader hands it over",
                    reference.title
                ),
            )
        })?;

        // -- what is set, read before anything is touched ----------------------
        let mut edits = Vec::new();
        let mut references = Vec::new();
        for text in &params.fields {
            edits.push(parse_field(text).map_err(|why| Refusal::on("fields", why))?);
        }
        match (&params.reference, &params.reference_at) {
            (Some(target), Some(at)) => {
                let steps = parse_path(at).map_err(|why| Refusal::on("referenceAt", why))?;
                if !target.0.is_object() {
                    return Err(Refusal::on(
                        "reference",
                        format!(
                            "expected a DataObjectReference, got {} - a bare uuid names no type",
                            target.0
                        ),
                    ));
                }
                references.push((at.trim().to_string(), steps, target.0.clone()));
            }
            (Some(_), None) => {
                return Err(Refusal::on(
                    "referenceAt",
                    "`reference` is given, and nothing says where it goes",
                ));
            }
            (None, Some(_)) => {
                return Err(Refusal::on(
                    "reference",
                    "`referenceAt` says where a reference goes, and none is given",
                ));
            }
            (None, None) => {}
        }
        if edits.is_empty() && references.is_empty() {
            return Err(Refusal::on(
                "fields",
                "nothing to set: an edit that changes nothing would rewrite the object as it is",
            ));
        }
        let copy = params
            .copy
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty());
        if edits.iter().any(|(path, _, _)| path == "Citation.Title") {
            return Err(Refusal::on(
                "fields",
                match copy {
                    Some(_) => "`copy` titles the copy, and `fields` sets `Citation.Title` too: say it once"
                        .to_string(),
                    None => format!(
                        "`Citation.Title` is not rewritten in place: every reference to `{}` holds a copy of its title, and each would go stale - copy it under the new title instead",
                        reference.title
                    ),
                },
            ));
        }
        let paths: Vec<&str> = edits
            .iter()
            .chain(&references)
            .map(|(path, _, _)| path.as_str())
            .collect();
        for (at, path) in paths.iter().enumerate() {
            if paths[..at].contains(path) {
                return Err(Refusal::on("fields", format!("`{path}` is set twice")));
            }
        }

        // -- the edits ------------------------------------------------------------
        let mut says = Vec::new();
        for (path, steps, value) in edits {
            let new = shown(Some(&value).filter(|value| !value.is_null()));
            let old = set(&mut json, &path, &steps, value).map_err(|why| Refusal::on("fields", why))?;
            says.push(Note {
                key: "set",
                detail: format!("`{path}`: {} → {new}", shown(old.as_ref())),
            });
        }
        // A reference replaces what the one there names and keeps what it is:
        // a `ContactElement` stays one, with its qualifier, and only a place
        // that holds nothing receives the `DataObjectReference` as given.
        for (path, steps, target) in references {
            let old = set(&mut json, &path, &steps, target.clone())
                .map_err(|why| Refusal::on("referenceAt", why))?;
            if let Some(Value::Object(mut kept)) = old.clone() {
                for key in ["Uuid", "QualifiedType", "Title"] {
                    match target.get(key) {
                        Some(value) => kept.insert(key.to_string(), value.clone()),
                        None => kept.remove(key),
                    };
                }
                kept.remove("ObjectVersion");
                set(&mut json, &path, &steps, Value::Object(kept))
                    .map_err(|why| Refusal::on("referenceAt", why))?;
            }
            says.push(Note {
                key: "reference",
                detail: format!(
                    "`{path}`: {} → `{}` ({})",
                    old.as_ref()
                        .and_then(|old| old["Title"].as_str())
                        .map(|title| format!("`{title}`"))
                        .unwrap_or_else(|| shown(old.as_ref())),
                    target["Title"].as_str().unwrap_or_default(),
                    target["QualifiedType"].as_str().unwrap_or_default()
                ),
            });
        }

        // -- identity and citation -------------------------------------------------
        let (uuid, title, role) = match copy {
            Some(title) => {
                let uuid = cx
                    .identity
                    .uuid(&format!("edit-object:copy:{}:{title}", reference.uuid), 0)?;
                json["uuid"] = json!(uuid);
                let citation = json
                    .get_mut("Citation")
                    .and_then(Value::as_object_mut)
                    .ok_or_else(|| {
                        Refusal::on(
                            "copy",
                            format!("`{}` has no Citation to title a copy in", reference.title),
                        )
                    })?;
                citation.insert("Title".to_string(), json!(title));
                citation.insert("Creation".to_string(), json!(cx.when.creation));
                citation.insert("Originator".to_string(), json!(cx.when.originator));
                citation.insert("Format".to_string(), json!(energyml_core::CITATION_FORMAT));
                citation.remove("LastUpdate");
                citation.remove("Editor");
                says.push(Note {
                    key: "copy",
                    detail: format!(
                        "a copy of `{}` ({}), under a new uuid",
                        reference.title, reference.uuid
                    ),
                });
                (uuid, title.to_string(), "copy")
            }
            None => {
                if let Some(citation) = json.get_mut("Citation").and_then(Value::as_object_mut) {
                    citation.insert("LastUpdate".to_string(), json!(cx.when.creation));
                    citation.insert("Editor".to_string(), json!(cx.when.originator));
                    // Abstract.xsd l. 395: the software that "creates or edits".
                    citation.insert("Format".to_string(), json!(energyml_core::CITATION_FORMAT));
                }
                let title = json["Citation"]["Title"]
                    .as_str()
                    .unwrap_or(&reference.title)
                    .to_string();
                says.push(Note {
                    key: "in-place",
                    detail: format!("`{title}` rewritten under its own uuid: what references it still does"),
                });
                (reference.uuid.clone(), title, "edited")
            }
        };

        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            objects: vec![NewObject {
                uuid: uuid.clone(),
                qualified_type: reference.qualified_type.clone(),
                role: role.to_string(),
                title: title.clone(),
                json,
            }],
            datasets: Vec::new(),
            activity: ActivityDraft {
                recipe: "edit-object",
                title: format!("edit-object → {title}"),
                seed: format!(
                    "edit-object:{}:{uuid}:{}",
                    reference.uuid,
                    params.fields.join("|")
                ),
                parameters: vec![Parameter::Object {
                    title: "input".to_string(),
                    reference: reference.dor(),
                }],
            },
            says,
            dropped: Vec::new(),
            shapes: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_read_step_by_step_and_refused_by_name() {
        assert_eq!(
            parse_path("ThrowInterpretation[0].Throw").unwrap(),
            vec![
                Step {
                    name: "ThrowInterpretation".into(),
                    index: Some(0)
                },
                Step {
                    name: "Throw".into(),
                    index: None
                }
            ]
        );
        assert!(parse_path("uuid").unwrap_err().contains("identity"));
        assert!(parse_path("$type").is_err());
        assert!(parse_path("A..B").unwrap_err().contains("empty step"));
        assert!(parse_path("A[x]").unwrap_err().contains("no rank"));
        assert!(parse_path("A[0").unwrap_err().contains("does not close"));
    }

    #[test]
    fn a_value_is_json_and_text_that_is_not_is_refused() {
        let (_, _, value) = parse_field("Throw = \"reverse\"").unwrap();
        assert_eq!(value, json!("reverse"));
        let why = parse_field("Throw = reverse").unwrap_err();
        assert!(why.contains("not JSON") && why.contains("\"reverse\""), "{why}");
    }

    #[test]
    fn a_field_is_set_added_or_removed_and_a_missing_step_is_refused() {
        let mut object = json!({ "A": { "B": [ { "C": 1 } ] }, "D": true });
        let old = set(
            &mut object,
            "A.B[0].C",
            &parse_path("A.B[0].C").unwrap(),
            json!(2),
        )
        .unwrap();
        assert_eq!(old, Some(json!(1)));
        assert_eq!(object["A"]["B"][0]["C"], json!(2));
        // The last step may be new.
        assert_eq!(
            set(&mut object, "A.E", &parse_path("A.E").unwrap(), json!("x")).unwrap(),
            None
        );
        assert_eq!(object["A"]["E"], json!("x"));
        // `null` removes.
        set(&mut object, "D", &parse_path("D").unwrap(), Value::Null).unwrap();
        assert!(object.get("D").is_none());
        // A step on the way is never created.
        let why = set(&mut object, "X.Y", &parse_path("X.Y").unwrap(), json!(1)).unwrap_err();
        assert!(why.contains("has no `X`"), "{why}");
        let why = set(
            &mut object,
            "A.B[3].C",
            &parse_path("A.B[3].C").unwrap(),
            json!(1),
        )
        .unwrap_err();
        assert!(why.contains("holds 1 item(s)"), "{why}");
    }
}
