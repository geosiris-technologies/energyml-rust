//! `draw::layered`, the placement of a graph in layers: each test states the
//! rule it checks and what would have to break for it to turn red.
//!
//! The rules come from the design of the two graph views (the provenance graph
//! and the table structure), not from any package: every graph here is built
//! in memory.

use energyml_sheet::draw::layered::{
    BoxSpec, Flow, LAYERED_MAX_NODES, LayerInput, LayeredOptions, LayeringMethod, LayoutRefusal, Methods,
    Placed, PlacedBox, RouteKind, layered,
};

fn boxes(count: usize) -> Vec<BoxSpec> {
    (0..count)
        .map(|_| BoxSpec {
            width: 40.0,
            height: 20.0,
        })
        .collect()
}

fn input(count: usize, edges: &[(usize, usize)]) -> LayerInput {
    LayerInput {
        nodes: boxes(count),
        edges: edges.to_vec(),
        ..LayerInput::default()
    }
}

fn place(input: &LayerInput) -> Result<Placed, LayoutRefusal> {
    layered(input, &LayeredOptions::default(), &Methods::default())
}

/// The twelve-node graph of the determinism, overlap and pin tests: sizes
/// `40 + 10 (i % 3)` by `20 + 5 (i % 4)`, edges `(i, (5 i + 3) % 12)` without
/// the loops, plus `(0, 11)` and `(11, 0)`.
fn twelve() -> LayerInput {
    let nodes = (0..12)
        .map(|i| BoxSpec {
            width: 40.0 + 10.0 * (i % 3) as f64,
            height: 20.0 + 5.0 * (i % 4) as f64,
        })
        .collect();
    let mut edges: Vec<(usize, usize)> = (0..12)
        .map(|i| (i, (5 * i + 3) % 12))
        .filter(|(a, b)| a != b)
        .collect();
    edges.push((0, 11));
    edges.push((11, 0));
    LayerInput {
        nodes,
        edges,
        ..LayerInput::default()
    }
}

fn overlap(a: &PlacedBox, b: &PlacedBox) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

/// Design of the provenance graph, test 30: a cycle is broken by reversing an
/// edge, never by dropping one.
///
/// To break: delete the back edge instead of reversing it (`routes` would
/// have one entry too few).
#[test]
fn a_two_node_cycle_reverses_one_edge_and_drops_none() {
    let placed = place(&input(2, &[(0, 1), (1, 0)])).unwrap();
    assert_eq!(placed.back, vec![1]);
    assert_eq!(placed.routes.len(), 2);
    assert_eq!(placed.routes[1].kind, RouteKind::Back);
    // The back edge keeps its sense: it starts on box 1, its own source.
    let start = placed.routes[1].segments[0][0];
    let b = &placed.boxes[1];
    assert!(start[0] >= b.x && start[0] <= b.x + b.width);
    assert!(start[1] >= b.y && start[1] <= b.y + b.height);
}

/// Design of the provenance graph, test 31: a declared column holds whatever
/// the graph does.
///
/// To break: let the depth-first walk decide the back edges when every node
/// is fixed (the first edge, `(2, 0)`, would not be the one reversed).
#[test]
fn a_fixed_column_holds_whatever_the_graph() {
    let mut graph = input(4, &[(2, 0), (0, 1), (1, 2)]);
    graph.fixed_layer = vec![Some(0), Some(1), Some(2), Some(1)];
    let options = LayeredOptions {
        layering: "fixed-columns".to_string(),
        ..LayeredOptions::default()
    };
    let placed = layered(&graph, &options, &Methods::default()).unwrap();
    assert_eq!(placed.boxes[0].layer, 0);
    assert_eq!(placed.boxes[1].layer, 1);
    assert_eq!(placed.boxes[2].layer, 2);
    assert_eq!(placed.boxes[3].layer, 1);
    assert_eq!(placed.back, vec![0]);
}

/// Design of the provenance graph, test 32: the ordering never crosses more
/// than the order it started from; here the three pairs cross at first.
///
/// To break: set `sweeps` to 0 in the options (3 crossings remain).
#[test]
fn barycentre_does_not_cross_more_than_the_initial_order() {
    let placed = place(&input(6, &[(0, 5), (1, 4), (2, 3)])).unwrap();
    assert_eq!(placed.initial_crossings, 3);
    assert_eq!(placed.crossings, 0);
    // Layer 1 reads b2, b1, b0.
    assert_eq!(placed.boxes[5].order, 0);
    assert_eq!(placed.boxes[4].order, 1);
    assert_eq!(placed.boxes[3].order, 2);
}

/// Design of the provenance graph, test 33: beyond the bound the layout is
/// refused, not slowly attempted.
///
/// To break: remove the check on the node count.
#[test]
fn beyond_the_bound_the_layout_is_refused() {
    let graph = input(LAYERED_MAX_NODES + 1, &[]);
    assert_eq!(
        place(&graph),
        Err(LayoutRefusal::TooLarge {
            nodes: 2001,
            limit: 2000
        })
    );
}

