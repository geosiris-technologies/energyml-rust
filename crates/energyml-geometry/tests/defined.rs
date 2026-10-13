//! Nodes with no position, and what happens to what was indexed on them.
//!
//! The bug these pin is not "the output looks odd". It is that a `Grid2d` with
//! null Z values wrote `NaN` into a `.geojson`, and **`NaN` is not a JSON
//! value** - one null among 34 000 positions made the whole file fail
//! `JSON.parse`. So the rule is: a node that has no position is not a node, and
//! nothing that indexes it survives either.
//!
//! What the standard does and does not say about those nulls is in the
//! documentation of `energyml_geometry::defined`. It is worth reading before
//! changing anything here: the NaN rule is a **reader's decision**, while the
//! two `…IsDefined` flags are the standard's own, and they are not the same
//! kind of claim.

use energyml_geometry::defined::is_defined;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

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

#[test]
fn a_geometry_with_every_node_placed_is_left_alone() {
    // `None`, not a clone: the overwhelmingly common package must pay one pass
    // and no copy for a rule that exists for the others.
    let g = geometry(vec![patch(
        vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        Topology::Triangles(vec![[0, 1, 2]]),
    )]);
    assert_eq!(g.undefined_count(), 0);
    assert!(
        g.defined_only().is_none(),
        "a geometry with nothing to drop should not be rebuilt"
    );
}

#[test]
fn infinity_counts_as_no_position_too() {
    // Not a null the standard defines, but equally undrawable: a viewer handed
    // one clips the scene to nothing or refuses the file.
    assert!(!is_defined(&[0.0, 0.0, f64::INFINITY]));
    assert!(!is_defined(&[f64::NEG_INFINITY, 0.0, 0.0]));
    assert!(!is_defined(&[0.0, f64::NAN, 0.0]));
    assert!(is_defined(&[0.0, 0.0, 0.0]));
}

#[test]
fn a_triangle_touching_an_undefined_node_goes_with_it() {
    let g = geometry(vec![patch(
        vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, f64::NAN],
        ],
        // The first triangle is whole; the second leans on the null.
        Topology::Triangles(vec![[0, 1, 2], [1, 2, 3]]),
    )]);

    let c = g.defined_only().expect("there is a node to drop");
    assert_eq!(c.dropped_points, 1);
    assert_eq!(c.dropped_cells, 1);
    assert_eq!(c.geometry.point_count(), 3);
    match &c.geometry.patches[0].topology {
        Topology::Triangles(t) => assert_eq!(t, &vec![[0, 1, 2]]),
        other => panic!("topology should stay triangles, got {other:?}"),
    }
}

#[test]
fn the_survivors_are_renumbered_and_not_merely_filtered() {
    // The failure this forbids is subtle and total: drop point 0 and leave the
    // indices alone, and every triangle now names its neighbour's corner. The
    // mesh still draws, with the right number of triangles, in roughly the
    // right place - the worst kind of wrong.
    let g = geometry(vec![patch(
        vec![
            [9.0, 9.0, f64::NAN],
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        ],
        Topology::Triangles(vec![[1, 2, 3]]),
    )]);

    let c = g.defined_only().expect("there is a node to drop");
    match &c.geometry.patches[0].topology {
        // Was [1,2,3]; the dropped point 0 shifts every survivor down one.
        Topology::Triangles(t) => assert_eq!(t, &vec![[0, 1, 2]]),
        other => panic!("expected triangles, got {other:?}"),
    }
    assert_eq!(c.geometry.patches[0].points[0], [0.0, 0.0, 0.0]);
}

#[test]
fn a_lattice_with_a_hole_stops_calling_itself_a_lattice() {
    // `Topology::Grid` promises a fastest × slowest rectangle, row by row. Drop
    // one node and every index after it means something else, so the promise
    // has to be withdrawn rather than quietly broken. This is the 2D-grid case
    // the Volve horizons hit: 34 232 of 293 664 nodes with no Z.
    let mut points = vec![[0.0, 0.0, 0.0]; 9];
    points[4] = [0.0, 0.0, f64::NAN];
    let g = geometry(vec![patch(
        points,
        Topology::Grid {
            fastest: 3,
            slowest: 3,
        },
    )]);

    let c = g.defined_only().expect("there is a node to drop");
    assert_eq!(c.geometry.point_count(), 8);
    assert!(
        matches!(c.geometry.patches[0].topology, Topology::Points),
        "a grid that lost a node must not still claim to be a grid, got {:?}",
        c.geometry.patches[0].topology
    );
    // 2 × 2 quads, none of them addressable any more.
    assert_eq!(c.dropped_cells, 4);
}

#[test]
fn a_polyline_is_cut_at_the_hole_and_not_bridged_across_it() {
    // Joining the two survivors would draw a segment the file never described
    // - and one that looks entirely plausible on screen, which is why it is
    // worth a test of its own.
    let g = geometry(vec![patch(
        vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [2.0, 0.0, f64::NAN],
            [3.0, 0.0, 0.0],
            [4.0, 0.0, 0.0],
        ],
        Topology::Polylines(vec![5]),
    )]);

    let c = g.defined_only().expect("there is a node to drop");
    match &c.geometry.patches[0].topology {
        Topology::Polylines(runs) => assert_eq!(
            runs,
            &vec![2, 2],
            "one run through a hole must become two, not one of four"
        ),
        other => panic!("expected polylines, got {other:?}"),
    }
}

