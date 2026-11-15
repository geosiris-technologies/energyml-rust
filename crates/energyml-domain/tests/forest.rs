//! The generic forest of `energyml_domain::forest` (WP06 §4.11).
//!
//! No specification governs it: the rules are the ones the views state - a cycle is
//! *named*, never cut in silence (07 §2.3), an orphan is a root *marked*, and a parent that
//! is out of range is an orphan, not a panic. Every test builds its parents by hand and
//! writes the expected forest in the test.
//!
//! What it takes to make each one fail is in the last line of its comment.

use energyml_domain::forest::{Parent, forest};

#[test]
fn a_forest_has_its_roots_children_and_depths() {
    let parents = [Parent::Root, Parent::Of(0), Parent::Of(0), Parent::Of(1)];
    let found = forest(&parents);
    assert_eq!(found.roots, vec![0]);
    assert_eq!(found.children[0], vec![1, 2], "children keep input order");
    assert_eq!(found.depth[3], Some(2));
    assert_eq!(found.walk(), vec![(0, 0), (1, 1), (3, 2), (2, 1)], "preorder");
    assert!(found.is_whole());
    assert_eq!(found.ancestors(3, &parents), vec![1, 0], "nearest first");
    // Break it: push the children in reverse order, and `children[0]` and the walk change.
}

#[test]
fn an_orphan_parent_is_a_marked_root() {
    let found = forest(&[Parent::Root, Parent::Missing, Parent::Of(1)]);
    assert_eq!(found.orphans, vec![1]);
    assert_eq!(found.roots, vec![0], "an orphan is not a root like the others");
    assert_eq!(found.depth[2], Some(1));
    assert_eq!(found.depth[1], Some(0));
    // Break it: put the orphan in `roots`.
}

#[test]
fn a_cycle_names_its_nodes_and_never_hangs_on_a_root() {
    // 0 -> 2 -> 1 -> 0 is a cycle; 3 hangs under it; 4 is a root.
    let found = forest(&[
        Parent::Of(2),
        Parent::Of(0),
        Parent::Of(1),
        Parent::Of(0),
        Parent::Root,
    ]);
    assert_eq!(
        found.cycles,
        vec![vec![0, 2, 1]],
        "from the smallest node, following the parents"
    );
    assert_eq!(found.unreached, vec![3]);
    assert_eq!(found.roots, vec![4]);
    assert_eq!(found.depth[0], None);
    assert_eq!(found.depth[3], None, "a node under a cycle has no depth");
    assert!(!found.is_whole());
    assert_eq!(found.walk(), vec![(4, 0)], "the walk never enters a cycle");
    // Break it: cut the cycle at its first node, and `cycles` is empty.
}

#[test]
fn a_node_that_is_its_own_parent_is_a_cycle_of_one() {
    let found = forest(&[Parent::Of(0)]);
    assert_eq!(found.cycles, vec![vec![0]]);
    assert!(found.roots.is_empty());
    // Break it: treat a self parent as a root.
}

#[test]
fn a_parent_out_of_range_is_an_orphan() {
    let found = forest(&[Parent::Of(7)]);
    assert_eq!(found.orphans, vec![0]);
    assert!(found.cycles.is_empty());
    // Break it: index `children[7]`, and the test panics.
}

#[test]
fn an_empty_forest_is_whole() {
    let found = forest(&[]);
    assert!(found.is_whole());
    assert!(found.walk().is_empty());
}