/// Design of the table structure: the same graph placed twice gives the same
/// numbers, to the bit.
///
/// To break: sort something by a `HashMap` iteration (its order varies from
/// one run to the next).
#[test]
fn layered_layout_is_deterministic() {
    let graph = twelve();
    assert_eq!(place(&graph).unwrap(), place(&graph).unwrap());
}

/// Design of the table structure: two boxes never overlap, in either flow.
///
/// To break: stack the boxes of a layer without `node_gap` and with the same
/// start (every box of a layer would overlap its neighbour).
#[test]
fn no_two_boxes_overlap() {
    let graph = twelve();
    for flow in [Flow::LeftToRight, Flow::TopToBottom] {
        let options = LayeredOptions {
            flow,
            ..LayeredOptions::default()
        };
        let placed = layered(&graph, &options, &Methods::default()).unwrap();
        for i in 0..placed.boxes.len() {
            for j in i + 1..placed.boxes.len() {
                assert!(
                    !overlap(&placed.boxes[i], &placed.boxes[j]),
                    "boxes {i} and {j} overlap with {flow:?}"
                );
            }
        }
    }
}

/// Design of the table structure: a box the user pinned stays where it was
/// put, and no other box overlaps it.
///
/// To break: ignore the pins (the box would sit at its layer's position).
#[test]
fn a_pinned_node_stays_where_pinned() {
    let mut graph = twelve();
    graph.pins = vec![None; 12];
    graph.pins[2] = Some([300.0, 10.0]);
    let placed = place(&graph).unwrap();
    let pinned = &placed.boxes[2];
    assert_eq!((pinned.x, pinned.y), (300.0, 10.0));
    assert!(pinned.pinned);
    for (index, other) in placed.boxes.iter().enumerate() {
        if index != 2 {
            assert!(!other.pinned);
            assert!(!overlap(pinned, other), "box {index} overlaps the pinned box");
        }
    }
}

/// A long edge passes through one bend point per layer it skips: with the
/// chain `0 -> 1 -> 2 -> 3` and the edge `(0, 3)`, that edge has 3 pieces.
///
/// To break: do not create the bends (the route would be one piece).
#[test]
fn a_long_edge_passes_through_one_point_per_layer() {
    let placed = place(&input(4, &[(0, 1), (1, 2), (2, 3), (0, 3)])).unwrap();
    assert_eq!(placed.routes[3].kind, RouteKind::Forward);
    assert_eq!(placed.routes[3].segments.len(), 3);
    assert_eq!(placed.routes[0].segments.len(), 1);
    // The route is one connected curve.
    let pieces = &placed.routes[3].segments;
    assert_eq!(pieces[0][3], pieces[1][0]);
    assert_eq!(pieces[1][3], pieces[2][0]);
}

/// A self loop is kept and drawn as a loop, not taken for a back edge.
///
/// To break: treat `(1, 1)` as a cycle to break (it would be in `back`).
#[test]
fn a_self_loop_is_kept_and_drawn_as_a_loop() {
    let placed = place(&input(2, &[(0, 1), (1, 1)])).unwrap();
    assert!(placed.back.is_empty());
    assert_eq!(placed.routes[1].kind, RouteKind::Loop);
    assert_eq!(placed.routes[1].segments.len(), 1);
}

/// A method id nobody registered is refused by its name, never replaced by
/// the default one without saying so.
///
/// To break: fall back on `longest-path` when the id is not found.
#[test]
fn an_unknown_method_is_refused_by_name() {
    let options = LayeredOptions {
        layering: "spring".to_string(),
        ..LayeredOptions::default()
    };
    assert_eq!(
        layered(&input(2, &[(0, 1)]), &options, &Methods::default()),
        Err(LayoutRefusal::UnknownMethod {
            kind: "layering",
            id: "spring".to_string()
        })
    );
}

/// A method of the caller is tried before the built-in of the same id, and
/// what it returns is checked (ADR-0006: a registry; ADR-0005: refuse).
///
/// To break: register by `push` (the built-in would answer first), or remove
/// the check of the layers (the layout would be drawn with an edge backwards).
#[test]
fn a_callers_method_is_tried_first_and_checked() {
    struct Backwards;
    impl LayeringMethod for Backwards {
        fn id(&self) -> &'static str {
            "longest-path"
        }
        fn layers(&self, _: &LayerInput, _: &[usize]) -> Result<Vec<usize>, LayoutRefusal> {
            Ok(vec![1, 0])
        }
    }
    let mut methods = Methods::default();
    methods.layering.register(Box::new(Backwards));
    let graph = input(2, &[(0, 1)]);
    let found = methods.layering.find("longest-path").unwrap();
    assert_eq!(found.layers(&graph, &[]), Ok(vec![1, 0]));
    assert_eq!(
        layered(&graph, &LayeredOptions::default(), &methods),
        Err(LayoutRefusal::MethodBroke {
            method: "longest-path".to_string(),
            edge: 0
        })
    );
}

/// An edge to a node that does not exist is refused, not indexed (a panic).
///
/// To break: index the nodes without checking the edges first.
#[test]
fn an_edge_to_a_missing_node_is_refused() {
    assert_eq!(
        place(&input(3, &[(0, 7)])),
        Err(LayoutRefusal::EdgeOutOfRange {
            edge: 0,
            node: 7,
            nodes: 3
        })
    );
}
