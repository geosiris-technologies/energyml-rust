//! `horizon-from-law` — a horizon laid by a sourced law over a reference grid,
//! cut by the erosion surfaces above it (step
//! 4 of the reference model).
//!
//! The form of the assertions comes from what the reference-model description and the schemas
//! state: a thickness laid **below** is down in the frame's sense (a depth
//! axis counts down, RESQML's `VerticalAxis.Direction`); an older horizon
//! **stops** at the erosion surface above it (RESQML Technical Usage Guide 2.2,
//! draft, §7.2.2.2, table 5 p. 84); a grid is
//! written in the house lattice order (`crates/energyml-build/ARCHITECTURE.md`); an
//! invented object says so. The numbers are
//! the tests' own.
//!
//! # What would have to break to make it turn red
//!
//! Reading the vertical sense backwards or ignoring it (the depth turns red);
//! forgetting the cuts, or applying them on the wrong side (a node above
//! the cut survives); filling a hole of the reference; accepting an empty
//! source, one more frame, a reference in time, a turned lattice;
//! suggesting the interpretation of the reference for the new horizon; naming
//! the object after something other than what it is.

use serde_json::json;

use energyml_build::operation::{Choices, Fitness, Identity, Numbers, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When { creation: "2026-09-24T00:00:00Z".into(), originator: "test".into() }
}

const REFERENCE: &str = "6b0e4c2a-1f3d-4e5a-9b7c-8d9e0f1a2b3c";
const CUT: &str = "7c1f5d3b-2a4e-4f6b-8c8d-9e0f1a2b3c4d";

fn frame() -> Ref {
    Ref::new("cccccccc-0000-4000-8000-000000000003", "eml23.LocalEngineeringCompoundCrs", "The frame")
}

fn other_frame() -> Ref {
    Ref::new("dddddddd-0000-4000-8000-000000000004", "eml23.LocalEngineeringCompoundCrs", "The other frame")
}

/// The positions of the reference: a 5 × 4 lattice from (1000, 2000), 25
/// along x and **40** along y — two spacings, so that a transposed lattice
/// shows — `i` along x varying fastest, the house order.
fn positions() -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    for j in 0..4 {
        for i in 0..5 {
            out.push([1000.0 + 25.0 * i as f64, 2000.0 + 40.0 * j as f64]);
        }
    }
    out
}

/// The reference grid at `z(x, y)`, read as a `Grid2dRepresentation` reads.
fn lattice(z: impl Fn(f64, f64) -> f64) -> Geometry {
    let points = positions().into_iter().map(|[x, y]| [x, y, z(x, y)]).collect();
    Geometry {
        kind: "grid",
        patches: vec![Patch { index: 0, node_offset: 0, points, topology: Topology::Grid { fastest: 5, slowest: 4 }, closed: Vec::new() }],
    }
}

/// What the caller knows of an object of the package — with an
/// interpretation, which the new horizon must **not** be offered.
fn known(uuid: &str, title: &str, frame: &Ref, uom: &str, up: bool) -> Source {
    Source::Object(Known {
        reference: Some(Ref::new(uuid, "resqml22.Grid2dRepresentation", title)),
        crs: Some(frame.clone()),
        vertical_uom: Some(uom.into()),
        vertical_up: Some(up),
        interpretation: Some(Ref::new("eeeeeeee-0000-4000-8000-000000000005", "resqml22.HorizonInterpretation", "H6")),
        node_count: Some(20),
        ..Default::default()
    })
}

/// The reference alone, at `z(x, y)`, in an elevation (`up`) or depth frame.
fn reference(z: impl Fn(f64, f64) -> f64, up: bool) -> Pieces {
    Pieces::empty()
        .with("reference", Source::Shape { geometry: lattice(z), closed: None })
        .with("reference (object)", known(REFERENCE, "H6 — top of the Chalk", &frame(), "m", up))
}

