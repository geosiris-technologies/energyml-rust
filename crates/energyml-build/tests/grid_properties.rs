//! `grid-properties` - a pressure or a temperature by a sourced law of each
//! cell's own vertical depth below the ground, or a porosity, a net-to-gross
//! ratio or a permeability uniform within each K layer, laid on the cells of
//! an `IjkGridRepresentation` (step 8 of the reference model:
//! the grid properties and the base that the property series need).
//!
//! The form of the assertions comes from what the schemas and the reference-model description
//! state: a RESQML cell is numbered `i` fastest, then `j`, then `k` (RESQML
//! Technical Usage Guide 2.2, draft, Table 7 and §9.12; the same order
//! `energyml-geometry`'s reader assembles `Topology::Hexahedra` in); a cell
//! with no defined geometry has no location to read a depth from and is
//! written `NaN` (TRG 2.2 §5.1 p. 94); a value stated per K layer is the same
//! on every cell of it, geometry defined or not (`crates/energyml-build/ARCHITECTURE.md`,
//! *a grid is not reprojected*, applied one door down: a fact about a
//! layer is not a fact about a corner point). The grid, its ground and the
//! coefficients are the test's own.
//!
//! # What would have to break to make it turn red
//!
//! Dividing by the wrong count to find the K layer of a cell (`i +
//! ni·(j + nj·k)` - breaking it into `cell_index % per_layer` turns the layers
//! test red); ignoring `Topology::position_of` and indexing a cell
//! directly by its RESQML number (turns the hole test red, which counts
//! on cell 5; tried and put back); accepting an empty source; accepting a
//! number of layers that does not match `Nk`; accepting a shape that
//! is not an IJK grid.

use serde_json::json;

use energyml_build::operation::{Choices, Identity, Numbers, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When {
        creation: "2026-09-28T00:00:00Z".into(),
        originator: "test".into(),
    }
}

const GRID: &str = "40100000-0000-4000-8000-000000000401";
const FRAME: &str = "cccccccc-0000-4000-8000-000000000001";
const GROUND: &str = "eeeeeeee-0000-4000-8000-000000000005";

fn crs() -> Ref {
    Ref::new(
        FRAME,
        "eml23.LocalEngineeringCompoundCrs",
        "test frame - elevation",
    )
}

