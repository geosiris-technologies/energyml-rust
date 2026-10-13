//! `subrepresentation` — a `SubRepresentation` of one representation of the
//! package.
//!
//! The form of the assertions comes from the TRG and the guide: *“An ordered
//! list of indexable elements”* of *“an existing representation”* (RESQML
//! TRG 2.2 §8.9 p. 246), each patch *“drawn from the supporting
//! representation”* it names (§8.10 p. 247); an IJK grid's cells are `NI × NJ
//! × NK`, numbered `i` fastest (RESQML Technical Usage Guide 2.2, draft, Table
//! 7 and §9.12); indices below the number of cells. The shapes are the
//! tests' own. What the geometry reader draws of a written sub-representation
//! is confronted in `tests/blocked_wellbore.rs`, where one is written to a
//! package with its HDF5, reopened and drawn.
//!
//! # What would have to break to make it turn red
//!
//! Sorting or de-duplicating the indices; counting the defined cells instead of the
//! declared cells, or the drawn nodes instead of the nodes of the lattice;
//! numbering the `k` box fastest; drawing a cell by its position
//! rather than by its RESQML number; accepting an artefact as a support.

use serde_json::json;

use energyml_build::operation::{Choices, Identity, Numbers, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When {
        creation: "2026-09-25T00:00:00Z".into(),
        originator: "test".into(),
    }
}

/// A 3 × 2 × 2 IJK grid of unit boxes, cell `(i, j, k)` at `[i, i+1] × [j,
/// j+1] × [−k−1, −k]`, drawn as the reader draws it — `index` the RESQML
/// numbers of the cells kept when one is undefined.
fn boxes(undefined: Option<u32>) -> Geometry {
    let (ni, nj, nk) = (3usize, 2usize, 2usize);
    let mut points = Vec::new();
    for level in 0..=nk {
        for j in 0..=nj {
            for i in 0..=ni {
                points.push([i as f64, j as f64, -(level as f64)]);
            }
        }
    }
    let node = |level: usize, i: usize, j: usize| (level * (nj + 1) * (ni + 1) + j * (ni + 1) + i) as u32;
    let mut cells = Vec::new();
    let mut index = Vec::new();
    for k in 0..nk {
        for j in 0..nj {
            for i in 0..ni {
                let number = (i + ni * (j + nj * k)) as u32;
                if Some(number) == undefined {
                    continue;
                }
                let around = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
                let mut cell = [0u32; 8];
                for (at, (a, b)) in around.iter().enumerate() {
                    cell[at] = node(k, *a, *b);
                    cell[at + 4] = node(k + 1, *a, *b);
                }
                cells.push(cell);
                index.push(number);
            }
        }
    }
    if undefined.is_none() {
        index.clear();
    }
    Geometry {
        kind: "grid",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points,
            topology: Topology::Hexahedra {
                cells,
                dimensions: [ni, nj, nk],
                index,
            },
            closed: Vec::new(),
        }],
    }
}

/// The cell `(i, j, k)` of `boxes` holding a point, by arithmetic.
fn cell_at(p: [f64; 3]) -> u64 {
    let (i, j, k) = (p[0].floor() as u64, p[1].floor() as u64, (-p[2]).floor() as u64);
    i + 3 * (j + 2 * k)
}

fn support() -> Ref {
    Ref::new(
        "50bb0000-0000-4000-8000-000000000001",
        "resqml22.IjkGridRepresentation",
        "The grid",
    )
}

fn bag(geometry: Geometry) -> Pieces {
    Pieces::empty()
        .with(
            "support",
            Source::Shape {
                geometry,
                closed: None,
            },
        )
        .with(
            "support (object)",
            Source::Object(Known {
                reference: Some(support()),
                ..Default::default()
            }),
        )
}

fn planned(pieces: &Pieces, choices: &Choices) -> Result<Plan, String> {
    Registry::new()
        .plan(
            "subrepresentation",
            pieces,
            choices,
            &Identity::derived(),
            &when(),
        )
        .map_err(|why| why.message())
}

fn written(plan: &Plan) -> Vec<i64> {
    match &plan.datasets[0].values {
        Numbers::I64(v) => v.clone(),
        Numbers::F64(_) => panic!("indices are whole"),
    }
}

