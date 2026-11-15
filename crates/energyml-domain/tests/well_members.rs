//! Who a wellbore has - `energyml_domain::well_members` (ADR 0040).
//!
//! The walk is a pure function of the inventory and the references, so the
//! first three tests feed it facts written by hand (four wells: a complete one
//! in RESQML 2.2, a WITSML one named by a feature, one with no representation,
//! a trajectory with no well). The fourth is the triangulation: an index built
//! over an in-memory package on one side, the same facts typed by hand on the
//! other, and the walk of the index must be the walk of the facts.
//!
//! The shape of the model is the standards': a well representation represents
//! an interpretation, which interprets a feature (RESQML TRG 2.2 chapter on
//! wells); a WITSML trajectory reaches its well by the wellbore a feature names
//! (`WellboreFeature.WitsmlWellbore`).

use std::cell::Cell;
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

use energyml_core::registry::DynObject;
use energyml_domain::package_index::PackageIndex;
use energyml_domain::well_members::{ObjectFact, WellMembers, facts_of_index, well_members, well_members_of_index};
use energyml_resqml_2_2 as resqml22;
use energyml_storage::EpcStore;

use crate::inherit::{citation, dor, u};

// -- facts written by hand ---------------------------------------------------

fn fact(uri: &str, qualified_type: &str, title: &str) -> ObjectFact {
    ObjectFact {
        uri: uri.to_string(),
        qualified_type: qualified_type.to_string(),
        title: title.to_string(),
    }
}

/// The references, by source uri.
fn edges(pairs: &[(&str, Vec<&str>)]) -> HashMap<String, Vec<String>> {
    pairs
        .iter()
        .map(|(from, to)| (from.to_string(), to.iter().map(|t| t.to_string()).collect()))
        .collect()
}

fn walk(objects: &[ObjectFact], references: &HashMap<String, Vec<String>>) -> Vec<WellMembers> {
    well_members(objects, &|uri| references.get(uri).cloned().unwrap_or_default())
}

fn well<'a>(wells: &'a [WellMembers], key: &str) -> &'a WellMembers {
    wells
        .iter()
        .find(|w| w.key == key)
        .unwrap_or_else(|| panic!("no entry for {key}: {:?}", wells.iter().map(|w| &w.key).collect::<Vec<_>>()))
}

fn uris(members: &[energyml_domain::well_members::Member]) -> Vec<&str> {
    members.iter().map(|m| m.uri.as_str()).collect()
}

/// The four wells of the grid test of the desktop: complete in 2.2; WITSML
/// named by a feature; no representation; a trajectory alone.
fn four_wells() -> (Vec<ObjectFact>, HashMap<String, Vec<String>>) {
    let objects = vec![
        // W1, complete.
        fact("f1", "resqml22.WellboreFeature", "Well one"),
        fact("i1", "resqml22.WellboreInterpretation", "interpretation one"),
        fact("t1", "resqml22.WellboreTrajectoryRepresentation", "trajectory one"),
        fact("fr1", "resqml22.WellboreFrameRepresentation", "frame one"),
        fact("fr2", "resqml22.WellboreFrameRepresentation", "frame two"),
        fact("m1", "resqml22.WellboreMarkerFrameRepresentation", "tops"),
        fact("p1", "resqml22.ContinuousProperty", "porosity"),
        fact("p2", "resqml22.CategoricalProperty", "facies"),
        fact("l1", "eml23.ColumnBasedTable", "facies table"),
        // A property on nothing a well holds.
        fact("p9", "resqml22.ContinuousProperty", "stray"),
        // W2, WITSML: the feature names the wellbore the trajectory is of.
        fact("f2", "resqml22.WellboreFeature", "Well two"),
        fact("wb2", "witsml21.Wellbore", "wellbore two"),
        fact("wt2", "witsml21.Trajectory", "witsml trajectory"),
        // W3, nothing at all.
        fact("f3", "resqml22.WellboreFeature", "Well three"),
        // W4, a trajectory that reaches no well.
        fact("t4", "resqml22.WellboreTrajectoryRepresentation", "Lone"),
        // Not part of the walk.
        fact("model", "resqml22.Model", "the model"),
    ];
    let references = edges(&[
        ("i1", vec!["f1"]),
        ("t1", vec!["i1"]),
        ("fr1", vec!["i1", "t1"]),
        ("fr2", vec!["i1", "t1"]),
        ("m1", vec!["i1"]),
        ("p1", vec!["fr1"]),
        ("p2", vec!["fr2", "l1"]),
        ("p9", vec!["l1"]),
        ("f2", vec!["wb2"]),
        ("wt2", vec!["wb2"]),
    ]);
    (objects, references)
}

#[test]
fn every_well_is_an_entry_whatever_it_holds() {
    let (objects, references) = four_wells();
    let wells = walk(&objects, &references);

    assert_eq!(wells.len(), 4, "{wells:?}");
    // Sorted by lower-case title, then key.
    assert_eq!(
        wells.iter().map(|w| w.title.as_str()).collect::<Vec<_>>(),
        ["Lone", "Well one", "Well three", "Well two"]
    );

    let w1 = well(&wells, "f1");
    assert_eq!(uris(&w1.trajectories), ["t1"]);
    assert_eq!(uris(&w1.frames), ["fr1", "fr2"]);
    assert_eq!(uris(&w1.markers), ["m1"]);
    assert_eq!(uris(&w1.properties), ["p1", "p2"]);
    assert_eq!(uris(&w1.categorical), ["p2"]);
    assert_eq!(w1.interpretations, ["i1"]);
    assert_eq!(w1.versions, ["resqml22"]);
    assert_eq!(w1.title, "Well one");

    // The WITSML trajectory joins the feature that names its wellbore.
    let w2 = well(&wells, "f2");
    assert_eq!(uris(&w2.trajectories), ["wt2"]);
    assert_eq!(w2.versions, ["witsml21"]);

    // A well with no representation is still a row.
    let w3 = well(&wells, "f3");
    assert!(w3.trajectories.is_empty() && w3.frames.is_empty() && w3.markers.is_empty());
    assert!(w3.properties.is_empty() && w3.categorical.is_empty() && w3.interpretations.is_empty());
    assert!(w3.versions.is_empty());

    // A trajectory that reaches no well makes its own entry, titled by itself.
    let w4 = well(&wells, "t4");
    assert_eq!(uris(&w4.trajectories), ["t4"]);
    assert_eq!(w4.title, "Lone");

    // The stray property is nobody's.
    assert!(wells.iter().all(|w| !uris(&w.properties).contains(&"p9")));
}

