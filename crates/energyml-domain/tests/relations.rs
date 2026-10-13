//! The relations the index lays while it builds, and keeps while it updates —
//! `energyml_domain::package_index` (ADR-0010).
//!
//! The link optimisation of the inheritance walk also applies to attachments and
//! similar relations, and the relations are built in addition at
//! graph-construction time, rather than by traversing the whole graph
//! afterwards.
//!
//! The packages are **built in memory** after the specifications — common TRG
//! 2.3 §8.3 and §8.5 for the associations, `Attachment.xsd` for the attachments,
//! `GraphicalInformationSet` of eml 2.3 for the decorations — never read from a
//! file. The shape of the assertions is the design's; what is counted is
//! **work** (`PackageIndex::traffic`, `attachments::CacheStats`), never time.
//!
//! # Triangulation
//!
//! A component must never be asked to check itself: the tables
//! are confronted with a second reading that shares nothing with the index —
//! the objects read back **by their field names** (`reread`), where the index
//! reads reference **paths** — and, for the graphical entries, with
//! `GraphicalSet::read`, which scans the XML with another scanner than the
//! reference walk the index is built from.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use std::sync::Arc;

use energyml_core::any_xml::AnyXml;
use energyml_core::registry::DynObject;
use energyml_core::value::{FieldValue, Reflect};
use energyml_domain::attachments::{self, Cache, Files, attachments_of_with, encode_base64};
use energyml_domain::color::{Cascade, GraphicalSet, Information, Level, Scope};
use energyml_domain::color_walks::{Ask, ColourCache};
use energyml_domain::inherit::Walker;
use energyml_domain::package_index::PackageIndex;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_storage::EpcStore;

use crate::inherit::{citation, dor, u};

// -- building blocks ---------------------------------------------------------

fn store() -> EpcStore {
    EpcStore::from_reader(
        Box::new(Cursor::new(
            energyml_storage::extract::empty_package(None).expect("an empty package"),
        )),
        "relations.epc",
        Arc::new(energyml_schemas::all()),
    )
    .expect("it opens")
}

fn put(store: &mut EpcStore, object: impl DynObject + 'static) {
    store.put(Box::new(object)).expect("it goes in");
}

fn feature(store: &mut EpcStore, n: u32) {
    put(
        store,
        resqml22::WellboreFeature {
            uuid: u(n),
            schema_version: "2.2".into(),
            citation: citation(&format!("feature {n}")),
            ..Default::default()
        },
    );
}

fn interpretation(store: &mut EpcStore, n: u32, of: u32) {
    put(
        store,
        resqml22::WellboreInterpretation {
            uuid: u(n),
            schema_version: "2.2".into(),
            citation: citation(&format!("interpretation {n}")),
            interpreted_feature: dor(&u(of), "resqml22.WellboreFeature"),
            ..Default::default()
        },
    );
}

fn representation(store: &mut EpcStore, n: u32, of: Option<u32>) {
    put(
        store,
        resqml22::WellboreTrajectoryRepresentation {
            uuid: u(n),
            schema_version: "2.2".into(),
            citation: citation(&format!("representation {n}")),
            represented_object: of.map(|i| dor(&u(i), "resqml22.WellboreInterpretation")),
            ..Default::default()
        },
    );
}

fn collection(store: &mut EpcStore, n: u32, kind: eml23::CollectionKindExt) {
    put(
        store,
        eml23::DataobjectCollection {
            uuid: u(n),
            schema_version: "2.3".into(),
            citation: citation(&format!("collection {n}")),
            kind: Some(kind),
            ..Default::default()
        },
    );
}

