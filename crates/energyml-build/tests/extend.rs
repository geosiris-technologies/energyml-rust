//! `extend` — a polyline
//! prolonged at its ends, a triangulated surface grown past its border. The
//! arithmetic is tested in `energyml_analysis::extend`; this holds the seam,
//! and confronts what comes out with the extrapolation **recomputed here**
//! from the analytic shape, not read back from the module.
//!
//! # What would have to break to make it turn red
//!
//! Touching an original node (shifting it, reordering it); reading the direction
//! on the last edge instead of the band; reading *up* the wrong way
//! or taking the height for a length; losing the child that marks
//! the invented; accepting a cloud or a ring.

use energyml_build::operation::{Choices, Fitness, Identity, Plan, Purpose, Registry, When};
use energyml_build::pieces::{Pieces, Source};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When {
        creation: "2026-09-25T00:00:00Z".into(),
        originator: "test".into(),
    }
}

fn shape(points: Vec<[f64; 3]>, topology: Topology, closed: Option<Vec<bool>>) -> Pieces {
    Pieces::empty().with(
        "shape",
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
            closed,
        },
    )
}

/// A `side × side` grid on the plane `z = y` — a fault dipping 45° to the
/// south, its up-dip direction `(0, 1, 1)/√2` — two triangles per cell.
fn fault(side: usize) -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
    let mut points = Vec::new();
    for j in 0..side {
        for i in 0..side {
            points.push([i as f64, j as f64, j as f64]);
        }
    }
    let mut triangles = Vec::new();
    for j in 0..side - 1 {
        for i in 0..side - 1 {
            let a = (j * side + i) as u32;
            let (b, c, d) = (a + 1, a + side as u32 + 1, a + side as u32);
            triangles.push([a, b, c]);
            triangles.push([a, c, d]);
        }
    }
    (points, triangles)
}

fn plan(bag: &Pieces, choices: Choices) -> Plan {
    Registry::new()
        .plan_for(
            "extend",
            bag,
            &choices,
            &Identity::derived(),
            &when(),
            Purpose::Geometry,
        )
        .unwrap_or_else(|why| panic!("planned: {why}"))
}

fn count(plan: &Plan, name: &str) -> Option<f64> {
    plan.activity.parameters.iter().find_map(|p| match p {
        Parameter::Double { title, value, .. } if title == name => Some(*value),
        _ => None,
    })
}

fn bits(points: &[[f64; 3]]) -> Vec<[u64; 3]> {
    points.iter().map(|p| p.map(f64::to_bits)).collect()
}

/// The example case, on a planar fault: ten metres up **as a height**, five
/// down, eight on the sides. The original nodes and triangles come back bit for
/// bit and first; every invented node is on the fault's plane; the middle of the
/// top edge rises by exactly ten metres, the bottom sinks by five, a side goes
/// eight metres along the strike — each recomputed from the plane, not read
/// back from the module.
#[test]
fn a_fault_extends_10_m_up_5_down_8_on_the_sides() {
    let side = 7;
    let (points, triangles) = fault(side);
    let bag = shape(points.clone(), Topology::Triangles(triangles.clone()), None);
    let made = plan(
        &bag,
        Choices::new()
            .set("band", 2.0)
            .set("toward", "dip")
            .set("up", 10.0)
            .set("down", 5.0)
            .set("sides", 8.0)
            .set("measure", "height")
            .set("axis", "elevation"),
    );
    let root = &made.shapes[0];
    let patch = &root.geometry.patches[0];
    let Topology::Triangles(out) = &patch.topology else {
        panic!("a surface comes out: {:?}", patch.topology)
    };
    let n = points.len();
    assert_eq!(
        bits(&patch.points[..n]),
        bits(&points),
        "the original nodes, bit for bit and first"
    );
    assert_eq!(
        &out[..triangles.len()],
        triangles.as_slice(),
        "the original triangles, as they were and first"
    );
    assert!(
        patch.points.len() > n && out.len() > triangles.len(),
        "something was added"
    );
    for p in &patch.points[n..] {
        assert!(
            (p[2] - p[1]).abs() < 1e-9,
            "an invented node off the fault's plane z = y: {p:?}"
        );
    }
    // The tip of a border node's extension: the farthest invented node on the
    // ray from it — found here by geometry, the ray recomputed from the plane.
    let tip = |node: usize, direction: [f64; 3]| -> f64 {
        let p = points[node];
        patch.points[n..]
            .iter()
            .filter_map(|q| {
                let d = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
                let along = d[0] * direction[0] + d[1] * direction[1] + d[2] * direction[2];
                let off = [
                    d[0] - along * direction[0],
                    d[1] - along * direction[1],
                    d[2] - along * direction[2],
                ];
                ((off[0] * off[0] + off[1] * off[1] + off[2] * off[2]).sqrt() < 1e-9 && along > 0.0)
                    .then_some(along)
            })
            .fold(0.0, f64::max)
    };
    let up = [0.0, 0.5f64.sqrt(), 0.5f64.sqrt()];
    let down = [0.0, -up[1], -up[2]];
    let top_middle = (side - 1) * side + side / 2;
    let bottom_middle = side / 2;
    let left_middle = (side / 2) * side;
    // Ten metres of height along a 45° line is 10·√2 of length.
    assert!(
        (tip(top_middle, up) * up[2] - 10.0).abs() < 1e-9,
        "the top rises by 10 m: {}",
        tip(top_middle, up)
    );
    assert!(
        (tip(bottom_middle, down) * up[2] - 5.0).abs() < 1e-9,
        "the bottom sinks by 5 m: {}",
        tip(bottom_middle, down)
    );
    assert!(
        (tip(left_middle, [-1.0, 0.0, 0.0]) - 8.0).abs() < 1e-9,
        "the side goes 8 m along the strike: {}",
        tip(left_middle, [-1.0, 0.0, 0.0])
    );

    // Marked, not mixed: the counts, the note, and the child of the tree.
    let added = patch.points.len() - n;
    assert_eq!(count(&made, "originalNodes"), Some(n as f64));
    assert_eq!(count(&made, "addedNodes"), Some(added as f64));
    assert!(
        made.says
            .iter()
            .any(|s| s.key == "invented" && s.detail.contains(&format!("nodes 0..{n}"))),
        "{:?}",
        made.says
    );
    let child = made
        .shapes
        .iter()
        .find(|s| s.parent.as_deref() == Some(root.id.as_str()))
        .expect("the invented nodes, a child of the result");
    assert_eq!(child.geometry.patches[0].topology, Topology::Points);
    assert_eq!(
        bits(&child.geometry.patches[0].points),
        bits(&patch.points[n..]),
        "the child holds exactly the invented nodes"
    );
    assert!(
        child.object.is_none(),
        "the child is a part of the result, not an object"
    );
    assert_eq!(
        made.objects.last().unwrap().qualified_type,
        "resqml22.TriangulatedSetRepresentation"
    );
    assert_eq!(made.activity.recipe, "extend");
}

