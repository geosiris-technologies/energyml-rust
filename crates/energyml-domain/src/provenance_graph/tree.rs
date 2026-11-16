//! The tree of activities, triangulated by its two writings.
//!
//! An activity states its place in a tree twice: its own `Parent`, and the
//! `subActivity` parameters of the parent. The walk itself is one call to
//! [`crate::forest::forest`] (the generic forest, shared with the rows of a
//! self-referent table); this module only translates between activities and the
//! indices that function takes, and confronts the two writings.

use std::collections::{BTreeMap, BTreeSet};

use super::{GraphIssue, Statement, resolve_activity};
use crate::activity::{ActivityRecord, ActivityRole, ParameterRole, ParameterValue};
use crate::forest::{Parent, forest};

/// Why a link of the tree is not drawn.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TreeRefusal {
    /// A chain of `Parent` loops: these activities are shown flat.
    #[error("the Parent links of {uris:?} loop; they are shown flat")]
    ParentCycle {
        /// The activities of the loop, from the smallest in input order.
        uris: Vec<String>,
    },
    /// `Parent` names an activity the list does not hold: a marked root.
    #[error("{child} names {parent} as its parent, which the package does not hold")]
    ParentAbsent {
        /// The activity that names a parent.
        child: String,
        /// The parent it names, as written.
        parent: String,
    },
}

/// Whether the activities form a tree or a flat list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeShape {
    /// No usable `Parent` link: every activity is a root.
    Flat,
    /// At least one activity hangs under another.
    Tree,
}

/// The tree of a list of activities.
#[derive(Debug, Clone, PartialEq)]
pub struct ActivityTree {
    /// Flat or tree.
    pub shape: TreeShape,
    /// Root activity uris, in input order: the true roots, the activities whose
    /// parent is absent (marked by a [`TreeRefusal::ParentAbsent`]), and the
    /// activities in or under a loop (marked by a [`TreeRefusal::ParentCycle`]).
    pub roots: Vec<String>,
    /// Children of each activity uri that has some, ordered by the parent's
    /// sub-activity parameter position when it names them, then in input order.
    /// An activity in or under a loop has no entry.
    pub children: BTreeMap<String, Vec<String>>,
    /// What is not drawn, and why.
    pub refusals: Vec<TreeRefusal>,
    /// Only [`GraphIssue::TreeDisagreement`]: where the two writings differ.
    pub disagreements: Vec<GraphIssue>,
}

/// The activities a parent's `SubActivity` parameters name, in parameter order:
/// `(parameter position, index in the list)` of those that resolve.
fn named_children(
    activity: &ActivityRecord,
    roles: &[ParameterRole],
    activities: &[ActivityRecord],
) -> Vec<usize> {
    let mut named = Vec::new();
    for parameter in &activity.parameters {
        let is_sub = roles
            .iter()
            .any(|role| role.position == parameter.position && role.role == ActivityRole::SubActivity);
        if !is_sub {
            continue;
        }
        if let ParameterValue::Object(reference) = &parameter.value
            && let Some(child) = resolve_activity(&reference.address, activities)
        {
            named.push(child);
        }
    }
    named
}

/// Whether some parameter of `activity` has the role `SubActivity`.
fn has_sub_activity(roles: &[ParameterRole]) -> bool {
    roles.iter().any(|role| role.role == ActivityRole::SubActivity)
}

/// The roles of the activity at `index`; none when the caller gave fewer lists.
fn roles_at(roles: &[Vec<ParameterRole>], index: usize) -> &[ParameterRole] {
    roles.get(index).map(|list| list.as_slice()).unwrap_or(&[])
}

