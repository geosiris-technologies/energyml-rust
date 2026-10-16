//! A store that is not an EPC, and what the mother trait owes it.
//!
//! Everything above `energyml-storage` was written against traits rather than
//! against [`EpcStore`](energyml_storage::EpcStore) so that an ETP client could
//! be dropped in without touching a reader. That claim was never *tested*: the
//! only implementation of [`EnergymlStorage`] in the workspace was the EPC one,
//! so nothing would have noticed if the trait had quietly grown a hole a
//! network store could not fill.
//!
//! This file fills the hole with a store made of a `Vec` and no file anywhere,
//! and asks it the questions a viewer asks:
//!
//! - can it locate the arrays of an object it just handed over? (`arrays_of`)
//! - can it say who references whom? (`edges`)
//! - can it draw the graph? (`graph`)
//!
//! And the one that matters for a large store: a store that answers
//! `references` from an **index** - what the objects write, kept beside them -
//! must get the same graph without the trait falling back to deserializing
//! everything. `Indexed` below refuses to hand over objects at all, which is the
//! only way to prove it. (A server's stated `edges` are another fact: they carry
//! no path and no reference to an absent object; design note
//! `deux-sources-un-contrat.md` §3.)

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use energyml_core::registry::{DynObject, Registry};
use energyml_core::value::Reflect;
use energyml_storage::array::{ArrayData, ArrayLocation, ArrayMetadata};
use energyml_storage::error::{Diagnostic, Error, Result};
use energyml_storage::graph::Edge;
use energyml_storage::resource::{Dataspace, ObjectHeader, ObjectSummary};
use energyml_storage::storage::EnergymlStorage;

const FAULT: &str = "3d5f8c21-0000-4000-8000-000000000001";
const INTERP: &str = "1a4112fa-c4ef-4c8d-aed0-47d9273bebc5";

fn registry() -> Arc<Registry> {
    let mut registry = Registry::new();
    energyml_eml_2_3::register(&mut registry);
    energyml_resqml_2_2::register(&mut registry);
    Arc::new(registry)
}

fn uri(qualified_type: &str, uuid: &str) -> String {
    format!("eml:///{qualified_type}({uuid})")
}

/// The feature the interpretation points at, and the interpretation.
///
/// Two objects and one reference between them: the smallest thing that has a
/// graph at all.
fn documents() -> Vec<(ObjectSummary, String)> {
    let feature = format!(
        "<resqml:TectonicBoundaryFeature \
xmlns:resqml=\"http://www.energistics.org/energyml/data/resqmlv2\" \
xmlns:eml=\"http://www.energistics.org/energyml/data/commonv2\" \
uuid=\"{FAULT}\" schemaVersion=\"2.2\">\
<eml:Citation><eml:Title>a fault</eml:Title><eml:Originator>test</eml:Originator>\
<eml:Creation>2026-08-02T00:00:00Z</eml:Creation><eml:Format>test</eml:Format></eml:Citation>\
<resqml:TectonicBoundaryKind>fault</resqml:TectonicBoundaryKind>\
</resqml:TectonicBoundaryFeature>"
    );
    let interpretation = format!(
        "<resqml:FaultInterpretation \
xmlns:resqml=\"http://www.energistics.org/energyml/data/resqmlv2\" \
xmlns:eml=\"http://www.energistics.org/energyml/data/commonv2\" \
uuid=\"{INTERP}\" schemaVersion=\"2.2\">\
<eml:Citation><eml:Title>an interpretation</eml:Title><eml:Originator>test</eml:Originator>\
<eml:Creation>2026-08-02T00:00:00Z</eml:Creation><eml:Format>test</eml:Format></eml:Citation>\
<resqml:InterpretedFeature>\
<eml:QualifiedType>resqml22.TectonicBoundaryFeature</eml:QualifiedType>\
<eml:Title>a fault</eml:Title><eml:Uuid>{FAULT}</eml:Uuid>\
</resqml:InterpretedFeature>\
</resqml:FaultInterpretation>"
    );
    let summary = |ty: &str, uuid: &str, name: &str| {
        ObjectSummary::new(uri(ty, uuid), uuid.to_string(), ty.to_string())
            .with_content_type(format!("application/x-resqml+xml;version=2.2;type={name}"))
            // Not a part name: this store has no parts. Which is the point - the
            // locator is opaque above this crate, and nothing may build on it.
            .with_locator(format!("in-memory:{uuid}"))
    };
    vec![
        (
            summary(
                "resqml22.TectonicBoundaryFeature",
                FAULT,
                "TectonicBoundaryFeature",
            ),
            feature,
        ),
        (
            summary("resqml22.FaultInterpretation", INTERP, "FaultInterpretation"),
            interpretation,
        ),
    ]
}

