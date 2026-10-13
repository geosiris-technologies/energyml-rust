//! What a viewer gets, per topology.
//!
//! The bug this pins was reported twice from a real session, in the same words
//! both times: grids are always read as a point cloud and never as a surface,
//! and wellbores are loaded as point clouds. The data
//! was there the whole way - `Topology::Grid` and `Topology::Polylines` are
//! what the readers already produced - but nothing turned a cell into two
//! triangles or a station list into segments, so a viewer given only points
//! drew only points.

use energyml_geometry::geometry::{Geometry, Patch, Topology};

type Point = [f64; 3];

fn patch(points: Vec<Point>, topology: Topology) -> Patch {
    Patch {
        index: 0,
        node_offset: 0,
        points,
        topology,
        closed: Vec::new(),
    }
}

/// A 3 × 2 lattice, row-major with the fast axis first.
fn grid() -> Geometry {
    let mut points = Vec::new();
    for row in 0..2 {
        for column in 0..3 {
            points.push([column as f64, row as f64, 0.0]);
        }
    }
    Geometry {
        kind: "Grid2dRepresentation",
        patches: vec![patch(
            points,
            Topology::Grid {
                fastest: 3,
                slowest: 2,
            },
        )],
    }
}

#[test]
fn a_grid_becomes_two_triangles_per_cell() {
    // 3 × 2 points is 2 × 1 cells, so four triangles - not a spray of six dots.
    let drawn = grid().drawable();
    assert_eq!(drawn.points.len(), 6);
    assert_eq!(drawn.triangles.len(), 4);
    assert!(drawn.lines.is_empty());

    // Every index is inside the point list. An index past the end does not draw
    // a wrong triangle; it makes the whole draw call fail, silently, in the GPU.
    for triangle in &drawn.triangles {
        for index in triangle {
            assert!((*index as usize) < drawn.points.len(), "{index} is out of range");
        }
    }
    // And the first cell is the one the lattice starts with.
    assert_eq!(drawn.triangles[0], [0, 1, 4]);
    assert_eq!(drawn.triangles[1], [0, 4, 3]);
}

#[test]
fn a_cell_with_a_missing_corner_is_dropped_rather_than_drawn() {
    // A grid with holes is ordinary - an eroded horizon, a survey clipped to a
    // licence block. A triangle through `NaN` does not render as a hole; it
    // renders as a spike across the entire scene.
    let mut geometry = grid();
    geometry.patches[0].points[4] = [f64::NAN, f64::NAN, f64::NAN];
    let drawn = geometry.drawable();
    // Point 4 is a corner of three of the four triangles, so one survives - the
    // far half of the second cell. Named rather than counted: what matters is
    // that every triangle still standing avoids the hole.
    //
    // `4` and not `5`: the hole's node is gone from the buffer as well, so
    // point 5 moved down to 4 and the triangle followed it. See
    // [`a_hole_leaves_no_vertex_behind`] for why the vertex cannot stay.
    assert_eq!(drawn.triangles, vec![[1, 2, 4]]);
    assert_eq!(drawn.dropped, 1);
}

#[test]
fn a_trajectory_becomes_the_segments_between_its_stations() {
    let geometry = Geometry {
        kind: "WellboreTrajectoryRepresentation",
        patches: vec![patch(
            vec![[0.0, 0.0, 0.0], [0.0, 0.0, -10.0], [1.0, 0.0, -20.0]],
            Topology::Polylines(vec![3]),
        )],
    };
    let drawn = geometry.drawable();
    assert_eq!(drawn.lines, vec![[0, 1], [1, 2]]);
    assert!(drawn.triangles.is_empty());
}

