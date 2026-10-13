//! `ijk-grid-from-horizons` - an IJK grid between horizon grids given top to
//! bottom.
//!
//! The form of the assertions comes from what the schemas and the guides
//! state: `NKL = NK + gapCount + 1` and *“Number of intervals = gapCount + NK”*
//! (RESQML TRG 2.2 §5.36 `KGaps` p. 132; 11 intervals
//! = 10 layers + 1 gap); the nodes are `levels × pillars`, levels first
//! (TRG §5.1 p. 94, and the reader `energyml_geometry::ijk`); a cell whose
//! geometry is not defined is said by `CellGeometryIsDefined`, and a pillar
//! with no defined cell by `PillarGeometryIsDefined` (same page); K increases
//! with depth when the horizons are given top to bottom (TRG §5.35 p. 131).
//! The numbers - the lattice, the heights - are the tests' own, at map
//! coordinates in Lambert-93 (x ≈ 651 000, y ≈ 6 861 000), where the
//! recipe's horizons are.
//!
//! # What would have to break to make it turn red
//!
//! Counting the K gaps as layers (the counts and the levels turn red);
//! transposing the lattice (x and y of the pillars); recomputing a horizon instead
//! of copying it, or spreading the layers other than linearly;
//! defining the whole column around a hole, or saying nothing of it; sorting the
//! horizons by altitude, ignoring the axis, correcting a crossing without being
//! asked; putting the heights in the seed or forgetting the layers;
//! accepting two lattices, two frames, a turned lattice.

use serde_json::{Value, json};