/// A store made of two XML strings and a registry.
///
/// No zip, no `.rels`, no `[Content_Types].xml`, no HDF5 - the closest thing to
/// an ETP client this workspace can build before the client exists.
struct InMemory {
    registry: Arc<Registry>,
    parts: Vec<(ObjectSummary, String)>,
    /// How many times an object was actually deserialized, so a test can say
    /// whether the default path or the native one was taken.
    reads: AtomicUsize,
}

impl InMemory {
    fn new() -> Self {
        InMemory {
            registry: registry(),
            parts: documents(),
            reads: AtomicUsize::new(0),
        }
    }

    fn parse(&self, summary: &ObjectSummary, source: &str) -> Result<Box<dyn DynObject>> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        let entry = self
            .registry
            .by_qualified_type(&summary.qualified_type)
            .ok_or_else(|| Error::UnknownType(summary.qualified_type.clone()))?;
        let content_type = entry
            .info
            .content_type
            .ok_or_else(|| Error::UnknownType(summary.qualified_type.clone()))?;
        Ok(self.registry.read_xml(content_type, source.as_bytes())?)
    }
}

#[async_trait::async_trait]
impl EnergymlStorage for InMemory {
    fn store_kind(&self) -> &'static str {
        "in-memory"
    }

    async fn dataspaces(&self) -> Result<Vec<Dataspace>> {
        Ok(vec![Dataspace::new(
            "eml:///".to_string(),
            "in-memory".to_string(),
        )])
    }

    async fn resources(&self) -> Result<Vec<ObjectSummary>> {
        Ok(self.parts.iter().map(|(s, _)| s.clone()).collect())
    }

    async fn headers(&self, uris: &[String]) -> Vec<Result<ObjectHeader>> {
        uris.iter()
            .map(|wanted| {
                let (summary, source) = self
                    .parts
                    .iter()
                    .find(|(s, _)| &s.uri == wanted)
                    .ok_or_else(|| Error::NotFound(wanted.clone()))?;
                // Crude on purpose: a header is whatever the store can say
                // without a full parse, and every store says it differently.
                let title = source
                    .split_once("<eml:Title>")
                    .and_then(|(_, rest)| rest.split_once("</eml:Title>"))
                    .map(|(title, _)| title.to_string());
                Ok({
                    let mut header = ObjectHeader::new(summary.clone());
                    header.title = title;
                    header.originator = Some("test".to_string());
                    header.format = Some("test".to_string());
                    header.schema_version = Some("2.2".to_string());
                    header
                })
            })
            .collect()
    }

    async fn get_objects(&self, uris: &[String]) -> Vec<Result<Box<dyn DynObject>>> {
        uris.iter()
            .map(|wanted| {
                let (summary, source) = self
                    .parts
                    .iter()
                    .find(|(s, _)| &s.uri == wanted)
                    .ok_or_else(|| Error::NotFound(wanted.clone()))?;
                self.parse(summary, source)
            })
            .collect()
    }

    async fn supported_types(&self) -> Vec<&'static energyml_core::reflect::TypeInfo> {
        self.registry.types().collect()
    }

    fn arrays_of(&self, object: &dyn Reflect) -> Vec<ArrayLocation> {
        // A store with no external files still has to answer: what an object
        // *declares* is in the object, and only the resolution to a file is
        // the store's business. `locate_with` is that split made explicit.
        energyml_storage::array::locate_with(object, &energyml_storage::array::NoProxies)
    }

    async fn array_metadata(&self, location: &ArrayLocation) -> Result<ArrayMetadata> {
        Err(Error::NotFound(location.path_in_file.clone()))
    }

    async fn read_array(&self, location: &ArrayLocation) -> Result<ArrayData> {
        Err(Error::NotFound(location.path_in_file.clone()))
    }

    fn diagnostics(&self) -> Vec<Diagnostic> {
        Vec::new()
    }
}

