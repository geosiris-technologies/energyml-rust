//! The package index, confronted with the readers it replaces.
//!
//! ADR-0010. The index is only
//! worth having if it answers **exactly** what the direct reads answered, so
//! every test here sets it beside an independent reading of the same store:
//!
//! - its graph beside `graph::build` over the objects (what a front end's graph builder
//!   did), node for node and edge for edge;
//! - its property membership and sheets beside scanning every property with
//!   `property::read` (what `attributes` and `attributes_of_all` did);
//! - its dependents beside `graph::dependents`;
//! - a pass in slices of seven beside one pass;
//! - an `update` after an edit beside a rebuild from scratch.
//!
//! And **each confrontation is shown able to fail** (`…_can_fail`): an index
//! with one edge or one sheet forgotten must be reported different, otherwise
//! the rest of this file proves nothing (the rule of `tests/colouring.rs`).
//!
//! The corpus tests skip when `testdata/` is absent; the synthetic ones do not,
//! so this file never passes on empty sets alone.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use energyml_domain::package_index::{GraphAround, IndexBuilder, NodeState, PackageIndex, PropertySheet};
use energyml_domain::property::{self, Property};
use energyml_domain::values;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_storage::EpcStore;
use energyml_storage::graph::{self, Graph};

/// Beyond this many objects a package is skipped: the reference side of every
/// confrontation keeps all the objects alive - the very thing the index exists
/// not to do - and a debug build of a 77 000-object package does not fit.
const LARGEST: usize = 20_000;

// ---------------------------------------------------------------------------
// Corpus
// ---------------------------------------------------------------------------

/// The public reference packages; see `fixtures.rs`.
fn corpus() -> Vec<PathBuf> {
    crate::fixtures::public_corpus()
}

/// Every openable package of the corpus that is small enough, with its name.
fn packages() -> Vec<(String, EpcStore)> {
    let registry = Arc::new(energyml_schemas::all());
    let mut out = Vec::new();
    for path in corpus() {
        let name = path.display().to_string();
        let Ok(store) = EpcStore::open(&path, registry.clone()) else {
            eprintln!("skipped: {name} does not open");
            continue;
        };
        let count = store.resources().len();
        if count > LARGEST {
            eprintln!("skipped: {name} holds {count} objects, more than {LARGEST}");
            continue;
        }
        out.push((name, store));
    }
    if out.is_empty() {
        eprintln!("skipped: no EPC under testdata/");
    }
    out
}

// ---------------------------------------------------------------------------
// The references: what the index is confronted with
// ---------------------------------------------------------------------------

/// A front end's graph builder, line for line: every object that reads, handed to
/// `graph::build`.
fn expected_graph(store: &EpcStore) -> Graph {
    let summaries = store.resources();
    let mut read = Vec::new();
    for summary in &summaries {
        if let Ok(object) = store.get_object(&summary.uri) {
            read.push((summary, object));
        }
    }
    graph::build(
        read.iter()
            .map(|(summary, object)| (*summary, object.as_reflect())),
    )
}

/// The nodes sorted by uri, for comparing an updated index with a rebuilt one:
/// an update puts what it adds at the end, where a rebuild follows the
/// inventory (see `PackageIndex::update`). The edges are sorted already.
fn canonical(graph: &Graph) -> Graph {
    let mut graph = graph.clone();
    graph.nodes.sort_by(|a, b| a.uri.cmp(&b.uri));
    graph
}

/// Where two graphs first differ, or `None`.
fn difference(left: &Graph, right: &Graph) -> Option<String> {
    if left.nodes.len() != right.nodes.len() {
        return Some(format!(
            "{} nodes against {}",
            left.nodes.len(),
            right.nodes.len()
        ));
    }
    for (at, (a, b)) in left.nodes.iter().zip(&right.nodes).enumerate() {
        if a != b {
            return Some(format!("node {at}: {a:?} against {b:?}"));
        }
    }
    if left.edges.len() != right.edges.len() {
        return Some(format!(
            "{} edges against {}",
            left.edges.len(),
            right.edges.len()
        ));
    }
    for (at, (a, b)) in left.edges.iter().zip(&right.edges).enumerate() {
        if a != b {
            return Some(format!("edge {at}: {a:?} against {b:?}"));
        }
    }
    if left.to_json() != right.to_json() {
        return Some("same nodes and edges, different JSON".to_string());
    }
    None
}

