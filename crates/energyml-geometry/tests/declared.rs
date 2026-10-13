//! What a representation says about its own size, before anything is read.
//!
//! # Why this exists
//!
//! Listing a package's drawable objects used to evaluate every one of them:
//! `geometry::of` per representation, which decompresses every dataset it
//! touches. Measured on 06/09 through the browser road, on `h+g_PaleoGTM.epc`
//! and its 208 MB of HDF5: **47.9 s and 1 530 dataset reads**, against 302 ms
//! for the reference graph and 85 ms for the inventory. Nothing was drawn at
//! the end of it - the front end holds every row unread until somebody asks - so the
//! whole of it went into filling in numbers.
//!
//! [`geometry::declared_nodes`] is the cheap half of that answer, and the
//! contract it must keep is what this file pins:
//!
//! - it reads **no array**, which is why every test here runs without a
//!   [`ValueSource`] of any kind. A version that quietly needed one would still
//!   pass a test that handed it one;
//! - it answers **`None` rather than zero** where the document says nothing. A
//!   zero is a claim about the contents, and the wrong one;
//! - it is **declared, not drawn**. `corpus.rs` holds the other half - the
//!   confrontation with what the reader actually produces, on real files.
//!
//! [`ValueSource`]: energyml_geometry::source::ValueSource

use energyml_core::registry::DynObject;
use energyml_eml_2_0::{DataObjectReference, Hdf5Dataset};
use energyml_geometry::geometry;
use energyml_resqml_2_0_1::{
    AbstractPoint3dArray, AnyPointGeometry, Grid2dPatch, Grid2dRepresentation, NodePatch, Point3dHdf5Array,
    PointGeometry, PointSetRepresentation, PolylineSetPatch, PolylineSetRepresentation, TrianglePatch,
    TriangulatedSetRepresentation,
};

/// Points that live in a file this test never opens.
///
/// The point of the fixture: every object below names a dataset, and not one
/// test supplies anything able to read it. A `declared_nodes` that reached for
/// the numbers would have nowhere to reach.
fn points(path: &str) -> AnyPointGeometry {
    AnyPointGeometry::PointGeometry(PointGeometry {
        points: Some(AbstractPoint3dArray::Point3dHdf5Array(Point3dHdf5Array {
            coordinates: Hdf5Dataset {
                path_in_hdf_file: path.to_string(),
                hdf_proxy: DataObjectReference {
                    uuid: "33333333-3333-3333-3333-333333333333".to_string(),
                    ..Default::default()
                },
            },
        })),
        ..Default::default()
    })
}

