//! What the module keeps of a package: the links and the property sheets,
//! **never the objects**.
//!
//! Design (ADR-0010): the module keeps the links and the property sheets, never
//! the objects. An object is read again on demand, and only the one asked for;
//! what is true of the package is computed once, updated object by object when
//! an object changes, and never claims to be complete when the store is not.
//!
//! # Why this exists
//!
//! a front end's graph builder parsed the 77 090 objects of a package and kept them **all
//! alive** until `graph::build` returned: 30 s and +632 MiB for something that
//! keeps a few megabytes of links. And *"which properties does representation X
//! carry"* was answered by six code paths that each parsed all 63 525
//! properties. The index answers both from what one streaming pass kept.
//!
//! # What it keeps
//!
//! - **nodes** — uuid, uri, type, title, and whether the package holds the
//!   object (§5.1);
//! - **edges**, both directions, each carrying an *interned* path — a package of
//!   319 244 references names a handful of distinct paths;
//! - **one `PropertySheet` per property**: what [`property::read`] returns and
//!   the front end's attribute entry needs, *without the object* (§5.1);
//! - **the relation tables** the inheritance walks and the attachment listings
//!   read — parents, associations and memberships, projects, models, graphical
//!   entries, attachments — laid **in the same pass**, from the references just
//!   taken (`package_index/relations.rs`). The relations are built in addition at
//!   graph-construction time, rather than by traversing the whole graph
//!   afterwards.
//!
//! # The two promises
//!
//! 1. **`PackageIndex::graph` is `graph::build`'s graph, node for node and edge
//!    for edge** — same nodes, same order, same edges, same JSON. The index is
//!    built by the same walk (`identity::collect_refs`), with the same rules for
//!    the nodes of objects the package does not hold (see `IndexBuilder`). It is
//!    not checked by this module: `tests/package_index.rs` confronts the two on
//!    every package of the corpus, and breaks the index on purpose to be sure the
//!    confrontation can fail (a component must never be asked to check itself).
//! 2. **`PackageIndex::properties_of` is `property::read`'s rule** — a property
//!    hangs on the representation its **own** `SupportingRepresentation` field
//!    names, never on the `.rels` (a package may have none: `80wells` has none).
//!    The candidates come from the incoming edges, and
//!    each is then confirmed against the sheet's field, so the two readings
//!    cannot disagree.
//!
//! # Construction: one pass, in slices, dropping every object at once (§5.2, §5.3)
//!
//! `IndexBuilder::step` reads `n` objects, takes their links and sheets, and
//! lets each object go before reading the next. A caller that must not block
//! (a wasm worker) calls it in a loop and yields between slices. The numbers it
//! reports in `Progress` are the real ones.
//!
//! # Updating: object by object (§4)
//!
//! `PackageIndex::update` takes the uuids that changed — the store revision
//! journal is where a caller gets them — and re-reads **those** objects
//! only. It is not a substitute for a rebuild in one respect, stated on the
//! method: the *order* of nodes after an update is not the order a rebuild would
//! give, so a caller that needs `graph::build`'s exact order rebuilds.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use energyml_core::identifiers::EtpUri;
use energyml_core::identity;
use energyml_core::naming;
use energyml_core::value::Reflect;
use energyml_storage::graph::{self, Edge, Graph, Node, Pointer};
use energyml_storage::{EpcStore, ObjectSummary};

use crate::property::{self, IndexableElement, Property, PropertyKindRef, TimeRef};
use crate::source::ObjectSource;
use crate::values;

mod relations;
pub use relations::{AssociationRow, GraphicalRow, Membership, ModelRef};

/// A node's position in [`PackageIndex`]. `u32` because a package of 77 090
/// objects is far from 4 billion and every edge stores two of them.
pub type NodeId = u32;

/// An interned path, see [`PackageIndex::path`].
pub type PathId = u32;

/// The suffix of the edge path that makes a property hang on a representation:
/// the field is `SupportingRepresentation`, at the top of the property.
const SUPPORTING_REPRESENTATION: &str = "SupportingRepresentation";

// ---------------------------------------------------------------------------
// Interning
// ---------------------------------------------------------------------------

/// Strings that repeat: paths, qualified types, units, kind names, lookup and
/// support uuids. A path is an id; the other uses clone an `Arc<str>` — sixteen
/// bytes instead of a `String` per property.
#[derive(Default)]
struct Interner {
    ids: HashMap<Arc<str>, u32>,
    list: Vec<Arc<str>>,
}

impl Interner {
    fn arc(&mut self, text: &str) -> Arc<str> {
        if let Some(id) = self.ids.get(text) {
            return self.list[*id as usize].clone();
        }
        let shared: Arc<str> = Arc::from(text);
        self.ids.insert(shared.clone(), self.list.len() as u32);
        self.list.push(shared.clone());
        shared
    }

    fn id(&mut self, text: &str) -> u32 {
        if let Some(id) = self.ids.get(text) {
            return *id;
        }
        let shared: Arc<str> = Arc::from(text);
        let id = self.list.len() as u32;
        self.ids.insert(shared.clone(), id);
        self.list.push(shared);
        id
    }

    fn get(&self, id: u32) -> &str {
        self.list.get(id as usize).map(|s| &**s).unwrap_or("")
    }
}

// ---------------------------------------------------------------------------
// Nodes
// ---------------------------------------------------------------------------

/// What the index knows of a node's presence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeState {
    /// The package holds the object and it was read: its links are in the index.
    Present,
    /// Referenced by an object of the package, held by none — the node
    /// `graph::build` gives such a target (`present: false`). It exists **only**
    /// while something references it.
    Absent,
    /// In the inventory, not read yet: a builder in progress. Never in a
    /// finished index.
    Pending,
    /// In the inventory and unreadable. Transient: settled at the end of a pass
    /// into [`NodeState::Absent`] when something references it (what
    /// a front end's graph builder did — it skipped the object, and the reference made a
    /// node of its own), or dropped when nothing does.
    Unreadable,
    /// A tombstone. Ids are positions, so a node that leaves keeps its slot.
    Gone,
}

/// One object, as the graph labels it.
///
/// The fields of [`graph::Node`], with the strings that repeat shared.
#[derive(Clone, Debug)]
pub struct IndexNode {
    /// ETP URI.
    pub uri: Box<str>,
    /// Uuid, as the summary (or, for an absent node, the reference) wrote it.
    pub uuid: Box<str>,
    /// `resqml22.TriangulatedSetRepresentation`.
    pub qualified_type: Arc<str>,
    /// The concept without `obj_`: `TriangulatedSetRepresentation`.
    pub type_key: Arc<str>,
    /// `Citation/Title` — of the object when present, cached by the first
    /// reference when absent (as `graph::build` does).
    pub title: Option<Arc<str>>,
    /// Presence.
    pub state: NodeState,
    /// See [`graph::Node::externally_defined`].
    pub externally_defined: bool,
    /// For an absent node: where `graph::build` met it first, as
    /// `referrer id << 32 | index of the edge in that referrer`. That is the
    /// order absent nodes come out in, and the referrer whose reference supplied
    /// their type and title. Zero for anything else.
    seen: u64,
}

/// The version a node's uri names (`…(uuid,version='1')`), when it names one.
fn version_in_uri(uri: &str) -> Option<String> {
    identity::parse_address(uri).1
}

