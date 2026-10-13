//! `measure` — areas, lengths and volumes (
//! the geological operations design), confronted with answers computed
//! **another way**: an analytic formula, or a naive integration written here.
//!
//! # What would have to break to make it turn red
//!
//! Writing the plan area under the name of the true area (or the reverse); cutting
//! a cell by a diagonal, or integrating it by a Gauss point; forgetting
//! the closing segment that a file states, or adding one that it does not
//! state; laying the volume of a cell at the compacted rank rather than at its number
//! in the grid; measuring in degrees; adding metres and feet.

use serde_json::json;

use energyml_build::operation::{Choices, Identity, Numbers, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_domain::crs::{Crs, CrsKind, VerticalDirection, VerticalQuantity};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When {
        creation: "2026-09-25T00:00:00Z".into(),
        originator: "test".into(),
    }
}

/// A frame whose units are stated: `horizontal` on x and y, `vertical` on z.
fn frame(horizontal: &str, vertical: &str, quantity: VerticalQuantity) -> Crs {
    Crs {
        uuid: "cccccccc-0000-4000-8000-000000000003".into(),
        kind: "LocalEngineeringCompoundCrs",
        x_offset: 0.0,
        y_offset: 0.0,
        z_offset: 0.0,
        areal_rotation: 0.0,
        vertical_direction: VerticalDirection::Up,
        vertical_quantity: quantity,
        horizontal_uom: Some(horizontal.into()),
        vertical_uom: Some(vertical.into()),
        time_uom: None,
        projected: CrsKind::Epsg(2154),
        vertical: CrsKind::Absent,
    }
}

fn known(qualified_type: &str, placement: Option<Crs>) -> Known {
    Known {
        reference: Some(Ref::new(
            "11111111-0000-4000-8000-000000000001",
            qualified_type,
            "Horizon",
        )),
        crs: Some(Ref::new(
            "cccccccc-0000-4000-8000-000000000003",
            "eml23.LocalEngineeringCompoundCrs",
            "The frame",
        )),
        vertical_uom: placement.as_ref().and_then(|p| p.vertical_uom.clone()),
        placement,
        ..Default::default()
    }
}

fn bag(topology: Topology, points: Vec<[f64; 3]>, closed: Vec<bool>, known: Known) -> Pieces {
    Pieces::empty()
        .with(
            "shape",
            Source::Shape {
                geometry: Geometry {
                    kind: "x",
                    patches: vec![Patch {
                        index: 0,
                        node_offset: 0,
                        points,
                        topology,
                        closed,
                    }],
                },
                closed: None,
            },
        )
        .with("shape (object)", Source::Object(known))
}

fn metres() -> Option<Crs> {
    Some(frame("m", "m", VerticalQuantity::Length))
}

fn run(pieces: &Pieces, choices: Choices) -> Result<Plan, energyml_build::operation::Error> {
    Registry::new().plan("measure", pieces, &choices, &Identity::derived(), &when())
}

fn values(plan: &Plan) -> Vec<f64> {
    match &plan.datasets[0].values {
        Numbers::F64(values) => values.clone(),
        other => panic!("doubles expected, got {other:?}"),
    }
}

fn reported(plan: &Plan, title: &str) -> f64 {
    plan.activity
        .parameters
        .iter()
        .find_map(|p| match p {
            Parameter::Double { title: t, value, .. } if t == title => Some(*value),
            _ => None,
        })
        .unwrap_or_else(|| panic!("`{title}` is not reported"))
}

fn close(a: f64, b: f64, relative: f64) -> bool {
    (a - b).abs() <= relative * b.abs().max(1.0)
}

/// A square of side 10 on the plane `z = 0.5 x`, in two triangles.
fn tilted_square() -> (Topology, Vec<[f64; 3]>) {
    let points = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]
        .iter()
        .map(|&[x, y]| [x, y, 0.5 * x])
        .collect();
    (Topology::Triangles(vec![[0, 1, 2], [0, 2, 3]]), points)
}

