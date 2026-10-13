//! `interpolate` onto a **given** lattice — the input `onto`, an existing 2D
//! grid whose origin, steps and counts are reused as they are.
//!
//! Why it exists: `ijk-grid-from-horizons` stands its pillars on **one**
//! lattice and refuses two by name (`tests/ijk_grid.rs`), and every horizon of
//! Lutetia fitted to its own seeds landed on its own. The form of the
//! assertions is the house lattice (`crates/energyml-build/ARCHITECTURE.md`: a grid
//! is not reprojected, and its order is the house's): node
//! `(i, j)` at `origin + (i·dx, j·dy)`, `i` along x varying fastest, the slow
//! axis first in `Dimension`. The numbers are the tests' own, at map
//! coordinates in Lambert-93 where the recipe's horizons are, with an origin
//! that is no round number and two different steps — a transposed lattice shows.
//!
//! # What would have to break to make it turn red
//!
//! Ignoring `onto` and fitting the lattice to the seeds (the lattice turns red);
//! swapping the two steps (idem); forgetting the `Without(["onto"])` condition or
//! not seeing an input given as an answer (`spacing` becomes required again);
//! accepting a grid of another frame or a turned lattice; filling a
//! node out of reach; moving the uuid of a grid written without `onto`.

use serde_json::{Value, json};

