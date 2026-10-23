//! Which objects a request reaches: ETP's `ContextInfo` and `ContextScopeKind`.
//!
//! `GetResources`, `FindResources`, `FindDataObjects`, `GetSupportedTypes` and a
//! notification subscription all name the objects they are about the same way:
//! a starting URI, a depth, a direction, the kinds of relationship to follow and
//! the types to keep (ETP 1.2 §8.1.1 and §23.34.3). [`Context`] is that request,
//! spelled without depending on the protocol crates, and [`discover`] answers it
//! from what any store can list - its resources and its edges.
//!
//! That answer is the **default** of [`crate::EnergymlStorage::resources_in`]: right
//! everywhere, and the wrong thing for a store that indexes its graph, which
//! answers natively and overrides it - the same arrangement as
//! [`crate::EnergymlStorage::edges`].
//!
//! # What is refused rather than guessed
//!
//! - **Following only primary or only secondary relationships**, and the
//!   secondary expansions, need to know the kind of each edge. Which relationship
//!   is primary is ML-specific (ETP 1.2 §8.1.1.1.1, note 1: *"see the relevant
//!   ML's ETP implementation specification"*); a store that derives its edges
//!   from the objects states it by the rules of [`crate::kinds`], which give
//!   every edge a kind (primary by default where no rule applies, labelled so).
//!   An edge can still have none: one nobody judged (a store without rules), or
//!   one a caller's rules leave unknown ([`crate::graph::Edge::kind`]). The
//!   refusal is **met**, not global: a request by kind answers while its walk
//!   meets only edges that have a kind, and refuses ([`crate::Error::UnstatedKind`],
//!   naming the type and the cause) at the first edge that has none.
//! - **Filtering on a fact the store does not state** - the active status, the
//!   store's last write - is refused by name for the object that lacks it, never
//!   answered as "nothing matches".
//!
//! A type ending in `.*` is a wildcard: every type of that schema version
//! (`resqml20.*`), as `ContextInfo.dataObjectTypes` allows (ETP 1.2 §23.34.15,
//! p. 475).

use std::collections::{BTreeSet, HashMap, VecDeque};

use energyml_core::identifiers::EtpUri;

use crate::error::{Error, Result};
use crate::graph::{Edge, EdgeKind};
use crate::resource::{ActiveStatus, ObjectSummary};

/// ETP `ContextScopeKind`: which way from the starting object a request goes,
/// and whether the starting object is part of the answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Scope {
    /// The starting object alone (depth is ignored).
    #[default]
    Itself,
    /// The objects pointing at it, up to the depth.
    Sources,
    /// The objects it points at, up to the depth.
    Targets,
    /// [`Scope::Sources`] and the starting object.
    SourcesOrSelf,
    /// [`Scope::Targets`] and the starting object.
    TargetsOrSelf,
}

/// ETP `RelationshipKind` as a filter: which relationships a request follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Navigable {
    /// Primary relationships only.
    Primary,
    /// Secondary relationships only.
    Secondary,
    /// Both - the only filter that needs no knowledge of the edges' kinds.
    #[default]
    Both,
}

/// ETP `ContextInfo` plus the scope and filters of a discovery request.
///
/// Built with [`Context::of`]; `#[non_exhaustive]` so that what ETP adds is not a
/// breaking change.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Context {
    /// The dataspace or the object to start from, by its canonical URI. For a
    /// dataspace, scope and depth are ignored and the answer is its content
    /// (ETP 1.2 §8.2.2, row 9).
    pub uri: String,
    /// How many relationships away from the starting object to go.
    pub depth: u32,
    /// The direction, and whether the starting object is included.
    pub scope: Scope,
    /// The qualified types to keep (`resqml22.HorizonInterpretation`), applied
    /// **after** the traversal (§8.2.2, row 10). Empty keeps every type.
    pub types: Vec<String>,
    /// Which relationships to follow.
    pub navigable: Navigable,
    /// Add the secondary targets of the objects found (§8.1.1.1.4, step 2).
    pub include_secondary_targets: bool,
    /// Add the secondary sources of the objects found.
    pub include_secondary_sources: bool,
    /// Keep only objects the store last wrote after this ETP timestamp
    /// (`storeLastWriteFilter`). `None`: no filter.
    pub store_last_write_after: Option<i64>,
    /// Keep only objects in this active status (`activeStatusFilter`). `None`:
    /// no filter.
    pub active_status: Option<ActiveStatus>,
    /// Count, for each object found, the objects pointing at it and those it
    /// points at (`countObjects`, ETP 1.2 §8.3.1), over the relationships the
    /// store states.
    pub count_objects: bool,
}