#[test]
fn a_tilted_square_has_its_analytic_area_along_and_in_plan() {
    // Analytic: the plane z = 0.5 x stretches x by √(1 + 0.25).
    let (topology, points) = tilted_square();
    let pieces = bag(
        topology,
        points,
        Vec::new(),
        known("resqml22.TriangulatedSetRepresentation", metres()),
    );
    let along = run(&pieces, Choices::new()).expect("planned");
    let per_triangle = values(&along);
    assert_eq!(per_triangle.len(), 2, "one value per triangle");
    let expected = 100.0 * 1.25f64.sqrt();
    assert!(
        close(per_triangle.iter().sum(), expected, 1e-12),
        "{per_triangle:?}"
    );
    assert!(close(reported(&along, "total"), expected, 1e-12));
    assert!(close(reported(&along, "totalAlong"), expected, 1e-12));
    assert!(
        close(reported(&along, "totalPlan"), 100.0, 1e-12),
        "both totals are said"
    );
    let object = &along.objects[0].json;
    assert_eq!(object["IndexableElement"], json!("triangles"));
    assert_eq!(object["Uom"], json!("m2"));
    assert_eq!(
        object["PropertyKind"]["Title"],
        json!("area"),
        "the dictionary's own kind"
    );

    let plan = run(&pieces, Choices::new().set("measured", "plan")).expect("planned");
    assert!(close(values(&plan).iter().sum(), 100.0, 1e-12));
    assert!(
        plan.objects[0].title.contains("plan area"),
        "{}",
        plan.objects[0].title
    );
}

/// `z = x y / 100` over `[0, 100]²` is bilinear on every cell, so the cells
/// *are* the surface: their areas must add up to the surface integral,
/// computed here by a plain midpoint rule — no Gauss, no bilinear patch.
#[test]
fn a_warped_grid_adds_up_to_an_independent_integration() {
    let n = 4usize;
    let step = 100.0 / n as f64;
    let mut points = Vec::new();
    for j in 0..=n {
        for i in 0..=n {
            let (x, y) = (i as f64 * step, j as f64 * step);
            points.push([x, y, x * y / 100.0]);
        }
    }
    let pieces = bag(
        Topology::Grid {
            fastest: n + 1,
            slowest: n + 1,
        },
        points,
        Vec::new(),
        known("resqml22.Grid2dRepresentation", metres()),
    );
    let planned = run(&pieces, Choices::new()).expect("planned");
    let cells = values(&planned);
    assert_eq!(cells.len(), n * n, "one value per cell");
    assert_eq!(planned.objects[0].json["IndexableElement"], json!("cells"));

    // ∫∫ √(1 + (y/100)² + (x/100)²) dx dy, midpoint rule, 2000².
    let m = 2000usize;
    let h = 100.0 / m as f64;
    let mut naive = 0.0;
    for j in 0..m {
        for i in 0..m {
            let (x, y) = ((i as f64 + 0.5) * h, (j as f64 + 0.5) * h);
            naive += (1.0 + (y / 100.0).powi(2) + (x / 100.0).powi(2)).sqrt() * h * h;
        }
    }
    let total: f64 = cells.iter().sum();
    assert!(
        close(total, naive, 1e-7),
        "cells {total} against the integral {naive}"
    );
    assert!(
        close(reported(&planned, "totalPlan"), 10_000.0, 1e-12),
        "the plan area of the grid is its extent"
    );
}

