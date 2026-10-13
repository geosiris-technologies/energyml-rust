//! `densify` — polylines with every vertex kept and points added between them
//!. The arithmetic is tested in
//! `energyml_analysis::densify`; this holds the seam.
//!
//! # What would have to break to make it turn red
//!
//! Losing a vertex; adding on the closing edge of a line that nobody
//! said was closed; losing the closure of a ring at writing;
//! counting the polylines of a patch in the wrong order; accepting a surface.

use energyml_build::operation::{Choices, Fitness, Identity, Plan, Purpose, Registry, When};
use energyml_build::pieces::{Pieces, Source};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When { creation: "2026-09-14T00:00:00Z".into(), originator: "test".into() }
}

fn lines(points: Vec<[f64; 3]>, runs: Vec<u32>, closed: Option<Vec<bool>>) -> Pieces {
    Pieces::empty().with(
        "shape",
        Source::Shape {
            geometry: Geometry { kind: "test", patches: vec![Patch { index: 0, node_offset: 0, points, topology: Topology::Polylines(runs), closed: Vec::new() }] },
            closed,
        },
    )
}

fn plan(bag: &Pieces, choices: Choices) -> Plan {
    Registry::new()
        .plan_for("densify", bag, &choices, &Identity::derived(), &when(), Purpose::Geometry)
        .unwrap_or_else(|why| panic!("planned: {why}"))
}

fn added(plan: &Plan) -> Option<f64> {
    plan.activity.parameters.iter().find_map(|p| match p {
        Parameter::Double { title, value, .. } if title == "addedNodes" => Some(*value),
        _ => None,
    })
}

#[test]
fn every_vertex_stays_and_the_points_are_added_polyline_by_polyline() {
    // Two lines in one patch: 0→10 on x, then 0→4 on y.
    let bag = lines(vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 4.0, 1.0]], vec![2, 2], None);
    let made = plan(&bag, Choices::new().set("count", 1_i64));
    let patch = &made.shapes[0].geometry.patches[0];
    assert_eq!(patch.topology, Topology::Polylines(vec![3, 3]), "one point on each line, each line its own");
    assert_eq!(patch.points[1], [5.0, 0.0, 0.0]);
    assert_eq!(patch.points[4], [0.0, 2.0, 1.0]);
    assert_eq!(added(&made), Some(2.0));
    assert_eq!(made.objects.last().unwrap().qualified_type, "resqml22.PolylineSetRepresentation");
    assert_eq!(made.activity.recipe, "densify");
}

#[test]
fn stations_from_the_start_with_a_tolerance() {
    let bag = lines(vec![[0.0, 0.0, 0.0], [4.8, 0.0, 0.0], [12.0, 0.0, 0.0]], vec![3], None);
    let made = plan(&bag, Choices::new().set("method", "spacing").set("every", 5.0).set("tolerance", 0.25));
    let xs: Vec<f64> = made.shapes[0].geometry.patches[0].points.iter().map(|p| p[0]).collect();
    assert_eq!(xs, vec![0.0, 4.8, 10.0, 12.0]);
}

/// **A closure nobody stated is open**, and a stated one is kept: points on the
/// closing edge only for a ring, and the ring still closed once written.
#[test]
fn a_ring_densifies_over_its_closure_and_stays_closed() {
    let square = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]];
    let ring = plan(&lines(square.clone(), vec![4], Some(vec![true])), Choices::new().set("count", 4_i64));
    let open = plan(&lines(square, vec![4], None), Choices::new().set("count", 4_i64));
    let last = |plan: &Plan| *plan.shapes[0].geometry.patches[0].points.last().unwrap();
    assert!((last(&ring)[1] - 0.8).abs() < 1e-12 && last(&ring)[0] == 0.0, "a point on the closing edge: {:?}", last(&ring));
    assert_eq!(last(&open), [0.0, 1.0, 0.0], "an open line ends on its last vertex");
    assert_eq!(ring.shapes[0].geometry.patches[0].closed, vec![true], "still closed once written");
}

#[test]
fn a_surface_is_unfit_saying_so() {
    let registry = Registry::new();
    let op = registry.get("densify").unwrap();
    let surface = Pieces::empty().with(
        "shape",
        Source::Shape {
            geometry: Geometry {
                kind: "test",
                patches: vec![Patch { index: 0, node_offset: 0, points: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], topology: Topology::Triangles(vec![[0, 1, 2]]), closed: Vec::new() }],
            },
            closed: None,
        },
    );
    assert!(matches!(op.fitness(&surface), Fitness::Unfit(_)), "a surface's nodes are remeshed, not densified");
}

#[test]
fn refusals_name_their_field() {
    let bag = lines(vec![[0.0; 3], [1.0, 0.0, 0.0]], vec![2], None);
    let refused = |choices: Choices| {
        Registry::new().plan_for("densify", &bag, &choices, &Identity::derived(), &when(), Purpose::Geometry).expect_err("refused")
    };
    let why = refused(Choices::new().set("method", "spacing").set("every", 0.0));
    assert_eq!(why.refusals()[0].slot.as_deref(), Some("every"), "{why}");
    let why = refused(Choices::new().set("count", 0_i64));
    assert_eq!(why.refusals()[0].slot.as_deref(), Some("count"), "{why}");
}