fn the_object(plan: &Plan) -> &serde_json::Value {
    &plan
        .of_role("representation")
        .next()
        .expect("the sub-representation")
        .json
}

/// The centre of each cell the plan draws.
fn centres(plan: &Plan) -> Vec<[f64; 3]> {
    let [patch] = plan.shapes[0].geometry.patches.as_slice() else {
        panic!("one patch")
    };
    let Topology::Hexahedra { cells, .. } = &patch.topology else {
        panic!("cells: {:?}", patch.topology)
    };
    cells
        .iter()
        .map(|cell| {
            std::array::from_fn(|axis| {
                cell.iter().map(|n| patch.points[*n as usize][axis]).sum::<f64>() / 8.0
            })
        })
        .collect()
}

/// **Given indices are written as given** — order kept, the element named on
/// the object, the support on the patch — and the cells drawn are those cells.
#[test]
fn given_indices_are_written_in_order_and_drawn_as_those_cells() {
    let choices = Choices::new()
        .set("element", "cells")
        .set("indices", json!([7, 0, 11]))
        .set("title", "Three cells");
    let plan = planned(&bag(boxes(None)), &choices).unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(written(&plan), vec![7, 0, 11]);
    let object = the_object(&plan);
    assert_eq!(object["$type"], "resqml22.SubRepresentation");
    assert_eq!(object["IndexableElement"], "cells");
    let patch = &object["SubRepresentationPatch"][0];
    assert_eq!(patch["SupportingRepresentation"]["Uuid"], json!(support().uuid));
    assert_eq!(
        patch["Indices"]["Values"]["ExternalDataArrayPart"][0]["Count"],
        json!([3])
    );
    let drawn: Vec<u64> = centres(&plan).into_iter().map(cell_at).collect();
    assert_eq!(drawn, vec![7, 0, 11]);
}

/// **The box is the productive reservoir's rule**: the lower layer, `k = 1`, is cells 6 to 11 in
/// RESQML order — `i` fastest — and two ranges cut it further.
#[test]
fn an_ijk_box_is_its_cells_in_resqml_order() {
    let lower = planned(
        &bag(boxes(None)),
        &Choices::new()
            .set("select", "ijk-box")
            .set("kFirst", 1)
            .set("title", "Lower"),
    )
    .unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(written(&lower), (6..12).collect::<Vec<i64>>());
    assert_eq!(the_object(&lower)["IndexableElement"], "cells");
    let corner = planned(
        &bag(boxes(None)),
        &Choices::new()
            .set("select", "ijk-box")
            .set("iFirst", 1)
            .set("iLast", 2)
            .set("jFirst", 1)
            .set("title", "Corner"),
    )
    .unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(written(&corner), vec![4, 5, 10, 11]);
    let drawn: Vec<u64> = centres(&corner).into_iter().map(cell_at).collect();
    assert_eq!(drawn, vec![4, 5, 10, 11]);
    let why = planned(
        &bag(boxes(None)),
        &Choices::new()
            .set("select", "ijk-box")
            .set("kLast", 2)
            .set("title", "Past"),
    )
    .unwrap_err();
    assert!(
        why.contains("`kLast`") && why.contains("2 cell(s) along k"),
        "{why}"
    );
}

/// **The count is the support's, declared**: a grid with an undefined
/// cell still has twelve; an index at twelve is refused with the count, an
/// index twice naming both places. The undefined cell chosen is written, said
/// and not drawn.
#[test]
fn an_index_past_the_support_or_twice_is_refused_by_name() {
    let holed = boxes(Some(4));
    let why = planned(
        &bag(holed.clone()),
        &Choices::new()
            .set("element", "cells")
            .set("indices", json!([3, 12]))
            .set("title", "t"),
    )
    .unwrap_err();
    assert!(
        why.contains("`indices`") && why.contains("12 is no index") && why.contains("12 cells"),
        "{why}"
    );
    let why = planned(
        &bag(holed.clone()),
        &Choices::new()
            .set("element", "cells")
            .set("indices", json!([3, 5, 3]))
            .set("title", "t"),
    )
    .unwrap_err();
    assert!(why.contains("places 1 and 3"), "{why}");

    let plan = planned(
        &bag(holed),
        &Choices::new()
            .set("element", "cells")
            .set("indices", json!([3, 4, 5, 11]))
            .set("title", "t"),
    )
    .unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(
        written(&plan),
        vec![3, 4, 5, 11],
        "the undefined cell is in range, and written"
    );
    assert!(
        plan.says.iter().any(|note| note.key == "undefined-cells"),
        "{:?}",
        plan.says
    );
    // Drawn by its RESQML number, not its position: 5 is the fifth drawn.
    let drawn: Vec<u64> = centres(&plan).into_iter().map(cell_at).collect();
    assert_eq!(drawn, vec![3, 5, 11]);
}

