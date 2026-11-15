//! A forest over nodes `0..n` that each name their parent - generic, so the rows of a
//! self-referent table and the activities of a package are walked by one function.
//!
//! Nothing here knows what a node is. The caller says, for each node, whether it has no
//! parent ([`Parent::Root`]), names the node `n` ([`Parent::Of`]) or names a parent that is
//! not among the nodes ([`Parent::Missing`]), and [`forest`] returns the roots, the
//! orphans, the children, the depths and - never cut in silence - the cycles.
//!
//! **A cycle is a result, not an accident to repair.** Nodes that are their own ancestors
//! are named in [`Forest::cycles`]; the nodes that hang under a cycle are named in
//! [`Forest::unreached`]; neither is attached to a root, because attaching them would draw
//! a tree the file does not state.

use std::collections::BTreeSet;

/// What a node says of its parent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parent {
    /// No parent: a root.
    Root,
    /// The parent is node `n`.
    Of(usize),
    /// A parent is named and is not among the nodes: a root **marked** orphan, never a
    /// root like the others.
    Missing,
}

/// The forest the parents describe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Forest {
    /// `Parent::Root` nodes, in input order.
    pub roots: Vec<usize>,
    /// `Parent::Missing` nodes (and `Of(n)` with `n` out of range), in input order.
    pub orphans: Vec<usize>,
    /// The children of each node, in input order.
    pub children: Vec<Vec<usize>>,
    /// Each cycle once, starting at its smallest node, then following the parents.
    pub cycles: Vec<Vec<usize>>,
    /// Nodes outside any cycle whose parent chain ends in one: no root reaches them.
    pub unreached: Vec<usize>,
    /// Depth from its root or orphan (0 for them); `None` for a node in or under a cycle.
    pub depth: Vec<Option<usize>>,
}

/// The parent a node really has among `0..n`, if any.
fn parent_node(parent: Parent, n: usize) -> Option<usize> {
    match parent {
        Parent::Of(p) if p < n => Some(p),
        _ => None,
    }
}

/// Depth-first, preorder: the roots then the orphans, children in input order.
fn traverse(roots: &[usize], orphans: &[usize], children: &[Vec<usize>]) -> Vec<(usize, usize)> {
    let mut stack: Vec<(usize, usize)> = roots
        .iter()
        .chain(orphans.iter())
        .rev()
        .map(|node| (*node, 0))
        .collect();
    let mut walked = Vec::new();
    while let Some((node, depth)) = stack.pop() {
        walked.push((node, depth));
        if let Some(kids) = children.get(node) {
            stack.extend(kids.iter().rev().map(|kid| (*kid, depth + 1)));
        }
    }
    walked
}

/// Build the forest. Linear in the number of nodes.
pub fn forest(parents: &[Parent]) -> Forest {
    let n = parents.len();
    let mut roots = Vec::new();
    let mut orphans = Vec::new();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (node, parent) in parents.iter().enumerate() {
        match parent {
            Parent::Root => roots.push(node),
            Parent::Missing => orphans.push(node),
            Parent::Of(p) if *p >= n => orphans.push(node),
            Parent::Of(p) => children[*p].push(node),
        }
    }

    // Cycles: an iterative walk up the parents in three states (unseen, on the current
    // path, finished). Meeting a node on the path closes a cycle.
    const UNSEEN: u8 = 0;
    const ON_PATH: u8 = 1;
    const DONE: u8 = 2;
    let mut state = vec![UNSEEN; n];
    let mut in_cycle = vec![false; n];
    // Whether the chain above a node ends in a cycle (true for the cycle's own nodes).
    let mut leads_to_cycle = vec![false; n];
    let mut cycles: Vec<Vec<usize>> = Vec::new();
    for start in 0..n {
        if state[start] != UNSEEN {
            continue;
        }
        let mut path: Vec<usize> = Vec::new();
        let mut current = start;
        let mut ends_in_cycle = false;
        loop {
            match state[current] {
                UNSEEN => {
                    state[current] = ON_PATH;
                    path.push(current);
                    match parent_node(parents[current], n) {
                        Some(next) => current = next,
                        None => break,
                    }
                }
                ON_PATH => {
                    // `current` is on the path: the part of the path from it is a cycle.
                    let at = path.iter().position(|node| *node == current).unwrap_or(0);
                    let mut cycle: Vec<usize> = path[at..].to_vec();
                    let smallest = cycle
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, node)| **node)
                        .map(|(position, _)| position)
                        .unwrap_or(0);
                    cycle.rotate_left(smallest);
                    for node in &cycle {
                        in_cycle[*node] = true;
                    }
                    cycles.push(cycle);
                    ends_in_cycle = true;
                    break;
                }
                _ => {
                    ends_in_cycle = leads_to_cycle[current];
                    break;
                }
            }
        }
        for node in path {
            state[node] = DONE;
            if ends_in_cycle {
                leads_to_cycle[node] = true;
            }
        }
    }
    let unreached: Vec<usize> = (0..n).filter(|node| leads_to_cycle[*node] && !in_cycle[*node]).collect();

    let mut depth: Vec<Option<usize>> = vec![None; n];
    for (node, at) in traverse(&roots, &orphans, &children) {
        depth[node] = Some(at);
    }

    Forest {
        roots,
        orphans,
        children,
        cycles,
        unreached,
        depth,
    }
}

impl Forest {
    /// No cycle and nothing unreached.
    pub fn is_whole(&self) -> bool {
        self.cycles.is_empty() && self.unreached.is_empty()
    }

    /// Depth-first, preorder: the roots then the orphans, children in input order;
    /// `(node, depth)`. A node in or under a cycle is not in the walk.
    pub fn walk(&self) -> Vec<(usize, usize)> {
        traverse(&self.roots, &self.orphans, &self.children)
    }

    /// The parents of a node, nearest first, up to its root or orphan; empty for a root.
    /// A node in or under a cycle stops when the chain would come back on itself.
    pub fn ancestors(&self, node: usize, parents: &[Parent]) -> Vec<usize> {
        let n = parents.len();
        let mut seen = BTreeSet::new();
        seen.insert(node);
        let mut found = Vec::new();
        let mut current = node;
        while let Some(parent) = parents.get(current).and_then(|parent| parent_node(*parent, n)) {
            if !seen.insert(parent) {
                break;
            }
            found.push(parent);
            current = parent;
        }
        found
    }
}