/// `graph::build`'s `type_key` for a summary: the concept of the qualified type,
/// the whole string when it does not parse. Lines 1330–1332 of `graph.rs` say it
/// inline twice; it is three public calls, not a private rule.
fn type_key_of(qualified_type: &str) -> String {
    identity::parse_qualified_type(qualified_type)
        .map(|parts| naming::strip_obj_prefix(parts.object_type).to_string())
        .unwrap_or_else(|_| qualified_type.to_string())
}

/// The two facts the relation tables need from the object itself — read while
/// the index holds it for its references, never in a pass of their own.
fn relations_facts(node: &IndexNode, object: &dyn Reflect, strings: &mut Interner) -> relations::Facts {
    use energyml_core::any_xml::AnyXml;
    use energyml_core::value::FieldValue;

    let mut facts = relations::Facts::default();
    match &*node.type_key {
        "DataobjectCollection" => {
            facts.project = object
                .field("Kind")
                .and_then(|value| value.as_scalar())
                .is_some_and(|kind| kind.to_string() == "project");
        }
        "GraphicalInformationSet" => {
            let items = match object.field("GraphicalInformation") {
                Some(FieldValue::List(items)) => items,
                Some(FieldValue::Null) | None => Vec::new(),
                Some(other) => vec![other],
            };
            for item in items {
                let name = match item.as_object() {
                    // The usual case: the entry is a subtree nothing could type, and
                    // its `xsi:type` says which entry it is (`GraphicalSet::read`).
                    Some(entry) => match entry.as_any().downcast_ref::<AnyXml>() {
                        Some(raw) => {
                            let declared = raw
                                .attributes
                                .iter()
                                .find(|(name, _)| {
                                    name == "xsi:type" || name == "type" || name.ends_with(":type")
                                })
                                .map(|(_, value)| value.as_str())
                                .unwrap_or("");
                            declared.rsplit(':').next().unwrap_or(declared).to_string()
                        }
                        None => entry.type_info().name.to_string(),
                    },
                    None => "<unreadable>".to_string(),
                };
                facts.entry_types.push(strings.arc(&name));
            }
        }
        _ => {}
    }
    facts
}

// ---------------------------------------------------------------------------
// Property sheets
// ---------------------------------------------------------------------------

/// The property kind as the document states it — [`PropertyKindRef`] with its
/// strings shared.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SheetKind {
    /// RESQML 2.0.1 standard kind, by name.
    Standard(Arc<str>),
    /// A reference to a `PropertyKind` object.
    Object {
        /// Its uuid.
        uuid: Arc<str>,
        /// The title the reference cached.
        title: Option<Arc<str>>,
    },
}

/// A property, without the object.
///
/// Every field is a field of [`property::Property`] that `attribute_json` or a
/// colour decision reads; the **patches** are not here (they borrow the object,
/// and nothing that lists properties needs them). `values_path` is what
/// [`values::locate`] finds first, the address `statistics` and `readArray` take.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertySheet {
    /// The property's uuid.
    pub uuid: Box<str>,
    /// `Citation.Title` as [`property::read`] gives it (an empty title stays
    /// `Some("")`; the node's title is the one filtered for emptiness).
    pub title: Option<Arc<str>>,
    /// `resqml22.ContinuousProperty`…
    pub qualified_type: Arc<str>,
    /// What the values hang on.
    pub indexable: IndexableElement,
    /// Values per element, slowest dimension first.
    pub value_count_per_element: Box<[u64]>,
    /// The unit, for a continuous property.
    pub uom: Option<Arc<str>>,
    /// The kind, unresolved.
    pub kind: Option<SheetKind>,
    /// Uuid of the supporting representation **as the property writes it**.
    pub supporting_representation: Option<Arc<str>>,
    /// Uuid of the lookup table.
    pub lookup: Option<Arc<str>>,
    /// When it applies.
    pub time: Option<Box<TimeRef>>,
    /// 2.0.1 realization index.
    pub realization: Option<u64>,
    /// Whether the document declares `RealizationIndices`.
    pub has_realization_indices: bool,
    /// Where the first value array sits: `ValuesForPatch[0]`.
    pub values_path: Option<Arc<str>>,
}

impl PropertySheet {
    /// `Property::values_per_element`, on the sheet.
    pub fn values_per_element(&self) -> u64 {
        self.value_count_per_element.iter().product::<u64>().max(1)
    }

    /// The kind as a [`PropertyKindRef`], for code written against `property`.
    pub fn kind_ref(&self) -> Option<PropertyKindRef> {
        self.kind.as_ref().map(|kind| match kind {
            SheetKind::Standard(name) => PropertyKindRef::Standard {
                name: name.to_string(),
            },
            SheetKind::Object { uuid, title } => PropertyKindRef::Object {
                uuid: uuid.to_string(),
                title: title.as_ref().map(|t| t.to_string()),
            },
        })
    }
}

// ---------------------------------------------------------------------------
// The index
// ---------------------------------------------------------------------------

/// Links and sheets of one package. See the module documentation.
pub struct PackageIndex {
    revision: u64,
    complete: bool,
    nodes: Vec<IndexNode>,
    /// Lowercase uuid → node. A uuid held twice keeps the **last** node, as
    /// `graph::build` does (its `index.insert` overwrites).
    by_uuid: HashMap<Box<str>, NodeId>,
    /// What a node references: sorted, without duplicates.
    out: Vec<Box<[(NodeId, PathId)]>>,
    /// Who references a node, with the path: the dependents.
    inn: Vec<Vec<(NodeId, PathId)>>,
    strings: Interner,
    sheets: HashMap<NodeId, PropertySheet>,
    /// Objects that could not be read: uuid and reason.
    unreadable: Vec<(String, String)>,
    /// Properties that read but whose sheet could not be taken: uuid and reason
    /// — `property::read` refused them (an indexable element this build does not
    /// know). Named rather than skipped, like `property::Index::build`'s refused.
    unsheeted: Vec<(String, String)>,
    /// Nodes whose absence/presence must be re-decided at the end of a pass or
    /// an update.
    to_settle: Vec<NodeId>,
    /// What the inheritance walks read, laid in the same pass as the references
    /// — see [`relations`].
    relations: relations::Relations,
    /// The data objects nested in a part, by lowercased uuid → the node of the
    /// part holding them: a WITSML `ChannelSet` in a `Log`, its `Channel`s.
    /// Filled when a part is absorbed, emptied when it is detached. A reference
    /// to one of these is a link to its part in [`PackageIndex::graph`] — the
    /// rule of `graph::build` (`graph::nested_index`), which the graph must
    /// match byte for byte. Reported on the synthetic reference model: the
    /// statistics kept showing "references to an absent object".
    nested: HashMap<Box<str>, NodeId>,
    /// Lowercase uuid → every node of a uuid the store holds **more than once** —
    /// under two `objectVersion`s, or two releases (an entry is (uuid, type,
    /// version), ADR-0010). Empty for every package without
    /// namesakes. `by_uuid` answers for such a uuid with **one** of them (the one
    /// that declares no version, else the first), which is what a question asked
    /// of a bare uuid can mean; these nodes are read by their **uri**, since the
    /// bare uuid is ambiguous in the store, and a reference finds its node by the
    /// type and the version it names.
    shared: HashMap<Box<str>, Vec<NodeId>>,
    /// What the readers did, counted: see [`PackageIndex::traffic`].
    work: Work,
}

