//! `local-grid` - a refined window of an `IjkGridRepresentation`, its
//! geometry derived from the parent's (the reference model's local grid:
//! `LocalGridSet` + a child `IjkGridRepresentation` - window I
//! 10-13, J 8-11, all K, refined 2 × 2 × 1 → 8 × 8 × 10 = 640;
//! `IjkParentWindow` (TRG §5.31 p. 127 ◐), and
//! `ParentWindow.ParentIjkGridRepresentation` 1..1 pointing at the parent grid).
//!
//! The mother grid here is a small synthetic corner-point box built by hand,
//! its cells laid the way the geometry reader lays them (`cells_of`) - and, to
//! stand for the Chalk grid, the same box with a K gap (`gapped_mother`) or a fault
//! (`faulted_mother`). Its three coordinates
//! are affine in `(i, j, k)` - `X0 + DX·i + 0.1·k`, and so on - on purpose:
//! trilinear interpolation reproduces an affine function **exactly** at any
//! fractional position, so `expected()` below, evaluated independently of the
//! operation at the child's own fractional coordinates, is a real check on
//! the arithmetic - an axis swap, a wrong pillar formula or an off-by-one
//! would show up as a mismatch at generic interior nodes, not only at the
//! corners.
//!
//! # The load-bearing check is a triangulation
//!
//! `the_child_reads_back_through_the_geometry_reader` (behind `feature =
//! "hdf5"`) writes the plan's objects into a real `EpcStore`, reopens it, and
//! reads the child back through `energyml_geometry::scene::object_of` -
//! which, for an `IjkGridRepresentation` with a `ParentWindow` and no
//! `Geometry`, delegates to `crates/energyml-geometry/src/parent.rs`: a
//! second, independent trilinear interpolation over a real HDF5 hyperslab of
//! the parent's `Points`, sharing no code with this operation's own. The two
//! must agree on every node.
//!
//! # What would have to break to make each test turn red
//!
//! Confusing `i` and `j` in `ParentGrid::at` turns the coincidence at the
//! edges and the interpolated midpoints red; loosening `hi >= extent` into `hi > extent`
//! turns the refusal of a window outside the parent red; removing the call to
//! `first_undefined` turns the refusal of an undefined corner red. The three were
//! broken one by one, each turned red exactly the test that names it, and
//! the code was put back in order.

use serde_json::{Value, json};

use energyml_build::operation::{Choices, Identity, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

/// The parent's cell counts - small, but more than one refined cell wide on
/// every axis, so a transposed axis or a wrong step would still show.
const PNI: usize = 6;
const PNJ: usize = 5;
const PNK: usize = 3;

const X0: f64 = 651_000.0;
const Y0: f64 = 6_861_000.0;
const Z0: f64 = 20.0;
const DX: f64 = 30.0;
const DY: f64 = 25.0;
const DZ: f64 = 12.0;

fn when() -> When {
    When {
        creation: "2026-09-28T00:00:00Z".into(),
        originator: "test".into(),
    }
}

/// The parent's corner at pillar `(i, j)`, level `k` - affine in `(i, j, k)`,
/// and each coordinate reads a different mix of the three so an axis mix-up
/// cannot hide behind another axis agreeing by coincidence.
fn corner(i: usize, j: usize, k: usize) -> [f64; 3] {
    let (i, j, k) = (i as f64, j as f64, k as f64);
    [
        X0 + DX * i + 0.1 * k,
        Y0 + DY * j + 0.2 * k,
        Z0 - DZ * k + 0.05 * i - 0.03 * j,
    ]
}

/// The same corner, evaluated at a **fractional** parent position - what a
/// correct trilinear interpolation of an affine field must equal, computed
/// here independently of `crates/energyml-build/src/ops/local_grid.rs`.
fn expected(i: f64, j: f64, k: f64) -> [f64; 3] {
    [
        X0 + DX * i + 0.1 * k,
        Y0 + DY * j + 0.2 * k,
        Z0 - DZ * k + 0.05 * i - 0.03 * j,
    ]
}

const PILLARS: usize = (PNI + 1) * (PNJ + 1);

/// Cells as `energyml_geometry::ijk::read` lays them: RESQML order (i fastest,
/// then j, then k), each the four pillars of its column - `(i, j)`,
/// `(i + 1, j)`, `(i + 1, j + 1)`, `(i, j + 1)` - on its top level, then the
/// same four on its base level. `tops[k]` is the level layer `k`'s top sits
/// at (one more past a K gap), and `line_of(pillar, column)` the coordinate
/// line a column uses at a pillar - a split line on the far side of a fault.
fn cells_of(lines: usize, tops: &[usize], line_of: impl Fn(usize, usize) -> usize) -> Vec<[u32; 8]> {
    let mut cells = Vec::new();
    for &top in tops {
        for j in 0..PNJ {
            for i in 0..PNI {
                let column = j * PNI + i;
                let mut node = [0u32; 8];
                for (at, (ci, cj)) in [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)]
                    .into_iter()
                    .enumerate()
                {
                    let line = line_of(cj * (PNI + 1) + ci, column);
                    node[at] = (top * lines + line) as u32;
                    node[at + 4] = ((top + 1) * lines + line) as u32;
                }
                cells.push(node);
            }
        }
    }
    cells
}

