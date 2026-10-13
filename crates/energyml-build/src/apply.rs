//! Applying a plan: the only place that touches storage.
//!
//! Everything has been decided already — the objects, their uuids, the arrays,
//! what the activity records — so what is left is plumbing, and plumbing is
//! where an honest error message matters more than a clever one. Nothing here
//! is refused silently: an object that will not read back, an array the
//! backend cannot write, a template that is missing, each is named in
//! [`Made::refused`] and the rest goes on.
//!
//! # The order, and why it is not free
//!
//! The same as the SIGEOL writer's, for the same reason — it is about what a
//! **reader** of a half-written package would see:
//!
//! 1. the objects, in the plan's order — an operation puts what nothing points
//!    at first;
//! 2. the arrays, into the external file;
//! 3. the files the plan carries (`Plan::media`) and their relations to the
//!    objects they show — after the objects, so that one of the same plan can
//!    be named;
//! 4. the canonical `ActivityTemplate`, unless the package already holds it;
//! 5. the activity, which names everything above.
//!
//! # `EpcStore`, and not the trait — for now
//!
//! `EnergymlStorage` is the reading trait; `put` and `write_array_to` are the
//! package's own. When the ETP client arrives, this is the one function that
//! grows a second implementation — which is the point of keeping it alone.

use std::collections::BTreeSet;
use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_domain::provenance::{self, Parameter, Record};
use energyml_storage::EpcStore;
use energyml_storage::array::{ArrayData, ArrayMetadata, ArrayValues};

use crate::operation::{NewObject, Numbers, Plan, When};
use crate::pieces::Ref;

/// What applying a plan did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Made {
    /// The uris written, in order — objects, then the template if it was
    /// added, then the activity.
    pub written: Vec<String>,
    /// What could not be written, and why.
    pub refused: Vec<String>,
    /// The activity's uri, when it was written.
    pub activity: Option<String>,
    /// The uris that this write **replaced** — the object was already there.
    ///
    /// This is news and not a detail: a package written twice from two
    /// different sources is the ordinary case, and `put` replaces silently.
    /// Replaying the same file replaces its own objects — the identity is
    /// derived, that is intended, and the count says so; overwriting those of
    /// **another** run is what one wants to see before discovering it in the
    /// package.
    ///
    /// They are also in [`Made::written`]: this is not a separate category, it
    /// is something more that is known about some of them.
    pub replaced: Vec<String>,
    /// The titles written **beside** an object of the same title and the same
    /// type.
    ///
    /// This is the news the package would otherwise leave unsaid. Since the
    /// identity comes from the content, a different content no longer
    /// overwrites: it **adds**. That is the intended behaviour — when the
    /// values differ, a second property is created — and it is also the one
    /// thing a package does not show by itself: two objects with the same
    /// title read as a duplicate, not as two versions, as long as nobody says
    /// that the second has just been written.
    ///
    /// What it does **not** say, and which remains to be done: *how* the second
    /// differs from the first. Knowing that requires reading both whole
    /// objects; this only opens their header.
    pub versioned: Vec<String>,
    /// The files put in the package (`Plan::media`), by part name — a part is
    /// not an object and has no uri, so it is not in [`Made::written`].
    pub media: Vec<String>,
    /// **What an object written breaks in its schema** — one line per error of
    /// `Validate` (pattern, facet, cardinality, `fixed`, lexical type…), naming
    /// the object, the path and the rule.
    ///
    /// Not a refusal: the object is written, because losing what somebody asked
    /// for over a facet is a worse answer than saying it. But **an operation
    /// that writes non-conformant objects is a defect of the operation**, and
    /// this is where it shows — `section-view` wrote `CulturalFeatureKind =
    /// "section line"`, which `CulturalFeatureKindExt` (`.*:.*`) forbids, and
    /// nothing said so until it was found. Tests of operations assert it is empty.
    pub invalid: Vec<String>,
    /// **The collections this run wrote, by role** — `(role, uri)`, in the plan's
    /// order of roles. A role of eight objects or more (`MANY`) is grouped into a
    /// `DataobjectCollection`; this is where its address is said, so that the run
    /// manifest ([`crate::manifest`]) names it without deriving it a second
    /// time. A role the collection could not be written for is absent.
    pub collections: Vec<(String, String)>,
}

