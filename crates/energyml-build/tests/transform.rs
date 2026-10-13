//! `transform` - a shape moved, turned and scaled
//!.
//!
//! # What would have to break to make it turn red
//!
//! Rotating after having translated (the object would rotate around a pivot it has
//! left); scaling after having rotated (the stretching would follow the rotated
//! axes); reading the angle in degrees when the unit says radians; computing the pivot
//! on the whole shape when only a few nodes move; moving a node that
//! is not chosen; letting through a null axis, a null or negative scale,
//! a node outside the shape, a rotation that mixes time and metres; copying the
//! topology wrongly; recording a pivot for a translation alone.

use energyml_build::operation::{Choices, Fitness, Identity, Plan, Purpose, Registry, When};
use energyml_build::pieces::{Known, Pieces, Source};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};
use serde_json::json;

fn when() -> When {
    When {
        creation: "2026-09-13T00:00:00Z".into(),
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

/// A unit square in the plane z = 5, as one open polyline.
fn square() -> Pieces {
    Pieces::empty().with(
        "shape",
        shape(
            vec![[0.0, 0.0, 5.0], [1.0, 0.0, 5.0], [1.0, 1.0, 5.0], [0.0, 1.0, 5.0]],
            Topology::Polylines(vec![4]),
        ),
    )
}

fn plan(bag: &Pieces, choices: Choices) -> Plan {
    Registry::new()
        .plan_for(
            "transform",
            bag,
            &choices,
            &Identity::derived(),
            &when(),
            Purpose::Geometry,
        )
        .unwrap_or_else(|why| panic!("planned: {why}"))
}

/// A recorded number, by its title.
fn double(plan: &Plan, title: &str) -> Option<f64> {
    plan.activity.parameters.iter().find_map(|p| match p {
        Parameter::Double { title: t, value, .. } if t == title => Some(*value),
        _ => None,
    })
}

fn points_of(plan: &Plan) -> Vec<[f64; 3]> {
    plan.shapes[0]
        .geometry
        .patches
        .iter()
        .flat_map(|p| p.points.iter().copied())
        .collect()
}

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|axis| (a[axis] - b[axis]).abs() < 1e-9)
}

fn assert_points(got: &[[f64; 3]], want: &[[f64; 3]]) {
    assert_eq!(got.len(), want.len());
    for (at, (g, w)) in got.iter().zip(want).enumerate() {
        assert!(close(*g, *w), "node {at}: {g:?} and not {w:?}");
    }
}

#[test]
fn a_translation_moves_every_node_and_keeps_the_topology() {
    let made = plan(
        &square(),
        Choices::new().set("dx", 10.0).set("dy", -2.0).set("dz", 0.5),
    );
    assert_points(
        &points_of(&made),
        &[
            [10.0, -2.0, 5.5],
            [11.0, -2.0, 5.5],
            [11.0, -1.0, 5.5],
            [10.0, -1.0, 5.5],
        ],
    );
    assert_eq!(
        made.shapes[0].geometry.patches[0].topology,
        Topology::Polylines(vec![4]),
        "the same runs"
    );
    assert_eq!(
        made.objects.last().unwrap().qualified_type,
        "resqml22.PolylineSetRepresentation",
        "written as what it was"
    );
    assert_eq!(made.activity.recipe, "transform");
    assert_eq!(
        (double(&made, "dx"), double(&made, "movedNodes")),
        (Some(10.0), Some(4.0)),
        "{:?}",
        made.activity.parameters
    );
}

/// **The rotation first, about the pivot, then the translation** - pinned on a
/// point whose two orders disagree.
#[test]
fn rotation_first_then_translation() {
    let bag = Pieces::empty().with("shape", shape(vec![[1.0, 0.0, 0.0]], Topology::Points));
    let turned = plan(
        &bag,
        Choices::new()
            .set("rotate", "axis")
            .set("angle", 90.0)
            .set("pivot", "origin")
            .set("dx", 10.0),
    );
    // Turned about the origin: (0, 1, 0); then moved: (10, 1, 0). The other
    // order would give (0, 11, 0).
    assert_points(&points_of(&turned), &[[10.0, 1.0, 0.0]]);
    assert_eq!(
        turned.objects.last().unwrap().qualified_type,
        "resqml22.PointSetRepresentation"
    );
}