/// One sheet against the `Property` it was taken from.
fn sheet_difference(
    sheet: &PropertySheet,
    read: &Property<'_>,
    object: &dyn energyml_core::value::Reflect,
) -> Option<String> {
    macro_rules! same {
        ($name:literal, $a:expr, $b:expr) => {
            if $a != $b {
                return Some(format!("{}: {:?} against {:?}", $name, $a, $b));
            }
        };
    }
    same!("uuid", &*sheet.uuid, read.uuid.as_str());
    same!("title", sheet.title.as_deref(), read.title.as_deref());
    same!(
        "qualified_type",
        &*sheet.qualified_type,
        read.qualified_type.as_str()
    );
    same!("indexable", sheet.indexable, read.indexable);
    same!(
        "value_count_per_element",
        &*sheet.value_count_per_element,
        read.value_count_per_element.as_slice()
    );
    same!("uom", sheet.uom.as_deref(), read.uom.as_deref());
    same!("kind", sheet.kind_ref(), read.kind);
    same!(
        "supporting_representation",
        sheet.supporting_representation.as_deref(),
        read.supporting_representation.as_deref()
    );
    same!("lookup", sheet.lookup.as_deref(), read.lookup.as_deref());
    same!("time", sheet.time.as_deref(), read.time.as_ref());
    same!("realization", sheet.realization, read.realization);
    same!(
        "has_realization_indices",
        sheet.has_realization_indices,
        read.has_realization_indices
    );
    let located = values::locate(object);
    same!(
        "values_path",
        sheet.values_path.as_deref(),
        located.first().map(|found| found.path.as_str())
    );
    same!(
        "values_per_element",
        sheet.values_per_element(),
        read.values_per_element()
    );
    None
}

/// The property membership and the sheets of `index`, against scanning every
/// property of `store` with `property::read` - the rule a front end's attribute listing
/// applied, and the one `attributes_of_all` applied once per package.
fn property_difference(store: &EpcStore, index: &PackageIndex) -> Option<String> {
    let mut expected: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut representations: BTreeSet<String> = BTreeSet::new();
    let mut read_count = 0usize;

    for summary in store.resources() {
        if summary.qualified_type.ends_with("Representation") {
            representations.insert(summary.uuid.to_ascii_lowercase());
        }
        if !summary.qualified_type.ends_with("Property") {
            continue;
        }
        let Ok(object) = store.get_object(&summary.uri) else {
            continue;
        };
        let Ok(read) = property::read(object.as_reflect()) else {
            continue;
        };
        read_count += 1;

        let Some(sheet) = index.sheet(&read.uuid) else {
            return Some(format!("{}: no sheet in the index", summary.uri));
        };
        if let Some(why) = sheet_difference(sheet, &read, object.as_reflect()) {
            return Some(format!("{}: sheet differs - {why}", summary.uri));
        }
        if let Some(support) = &read.supporting_representation {
            expected
                .entry(support.to_ascii_lowercase())
                .or_default()
                .insert(summary.uri.clone());
        }
    }
    if index.sheet_count() != read_count {
        return Some(format!(
            "{} sheets against {read_count} properties read",
            index.sheet_count()
        ));
    }

    // Every representation of the package, and every uuid a property names -
    // absent ones included, for the index answers those too.
    let mut askable = representations;
    askable.extend(expected.keys().cloned());
    for representation in askable {
        let got: BTreeSet<String> = index
            .properties_of(&representation)
            .map(|(uri, _)| uri.to_string())
            .collect();
        let want = expected.get(&representation).cloned().unwrap_or_default();
        if got != want {
            return Some(format!(
                "properties of {representation}: {got:?} against {want:?}"
            ));
        }
    }
    None
}

