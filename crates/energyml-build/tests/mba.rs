//! `interpolate`'s `Method::Mba`, `estimate` and `resurface` — the
//! build-layer side of the MBA operations design.
//!
//! **These tests only compile once the integration retouches are applied** —
//! `estimate`/`resurface` are not yet in `ops::builtin()`, `Method` has no
//! `Mba` case yet, and `pointset::write` does not exist as a free function
//! yet. Written now, against the shape the integration doc promises, the way
//! every other test in this suite already talks to an operation: through
//! `Registry::plan`, a `Pieces` bag and a flat `Choices`, never the typed
//! structs directly — see `tests/interpolate_onto.rs`, whose helpers this
//! file's own mirror closely.
//!
//! The numeric properties of MBA itself — exact interpolation, a constant
//! or a plane reproduced, non-affine-invariance, the tanh test — are already
//! covered without any package at all, in
//! `crates/energyml-analysis/src/mba.rs` and `mba_frame.rs`'s own
//! `#[cfg(test)]` modules. What is specific to *this* layer, and only
//! testable through a plan, is what these tests are after: the lattice/ring
//! plumbing, and that `resurface` truly recopies a topology rather than
//! reinventing one.

use serde_json::json;

use energyml_build::operation::{Choices, Identity, Numbers, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When {
        creation: "2026-09-29T00:00:00Z".into(),
        originator: "test".into(),
    }
}

fn frame() -> Ref {
    Ref::new(
        "aaaaaaaa-0000-4000-8000-00000000000a",
        "eml23.LocalEngineeringCompoundCrs",
        "The test frame",
    )
}

fn known(uuid: &str, qualified_type: &str, title: &str) -> Source {
    Source::Object(Known {
        reference: Some(Ref::new(uuid, qualified_type, title)),
        crs: Some(frame()),
        vertical_uom: Some("m".into()),
        vertical_up: Some(true),
        ..Default::default()
    })
}

fn plan_of(recipe: &str, pieces: &Pieces, choices: Choices) -> Result<Plan, String> {
    Registry::new()
        .plan(recipe, pieces, &choices, &Identity::derived(), &when())
        .map_err(|why| why.message())
}

/// A regular cloud of `n × n` points on the plane `z = plane(x, y)`.
fn plane_cloud(n: usize, spacing: f64, plane: impl Fn(f64, f64) -> f64) -> Geometry {
    let points = (0..n)
        .flat_map(|j| (0..n).map(move |i| (i, j)))
        .map(|(i, j)| {
            let (x, y) = (i as f64 * spacing, j as f64 * spacing);
            [x, y, plane(x, y)]
        })
        .collect();
    Geometry {
        kind: "cloud",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points,
            topology: Topology::Points,
            closed: Vec::new(),
        }],
    }
}

/// **`interpolate` with `method = mba` reproduces a constant field**
/// (`Fitted` extent, `world` frame — the simplest path). To break: in
/// `interpolate_mba::plan_fitted`, keeping only the last level (removing
/// the coarsest level from the sum) would make the value fall back to zero
/// far from the samples, cf. `mba.rs`'s own test of the same property without a
/// package.
#[test]
fn interpolate_with_mba_reproduces_a_constant_field() {
    let samples = plane_cloud(5, 3.0, |_, _| 42.0);
    let pieces = Pieces::empty()
        .with(
            "shape",
            Source::Shape {
                geometry: samples,
                closed: None,
            },
        )
        .with(
            "shape (object)",
            known(
                "b0b0b0b0-0000-4000-8000-000000000001",
                "resqml22.PointSetRepresentation",
                "constant",
            ),
        );
    let choices = Choices::new()
        .set("method", "mba")
        .set("title", "constant-mba")
        .set("spacing", 1.5)
        .set("margin", 3.0);
    let made = plan_of("interpolate", &pieces, choices).expect("planned");
    let Numbers::F64(z) = &made.datasets[0].values else {
        panic!("heights are doubles")
    };
    for &value in z {
        assert!(
            (value - 42.0).abs() < 1e-6,
            "{value} != 42.0 away from the samples too"
        );
    }
}