#[test]
fn degrees_or_radians_and_the_pivot_at_the_reported_centre() {
    let quarter_deg = plan(&square(), Choices::new().set("rotate", "axis").set("angle", 90.0));
    let quarter_rad = plan(
        &square(),
        Choices::new()
            .set("rotate", "axis")
            .set("angle", std::f64::consts::FRAC_PI_2)
            .set("unit", "rad"),
    );
    // About the vertical through the square's centre (0.5, 0.5, 5), anticlockwise:
    // the square maps onto itself, each corner to the next one.
    let want = [[1.0, 0.0, 5.0], [1.0, 1.0, 5.0], [0.0, 1.0, 5.0], [0.0, 0.0, 5.0]];
    assert_points(&points_of(&quarter_deg), &want);
    assert_points(&points_of(&quarter_rad), &want);
    let reported = |title: &str| double(&quarter_deg, title);
    assert_eq!(
        (
            reported("pivotUsedX"),
            reported("pivotUsedY"),
            reported("pivotUsedZ")
        ),
        (Some(0.5), Some(0.5), Some(5.0))
    );
}

/// **A rigid move**: about any axis, by any angle, every distance between
/// moved nodes is kept.
#[test]
fn a_rigid_move_keeps_the_distances() {
    let points = vec![
        [0.0, 0.0, 0.0],
        [3.0, 1.0, -2.0],
        [-1.5, 4.0, 7.0],
        [2.0, 2.0, 2.0],
    ];
    let bag = Pieces::empty().with("shape", shape(points.clone(), Topology::Points));
    let made = plan(
        &bag,
        Choices::new()
            .set("rotate", "axis")
            .set("axisX", 1.0)
            .set("axisY", 2.0)
            .set("axisZ", 3.0)
            .set("angle", 37.0)
            .set("dx", 5.0)
            .set("dz", -1.0),
    );
    let after = points_of(&made);
    let d = |p: &[[f64; 3]], a: usize, b: usize| {
        ((p[a][0] - p[b][0]).powi(2) + (p[a][1] - p[b][1]).powi(2) + (p[a][2] - p[b][2]).powi(2)).sqrt()
    };
    for a in 0..points.len() {
        for b in a + 1..points.len() {
            assert!((d(&points, a, b) - d(&after, a, b)).abs() < 1e-9, "{a}-{b}");
        }
    }
    assert!(!close(after[1], points[1]), "and something did move");
}

#[test]
fn only_some_nodes_move_and_the_pivot_is_theirs() {
    // Nodes 1 and 2 only: a quarter turn about **their** centre (1, 0.5, 5) -
    // node 1 (1, 0, 5) goes to (1.5, 0.5, 5), node 2 (1, 1, 5) to (0.5, 0.5, 5).
    let made = plan(
        &square(),
        Choices::new()
            .set("nodes", json!([1, 2]))
            .set("rotate", "axis")
            .set("angle", 90.0),
    );
    assert_points(
        &points_of(&made),
        &[[0.0, 0.0, 5.0], [1.5, 0.5, 5.0], [0.5, 0.5, 5.0], [0.0, 1.0, 5.0]],
    );
    assert!(
        made.says
            .iter()
            .any(|note| note.key == "nodes" && note.detail.contains("2 of 4")),
        "{:?}",
        made.says
    );
}

/// **The scale first, then the rotation, both about the pivot, then the
/// translation** - pinned on a point where every other order disagrees.
#[test]
fn scale_first_then_rotation_then_translation() {
    let bag = Pieces::empty().with("shape", shape(vec![[1.0, 0.0, 0.0]], Topology::Points));
    let made = plan(
        &bag,
        Choices::new()
            .set("scale", "axes")
            .set("scaleX", 3.0)
            .set("rotate", "axis")
            .set("angle", 90.0)
            .set("pivot", "origin")
            .set("dx", 10.0),
    );
    // Scaled along x: (3, 0, 0); turned a quarter about z: (0, 3, 0); moved:
    // (10, 3, 0). Turned first then scaled along x would give (10, 1, 0).
    assert_points(&points_of(&made), &[[10.0, 3.0, 0.0]]);
    assert!(
        made.says.iter().any(|note| note.key == "scale"),
        "{:?}",
        made.says
    );
}

