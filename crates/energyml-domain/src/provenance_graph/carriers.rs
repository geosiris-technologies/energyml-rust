//! The representations, interpretations and features that carry an object.
//!
//! A walk over the references an object makes, following only the ones a named
//! rule says carry it. The rules are a registry (ADR-0006): the caller's are
//! tried first.

use std::collections::HashSet;

use energyml_core::{identity, naming};

use super::{MAX_DEPTH, NodeKey, ProvEdge, ProvEdgeKind, EdgeBasis};
use crate::package_index::PackageIndex;

/// A reference a carrier walk follows.
pub trait CarrierRule: Send + Sync {
    /// Stable id, shown beside the edge it gives.
    fn id(&self) -> &'static str;
    /// Whether the reference is a carrier. `holder` and `target` are type keys
    /// (concept without package or `obj_`); `field` is the last segment of the
    /// reference path, without `[n]`.
    fn follows(&self, holder: &str, field: &str, target: &str) -> bool;
}

/// `RepresentedObject`, `RepresentedInterpretation`, `InterpretedFeature`: the
/// three fields the index already holds as parents.
struct Firp;

impl CarrierRule for Firp {
    fn id(&self) -> &'static str {
        "firp"
    }

    fn follows(&self, _holder: &str, field: &str, _target: &str) -> bool {
        matches!(
            field,
            "RepresentedObject" | "RepresentedInterpretation" | "InterpretedFeature"
        )
    }
}

/// `SupportingRepresentation`: a property hangs on its support.
struct SupportingRepresentation;

impl CarrierRule for SupportingRepresentation {
    fn id(&self) -> &'static str {
        "supporting-representation"
    }

    fn follows(&self, _holder: &str, field: &str, _target: &str) -> bool {
        field == "SupportingRepresentation"
    }
}

/// `Trajectory` of the representations of a wellbore frame.
struct WellboreTrajectory;

impl CarrierRule for WellboreTrajectory {
    fn id(&self) -> &'static str {
        "wellbore-trajectory"
    }

    fn follows(&self, holder: &str, field: &str, _target: &str) -> bool {
        field == "Trajectory"
            && matches!(
                holder,
                "WellboreFrameRepresentation"
                    | "BlockedWellboreRepresentation"
                    | "SeismicWellboreFrameRepresentation"
                    | "WellboreMarkerFrameRepresentation"
            )
    }
}

/// The links to the WITSML object a RESQML object stands for.
struct WitsmlLink;

impl CarrierRule for WitsmlLink {
    fn id(&self) -> &'static str {
        "witsml-link"
    }

    fn follows(&self, _holder: &str, field: &str, _target: &str) -> bool {
        matches!(field, "WitsmlTrajectory" | "WitsmlWellbore")
    }
}

/// The rules, the caller's first.
pub struct CarrierRules {
    rules: Vec<Box<dyn CarrierRule>>,
    added: usize,
}

impl CarrierRules {
    /// The built-in rules: `firp`, `supporting-representation`,
    /// `wellbore-trajectory`, `witsml-link`.
    pub fn new() -> Self {
        CarrierRules {
            rules: vec![
                Box::new(Firp),
                Box::new(SupportingRepresentation),
                Box::new(WellboreTrajectory),
                Box::new(WitsmlLink),
            ],
            added: 0,
        }
    }

    /// No rule at all.
    pub fn empty() -> Self {
        CarrierRules {
            rules: Vec::new(),
            added: 0,
        }
    }

    /// A caller's rule, tried before the built-ins and after the caller's
    /// earlier ones.
    pub fn add(&mut self, rule: Box<dyn CarrierRule>) {
        self.rules.insert(self.added, rule);
        self.added += 1;
    }

    /// The ids, in the order they are tried.
    pub fn ids(&self) -> Vec<&'static str> {
        self.rules.iter().map(|rule| rule.id()).collect()
    }

    fn follows(&self, holder: &str, field: &str, target: &str) -> bool {
        self.rules.iter().any(|rule| rule.follows(holder, field, target))
    }
}

impl Default for CarrierRules {
    fn default() -> Self {
        Self::new()
    }
}

/// The last segment of a reference path, without `[n]`.
fn last_segment(path: &str) -> &str {
    let segment = path.rsplit('.').next().unwrap_or(path);
    match segment.find('[') {
        Some(at) => &segment[..at],
        None => segment,
    }
}

/// The concept of a qualified type: without package or `obj_`.
fn concept_of(qualified_type: &str) -> String {
    identity::parse_qualified_type(qualified_type)
        .map(|parts| naming::strip_obj_prefix(parts.object_type).to_string())
        .unwrap_or_else(|_| qualified_type.to_string())
}

/// The carrier edges around `seeds` (full uris): a breadth-first walk over the
/// references the rules follow, `depth` levels at most (capped at
/// [`MAX_DEPTH`]). Edges are deduplicated, in the order they were found.
pub fn carriers(index: &PackageIndex, seeds: &[String], rules: &CarrierRules, depth: u8) -> Vec<ProvEdge> {
    let depth = depth.min(MAX_DEPTH);
    let mut seen: HashSet<String> = HashSet::new();
    let mut frontier: Vec<String> = Vec::new();
    for seed in seeds {
        if seen.insert(seed.clone()) {
            frontier.push(seed.clone());
        }
    }
    let mut found: HashSet<(String, String, String)> = HashSet::new();
    let mut edges: Vec<ProvEdge> = Vec::new();
    for _ in 0..depth {
        let mut next: Vec<String> = Vec::new();
        for node in &frontier {
            let holder = index
                .node(node)
                .map(|held| held.type_key.to_string())
                .unwrap_or_default();
            let uuid = identity::parse_address(node).0;
            for pointer in index.references_of(&uuid) {
                let field = last_segment(&pointer.path);
                let target = concept_of(&pointer.qualified_type);
                if !rules.follows(&holder, field, &target) {
                    continue;
                }
                let key = (node.clone(), pointer.uri.clone(), pointer.path.clone());
                if !found.insert(key) {
                    continue;
                }
                edges.push(ProvEdge {
                    from: NodeKey::Object(node.clone()),
                    to: NodeKey::Object(pointer.uri.clone()),
                    kind: ProvEdgeKind::Carrier {
                        path: pointer.path.clone(),
                    },
                    basis: EdgeBasis::Reference,
                });
                if seen.insert(pointer.uri.clone()) {
                    next.push(pointer.uri);
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    edges
}