#[test]
fn a_release_2_0_1_type_is_the_same_concept() {
    let objects = vec![
        fact("f", "resqml20.obj_WellboreFeature", "old well"),
        fact("i", "resqml20.obj_WellboreInterpretation", "old interpretation"),
        fact("fr", "resqml20.obj_WellboreFrameRepresentation", "old frame"),
    ];
    let references = edges(&[("i", vec!["f"]), ("fr", vec!["i"])]);
    let wells = walk(&objects, &references);

    assert_eq!(wells.len(), 1, "{wells:?}");
    assert_eq!(wells[0].key, "f");
    assert_eq!(uris(&wells[0].frames), ["fr"]);
    assert_eq!(wells[0].versions, ["resqml20"]);
}

#[test]
fn outgoing_is_asked_once_per_relevant_object() {
    let (objects, references) = four_wells();
    let asked = Cell::new(0usize);
    let model_asked = Cell::new(false);
    let wells = well_members(&objects, &|uri| {
        asked.set(asked.get() + 1);
        if uri == "model" {
            model_asked.set(true);
        }
        references.get(uri).cloned().unwrap_or_default()
    });

    let relevant = objects
        .iter()
        .filter(|f| energyml_domain::well_members::relevant(f.qualified_type.rsplit('.').next().unwrap()))
        .count();
    assert_eq!(relevant, objects.len() - 1, "only the model is not part of the walk");
    assert_eq!(asked.get(), relevant);
    assert!(!model_asked.get(), "an object outside the walk is never asked");
    assert_eq!(wells, walk(&objects, &references), "asking once changes no answer");
}

// -- the index, and the facts typed by hand ----------------------------------

fn store() -> EpcStore {
    EpcStore::from_reader(
        Box::new(Cursor::new(
            energyml_storage::extract::empty_package(None).expect("an empty package"),
        )),
        "well_members.epc",
        Arc::new(energyml_schemas::all()),
    )
    .expect("it opens")
}

fn put(store: &mut EpcStore, object: impl DynObject + 'static) {
    store.put(Box::new(object)).expect("it goes in");
}

fn uri_of(store: &EpcStore, n: u32) -> String {
    store.find_by_uuid(&u(n)).expect("the object is in the store").uri.clone()
}

#[test]
fn the_walk_over_an_index_is_the_walk_over_its_facts() {
    let mut store = store();
    put(
        &mut store,
        resqml22::WellboreFeature {
            uuid: u(1),
            schema_version: "2.2".into(),
            citation: citation("a well"),
            ..Default::default()
        },
    );
    put(
        &mut store,
        resqml22::WellboreInterpretation {
            uuid: u(2),
            schema_version: "2.2".into(),
            citation: citation("its interpretation"),
            interpreted_feature: dor(&u(1), "resqml22.WellboreFeature"),
            ..Default::default()
        },
    );
    put(
        &mut store,
        resqml22::WellboreFrameRepresentation {
            uuid: u(3),
            schema_version: "2.2".into(),
            citation: citation("its frame"),
            represented_object: Some(dor(&u(2), "resqml22.WellboreInterpretation")),
            // The trajectory is not in the package: an absent node, not a member.
            trajectory: dor(&u(4), "resqml22.WellboreTrajectoryRepresentation"),
            ..Default::default()
        },
    );
    let index = PackageIndex::build(&store, 1);

    // The facts, typed by hand from the standards' names: only `Present`
    // objects, the absent trajectory is not among them.
    let by_hand = vec![
        fact(&uri_of(&store, 1), "resqml22.WellboreFeature", "a well"),
        fact(&uri_of(&store, 2), "resqml22.WellboreInterpretation", "its interpretation"),
        fact(&uri_of(&store, 3), "resqml22.WellboreFrameRepresentation", "its frame"),
    ];
    let mut facts = facts_of_index(&index);
    facts.sort_by(|a, b| a.uri.cmp(&b.uri));
    let mut expected = by_hand.clone();
    expected.sort_by(|a, b| a.uri.cmp(&b.uri));
    assert_eq!(facts, expected, "an absent object is no fact");

    let references: HashMap<String, Vec<String>> = HashMap::from([
        (uri_of(&store, 2), vec![uri_of(&store, 1)]),
        (uri_of(&store, 3), vec![uri_of(&store, 2), "absent-trajectory".to_string()]),
    ]);
    let by_facts = walk(&by_hand, &references);
    let over_index = well_members_of_index(&index);

    assert_eq!(over_index, by_facts);
    assert_eq!(over_index.len(), 1, "{over_index:?}");
    assert_eq!(over_index[0].key, uri_of(&store, 1));
    assert_eq!(uris(&over_index[0].frames), [uri_of(&store, 3).as_str()]);
    assert_eq!(over_index[0].interpretations, [uri_of(&store, 2)]);
}
