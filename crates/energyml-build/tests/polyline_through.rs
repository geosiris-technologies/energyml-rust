//! A selection of nodes as an input — `Source::only_nodes` — and the line
//! through them, `polyline-through`. The need:
//! create a polyline from the selection […] or extract them as a point cloud.
//!
//! # What would have to break to make it turn red
//!
//! Reading the ranks in a single patch; sorting the nodes instead of keeping
//! the given order; keeping the topology of the shape; letting through a rank outside
//! the shape or an empty selection; forgetting the closure asked for; joining
//! the shapes in another order than that of the slot.

use energyml_build::operation::{Choices, Identity, Plan, Purpose, Registry, When};
use energyml_build::pieces::{Pieces, Source};
use energyml_geometry::geometry::{Geometry, Patch, Topology};
use serde_json::json;

fn when() -> When {
    When { creation: "2026-09-14T00:00:00Z".into(), originator: "test".into() }
}

fn plan(operation: &str, bag: &Pieces, choices: Choices) -> Result<Plan, energyml_build::operation::Error> {
    Registry::new().plan_for(operation, bag, &choices, &Identity::derived(), &when(), Purpose::Geometry)
}

/// A fan of four triangles (nodes 0–4), then a second patch of two nodes (5, 6).
fn surface() -> Source {
    let fan = Patch {
        index: 0,
        node_offset: 0,
        points: vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 2.0, 0.0], [0.0, 2.0, 0.0], [1.0, 1.0, 1.0]],
        topology: Topology::Triangles(vec![[0, 1, 4], [1, 2, 4], [2, 3, 4], [3, 0, 4]]),
        closed: Vec::new(),
    };
    let more = Patch {
        index: 1,
        node_offset: 5,
        points: vec![[5.0, 0.0, 0.0], [6.0, 0.0, 0.0]],
        topology: Topology::Polylines(vec![2]),
        closed: vec![false],
    };
    Source::Shape { geometry: Geometry { kind: "test", patches: vec![fan, more] }, closed: None }
}

fn points_of(source: &Source) -> Vec<[f64; 3]> {
    match source {
        Source::Shape { geometry, .. } => geometry.patches.iter().flat_map(|p| p.points.iter().copied()).collect(),
        other => panic!("{} is not a shape", other.kind()),
    }
}

/// **The nodes named, in the order named, across the patches — and nothing of
/// the topology.** Node 5 is the first of the second patch.
#[test]
fn a_selection_of_nodes_is_the_cloud_of_them_in_its_order() {
    let only = surface().only_nodes(&[5, 0, 2]).unwrap();
    assert_eq!(points_of(&only), vec![[5.0, 0.0, 0.0], [0.0, 0.0, 0.0], [2.0, 2.0, 0.0]]);
    let Source::Shape { geometry, closed } = &only else { unreachable!() };
    assert_eq!(geometry.patches.len(), 1);
    assert_eq!(geometry.patches[0].topology, Topology::Points);
    assert_eq!(closed, &None, "the closure spoke of the lines, which are gone");
}

#[test]
fn a_rank_past_the_shape_or_no_rank_at_all_is_refused() {
    assert!(surface().only_nodes(&[7]).unwrap_err().contains("holds 7"));
    assert!(surface().only_nodes(&[]).is_err());
    // What is not a shape is left as it is.
    let ramp = Source::Ramp { levels: vec![0.0], colours: Vec::new() };
    assert!(matches!(ramp.only_nodes(&[0]).unwrap(), Source::Ramp { .. }));
}

/// The cloud: `pointset` over a selection writes the nodes chosen, in order.
#[test]
fn pointset_over_a_selection_is_the_extraction() {
    let bag = Pieces::empty().with("shape", surface().only_nodes(&[4, 1]).unwrap());
    let made = plan("pointset", &bag, Choices::new()).unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(made.shapes[0].geometry.patches.iter().flat_map(|p| p.points.iter().copied()).collect::<Vec<_>>(), vec![[1.0, 1.0, 1.0], [2.0, 0.0, 0.0]]);
    assert_eq!(made.objects.last().unwrap().qualified_type, "resqml22.PointSetRepresentation");
}

/// **One line through the nodes, in the order given**, the shapes one after the
/// other in the order of the slot.
#[test]
fn one_line_through_the_nodes_in_their_order() {
    let bag = Pieces::empty()
        .with("shape", surface().only_nodes(&[2, 0]).unwrap())
        .with("shape 2", surface().only_nodes(&[6]).unwrap());
    let made = plan("polyline-through", &bag, Choices::new()).unwrap_or_else(|why| panic!("{why}"));
    let patch = &made.shapes[0].geometry.patches[0];
    assert_eq!(patch.points, vec![[2.0, 2.0, 0.0], [0.0, 0.0, 0.0], [6.0, 0.0, 0.0]]);
    assert_eq!(patch.topology, Topology::Polylines(vec![3]));
    assert_eq!(patch.closed, vec![false], "open unless asked");
    assert_eq!(made.objects.last().unwrap().qualified_type, "resqml22.PolylineSetRepresentation");
    assert_eq!(made.activity.recipe, "polyline-through");
}

#[test]
fn closed_when_asked_and_a_single_node_is_refused() {
    let bag = Pieces::empty().with("shape", surface().only_nodes(&[0, 1, 2]).unwrap());
    let made = plan("polyline-through", &bag, Choices::new().set("closed", json!(true))).unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(made.shapes[0].geometry.patches[0].closed, vec![true]);

    let one = Pieces::empty().with("shape", surface().only_nodes(&[3]).unwrap());
    let refused = plan("polyline-through", &one, Choices::new()).expect_err("one node makes no line");
    assert!(format!("{refused}").contains("two at least"), "{refused}");
}