fn hexahedra(points: Vec<[f64; 3]>, cells: Vec<[u32; 8]>) -> Geometry {
    Geometry {
        kind: "IjkGridRepresentation",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points,
            topology: Topology::Hexahedra {
                cells,
                dimensions: [PNI, PNJ, PNK],
                index: Vec::new(),
            },
            closed: Vec::new(),
        }],
    }
}

/// The points of `levels` levels, `corner()` read at each level.
fn levels_of(levels: usize) -> Vec<[f64; 3]> {
    let mut points = Vec::with_capacity(levels * PILLARS);
    for level in 0..levels {
        for j in 0..=PNJ {
            for i in 0..=PNI {
                points.push(corner(i, j, level));
            }
        }
    }
    points
}

fn mother_geometry() -> Geometry {
    hexahedra(
        levels_of(PNK + 1),
        cells_of(PILLARS, &(0..PNK).collect::<Vec<_>>(), |pillar, _| pillar),
    )
}

/// The mother with a **K gap after layer 0**: `PNK + 2` levels, layer 0
/// between levels 0 and 1, the void between 1 and 2, layer `k ≥ 1` between
/// `k + 1` and `k + 2` - so `corner(i, j, level)` puts layer 1's top at 2.
fn gapped_mother() -> Geometry {
    let tops: Vec<usize> = (0..PNK).map(|k| if k == 0 { 0 } else { k + 1 }).collect();
    hexahedra(levels_of(PNK + 2), cells_of(PILLARS, &tops, |pillar, _| pillar))
}

/// The mother with **a fault at pillar `(pi, pj)`**: one split coordinate
/// line, 5 m down-thrown, appended after the regular pillars of every level
/// the way the file stores it, and used by the two columns east of the pillar,
/// `(pi, pj - 1)` and `(pi, pj)`.
fn faulted_mother(pi: usize, pj: usize) -> Geometry {
    let lines = PILLARS + 1;
    let split = pj * (PNI + 1) + pi;
    let mut points = Vec::with_capacity((PNK + 1) * lines);
    for level in 0..=PNK {
        for j in 0..=PNJ {
            for i in 0..=PNI {
                points.push(corner(i, j, level));
            }
        }
        let [x, y, z] = corner(pi, pj, level);
        points.push([x, y, z - 5.0]);
    }
    let east = [(pj - 1) * PNI + pi, pj * PNI + pi];
    let cells = cells_of(lines, &(0..PNK).collect::<Vec<_>>(), |pillar, column| {
        if pillar == split && east.contains(&column) {
            PILLARS
        } else {
            pillar
        }
    });
    hexahedra(points, cells)
}