fn triangulated(patches: &[(u64, u64)]) -> TriangulatedSetRepresentation {
    TriangulatedSetRepresentation {
        triangle_patch: patches
            .iter()
            .enumerate()
            .map(|(index, (nodes, triangles))| TrianglePatch {
                patch_index: index as u64,
                count: *triangles,
                node_count: *nodes,
                geometry: points("/surface/Points"),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

#[test]
fn a_surface_states_the_nodes_of_every_patch() {
    // TRG §5.20: a `TrianglePatch` carries `NodeCount` beside its `Count` of
    // triangles, so a set of patches states its own size in full.
    let surface = triangulated(&[(120, 200), (30, 44)]);
    assert_eq!(geometry::declared_nodes(surface.as_reflect()), Some(150));
}

#[test]
fn one_patch_is_the_whole_answer() {
    let surface = triangulated(&[(7, 5)]);
    assert_eq!(geometry::declared_nodes(surface.as_reflect()), Some(7));
}

#[test]
fn a_surface_with_no_patch_says_nothing_rather_than_zero() {
    // The distinction the whole module turns on. A sum over no patch is zero,
    // and zero would be shown as *this surface holds nothing* - a claim about
    // the file, made by arithmetic over an empty list.
    let surface = triangulated(&[]);
    assert_eq!(geometry::declared_nodes(surface.as_reflect()), None);
}

#[test]
fn a_point_set_states_the_count_of_each_node_patch() {
    // `NodePatch.Count` is *number of items in the patch*, which for a point
    // set is its nodes.
    let set = PointSetRepresentation {
        node_patch: vec![
            NodePatch {
                patch_index: 0,
                count: 26,
                geometry: points("/cloud/Points"),
            },
            NodePatch {
                patch_index: 1,
                count: 4,
                geometry: points("/cloud/Points2"),
            },
        ],
        ..Default::default()
    };
    assert_eq!(geometry::declared_nodes(set.as_reflect()), Some(30));
}

#[test]
fn a_lattice_states_both_its_axes() {
    // The product is the nodes of the lattice - an **upper bound** on what will
    // be drawn, since a node carrying no location is dropped by the reader.
    // That is why the number is called declared and never replaces a measured
    // one; `corpus.rs` is where the two are confronted.
    let grid = Grid2dRepresentation {
        grid2d_patch: Grid2dPatch {
            patch_index: 0,
            fastest_axis_count: 1_301,
            slowest_axis_count: 759,
            geometry: points("/horizon/Points"),
        },
        ..Default::default()
    };
    assert_eq!(geometry::declared_nodes(grid.as_reflect()), Some(987_459));
}

#[test]
fn a_polyline_set_keeps_its_counts_in_an_array_and_so_states_nothing() {
    // `NodeCountPerPolyline` is an `AbstractIntegerArray` - the numbers are in
    // the data file. Answering here would mean reading it, which is the cost
    // this function exists to avoid; answering zero would be a lie. So: `None`,
    // and the row shows a size once it is drawn.
    let lines = PolylineSetRepresentation {
        line_patch: vec![PolylineSetPatch {
            patch_index: 0,
            geometry: points("/lines/Points"),
            ..Default::default()
        }],
        ..Default::default()
    };
    assert!(geometry::is_supported(lines.as_reflect()));
    assert_eq!(geometry::declared_nodes(lines.as_reflect()), None);
}

#[test]
fn a_concept_no_reader_handles_states_nothing() {
    // Two silences the caller has no reason to tell apart: nobody reads this
    // concept, and the reader that does cannot say. Both mean *ask the
    // geometry*.
    let interpretation = energyml_resqml_2_0_1::HorizonInterpretation::default();
    assert!(!geometry::is_supported(interpretation.as_reflect()));
    assert_eq!(geometry::declared_nodes(interpretation.as_reflect()), None);
}

#[test]
fn a_wellbore_frame_states_its_nodes_on_the_object() {
    // `NodeCount` is *the number of nodes*, and `NodeMd` - one measured depth
    // per node - is the array it counts. Reading the array is what this
    // function exists not to do.
    let frame = energyml_resqml_2_0_1::WellboreFrameRepresentation {
        node_count: 1_204,
        ..Default::default()
    };
    assert_eq!(geometry::declared_nodes(frame.as_reflect()), Some(1_204));
}

#[test]
fn an_unstructured_grid_states_its_nodes_under_its_geometry() {
    // The one concept that hangs `NodeCount` somewhere else: the object states
    // `CellCount`, and the node count sits on the `Geometry` beside the
    // `Points` it counts. Looked for by field rather than by concept, so a
    // release that moves it again costs nothing - and so that reading the
    // object's own `CellCount` as nodes stays impossible.
    let grid = energyml_resqml_2_0_1::UnstructuredGridRepresentation {
        cell_count: 27,
        geometry: Some(energyml_resqml_2_0_1::UnstructuredGridGeometry {
            node_count: 98,
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_eq!(geometry::declared_nodes(grid.as_reflect()), Some(98));
}

#[test]
fn a_trajectory_states_the_knots_of_its_single_line() {
    // `KnotCount` is answerable here and **not** for a parametric line *set*:
    // there it is the longest line's, and the short lines are padded with `NaN`
    // (see `parametric.rs`), so a set states a bound and not a count. A
    // trajectory carries one line, so the two coincide - and the corpus agrees
    // on all forty of its trajectories.
    let trajectory = energyml_resqml_2_0_1::WellboreTrajectoryRepresentation {
        geometry: Some(
            energyml_resqml_2_0_1::AbstractParametricLineGeometry::ParametricLineGeometry(
                energyml_resqml_2_0_1::ParametricLineGeometry {
                    knot_count: 313,
                    ..Default::default()
                },
            ),
        ),
        ..Default::default()
    };
    assert_eq!(geometry::declared_nodes(trajectory.as_reflect()), Some(313));
}

#[test]
fn a_trajectory_with_no_geometry_states_nothing() {
    // It is drawn from its datum instead - the reader's own fallback - and
    // nothing in the document says how many stations that will give.
    let trajectory = energyml_resqml_2_0_1::WellboreTrajectoryRepresentation::default();
    assert!(geometry::is_supported(trajectory.as_reflect()));
    assert_eq!(geometry::declared_nodes(trajectory.as_reflect()), None);
}
