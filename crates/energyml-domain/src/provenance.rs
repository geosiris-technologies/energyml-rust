//! Recording that an operation happened, in EnergyML's own terms.
//!
//! An `ActivityTemplate` says *what kind of operation this is and what it
//! takes*; an `Activity` says *this operation ran, on these inputs, with these
//! parameters, and produced this*. Together they are the only place a package
//! says where an object came from.
//!
//! # Why this is here and not in the JavaScript face
//!
//! It used to live in a WebAssembly front end, because artefact promotion —
//! the first thing that needed provenance — was written for the 3D view. That
//! was accretion, not a decision, and it contradicted the rule that crate states
//! about itself in its own first paragraph: *a translation, not a layer — nothing
//! here knows anything the libraries do not*. The canonical templates and the way
//! an activity is assembled were knowledge nothing else had.
//!
//! The question was whether these templates could be used directly from Rust,
//! and the proof arrived the same day: `energyml-build` needs the identical mechanism to record a GeoJSON
//! import. Two consumers is what settles where something belongs, and the
//! alternative was a second copy of ten canonical uuids, which is the one thing
//! the resource file forbids in its own doc.
//!
//! # It builds objects; it does not store them
//!
//! The rule this crate and `energyml-storage::graph` both follow: **free
//! functions over values, no store**. [`activity`] returns the JSON of an
//! `Activity`; putting it in a package, looking up whether the template is
//! already there, resolving a uri into a `DataObjectReference` — all of that
//! belongs to whoever owns the store, and differs between an EPC and a
//! dataspace.
//!
//! # JSON and not typed objects, deliberately
//!
//! The templates are eml 2.3 documents, so this is version-specific *data* —
//! like `data/property_kinds.tsv`, which this crate
//! already carries the same way. Keeping it as the workspace's own JSON encoding
//! means no dependency on a generated crate, one registry rather than one per
//! release, and a caller free to `put_json` it into whatever store it has.

use std::sync::OnceLock;

use serde_json::{Value, json};

/// The canonical templates, by recipe name.
///
/// **One fixed uuid per recipe, the same in every package**. An `Activity` always references the same template,
/// so a consumer recognises the operation by uuid, and inserting the template
/// into two packages inserts the same object byte for byte (which is why its
/// `Creation` is fixed too).
///
/// Never change an existing uuid: it would orphan every activity already
/// written. Adding a recipe is a new entry with a new uuid, canonical from its
/// first commit.
pub fn templates() -> &'static Value {
    static TEMPLATES: OnceLock<Value> = OnceLock::new();
    TEMPLATES.get_or_init(|| {
        let mut all: Value = serde_json::from_str(include_str!("../data/activityTemplates.json"))
            .expect("data/activityTemplates.json is valid JSON — checked by a test");
        // `Citation.Format` carries the software's version, so it cannot live in
        // a file written by hand: it would go stale at the first release. It is
        // set here, once, for every template.
        if let Some(templates) = all.as_object_mut() {
            for template in templates.values_mut() {
                if let Some(citation) = template.get_mut("Citation").and_then(Value::as_object_mut) {
                    citation.insert("Format".to_string(), Value::from(energyml_core::CITATION_FORMAT));
                }
            }
        }
        all
    })
}

/// One recipe's canonical template, or a refusal naming the file to edit.
///
/// A refusal and not a silently invented template: inventing one per package is
/// exactly what the fixed-uuid rule exists to prevent, and the failure would be
/// invisible — every package would carry a plausible template of its own and
/// nothing would join them.
pub fn template(recipe: &str) -> Result<&'static Value, String> {
    let found = &templates()[recipe];
    if found.is_null() {
        return Err(format!(
            "no canonical template for `{recipe}` — add it to data/activityTemplates.json"
        ));
    }
    Ok(found)
}

/// The uuid a recipe's template always has.
pub fn template_uuid(recipe: &str) -> Result<&'static str, String> {
    Ok(template(recipe)?["uuid"]
        .as_str()
        .expect("every canonical template carries a uuid"))
}