/// The sheets, by property uri, for comparing two indexes.
fn sheets_of(index: &PackageIndex) -> Vec<(String, PropertySheet)> {
    let mut out: Vec<(String, PropertySheet)> = index
        .properties()
        .map(|(uri, sheet)| (uri.to_string(), sheet.clone()))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Who references a uuid, from the objects themselves and from the index.
fn dependents_difference(store: &EpcStore, index: &PackageIndex, sample: usize) -> Option<String> {
    let summaries = store.resources();
    let mut read = Vec::new();
    for summary in &summaries {
        if let Ok(object) = store.get_object(&summary.uri) {
            read.push((summary, object));
        }
    }
    let step = (summaries.len() / sample.max(1)).max(1);
    for summary in summaries.iter().step_by(step) {
        let mut want: Vec<(String, String)> =
            graph::dependents(read.iter().map(|(s, o)| (*s, o.as_reflect())), &summary.uuid)
                .into_iter()
                .map(|p| (p.uri, p.path))
                .collect();
        let mut got: Vec<(String, String)> = index
            .dependents_of(&summary.uuid)
            .into_iter()
            .map(|p| (p.uri, p.path))
            .collect();
        want.sort();
        got.sort();
        if want != got {
            return Some(format!("dependents of {}: {got:?} against {want:?}", summary.uri));
        }
    }
    None
}

/// An index, judged against the store it claims to describe.
fn assert_index_is_right(label: &str, store: &EpcStore, index: &PackageIndex) {
    let expected = expected_graph(store);
    if let Some(why) = difference(&index.graph(), &expected) {
        panic!("{label}: the index's graph is not graph::build's - {why}");
    }
    if let Some(why) = property_difference(store, index) {
        panic!("{label}: {why}");
    }
}

// ---------------------------------------------------------------------------
// The corpus
// ---------------------------------------------------------------------------

#[test]
fn the_index_draws_the_graph_graph_build_draws() {
    let mut compared = 0usize;
    for (name, store) in packages() {
        let index = PackageIndex::build(&store, 1);
        let expected = expected_graph(&store);
        if let Some(why) = difference(&index.graph(), &expected) {
            panic!("{name}: {why}");
        }
        // The JSON is what the front end receives; the contract is byte for byte.
        assert_eq!(index.graph_json(), expected.to_json(), "{name}");
        assert!(
            index.is_complete(),
            "{name}: a finished pass over a whole store is complete"
        );
        assert_eq!(index.revision(), 1);
        compared += 1;
    }
    eprintln!("{compared} packages: graph equal to graph::build");
}

#[test]
fn the_properties_of_a_representation_are_the_ones_property_read_finds() {
    let mut properties = 0usize;
    for (name, store) in packages() {
        let index = PackageIndex::build(&store, 1);
        if let Some(why) = property_difference(&store, &index) {
            panic!("{name}: {why}");
        }
        properties += index.sheet_count();
    }
    eprintln!("{properties} property sheets confronted with property::read");
}

#[test]
fn the_dependents_are_the_ones_graph_dependents_finds() {
    for (name, store) in packages() {
        let index = PackageIndex::build(&store, 1);
        if let Some(why) = dependents_difference(&store, &index, 20) {
            panic!("{name}: {why}");
        }
    }
}

#[test]
fn a_pass_in_slices_of_seven_is_one_pass() {
    for (name, store) in packages() {
        let whole = PackageIndex::build(&store, 1);

        let mut builder = IndexBuilder::new(store.resources(), 1);
        let total = builder.total();
        let mut steps = 0usize;
        loop {
            let progress = builder.step(&store, 7);
            steps += 1;
            assert!(progress.done <= total);
            if progress.is_done() {
                break;
            }
            assert!(steps <= total / 7 + 2, "{name}: the pass does not advance");
        }
        let sliced = builder.finish();

        if let Some(why) = difference(&sliced.graph(), &whole.graph()) {
            panic!("{name}: slices differ from one pass - {why}");
        }
        assert_eq!(sheets_of(&sliced), sheets_of(&whole), "{name}");
        assert_eq!(sliced.edge_count(), whole.edge_count(), "{name}");
    }
}

/// An edit that changes what the index records - a title, which is on the node
/// and, for a property, on its sheet - then `update` of that one uuid.
#[test]
fn an_update_after_an_edit_is_a_rebuild() {
    let mut edited = 0usize;
    for (name, mut store) in packages() {
        let mut index = PackageIndex::build(&store, 1);
        let summaries = store.resources();
        let Some(target) = summaries
            .iter()
            .find(|s| s.qualified_type.ends_with("Property"))
            .or_else(|| summaries.first())
        else {
            continue;
        };
        let changed = {
            let Ok(object) = store.edit(&target.uri) else {
                eprintln!("skipped: {name}: {} cannot be edited", target.uri);
                continue;
            };
            if energyml_core::path::set(
                object,
                "Citation.Title",
                energyml_core::value::OwnedValue::Str("renamed by the test".to_string()),
            )
            .is_err()
            {
                eprintln!("skipped: {name}: {} has no settable title", target.uri);
                continue;
            }
            target.uuid.clone()
        };

        index.update(&store, std::slice::from_ref(&changed), 2);
        assert_eq!(index.revision(), 2);
        assert_eq!(
            index.node(&changed).and_then(|n| n.title.as_deref()),
            Some("renamed by the test"),
            "{name}: the node did not take the new title"
        );
        assert_index_is_right(&format!("{name} after the edit"), &store, &index);
        edited += 1;
    }
    eprintln!("{edited} packages edited and updated");
}

#[test]
fn the_corpus_confrontations_can_fail() {
    let mut sabotaged = 0usize;
    for (name, store) in packages() {
        let expected = expected_graph(&store);
        let Some(edge) = expected.edges.first() else {
            continue;
        };
        let Some(node) = expected.nodes.iter().find(|n| n.uri == edge.from) else {
            continue;
        };

        // SABOTAGE 1: the index forgets one reference. `difference` must say so;
        // if it does not, every graph test above proves nothing.
        let mut index = PackageIndex::build(&store, 1);
        assert!(
            index.sabotage_forget_an_edge(&node.uuid),
            "{name}: nothing to forget"
        );
        assert!(
            difference(&index.graph(), &expected).is_some(),
            "{name}: a forgotten edge was not noticed"
        );
        sabotaged += 1;
    }
    eprintln!("{sabotaged} packages sabotaged and caught");
}

// ---------------------------------------------------------------------------
// Synthetic packages: they do not depend on the corpus
// ---------------------------------------------------------------------------

const REP_A: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const REP_B: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const GHOST: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const KIND: &str = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
const P1: &str = "11111111-1111-4111-8111-111111111111";
const P2: &str = "22222222-2222-4222-8222-222222222222";
const P3: &str = "33333333-3333-4333-8333-333333333333";
const P4: &str = "44444444-4444-4444-8444-444444444444";
const P5: &str = "55555555-5555-4555-8555-555555555555";

const REPRESENTATION: &str = "resqml22.WellboreTrajectoryRepresentation";

fn dor(uuid: &str, qualified_type: &str, title: &str) -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: uuid.to_string(),
        qualified_type: qualified_type.to_string(),
        title: title.to_string(),
        ..Default::default()
    }
}