/// The tree of `activities`, `roles[i]` being the roles of `activities[i]`.
pub fn activity_tree(activities: &[ActivityRecord], roles: &[Vec<ParameterRole>]) -> ActivityTree {
    let roles_of = |index: usize| roles_at(roles, index);

    // 1. Each `Parent`, resolved among the activities by uuid then version.
    let mut refusals: Vec<TreeRefusal> = Vec::new();
    let mut parents: Vec<Parent> = Vec::with_capacity(activities.len());
    for activity in activities {
        match &activity.parent {
            None => parents.push(Parent::Root),
            Some(reference) => match resolve_activity(&reference.address, activities) {
                Some(parent) => parents.push(Parent::Of(parent)),
                None => {
                    refusals.push(TreeRefusal::ParentAbsent {
                        child: activity.uri.clone(),
                        parent: reference.address.clone(),
                    });
                    parents.push(Parent::Missing);
                }
            },
        }
    }

    // 2. One call: the walk is WP06's.
    let found = forest(&parents);

    // 3. Translate. Nodes in or under a loop are roots shown flat, never given children.
    for cycle in &found.cycles {
        refusals.push(TreeRefusal::ParentCycle {
            uris: cycle.iter().map(|node| activities[*node].uri.clone()).collect(),
        });
    }
    let mut flat: BTreeSet<usize> = BTreeSet::new();
    flat.extend(found.cycles.iter().flatten().copied());
    flat.extend(found.unreached.iter().copied());
    let mut root_nodes: BTreeSet<usize> = BTreeSet::new();
    root_nodes.extend(found.roots.iter().copied());
    root_nodes.extend(found.orphans.iter().copied());
    root_nodes.extend(flat.iter().copied());
    let roots: Vec<String> = root_nodes
        .iter()
        .map(|node| activities[*node].uri.clone())
        .collect();

    let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (parent, kids) in found.children.iter().enumerate() {
        if kids.is_empty() || flat.contains(&parent) {
            continue;
        }
        let kids: Vec<usize> = kids.iter().copied().filter(|kid| !flat.contains(kid)).collect();
        if kids.is_empty() {
            continue;
        }
        // The parent's own order first, for the children it names; then input order.
        let mut ordered: Vec<usize> = Vec::new();
        for named in named_children(&activities[parent], roles_of(parent), activities) {
            if kids.contains(&named) && !ordered.contains(&named) {
                ordered.push(named);
            }
        }
        for kid in &kids {
            if !ordered.contains(kid) {
                ordered.push(*kid);
            }
        }
        children.insert(
            activities[parent].uri.clone(),
            ordered
                .into_iter()
                .map(|kid| activities[kid].uri.clone())
                .collect(),
        );
    }

    // 4. The shape: at least one real link, outside any loop.
    let shape = if parents.iter().enumerate().any(|(node, parent)| {
        matches!(parent, Parent::Of(_)) && found.depth[node].is_some() && !flat.contains(&node)
    }) {
        TreeShape::Tree
    } else {
        TreeShape::Flat
    };

    // 5. Triangulation: the parent's sub-activity parameters against the child's `Parent`.
    //    A parent that names no sub-activity states the tree once: nothing to confront.
    let mut disagreements: Vec<GraphIssue> = Vec::new();
    for (parent, activity) in activities.iter().enumerate() {
        if !has_sub_activity(roles_of(parent)) {
            continue;
        }
        let named = named_children(activity, roles_of(parent), activities);
        for child in &named {
            if parents[*child] != Parent::Of(parent) {
                disagreements.push(GraphIssue::TreeDisagreement {
                    parent: activity.uri.clone(),
                    child: activities[*child].uri.clone(),
                    stated_by: Statement::ParameterOnly,
                });
            }
        }
        for (child, link) in parents.iter().enumerate() {
            if *link == Parent::Of(parent) && !named.contains(&child) {
                disagreements.push(GraphIssue::TreeDisagreement {
                    parent: activity.uri.clone(),
                    child: activities[child].uri.clone(),
                    stated_by: Statement::ParentOnly,
                });
            }
        }
    }

    ActivityTree {
        shape,
        roots,
        children,
        refusals,
        disagreements,
    }
}
