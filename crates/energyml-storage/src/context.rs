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
//!   from the objects does not state it ([`crate::graph::Edge::kind`]), so these
//!   requests are refused by name over such edges.
//! - **A wildcard in `dataObjectTypes`** (`resqml20.*`) is refused: `TODO(verify)`
//!   the wildcard forms ETP 1.2 allows there (§8.2.2 and §3.3.4).

use std::collections::{BTreeSet, HashMap, VecDeque};

use energyml_core::identifiers::EtpUri;

use crate::error::{Error, Result};
use crate::graph::{Edge, EdgeKind};
use crate::resource::ObjectSummary;

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
    /// The edges whose two ends are both among [`Found::resources`] (§8.1.1.1.4,
    /// step 4).
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
    if context.types.iter().any(|t| t.contains('*')) {
        return Err(Error::Unsupported {
            operation: "a wildcard in the types of a context",
            store: "discover",
        });
    }
    let typed = |summary: &ObjectSummary| {
        context.types.is_empty() || context.types.iter().any(|t| t == &summary.qualified_type)
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
            if typed(&summary) && written_after(&summary)? {
                kept.push(summary);
            }
        }
        return Ok(with_edges(kept, edges));
    }

    let wants_kinds = context.navigable != Navigable::Both
        || context.include_secondary_targets
        || context.include_secondary_sources;
    if wants_kinds && edges.iter().any(|edge| edge.kind.is_none()) {
        return Err(Error::Unsupported {
            operation: "following relationships by kind, over edges whose kind is not stated",
            store: "discover",
        });
    }
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
            next.extend(
                out_of
                    .get(uri)
                    .into_iter()
                    .flatten()
                    .filter(|e| follows(e))
                    .map(|e| e.to.as_str()),
            );
        }
        if towards_sources {
            next.extend(
                into.get(uri)
                    .into_iter()
                    .flatten()
                    .filter(|e| follows(e))
                    .map(|e| e.from.as_str()),
            );
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
                if edge.kind == Some(EdgeKind::Secondary) {
                    reached.insert(edge.to.clone());
                }
            }
        }
        if context.include_secondary_sources {
            for edge in into.get(uri.as_str()).into_iter().flatten() {
                if edge.kind == Some(EdgeKind::Secondary) {
                    reached.insert(edge.from.clone());
                }
            }
        }
    }

    // Step 3: the types, after the traversal; the store's order kept.
    let mut kept = Vec::new();
    for summary in resources {
        if reached.contains(&summary.uri) && typed(&summary) && written_after(&summary)? {
            kept.push(summary);
        }
    }
    // Step 4: the edges between what is kept.
    Ok(with_edges(kept, edges))
}

fn with_edges(resources: Vec<ObjectSummary>, edges: Vec<Edge>) -> Found {
    let held: BTreeSet<&str> = resources.iter().map(|summary| summary.uri.as_str()).collect();
    let edges = edges
        .into_iter()
        .filter(|edge| held.contains(edge.from.as_str()) && held.contains(edge.to.as_str()))
        .collect();
    Found { resources, edges }
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