impl Context {
    /// The objects reached from `uri` in this direction, every type, every
    /// relationship, depth 1.
    pub fn of(uri: impl Into<String>, scope: Scope) -> Self {
        Self {
            uri: uri.into(),
            depth: 1,
            scope,
            types: Vec::new(),
            navigable: Navigable::Both,
            include_secondary_targets: false,
            include_secondary_sources: false,
            store_last_write_after: None,
            active_status: None,
            count_objects: false,
        }
    }

    /// The content of a dataspace.
    pub fn dataspace(uri: impl Into<String>) -> Self {
        Self::of(uri, Scope::Itself)
    }

    /// With a depth.
    pub fn with_depth(mut self, depth: u32) -> Self {
        self.depth = depth;
        self
    }

    /// Keeping only these qualified types.
    pub fn with_types(mut self, types: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.types = types.into_iter().map(Into::into).collect();
        self
    }
}

/// What a discovery found: the objects, and the edges between them (ETP's
/// `GetResourcesResponse` and `GetResourcesEdgesResponse`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Found {
    /// The objects, in the order the store listed them.
    pub resources: Vec<ObjectSummary>,
    /// The edges with at least one end among [`Found::resources`]: ETP 1.2
    /// §8.3.1 (p. 163, `GetResourcesEdgesResponse` row, 3.c) recommends an
    /// edge "if EITHER end (or both) are present", even when the other end was
    /// filtered out by the types.
    pub edges: Vec<Edge>,
}

/// An object a store deleted, as ETP's `DeletedResource` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DeletedResource {
    /// The URI it had.
    pub uri: String,
    /// When it was deleted (ETP timestamp, microseconds since the Unix epoch,
    /// UTC).
    pub deleted_time: i64,
}

impl DeletedResource {
    /// A deletion.
    pub fn new(uri: impl Into<String>, deleted_time: i64) -> Self {
        Self {
            uri: uri.into(),
            deleted_time,
        }
    }
}

/// ETP `GetDeletedResources`: the deletions of one dataspace, at the dataspace
/// level (tombstones have no context left, §8.3, `GetDeletedResources`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DeletedFilter {
    /// The dataspace.
    pub dataspace: String,
    /// Only deletions after this ETP timestamp. `None`: all that are kept.
    pub after: Option<i64>,
    /// Only these qualified types. Empty: every type.
    pub types: Vec<String>,
}

impl DeletedFilter {
    /// Every deletion kept for a dataspace.
    pub fn of(dataspace: impl Into<String>) -> Self {
        Self {
            dataspace: dataspace.into(),
            after: None,
            types: Vec::new(),
        }
    }
}

/// Whether a URI names a dataspace rather than an object - through the one
/// parser of ETP URIs.
pub fn is_dataspace_uri(uri: &str) -> Result<bool> {
    let parsed = EtpUri::parse(uri).map_err(|error| Error::BadUri {
        uri: uri.to_string(),
        reason: error.to_string(),
    })?;
    Ok(parsed.uuid.is_none() && parsed.object_type.is_none())
}

/// The refusal of an edge whose kind nobody states: the type of its source and why.
fn unstated(edge: &Edge) -> Error {
    let source_type = EtpUri::parse(&edge.from)
        .ok()
        .and_then(|uri| Some(format!("{}{}.{}", uri.domain?, uri.version?, uri.object_type?)))
        .unwrap_or_else(|| edge.from.clone());
    let why = match edge.basis {
        Some(crate::kinds::KindBasis::Unknown { why }) => why,
        _ => "no rule or server states the kind of this edge",
    };
    Error::UnstatedKind { source_type, why }
}

/// An edge met by a request that needs its kind: refused when it has none.
fn needs_kind(edge: &Edge) -> Result<()> {
    if edge.kind.is_none() {
        return Err(unstated(edge));
    }
    Ok(())
}