fn bag_of(geometry: Geometry) -> Pieces {
    Pieces::empty()
        .with(
            "parent",
            Source::Shape {
                geometry,
                closed: None,
            },
        )
        .with(
            "parent (object)",
            Source::Object(Known {
                reference: Some(mother_ref()),
                ..Default::default()
            }),
        )
}

fn mother_ref() -> Ref {
    Ref::new(
        "aaaaaaaa-0000-4000-8000-000000000001",
        "resqml22.IjkGridRepresentation",
        "Mother grid",
    )
}

fn bag() -> Pieces {
    bag_of(mother_geometry())
}

/// The window, brought down to this mother's size: I 1-3, J 1-2 (spans 3
/// and 2), every K (0-2, span 3), refined 2 × 2 × 1 - 6 × 4 × 3 = 72 cells.
fn answers() -> Choices {
    Choices::new()
        .set("i0", 1)
        .set("i1", 3)
        .set("j0", 1)
        .set("j1", 2)
        .set("ri", 2)
        .set("rj", 2)
        .set("title", "GTH-1 local grid")
}

fn planned(pieces: &Pieces, choices: &Choices) -> Result<Plan, String> {
    Registry::new()
        .plan("local-grid", pieces, choices, &Identity::derived(), &when())
        .map_err(|why| why.message())
}

fn the_child(plan: &Plan) -> &Value {
    &plan
        .of_role("representation")
        .next()
        .expect("the local grid")
        .json
}

fn the_set(plan: &Plan) -> &Value {
    &plan
        .of_role("local-grid-set")
        .next()
        .expect("the local grid set")
        .json
}

fn says(plan: &Plan, key: &str) -> bool {
    plan.says.iter().any(|note| note.key == key)
}

/// The plan's own computed node at `(ci, cj, ck)` of a `pi`-wide, `pj`-wide
/// child.
fn node(points: &[[f64; 3]], child: [usize; 2], ci: usize, cj: usize, ck: usize) -> [f64; 3] {
    let [pi, pj] = child;
    points[(ck * (pj + 1) + cj) * (pi + 1) + ci]
}

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|axis| (a[axis] - b[axis]).abs() < 1e-9)
}

// -- counts and the window's own arithmetic ----------------------------------------------