/// A parameter to record, before it is typed.
///
/// Three kinds and no more, because three are what the templates declare:
/// `double` with a unit, `string`, and a reference to an object. A caller with
/// something else has a template to extend first, which is the right order —
/// a parameter no template declares is a parameter no consumer will look for.
#[derive(Debug, Clone)]
pub enum Parameter {
    /// A measured value and its unit of measure.
    Double {
        /// The parameter's name, matching a `ParameterTemplate` title.
        title: String,
        /// The value.
        value: f64,
        /// The Energistics uom code. `None` becomes `Euc`, Energistics'
        /// *unitless* — never an invented unit.
        uom: Option<String>,
    },
    /// A word: a mode, a file name, a stage.
    Text {
        /// The parameter's name.
        title: String,
        /// The value.
        value: String,
    },
    /// An object, named by an already-built `DataObjectReference`.
    ///
    /// The reference is built by the caller because building one needs the
    /// store: a uuid alone is not a DOR, and guessing the content type would be
    /// this module inventing what a package says.
    Object {
        /// Usually `input` or `output`.
        title: String,
        /// The `DataObjectReference`, as JSON.
        reference: Value,
    },
}

impl Parameter {
    /// The typed eml 2.3 parameter this becomes.
    fn written(&self) -> Value {
        match self {
            Parameter::Double { title, value, uom } => json!({
                "$type": "eml23.DoubleQuantityParameter",
                "Title": title,
                "IsUncertain": false,
                "Value": value,
                "Uom": uom.as_deref().unwrap_or("Euc"),
            }),
            Parameter::Text { title, value } => json!({
                "$type": "eml23.StringParameter",
                "Title": title,
                "IsUncertain": false,
                "Value": value,
            }),
            Parameter::Object { title, reference } => json!({
                "$type": "eml23.DataObjectParameter",
                "Title": title,
                "IsUncertain": false,
                "DataObject": reference,
            }),
        }
    }
}

/// Everything an activity needs that this module cannot know.
#[derive(Debug, Clone)]
pub struct Record<'a> {
    /// Which canonical recipe ran.
    pub recipe: &'a str,
    /// The activity's own uuid.
    ///
    /// **From the caller, never generated here.** wasm has no randomness of its
    /// own and the house rule is that it enters through the seam; a command line
    /// that wants a reproducible run derives it rather than drawing it.
    pub uuid: &'a str,
    /// `YYYY-MM-DDThh:mm:ssZ`. From the caller for the same reason as the uuid:
    /// a clock is an input, and a build that reads one cannot be replayed.
    pub creation: &'a str,
    /// What the activity is called. `None` builds `<recipe> → <output>` from
    /// `output_title`.
    pub title: Option<&'a str>,
    /// The output object's title, for the default activity title.
    pub output_title: Option<&'a str>,
    /// The reference to the template, as JSON — the caller builds it because it
    /// knows where the template ended up in its store.
    pub descriptor: Value,
    /// The parameters, inputs and outputs among them.
    pub parameters: Vec<Parameter>,
}