/// Work counters. Atomics, because the index is read through `&self`; relaxed,
/// because they are a gauge and not a synchronisation.
#[derive(Default)]
struct Work {
    nodes: AtomicU64,
    edges: AtomicU64,
    rows: AtomicU64,
}

/// What the readers of an index did, counted — **work, not time**: what a test
/// asserts to say a question no longer walks the package.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Traffic {
    /// Nodes yielded by [`PackageIndex::nodes`]: a scan of the package.
    pub nodes_scanned: u64,
    /// References turned into a [`Pointer`] ([`PackageIndex::references_of`],
    /// [`PackageIndex::dependents_of`]): the strings built from the edges.
    pub edges_read: u64,
    /// Rows read from the relation tables (parents, memberships, attachments…).
    pub rows_read: u64,
}

impl PackageIndex {
    /// The index of a whole store, in one call.
    ///
    /// `revision` is the store revision the index is valid for — the caller's,
    /// because this crate does not own the counter. Prefer
    /// [`IndexBuilder`] when the pass must not block.
    pub fn build(store: &EpcStore, revision: u64) -> Self {
        let mut builder = IndexBuilder::new(store.resources(), revision);
        let total = builder.total();
        builder.step(store, total);
        builder.finish()
    }

    fn empty(revision: u64) -> Self {
        Self {
            revision,
            complete: false,
            nodes: Vec::new(),
            by_uuid: HashMap::new(),
            out: Vec::new(),
            inn: Vec::new(),
            strings: Interner::default(),
            sheets: HashMap::new(),
            unreadable: Vec::new(),
            unsheeted: Vec::new(),
            to_settle: Vec::new(),
            relations: relations::Relations::default(),
            nested: HashMap::new(),
            shared: HashMap::new(),
            work: Work::default(),
        }
    }

    /// What the readers did so far — see [`Traffic`].
    pub fn traffic(&self) -> Traffic {
        Traffic {
            nodes_scanned: self.work.nodes.load(Ordering::Relaxed),
            edges_read: self.work.edges.load(Ordering::Relaxed),
            rows_read: self.work.rows.load(Ordering::Relaxed),
        }
    }

    // -- Facts about the index --------------------------------------------

    /// The store revision this index is valid for.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether the index was built over the store's **whole** inventory.
    ///
    /// `false` for an ETP connection, which is never complete (the index of a
    /// connection is never `complete`): the index then answers for
    /// what was taken, and says so. [`PackageIndex::mark_incomplete`] is the
    /// caller's way to say it.
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// Declare the index a report on **what was taken** rather than on the
    /// package (an ETP dataspace).
    pub fn mark_incomplete(&mut self) {
        self.complete = false;
    }

