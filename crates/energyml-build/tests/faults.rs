//! Faults in the two operations that take them - a barrier for
//! the interpolation, a constraint for the meshing.
//!
//! # What this file holds
//!
//! The need: the BRGM uses kriging with possible
//! fault lines and other meshing parameters. The computation
//! is held by the unit tests of `energyml-analysis`; this file holds the
//! **seam** - that a trace filed under `faults` does reach the computation, by the
//! name of its slot, and that what comes out of it is said.
//!
//! # What would have to break to make it turn red
//!
//! Reading the faults as more samples; counting them in the cloud
//! to triangulate; letting `rbf` ignore the fault in silence; not saying the
//! nodes that a constraint adds to the surface.

use energyml_build::operation::{AnyOperation, Choices, Fitness, Identity, Registry, When};
use energyml_build::pieces::{Pieces, Source};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When {
        creation: "2026-09-12T00:00:00Z".to_string(),
        originator: "tests".to_string(),
    }
}

fn operation(name: &'static str) -> &'static dyn AnyOperation {
    Box::leak(Box::new(Registry::new()))
        .get(name)
        .unwrap_or_else(|| panic!("`{name}` is in the registry"))
}

/// A cloud: four corners at z = 1, and a point to the east at z = 100.
fn cloud() -> Source {
    Source::Shape {
        geometry: Geometry {
            kind: "cloud",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points: vec![
                    [0.0, 0.0, 1.0],
                    [4.0, 0.0, 1.0],
                    [4.0, 10.0, 1.0],
                    [0.0, 10.0, 1.0],
                    [6.0, 5.0, 100.0],
                ],
                topology: Topology::Points,
                closed: Vec::new(),
            }],
        },
        closed: None,
    }
}

/// A vertical fault at x = 5, between the cloud and the point at 100.
fn wall() -> Source {
    Source::Shape {
        geometry: Geometry {
            kind: "fault",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points: vec![[5.0, -1.0, 0.0], [5.0, 11.0, 0.0]],
                topology: Topology::Polylines(vec![2]),
                closed: Vec::new(),
            }],
        },
        closed: None,
    }
}

fn frame(extra: &[(&str, serde_json::Value)]) -> Choices {
    let mut choices = Choices::new()
        .set("sourceEpsg", 2154)
        .set("project", 2154)
        .set("title", "Essai")
        .set("dataFile", "data.h5");
    for (name, value) in extra {
        choices = choices.set(name, value.clone());
    }
    choices
}

/// The heights of the lattice of an interpolation plan.
fn heights(plan: &energyml_build::operation::Plan) -> Vec<f64> {
    let made = plan.shapes.first().expect("a shape");
    made.geometry
        .patches
        .iter()
        .flat_map(|p| p.points.iter().map(|q| q[2]))
        .collect()
}

// -- the interpolation -------------------------------------------------------

/// **A fault is a barrier**, and it arrives through its slot.
///
/// Without it, the point at 100 pulls the western nodes upward; with it,
/// the western nodes no longer hear it and stay at 1.
#[test]
fn a_fault_filed_under_faults_cuts_the_interpolation() {
    let op = operation("interpolate");
    let bare = Pieces::empty().with("shape", cloud());
    let cut = bare.clone().with("faults", wall());
    // A stated model: five samples do not make enough classes to
    // fit one, and it is not the fit that is being tested here.
    let settings = |method: &str| {
        frame(&[
            ("method", method.into()),
            ("spacing", 1.0.into()),
            ("model", "spherical".into()),
            ("nugget", 0.0.into()),
            ("sill", 1.0.into()),
            ("range", 10.0.into()),
        ])
    };

    for method in ["idw", "kriging"] {
        let open = op
            .plan(&bare, &settings(method), &Identity::derived(), &when())
            .expect("without a fault");
        let closed = op
            .plan(&cut, &settings(method), &Identity::derived(), &when())
            .expect("with a fault");
        let z_open = heights(&open);
        let z_cut = heights(&closed);
        assert_eq!(z_open.len(), z_cut.len());
        // Node (2, 5) is west of the fault: without it, it goes up,
        // with it, it stays at the level of the four corners.
        // ni = 7 (x from 0 to 6 in steps of 1), and row j = 5: node (2, 5).
        let at = |z: &[f64]| z[5 * 7 + 2];
        assert!(
            at(&z_open) > 1.5,
            "{method}: without a fault the 100 pulls upward - {}",
            at(&z_open)
        );
        assert!(
            (at(&z_cut) - 1.0).abs() < 1e-6,
            "{method}: with a fault the 100 is invisible - {}",
            at(&z_cut)
        );
        // And the number of fault segments is in the activity.
        assert!(closed.activity.parameters.iter().any(|p| matches!(
            p,
            energyml_domain::provenance::Parameter::Double { title, value, .. } if title == "faultSegments" && *value == 1.0
        )));
    }
}