/// **`Ni`, `Nj`, `Nk` come from the window and the refinement**: span 3 × 2 × 3
/// parent cells, refined 2 × 2 × 1, is 6 × 4 × 3 child cells.
#[test]
fn the_counts_come_from_the_window_and_the_refinement() {
    let plan = planned(&bag(), &answers()).unwrap_or_else(|why| panic!("{why}"));
    let child = the_child(&plan);
    assert_eq!(child["$type"], json!("resqml22.IjkGridRepresentation"));
    assert_eq!(
        (child["Ni"].clone(), child["Nj"].clone(), child["Nk"].clone()),
        (json!(6), json!(4), json!(3))
    );
    assert!(
        child.get("Geometry").is_none(),
        "a local grid stores no geometry of its own: {child}"
    );

    let window = &child["ParentWindow"];
    assert_eq!(window["$type"], json!("resqml22.IjkParentWindow"));
    assert_eq!(
        window["ParentIjkGridRepresentation"]["Uuid"],
        json!(mother_ref().uuid)
    );
    // One interval per parent cell, one parent and `ri` children in each: *“
    // writers should set the parent count per interval to 1”* (RESQML TUG 2.2
    // §9.8.2, p. 142).
    assert_eq!(window["IRegrid"]["InitialIndexOnParentGrid"], json!(1));
    assert_eq!(window["IRegrid"]["Intervals"]["IntervalCount"], json!(3));
    assert_eq!(
        window["IRegrid"]["Intervals"]["ParentCountPerInterval"],
        json!({ "$type": "eml23.IntegerConstantArray", "Value": 1, "Count": 3 })
    );
    assert_eq!(
        window["IRegrid"]["Intervals"]["ChildCountPerInterval"],
        json!({ "$type": "eml23.IntegerConstantArray", "Value": 2, "Count": 3 })
    );
    assert_eq!(window["JRegrid"]["InitialIndexOnParentGrid"], json!(1));
    assert_eq!(window["JRegrid"]["Intervals"]["IntervalCount"], json!(2));
    assert_eq!(
        window["JRegrid"]["Intervals"]["ChildCountPerInterval"],
        json!({ "$type": "eml23.IntegerConstantArray", "Value": 2, "Count": 2 })
    );
    // K is not refined (`rk` defaults to 1): a 1:1 correspondence needs no
    // `Intervals` at all (Grids.xsd).
    assert_eq!(window["KRegrid"]["InitialIndexOnParentGrid"], json!(0));
    assert!(
        window["KRegrid"].get("Intervals").is_none(),
        "no refinement on K: {window}"
    );

    let set = the_set(&plan);
    assert_eq!(set["$type"], json!("resqml22.LocalGridSet"));
    assert_eq!(
        set["ChildGrid"][0]["Uuid"], child["uuid"],
        "the set names the child it groups"
    );

    assert!(says(&plan, "all-k"), "no K window was given: {:?}", plan.says);
    assert_eq!(
        plan.shapes[0].geometry.patches[0].points.len(),
        7 * 5 * 4,
        "(ni+1)(nj+1)(nk+1)"
    );
    let Topology::Hexahedra {
        cells, dimensions, ..
    } = &plan.shapes[0].geometry.patches[0].topology
    else {
        panic!("not cells")
    };
    assert_eq!(*dimensions, [6, 4, 3]);
    assert_eq!(cells.len(), 6 * 4 * 3);
}

// -- the boundary of the window coincides exactly with the parent's nodes ----------------

/// **Every node on the window's boundary is the parent's own corner, bit for
/// bit** - no interpolation error at the seam a reader must stitch to the
/// parent grid.
#[test]
fn the_windows_boundary_coincides_exactly_with_the_parent() {
    let plan = planned(&bag(), &answers()).unwrap();
    let points = &plan.shapes[0].geometry.patches[0].points;
    // Corners of the child box: (ci, cj, ck) at 0 or the max map to parent
    // pillars (1 or 4, i.e. i0 or i1+1) and (1 or 3), (0 or 3) - i.e. i0..=i1+1
    // and so on.
    for &(ci, i) in &[(0usize, 1usize), (6, 4)] {
        for &(cj, j) in &[(0usize, 1usize), (4, 3)] {
            for &(ck, k) in &[(0usize, 0usize), (3, 3)] {
                let got = node(points, [6, 4], ci, cj, ck);
                let want = corner(i, j, k);
                assert_eq!(
                    got, want,
                    "child ({ci},{cj},{ck}) should be exactly parent pillar ({i},{j}) level {k}"
                );
            }
        }
    }
    // A boundary node that is not a corner of the box - e.g. the middle of an
    // edge, ci = 2 (parent i = 1 + 2/2 = 2, an integer pillar), cj = 0 - still
    // lands exactly on the parent's node, since I is refined but J is not
    // interpolated away from cj = 0.
    assert_eq!(node(points, [6, 4], 2, 0, 0), corner(2, 1, 0));
}

