//! `faulted-ijk-grid` - an IJK grid between horizons, cut by fault surfaces
//! (the need: at least one grid that is “cut” by the
//! faults […] a little displaced downwards on one side, and with cells
//! removed near one or two faults).
//!
//! The form of the assertions comes from the standard:
//!
//! - split coordinate lines come **after** the pillars, each on the pillar
//!   `PillarIndices` names, used by the columns its row of
//!   `ColumnsPerSplitCoordinateLine` lists (RESQML TRG 2.2 §5.17 p. 111); a
//!   pillar of an IJK grid has at most four lines (RESQML Technical Usage Guide
//!   2.2, draft, §9.11.1 p. 151);
//! - the nodes are `NKL × (pillars + splits)`, with `NKL = NK + gapCount + 1`
//!   (TRG §5.1 p. 94, §5.36 p. 132; the usage guide's 4 × 3 × 2 example writes
//!   *“NODES = 24 x 3”* for 20 pillars and 4 splits, §9.19.2 p. 174);
//! - a cell whose `CellGeometryIsDefined` is false has no geometry, whatever its
//!   nodes say (TRG §5.1 p. 94) - so it is not drawn.
//!
//! The throw is this operation's stated law, the one `fault-throw` writes
//! (`maximum · 4s(1 − s) · (1 − d / reach)`), and the tests restate it from the
//! geometry they build rather than asking the code for it.
//!
//! The numbers are the tests' own: a lattice of 7 × 5 nodes, 25 m along x and
//! **40** along y (a transposition shows), at Lambert-93 coordinates, four
//! horizons with a K gap between the second and the third. Fault A strikes
//! north and dips 60° east; fault B strikes east and dips 70° north; they
//! cross, so one pillar has four lines.
//!
//! # What would have to break to make it turn red
//!
//! Keeping the wrong group on the pillar line, forgetting the offset of the
//! split lines in the points array, counting one dead column too many,
//! reading the roof side backwards, writing the split lines before the
//! pillars.

use serde_json::{Value, json};