/// **`rbf` cannot honour a barrier**, and says so rather than
/// ignoring it - a map that looks faulted and is not would be worse.
#[test]
fn rbf_with_a_fault_is_refused_by_name() {
    let op = operation("interpolate");
    let cut = Pieces::empty().with("shape", cloud()).with("faults", wall());
    let why = op
        .plan(
            &cut,
            &frame(&[("method", "rbf".into()), ("spacing", 1.0.into())]),
            &Identity::derived(),
            &when(),
        )
        .expect_err("refused");
    assert!(
        why.message().contains("rbf") && why.message().contains("faults"),
        "{}",
        why.message()
    );
}

/// Kriging gained the neighbourhood - radius, ceiling - and says so.
#[test]
fn kriging_takes_a_radius_and_a_ceiling() {
    let op = operation("interpolate");
    let bare = Pieces::empty().with("shape", cloud());
    let plan = op
        .plan(
            &bare,
            &frame(&[
                ("method", "kriging".into()),
                ("spacing", 1.0.into()),
                ("model", "spherical".into()),
                ("nugget", 0.0.into()),
                ("sill", 1.0.into()),
                ("range", 10.0.into()),
                ("radius", 3.0.into()),
                ("neighbours", 3.into()),
            ]),
            &Identity::derived(),
            &when(),
        )
        .expect("the plan succeeds");
    let named = |title: &str| {
        plan.activity.parameters.iter().any(
            |p| matches!(p, energyml_domain::provenance::Parameter::Double { title: t, .. } if t == title),
        )
    };
    assert!(named("radius") && named("neighbours"));
    // A radius of 3 leaves nodes without a sample: holes, counted.
    assert!(
        heights(&plan).iter().any(|z| z.is_nan()),
        "a short radius leaves holes"
    );
}

/// The fault is **not** one more sample: `fitness` and the count
/// of samples do not see it.
#[test]
fn a_fault_is_not_counted_among_the_samples() {
    let op = operation("interpolate");
    // Only two points: unfit - and the fault does not make a third.
    let two = Source::Shape {
        geometry: Geometry {
            kind: "cloud",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points: vec![[0.0, 0.0, 1.0], [1.0, 0.0, 1.0]],
                topology: Topology::Points,
                closed: Vec::new(),
            }],
        },
        closed: None,
    };
    let pieces = Pieces::empty().with("shape", two).with("faults", wall());
    assert!(matches!(op.fitness(&pieces), Fitness::Unfit(_)));
}

// -- the triangulation ------------------------------------------------------

/// **A fault is a constraint**: no triangle straddles it, and the
/// nodes it adds are said.
#[test]
fn a_fault_constrains_the_triangulation_and_its_nodes_are_said() {
    let op = operation("triangulate");
    let bare = Pieces::empty().with("shape", cloud());
    let cut = bare.clone().with("faults", wall());
    assert_eq!(op.fitness(&cut), Fitness::Fit);

    let open = op
        .plan(&bare, &frame(&[]), &Identity::derived(), &when())
        .expect("without a fault");
    let closed = op
        .plan(&cut, &frame(&[]), &Identity::derived(), &when())
        .expect("with a fault");
    let nodes = |plan: &energyml_build::operation::Plan| plan.shapes[0].geometry.point_count();
    assert_eq!(nodes(&open), 5);
    // The two vertices of the trace entered the surface.
    assert_eq!(nodes(&closed), 7);
    assert!(closed.says.iter().any(|note| note.key == "nodes-added"));
    let how = |plan: &energyml_build::operation::Plan| {
        plan.activity.parameters.iter().find_map(|p| match p {
            energyml_domain::provenance::Parameter::Text { title, value } if title == "how" => {
                Some(value.clone())
            }
            _ => None,
        })
    };
    assert_eq!(how(&open).as_deref(), Some("delaunay-xy"));
    assert_eq!(how(&closed).as_deref(), Some("constrained-delaunay-xy"));

    // And no edge strictly crosses x = 5: each triangle is
    // on one side only, or touches the trace.
    let geometry = &closed.shapes[0].geometry;
    let patch = &geometry.patches[0];
    let Topology::Triangles(triangles) = &patch.topology else {
        panic!("triangles")
    };
    for t in triangles {
        let xs: Vec<f64> = t.iter().map(|&i| patch.points[i as usize][0]).collect();
        let west = xs.iter().any(|&x| x < 5.0 - 1e-9);
        let east = xs.iter().any(|&x| x > 5.0 + 1e-9);
        assert!(!(west && east), "a triangle straddles the fault : {xs:?}");
    }
}

