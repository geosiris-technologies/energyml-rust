//! A layered placement of a directed graph: columns from left to right (or
//! rows from top to bottom), deterministic, pin-aware, with its edges routed
//! as cubic curves.
//!
//! The steps are separate and named, so each can be replaced or checked on
//! its own: back edges ([`cycles`]), layers ([`layering`]), order inside a
//! layer ([`ordering`]), positions ([`place`]) and routes ([`route`]). The
//! module does not draw: a view turns [`Placed`] into a
//! [`DisplayList`](crate::draw::list::DisplayList).
//!
//! # Rules
//!
//! - Nothing is dropped: a cycle is broken by drawing an edge against the flow
//!   (it is named in [`Placed::back`]), a self loop is a [`RouteKind::Loop`].
//! - Nothing is guessed: a graph beyond the bound, an edge naming a missing
//!   node or an unknown method is refused by name ([`LayoutRefusal`]).
//! - The same input gives the same numbers, to the bit: no hash map is
//!   iterated, every sort is stable and compares with `total_cmp`.
//! - The layering and ordering methods are registries (ADR-0006): the caller's
//!   additions are tried first, and what they return is checked here.

mod cycles;
mod layering;
mod ordering;
mod place;
mod route;

pub use cycles::break_cycles;
pub use layering::{FixedColumns, LayeringMethod, LayeringMethods, LongestPath};
pub use ordering::{
    Barycenter, Layers, Median, OrderingMethod, OrderingMethods, Slot, build_layers,
};
pub use place::{Positions, place};
pub use route::route;

use crate::draw::list::{PathData, Segment};

/// The most real nodes [`layered`] places. TODO(verify): the bound, by a
/// measurement.
pub const LAYERED_MAX_NODES: usize = 2_000;
/// The most slots (nodes and bend points) once long edges are cut by layer.
/// TODO(verify): the bound, by a measurement.
pub const LAYERED_MAX_SLOTS: usize = 20_000;

/// A node's box, user units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxSpec {
    pub width: f64,
    pub height: f64,
}

/// What [`layered`] places.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct LayerInput {
    pub nodes: Vec<BoxSpec>,
    /// (from, to), indices into `nodes`. Duplicates are kept; `(n, n)` is a loop.
    pub edges: Vec<(usize, usize)>,
    /// Empty, or one per node: the layer a node must sit in (a fixed column:
    /// files, activities, objects, carriers).
    pub fixed_layer: Vec<Option<usize>>,
    /// Empty, or one per node: where a dragged box was pinned (its top-left).
    pub pins: Vec<Option<[f64; 2]>>,
}

/// Which way the layers go.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Flow {
    /// Layers are columns, from left to right.
    #[default]
    LeftToRight,
    /// Layers are rows, from top to bottom.
    TopToBottom,
}

/// Presentation distances, user units. The defaults are a look, not data.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spacing {
    /// Around the whole drawing.
    pub margin: f64,
    /// Between two layers.
    pub layer_gap: f64,
    /// Between two boxes of a layer.
    pub node_gap: f64,
    /// Room a bend point takes in its layer.
    pub bend_gap: f64,
    /// How far a back edge swings out.
    pub back_bulge: f64,
    /// How far a self loop reaches.
    pub loop_size: f64,
}

impl Default for Spacing {
    fn default() -> Self {
        Spacing {
            margin: 16.0,
            layer_gap: 64.0,
            node_gap: 16.0,
            bend_gap: 8.0,
            back_bulge: 24.0,
            loop_size: 16.0,
        }
    }
}

/// How [`layered`] places a graph.
#[derive(Clone, Debug, PartialEq)]
pub struct LayeredOptions {
    /// The id of the layering method (`longest-path`, `fixed-columns`).
    pub layering: String,
    /// The id of the ordering method (`barycenter`, `median`).
    pub ordering: String,
    /// How many sweeps the ordering makes.
    pub sweeps: u8,
    pub flow: Flow,
    pub spacing: Spacing,
    pub max_nodes: usize,
    pub max_slots: usize,
}

impl Default for LayeredOptions {
    fn default() -> Self {
        LayeredOptions {
            layering: "longest-path".to_string(),
            ordering: "barycenter".to_string(),
            sweeps: 6,
            flow: Flow::LeftToRight,
            spacing: Spacing::default(),
            max_nodes: LAYERED_MAX_NODES,
            max_slots: LAYERED_MAX_SLOTS,
        }
    }
}

