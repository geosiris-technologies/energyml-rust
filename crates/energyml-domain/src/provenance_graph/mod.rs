//! The provenance graph of a package: what each activity read and wrote, how
//! the activities nest, what carries each object, and in what order it all
//! happened.
//!
//! One composition, [`of_package`], so that the desktop and the studio build the
//! same graph without copying the chain (no face holds logic). Each stage is a
//! function of its own and is public:
//!
//! - [`activity_edges`]: the edges one activity states by its parameters and its `Parent`;
//! - [`activity_tree`]: the tree of activities, confronted with its two writings;
//! - [`workflow_steps`]: the steps a workflow template declares, held ones included;
//! - [`carriers`]: the representations, interpretations and features that carry an object;
//! - [`PackageIndex::expand_collection`]: the members of a `DataobjectCollection`;
//! - [`assemble`]: the nodes, deduplicated by full uri, the edges, the issues;
//! - [`focus`]: the graph around one node;
//! - [`timeline`]: the order, measured or not - and it says which.
//!
//! **Nothing here refuses the graph.** What departs from a template, what two
//! writings disagree on, what the package lacks, is a [`GraphIssue`] beside the
//! graph (rule of 16/10: detect and signal). A role nobody declares is
//! [`ActivityRole::Undeclared`], drawn as a participant without a direction,
//! never guessed from a title.
//!
//! Identity is the full uri, everywhere inside: a bare uuid is accepted only at
//! the entry (an activity that names an object by uuid), and [`assemble`]
//! changes it at once into the index's own uri.

mod carriers;
mod collections;
mod steps;
mod timeline;
mod tree;

use std::collections::{HashMap, HashSet};

use energyml_core::identifiers::EtpUri;
use energyml_core::{identity, naming};

use crate::activity::{
    ActivityRecord, ActivityRole, ParameterIssue, ParameterRole, ParameterValue, RoleBasis, RoleRules,
    TemplateRecord, conformance as conformance_of, descriptor_of, read_activity, read_template,
    roles as roles_of,
};
use crate::package_index::{NodeState, PackageIndex};
use crate::provenance_verbatim::{VerbatimId, VerbatimRefusal, VerbatimSources, verbatim_edges};
use crate::source::ArraySource;
use crate::table::Table;

pub use carriers::{CarrierRule, CarrierRules, carriers};
pub use collections::ExpandRefusal;
pub use steps::{
    DeclaredStep, InputFileReader, NoInputFiles, NoStepConstraints, StepConstraint, StepConstraintReader,
    workflow_steps,
};
pub use timeline::{OrderBasis, Timeline, TimelineRefusal, TimelineRow, timeline};
pub use tree::{ActivityTree, TreeRefusal, TreeShape, activity_tree};

/// The type keys set aside by default (concept without package and `obj_`), as
/// the mockup does, less `ColumnBasedTable`: a dictionary table cannot be told
/// from a data table by its type alone.
pub const DEFAULT_SET_ASIDE: &[&str] = &[
    "LocalEngineeringCompoundCrs",
    "LocalEngineering2dCrs",
    "VerticalCrs",
    "LocalDepth3dCrs",
    "LocalTime3dCrs",
    "ReferencePointInALocalEngineeringCompoundCrs",
    "PropertyKind",
    "ActivityTemplate",
];

/// The depth bound of [`focus`] and [`carriers`] (0 to 64).
pub const MAX_DEPTH: u8 = 64;

// -- The types -----------------------------------------------------------------

/// What a node of the graph is keyed by - deduplication is by this key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NodeKey {
    /// An object or an activity: its full uri (canonical through the index when one is given).
    Object(String),
    /// A file path, as written (never normalised: `./a.las` and `a.las` are two nodes).
    File(String),
    /// A declared step of a workflow that did not run.
    HeldStep {
        /// The workflow template's uri.
        workflow: String,
        /// The step's title.
        step: String,
    },
    /// A value of a verbatim source that nothing resolves.
    Unresolved {
        /// The source that wrote it.
        source: VerbatimId,
        /// The value, as written.
        label: String,
    },
}

impl NodeKey {
    /// What the key names, as text: the uri, the path, the step, the label.
    pub fn name(&self) -> &str {
        match self {
            NodeKey::Object(uri) => uri,
            NodeKey::File(path) => path,
            NodeKey::HeldStep { step, .. } => step,
            NodeKey::Unresolved { label, .. } => label,
        }
    }
}