#[test]
fn polylines_measure_per_line_and_close_only_what_is_said_closed() {
    // A 3-4-5 leg then a 12 m drop — 17 along, 5 in plan; then a square of
    // side 10 the file says closed — 40 both ways.
    let points = vec![
        [0.0, 0.0, 0.0],
        [3.0, 4.0, 0.0],
        [3.0, 4.0, -12.0],
        [0.0, 0.0, 5.0],
        [10.0, 0.0, 5.0],
        [10.0, 10.0, 5.0],
        [0.0, 10.0, 5.0],
    ];
    let pieces = bag(
        Topology::Polylines(vec![3, 4]),
        points.clone(),
        vec![false, true],
        known("resqml22.PolylineSetRepresentation", metres()),
    );
    let along = run(&pieces, Choices::new()).expect("planned");
    assert_eq!(values(&along), vec![17.0, 40.0]);
    assert_eq!(along.objects[0].json["IndexableElement"], json!("lines"));
    assert_eq!(along.objects[0].json["PropertyKind"]["Title"], json!("length"));
    assert_eq!(along.objects[0].json["Uom"], json!("m"));
    let plan = run(&pieces, Choices::new().set("measured", "plan")).expect("planned");
    assert_eq!(values(&plan), vec![5.0, 40.0]);

    // Nobody said: open, and the plan says so.
    let unsaid = bag(
        Topology::Polylines(vec![3, 4]),
        points,
        Vec::new(),
        known("resqml22.PolylineSetRepresentation", metres()),
    );
    let planned = run(&unsaid, Choices::new()).expect("planned");
    assert_eq!(values(&planned), vec![17.0, 30.0]);
    assert!(planned.says.iter().any(|note| note.key == "closure-unsaid"));
}

/// Eight corners, bottom ring then top ring, VTK order.
fn cell(bottom: [[f64; 3]; 4], top: [[f64; 3]; 4]) -> Vec<[f64; 3]> {
    bottom.iter().chain(top.iter()).copied().collect()
}

#[test]
fn cell_volumes_are_exact_and_sit_at_their_number_in_the_grid() {
    // Cell 0: a frustum of square bases 2 × 2 at z = 0 and 1 × 1 at z = 3 —
    // planar faces, V = h/3 (A₁ + A₂ + √(A₁A₂)) = 7.
    let frustum = cell(
        [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 2.0, 0.0], [0.0, 2.0, 0.0]],
        [[0.5, 0.5, 3.0], [1.5, 0.5, 3.0], [1.5, 1.5, 3.0], [0.5, 1.5, 3.0]],
    );
    // Cell 2: the unit cube with one top corner raised by 0.8 — a warped top
    // z = 1 + 0.8 u v, so V = 1 + 0.8/4 = 1.2 exactly; no tetrahedra give it.
    let warped = cell(
        [[5.0, 0.0, 0.0], [6.0, 0.0, 0.0], [6.0, 1.0, 0.0], [5.0, 1.0, 0.0]],
        [[5.0, 0.0, 1.0], [6.0, 0.0, 1.0], [6.0, 1.0, 1.8], [5.0, 1.0, 1.0]],
    );
    let points: Vec<[f64; 3]> = frustum.into_iter().chain(warped).collect();
    // Cell 1 of the 3 × 1 × 1 grid has no geometry: the reader dropped it,
    // and `index` numbers the two it kept.
    let topology = Topology::Hexahedra {
        cells: vec![[0, 1, 2, 3, 4, 5, 6, 7], [8, 9, 10, 11, 12, 13, 14, 15]],
        dimensions: [3, 1, 1],
        index: vec![0, 2],
    };
    let pieces = bag(
        topology,
        points,
        Vec::new(),
        known("resqml22.IjkGridRepresentation", metres()),
    );
    let planned = run(&pieces, Choices::new()).expect("planned");
    let volumes = values(&planned);
    assert_eq!(
        volumes.len(),
        3,
        "one value per cell of the grid, dropped ones included"
    );
    assert!(close(volumes[0], 7.0, 1e-12), "{volumes:?}");
    assert!(
        volumes[1].is_nan(),
        "a cell with no geometry is NaN at its own number: {volumes:?}"
    );
    assert!(close(volumes[2], 1.2, 1e-12), "{volumes:?}");
    assert_eq!(planned.objects[0].json["Uom"], json!("m3"));
    assert_eq!(planned.objects[0].json["PropertyKind"]["Title"], json!("volume"));
    assert!(close(reported(&planned, "total"), 8.2, 1e-12));
    assert_eq!(reported(&planned, "undefinedCount"), 1.0);

    let plan = run(&pieces, Choices::new().set("measured", "plan")).expect_err("a volume has no plan");
    assert_eq!(plan.refusals()[0].slot.as_deref(), Some("measured"));
}

