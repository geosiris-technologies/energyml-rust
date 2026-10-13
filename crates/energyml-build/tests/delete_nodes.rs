//! `delete-nodes` — some nodes of a shape taken out
//!. The cutting is tested in
//! `energyml_analysis::nodes`; this holds the seam.
//!
//! # What would have to break to make it turn red
//!
//! Removing a node from another patch than its own; losing the closure of an
//! intact ring; writing an empty patch; letting through a rank outside the shape
//! or a deletion that leaves nothing; writing another kind of object.

use energyml_build::operation::{Choices, Identity, Plan, Purpose, Registry, When};
use energyml_build::pieces::{Pieces, Source};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};
use serde_json::json;

fn when() -> When {
    When {
        creation: "2026-09-14T00:00:00Z".into(),
        originator: "test".into(),
    }
}

fn bag(patches: Vec<Patch>, closed: Option<Vec<bool>>) -> Pieces {
    Pieces::empty().with(
        "shape",
        Source::Shape {
            geometry: Geometry {
                kind: "test",
                patches,
            },
            closed,
        },
    )
}

fn plan(bag: &Pieces, nodes: serde_json::Value) -> Result<Plan, energyml_build::operation::Error> {
    Registry::new().plan_for(
        "delete-nodes",
        bag,
        &Choices::new().set("nodes", nodes),
        &Identity::derived(),
        &when(),
        Purpose::Geometry,
    )
}

fn reported(plan: &Plan, title: &str) -> Option<f64> {
    plan.activity.parameters.iter().find_map(|p| match p {
        Parameter::Double { title: t, value, .. } if t == title => Some(*value),
        _ => None,
    })
}

/// A fan of four triangles round a centre (node 4), as one patch.
fn fan() -> Patch {
    Patch {
        index: 0,
        node_offset: 0,
        points: vec![
            [0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [2.0, 2.0, 0.0],
            [0.0, 2.0, 0.0],
            [1.0, 1.0, 1.0],
        ],
        topology: Topology::Triangles(vec![[0, 1, 4], [1, 2, 4], [2, 3, 4], [3, 0, 4]]),
        closed: Vec::new(),
    }
}

#[test]
fn a_surface_loses_the_triangles_that_name_the_node() {
    let made = plan(&bag(vec![fan()], None), json!([0])).unwrap_or_else(|why| panic!("{why}"));
    let patch = &made.shapes[0].geometry.patches[0];
    assert_eq!(patch.points.len(), 4);
    assert_eq!(
        patch.topology,
        Topology::Triangles(vec![[0, 1, 3], [1, 2, 3]]),
        "renumbered onto the nodes that stay"
    );
    assert_eq!(
        (
            reported(&made, "deletedNodes"),
            reported(&made, "removedTriangles")
        ),
        (Some(1.0), Some(2.0))
    );
    assert_eq!(
        made.objects.last().unwrap().qualified_type,
        "resqml22.TriangulatedSetRepresentation"
    );
    assert!(
        made.says
            .iter()
            .any(|note| note.key == "deleted" && note.detail.contains("ranks")),
        "{:?}",
        made.says
    );
}

/// **Ranks are the shape's, across its patches**: node 4 is the first of the
/// second patch, and only that patch loses it.
#[test]
fn ranks_cross_patches_and_an_intact_ring_stays_closed() {
    let ring = Patch {
        index: 0,
        node_offset: 0,
        points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
        topology: Topology::Polylines(vec![4]),
        closed: vec![true],
    };
    let line = Patch {
        index: 1,
        node_offset: 4,
        points: vec![[5.0, 0.0, 0.0], [6.0, 0.0, 0.0], [7.0, 0.0, 0.0]],
        topology: Topology::Polylines(vec![3]),
        closed: vec![false],
    };
    let made = plan(&bag(vec![ring, line], None), json!([4])).unwrap_or_else(|why| panic!("{why}"));
    let patches = &made.shapes[0].geometry.patches;
    // The first patch untouched, the ring still closed.
    assert_eq!(patches[0].topology, Topology::Polylines(vec![4]));
    assert_eq!(patches[0].closed, vec![true]);
    // The second: its first node gone, a line of two left.
    assert_eq!(patches[1].points, vec![[6.0, 0.0, 0.0], [7.0, 0.0, 0.0]]);
    assert_eq!(patches[1].node_offset, 4, "the running sum");
}

#[test]
fn refusals_name_their_field() {
    let why = plan(&bag(vec![fan()], None), json!([9])).expect_err("out of the shape");
    assert_eq!(why.refusals()[0].slot.as_deref(), Some("nodes"), "{why}");
    assert!(
        why.message().contains("node 9") && why.message().contains("holds 5"),
        "{why}"
    );
    // Everything taken out: nothing left to write.
    let why = plan(&bag(vec![fan()], None), json!([4])).expect_err("nothing left");
    assert!(why.message().contains("leaves nothing"), "{why}");
    // No node named at all.
    let why = plan(&bag(vec![fan()], None), json!([])).expect_err("none named");
    assert_eq!(why.refusals()[0].slot.as_deref(), Some("nodes"), "{why}");
}