/// **`resurface` on a `Grid2dRepresentation` keeps the lattice
/// identical, bit for bit** — the origin, the steps and the counts do not
/// move, only `z` changes. To break: swap `fastest`/`slowest` in
/// `resurface::plan`'s reconstruction of `dir2_raw` (read `points[slowest]`
/// instead of `points[fastest]`) turns red on the counts/directions, not
/// on the values — the proof that this test looks at the topology and not
/// only at the numeric result.
#[test]
fn resurface_keeps_a_grids_lattice_bit_for_bit() {
    let (ni, nj, dx, dy) = (6usize, 5usize, 4.0, 3.0);
    let noisy = |i: usize, j: usize| 10.0 + 0.3 * (i as f64).sin() * (j as f64).cos();
    let points = (0..nj)
        .flat_map(|j| (0..ni).map(move |i| (i, j)))
        .map(|(i, j)| [i as f64 * dx, j as f64 * dy, noisy(i, j)])
        .collect();
    let grid = Geometry {
        kind: "grid",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points,
            topology: Topology::Grid {
                fastest: ni,
                slowest: nj,
            },
            closed: Vec::new(),
        }],
    };
    let pieces = Pieces::empty()
        .with(
            "shape",
            Source::Shape {
                geometry: grid,
                closed: None,
            },
        )
        .with(
            "shape (object)",
            known(
                "c0c0c0c0-0000-4000-8000-000000000002",
                "resqml22.Grid2dRepresentation",
                "surface",
            ),
        );
    let made = plan_of(
        "resurface",
        &pieces,
        Choices::new().set("title", "resurfaced").set("spacing", 1.0),
    )
    .expect("planned");

    let written = &made.of_role("representation").next().expect("a grid").json;
    let support = &written["Geometry"]["Points"]["SupportingGeometry"];
    assert_eq!(
        written["FastestAxisCount"],
        json!(ni),
        "the fastest-axis count moved"
    );
    assert_eq!(
        written["SlowestAxisCount"],
        json!(nj),
        "the slowest-axis count moved"
    );
    let origin = &support["Origin"];
    assert_eq!(origin["Coordinate1"], json!(0.0));
    assert_eq!(origin["Coordinate2"], json!(0.0));
    // The slow axis first, the fast one second — the house order.
    let [slow, fast] = [&support["Dimension"][0], &support["Dimension"][1]];
    assert_eq!(fast["Spacing"]["Value"], json!(dx), "the fast axis' step moved");
    assert_eq!(slow["Spacing"]["Value"], json!(dy), "the slow axis' step moved");

    let Numbers::F64(z) = &made.datasets[0].values else {
        panic!("heights are doubles")
    };
    assert_eq!(z.len(), ni * nj, "the node count moved");
    // Loose sanity on the heights themselves — the point of *this* test is
    // the lattice staying put, not MBA's numerics (already covered without
    // a package at all, `crates/energyml-analysis/src/mba.rs`'s own tests).
    for j in 0..nj {
        for i in 0..ni {
            let original = noisy(i, j);
            let got = z[j * ni + i];
            assert!(
                (got - original).abs() < 5.0,
                "node ({i},{j}): {got} far from the original {original}"
            );
        }
    }
}

/// **`resurface` on a `TriangulatedSetRepresentation` keeps the triangle
/// indices identical, row for row.** To break: sorting the triangles by
/// their first index before rewriting them would turn this test red without
/// touching the count or the positions — precisely the bug that a test that
/// compared only sets would not see.
#[test]
fn resurface_keeps_a_triangulated_surfaces_indices_line_for_line() {
    let points = vec![
        [0.0, 0.0, 1.0],
        [2.0, 0.0, 1.1],
        [0.0, 2.0, 0.9],
        [2.0, 2.0, 1.2],
        [1.0, 1.0, 1.0],
    ];
    // Deliberately **not** sorted by first index: 4 before 0, 2 before 1 —
    // exactly the order a "tidy" rewrite would be tempted to normalise.
    let triangles: Vec<[u32; 3]> = vec![[4, 0, 1], [4, 1, 3], [2, 0, 4], [3, 4, 2]];
    let surface = Geometry {
        kind: "triangles",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points,
            topology: Topology::Triangles(triangles.clone()),
            closed: Vec::new(),
        }],
    };
    let pieces = Pieces::empty()
        .with(
            "shape",
            Source::Shape {
                geometry: surface,
                closed: None,
            },
        )
        .with(
            "shape (object)",
            known(
                "d0d0d0d0-0000-4000-8000-000000000003",
                "resqml22.TriangulatedSetRepresentation",
                "fault",
            ),
        );
    let made = plan_of(
        "resurface",
        &pieces,
        Choices::new().set("title", "resurfaced-tri").set("spacing", 0.5),
    )
    .expect("planned");

    let written = &made
        .of_role("representation")
        .next()
        .expect("a triangulated surface")
        .json;
    let written_indices = &written["TrianglePatch"][0]["Triangles"]["Values"];
    // The dataset the writer actually queued is the source of truth for the
    // indices — the JSON only carries a pointer to it (`support::external`).
    let dataset = made
        .datasets
        .iter()
        .find(|d| d.path.contains("triangles"))
        .expect("a triangles dataset");
    let Numbers::I64(indices) = &dataset.values else {
        panic!("triangle indices are integers")
    };
    let expected: Vec<i64> = triangles.iter().flatten().map(|&i| i as i64).collect();
    assert_eq!(
        indices, &expected,
        "the triangle indices moved — resurface must recopy them, not re-triangulate"
    );
    assert!(
        written_indices.is_object() || written_indices.is_string(),
        "sanity: the JSON names an external array"
    );
}