/// A fault filed under `faults` does not enter the cloud: three fault points
/// and two sample points do not make five points to triangulate.
#[test]
fn a_fault_is_not_one_more_cloud() {
    let op = operation("triangulate");
    let two = Source::Shape {
        geometry: Geometry {
            kind: "cloud",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points: vec![[0.0, 0.0, 1.0], [1.0, 0.0, 1.0]],
                topology: Topology::Points,
                closed: Vec::new(),
            }],
        },
        closed: None,
    };
    let pieces = Pieces::empty().with("shape", two).with("faults", wall());
    match op.fitness(&pieces) {
        Fitness::Unfit(why) => assert!(why.message.contains("holds 2"), "{why}"),
        Fitness::Fit => panic!("two points are not triangulated, fault or not"),
    }
}

// -- remeshing cleanly near the fault -------------------------------------------

/// **A buffer sets aside, a step regularises**, and both are said.
///
/// The point at 100, one metre from the fault, is set aside; the trace of length 12
/// resampled at 4 gives 4 vertices (0, 4, 8, 12) - and as the two sides
/// of the constraint share these vertices, they have the same number of edges on
/// the fault by construction.
#[test]
fn the_buffer_sets_aside_close_points_and_the_step_regularises_the_trace() {
    let op = operation("triangulate");
    // Four corners far from the fault, and two noisy measurements stuck to it
    // - those that one does not want to keep “because they were there”.
    let noisy = Source::Shape {
        geometry: Geometry {
            kind: "cloud",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points: vec![
                    [0.0, 0.0, 1.0],
                    [10.0, 0.0, 1.0],
                    [10.0, 10.0, 1.0],
                    [0.0, 10.0, 1.0],
                    [5.3, 4.0, 40.0],
                    [4.6, 7.0, -30.0],
                ],
                topology: Topology::Points,
                closed: Vec::new(),
            }],
        },
        closed: None,
    };
    let pieces = Pieces::empty().with("shape", noisy).with("faults", wall());
    let plan = op
        .plan(
            &pieces,
            &frame(&[("faultBuffer", 0.8.into()), ("faultSpacing", 4.0.into())]),
            &Identity::derived(),
            &when(),
        )
        .expect("the plan succeeds");
    let buffered = plan.activity.parameters.iter().find_map(|p| match p {
        energyml_domain::provenance::Parameter::Double { title, value, .. } if title == "nodesBuffered" => {
            Some(*value)
        }
        _ => None,
    });
    assert_eq!(buffered, Some(2.0), "the two points within 0.8 of the fault");
    assert!(plan.says.iter().any(|n| n.key == "nodes-buffered"));
    // 4 corners kept + 4 trace vertices (0, 4, 8, 12 on a trace of 12).
    assert_eq!(plan.shapes[0].geometry.point_count(), 8);
    let patch = &plan.shapes[0].geometry.patches[0];
    let on_trace: Vec<f64> = patch
        .points
        .iter()
        .filter(|p| (p[0] - 5.0).abs() < 1e-9)
        .map(|p| p[1])
        .collect();
    assert_eq!(on_trace.len(), 4, "{on_trace:?}");
}

/// Without a fault, the two settings do nothing - and do not complain.
#[test]
fn without_a_fault_the_buffer_and_the_step_have_no_effect() {
    let op = operation("triangulate");
    let pieces = Pieces::empty().with("shape", cloud());
    let plan = op
        .plan(
            &pieces,
            &frame(&[("faultBuffer", 100.0.into()), ("faultSpacing", 0.1.into())]),
            &Identity::derived(),
            &when(),
        )
        .expect("the plan succeeds");
    assert_eq!(plan.shapes[0].geometry.point_count(), 5);
}

// -- the right piece ------------------------------------------------------------

