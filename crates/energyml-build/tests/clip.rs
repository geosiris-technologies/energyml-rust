//! `clip` — a shape cut by rings. What a hole does to each kind is
//! `delete-nodes`' own (`tests/delete_nodes.rs`); this holds what the rings
//! decide, checked by an even-odd loop written here and never by
//! `energyml_geometry::clip`, which would have the function agree with itself.
//!
//! # What would have to break to make it turn red
//!
//! Inverting the sense of a ring (keeping what is outside `outer`, or inside an
//! `inner`); forgetting the `inner`s; writing a grid without its holes; letting
//! through a cut that cuts nothing or that cuts everything, or absent
//! rings.

use energyml_build::operation::{Choices, Identity, Plan, Purpose, Registry, When};
use energyml_build::pieces::{Pieces, Source};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When {
        creation: "2026-10-01T00:00:00Z".into(),
        originator: "test".into(),
    }
}

/// Inside a polygon, by the even-odd rule — the check this file does not share
/// with the implementation.
fn inside(polygon: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut odd = false;
    for k in 0..polygon.len() {
        let (a, b) = (polygon[k], polygon[(k + 1) % polygon.len()]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
            odd = !odd;
        }
    }
    odd
}

/// The rings of every test — off-centre, corners at `.3` so that no node of
/// the integer grids below lies on an edge, and the hole inside the outer one.
const OUTER: [[f64; 2]; 4] = [[0.3, 0.3], [8.3, 1.3], [7.3, 8.3], [1.3, 6.3]];
const INNER: [[f64; 2]; 3] = [[3.3, 3.3], [5.3, 3.3], [4.3, 5.3]];

fn kept(p: [f64; 2]) -> bool {
    inside(&OUTER, p) && !inside(&INNER, p)
}

fn ring(corners: &[[f64; 2]]) -> Source {
    let geometry = Geometry {
        kind: "ring",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points: corners.iter().map(|&[x, y]| [x, y, 0.0]).collect(),
            topology: Topology::Polylines(vec![corners.len() as u32]),
            closed: vec![true],
        }],
    };
    Source::Shape {
        geometry,
        closed: Some(vec![true]),
    }
}