/// **`estimate` estimates the z of a given cloud.** A plane (degree 1), instead
/// of a constant, to tell a fit that really works from one
/// that would fall back on a mean. To break: in `estimate::plan`, forgetting
/// the margin of the domain (`Ω` = bbox of the samples alone rather than
/// samples ∪ queries) would make every point of `at` laid beyond
/// the samples refused rather than estimated.
#[test]
fn estimate_reads_the_z_of_a_cloud_the_caller_gives() {
    let plane = |x: f64, y: f64| 5.0 + 0.4 * x - 0.2 * y;
    let samples = plane_cloud(6, 2.0, plane);
    let at_points: Vec<[f64; 3]> = vec![[3.0, 3.0, 0.0], [7.0, 1.0, 0.0], [0.5, 8.5, 0.0]];
    let at = Geometry {
        kind: "cloud",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points: at_points.clone(),
            topology: Topology::Points,
            closed: Vec::new(),
        }],
    };
    let pieces = Pieces::empty()
        .with(
            "samples",
            Source::Shape {
                geometry: samples,
                closed: None,
            },
        )
        .with(
            "samples (object)",
            known(
                "e0e0e0e0-0000-4000-8000-000000000004",
                "resqml22.PointSetRepresentation",
                "seeds",
            ),
        )
        .with(
            "at",
            Source::Shape {
                geometry: at,
                closed: None,
            },
        );
    let made = plan_of(
        "estimate",
        &pieces,
        Choices::new()
            .set("title", "estimated")
            .set("spacing", 1.0)
            .set("margin", 1.0),
    )
    .expect("planned");

    let Numbers::F64(flat) = &made.datasets[0].values else {
        panic!("points are doubles")
    };
    assert_eq!(flat.len(), at_points.len() * 3);
    for (rank, p) in at_points.iter().enumerate() {
        let got_z = flat[rank * 3 + 2];
        let want_z = plane(p[0], p[1]);
        assert!((got_z - want_z).abs() < 1e-3, "query {rank}: {got_z} != {want_z}");
    }
}