fn citation(title: &str) -> eml23::Citation {
    eml23::Citation {
        title: title.to_string(),
        ..Default::default()
    }
}

fn representation(uuid: &str, title: &str) -> resqml22::WellboreTrajectoryRepresentation {
    resqml22::WellboreTrajectoryRepresentation {
        uuid: uuid.to_string(),
        citation: citation(title),
        ..Default::default()
    }
}

/// A continuous property on `support`, naming one kind (an object the package
/// never holds, and that the standard publishes in a catalogue).
fn property_on(uuid: &str, support: &str, title: &str) -> resqml22::ContinuousProperty {
    resqml22::ContinuousProperty {
        uuid: uuid.to_string(),
        citation: citation(title),
        indexable_element: eml23::IndexableElement::Cells,
        value_count_per_indexable_element: vec![1],
        property_kind: dor(KIND, "eml23.PropertyKind", "length"),
        supporting_representation: dor(support, REPRESENTATION, "a representation"),
        values_for_patch: vec![resqml22::AbstractValueArray::IntegerConstantArray(
            eml23::IntegerConstantArray {
                value: 1,
                count: 3,
                ..Default::default()
            },
        )],
        ..Default::default()
    }
}

fn empty_store() -> EpcStore {
    let bytes = energyml_storage::extract::empty_package(None).expect("an empty package is written");
    EpcStore::from_reader(
        Box::new(std::io::Cursor::new(bytes)),
        "built.epc",
        Arc::new(energyml_schemas::all()),
    )
    .expect("it opens")
}