#[test]
fn a_hexahedron_touching_an_undefined_pillar_goes_with_it() {
    let mut points: Vec<[f64; 3]> = (0..16).map(|i| [i as f64, 0.0, 0.0]).collect();
    points[15] = [0.0, 0.0, f64::NAN];
    let g = geometry(vec![patch(
        points,
        Topology::Hexahedra {
            cells: vec![[0, 1, 2, 3, 4, 5, 6, 7], [8, 9, 10, 11, 12, 13, 14, 15]],
            dimensions: [2, 1, 1],
            index: Vec::new(),
        },
    )]);

    let c = g.defined_only().expect("there is a node to drop");
    match &c.geometry.patches[0].topology {
        Topology::Hexahedra {
            cells, dimensions, ..
        } => {
            assert_eq!(cells.len(), 1);
            assert_eq!(cells[0], [0, 1, 2, 3, 4, 5, 6, 7]);
            // Kept as it was: it says what the cells were *built from*, and
            // that stays true. It is not the count of what remains.
            assert_eq!(*dimensions, [2, 1, 1]);
        }
        other => panic!("expected hexahedra, got {other:?}"),
    }
    assert_eq!(c.dropped_cells, 1);
}

#[test]
fn a_second_patch_is_renumbered_after_the_first_one_shrank() {
    // `node_offset` is what anything indexing *nodes of the representation* -
    // a subrepresentation, a property - counts from. Leaving the original there
    // after the preceding patch lost a node sends every one of them into the
    // wrong patch.
    let g = geometry(vec![
        Patch {
            index: 0,
            node_offset: 0,
            points: vec![[0.0, 0.0, 0.0], [1.0, 1.0, f64::NAN]],
            topology: Topology::Points,
            closed: Vec::new(),
        },
        Patch {
            index: 1,
            node_offset: 2,
            points: vec![[2.0, 0.0, 0.0], [3.0, 0.0, 0.0]],
            topology: Topology::Points,
            closed: Vec::new(),
        },
    ]);

    let c = g.defined_only().expect("there is a node to drop");
    assert_eq!(c.geometry.patches[0].node_offset, 0);
    assert_eq!(
        c.geometry.patches[1].node_offset, 1,
        "the second patch must start where the first now ends"
    );
}

#[test]
fn the_point_map_says_which_survivors_are_which() {
    // This map is what carries a property's values across the compaction. Get
    // it wrong and the survivors are coloured with their neighbours' numbers -
    // the failure that looks right.
    let g = geometry(vec![patch(
        vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, f64::NAN],
            [2.0, 0.0, 0.0],
            [3.0, 0.0, f64::NAN],
            [4.0, 0.0, 0.0],
        ],
        Topology::Points,
    )]);

    let c = g.defined_only().expect("there are nodes to drop");
    assert_eq!(c.points, vec![0, 2, 4]);
    assert_eq!(c.dropped_points, 2);
}

/// A cell the grid says is undefined takes its **number** out of the list too.
///
/// The trap this guards is silent and it only appears on the grids that matter.
/// `cells` is compacted - a cell whose flag marks it undefined, or whose pillars
/// carry no location, is not pushed - so a position in it stops being the number
/// RESQML calls the cell as soon as a grid has one hole. Anything addressing a
/// cell **by the file's number** would then be right on an unbroken grid and
/// wrong on exactly the faulted and eroded ones: PaleoGTM has 3 647 undefined
/// cells out of 33 048, and a grid connection set names its pairs that way.
#[test]
fn a_dropped_cell_takes_its_number_with_it() {
    use energyml_geometry::Topology;

    // Three cells in a row, the middle one undefined: the survivors are the
    // file's cells 0 and 2, at positions 0 and 1.
    let topology = Topology::Hexahedra {
        cells: vec![[0, 1, 2, 3, 4, 5, 6, 7], [8, 9, 10, 11, 12, 13, 14, 15]],
        dimensions: [3, 1, 1],
        index: vec![0, 2],
    };

    assert_eq!(topology.cell_at(0), Some(0));
    assert_eq!(
        topology.cell_at(1),
        Some(2),
        "the second survivor is the file's cell 2"
    );
    assert_eq!(topology.cell_at(2), None, "there is no third cell");

    assert_eq!(topology.position_of(0), Some(0));
    assert_eq!(topology.position_of(2), Some(1));
    // **The answer for the hole is `None`, not a neighbour.** The file names a
    // cell that has no shape; drawing something for it would draw a place the
    // grid says is not one.
    assert_eq!(topology.position_of(1), None, "the undefined cell is nowhere");
    assert_eq!(topology.position_of(9), None);
}

/// An empty numbering means the identity, and one place says so.
#[test]
fn nothing_dropped_costs_no_list() {
    use energyml_geometry::Topology;

    // The ordinary case, and the one a grid of ten million cells is in: no list
    // at all rather than a list saying `0, 1, 2, …`.
    let topology = Topology::Hexahedra {
        cells: vec![[0, 1, 2, 3, 4, 5, 6, 7], [8, 9, 10, 11, 12, 13, 14, 15]],
        dimensions: [2, 1, 1],
        index: Vec::new(),
    };

    assert_eq!(topology.cell_at(1), Some(1));
    assert_eq!(topology.position_of(1), Some(1));
    assert_eq!(topology.position_of(2), None);
}