#[test]
fn several_runs_do_not_join_across_their_ends() {
    // Two separate polylines drawn as one would put a segment across the map
    // between the last station of one well and the first of the next.
    let geometry = Geometry {
        kind: "PolylineSetRepresentation",
        patches: vec![patch(
            vec![[0.0; 3], [1.0, 0.0, 0.0], [5.0, 0.0, 0.0], [6.0, 0.0, 0.0]],
            Topology::Polylines(vec![2, 2]),
        )],
    };
    assert_eq!(geometry.drawable().lines, vec![[0, 1], [2, 3]]);
}

#[test]
fn a_ring_the_file_calls_closed_gets_the_segment_that_closes_it() {
    // Reported on Volve: a `PolylineSetRepresentation` closed in the file was
    // drawn open, because the reader took `NodeCountPerPolyline` and never
    // `ClosedPolylines`. What one would have to break to make this go red:
    // forget the last segment, or repeat the first node instead of going back
    // to it - which would shift every property indexed on the nodes.
    let square = vec![[0.0; 3], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]];
    let ring = Patch {
        index: 0,
        node_offset: 0,
        points: square.clone(),
        topology: Topology::Polylines(vec![4]),
        closed: vec![true],
    };
    let drawn = Geometry {
        kind: "PolylineSetRepresentation",
        patches: vec![ring],
    }
    .drawable();
    assert_eq!(drawn.lines, vec![[0, 1], [1, 2], [2, 3], [3, 0]]);
    assert_eq!(drawn.points.len(), 4, "the first node is not repeated");

    // Nothing said, nothing closed: a producer that omits the array has not
    // said *open*, and closing anyway would invent a ring.
    let silent = Geometry {
        kind: "PolylineSetRepresentation",
        patches: vec![patch(square.clone(), Topology::Polylines(vec![4]))],
    };
    assert_eq!(silent.drawable().lines, vec![[0, 1], [1, 2], [2, 3]]);

    // Mixed in one patch, each on its own terms - and the indices stay the
    // patch's, the second line closing back on *its own* first node.
    let mixed = Patch {
        index: 0,
        node_offset: 0,
        points: vec![
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [5.0, 0.0, 0.0],
            [6.0, 0.0, 0.0],
            [6.0, 1.0, 0.0],
        ],
        topology: Topology::Polylines(vec![3, 3]),
        closed: vec![false, true],
    };
    let drawn = Geometry {
        kind: "PolylineSetRepresentation",
        patches: vec![mixed],
    }
    .drawable();
    assert_eq!(drawn.lines, vec![[0, 1], [1, 2], [3, 4], [4, 5], [5, 3]]);

    // Two nodes do not make a ring: closing it would lay two segments on the
    // same stroke, which shows as flicker.
    let thin = Patch {
        index: 0,
        node_offset: 0,
        points: vec![[0.0; 3], [1.0, 0.0, 0.0]],
        topology: Topology::Polylines(vec![2]),
        closed: vec![true],
    };
    let drawn = Geometry {
        kind: "PolylineSetRepresentation",
        patches: vec![thin],
    }
    .drawable();
    assert_eq!(drawn.lines, vec![[0, 1]]);
}

#[test]
fn indices_are_rebased_when_several_patches_are_merged() {
    // The failure this prevents draws patch two using patch one's points -
    // which looks like a plausible surface in the wrong place.
    let geometry = Geometry {
        kind: "TriangulatedSetRepresentation",
        patches: vec![
            patch(
                vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                Topology::Triangles(vec![[0, 1, 2]]),
            ),
            patch(
                vec![[9.0, 0.0, 0.0], [10.0, 0.0, 0.0], [9.0, 1.0, 0.0]],
                Topology::Triangles(vec![[0, 1, 2]]),
            ),
        ],
    };
    let drawn = geometry.drawable();
    assert_eq!(drawn.triangles, vec![[0, 1, 2], [3, 4, 5]]);
}

