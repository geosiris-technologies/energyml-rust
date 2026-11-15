//! The order of activities: measured when the file measures it, otherwise the
//! declared order or the order of dependencies - and it says which.
//!
//! No clock is read: an instant is a text the file wrote, placed on the line
//! through [`crate::series::instant::unix_seconds`], the one door that reads an
//! instant. An activity whose instants are all equal (a recipe writes one
//! `Creation` for the whole package) has no measured order; showing the order of
//! the file as if it were a time would be a drawing, so the refusal is **named**.

use std::collections::{HashMap, HashSet};

use energyml_core::identity;

use super::{NodeKey, ProvEdgeKind, ProvenanceGraph, TreeShape};
use crate::activity::ActivityRecord;
use crate::provenance_graph::ActivityTree;
use crate::provenance_verbatim::VerbatimProcess;
use crate::series::instant::unix_seconds;

/// Which order a timeline is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderBasis {
    /// Measured when it can be, otherwise declared or by dependency.
    Auto,
    /// By the instants the file writes.
    Measured,
    /// By the tree, parameter order first.
    Declared,
    /// By what each activity reads of what another wrote.
    Dependency,
}

/// Why the order shown is not a measured one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TimelineRefusal {
    /// Fewer than two distinct instants: no measured order.
    #[error("every activity carries the same instant (or none): there is no measured order, the order shown is {0:?}")]
    NoMeasuredOrder(OrderBasis),
}

/// One activity on the line.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineRow {
    /// The activity's uri.
    pub activity: String,
    /// The instant as written (`Creation`, or the verbatim timestamp).
    pub instant: Option<String>,
    /// Milliseconds from the earliest placed instant; measured order only.
    pub offset_ms: Option<i64>,
    /// Position in the order shown (0 first).
    pub rank: usize,
}

/// The activities in an order, and the basis of that order.
#[derive(Debug, Clone, PartialEq)]
pub struct Timeline {
    /// The order really used.
    pub basis_used: OrderBasis,
    /// One row per activity, in the order shown.
    pub rows: Vec<TimelineRow>,
    /// Set whenever a measured order was impossible and another is shown.
    pub note: Option<TimelineRefusal>,
}

/// Whether the address names the activity: same uuid, versions that agree.
fn names(address: &str, activity: &ActivityRecord) -> bool {
    let (uuid, version) = identity::parse_address(address);
    let (held, held_version) = identity::parse_address(&activity.uri);
    uuid == held && identity::versions_agree(version.as_deref(), held_version.as_deref())
}

/// The instant text of an activity: its `Creation`, else the `timestamp` field
/// of a verbatim process tied to it.
fn instant_text(activity: &ActivityRecord, verbatim: &[VerbatimProcess]) -> Option<String> {
    if let Some(creation) = activity.creation.as_deref()
        && !creation.trim().is_empty()
    {
        return Some(creation.to_string());
    }
    verbatim.iter().find_map(|process| {
        let tied = [process.activity.as_deref(), process.activity_by_mapping.as_deref()]
            .into_iter()
            .flatten()
            .any(|address| names(address, activity));
        if !tied {
            return None;
        }
        process
            .fields
            .iter()
            .find(|(title, text)| title == "timestamp" && !text.trim().is_empty())
            .map(|(_, text)| text.clone())
    })
}

