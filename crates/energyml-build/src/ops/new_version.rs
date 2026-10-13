//! `new-version` — an object of the package copied under another
//! `objectVersion`: the same uuid, the same type, the same content, **a second
//! entry** beside the first.
//!
//! Needed to test the identity of a package that
//! holds one object in two versions (ADR 0003):
//! a copy of an object with a different version.
//! And the rule it exercises: accept the short uri when there is no conflict (or when an object
//! has no version), and the form with the version when there is one
//! — so when the object copied
//! declares no version, it keeps the short uri and the copy takes the form with
//! its version; when it declares one, both take the form with theirs.
//!
//! # What it is not
//!
//! It is not `edit-object`. That one rewrites an object under its own identity
//! (a `put` that **replaces**) or copies it under a **new uuid**. This one keeps
//! the uuid and changes the third part of the identity, and it is written
//! through the one gate that adds a version — `EpcStore::put_as_new_version` — so
//! that an ordinary write can never make a second version by accident. The plan
//! says so with the role of the object ([`NEW_VERSION_ROLE`]); `apply` reads it.
//!
//! # What is refused, by name
//!
//! - an object arriving without its content (the package's reader hands it over);
//! - an empty `version`;
//! - a `version` the object already carries: the copy would be the object itself,
//!   and the store refuses the exact identity twice;
//! - an object whose JSON holds no place for a version — the refusal names the
//!   two spellings looked for, `objectVersion` (eml 2.1 and later) and
//!   `Citation.VersionString` (eml 2.0, where RESQML 2.0.1 keeps it).
//!
//! The copy keeps the object's **title** — every reference to the object holds a
//! copy of it, and a title that moved would make each of them stale — and its
//! creation; the citation says it was edited (`LastUpdate`, `Editor`).

use serde_json::{Value, json};

use energyml_domain::provenance::Parameter;

use crate::declare::{Inputs, ObjectIn, Params};
use crate::operation::{
    About, ActivityDraft, Category, Context, NEW_VERSION_ROLE, NewObject, Note, Operation, Plan, Refusal,
};

/// The operation.
pub struct NewVersion;

/// What `new-version` is told.
#[derive(Debug, Params)]
pub struct NewVersionParams {
    /// the `objectVersion` the copy carries — another than the object's own
    pub version: String,
}

/// What `new-version` takes.
#[derive(Debug, Inputs)]
pub struct NewVersionInputs {
    /// the object to copy beside itself, a second entry of its uuid — of any type
    // (Not worded with the release vocabulary: `no_unfitness_names_a_release`
    // reads every refusal for it, and an objectVersion is not a release.)
    pub object: ObjectIn,
}

/// Where an object keeps its version, in the workspace's JSON encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    /// `objectVersion`, an attribute of the root (eml 2.1 and later).
    Root,
    /// `Citation.VersionString` (eml 2.0).
    Citation,
}

/// Which spelling this object uses: the one it already has, or the one its
/// release uses. `None` when the JSON gives no way to tell.
fn place_of(json: &Value, qualified_type: &str) -> Option<Place> {
    if json.get("objectVersion").is_some() {
        return Some(Place::Root);
    }
    if json["Citation"].get("VersionString").is_some() {
        return Some(Place::Citation);
    }
    // Nothing written yet: the release decides. RESQML 2.0.1, WITSML 1.x and
    // eml 2.0 keep the version in the citation; the later releases on the root.
    let release = qualified_type.split('.').next().unwrap_or_default();
    let digits: String = release.chars().filter(char::is_ascii_digit).collect();
    match digits.as_str() {
        "20" if release.starts_with("resqml") || release.starts_with("eml") => Some(Place::Citation),
        "" => None,
        _ if json["Citation"].is_object() => Some(Place::Root),
        _ => None,
    }
}

impl Operation for NewVersion {
    type Params = NewVersionParams;
    type Inputs = NewVersionInputs;

    fn about(&self) -> About {
        About {
            name: "new-version",
            describe: "an object of the package copied under another objectVersion — the same uuid and type, a second entry beside the first",
            category: Category::Create,
            // Any type: the object's own.
            makes: &[],
            recipe: "new-version",
            represents: None,
        }
    }

    fn plan(
        &self,
        inputs: NewVersionInputs,
        params: NewVersionParams,
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
                    "`{}` arrived without its content — an object is copied whole, and only the package's reader hands it over",
                    reference.title
                ),
            )
        })?;

        let version = params.version.trim();
        if version.is_empty() {
            return Err(Refusal::on("version", "an empty objectVersion names no version"));
        }
        let place = place_of(&json, &reference.qualified_type).ok_or_else(|| {
            Refusal::on(
                "object",
                format!(
                    "`{}` ({}) has no place for a version this operation can find: `objectVersion` on the object, or `Citation.VersionString` in its citation",
                    reference.title, reference.qualified_type
                ),
            )
        })?;
        let held = match place {
            Place::Root => json.get("objectVersion"),
            Place::Citation => json["Citation"].get("VersionString"),
        }
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|held| !held.is_empty())
        .map(str::to_string);
        if held.as_deref() == Some(version) {
            return Err(Refusal::on(
                "version",
                format!(
                    "`{}` already carries the objectVersion `{version}`: a copy of it under that version would be the object itself",
                    reference.title
                ),
            ));
        }
        match place {
            Place::Root => json["objectVersion"] = json!(version),
            Place::Citation => {
                json["Citation"]
                    .as_object_mut()
                    .ok_or_else(|| {
                        Refusal::on(
                            "object",
                            format!("`{}` has no Citation to carry a VersionString", reference.title),
                        )
                    })?
                    .insert("VersionString".to_string(), json!(version));
            }
        }
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
        let says = vec![Note {
            key: "new-version",
            detail: format!(
                "`{title}` ({}) copied under the objectVersion `{version}`: {}",
                reference.uuid,
                match &held {
                    Some(held) => format!("beside the one that carries `{held}`, each named with its version"),
                    None => "the original declares none, so it keeps the short uri and the copy takes the form with its version".to_string(),
                }
            ),
        }];

        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            objects: vec![NewObject {
                uuid: reference.uuid.clone(),
                qualified_type: reference.qualified_type.clone(),
                role: NEW_VERSION_ROLE.to_string(),
                title: title.clone(),
                json,
            }],
            datasets: Vec::new(),
            activity: ActivityDraft {
                recipe: "new-version",
                title: format!("new-version → {title} @ {version}"),
                seed: format!("new-version:{}:{version}", reference.uuid),
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
    fn the_place_of_a_version_is_the_one_the_object_uses_or_its_release() {
        let two_one = json!({ "Citation": { "Title": "t" } });
        assert_eq!(
            place_of(&two_one, "resqml20.obj_TriangulatedSetRepresentation"),
            Some(Place::Citation)
        );
        assert_eq!(
            place_of(&two_one, "resqml22.TriangulatedSetRepresentation"),
            Some(Place::Root)
        );
        // What the object already has wins over its release.
        assert_eq!(
            place_of(&json!({ "objectVersion": "1" }), "resqml20.obj_X"),
            Some(Place::Root)
        );
        assert_eq!(
            place_of(&json!({ "Citation": { "VersionString": "1" } }), "resqml22.X"),
            Some(Place::Citation)
        );
        // Nothing to say.
        assert_eq!(place_of(&json!({}), "resqml22.X"), None);
    }
}
