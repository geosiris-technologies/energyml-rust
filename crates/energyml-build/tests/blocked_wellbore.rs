//! `blocked-wellbore` - the cells of IJK grids a wellbore frame crosses
//!.
//!
//! The form of the assertions comes from the TRG and the usage guide: a node
//! at each limit between cells, *“for each frame interval corresponding to a
//! grid cell, the corresponding grid index […] a value of -1 is used”* for the
//! others, *“the cell index and the cell face indices where the trajectory
//! enters and exits”* (RESQML Technical Usage Guide 2.2, draft, §11.5 p. 214);
//! *“0=top, 1=bottom, 2-5 are side faces following the IJ circular node
//! ordering”* (same guide §9.12.3 p. 159); `CellCount` *“the number of
//! non-null entries”*, the arrays one per interval, the faces two (RESQML TRG
//! 2.2 §5.32 `IntervalGridCells` p. 128). The numbers - the lattice, the
//! planes, the path - are the tests' own.
//!
//! **What the path crosses is confronted with arithmetic** (the cells are
//! those that the path really crosses): the grid is on
//! a lattice and its layer boundaries are planes this file writes as formulas,
//! so the cell a point is in is `floor` of its offsets and a comparison with
//! the planes - nothing shared with the clipping and the roots of
//! `energyml_analysis::cells`, nor with the IJK grid writer.
//!
//! # What would have to break to make it turn red
//!
//! Not undoing the exchange of the two VTK faces (faces 0 and 1 swap
//! on a grid in elevation); numbering the sides other than in the
//! circular order; writing a node elsewhere than at a limit; forgetting the null
//! interval of a K gap; counting `CellCount` over all the intervals; naming
//! another trajectory than that of the frame; accepting a grid of another
//! frame or a frame without depths.

use serde_json::{Value, json};

use energyml_build::operation::{Choices, Identity, Numbers, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

const X0: f64 = 1_000.0;
const Y0: f64 = 2_000.0;
const DX: f64 = 10.0;
const DY: f64 = 20.0;
/// Nodes of the horizons' lattice: 3 × 2 cells.
const NI: usize = 4;
const NJ: usize = 3;

fn when() -> When {
    When {
        creation: "2026-09-25T00:00:00Z".into(),
        originator: "test".into(),
    }
}

fn frame() -> Ref {
    Ref::new(
        "cccccccc-0000-4000-8000-000000000003",
        "eml23.LocalEngineeringCompoundCrs",
        "The frame",
    )
}

/// The top of the grid, a plane dipping along x - so a level is no flat.
fn top(x: f64, _y: f64) -> f64 {
    0.1 * (x - X0)
}

/// The four horizons, top to bottom: `layers = [2, 0, 1]` makes two layers
/// between the first two, a K gap, one layer. The levels of the grid are then
/// the planes `top`, `top − 5`, `top − 10` | gap | `top − 15`, `top − 30`.
fn horizons() -> Vec<(&'static str, Geometry)> {
    let grid = |drop: f64| {
        let mut points = Vec::new();
        for j in 0..NJ {
            for i in 0..NI {
                let (x, y) = (X0 + DX * i as f64, Y0 + DY * j as f64);
                points.push([x, y, top(x, y) - drop]);
            }
        }
        Geometry {
            kind: "grid",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points,
                topology: Topology::Grid {
                    fastest: NI,
                    slowest: NJ,
                },
                closed: Vec::new(),
            }],
        }
    };
    vec![
        ("A", grid(0.0)),
        ("B", grid(10.0)),
        ("C", grid(15.0)),
        ("D", grid(30.0)),
    ]
}

/// The cell a point is in, by arithmetic on the lattice and the planes.
fn by_arithmetic(p: [f64; 3]) -> Option<u64> {
    let i = ((p[0] - X0) / DX).floor();
    let j = ((p[1] - Y0) / DY).floor();
    if !(0.0..3.0).contains(&i) || !(0.0..2.0).contains(&j) {
        return None;
    }
    let t = top(p[0], p[1]);
    let depth = t - p[2];
    let k = if (0.0..5.0).contains(&depth) {
        0
    } else if (5.0..10.0).contains(&depth) {
        1
    } else if (15.0..30.0).contains(&depth) {
        2
    } else {
        return None;
    };
    Some((i as usize + 3 * (j as usize + 2 * k)) as u64)
}

