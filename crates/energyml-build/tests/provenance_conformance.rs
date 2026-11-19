//! What an operation writes about itself agrees with its template, and a
//! template a package already holds does not outlive the canonical one.
//!
//! # Where the forms come from
//!
//! - common TRG 2.3 section 3.1, p. 40: a parameter *"must have an equivalent
//!   in the activity descriptor parameters"*;
//! - common TRG 2.3 section 3.10, p. 49: `MinOccurs`, `MaxOccurs` (-1 is
//!   infinite), `AllowedKind` (empty allows every kind);
//! - common TRG 2.3 section 3.8, p. 47: `IntegerQuantityParameter` holds a
//!   `Value: long`.
//!
//! # Two independent sources of one fact
//!
//! The Activity is what `apply` wrote for the operation; the template is what
//! the package holds under the operation's uuid; the rule is
//! `energyml_domain::activity::conformance`, which reads both back from the
//! package and knows nothing of the operations. Before this file, no test ran
//! an operation and then asked the rule about what it had written: the
//! projection of the slots onto the template was checked
//! (`operations.rs`), and the rule was checked on activities written by hand
//! (`energyml-domain`), but never the two together.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::{Value, json};

use energyml_build::apply::{Made, apply, apply_with};
use energyml_build::operation::{Choices, Identity, Need, Registry, Slot, When};
use energyml_build::pieces::Pieces;
use energyml_domain::activity::{ActivityRecord, TemplateRecord, conformance, read_activity, read_template};
use energyml_storage::EpcStore;

use crate::operations::{bag_for, sigeol_pieces};

fn when() -> When {
    When {
        creation: "2026-11-19T00:00:00Z".into(),
        originator: "test".into(),
    }
}

fn schemas() -> Arc<energyml_core::registry::Registry> {
    Arc::new(energyml_schemas::all())
}

fn empty_store() -> EpcStore {
    energyml_build::epc::open(None, "conformance.epc", schemas()).expect("an empty package")
}

fn activity_in(store: &EpcStore, uri: &str) -> ActivityRecord {
    let object = store.get_object(uri).expect("the activity reads");
    read_activity(object.as_reflect()).expect("an activity")
}

fn template_in(store: &EpcStore, uuid: &str) -> (String, TemplateRecord) {
    let uri = store
        .find_by_uuid(uuid)
        .unwrap_or_else(|| panic!("the template {uuid} is in the package"))
        .uri
        .clone();
    let object = store.get_object(&uri).expect("the template reads");
    (uri, read_template(object.as_reflect()).expect("a template"))
}

// -- what each operation writes agrees with its template ---------------------