use energyml_build::operation::{Applies, Choices, Identity, Numbers, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

/// The given grid: no round origin, two steps, 9 × 7 nodes.
const X0: f64 = 650_987.123_456;
const Y0: f64 = 6_860_512.654_321;
const DX: f64 = 25.0;
const DY: f64 = 40.0;
const NI: usize = 9;
const NJ: usize = 7;

fn when() -> When {
    When { creation: "2026-09-25T00:00:00Z".into(), originator: "test".into() }
}

fn frame() -> Ref {
    Ref::new("cccccccc-0000-4000-8000-000000000003", "eml23.LocalEngineeringCompoundCrs", "The boreholes frame")
}

fn other_frame() -> Ref {
    Ref::new("dddddddd-0000-4000-8000-000000000004", "eml23.LocalEngineeringCompoundCrs", "Depth frame")
}

/// The ground: a `Grid2dRepresentation` read back, its nodes placed the way
/// the reader places a lattice — `origin + direction × distance`.
fn ground() -> Geometry {
    let mut points = Vec::new();
    for j in 0..NJ {
        for i in 0..NI {
            points.push([X0 + i as f64 * DX, Y0 + j as f64 * DY, 35.0 + 0.001 * i as f64]);
        }
    }
    Geometry {
        kind: "grid",
        patches: vec![Patch { index: 0, node_offset: 0, points, topology: Topology::Grid { fastest: NI, slowest: NJ }, closed: Vec::new() }],
    }
}

/// A seed cloud: `n` points on a line across part of the grid, at `z`.
fn seeds(from: [f64; 2], to: [f64; 2], z: f64) -> Geometry {
    let n = 6;
    let points = (0..n)
        .map(|k| {
            let t = k as f64 / (n - 1) as f64;
            [from[0] + t * (to[0] - from[0]), from[1] + t * (to[1] - from[1]) + 7.0 * (k % 2) as f64, z - t]
        })
        .collect();
    Geometry {
        kind: "cloud",
        patches: vec![Patch { index: 0, node_offset: 0, points, topology: Topology::Points, closed: Vec::new() }],
    }
}

fn known(uuid: &str, qualified_type: &str, title: &str, frame: &Ref) -> Source {
    Source::Object(Known {
        reference: Some(Ref::new(uuid, qualified_type, title)),
        crs: Some(frame.clone()),
        vertical_uom: Some("m".into()),
        vertical_up: Some(true),
        interpretation: Some(Ref::new(format!("eeeeeeee-{}", &uuid[9..]), "resqml22.HorizonInterpretation", title)),
        ..Default::default()
    })
}

/// The samples, and the ground as `onto` when given — each with what the
/// package says of it, as `read::in_slot` names them.
fn bag(samples: Geometry, samples_frame: &Ref, onto: Option<(Geometry, &Ref)>) -> Pieces {
    let mut pieces = Pieces::empty()
        .with("shape", Source::Shape { geometry: samples, closed: None })
        .with("shape (object)", known("5eed5eed-0000-4000-8000-000000000001", "resqml22.PointSetRepresentation", "H4 — top", samples_frame));
    if let Some((grid, grid_frame)) = onto {
        pieces = pieces
            .with("onto", Source::Shape { geometry: grid, closed: None })
            .with("onto (object)", known("9a0d9a0d-0000-4000-8000-000000000002", "resqml22.Grid2dRepresentation", "Lutetia — DTM", grid_frame));
    }
    pieces
}

fn plan(pieces: &Pieces, choices: Choices) -> Result<Plan, String> {
    Registry::new().plan("interpolate", pieces, &choices, &Identity::derived(), &when()).map_err(|why| why.message())
}

fn idw(title: &str) -> Choices {
    Choices::new().set("method", "idw").set("title", title)
}

/// The written grid's lattice, as the file will say it:
/// `(origin, [dx, dy], [ni, nj])` — read off the object, not the plan's notes.
fn written_lattice(plan: &Plan) -> ([f64; 2], [f64; 2], [usize; 2]) {
    let grid = &plan.of_role("representation").next().expect("a grid").json;
    let support = &grid["Geometry"]["Points"]["SupportingGeometry"];
    let number = |value: &Value| value.as_f64().expect("a number");
    let count = |value: &Value| value.as_u64().expect("a count") as usize;
    let origin = [number(&support["Origin"]["Coordinate1"]), number(&support["Origin"]["Coordinate2"])];
    // The slow axis first — j, along y — as the array beside it is laid out.
    let [slow, fast] = [&support["Dimension"][0], &support["Dimension"][1]];
    assert_eq!(slow["Direction"]["Coordinate2"], json!(1.0), "the first dimension runs along y");
    assert_eq!(fast["Direction"]["Coordinate1"], json!(1.0), "the second runs along x");
    let steps = [number(&fast["Spacing"]["Value"]), number(&slow["Spacing"]["Value"])];
    let counts = [count(&grid["FastestAxisCount"]), count(&grid["SlowestAxisCount"])];
    assert_eq!(count(&fast["Spacing"]["Count"]) + 1, counts[0]);
    assert_eq!(count(&slow["Spacing"]["Count"]) + 1, counts[1]);
    (origin, steps, counts)
}

fn bits(lattice: &([f64; 2], [f64; 2], [usize; 2])) -> ([u64; 2], [u64; 2], [usize; 2]) {
    let (origin, steps, counts) = lattice;
    ([origin[0].to_bits(), origin[1].to_bits()], [steps[0].to_bits(), steps[1].to_bits()], *counts)
}

fn double(plan: &Plan, title: &str) -> Option<f64> {
    plan.activity.parameters.iter().find_map(|p| match p {
        Parameter::Double { title: t, value, .. } if t == title => Some(*value),
        _ => None,
    })
}

fn titles(plan: &Plan) -> Vec<&str> {
    plan.activity
        .parameters
        .iter()
        .map(|p| match p {
            Parameter::Double { title, .. } | Parameter::Text { title, .. } | Parameter::Object { title, .. } => title.as_str(),
        })
        .collect()
}

/// Two horizons, two seed sets over two different parts of the ground, onto
/// the ground's grid: one lattice, **bit for bit**, and it is the ground's.
#[test]
fn two_seed_sets_onto_one_grid_share_its_lattice_bit_for_bit() {
    let west = seeds([X0 + 10.0, Y0 + 20.0], [X0 + 90.0, Y0 + 150.0], 20.0);
    let east = seeds([X0 + 120.0, Y0 + 60.0], [X0 + 190.0, Y0 + 230.0], 5.0);
    let h4 = plan(&bag(west.clone(), &frame(), Some((ground(), &frame()))), idw("H4")).expect("H4 onto the ground");
    let h5 = plan(&bag(east.clone(), &frame(), Some((ground(), &frame()))), idw("H5")).expect("H5 onto the ground");

    let (a, b) = (written_lattice(&h4), written_lattice(&h5));
    assert_eq!(bits(&a), bits(&b), "two horizons onto one grid, two lattices: {a:?} and {b:?}");
    // And it is the given one's — origin and counts exactly, steps too, on
    // an axis each: 25 along x, 40 along y.
    assert_eq!(bits(&a), bits(&([X0, Y0], [DX, DY], [NI, NJ])), "not the ground's lattice: {a:?}");
    // What the activity reports is the given grid's as well.
    assert_eq!(double(&h4, "originX").map(f64::to_bits), Some(X0.to_bits()));
    assert_eq!(double(&h4, "originY").map(f64::to_bits), Some(Y0.to_bits()));
    assert_eq!(double(&h4, "fastestAxisCount"), Some(NI as f64));
    assert_eq!(double(&h4, "slowestAxisCount"), Some(NJ as f64));
    // What it was told: the grid, as an input — neither `spacing` nor
    // `margin`, which nobody gave.
    let told = titles(&h4);
    assert!(!told.contains(&"spacing") && !told.contains(&"margin"), "{told:?}");
    assert!(
        h4.activity.parameters.iter().any(|p| matches!(p, Parameter::Object { title, reference } if title == "input" && reference["Uuid"] == "9a0d9a0d-0000-4000-8000-000000000002")),
        "the given grid is not recorded as an input: {told:?}"
    );

    // The control: the same seeds fitted each to itself are two lattices —
    // what the pillars of `ijk-grid-from-horizons` could not stand on.
    let fitted = |geometry: Geometry, title: &str| {
        plan(&bag(geometry, &frame(), None), idw(title).set("spacing", 25.0).set("margin", 50.0)).expect("fitted")
    };
    let (own_a, own_b) = (written_lattice(&fitted(west, "H4")), written_lattice(&fitted(east, "H5")));
    assert_ne!(bits(&own_a), bits(&own_b), "the control does not tell two lattices apart");
}

/// `spacing` and `margin` are the other way of saying the lattice: declared
/// `Without(["onto"])`, projected optional, and **heard** — a run given
/// `onto` is not asked its spacing, and one given both is refused naming both.
#[test]
fn spacing_and_onto_are_alternatives_and_said_so() {
    let registry = Registry::new();
    let operation = registry.get("interpolate").unwrap();
    let slots = operation.needs(&Pieces::empty());
    for name in ["spacing", "margin"] {
        let slot = slots.iter().find(|s| s.name == name).unwrap();
        assert_eq!(slot.applies, Applies::Without(&["onto"]), "`{name}`");
    }
    assert!(operation.takes().iter().any(|s| s.name == "onto" && !s.is_required()), "`onto` is an optional input");
    let spacing = slots.iter().find(|s| s.name == "spacing").unwrap();
    assert!(spacing.is_required(), "`spacing` is required when it acts");

    let samples = seeds([X0 + 10.0, Y0 + 20.0], [X0 + 90.0, Y0 + 150.0], 20.0);
    // No lattice at all: `spacing` is required, as it always was.
    let why = plan(&bag(samples.clone(), &frame(), None), idw("H4")).expect_err("no lattice");
    assert!(why.contains("`spacing` is required"), "{why}");
    // Given both: refused, naming both.
    let why = plan(&bag(samples.clone(), &frame(), Some((ground(), &frame()))), idw("H4").set("spacing", 25.0))
        .expect_err("two lattices asked");
    assert!(why.contains("`spacing`") && why.contains("`onto`"), "{why}");
    let why = plan(&bag(samples, &frame(), Some((ground(), &frame()))), idw("H4").set("margin", 400.0))
        .expect_err("a margin beside a given lattice");
    assert!(why.contains("`margin`") && why.contains("`onto`"), "{why}");
}

/// A grid is never reprojected: a lattice in another frame than the samples is
/// refused naming both, and so is a written frame other than the grid's.
#[test]
fn a_grid_in_another_frame_is_refused_naming_both() {
    let samples = seeds([X0 + 10.0, Y0 + 20.0], [X0 + 90.0, Y0 + 150.0], 20.0);
    let why = plan(&bag(samples.clone(), &frame(), Some((ground(), &other_frame()))), idw("H4")).expect_err("two frames");
    assert!(why.contains("The boreholes frame") && why.contains("Depth frame"), "{why}");
    assert!(why.contains("never reprojected"), "{why}");

    // The samples say no frame; the written one is named and is not the grid's.
    let bare = Pieces::empty()
        .with("shape", Source::Shape { geometry: samples.clone(), closed: None })
        .with("onto", Source::Shape { geometry: ground(), closed: None })
        .with("onto (object)", known("9a0d9a0d-0000-4000-8000-000000000002", "resqml22.Grid2dRepresentation", "Lutetia — DTM", &frame()));
    let why = plan(&bare, idw("H4").set("crs", other_frame().dor())).expect_err("another written frame");
    assert!(why.contains("`crs`") && why.contains("The boreholes frame"), "{why}");
    let why = plan(&bare, idw("H4").set("sourceEpsg", 2154).set("project", 2154)).expect_err("a frame created here");
    assert!(why.contains("`sourceEpsg`") && why.contains("The boreholes frame"), "{why}");
    // And left out, the grid's frame is the one suggested — and written.
    let made = plan(&bare, idw("H4")).expect("the grid's frame");
    let grid = &made.of_role("representation").next().unwrap().json;
    assert_eq!(grid["Geometry"]["LocalCrs"]["Uuid"], json!(frame().uuid));
}

/// A lattice turned in its frame's numbers is refused by name, never filled as
/// if it ran along x and y.
#[test]
fn a_turned_lattice_is_refused_by_name() {
    let (sin, cos) = 30f64.to_radians().sin_cos();
    let mut turned = ground();
    for point in &mut turned.patches[0].points {
        let (dx, dy) = (point[0] - X0, point[1] - Y0);
        point[0] = X0 + dx * cos - dy * sin;
        point[1] = Y0 + dx * sin + dy * cos;
    }
    let samples = seeds([X0 + 10.0, Y0 + 20.0], [X0 + 90.0, Y0 + 150.0], 20.0);
    let why = plan(&bag(samples.clone(), &frame(), Some((turned, &frame()))), idw("H4")).expect_err("turned");
    assert!(why.contains("`onto`") && why.contains("turned"), "{why}");

    // Not a grid at all: refused at the offer, by the slot's name.
    let mut cloud = ground();
    cloud.patches[0].topology = Topology::Points;
    let why = plan(&bag(samples, &frame(), Some((cloud, &frame()))), idw("H4")).expect_err("no grid");
    assert!(why.contains("`onto`") && why.contains("no 2D grid"), "{why}");
}

/// What the samples cannot reach on the given lattice stays a hole: `NaN` in
/// the array, counted in the plan — never filled.
#[test]
fn nodes_out_of_reach_on_the_given_lattice_stay_holes_counted() {
    let samples = seeds([X0 + 10.0, Y0 + 20.0], [X0 + 40.0, Y0 + 60.0], 20.0);
    let made = plan(&bag(samples, &frame(), Some((ground(), &frame()))), idw("H4").set("radius", 60.0)).expect("planned");
    let Numbers::F64(z) = &made.datasets[0].values else { panic!("heights are doubles") };
    assert_eq!(z.len(), NI * NJ);
    let holes = z.iter().filter(|v| v.is_nan()).count();
    assert!(holes > 0 && holes < NI * NJ, "{holes} hole(s) of {}", NI * NJ);
    let said = made.says.iter().find(|n| n.key == "unreached").expect("the holes are said");
    assert!(said.detail.starts_with(&format!("{holes} node(s)")), "{}", said.detail);
}

/// A grid written **without** `onto` keeps the uuid it always had: the seed is
/// `grid:<method> of <samples>:<title>` (`ops/interpolate.rs`, `ops/grid.rs`),
/// and the given grid enters it only when there is one.
#[test]
fn a_grid_written_without_onto_keeps_its_uuid() {
    let samples = seeds([X0 + 10.0, Y0 + 20.0], [X0 + 90.0, Y0 + 150.0], 20.0);
    let made = plan(&bag(samples.clone(), &frame(), None), idw("H4").set("spacing", 25.0)).expect("planned");
    let expected = uuid::Uuid::new_v5(&energyml_build::naming::NAMESPACE, b"grid:idw of shape:H4").to_string();
    assert_eq!(made.of_role("representation").next().unwrap().uuid, expected);
    // And onto a grid, another object.
    let onto = plan(&bag(samples, &frame(), Some((ground(), &frame()))), idw("H4")).expect("planned");
    assert_ne!(onto.of_role("representation").next().unwrap().uuid, expected);
}