/// What the objects of a plan break in their schema — what [`Made::invalid`]
/// reports after the write, for a caller that only **plans** (a test of one
/// operation, a preview). One line per error of `Validate`: object, path, rule.
///
/// An object that does not read back typed is **not** reported here: that is
/// `apply`'s refusal, and a plan whose JSON no reader accepts is a bigger fault
/// than a facet.
pub fn conformance(plan: &Plan, registry: &Registry) -> Vec<String> {
    let mut out = Vec::new();
    for object in &plan.objects {
        if let Ok(document) = energyml_core::json::read_string(&object.json.to_string(), registry) {
            out.extend(invalid_lines(&object.title, &object.uuid, document.root.as_validate()));
        }
    }
    out
}

fn invalid_lines(what: &str, uuid: &str, object: &dyn energyml_core::validate::Validate) -> Vec<String> {
    energyml_core::validate::validate_with(object, &[])
        .errors()
        .map(|issue| format!("{what} ({uuid}): {} at {}: {}", issue.code, issue.path, issue.message))
        .collect()
}

/// Write a plan into a package: objects, arrays, template, activity.
pub fn apply(plan: &Plan, store: &mut EpcStore, when: &When) -> Made {
    apply_with(plan, store, when, true)
}

/// Write a plan, with or without its arrays.
///
/// Without, for a store whose array backend cannot write — the browser's
/// h5wasm bridge — where the caller writes the arrays itself, right after the
/// objects that name them exist. The external file is still registered, so
/// that `save` names it in the `.rels`.
pub fn apply_with(plan: &Plan, store: &mut EpcStore, when: &When, arrays: bool) -> Made {
    apply_under(plan, store, when, arrays, None)
}