/// A triangulated plate over `[x0, x1] × [1990, 2130]` at `z(x)`.
fn plate(x0: f64, x1: f64, z: impl Fn(f64) -> f64) -> Geometry {
    let corners = [[x0, 1990.0], [x1, 1990.0], [x1, 2130.0], [x0, 2130.0]];
    Geometry {
        kind: "surface",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points: corners.iter().map(|&[x, y]| [x, y, z(x)]).collect(),
            topology: Topology::Triangles(vec![[0, 1, 2], [0, 2, 3]]),
            closed: Vec::new(),
        }],
    }
}

/// The bag with one cut, in the reference's frame.
fn with_cut(pieces: Pieces, cut: Geometry, up: bool) -> Pieces {
    pieces
        .with("cuts", Source::Shape { geometry: cut, closed: None })
        .with("cuts (object)", known(CUT, "H1 — base of the alluvium", &frame(), "m", up))
}

/// H7, 95 below its reference — the answers every test starts from.
fn answers() -> Choices {
    Choices::new()
        .set("thickness", 95.0)
        .set("side", "below")
        .set("source", "synthetic: the test's own value")
        .set("title", "H7 — top of the Santonian")
}

fn planned(pieces: &Pieces, choices: &Choices) -> Result<Plan, String> {
    Registry::new().plan("horizon-from-law", pieces, choices, &Identity::derived(), &when()).map_err(|why| why.message())
}

fn heights(plan: &Plan) -> Vec<f64> {
    match &plan.datasets[0].values {
        Numbers::F64(values) => values.clone(),
        other => panic!("not floating point: {other:?}"),
    }
}

fn recorded(plan: &Plan, title: &str) -> Option<Parameter> {
    plan.activity
        .parameters
        .iter()
        .find(|one| match one {
            Parameter::Double { title: t, .. } | Parameter::Text { title: t, .. } | Parameter::Object { title: t, .. } => t == title,
        })
        .cloned()
}

fn number(plan: &Plan, title: &str) -> f64 {
    match recorded(plan, title) {
        Some(Parameter::Double { value, .. }) => value,
        other => panic!("`{title}` is not a number of the activity: {other:?}"),
    }
}

fn text(plan: &Plan, title: &str) -> String {
    match recorded(plan, title) {
        Some(Parameter::Text { value, .. }) => value,
        other => panic!("`{title}` is not a word of the activity: {other:?}"),
    }
}

fn says(plan: &Plan, key: &str) -> bool {
    plan.says.iter().any(|note| note.key == key)
}

/// A dipping reference, so that a constant answer cannot pass.
fn dipping(x: f64, y: f64) -> f64 {
    40.0 + 0.01 * (x - 1000.0) - 0.02 * (y - 2000.0)
}

// -- the law, laid ---------------------------------------------------------