fn put(store: &mut EpcStore, object: impl energyml_core::registry::DynObject + 'static) {
    store.put(Box::new(object)).expect("the package takes it");
}

fn uri_of(store: &EpcStore, uuid: &str) -> String {
    store
        .find_by_uuid(uuid)
        .expect("the object is in the store")
        .uri
        .clone()
}

/// Two representations, four properties - one of them on a representation the
/// package does not hold.
fn package() -> EpcStore {
    let mut store = empty_store();
    put(&mut store, representation(REP_A, "rep a"));
    put(&mut store, representation(REP_B, "rep b"));
    put(&mut store, property_on(P1, REP_A, "p1"));
    put(&mut store, property_on(P2, REP_A, "p2"));
    put(&mut store, property_on(P3, REP_B, "p3"));
    put(&mut store, property_on(P4, GHOST, "p4"));
    store
}

fn uris(index: &PackageIndex, representation: &str) -> BTreeSet<String> {
    index
        .properties_of(representation)
        .map(|(uri, _)| uri.to_string())
        .collect()
}

/// What an update must be: the graph of a rebuild from scratch and of
/// `graph::build` - nodes compared sorted - and the same sheets.
fn assert_update_is_a_rebuild(label: &str, store: &EpcStore, updated: &PackageIndex) {
    let rebuilt = PackageIndex::build(store, 0);
    if let Some(why) = difference(&canonical(&updated.graph()), &canonical(&rebuilt.graph())) {
        panic!("{label}: the updated index is not the rebuilt one - {why}");
    }
    // …and the rebuild is not judged by itself: graph::build, which shares no
    // code with the index, is the second witness.
    if let Some(why) = difference(&canonical(&updated.graph()), &canonical(&expected_graph(store))) {
        panic!("{label}: the updated index is not graph::build's - {why}");
    }
    assert_eq!(sheets_of(updated), sheets_of(&rebuilt), "{label}: sheets");
    if let Some(why) = property_difference(store, updated) {
        panic!("{label}: {why}");
    }
    assert_eq!(updated.edge_count(), rebuilt.edge_count(), "{label}: edge count");
}

#[test]
fn the_synthetic_package_is_indexed_like_graph_build_draws_it() {
    let store = package();
    let index = PackageIndex::build(&store, 7);

    assert_index_is_right("synthetic", &store, &index);
    assert_eq!(index.revision(), 7);
    assert_eq!(index.sheet_count(), 4);

    // The membership rule, read off the properties' own field.
    assert_eq!(
        uris(&index, REP_A),
        BTreeSet::from([uri_of(&store, P1), uri_of(&store, P2)])
    );
    assert_eq!(uris(&index, REP_B), BTreeSet::from([uri_of(&store, P3)]));
    // A representation the package does not hold still has its properties, and
    // its node says it is not here.
    assert_eq!(uris(&index, GHOST), BTreeSet::from([uri_of(&store, P4)]));
    assert_eq!(index.node(GHOST).map(|n| n.state), Some(NodeState::Absent));
    assert_eq!(index.node(REP_A).map(|n| n.state), Some(NodeState::Present));
    // The kind is referenced by four properties and held by none: one node,
    // marked as the catalogue's.
    let kind = index.node(KIND).expect("the kind has a node");
    assert_eq!(kind.state, NodeState::Absent);
    assert!(kind.externally_defined);
    assert_eq!(index.dependents_of(KIND).len(), 4);

    // The sheet says what property::read says, without the object.
    let sheet = index.sheet(P1).expect("a sheet");
    assert_eq!(sheet.values_path.as_deref(), Some("ValuesForPatch[0]"));
    assert_eq!(sheet.supporting_representation.as_deref(), Some(REP_A));
    assert_eq!(sheet.values_per_element(), 1);

    // A representation is not a property, and has no sheet.
    assert!(index.sheet(REP_A).is_none());
    // references_of is the cheap direction.
    let refs: BTreeSet<String> = index.references_of(P1).into_iter().map(|p| p.uuid).collect();
    assert_eq!(refs, BTreeSet::from([REP_A.to_string(), KIND.to_string()]));
}