/// **A node strictly inside the window is the parent's lattice, trilinearly
/// interpolated** - checked against `expected()`, computed independently of
/// the operation, at the exact fractional parent position the child node asks
/// for.
#[test]
fn an_interior_node_is_the_parents_lattice_interpolated() {
    let plan = planned(&bag(), &answers()).unwrap();
    let points = &plan.shapes[0].geometry.patches[0].points;
    // ci = 1 -> halfway across the first refined parent cell on I: i = 1.5.
    // cj = 1 -> halfway on J: j = 1.5. ck = 1 -> the second of three levels
    // (K is not refined): k = 1.0 exactly, still worth checking against the
    // same formula.
    let got = node(points, [6, 4], 1, 1, 1);
    assert!(
        close(got, expected(1.5, 1.5, 1.0)),
        "got {got:?}, expected {:?}",
        expected(1.5, 1.5, 1.0)
    );
    let got = node(points, [6, 4], 5, 3, 2);
    assert!(
        close(got, expected(3.5, 2.5, 2.0)),
        "got {got:?}, expected {:?}",
        expected(3.5, 2.5, 2.0)
    );
}

// -- refusals, before any computation ------------------------------------------------------

/// **A window past the parent's extent is refused by name.** The parent has 6
/// cells along I (0..6); `i1 = 6` names a seventh.
#[test]
fn a_window_past_the_parents_extent_is_refused() {
    let why = planned(&bag(), &answers().set("i1", 6)).unwrap_err();
    assert!(why.contains("`i1`") && why.contains("6 cell(s)"), "{why}");
}

/// **A reversed window is refused by name**, before the extent is even
/// checked.
#[test]
fn a_reversed_window_is_refused() {
    let why = planned(&bag(), &answers().set("j1", 0)).unwrap_err();
    assert!(why.contains("`j1`") && why.contains("before it starts"), "{why}");
}

/// **A parent cell corner with no height is refused by name**, naming the
/// pillar and the level - never a silently propagated `NaN`.
#[test]
fn an_undefined_parent_corner_is_refused() {
    let mut geometry = mother_geometry();
    // Pillar (2, 2), level 1 - inside the window's span (I 1..=3, J 1..=2, K
    // 0..=2), so a cell the window covers touches it.
    let level = 1;
    let at = (level * (PNJ + 1) + 2) * (PNI + 1) + 2;
    geometry.patches[0].points[at][2] = f64::NAN;
    let why = planned(&bag_of(geometry), &answers()).unwrap_err();
    assert!(
        why.contains("`parent`") && why.contains("(2, 2)") && why.contains("level 1"),
        "{why}"
    );
}

// -- faults and K gaps: refused inside the window, nothing outside it -----------------------

/// **A fault inside the window is refused, naming its pillar.** Pillar
/// (3, 2) is split, and the window (I 1-3, J 1-2) holds columns on both sides
/// of it: the child's node there would have one position per side.
#[test]
fn a_fault_inside_the_window_is_refused() {
    let why = planned(&bag_of(faulted_mother(3, 2)), &answers()).unwrap_err();
    assert!(
        why.contains("`parent`") && why.contains("split coordinate line") && why.contains("(3, 2)"),
        "{why}"
    );
}

/// **A fault outside the window changes nothing**: the window's cells name the
/// points they use, so the child is the unfaulted one, node for node. The Chalk grid is
/// faulted along F1, and GTH-1's window is clear of it.
#[test]
fn a_fault_outside_the_window_changes_nothing() {
    let plain = planned(&bag(), &answers()).unwrap_or_else(|why| panic!("{why}"));
    let faulted = planned(&bag_of(faulted_mother(5, 4)), &answers()).unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(
        faulted.shapes[0].geometry.patches[0].points,
        plain.shapes[0].geometry.patches[0].points
    );
}

/// **A K gap inside the window is refused, naming the two layers.** Every K is
/// the default, and the gap after layer 0 is between two of them.
#[test]
fn a_k_gap_inside_the_window_is_refused() {
    let why = planned(&bag_of(gapped_mother()), &answers()).unwrap_err();
    assert!(
        why.contains("`parent`") && why.contains("K gap") && why.contains("layers 0 and 1"),
        "{why}"
    );
}