use energyml_build::operation::{Choices, Identity, Numbers, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

const X0: f64 = 651_000.0;
const Y0: f64 = 6_861_000.0;
const DX: f64 = 25.0;
const DY: f64 = 40.0;
const NI: usize = 5;
const NJ: usize = 4;

fn when() -> When {
    When {
        creation: "2026-09-24T00:00:00Z".into(),
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

/// The lattice: 5 × 4 nodes, 25 along x and **40** along y - two spacings, so
/// that a transposed lattice shows - `i` along x varying fastest.
fn positions() -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    for j in 0..NJ {
        for i in 0..NI {
            out.push([X0 + DX * i as f64, Y0 + DY * j as f64]);
        }
    }
    out
}

/// A horizon at `z(x, y)`, read as a `Grid2dRepresentation` reads.
fn grid(z: impl Fn(f64, f64) -> f64) -> Geometry {
    let points = positions().into_iter().map(|[x, y]| [x, y, z(x, y)]).collect();
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
}

/// The same horizon with no height at lattice node `(i, j)`.
fn holed(mut geometry: Geometry, i: usize, j: usize) -> Geometry {
    geometry.patches[0].points[j * NI + i][2] = f64::NAN;
    geometry
}

/// The same horizon at `z` at lattice node `(i, j)`.
fn moved(mut geometry: Geometry, i: usize, j: usize, z: f64) -> Geometry {
    geometry.patches[0].points[j * NI + i][2] = z;
    geometry
}

/// The top of the stack, dipping in both directions, in elevation (NGF).
fn h3(x: f64, y: f64) -> f64 {
    20.0 - 0.01 * (x - X0) + 0.005 * (y - Y0)
}

/// The five horizons of the Chalk grid, H3 to H7, top to bottom, each a different
/// distance below the one above - so that a layer spread between the wrong
/// pair shows.
fn o401() -> Vec<(&'static str, Geometry)> {
    vec![
        ("H3 - top of the Ypresian", grid(h3)),
        (
            "H4 - top of the Sparnacian",
            grid(|x, y| h3(x, y) - 15.0 - 0.02 * (x - X0)),
        ),
        (
            "H5 - top of the Selandian",
            grid(|x, y| h3(x, y) - 30.0 - 0.02 * (x - X0)),
        ),
        (
            "H6 - top of the Chalk",
            grid(|x, y| h3(x, y) - 45.0 - 0.01 * (y - Y0)),
        ),
        (
            "H7 - top of the Santonian",
            grid(|x, y| h3(x, y) - 140.0 - 0.01 * (y - Y0)),
        ),
    ]
}

/// The name a horizon takes in the bag: `horizons`, `horizons 2`, …
fn slot_name(rank: usize) -> String {
    if rank == 0 {
        "horizons".to_string()
    } else {
        format!("horizons {}", rank + 1)
    }
}

/// What the caller knows of a horizon of the package.
fn known(rank: usize, title: &str, frame: &Ref, uom: &str, up: bool) -> Source {
    Source::Object(Known {
        reference: Some(Ref::new(
            format!("1a2b3c4d-0000-4000-8000-{rank:012}"),
            "resqml22.Grid2dRepresentation",
            title,
        )),
        crs: Some(frame.clone()),
        vertical_uom: Some(uom.into()),
        vertical_up: Some(up),
        interpretation: Some(Ref::new(
            format!("eeeeeeee-0000-4000-8000-{rank:012}"),
            "resqml22.HorizonInterpretation",
            title,
        )),
        node_count: Some(NI * NJ),
        ..Default::default()
    })
}

/// The horizons, in the order given, each with what the package says of it.
fn bag_in(horizons: &[(&str, Geometry)], frame: &Ref, up: bool) -> Pieces {
    let mut pieces = Pieces::empty();
    for (rank, (title, geometry)) in horizons.iter().enumerate() {
        pieces = pieces
            .with(
                slot_name(rank),
                Source::Shape {
                    geometry: geometry.clone(),
                    closed: None,
                },
            )
            .with(
                format!("{} (object)", slot_name(rank)),
                known(rank, title, frame, "m", up),
            );
    }
    pieces
}

fn bag(horizons: &[(&str, Geometry)]) -> Pieces {
    bag_in(horizons, &frame(), true)
}

/// The Chalk grid's layering: H3-H4 two layers, H4-H5 one, H5-H6 a gap, H6-H7 seven.
fn answers() -> Choices {
    Choices::new()
        .set("layers", json!([2, 1, 0, 7]))
        .set("axis", "elevation")
        .set("title", "Ypresian-Chalk grid")
}

fn planned(pieces: &Pieces, choices: &Choices) -> Result<Plan, String> {
    Registry::new()
        .plan(
            "ijk-grid-from-horizons",
            pieces,
            choices,
            &Identity::derived(),
            &when(),
        )
        .map_err(|why| why.message())
}

fn the_grid(plan: &Plan) -> &Value {
    &plan.of_role("representation").next().expect("the grid").json
}

fn uuid_of(plan: &Plan) -> String {
    plan.of_role("representation")
        .next()
        .expect("the grid")
        .uuid
        .clone()
}

fn floats(plan: &Plan, path_end: &str) -> Vec<f64> {
    match &plan
        .datasets
        .iter()
        .find(|d| d.path.ends_with(path_end))
        .unwrap_or_else(|| panic!("no dataset {path_end}"))
        .values
    {
        Numbers::F64(values) => values.clone(),
        Numbers::I64(_) => panic!("{path_end} is not floating point"),
    }
}

fn integers(plan: &Plan, path_end: &str) -> Vec<i64> {
    match &plan
        .datasets
        .iter()
        .find(|d| d.path.ends_with(path_end))
        .unwrap_or_else(|| panic!("no dataset {path_end}"))
        .values
    {
        Numbers::I64(values) => values.clone(),
        Numbers::F64(_) => panic!("{path_end} is not whole"),
    }
}

fn dimensions(plan: &Plan, path_end: &str) -> Vec<i64> {
    plan.datasets
        .iter()
        .find(|d| d.path.ends_with(path_end))
        .unwrap_or_else(|| panic!("no dataset {path_end}"))
        .dimensions
        .clone()
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

fn says(plan: &Plan, key: &str) -> bool {
    plan.says.iter().any(|note| note.key == key)
}

/// The node of the plan at `level` on pillar `(i, j)` of a `pi`-wide grid.
fn node(points: &[f64], pillars: [usize; 2], level: usize, i: usize, j: usize) -> [f64; 3] {
    let at = 3 * ((level * pillars[1] + j) * pillars[0] + i);
    [points[at], points[at + 1], points[at + 2]]
}

// -- counts ---------------------------------------------------------------------

/// **The counts come from the inputs**: `Ni`, `Nj` are the lattice's cells, `Nk`
/// the sum of the layers, one gap per zone of no layer - the Chalk grid's *“11
/// intervalles = 10 couches + 1 gap”* - and `Nk + 1 + gaps` levels of
/// `(Ni + 1) × (Nj + 1)` pillars.
#[test]
fn the_counts_come_from_the_horizons_and_the_layers() {
    let plan = planned(&bag(&o401()), &answers()).unwrap_or_else(|why| panic!("{why}"));
    let grid = the_grid(&plan);
    assert_eq!(grid["$type"], json!("resqml22.IjkGridRepresentation"));
    assert_eq!(
        (grid["Ni"].clone(), grid["Nj"].clone(), grid["Nk"].clone()),
        (json!(4), json!(3), json!(10))
    );
    assert_eq!(grid["KGaps"]["Count"], json!(1));
    // `Nk − 1` flags, the gap after the third layer - the last of H4-H5.
    let flags: Vec<bool> = grid["KGaps"]["GapAfterLayer"]["Values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_bool().unwrap())
        .collect();
    assert_eq!(
        flags,
        vec![false, false, true, false, false, false, false, false, false]
    );
    assert_eq!(
        dimensions(&plan, "/points"),
        vec![12, 4, 5, 3],
        "levels, then j, then i, then xyz"
    );
    assert_eq!(floats(&plan, "/points").len(), 12 * 20 * 3);
    assert_eq!(
        grid["Geometry"]["Points"]["Coordinates"]["ExternalDataArrayPart"][0]["Count"],
        json!([12, 4, 5, 3])
    );
    assert_eq!(
        (
            number(&plan, "ni"),
            number(&plan, "nj"),
            number(&plan, "nk"),
            number(&plan, "gapCount")
        ),
        (4.0, 3.0, 10.0, 1.0)
    );
    // No gap, no `KGaps`.
    let plain = planned(&bag(&o401()), &answers().set("layers", json!([2, 1, 3, 7]))).unwrap();
    assert!(
        the_grid(&plain).get("KGaps").is_none(),
        "a grid with no gap declares none"
    );
    assert_eq!(dimensions(&plain, "/points")[0], 14);
}

// -- nodes --------------------------------------------------------------------------

/// **A horizon is a level, and the levels between are linear**: at its level a
/// node is the horizon's own height, copied; between two horizons the
/// `l`-th of `n` levels is `top + (base − top)·l/n`. And every pillar stands on
/// a node of the lattice, x along i, y along j.
#[test]
fn each_horizon_is_a_level_and_the_levels_between_are_linear() {
    let horizons = o401();
    let plan = planned(&bag(&horizons), &answers()).unwrap();
    let points = floats(&plan, "/points");
    let z = |h: usize, i: usize, j: usize| horizons[h].1.patches[0].points[j * NI + i][2];
    // H3 at 0, H4 at 2, H5 at 3, H6 at 4 - the gap is the step from 3 to 4 -, H7 at 11.
    let levels = [0, 2, 3, 4, 11];
    for j in 0..NJ {
        for i in 0..NI {
            for (h, &level) in levels.iter().enumerate() {
                let at = node(&points, [5, 4], level, i, j);
                assert_eq!(
                    at,
                    [X0 + DX * i as f64, Y0 + DY * j as f64, z(h, i, j)],
                    "horizon {h} at ({i}, {j})"
                );
            }
            let middle = node(&points, [5, 4], 1, i, j)[2];
            assert!(
                (middle - (z(0, i, j) + z(1, i, j)) / 2.0).abs() < 1e-9,
                "halfway between H3 and H4 at ({i}, {j})"
            );
            for l in 1..7 {
                let expected = z(3, i, j) + (z(4, i, j) - z(3, i, j)) * l as f64 / 7.0;
                let got = node(&points, [5, 4], 4 + l, i, j)[2];
                assert!(
                    (got - expected).abs() < 1e-9,
                    "layer boundary {l} of the Chalk at ({i}, {j}): {got} where {expected}"
                );
            }
        }
    }
}

// -- holes --------------------------------------------------------------------------

/// **A hole undefines the cells it bounds, and no other**: no height on H4 at
/// node (2, 1) leaves undefined the three layers H3-H5 of the four columns
/// around it, and keeps the Chalk under it - a hole in H4 says nothing of the
/// rock between H6 and H7. Written as `CellGeometryIsDefined` (TRG §5.1
/// p. 94), NaN at the missing nodes, counted and said.
#[test]
fn a_hole_leaves_undefined_the_cells_it_bounds_and_no_other() {
    let mut horizons = o401();
    horizons[1].1 = holed(horizons[1].1.clone(), 2, 1);
    let plan = planned(&bag(&horizons), &answers()).unwrap_or_else(|why| panic!("{why}"));
    let grid = the_grid(&plan);
    assert_eq!(
        grid["Geometry"]["CellGeometryIsDefined"]["$type"],
        json!("eml23.BooleanExternalArray")
    );
    assert_eq!(
        dimensions(&plan, "/cellGeometryIsDefined"),
        vec![10, 3, 4],
        "k, j, i"
    );
    let flags = integers(&plan, "/cellGeometryIsDefined");
    for k in 0..10 {
        for j in 0..3 {
            for i in 0..4 {
                let touches = k < 3 && (1..=2).contains(&i) && (0..=1).contains(&j);
                assert_eq!(
                    flags[(k * 3 + j) * 4 + i],
                    i64::from(!touches),
                    "cell ({i}, {j}, {k})"
                );
            }
        }
    }
    // Every pillar keeps a defined cell below the hole: a constant, all true.
    assert_eq!(
        grid["Geometry"]["PillarGeometryIsDefined"]["$type"],
        json!("eml23.BooleanConstantArray")
    );
    let points = floats(&plan, "/points");
    assert!(node(&points, [5, 4], 2, 2, 1)[2].is_nan(), "H4's node is missing");
    assert!(
        node(&points, [5, 4], 1, 2, 1)[2].is_nan(),
        "and the level interpolated from it"
    );
    assert!(node(&points, [5, 4], 3, 2, 1)[2].is_finite(), "H5 is there");
    assert_eq!(
        (number(&plan, "holeCount"), number(&plan, "undefinedCellCount")),
        (1.0, 12.0)
    );
    assert!(says(&plan, "holes") && says(&plan, "undefined-cells") && says(&plan, "no-activity"));
    let shape = &plan.shapes[0].geometry.patches[0];
    match &shape.topology {
        Topology::Hexahedra {
            cells,
            dimensions,
            index,
        } => {
            assert_eq!(*dimensions, [4, 3, 10]);
            assert_eq!(cells.len(), 120 - 12);
            assert_eq!(index.len(), 108, "the RESQML index of each kept cell");
        }
        other => panic!("not cells: {other:?}"),
    }
}

/// **A pillar no defined cell touches is said undefined**: a corner node with
/// no height on any horizon takes out its column, and its pillar.
#[test]
fn a_pillar_with_no_defined_cell_is_said_undefined() {
    let horizons: Vec<(&str, Geometry)> = o401()
        .into_iter()
        .map(|(title, g)| (title, holed(g, 0, 0)))
        .collect();
    let plan = planned(&bag(&horizons), &answers()).unwrap();
    assert_eq!(dimensions(&plan, "/pillarGeometryIsDefined"), vec![4, 5]);
    let pillars = integers(&plan, "/pillarGeometryIsDefined");
    assert_eq!(pillars[0], 0);
    assert!(pillars[1..].iter().all(|flag| *flag == 1));
    let cells = integers(&plan, "/cellGeometryIsDefined");
    for k in 0..10 {
        assert_eq!(cells[k * 12], 0, "column (0, 0) of layer {k}");
        assert_eq!(cells[k * 12 + 1], 1, "column (1, 0) of layer {k}");
    }
    assert_eq!(number(&plan, "holeCount"), 5.0);
}

/// A grid of nothing but holes is refused: it would read as a grid with
/// nothing to show.
#[test]
fn a_grid_with_no_defined_cell_is_refused() {
    let horizons: Vec<(&str, Geometry)> = o401()
        .into_iter()
        .map(|(title, _)| (title, grid(|_, _| f64::NAN)))
        .collect();
    let why = planned(&bag(&horizons), &answers()).unwrap_err();
    assert!(why.contains("nothing would be drawn"), "{why}");
}

// -- crossings ----------------------------------------------------------------------

/// **A crossing is refused by default, pinched on request**: H5 a metre above
/// H4 at node (3, 2) is two picks that disagree. Refused, naming where; with
/// `crossing = pinch`, H5 is brought up to H4 there - a zero-thickness layer,
/// counted and said - and nothing else moves.
#[test]
fn a_crossing_is_refused_by_default_and_pinched_on_request() {
    let mut horizons = o401();
    let h4 = horizons[1].1.patches[0].points[2 * NI + 3][2];
    horizons[2].1 = moved(horizons[2].1.clone(), 3, 2, h4 + 1.0);
    let why = planned(&bag(&horizons), &answers()).unwrap_err();
    assert!(why.contains("`crossing`") && why.contains("pinch"), "{why}");
    assert!(
        why.contains(&format!("({}, {})", X0 + 3.0 * DX, Y0 + 2.0 * DY)),
        "the refusal names where: {why}"
    );
    assert!(
        why.contains("H5 - top of the Selandian") && why.contains("H4 - top of the Sparnacian"),
        "{why}"
    );

    let plan =
        planned(&bag(&horizons), &answers().set("crossing", "pinch")).unwrap_or_else(|why| panic!("{why}"));
    let points = floats(&plan, "/points");
    assert_eq!(node(&points, [5, 4], 3, 3, 2)[2], h4, "H5 brought up to H4");
    assert_eq!(node(&points, [5, 4], 2, 3, 2)[2], h4, "H4 itself stays");
    let before = planned(&bag(&o401()), &answers()).unwrap();
    let untouched = floats(&before, "/points");
    let changed: Vec<usize> = (0..points.len())
        .filter(|&at| points[at] != untouched[at])
        .collect();
    assert_eq!(
        changed,
        vec![3 * ((3 * 4 + 2) * 5 + 3) + 2],
        "only H5's node at (3, 2) moves"
    );
    assert_eq!(number(&plan, "crossedCount"), 1.0);
    assert!(number(&plan, "zeroThicknessCount") >= 1.0);
    assert!(says(&plan, "pinched-crossings") && says(&plan, "zero-thickness"));
    assert_eq!(
        number(&plan, "undefinedCellCount"),
        0.0,
        "a pinched cell is flat, and defined"
    );
}

/// **Above is read off the axis**: the same numbers are in order as
/// elevations and crossed everywhere as depths. With nothing answered, the
/// axis comes from the horizons' frame; and a crossing over a hole is still a
/// crossing - compared against the last horizon that has a height.
#[test]
fn the_axis_decides_what_above_means() {
    let unanswered = Choices::new()
        .set("layers", json!([2, 1, 0, 7]))
        .set("title", "Ypresian-Chalk grid");
    planned(&bag_in(&o401(), &frame(), true), &unanswered).expect("elevations, top to bottom");
    let why = planned(&bag_in(&o401(), &frame(), false), &unanswered).unwrap_err();
    assert!(
        why.contains("`crossing`"),
        "read as depths, H3 is above nothing: {why}"
    );
    let why = planned(&bag(&o401()), &answers().set("axis", "depth")).unwrap_err();
    assert!(why.contains("`crossing`"), "{why}");

    let mut horizons = o401();
    horizons[1].1 = holed(horizons[1].1.clone(), 1, 1);
    let h3_there = h3(X0 + DX, Y0 + DY);
    horizons[2].1 = moved(horizons[2].1.clone(), 1, 1, h3_there + 2.0);
    let why = planned(&bag(&horizons), &answers()).unwrap_err();
    assert!(
        why.contains("H5 - top of the Selandian") && why.contains("H3 - top of the Ypresian"),
        "over the hole in H4: {why}"
    );
}

// -- what the grid says of itself ------------------------------------------------

/// **K down, vertical pillars, and the handedness of the numbers**: the
/// horizons top to bottom make K increase with depth (TRG §5.35 p. 131); the
/// pillars are vertical (§5.39 p. 135); I along +x, J along +y and K down is a
/// left-handed triple in an elevation frame, a right-handed one in a depth
/// frame (§5.29 p. 125). In the horizons' frame, and nothing else written.
#[test]
fn the_grid_says_its_direction_its_pillars_and_its_handedness() {
    let plan = planned(&bag(&o401()), &answers()).unwrap();
    let geometry = &the_grid(&plan)["Geometry"];
    assert_eq!(geometry["KDirection"], json!("down"));
    assert_eq!(geometry["PillarShape"], json!("vertical"));
    assert_eq!(geometry["GridIsRighthanded"], json!(false));
    assert_eq!(geometry["LocalCrs"]["Uuid"], json!(frame().uuid));
    assert_eq!(
        geometry["CellGeometryIsDefined"]["$type"],
        json!("eml23.BooleanConstantArray"),
        "every cell defined"
    );
    assert_eq!(geometry["CellGeometryIsDefined"]["Count"], json!(120));
    assert_eq!(
        plan.objects.len(),
        1,
        "{:?}",
        plan.objects.iter().map(|o| &o.qualified_type).collect::<Vec<_>>()
    );
    assert_eq!(plan.datasets.len(), 1, "no flag array when every flag is true");

    // The same stack as depths: top to bottom is now increasing z.
    let depths: Vec<(&str, Geometry)> = o401()
        .into_iter()
        .map(|(title, mut g)| {
            for p in &mut g.patches[0].points {
                p[2] = -p[2];
            }
            (title, g)
        })
        .collect();
    let plan = planned(&bag_in(&depths, &frame(), false), &answers().set("axis", "depth"))
        .unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(the_grid(&plan)["Geometry"]["GridIsRighthanded"], json!(true));
    assert_eq!(the_grid(&plan)["Geometry"]["KDirection"], json!("down"));
}

/// **A coarser lattice takes every n-th node**: every second node of 5 × 4 is
/// 3 × 2 pillars, and the row past the last pillar is left out and said.
#[test]
fn a_coarser_lattice_takes_every_other_node() {
    let horizons = o401();
    let plan =
        planned(&bag(&horizons), &answers().set("coarsening", 2)).unwrap_or_else(|why| panic!("{why}"));
    let grid = the_grid(&plan);
    assert_eq!((grid["Ni"].clone(), grid["Nj"].clone()), (json!(2), json!(1)));
    assert_eq!(dimensions(&plan, "/points"), vec![12, 2, 3, 3]);
    let points = floats(&plan, "/points");
    // Pillar (1, 1) stands on lattice node (2, 2).
    assert_eq!(
        node(&points, [3, 2], 0, 1, 1),
        [
            X0 + 2.0 * DX,
            Y0 + 2.0 * DY,
            horizons[0].1.patches[0].points[2 * NI + 2][2]
        ]
    );
    assert!(says(&plan, "trimmed"), "{:?}", plan.says);
}

// -- identity -----------------------------------------------------------------------

/// **Same inputs, same grid; another layering, another grid**: the uuid comes
/// from the horizons, the layers, the coarsening, the rule and the title - a
/// new layering changes the cell count, and a property written on the old grid
/// would no longer fit it. The heights are not in it: re-gridding a horizon
/// rewrites the same grid.
#[test]
fn the_same_inputs_name_the_same_grid() {
    let first = uuid_of(&planned(&bag(&o401()), &answers()).unwrap());
    assert_eq!(first, uuid_of(&planned(&bag(&o401()), &answers()).unwrap()));
    assert_ne!(
        first,
        uuid_of(&planned(&bag(&o401()), &answers().set("layers", json!([2, 1, 0, 6]))).unwrap()),
        "another layering"
    );
    assert_ne!(
        first,
        uuid_of(&planned(&bag(&o401()), &answers().set("coarsening", 2)).unwrap()),
        "other pillars"
    );
    assert_ne!(
        first,
        uuid_of(&planned(&bag(&o401()), &answers().set("title", "Another grid")).unwrap()),
        "another title"
    );
    let lowered: Vec<(&str, Geometry)> = o401()
        .into_iter()
        .map(|(title, mut g)| {
            for p in &mut g.patches[0].points {
                p[2] -= 1.0;
            }
            (title, g)
        })
        .collect();
    assert_eq!(
        first,
        uuid_of(&planned(&bag(&lowered), &answers()).unwrap()),
        "the same horizons re-gridded"
    );
}

// -- suggestions ---------------------------------------------------------------------

/// **The frame and the axis come from the horizons; the title and the
/// interpretation do not**: the grid is in its horizons' frame, and it is none
/// of its horizons - H3's name or interpretation on it would be a mistake that
/// looks deliberate.
#[test]
fn the_frame_is_suggested_and_the_horizons_name_is_not() {
    let registry = Registry::new();
    let operation = registry.get("ijk-grid-from-horizons").expect("in the registry");
    let needs = operation.needs(&bag(&o401()));
    let suggested = |name: &str| {
        needs
            .iter()
            .find(|slot| slot.name == name)
            .unwrap_or_else(|| panic!("no slot {name}"))
            .suggested
            .clone()
    };
    assert_eq!(
        suggested("crs").and_then(|v| v.get("Uuid").cloned()),
        Some(json!(frame().uuid))
    );
    assert_eq!(suggested("axis"), Some(json!("elevation")));
    assert_eq!(suggested("title"), None);
    assert_eq!(suggested("interpretation"), None);
    assert_eq!(
        suggested("crossing"),
        Some(json!("refuse")),
        "refusing is the default"
    );
}

// -- refusals -----------------------------------------------------------------------

/// **What cannot be a grid is refused by name**: one horizon, a horizon on
/// another lattice or of another size, a turned lattice, a surface that is no
/// grid; a layering that does not fit the zones; a coarsening that leaves no
/// cell.
#[test]
fn what_cannot_be_a_grid_is_refused_by_name() {
    let one = planned(&bag(&o401()[..1]), &answers().set("layers", json!([2]))).unwrap_err();
    assert!(one.contains("horizons"), "{one}");

    let mut shifted = o401();
    for p in &mut shifted[2].1.patches[0].points {
        p[0] += 5.0;
    }
    let why = planned(&bag(&shifted), &answers()).unwrap_err();
    assert!(
        why.contains("H5 - top of the Selandian")
            && why.contains("H3 - top of the Ypresian")
            && why.contains("same one"),
        "{why}"
    );

    let mut small = o401();
    small[4].1 = Geometry {
        kind: "grid",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points: small[4].1.patches[0].points[..15].to_vec(),
            topology: Topology::Grid {
                fastest: NI,
                slowest: 3,
            },
            closed: Vec::new(),
        }],
    };
    let why = planned(&bag(&small), &answers()).unwrap_err();
    assert!(why.contains("H7 - top of the Santonian"), "{why}");

    let (c, s) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
    let turned: Vec<(&str, Geometry)> = o401()
        .into_iter()
        .map(|(title, mut g)| {
            for p in &mut g.patches[0].points {
                let (u, v) = (p[0] - X0, p[1] - Y0);
                p[0] = X0 + c * u - s * v;
                p[1] = Y0 + s * u + c * v;
            }
            (title, g)
        })
        .collect();
    let why = planned(&bag(&turned), &answers()).unwrap_err();
    assert!(why.contains("turned"), "{why}");

    let mut surface = o401();
    surface[0].1.patches[0].topology = Topology::Triangles(vec![[0, 1, 5]]);
    let why = planned(&bag(&surface), &answers()).unwrap_err();
    assert!(why.contains("no 2D grid"), "{why}");

    for (layers, words) in [
        (json!([2, 1, 0]), "one per pair of consecutive horizons"),
        (json!([0, 1, 1, 7]), "top of the stack"),
        (json!([2, 1, 1, 0]), "bottom of the stack"),
        (json!([2, 0, 0, 7]), "both have no layer"),
    ] {
        let why = planned(&bag(&o401()), &answers().set("layers", layers.clone())).unwrap_err();
        assert!(why.contains("`layers`") && why.contains(words), "{layers}: {why}");
    }
    let why = planned(&bag(&o401()), &answers().set("coarsening", 4)).unwrap_err();
    assert!(why.contains("`coarsening`"), "{why}");
}

/// **One frame, the horizons'**: two frames or two units among the horizons
/// are refused, and so is a `crs` naming another frame or a frame created
/// here - the pillars are the horizons' nodes.
#[test]
fn the_grid_is_in_the_frame_of_its_horizons_or_refused() {
    let mut pieces = Pieces::empty();
    for (rank, (title, geometry)) in o401().iter().enumerate() {
        let theirs = if rank == 3 { other_frame() } else { frame() };
        pieces = pieces
            .with(
                slot_name(rank),
                Source::Shape {
                    geometry: geometry.clone(),
                    closed: None,
                },
            )
            .with(
                format!("{} (object)", slot_name(rank)),
                known(rank, title, &theirs, "m", true),
            );
    }
    let why = planned(&pieces, &answers()).unwrap_err();
    assert!(
        why.contains("two frames") && why.contains("The other frame"),
        "{why}"
    );

    let mut pieces = Pieces::empty();
    for (rank, (title, geometry)) in o401().iter().enumerate() {
        let uom = if rank == 4 { "ft" } else { "m" };
        pieces = pieces
            .with(
                slot_name(rank),
                Source::Shape {
                    geometry: geometry.clone(),
                    closed: None,
                },
            )
            .with(
                format!("{} (object)", slot_name(rank)),
                known(rank, title, &frame(), uom, true),
            );
    }
    let why = planned(&pieces, &answers()).unwrap_err();
    assert!(why.contains("one unit"), "{why}");

    let why = planned(&bag(&o401()), &answers().set("crs", other_frame().dor())).unwrap_err();
    assert!(
        why.contains("`crs`") && why.contains("The boreholes frame"),
        "{why}"
    );
    let why = planned(&bag(&o401()), &answers().set("sourceEpsg", 2154)).unwrap_err();
    assert!(why.contains("`sourceEpsg`"), "{why}");
}

// -- the reader, confronted -----------------------------------------------------------

/// **The geometry reader reads the nodes and the cells planned.** The grid is
/// planned by `energyml_analysis::ijk`, written through `apply` into a package
/// with its HDF5, reopened, and read back by `energyml_geometry::ijk` - a
/// reader written against the TRG, which shares no line with the writer. The
/// two must agree on every node (a NaN where a horizon has a hole), on which
/// cells are kept - the reader drops the ones `CellGeometryIsDefined` marks,
/// and a K gap it misread would shift every cell under it by a level - and on
/// where each kept cell is (its centroid; the winding is the reader's own
/// business, measured per cell).
#[cfg(feature = "hdf5")]
#[test]
fn the_geometry_reader_reads_the_nodes_and_the_cells_planned() {
    use std::sync::Arc;

    // No frame of the package: the frame is created, the identity compound
    // over Lambert-93 and NGF-IGN69, which counts up.
    let mut horizons = o401();
    horizons[1].1 = holed(horizons[1].1.clone(), 2, 1);
    let mut pieces = Pieces::empty();
    for (rank, (_, geometry)) in horizons.iter().enumerate() {
        pieces = pieces.with(
            slot_name(rank),
            Source::Shape {
                geometry: geometry.clone(),
                closed: None,
            },
        );
    }
    let choices = answers()
        .set("sourceEpsg", 2154)
        .set("project", 2154)
        .set("vertical", 5720);
    let plan = planned(&pieces, &choices).unwrap_or_else(|why| panic!("{why}"));
    let planned_points = floats(&plan, "/points");
    let uuid = uuid_of(&plan);
    let (planned_cells, planned_index) = match &plan.shapes[0].geometry.patches[0].topology {
        Topology::Hexahedra { cells, index, .. } => (cells.clone(), index.clone()),
        other => panic!("not cells: {other:?}"),
    };

    let dir = std::env::temp_dir().join(format!("energyml-build-ijk-grid-{}", std::process::id()));
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
    let built = energyml_geometry::scene::object_of(object.as_reflect(), &reread, &summary.uri)
        .expect("the grid reads");
    let _ = std::fs::remove_dir_all(&dir);

    let [patch] = built.geometry.patches.as_slice() else {
        panic!("one patch")
    };
    assert_eq!(
        patch.points.len() * 3,
        planned_points.len(),
        "one point per node, levels × pillars"
    );
    for (at, point) in patch.points.iter().enumerate() {
        for (axis, &read) in point.iter().enumerate() {
            let planned = planned_points[3 * at + axis];
            assert!(
                (read.is_nan() && planned.is_nan()) || (read - planned).abs() < 1e-6,
                "node {at}, axis {axis}: read {read}, planned {planned}"
            );
        }
    }
    let Topology::Hexahedra {
        cells,
        dimensions,
        index,
    } = &patch.topology
    else {
        panic!("not cells: {:?}", patch.topology)
    };
    assert_eq!(*dimensions, [4, 3, 10]);
    assert_eq!(
        cells.len(),
        108,
        "the twelve cells around the hole in H4 are dropped, and only those"
    );
    assert_eq!(
        *index, planned_index,
        "the same cells kept, by their RESQML index"
    );
    let centroid = |cell: &[u32; 8], points: &dyn Fn(usize) -> [f64; 3]| -> [f64; 3] {
        let mut sum = [0.0; 3];
        for &n in cell {
            for (total, value) in sum.iter_mut().zip(points(n as usize)) {
                *total += value / 8.0;
            }
        }
        sum
    };
    let read_at = |n: usize| patch.points[n];
    let planned_at = |n: usize| {
        [
            planned_points[3 * n],
            planned_points[3 * n + 1],
            planned_points[3 * n + 2],
        ]
    };
    for (read_cell, planned_cell) in cells.iter().zip(&planned_cells) {
        let (a, b) = (centroid(read_cell, &read_at), centroid(planned_cell, &planned_at));
        assert!(
            (0..3).all(|axis| (a[axis] - b[axis]).abs() < 1e-6),
            "a cell read at {a:?}, planned at {b:?}"
        );
    }
}