/// Write a plan, its activity a **sub-activity** of `parent` when one is given.
///
/// eml 2.3 `Activity.Parent`, 0..1 — *« An optional parent activity, when the
/// activity is a sub-activity »* (common TRG 2.3 §3.3 p. 42). A recipe
/// ([`crate::recipe`]) is a workflow whose steps are operations, and each
/// operation's activity names the workflow's root **as it is written**, which
/// is what the workflow structure requires.
///
/// Every other caller passes `None`, through [`apply`] and [`apply_with`], and
/// writes exactly what it wrote before: the parent is the only thing this
/// adds, and it adds it to the activity alone.
pub fn apply_under(plan: &Plan, store: &mut EpcStore, when: &When, arrays: bool, parent: Option<&Ref>) -> Made {
    let _span = tracing::info_span!("apply", objects = plan.objects.len(), datasets = plan.datasets.len(), arrays).entered();
    let mut made = Made::default();
    // **What is missing for writing is refused here, and nowhere else.**
    //
    // A blocked plan has computed its geometry — it is good, the scene can keep
    // it — and it lacks what is needed to write the objects that would carry it
    // (a plan without a frame is blocked, not refused). `apply` is the only
    // place that touches storage, so the only one that has to refuse.
    //
    // Before the emptiness check, and the order is the point: a blocked plan
    // may well carry objects, and saying *"there is nothing to write"* would be
    // the wrong news.
    if let Some(why) = &plan.blocked {
        made.refused.push(format!("nothing was written: {why}"));
        return made;
    }
    if plan.is_empty() {
        made.refused.push("the plan holds no object: nothing was written".to_string());
        return made;
    }
    let registry = store.registry_arc();

    // **What the package already holds, by (type, title)**, read **once** and
    // only for the types this plan writes.
    //
    // Two rules of the project meet here. *What is true of the package is paid
    // for per package*: asking it per object would reread the whole package as
    // many times as we write, the cost that a front end's colour chains once paid.
    // And *before adding a call, say what it reads*: a title is not in an
    // object's summary, it is in its citation, so this opens the header of each
    // part **of the type concerned** — a few hundred bytes each. Filtering by
    // type is what makes it cheap in the case that matters: enriching a
    // package of boreholes with properties only opens the properties, and
    // there are none yet.
    //
    // And it is **before** the loop, because that is the only window in which
    // the question has an answer — afterwards, all the titles are there.
    let writing: BTreeSet<&str> = plan.objects.iter().map(|one| one.qualified_type.as_str()).collect();
    let looking: Vec<String> = store
        .resources()
        .into_iter()
        .filter(|one| writing.contains(one.qualified_type.as_str()))
        .map(|one| one.uri)
        .collect();
    let held: BTreeSet<(String, String)> = store
        .headers(&looking)
        .into_iter()
        .flatten()
        .filter_map(|one| one.title.map(|title| (one.summary.qualified_type, title)))
        .collect();

    for object in &plan.objects {
        // Asked **before** writing, like `replaced` and for the same reason:
        // after `put`, the title is there in both cases.
        let beside = held.contains(&(object.qualified_type.clone(), object.title.clone()));
        let at = made.written.len();
        put(
            store,
            &registry,
            &object.json,
            &object.title,
            &mut made,
            object.role == crate::operation::NEW_VERSION_ROLE,
        );
        // *Beside* and not *instead of*: an object that replaces another one is
        // not a second version, it is the same one rewritten.
        if beside && made.written.len() > at && made.replaced.last() != made.written.last() {
            made.versioned.push(object.title.clone());
        }
    }

    // **Grouped by file, written in one opening each** (`write_arrays_to`):
    // one opening per dataset was 95 % of the time a SIGEOL import took.
    // The order within a file is the plan's.
    let mut files: Vec<(&str, Vec<(&str, ArrayData)>)> = Vec::new();
    for dataset in plan.datasets.iter().filter(|_| arrays) {
        let data = ArrayData {
            metadata: ArrayMetadata {
                dimensions: dataset.dimensions.clone(),
                element_type: match dataset.values {
                    Numbers::F64(_) => "float64".to_string(),
                    Numbers::I64(_) => "int64".to_string(),
                },
            },
            values: match &dataset.values {
                Numbers::F64(values) => ArrayValues::F64(values.clone()),
                Numbers::I64(values) => ArrayValues::I64(values.clone()),
            },
        };
        match files.iter_mut().find(|(file, _)| *file == dataset.file.as_str()) {
            Some((_, items)) => items.push((dataset.path.as_str(), data)),
            None => files.push((dataset.file.as_str(), vec![(dataset.path.as_str(), data)])),
        }
    }
    {
        let _arrays = tracing::info_span!("apply.arrays", files = files.len()).entered();
        for (file, items) in &files {
            let borrowed: Vec<(&str, &ArrayData)> = items.iter().map(|(path, data)| (*path, data)).collect();
            for ((path, _), result) in items.iter().zip(store.write_arrays_to(file, &borrowed)) {
                if let Err(e) = result {
                    made.refused.push(format!("{file}:{path}: {e}"));
                }
            }
        }
    }
    if let (Some(first), Some(dataset)) = (plan.objects.first(), plan.datasets.first()) {
        // Named for `save`, which regenerates the `.rels` from what the store
        // knows: a fresh external file nothing names is a package whose arrays
        // cannot be found.
        store.register_external_file(&first.uuid, dataset.file.clone());
    }

    // The files, then what each one shows. The store refuses by name what the
    // packaging conventions forbid — a reserved name, a content type that is
    // not one, an object or a part that is not there — and each refusal is
    // said here, never skipped.
    for media in &plan.media {
        if let Err(e) = store.set_attachment(&media.path, media.bytes.clone(), Some(media.content_type.clone())) {
            made.refused.push(format!("{}: {e}", media.path));
            continue;
        }
        made.media.push(media.path.clone());
        for uuid in &media.related_to {
            if let Err(e) = store.relate_media(uuid, &media.path) {
                made.refused.push(format!("{} as a media representation of {uuid}: {e}", media.path));
            }
        }
    }

    let recipe = plan.activity.recipe;
    let template_uuid = match provenance::template_uuid(recipe) {
        Ok(uuid) => uuid,
        Err(why) => {
            made.refused.push(why);
            return made;
        }
    };
    // *Is there such an object*: the template is one per uuid, and the question is
    // existence — what `find_by_uuid` is for.
    if store.find_by_uuid(template_uuid).is_none() {
        match provenance::template(recipe) {
            Ok(template) => put(store, &registry, template, "the canonical template", &mut made, false),
            Err(why) => made.refused.push(why),
        }
    }

    let mut parameters = plan.activity.parameters.clone();
    let grouped = grouped_outputs(&plan.objects);
    for (role, objects) in &grouped.collected {
        let collection_uuid = crate::naming::collection(recipe, &plan.activity.seed, role);
        let title = collection_title(&plan.activity.title, role, objects.len());
        let before = made.written.len();
        put(
            store,
            &registry,
            &serde_json::json!({
                "$type": "eml23.DataobjectCollection",
                "uuid": collection_uuid,
                "schemaVersion": "2.3",
                "Citation": {
                    "$type": "eml23.Citation",
                    "Title": title,
                    "Originator": when.originator,
                    "Creation": when.creation,
                    "Format": energyml_core::CITATION_FORMAT,
                },
                // `folder` and not `study`: what is grouped here is what **a run
                // produced**, not a modelling intention. The other four values
                // of `CollectionKindExt` all say something about the trade that
                // nobody has stated.
                "Kind": "folder",
                // **A flag, not a type.** Collection.xsd: *« Boolean flag.
                // If true all data objects in the collection are of the same
                // Energistics data type »*. This field used to carry the type
                // name, and the consequence was not cosmetic: the collection
                // could not be read back, so `apply` refused it and **no run
                // had a collection** — a refusal measured on 120 boreholes, the
                // only place where it showed.
                //
                // And it is **computed** rather than asserted true. The grouping
                // is by role, so it is true in practice; but a role that one
                // day carried two types would then write a false flag, and a
                // false flag is what a reader believes without opening the
                // list — which is all it is for.
                "HomogeneousDatatype": homogeneous(objects),
            }),
            "the collection of this run",
            &mut made,
            false,
        );
        if made.written.len() > before
            && let Some(uri) = made.written.last()
        {
            made.collections.push((role.clone(), uri.clone()));
        }
        // **The activity names only the collection.** That is the whole point
        // of the change: 936 boreholes made 4 682 `output` parameters, of which
        // 4 672 said the same thing as this one.
        parameters.push(Parameter::Object {
            title: "output".to_string(),
            reference: serde_json::json!({
                "$type": "eml23.DataObjectReference",
                "Uuid": collection_uuid,
                "QualifiedType": "eml23.DataobjectCollection",
                "Title": title,
            }),
        });
    }

    // **One set of associations for the run**, its collections inside it:
    // `SingleCollectionAssociation` is `1..∞`, so one per role would be as
    // many more objects in a package as this change exists to lighten.
    if !grouped.collected.is_empty() {
        let associations: Vec<serde_json::Value> = grouped
            .collected
            .iter()
            .map(|(role, objects)| {
                serde_json::json!({
                    "$type": "eml23.SingleCollectionAssociation",
                    "HomogeneousDatatype": homogeneous(objects),
                    "Collection": {
                        "$type": "eml23.DataObjectReference",
                        "Uuid": crate::naming::collection(recipe, &plan.activity.seed, role),
                        "QualifiedType": "eml23.DataobjectCollection",
                        // **The collection's own title**, count included: a
                        // reference's title is a copy, and a copy that differs
                        // is what `check` reports as stale — on every run that
                        // grouped anything (two warnings per package).
                        "Title": collection_title(&plan.activity.title, role, objects.len()),
                    },
                    "Dataobject": objects
                        .iter()
                        .map(|one| serde_json::json!({
                            "$type": "eml23.DataObjectReference",
                            "Uuid": one.uuid,
                            "QualifiedType": one.qualified_type,
                            "Title": one.title,
                        }))
                        .collect::<Vec<_>>(),
                })
            })
            .collect();
        put(
            store,
            &registry,
            &serde_json::json!({
                "$type": "eml23.CollectionsToDataobjectsAssociationSet",
                "uuid": crate::naming::associations(recipe, &plan.activity.seed),
                "schemaVersion": "2.3",
                "Citation": {
                    "$type": "eml23.Citation",
                    "Title": format!("{} — collections", plan.activity.title),
                    "Originator": when.originator,
                    "Creation": when.creation,
                    "Format": energyml_core::CITATION_FORMAT,
                },
                "SingleCollectionAssociation": associations,
            }),
            "the collection memberships",
            &mut made,
            false,
        );
    }

    // And what remains is named directly: the unique ones — a CRS, a graphical
    // information set — are what one goes looking for in an activity.
    for object in &grouped.named {
        parameters.push(Parameter::Object {
            title: "output".to_string(),
            reference: serde_json::json!({
                "$type": "eml23.DataObjectReference",
                "Uuid": object.uuid,
                "QualifiedType": object.qualified_type,
                "Title": object.title,
            }),
        });
    }
    let uuid = crate::naming::activity_seeded(recipe, &plan.activity.seed, &when.creation);
    match provenance::activity(&Record {
        recipe,
        uuid: &uuid,
        creation: &when.creation,
        title: Some(&plan.activity.title),
        output_title: None,
        descriptor: serde_json::json!({
            "$type": "eml23.DataObjectReference",
            "Uuid": template_uuid,
            "QualifiedType": "eml23.ActivityTemplate",
            "Title": format!("fabricant:{recipe}"),
        }),
        parameters,
    }) {
        Ok(mut json) => {
            // `Parent` is read back by name like every other field: a key the
            // reader did not know would be dropped without a word, which is
            // why `tests/recipe.rs` reads the activity back from the package.
            if let Some(parent) = parent {
                json["Parent"] = parent.dor();
            }
            let before = made.written.len();
            put(store, &registry, &json, "the activity", &mut made, false);
            if made.written.len() > before {
                made.activity = made.written.last().cloned();
            }
        }
        Err(why) => made.refused.push(why),
    }
    made
}