#[test]
fn a_frame_in_degrees_is_refused_by_name() {
    let (topology, points) = tilted_square();
    let mut degrees = known("resqml22.TriangulatedSetRepresentation", None);
    degrees.crs = Some(Ref::new(
        "dddddddd-0000-4000-8000-000000000004",
        "eml23.Geographic2dCrs",
        "WGS 84",
    ));
    let refused = run(
        &bag(topology.clone(), points.clone(), Vec::new(), degrees),
        Choices::new().set("lengthUom", "m"),
    )
    .expect_err("degrees");
    assert_eq!(refused.refusals()[0].key, "degrees", "{refused}");

    // The same by the unit a frame states.
    let angular = known(
        "resqml22.TriangulatedSetRepresentation",
        Some(frame("dega", "m", VerticalQuantity::Length)),
    );
    let refused = run(
        &bag(topology, points, Vec::new(), angular),
        Choices::new().set("measured", "plan"),
    )
    .expect_err("degrees");
    assert_eq!(refused.refusals()[0].key, "degrees", "{refused}");
}

#[test]
fn units_that_disagree_are_refused_in_three_dimensions_and_read_in_plan() {
    let (topology, points) = tilted_square();
    let mixed = bag(
        topology.clone(),
        points.clone(),
        Vec::new(),
        known(
            "x.TriangulatedSetRepresentation",
            Some(frame("ft", "m", VerticalQuantity::Length)),
        ),
    );
    let refused = run(&mixed, Choices::new()).expect_err("ft and m");
    assert_eq!(refused.refusals()[0].key, "units", "{refused}");
    let plan =
        run(&mixed, Choices::new().set("measured", "plan")).expect("the plan reads the horizontal only");
    assert_eq!(plan.objects[0].json["Uom"], json!("ft2"));

    let time = bag(
        topology.clone(),
        points.clone(),
        Vec::new(),
        known(
            "x.TriangulatedSetRepresentation",
            Some(frame("m", "ms", VerticalQuantity::Time)),
        ),
    );
    assert_eq!(
        run(&time, Choices::new()).expect_err("time").refusals()[0].key,
        "units"
    );
    run(&time, Choices::new().set("measured", "plan")).expect("a plan area in a time frame is an area");

    // Nothing stated: asked for, and checked when the frame does say.
    let silent = bag(
        topology.clone(),
        points.clone(),
        Vec::new(),
        known("x.TriangulatedSetRepresentation", None),
    );
    let refused = run(&silent, Choices::new()).expect_err("no unit");
    assert_eq!(refused.refusals()[0].slot.as_deref(), Some("lengthUom"));
    let given = run(&silent, Choices::new().set("lengthUom", "ft")).expect("given");
    assert_eq!(given.objects[0].json["Uom"], json!("ft2"));
    let metric = bag(
        topology,
        points,
        Vec::new(),
        known("x.TriangulatedSetRepresentation", metres()),
    );
    let refused = run(&metric, Choices::new().set("lengthUom", "ft")).expect_err("ft against m");
    assert_eq!(refused.refusals()[0].slot.as_deref(), Some("lengthUom"));
}

#[test]
fn a_trajectory_is_not_a_chord_sum() {
    let points = vec![[0.0, 0.0, 0.0], [0.0, 0.0, -100.0]];
    let pieces = bag(
        Topology::Polylines(vec![2]),
        points,
        Vec::new(),
        known("resqml22.WellboreTrajectoryRepresentation", metres()),
    );
    let refused = run(&pieces, Choices::new()).expect_err("a trajectory");
    assert!(refused.message().contains("measured-depth"), "{refused}");
}