/// The IJK grid, as `ijk-grid-from-horizons` plans it - its shape the one the
/// geometry reader draws, VTK's order and all - with what the package says of it.
fn the_grid() -> (Geometry, Known) {
    let mut pieces = Pieces::empty();
    for (rank, (title, geometry)) in horizons().into_iter().enumerate() {
        let name = if rank == 0 {
            "horizons".to_string()
        } else {
            format!("horizons {}", rank + 1)
        };
        pieces = pieces
            .with(
                name.clone(),
                Source::Shape {
                    geometry,
                    closed: None,
                },
            )
            .with(
                format!("{name} (object)"),
                Source::Object(Known {
                    reference: Some(Ref::new(
                        format!("1a2b3c4d-0000-4000-8000-{rank:012}"),
                        "resqml22.Grid2dRepresentation",
                        title,
                    )),
                    crs: Some(frame()),
                    vertical_uom: Some("m".into()),
                    vertical_up: Some(true),
                    ..Default::default()
                }),
            );
    }
    let choices = Choices::new()
        .set("layers", json!([2, 0, 1]))
        .set("axis", "elevation")
        .set("title", "Grid");
    let plan = Registry::new()
        .plan(
            "ijk-grid-from-horizons",
            &pieces,
            &choices,
            &Identity::derived(),
            &when(),
        )
        .unwrap_or_else(|why| panic!("{}", why.message()));
    let grid = plan.of_role("representation").next().unwrap();
    let known = Known {
        reference: Some(Ref::new(
            grid.uuid.clone(),
            grid.qualified_type.clone(),
            grid.title.clone(),
        )),
        crs: Some(frame()),
        ..Default::default()
    };
    (plan.shapes[0].geometry.clone(), known)
}

/// A bent path: two straight pieces, its MD their chords from 100.
fn path() -> Vec<[f64; 3]> {
    vec![
        [1_005.0, 2_005.0, 6.0],
        [1_012.0, 2_016.0, -8.0],
        [1_026.0, 2_036.0, -33.0],
    ]
}

fn mds_of(points: &[[f64; 3]]) -> Vec<f64> {
    let mut out = vec![100.0];
    for w in points.windows(2) {
        let d =
            ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2) + (w[1][2] - w[0][2]).powi(2)).sqrt();
        out.push(out.last().unwrap() + d);
    }
    out
}

fn trajectory() -> Ref {
    Ref::new(
        "7a7a0000-0000-4000-8000-000000000001",
        "resqml22.WellboreTrajectoryRepresentation",
        "GTH-1 trajectory",
    )
}

fn interpretation() -> Ref {
    Ref::new(
        "1e1e0000-0000-4000-8000-000000000001",
        "resqml22.WellboreInterpretation",
        "GTH-1",
    )
}

/// What the package says of the frame.
fn frame_known(mds: Vec<f64>) -> Known {
    Known {
        reference: Some(Ref::new(
            "f4a3e000-0000-4000-8000-000000000001",
            "resqml22.WellboreFrameRepresentation",
            "GTH-1 frame",
        )),
        crs: Some(frame()),
        node_mds: Some(mds),
        trajectory: Some(trajectory()),
        interpretation: Some(interpretation()),
        ..Default::default()
    }
}