    /// How many nodes the graph would show (present and absent).
    pub fn len(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| matches!(node.state, NodeState::Present | NodeState::Absent))
            .count()
    }

    /// Whether the graph would be empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How many distinct references the index holds.
    pub fn edge_count(&self) -> usize {
        self.out.iter().map(|edges| edges.len()).sum()
    }

    /// How many property sheets.
    pub fn sheet_count(&self) -> usize {
        self.sheets.len()
    }

    /// The objects that could not be read, uuid and reason. A pass that loses an
    /// object says so; it never reports a smaller package.
    pub fn unreadable(&self) -> &[(String, String)] {
        &self.unreadable
    }

    /// The properties `property::read` refused, uuid and reason.
    pub fn unsheeted(&self) -> &[(String, String)] {
        &self.unsheeted
    }

    /// The text of an interned path.
    pub fn path(&self, id: PathId) -> &str {
        self.strings.get(id)
    }

    // -- Nodes -------------------------------------------------------------

    fn id_of(&self, lowercase_uuid: &str) -> Option<NodeId> {
        self.by_uuid.get(lowercase_uuid).copied()
    }

    fn uri_str(&self, id: NodeId) -> &str {
        &self.nodes[id as usize].uri
    }

    /// **The one door to a node**: by its address — the full uri (the identity,
    /// version included), or a bare uuid, which is an adapter for a caller that
    /// has no better and answers the uuid's default node.
    ///
    /// A uri is read as it says: the node of that exact uri; failing that, the
    /// node of the uuid whose version contradicts nothing the uri names
    /// ([`identity::version_fit`], the one rule) — the exact version first, then
    /// one that names none, then the uuid's default. A uri that **names a version
    /// the package's node of that uuid contradicts** is not that node: `None`.
    fn find(&self, address: &str) -> Option<NodeId> {
        let address = address.trim();
        let (key, wanted) = identity::parse_address(address);
        let live = |id: &NodeId| !matches!(self.nodes[*id as usize].state, NodeState::Gone);
        let held: Vec<NodeId> = match self.shared.get(key.as_str()) {
            Some(held) => held.iter().copied().filter(live).collect(),
            None => self
                .by_uuid
                .get(key.as_str())
                .copied()
                .filter(live)
                .into_iter()
                .collect(),
        };
        if held.len() > 1
            && let Some(exact) = held
                .iter()
                .find(|&&id| self.nodes[id as usize].uri.eq_ignore_ascii_case(address))
        {
            return Some(*exact);
        }
        let mut best: Option<(identity::VersionFit, NodeId)> = None;
        for &id in &held {
            let fit = identity::version_fit(
                wanted.as_deref(),
                version_in_uri(&self.nodes[id as usize].uri).as_deref(),
            );
            if fit == identity::VersionFit::Contradicts {
                continue;
            }
            let better = match best {
                None => true,
                Some((kept, _)) => fit < kept,
            };
            if better {
                best = Some((fit, id));
            }
        }
        let (fit, id) = best?;
        // Several nodes fit equally: the uuid's default one, when it is among them.
        if held.len() > 1
            && let Some(default) = self.by_uuid.get(key.as_str()).copied()
            && held.contains(&default)
            && identity::version_fit(
                wanted.as_deref(),
                version_in_uri(&self.nodes[default as usize].uri).as_deref(),
            ) == fit
        {
            return Some(default);
        }
        Some(id)
    }

    /// The node an address names (see `PackageIndex::find`: a uri, or a bare
    /// uuid at the library's entry), when the graph shows one.
    pub fn node(&self, address: &str) -> Option<&IndexNode> {
        let node = &self.nodes[self.find(address)? as usize];
        matches!(node.state, NodeState::Present | NodeState::Absent).then_some(node)
    }

    /// The identity (full uri) of the node an address names — what a walk keys
    /// by. `None` when the graph shows no such node.
    pub fn uri_of(&self, address: &str) -> Option<&str> {
        self.node(address).map(|node| &*node.uri)
    }

    /// Every node the graph shows, present ones in inventory order.
    pub fn nodes(&self) -> impl Iterator<Item = &IndexNode> {
        self.nodes
            .iter()
            .filter(|node| matches!(node.state, NodeState::Present | NodeState::Absent))
            .inspect(|_| {
                self.work.nodes.fetch_add(1, Ordering::Relaxed);
            })
    }

    // -- Queries -----------------------------------------------------------

    /// The sheet of a property, by its address (a uri, or a bare uuid at the
    /// entry).
    pub fn sheet(&self, property: &str) -> Option<&PropertySheet> {
        let id = self.find(property)?;
        self.sheets.get(&id)
    }

    /// Every sheet, with the property's uri. Unordered.
    pub fn properties(&self) -> impl Iterator<Item = (&str, &PropertySheet)> {
        self.sheets
            .iter()
            .map(|(id, sheet)| (&*self.nodes[*id as usize].uri, sheet))
    }

    /// The properties hanging on a representation: `(property uri, sheet)`.
    ///
    /// **`property::read`'s membership rule, not the `.rels`'**: the candidates
    /// are the incoming edges whose path ends with `SupportingRepresentation`,
    /// the source of which has a sheet — only a property has one — and each is
    /// confirmed against the uuid the sheet itself records, so the answer
    /// cannot differ from scanning the properties and comparing
    /// `supporting_representation`. The representation need not be in the
    /// package: a property can name an absent one, and is then found.
    ///
    /// In inventory order of the properties.
    pub fn properties_of<'a>(
        &'a self,
        representation_uuid: &str,
    ) -> impl Iterator<Item = (&'a str, &'a PropertySheet)> + use<'a> {
        let wanted = representation_uuid.to_ascii_lowercase();
        let candidates: &'a [(NodeId, PathId)] = match self.id_of(&wanted) {
            Some(id) => self.inn[id as usize].as_slice(),
            None => &[],
        };
        candidates.iter().filter_map(move |&(source, path)| {
            if !self.strings.get(path).ends_with(SUPPORTING_REPRESENTATION) {
                return None;
            }
            let sheet = self.sheets.get(&source)?;
            let support = sheet.supporting_representation.as_deref()?;
            if !support.eq_ignore_ascii_case(&wanted) {
                return None;
            }
            Some((&*self.nodes[source as usize].uri, sheet))
        })
    }

    /// Who references this uuid — `graph::dependents`, from the index.
    ///
    /// One [`Pointer`] per reference. Same set as `graph::dependents` over the
    /// same objects; the order is by source node, then by path id, where
    /// `graph::dependents` gives the order the walk met them in.
    pub fn dependents_of(&self, uuid: &str) -> Vec<Pointer> {
        let Some(id) = self.id_of(&uuid.to_ascii_lowercase()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for &(source, path) in &self.inn[id as usize] {
            if source == id {
                continue; // graph::dependents skips the object itself
            }
            let node = &self.nodes[source as usize];
            if node.state != NodeState::Present {
                continue;
            }
            out.push(self.pointer(node, path));
        }
        out
    }

    /// What this uuid references — the cheap direction, from the index.
    ///
    /// Unlike `graph::references`, which reads the object and builds the uri from
    /// the **reference's** declared type, the target here is **the node it
    /// resolves to**: the object's real uri when the package holds it. The two
    /// agree unless the reference names the wrong type or version, which is what
    /// `check` reports.
    pub fn references_of(&self, uuid: &str) -> Vec<Pointer> {
        let Some(id) = self.id_of(&uuid.to_ascii_lowercase()) else {
            return Vec::new();
        };
        self.out[id as usize]
            .iter()
            .map(|&(target, path)| self.pointer(&self.nodes[target as usize], path))
            .collect()
    }

    fn pointer(&self, node: &IndexNode, path: PathId) -> Pointer {
        self.work.edges.fetch_add(1, Ordering::Relaxed);
        Pointer {
            uri: node.uri.to_string(),
            uuid: node.uuid.to_string(),
            qualified_type: node.qualified_type.to_string(),
            title: node.title.as_ref().map(|t| t.to_string()),
            path: self.strings.get(path).to_string(),
        }
    }

    // -- The graph ---------------------------------------------------------

    /// The graph, as `graph::build` would draw it.
    ///
    /// **Nodes**: the objects the package holds, in inventory order, then the
    /// objects only referenced, in the order the walk met them — the order
    /// `build` pushes them in. **Edges**: sorted by `(from, to, path)` and
    /// deduplicated, `build`'s last two lines. The sort is done on ids first so
    /// that the 319 244 `Edge` strings are made once, in their final order.
    pub fn graph(&self) -> Graph {
        let mut present: Vec<NodeId> = Vec::new();
        let mut absent: Vec<NodeId> = Vec::new();
        for (id, node) in self.nodes.iter().enumerate() {
            match node.state {
                NodeState::Present => present.push(id as NodeId),
                NodeState::Absent if self.holder(id as NodeId).is_none() => absent.push(id as NodeId),
                _ => {}
            }
        }
        absent.sort_by_key(|&id| (self.nodes[id as usize].seen, id));

        let nodes: Vec<Node> = present
            .iter()
            .chain(absent.iter())
            .map(|&id| self.graph_node(id))
            .collect();
        let mut triples: Vec<(NodeId, NodeId, PathId)> = Vec::with_capacity(self.edge_count());
        for &source in &present {
            self.edges_from(source, &mut triples);
        }
        Graph {
            nodes,
            edges: self.sorted_edges(triples),
        }
    }

    /// What [`PackageIndex::graph`] says **around these objects** — so that a
    /// copy of the graph held elsewhere is brought up to date for an edit of a
    /// few objects instead of being read again whole.
    ///
    /// The rule: adding an object should trigger graph or colour updates only for
    /// the objects it links to. A
    /// creation used to re-ask the whole graph, which a front end holding 62 000
    /// objects paid in full for one new property.
    ///
    /// For each address (a uri, through `PackageIndex::find`):
    ///
    /// - a node the graph shows: its node, and — when the package holds it —
    ///   its outgoing edges, with the nodes they reach;
    /// - an address that is no node of the graph, or that the index resolves
    ///   to a node of another uri: in `gone`.
    ///
    /// **Every edge and node here is the one `graph()` draws** — the same
    /// helpers build both, the nested parts and the dedup included. A copy
    /// applies it by dropping the edges that leave the asked addresses and the
    /// `sources`, dropping the `gone` nodes, setting the nodes and adding the
    /// edges. What it does **not** carry: the edges *into* an asked object from
    /// an object not asked. They do not move when that object is written —
    /// except for a node that was absent and is held now under **another uri**,
    /// which the caller detects by uuid and answers by reading the graph whole.
    pub fn graph_around(&self, addresses: &[String]) -> GraphAround {
        let mut ids: Vec<NodeId> = Vec::new();
        let mut sources: Vec<NodeId> = Vec::new();
        let mut gone: Vec<String> = Vec::new();
        for address in addresses {
            let shown = self
                .find(address)
                .filter(|&id| match self.nodes[id as usize].state {
                    NodeState::Present => true,
                    NodeState::Absent => self.holder(id).is_none(),
                    _ => false,
                });
            match shown {
                Some(id) => {
                    if !self.uri_str(id).eq_ignore_ascii_case(address.trim()) {
                        gone.push(address.clone());
                    }
                    ids.push(id);
                    if self.nodes[id as usize].state == NodeState::Present {
                        sources.push(id);
                    }
                }
                None => gone.push(address.clone()),
            }
        }
        sources.sort_unstable();
        sources.dedup();
        let mut triples: Vec<(NodeId, NodeId, PathId)> = Vec::new();
        for &source in &sources {
            self.edges_from(source, &mut triples);
        }
        ids.extend(triples.iter().map(|&(_, to, _)| to));
        let mut seen = std::collections::HashSet::new();
        ids.retain(|id| seen.insert(*id));
        GraphAround {
            graph: Graph {
                nodes: ids.iter().map(|&id| self.graph_node(id)).collect(),
                edges: self.sorted_edges(triples),
            },
            sources: sources.iter().map(|&id| self.uri_str(id).to_string()).collect(),
            gone,
        }
    }

    /// An absent node for an object nested in a present part is no node of the
    /// graph: references to it go to the part (see `nested`).
    fn holder(&self, id: NodeId) -> Option<NodeId> {
        let node = &self.nodes[id as usize];
        if node.state != NodeState::Absent {
            return None;
        }
        let holder = *self.nested.get(node.uuid.to_ascii_lowercase().as_str())?;
        (self.nodes[holder as usize].state == NodeState::Present).then_some(holder)
    }

    fn graph_node(&self, id: NodeId) -> Node {
        let node = &self.nodes[id as usize];
        Node {
            uri: node.uri.to_string(),
            uuid: node.uuid.to_string(),
            qualified_type: node.qualified_type.to_string(),
            type_key: node.type_key.to_string(),
            title: node.title.as_ref().map(|t| t.to_string()),
            present: node.state == NodeState::Present,
            externally_defined: node.externally_defined,
        }
    }

    /// The edges `graph()` draws out of one present node, as id triples.
    fn edges_from(&self, source: NodeId, triples: &mut Vec<(NodeId, NodeId, PathId)>) {
        for &(target, path) in self.out[source as usize].iter() {
            let target = match self.holder(target) {
                // Nested in this very object: no link between two parts.
                Some(part) if part == source => continue,
                Some(part) => part,
                None => target,
            };
            if matches!(
                self.nodes[target as usize].state,
                NodeState::Present | NodeState::Absent
            ) {
                triples.push((source, target, path));
            }
        }
    }

    /// `graph::build`'s last two lines: sorted by `(from, to, path)`, deduplicated.
    fn sorted_edges(&self, mut triples: Vec<(NodeId, NodeId, PathId)>) -> Vec<Edge> {
        let uri = |id: NodeId| self.uri_str(id);
        triples.sort_by(|a, b| {
            (uri(a.0), uri(a.1), self.strings.get(a.2)).cmp(&(uri(b.0), uri(b.1), self.strings.get(b.2)))
        });
        triples.dedup_by(|a, b| {
            uri(a.0) == uri(b.0) && uri(a.1) == uri(b.1) && self.strings.get(a.2) == self.strings.get(b.2)
        });
        triples
            .iter()
            .map(|&(from, to, path)| Edge {
                from: uri(from).to_string(),
                to: uri(to).to_string(),
                path: self.strings.get(path).to_string(),
            })
            .collect()
    }

    /// The graph as the JSON the front end receives — `Graph::to_json`, byte for byte.
    pub fn graph_json(&self) -> String {
        self.graph().to_json()
    }
}