#[test]
fn a_pass_in_slices_of_seven_is_one_pass_on_the_synthetic_package() {
    let store = package();
    let whole = PackageIndex::build(&store, 1);
    let mut builder = IndexBuilder::new(store.resources(), 1);
    assert_eq!(builder.progress().done, 0);
    let mut last = 0;
    while !builder.step(&store, 2).is_done() {
        let done = builder.progress().done;
        assert!(done > last, "a slice reads something");
        last = done;
    }
    let sliced = builder.finish();
    assert!(difference(&sliced.graph(), &whole.graph()).is_none());
    assert_eq!(sheets_of(&sliced), sheets_of(&whole));
}

#[test]
fn an_update_follows_a_reference_that_moves_leaves_and_arrives() {
    let mut store = package();
    let mut index = PackageIndex::build(&store, 1);
    assert_update_is_a_rebuild("initial", &store, &index);

    // 1. A reference MOVES: P2 leaves REP_A for REP_B.
    put(&mut store, property_on(P2, REP_B, "p2"));
    index.update(&store, &[P2.to_string()], 2);
    assert_update_is_a_rebuild("after P2 moved", &store, &index);
    assert_eq!(uris(&index, REP_A), BTreeSet::from([uri_of(&store, P1)]));
    assert_eq!(
        uris(&index, REP_B),
        BTreeSet::from([uri_of(&store, P2), uri_of(&store, P3)])
    );

    // 2. An object LEAVES while it is referenced: REP_A goes, P1 still names it.
    // It does not vanish - it becomes the absent node a rebuild would draw.
    let uri_a = uri_of(&store, REP_A);
    store.remove(&uri_a).expect("it is removed");
    index.update(&store, &[REP_A.to_string()], 3);
    assert_update_is_a_rebuild("after REP_A left", &store, &index);
    assert_eq!(index.node(REP_A).map(|n| n.state), Some(NodeState::Absent));
    assert_eq!(uris(&index, REP_A), BTreeSet::from([uri_of(&store, P1)]));

    // 3. The last reference to an absent node goes: P4 leaves GHOST, and GHOST
    // leaves the graph with it (a node that is only a name exists while it is
    // named).
    put(&mut store, property_on(P4, REP_B, "p4"));
    index.update(&store, &[P4.to_string()], 4);
    assert_update_is_a_rebuild("after P4 left GHOST", &store, &index);
    assert!(index.node(GHOST).is_none(), "nothing names GHOST any more");

    // 4. An object ARRIVES, naming the absent REP_A; then REP_A comes back and
    // the absent node becomes the present one.
    put(&mut store, property_on(P5, REP_A, "p5"));
    index.update(&store, &[P5.to_string()], 5);
    assert_update_is_a_rebuild("after P5 arrived", &store, &index);
    put(&mut store, representation(REP_A, "rep a again"));
    index.update(&store, &[REP_A.to_string()], 6);
    assert_update_is_a_rebuild("after REP_A came back", &store, &index);
    assert_eq!(index.node(REP_A).map(|n| n.state), Some(NodeState::Present));
    assert_eq!(
        index.node(REP_A).and_then(|n| n.title.as_deref()),
        Some("rep a again")
    );
    assert_eq!(index.revision(), 6);

    // 5. The FIRST referrer of a shared absent node leaves. The kind's type and
    // title were supplied by P1's reference; P2 (and the others) still name it,
    // so the node stays and takes its label from the next.
    let uri_p1 = uri_of(&store, P1);
    store.remove(&uri_p1).expect("it is removed");
    index.update(&store, &[P1.to_string()], 7);
    assert_update_is_a_rebuild("after P1 left", &store, &index);
    assert!(index.sheet(P1).is_none());
    assert_eq!(index.node(KIND).map(|n| n.state), Some(NodeState::Absent));

    // 6. Everything that named the kind goes; the kind goes.
    for uuid in [P2, P3, P4, P5] {
        let uri = uri_of(&store, uuid);
        store.remove(&uri).expect("it is removed");
    }
    index.update(
        &store,
        &[P2.to_string(), P3.to_string(), P4.to_string(), P5.to_string()],
        8,
    );
    assert_update_is_a_rebuild("after the last property left", &store, &index);
    assert!(index.node(KIND).is_none());
    assert_eq!(index.sheet_count(), 0);
}