fn bag_with(points: Vec<[f64; 3]>, known: Known, grids: Vec<(Geometry, Known)>) -> Pieces {
    let n = points.len() as u32;
    let mut pieces = Pieces::empty()
        .with(
            "frame",
            Source::Shape {
                geometry: Geometry {
                    kind: "frame",
                    patches: vec![Patch {
                        index: 0,
                        node_offset: 0,
                        points,
                        topology: Topology::Polylines(vec![n]),
                        closed: Vec::new(),
                    }],
                },
                closed: None,
            },
        )
        .with("frame (object)", Source::Object(known));
    for (rank, (geometry, known)) in grids.into_iter().enumerate() {
        let name = if rank == 0 {
            "grids".to_string()
        } else {
            format!("grids {}", rank + 1)
        };
        pieces = pieces
            .with(
                name.clone(),
                Source::Shape {
                    geometry,
                    closed: None,
                },
            )
            .with(format!("{name} (object)"), Source::Object(known));
    }
    pieces
}

fn bag() -> Pieces {
    let points = path();
    let mds = mds_of(&points);
    bag_with(points, frame_known(mds), vec![the_grid()])
}

fn planned(pieces: &Pieces, choices: &Choices) -> Result<Plan, String> {
    Registry::new()
        .plan("blocked-wellbore", pieces, choices, &Identity::derived(), &when())
        .map_err(|why| why.message())
}

fn the_object(plan: &Plan) -> &Value {
    &plan
        .of_role("representation")
        .next()
        .expect("the blocked wellbore")
        .json
}

fn floats(plan: &Plan, end: &str) -> Vec<f64> {
    match &plan
        .datasets
        .iter()
        .find(|d| d.path.ends_with(end))
        .unwrap_or_else(|| panic!("no {end}"))
        .values
    {
        Numbers::F64(v) => v.clone(),
        Numbers::I64(_) => panic!("{end} is whole"),
    }
}

fn integers(plan: &Plan, end: &str) -> Vec<i64> {
    match &plan
        .datasets
        .iter()
        .find(|d| d.path.ends_with(end))
        .unwrap_or_else(|| panic!("no {end}"))
        .values
    {
        Numbers::I64(v) => v.clone(),
        Numbers::F64(_) => panic!("{end} is floating point"),
    }
}

/// Where along the path an MD falls, on its straight pieces.
fn along(points: &[[f64; 3]], mds: &[f64], md: f64) -> [f64; 3] {
    let s = mds
        .windows(2)
        .position(|w| w[0] <= md && md <= w[1])
        .expect("an MD on the path");
    let t = (md - mds[s]) / (mds[s + 1] - mds[s]);
    std::array::from_fn(|axis| points[s][axis] + t * (points[s + 1][axis] - points[s][axis]))
}

/// The arrays of a blocked wellbore, as the plan or the file holds them.
struct Arrays {
    node_md: Vec<f64>,
    grids: Vec<i64>,
    cells: Vec<i64>,
    faces: Vec<i64>,
}

/// **The blocked cells against arithmetic**: every interval's middle is in the cell the
/// arrays name (or in none, null); a hundred points along the path are each in
/// a cell the arrays name or in none; an exit is the next cell's entry from
/// the other side; `CellCount` is the non-null entries.
fn confront(arrays: &Arrays, cell_count: i64, points: &[[f64; 3]], mds: &[f64]) {
    let n = arrays.node_md.len();
    assert_eq!(arrays.cells.len(), n - 1, "one cell index per interval");
    assert_eq!(arrays.grids.len(), n - 1, "one grid index per interval");
    assert_eq!(arrays.faces.len(), 2 * (n - 1), "two faces per interval");
    assert!(
        arrays.node_md.windows(2).all(|w| w[0] < w[1]),
        "the nodes go down the hole: {:?}",
        arrays.node_md
    );
    assert_eq!(
        cell_count,
        arrays.grids.iter().filter(|g| **g != -1).count() as i64,
        "CellCount is the non-null entries"
    );
    for at in 0..n - 1 {
        let middle = 0.5 * (arrays.node_md[at] + arrays.node_md[at + 1]);
        let expected = by_arithmetic(along(points, mds, middle));
        let named = (arrays.grids[at] != -1).then_some(arrays.cells[at] as u64);
        assert_eq!(named, expected, "interval {at}, MD {middle}");
        assert_eq!(
            arrays.grids[at] == -1,
            arrays.cells[at] == -1,
            "a null grid index, a null cell index"
        );
    }
    let named: Vec<i64> = arrays.cells.clone();
    for step in 0..=100 {
        let md = mds[0] + (mds[mds.len() - 1] - mds[0]) * step as f64 / 100.0;
        if let Some(cell) = by_arithmetic(along(points, mds, md)) {
            assert!(
                named.contains(&(cell as i64)),
                "cell {cell} at MD {md} is crossed and not named"
            );
        }
    }
    for at in 0..n.saturating_sub(2) {
        let (exit, entry) = (arrays.faces[2 * at + 1], arrays.faces[2 * at + 2]);
        if arrays.cells[at] == -1 || arrays.cells[at + 1] == -1 {
            continue;
        }
        let opposite = match exit {
            0 => 1,
            1 => 0,
            2 => 4,
            3 => 5,
            4 => 2,
            5 => 3,
            other => panic!("an exit between two cells is a face, not {other}"),
        };
        assert_eq!(
            entry,
            opposite,
            "leaving cell {} by {exit}, entering {} by {entry}",
            arrays.cells[at],
            arrays.cells[at + 1]
        );
    }
}