/// **The band is what keeps a misplaced node from steering** (a test
/// written in advance): a border node lifted off the plane. Its extension
/// starts from it — it is not moved — and runs parallel to the plane read over
/// the band; the direction the last edge gives would climb out of it.
#[test]
fn a_badly_placed_edge_node_does_not_twist_the_extension() {
    let side = 7;
    let (mut points, triangles) = fault(side);
    let lifted = (side - 1) * side + side / 2;
    points[lifted][2] += 0.6;
    let bag = shape(points.clone(), Topology::Triangles(triangles), None);
    let made = plan(&bag, Choices::new().set("band", 2.5).set("distance", 3.0));
    let patch = &made.shapes[0].geometry.patches[0];
    let n = points.len();
    assert_eq!(
        bits(&patch.points[..n]),
        bits(&points),
        "the misplaced node is kept where it was, bit for bit"
    );
    // The plane's unit normal, and the component of a direction along it.
    let normal = [0.0, -(0.5f64.sqrt()), 0.5f64.sqrt()];
    let off_plane = |d: [f64; 3]| {
        let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        (d[0] * normal[0] + d[1] * normal[1] + d[2] * normal[2]).abs() / length
    };
    let p = points[lifted];
    // Its farthest invented node: the one along its own ray, the longest.
    let tip = patch.points[n..]
        .iter()
        .copied()
        .filter(|q| (q[0] - p[0]).abs() < 1e-9)
        .max_by(|a, b| a[1].total_cmp(&b[1]))
        .expect("the lifted node moved");
    let robust = off_plane([tip[0] - p[0], tip[1] - p[1], tip[2] - p[2]]);
    let inner = points[lifted - side];
    let last_edge = off_plane([p[0] - inner[0], p[1] - inner[1], p[2] - inner[2]]);
    assert!(robust < 1e-9, "the band's plane is the fault's: {robust}");
    assert!(
        last_edge > 0.2,
        "and the last edge would have climbed by: {last_edge}"
    );
    assert!(
        count(&made, "outlierNodes").unwrap() >= 1.0,
        "the misplaced node is counted"
    );
    assert!(made.says.iter().any(|s| s.key == "outliers"), "{:?}", made.says);
}