/// **A parallel horizon is the thickness away, toward its side, in the
/// frame's sense**: below is `z − t` in elevation and `z + t` in depth, above
/// the reverse — the sense read off the reference's frame and recorded. And it
/// is written on the reference's own lattice: origin, both spacings, both
/// counts, the slow axis first.
#[test]
fn a_parallel_horizon_is_at_the_thickness_towards_its_side_in_the_sense_of_the_frame() {
    let cases = [(true, "below", -95.0, "elevation"), (false, "below", 95.0, "depth"), (true, "above", 95.0, "elevation"), (false, "above", -95.0, "depth")];
    for (up, side, offset, axis) in cases {
        let plan = planned(&reference(dipping, up), &answers().set("side", side)).unwrap_or_else(|why| panic!("{why}"));
        let laid = heights(&plan);
        assert_eq!(laid.len(), 20, "one height per node of the reference");
        for ([x, y], z) in positions().into_iter().zip(&laid) {
            let expected = dipping(x, y) + offset;
            assert!((z - expected).abs() < 1e-9, "{side} in {axis}, at ({x}, {y}): {z} where {expected}");
        }
        assert_eq!(text(&plan, "axis"), axis, "the sense comes from the frame");
    }

    let plan = planned(&reference(dipping, true), &answers()).unwrap();
    let grid = &plan.of_role("representation").next().expect("the grid").json;
    assert_eq!(grid["$type"], json!("resqml22.Grid2dRepresentation"));
    assert_eq!(grid["FastestAxisCount"], json!(5));
    assert_eq!(grid["SlowestAxisCount"], json!(4));
    let lattice = &grid["Geometry"]["Points"]["SupportingGeometry"];
    assert_eq!(lattice["Origin"]["Coordinate1"], json!(1000.0));
    assert_eq!(lattice["Origin"]["Coordinate2"], json!(2000.0));
    // The slow axis first — y, 40 apart, 3 steps — then x, 25 apart, 4 steps.
    assert_eq!(lattice["Dimension"][0]["Spacing"]["Value"], json!(40.0));
    assert_eq!(lattice["Dimension"][0]["Spacing"]["Count"], json!(3));
    assert_eq!(lattice["Dimension"][1]["Spacing"]["Value"], json!(25.0));
    assert_eq!(lattice["Dimension"][1]["Spacing"]["Count"], json!(4));
    // In the reference's frame, and no other object: nothing was created.
    assert_eq!(grid["Geometry"]["LocalCrs"]["Uuid"], json!(frame().uuid));
    assert_eq!(plan.objects.len(), 1, "{:?}", plan.objects.iter().map(|o| &o.qualified_type).collect::<Vec<_>>());
    assert_eq!(plan.shapes.len(), 1, "the grid computed travels with the plan");
}

/// **A linear law** is its thickness at the anchor, and changes along its
/// azimuth only — east here, so along x — and the plan says the range laid.
#[test]
fn a_linear_law_changes_along_its_azimuth_only() {
    let answers = answers()
        .set("law", "linear")
        .set("thickness", 10.0)
        .set("gradient", 0.1)
        .set("azimuth", 90.0)
        .set("anchorX", 1000.0)
        .set("anchorY", 2000.0);
    let plan = planned(&reference(|_, _| 0.0, true), &answers).unwrap_or_else(|why| panic!("{why}"));
    for ([x, y], z) in positions().into_iter().zip(heights(&plan)) {
        let expected = -(10.0 + 0.1 * (x - 1000.0));
        assert!((z - expected).abs() < 1e-9, "at ({x}, {y}): {z} where {expected}");
    }
    assert!(says(&plan, "thickness-range"), "{:?}", plan.says);
    assert_eq!(text(&plan, "law"), "linear");
}

/// **A hole of the reference is a hole of the horizon, and it is counted** —
/// never filled from its neighbours.
#[test]
fn a_hole_in_the_reference_stays_a_hole_and_is_counted() {
    let holed = |x: f64, y: f64| if (x == 1025.0 && y == 2040.0) || (x == 1100.0 && y == 2120.0) { f64::NAN } else { dipping(x, y) };
    let plan = planned(&reference(holed, true), &answers()).unwrap_or_else(|why| panic!("{why}"));
    let laid = heights(&plan);
    for ([x, y], z) in positions().into_iter().zip(&laid) {
        assert_eq!(z.is_nan(), holed(x, y).is_nan(), "at ({x}, {y}): {z}");
    }
    assert_eq!(laid.iter().filter(|z| z.is_nan()).count(), 2);
    assert_eq!(number(&plan, "holeCount"), 2.0);
    assert!(says(&plan, "reference-holes") && says(&plan, "holes"), "{:?}", plan.says);
}

// -- the truncation --------------------------------------------------------