#[test]
fn quadrilaterals_and_rings_are_fanned() {
    let quads = Geometry {
        kind: "x",
        patches: vec![patch(
            vec![[0.0; 3], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
            Topology::Quads(vec![[0, 1, 2, 3]]),
        )],
    };
    assert_eq!(quads.drawable().triangles, vec![[0, 1, 2], [0, 2, 3]]);

    let ring = Geometry {
        kind: "x",
        patches: vec![patch(
            vec![
                [0.0; 3],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.5, 2.0, 0.0],
                [0.0, 1.0, 0.0],
            ],
            Topology::Polygons(vec![vec![0, 1, 2, 3, 4]]),
        )],
    };
    assert_eq!(ring.drawable().triangles.len(), 3);
}

#[test]
fn points_that_mean_nothing_together_stay_points() {
    let geometry = Geometry {
        kind: "PointSetRepresentation",
        patches: vec![patch(vec![[0.0; 3], [1.0; 3]], Topology::Points)],
    };
    let drawn = geometry.drawable();
    assert_eq!(drawn.points.len(), 2);
    assert!(drawn.triangles.is_empty() && drawn.lines.is_empty());
}

#[test]
fn a_hole_leaves_no_vertex_behind() {
    // Dropping the *triangles* through a `NaN` is half the answer. The point
    // itself is still in the buffer a viewer uploads, and one `NaN` coordinate
    // there is not a missing dot: `computeBoundingSphere` comes out `NaN`, the
    // frustum test fails for the whole object, and three.js says so once per
    // frame. The scene loses its framing over a node that means "no data here".
    let mut geometry = grid();
    geometry.patches[0].points[4] = [f64::NAN, 0.0, 0.0];
    let drawn = geometry.drawable();
    assert!(
        drawn.points.iter().all(|p| p.iter().all(|c| c.is_finite())),
        "a viewer must never be handed a coordinate that is not a number",
    );
    assert_eq!(drawn.points.len(), 5);
    assert_eq!(drawn.dropped, 1);
    // Renumbered, not merely shortened: point 5 became point 4, and the one
    // surviving triangle has to follow it or it draws the wrong corner.
    assert_eq!(drawn.triangles, vec![[1, 2, 4]]);
    for triangle in &drawn.triangles {
        for index in triangle {
            assert!((*index as usize) < drawn.points.len(), "{index} is out of range");
        }
    }
}

#[test]
fn a_polyline_through_a_hole_keeps_its_remaining_segments() {
    let geometry = Geometry {
        kind: "PolylineSetRepresentation",
        patches: vec![patch(
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [f64::NAN, 0.0, 0.0],
                [3.0, 0.0, 0.0],
                [4.0, 0.0, 0.0],
            ],
            Topology::Polylines(vec![5]),
        )],
    };
    let drawn = geometry.drawable();
    assert_eq!(drawn.points.len(), 4);
    assert_eq!(drawn.dropped, 1);
    assert_eq!(drawn.lines, vec![[0, 1], [2, 3]]);
}

// ---------------------------------------------------------------------------
// The corpus, which is the only place the two symptoms were seen.

#[cfg(feature = "hdf5")]
mod corpus {
    use std::path::PathBuf;
    use std::sync::Arc;

    use energyml_core::registry::Registry;
    use energyml_geometry::{Topology, geometry};
    use energyml_storage::EpcStore;

    /// The public reference packages; with `private-corpus`, the real files of
    /// ALWYN and Volve too (where the two symptoms were first seen). Real-file
    /// campaign, private, run on real files.
    fn files() -> Vec<PathBuf> {
        #[allow(unused_mut)]
        let mut found = crate::fixtures::public_corpus();
        #[cfg(feature = "private-corpus")]
        found.extend(crate::fixtures::private_corpus());
        found
    }

    fn registry() -> Arc<Registry> {
        Arc::new(energyml_schemas::all())
    }