/// **An inner ring removes what it should, and only that.**
/// Verified by an independent computation (an even-odd loop written here, not
/// `energyml_geometry::clip::nodes_in_ring`) — otherwise the test verifies that the
/// function agrees with itself. The cloud is **not** symmetric
/// with respect to the ring, on purpose: a symmetric set
/// would let a test through that inverted `inside`/`outside`. To break:
/// invert the sense of the `nodes_in_ring(..., true)` filter into
/// `nodes_in_ring(..., false)` in `estimate::ring_keep` would keep the ring
/// instead of the rest — this test detects it because the expected count is
/// computed without ever sharing code with the implementation.
#[test]
fn an_inner_ring_removes_exactly_the_samples_it_should() {
    fn inside(polygon: &[[f64; 2]], p: [f64; 2]) -> bool {
        let mut odd = false;
        let n = polygon.len();
        for k in 0..n {
            let a = polygon[k];
            let b = polygon[(k + 1) % n];
            if (a[1] > p[1]) != (b[1] > p[1]) {
                let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
                if p[0] < x {
                    odd = !odd;
                }
            }
        }
        odd
    }
    // An off-centre ring, deliberately **not** aligned with the sample
    // grid's own integer coordinates (0..9, step 1): every corner and edge
    // sits at a `.3`, so no sample ever lands exactly on the boundary, where
    // this hand-rolled check and `clip::inside_ring`'s own "on the edge
    // counts as inside" rule could disagree for reasons that are about
    // tie-breaking, not correctness.
    let ring: Vec<[f64; 2]> = vec![[1.3, 1.3], [5.3, 1.3], [5.3, 3.3], [1.3, 3.3]];
    let plane = |x: f64, y: f64| 1.0 + x + y;
    let samples_geom = plane_cloud(10, 1.0, plane); // (x, y) in 0..9, step 1
    let expected_kept: usize = samples_geom.patches[0]
        .points
        .iter()
        .filter(|p| !inside(&ring, [p[0], p[1]]))
        .count();
    let expected_removed = samples_geom.patches[0].points.len() - expected_kept;
    assert!(
        expected_removed > 0 && expected_removed < samples_geom.patches[0].points.len(),
        "sanity: the ring must cut something and not everything"
    );

    let ring_geom = Geometry {
        kind: "ring",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points: ring.iter().map(|&[x, y]| [x, y, 0.0]).collect(),
            topology: Topology::Polylines(vec![4]),
            closed: Vec::new(),
        }],
    };
    let at = Geometry {
        kind: "cloud",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points: vec![[8.0, 8.0, 0.0]],
            topology: Topology::Points,
            closed: Vec::new(),
        }],
    };
    let pieces = Pieces::empty()
        .with(
            "samples",
            Source::Shape {
                geometry: samples_geom,
                closed: None,
            },
        )
        .with(
            "samples (object)",
            known(
                "f0f0f0f0-0000-4000-8000-000000000005",
                "resqml22.PointSetRepresentation",
                "ringed",
            ),
        )
        .with(
            "at",
            Source::Shape {
                geometry: at,
                closed: None,
            },
        )
        .with(
            "inner",
            Source::Shape {
                geometry: ring_geom,
                closed: Some(vec![true]),
            },
        );
    let made = plan_of(
        "estimate",
        &pieces,
        Choices::new()
            .set("title", "ring-filtered")
            .set("spacing", 1.0)
            .set("margin", 1.0),
    )
    .expect("planned");
    let said = made
        .says
        .iter()
        .find(|n| n.key == "ring-filtered")
        .expect("the removal is said");
    assert!(
        said.detail.starts_with(&format!("{expected_removed} sample(s)")),
        "expected {expected_removed}: {}",
        said.detail
    );
}

/// **Given a field, `estimate` estimates the field — not the samples' z.**
///
/// The first version fitted the samples' local height in the world frame
/// too, i.e. their elevation even when a field was given (caught by the
/// compiler's unused-`fit_values` warning, 29/09). The samples lie on a
/// sloping plane and carry a constant field of 7: a result that followed
/// the slope would be the elevation. To break: fit `to_local(p)[2]` again in
/// the world frame.
#[test]
fn estimate_with_a_field_estimates_the_field() {
    let slope = |x: f64, y: f64| 100.0 + 3.0 * x - 2.0 * y;
    let samples = plane_cloud(6, 2.0, slope);
    let count = 36;
    let at_points: Vec<[f64; 3]> = vec![[3.0, 3.0, 0.0], [7.0, 1.0, 0.0]];
    let at = Geometry {
        kind: "cloud",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points: at_points.clone(),
            topology: Topology::Points,
            closed: Vec::new(),
        }],
    };
    let pieces = Pieces::empty()
        .with(
            "samples",
            Source::Shape {
                geometry: samples,
                closed: None,
            },
        )
        .with(
            "samples (object)",
            known(
                "e0e0e0e0-0000-4000-8000-000000000005",
                "resqml22.PointSetRepresentation",
                "seeds",
            ),
        )
        .with(
            "field",
            Source::Field {
                over: "samples".into(),
                on: "nodes".into(),
                values: vec![7.0; count],
                uom: None,
            },
        )
        .with(
            "at",
            Source::Shape {
                geometry: at,
                closed: None,
            },
        );
    let made = plan_of(
        "estimate",
        &pieces,
        Choices::new()
            .set("title", "field-estimated")
            .set("spacing", 1.0)
            .set("margin", 1.0),
    )
    .expect("planned");
    let Numbers::F64(flat) = &made.datasets[0].values else {
        panic!("points are doubles")
    };
    for (rank, p) in at_points.iter().enumerate() {
        let got = flat[rank * 3 + 2];
        assert!(
            (got - 7.0).abs() < 1e-6,
            "query {rank}: {got} — the field is 7, the elevation there is {}",
            slope(p[0], p[1])
        );
    }
}