/// Read the JSON back through the ordinary reader, then put it: a misspelled
/// field fails loudly at the seam instead of producing a part nothing can read.
/// From how many objects of the same **role** the activity names a
/// collection rather than each of them.
///
/// A number, and a small one. Measured on an import of 936 boreholes: the
/// activity carried **4 692 parameters, 4 682 of them `output`** — 99.8 % —
/// and the front end's object editor froze on it. It was not a defect of the
/// editor: it was given an object nobody can read.
///
/// Why not **two**, that is, *as soon as there are several*: an operation that
/// writes three objects of one role is exactly the case where naming them
/// stays more readable than opening a collection to find them. And why not a
/// hundred: the threshold must bite well before an activity becomes
/// unreadable, not at the moment it already is.
const MANY: usize = 8;

/// What an activity names: collections on one side, objects on the other.
struct Outputs<'a> {
    /// By role, in the order in which the roles appear in the plan.
    collected: Vec<(String, Vec<&'a crate::operation::NewObject>)>,
    /// Those that the activity names one by one.
    named: Vec<&'a crate::operation::NewObject>,
}

/// Group a plan's objects **by role**, and decide which go into a
/// collection.
///
/// By role and not by qualified type, because the role says *what this object
/// is for this run* — two `PointSetRepresentation`s produced as `roof` and as
/// `base` are two batches, not one. `NewObject::role` exists for exactly that.
///
/// **The plan's order is kept**, so that two runs of the same file write the
/// same package: a `HashMap` would give its roles in an order that changes from
/// one execution to the next, and the collection titles with it.
fn grouped_outputs(objects: &[crate::operation::NewObject]) -> Outputs<'_> {
    let mut order: Vec<String> = Vec::new();
    let mut by_role: std::collections::HashMap<&str, Vec<&crate::operation::NewObject>> =
        std::collections::HashMap::new();
    for object in objects {
        if !by_role.contains_key(object.role.as_str()) {
            order.push(object.role.clone());
        }
        by_role.entry(object.role.as_str()).or_default().push(object);
    }

    let mut collected = Vec::new();
    let mut named = Vec::new();
    for role in order {
        let group = by_role.remove(role.as_str()).unwrap_or_default();
        if group.len() >= MANY {
            collected.push((role, group));
        } else {
            named.extend(group);
        }
    }
    Outputs { collected, named }
}