fn arrays_of(plan: &Plan) -> Arrays {
    Arrays {
        node_md: floats(plan, "/nodeMd"),
        grids: integers(plan, "/gridIndices"),
        cells: integers(plan, "/cellIndices"),
        faces: integers(plan, "/localFacePairPerCellIndices"),
    }
}

#[test]
fn the_cells_crossed_are_the_cells_arithmetic_finds() {
    let plan = planned(&bag(), &Choices::new()).unwrap_or_else(|why| panic!("{why}"));
    let object = the_object(&plan);
    let arrays = arrays_of(&plan);
    let points = path();
    confront(
        &arrays,
        object["IntervalGridCells"]["CellCount"].as_i64().unwrap(),
        &points,
        &mds_of(&points),
    );
    // The K gap is crossed, so one interval at least is in no cell, and it is
    // written between two cells, not trimmed.
    let first = arrays.grids.iter().position(|g| *g != -1).unwrap();
    let last = arrays.grids.iter().rposition(|g| *g != -1).unwrap();
    assert_eq!(
        (first, last),
        (0, arrays.grids.len() - 1),
        "the frame runs from the first entry to the last exit"
    );
    assert!(
        arrays.grids.contains(&-1),
        "the gap between B and C is an interval in no cell: {:?}",
        arrays.grids
    );
}

/// **A vertical hole enters each cell by the top and leaves by the bottom** -
/// faces 0 and 1 - on a grid the reader hands in VTK's order, its two
/// quadrilaterals swapped in elevation. Undoing the swap is what makes it 0
/// then 1, and not 1 then 0.
#[test]
fn a_vertical_hole_goes_in_by_face_0_and_out_by_face_1() {
    let points = vec![[1_015.0, 2_030.0, 20.0], [1_015.0, 2_030.0, -40.0]];
    let mds = vec![0.0, 60.0];
    let plan = planned(
        &bag_with(points.clone(), frame_known(mds.clone()), vec![the_grid()]),
        &Choices::new(),
    )
    .unwrap_or_else(|why| panic!("{why}"));
    let arrays = arrays_of(&plan);
    // Column (1, 1); top at 1.5: layers 1.5…−3.5, −3.5…−8.5, gap, −13.5…−28.5.
    let expected_md = [18.5, 23.5, 28.5, 33.5, 48.5];
    assert_eq!(arrays.node_md.len(), expected_md.len(), "{:?}", arrays.node_md);
    for (read, want) in arrays.node_md.iter().zip(expected_md) {
        assert!((read - want).abs() < 1e-9, "{:?}", arrays.node_md);
    }
    assert_eq!(arrays.cells, vec![4, 10, -1, 16]);
    assert_eq!(arrays.faces, vec![0, 1, 0, 1, -1, -1, 0, 1]);
    confront(
        &arrays,
        the_object(&plan)["IntervalGridCells"]["CellCount"]
            .as_i64()
            .unwrap(),
        &points,
        &mds,
    );
}