    /// The report: 2D grids (notably RESQML 2.0.1) are sometimes read as a point
    /// cloud when surfaces are wanted - and a viewer draws a point
    /// cloud for exactly one reason: it was handed no index at all.
    ///
    /// # What the corpus says, and why this is not `triangles > 0`
    ///
    /// Eight of ALWYN's nine 2D grids come out with no triangle, and the reason
    /// is in the file rather than in the reader. Their `ZValues` dataset is
    /// declared 849 × 1163 - the lattice's own size - and holds a value on
    /// **every other node**: `nan, 3251.18, nan, 3249.63, …` in `h5dump`, with
    /// 193 288 of the 202 136 gaps between defined values equal to 2. No cell
    /// of the declared lattice has four defined corners, so there is no quad to
    /// draw, and joining across the undefined ones would be this crate
    /// inventing a surface the file does not state.
    ///
    /// So the check is the one that can actually fail: **every cell whose four
    /// corners are defined is drawn**, counted here from the points rather than
    /// taken from `drawable`. A reader that lost the lattice - wrong axis
    /// counts, a transposed row length, a topology dropped on the way to the
    /// viewer - comes out with defined cells and no triangles, which is the
    /// bug this is for, and it stays red on ALWYN's grids as well as on the
    /// ones that do draw.
    #[test]
    fn every_2d_grid_draws_every_cell_its_nodes_define() {
        let found = files();
        if found.is_empty() {
            eprintln!("skipped: no reference package (scripts/fetch-fixtures.py)");
            return;
        }
        let mut seen = 0usize;
        let mut surfaces = 0usize;
        let mut wrong = Vec::new();
        for path in &found {
            let store = EpcStore::open(path, registry()).expect("the package opens");
            for summary in store.resources() {
                if !summary
                    .qualified_type
                    .to_lowercase()
                    .contains("grid2drepresentation")
                {
                    continue;
                }
                let Ok(object) = store.get_object(&summary.uri) else {
                    continue;
                };
                let Ok(shape) = geometry::of(object.as_reflect(), &store) else {
                    continue;
                };
                seen += 1;
                let drawn = shape.drawable();
                if !drawn.triangles.is_empty() {
                    surfaces += 1;
                }
                // The same fact from the other side: the triangles the *points*
                // define, counted here without asking the code under test.
                //
                // Per triangle and not per cell, because a cell with three
                // defined corners still has one: half a quad is a real half of
                // a real surface, and dropping it would eat a row of triangles
                // all along the edge of every hole.
                let mut cells = 0usize;
                for patch in &shape.patches {
                    let Topology::Grid { fastest, slowest } = patch.topology else {
                        continue;
                    };
                    let defined = |index: usize| patch.points[index].iter().all(|c: &f64| c.is_finite());
                    for row in 0..slowest.saturating_sub(1) {
                        for column in 0..fastest.saturating_sub(1) {
                            let a = row * fastest + column;
                            let (b, c, d) = (a + 1, a + fastest, a + fastest + 1);
                            if defined(a) && defined(b) && defined(d) {
                                cells += 1;
                            }
                            if defined(a) && defined(d) && defined(c) {
                                cells += 1;
                            }
                        }
                    }
                }
                if drawn.triangles.len() != cells {
                    wrong.push(format!(
                        "{} {} - {cells} triangle(s) the nodes define, {} drawn",
                        path.file_name().unwrap_or_default().to_string_lossy(),
                        summary.uri,
                        drawn.triangles.len(),
                    ));
                }
            }
        }
        assert!(
            seen > 0,
            "the corpus holds no 2D grid, so this test proves nothing"
        );
        assert!(
            wrong.is_empty(),
            "cells the reader had and did not draw:\n{}",
            wrong.join("\n")
        );
        assert!(surfaces > 0, "not one 2D grid of the corpus drew a surface");
        eprintln!("{seen} 2D grid(s), {surfaces} of them with a surface to draw");
    }