#[test]
fn a_changed_uuid_the_index_never_saw_is_added() {
    let mut store = package();
    let mut index = PackageIndex::build(&store, 1);
    put(
        &mut store,
        representation("eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee", "brand new"),
    );
    put(
        &mut store,
        property_on(
            "66666666-6666-4666-8666-666666666666",
            "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee",
            "p6",
        ),
    );
    index.update(
        &store,
        &[
            "EEEEEEEE-EEEE-4EEE-8EEE-EEEEEEEEEEEE".to_string(), // any case
            "66666666-6666-4666-8666-666666666666".to_string(),
        ],
        2,
    );
    assert_update_is_a_rebuild("after two arrivals", &store, &index);
    assert_eq!(uris(&index, "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee").len(), 1);
}

#[test]
fn the_synthetic_confrontations_can_fail() {
    let store = package();
    let expected = expected_graph(&store);

    // SABOTAGE 1: forget a reference of P1. Its edges are sorted by target id,
    // and REP_A was made before the absent kind, so the edge lost is P1 → REP_A:
    // both the graph and the membership of REP_A must notice.
    let mut index = PackageIndex::build(&store, 1);
    assert!(index.sabotage_forget_an_edge(P1));
    assert!(
        difference(&index.graph(), &expected).is_some(),
        "a forgotten edge must make the graph differ from graph::build"
    );
    assert!(
        property_difference(&store, &index).is_some(),
        "a forgotten support edge must make REP_A lose a property"
    );
    assert!(dependents_difference(&store, &index, 100).is_some());

    // SABOTAGE 2: forget a sheet. The graph is untouched, the properties are not.
    let mut index = PackageIndex::build(&store, 1);
    assert!(index.sabotage_forget_a_sheet(P3));
    assert!(difference(&index.graph(), &expected).is_none());
    assert!(
        property_difference(&store, &index).is_some(),
        "a forgotten sheet must be reported"
    );

    // SABOTAGE 3: an update that is told nothing. After an edit that moves a
    // reference, an index not updated must be reported different from a rebuild
    // - otherwise `assert_update_is_a_rebuild` could pass on an index that
    // never changes.
    let mut store = package();
    let stale = PackageIndex::build(&store, 1);
    put(&mut store, property_on(P2, REP_B, "p2"));
    assert!(
        difference(&canonical(&stale.graph()), &canonical(&expected_graph(&store))).is_some(),
        "an index that missed an edit must differ from graph::build"
    );
    assert!(property_difference(&store, &stale).is_some());
}

// ---------------------------------------------------------------------------
// A copy of the graph brought up to date by `graph_around`
// ---------------------------------------------------------------------------

/// What a copy of the graph asks after an edit of `changed` (uuids): the uris it
/// held for them, the uris the store holds for them now, and the objects they
/// pointed at - a front ender's request (`patchGraph`).
fn asked_after(old: &Graph, store: &EpcStore, changed: &[&str]) -> Vec<String> {
    let mut asked: Vec<String> = Vec::new();
    for uuid in changed {
        let held: Vec<String> = old
            .nodes
            .iter()
            .filter(|n| n.uuid.eq_ignore_ascii_case(uuid))
            .map(|n| n.uri.clone())
            .collect();
        for uri in &held {
            asked.extend(old.edges.iter().filter(|e| &e.from == uri).map(|e| e.to.clone()));
        }
        asked.extend(held);
        if let Some(summary) = store.find_by_uuid(uuid) {
            asked.push(summary.uri.clone());
        }
    }
    asked.sort();
    asked.dedup();
    asked
}

/// The copy, patched as `GraphAround` says it must be. `skip_gone` is the
/// sabotage: a copy that keeps what the graph no longer shows.
fn patched(old: &Graph, around: &GraphAround, asked: &[String], skip_gone: bool) -> Graph {
    let dropped: BTreeSet<&str> = asked.iter().chain(&around.sources).map(String::as_str).collect();
    let gone: BTreeSet<&str> = if skip_gone {
        BTreeSet::new()
    } else {
        around.gone.iter().map(String::as_str).collect()
    };
    let mut nodes: Vec<graph::Node> = old
        .nodes
        .iter()
        .filter(|n| !gone.contains(n.uri.as_str()))
        .cloned()
        .collect();
    for node in &around.graph.nodes {
        match nodes.iter_mut().find(|n| n.uri == node.uri) {
            Some(held) => *held = node.clone(),
            None => nodes.push(node.clone()),
        }
    }
    let mut edges: Vec<graph::Edge> = old
        .edges
        .iter()
        .filter(|e| !dropped.contains(e.from.as_str()))
        .cloned()
        .collect();
    edges.extend(around.graph.edges.iter().cloned());
    Graph { nodes, edges }
}

