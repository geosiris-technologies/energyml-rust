//! The package index over a package that holds **one uuid more than once** -
//! (ADR-0010, ADR-0003). An entry is
//! (uuid, type, `objectVersion`), so the index has a node per entry, reads each
//! by the uri that names it alone (the bare uuid is ambiguous in the store), and
//! sends a reference to the node it names.
//!
//! Every assertion sets the index beside `graph::build` over the objects, the
//! confrontation `package_index.rs` makes for every package - two readers of one
//! fact, written independently. The package is fabricated here, zip and all.
//!
//! What would have to break: `read_into` asking the store for the bare uuid
//! (both nodes come back unreadable, *names 2 objects*), or a reference to
//! version 2 drawn to version 1 as well.

use std::io::Cursor;
use std::sync::Arc;

use energyml_domain::package_index::{NodeState, PackageIndex};
use energyml_storage::EpcStore;
use energyml_storage::graph;

const RESQML: &str = "http://www.energistics.org/energyml/data/resqmlv2";
const EML: &str = "http://www.energistics.org/energyml/data/commonv2";
const S: &str = "7f000000-0000-4000-8000-000000000001";
const H: &str = "7f000000-0000-4000-8000-0000000000a1";
const H0: &str = "7f000000-0000-4000-8000-0000000000a2";
const CT: &str = "application/x-resqml+xml;version=2.2;type=BoundaryFeature";
const CT_H: &str = "application/x-resqml+xml;version=2.2;type=HorizonInterpretation";

fn feature(version: Option<&str>) -> String {
    let version = version
        .map(|v| format!(" objectVersion=\"{v}\""))
        .unwrap_or_default();
    format!(
        "<resqml:BoundaryFeature xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" uuid=\"{S}\" \
         schemaVersion=\"2.2\"{version}><eml:Citation><eml:Title>a surface</eml:Title>\
         <eml:Originator>test</eml:Originator><eml:Creation>2026-10-05T00:00:00Z</eml:Creation>\
         <eml:Format>test</eml:Format></eml:Citation><resqml:IsWellKnown>false</resqml:IsWellKnown>\
         </resqml:BoundaryFeature>"
    )
}

fn horizon(uuid: &str, version: Option<&str>) -> String {
    let version = version
        .map(|v| format!("<eml:ObjectVersion>{v}</eml:ObjectVersion>"))
        .unwrap_or_default();
    format!(
        "<resqml:HorizonInterpretation xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" uuid=\"{uuid}\" \
         schemaVersion=\"2.2\"><eml:Citation><eml:Title>a horizon</eml:Title>\
         <eml:Originator>test</eml:Originator><eml:Creation>2026-10-05T00:00:00Z</eml:Creation>\
         <eml:Format>test</eml:Format></eml:Citation><resqml:Domain>depth</resqml:Domain>\
         <resqml:InterpretedFeature><eml:Uuid>{S}</eml:Uuid>{version}\
         <eml:QualifiedType>resqml22.BoundaryFeature</eml:QualifiedType><eml:Title>a surface</eml:Title>\
         </resqml:InterpretedFeature></resqml:HorizonInterpretation>"
    )
}

/// An object from its XML, read tolerantly (the fixtures are short).
fn object(content_type: &str, xml: String) -> Box<dyn energyml_core::registry::DynObject> {
    energyml_schemas::all()
        .read_xml_as(
            content_type,
            xml.as_bytes(),
            energyml_core::registry::ReadMode::Tolerant,
        )
        .expect("readable")
        .0
}

/// The package: the surface in two versions, put through the two gates, written,
/// and **read back** - so that the index meets entries of an archive.
fn package() -> EpcStore {
    let registry = Arc::new(energyml_schemas::all());
    let empty = energyml_storage::extract::empty_package(Some(0)).expect("an empty package");
    let mut store =
        EpcStore::from_reader(Box::new(Cursor::new(empty)), "versions.epc", registry.clone()).expect("opens");
    store.put(object(CT, feature(None))).expect("the original");
    store
        .put_as_new_version(object(CT, feature(Some("2"))))
        .expect("a second version");
    store.put(object(CT_H, horizon(H, Some("2")))).expect("a horizon");
    store.put(object(CT_H, horizon(H0, None))).expect("another");
    let mut bytes = Cursor::new(Vec::new());
    store
        .write_to(&mut bytes, &energyml_storage::epc::SaveOptions::default())
        .expect("written");
    EpcStore::from_reader(
        Box::new(Cursor::new(bytes.into_inner())),
        "versions.epc",
        registry,
    )
    .expect("reads back")
}

/// The reference side: `graph::build` over every object the store reads.
fn expected(store: &EpcStore) -> graph::Graph {
    let read = store.read_objects(None).0;
    graph::build(
        read.iter()
            .map(|(summary, object)| (summary, object.as_reflect())),
    )
}

fn sorted_nodes(graph: &graph::Graph) -> Vec<(String, bool)> {
    let mut nodes: Vec<(String, bool)> = graph
        .nodes
        .iter()
        .map(|node| (node.uri.clone(), node.present))
        .collect();
    nodes.sort();
    nodes
}

#[test]
fn the_index_has_a_node_per_entry_and_agrees_with_the_graph() {
    let store = package();
    let index = PackageIndex::build(&store, store.revision());
    assert!(
        index.unreadable().is_empty(),
        "every entry reads by its own uri: {:?}",
        index.unreadable()
    );
    let built = index.graph();
    let reference = expected(&store);
    assert_eq!(sorted_nodes(&built), sorted_nodes(&reference));
    assert_eq!(
        built.edges, reference.edges,
        "edge for edge, as `graph::build` draws them"
    );
    // The two entries of the surface are two present nodes.
    let surface: Vec<&str> = built
        .nodes
        .iter()
        .filter(|n| n.uuid == S)
        .map(|n| n.uri.as_str())
        .collect();
    assert_eq!(surface.len(), 2, "{surface:?}");
    assert!(
        index
            .nodes()
            .filter(|n| n.uuid.as_ref() == S)
            .all(|n| n.state == NodeState::Present)
    );
    // H names version 2: one edge, to it. H0 names none, and the surface that
    // declares no version is the one the short reference means (ADR-0003):
    // one edge, to it.
    let to = |from: &str| -> Vec<&str> {
        built
            .edges
            .iter()
            .filter(|e| e.from.contains(from))
            .map(|e| e.to.as_str())
            .collect()
    };
    assert_eq!(to(H).len(), 1);
    assert!(to(H)[0].contains("version='2'"), "{:?}", to(H));
    assert_eq!(to(H0).len(), 1);
    assert!(!to(H0)[0].contains("version="), "{:?}", to(H0));
}

#[test]
fn an_update_that_touches_a_uuid_held_twice_rebuilds_and_still_agrees() {
    let mut store = package();
    let mut index = PackageIndex::build(&store, store.revision());
    // A third version arrives through the gate.
    let third = object(CT, feature(Some("3")));
    store.put_as_new_version(third).expect("a third version");
    let disturbed = index.update(&store, &[S.to_string()], store.revision());
    assert!(disturbed.iter().any(|uuid| uuid == S), "{disturbed:?}");
    let reference = expected(&store);
    let built = index.graph();
    assert_eq!(sorted_nodes(&built), sorted_nodes(&reference));
    assert_eq!(built.edges, reference.edges);
    assert_eq!(
        built.nodes.iter().filter(|n| n.uuid == S).count(),
        3,
        "three versions, three nodes"
    );
}