/// Answer a [`Context`] from what a store lists: its resources and its edges.
///
/// `dataspace` is the URI of the one dataspace these resources belong to: a
/// context naming another dataspace is refused as not found, never answered
/// with this one's content.
pub fn discover(
    dataspace: &str,
    resources: Vec<ObjectSummary>,
    edges: Vec<Edge>,
    context: &Context,
) -> Result<Found> {
    if let Some(odd) = context
        .types
        .iter()
        .find(|t| t.contains('*') && (!t.ends_with(".*") || t[..t.len() - 1].contains('*')))
    {
        return Err(Error::BadUri {
            uri: odd.clone(),
            reason: "a type wildcard is a schema version followed by `.*` (ETP 1.2 §23.34.15)".into(),
        });
    }
    let typed = |summary: &ObjectSummary| {
        context.types.is_empty()
            || context
                .types
                .iter()
                .any(|t| type_matches(t, &summary.qualified_type))
    };
    let in_status = |summary: &ObjectSummary| match context.active_status {
        None => Ok(true),
        Some(wanted) => summary
            .active_status
            .map(|status| status == wanted)
            .ok_or(Error::Unsupported {
                operation: "filtering on the active status",
                store: "discover",
            }),
    };
    let written_after = |summary: &ObjectSummary| match context.store_last_write_after {
        None => Ok(true),
        Some(after) => summary
            .store_last_write
            .map(|written| written > after)
            .ok_or(Error::Unsupported {
                operation: "filtering on the store's last write",
                store: "discover",
            }),
    };

    if is_dataspace_uri(&context.uri)? {
        if !same_dataspace(&context.uri, dataspace)? {
            return Err(Error::NotFound(context.uri.clone()));
        }
        let mut kept = Vec::new();
        for summary in resources {
            if typed(&summary) && written_after(&summary)? && in_status(&summary)? {
                kept.push(summary);
            }
        }
        return Ok(counted(with_edges(kept, &edges), &edges, context));
    }

    // A request by kind is refused when its walk meets an edge with no kind
    // (`needs_kind`), not because some edge elsewhere in the dataspace has none.
    let follows = |edge: &Edge| match context.navigable {
        Navigable::Both => true,
        Navigable::Primary => edge.kind == Some(EdgeKind::Primary),
        Navigable::Secondary => edge.kind == Some(EdgeKind::Secondary),
    };

    let start = context.uri.as_str();
    if !resources.iter().any(|summary| summary.uri == start) {
        return Err(Error::NotFound(context.uri.clone()));
    }
    let mut out_of: HashMap<&str, Vec<&Edge>> = HashMap::new();
    let mut into: HashMap<&str, Vec<&Edge>> = HashMap::new();
    for edge in &edges {
        out_of.entry(edge.from.as_str()).or_default().push(edge);
        into.entry(edge.to.as_str()).or_default().push(edge);
    }

    // Step 1: the candidates the URI and the depth reach.
    let (towards_targets, towards_sources, with_self) = match context.scope {
        Scope::Itself => (false, false, true),
        Scope::Sources => (false, true, false),
        Scope::Targets => (true, false, false),
        Scope::SourcesOrSelf => (false, true, true),
        Scope::TargetsOrSelf => (true, false, true),
    };
    let mut reached: BTreeSet<String> = BTreeSet::new();
    let mut seen: BTreeSet<&str> = BTreeSet::from([start]);
    let mut queue: VecDeque<(&str, u32)> = VecDeque::from([(start, 0)]);
    let depth = if context.scope == Scope::Itself {
        0
    } else {
        context.depth
    };
    while let Some((uri, level)) = queue.pop_front() {
        if level == depth {
            continue;
        }
        let mut next: Vec<&str> = Vec::new();
        if towards_targets {
            for edge in out_of.get(uri).into_iter().flatten() {
                if context.navigable != Navigable::Both {
                    needs_kind(edge)?;
                }
                if follows(edge) {
                    next.push(edge.to.as_str());
                }
            }
        }
        if towards_sources {
            for edge in into.get(uri).into_iter().flatten() {
                if context.navigable != Navigable::Both {
                    needs_kind(edge)?;
                }
                if follows(edge) {
                    next.push(edge.from.as_str());
                }
            }
        }
        for other in next {
            if seen.insert(other) {
                reached.insert(other.to_string());
                queue.push_back((other, level + 1));
            }
        }
    }
    // The starting object is never reached by the walk (it is seen from the
    // start), so a cycle back to it does not make it "self".
    if with_self {
        reached.insert(start.to_string());
    }

    // Step 2: the secondary expansions, applied once and together.
    let candidates: Vec<String> = reached.iter().cloned().collect();
    for uri in &candidates {
        if context.include_secondary_targets {
            for edge in out_of.get(uri.as_str()).into_iter().flatten() {
                needs_kind(edge)?;
                if edge.kind == Some(EdgeKind::Secondary) {
                    reached.insert(edge.to.clone());
                }
            }
        }
        if context.include_secondary_sources {
            for edge in into.get(uri.as_str()).into_iter().flatten() {
                needs_kind(edge)?;
                if edge.kind == Some(EdgeKind::Secondary) {
                    reached.insert(edge.from.clone());
                }
            }
        }
    }

    // Step 3: the types, after the traversal; the store's order kept.
    let mut kept = Vec::new();
    for summary in resources {
        if reached.contains(&summary.uri)
            && typed(&summary)
            && written_after(&summary)?
            && in_status(&summary)?
        {
            kept.push(summary);
        }
    }
    // Step 4: the edges touching what is kept.
    Ok(counted(with_edges(kept, &edges), &edges, context))
}