/// A node.
#[derive(Debug, Clone, PartialEq)]
pub enum ProvNode {
    /// A path, not an object: no uri, nothing to open.
    File {
        /// The path as written.
        path: String,
    },
    /// An activity.
    Activity {
        /// Its full uri.
        uri: String,
        /// `Citation.Title`.
        title: Option<String>,
        /// The template it names, as written.
        template: Option<String>,
    },
    /// A declared step that did not run, with its reason.
    HeldStep {
        /// The workflow template's uri.
        workflow: String,
        /// The step's title.
        step: String,
        /// Why it was held, as the constraint says.
        reason: String,
    },
    /// An object.
    Object {
        /// Its full uri.
        uri: String,
        /// Its qualified type (empty when nothing says it).
        qualified_type: String,
        /// Its title, when the index knows it.
        title: Option<String>,
        /// Whether the package holds it.
        present: bool,
    },
    /// A `DataobjectCollection`.
    Collection {
        /// Its full uri.
        uri: String,
        /// Its title, when the index knows it.
        title: Option<String>,
        /// How many members the association sets list; `None` when nothing states it.
        members: Option<usize>,
        /// Whether the caller expanded it.
        expanded: bool,
    },
    /// A value nothing resolves.
    Unresolved {
        /// The value, as written.
        label: String,
        /// The source that wrote it.
        source: VerbatimId,
    },
}

impl ProvNode {
    /// The key this node is deduplicated by.
    pub fn key(&self) -> NodeKey {
        match self {
            ProvNode::File { path } => NodeKey::File(path.clone()),
            ProvNode::Activity { uri, .. }
            | ProvNode::Object { uri, .. }
            | ProvNode::Collection { uri, .. } => NodeKey::Object(uri.clone()),
            ProvNode::HeldStep { workflow, step, .. } => NodeKey::HeldStep {
                workflow: workflow.clone(),
                step: step.clone(),
            },
            ProvNode::Unresolved { label, source } => NodeKey::Unresolved {
                source: *source,
                label: label.clone(),
            },
        }
    }
}

/// What an edge says.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProvEdgeKind {
    /// Object (or file) to activity: the activity read it.
    Consumed,
    /// Activity to object: the activity wrote it.
    Produced,
    /// Activity and object, no direction stated (`Neither`, `Undeclared`).
    Participant,
    /// Parent activity to child activity (or held step).
    Parent,
    /// Object to the representation, interpretation or feature that carries it.
    Carrier {
        /// The path of the reference that carries it.
        path: String,
    },
    /// Collection to member.
    Member,
}

/// Where an edge comes from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EdgeBasis {
    /// An activity's parameter, with the basis of its role.
    Activity(RoleBasis),
    /// A verbatim source only.
    Verbatim(VerbatimId),
    /// A reference field of an object (`Parent`, a carrier, a collection set).
    Reference,
    /// Both an activity and a verbatim source state it.
    Both(VerbatimId),
}

/// An edge.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProvEdge {
    /// The node it leaves.
    pub from: NodeKey,
    /// The node it reaches.
    pub to: NodeKey,
    /// What it says.
    pub kind: ProvEdgeKind,
    /// Where it comes from.
    pub basis: EdgeBasis,
}

/// Which of the two writings of the tree states a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Statement {
    /// Only the child's `Parent` says it.
    ParentOnly,
    /// Only a sub-activity parameter of the parent says it.
    ParameterOnly,
}

/// Something the graph signals: never a refusal of the graph.
#[derive(Debug, Clone, PartialEq)]
pub enum GraphIssue {
    /// A refusal of the tree (cycle, absent parent).
    Tree(TreeRefusal),
    /// `Parent` and the parent's sub-activity parameters disagree.
    TreeDisagreement {
        /// The parent activity's uri.
        parent: String,
        /// The child activity's uri.
        child: String,
        /// The only writing that states the link.
        stated_by: Statement,
    },
    /// A step declared held that ran anyway.
    HeldButPlayed {
        /// The workflow template's uri.
        workflow: String,
        /// The step's title.
        step: String,
    },
    /// An object an activity says it wrote, which the package does not hold.
    OutputAbsent {
        /// The activity's uri.
        activity: String,
        /// The object's uri.
        object: String,
    },
    /// A parameter departs from its template.
    Conformance {
        /// The activity's uri.
        activity: String,
        /// What departs.
        issue: ParameterIssue,
    },
    /// A verbatim process is tied to two different activities by two paths.
    ActivityLinkDiffers {
        /// The source that wrote it.
        source: VerbatimId,
        /// The process's key.
        process: String,
        /// The activity the processes table names.
        by_column: Option<String>,
        /// The activity the mapping table names.
        by_mapping: Option<String>,
    },
    /// A verbatim process names no activity: its edges are left out.
    ProcessWithoutActivity {
        /// The source that wrote it.
        source: VerbatimId,
        /// The process's key.
        process: String,
    },
    /// A verbatim source refused.
    Verbatim(VerbatimRefusal),
    /// An activity or a template could not be read.
    Unreadable {
        /// The object's uri.
        uri: String,
        /// Why.
        why: String,
    },
    /// A collection could not be expanded.
    Expand(ExpandRefusal),
}