/// A 2 × 2 × 2 IJK grid of flat, vertical-pillar boxes, 100 m square cells,
/// elevation top to bottom 1000, 950, 900 m - drawn as the reader draws it
/// (`Topology::position_of`, `index` the RESQML numbers of the cells kept
/// when one is undefined). Cell `(i, j, k)` is number `i + 2·(j + 2·k)`, so
/// cell 5 is `(1, 0, 1)`.
fn grid_geometry(undefined: Option<u32>) -> Geometry {
    let (ni, nj, nk) = (2usize, 2usize, 2usize);
    let zs = [1000.0, 950.0, 900.0];
    let mut points = Vec::new();
    for &z in &zs {
        for j in 0..=nj {
            for i in 0..=ni {
                points.push([i as f64 * 100.0, j as f64 * 100.0, z]);
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
        kind: "resqml22.IjkGridRepresentation",
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

fn grid_known() -> Known {
    Known {
        reference: Some(Ref::new(
            GRID,
            "resqml22.IjkGridRepresentation",
            "Chalk grid test",
        )),
        crs: Some(crs()),
        vertical_uom: Some("m".into()),
        vertical_up: Some(true),
        ..Known::default()
    }
}

/// A flat ground at 1050 m of elevation, covering the whole grid in plan.
fn ground_geometry() -> Geometry {
    let corners = [[-50.0, -50.0], [250.0, -50.0], [250.0, 250.0], [-50.0, 250.0]];
    let points = corners.iter().map(|&[x, y]| [x, y, 1050.0]).collect();
    Geometry {
        kind: "resqml22.TriangulatedSetRepresentation",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points,
            topology: Topology::Triangles(vec![[0, 1, 2], [0, 2, 3]]),
            closed: Vec::new(),
        }],
    }
}

fn ground_known() -> Known {
    Known {
        reference: Some(Ref::new(
            GROUND,
            "resqml22.TriangulatedSetRepresentation",
            "test ground",
        )),
        crs: Some(crs()),
        vertical_uom: Some("m".into()),
        vertical_up: Some(true),
        ..Known::default()
    }
}

fn bag(undefined: Option<u32>, with_ground: bool) -> Pieces {
    let mut out = Pieces::empty()
        .with(
            "grid",
            Source::Shape {
                geometry: grid_geometry(undefined),
                closed: None,
            },
        )
        .with("grid (object)", Source::Object(grid_known()));
    if with_ground {
        out = out
            .with(
                "ground",
                Source::Shape {
                    geometry: ground_geometry(),
                    closed: None,
                },
            )
            .with("ground (object)", Source::Object(ground_known()));
    }
    out
}

fn planned(pieces: &Pieces, choices: &Choices) -> Result<Plan, String> {
    Registry::new()
        .plan("grid-properties", pieces, choices, &Identity::derived(), &when())
        .map_err(|why| why.to_string())
}

/// The values written for the property of that uuid.
fn values_of(plan: &Plan, uuid: &str) -> Vec<f64> {
    let path = format!("/resqml22/{uuid}/values");
    let dataset = plan
        .datasets
        .iter()
        .find(|one| one.path == path)
        .expect("the property's values");
    match &dataset.values {
        Numbers::F64(values) => values.clone(),
        other => panic!("not floating point: {other:?}"),
    }
}

fn recorded_double(plan: &Plan, title: &str) -> f64 {
    plan.activity
        .parameters
        .iter()
        .find_map(|one| match one {
            Parameter::Double { title: t, value, .. } if t == title => Some(*value),
            _ => None,
        })
        .unwrap_or_else(|| panic!("`{title}` is not recorded in the activity"))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * (1.0 + b.abs())
}

fn temperature_choices() -> Choices {
    Choices::new()
        .set("what", "temperature")
        .set("referenceTemperature", 10.0)
        .set("referenceDepth", 0.0)
        .set("gradient", 20.0)
        .set("source", "synthetic: the test's own values")
}

// -- pressure and temperature: a law of each cell's own depth below the ground ---

/// **The core of the operation**: the law is read at each cell's own depth
/// below the ground, in the RESQML order, and a cell with no defined geometry
/// is `NaN` - never invented, never skipped.
#[test]
fn a_temperature_follows_the_cells_own_depth_and_a_cell_with_no_geometry_is_nan() {
    // Cell 5 - (i=1, j=0, k=1) - has no defined geometry.
    let plan = planned(&bag(Some(5), true), &temperature_choices()).unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(plan.objects.len(), 1, "one property for the grid");
    let property = &plan.objects[0];
    assert_eq!(property.json["$type"], "resqml22.ContinuousProperty");
    assert_eq!(property.json["IndexableElement"], "cells");
    assert_eq!(property.json["SupportingRepresentation"]["Uuid"], json!(GRID));
    assert_eq!(property.json["PropertyKind"]["Title"], json!("temperature"));

    let values = values_of(&plan, &property.uuid);
    assert_eq!(values.len(), 8, "one value per cell the grid declares");

    // k = 0: depth 1050 − 975 = 75 m, 10 + 0.02 × 75 = 11.5 °C. Cells 0..4.
    for (cell, value) in values.iter().enumerate().take(4) {
        assert!(close(*value, 11.5), "cell {cell} at k=0: {value} (expected 11.5)");
    }
    // k = 1: depth 1050 − 925 = 125 m, 10 + 0.02 × 125 = 12.5 °C. Cells 4, 6, 7.
    for cell in [4usize, 6, 7] {
        assert!(
            close(values[cell], 12.5),
            "cell {cell} at k=1: {} (expected 12.5)",
            values[cell]
        );
    }
    // Cell 5 has no defined geometry: no depth to read.
    assert!(
        values[5].is_nan(),
        "cell 5 has no defined geometry and no depth to read: {}",
        values[5]
    );

    assert_eq!(recorded_double(&plan, "cellCount"), 8.0);
    assert_eq!(recorded_double(&plan, "undefinedCellCount"), 1.0);
    assert_eq!(recorded_double(&plan, "aboveGroundCount"), 0.0);
    assert_eq!(recorded_double(&plan, "shallowCount"), 0.0);

    // The rule for invented values: synthetic, and it says so.
    assert_eq!(property.json["ExtensionNameValue"][0]["Name"], json!("synthetic"));
    assert!(
        property.json["Citation"]["Description"]
            .as_str()
            .unwrap()
            .contains("Laid by a law")
    );
}

/// A hydrostatic pressure follows the same reading of depth, in bar.
#[test]
fn a_hydrostatic_pressure_follows_the_same_depth() {
    let choices = Choices::new()
        .set("what", "pressure")
        .set("waterLevel", 1050.0)
        .set("source", "synthetic: the test's own values");
    let plan = planned(&bag(None, true), &choices).unwrap_or_else(|why| panic!("{why}"));
    let property = &plan.objects[0];
    assert_eq!(property.json["PropertyKind"]["Title"], json!("pressure"));
    let values = values_of(&plan, &property.uuid);
    // The water table is exactly at the ground: 75 m and 125 m of water,
    // fresh water and standard gravity - about 1 bar per 10 m.
    assert!(close(values[0], 75.0 * 1000.0 * 9.80665 / 1e5), "{}", values[0]);
    assert!(close(values[4], 125.0 * 1000.0 * 9.80665 / 1e5), "{}", values[4]);
}

/// No `ground` is a named refusal, not a guess.
#[test]
fn a_depth_law_without_a_ground_is_refused_by_name() {
    let why = planned(&bag(None, false), &temperature_choices()).unwrap_err();
    assert!(why.contains("ground"), "{why}");
}

// -- porosity, net-to-gross: one value per K layer, geometry or not --------------

/// **The other half of the operation**: a K-layer value does not read the
/// geometry at all - even the cell with no defined geometry gets its layer's
/// value.
#[test]
fn a_porosity_is_the_same_on_every_cell_of_its_k_layer_defined_geometry_or_not() {
    let choices = Choices::new()
        .set("what", "porosity")
        .set("layers", json!([0.2, 0.35]))
        .set("uom", "Euc")
        .set("source", "synthetic: test");
    let plan = planned(&bag(Some(5), false), &choices).unwrap_or_else(|why| panic!("{why}"));
    let property = &plan.objects[0];
    assert_eq!(property.json["PropertyKind"]["Title"], json!("porosity"));
    let values = values_of(&plan, &property.uuid);
    assert_eq!(values.len(), 8);
    for (cell, value) in values.iter().enumerate().take(4) {
        assert!(close(*value, 0.2), "cell {cell} at k=0: {value}");
    }
    for (cell, value) in values.iter().enumerate().skip(4) {
        assert!(
            close(*value, 0.35),
            "cell {cell} at k=1 (5 has no defined geometry): {value}"
        );
    }
    assert_eq!(recorded_double(&plan, "layerCount"), 2.0);
}

#[test]
fn a_net_to_gross_ratio_is_looked_up_by_its_dictionary_title() {
    let choices = Choices::new()
        .set("what", "ntg")
        .set("layers", json!([1.0, 0.8]))
        .set("uom", "Euc")
        .set("source", "synthetic: test");
    let plan = planned(&bag(None, false), &choices).unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(
        plan.objects[0].json["PropertyKind"]["Title"],
        json!("net gross thickness ratio")
    );
}

#[test]
fn a_layer_count_that_does_not_match_the_grids_nk_is_refused_by_name() {
    let choices = Choices::new()
        .set("what", "porosity")
        .set("layers", json!([0.5]))
        .set("uom", "Euc")
        .set("source", "test");
    let why = planned(&bag(None, false), &choices).unwrap_err();
    assert!(why.contains("layers"), "{why}");
}

#[test]
fn a_porosity_outside_zero_one_is_refused_by_name() {
    let choices = Choices::new()
        .set("what", "porosity")
        .set("layers", json!([0.2, 1.4]))
        .set("uom", "Euc")
        .set("source", "test");
    let why = planned(&bag(None, false), &choices).unwrap_err();
    assert!(why.contains("layers"), "{why}");
}

// -- permeability: three scalar properties, one per direction --------------------

#[test]
fn a_permeability_is_written_as_three_scalar_properties_one_per_direction() {
    let choices = Choices::new()
        .set("what", "permeability")
        .set("kx", json!([100.0, 10.0]))
        .set("ky", json!([100.0, 10.0]))
        .set("kz", json!([10.0, 1.0]))
        .set("uom", "mD")
        .set("source", "synthetic: test");
    let plan = planned(&bag(None, false), &choices).unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(plan.objects.len(), 3, "kx, ky, kz - three scalar properties");
    for object in &plan.objects {
        assert_eq!(
            object.json["PropertyKind"]["Title"],
            json!("absolute permeability")
        );
        assert_eq!(object.json["ValueCountPerIndexableElement"], json!([1]));
    }
    let mut facets: Vec<String> = plan
        .objects
        .iter()
        .map(|one| one.json["Facet"][0]["Facet"].as_str().unwrap().to_string())
        .collect();
    facets.sort();
    assert_eq!(facets, vec!["I".to_string(), "J".to_string(), "K".to_string()]);
    // The three uuids differ: not the same property written thrice.
    let mut uuids: Vec<&str> = plan.objects.iter().map(|one| one.uuid.as_str()).collect();
    uuids.sort();
    uuids.dedup();
    assert_eq!(uuids.len(), 3);
}

// -- refusals that do not depend on the law or the layers -------------------------

#[test]
fn an_empty_source_is_refused_by_name() {
    let choices = Choices::new()
        .set("what", "porosity")
        .set("layers", json!([0.5, 0.5]))
        .set("uom", "Euc")
        .set("source", "  ");
    let why = planned(&bag(None, false), &choices).unwrap_err();
    assert!(why.contains("source"), "{why}");
}

#[test]
fn a_shape_that_is_no_ijk_grid_is_refused_by_name() {
    let pieces = Pieces::empty()
        .with(
            "grid",
            Source::Shape {
                geometry: ground_geometry(),
                closed: None,
            },
        )
        .with("grid (object)", Source::Object(grid_known()));
    let choices = Choices::new()
        .set("what", "porosity")
        .set("layers", json!([0.5, 0.5]))
        .set("uom", "Euc")
        .set("source", "test");
    let why = planned(&pieces, &choices).unwrap_err();
    assert!(
        why.to_lowercase().contains("ijk") || why.to_lowercase().contains("hexahedra"),
        "{why}"
    );
}