/// Whether a qualified type matches one entry of `dataObjectTypes`: equal, or
/// of the schema version a `prefix.*` wildcard names (ETP 1.2 §23.34.15).
/// Type names are case sensitive there; the prefix is compared the same way.
pub fn type_matches(pattern: &str, qualified_type: &str) -> bool {
    match pattern.strip_suffix(".*") {
        Some(version) => qualified_type
            .split_once('.')
            .is_some_and(|(prefix, _)| prefix == version),
        None => pattern == qualified_type,
    }
}

/// The edges of an answer: those with **either** end among the objects found
/// (ETP 1.2 §8.3.1, p. 163, 3.c - the reading of §8.1.1.1.4 step 4, "not
/// connected to a node in the final set", that the specification spells out).
fn with_edges(resources: Vec<ObjectSummary>, edges: &[Edge]) -> Found {
    let found: BTreeSet<&str> = resources.iter().map(|summary| summary.uri.as_str()).collect();
    let edges = edges
        .iter()
        .filter(|edge| found.contains(edge.from.as_str()) || found.contains(edge.to.as_str()))
        .cloned()
        .collect();
    Found { resources, edges }
}

/// The source and target counts of each object found, when the context asks
/// for them: distinct objects at the other end of the store's relationships.
///
/// `TODO(verify)`: whether `countObjects` counts over every relationship or
/// only those `navigableEdges` names (ETP 1.2 §8.3.1 says "counts of sources
/// and targets for each type of resource"); every stated relationship is
/// counted here.
fn counted(mut found: Found, edges: &[Edge], context: &Context) -> Found {
    if !context.count_objects {
        return found;
    }
    let mut sources: HashMap<&str, BTreeSet<&str>> = HashMap::new();
    let mut targets: HashMap<&str, BTreeSet<&str>> = HashMap::new();
    for edge in edges {
        sources
            .entry(edge.to.as_str())
            .or_default()
            .insert(edge.from.as_str());
        targets
            .entry(edge.from.as_str())
            .or_default()
            .insert(edge.to.as_str());
    }
    let count = |map: &HashMap<&str, BTreeSet<&str>>, uri: &str| {
        u32::try_from(map.get(uri).map_or(0, BTreeSet::len)).unwrap_or(u32::MAX)
    };
    for summary in &mut found.resources {
        summary.source_count = Some(count(&sources, &summary.uri));
        summary.target_count = Some(count(&targets, &summary.uri));
    }
    found
}

/// The types a context reaches and how many objects of each - what ETP's
/// `GetSupportedTypes` answers with `countObjects` (ETP 1.2 §25), derived from
/// a discovery rather than asked again. Sorted by type.
pub fn types_of(found: &Found) -> Vec<(String, usize)> {
    let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for summary in &found.resources {
        *counts.entry(summary.qualified_type.as_str()).or_default() += 1;
    }
    counts.into_iter().map(|(t, n)| (t.to_string(), n)).collect()
}

fn same_dataspace(a: &str, b: &str) -> Result<bool> {
    let parse = |uri: &str| {
        EtpUri::parse(uri).map_err(|error| Error::BadUri {
            uri: uri.to_string(),
            reason: error.to_string(),
        })
    };
    Ok(parse(a)?.dataspace == parse(b)?.dataspace)
}