/// An object set aside by type (a CRS, a property kind): not a node, listed.
#[derive(Debug, Clone, PartialEq)]
pub struct SetAside {
    /// Its full uri.
    pub uri: String,
    /// Its qualified type.
    pub qualified_type: String,
    /// What it was linked to: the other end of the edge left out.
    pub named_by: String,
}

/// The graph.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProvenanceGraph {
    /// The nodes: the activities first, then the ends of the edges in order.
    pub nodes: Vec<ProvNode>,
    /// The edges, deduplicated by `(from, to, kind)`.
    pub edges: Vec<ProvEdge>,
    /// What the graph signals.
    pub issues: Vec<GraphIssue>,
    /// The objects set aside by type.
    pub set_aside: Vec<SetAside>,
}

impl ProvenanceGraph {
    /// The position of a node, by key.
    pub fn position(&self, key: &NodeKey) -> Option<usize> {
        self.nodes.iter().position(|node| &node.key() == key)
    }
}

// -- Shared helpers --------------------------------------------------------------

/// The activity of `activities` an address names: same uuid, versions that agree
/// (the rule of the library's one reading of an address); the first of an exact
/// version, else the first compatible.
pub(crate) fn resolve_activity(address: &str, activities: &[ActivityRecord]) -> Option<usize> {
    let (uuid, version) = identity::parse_address(address);
    let mut best: Option<(identity::VersionFit, usize)> = None;
    for (at, activity) in activities.iter().enumerate() {
        let (held, held_version) = identity::parse_address(&activity.uri);
        if held != uuid {
            continue;
        }
        let fit = identity::version_fit(version.as_deref(), held_version.as_deref());
        if fit == identity::VersionFit::Contradicts {
            continue;
        }
        if best.is_none_or(|(kept, _)| fit < kept) {
            best = Some((fit, at));
        }
    }
    best.map(|(_, at)| at)
}

/// The concept of a qualified type, without package or `obj_`.
fn concept_of(qualified_type: &str) -> String {
    identity::parse_qualified_type(qualified_type)
        .map(|parts| naming::strip_obj_prefix(parts.object_type).to_string())
        .unwrap_or_else(|_| qualified_type.to_string())
}

fn edge(from: NodeKey, to: NodeKey, kind: ProvEdgeKind, basis: EdgeBasis) -> ProvEdge {
    ProvEdge {
        from,
        to,
        kind,
        basis,
    }
}

// -- Edges of an activity ----------------------------------------------------------

/// The edges an activity states by its parameters and its `Parent`.
///
/// `roles` is the role of each parameter (matched by `position`; a parameter
/// with no role is `Undeclared`). An object parameter becomes an edge by its
/// role; a text parameter becomes a `Consumed` edge from a file **only** when
/// `files` recognises it; anything else (a number, a time index) is not a node.
pub fn activity_edges(
    activity: &ActivityRecord,
    roles: &[ParameterRole],
    files: &dyn InputFileReader,
) -> Vec<ProvEdge> {
    let me = NodeKey::Object(activity.uri.clone());
    let mut edges = Vec::new();
    for parameter in &activity.parameters {
        let (role, role_basis) = roles
            .iter()
            .find(|found| found.position == parameter.position)
            .map(|found| (found.role, found.basis))
            .unwrap_or((ActivityRole::Undeclared, RoleBasis::None));
        let basis = EdgeBasis::Activity(role_basis);
        match &parameter.value {
            ParameterValue::Object(reference) => {
                let them = NodeKey::Object(reference.address.clone());
                match role {
                    ActivityRole::Input => {
                        edges.push(edge(them, me.clone(), ProvEdgeKind::Consumed, basis));
                    }
                    ActivityRole::Output => {
                        edges.push(edge(me.clone(), them, ProvEdgeKind::Produced, basis));
                    }
                    ActivityRole::Both => {
                        edges.push(edge(
                            them.clone(),
                            me.clone(),
                            ProvEdgeKind::Consumed,
                            basis.clone(),
                        ));
                        edges.push(edge(me.clone(), them, ProvEdgeKind::Produced, basis));
                    }
                    ActivityRole::Neither | ActivityRole::Undeclared => {
                        edges.push(edge(me.clone(), them, ProvEdgeKind::Participant, basis));
                    }
                    ActivityRole::SubActivity => {
                        edges.push(edge(me.clone(), them, ProvEdgeKind::Parent, basis));
                    }
                }
            }
            ParameterValue::Text(_) => {
                if let Some(path) = files.file(activity, parameter) {
                    edges.push(edge(
                        NodeKey::File(path),
                        me.clone(),
                        ProvEdgeKind::Consumed,
                        EdgeBasis::Activity(RoleBasis::Rule(files.id())),
                    ));
                }
            }
            _ => {}
        }
    }
    if let Some(parent) = &activity.parent {
        edges.push(edge(
            NodeKey::Object(parent.address.clone()),
            me,
            ProvEdgeKind::Parent,
            EdgeBasis::Reference,
        ));
    }
    edges
}