/// What [`PackageIndex::graph_around`] answers: a piece of the graph, the
/// present objects whose outgoing edges it carries in full, and the addresses
/// the graph no longer shows.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphAround {
    /// The nodes of the asked objects and of what they reference, and the
    /// outgoing edges of the asked objects the package holds.
    pub graph: Graph,
    /// The uris of the asked objects whose outgoing edges `graph` carries in
    /// full — a copy drops every edge leaving them before adding these.
    pub sources: Vec<String>,
    /// The asked addresses that are no node of the graph (or resolve to a node
    /// of another uri) — a copy drops them.
    pub gone: Vec<String>,
}

impl GraphAround {
    /// `{ "nodes", "edges", "sources", "gone" }` — the nodes and edges written
    /// by `Graph::to_json`, so a front end reads them with the reader it has.
    pub fn to_json(&self) -> String {
        let graph = self.graph.to_json();
        let body = graph.trim_end().strip_suffix('}').unwrap_or(&graph).trim_end();
        let list = |items: &[String]| -> String {
            serde_json::to_string(items).unwrap_or_else(|_| "[]".to_string())
        };
        format!(
            "{body},\n  \"sources\": {},\n  \"gone\": {}\n}}",
            list(&self.sources),
            list(&self.gone)
        )
    }
}

impl PackageIndex {
    // -- Updating ----------------------------------------------------------

    /// Bring the index up to date with `store` after `changed` objects changed,
    /// re-reading **those objects only**.
    ///
    /// `changed` are uuids (any case): what the store's journal names, for an
    /// object put, edited in place, removed, or touched by an abandoned
    /// transaction (an abandoned transaction *names* its uuids, the state
    /// having changed twice). `revision` is the store revision the index is valid
    /// for afterwards.
    ///
    /// For each uuid: its outgoing edges and its sheet are dropped, then the
    /// object is read again if the store still holds it and its edges, title and
    /// sheet are taken afresh. Dangling targets follow `graph::build`'s rule:
    ///
    /// - a target the package does not hold **exists as an absent node only
    ///   while something references it** — when the last reference goes, the node
    ///   goes (a tombstone, so ids stay put);
    /// - an object that **leaves** the store but is still referenced does not
    ///   vanish: it becomes an absent node, typed and titled by the first
    ///   reference to it, exactly what a rebuild would make of it;
    /// - an absent node that the store now **holds** becomes a present one;
    /// - an absent node whose type and title were supplied by a reference that
    ///   just changed takes them from the next referrer.
    ///
    /// **Not identical to a rebuild in one respect**: a node added by an update
    /// goes at the end, and the order of absent nodes follows ids, so
    /// [`PackageIndex::graph`] after an update lists the same nodes and edges as
    /// a rebuild but not necessarily in its order. The test compares them sorted.
    /// When the journal overflows and answers *everything*, rebuild.
    ///
    /// Returns **what the update disturbed** — see
    /// [`PackageIndex::update_with`].
    ///
    /// **A uuid the store holds more than once** (another `objectVersion`,
    /// another release — an entry is (uuid, type, version)) is not one node to
    /// re-read: when a changed uuid is held more than once now, or was by the
    /// time the index was built, the index is **rebuilt** from the store, and
    /// what it returns is every uuid of the package, before and after — wider
    /// than any update would say, never narrower. Packages without namesakes
    /// never take that path.
    pub fn update(&mut self, store: &EpcStore, changed: &[String], revision: u64) -> Vec<String> {
        let namesakes = changed.iter().any(|raw| {
            let key = raw.trim().to_ascii_lowercase();
            self.shared.contains_key(key.as_str()) || store.summaries_of(&key).len() > 1
        });
        if !namesakes {
            return self.update_with(
                store,
                &|uuid| store.find_by_uuid(uuid).cloned(),
                changed,
                revision,
            );
        }
        let mut disturbed: Vec<String> = self
            .nodes
            .iter()
            .filter(|node| matches!(node.state, NodeState::Present | NodeState::Absent))
            .map(|node| node.uuid.to_ascii_lowercase())
            .collect();
        let mut fresh = PackageIndex::build(store, revision);
        fresh.complete = self.complete;
        disturbed.extend(
            fresh
                .nodes
                .iter()
                .filter(|node| matches!(node.state, NodeState::Present | NodeState::Absent))
                .map(|node| node.uuid.to_ascii_lowercase()),
        );
        disturbed.extend(changed.iter().map(|raw| raw.trim().to_ascii_lowercase()));
        *self = fresh;
        disturbed.sort();
        disturbed.dedup();
        disturbed
    }