/// The order of `activities`.
///
/// `tree` gives the declared order, `graph` the dependencies (an edge
/// `A -> o` `Produced` and `o -> B` `Consumed` puts A before B), `verbatim` the
/// instants an activity does not carry itself.
pub fn timeline(
    activities: &[ActivityRecord],
    verbatim: &[VerbatimProcess],
    tree: &ActivityTree,
    graph: &ProvenanceGraph,
    basis: OrderBasis,
) -> Timeline {
    let count = activities.len();

    // 1. The instants. An unreadable text (no zone, a leap second) is not placed.
    let texts: Vec<Option<String>> = activities
        .iter()
        .map(|activity| instant_text(activity, verbatim))
        .collect();
    let placed: Vec<Option<f64>> = texts
        .iter()
        .map(|text| text.as_deref().and_then(|text| unix_seconds(text).ok()))
        .collect();

    // 2. The declared rank: preorder over the tree, then what it does not reach.
    let index_of: HashMap<&str, usize> = activities
        .iter()
        .enumerate()
        .map(|(at, activity)| (activity.uri.as_str(), at))
        .collect();
    let mut declared_order: Vec<usize> = Vec::with_capacity(count);
    let mut visited = vec![false; count];
    let mut stack: Vec<usize> = tree
        .roots
        .iter()
        .rev()
        .filter_map(|uri| index_of.get(uri.as_str()).copied())
        .collect();
    while let Some(at) = stack.pop() {
        if visited[at] {
            continue;
        }
        visited[at] = true;
        declared_order.push(at);
        if let Some(kids) = tree.children.get(&activities[at].uri) {
            stack.extend(
                kids.iter()
                    .rev()
                    .filter_map(|uri| index_of.get(uri.as_str()).copied()),
            );
        }
    }
    for at in 0..count {
        if !visited[at] {
            declared_order.push(at);
        }
    }
    let mut declared = vec![0usize; count];
    for (rank, at) in declared_order.iter().enumerate() {
        declared[*at] = rank;
    }

    // 3. Is a measured order possible: two placed instants that differ.
    let first = placed.iter().flatten().copied().next();
    let measured_possible = match first {
        Some(first) => placed.iter().flatten().any(|seconds| *seconds != first),
        None => false,
    };

    // 4. The basis really used.
    let fallback = if tree.shape == TreeShape::Tree {
        OrderBasis::Declared
    } else {
        OrderBasis::Dependency
    };
    let (used, note) = match basis {
        OrderBasis::Auto | OrderBasis::Measured => {
            if measured_possible {
                (OrderBasis::Measured, None)
            } else {
                (fallback, Some(TimelineRefusal::NoMeasuredOrder(fallback)))
            }
        }
        OrderBasis::Declared => (OrderBasis::Declared, None),
        OrderBasis::Dependency => (OrderBasis::Dependency, None),
    };

    // 5 and 6. The order itself.
    let order: Vec<usize> = match used {
        OrderBasis::Measured => {
            let mut with: Vec<usize> = (0..count).filter(|at| placed[*at].is_some()).collect();
            with.sort_by(|a, b| {
                let left = placed[*a].unwrap_or(0.0);
                let right = placed[*b].unwrap_or(0.0);
                left.partial_cmp(&right)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(declared[*a].cmp(&declared[*b]))
            });
            let mut without: Vec<usize> = (0..count).filter(|at| placed[*at].is_none()).collect();
            without.sort_by_key(|at| declared[*at]);
            with.extend(without);
            with
        }
        OrderBasis::Dependency => dependency_order(activities, graph, &declared),
        OrderBasis::Declared | OrderBasis::Auto => declared_order,
    };

    let earliest = placed.iter().flatten().copied().fold(f64::INFINITY, f64::min);
    let rows = order
        .iter()
        .enumerate()
        .map(|(rank, at)| TimelineRow {
            activity: activities[*at].uri.clone(),
            instant: texts[*at].clone(),
            offset_ms: if used == OrderBasis::Measured {
                placed[*at].map(|seconds| ((seconds - earliest) * 1000.0).round() as i64)
            } else {
                None
            },
            rank,
        })
        .collect();

    Timeline {
        basis_used: used,
        rows,
        note,
    }
}

/// Kahn's order: A before B when A wrote something B read. Ties, and what a
/// loop leaves, go by declared rank.
fn dependency_order(activities: &[ActivityRecord], graph: &ProvenanceGraph, declared: &[usize]) -> Vec<usize> {
    let count = activities.len();
    let activity_at: HashMap<NodeKey, usize> = activities
        .iter()
        .enumerate()
        .map(|(at, activity)| (NodeKey::Object(activity.uri.clone()), at))
        .collect();
    // Which activities wrote each object.
    let mut writers: HashMap<&NodeKey, Vec<usize>> = HashMap::new();
    for edge in &graph.edges {
        if edge.kind == ProvEdgeKind::Produced
            && let Some(at) = activity_at.get(&edge.from)
        {
            writers.entry(&edge.to).or_default().push(*at);
        }
    }
    let mut successors: Vec<Vec<usize>> = vec![Vec::new(); count];
    let mut waiting = vec![0usize; count];
    let mut linked: HashSet<(usize, usize)> = HashSet::new();
    for edge in &graph.edges {
        if edge.kind != ProvEdgeKind::Consumed {
            continue;
        }
        let Some(reader) = activity_at.get(&edge.to) else {
            continue;
        };
        for writer in writers.get(&edge.from).into_iter().flatten() {
            if writer != reader && linked.insert((*writer, *reader)) {
                successors[*writer].push(*reader);
                waiting[*reader] += 1;
            }
        }
    }
    let mut done = vec![false; count];
    let mut order: Vec<usize> = Vec::with_capacity(count);
    while let Some(next) = (0..count)
        .filter(|at| !done[*at] && waiting[*at] == 0)
        .min_by_key(|at| declared[*at])
    {
        done[next] = true;
        order.push(next);
        for after in &successors[next] {
            waiting[*after] -= 1;
        }
    }
    let mut rest: Vec<usize> = (0..count).filter(|at| !done[*at]).collect();
    rest.sort_by_key(|at| declared[*at]);
    order.extend(rest);
    order
}