/// The operations the test cannot feed yet, **named**, each with its reason.
///
/// An operation is here because the generic feeder (`bag_for` for its inputs,
/// `answers` for its required settings) does not produce a plan that `apply`
/// writes with an activity: it needs an object of the package that exists, a
/// file, or a setting the feeder cannot invent. Each line says which. An
/// operation absent from this list and from the registry is a dead line
/// (`no_exclusion_names_a_missing_operation`); one that is in the list and
/// *can* be fed is a stale line
/// (`an_excluded_operation_is_still_one_the_feeder_cannot_write`). Nothing is
/// skipped in silence.
const NOT_FED: &[(&str, &str)] = &[
    (
        "isocontours",
        "the exemplar shape is flat: there is no interval to cut",
    ),
    (
        "cross-section",
        "the exemplar surfaces hold no material along a trace",
    ),
    ("intersect", "the exemplar fault cuts none of the surfaces"),
    (
        "project",
        "projecting onto a surface needs a surface or a level with a z",
    ),
    (
        "fault-from-sticks",
        "a band is lofted between two sticks at least, the exemplar has one",
    ),
    (
        "fault-throw",
        "needs a LocalEngineeringCompoundCrs of the package as `crs`",
    ),
    ("clip", "needs a ring, `outer` or `inner`"),
    (
        "faulted-ijk-grid",
        "needs an interpretation of the package, or a feature type and an interpretation type",
    ),
    (
        "subrepresentation",
        "needs `element` and the indices into the support",
    ),
    (
        "representation-set",
        "needs members that are representations; the exemplar object is a Model",
    ),
    ("blocked-wellbore", "needs a frame whose measured depths read"),
    (
        "change-frame",
        "needs a LocalEngineeringCompoundCrs of the package as `crs`",
    ),
    (
        "zone-statistics",
        "needs the zone of each element, as a field or a `zoneProperty`",
    ),
    (
        "property",
        "needs a property kind, which the feeder does not invent",
    ),
    (
        "property-time-series",
        "needs a property of the package as `base`",
    ),
    (
        "combine",
        "needs a property kind, which the feeder does not invent",
    ),
    ("graphical-information", "needs colours for its targets"),
    (
        "well-from-plan",
        "needs a plan of sections in the operation's own grammar",
    ),
    (
        "witsml-wells",
        "needs a wellbore frame with measured depths read from arrays",
    ),
    ("witsml-logs", "needs a frame that represents an interpretation"),
    (
        "product-flow-model",
        "needs units drawn once and a loop through them",
    ),
    ("sigeol-boreholes", "needs a GeoSciML Borehole in its features"),
    (
        "stratigraphy-from-geolts",
        "needs divisions of the BRGM chart, a vocabulary the feeder does not invent",
    ),
    (
        "earth-model-from-stratigraphy",
        "needs a stratigraphy read from a package",
    ),
    (
        "edit-object",
        "needs the body of an object read by the package's reader",
    ),
    (
        "new-version",
        "needs the body of an object read by the package's reader",
    ),
    (
        "table-link",
        "needs the body of an object read by the package's reader",
    ),
    ("pointset-from-wellheads", "needs wells that state a datum"),
    (
        "properties-from-table",
        "needs a table with a column that is a property, not only structural roles",
    ),
    ("contacts-from-frames", "needs frames that map intervals to units"),
    ("extend-boreholes", "needs frames that state measured depths"),
    (
        "computed-tops",
        "needs a wellbore trajectory or a frame to follow",
    ),
    ("attach", "needs a file or an address"),
    (
        "section-view",
        "needs an SVG written by the section view, with its metadata",
    ),
    (
        "along-well-view",
        "needs an SVG written by the along-well view, with its metadata",
    ),
    (
        "export-media",
        "needs the name of an exporter, which the feeder does not know",
    ),
    (
        "time-series",
        "continues a parent and needs the index of the instant it continues from",
    ),
    (
        "polylines-from-cloud",
        "needs a cut count or a length, chosen by a mode the feeder does not pick",
    ),
    (
        "extend",
        "needs a band and a distance, chosen by a mode the feeder does not pick",
    ),
    (
        "grid-properties",
        "needs a grid whose frame says which way its vertical axis points",
    ),
    (
        "local-grid",
        "needs parent cells inside the parent grid, which the exemplar grid is too small for",
    ),
    (
        "thickness",
        "needs the unit of the heights, which the exemplar surfaces do not state",
    ),
    (
        "measure",
        "needs the unit of the coordinates, which the exemplar frame does not state",
    ),
    (
        "prodml-doublet",
        "needs two different holes, an injector and a producer",
    ),
    ("fluid-sample", "needs components written as formula=mg/L"),
    (
        "injection-test",
        "needs a nominal period from its own list of words",
    ),
    (
        "wells-and-properties-from-table",
        "needs a table with a column that is a property, not only structural roles",
    ),
    (
        "well-logs",
        "needs where the ground is: a surface or a datum height",
    ),
];