    /// [`PackageIndex::update`] over any [`ObjectSource`]: `summary_of` says
    /// whether the store still holds a uuid, and what it is.
    ///
    /// # What it returns: the objects whose walks may have changed
    ///
    /// The lowercase uuids a **cache of what is derived from the links** must
    /// drop — a walk of the inheritance order (`inherit`), a colour resolved by
    /// it — sorted and without duplicates: each object that changed, and each
    /// object that **gained or lost a reference from one that changed** (the
    /// symmetric difference of its references before and after, path included).
    /// A walk reads an object's links in both directions — what it references
    /// (its parents) and who references it (its collections, its model) — so a
    /// walk that went through `Y` is affected by a new edge `X → Y` even though
    /// `Y` itself did not change.
    ///
    /// One refinement, because it is where the common case is: an association
    /// set (`CollectionsToDataobjectsAssociationSet`) that gains a member
    /// disturbs **that member**, not the collection and not the other members —
    /// nothing a member's walk read changed. When an association's
    /// *collection* changes, every member of the set is disturbed: their
    /// membership changed, and so does which repeated collection counts
    /// (common TRG 2.3 §8.3).
    pub fn update_with(
        &mut self,
        source: &dyn ObjectSource,
        summary_of: &dyn Fn(&str) -> Option<ObjectSummary>,
        changed: &[String],
        revision: u64,
    ) -> Vec<String> {
        let mut disturbed: Vec<String> = Vec::new();
        for raw in changed {
            let key = raw.trim().to_ascii_lowercase();
            self.refresh(source, summary_of, &key, &mut disturbed);
        }
        self.settle(source);
        self.revision = revision;
        disturbed.sort();
        disturbed.dedup();
        disturbed
    }

    /// The uuids a change of references disturbs: see [`PackageIndex::update_with`].
    fn disturbed_by(
        &self,
        id: Option<NodeId>,
        old: &[(NodeId, PathId)],
        new: &[(NodeId, PathId)],
        out: &mut Vec<String>,
    ) {
        use crate::inherit::{Part, parse_association};

        let uuid_of = |node: NodeId| self.nodes[node as usize].uuid.to_ascii_lowercase();
        let is_set = id
            .is_some_and(|id| &*self.nodes[id as usize].type_key == "CollectionsToDataobjectsAssociationSet");
        let old_set: std::collections::HashSet<(NodeId, PathId)> = old.iter().copied().collect();
        let new_set: std::collections::HashSet<(NodeId, PathId)> = new.iter().copied().collect();
        let mut collection_changed = false;
        for &(target, path) in old_set.symmetric_difference(&new_set) {
            match (is_set, parse_association(self.strings.get(path))) {
                (true, Some((_, Part::Collection))) => collection_changed = true,
                _ => out.push(uuid_of(target)),
            }
        }
        if collection_changed {
            for &(target, path) in old.iter().chain(new.iter()) {
                if let Some((_, Part::Member(_))) = parse_association(self.strings.get(path)) {
                    out.push(uuid_of(target));
                }
            }
        }
    }

    fn refresh(
        &mut self,
        source: &dyn ObjectSource,
        summary_of: &dyn Fn(&str) -> Option<ObjectSummary>,
        key: &str,
        disturbed: &mut Vec<String>,
    ) {
        disturbed.push(key.to_string());
        let existing = self.id_of(key);
        let old_out: Vec<(NodeId, PathId)> = match existing {
            Some(id) => self.out[id as usize].to_vec(),
            None => Vec::new(),
        };
        let former_targets = match existing {
            Some(id) => self.detach(id),
            None => Vec::new(),
        };
        self.unreadable
            .retain(|(uuid, _)| !uuid.eq_ignore_ascii_case(key));
        self.unsheeted.retain(|(uuid, _)| !uuid.eq_ignore_ascii_case(key));

        match summary_of(key) {
            Some(summary) => {
                let id = match existing {
                    Some(id) => {
                        self.restate(id, &summary);
                        id
                    }
                    None => self.add_present(&summary),
                };
                self.read_into(source, id);
            }
            None => {
                if let Some(id) = existing {
                    match self.nodes[id as usize].state {
                        // Already only a name in other objects' references: it
                        // stays one while they remain, the settle below decides.
                        NodeState::Absent => self.to_settle.push(id),
                        _ => {
                            self.nodes[id as usize].state = NodeState::Unreadable;
                            self.to_settle.push(id);
                        }
                    }
                }
            }
        }

        let id_now = self.id_of(key);
        let new_out: Vec<(NodeId, PathId)> = match id_now {
            Some(id) => self.out[id as usize].to_vec(),
            None => Vec::new(),
        };
        self.disturbed_by(id_now, &old_out, &new_out, disturbed);

        // Targets this object stopped naming: dropped when nothing names them,
        // retyped when this object was the one that supplied their type.
        if let Some(id) = existing {
            for target in former_targets {
                if self.nodes[target as usize].state != NodeState::Absent {
                    continue;
                }
                let supplied_by_us = (self.nodes[target as usize].seen >> 32) as NodeId == id;
                if self.inn[target as usize].is_empty() || supplied_by_us {
                    self.to_settle.push(target);
                }
            }
        }
    }

    /// Drop a node's outgoing edges and its sheet; return the nodes it pointed at.
    fn detach(&mut self, id: NodeId) -> Vec<NodeId> {
        let outgoing = std::mem::take(&mut self.out[id as usize]);
        self.nested.retain(|_, holder| *holder != id);
        let mut targets: Vec<NodeId> = Vec::with_capacity(outgoing.len());
        for &(target, _) in outgoing.iter() {
            self.inn[target as usize].retain(|&(source, _)| source != id);
            targets.push(target);
        }
        targets.sort_unstable();
        targets.dedup();
        self.sheets.remove(&id);
        // The rows this node laid in the relation tables go with its edges —
        // the same place a re-read puts them back.
        self.relations.forget(id, &outgoing, &self.strings);
        targets
    }

    // -- Shared by the builder and the update ------------------------------

    fn push_node(&mut self, node: IndexNode) -> NodeId {
        let id = self.nodes.len() as NodeId;
        self.nodes.push(node);
        self.out.push(Box::default());
        self.inn.push(Vec::new());
        id
    }

    fn node_of_summary(&mut self, summary: &ObjectSummary, state: NodeState) -> IndexNode {
        IndexNode {
            uri: summary.uri.as_str().into(),
            uuid: summary.uuid.as_str().into(),
            qualified_type: self.strings.arc(&summary.qualified_type),
            type_key: {
                let key = type_key_of(&summary.qualified_type);
                self.strings.arc(&key)
            },
            title: None,
            state,
            externally_defined: graph::is_externally_defined(&summary.qualified_type),
            seen: 0,
        }
    }

    fn add_present(&mut self, summary: &ObjectSummary) -> NodeId {
        let node = self.node_of_summary(summary, NodeState::Pending);
        let id = self.push_node(node);
        let key = summary.uuid.to_ascii_lowercase().into_boxed_str();
        match self.by_uuid.get(&key).copied() {
            // **The store holds this uuid more than once** — another version, or
            // another release. Every entry is a node of its own; the uuid answers
            // with one of them: the one that declares no version (its uri is the
            // short one), else the first.
            Some(first) if self.nodes[first as usize].state == NodeState::Pending => {
                let held = self.shared.entry(key.clone()).or_insert_with(|| vec![first]);
                held.push(id);
                if version_in_uri(&self.nodes[first as usize].uri).is_some()
                    && version_in_uri(&summary.uri).is_none()
                {
                    self.by_uuid.insert(key, id);
                }
            }
            _ => {
                self.by_uuid.insert(key, id);
            }
        }
        id
    }