/// **A cut never lets the horizon rise above it**, in either sense of the
/// frame: a node above the erosion surface is a hole and counted, a node below
/// it keeps its height, a node no cut lies over is kept and counted apart.
#[test]
fn a_cut_never_lets_the_horizon_pass_above_it() {
    // In elevation: the horizon at 0 − 10 = −10; the cut over x ∈ [990, 1060]
    // at −13 + (x − 1000)/10 — below the horizon at x = 1000 and 1025
    // (eroded), above it at x = 1050 (kept); x = 1075 and 1100 lie beyond it.
    let cut = |x: f64| -13.0 + (x - 1000.0) / 10.0;
    let pieces = with_cut(reference(|_, _| 0.0, true), plate(990.0, 1060.0, cut), true);
    let plan = planned(&pieces, &answers().set("thickness", 10.0)).unwrap_or_else(|why| panic!("{why}"));
    let laid = heights(&plan);
    let mut kept_under = 0;
    for ([x, y], z) in positions().into_iter().zip(&laid) {
        if x <= 1060.0 {
            assert!(z.is_nan() || *z <= cut(x), "at ({x}, {y}): {z} above the cut at {}", cut(x));
            if z.is_finite() {
                kept_under += 1;
            }
        } else {
            assert_eq!(*z, -10.0, "beyond the cut at ({x}, {y})");
        }
    }
    assert_eq!(kept_under, 4, "the column under the cut keeps its heights: {laid:?}");
    assert_eq!(number(&plan, "erodedCount"), 8.0);
    assert_eq!(number(&plan, "beyondCutsCount"), 8.0);
    assert!(says(&plan, "eroded") && says(&plan, "beyond-cuts"), "{:?}", plan.says);

    // In depth, the same picture: the horizon at 0 + 10 = 10, the cut at
    // 13 − (x − 1000)/10 — deeper than the horizon at x = 1000, so above it
    // there is nothing left to cut: the horizon is eroded.
    let cut = |x: f64| 13.0 - (x - 1000.0) / 10.0;
    let pieces = with_cut(reference(|_, _| 0.0, false), plate(990.0, 1060.0, cut), false);
    let plan = planned(&pieces, &answers().set("thickness", 10.0)).unwrap_or_else(|why| panic!("{why}"));
    for ([x, y], z) in positions().into_iter().zip(heights(&plan)) {
        if x <= 1060.0 {
            assert!(z.is_nan() || z >= cut(x), "at ({x}, {y}): {z} shallower than the cut at depth {}", cut(x));
        }
    }
    assert_eq!(number(&plan, "erodedCount"), 8.0);
}

/// **A grid is a cut too** — the ground is one — read on its cells,
/// bilinearly, the reading `thickness` gives its base.
#[test]
fn a_grid_cuts_like_a_surface() {
    // A 3 × 2 grid over the whole reference at −11 + (x − 1000)/20: below the
    // horizon (−10) at x = 1000 only.
    let mut points = Vec::new();
    for y in [2000.0, 2120.0] {
        for x in [1000.0, 1050.0, 1100.0] {
            points.push([x, y, -11.0 + (x - 1000.0) / 20.0]);
        }
    }
    let ground = Geometry {
        kind: "grid",
        patches: vec![Patch { index: 0, node_offset: 0, points, topology: Topology::Grid { fastest: 3, slowest: 2 }, closed: Vec::new() }],
    };
    let pieces = with_cut(reference(|_, _| 0.0, true), ground, true);
    let plan = planned(&pieces, &answers().set("thickness", 10.0)).unwrap_or_else(|why| panic!("{why}"));
    for ([x, y], z) in positions().into_iter().zip(heights(&plan)) {
        assert_eq!(z.is_nan(), x == 1000.0, "at ({x}, {y}): {z}");
    }
    assert_eq!(number(&plan, "erodedCount"), 4.0);
    assert_eq!(number(&plan, "beyondCutsCount"), 0.0);
}

// -- what the object says of itself ----------------------------------------

