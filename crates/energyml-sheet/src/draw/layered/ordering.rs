//! Slots, the order of slots inside each layer, and the crossings that order
//! makes. The ordering methods are a registry, the caller's additions tried
//! first (ADR-0006).

use super::{LayerInput, oriented};

/// What sits in a layer: a node, or the point where a long edge passes
/// through a layer it does not end in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// A real node, by its index in the input.
    Node(usize),
    /// The bend of `edge` in the layer `step` layers after its first end
    /// (in the direction of the flow); `step` starts at 1.
    Bend {
        /// Index of the edge that bends here.
        edge: usize,
        /// Layers after the edge's first end, from 1.
        step: usize,
    },
}

/// The layered graph: nodes and bends, which layer each is in, their order in
/// the layer, and the pieces of edges between consecutive layers.
#[derive(Clone, Debug, PartialEq)]
pub struct Layers {
    /// Slot ids are indices here; the nodes come first, in node order.
    pub slots: Vec<Slot>,
    /// The layer of each slot.
    pub layer_of: Vec<usize>,
    /// Per layer, the slot ids in order.
    pub order: Vec<Vec<usize>>,
    /// `(upper slot, lower slot)` in consecutive layers, one per edge piece.
    pub links: Vec<(usize, usize)>,
}

impl Layers {
    /// Each slot's index in its layer.
    pub fn positions(&self) -> Vec<usize> {
        let mut positions = vec![0usize; self.slots.len()];
        for layer in &self.order {
            for (index, &slot) in layer.iter().enumerate() {
                positions[slot] = index;
            }
        }
        positions
    }

    /// Pairs of links that cross between consecutive layers: two links cross
    /// when one is above the other at the upper layer and below it at the
    /// lower one. Links that share an end do not cross. `O(e log e)`.
    pub fn crossings(&self) -> usize {
        let positions = self.positions();
        let mut by_layer: Vec<Vec<(usize, usize)>> = vec![Vec::new(); self.order.len()];
        for &(upper, lower) in &self.links {
            let layer = self.layer_of[upper];
            if layer < by_layer.len() {
                by_layer[layer].push((positions[upper], positions[lower]));
            }
        }
        let width = self.order.iter().map(Vec::len).max().unwrap_or(0) + 1;
        let mut total = 0usize;
        for links in &mut by_layer {
            links.sort_unstable();
            // Strict inversions of the lower positions, by a Fenwick tree.
            let mut tree = vec![0usize; width + 1];
            for (seen, &(_, lower)) in links.iter().enumerate() {
                // Count of earlier links whose lower position is not above
                // `lower`: seen - that = the inversions this link closes.
                let mut at_most = 0usize;
                let mut i = lower + 1;
                while i > 0 {
                    at_most += tree[i];
                    i &= i - 1;
                }
                total += seen - at_most;
                let mut i = lower + 1;
                while i <= width {
                    tree[i] += 1;
                    i += i & i.wrapping_neg();
                }
            }
        }
        total
    }
}

/// One way of ordering the slots of each layer.
pub trait OrderingMethod: Send + Sync {
    /// The id the options name it by.
    fn id(&self) -> &'static str;

    /// Reorder `layers.order` (a permutation of each layer's slots) in
    /// `sweeps` passes. `layered` falls back to the order it started from if
    /// the result crosses more.
    fn order(&self, layers: &mut Layers, sweeps: u8);
}

/// Each slot goes to the mean position of its neighbours in the layer the
/// sweep comes from.
pub struct Barycenter;

/// The same with the median of the neighbours' positions.
pub struct Median;

impl OrderingMethod for Barycenter {
    fn id(&self) -> &'static str {
        "barycenter"
    }

    fn order(&self, layers: &mut Layers, sweeps: u8) {
        sweep(layers, sweeps, |neighbours| {
            neighbours.iter().sum::<f64>() / neighbours.len() as f64
        });
    }
}

impl OrderingMethod for Median {
    fn id(&self) -> &'static str {
        "median"
    }

    fn order(&self, layers: &mut Layers, sweeps: u8) {
        sweep(layers, sweeps, |neighbours| {
            neighbours.sort_by(|a, b| a.total_cmp(b));
            let len = neighbours.len();
            if len % 2 == 1 {
                neighbours[len / 2]
            } else {
                (neighbours[len / 2 - 1] + neighbours[len / 2]) / 2.0
            }
        });
    }
}