/// A value for a required setting, of the shape its need expects - or `None`
/// when the feeder cannot invent one (an object that must exist, a CRS of the
/// package, a vocabulary term).
///
/// Lists take as many values as their floor, increasing (numbers and
/// instants) so that an ordering rule is met; a bound at zero is stepped over,
/// zero being the value rules refuse most.
fn answer(slot: &Slot) -> Option<Value> {
    let many = slot.takes_many();
    let count = if many { slot.least.max(1) as usize } else { 1 };
    let about = slot.about.to_lowercase();
    let a_date = ["utc", "date", "instant", "iso 8601"]
        .iter()
        .any(|word| about.contains(word));
    let ones: Vec<Value> = (0..count)
        .map(|rank| {
            Some(match &slot.need {
                Need::Number { least, most, .. } => {
                    let base = match (least, most) {
                        (Some(least), Some(most)) => (least + most) / 2.0,
                        (Some(value), None) | (None, Some(value)) => *value,
                        (None, None) => 1.0,
                    };
                    let base = if base <= 0.0 && most.is_none_or(|most| most > 0.0) {
                        1.0
                    } else {
                        base
                    };
                    json!(base + rank as f64)
                }
                Need::Integer { least, most } => {
                    let base = match (least, most) {
                        (Some(least), Some(most)) => (least + most) / 2,
                        (Some(value), None) | (None, Some(value)) => *value,
                        (None, None) => 1,
                    };
                    let base = if base <= 0 && most.is_none_or(|most| most > 0) {
                        1
                    } else {
                        base
                    };
                    json!(base + rank as i64)
                }
                Need::Text if a_date => json!(format!("2025-01-{:02}T00:00:00Z", rank + 1)),
                Need::Text => json!("x"),
                Need::Flag => json!(false),
                Need::Choice(words) => json!(words.first()?),
                Need::Epsg => json!(2154),
                Need::Title => json!("Exemplar"),
                Need::Object { types } => json!({
                    "Uuid": "7a1c2d3e-4f50-4a6b-8c7d-9e0f1a2b3c4d",
                    "QualifiedType": types.first().copied().unwrap_or("eml23.BusinessAssociate"),
                    "Title": slot.name,
                }),
                _ => return None,
            })
        })
        .collect::<Option<Vec<Value>>>()?;
    Some(if many {
        Value::Array(ones)
    } else {
        ones.into_iter().next()?
    })
}

/// What the generic feeder makes of an operation: the bag of its inputs and an
/// answer for each required setting that has no suggestion.
fn fed(registry: &Registry, name: &str) -> (Pieces, Choices) {
    let operation = registry.get(name).expect("the operation is in the registry");
    let pieces = if name == "sigeol-units" {
        // Its content is checked, not only its shape: the exemplar of the
        // operation's own tests.
        sigeol_pieces()
    } else {
        bag_for(&operation.takes())
    };
    let slots = operation.needs(&pieces);
    let mut choices = Choices::new();
    for slot in &slots {
        // A slot that cannot act is neither required nor filled: answering it
        // anyway is a contradiction the settle refuses. Whether it acts is
        // asked of its own declaration, against the answers so far. Besides
        // the required ones, `sourceEpsg` is answered: the frame an operation
        // may write its coordinates in when no CRS of the package is named.
        let so_far = choices.clone();
        let answered = |one: &str| so_far.get(one).is_some_and(|value| !value.is_null());
        let effective = |one: &str| {
            answered(one)
                || slots
                    .iter()
                    .any(|other| other.name == one && other.suggested.is_some())
        };
        let settled = |one: &str| so_far.get(one).cloned();
        let wanted = slot.is_required() || (matches!(slot.need, Need::Epsg) && slot.name == "sourceEpsg");
        if wanted
            && slot.suggested.is_none()
            && slot.applies_now(answered, effective, settled)
            && let Some(value) = answer(slot)
        {
            choices = choices.set(slot.name, value);
        }
    }
    (pieces, choices)
}

/// Plan and apply one operation into a new package. `Err` says where it stopped.
fn run(registry: &Registry, name: &str) -> Result<(EpcStore, Made, &'static str), String> {
    let (pieces, choices) = fed(registry, name);
    let plan = registry
        .plan(name, &pieces, &choices, &Identity::derived(), &when())
        .map_err(|why| format!("no plan: {why}"))?;
    let recipe = plan.activity.recipe;
    let mut store = empty_store();
    // Without the arrays: what is checked is the activity, and an array backend
    // is not what decides it.
    let made = apply_with(&plan, &mut store, &when(), false);
    if made.activity.is_none() {
        return Err(format!("no activity written: {:?}", made.refused));
    }
    Ok((store, made, recipe))
}