/// **The object names what the frame names**: its trajectory, the grids
/// in the order given, the counts, and - suggested - the frame's
/// interpretation and a title from the frame's.
#[test]
fn the_object_names_the_frames_trajectory_and_the_grids() {
    let plan = planned(&bag(), &Choices::new()).unwrap_or_else(|why| panic!("{why}"));
    let object = the_object(&plan);
    assert_eq!(object["$type"], "resqml22.BlockedWellboreRepresentation");
    assert_eq!(object["Trajectory"]["Uuid"], json!(trajectory().uuid));
    assert_eq!(object["RepresentedObject"]["Uuid"], json!(interpretation().uuid));
    assert_eq!(object["Citation"]["Title"], "GTH-1 frame - blocked");
    let (_, grid) = the_grid();
    let grids = object["IntervalGridCells"]["Grid"].as_array().unwrap();
    assert_eq!(grids.len(), 1);
    assert_eq!(grids[0]["Uuid"], json!(grid.reference.unwrap().uuid));
    assert_eq!(
        object["NodeCount"].as_u64().unwrap() as usize,
        floats(&plan, "/nodeMd").len()
    );
    let dims = |end: &str| {
        plan.datasets
            .iter()
            .find(|d| d.path.ends_with(end))
            .unwrap()
            .dimensions
            .clone()
    };
    let intervals = floats(&plan, "/nodeMd").len() as i64 - 1;
    assert_eq!(dims("/localFacePairPerCellIndices"), vec![intervals, 2]);
    assert_eq!(dims("/cellIndices"), vec![intervals]);
    assert_eq!(object["IntervalGridCells"]["CellIndices"]["NullValue"], json!(-1));
}

/// **What cannot be blocked is refused by its name.**
#[test]
fn what_cannot_be_blocked_is_refused_by_name() {
    let points = path();
    let mds = mds_of(&points);
    let refused = |pieces: Pieces| planned(&pieces, &Choices::new()).unwrap_err();

    let why = refused(bag_with(
        points.clone(),
        Known {
            node_mds: None,
            ..frame_known(mds.clone())
        },
        vec![the_grid()],
    ));
    assert!(
        why.contains("`frame`") && why.contains("measured depths"),
        "{why}"
    );
    let why = refused(bag_with(
        points.clone(),
        Known {
            trajectory: None,
            ..frame_known(mds.clone())
        },
        vec![the_grid()],
    ));
    assert!(why.contains("`frame`") && why.contains("trajectory"), "{why}");
    let why = refused(bag_with(
        points[..2].to_vec(),
        frame_known(mds.clone()),
        vec![the_grid()],
    ));
    assert!(why.contains("2 node(s)") && why.contains("3 measured"), "{why}");

    let (grid, known) = the_grid();
    let elsewhere = Known {
        crs: Some(Ref::new(
            "dddddddd-0000-4000-8000-000000000004",
            "eml23.LocalEngineeringCompoundCrs",
            "The other",
        )),
        ..known.clone()
    };
    let why = refused(bag_with(
        points.clone(),
        frame_known(mds.clone()),
        vec![(grid.clone(), elsewhere)],
    ));
    assert!(why.contains("The frame") && why.contains("The other"), "{why}");

    let cloud = Geometry {
        kind: "points",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points: points.clone(),
            topology: Topology::Points,
            closed: Vec::new(),
        }],
    };
    let why = refused(bag_with(
        points.clone(),
        frame_known(mds.clone()),
        vec![(cloud, known.clone())],
    ));
    assert!(why.contains("`grids`") && why.contains("no IJK grid"), "{why}");

    let away: Vec<[f64; 3]> = points.iter().map(|p| [p[0] + 5_000.0, p[1], p[2]]).collect();
    let why = refused(bag_with(away, frame_known(mds.clone()), vec![(grid, known)]));
    assert!(why.contains("no cell"), "{why}");

    // An artefact of the scene has no uuid to name: unfit, before any plan.
    let artefact = bag_with(
        points,
        Known {
            reference: None,
            ..frame_known(mds)
        },
        vec![the_grid()],
    );
    let why = refused(artefact);
    assert!(
        why.contains("`frame`") && why.contains("no object of the package"),
        "{why}"
    );
}