/// The same store, except that it answers `edges` from an index - the way ETP
/// does - and **refuses** to deserialize anything.
///
/// The refusal is the assertion: if the provided `graph` ever falls back to
/// reading objects, this store returns an empty graph and the test fails.
struct Indexed {
    inner: InMemory,
}

#[async_trait::async_trait]
impl EnergymlStorage for Indexed {
    fn store_kind(&self) -> &'static str {
        "indexed"
    }

    async fn dataspaces(&self) -> Result<Vec<Dataspace>> {
        self.inner.dataspaces().await
    }

    async fn resources(&self) -> Result<Vec<ObjectSummary>> {
        self.inner.resources().await
    }

    async fn headers(&self, uris: &[String]) -> Vec<Result<ObjectHeader>> {
        self.inner.headers(uris).await
    }

    async fn get_objects(&self, uris: &[String]) -> Vec<Result<Box<dyn DynObject>>> {
        uris.iter()
            .map(|_| {
                Err(Error::Unsupported {
                    operation: "handing over objects",
                    store: "indexed",
                })
            })
            .collect()
    }

    async fn supported_types(&self) -> Vec<&'static energyml_core::reflect::TypeInfo> {
        self.inner.supported_types().await
    }

    fn arrays_of(&self, object: &dyn Reflect) -> Vec<ArrayLocation> {
        self.inner.arrays_of(object)
    }

    async fn array_metadata(&self, location: &ArrayLocation) -> Result<ArrayMetadata> {
        self.inner.array_metadata(location).await
    }

    async fn read_array(&self, location: &ArrayLocation) -> Result<ArrayData> {
        self.inner.read_array(location).await
    }

    fn diagnostics(&self) -> Vec<Diagnostic> {
        Vec::new()
    }

    /// What an index of the references the objects write answers - the path
    /// included, which a server's stated edges do not carry.
    async fn references(&self, _uris: &[String]) -> Result<Vec<Edge>> {
        Ok(vec![Edge::new(
            uri("resqml22.FaultInterpretation", INTERP),
            uri("resqml22.TectonicBoundaryFeature", FAULT),
            "InterpretedFeature".to_string(),
        )])
    }
}

fn block<F: std::future::Future>(future: F) -> F::Output {
    futures_executor::block_on(future)
}

#[test]
fn a_store_with_no_file_behind_it_satisfies_the_trait() {
    let store = InMemory::new();
    // `+ Sync` because `async_trait` bounds every *provided* method on it: the
    // desugared future has to be `Send`, and a `&Self` inside it makes that a
    // `Self: Sync` requirement. Worth knowing before writing `&dyn
    // EnergymlStorage` and finding half the trait unreachable.
    let store: &(dyn EnergymlStorage + Sync) = &store;
    assert_eq!(store.store_kind(), "in-memory");
    let resources = block(store.resources()).expect("it lists");
    assert_eq!(resources.len(), 2);
    let object =
        block(store.get_object(&uri("resqml22.FaultInterpretation", INTERP))).expect("it deserializes");
    assert_eq!(object.type_info().type_key, "FaultInterpretation");
}

#[test]
fn the_arrays_of_an_object_are_reachable_through_the_trait_alone() {
    // The gap this closed: `dyn EnergymlStorage` could read an array whose
    // location it was handed, and had no way to obtain one. Every geometry
    // reader would have had to name `EpcStore` to get past it.
    let store = InMemory::new();
    let store: &(dyn EnergymlStorage + Sync) = &store;
    let object = block(store.get_object(&uri("resqml22.FaultInterpretation", INTERP))).expect("it reads");
    // This interpretation holds no array; what is being tested is that the
    // question can be *asked* of the trait, not the answer.
    assert!(store.arrays_of(object.as_reflect()).is_empty());
}

#[test]
fn the_default_edges_are_derived_from_the_objects() {
    let store = InMemory::new();
    let before = store.reads.load(Ordering::SeqCst);
    let edges = block(EnergymlStorage::edges(&store, &[])).expect("edges");
    assert!(
        store.reads.load(Ordering::SeqCst) > before,
        "the default has to read the objects; there is nowhere else the edges could come from"
    );
    assert_eq!(edges.len(), 1, "one reference: the interpreted feature");
    assert_eq!(edges[0].from, uri("resqml22.FaultInterpretation", INTERP));
    assert_eq!(edges[0].to, uri("resqml22.TectonicBoundaryFeature", FAULT));
    assert!(
        edges[0].path.contains("InterpretedFeature"),
        "the edge says where in the object it came from: {}",
        edges[0].path
    );
}