#[test]
fn a_uniform_scale_starts_from_the_centre_of_what_moves() {
    // Twice as large, about the square's centre (0.5, 0.5, 5): the corners go
    // to (-0.5, -0.5), (1.5, -0.5), … and z stays, the centre being at z = 5.
    let made = plan(
        &square(),
        Choices::new().set("scale", "uniform").set("factor", 2.0),
    );
    assert_points(
        &points_of(&made),
        &[
            [-0.5, -0.5, 5.0],
            [1.5, -0.5, 5.0],
            [1.5, 1.5, 5.0],
            [-0.5, 1.5, 5.0],
        ],
    );
    assert_eq!(double(&made, "pivotUsedX"), Some(0.5));
    assert_eq!(
        double(&made, "factor"),
        Some(2.0),
        "the factor recorded: {:?}",
        made.activity.parameters
    );
}

/// **A translation alone has no pivot** - not answered, not suggested, not
/// recorded (`Applies::Any`). Without the condition it would record
/// `pivot: centre`, the defect `When` was made for.
#[test]
fn a_translation_alone_has_no_pivot() {
    let made = plan(&square(), Choices::new().set("dx", 1.0));
    let titles: Vec<String> = made
        .activity
        .parameters
        .iter()
        .map(|p| match p {
            Parameter::Double { title, .. }
            | Parameter::Text { title, .. }
            | Parameter::Object { title, .. } => title.clone(),
        })
        .collect();
    assert!(!titles.iter().any(|t| t.starts_with("pivot")), "{titles:?}");
    // Given anyway, it is refused by name - it would do nothing.
    let why = Registry::new()
        .plan_for(
            "transform",
            &square(),
            &Choices::new().set("dx", 1.0).set("pivot", "origin"),
            &Identity::derived(),
            &when(),
            Purpose::Geometry,
        )
        .expect_err("a pivot with nothing about it");
    assert!(
        why.message()
            .contains("`rotate` is `axis`, or `scale` is `uniform` or `axes`"),
        "{why}"
    );
    // And with a scale alone, it is read.
    let scaled = plan(
        &square(),
        Choices::new()
            .set("scale", "uniform")
            .set("factor", 1.0)
            .set("pivot", "origin"),
    );
    assert_eq!(double(&scaled, "pivotUsedX"), Some(0.0));
}

#[test]
fn nothing_to_do_is_a_copy() {
    let made = plan(&square(), Choices::new());
    let original: Vec<[f64; 3]> = vec![[0.0, 0.0, 5.0], [1.0, 0.0, 5.0], [1.0, 1.0, 5.0], [0.0, 1.0, 5.0]];
    assert_points(&points_of(&made), &original);
}

#[test]
fn a_triangulated_surface_keeps_its_triangles() {
    let bag = Pieces::empty().with(
        "shape",
        shape(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            Topology::Triangles(vec![[0, 1, 2]]),
        ),
    );
    let made = plan(&bag, Choices::new().set("dz", 3.0));
    assert_eq!(
        made.shapes[0].geometry.patches[0].topology,
        Topology::Triangles(vec![[0, 1, 2]])
    );
    assert_eq!(
        made.objects.last().unwrap().qualified_type,
        "resqml22.TriangulatedSetRepresentation"
    );
    assert!(points_of(&made).iter().all(|p| p[2] == 3.0));
}