    /// What to ask the source for a node's object: **its uri**, always — the
    /// full identity of this node, version included, whether or not the store
    /// holds the uuid twice (ADR-0003: internally, full uris only).
    fn handle_of(&self, id: NodeId) -> String {
        self.nodes[id as usize].uri.to_string()
    }

    /// The nodes a reference names, among the nodes of a uuid held more than
    /// once: by type when the type matches something (*a wrong version is not a
    /// wrong type*), by version when the reference carries one, else the one that
    /// declares none if exactly one does, **else all** — a link that is too wide
    /// costs a copy, one that is too narrow breaks a package.
    fn targets_among(&self, held: &[NodeId], edge: &identity::RefEdge<'_>) -> Vec<NodeId> {
        let version_of = |id: NodeId| -> Option<String> { version_in_uri(&self.nodes[id as usize].uri) };
        let declared = edge.target.qualified_type();
        let mut left: Vec<NodeId> = held.to_vec();
        if left
            .iter()
            .any(|&id| *self.nodes[id as usize].qualified_type == *declared)
        {
            left.retain(|&id| *self.nodes[id as usize].qualified_type == *declared);
        }
        let wanted = edge
            .target
            .object_version
            .map(str::trim)
            .filter(|version| !version.is_empty());
        match wanted {
            Some(wanted) => {
                if left.iter().any(|&id| version_of(id).as_deref() == Some(wanted)) {
                    left.retain(|&id| version_of(id).as_deref() == Some(wanted));
                }
            }
            None if left.len() > 1 => {
                let bare: Vec<NodeId> = left
                    .iter()
                    .copied()
                    .filter(|&id| version_of(id).is_none())
                    .collect();
                if let [only] = bare[..] {
                    return vec![only];
                }
            }
            None => {}
        }
        left
    }

    /// A node that was absent (or stale) becomes the node of this summary.
    fn restate(&mut self, id: NodeId, summary: &ObjectSummary) {
        let fresh = self.node_of_summary(summary, NodeState::Pending);
        self.nodes[id as usize] = fresh;
        // A Gone tombstone that comes back must be findable again.
        self.by_uuid
            .insert(summary.uuid.to_ascii_lowercase().into_boxed_str(), id);
    }

    /// Read the object of node `id`, take its links and sheet, **let it go**.
    fn read_into(&mut self, source: &dyn ObjectSource, id: NodeId) {
        let uuid = self.nodes[id as usize].uuid.to_string();
        match source.read_object_at(&self.handle_of(id)) {
            Ok(object) => self.absorb(id, object.as_reflect()),
            Err(why) => {
                self.nodes[id as usize].state = NodeState::Unreadable;
                self.unreadable.push((uuid, why));
                self.to_settle.push(id);
            }
        }
    }

    /// Take from one object everything the index keeps. The caller drops the
    /// object as soon as this returns.
    fn absorb(&mut self, id: NodeId, object: &dyn Reflect) {
        let title: Option<Arc<str>> = graph::title_of(object).map(|t| Arc::from(t.as_str()));
        self.nodes[id as usize].title = title.clone();
        self.nodes[id as usize].state = NodeState::Present;

        for nested in identity::nested_objects(object) {
            self.nested
                .insert(nested.key.uuid.to_ascii_lowercase().into_boxed_str(), id);
        }
        let mut outgoing: Vec<(NodeId, PathId)> = Vec::new();
        for (n, edge) in identity::collect_refs(object).into_iter().enumerate() {
            if edge.target.uuid.is_empty() {
                continue;
            }
            let key = edge.target.uuid.to_ascii_lowercase();
            // A uuid the store holds more than once: the reference names one of
            // its entries by type and version — or, naming no version, all.
            if let Some(held) = self.shared.get(key.as_str()) {
                let targets = self.targets_among(held, &edge);
                let path = self.strings.id(&edge.path);
                outgoing.extend(targets.into_iter().map(|target| (target, path)));
                continue;
            }
            let target = match self.id_of(&key) {
                Some(target) => target,
                None => {
                    // Referenced and not in the package: a node saying so, made
                    // from the reference — `graph::build`'s `or_insert_with`.
                    let node = self.absent_node(&edge, ((id as u64) << 32) | n as u64);
                    let target = self.push_node(node);
                    self.by_uuid.insert(key.into_boxed_str(), target);
                    target
                }
            };
            let path = self.strings.id(&edge.path);
            outgoing.push((target, path));
        }
        // `build` sorts and deduplicates (from, to, path); ids are a finer key,
        // and `graph()` dedups again on the text for the duplicated-uuid case.
        outgoing.sort_unstable();
        outgoing.dedup();
        for &(target, path) in &outgoing {
            self.inn[target as usize].push((id, path));
        }
        // The relations the walks read, from the references just taken and the
        // two facts the object shows — no second read of it (§4.5).
        let facts = relations_facts(&self.nodes[id as usize], object, &mut self.strings);
        self.relations
            .take(&self.nodes, &self.strings, id, &outgoing, facts);
        self.out[id as usize] = outgoing.into_boxed_slice();

        if self.nodes[id as usize].qualified_type.ends_with("Property") {
            match property::read(object) {
                Ok(read) => {
                    let sheet = self.sheet_of(&read, object, title);
                    self.sheets.insert(id, sheet);
                }
                Err(why) => {
                    let uuid = self.nodes[id as usize].uuid.to_string();
                    self.unsheeted.push((uuid, why.to_string()));
                }
            }
        }
    }

    fn absent_node(&mut self, edge: &identity::RefEdge<'_>, seen: u64) -> IndexNode {
        let qualified_type = edge.target.qualified_type();
        IndexNode {
            uri: EtpUri::object(None, &qualified_type, edge.target.uuid).into_boxed_str(),
            uuid: edge.target.uuid.into(),
            type_key: self
                .strings
                .arc(naming::strip_obj_prefix(edge.target.object_type)),
            externally_defined: graph::is_externally_defined(&qualified_type),
            qualified_type: self.strings.arc(&qualified_type),
            title: edge.target.title.map(Arc::from),
            state: NodeState::Absent,
            seen,
        }
    }

    fn sheet_of(
        &mut self,
        read: &Property<'_>,
        object: &dyn Reflect,
        node_title: Option<Arc<str>>,
    ) -> PropertySheet {
        let title = match (&read.title, node_title) {
            (None, _) => None,
            (Some(own), Some(shared)) if &*shared == own.as_str() => Some(shared),
            (Some(own), _) => Some(Arc::from(own.as_str())),
        };
        let kind = match &read.kind {
            None => None,
            Some(PropertyKindRef::Standard { name }) => Some(SheetKind::Standard(self.strings.arc(name))),
            Some(PropertyKindRef::Object { uuid, title }) => {
                let uuid = self.strings.arc(uuid);
                let title = title.as_deref().map(|t| self.strings.arc(t));
                Some(SheetKind::Object { uuid, title })
            }
        };
        let values_path = values::locate(object)
            .into_iter()
            .next()
            .map(|found| self.strings.arc(&found.path));
        PropertySheet {
            uuid: read.uuid.as_str().into(),
            title,
            qualified_type: self.strings.arc(&read.qualified_type),
            indexable: read.indexable,
            value_count_per_element: read.value_count_per_element.clone().into_boxed_slice(),
            uom: read.uom.as_deref().map(|u| self.strings.arc(u)),
            kind,
            supporting_representation: read
                .supporting_representation
                .as_deref()
                .map(|s| self.strings.arc(s)),
            lookup: read.lookup.as_deref().map(|l| self.strings.arc(l)),
            time: read.time.clone().map(Box::new),
            realization: read.realization,
            has_realization_indices: read.has_realization_indices,
            values_path,
        }
    }