/// Why a graph is not placed. Typed; the face writes the sentence.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum LayoutRefusal {
    #[error(
        "{nodes} nodes are more than the {limit} a layered layout places: centre the graph on a selection"
    )]
    TooLarge { nodes: usize, limit: usize },
    #[error("cutting the long edges by layer makes {slots} slots, more than {limit}")]
    TooManySlots { slots: usize, limit: usize },
    #[error("edge {edge} names node {node}, and there are {nodes} nodes")]
    EdgeOutOfRange {
        edge: usize,
        node: usize,
        nodes: usize,
    },
    #[error("{what} has {found} entries, and there are {expected} nodes")]
    LengthMismatch {
        what: &'static str,
        expected: usize,
        found: usize,
    },
    #[error("node {node} has a size that is negative or not finite")]
    BadBox { node: usize },
    #[error("node {node} is pinned at a point that is negative or not finite")]
    BadPin { node: usize },
    #[error("the {method} layering needs a fixed layer for every node, and node {node} has none")]
    NoFixedLayer { method: &'static str, node: usize },
    #[error("no {kind} method is registered under `{id}`")]
    UnknownMethod { kind: &'static str, id: String },
    #[error("the {method} layering put edge {edge} against the flow")]
    MethodBroke { method: String, edge: usize },
}

/// A node's box once placed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// The layer the node sits in.
    pub layer: usize,
    /// Its position among the slots of its layer (bend points count).
    pub order: usize,
    /// Whether it sits where the caller pinned it.
    pub pinned: bool,
}

/// How an edge goes from its source to its target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteKind {
    /// With the flow, to a later layer.
    Forward,
    /// Against the flow (a cycle was broken here), swung out of the way.
    Back,
    /// Between two boxes of the same layer.
    Flat,
    /// From a node to itself.
    Loop,
}

/// An edge's route: cubic Bezier pieces `[start, c1, c2, end]`, from the
/// edge's own source to its own target (a back edge too).
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    /// The edge's index in the input.
    pub edge: usize,
    pub kind: RouteKind,
    pub segments: Vec<[[f64; 2]; 4]>,
}

impl Route {
    /// The route as a path (a `MoveTo`, then one `CubicTo` per piece).
    pub fn path(&self) -> PathData {
        let mut segments = Vec::with_capacity(self.segments.len() + 1);
        if let Some(first) = self.segments.first() {
            segments.push(Segment::MoveTo(first[0]));
        }
        for piece in &self.segments {
            segments.push(Segment::CubicTo {
                c1: piece[1],
                c2: piece[2],
                to: piece[3],
            });
        }
        PathData { segments }
    }
}

/// A graph, placed.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    /// One per node, in the input's order.
    pub boxes: Vec<PlacedBox>,
    /// One per edge, in the input's order.
    pub routes: Vec<Route>,
    /// The edges drawn against the flow, ascending: named, never dropped.
    pub back: Vec<usize>,
    /// Width and height of the drawing, margins included.
    pub size: [f64; 2],
    pub layer_count: usize,
    /// Crossings between edge pieces in the final order.
    pub crossings: usize,
    /// Crossings the order built from the layering had, before the ordering.
    pub initial_crossings: usize,
}

/// The two registries [`layered`] reads.
pub struct Methods {
    pub layering: LayeringMethods,
    pub ordering: OrderingMethods,
}

impl Default for Methods {
    fn default() -> Self {
        Methods {
            layering: LayeringMethods::with_builtins(),
            ordering: OrderingMethods::with_builtins(),
        }
    }
}