/// **An object with one trace per horizon does not wall off that horizon with the
/// others.** Two traces in one object, one at the depth of the cloud,
/// the other a hundred metres lower and shifted in (x, y) - the lower one is
/// set aside, said, and does not act.
#[test]
fn only_the_traces_at_the_depth_of_the_horizon_act() {
    let two_traces = Source::Shape {
        geometry: Geometry {
            kind: "fault",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                // At the depth of the cloud (z ≈ 1): the wall at x = 5. A hundred metres
                // lower: a trace at x = 2, which would cut the cloud in two.
                points: vec![
                    [5.0, -1.0, 1.0],
                    [5.0, 11.0, 1.0],
                    [2.0, -1.0, -100.0],
                    [2.0, 11.0, -100.0],
                ],
                topology: Topology::Polylines(vec![2, 2]),
                closed: Vec::new(),
            }],
        },
        closed: None,
    };
    // A cloud all at z = 1, so that the depth range is clear.
    let flat = Source::Shape {
        geometry: Geometry {
            kind: "cloud",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points: vec![
                    [0.0, 0.0, 1.0],
                    [4.0, 0.0, 1.0],
                    [4.0, 10.0, 1.0],
                    [0.0, 10.0, 1.0],
                    [6.0, 5.0, 1.0],
                ],
                topology: Topology::Points,
                closed: Vec::new(),
            }],
        },
        closed: None,
    };
    let op = operation("triangulate");
    let pieces = Pieces::empty()
        .with("shape", flat.clone())
        .with("faults", two_traces.clone());
    let plan = op
        .plan(&pieces, &frame(&[]), &Identity::derived(), &when())
        .expect("the plan succeeds");
    assert!(
        plan.says.iter().any(|n| n.key == "faults-elsewhere"),
        "the lower trace is said"
    );
    // Only the two vertices of the trace at x = 5 entered: 5 + 2.
    assert_eq!(plan.shapes[0].geometry.point_count(), 7);

    // And if no trace is at this depth, it is a named refusal.
    let deep = Source::Shape {
        geometry: Geometry {
            kind: "fault",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points: vec![[2.0, -1.0, -100.0], [2.0, 11.0, -100.0]],
                topology: Topology::Polylines(vec![2]),
                closed: Vec::new(),
            }],
        },
        closed: None,
    };
    let pieces = Pieces::empty().with("shape", flat).with("faults", deep);
    let why = op
        .plan(&pieces, &frame(&[]), &Identity::derived(), &when())
        .expect_err("refused");
    assert!(why.message().contains("intersect"), "{}", why.message());
}

// -- remailler ----------------------------------------------------------------

/// **`remesh` throws away the topology and keeps the nodes.** A horizon whose single
/// triangle joins five nodes comes out with a full mesh, and `triangulate`
/// would have refused it as already triangulated.
#[test]
fn remesh_redoes_the_mesh_from_the_nodes() {
    let poor = Source::Shape {
        geometry: Geometry {
            kind: "horizon",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points: vec![
                    [0.0, 0.0, 1.0],
                    [10.0, 0.0, 1.0],
                    [10.0, 10.0, 1.0],
                    [0.0, 10.0, 1.0],
                    [5.0, 5.0, 2.0],
                ],
                topology: Topology::Triangles(vec![[0, 1, 2]]),
                closed: Vec::new(),
            }],
        },
        closed: None,
    };
    let pieces = Pieces::empty().with("shape", poor.clone());
    assert!(
        matches!(operation("triangulate").fitness(&pieces), Fitness::Unfit(_)),
        "already triangulated: refused there"
    );
    let op = operation("remesh");
    assert_eq!(op.fitness(&pieces), Fitness::Fit);
    let plan = op
        .plan(&pieces, &frame(&[]), &Identity::derived(), &when())
        .expect("the plan succeeds");
    let Topology::Triangles(t) = &plan.shapes[0].geometry.patches[0].topology else {
        panic!("triangles")
    };
    assert_eq!(
        t.len(),
        4,
        "five nodes of which one at the centre: four triangles"
    );
    assert_eq!(plan.activity.recipe, "remesh");

    // And a cloud is not for it: it names `triangulate`.
    let cloud_only = Pieces::empty().with("shape", cloud());
    match op.fitness(&cloud_only) {
        Fitness::Unfit(why) => assert!(why.message.contains("triangulate"), "{why}"),
        Fitness::Fit => panic!("a cloud goes to `triangulate`"),
    }
}