/// `n × n` nodes at the integers, `z = x + 10 y`, in the topology given.
fn square(n: usize, topology: Topology) -> Source {
    let points = (0..n)
        .flat_map(|j| (0..n).map(move |i| [i as f64, j as f64, i as f64 + 10.0 * j as f64]))
        .collect();
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

fn plan(bag: &Pieces) -> Result<Plan, String> {
    Registry::new()
        .plan_for(
            "clip",
            bag,
            &Choices::new().set("title", "clipped"),
            &Identity::derived(),
            &when(),
            Purpose::Geometry,
        )
        .map_err(|why| why.message())
}

fn reported(plan: &Plan, title: &str) -> Option<f64> {
    plan.activity.parameters.iter().find_map(|p| match p {
        Parameter::Double { title: t, value, .. } if t == title => Some(*value),
        _ => None,
    })
}

/// **A cloud keeps exactly the nodes between the two rings**, in their order.
#[test]
fn a_cloud_keeps_what_is_between_the_two_rings() {
    let bag = Pieces::empty()
        .with("shape", square(10, Topology::Points))
        .with("outer", ring(&OUTER))
        .with("inner", ring(&INNER));
    let made = plan(&bag).unwrap_or_else(|why| panic!("{why}"));
    let want: Vec<[f64; 3]> = (0..10)
        .flat_map(|j| (0..10).map(move |i| [i as f64, j as f64, i as f64 + 10.0 * j as f64]))
        .filter(|p| kept([p[0], p[1]]))
        .collect();
    let got: Vec<[f64; 3]> = made.shapes[0]
        .geometry
        .patches
        .iter()
        .flat_map(|p| p.points.iter().copied())
        .collect();
    assert_eq!(got, want);
    assert!(
        want.len() > 10 && want.len() < 90,
        "sanity: the rings cut some and keep some ({})",
        want.len()
    );
    assert_eq!(reported(&made, "cutNodes"), Some((100 - want.len()) as f64));
    assert!(
        made.says.first().is_some_and(|note| note.key == "cut"),
        "{:?}",
        made.says
    );
}

/// **A 2D grid keeps its lattice and gets holes** where the rings cut, and
/// nowhere else.
#[test]
fn a_grid_keeps_its_lattice_and_takes_holes() {
    let bag = Pieces::empty()
        .with(
            "shape",
            square(
                10,
                Topology::Grid {
                    fastest: 10,
                    slowest: 10,
                },
            ),
        )
        .with("outer", ring(&OUTER))
        .with("inner", ring(&INNER));
    let made = plan(&bag).unwrap_or_else(|why| panic!("{why}"));
    let points: Vec<[f64; 3]> = made.shapes[0]
        .geometry
        .patches
        .iter()
        .flat_map(|p| p.points.iter().copied())
        .collect();
    assert_eq!(points.len(), 100, "the lattice keeps every node");
    for p in points {
        assert_eq!(
            p[2].is_finite(),
            kept([p[0], p[1]]),
            "node at ({}, {}): {}",
            p[0],
            p[1],
            p[2]
        );
        if p[2].is_finite() {
            assert_eq!(p[2], p[0] + 10.0 * p[1], "a kept node keeps its height");
        }
    }
    assert_eq!(
        made.objects.last().unwrap().qualified_type,
        "resqml22.Grid2dRepresentation"
    );
}

/// **A surface loses the triangles that name a cut node**, and only them.
#[test]
fn a_surface_loses_the_triangles_that_touch_a_cut() {
    let n = 10u32;
    let at = |i: u32, j: u32| j * n + i;
    let triangles: Vec<[u32; 3]> = (0..n - 1)
        .flat_map(|j| {
            (0..n - 1).flat_map(move |i| {
                [
                    [at(i, j), at(i + 1, j), at(i + 1, j + 1)],
                    [at(i, j), at(i + 1, j + 1), at(i, j + 1)],
                ]
            })
        })
        .collect();
    let node = |k: u32| [(k % n) as f64, (k / n) as f64];
    let survive = triangles
        .iter()
        .filter(|t| t.iter().all(|&k| kept(node(k))))
        .count();
    let bag = Pieces::empty()
        .with("shape", square(10, Topology::Triangles(triangles.clone())))
        .with("outer", ring(&OUTER))
        .with("inner", ring(&INNER));
    let made = plan(&bag).unwrap_or_else(|why| panic!("{why}"));
    let Topology::Triangles(left) = &made.shapes[0].geometry.patches[0].topology else {
        panic!("triangles")
    };
    assert_eq!(left.len(), survive);
    assert_eq!(
        reported(&made, "removedTriangles"),
        Some((triangles.len() - survive) as f64)
    );
    assert!(
        made.says.iter().any(|note| note.key == "follows-the-mesh"),
        "the jagged edge is said"
    );
}

/// **Refused by name**: no ring, a cut of nothing, a cut of everything.
#[test]
fn without_a_ring_nothing_cuts_or_everything_cuts_is_refused() {
    let no_ring = plan(&Pieces::empty().with("shape", square(4, Topology::Points))).expect_err("no ring");
    assert!(no_ring.contains("nothing to cut with"), "{no_ring}");

    let around = [[-1.0, -1.0], [9.0, -1.0], [9.0, 9.0], [-1.0, 9.0]];
    let nothing = plan(
        &Pieces::empty()
            .with("shape", square(4, Topology::Points))
            .with("outer", ring(&around)),
    )
    .expect_err("nothing cut");
    assert!(nothing.contains("nothing to cut"), "{nothing}");

    let far = [[50.3, 50.3], [60.3, 50.3], [55.3, 60.3]];
    let everything = plan(
        &Pieces::empty()
            .with("shape", square(4, Topology::Points))
            .with("outer", ring(&far)),
    )
    .expect_err("all cut");
    assert!(everything.contains("nothing would be left"), "{everything}");
}