/// A straight polyline goes on straight: each added node on the line, the
/// last exactly at the distance asked from its end, the originals in the middle.
#[test]
fn a_straight_polyline_extends_straight_at_each_end() {
    let line: Vec<[f64; 3]> = (0..6)
        .map(|k| [100.0 + 3.0 * k as f64, 50.0 - 4.0 * k as f64, -10.0])
        .collect();
    let bag = shape(line.clone(), Topology::Polylines(vec![6]), None);
    let made = plan(
        &bag,
        Choices::new()
            .set("band", 20.0)
            .set("toward", "ends")
            .set("start", 12.0)
            .set("end", 7.5),
    );
    let patch = &made.shapes[0].geometry.patches[0];
    let Topology::Polylines(runs) = &patch.topology else {
        panic!()
    };
    assert_eq!(runs.len(), 1, "one line still");
    let direction = [0.6, -0.8, 0.0];
    let at = |p: [f64; 3], from: [f64; 3]| {
        let d = [p[0] - from[0], p[1] - from[1], p[2] - from[2]];
        let along = d[0] * direction[0] + d[1] * direction[1] + d[2] * direction[2];
        let off = (d[0] - along * direction[0])
            .hypot(d[1] - along * direction[1])
            .hypot(d[2]);
        (along, off)
    };
    let before = patch
        .points
        .iter()
        .position(|p| p.map(f64::to_bits) == line[0].map(f64::to_bits))
        .expect("the first node is there");
    assert_eq!(
        bits(&patch.points[before..before + 6]),
        bits(&line),
        "the originals in the middle, bit for bit"
    );
    for p in patch.points[..before].iter().chain(&patch.points[before + 6..]) {
        assert!(at(*p, line[0]).1 < 1e-9, "an added node off the line: {p:?}");
    }
    let (first, _) = at(patch.points[0], line[0]);
    let (last, _) = at(*patch.points.last().unwrap(), line[5]);
    assert!(
        (first + 12.0).abs() < 1e-9 && (last - 7.5).abs() < 1e-9,
        "{first} before, {last} after"
    );
    assert!(made.says.iter().any(|s| s.key == "invented"), "{:?}", made.says);
    assert_eq!(
        made.objects.last().unwrap().qualified_type,
        "resqml22.PolylineSetRepresentation"
    );
}

/// A fault stick in the dip frame: its top goes up, its bottom down — read in
/// the frame's sense, not the shape's. In depth, up is toward smaller z.
#[test]
fn a_fault_stick_goes_up_and_down_in_the_sense_of_the_frame() {
    let stick: Vec<[f64; 3]> = (0..5).map(|k| [0.0, 0.0, 1000.0 + 10.0 * k as f64]).collect();
    let bag = shape(stick.clone(), Topology::Polylines(vec![5]), None);
    let run = |axis: &str| {
        let made = plan(
            &bag,
            Choices::new()
                .set("band", 30.0)
                .set("toward", "dip")
                .set("up", 20.0)
                .set("down", 5.0)
                .set("axis", axis),
        );
        let points = made.shapes[0].geometry.patches[0].points.clone();
        (points[0][2], points.last().unwrap()[2])
    };
    // Depth: z grows downward, so the first node (z = 1000) is the top.
    assert_eq!(run("depth"), (980.0, 1045.0));
    // Elevation: the last node (z = 1040) is the top.
    assert_eq!(run("elevation"), (995.0, 1060.0));
}

#[test]
fn a_cloud_and_a_ring_are_refused_saying_so() {
    let registry = Registry::new();
    let op = registry.get("extend").unwrap();
    let square = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]];
    let unfit = |bag: &Pieces| match op.fitness(bag) {
        Fitness::Unfit(why) => why.to_string(),
        other => panic!("fit: {other:?}"),
    };
    let cloud = unfit(&shape(square.clone(), Topology::Points, None));
    assert!(
        cloud.contains("triangulate"),
        "the refusal says where to go: {cloud}"
    );
    let ring = unfit(&shape(
        square.clone(),
        Topology::Polylines(vec![4]),
        Some(vec![true]),
    ));
    assert!(ring.contains("closed"), "{ring}");
    assert_eq!(
        op.fitness(&shape(square, Topology::Polylines(vec![4]), None)),
        Fitness::Fit,
        "the same line, open, is fit"
    );
}

#[test]
fn refusals_name_their_field() {
    let refused = |bag: &Pieces, choices: Choices| {
        Registry::new()
            .plan_for(
                "extend",
                bag,
                &choices,
                &Identity::derived(),
                &when(),
                Purpose::Geometry,
            )
            .expect_err("refused")
    };
    // A flat surface has no up: the refusal names the dip it measured.
    let (mut points, triangles) = fault(4);
    for p in &mut points {
        p[2] = 0.0;
    }
    let flat = shape(points, Topology::Triangles(triangles.clone()), None);
    let why = refused(
        &flat,
        Choices::new()
            .set("band", 1.0)
            .set("toward", "dip")
            .set("up", 1.0)
            .set("axis", "elevation"),
    );
    assert_eq!(why.refusals()[0].slot.as_deref(), Some("toward"), "{why}");
    assert!(why.to_string().contains("dips 0.000°"), "{why}");
    // A surface has no ends.
    let (points, _) = fault(4);
    let tilted = shape(points, Topology::Triangles(triangles), None);
    let why = refused(
        &tilted,
        Choices::new()
            .set("band", 1.0)
            .set("toward", "ends")
            .set("end", 1.0),
    );
    assert_eq!(why.refusals()[0].slot.as_deref(), Some("toward"), "{why}");
    // Nothing to extend.
    let why = refused(&tilted, Choices::new().set("band", 1.0).set("distance", 0.0));
    assert_eq!(why.refusals()[0].slot.as_deref(), Some("distance"), "{why}");
}