/// **Below a K gap, the window starts at its first layer's top - a level, not
/// the layer's index.** Layers 1-2 of the gapped mother sit at levels 2-4, so
/// the child's three levels are 2, 3 and 4; the layer's index would have put
/// its top at level 1, inside the void.
#[test]
fn below_a_k_gap_the_window_starts_at_its_layers_top() {
    let plan = planned(&bag_of(gapped_mother()), &answers().set("k0", 1).set("k1", 2))
        .unwrap_or_else(|why| panic!("{why}"));
    let points = &plan.shapes[0].geometry.patches[0].points;
    assert_eq!(points.len(), 7 * 5 * 3);
    for ck in 0..=2 {
        assert_eq!(
            node(points, [6, 4], 0, 0, ck),
            corner(1, 1, 2 + ck),
            "level {ck} of the child"
        );
        let got = node(points, [6, 4], 3, 1, ck);
        assert!(
            close(got, expected(2.5, 1.5, (2 + ck) as f64)),
            "interior node at child level {ck}: {got:?}"
        );
    }
}

/// **A cell turned for VTK reads the same.** The reader puts a cell's base
/// face first when that is the winding VTK asks for; the window tells the top
/// by the point index, never by the order.
#[test]
fn a_cell_turned_for_vtk_reads_the_same() {
    let mut turned = mother_geometry();
    let Topology::Hexahedra { cells, .. } = &mut turned.patches[0].topology else {
        unreachable!()
    };
    for cell in cells.iter_mut() {
        *cell = [
            cell[4], cell[5], cell[6], cell[7], cell[0], cell[1], cell[2], cell[3],
        ];
    }
    let plain = planned(&bag(), &answers()).unwrap_or_else(|why| panic!("{why}"));
    let plan = planned(&bag_of(turned), &answers()).unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(
        plan.shapes[0].geometry.patches[0].points,
        plain.shapes[0].geometry.patches[0].points
    );
}

/// **A cell the reader dropped, inside the window, is refused by name.** The
/// reader drops a cell with no location and numbers the rest (`index`); the
/// window finds its cells by that number, and one missing is no position to
/// interpolate.
#[test]
fn a_cell_dropped_by_the_reader_inside_the_window_is_refused() {
    let mut geometry = mother_geometry();
    let Topology::Hexahedra { cells, index, .. } = &mut geometry.patches[0].topology else {
        unreachable!()
    };
    let dropped = PNI * PNJ + PNI + 2; // cell (2, 1, 1)
    cells.remove(dropped);
    *index = (0..(PNI * PNJ * PNK) as u32)
        .filter(|n| *n as usize != dropped)
        .collect();
    let why = planned(&bag_of(geometry), &answers()).unwrap_err();
    assert!(why.contains("no geometry at cell (2, 1, 1)"), "{why}");
}

// -- the reader, confronted ----------------------------------------------------------------

/// **The geometry reader reads the same nodes this operation planned.** The
/// mother is written into a real package, with a real HDF5 `Points` array, and
/// **read back through the geometry reader** - what the operation is given in
/// a recipe, cells laid and turned the reader's way, not a shape built here.
/// The local grid planned from it is applied beside it, the package saved and
/// reopened, and the child read back through
/// `energyml_geometry::scene::object_of` - which, for an
/// `IjkGridRepresentation` with a `ParentWindow` and no `Geometry`, is
/// `crates/energyml-geometry/src/parent.rs`: a hyperslab of the parent's real
/// `Points`, trilinearly interpolated by code that shares nothing with
/// `local-grid`'s own. The two must agree on every node, and both with
/// `expected()`.
#[cfg(feature = "hdf5")]
struct Trip {
    /// The nodes the operation planned.
    planned: Vec<[f64; 3]>,
    /// The nodes the geometry reader reads back.
    read: Vec<[f64; 3]>,
    /// The child's dimensions, as read.
    dimensions: [usize; 3],
}