/// One association set: `(collection, members)` per association, in order.
fn set(store: &mut EpcStore, n: u32, associations: &[(u32, &[u32])]) {
    put(
        store,
        eml23::CollectionsToDataobjectsAssociationSet {
            uuid: u(n),
            schema_version: "2.3".into(),
            citation: citation(&format!("set {n}")),
            single_collection_association: associations
                .iter()
                .map(|(collection, members)| eml23::SingleCollectionAssociation {
                    collection: dor(&u(*collection), "eml23.DataobjectCollection"),
                    dataobject: members.iter().map(|m| dor(&u(*m), "resqml22.Anything")).collect(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        },
    );
}

fn model(store: &mut EpcStore, n: u32) {
    put(
        store,
        resqml22::Model {
            uuid: u(n),
            schema_version: "2.2".into(),
            citation: citation("the model"),
            ..Default::default()
        },
    );
}

fn earth_model(store: &mut EpcStore, n: u32, model: u32, wells: &[u32]) {
    put(
        store,
        resqml22::EarthModelInterpretation {
            uuid: u(n),
            schema_version: "2.2".into(),
            citation: citation("the earth model"),
            interpreted_feature: dor(&u(model), "resqml22.Model"),
            wellbore_interpretation_set: wells
                .iter()
                .map(|w| dor(&u(*w), "resqml22.WellboreInterpretation"))
                .collect(),
            ..Default::default()
        },
    );
}

fn attach(store: &mut EpcStore, n: u32, target: u32, qualified_type: &str, content: &[u8]) {
    put(
        store,
        eml23::Attachment {
            uuid: u(n),
            schema_version: "2.3".into(),
            citation: citation(&format!("attachment {n}")),
            object_reference: Some(dor(&u(target), qualified_type)),
            content: encode_base64(content),
            file_name: Some(format!("file-{n}.png")),
            file_type: Some("image/png".into()),
            ..Default::default()
        },
    );
}

/// One entry of a `GraphicalInformationSet`: the subtree nothing could type, as
/// every real set carries it, naming `targets`.
fn entry(kind: &str, targets: &[u32]) -> AnyXml {
    let content: String = targets
        .iter()
        .map(|t| {
            format!(
                "<eml23:TargetObject><eml23:Uuid>{}</eml23:Uuid>\
                 <eml23:QualifiedType>resqml22.WellboreTrajectoryRepresentation</eml23:QualifiedType>\
                 <eml23:Title>t</eml23:Title></eml23:TargetObject>",
                u(*t)
            )
        })
        .collect();
    AnyXml {
        attributes: vec![("xsi:type".to_string(), format!("resqml22:{kind}"))],
        content,
        namespaces: Vec::new(),
    }
}

fn graphical_set(store: &mut EpcStore, n: u32, entries: Vec<AnyXml>) {
    put(
        store,
        eml23::GraphicalInformationSet {
            uuid: u(n),
            schema_version: "2.3".into(),
            citation: citation(&format!("graphical set {n}")),
            graphical_information: entries,
            ..Default::default()
        },
    );
}

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n-not really a picture-";
const IMAGE: &str = "resqml22.WellboreTrajectoryRepresentation";

/// The package every table is read on:
///
/// ```text
/// feature 1 <- interpretation 2 <- representations 3 and 4
/// folder 10, folder 11, project 12
/// set 20: association 0  folder 10 holds {3, 4}
///         association 1  folder 11 holds {10}
///         association 2  folder 10 again (§8.3: ignored) holds {4}
///         association 3  project 12 holds {11}
///         association 4  names no collection... (written below, by hand) holds {3}
/// model 30, earth model 31 (of 30, naming the interpretation 2)
/// attachments 40 (on 3) and 41 (on 1)
/// graphical set 50: entry 0 Default -> 3; entry 1 Color -> 1; entry 2 Default -> 4, 11
/// ```
fn fabricated() -> EpcStore {
    let mut store = store();
    feature(&mut store, 1);
    interpretation(&mut store, 2, 1);
    representation(&mut store, 3, Some(2));
    representation(&mut store, 4, Some(2));
    collection(&mut store, 10, eml23::CollectionKindExt::Folder);
    collection(&mut store, 11, eml23::CollectionKindExt::Folder);
    collection(&mut store, 12, eml23::CollectionKindExt::Project);
    set(
        &mut store,
        20,
        &[(10, &[3, 4]), (11, &[10]), (10, &[4]), (12, &[11])],
    );
    model(&mut store, 30);
    earth_model(&mut store, 31, 30, &[2]);
    attach(&mut store, 40, 3, IMAGE, PNG);
    attach(&mut store, 41, 1, "resqml22.WellboreFeature", PNG);
    graphical_set(
        &mut store,
        50,
        vec![
            entry("DefaultGraphicalInformation", &[3]),
            entry("ColorInformation", &[1]),
            entry("DefaultGraphicalInformation", &[4, 11]),
        ],
    );
    store
}

fn index_of(store: &EpcStore) -> PackageIndex {
    PackageIndex::build(store, store.revision())
}

// -- the two readings --------------------------------------------------------

/// What the index says, from its accessors only, as lines to compare.
fn digest_of_index(index: &PackageIndex, uuids: &[String]) -> BTreeSet<String> {
    let mut lines = BTreeSet::new();
    for uuid in uuids {
        for (rank, parent) in index.parent_rows(uuid) {
            lines.insert(format!("parent {uuid} {rank} {}", parent.uuid));
        }
        for row in index.associations_of(uuid) {
            lines.insert(format!(
                "association {uuid} {} {} repeated={:?}",
                row.index, row.collection, row.repeated_of
            ));
            for member in index.members_of_association(uuid, row.index) {
                lines.insert(format!("member {uuid} {} {}", row.index, member.uuid));
            }
        }
        for held in index.memberships_of(uuid) {
            lines.insert(format!(
                "held {uuid} {} {} {}",
                held.holder, held.association, held.position
            ));
        }
        if index.is_project(uuid) {
            lines.insert(format!("project {uuid}"));
        }
        for model in index.models_referencing(uuid) {
            lines.insert(format!("model {uuid} {} {}", model.uuid, model.type_key));
        }
        for attachment in index.attachments_naming(uuid) {
            lines.insert(format!("attached {uuid} {attachment}"));
        }
        for row in index.graphical_entries_of(uuid) {
            lines.insert(format!(
                "graphical {uuid} {} {} default={}",
                row.set,
                row.entry,
                row.is_default()
            ));
        }
    }
    lines
}

fn names(object: &dyn Reflect, field: &str) -> Vec<String> {
    let one = |value: &FieldValue<'_>| -> Option<String> {
        let reference = value.as_object()?;
        Some(
            reference
                .field("Uuid")?
                .as_scalar()?
                .to_string()
                .to_ascii_lowercase(),
        )
    };
    match object.field(field) {
        Some(FieldValue::List(items)) => items.iter().filter_map(one).collect(),
        Some(other) => one(&other).into_iter().collect(),
        None => Vec::new(),
    }
}

/// What the objects say, **read back by their field names** — no reference
/// path, no `collect_refs`, nothing the index is built from.
fn reread(store: &EpcStore) -> BTreeSet<String> {
    let mut lines = BTreeSet::new();
    for summary in store.resources() {
        let uuid = summary.uuid.to_ascii_lowercase();
        let object = store.get_object(&summary.uri).expect("it reads");
        let object: &dyn Reflect = object.as_reflect();
        let ty = summary
            .qualified_type
            .rsplit('.')
            .next()
            .unwrap()
            .trim_start_matches("obj_")
            .to_string();

        for (rank, field) in [
            "RepresentedObject",
            "RepresentedInterpretation",
            "InterpretedFeature",
        ]
        .iter()
        .enumerate()
        {
            for parent in names(object, field) {
                lines.insert(format!("parent {uuid} {rank} {parent}"));
            }
        }
        match ty.as_str() {
            "CollectionsToDataobjectsAssociationSet" => {
                let Some(FieldValue::List(associations)) = object.field("SingleCollectionAssociation") else {
                    continue;
                };
                let mut first: BTreeMap<String, usize> = BTreeMap::new();
                for (i, association) in associations.iter().enumerate() {
                    let association = association.as_object().expect("an association");
                    let members = names(association, "Dataobject");
                    for (j, member) in members.iter().enumerate() {
                        if *member != uuid {
                            lines.insert(format!("held {member} {uuid} {i} {j}"));
                        }
                        lines.insert(format!("member {uuid} {i} {member}"));
                    }
                    if let Some(collection) = names(association, "Collection").into_iter().next() {
                        let repeated = first.get(&collection).copied();
                        first.entry(collection.clone()).or_insert(i);
                        lines.insert(format!(
                            "association {uuid} {i} {collection} repeated={repeated:?}"
                        ));
                    }
                }
            }
            "DataobjectCollection" => {
                let project = object
                    .field("Kind")
                    .and_then(|k| k.as_scalar())
                    .is_some_and(|k| k.to_string() == "project");
                if project {
                    lines.insert(format!("project {uuid}"));
                }
            }
            "Model" | "EarthModelInterpretation" => {
                for field in ["InterpretedFeature", "WellboreInterpretationSet"] {
                    for target in names(object, field) {
                        if target != uuid {
                            lines.insert(format!("model {target} {uuid} {ty}"));
                        }
                    }
                }
            }
            "Attachment" => {
                for target in names(object, "ObjectReference") {
                    lines.insert(format!("attached {target} {uuid}"));
                }
            }
            "GraphicalInformationSet" => {
                let read = GraphicalSet::read(object);
                for entry in &read.entries {
                    let default = matches!(entry.information, Information::Default { .. });
                    for target in &entry.targets {
                        lines.insert(format!(
                            "graphical {} {uuid} {} default={default}",
                            target.to_ascii_lowercase(),
                            entry.position
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    lines
}

fn uuids_of(store: &EpcStore) -> Vec<String> {
    store
        .resources()
        .into_iter()
        .map(|summary| summary.uuid.to_ascii_lowercase())
        .collect()
}

fn assert_same(label: &str, got: &BTreeSet<String>, wanted: &BTreeSet<String>) {
    let missing: Vec<_> = wanted.difference(got).collect();
    let extra: Vec<_> = got.difference(wanted).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "{label}\n  the index lacks: {missing:#?}\n  the index adds: {extra:#?}"
    );
}

// -- the tables against a second reading ---------------------------------------

/// Every table says what the objects say when they are read again, by name.
/// The fabricated package puts a row in each table and a case in each rule: a
/// collection named twice in one set (§8.3), a project, a model and an earth
/// model, an attachment, a graphical set with a default and a colour entry and
/// one entry of two targets.
///
/// To break it: do not remove `target != id` from `lay_set`, or make an association's
/// `Collection` say `Part::Member` — the `held` row changes and the test goes
/// red; or empty a table in `Relations::take`.
#[test]
fn the_tables_say_what_a_second_reading_says() {
    let store = fabricated();
    let index = index_of(&store);
    let uuids = uuids_of(&store);
    let got = digest_of_index(&index, &uuids);
    let wanted = reread(&store);
    // The negative control is in the shape: a digest that holds every table.
    for table in [
        "parent ",
        "association ",
        "member ",
        "held ",
        "project ",
        "model ",
        "attached ",
        "graphical ",
    ] {
        assert!(
            wanted.iter().any(|line| line.starts_with(table)),
            "the fabricated package has no `{table}` row to prove anything on"
        );
    }
    assert_same("tables against a second reading", &got, &wanted);
    assert!(index.relation_rows() > 0);
}

/// §8.3: the same collection named by two associations of one set — the second
/// is ignored, and the row **says so**, so a walk does not have to count.
#[test]
fn a_collection_named_twice_in_a_set_is_marked_repeated_once_and_for_all() {
    let store = fabricated();
    let index = index_of(&store);
    let rows = index.associations_of(&u(20));
    let repeated: Vec<(u64, Option<u64>)> = rows.iter().map(|row| (row.index, row.repeated_of)).collect();
    assert_eq!(repeated, vec![(0, None), (1, None), (2, Some(0)), (3, None)]);
    // The way back: who holds folder 11, and who the associations list.
    let named: Vec<(&str, u64)> = index
        .collection_named_by(&u(11))
        .into_iter()
        .map(|(holder, at)| (&*holder.uuid, at))
        .collect();
    assert_eq!(named, vec![(u(20).as_str(), 1)]);
    let members: Vec<String> = index
        .members_of_association(&u(20), 0)
        .into_iter()
        .map(|node| node.uuid.to_string())
        .collect();
    assert_eq!(members, vec![u(3), u(4)]);
}

/// Who carries a graphical information, and what kind: the decoration of a
/// colour and of a default are told apart without opening the set.
#[test]
fn who_carries_a_default_graphical_information_is_a_table() {
    let store = fabricated();
    let index = index_of(&store);
    let default_of = |n: u32| {
        index
            .graphical_entries_of(&u(n))
            .iter()
            .any(|row| row.is_default())
    };
    assert!(default_of(3) && default_of(4) && default_of(11));
    assert!(
        !default_of(1),
        "object 1 is decorated by a ColorInformation, not a default one"
    );
    assert!(index.is_graphically_decorated(&u(1)));
    assert!(!index.is_graphically_decorated(&u(2)));
    let sets: Vec<String> = index
        .graphical_sets()
        .iter()
        .map(|node| node.uuid.to_string())
        .collect();
    assert_eq!(sets, vec![u(50)]);
    assert_eq!(index.graphical_entry_counts(), vec![3]);
}

// -- the update ----------------------------------------------------------------

/// What an update must leave: **the tables a rebuild would give**, and the
/// objects' own reading.
fn assert_update_equals_rebuild(store: &EpcStore, updated: &PackageIndex) {
    let uuids = uuids_of(store);
    let rebuilt = digest_of_index(&index_of(store), &uuids);
    assert_same(
        "updated against rebuilt",
        &digest_of_index(updated, &uuids),
        &rebuilt,
    );
    assert_same(
        "updated against the objects",
        &digest_of_index(updated, &uuids),
        &reread(store),
    );
}

/// A collection that **gains a member**: the member's memberships grow, nobody
/// else's change, and what the update disturbs is that member.
///
/// To break it: do not remove the `held` rows of a re-absorbed set
/// (`Relations::forget`) — the existing member would be counted twice.
#[test]
fn a_collection_that_gains_a_member_is_followed_by_the_tables() {
    let mut store = fabricated();
    let mut index = index_of(&store);
    let before = index.memberships_of(&u(2)).len();
    assert_eq!(before, 0);

    // The set 20 writes association 0 again with member 2 added.
    set(
        &mut store,
        20,
        &[(10, &[3, 4, 2]), (11, &[10]), (10, &[4]), (12, &[11])],
    );
    let disturbed = index.update(&store, &[u(20)], store.revision());

    assert_eq!(index.memberships_of(&u(2)).len(), 1, "2 is now listed once");
    assert_eq!(index.memberships_of(&u(3)).len(), 1, "3 is untouched");
    assert_update_equals_rebuild(&store, &index);
    // The set itself and the new member; not the collection, not the old members.
    assert!(
        disturbed.contains(&u(2)) && disturbed.contains(&u(20)),
        "{disturbed:?}"
    );
    assert!(
        !disturbed.contains(&u(3)) && !disturbed.contains(&u(10)),
        "{disturbed:?}"
    );

    // And the walk reads it: 2 now inherits from the folder.
    let has = |identity: &str| (crate::inherit::uuid_of(identity) == u(10)).then_some("folder");
    let walked = Walker::new(&index, &store, &has).resolve(&u(2));
    let found = walked.resolution.expect("the folder is reached");
    assert_eq!(found.carrier, u(10));
}

/// A project turned into a folder (and back): the table of projects follows the
/// `Kind` of the collection, and a walk through it follows the table.
#[test]
fn a_collection_whose_kind_changes_is_followed_and_the_walks_through_it_are_dropped() {
    let mut store = fabricated();
    let mut index = index_of(&store);
    assert!(index.is_project(&u(12)) && !index.is_project(&u(10)));

    // Colour on the project 12, which holds the folder 11, which holds 10, which holds 3.
    let colour = |hue: f64| GraphicalSet {
        entries: vec![energyml_domain::color::Entry {
            position: 0,
            targets: vec![u(12)],
            target_uris: Vec::new(),
            map_uri: None,
            information: Information::Default {
                viewer_kind: None,
                viewer_id: None,
                elements: vec![energyml_domain::color::ElementInfo {
                    at: 0,
                    scope: Scope::WholeObject,
                    visible: true,
                    constant_color: Some(energyml_domain::color::Hsv {
                        hue,
                        saturation: 1.0,
                        value: 1.0,
                        alpha: 1.0,
                        title: None,
                    }),
                    constant_alpha: None,
                    overwrite_color_alpha: false,
                    active_color: None,
                }],
            },
        }],
        notes: Vec::new(),
    };
    let set_of_colours = colour(120.0);
    let mut cache = ColourCache::new();
    let ask = |cache: &mut ColourCache, index: &PackageIndex, store: &EpcStore| {
        let cascade = Cascade::new(&set_of_colours);
        let uuid = u(3);
        let scopes = [Scope::WholeObject];
        cache
            .resolve(
                &cascade,
                index,
                store,
                &[Ask {
                    uri: &uuid,
                    scopes: &scopes,
                }],
            )
            .remove(0)
            .remove(0)
    };
    // A project's collections are not followed: nothing is found, and it is said.
    let answer = ask(&mut cache, &index, &store);
    assert!(
        answer.walked.resolution.is_none(),
        "the colour sits on a project: it is not followed"
    );
    assert!(
        answer.walked.notes.iter().any(|note| note.contains("project")),
        "{:?}",
        answer.walked.notes
    );

    // The collection stops being a project.
    collection(&mut store, 12, eml23::CollectionKindExt::Folder);
    let disturbed = index.update(&store, &[u(12)], store.revision());
    assert!(!index.is_project(&u(12)));
    assert_update_equals_rebuild(&store, &index);
    let dropped = cache.disturb(&disturbed);
    assert_eq!(
        dropped, 1,
        "the walk of 3 went through the collection 12; no other was held"
    );
    let answer = ask(&mut cache, &index, &store);
    let found = answer.walked.resolution.expect("a folder is followed");
    assert_eq!(
        (found.carrier.as_str(), found.value.level),
        (u(12).as_str(), Level::Collection)
    );
}

/// A `GraphicalInformationSet` **modified**: the decoration moves from one
/// object to another, and the tables say so.
#[test]
fn a_graphical_set_that_changes_moves_its_rows() {
    let mut store = fabricated();
    let mut index = index_of(&store);
    assert!(index.is_graphically_decorated(&u(3)));

    graphical_set(
        &mut store,
        50,
        vec![
            entry("DefaultGraphicalInformation", &[2]),
            entry("ColorInformation", &[1]),
        ],
    );
    let disturbed = index.update(&store, &[u(50)], store.revision());

    assert!(!index.is_graphically_decorated(&u(3)), "3 is no longer targeted");
    assert!(
        index
            .graphical_entries_of(&u(2))
            .iter()
            .any(|row| row.is_default() && row.entry == 0)
    );
    assert_eq!(index.graphical_entry_counts(), vec![2]);
    assert_update_equals_rebuild(&store, &index);
    // Gained or lost a reference from the set: 2, 3, 4, 11 (and 50 itself).
    for gone in [3, 4, 11, 2] {
        assert!(disturbed.contains(&u(gone)), "{gone}: {disturbed:?}");
    }
    assert!(
        !disturbed.contains(&u(1)),
        "1 is targeted before and after: {disturbed:?}"
    );
}

/// An `Attachment` **added**: the table names it, the update disturbs the
/// object it names — and only that one.
#[test]
fn an_attachment_that_is_added_is_followed_and_disturbs_its_object() {
    let mut store = fabricated();
    let mut index = index_of(&store);
    assert_eq!(index.attachments_naming(&u(4)), Vec::<String>::new());

    attach(&mut store, 42, 4, IMAGE, PNG);
    let disturbed = index.update(&store, &[u(42)], store.revision());

    assert_eq!(index.attachments_naming(&u(4)), vec![u(42)]);
    assert_eq!(index.attachments_naming(&u(3)), vec![u(40)], "3 keeps its own");
    assert_update_equals_rebuild(&store, &index);
    assert!(
        disturbed.contains(&u(4)) && disturbed.contains(&u(42)),
        "{disturbed:?}"
    );
    assert!(!disturbed.contains(&u(3)), "{disturbed:?}");

    // And an attachment that leaves the package takes its row with it.
    store
        .remove(&format!("eml:///eml23.Attachment({})", u(42)))
        .expect("it goes out");
    let disturbed = index.update(&store, &[u(42)], store.revision());
    assert!(index.attachments_naming(&u(4)).is_empty());
    assert!(disturbed.contains(&u(4)));
    assert_update_equals_rebuild(&store, &index);
}

// -- the walks read rows, not edges ----------------------------------------------

/// A walk answers from the relation tables: **no reference is turned into a
/// `Pointer`** and no node is scanned — the work counters say so, and the
/// answer is the same as ever.
///
/// To break it: put `index.dependents_of(uuid)` back in `Walker::links_of` — the
/// count of edges read stops being zero.
#[test]
fn a_walk_reads_rows_and_never_turns_edges_into_pointers() {
    let store = fabricated();
    let index = index_of(&store);
    let before = index.traffic();
    let has = |identity: &str| (crate::inherit::uuid_of(identity) == u(10)).then_some("folder");
    let walked = Walker::new(&index, &store, &has).resolve(&u(3));
    let found = walked.resolution.expect("the folder 10 holds 3");
    assert_eq!((found.rank, found.carrier.as_str()), (2, u(10).as_str()));
    let after = index.traffic();
    assert_eq!(
        after.edges_read, before.edges_read,
        "no Pointer was built by the walk"
    );
    assert_eq!(
        after.nodes_scanned, before.nodes_scanned,
        "no node was scanned by the walk"
    );
    assert!(after.rows_read > before.rows_read, "the walk read rows");
}

// -- the attachments --------------------------------------------------------------

/// A package of `n` objects with one attachment on the first: the listing of that
/// object must not cost the package.
fn big(n: u32) -> EpcStore {
    let mut store = store();
    for i in 1..=n {
        feature(&mut store, i);
    }
    attach(&mut store, n + 1, 1, "resqml22.WellboreFeature", PNG);
    store
}

/// **`attachments_of` no longer walks the package.** It scanned every node of the
/// index (to find the `Attachment`s), and every reference of each: N nodes per
/// call. It reads a row.
///
/// To break it: put the `index.nodes()` scan back in `Reader::carries`, or in
/// `Reader::pieces_of` — `nodes_scanned` stops being zero and the test goes red.
#[test]
fn listing_what_is_attached_does_not_scan_the_package() {
    const N: u32 = 400;
    let store = big(N);
    let index = index_of(&store);
    let files = Files::read(&store);
    let before = index.traffic();

    let listings = attachments_of_with(&files, &store, &index, &[u(1)]);
    assert_eq!(listings[0].pieces.len(), 1, "the attachment is found");
    let all: Vec<String> = (1..=N).map(u).collect();
    let listings = attachments_of_with(&files, &store, &index, &all);
    assert_eq!(
        listings
            .iter()
            .filter(|listing| !listing.pieces.is_empty())
            .count(),
        1
    );

    let after = index.traffic();
    assert_eq!(
        after.nodes_scanned - before.nodes_scanned,
        0,
        "no node is scanned — the package holds {N}+"
    );
    assert_eq!(
        after.edges_read - before.edges_read,
        0,
        "no reference is spelt out"
    );
}

/// A listing is **kept**: asking again walks nothing, and what an update
/// disturbs drops exactly the listings that were read from it.
///
/// To break it: make `Cache::disturb` a `clear` — the listing of 4, which nothing
/// changed here touched, would go with the others and the count of dropped
/// listings would go from 1 to 2.
#[test]
fn a_listing_is_kept_and_dropped_by_what_it_was_read_from() {
    let mut store = fabricated();
    let mut index = index_of(&store);
    let mut cache = Cache::new();

    // 3 has its own attachment; 4 has none and inherits the feature's (41), by its
    // interpretation.
    let first = cache.listings(&store, store.files_revision(), &index, &[u(3), u(4)]);
    assert_eq!(
        (first[0].pieces.len(), first[0].pieces[0].provenance.kind),
        (1, "self")
    );
    assert_eq!(
        (first[1].pieces.len(), first[1].pieces[0].provenance.kind),
        (1, "parent")
    );
    assert_eq!(first[1].pieces[0].provenance.carrier, u(1));
    assert_eq!(
        cache.stats(),
        attachments::CacheStats {
            listings_built: 2,
            listings_dropped: 0,
            files_read: 1
        }
    );

    // Asked again: nothing is walked, nothing is listed again.
    let again = cache.listings(&store, store.files_revision(), &index, &[u(4), u(3), u(3)]);
    assert_eq!(again.len(), 3);
    assert_eq!(cache.stats().listings_built, 2);
    assert_eq!(cache.stats().files_read, 1);
    // And it is what the uncached door says.
    let direct = attachments::attachments_of(&store, &index, &[u(4), u(3), u(3)]);
    assert_eq!(again, direct);

    // An attachment comes to name 3: 3's listing goes, 4's — read from the
    // feature, the interpretation and their collections, never from 3 — stays.
    attach(&mut store, 42, 3, IMAGE, PNG);
    let disturbed = index.update(&store, &[u(42)], store.revision());
    assert_eq!(cache.disturb(&disturbed), 1);
    assert!(!cache.holds(&u(3)) && cache.holds(&u(4)));
    let listings = cache.listings(&store, store.files_revision(), &index, &[u(3), u(4)]);
    assert_eq!(listings[0].pieces.len(), 2, "3 lists its second attachment");
    assert_eq!(cache.stats().listings_built, 3, "only 3 was walked again");
    assert_eq!(
        listings,
        attachments::attachments_of(&store, &index, &[u(3), u(4)])
    );

    // The feature's attachment is replaced by another: 4 read the carrier and its
    // attachments, so 4 goes — 3 stays.
    attach(&mut store, 41, 1, "resqml22.WellboreFeature", b"another file");
    let disturbed = index.update(&store, &[u(41)], store.revision());
    assert_eq!(cache.disturb(&disturbed), 1);
    assert!(cache.holds(&u(3)) && !cache.holds(&u(4)));
    let listings = cache.listings(&store, store.files_revision(), &index, &[u(4)]);
    assert_eq!(listings[0].pieces[0].size, Some(b"another file".len() as u64));
}

/// The files are not objects: no journal names them, so they carry a stamp, and
/// a change of it drops **every** listing and lists the files again — a media
/// relation made after the listing is not served from before it.
///
/// To break it: do not compare `files_stamp` in `Cache::listings` — the listing of
/// 3 would stay at one piece after `relate_media`.
#[test]
fn a_change_of_the_packages_files_drops_every_listing() {
    let mut store = fabricated();
    let index = index_of(&store);
    let mut cache = Cache::new();
    let stamp = store.files_revision();
    let first = cache.listings(&store, stamp, &index, &[u(3), u(4)]);
    assert_eq!(first[0].pieces.len(), 1);

    store
        .set_attachment(
            "media/fiche.pdf",
            b"%PDF".to_vec(),
            Some("application/pdf".into()),
        )
        .unwrap();
    store.relate_media(&u(3), "media/fiche.pdf").unwrap();
    assert_ne!(
        store.files_revision(),
        stamp,
        "the store counts a change of its files"
    );

    let second = cache.listings(&store, store.files_revision(), &index, &[u(3), u(4)]);
    assert_eq!(
        second[0].pieces.len(),
        2,
        "the new part is listed: {:?}",
        second[0].pieces
    );
    assert_eq!(cache.stats().files_read, 2);
    assert_eq!(cache.stats().listings_dropped, 2, "both went");
    assert_eq!(second, attachments::attachments_of(&store, &index, &[u(3), u(4)]));
}

/// The stamp is moved by every door that changes a file, and by nothing that
/// does not: a read leaves it alone.
#[test]
fn the_files_stamp_moves_with_the_files_and_with_nothing_else() {
    let mut store = fabricated();
    let stamp = store.files_revision();
    let _ = store.media_links();
    let _ = store.parts();
    assert_eq!(store.files_revision(), stamp, "reading does not count");

    store
        .set_attachment("media/a.png", PNG.to_vec(), Some("image/png".into()))
        .unwrap();
    let after_set = store.files_revision();
    assert!(after_set > stamp);
    store.relate_media(&u(1), "media/a.png").unwrap();
    let after_relate = store.files_revision();
    assert!(after_relate > after_set);
    store.unrelate_media(&u(1), "media/a.png").unwrap();
    let after_unrelate = store.files_revision();
    assert!(after_unrelate > after_relate);
    store.remove_attachment("media/a.png").unwrap();
    assert!(store.files_revision() > after_unrelate);
}