// -- Assembling the graph ------------------------------------------------------------

/// What [`assemble`] takes.
pub struct Parts<'a> {
    /// The activities, in input order.
    pub activities: &'a [ActivityRecord],
    /// All edges: activity edges, verbatim edges, carriers, `Member` edges, held-step edges.
    pub edges: Vec<ProvEdge>,
    /// Declared steps (for the reason of a `HeldStep` node).
    pub steps: &'a [DeclaredStep],
    /// The index: presence, types, titles, canonical uris. `None` in tests without a package.
    pub index: Option<&'a PackageIndex>,
    /// Type keys set aside ([`DEFAULT_SET_ASIDE`] by default).
    pub set_aside: &'a [&'a str],
    /// Collections the caller expanded (uris).
    pub expanded: &'a [String],
    /// Issues found upstream, carried into the graph.
    pub issues: Vec<GraphIssue>,
}

/// The pieces of an object node, from the index or, failing that, its uri.
struct Described {
    qualified_type: String,
    type_key: String,
    title: Option<String>,
    present: bool,
}

fn describe(index: Option<&PackageIndex>, uri: &str) -> Described {
    if let Some(node) = index.and_then(|held| held.node(uri)) {
        return Described {
            qualified_type: node.qualified_type.to_string(),
            type_key: node.type_key.to_string(),
            title: node.title.as_ref().map(|title| title.to_string()),
            present: node.state == NodeState::Present,
        };
    }
    let qualified_type = EtpUri::parse(uri)
        .ok()
        .and_then(
            |parsed| match (parsed.domain, parsed.version, parsed.object_type) {
                (Some(domain), Some(version), Some(object_type)) => {
                    Some(format!("{domain}{version}.{object_type}"))
                }
                _ => None,
            },
        )
        .unwrap_or_default();
    Described {
        type_key: concept_of(&qualified_type),
        qualified_type,
        title: None,
        present: false,
    }
}

/// Two bases for the same edge: an activity and a verbatim source make `Both`;
/// otherwise the first is kept.
fn merge_basis(kept: &EdgeBasis, other: &EdgeBasis) -> EdgeBasis {
    match (kept, other) {
        (EdgeBasis::Activity(_), EdgeBasis::Verbatim(source))
        | (EdgeBasis::Verbatim(source), EdgeBasis::Activity(_)) => EdgeBasis::Both(*source),
        _ => kept.clone(),
    }
}