/// **Rings cut the grid too, not only the samples.**
/// The report: an outerring was tried […] and the grid overflows all the same —
/// `outer`/`inner` only filtered the scatter, and the rectangle
/// covered everything. A node outside `outer` or inside an `inner` must come out NaN,
/// the others not. Verified by an even-odd loop written here, never
/// `clip::inside_ring`, and on non-symmetric rings with corners at `.3`
/// (no node on an edge). To break: remove the call to `mask_by_rings` in
/// `plan_fitted` — all the nodes outside the ring come back finite.
#[test]
fn the_rings_cut_the_grids_nodes_too() {
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
    let ring_shape = |ring: &[[f64; 2]], kind: &'static str| Geometry {
        kind,
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points: ring.iter().map(|&[x, y]| [x, y, 0.0]).collect(),
            topology: Topology::Polylines(vec![ring.len() as u32]),
            closed: Vec::new(),
        }],
    };
    let outer: Vec<[f64; 2]> = vec![[0.3, 0.3], [8.3, 1.3], [7.3, 8.3], [1.3, 6.3]];
    let inner: Vec<[f64; 2]> = vec![[3.3, 3.3], [5.3, 3.3], [4.3, 5.3]];
    let pieces = Pieces::empty()
        .with(
            "shape",
            Source::Shape {
                geometry: plane_cloud(10, 1.0, |x, y| 1.0 + x + 2.0 * y),
                closed: None,
            },
        )
        .with(
            "shape (object)",
            known(
                "a1a1a1a1-0000-4000-8000-000000000006",
                "resqml22.PointSetRepresentation",
                "ringed",
            ),
        )
        .with(
            "outer",
            Source::Shape {
                geometry: ring_shape(&outer, "outer"),
                closed: Some(vec![true]),
            },
        )
        .with(
            "inner",
            Source::Shape {
                geometry: ring_shape(&inner, "inner"),
                closed: Some(vec![true]),
            },
        );
    let choices = Choices::new()
        .set("method", "mba")
        .set("title", "ring-cut")
        .set("spacing", 1.0)
        .set("margin", 0.0);
    let made = plan_of("interpolate", &pieces, choices).expect("planned");

    let written = &made.of_role("representation").next().expect("a grid").json;
    let support = &written["Geometry"]["Points"]["SupportingGeometry"];
    let (x0, y0) = (
        support["Origin"]["Coordinate1"].as_f64().unwrap(),
        support["Origin"]["Coordinate2"].as_f64().unwrap(),
    );
    let ni = written["FastestAxisCount"].as_u64().unwrap() as usize;
    let nj = written["SlowestAxisCount"].as_u64().unwrap() as usize;
    let [slow, fast] = [&support["Dimension"][0], &support["Dimension"][1]];
    let (dx, dy) = (
        fast["Spacing"]["Value"].as_f64().unwrap(),
        slow["Spacing"]["Value"].as_f64().unwrap(),
    );
    let Numbers::F64(z) = &made.datasets[0].values else {
        panic!("heights are doubles")
    };
    assert_eq!(z.len(), ni * nj);

    let mut cut = 0;
    for j in 0..nj {
        for i in 0..ni {
            let p = [x0 + i as f64 * dx, y0 + j as f64 * dy];
            let kept = inside(&outer, p) && !inside(&inner, p);
            let value = z[j * ni + i];
            assert_eq!(value.is_finite(), kept, "node ({i}, {j}) at {p:?}: {value}");
            cut += usize::from(!kept);
        }
    }
    assert!(
        cut > 0 && cut < ni * nj,
        "sanity: the rings must cut something and not everything"
    );
    let said = made
        .says
        .iter()
        .find(|n| n.key == "ring-cut")
        .expect("the cut is said");
    assert!(
        said.detail.starts_with(&format!("{cut} node(s)")),
        "expected {cut}: {}",
        said.detail
    );
}