/// `sweeps` passes, down (even) then up (odd). A slot with no neighbour in the
/// layer the pass comes from keeps its position as its key. The sort is
/// stable. The best order seen is kept (strictly fewer crossings; on a tie the
/// oldest), the starting order included, so the result never crosses more
/// than the start.
fn sweep(layers: &mut Layers, sweeps: u8, key: impl Fn(&mut Vec<f64>) -> f64) {
    let count = layers.order.len();
    if count < 2 || sweeps == 0 {
        return;
    }
    let mut up: Vec<Vec<usize>> = vec![Vec::new(); layers.slots.len()];
    let mut down: Vec<Vec<usize>> = vec![Vec::new(); layers.slots.len()];
    for &(upper, lower) in &layers.links {
        down[upper].push(lower);
        up[lower].push(upper);
    }

    let mut best_order = layers.order.clone();
    let mut best = layers.crossings();
    let mut positions = layers.positions();
    for pass in 0..sweeps {
        let descending = pass % 2 == 0;
        let indices: Vec<usize> = if descending {
            (1..count).collect()
        } else {
            (0..count - 1).rev().collect()
        };
        for layer in indices {
            let neighbours = if descending { &up } else { &down };
            let mut keyed: Vec<(f64, usize)> = layers.order[layer]
                .iter()
                .map(|&slot| {
                    let mut around: Vec<f64> = neighbours[slot]
                        .iter()
                        .map(|&other| positions[other] as f64)
                        .collect();
                    let value = if around.is_empty() {
                        positions[slot] as f64
                    } else {
                        key(&mut around)
                    };
                    (value, slot)
                })
                .collect();
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (index, &(_, slot)) in keyed.iter().enumerate() {
                positions[slot] = index;
                layers.order[layer][index] = slot;
            }
        }
        let now = layers.crossings();
        if now < best {
            best = now;
            best_order = layers.order.clone();
        }
    }
    layers.order = best_order;
}

/// How many slots [`build_layers`] would make, saturating: the nodes, plus one
/// bend per layer a non-flat edge passes through. Counted without building,
/// so a huge span is refused before it allocates.
pub(super) fn slot_count(input: &LayerInput, layer: &[usize], back: &[usize]) -> usize {
    let mut total = input.nodes.len();
    for (index, &(from, to)) in input.edges.iter().enumerate() {
        if from == to {
            continue;
        }
        let (a, b) = oriented(index, from, to, back);
        let span = layer[b].saturating_sub(layer[a]);
        total = total.saturating_add(span.saturating_sub(1));
    }
    total
}

/// The layered graph of a layering: one slot per node, then one bend per layer
/// a non-flat edge passes through (`span - 1` for an edge `a -> b` with
/// `span = layer[b] - layer[a] > 1`, in edge order). `links` chain
/// `a -> bend 1 -> ... -> b`; flat edges and loops have none. Each layer starts
/// ordered by node index, then bends by `(edge, step)`.
///
/// `back` lists the edges drawn against the flow: they are oriented reversed.
/// `layer` has one entry per node and the edges are in range (checked by
/// `layered`).
pub fn build_layers(input: &LayerInput, layer: &[usize], back: &[usize]) -> Layers {
    let nodes = input.nodes.len();
    let mut slots: Vec<Slot> = (0..nodes).map(Slot::Node).collect();
    let mut layer_of: Vec<usize> = layer.to_vec();
    let mut links: Vec<(usize, usize)> = Vec::new();

    for (index, &(from, to)) in input.edges.iter().enumerate() {
        if from == to {
            continue;
        }
        let (a, b) = oriented(index, from, to, back);
        if layer[b] <= layer[a] {
            continue;
        }
        let span = layer[b] - layer[a];
        let mut previous = a;
        for step in 1..span {
            let slot = slots.len();
            slots.push(Slot::Bend { edge: index, step });
            layer_of.push(layer[a] + step);
            links.push((previous, slot));
            previous = slot;
        }
        links.push((previous, b));
    }

    let count = layer.iter().max().map_or(0, |top| top + 1);
    let mut order: Vec<Vec<usize>> = vec![Vec::new(); count];
    // Nodes first by index, then bends by (edge, step): they were pushed in
    // that order, so walking the slots keeps it.
    for slot in 0..slots.len() {
        order[layer_of[slot]].push(slot);
    }
    Layers {
        slots,
        layer_of,
        order,
        links,
    }
}

/// The ordering registry: built-ins first, the caller's additions tried
/// before them.
pub struct OrderingMethods(Vec<Box<dyn OrderingMethod>>);

impl OrderingMethods {
    /// [`Barycenter`] and [`Median`], and nothing else.
    pub fn with_builtins() -> Self {
        OrderingMethods(vec![Box::new(Barycenter), Box::new(Median)])
    }

    /// Add a method, tried before every one already registered.
    pub fn register(&mut self, method: Box<dyn OrderingMethod>) {
        self.0.insert(0, method);
    }

    /// The method of this id, if one is registered.
    pub fn find(&self, id: &str) -> Option<&dyn OrderingMethod> {
        self.0
            .iter()
            .find(|method| method.id() == id)
            .map(|method| method.as_ref())
    }
}

impl Default for OrderingMethods {
    fn default() -> Self {
        Self::with_builtins()
    }
}