    /// Not one coordinate a viewer uploads is `NaN`.
    ///
    /// The console said it three times in one session, and it is not cosmetic:
    /// a single `NaN` in a position buffer makes the object's bounding sphere
    /// `NaN`, which loses the framing of the **whole scene**.
    #[test]
    fn nothing_a_viewer_is_handed_is_a_non_number() {
        let found = files();
        if found.is_empty() {
            eprintln!("skipped: no reference package (scripts/fetch-fixtures.py)");
            return;
        }
        let mut checked = 0usize;
        let mut dropped = 0usize;
        let mut guilty = Vec::new();
        for path in &found {
            let store = EpcStore::open(path, registry()).expect("the package opens");
            for summary in store.resources() {
                let Ok(object) = store.get_object(&summary.uri) else {
                    continue;
                };
                if !geometry::is_supported(object.as_reflect()) {
                    continue;
                }
                let Ok(shape) = geometry::of(object.as_reflect(), &store) else {
                    continue;
                };
                checked += 1;
                let drawn = shape.drawable();
                dropped += drawn.dropped;
                if drawn.points.iter().any(|p| p.iter().any(|c| !c.is_finite())) {
                    guilty.push(format!("{} {}", path.display(), summary.uri));
                }
                let outside = drawn
                    .triangles
                    .iter()
                    .flatten()
                    .chain(drawn.lines.iter().flatten())
                    .any(|index| *index as usize >= drawn.points.len());
                assert!(
                    !outside,
                    "{}: an index past the end of the point list",
                    summary.uri
                );
            }
        }
        assert!(checked > 0, "nothing was read, so this test proves nothing");
        assert!(
            guilty.is_empty(),
            "non-finite coordinates handed to a viewer:\n{}",
            guilty.join("\n")
        );
        eprintln!("{checked} representation(s) drawable, {dropped} node(s) without a location");
    }
}

// ---------------------------------------------------------------------------
// Volumes - the same bug one level up
// ---------------------------------------------------------------------------
//
// Reported in the same words as the two above: some IJK grids of the testing
// package show up as a point cloud, where volumes were expected.
// An IJK grid reaches `drawable` as `Topology::Hexahedra` and an unstructured
// one as `Topology::Polyhedra`; neither had an arm, so a viewer fell through to
// its last resort and drew the nodes.
//
// What these pin is not "a volume produces triangles" but **which** triangles:
// the boundary, not every face. A grid that emitted all of them would pass a
// count-something assertion and still render as a block z-fighting with itself.

/// The unit cube in VTK's corner order: bottom 0-1-2-3, top 4-5-6-7.
fn cube(z: f64) -> Vec<Point> {
    vec![
        [0.0, 0.0, z],
        [1.0, 0.0, z],
        [1.0, 1.0, z],
        [0.0, 1.0, z],
        [0.0, 0.0, z + 1.0],
        [1.0, 0.0, z + 1.0],
        [1.0, 1.0, z + 1.0],
        [0.0, 1.0, z + 1.0],
    ]
}

#[test]
fn a_hexahedral_grid_becomes_its_surface_not_a_point_cloud() {
    let geometry = Geometry {
        kind: "IjkGridRepresentation",
        patches: vec![patch(
            cube(0.0),
            Topology::Hexahedra {
                cells: vec![[0, 1, 2, 3, 4, 5, 6, 7]],
                dimensions: [1, 1, 1],
                index: Vec::new(),
            },
        )],
    };

    let drawn = geometry.drawable();
    assert_eq!(drawn.points.len(), 8);
    // Six faces, two triangles each. The number that matters is that it is not
    // zero: eight points and nothing joining them is exactly what the viewer
    // drew as dots.
    assert_eq!(drawn.triangles.len(), 12, "a lone cell has six faces on its skin");
    assert!(drawn.lines.is_empty());
}