/// Assemble the graph: canonical keys, nodes, deduplicated edges, objects set
/// aside, and what the package lacks.
///
/// 1. With an index, every `Object` key becomes the index's own uri for it (an
///    activity that names an object by a bare uuid and another by a versioned
///    uri meet in one node). Without one, keys stay as written.
/// 2. Nodes: the activities, then the ends of the edges, in order. An object
///    whose type key is in `set_aside` is **no node**: its edges are taken out and
///    it is listed in [`ProvenanceGraph::set_aside`].
/// 3. Edges are deduplicated by `(from, to, kind)`; an activity and a verbatim
///    source that state the same edge make it [`EdgeBasis::Both`].
/// 4. An object an activity wrote that the package does not hold is a
///    [`GraphIssue::OutputAbsent`].
pub fn assemble(parts: Parts<'_>) -> ProvenanceGraph {
    let Parts {
        activities,
        edges,
        steps,
        index,
        set_aside,
        expanded,
        issues,
    } = parts;

    let canon_uri = |address: &str| -> String {
        index
            .and_then(|held| held.uri_of(address))
            .unwrap_or(address)
            .to_string()
    };
    let canon = |key: &NodeKey| -> NodeKey {
        match key {
            NodeKey::Object(address) => NodeKey::Object(canon_uri(address)),
            other => other.clone(),
        }
    };

    let activity_uris: Vec<String> = activities
        .iter()
        .map(|activity| canon_uri(&activity.uri))
        .collect();
    let activity_keys: HashSet<NodeKey> = activity_uris
        .iter()
        .map(|uri| NodeKey::Object(uri.clone()))
        .collect();

    // Edges: canonical keys, objects set aside taken out, duplicates merged.
    let mut kept: Vec<ProvEdge> = Vec::new();
    let mut at_of: HashMap<(NodeKey, NodeKey, ProvEdgeKind), usize> = HashMap::new();
    let mut aside: Vec<SetAside> = Vec::new();
    for edge in edges {
        let from = canon(&edge.from);
        let to = canon(&edge.to);
        let mut left_out = false;
        for (this, other) in [(&from, &to), (&to, &from)] {
            if let NodeKey::Object(uri) = this
                && !activity_keys.contains(this)
            {
                let described = describe(index, uri);
                if set_aside.contains(&described.type_key.as_str()) {
                    let entry = SetAside {
                        uri: uri.clone(),
                        qualified_type: described.qualified_type,
                        named_by: other.name().to_string(),
                    };
                    if !aside.contains(&entry) {
                        aside.push(entry);
                    }
                    left_out = true;
                }
            }
        }
        if left_out {
            continue;
        }
        let key = (from.clone(), to.clone(), edge.kind.clone());
        match at_of.get(&key) {
            Some(at) => {
                let merged = merge_basis(&kept[*at].basis, &edge.basis);
                kept[*at].basis = merged;
            }
            None => {
                at_of.insert(key, kept.len());
                kept.push(ProvEdge {
                    from,
                    to,
                    kind: edge.kind,
                    basis: edge.basis,
                });
            }
        }
    }

    // Nodes: the activities, then the ends of the edges.
    let mut nodes: Vec<ProvNode> = Vec::new();
    let mut placed: HashSet<NodeKey> = HashSet::new();
    for (activity, uri) in activities.iter().zip(&activity_uris) {
        if placed.insert(NodeKey::Object(uri.clone())) {
            nodes.push(ProvNode::Activity {
                uri: uri.clone(),
                title: activity.title.clone(),
                template: activity
                    .descriptor
                    .as_ref()
                    .map(|descriptor| descriptor.address.clone()),
            });
        }
    }
    for edge in &kept {
        for key in [&edge.from, &edge.to] {
            if !placed.insert(key.clone()) {
                continue;
            }
            nodes.push(match key {
                NodeKey::Object(uri) => {
                    let described = describe(index, uri);
                    if described.type_key == "DataobjectCollection" {
                        ProvNode::Collection {
                            uri: uri.clone(),
                            title: described.title,
                            members: index
                                .and_then(|held| held.expand_collection(uri, usize::MAX).ok())
                                .map(|members| members.len()),
                            expanded: expanded.iter().any(|asked| canon_uri(asked) == *uri),
                        }
                    } else {
                        ProvNode::Object {
                            uri: uri.clone(),
                            qualified_type: described.qualified_type,
                            title: described.title,
                            present: described.present,
                        }
                    }
                }
                NodeKey::File(path) => ProvNode::File { path: path.clone() },
                NodeKey::HeldStep { workflow, step } => ProvNode::HeldStep {
                    workflow: workflow.clone(),
                    step: step.clone(),
                    reason: steps
                        .iter()
                        .find(|declared| declared.workflow == *workflow && declared.title == *step)
                        .and_then(|declared| declared.held())
                        .unwrap_or_default()
                        .to_string(),
                },
                NodeKey::Unresolved { source, label } => ProvNode::Unresolved {
                    label: label.clone(),
                    source: *source,
                },
            });
        }
    }

    // What an activity says it wrote, the package does not hold.
    let absent: HashSet<&str> = nodes
        .iter()
        .filter_map(|node| match node {
            ProvNode::Object {
                uri, present: false, ..
            } => Some(uri.as_str()),
            _ => None,
        })
        .collect();
    let mut found: Vec<GraphIssue> = Vec::new();
    for edge in &kept {
        if edge.kind != ProvEdgeKind::Produced {
            continue;
        }
        if let NodeKey::Object(object) = &edge.to
            && absent.contains(object.as_str())
        {
            found.push(GraphIssue::OutputAbsent {
                activity: edge.from.name().to_string(),
                object: object.clone(),
            });
        }
    }

    let mut all_issues = issues;
    all_issues.extend(found);
    ProvenanceGraph {
        nodes,
        edges: kept,
        issues: all_issues,
        set_aside: aside,
    }
}

// -- Focus ---------------------------------------------------------------------------

/// Why a graph cannot be focused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FocusRefusal {
    /// The key is no node of the graph.
    #[error("{0:?} is not a node of the graph")]
    NotInGraph(NodeKey),
    /// A depth beyond the bound.
    #[error("a depth of {asked} is more than {limit}")]
    DepthTooLarge {
        /// The depth asked for.
        asked: u8,
        /// The bound.
        limit: u8,
    },
}

fn is_flow(edge: &ProvEdge) -> bool {
    matches!(edge.kind, ProvEdgeKind::Consumed | ProvEdgeKind::Produced)
}