#[test]
fn a_store_that_indexes_its_references_draws_its_graph_without_reading_an_object() {
    let store = Indexed {
        inner: InMemory::new(),
    };
    let graph = block(EnergymlStorage::graph(&store, &[])).expect("a graph");
    assert_eq!(
        store.inner.reads.load(Ordering::SeqCst),
        0,
        "the indexed references must be used; deserializing anything defeats the purpose"
    );
    assert_eq!(graph.edges.len(), 1);
    assert_eq!(graph.nodes.len(), 2);
    assert!(graph.nodes.iter().all(|node| node.present));
    // The nodes come from `resources` + `headers`, so they carry their labels
    // and their version-independent concept without an object being parsed.
    let interpretation = graph
        .nodes
        .iter()
        .find(|node| node.uuid == INTERP)
        .expect("the interpretation is a node");
    assert_eq!(interpretation.type_key, "FaultInterpretation");
    assert_eq!(interpretation.title.as_deref(), Some("an interpretation"));
}

#[test]
fn the_two_paths_agree() {
    // The whole claim in one assertion: a store that derives its edges and a
    // store that is told them draw the same picture.
    let derived = block(EnergymlStorage::graph(&InMemory::new(), &[])).expect("derived");
    let native = block(EnergymlStorage::graph(
        &Indexed {
            inner: InMemory::new(),
        },
        &[],
    ))
    .expect("native");

    let key = |graph: &energyml_storage::graph::Graph| {
        let mut nodes: Vec<(String, String, bool)> = graph
            .nodes
            .iter()
            .map(|n| (n.uri.clone(), n.type_key.clone(), n.present))
            .collect();
        nodes.sort();
        let mut edges: Vec<(String, String)> = graph
            .edges
            .iter()
            .map(|e| (e.from.clone(), e.to.clone()))
            .collect();
        edges.sort();
        (nodes, edges)
    };
    assert_eq!(key(&derived), key(&native));
}

#[test]
fn a_graph_may_be_asked_for_a_selection() {
    let store = InMemory::new();
    let only = vec![uri("resqml22.FaultInterpretation", INTERP)];
    let graph = block(EnergymlStorage::graph(&store, &only)).expect("a graph");
    // The interpretation is held; the feature it names is outside the
    // selection, and gets a node saying so rather than an edge into nothing.
    assert_eq!(graph.edges.len(), 1);
    assert_eq!(graph.nodes.len(), 2);
    let outside: Vec<&_> = graph.nodes.iter().filter(|n| !n.present).collect();
    assert_eq!(outside.len(), 1);
    assert_eq!(outside[0].uuid, FAULT);
    assert_eq!(
        outside[0].type_key, "TectonicBoundaryFeature",
        "the concept of an absent object is still known: the reference declares it"
    );
}

// ---------------------------------------------------------------------------
// The EPC, taken through the same door
// ---------------------------------------------------------------------------