/// Nodes by uri, edges by `(from, to, path)`: a patched copy appends.
fn sorted(graph: &Graph) -> Graph {
    let mut graph = canonical(graph);
    graph
        .edges
        .sort_by(|a, b| (&a.from, &a.to, &a.path).cmp(&(&b.from, &b.to, &b.path)));
    graph
}

#[test]
fn a_copy_patched_around_the_changed_objects_is_the_whole_graph() {
    let mut store = package();
    let mut index = PackageIndex::build(&store, 1);
    let mut revision = 1;
    // Every move of `an_update_follows_a_reference_that_moves_leaves_and_arrives`,
    // and a creation that links to a held object - the case of 08/10.
    let mut step = |store: &mut EpcStore,
                    index: &mut PackageIndex,
                    label: &str,
                    edit: &dyn Fn(&mut EpcStore),
                    changed: &[&str]| {
        let old = index.graph();
        edit(store);
        revision += 1;
        index.update(
            store,
            &changed.iter().map(|u| u.to_string()).collect::<Vec<_>>(),
            revision,
        );
        let asked = asked_after(&old, store, changed);
        let around = index.graph_around(&asked);
        let copy = patched(&old, &around, &asked, false);
        if let Some(why) = difference(&sorted(&copy), &sorted(&index.graph())) {
            panic!("{label}: the patched copy is not the graph - {why}");
        }
        (old, around, asked)
    };
    step(
        &mut store,
        &mut index,
        "a creation on REP_B",
        &|s| {
            put(
                s,
                property_on("66666666-6666-4666-8666-666666666666", REP_B, "p6"),
            )
        },
        &["66666666-6666-4666-8666-666666666666"],
    );
    step(
        &mut store,
        &mut index,
        "P2 moves",
        &|s| put(s, property_on(P2, REP_B, "p2")),
        &[P2],
    );
    step(
        &mut store,
        &mut index,
        "REP_A leaves",
        &|s| {
            let uri = uri_of(s, REP_A);
            s.remove(&uri).expect("removed");
        },
        &[REP_A],
    );
    let (old, around, asked) = step(
        &mut store,
        &mut index,
        "the last reference to GHOST goes",
        &|s| put(s, property_on(P4, REP_B, "p4")),
        &[P4],
    );
    // The control: a copy that ignores `gone` keeps GHOST, and is caught.
    assert!(
        around.gone.iter().any(|uri| uri.contains(GHOST)),
        "GHOST is said gone: {:?}",
        around.gone
    );
    assert!(
        difference(
            &sorted(&patched(&old, &around, &asked, true)),
            &sorted(&index.graph())
        )
        .is_some(),
        "a copy that keeps a node the graph dropped must be caught"
    );
    step(
        &mut store,
        &mut index,
        "P5 arrives naming the absent REP_A",
        &|s| put(s, property_on(P5, REP_A, "p5")),
        &[P5],
    );
    step(
        &mut store,
        &mut index,
        "REP_A comes back",
        &|s| put(s, representation(REP_A, "rep a again")),
        &[REP_A],
    );
    step(
        &mut store,
        &mut index,
        "P1 leaves",
        &|s| {
            let uri = uri_of(s, P1);
            s.remove(&uri).expect("removed");
        },
        &[P1],
    );
}

#[test]
fn an_unfinished_pass_does_not_claim_to_be_complete() {
    let store = package();
    let mut builder = IndexBuilder::new(store.resources(), 1);
    builder.step(&store, 1);
    let partial = builder.finish();
    assert!(!partial.is_complete());

    let mut complete = PackageIndex::build(&store, 1);
    assert!(complete.is_complete());
    // An ETP connection says so itself (§7).
    complete.mark_incomplete();
    assert!(!complete.is_complete());
}