// -- written, reread, checked --------------------------------------------------------

/// A nested object of a read object - the first, for a list.
#[cfg(feature = "hdf5")]
fn child<'a>(
    reflect: &'a dyn energyml_core::value::Reflect,
    name: &str,
) -> &'a dyn energyml_core::value::Reflect {
    match reflect.field(name) {
        Some(energyml_core::value::FieldValue::Object(one)) => one,
        Some(energyml_core::value::FieldValue::List(items)) => items
            .first()
            .and_then(|one| one.as_object())
            .unwrap_or_else(|| panic!("an empty `{name}`")),
        _ => panic!("no `{name}`"),
    }
}

/// **Written, reread, confronted, checked.** The chain as a package makes it:
/// a frame of reference (`crs`), the grid (`ijk-grid-from-horizons`), a well
/// (`well-from-polyline`), all reopened from the file; then the blocked
/// wellbore, a sub-representation of the grid (the lower zone, an IJK box -
/// the sub-representation rule) and a set of the grid and the blocked wellbore, from what
/// was **read back**. Reopened again: the blocked wellbore's arrays read from
/// the HDF5 against the arithmetic, the sub-representation drawn by
/// `energyml_geometry` against the grid it selects from, and `energyml check`
/// at 0 error, 0 warning.
#[cfg(feature = "hdf5")]
#[test]
fn blocked_subrepresentation_and_set_are_written_reread_and_checked() {
    use std::sync::Arc;

    use energyml_build::read;
    use energyml_storage::{EpcStore, SaveOptions};

    let dir = std::env::temp_dir().join(format!("energyml-build-blocked-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let schemas = Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "blocked.epc", schemas.clone()).unwrap();
    store.external_files_mut().set_root(&dir);
    let registry = Registry::new();
    let apply = |plan: &Plan, store: &mut EpcStore| {
        let made = energyml_build::apply::apply(plan, store, &when());
        assert!(
            made.invalid.is_empty(),
            "objects written against their schema: {:#?}",
            made.invalid
        );
        assert!(made.refused.is_empty(), "{:?}", made.refused);
    };

    // -- a frame, a grid, a well ---------------------------------------------------
    let crs = registry
        .plan(
            "crs",
            &Pieces::empty(),
            &Choices::new().set("epsg", 2154).set("vertical", 5720),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    apply(&crs, &mut store);
    let compound = crs.of_role("crs").next().unwrap();
    let compound = Ref::new(
        compound.uuid.clone(),
        compound.qualified_type.clone(),
        compound.title.clone(),
    );
    let mut pieces = Pieces::empty();
    for (rank, (_, geometry)) in horizons().into_iter().enumerate() {
        let name = if rank == 0 {
            "horizons".to_string()
        } else {
            format!("horizons {}", rank + 1)
        };
        pieces = pieces
            .with(
                name.clone(),
                Source::Shape {
                    geometry,
                    closed: None,
                },
            )
            .with(
                format!("{name} (object)"),
                Source::Object(Known {
                    crs: Some(compound.clone()),
                    vertical_uom: Some("m".into()),
                    vertical_up: Some(true),
                    ..Default::default()
                }),
            );
    }
    let grid = registry
        .plan(
            "ijk-grid-from-horizons",
            &pieces,
            &Choices::new()
                .set("layers", json!([2, 0, 1]))
                .set("axis", "elevation")
                .set("title", "Grid"),
            &Identity::derived(),
            &when(),
        )
        .unwrap_or_else(|why| panic!("{}", why.message()));
    apply(&grid, &mut store);
    let grid_uuid = grid.of_role("representation").next().unwrap().uuid.clone();
    let line = path();
    let n = line.len() as u32;
    let trace = Pieces::empty()
        .with(
            "path",
            Source::Shape {
                geometry: Geometry {
                    kind: "polyline",
                    patches: vec![Patch {
                        index: 0,
                        node_offset: 0,
                        points: line.clone(),
                        topology: Topology::Polylines(vec![n]),
                        closed: Vec::new(),
                    }],
                },
                closed: None,
            },
        )
        .with(
            "path (object)",
            Source::Object(Known {
                crs: Some(compound.clone()),
                vertical_up: Some(true),
                vertical_uom: Some("m".into()),
                ..Default::default()
            }),
        );
    let well = registry
        .plan(
            "well-from-polyline",
            &trace,
            &Choices::new().set("ground", 6.0).set("title", "GTH-1"),
            &Identity::derived(),
            &when(),
        )
        .unwrap_or_else(|why| panic!("{}", why.message()));
    apply(&well, &mut store);
    let frame_uuid = well.of_role("frame").next().unwrap().uuid.clone();
    let trajectory_uuid = well.of_role("trajectory").next().unwrap().uuid.clone();
    let first = dir.join("first.epc");
    store.save_as(&first, &SaveOptions::default()).unwrap();

    // -- reread, and what is built on it ------------------------------------------------
    let mut store = EpcStore::open(&first, schemas.clone()).unwrap();
    store.external_files_mut().set_root(&dir);
    let mut pieces = Pieces::empty();
    for (name, source) in read::in_slot("frame", read::pieces_of_object(&store, &frame_uuid).unwrap()) {
        pieces = pieces.with(name, source);
    }
    for (name, source) in read::in_slot("grids", read::pieces_of_object(&store, &grid_uuid).unwrap()) {
        pieces = pieces.with(name, source);
    }
    let blocked = registry
        .plan(
            "blocked-wellbore",
            &pieces,
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .unwrap_or_else(|why| panic!("{}", why.message()));
    apply(&blocked, &mut store);
    let blocked_object = blocked.of_role("representation").next().unwrap().clone();

    let mut pieces = Pieces::empty();
    for (name, source) in read::in_slot("support", read::pieces_of_object(&store, &grid_uuid).unwrap()) {
        pieces = pieces.with(name, source);
    }
    let sub = registry
        .plan(
            "subrepresentation",
            &pieces,
            &Choices::new()
                .set("select", "ijk-box")
                .set("kFirst", 2)
                .set("title", "Lower zone"),
            &Identity::derived(),
            &when(),
        )
        .unwrap_or_else(|why| panic!("{}", why.message()));
    apply(&sub, &mut store);
    let sub_uuid = sub.of_role("representation").next().unwrap().uuid.clone();

    let grid_known = read::known_of_object(&store, &grid_uuid).unwrap();
    let blocked_known = Known {
        reference: Some(Ref::new(
            blocked_object.uuid.clone(),
            blocked_object.qualified_type.clone(),
            blocked_object.title.clone(),
        )),
        ..Default::default()
    };
    let members = Pieces::empty()
        .with("representations", Source::Object(grid_known))
        .with("representations 2", Source::Object(blocked_known));
    let set = registry
        .plan(
            "representation-set",
            &members,
            &Choices::new().set("title", "Grid and well"),
            &Identity::derived(),
            &when(),
        )
        .unwrap_or_else(|why| panic!("{}", why.message()));
    apply(&set, &mut store);
    let second = dir.join("second.epc");
    store.save_as(&second, &SaveOptions::default()).unwrap();

    // -- reopened: checked ------------------------------------------------------------------
    let store = EpcStore::open(&second, schemas).unwrap();
    let report = energyml_domain::check::of_store(
        &store,
        &energyml_domain::check::Options {
            schema: true,
            rules: true,
            extra_rules: Vec::new(),
        },
    );
    assert!(report.read >= 8, "objects were read: {}", report.read);
    assert_eq!(
        (report.errors(), report.warnings()),
        (0, 0),
        "{:#?}",
        (
            &report.schema,
            &report.rules,
            &report.references,
            &report.relationships,
            &report.diagnostics,
            &report.unreadable
        )
    );
    let object = |uuid: &str| {
        let summary = store
            .find_by_uuid(uuid)
            .unwrap_or_else(|| panic!("{uuid} is in the package"))
            .clone();
        store.get_object(&summary.uri).unwrap()
    };

    // -- the blocked wellbore, from the file, against arithmetic --------------------------
    let read = object(&blocked_object.uuid);
    let json = energyml_core::json::write_value(read.as_reflect());
    assert_eq!(
        json["Trajectory"]["Uuid"],
        json!(trajectory_uuid),
        "the frame's trajectory"
    );
    let cells_part = child(read.as_reflect(), "IntervalGridCells");
    let ints =
        |name: &str| energyml_geometry::values::read_integers(child(cells_part, name), &store, name).unwrap();
    let arrays = Arrays {
        node_md: energyml_geometry::values::read_doubles(
            child(read.as_reflect(), "NodeMd"),
            &store,
            "NodeMd",
        )
        .unwrap(),
        grids: ints("GridIndices"),
        cells: ints("CellIndices"),
        faces: ints("LocalFacePairPerCellIndices"),
    };
    // The frame's MD as the file states them, on the line as it was drawn:
    // `well-from-polyline` measures from the first point, at the ground.
    let frame = object(&frame_uuid);
    let stated =
        energyml_geometry::values::read_doubles(child(frame.as_reflect(), "NodeMd"), &store, "NodeMd")
            .unwrap();
    assert_eq!(stated.len(), line.len());
    confront(
        &arrays,
        json["IntervalGridCells"]["CellCount"].as_i64().unwrap(),
        &line,
        &stated,
    );
    assert!(arrays.grids.contains(&-1), "the K gap is crossed");

    // -- the sub-representation, drawn, against the grid it selects from -------------------
    let drawn = energyml_geometry::scene::object_of(object(&sub_uuid).as_reflect(), &store, "sub")
        .expect("the sub-representation draws");
    let grid_drawn = energyml_geometry::scene::object_of(object(&grid_uuid).as_reflect(), &store, "grid")
        .expect("the grid draws");
    let sub_read = object(&sub_uuid);
    let patch = child(sub_read.as_reflect(), "SubRepresentationPatch");
    let written =
        energyml_geometry::values::read_integers(child(patch, "Indices"), &store, "Indices").unwrap();
    // The lower zone, k = 2: 3 × 2 cells, in RESQML order.
    assert_eq!(written, (12..18).collect::<Vec<i64>>());
    let centroid = |patch: &Patch, cell: &[u32; 8]| -> [f64; 3] {
        std::array::from_fn(|axis| cell.iter().map(|n| patch.points[*n as usize][axis]).sum::<f64>() / 8.0)
    };
    let [grid_patch] = grid_drawn.geometry.patches.as_slice() else {
        panic!("one patch")
    };
    let Topology::Hexahedra {
        cells: grid_cells,
        index,
        ..
    } = &grid_patch.topology
    else {
        panic!("cells")
    };
    assert!(index.is_empty(), "every cell is defined");
    let [sub_patch] = drawn.geometry.patches.as_slice() else {
        panic!("one patch")
    };
    let Topology::Hexahedra { cells: sub_cells, .. } = &sub_patch.topology else {
        panic!("cells")
    };
    assert_eq!(sub_cells.len(), written.len());
    for (cell, at) in sub_cells.iter().zip(&written) {
        let (a, b) = (
            centroid(sub_patch, cell),
            centroid(grid_patch, &grid_cells[*at as usize]),
        );
        assert!(
            (0..3).all(|axis| (a[axis] - b[axis]).abs() < 1e-9),
            "cell {at}: drawn at {a:?}, the grid's at {b:?}"
        );
        // And by arithmetic: the lower zone.
        assert_eq!(by_arithmetic(a), Some(*at as u64));
    }
    let _ = std::fs::remove_dir_all(&dir);
}