/// The nodes `depth` flow edges away from `on`, backwards (towards what produced
/// them) or forwards, `on` included.
fn walk(graph: &ProvenanceGraph, on: &NodeKey, depth: u8, backwards: bool) -> HashSet<NodeKey> {
    let mut seen: HashSet<NodeKey> = HashSet::new();
    seen.insert(on.clone());
    let mut frontier: Vec<NodeKey> = vec![on.clone()];
    for _ in 0..depth {
        let mut next: Vec<NodeKey> = Vec::new();
        for edge in graph.edges.iter().filter(|edge| is_flow(edge)) {
            let (near, far) = if backwards {
                (&edge.to, &edge.from)
            } else {
                (&edge.from, &edge.to)
            };
            if frontier.contains(near) && seen.insert(far.clone()) {
                next.push(far.clone());
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    seen
}

/// The names an issue gives (uris, mostly): it is kept when one of them is kept.
fn issue_names(issue: &GraphIssue) -> Vec<&str> {
    match issue {
        GraphIssue::Tree(TreeRefusal::ParentCycle { uris }) => uris.iter().map(String::as_str).collect(),
        GraphIssue::Tree(TreeRefusal::ParentAbsent { child, parent }) => {
            vec![child.as_str(), parent.as_str()]
        }
        GraphIssue::TreeDisagreement { parent, child, .. } => vec![parent.as_str(), child.as_str()],
        GraphIssue::HeldButPlayed { workflow, .. } => vec![workflow.as_str()],
        GraphIssue::OutputAbsent { activity, object } => vec![activity.as_str(), object.as_str()],
        GraphIssue::Conformance { activity, .. } => vec![activity.as_str()],
        GraphIssue::ActivityLinkDiffers {
            by_column,
            by_mapping,
            ..
        } => by_column
            .iter()
            .chain(by_mapping.iter())
            .map(String::as_str)
            .collect(),
        GraphIssue::ProcessWithoutActivity { .. } | GraphIssue::Verbatim(_) => Vec::new(),
        GraphIssue::Unreadable { uri, .. } => vec![uri.as_str()],
        GraphIssue::Expand(ExpandRefusal::NotInIndex { address }) => vec![address.as_str()],
        GraphIssue::Expand(
            ExpandRefusal::NotACollection { uri, .. }
            | ExpandRefusal::NoAssociationSet { uri }
            | ExpandRefusal::TooMany { uri, .. },
        ) => vec![uri.as_str()],
    }
}

/// The graph around a node.
///
/// The **flow** is the `Consumed` and `Produced` edges. Upstream goes back
/// along the flow, at most `upstream` **edges** away; downstream goes forward,
/// at most `downstream`. Around every kept node are also kept: the whole chain
/// of its `Carrier` edges, its `Member` edges (towards the members), its
/// `Parent` edges and its `Participant` edges (one hop). Edges are kept between
/// kept nodes only; an issue or a set-aside object is kept when it names a kept
/// node, and issues that name no node are left out.
pub fn focus(
    graph: &ProvenanceGraph,
    on: &NodeKey,
    upstream: u8,
    downstream: u8,
) -> Result<ProvenanceGraph, FocusRefusal> {
    if graph.position(on).is_none() {
        return Err(FocusRefusal::NotInGraph(on.clone()));
    }
    for asked in [upstream, downstream] {
        if asked > MAX_DEPTH {
            return Err(FocusRefusal::DepthTooLarge {
                asked,
                limit: MAX_DEPTH,
            });
        }
    }

    let mut kept = walk(graph, on, upstream, true);
    kept.extend(walk(graph, on, downstream, false));

    // The chain of carriers that starts at a kept node: all of it.
    let mut queue: Vec<NodeKey> = kept.iter().cloned().collect();
    while let Some(node) = queue.pop() {
        for edge in &graph.edges {
            if matches!(edge.kind, ProvEdgeKind::Carrier { .. })
                && edge.from == node
                && kept.insert(edge.to.clone())
            {
                queue.push(edge.to.clone());
            }
        }
    }
    // The members of a kept collection; then one hop of parent and participant.
    let members: Vec<NodeKey> = graph
        .edges
        .iter()
        .filter(|edge| edge.kind == ProvEdgeKind::Member && kept.contains(&edge.from))
        .map(|edge| edge.to.clone())
        .collect();
    kept.extend(members);
    let mut hops: Vec<NodeKey> = Vec::new();
    for edge in &graph.edges {
        if matches!(edge.kind, ProvEdgeKind::Parent | ProvEdgeKind::Participant) {
            if kept.contains(&edge.from) {
                hops.push(edge.to.clone());
            }
            if kept.contains(&edge.to) {
                hops.push(edge.from.clone());
            }
        }
    }
    kept.extend(hops);

    let names: HashSet<&str> = kept.iter().map(NodeKey::name).collect();
    Ok(ProvenanceGraph {
        nodes: graph
            .nodes
            .iter()
            .filter(|node| kept.contains(&node.key()))
            .cloned()
            .collect(),
        edges: graph
            .edges
            .iter()
            .filter(|edge| kept.contains(&edge.from) && kept.contains(&edge.to))
            .cloned()
            .collect(),
        issues: graph
            .issues
            .iter()
            .filter(|issue| issue_names(issue).iter().any(|name| names.contains(name)))
            .cloned()
            .collect(),
        set_aside: graph
            .set_aside
            .iter()
            .filter(|aside| names.contains(aside.named_by.as_str()))
            .cloned()
            .collect(),
    })
}

// -- The composition --------------------------------------------------------------------

/// What [`of_package`] is told.
pub struct GraphOptions<'a> {
    /// The role rules, the caller's first.
    pub roles: &'a RoleRules,
    /// The carrier rules, the caller's first.
    pub carriers: &'a CarrierRules,
    /// Carrier walk depth: 4 reaches property, frame, trajectory, interpretation, feature.
    pub carrier_depth: u8,
    /// Type keys set aside ([`DEFAULT_SET_ASIDE`] by default).
    pub set_aside: &'a [&'a str],
    /// Collections to expand (uris).
    pub expanded: &'a [String],
    /// Upper bound of [`PackageIndex::expand_collection`] for an expanded collection.
    pub expand_limit: usize,
    /// The reader of the free-text constraint of a step.
    pub steps: &'a dyn StepConstraintReader,
    /// The reader of the file a text parameter names.
    pub files: &'a dyn InputFileReader,
    /// The verbatim provenance sources.
    pub verbatim: &'a VerbatimSources,
}

/// Everything read of a package's provenance, and the graph made of it.
pub struct PackageProvenance {
    /// The activities that could be read, in index order.
    pub activities: Vec<ActivityRecord>,
    /// The templates that could be read.
    pub templates: Vec<TemplateRecord>,
    /// The role of each parameter of each activity.
    pub roles: Vec<Vec<ParameterRole>>,
    /// The tree of activities.
    pub tree: ActivityTree,
    /// The declared steps of every workflow template.
    pub steps: Vec<DeclaredStep>,
    /// The verbatim processes, with the source that read each.
    pub processes: Vec<(VerbatimId, crate::provenance_verbatim::VerbatimProcess)>,
    /// The graph.
    pub graph: ProvenanceGraph,
}

/// The provenance graph of a package, composed once so that every face shows
/// the same one.
///
/// 1. Read every activity and template the index holds (a failure is a
///    [`GraphIssue::Unreadable`]).
/// 2. Roles and conformance of each activity against its template.
/// 3. The edges of each activity.
/// 4. The tree of activities; its refusals and disagreements become issues.
/// 5. The steps of each workflow template; a held step no root names is a
///    `HeldStep` node, one the root names is [`GraphIssue::HeldButPlayed`].
/// 6. The verbatim sources that recognise the package's tables.
/// 7. The carriers of the objects the edges name.
/// 8. The collections the caller asked to expand.
/// 9. [`assemble`].
pub fn of_package(
    index: &PackageIndex,
    source: &dyn ArraySource,
    options: &GraphOptions<'_>,
) -> PackageProvenance {
    let mut issues: Vec<GraphIssue> = Vec::new();

    // 1. Reading.
    let mut activities: Vec<ActivityRecord> = Vec::new();
    let mut templates: Vec<TemplateRecord> = Vec::new();
    for node in index.nodes() {
        if node.state != NodeState::Present {
            continue;
        }
        let is_activity = match &*node.type_key {
            "Activity" => true,
            "ActivityTemplate" => false,
            _ => continue,
        };
        let uri = node.uri.to_string();
        let read = match source.read_object_at(&uri) {
            Ok(object) => {
                if is_activity {
                    read_activity(object.as_reflect())
                        .map(|record| activities.push(record))
                        .map_err(|refusal| refusal.to_string())
                } else {
                    read_template(object.as_reflect())
                        .map(|record| templates.push(record))
                        .map_err(|refusal| refusal.to_string())
                }
            }
            Err(why) => Err(why),
        };
        if let Err(why) = read {
            issues.push(GraphIssue::Unreadable { uri, why });
        }
    }

    // 2. Roles and conformance.
    let mut all_roles: Vec<Vec<ParameterRole>> = Vec::with_capacity(activities.len());
    for activity in &activities {
        let template = descriptor_of(activity, &templates);
        all_roles.push(roles_of(activity, template, options.roles));
        for issue in conformance_of(activity, template) {
            issues.push(GraphIssue::Conformance {
                activity: activity.uri.clone(),
                issue,
            });
        }
    }

    // 3. The edges of each activity.
    let mut edges: Vec<ProvEdge> = Vec::new();
    for (activity, roles) in activities.iter().zip(&all_roles) {
        edges.extend(activity_edges(activity, roles, options.files));
    }

    // 4. The tree.
    let tree = activity_tree(&activities, &all_roles);
    issues.extend(tree.refusals.iter().cloned().map(GraphIssue::Tree));
    issues.extend(tree.disagreements.iter().cloned());

    // 5. Declared steps, held or played.
    let mut steps: Vec<DeclaredStep> = Vec::new();
    for template in &templates {
        let declared = workflow_steps(template, options.steps);
        if declared.is_empty() {
            continue;
        }
        for activity in &activities {
            if descriptor_of(activity, std::slice::from_ref(template)).is_none() {
                continue;
            }
            for step in declared.iter().filter(|step| step.held().is_some()) {
                let named = activity
                    .parameters
                    .iter()
                    .any(|parameter| parameter.title == step.title);
                if named {
                    issues.push(GraphIssue::HeldButPlayed {
                        workflow: template.uri.clone(),
                        step: step.title.clone(),
                    });
                } else {
                    edges.push(edge(
                        NodeKey::Object(activity.uri.clone()),
                        NodeKey::HeldStep {
                            workflow: template.uri.clone(),
                            step: step.title.clone(),
                        },
                        ProvEdgeKind::Parent,
                        EdgeBasis::Reference,
                    ));
                }
            }
        }
        steps.extend(declared);
    }

    // 6. Verbatim sources.
    let mut processes: Vec<(VerbatimId, crate::provenance_verbatim::VerbatimProcess)> = Vec::new();
    let mut table_objects = Vec::new();
    for node in index.nodes() {
        if node.state == NodeState::Present && &*node.type_key == "ColumnBasedTable" {
            match source.read_object_at(&node.uri) {
                Ok(object) => table_objects.push(object),
                Err(why) => issues.push(GraphIssue::Verbatim(VerbatimRefusal::Unreadable {
                    table: node.uri.to_string(),
                    why,
                })),
            }
        }
    }
    let mut tables: Vec<Table<'_>> = Vec::new();
    for object in &table_objects {
        match Table::read(object.as_reflect()) {
            Ok(table) => tables.push(table),
            Err(why) => issues.push(GraphIssue::Verbatim(VerbatimRefusal::Unreadable {
                table: energyml_core::identity::uri_of_reflect(object.as_reflect()).unwrap_or_default(),
                why: why.to_string(),
            })),
        }
    }
    for reader in options.verbatim.recognised(&tables) {
        match reader.read(&tables, source) {
            Ok(read) => {
                for process in read {
                    let (more_edges, more_issues) = verbatim_edges(reader.id(), &process);
                    edges.extend(more_edges);
                    issues.extend(more_issues);
                    processes.push((reader.id(), process));
                }
            }
            Err(refusal) => issues.push(GraphIssue::Verbatim(refusal)),
        }
    }

    // 7. Carriers of the objects the edges name (activities are not carried).
    let activity_keys: HashSet<NodeKey> = activities
        .iter()
        .map(|activity| NodeKey::Object(activity.uri.clone()))
        .collect();
    let mut seeds: Vec<String> = Vec::new();
    let mut seeded: HashSet<String> = HashSet::new();
    for edge in &edges {
        for key in [&edge.from, &edge.to] {
            if let NodeKey::Object(uri) = key
                && !activity_keys.contains(key)
                && seeded.insert(uri.clone())
            {
                seeds.push(uri.clone());
            }
        }
    }
    edges.extend(carriers(index, &seeds, options.carriers, options.carrier_depth));

    // 8. The collections the caller expanded.
    for collection in options.expanded {
        match index.expand_collection(collection, options.expand_limit) {
            Ok(members) => {
                for member in members {
                    edges.push(edge(
                        NodeKey::Object(collection.clone()),
                        NodeKey::Object(member),
                        ProvEdgeKind::Member,
                        EdgeBasis::Reference,
                    ));
                }
            }
            Err(refusal) => issues.push(GraphIssue::Expand(refusal)),
        }
    }

    // 9. Assemble.
    let graph = assemble(Parts {
        activities: &activities,
        edges,
        steps: &steps,
        index: Some(index),
        set_aside: options.set_aside,
        expanded: options.expanded,
        issues,
    });
    PackageProvenance {
        activities,
        templates,
        roles: all_roles,
        tree,
        steps,
        processes,
        graph,
    }
}