/// Are all these objects of the same Energistics type?
///
/// `Collection.xsd` §`HomogeneousDatatype`: *« Boolean flag. If true all data
/// objects in the collection are of the same Energistics data type (EXAMPLE:
/// All wellbores or all horizons). »*
///
/// An empty batch answers **true**: there is nothing that contradicts, and
/// answering false would say *there are several types in there* of a collection
/// that holds none.
fn homogeneous(objects: &[&NewObject]) -> bool {
    let Some(first) = objects.first() else { return true };
    objects.iter().all(|one| one.qualified_type == first.qualified_type)
}

fn put(
    store: &mut EpcStore,
    registry: &Arc<Registry>,
    json: &serde_json::Value,
    what: &str,
    made: &mut Made,
    new_version: bool,
) {
    match energyml_core::json::read_string(&json.to_string(), registry) {
        Ok(document) => {
            // **Asked before writing**, and it is the only window in which the question
            // has an answer: after `put`, the object is there in both cases. By the
            // identity of the object read — (uuid, type) —, not by its uuid: a uuid
            // held under another type is not what `put` replaces.
            let already = energyml_core::identity::ObjectId::of_reflect(document.root.as_reflect()).is_some_and(|id| {
                let qualified_type = id.qualified_type();
                store.summaries_of(&id.uuid).iter().any(|held| held.qualified_type == qualified_type)
            });
            made.invalid.extend(invalid_lines(what, json["uuid"].as_str().unwrap_or("?"), document.root.as_validate()));
            // Another version of an object the package holds stands **beside** it:
            // that is the one gate that adds a version, and nothing is replaced.
            let written = if new_version { store.put_as_new_version(document.root) } else { store.put(document.root) };
            match written {
                Ok(uri) => {
                    if already && !new_version {
                        made.replaced.push(uri.clone());
                    }
                    made.written.push(uri)
                }
                Err(e) => made.refused.push(format!("{what}: {e}")),
            }
        }
        Err(e) => made.refused.push(format!("{what} did not read back: {e}")),
    }
}

/// The title of a run's collection — written once, so the collection and the
/// references to it cannot disagree.
fn collection_title(activity: &str, role: &str, count: usize) -> String {
    format!("{activity} — {role} ({count} objects)")
}