/// **Nodes and triangles** are counted on the support's shape: a 2D grid its
/// declared lattice, a surface its triangles across patches; a triangle of a
/// grid is refused by name.
#[test]
fn nodes_and_triangles_are_counted_on_the_support() {
    let surface = Geometry {
        kind: "surface",
        patches: vec![
            Patch {
                index: 0,
                node_offset: 0,
                points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                topology: Topology::Triangles(vec![[0, 1, 2]]),
                closed: Vec::new(),
            },
            Patch {
                index: 1,
                node_offset: 3,
                points: vec![[5.0, 0.0, 0.0], [6.0, 0.0, 0.0], [5.0, 1.0, 0.0], [6.0, 1.0, 0.0]],
                topology: Topology::Triangles(vec![[0, 1, 2], [1, 3, 2]]),
                closed: Vec::new(),
            },
        ],
    };
    let plan = planned(
        &bag(surface.clone()),
        &Choices::new()
            .set("element", "triangles")
            .set("indices", json!([2]))
            .set("title", "t"),
    )
    .unwrap_or_else(|why| panic!("{why}"));
    let [patch] = plan.shapes[0].geometry.patches.as_slice() else {
        panic!()
    };
    assert_eq!(
        patch.points,
        vec![[6.0, 0.0, 0.0], [6.0, 1.0, 0.0], [5.0, 1.0, 0.0]],
        "the second triangle of the second patch"
    );
    let why = planned(
        &bag(surface.clone()),
        &Choices::new()
            .set("element", "triangles")
            .set("indices", json!([3]))
            .set("title", "t"),
    )
    .unwrap_err();
    assert!(why.contains("3 triangles"), "{why}");
    let plan = planned(
        &bag(surface),
        &Choices::new()
            .set("element", "nodes")
            .set("indices", json!([6, 0]))
            .set("title", "t"),
    )
    .unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(
        plan.shapes[0].geometry.patches[0].points,
        vec![[6.0, 1.0, 0.0], [0.0, 0.0, 0.0]]
    );
    let why = planned(
        &bag(boxes(None)),
        &Choices::new()
            .set("element", "triangles")
            .set("indices", json!([0]))
            .set("title", "t"),
    )
    .unwrap_err();
    assert!(why.contains("no triangle"), "{why}");
    // 36 nodes: three levels of 4 × 3 pillars.
    let why = planned(
        &bag(boxes(None)),
        &Choices::new()
            .set("element", "nodes")
            .set("indices", json!([36]))
            .set("title", "t"),
    )
    .unwrap_err();
    assert!(why.contains("36 nodes"), "{why}");
}

/// **An artefact is no support**: a sub-representation names its support by
/// uuid. Unfit before any plan.
#[test]
fn an_artefact_is_no_support() {
    let pieces = Pieces::empty().with(
        "support",
        Source::Shape {
            geometry: boxes(None),
            closed: None,
        },
    );
    let why = planned(
        &pieces,
        &Choices::new()
            .set("element", "cells")
            .set("indices", json!([0]))
            .set("title", "t"),
    )
    .unwrap_err();
    assert!(
        why.contains("`support`") && why.contains("no object of the package"),
        "{why}"
    );
}

/// **Another selection is another object**; the same is the same.
#[test]
fn the_same_selection_is_the_same_object() {
    let uuid = |indices: serde_json::Value| {
        the_object(
            &planned(
                &bag(boxes(None)),
                &Choices::new()
                    .set("element", "cells")
                    .set("indices", indices)
                    .set("title", "t"),
            )
            .unwrap(),
        )["uuid"]
            .clone()
    };
    assert_eq!(uuid(json!([1, 2])), uuid(json!([1, 2])));
    assert_ne!(uuid(json!([1, 2])), uuid(json!([2, 1])));
}