/// A node of a volume that no cell names is dropped with the cells that
/// would have named it - not drawn as a dot where the grid says there is
/// nothing.
///
/// The case is the IJK reader's: a cell its `CellGeometryIsDefined` flag marks
/// undefined is skipped, and the standard makes the flag override the numbers,
/// so the cell's corners stay finite in the buffer. Two cubes side by side,
/// the second one skipped: its four exclusive corners must go, the shared four
/// must stay, and the count must say so. The numbers are the cube's own - eight
/// corners, six faces, twelve triangles - not a first run's.
#[test]
fn a_volume_node_no_cell_names_is_dropped_not_drawn_as_a_dot() {
    let mut points = cube(0.0);
    // The second cube's exclusive corners, one unit further along x.
    points.extend([[2.0, 0.0, 0.0], [2.0, 1.0, 0.0], [2.0, 1.0, 1.0], [2.0, 0.0, 1.0]]);
    let geometry = Geometry {
        kind: "IjkGridRepresentation",
        patches: vec![patch(
            points,
            Topology::Hexahedra {
                cells: vec![[0, 1, 2, 3, 4, 5, 6, 7]],
                dimensions: [2, 1, 1],
                index: Vec::new(),
            },
        )],
    };

    let drawn = geometry.drawable();
    assert_eq!(drawn.dropped, 4, "the skipped cube's exclusive corners");
    assert_eq!(drawn.points.len(), 8);
    assert_eq!(drawn.triangles.len(), 12, "the kept cube's skin, whole");
    assert!(
        drawn.points.iter().all(|p| p[0] < 1.5),
        "no point of the skipped cube survives: {:?}",
        drawn.points
    );

    // A surface keeps its lone nodes: a node a lattice samples is a location
    // whether or not the cells around it are complete - the ALWYN horizons are
    // nothing but lone nodes.
    let lone = Geometry {
        kind: "Grid2dRepresentation",
        patches: vec![patch(
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, f64::NAN],
                [0.0, 1.0, f64::NAN],
                [1.0, 1.0, 0.0],
            ],
            Topology::Grid {
                fastest: 2,
                slowest: 2,
            },
        )],
    };
    let drawn = lone.drawable();
    assert_eq!(
        (drawn.points.len(), drawn.dropped, drawn.triangles.len()),
        (2, 2, 0)
    );
}

#[test]
fn a_shared_face_is_drawn_once_and_from_the_outside() {
    // Two cells stacked in Z. Twelve face-instances, of which two are the same
    // interior face seen from either side - so the skin is ten faces, and
    // emitting every face would give twelve.
    let mut points = cube(0.0);
    points.extend_from_slice(&cube(1.0)[4..]);
    let geometry = Geometry {
        kind: "IjkGridRepresentation",
        patches: vec![patch(
            points,
            Topology::Hexahedra {
                cells: vec![[0, 1, 2, 3, 4, 5, 6, 7], [4, 5, 6, 7, 8, 9, 10, 11]],
                dimensions: [1, 1, 2],
                index: Vec::new(),
            },
        )],
    };

    let drawn = geometry.drawable();
    assert_eq!(drawn.points.len(), 12);
    assert_eq!(
        drawn.triangles.len(),
        20,
        "ten boundary faces, not the twelve a per-cell emission would give"
    );
}

#[test]
fn an_unstructured_grid_becomes_its_surface_too() {
    use energyml_geometry::geometry::Polyhedra;

    // Two tetrahedra sharing the face (1, 2, 3): seven distinct faces, eight
    // cell-face pairs, and one face claimed twice.
    let points = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
    ];
    let cells = Polyhedra::new(
        vec![1, 2, 3, 0, 2, 1, 0, 3, 2, 0, 1, 3, 4, 2, 1, 4, 3, 2, 4, 1, 3],
        vec![0, 3, 6, 9, 12, 15, 18, 21],
        vec![0, 1, 2, 3, 0, 4, 5, 6],
        vec![0, 4, 8],
        vec![true; 8],
    );
    assert_eq!(cells.cell_count(), 2);
    assert_eq!(cells.face_count(), 7);

    let geometry = Geometry {
        kind: "UnstructuredGridRepresentation",
        patches: vec![patch(points, Topology::Polyhedra(cells))],
    };

    let drawn = geometry.drawable();
    assert_eq!(drawn.points.len(), 5);
    // Six boundary faces, each a triangle. The shared one is left out - which
    // is the whole difference between this and `Polyhedra::triangles`, whose
    // answer here would be seven.
    assert_eq!(drawn.triangles.len(), 6, "the shared face is interior");
    assert!(drawn.lines.is_empty());
}