#[test]
fn refusals_name_their_field() {
    let refused = |bag: &Pieces, choices: Choices| {
        Registry::new()
            .plan_for(
                "transform",
                bag,
                &choices,
                &Identity::derived(),
                &when(),
                Purpose::Geometry,
            )
            .expect_err("refused")
    };
    let slot = |error: &energyml_build::operation::Error| error.refusals()[0].slot.clone();

    let why = refused(
        &square(),
        Choices::new()
            .set("rotate", "axis")
            .set("axisZ", 0.0)
            .set("angle", 10.0),
    );
    assert_eq!(slot(&why).as_deref(), Some("axisZ"), "{why}");
    assert!(why.message().contains("no length"), "{why}");

    let why = refused(
        &square(),
        Choices::new().set("scale", "uniform").set("factor", 0.0),
    );
    assert_eq!(slot(&why).as_deref(), Some("factor"), "{why}");
    assert!(
        why.message().contains("zero") && why.message().contains("projection"),
        "{why}"
    );
    let why = refused(&square(), Choices::new().set("scale", "axes").set("scaleZ", -1.0));
    assert_eq!(slot(&why).as_deref(), Some("scaleZ"), "{why}");
    assert!(why.message().contains("mirror"), "{why}");

    let why = refused(&square(), Choices::new().set("nodes", json!([0, 7])));
    assert_eq!(slot(&why).as_deref(), Some("nodes"), "{why}");
    assert!(
        why.message().contains("node 7") && why.message().contains("holds 4"),
        "{why}"
    );

    // A time vertical: turning about a tilted axis is refused, about the
    // vertical it is not.
    let in_time = Pieces::empty()
        .with(
            "shape",
            shape(vec![[0.0, 0.0, 1200.0], [10.0, 0.0, 1210.0]], Topology::Points),
        )
        .with(
            "shape (object)",
            Source::Object(Known {
                vertical_uom: Some("ms".into()),
                ..Default::default()
            }),
        );
    let why = refused(
        &in_time,
        Choices::new()
            .set("rotate", "axis")
            .set("axisX", 1.0)
            .set("axisZ", 0.0)
            .set("angle", 5.0),
    );
    assert!(why.message().contains("time"), "{why}");
    plan(&in_time, Choices::new().set("rotate", "axis").set("angle", 5.0));
}

#[test]
fn a_grid_or_a_mixture_is_unfit_saying_so() {
    let registry = Registry::new();
    let op = registry.get("transform").unwrap();
    let grid = Pieces::empty().with(
        "shape",
        shape(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
            Topology::Quads(vec![[0, 1, 2, 3]]),
        ),
    );
    match op.fitness(&grid) {
        Fitness::Unfit(why) => assert!(
            why.message.contains("grid") && why.slot.as_deref() == Some("shape"),
            "{why:?}"
        ),
        Fitness::Fit => panic!("a grid is not moved yet"),
    }
    let mixed = Pieces::empty().with(
        "shape",
        Source::Shape {
            geometry: Geometry {
                kind: "test",
                patches: vec![
                    Patch {
                        index: 0,
                        node_offset: 0,
                        points: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                        topology: Topology::Triangles(vec![[0, 1, 2]]),
                        closed: Vec::new(),
                    },
                    Patch {
                        index: 1,
                        node_offset: 3,
                        points: vec![[0.0; 3], [1.0, 1.0, 0.0]],
                        topology: Topology::Polylines(vec![2]),
                        closed: Vec::new(),
                    },
                ],
            },
            closed: None,
        },
    );
    assert!(
        matches!(op.fitness(&mixed), Fitness::Unfit(ref why) if why.message.contains("mix")),
        "one representation holds one kind"
    );
}

/// **A move that turns a triangle over is counted, never refused** - a folded
/// surface is a legitimate result.
#[test]
fn a_flipped_triangle_is_counted() {
    let bag = Pieces::empty().with(
        "shape",
        shape(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            Topology::Triangles(vec![[0, 1, 2]]),
        ),
    );
    // Node 2 carried across the opposite edge, in plan.
    let made = plan(&bag, Choices::new().set("nodes", json!([2])).set("dy", -2.0));
    assert_eq!(double(&made, "flippedTriangles"), Some(1.0));
    assert!(
        made.says.iter().any(|note| note.key == "flipped"),
        "{:?}",
        made.says
    );
    let still = plan(&bag, Choices::new().set("dz", 3.0));
    assert_eq!(double(&still, "flippedTriangles"), Some(0.0));
}