#[cfg(feature = "hdf5")]
fn round_trip(gapped: bool, choices: &Choices, name: &str) -> Trip {
    use std::sync::Arc;

    let dir = std::env::temp_dir().join(format!("energyml-build-local-grid-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("local_grid.epc");
    let registry = Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "local_grid.epc", registry.clone()).unwrap();
    store.external_files_mut().set_root(&dir);

    // -- the mother, written by hand: a real IJK grid with a real Points array --------
    let levels = if gapped { PNK + 2 } else { PNK + 1 };
    let mother_uuid = mother_ref().uuid;
    let mother_points: Vec<f64> = levels_of(levels).iter().flatten().copied().collect();
    let points_path = format!("/resqml22/{mother_uuid}/points");
    let mut mother_json = json!({
        "$type": "resqml22.IjkGridRepresentation",
        "uuid": mother_uuid,
        "schemaVersion": "2.2",
        "Citation": { "$type": "eml23.Citation", "Title": "Mother grid", "Originator": "test", "Creation": when().creation },
        "Ni": PNI, "Nj": PNJ, "Nk": PNK,
        "Geometry": {
            "$type": "resqml22.IjkGridGeometry",
            "LocalCrs": {
                "$type": "eml23.DataObjectReference",
                "Uuid": "bbbbbbbb-0000-4000-8000-000000000002",
                "QualifiedType": "eml23.LocalEngineeringCompoundCrs",
                "Title": "no frame given",
            },
            "Points": {
                "$type": "resqml22.Point3dExternalArray",
                "Coordinates": {
                    "$type": "eml23.ExternalDataArray",
                    "ExternalDataArrayPart": [{
                        "$type": "eml23.ExternalDataArrayPart",
                        "Count": [levels, PNJ + 1, PNI + 1, 3],
                        "StartIndex": [0, 0, 0, 0],
                        "MimeType": "application/x-hdf5",
                        "PathInExternalFile": points_path,
                        "URI": "local_grid.h5",
                    }],
                },
            },
            "CellGeometryIsDefined": { "$type": "eml23.BooleanConstantArray", "Value": true, "Count": PNI * PNJ * PNK },
            "KDirection": "down",
            "PillarGeometryIsDefined": { "$type": "eml23.BooleanConstantArray", "Value": true, "Count": (PNI + 1) * (PNJ + 1) },
            "PillarShape": "vertical",
            "GridIsRighthanded": false,
        },
    });
    if gapped {
        // One flag per interface between layers (`Nk - 1`), the gap after layer 0.
        let mut after = vec![false; PNK - 1];
        after[0] = true;
        mother_json["KGaps"] = json!({
            "$type": "resqml22.KGaps",
            "Count": 1,
            "GapAfterLayer": { "$type": "eml23.BooleanXmlArray", "CountPerValue": 1, "Values": after },
        });
    }
    let mother_plan = Plan {
        media: Vec::new(),
        blocked: None,
        objects: vec![energyml_build::operation::NewObject {
            uuid: mother_uuid.clone(),
            qualified_type: "resqml22.IjkGridRepresentation".to_string(),
            role: "representation".to_string(),
            title: "Mother grid".to_string(),
            json: mother_json,
        }],
        datasets: vec![energyml_build::operation::Dataset {
            file: "local_grid.h5".to_string(),
            path: points_path,
            dimensions: vec![levels as i64, (PNJ + 1) as i64, (PNI + 1) as i64, 3],
            values: energyml_build::operation::Numbers::F64(mother_points),
        }],
        shapes: Vec::new(),
        activity: energyml_build::operation::ActivityDraft {
            recipe: "local-grid",
            title: "mother".to_string(),
            seed: "mother".to_string(),
            parameters: Vec::new(),
        },
        says: Vec::new(),
        dropped: Vec::new(),
    };
    // `local-grid` has its canonical template (in the shipped templates,
    // 28/09), so both applies record their activity - and a refusal is a
    // failure, not something to read past.
    energyml_build::apply::apply(&mother_plan, &mut store, &when())
        .activity
        .expect("the mother records its activity");

    // -- the mother as the reader hands it, and the local grid planned from it --------
    let summary = store
        .find_by_uuid(&mother_uuid)
        .cloned()
        .expect("the mother was written");
    let object = store.get_object(&summary.uri).unwrap();
    let mother = energyml_geometry::scene::object_of(object.as_reflect(), &store, &summary.uri)
        .expect("the mother reads");
    let plan = planned(&bag_of(mother.geometry), choices).unwrap_or_else(|why| panic!("{why}"));
    let planned_points = plan.shapes[0].geometry.patches[0].points.clone();
    let child_uuid = the_child(&plan)["uuid"].as_str().unwrap().to_string();
    energyml_build::apply::apply(&plan, &mut store, &when())
        .activity
        .expect("the local grid records its activity");

    store
        .save_as(&out, &energyml_storage::SaveOptions::default())
        .unwrap();
    let reread = energyml_storage::EpcStore::open(&out, registry).unwrap();
    let summary = reread.find_by_uuid(&child_uuid).cloned().unwrap();
    let object = reread.get_object(&summary.uri).unwrap();
    let built = energyml_geometry::scene::object_of(object.as_reflect(), &reread, &summary.uri)
        .expect("the local grid reads");
    let _ = std::fs::remove_dir_all(&dir);

    let [patch] = built.geometry.patches.as_slice() else {
        panic!("one patch")
    };
    let Topology::Hexahedra {
        cells, dimensions, ..
    } = &patch.topology
    else {
        panic!("not cells: {:?}", patch.topology)
    };
    assert_eq!(cells.len(), dimensions.iter().product::<usize>());
    Trip {
        planned: planned_points,
        read: patch.points.clone(),
        dimensions: *dimensions,
    }
}

#[cfg(feature = "hdf5")]
#[test]
fn the_child_reads_back_through_the_geometry_reader() {
    let Trip {
        planned: planned_points,
        read,
        dimensions,
    } = round_trip(false, &answers(), "plain");
    assert_eq!(dimensions, [6, 4, 3]);
    assert_eq!(
        read.len(),
        planned_points.len(),
        "one node per position, both sides"
    );
    for (at, (read, planned)) in read.iter().zip(&planned_points).enumerate() {
        assert!(
            close(*read, *planned),
            "node {at}: read {read:?}, planned {planned:?}"
        );
    }
    // And both agree with the formula, not only with each other.
    assert!(close(node(&read, [6, 4], 1, 1, 1), expected(1.5, 1.5, 1.0)));
}

/// **The same below a K gap, written and read.** The window starts under the
/// gap (layers 1-2), so the reader must find it at level 2 of the file's
/// five: the two sides agree with each other **and** with `corner()` read at
/// levels 2-4 - a reader that took the layer for the level would agree with
/// neither.
#[cfg(feature = "hdf5")]
#[test]
fn below_a_k_gap_the_child_reads_back_through_the_geometry_reader() {
    let Trip {
        planned: planned_points,
        read,
        dimensions,
    } = round_trip(true, &answers().set("k0", 1).set("k1", 2), "gapped");
    assert_eq!(dimensions, [6, 4, 2]);
    assert_eq!(read.len(), planned_points.len());
    for (at, (read, planned)) in read.iter().zip(&planned_points).enumerate() {
        assert!(
            close(*read, *planned),
            "node {at}: read {read:?}, planned {planned:?}"
        );
    }
    for ck in 0..=2 {
        assert!(
            close(node(&read, [6, 4], 3, 1, ck), expected(2.5, 1.5, (2 + ck) as f64)),
            "child level {ck}"
        );
    }
}