/// **Laid, not measured, and from what**: `synthetic = true` and a
/// description naming the law, the reference and the source — rule 4 of
/// the rule for invented values — and the thickness recorded with the frame's unit.
#[test]
fn the_horizon_says_it_is_laid_and_cites_its_source() {
    let plan = planned(&reference(dipping, true), &answers()).unwrap();
    let grid = &plan.of_role("representation").next().unwrap().json;
    assert_eq!(grid["ExtensionNameValue"][0]["Name"], json!("synthetic"));
    assert_eq!(grid["ExtensionNameValue"][0]["Value"]["_"], json!("true"));
    let description = grid["Citation"]["Description"].as_str().expect("a description");
    assert!(description.contains("not measured"), "{description}");
    assert!(description.contains("the test's own value"), "the source: {description}");
    assert!(description.contains("H6 — top of the Chalk"), "the reference: {description}");
    assert!(says(&plan, "synthetic") && says(&plan, "law") && says(&plan, "vertical-thickness"), "{:?}", plan.says);
    match recorded(&plan, "thickness") {
        Some(Parameter::Double { value, uom, .. }) => {
            assert_eq!(value, 95.0);
            assert_eq!(uom.as_deref(), Some("m"), "the unit of the reference's heights");
        }
        other => panic!("the thickness is not recorded: {other:?}"),
    }
    assert_eq!(text(&plan, "source"), "synthetic: the test's own value");
}

/// **The reference lends its frame and its sense, not its interpretation nor
/// its title**: the horizon laid is another one.
#[test]
fn the_reference_lends_its_frame_not_its_interpretation() {
    let registry = Registry::new();
    let needs = registry.get("horizon-from-law").expect("registered").needs(&reference(dipping, false));
    let slot = |name: &str| needs.iter().find(|one| one.name == name).unwrap_or_else(|| panic!("no slot `{name}`"));
    assert_eq!(slot("crs").suggested.as_ref().and_then(|v| v["Uuid"].as_str()), Some(frame().uuid.as_str()));
    assert_eq!(slot("axis").suggested, Some(json!("depth")));
    assert_eq!(slot("interpretation").suggested, None, "H6's interpretation offered to H7");
    assert_eq!(slot("title").suggested, None, "H6's title offered to H7");
    assert_eq!(slot("side").suggested, None, "which side is the geology: nobody's default");
}

/// **Same inputs, same objects** — and another horizon, another object.
#[test]
fn same_inputs_same_uuid_and_another_title_another_object() {
    let once = planned(&reference(dipping, true), &answers()).unwrap();
    let twice = planned(&reference(dipping, true), &answers()).unwrap();
    let uuids = |plan: &Plan| plan.objects.iter().map(|one| one.uuid.clone()).collect::<Vec<_>>();
    assert_eq!(uuids(&once), uuids(&twice));
    let other = planned(&reference(dipping, true), &answers().set("title", "H7 bis")).unwrap();
    assert_ne!(uuids(&once), uuids(&other));
}

// -- refused by name -------------------------------------------------------

/// **A reference that is no grid is unfit, and the reason names the role and
/// the remedy.**
#[test]
fn a_reference_that_is_not_a_grid_is_refused_at_the_offer() {
    let cloud = Geometry {
        kind: "points",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [10.0, 10.0, 0.0]],
            topology: Topology::Points,
            closed: Vec::new(),
        }],
    };
    let pieces = Pieces::empty().with("reference", Source::Shape { geometry: cloud, closed: None });
    match Registry::new().get("horizon-from-law").unwrap().fitness(&pieces) {
        Fitness::Unfit(why) => {
            assert!(why.message.contains("`reference`") && why.message.contains("interpolate"), "{}", why.message)
        }
        Fitness::Fit => panic!("a cloud was taken for a lattice"),
    }
    // And the lattice itself: fit.
    assert_eq!(Registry::new().get("horizon-from-law").unwrap().fitness(&reference(dipping, true)), Fitness::Fit);
}