/// **Every built-in operation writes an Activity its template accepts.**
///
/// Spec: common TRG 2.3 section 3.1 p. 40, section 3.10 p. 49 - the rule is
/// `energyml_domain::activity::conformance`. The operation's Activity and the
/// template are both read back from the package `apply` wrote, so the
/// comparison is not the operation's own idea of what it wrote.
///
/// Break it: have an operation record a parameter its slots do not declare (a
/// `Parameter::Text` under a title the template lacks, as `own` of a recorded
/// activity), or edit a `MaxOccurs` in `data/activityTemplates.json` below what
/// an operation writes - the line for that operation lists the issue.
#[test]
fn every_operation_writes_an_activity_its_template_accepts() {
    let registry = Registry::new();
    let excluded: BTreeSet<&str> = NOT_FED.iter().map(|(name, _)| *name).collect();
    let mut wrong = Vec::new();
    let mut checked = 0usize;
    for operation in registry.iter() {
        let name = operation.name();
        if excluded.contains(name) {
            continue;
        }
        let (store, made, recipe) = match run(&registry, name) {
            Ok(done) => done,
            Err(why) => {
                // Not skipped: add the operation to `NOT_FED` with this as its
                // reason, or feed it.
                wrong.push(format!("`{name}` was not fed: {why}"));
                continue;
            }
        };
        let uuid = energyml_domain::provenance::template_uuid(recipe).expect("a canonical template");
        let (_, template) = template_in(&store, uuid);
        let activity = activity_in(&store, made.activity.as_deref().expect("an activity"));
        for issue in conformance(&activity, Some(&template)) {
            wrong.push(format!("`{name}`: {issue:?}"));
        }
        checked += 1;
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    // The negative check: a feeder that feeds nothing would pass the loop.
    // 83 operations in the registry, 48 in `NOT_FED`: 35 are expected to be
    // fed; the floor is that minus a margin of 2.
    assert!(checked >= 33, "only {checked} operation(s) were fed");
}

/// An exclusion names an operation of the registry: a removed or renamed
/// operation does not leave a dead line behind.
///
/// Break it: add `("no-such-operation", "x")` to `NOT_FED`.
#[test]
fn no_exclusion_names_a_missing_operation() {
    let registry = Registry::new();
    let missing: Vec<&str> = NOT_FED
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| registry.get(name).is_none())
        .collect();
    assert!(
        missing.is_empty(),
        "excluded but not in the registry: {missing:?}"
    );
    let unique: BTreeSet<&str> = NOT_FED.iter().map(|(name, _)| *name).collect();
    assert_eq!(unique.len(), NOT_FED.len(), "an operation is excluded twice");
}

/// An excluded operation is one the feeder still cannot write: when it can, the
/// line is stale and the operation belongs under the check above.
///
/// Break it: name `crs` in `NOT_FED`.
#[test]
fn an_excluded_operation_is_still_one_the_feeder_cannot_write() {
    let registry = Registry::new();
    let fed_now: Vec<&str> = NOT_FED
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| registry.get(name).is_some() && run(&registry, name).is_ok())
        .collect();
    assert!(fed_now.is_empty(), "now fed, drop them from NOT_FED: {fed_now:?}");
}

// -- a template the package already holds ------------------------------------

/// The canonical template of `crs` as JSON, without the parameters `drop`.
fn crs_template_without(drop: &[&str]) -> Value {
    let mut template = energyml_domain::provenance::template("crs")
        .expect("the canonical `crs`")
        .clone();
    template["Parameter"]
        .as_array_mut()
        .expect("parameters")
        .retain(|one| !drop.contains(&one["Title"].as_str().unwrap_or("")));
    template
}

fn put_json(store: &mut EpcStore, json: &Value) -> String {
    let document = energyml_core::json::read_string(&json.to_string(), &schemas()).expect("it reads back");
    store.put(document.root).expect("it is put")
}

fn crs_plan() -> energyml_build::operation::Plan {
    Registry::new()
        .plan(
            "crs",
            &Pieces::empty(),
            &Choices::new().set("epsg", 2154),
            &Identity::derived(),
            &when(),
        )
        .expect("crs plans")
}

fn crs_uuid() -> &'static str {
    energyml_domain::provenance::template_uuid("crs").expect("the canonical `crs`")
}