#[test]
fn every_triangle_names_the_cell_it_was_cut_from() {
    // The table a flat-shaded (per-cell) painting stands on: three.js colours
    // vertices, so an aplat de-indexes the geometry - and each new vertex has
    // to know which cell's value it wears. The numbering is `cell_count`'s,
    // across patches, whatever the primitive was cut into.
    let two_patches = Geometry {
        kind: "TriangulatedSetRepresentation",
        patches: vec![
            patch(
                vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                Topology::Triangles(vec![[0, 1, 2]]),
            ),
            // A quad is cut into two triangles - both wear the same cell.
            patch(
                vec![[2.0, 0.0, 0.0], [3.0, 0.0, 0.0], [3.0, 1.0, 0.0], [2.0, 1.0, 0.0]],
                Topology::Quads(vec![[0, 1, 2, 3]]),
            ),
        ],
    };
    let drawn = two_patches.drawable();
    assert_eq!(drawn.triangles.len(), 3, "one triangle, then a quad as two");
    assert_eq!(
        drawn.triangle_cells,
        vec![0, 1, 1],
        "the quad is cell 1, both halves"
    );
    assert_eq!(
        two_patches.cell_count(),
        2,
        "the table's numbering is cell_count's"
    );
}

#[test]
fn a_grid_cell_and_a_dropped_corner_keep_the_table_in_lockstep() {
    // A lattice with one NaN corner: the two triangles touching it vanish, and
    // their cell entries must vanish **with them** - a table filtered on its
    // own is every triangle after the hole wearing its neighbour's value.
    let mut points = Vec::new();
    for row in 0..2 {
        for column in 0..3 {
            points.push([column as f64, row as f64, 0.0]);
        }
    }
    points[2] = [f64::NAN, f64::NAN, f64::NAN]; // the far corner of cell 1
    let holed = Geometry {
        kind: "Grid2dRepresentation",
        patches: vec![patch(
            points,
            Topology::Grid {
                fastest: 3,
                slowest: 2,
            },
        )],
    };
    let drawn = holed.drawable();
    assert_eq!(drawn.dropped, 1);
    assert_eq!(
        drawn.triangles.len(),
        drawn.triangle_cells.len(),
        "one entry per surviving triangle, exactly"
    );
    // Cell 0 keeps both halves; cell 1 lost the corner and keeps only the
    // triangle that avoids it.
    assert_eq!(drawn.triangle_cells, vec![0, 0, 1]);
}

#[test]
fn a_volume_drawn_by_its_skin_refuses_the_table_rather_than_guessing() {
    // A boundary face is a *face*, not a cell: naming one per triangle would
    // line a per-cell property up against elements it was not measured on.
    // Empty is the honest state, and the caller refuses per-cell painting by
    // name - exactly as it did before the table existed.
    let cube = Geometry {
        kind: "IjkGridRepresentation",
        patches: vec![patch(
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 1.0],
                [1.0, 1.0, 1.0],
                [0.0, 1.0, 1.0],
            ],
            Topology::Hexahedra {
                cells: vec![[0, 1, 2, 3, 4, 5, 6, 7]],
                dimensions: [1, 1, 1],
                index: Vec::new(),
            },
        )],
    };
    let drawn = cube.drawable();
    assert!(!drawn.triangles.is_empty(), "the skin is drawn");
    assert!(
        drawn.triangle_cells.is_empty(),
        "and the table is withheld, not invented"
    );
}