/// Place a graph in layers.
///
/// Steps, in order: validate; break the cycles; choose the layers; check what
/// the layering method returned (an edge must not go against the flow, whoever
/// wrote the method); cut long edges into bend points; order each layer (never
/// worse than the order it started from); position; route.
pub fn layered(
    input: &LayerInput,
    options: &LayeredOptions,
    methods: &Methods,
) -> Result<Placed, LayoutRefusal> {
    let nodes = input.nodes.len();

    // 1. Validation.
    if nodes > options.max_nodes {
        return Err(LayoutRefusal::TooLarge {
            nodes,
            limit: options.max_nodes,
        });
    }
    if !input.fixed_layer.is_empty() && input.fixed_layer.len() != nodes {
        return Err(LayoutRefusal::LengthMismatch {
            what: "fixed_layer",
            expected: nodes,
            found: input.fixed_layer.len(),
        });
    }
    if !input.pins.is_empty() && input.pins.len() != nodes {
        return Err(LayoutRefusal::LengthMismatch {
            what: "pins",
            expected: nodes,
            found: input.pins.len(),
        });
    }
    for (node, spec) in input.nodes.iter().enumerate() {
        if !valid_length(spec.width) || !valid_length(spec.height) {
            return Err(LayoutRefusal::BadBox { node });
        }
    }
    for (node, pin) in input.pins.iter().enumerate() {
        if let Some(point) = pin
            && (!valid_length(point[0]) || !valid_length(point[1]))
        {
            return Err(LayoutRefusal::BadPin { node });
        }
    }
    for (edge, &(from, to)) in input.edges.iter().enumerate() {
        for node in [from, to] {
            if node >= nodes {
                return Err(LayoutRefusal::EdgeOutOfRange { edge, node, nodes });
            }
        }
    }

    // 2. Back edges.
    let back = break_cycles(input);

    // 3. The methods, and the layers.
    let layering = methods
        .layering
        .find(&options.layering)
        .ok_or_else(|| LayoutRefusal::UnknownMethod {
            kind: "layering",
            id: options.layering.clone(),
        })?;
    let ordering = methods
        .ordering
        .find(&options.ordering)
        .ok_or_else(|| LayoutRefusal::UnknownMethod {
            kind: "ordering",
            id: options.ordering.clone(),
        })?;
    let layer = layering.layers(input, &back)?;
    if layer.len() != nodes {
        return Err(LayoutRefusal::LengthMismatch {
            what: "layers",
            expected: nodes,
            found: layer.len(),
        });
    }

    // 4. A method of the caller is checked, not trusted: an edge, once
    //    oriented by `back`, never goes to an earlier layer. Equal layers are
    //    a flat edge, which is accepted.
    for (edge, &(from, to)) in input.edges.iter().enumerate() {
        if from == to {
            continue;
        }
        let (a, b) = oriented(edge, from, to, &back);
        if layer[a] > layer[b] {
            return Err(LayoutRefusal::MethodBroke {
                method: layering.id().to_string(),
                edge,
            });
        }
    }

    // 5. Slots: nodes and bend points. The count is made before anything is
    //    built, so a huge span cannot allocate before it is refused.
    let layer_count = layer.iter().max().map_or(0, |top| top.saturating_add(1));
    let slots = ordering::slot_count(input, &layer, &back);
    if slots > options.max_slots {
        return Err(LayoutRefusal::TooManySlots {
            slots,
            limit: options.max_slots,
        });
    }
    if layer_count > options.max_slots {
        return Err(LayoutRefusal::TooManySlots {
            slots: layer_count,
            limit: options.max_slots,
        });
    }
    let mut layers = build_layers(input, &layer, &back);

    // 6. Order, never worse than where it started.
    let initial_crossings = layers.crossings();
    let initial_order = layers.order.clone();
    ordering.order(&mut layers, options.sweeps);
    let mut crossings = layers.crossings();
    if crossings > initial_crossings {
        layers.order = initial_order;
        crossings = initial_crossings;
    }

    // 7. Positions and routes.
    let positions = place(&layers, input, options.flow, &options.spacing);
    let routes = route(
        &layers,
        &positions,
        input,
        &back,
        options.flow,
        &options.spacing,
    );
    Ok(Placed {
        boxes: positions.boxes,
        routes,
        back,
        size: positions.size,
        layer_count: layers.order.len(),
        crossings,
        initial_crossings,
    })
}

// ---------------------------------------------------------------------------
// Helpers shared by the steps
// ---------------------------------------------------------------------------

/// A length that can be used: finite and not negative.
fn valid_length(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

/// An edge in the direction of the flow: reversed when it is a back edge.
fn oriented(edge: usize, from: usize, to: usize, back: &[usize]) -> (usize, usize) {
    if back.binary_search(&edge).is_ok() {
        (to, from)
    } else {
        (from, to)
    }
}

/// `(along the flow, across the flow)` of a point `[x, y]`.
fn along_cross(flow: Flow, point: [f64; 2]) -> (f64, f64) {
    match flow {
        Flow::LeftToRight => (point[0], point[1]),
        Flow::TopToBottom => (point[1], point[0]),
    }
}

/// The point `[x, y]` of an (along, across) pair.
fn point_of(flow: Flow, along: f64, cross: f64) -> [f64; 2] {
    match flow {
        Flow::LeftToRight => [along, cross],
        Flow::TopToBottom => [cross, along],
    }
}

/// A box's `(start, size)` along the flow.
fn along_span(flow: Flow, placed: &PlacedBox) -> (f64, f64) {
    match flow {
        Flow::LeftToRight => (placed.x, placed.width),
        Flow::TopToBottom => (placed.y, placed.height),
    }
}

/// A box's `(start, size)` across the flow.
fn cross_span(flow: Flow, placed: &PlacedBox) -> (f64, f64) {
    match flow {
        Flow::LeftToRight => (placed.y, placed.height),
        Flow::TopToBottom => (placed.x, placed.width),
    }
}