    /// Decide what the nodes queued in `to_settle` are.
    ///
    /// A node that is referenced and not readable (or no longer held) is an
    /// **absent node typed and titled by its first referrer** — the lowest id,
    /// which in a build is the first in inventory order, which is where
    /// `graph::build`'s walk meets it first. A node nothing references goes.
    fn settle(&mut self, source: &dyn ObjectSource) {
        let queue = std::mem::take(&mut self.to_settle);
        for id in queue {
            let state = self.nodes[id as usize].state;
            if !matches!(state, NodeState::Unreadable | NodeState::Absent) {
                continue;
            }
            let referrer = self.inn[id as usize].iter().map(|&(source, _)| source).min();
            let Some(referrer) = referrer else {
                self.forget(id);
                continue;
            };
            let wanted = self.nodes[id as usize].uuid.to_ascii_lowercase();
            let referrer_uuid = self.handle_of(referrer);
            let found = match source.read_object_at(&referrer_uuid) {
                Ok(object) => {
                    let mut found = None;
                    for (n, edge) in identity::collect_refs(object.as_reflect())
                        .into_iter()
                        .enumerate()
                    {
                        if edge.target.uuid.to_ascii_lowercase() == wanted {
                            found = Some(self.absent_node(&edge, ((referrer as u64) << 32) | n as u64));
                            break;
                        }
                    }
                    found
                }
                Err(_) => None,
            };
            match found {
                Some(node) => self.nodes[id as usize] = node,
                // The referrer cannot be read back: keep the node, absent, with
                // what it had. Better a node with a stale label than none.
                None => self.nodes[id as usize].state = NodeState::Absent,
            }
        }
    }

    /// Turn a node into a tombstone.
    fn forget(&mut self, id: NodeId) {
        let key = self.nodes[id as usize].uuid.to_ascii_lowercase();
        if self.by_uuid.get(key.as_str()) == Some(&id) {
            self.by_uuid.remove(key.as_str());
        }
        self.sheets.remove(&id);
        let node = &mut self.nodes[id as usize];
        node.state = NodeState::Gone;
        node.title = None;
    }

    // -- For the negative controls of the tests ----------------------------

    /// **Test sabotage**: forget the first outgoing edge of a node, in both
    /// directions. Exists so a test can prove that its confrontation with
    /// `graph::build` *can* fail (the rule of `tests/colouring.rs`).
    #[doc(hidden)]
    pub fn sabotage_forget_an_edge(&mut self, uuid: &str) -> bool {
        let Some(id) = self.id_of(&uuid.to_ascii_lowercase()) else {
            return false;
        };
        let mut edges: Vec<(NodeId, PathId)> = self.out[id as usize].to_vec();
        if edges.is_empty() {
            return false;
        }
        let (target, path) = edges.remove(0);
        if let Some(at) = self.inn[target as usize].iter().position(|&e| e == (id, path)) {
            self.inn[target as usize].remove(at);
        }
        self.out[id as usize] = edges.into_boxed_slice();
        true
    }

    /// **Test sabotage**: forget a property's sheet.
    #[doc(hidden)]
    pub fn sabotage_forget_a_sheet(&mut self, uuid: &str) -> bool {
        match self.id_of(&uuid.to_ascii_lowercase()) {
            Some(id) => self.sheets.remove(&id).is_some(),
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// The builder
// ---------------------------------------------------------------------------

/// How far a pass has got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    /// Objects read so far.
    pub done: usize,
    /// Objects to read.
    pub total: usize,
}

impl Progress {
    /// Whether the pass is over.
    pub fn is_done(&self) -> bool {
        self.done >= self.total
    }
}

/// The streaming pass that makes a [`PackageIndex`].
///
/// # Why the nodes are made first
///
/// `graph::build` pushes a node for every object, **then** walks the edges and
/// appends the objects that are only referenced. Making the inventory's nodes up
/// front reproduces that order whatever the slices, and lets a reference to an
/// object not read yet resolve to the right node instead of being taken for an
/// absent one.
///
/// # Unreadable objects
///
/// a front end's graph builder skipped an object it could not read, so it had no node —
/// unless something referenced it, in which case the reference made one of its
/// own (typed and titled as the reference says). The builder does the same: at
/// the end of the pass, an unreadable object that is referenced becomes that
/// absent node (see [`PackageIndex::update`] for how), one that nothing
/// references disappears. Either way it is listed in
/// [`PackageIndex::unreadable`].
pub struct IndexBuilder {
    index: PackageIndex,
    total: usize,
    cursor: usize,
    settled: bool,
}

impl IndexBuilder {
    /// Begin a pass over an inventory. `revision` is the store revision the
    /// finished index will be valid for.
    ///
    /// Takes the summaries by value and keeps none of them: a node holds the few
    /// strings it needs, and the `Vec` is freed on return.
    pub fn new(summaries: Vec<ObjectSummary>, revision: u64) -> Self {
        let mut index = PackageIndex::empty(revision);
        index.nodes.reserve(summaries.len());
        index.by_uuid.reserve(summaries.len());
        let total = summaries.len();
        for summary in &summaries {
            index.add_present(summary);
        }
        Self {
            index,
            total,
            cursor: 0,
            settled: false,
        }
    }

    /// How many objects the pass reads.
    pub fn total(&self) -> usize {
        self.total
    }

    /// Where the pass is.
    pub fn progress(&self) -> Progress {
        Progress {
            done: self.cursor,
            total: self.total,
        }
    }

    /// Read the next `n` objects, in inventory order.
    ///
    /// Each object is dropped as soon as its links and sheet are taken; at most
    /// one lives at a time. When the last one is done the pass settles the
    /// unreadable nodes (this is the one moment it may read a referrer a second
    /// time, and only for an unreadable object that is referenced).
    pub fn step(&mut self, source: &dyn ObjectSource, n: usize) -> Progress {
        let end = self.cursor.saturating_add(n).min(self.total);
        while self.cursor < end {
            self.index.read_into(source, self.cursor as NodeId);
            self.cursor += 1;
        }
        if self.cursor >= self.total && !self.settled {
            // Unreadable nodes queued during the pass are settled now that every
            // referrer's edges are in.
            self.index.settle(source);
            self.settled = true;
            self.index.complete = true;
        }
        self.progress()
    }

    /// The index. Meant for a pass that is over: an unfinished one returns what
    /// it has, `is_complete() == false`, its unread objects absent from the graph.
    pub fn finish(mut self) -> PackageIndex {
        if !self.settled {
            self.index.complete = false;
        }
        self.index
    }
}