/// **A template of an older build is replaced, and the run says so.**
///
/// The package holds a `crs` template without `output`, as an earlier build
/// could have written it; the Activity `apply` writes records its outputs. Kept,
/// the old template would report every output as a parameter with no equivalent
/// (common TRG 2.3 section 3.1 p. 40). Replaced by the canonical one, nothing is
/// reported; and the sentence in `Made::superseded` names the operation and the
/// parameter.
///
/// An Activity written *earlier* stays judged by the new template: a defect it
/// had (an integer written as a double) is still reported, and that is right -
/// the template was replaced, the Activity was not rewritten.
///
/// Break it: restore `if store.find_by_uuid(template_uuid).is_none()` in
/// `apply_with` - the template stays stale and the conformance below is not empty.
#[test]
fn a_stale_template_is_replaced_and_said() {
    let mut store = empty_store();
    let stale_uri = put_json(&mut store, &crs_template_without(&["output"]));
    let stale = template_in(&store, crs_uuid()).1;
    assert!(stale.parameters.iter().all(|one| one.title != "output"));

    let made = apply(&crs_plan(), &mut store, &when());
    assert!(made.refused.is_empty(), "{:?}", made.refused);
    assert_eq!(made.superseded.len(), 1, "{:?}", made.superseded);
    assert!(
        made.superseded[0].contains("`crs`") && made.superseded[0].contains("output"),
        "{}",
        made.superseded[0]
    );
    assert!(made.replaced.contains(&stale_uri), "{:?}", made.replaced);

    // One object under the uuid, the canonical one.
    assert_eq!(store.summaries_of(crs_uuid()).len(), 1);
    let (_, template) = template_in(&store, crs_uuid());
    assert!(template.parameters.iter().any(|one| one.title == "output"));
    let activity = activity_in(&store, made.activity.as_deref().expect("an activity"));
    assert_eq!(conformance(&activity, Some(&template)), Vec::new());
    // The fixture is not vacuous: against the stale template, the same Activity
    // is reported.
    assert!(!conformance(&activity, Some(&stale)).is_empty());
}

/// **The same content is not rewritten**, whatever its citation says: a package
/// rewritten without change stays byte for byte, and the run says nothing.
///
/// Break it: replace on any difference of the object (the citation's title
/// below differs) instead of a difference of the template's content.
#[test]
fn a_template_of_the_same_content_is_left_alone() {
    let mut store = empty_store();
    let first = apply(&crs_plan(), &mut store, &when());
    assert!(first.superseded.is_empty(), "{:?}", first.superseded);
    let (uri, _) = template_in(&store, crs_uuid());

    // The same parameters, another title and date: not a difference of content.
    let mut other = energyml_domain::provenance::template("crs").expect("crs").clone();
    other["Citation"]["Title"] = json!("held:crs");
    other["Citation"]["Creation"] = json!("2020-01-01T00:00:00Z");
    put_json(&mut store, &other);

    let again = apply(&crs_plan(), &mut store, &when());
    assert!(again.refused.is_empty(), "{:?}", again.refused);
    assert!(again.superseded.is_empty(), "{:?}", again.superseded);
    assert!(!again.written.contains(&uri), "{:?}", again.written);
    let held = store.get_object(&uri).expect("it reads");
    let title = read_template(held.as_reflect()).expect("a template").title;
    assert_eq!(
        title.as_deref(),
        Some("held:crs"),
        "the held object was kept as it is"
    );
}

/// **An object that is not a template under the template's uuid is refused by
/// name**, and no activity is written against it.
///
/// Break it: let `apply_with` write the Activity after a refused template.
#[test]
fn something_else_under_the_template_uuid_is_refused_by_name() {
    let mut store = empty_store();
    put_json(
        &mut store,
        &json!({
            "$type": "eml23.DataobjectCollection",
            "uuid": crs_uuid(),
            "schemaVersion": "2.3",
            "Citation": {
                "$type": "eml23.Citation",
                "Title": "not a template",
                "Originator": "test",
                "Creation": "2026-11-19T00:00:00Z",
                "Format": "test",
            },
            "Kind": "folder",
            "HomogeneousDatatype": false,
        }),
    );
    let made = apply(&crs_plan(), &mut store, &when());
    assert!(made.activity.is_none(), "{:?}", made.activity);
    assert!(
        made.refused
            .iter()
            .any(|why| why.contains("is not one") && why.contains(crs_uuid())),
        "{:?}",
        made.refused
    );
    assert!(made.superseded.is_empty());
}