/// The derived path and the assembled path have to agree on a real package.
///
/// [`energyml_storage::graph::build`] walks deserialized objects;
/// [`energyml_storage::graph::assemble`] joins an inventory to edges obtained
/// some other way. The trait's `graph` uses the second over the first, so if
/// the two ever drifted a store answering `edges` natively would silently draw
/// a different picture from one that does not - the exact failure mode this
/// whole seam exists to rule out. Checked against every EPC of the corpus
/// rather than a fixture, because the ways a real package differs from a
/// fixture are the ways the two could disagree.
#[test]
fn on_a_real_package_the_two_builders_agree() {
    let files = crate::fixtures::public_corpus();
    if files.is_empty() {
        eprintln!("skipped: no reference package (scripts/fetch-fixtures.py)");
        return;
    }

    let registry = Arc::new(energyml_schemas::all());
    let mut checked = 0usize;
    for path in &files {
        let Ok(store) = energyml_storage::EpcStore::open(path, registry.clone()) else {
            continue;
        };
        let through_the_trait = block(EnergymlStorage::graph(&store, &[])).expect("the trait draws it");

        // The same thing, the way the CLI does it: read everything, walk it.
        let summaries = energyml_storage::EpcStore::resources(&store);
        let mut held = Vec::new();
        for summary in &summaries {
            if let Ok(object) = energyml_storage::EpcStore::get_object(&store, &summary.uri) {
                held.push((summary, object));
            }
        }
        let directly = energyml_storage::graph::build(
            held.iter()
                .map(|(summary, object)| (*summary, object.as_reflect())),
        );

        let mut a: Vec<(String, String, String)> = through_the_trait
            .edges
            .iter()
            .map(|e| (e.from.clone(), e.to.clone(), e.path.clone()))
            .collect();
        // The trait gives each reference to the data object that writes it and
        // draws the objects held by value with their edge to the container:
        // the direct path does the same, from the same contents.
        let contents = block(EnergymlStorage::contents(&store)).expect("the contents");
        let written = energyml_storage::graph::attribute_to_writers(
            directly.edges.clone(),
            held.iter()
                .map(|(summary, object)| (*summary, object.as_reflect())),
            &contents.objects,
        );
        let mut b: Vec<(String, String, String)> = written
            .iter()
            .chain(contents.edges.iter())
            .map(|e| (e.from.clone(), e.to.clone(), e.path.clone()))
            .collect();
        a.sort();
        b.sort();
        assert_eq!(
            a,
            b,
            "{}: the trait and the direct build disagree on the edges",
            path.display()
        );

        let mut nodes_a: Vec<(String, bool)> = through_the_trait
            .nodes
            .iter()
            .map(|n| (n.uri.clone(), n.present))
            .collect();
        let mut nodes_b: Vec<(String, bool)> = directly
            .nodes
            .iter()
            .map(|n| (n.uri.clone(), n.present))
            .collect();
        nodes_a.sort();
        nodes_b.sort();
        // Not identical by construction: `build` only knows the objects it was
        // handed, so a package with an unreadable object is a node short. The
        // trait starts from the inventory and has it. That is the one allowed
        // difference, and it goes one way.
        for node in &nodes_b {
            assert!(
                nodes_a.contains(node),
                "{}: the trait lost the node `{}`",
                path.display(),
                node.0
            );
        }
        eprintln!(
            "{}: {} node(s), {} edge(s) - both builders agree",
            path.file_name().unwrap().to_string_lossy(),
            through_the_trait.nodes.len(),
            through_the_trait.edges.len()
        );
        checked += 1;
    }
    assert!(checked > 0, "no package of the corpus opened");
}

/// The reference check works from an inventory owned by the caller.
///
/// [`energyml_storage::graph::check_references`] takes a closure returning a
/// **borrowed** summary, which suits `EpcStore::find_by_uuid` - it borrows from
/// the store's own index. A store that hands its inventory over by value, as
/// `resources` does and as an ETP client has no choice but to do, was the case
/// nothing had tried. It works, and this is what says so: the caller owns the
/// `Vec` and borrows from it, no store involved.
#[test]
fn the_reference_check_runs_on_an_inventory_the_caller_owns() {
    use energyml_storage::graph;

    let store = InMemory::new();
    let resources = block(store.resources()).expect("it lists");
    let by_uuid: std::collections::HashMap<String, &ObjectSummary> = resources
        .iter()
        .map(|summary| (summary.uuid.to_ascii_lowercase(), summary))
        .collect();

    let interpretation = resources
        .iter()
        .find(|summary| summary.uuid == INTERP)
        .expect("the interpretation is listed");
    let object = block(store.get_object(&interpretation.uri)).expect("it reads");
    let issues = graph::check_references(
        interpretation,
        object.as_reflect(),
        &|uuid| by_uuid.get(&uuid.to_ascii_lowercase()).copied(),
        &|_| graph::Cached::Unread,
    );
    assert!(
        issues.is_empty(),
        "the feature is in the store and declares the right type: {issues:?}"
    );

    // And the other way: an inventory that does not hold the feature.
    let empty: std::collections::HashMap<String, &ObjectSummary> = std::collections::HashMap::new();
    let dangling = graph::check_references(
        interpretation,
        object.as_reflect(),
        &|uuid| empty.get(uuid).copied(),
        &|_| graph::Cached::Unread,
    );
    assert_eq!(dangling.len(), 1);
    assert!(dangling[0].is_error());
    assert_eq!(dangling[0].target_uuid, FAULT);
}
