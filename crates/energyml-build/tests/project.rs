//! `project` - nodes projected vertically onto a surface or a level
//!. The arithmetic is tested in
//! `energyml_analysis::project` and `heights`; this holds the seam.
//!
//! # What would have to break to make it turn red
//!
//! Moving x or y; removing, or silently projecting, a node without a surface
//! below it; projecting a node that is not chosen; accepting a surface and
//! a level together, or two frames; writing another kind of object than
//! the input; losing the topology.

use energyml_build::operation::{Choices, Identity, Plan, Purpose, Registry, When};
use energyml_build::pieces::{Known, Pieces, Source};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};
use serde_json::json;

fn when() -> When {
    When {
        creation: "2026-09-14T00:00:00Z".into(),
        originator: "test".into(),
    }
}

fn shape(points: Vec<[f64; 3]>, topology: Topology) -> Source {
    Source::Shape {
        geometry: Geometry {
            kind: "test",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points,
                topology,
                closed: Vec::new(),
            }],
        },
        closed: None,
    }
}

/// A square of side 10 in two triangles, its height `z = x + 100`.
fn tilted() -> Source {
    shape(
        vec![
            [0.0, 0.0, 100.0],
            [10.0, 0.0, 110.0],
            [10.0, 10.0, 110.0],
            [0.0, 10.0, 100.0],
        ],
        Topology::Triangles(vec![[0, 1, 2], [0, 2, 3]]),
    )
}

/// A polyline of three nodes, the last one past the surface's edge.
fn line() -> Source {
    shape(
        vec![[2.0, 5.0, 0.0], [6.0, 5.0, 0.0], [30.0, 5.0, 0.0]],
        Topology::Polylines(vec![3]),
    )
}

fn plan(bag: &Pieces, choices: Choices) -> Plan {
    Registry::new()
        .plan_for(
            "project",
            bag,
            &choices,
            &Identity::derived(),
            &when(),
            Purpose::Geometry,
        )
        .unwrap_or_else(|why| panic!("planned: {why}"))
}

fn reported(plan: &Plan, title: &str) -> Option<f64> {
    plan.activity.parameters.iter().find_map(|p| match p {
        Parameter::Double { title: t, value, .. } if t == title => Some(*value),
        _ => None,
    })
}

fn points(plan: &Plan) -> Vec<[f64; 3]> {
    plan.shapes[0].geometry.patches[0].points.clone()
}

/// **The z replaced, x and y kept; a node with nothing under it left, and
/// counted**.
#[test]
fn on_a_surface_the_z_is_replaced_and_a_node_without_surface_stays() {
    let made = plan(
        &Pieces::empty().with("shape", line()).with("surface", tilted()),
        Choices::new(),
    );
    assert_eq!(
        points(&made),
        vec![[2.0, 5.0, 102.0], [6.0, 5.0, 106.0], [30.0, 5.0, 0.0]]
    );
    assert_eq!(
        (reported(&made, "projectedNodes"), reported(&made, "outsideNodes")),
        (Some(2.0), Some(1.0))
    );
    assert!(
        made.says
            .iter()
            .any(|note| note.key == "outside" && note.detail.contains("left where they were")),
        "{:?}",
        made.says
    );
    assert_eq!(
        made.shapes[0].geometry.patches[0].topology,
        Topology::Polylines(vec![3]),
        "the same line"
    );
    assert_eq!(
        made.objects.last().unwrap().qualified_type,
        "resqml22.PolylineSetRepresentation",
        "written as what it was"
    );
}

#[test]
fn on_a_level_and_the_chosen_nodes_alone() {
    let made = plan(
        &Pieces::empty().with("shape", line()),
        Choices::new()
            .set("onto", "level")
            .set("z", -50.0)
            .set("nodes", json!([1])),
    );
    assert_eq!(
        points(&made),
        vec![[2.0, 5.0, 0.0], [6.0, 5.0, -50.0], [30.0, 5.0, 0.0]]
    );
    assert_eq!(made.activity.recipe, "project");
}

#[test]
fn refusals_name_what_is_missing_or_what_contradicts() {
    let refused = |bag: Pieces, choices: Choices| {
        Registry::new()
            .plan_for(
                "project",
                &bag,
                &choices,
                &Identity::derived(),
                &when(),
                Purpose::Geometry,
            )
            .expect_err("refused")
    };
    // A surface wanted, none given.
    let why = refused(Pieces::empty().with("shape", line()), Choices::new());
    assert_eq!(why.refusals()[0].slot.as_deref(), Some("surface"), "{why}");
    // A level and a surface together.
    let why = refused(
        Pieces::empty().with("shape", line()).with("surface", tilted()),
        Choices::new().set("onto", "level").set("z", 0.0),
    );
    assert!(why.message().contains("alternatives"), "{why}");
    // Two vertical units.
    let known = |uom: &str| {
        Source::Object(Known {
            vertical_uom: Some(uom.into()),
            ..Default::default()
        })
    };
    let bag = Pieces::empty()
        .with("shape", line())
        .with("shape (object)", known("m"))
        .with("surface", tilted())
        .with("surface (object)", known("ms"));
    let why = refused(bag, Choices::new());
    assert!(
        why.message().contains("`m`") && why.message().contains("`ms`"),
        "{why}"
    );
}

/// **A grid read as a lattice is projected onto bilinearly** - at the centre of
/// a cell whose one corner is at 4, the height is 1; cut in two triangles it
/// would be 2 along the diagonal. The front end draws the two triangles, which is
/// why its picking and this operation differ inside a grid's cells.
#[test]
fn on_a_lattice_the_cell_is_read_bilinearly() {
    let lattice = shape(
        vec![
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [0.0, 10.0, 0.0],
            [10.0, 10.0, 4.0],
        ],
        Topology::Grid {
            fastest: 2,
            slowest: 2,
        },
    );
    let points = shape(vec![[5.0, 5.0, 99.0]], Topology::Points);
    let made = plan(
        &Pieces::empty().with("shape", points).with("surface", lattice),
        Choices::new(),
    );
    assert!(
        (made.shapes[0].geometry.patches[0].points[0][2] - 1.0).abs() < 1e-9,
        "{:?}",
        made.shapes[0].geometry.patches[0].points
    );
}