/// **No thickness without a source**, absent or blank.
#[test]
fn a_thickness_without_a_source_is_refused() {
    let absent = Choices::new().set("thickness", 95.0).set("side", "below").set("title", "H7");
    let why = planned(&reference(dipping, true), &absent).unwrap_err();
    assert!(why.contains("`source` is required"), "{why}");
    let why = planned(&reference(dipping, true), &answers().set("source", "   ")).unwrap_err();
    assert!(why.contains("`source`") && why.contains("empty"), "{why}");
}

/// **One frame, the reference's**: a cut in another frame or unit, a `crs`
/// that names another system, a frame created here, a reference in time.
#[test]
fn a_single_frame_that_of_the_reference() {
    let elsewhere = reference(|_, _| 0.0, true)
        .with("cuts", Source::Shape { geometry: plate(990.0, 1060.0, |_| -20.0), closed: None })
        .with("cuts (object)", known(CUT, "H1", &other_frame(), "m", true));
    let why = planned(&elsewhere, &answers()).unwrap_err();
    assert!(why.contains("The other frame") && why.contains("two origins"), "{why}");

    let in_feet = reference(|_, _| 0.0, true)
        .with("cuts", Source::Shape { geometry: plate(990.0, 1060.0, |_| -20.0), closed: None })
        .with("cuts (object)", known(CUT, "H1", &frame(), "ft", true));
    let why = planned(&in_feet, &answers()).unwrap_err();
    assert!(why.contains("`ft`"), "{why}");

    let why = planned(&reference(dipping, true), &answers().set("crs", other_frame().dor())).unwrap_err();
    assert!(why.contains("`crs`") && why.contains("The frame"), "{why}");

    let why = planned(&reference(dipping, true), &answers().set("sourceEpsg", 2154)).unwrap_err();
    assert!(why.contains("`sourceEpsg`"), "{why}");

    let in_time = Pieces::empty()
        .with("reference", Source::Shape { geometry: lattice(dipping), closed: None })
        .with("reference (object)", known(REFERENCE, "H6 en temps", &frame(), "ms", false));
    let why = planned(&in_time, &answers()).unwrap_err();
    assert!(why.contains("`ms`") && why.contains("length"), "{why}");
}

/// **What is not a law is not laid**: a zero thickness, a negative one, a
/// linear law that turns negative, a lattice that is not the house one, and a
/// horizon eroded everywhere — which would be a grid of holes.
#[test]
fn what_is_not_a_law_is_not_laid() {
    let why = planned(&reference(dipping, true), &answers().set("thickness", 0.0)).unwrap_err();
    assert!(why.contains("`thickness`") && why.contains("under another name"), "{why}");

    let why = planned(&reference(dipping, true), &answers().set("thickness", -5.0)).unwrap_err();
    assert!(why.contains("`thickness`"), "{why}");

    // 5 at x = 1000, thinning by 0.1 a metre eastward: negative past x = 1050.
    let thinning = answers()
        .set("law", "linear")
        .set("thickness", 5.0)
        .set("gradient", -0.1)
        .set("azimuth", 90.0)
        .set("anchorX", 1000.0)
        .set("anchorY", 2000.0);
    let why = planned(&reference(dipping, true), &thinning).unwrap_err();
    assert!(why.contains("negative thickness at 8 node(s)"), "{why}");

    // One node moved off the lattice.
    let mut moved = lattice(dipping);
    moved.patches[0].points[7][0] += 3.0;
    let pieces = Pieces::empty()
        .with("reference", Source::Shape { geometry: moved, closed: None })
        .with("reference (object)", known(REFERENCE, "H6", &frame(), "m", true));
    let why = planned(&pieces, &answers()).unwrap_err();
    assert!(why.contains("`reference`") && why.contains("node (2, 1)"), "{why}");

    // A cut below the whole horizon erodes it all.
    let everywhere = with_cut(reference(|_, _| 0.0, true), plate(990.0, 1110.0, |_| -500.0), true);
    let why = planned(&everywhere, &answers()).unwrap_err();
    assert!(why.contains("no node of the horizon is left") && why.contains("20 eroded"), "{why}");
}