use energyml_build::operation::{Choices, Identity, Numbers, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

const X0: f64 = 651_000.0;
const Y0: f64 = 6_861_000.0;
const DX: f64 = 25.0;
const DY: f64 = 40.0;
/// Lattice nodes along x, then along y.
const NI: usize = 7;
const NJ: usize = 5;
/// Cells.
const CI: usize = NI - 1;
const CJ: usize = NJ - 1;
const CK: usize = 4;
/// Levels: 4 layers, 1 gap.
const LEVELS: usize = CK + 1 + 1;
const PILLARS: usize = NI * NJ;

fn when() -> When {
    When {
        creation: "2026-09-28T00:00:00Z".into(),
        originator: "test".into(),
    }
}

fn frame() -> Ref {
    Ref::new(
        "cccccccc-0000-4000-8000-000000000003",
        "eml23.LocalEngineeringCompoundCrs",
        "The boreholes frame",
    )
}

fn other_frame() -> Ref {
    Ref::new(
        "dddddddd-0000-4000-8000-000000000004",
        "eml23.LocalEngineeringCompoundCrs",
        "The other frame",
    )
}

fn earth_model() -> Value {
    json!({
        "$type": "eml23.DataObjectReference",
        "Uuid": "eeeeeeee-1111-4000-8000-000000000101",
        "QualifiedType": "resqml22.EarthModelInterpretation",
        "Title": "Lutetia",
    })
}

/// The top of the stack dips gently east; the others are flat.
fn top(x: f64) -> f64 {
    5.0 - 0.02 * (x - X0)
}

/// The four horizons, top to bottom: `layers = [2, 0, 2]` - two layers, a gap,
/// two layers.
fn horizons() -> Vec<(&'static str, Geometry)> {
    let grid = |z: &dyn Fn(f64) -> f64| {
        let mut points = Vec::new();
        for j in 0..NJ {
            for i in 0..NI {
                let (x, y) = (X0 + DX * i as f64, Y0 + DY * j as f64);
                points.push([x, y, z(x)]);
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
        ("Top", grid(&top)),
        ("Second", grid(&|_| -20.0)),
        ("Third", grid(&|_| -30.0)),
        ("Base", grid(&|_| -60.0)),
    ]
}

/// The heights the grid spans: the top's highest node, the base.
fn grid_span() -> (f64, f64) {
    (-60.0, top(X0))
}

/// The fault's own vertical extent.
const FAULT_TOP: f64 = 50.0;
const FAULT_BOTTOM: f64 = -100.0;

/// Where the faults are traced: the middle of the heights they share with the
/// grid.
fn level() -> f64 {
    let (low, high) = grid_span();
    (low.max(FAULT_BOTTOM) + high.min(FAULT_TOP)) / 2.0
}

/// Fault A: a plane striking north, dipping 60° **east**, through x = X0 + 55
/// at the trace level, from y = Y0 − 80 to Y0 + 240. Its hanging wall is east:
/// columns i ≥ 2.
const A_X: f64 = 55.0;
const A_FROM: f64 = -80.0;
const A_TO: f64 = 240.0;
fn fault_a() -> Geometry {
    fault_a_from(FAULT_TOP, FAULT_BOTTOM)
}

/// Fault A, from `top` down to `bottom`: through x = X0 + 55 at the middle
/// of the heights it shares with the grid, wherever that is.
fn fault_a_from(top: f64, bottom: f64) -> Geometry {
    let (low, high) = grid_span();
    let traced = (low.max(bottom) + high.min(top)) / 2.0;
    let x = |z: f64| X0 + A_X + (traced - z) / 60f64.to_radians().tan();
    plane(vec![
        [x(top), Y0 + A_FROM, top],
        [x(top), Y0 + A_TO, top],
        [x(bottom), Y0 + A_FROM, bottom],
        [x(bottom), Y0 + A_TO, bottom],
    ])
}

/// Fault B: a plane striking east, dipping 70° **north**, through y = Y0 + 70
/// at the trace level, from x = X0 − 100 to X0 + 250. Its hanging wall is
/// north: rows j ≥ 2.
const B_Y: f64 = 70.0;
const B_FROM: f64 = -100.0;
const B_TO: f64 = 250.0;
fn fault_b() -> Geometry {
    let y = |z: f64| Y0 + B_Y + (level() - z) / 70f64.to_radians().tan();
    plane(vec![
        [X0 + B_FROM, y(FAULT_TOP), FAULT_TOP],
        [X0 + B_TO, y(FAULT_TOP), FAULT_TOP],
        [X0 + B_FROM, y(FAULT_BOTTOM), FAULT_BOTTOM],
        [X0 + B_TO, y(FAULT_BOTTOM), FAULT_BOTTOM],
    ])
}

fn plane(points: Vec<[f64; 3]>) -> Geometry {
    Geometry {
        kind: "triangles",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points,
            topology: Topology::Triangles(vec![[0, 1, 2], [1, 3, 2]]),
            closed: Vec::new(),
        }],
    }
}

/// The throws, the reach.
const A_THROW: f64 = 10.0;
const B_THROW: f64 = 6.0;
const REACH: f64 = 100.0;

/// The law, restated: `maximum · 4s(1 − s) · (1 − d / reach)`.
fn law(maximum: f64, s: f64, d: f64) -> f64 {
    if !(s > 0.0 && s < 1.0) || d >= REACH {
        return 0.0;
    }
    maximum * 4.0 * s * (1.0 - s) * (1.0 - d / REACH)
}

/// Fault A's displacement at pillar `(i, j)` for a hanging-wall column.
fn a_at(i: usize, j: usize) -> f64 {
    let (x, y) = (DX * i as f64, DY * j as f64);
    law(A_THROW, (y - A_FROM) / (A_TO - A_FROM), (x - A_X).abs())
}

/// Fault B's.
fn b_at(i: usize, j: usize) -> f64 {
    let (x, y) = (DX * i as f64, DY * j as f64);
    law(B_THROW, (x - B_FROM) / (B_TO - B_FROM), (y - B_Y).abs())
}

/// The name a piece takes in the bag: `horizons`, `horizons 2`, …
fn slot_name(slot: &str, rank: usize) -> String {
    if rank == 0 {
        slot.to_string()
    } else {
        format!("{slot} {}", rank + 1)
    }
}

fn known(uuid: String, qualified_type: &str, title: &str, frame: &Ref) -> Source {
    Source::Object(Known {
        reference: Some(Ref::new(uuid, qualified_type, title)),
        crs: Some(frame.clone()),
        vertical_uom: Some("m".into()),
        vertical_up: Some(true),
        ..Default::default()
    })
}

/// The horizons and the faults, each with what the package says of it.
fn bag_with(faults: &[(&str, Geometry)], fault_frame: &Ref) -> Pieces {
    let mut pieces = Pieces::empty();
    for (rank, (title, geometry)) in horizons().into_iter().enumerate() {
        pieces = pieces
            .with(
                slot_name("horizons", rank),
                Source::Shape {
                    geometry,
                    closed: None,
                },
            )
            .with(
                format!("{} (object)", slot_name("horizons", rank)),
                known(
                    format!("1a2b3c4d-0000-4000-8000-{rank:012}"),
                    "resqml22.Grid2dRepresentation",
                    title,
                    &frame(),
                ),
            );
    }
    for (rank, (title, geometry)) in faults.iter().enumerate() {
        pieces = pieces
            .with(
                slot_name("faults", rank),
                Source::Shape {
                    geometry: geometry.clone(),
                    closed: None,
                },
            )
            .with(
                format!("{} (object)", slot_name("faults", rank)),
                known(
                    format!("fa000000-0000-4000-8000-{rank:012}"),
                    "resqml22.TriangulatedSetRepresentation",
                    title,
                    fault_frame,
                ),
            );
    }
    pieces
}

fn bag() -> Pieces {
    bag_with(&[("A", fault_a()), ("B", fault_b())], &frame())
}

/// Two faults, normal; B removes the hanging-wall row next to it in the two
/// layers under the gap; the grid represents the model.
fn answers() -> Choices {
    unrepresented().set("interpretation", earth_model())
}

/// The same, representing nothing yet.
fn unrepresented() -> Choices {
    Choices::new()
        .set("layers", json!([2, 0, 2]))
        .set("axis", "elevation")
        .set("title", "Faulted grid")
        .set("maximumThrow", json!([A_THROW, B_THROW]))
        .set("throw", json!(["normal", "normal"]))
        .set("profile", "parabolic")
        .set("reach", REACH)
        .set("deadColumns", json!([0, 1]))
        .set("deadLayers", json!([2, 3]))
}

fn planned(pieces: &Pieces, choices: &Choices) -> Result<Plan, String> {
    Registry::new()
        .plan("faulted-ijk-grid", pieces, choices, &Identity::derived(), &when())
        .map_err(|why| why.message())
}

fn the_grid(plan: &Plan) -> &Value {
    &plan.of_role("representation").next().expect("the grid").json
}

fn dataset(plan: &Plan, path_end: &str) -> (Vec<i64>, Numbers) {
    let found = plan
        .datasets
        .iter()
        .find(|d| d.path.ends_with(path_end))
        .unwrap_or_else(|| panic!("no dataset {path_end}"));
    (found.dimensions.clone(), found.values.clone())
}

fn floats(plan: &Plan, path_end: &str) -> Vec<f64> {
    match dataset(plan, path_end).1 {
        Numbers::F64(values) => values,
        Numbers::I64(_) => panic!("{path_end} is not floating point"),
    }
}

fn integers(plan: &Plan, path_end: &str) -> Vec<i64> {
    match dataset(plan, path_end).1 {
        Numbers::I64(values) => values,
        Numbers::F64(_) => panic!("{path_end} is not whole"),
    }
}

fn number(plan: &Plan, title: &str) -> f64 {
    plan.activity
        .parameters
        .iter()
        .find_map(|one| match one {
            Parameter::Double { title: t, value, .. } if t == title => Some(*value),
            _ => None,
        })
        .unwrap_or_else(|| panic!("`{title}` is not a number of the activity"))
}

/// The unfaulted height of level `level` at pillar `(i, j)`.
fn level_z(level: usize, i: usize) -> f64 {
    let x = X0 + DX * i as f64;
    match level {
        0 => top(x),
        1 => (top(x) - 20.0) / 2.0,
        2 => -20.0,
        3 => -30.0,
        4 => -45.0,
        _ => -60.0,
    }
}

/// The split lines as the plan writes them: `(pillar, columns)`, in order.
fn splits(plan: &Plan) -> Vec<(usize, Vec<usize>)> {
    let pillars = integers(plan, "/pillarIndices");
    let columns = integers(plan, "/elements");
    let cumulative = integers(plan, "/cumulativeLength");
    let mut start = 0usize;
    pillars
        .iter()
        .zip(&cumulative)
        .map(|(pillar, end)| {
            let row = columns[start..*end as usize]
                .iter()
                .map(|c| *c as usize)
                .collect();
            start = *end as usize;
            (*pillar as usize, row)
        })
        .collect()
}

// -- the split lines ---------------------------------------------------------------

/// **Which pillars split, and which columns take the split lines.** Fault A
/// runs between columns i = 1 and i = 2, so the five pillars at i = 2 split,
/// their split line serving the columns east of it (the side A dips to); B
/// runs between rows j = 1 and j = 2, so the seven pillars at j = 2 split,
/// serving the rows north of it. Where they cross, pillar (2, 2), the four
/// columns around it each move differently: its own line and **three** split
/// lines - the most an IJK pillar can have (usage guide §9.11.1 p. 151).
/// Every split line comes after the pillars, on a pillar that exists (TRG
/// §5.17 p. 111).
#[test]
fn the_pillars_on_a_fault_split_and_the_moved_side_takes_the_split_line() {
    let plan = planned(&bag(), &answers()).unwrap_or_else(|why| panic!("{why}"));
    let found = splits(&plan);
    let column = |i: usize, j: usize| j * CI + i;
    let pillar = |i: usize, j: usize| j * NI + i;
    let mut expected: Vec<(usize, Vec<usize>)> = Vec::new();
    for j in 0..NJ {
        for i in 0..NI {
            let around: Vec<(usize, usize)> = [
                (i.wrapping_sub(1), j.wrapping_sub(1)),
                (i, j.wrapping_sub(1)),
                (i.wrapping_sub(1), j),
                (i, j),
            ]
            .into_iter()
            .filter(|(a, b)| *a < CI && *b < CJ)
            .collect();
            let east = |a: usize| a >= 2;
            let north = |b: usize| b >= 2;
            // The groups: by side of A and side of B, where each throws.
            let a_cuts = i == 2 && a_at(i, j) > 0.0;
            let b_cuts = j == 2 && b_at(i, j) > 0.0;
            let mut groups: Vec<((bool, bool), Vec<usize>)> = Vec::new();
            for (a, b) in around {
                let key = (a_cuts && east(a), b_cuts && north(b));
                match groups.iter_mut().find(|(k, _)| *k == key) {
                    Some((_, members)) => members.push(column(a, b)),
                    None => groups.push((key, vec![column(a, b)])),
                }
            }
            for (key, mut members) in groups {
                if key != (false, false) {
                    members.sort_unstable();
                    expected.push((pillar(i, j), members));
                }
            }
        }
    }
    // Ordered by pillar, then by the first column each serves.
    expected.sort();
    let mut sorted = found.clone();
    sorted.sort();
    assert_eq!(sorted, expected);
    assert_eq!(
        found.len(),
        5 + 7 - 1 + 2,
        "A's five, B's seven, the crossing pillar counted once and given three"
    );
    assert!(
        found.windows(2).all(|pair| pair[0].0 <= pair[1].0),
        "split lines in pillar order"
    );
    assert_eq!(found.iter().filter(|(p, _)| *p == pillar(2, 2)).count(), 3);

    let grid = the_grid(&plan);
    let lines = &grid["Geometry"]["ColumnLayerSplitCoordinateLines"];
    assert_eq!(lines["Count"], json!(found.len()));
    assert_eq!(number(&plan, "splitLineCount"), found.len() as f64);
    let (dimensions, _) = dataset(&plan, "/points");
    assert_eq!(
        dimensions,
        vec![LEVELS as i64, (PILLARS + found.len()) as i64, 3],
        "NKL × (pillars + splits) × 3, the lines flattened"
    );
}

/// **The split nodes are the throw below the pillar's own, on one side only.**
/// At the pillars A alone splits, the split line of every level is the
/// pillar's own line lowered by A's throw there - `10 · 4s(1 − s) · (1 − d /
/// 100)`, `s` along A, `d` from it - and the pillar's own line is the footwall:
/// untouched south of B, lowered by B's throw alone north of it, where both
/// sides are in B's hanging wall. West of A and south of B nothing moves; east
/// of A, a pillar's own line is lowered by A's drag.
#[test]
fn the_split_nodes_are_displaced_by_the_throw_on_one_side_only() {
    let plan = planned(&bag(), &answers()).unwrap_or_else(|why| panic!("{why}"));
    let found = splits(&plan);
    let lines = PILLARS + found.len();
    let points = floats(&plan, "/points");
    let z = |level: usize, line: usize| points[3 * (level * lines + line) + 2];
    let xy = |level: usize, line: usize| {
        [
            points[3 * (level * lines + line)],
            points[3 * (level * lines + line) + 1],
        ]
    };
    for (split, (pillar, _)) in found.iter().enumerate() {
        let (i, j) = (pillar % NI, pillar / NI);
        for level in 0..LEVELS {
            assert_eq!(
                xy(level, PILLARS + split),
                xy(level, *pillar),
                "a split line stands on its pillar"
            );
        }
        if i != 2 || j == 2 {
            continue;
        }
        let b_own = if j > 2 { b_at(i, j) } else { 0.0 };
        for level in 0..LEVELS {
            let own = z(level, *pillar);
            let moved = z(level, PILLARS + split);
            assert!(
                (own - (level_z(level, i) - b_own)).abs() < 1e-9,
                "pillar ({i}, {j}) level {level}: its own line is the footwall's, {own}"
            );
            assert!(
                (moved - (own - a_at(i, j))).abs() < 1e-9,
                "pillar ({i}, {j}) level {level}: the split line is {moved}, {} below the pillar's own where A throws {}",
                own - moved,
                a_at(i, j)
            );
            assert!(a_at(i, j) > 1.0, "a throw worth the name at ({i}, {j})");
        }
    }
    for j in 0..2 {
        for i in 0..2 {
            for level in 0..LEVELS {
                assert_eq!(
                    z(level, j * NI + i),
                    level_z(level, i),
                    "west of A, south of B, nothing moves"
                );
            }
        }
    }
    for level in 0..LEVELS {
        assert!(
            (z(level, 4) - (level_z(level, 4) - a_at(4, 0))).abs() < 1e-9,
            "east of A, the pillar (4, 0) is dragged down"
        );
    }
}

// -- the removed cells ---------------------------------------------------------------

/// **The removed cells are exactly those asked for**: one column of B's
/// hanging wall next to B - the row j = 2 - in the layers 2 and 3, none next
/// to A (`deadColumns = [0, 1]`). `CellGeometryIsDefined` false for those
/// twelve, true for every other.
#[test]
fn the_cells_next_to_a_fault_are_removed_as_asked_and_only_those() {
    let plan = planned(&bag(), &answers()).unwrap_or_else(|why| panic!("{why}"));
    let flags = integers(&plan, "/cellGeometryIsDefined");
    assert_eq!(
        dataset(&plan, "/cellGeometryIsDefined").0,
        vec![CK as i64, CJ as i64, CI as i64],
        "k, j, i"
    );
    let mut removed = Vec::new();
    for k in 0..CK {
        for j in 0..CJ {
            for i in 0..CI {
                let asked = (k == 2 || k == 3) && j == 2;
                let cell = (k * CJ + j) * CI + i;
                assert_eq!(flags[cell], i64::from(!asked), "cell ({i}, {j}, {k})");
                if asked {
                    removed.push(cell);
                }
            }
        }
    }
    assert_eq!(removed.len(), 12);
    assert_eq!(number(&plan, "deadCellCount"), 12.0);
    // A removed cell is not drawn: the plan's own cells skip it.
    let Topology::Hexahedra { cells, index, .. } = &plan.shapes[0].geometry.patches[0].topology else {
        panic!("not cells")
    };
    assert_eq!(cells.len(), CI * CJ * CK - 12);
    assert!(removed.iter().all(|cell| !index.contains(&(*cell as u32))));

    // Two columns of B's hanging wall: rows 2 and 3.
    let wider = planned(&bag(), &answers().set("deadColumns", json!([0, 2]))).unwrap();
    assert_eq!(number(&wider, "deadCellCount"), 24.0);
    // Every layer when none is named.
    let every = planned(&bag(), &answers().set("deadLayers", json!([]))).unwrap();
    assert_eq!(number(&every, "deadCellCount"), 24.0);
    // None asked, none removed - and the flag array is the constant.
    let none = planned(&bag(), &answers().set("deadColumns", json!([]))).unwrap();
    assert_eq!(number(&none, "deadCellCount"), 0.0);
    assert_eq!(
        the_grid(&none)["Geometry"]["CellGeometryIsDefined"]["$type"],
        json!("eml23.BooleanConstantArray")
    );
}

// -- what the grid says of itself ------------------------------------------------------

/// **It represents the model, and says its throw is invented.** The
/// interpretation named is the grid's `RepresentedObject` (usage guide
/// §7.3.2.2 p. 104: the 3D grid *“represents Peter's earth model
/// interpretation”*); with `synthetic`, the grid carries the mark and its
/// source. K down, vertical pillars, in the horizons' frame.
#[test]
fn the_grid_represents_the_model_and_says_its_throw_is_invented() {
    let choices = answers()
        .set("synthetic", true)
        .set("source", "synthetic - a test");
    let plan = planned(&bag(), &choices).unwrap_or_else(|why| panic!("{why}"));
    let grid = the_grid(&plan);
    assert_eq!(grid["RepresentedObject"]["Uuid"], earth_model()["Uuid"]);
    assert_eq!(grid["ExtensionNameValue"][0]["Name"], json!("synthetic"));
    assert!(
        grid["Citation"]["Description"]
            .as_str()
            .unwrap()
            .contains("synthetic - a test")
    );
    assert_eq!(grid["Geometry"]["KDirection"], json!("down"));
    assert_eq!(grid["Geometry"]["PillarShape"], json!("vertical"));
    assert_eq!(grid["Geometry"]["LocalCrs"]["Uuid"], json!(frame().uuid));
    assert_eq!(grid["KGaps"]["Count"], json!(1));
    let levels: Vec<f64> = plan
        .activity
        .parameters
        .iter()
        .filter_map(|one| match one {
            Parameter::Double { title, value, .. } if title == "traceLevel" => Some(*value),
            _ => None,
        })
        .collect();
    assert_eq!(
        levels,
        vec![level(), level()],
        "each fault traced at the middle of the heights it shares with the grid"
    );

    let why = planned(&bag(), &answers().set("synthetic", true)).unwrap_err();
    assert!(why.contains("`source`"), "{why}");
}

/// **Not a sixth representation of nothing**: with neither an interpretation
/// nor a feature to create, the grid is refused by name; with `featureType`
/// and `interpretationType`, the pair is made and represented.
#[test]
fn a_faulted_grid_represents_something_or_is_refused() {
    let bare = unrepresented();
    let why = planned(&bag(), &bare).unwrap_err();
    assert!(
        why.contains("`interpretation`") && why.contains("§7.3.2.2"),
        "{why}"
    );

    let made = planned(
        &bag(),
        &bare
            .set("featureType", "resqml22.Model")
            .set("interpretationType", "resqml22.EarthModelInterpretation"),
    )
    .unwrap_or_else(|why| panic!("{why}"));
    let interpretation = made
        .of_role("interpretation")
        .next()
        .expect("an interpretation made");
    assert_eq!(
        the_grid(&made)["RepresentedObject"]["Uuid"],
        json!(interpretation.uuid)
    );
}

// -- identity -----------------------------------------------------------------------------

/// **Same inputs, same grid; another cut, another grid; and the grid between
/// the same horizons keeps its uuid**: a cut grid is not the plain one.
#[test]
fn the_same_cut_names_the_same_grid() {
    let uuid = |plan: &Plan| plan.of_role("representation").next().unwrap().uuid.clone();
    let first = uuid(&planned(&bag(), &answers()).unwrap());
    assert_eq!(first, uuid(&planned(&bag(), &answers()).unwrap()));
    assert_ne!(
        first,
        uuid(&planned(&bag(), &answers().set("maximumThrow", json!([12.0, 6.0]))).unwrap()),
        "another throw"
    );
    assert_ne!(
        first,
        uuid(&planned(&bag(), &answers().set("deadColumns", json!([1, 1]))).unwrap()),
        "other cells removed"
    );
    let plain = Registry::new()
        .plan(
            "ijk-grid-from-horizons",
            &bag_with(&[], &frame()),
            &Choices::new()
                .set("layers", json!([2, 0, 2]))
                .set("axis", "elevation")
                .set("title", "Faulted grid"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert_ne!(first, uuid(&plain), "the plain grid is another");
}

// -- refusals ------------------------------------------------------------------------------

/// **What cannot cut a grid is refused by name**: lists that do not follow the
/// faults, a play with no vertical sense, a vertical fault, a fault that splits
/// nothing, a fault in another frame, a removed layer past the grid, a throw
/// that turns a line over.
#[test]
fn what_cannot_cut_the_grid_is_refused_by_name() {
    let why = planned(&bag(), &answers().set("maximumThrow", json!([10.0]))).unwrap_err();
    assert!(
        why.contains("`maximumThrow`") && why.contains("1 throw(s) for 2 fault(s)"),
        "{why}"
    );
    let why = planned(&bag(), &answers().set("deadColumns", json!([1]))).unwrap_err();
    assert!(why.contains("`deadColumns`"), "{why}");
    let why = planned(&bag(), &answers().set("throw", json!(["normal", "strike-slip"]))).unwrap_err();
    assert!(why.contains("`B`") && why.contains("no vertical sense"), "{why}");
    let why = planned(&bag(), &answers().set("deadLayers", json!([4]))).unwrap_err();
    assert!(why.contains("`deadLayers`") && why.contains("layer 4"), "{why}");

    let vertical = plane(vec![
        [X0 + A_X, Y0 + A_FROM, FAULT_TOP],
        [X0 + A_X, Y0 + A_TO, FAULT_TOP],
        [X0 + A_X, Y0 + A_FROM, FAULT_BOTTOM],
        [X0 + A_X, Y0 + A_TO, FAULT_BOTTOM],
    ]);
    let one = || {
        answers()
            .set("maximumThrow", json!([A_THROW]))
            .set("throw", json!(["normal"]))
            .set("deadColumns", json!([]))
    };
    let why = planned(&bag_with(&[("V", vertical)], &frame()), &one()).unwrap_err();
    assert!(why.contains("`V` is vertical"), "{why}");

    let mut far = fault_a();
    for p in &mut far.patches[0].points {
        p[0] += 5_000.0;
    }
    let why = planned(&bag_with(&[("Far", far)], &frame()), &one()).unwrap_err();
    assert!(why.contains("`Far` splits no pillar"), "{why}");

    let why = planned(&bag_with(&[("A", fault_a())], &other_frame()), &one()).unwrap_err();
    assert!(why.contains("The other frame"), "{why}");

    // A reverse fault that stops at −25, inside the K gap's roof: the nodes
    // below it go up 30 m, the ones above stay - the top of the lower zone
    // (−30) ends above the base of the upper one (−20). Refused, never drawn
    // inside out. The same fault normal is a thicker gap, and is written.
    let sealed = bag_with(&[("A", fault_a_from(-25.0, FAULT_BOTTOM))], &frame());
    let reverse = one()
        .set("throw", json!(["reverse"]))
        .set("maximumThrow", json!([30.0]))
        .set("reach", 1000.0);
    let why = planned(&sealed, &reverse).unwrap_err();
    assert!(
        why.contains("`throw`")
            && why.contains("turns the levels over")
            && why.contains("level 3 ends up above level 2"),
        "{why}"
    );
    planned(&sealed, &reverse.set("throw", json!(["normal"]))).expect("a normal throw only thickens the gap");
}

/// **A node moves only where the fault is**: a fault that stops at −25 m -
/// sealed under the top of the grid, as F2 is under H6 - leaves the three
/// levels above it where they were on both sides of its split pillars, and
/// throws the three below.
#[test]
fn a_fault_sealed_inside_the_grid_leaves_the_levels_above_it() {
    let sealed = bag_with(&[("A", fault_a_from(-25.0, FAULT_BOTTOM))], &frame());
    let choices = answers()
        .set("maximumThrow", json!([A_THROW]))
        .set("throw", json!(["normal"]))
        .set("deadColumns", json!([]));
    let plan = planned(&sealed, &choices).unwrap_or_else(|why| panic!("{why}"));
    let found = splits(&plan);
    assert_eq!(
        found.iter().map(|(p, _)| *p).collect::<Vec<_>>(),
        vec![2, 9, 16, 23, 30],
        "the five pillars at i = 2"
    );
    let lines = PILLARS + found.len();
    let points = floats(&plan, "/points");
    let z = |level: usize, line: usize| points[3 * (level * lines + line) + 2];
    for (split, (pillar, _)) in found.iter().enumerate() {
        let j = pillar / NI;
        for level in 0..LEVELS {
            let (own, moved) = (z(level, *pillar), z(level, PILLARS + split));
            if level_z(level, 2) > -25.0 {
                assert_eq!(
                    moved, own,
                    "pillar (2, {j}), level {level}: above the fault's top, nothing moves"
                );
            } else {
                assert!(
                    (own - moved - a_at(2, j)).abs() < 1e-9,
                    "pillar (2, {j}), level {level}: {own} and {moved}"
                );
            }
        }
    }
}

// -- the reader, confronted -----------------------------------------------------------------

/// **Written, reopened, read by `energyml_geometry::ijk`** - a reader written
/// against the TRG that shares no line with this writer - and confronted with
/// what the file **declares**: the node count is
/// `(pillars + Count) × (Nk + 1 + gaps)` read off the XML (`Ni`, `Nj`, `Nk`, `KGaps/Count`,
/// `ColumnLayerSplitCoordinateLines/Count`); the cell count is `Ni · Nj · Nk`
/// less the false flags of the `CellGeometryIsDefined` dataset read back from
/// the HDF5; the removed cells are absent from the cells drawn; and the corner
/// a hanging-wall cell takes on a split pillar is the throw below the one its
/// footwall neighbour takes there.
#[cfg(feature = "hdf5")]
#[test]
fn the_reader_draws_the_split_nodes_and_not_the_removed_cells() {
    use std::sync::Arc;

    // No frame of the package: the frame is created, in metres, counting up.
    let mut pieces = Pieces::empty();
    for (rank, (_, geometry)) in horizons().into_iter().enumerate() {
        pieces = pieces.with(
            slot_name("horizons", rank),
            Source::Shape {
                geometry,
                closed: None,
            },
        );
    }
    for (rank, geometry) in [fault_a(), fault_b()].into_iter().enumerate() {
        pieces = pieces.with(
            slot_name("faults", rank),
            Source::Shape {
                geometry,
                closed: None,
            },
        );
    }
    let choices = unrepresented()
        .set("featureType", "resqml22.Model")
        .set("interpretationType", "resqml22.EarthModelInterpretation")
        .set("sourceEpsg", 2154)
        .set("project", 2154)
        .set("vertical", 5720);
    let plan = planned(&pieces, &choices).unwrap_or_else(|why| panic!("{why}"));
    let uuid = plan.of_role("representation").next().unwrap().uuid.clone();
    let planned_points = floats(&plan, "/points");

    let dir = std::env::temp_dir().join(format!("energyml-build-faulted-grid-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("grid.epc");
    let registry = Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "grid.epc", registry.clone()).unwrap();
    store.external_files_mut().set_root(&dir);
    let made = energyml_build::apply::apply(&plan, &mut store, &when());
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(made.refused.is_empty(), "{:?}", made.refused);
    store
        .save_as(&out, &energyml_storage::SaveOptions::default())
        .unwrap();
    let reread = energyml_storage::EpcStore::open(&out, registry).unwrap();
    let summary = reread.find_by_uuid(&uuid).cloned().unwrap();
    let object = reread.get_object(&summary.uri).unwrap();
    let declared = energyml_core::json::write_value(object.as_reflect());
    let flags: Vec<f64> = reread
        .arrays_of(object.as_reflect())
        .iter()
        .find(|location| location.path_in_file.ends_with("/cellGeometryIsDefined"))
        .map(|location| reread.read_array(location).expect("the flags read").to_f64())
        .expect("a CellGeometryIsDefined dataset");
    let built = energyml_geometry::scene::object_of(object.as_reflect(), &reread, &summary.uri)
        .expect("the grid reads");
    let _ = std::fs::remove_dir_all(&dir);

    // -- what the XML declares, against what the reader placed -----------------------
    let count = |value: &Value| value.as_u64().unwrap_or_else(|| panic!("not a count: {value}")) as usize;
    let (ni, nj, nk) = (
        count(&declared["Ni"]),
        count(&declared["Nj"]),
        count(&declared["Nk"]),
    );
    let gaps = count(&declared["KGaps"]["Count"]);
    let split = count(&declared["Geometry"]["ColumnLayerSplitCoordinateLines"]["Count"]);
    assert_eq!((ni, nj, nk, gaps), (CI, CJ, CK, 1));
    let [patch] = built.geometry.patches.as_slice() else {
        panic!("one patch")
    };
    assert_eq!(
        patch.points.len(),
        ((ni + 1) * (nj + 1) + split) * (nk + gaps + 1),
        "(pillars + split lines) × NKL nodes"
    );
    assert_eq!(patch.points.len() * 3, planned_points.len());
    let Topology::Hexahedra {
        cells,
        dimensions,
        index,
    } = &patch.topology
    else {
        panic!("not cells: {:?}", patch.topology)
    };
    assert_eq!(*dimensions, [ni, nj, nk]);
    let undefined = flags.iter().filter(|flag| **flag == 0.0).count();
    assert_eq!(flags.len(), ni * nj * nk);
    assert_eq!(undefined, 12, "the twelve cells asked, in the file");
    assert_eq!(
        cells.len(),
        ni * nj * nk - undefined,
        "every cell the file defines is drawn, and no other"
    );
    for (cell, flag) in flags.iter().enumerate() {
        assert_eq!(
            index.contains(&(cell as u32)),
            *flag != 0.0,
            "cell {cell}: drawn if and only if its flag says it has a geometry"
        );
    }

    // -- the corners on a split pillar ------------------------------------------------
    // Pillar (2, 0): column (1, 0) is A's footwall, column (2, 0) its hanging
    // wall. Each cell's corners at x = X0 + 50, y = Y0 are its corners on
    // that pillar - the reader's own winding aside.
    let at_pillar = |cell: usize| -> Vec<f64> {
        let position = index
            .iter()
            .position(|one| *one as usize == cell)
            .expect("the cell is drawn");
        let mut z: Vec<f64> = cells[position]
            .iter()
            .map(|n| patch.points[*n as usize])
            .filter(|p| (p[0] - (X0 + 2.0 * DX)).abs() < 1e-6 && (p[1] - Y0).abs() < 1e-6)
            .map(|p| p[2])
            .collect();
        z.sort_by(f64::total_cmp);
        z
    };
    for k in 0..CK {
        let footwall = at_pillar(k * CJ * CI + 1);
        let hanging = at_pillar(k * CJ * CI + 2);
        assert_eq!(
            (footwall.len(), hanging.len()),
            (2, 2),
            "two corners of each cell on the pillar, layer {k}"
        );
        for (f, h) in footwall.iter().zip(&hanging) {
            assert!(
                (f - h - a_at(2, 0)).abs() < 1e-6,
                "layer {k}: the hanging wall's corner {h} is A's throw {} below the footwall's {f}",
                a_at(2, 0)
            );
        }
    }
}
