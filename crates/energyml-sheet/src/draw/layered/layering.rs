//! Layering: which layer each node sits in. A registry, the caller's additions
//! tried first (ADR-0006).

use std::collections::BTreeSet;

use super::{LayerInput, LayoutRefusal, oriented};

/// One way of choosing layers.
pub trait LayeringMethod: Send + Sync {
    /// The id the options name it by.
    fn id(&self) -> &'static str;

    /// One layer per node. `back` lists the edges drawn against the flow: the
    /// method must treat them reversed. `layered` checks the result.
    fn layers(&self, input: &LayerInput, back: &[usize]) -> Result<Vec<usize>, LayoutRefusal>;
}

/// The longest path from a source: every node one layer after the latest of
/// its predecessors. A fixed layer is a minimum.
pub struct LongestPath;

/// The fixed layer of every node, empty columns kept.
pub struct FixedColumns;

impl LayeringMethod for LongestPath {
    fn id(&self) -> &'static str {
        "longest-path"
    }

    fn layers(&self, input: &LayerInput, back: &[usize]) -> Result<Vec<usize>, LayoutRefusal> {
        let nodes = input.nodes.len();
        let fixed = |node: usize| -> Option<usize> { input.fixed_layer.get(node).copied().flatten() };
        let mut layer: Vec<usize> = (0..nodes).map(|n| fixed(n).unwrap_or(0)).collect();

        // The edges of the acyclic graph: not loops, reversed when back, and
        // not those between two nodes fixed to the same layer (flat).
        let mut out: Vec<Vec<(usize, usize)>> = vec![Vec::new(); nodes]; // (edge, target)
        let mut in_degree = vec![0usize; nodes];
        for (index, &(from, to)) in input.edges.iter().enumerate() {
            if from == to || from >= nodes || to >= nodes {
                continue;
            }
            let (a, b) = oriented(index, from, to, back);
            if let (Some(fa), Some(fb)) = (fixed(a), fixed(b))
                && fa == fb
            {
                continue;
            }
            out[a].push((index, b));
            in_degree[b] += 1;
        }

        // Kahn, the smallest ready index first (deterministic).
        let mut ready: BTreeSet<usize> = (0..nodes).filter(|&n| in_degree[n] == 0).collect();
        let mut done = vec![false; nodes];
        while let Some(&node) = ready.iter().next() {
            ready.remove(&node);
            done[node] = true;
            for &(_, target) in &out[node] {
                if layer[target] < layer[node] + 1 {
                    layer[target] = layer[node] + 1;
                }
                in_degree[target] -= 1;
                if in_degree[target] == 0 {
                    ready.insert(target);
                }
            }
        }
        if let Some(stuck) = (0..nodes).find(|&n| !done[n]) {
            // A cycle left: impossible after `break_cycles`, named if a caller
            // hands in a `back` that does not break them all.
            let edge = out[stuck].first().map_or(0, |&(edge, _)| edge);
            return Err(LayoutRefusal::MethodBroke {
                method: "longest-path".to_string(),
                edge,
            });
        }
        Ok(layer)
    }
}

impl LayeringMethod for FixedColumns {
    fn id(&self) -> &'static str {
        "fixed-columns"
    }

    fn layers(&self, input: &LayerInput, _back: &[usize]) -> Result<Vec<usize>, LayoutRefusal> {
        (0..input.nodes.len())
            .map(|node| {
                input
                    .fixed_layer
                    .get(node)
                    .copied()
                    .flatten()
                    .ok_or(LayoutRefusal::NoFixedLayer {
                        method: "fixed-columns",
                        node,
                    })
            })
            .collect()
    }
}

/// The layering registry: built-ins first, the caller's additions tried
/// before them.
pub struct LayeringMethods(Vec<Box<dyn LayeringMethod>>);

impl LayeringMethods {
    /// [`LongestPath`] and [`FixedColumns`], and nothing else.
    pub fn with_builtins() -> Self {
        LayeringMethods(vec![Box::new(LongestPath), Box::new(FixedColumns)])
    }

    /// Add a method, tried before every one already registered: it can replace
    /// a built-in of the same id, not just extend the list.
    pub fn register(&mut self, method: Box<dyn LayeringMethod>) {
        self.0.insert(0, method);
    }

    /// The method of this id, if one is registered.
    pub fn find(&self, id: &str) -> Option<&dyn LayeringMethod> {
        self.0
            .iter()
            .find(|method| method.id() == id)
            .map(|method| method.as_ref())
    }
}

impl Default for LayeringMethods {
    fn default() -> Self {
        Self::with_builtins()
    }
}
