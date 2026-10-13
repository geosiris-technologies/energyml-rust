//! The wireframe addresses the mesh the caller is drawing, and not another one.
//!
//! # The bug this pins
//!
//! Reported on 04/09 as *IJK grids with edges shooting off towards the origin*
//! in the 3D view, on `h+g_PaleoGTM.epc`. The reading was never the problem: a
//! viewer draws the faces from `drawable()` and, until this was written, the
//! wireframe from `as_shape(Lines).drawable()` — and **those two do not share a
//! point list**.
//!
//! They diverge for one reason, and it is a rule this crate applies on purpose:
//! a node of a *volume* that no cell names is not a location. RESQML's
//! `CellGeometryIsDefined` marks cells undefined while their coordinates stay
//! perfectly finite in the file, so `drawable()` turns those orphans into holes
//! and drops them. Reshape to segments first and the volume topology is gone:
//! nothing is an orphan of a segment list, every node survives, and every index
//! after the first hole means a different vertex.
//!
//! On PaleoGTM that was 37 772 points against the mesh's 36 363, and 156 099
//! wireframe indices addressing vertices that did not exist. WebGL does not
//! report that — it reads whatever the buffer holds — so the edges simply landed
//! somewhere, which is the failure this workspace likes least: a wrong picture
//! with no message.
//!
//! # What these tests would take to break
//!
//! Reshaping without holing the orphans first. The counts below are chosen so
//! that the two roads disagree: a cell whose flag is off leaves a node that no
//! surviving cell names, and it is that node the two point lists differ by.

use energyml_geometry::geometry::{Geometry, Patch, Topology};
use energyml_geometry::shape::Shape;

fn patch(points: Vec<[f64; 3]>, topology: Topology) -> Patch {
    Patch {
        index: 0,
        node_offset: 0,
        points,
        topology,
        closed: Vec::new(),
    }
}

fn geometry(patches: Vec<Patch>) -> Geometry {
    Geometry {
        kind: "test",
        patches,
    }
}

/// Two cubes side by side, sharing a face, with one node of the second used by
/// nothing once its cell is gone.
fn two_cells(keep_second: bool) -> Geometry {
    // Twelve nodes: two stacked layers of six, so two hexahedra in a row.
    let mut points = Vec::new();
    for z in [0.0_f64, 1.0] {
        for x in [0.0_f64, 1.0, 2.0] {
            for y in [0.0_f64, 1.0] {
                points.push([x, y, z]);
            }
        }
    }
    // Corners of a cell, in the order `Topology::Hexahedra` expects: the four
    // of the bottom layer then the four above them.
    let cell = |left: u32| -> [u32; 8] {
        [
            left,
            left + 1,
            left + 3,
            left + 2,
            left + 6,
            left + 7,
            left + 9,
            left + 8,
        ]
    };
    let mut cells = vec![cell(0)];
    if keep_second {
        cells.push(cell(2));
    }
    // `dimensions` is what the cells were built from; two cells in a row.
    let dimensions = [if keep_second { 2 } else { 1 }, 1, 1];
    geometry(vec![patch(
        points,
        Topology::Hexahedra {
            cells,
            dimensions,
            index: Vec::new(),
        },
    )])
}

#[test]
fn every_edge_addresses_a_vertex_the_mesh_has() {
    // The whole point, in one assertion. An index past the end is not an error
    // any viewer reports — WebGL reads what is there — so it has to be caught
    // here or not at all.
    for keep in [true, false] {
        let g = two_cells(keep);
        let drawn = g.drawable();
        let vertices = drawn.points.len();
        let wire = g.wireframe().expect("a hexahedral grid has cell edges");
        assert!(!wire.is_empty(), "a grid of cubes has edges");
        for [from, to] in &wire {
            assert!(
                (*from as usize) < vertices && (*to as usize) < vertices,
                "edge {from}-{to} addresses {vertices} vertices (second cell kept: {keep})"
            );
        }
    }
}

#[test]
fn dropping_a_cell_drops_the_nodes_only_it_named_from_both() {
    // The case the two roads disagreed on. With the second cell gone, four of
    // the twelve nodes are named by nothing: `drawable()` holes and drops them,
    // and the wireframe has to lose them too — otherwise its indices are the
    // *old* numbering and every one of them is wrong by four.
    let whole = two_cells(true);
    let cut = two_cells(false);

    assert_eq!(whole.drawable().points.len(), 12, "every node is named by a cell");
    assert_eq!(
        cut.drawable().points.len(),
        8,
        "four nodes were named by the cell that went"
    );

    let wire = cut.wireframe().expect("edges");
    let vertices = cut.drawable().points.len();
    assert!(
        wire.iter().flatten().all(|index| (*index as usize) < vertices),
        "the wireframe still numbers the nodes that were dropped"
    );

    // And the counter-proof, which is the road that shipped: reshaping first
    // keeps all twelve, so an index of 11 is produced for a mesh of 8.
    let reshaped = cut.as_shape(Shape::Lines).expect("reshapes").drawable();
    assert_eq!(reshaped.points.len(), 12, "this is what the old road addressed");
    assert!(
        reshaped
            .lines
            .iter()
            .flatten()
            .any(|index| (*index as usize) >= vertices),
        "if this no longer overflows, the two roads have converged and this test is moot"
    );
}

#[test]
fn a_grid_with_nothing_undefined_is_unchanged() {
    // The ordinary package must not pay for the rule: with every node named,
    // the two roads describe the same edges.
    //
    // **As sets**, and that is not laziness: `reshape` de-duplicates the edges
    // through a hash set, so the order it hands them back in is not stable from
    // one run to the next. Comparing the lists would give a test that fails on
    // a Tuesday. Nothing downstream depends on the order — an edge list is
    // drawn, not read — so the set is the whole claim.
    let g = two_cells(true);
    let edges = |list: Vec<[u32; 2]>| -> std::collections::BTreeSet<(u32, u32)> {
        list.into_iter()
            .map(|[a, b]| if a <= b { (a, b) } else { (b, a) })
            .collect()
    };
    let wire = edges(g.wireframe().expect("edges"));
    let reshaped = edges(g.as_shape(Shape::Lines).expect("reshapes").drawable().lines);
    assert_eq!(wire, reshaped);
    assert!(!wire.is_empty(), "two cubes have edges");
}