#[test]
fn every_polyline_node_names_its_run_and_surface_nodes_name_none() {
    // One colour a polyline needs no de-indexing: every node of a run belongs
    // to that run and no other, so the table is a lookup. A surface node is
    // the opposite - shared between the cells around it - and its entry says
    // so (`NO_CELL`) instead of picking one.
    let mixed = Geometry {
        kind: "PolylineSetRepresentation",
        patches: vec![
            patch(
                vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                Topology::Triangles(vec![[0, 1, 2]]),
            ),
            // Two runs of two nodes: cells 1 and 2, after the triangle's 0.
            patch(
                vec![
                    [0.0, 0.0, -1.0],
                    [0.0, 0.0, -2.0],
                    [5.0, 0.0, -1.0],
                    [5.0, 0.0, -2.0],
                ],
                Topology::Polylines(vec![2, 2]),
            ),
        ],
    };
    let drawn = mixed.drawable();
    use energyml_geometry::geometry::Drawable;
    assert_eq!(
        drawn.node_cells,
        vec![
            Drawable::NO_CELL,
            Drawable::NO_CELL,
            Drawable::NO_CELL,
            1,
            1,
            2,
            2
        ],
        "surface nodes name no cell; each run's nodes name their run, cell_count's numbering"
    );
}

#[test]
fn a_dropped_node_takes_its_cell_entry_with_it() {
    // The table follows its nodes through the NaN filter - filtered any other
    // way, every node after the hole wears its neighbour's run.
    let holed = Geometry {
        kind: "PolylineSetRepresentation",
        patches: vec![patch(
            vec![
                [0.0, 0.0, 0.0],
                [f64::NAN, f64::NAN, f64::NAN],
                [2.0, 0.0, 0.0],
                [3.0, 0.0, 0.0],
            ],
            Topology::Polylines(vec![2, 2]),
        )],
    };
    let drawn = holed.drawable();
    assert_eq!(drawn.dropped, 1);
    assert_eq!(drawn.points.len(), 3);
    assert_eq!(drawn.node_cells, vec![0, 1, 1], "the hole's entry went with it");
}

/// Rule (Energistics common TRG v2.3 §6.31 `FloatingPointExternalArray`: "By
/// convention, the null value is NaN"; `defined.rs` quotes it): a terrain lattice
/// whose no-data pixels are NaN draws as a surface **with a hole in it** - the
/// node is not a point, and no triangle touches it. A 4 x 4 lattice has 18
/// triangles; the interior node (1, 1) is a corner of 6 of them (2 cells share
/// it as the diagonal's end, 4 as a plain corner).
/// To break it: remove **both** defences of `drawable()` (the
/// `finite(a) && finite(b) && finite(c)` in `triangle` and the
/// `renumbered.contains(&GONE)` in the index filter) -
/// a triangle with a NaN elevation is then drawn, and `NaN` reaches the front end.
#[test]
fn a_terrain_lattice_with_a_nan_node_draws_a_surface_with_a_hole() {
    let mut points = Vec::new();
    for j in 0..4 {
        for i in 0..4 {
            points.push([i as f64, j as f64, 10.0]);
        }
    }
    let hole = 4 + 1;
    points[hole][2] = f64::NAN;
    let geometry = Geometry {
        kind: "Grid2dRepresentation",
        patches: vec![patch(
            points,
            Topology::Grid {
                fastest: 4,
                slowest: 4,
            },
        )],
    };

    let drawn = geometry.drawable();
    assert_eq!(drawn.dropped, 1, "the NaN node is counted, not silent");
    assert_eq!(drawn.points.len(), 15);
    assert!(
        drawn.points.iter().all(|p| p.iter().all(|c| c.is_finite())),
        "no NaN reaches the front end"
    );
    assert_eq!(
        drawn.triangles.len(),
        18 - 6,
        "the six triangles that touched the node are gone"
    );
    assert!(
        drawn
            .triangles
            .iter()
            .flatten()
            .all(|&i| (i as usize) < drawn.points.len()),
        "every index follows the renumbering"
    );
}