/// Build the `Activity`.
///
/// Refuses only what it can judge: an unknown recipe. Everything else it is
/// handed — a uuid, a date, a reference — is the caller's to have got right,
/// and checking a uuid's shape here would be a second validator disagreeing with
/// the one in the schema.
pub fn activity(record: &Record<'_>) -> Result<Value, String> {
    // Asked for its side effect: an activity naming a recipe with no canonical
    // template would reference a template nobody can insert.
    template(record.recipe)?;
    let title = match record.title {
        Some(title) => title.to_string(),
        None => match record.output_title {
            Some(output) => format!("{} → {output}", record.recipe),
            None => record.recipe.to_string(),
        },
    };
    let written: Vec<Value> = record.parameters.iter().map(Parameter::written).collect();
    Ok(json!({
        "$type": "eml23.Activity",
        "uuid": record.uuid,
        "schemaVersion": "2.3",
        "Citation": {
            "$type": "eml23.Citation",
            "Title": title,
            "Originator": "energyml-rust",
            "Format": energyml_core::CITATION_FORMAT,
            "Creation": record.creation,
        },
        "ActivityDescriptor": record.descriptor,
        "Parameter": written,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_canonical_template_is_a_well_formed_activity_template() {
        let all = templates();
        let entries: Vec<(&String, &Value)> = all
            .as_object()
            .expect("the registry is an object")
            .iter()
            .filter(|(key, _)| !key.starts_with("__"))
            .collect();
        assert!(!entries.is_empty(), "an empty registry would prove nothing");
        for (recipe, template) in &entries {
            assert_eq!(template["$type"], "eml23.ActivityTemplate", "{recipe}");
            assert_eq!(template["schemaVersion"], "2.3", "{recipe}");
            assert_eq!(
                template["Citation"]["Title"].as_str(),
                Some(format!("fabricant:{recipe}").as_str()),
                "{recipe}: a template's title names its recipe, which is how a package's own \
                 template is recognised and reused instead of duplicated"
            );
            assert!(
                template["Parameter"].as_array().is_some_and(|p| !p.is_empty()),
                "{recipe}: a template with no parameter describes nothing"
            );
            // `Citation.Creation` is required by the schema (an xsd:dateTime,
            // pattern `.+T.+[Z+\-].*`), and the template is written as it is
            // here: 14 of 89 had none until 01/10, so every package written by
            // `transform`, `project`, `clip`, `terrain`… failed `check`.
            // To break it: remove the date from an entry of `data/activityTemplates.json`.
            let creation = template["Citation"]["Creation"].as_str().unwrap_or("");
            let (date, rest) = creation.split_once('T').unwrap_or(("", ""));
            assert!(
                !date.is_empty() && rest.contains(['Z', '+', '-']),
                "{recipe}: the template's Citation.Creation `{creation}` is no dateTime — it is written as it is, and `check` refuses it"
            );
        }
    }

    #[test]
    fn the_uuids_are_fixed_and_distinct() {
        // Two recipes sharing a uuid would make a consumer read one operation as
        // the other, and the file's own doc forbids ever changing one.
        let mut seen: Vec<&str> = Vec::new();
        for (recipe, template) in templates().as_object().unwrap() {
            if recipe.starts_with("__") {
                continue;
            }
            let uuid = template["uuid"]
                .as_str()
                .unwrap_or_else(|| panic!("{recipe} has no uuid"));
            assert!(!seen.contains(&uuid), "{recipe} reuses the uuid {uuid}");
            seen.push(uuid);
        }
        assert!(seen.len() >= 10, "the registry lost recipes: {}", seen.len());
    }

    #[test]
    fn an_unknown_recipe_is_refused_by_name_and_says_where_to_add_it() {
        let why = template("no-such-recipe").expect_err("refused");
        assert!(why.contains("no-such-recipe"), "{why}");
        assert!(why.contains("data/activityTemplates.json"), "{why}");
        // And an activity cannot be built for it either — the refusal is not
        // only on the lookup, or a caller could write an activity pointing at a
        // template nobody can insert.
        let record = Record {
            recipe: "no-such-recipe",
            uuid: "00000000-0000-4000-8000-000000000001",
            creation: "2026-09-07T00:00:00Z",
            title: None,
            output_title: None,
            descriptor: Value::Null,
            parameters: Vec::new(),
        };
        assert!(activity(&record).is_err());
    }

    #[test]
    fn the_parameters_come_out_typed_and_a_unitless_double_is_euc() {
        let record = Record {
            recipe: "offset",
            uuid: "00000000-0000-4000-8000-000000000002",
            creation: "2026-09-07T00:00:00Z",
            title: None,
            output_title: Some("Top Brent"),
            descriptor: json!({ "$type": "eml23.DataObjectReference" }),
            parameters: vec![
                Parameter::Double {
                    title: "dx".into(),
                    value: 12.5,
                    uom: Some("m".into()),
                },
                Parameter::Double {
                    title: "dz".into(),
                    value: 0.0,
                    uom: None,
                },
                Parameter::Text {
                    title: "mode".into(),
                    value: "line".into(),
                },
                Parameter::Object {
                    title: "output".into(),
                    reference: json!({ "Uuid": "abc" }),
                },
            ],
        };
        let made = activity(&record).expect("built");
        assert_eq!(made["Citation"]["Title"], "offset → Top Brent");
        let parameters = made["Parameter"].as_array().unwrap();
        assert_eq!(parameters[0]["$type"], "eml23.DoubleQuantityParameter");
        assert_eq!(parameters[0]["Uom"], "m");
        // `Euc` is Energistics' *unitless*. Never an invented unit, and never
        // an absent one: the field is required.
        assert_eq!(parameters[1]["Uom"], "Euc");
        assert_eq!(parameters[2]["$type"], "eml23.StringParameter");
        assert_eq!(parameters[3]["$type"], "eml23.DataObjectParameter");
        assert_eq!(parameters[3]["DataObject"]["Uuid"], "abc");
    }

    #[test]
    fn an_explicit_title_wins_and_a_nameless_output_falls_back_to_the_recipe() {
        let base = |title, output| Record {
            recipe: "offset",
            uuid: "00000000-0000-4000-8000-000000000003",
            creation: "2026-09-07T00:00:00Z",
            title,
            output_title: output,
            descriptor: Value::Null,
            parameters: Vec::new(),
        };
        assert_eq!(
            activity(&base(Some("À la main"), Some("x"))).unwrap()["Citation"]["Title"],
            "À la main"
        );
        // No output name is not a reason to write `offset → ` with nothing after
        // the arrow, which reads like a truncated title.
        assert_eq!(
            activity(&base(None, None)).unwrap()["Citation"]["Title"],
            "offset"
        );
    }
}
