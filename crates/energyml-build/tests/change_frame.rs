//! `change-frame` — a representation read in one frame and written in another
//!.
//!
//! The form of the assertions comes from the specification: a depth axis
//! counts down from an origin written *“in the base vertical CRS”* (CRS.xsd
//! l. 517–520; common TRG 2.3 §12.20 p. 322 and §12.34 p. 338); an azimuth
//! turns the local Y axis clockwise from grid north (RESQML 2.0.1 `Common.xsd`
//! l. 136–145, the sense `crs` writes); a linear velocity law `V(z) = V₀ + k·z`
//! gives the one-way time `τ(z) = ln(1 + k·z/V₀)/k`, twice that down and back,
//! and `2z/V₀` at constant velocity. The numbers are the tests' own, at **map
//! coordinates** — Lambert-93, x ≈ 651 000, y ≈ 6 861 000 — because two defects
//! of 24/09 only showed far from the origin, and in a **turned, offset** frame
//! like the reference model's.
//!
//! The frames are written by `crs` and read back by the boundary
//! (`read::known_of_object`), as the recipe reads them; where a written node is
//! is checked through `energyml_domain::crs::read_with` over the stored frame —
//! a second reading of the frame, not the operation's.
//!
//! # What would have to break to make it turn red
//!
//! Forgetting the vertical origin, or reading it in the sense of the local axis; not
//! flipping the sign of a depth; ignoring the azimuth, turning it
//! backwards or forgetting the origin of the plane; confusing one-way and two-way time;
//! counting the depth from the zero of the datum rather than from the origin of the
//! time frame; writing a grid in a turned frame; letting through
//! two projected systems, two datums, a time without a law, a law without time
//! or without a source, a unit of time that is not one, a frame without a
//! vertical; drawing the identity from the name of the piece rather than from the content; not
//! suggesting the interpretation of the input.

use std::sync::Arc;

use serde_json::{Value, json};

use energyml_build::apply::apply;
use energyml_build::operation::{Choices, Error, Identity, Numbers, Plan, Refusal, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_domain::crs::VerticalQuantity;
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};
use energyml_storage::EpcStore;

fn when() -> When {
    When {
        creation: "2026-09-25T00:00:00Z".into(),
        originator: "test".into(),
    }
}

const SHAPE: &str = "0f6c2a1e-5b3d-4c7e-9a8b-1c2d3e4f5a6b";
const H6: &str = "1a7d3b2f-6c4e-4d8f-8b9c-2d3e4f5a6b7c";

/// Where the tests work: Lambert-93, near the platform of Lutetia (§2.4).
const X0: f64 = 651_000.0;
const Y0: f64 = 6_861_000.0;

/// The platform, where the reference model's frames put their origin (`recette.toml`,
/// `repere-grille`).
const OX: f64 = 651_600.0;
const OY: f64 = 6_861_150.0;

/// A package of frames, each written by `crs` as the recipe writes
/// the base systems of the reference model.
struct Frames {
    store: EpcStore,
}

impl Frames {
    fn new() -> Frames {
        let schemas = Arc::new(energyml_schemas::all());
        Frames {
            store: energyml_build::epc::open(None, "frames.epc", schemas).expect("an empty package"),
        }
    }

    /// Write a frame with `crs`; what the boundary knows of the object of
    /// that role.
    fn write(&mut self, choices: Choices, role: &str) -> Known {
        let plan = Registry::new()
            .plan("crs", &Pieces::empty(), &choices, &Identity::derived(), &when())
            .expect("a frame");
        let made = apply(&plan, &mut self.store, &when());
        assert!(
            made.invalid.is_empty(),
            "objects written against their schema: {:#?}",
            made.invalid
        );
        assert!(made.refused.is_empty(), "{:?}", made.refused);
        let uuid = plan
            .of_role(role)
            .next()
            .unwrap_or_else(|| panic!("`crs` writes a `{role}`"))
            .uuid
            .clone();
        energyml_build::read::known_of_object(&self.store, &uuid).expect("the frame reads")
    }

    /// A compound over Lambert-93 and NGF-IGN69, with these settings of `crs`.
    fn frame(&mut self, settings: &[(&str, Value)]) -> Known {
        let mut choices = Choices::new().set("epsg", 2154).set("vertical", 5720);
        for (name, value) in settings {
            choices = choices.set(name, value.clone());
        }
        self.write(choices, "crs")
    }

    /// The frame `known` names, read again from the package — `read_with`,
    /// not the operation.
    fn read(&self, known: &Known) -> energyml_domain::crs::Crs {
        let uuid = &known.crs.as_ref().expect("a frame").uuid;
        let uri = self.store.find_by_uuid(uuid).expect("in the package").uri.clone();
        let object = self.store.get_object(&uri).expect("it reads");
        energyml_domain::crs::read_with(object.as_reflect(), &self.store).expect("a frame")
    }
}

/// A 3 × 2 lattice at map coordinates, 25 m along x and **40** along y — two
/// spacings, so that a transposed lattice shows — its heights `z(i, j)`, `i`
/// varying fastest: the house order.
fn lattice(z: impl Fn(usize, usize) -> f64) -> Geometry {
    let mut points = Vec::new();
    for j in 0..2 {
        for i in 0..3 {
            points.push([X0 + 25.0 * i as f64, Y0 + 40.0 * j as f64, z(i, j)]);
        }
    }
    Geometry {
        kind: "grid",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points,
            topology: Topology::Grid {
                fastest: 3,
                slowest: 2,
            },
            closed: Vec::new(),
        }],
    }
}

fn cloud(points: Vec<[f64; 3]>) -> Geometry {
    Geometry {
        kind: "points",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points,
            topology: Topology::Points,
            closed: Vec::new(),
        }],
    }
}

fn surface(points: Vec<[f64; 3]>, triangles: Vec<[u32; 3]>) -> Geometry {
    Geometry {
        kind: "triangles",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points,
            topology: Topology::Triangles(triangles),
            closed: Vec::new(),
        }],
    }
}

/// The bag the recipe builds: the shape named after its slot, what the
/// boundary knows of it — the frame it is in, placement included, and H6's
/// interpretation — and the frame `crs` names, as its companion.
fn bag(geometry: Geometry, qualified_type: &str, from: &Known, to: &Known) -> Pieces {
    let known = Known {
        reference: Some(Ref::new(SHAPE, qualified_type, "H6 — top of the Chalk")),
        interpretation: Some(Ref::new(H6, "resqml22.HorizonInterpretation", "H6")),
        node_count: Some(geometry.point_count()),
        ..from.clone()
    };
    Pieces::empty()
        .with(
            "shape",
            Source::Shape {
                geometry,
                closed: None,
            },
        )
        .with("shape (object)", Source::Object(known))
        .with("crs (object)", Source::Object(to.clone()))
}

/// Write into `to`.
fn into(to: &Known) -> Choices {
    Choices::new().set("crs", to.crs.as_ref().expect("a frame").dor())
}

/// A linear law, sourced.
fn linear(choices: Choices, v0: f64, k: f64, travel: &str) -> Choices {
    choices
        .set("law", "linear")
        .set("v0", v0)
        .set("k", k)
        .set("travelTime", travel)
        .set("source", "synthetic — a trial law, not a measurement")
}

fn plan(bag: &Pieces, choices: Choices) -> Result<Plan, Error> {
    Registry::new().plan("change-frame", bag, &choices, &Identity::derived(), &when())
}

fn refused(bag: &Pieces, choices: Choices) -> Refusal {
    let why = plan(bag, choices).expect_err("refused");
    why.refusals()[0].clone()
}

/// The nodes as written — the shape the plan carries is what the arrays hold.
fn written(plan: &Plan) -> Vec<[f64; 3]> {
    plan.shapes[0]
        .geometry
        .patches
        .iter()
        .flat_map(|patch| patch.points.iter().copied())
        .collect()
}

fn representation(plan: &Plan) -> &Value {
    &plan
        .of_role("representation")
        .next()
        .expect("one representation")
        .json
}

fn reported(plan: &Plan, title: &str) -> Option<f64> {
    plan.activity
        .parameters
        .iter()
        .find_map(|parameter| match parameter {
            Parameter::Double {
                title: named, value, ..
            } if named == title => Some(*value),
            _ => None,
        })
}

/// `z` of H6's top of the Chalk, NGF (§4.5: −27.7 to −48.3 m), with a hole.
fn chalk(i: usize, j: usize) -> f64 {
    if (i, j) == (2, 1) {
        f64::NAN
    } else {
        -30.0 - i as f64 - 10.0 * j as f64
    }
}

// -- elevation and depth ---------------------------------------------------------

/// **An elevation becomes a depth about the origin the depth frame states.**
///
/// The depth compound counts down from 35 m above NGF: *“in the base vertical
/// CRS”* (CRS.xsd l. 517–520), so a node at elevation z is written 35 − z, and
/// a hole stays a hole. To break: write −z (the origin forgotten), z − 35 (the
/// origin read along the local axis), z (the sign kept).
#[test]
fn an_elevation_becomes_a_depth_about_the_origin_the_frame_states() {
    let mut frames = Frames::new();
    let elevation = frames.frame(&[]);
    let depth = frames.frame(&[("axis", json!("depth")), ("originVertical", json!(35.0))]);
    let planned = plan(
        &bag(
            lattice(chalk),
            "resqml22.Grid2dRepresentation",
            &elevation,
            &depth,
        ),
        into(&depth),
    )
    .expect("planned");

    let object = representation(&planned);
    assert_eq!(object["$type"], "resqml22.Grid2dRepresentation");
    assert_eq!(
        object["Geometry"]["LocalCrs"]["Uuid"],
        json!(depth.crs.as_ref().unwrap().uuid)
    );
    let input = lattice(chalk).patches[0].points.clone();
    let output = written(&planned);
    assert_eq!(output.len(), input.len());
    for (at, (was, is)) in input.iter().zip(&output).enumerate() {
        // The two frames share their plane: x and y do not move.
        assert!(
            (is[0] - was[0]).abs() < 1e-9 && (is[1] - was[1]).abs() < 1e-9,
            "node {at}: {was:?} → {is:?}"
        );
        if was[2].is_nan() {
            assert!(is[2].is_nan(), "a hole stays a hole: node {at} → {is:?}");
            continue;
        }
        assert!(
            (is[2] - (35.0 - was[2])).abs() < 1e-9,
            "node {at}: elevation {} written {}, where 35 − z is {}",
            was[2],
            is[2],
            35.0 - was[2]
        );
    }
    // The other reading: the depth frame, read again, places each written node
    // where the elevation frame placed the input.
    let (source, target) = (frames.read(&elevation), frames.read(&depth));
    for (was, is) in input.iter().zip(&output).filter(|(was, _)| was[2].is_finite()) {
        let (a, b) = (source.to_projected(*was), target.to_projected(*is));
        for axis in 0..3 {
            assert!((a[axis] - b[axis]).abs() < 1e-6, "{a:?} placed back at {b:?}");
        }
    }
    // No law: nothing synthetic about it.
    assert!(object.get("ExtensionNameValue").is_none(), "{object}");
    assert!(reported(&planned, "timeOrigin").is_none());
}

// -- depth and time ---------------------------------------------------------------

/// **A depth becomes the analytic two-way time of a linear law.** To break:
/// one-way for two-way, seconds for milliseconds, the law's formula inverted.
#[test]
fn a_depth_becomes_the_analytic_two_way_time_of_a_linear_law() {
    let mut frames = Frames::new();
    let depth = frames.frame(&[("axis", json!("depth"))]);
    let time = frames.frame(&[("axis", json!("time")), ("timeUom", json!("ms"))]);
    let depths = [0.0, 45.0, 100.0, 150.0];
    let nodes: Vec<[f64; 3]> = depths
        .iter()
        .enumerate()
        .map(|(n, d)| [X0 + 10.0 * n as f64, Y0 - 5.0 * n as f64, *d])
        .collect();
    let pieces = bag(
        cloud(nodes.clone()),
        "resqml22.PointSetRepresentation",
        &depth,
        &time,
    );

    let planned = plan(&pieces, linear(into(&time), 1800.0, 0.6, "two-way")).expect("planned");
    let output = written(&planned);
    for ((d, was), is) in depths.iter().zip(&nodes).zip(&output) {
        // τ(z) = ln(1 + k·z/V₀)/k one way; twice that down and back; in ms.
        let expected = 1000.0 * 2.0 * (1.0 + 0.6 * d / 1800.0).ln() / 0.6;
        assert!(
            (is[2] - expected).abs() < 1e-9,
            "depth {d}: {} ms written, {expected} expected",
            is[2]
        );
        assert!(
            (is[0] - was[0]).abs() < 1e-9 && (is[1] - was[1]).abs() < 1e-9,
            "{was:?} → {is:?}"
        );
    }

    // At a constant velocity, numbers one checks on paper: 45 m at 1 800 m/s
    // is 25 ms one way and 50 ms down and back.
    let two = plan(&pieces, linear(into(&time), 1800.0, 0.0, "two-way")).expect("two-way");
    assert!(
        (written(&two)[1][2] - 50.0).abs() < 1e-9,
        "{:?}",
        written(&two)[1]
    );
    let one = plan(&pieces, linear(into(&time), 1800.0, 0.0, "one-way")).expect("one-way");
    assert!(
        (written(&one)[1][2] - 25.0).abs() < 1e-9,
        "{:?}",
        written(&one)[1]
    );

    // A time converted is not a time measured: said on the object, with its
    // source.
    let object = representation(&planned);
    assert_eq!(object["ExtensionNameValue"][0]["Name"], "synthetic", "{object}");
    let description = object["Citation"]["Description"].as_str().expect("a description");
    assert!(
        description.contains("a trial law") && description.contains("two-way"),
        "{description}"
    );
    assert_eq!(reported(&planned, "timeOrigin"), Some(0.0));
    assert!(
        planned.says.iter().any(|note| note.key == "law"),
        "{:?}",
        planned.says
    );
}

/// **The law counts depth from the time frame's origin, and comes back.**
///
/// A time frame whose origin is 10 m above NGF: 35 m below the datum is 45 m
/// below `t = 0`. A node above that origin gets a negative time, counted. And
/// the same law from the time frame gives the depths back. To break: count the
/// depth from the datum's zero (40 ms instead of 50), drop the sign of a node
/// above the origin, or invert the law the wrong way round on the way back.
#[test]
fn the_law_counts_depth_from_the_time_origin_and_comes_back() {
    let mut frames = Frames::new();
    let depth = frames.frame(&[("axis", json!("depth"))]);
    let time = frames.frame(&[
        ("axis", json!("time")),
        ("timeUom", json!("ms")),
        ("originVertical", json!(10.0)),
    ]);
    let pieces = bag(
        cloud(vec![[X0, Y0, 35.0], [X0 + 1.0, Y0, -20.0]]),
        "resqml22.PointSetRepresentation",
        &depth,
        &time,
    );

    let there = plan(&pieces, linear(into(&time), 1800.0, 0.0, "two-way")).expect("depth to time");
    let times = written(&there);
    assert!(
        (times[0][2] - 1000.0 * 2.0 * 45.0 / 1800.0).abs() < 1e-9,
        "{times:?}"
    );
    // −20 m of depth is 20 m above the datum, 10 m above `t = 0`.
    assert!(
        (times[1][2] - -1000.0 * 2.0 * 10.0 / 1800.0).abs() < 1e-9,
        "{times:?}"
    );
    assert_eq!(reported(&there, "aboveTimeOrigin"), Some(1.0));
    assert_eq!(reported(&there, "timeOrigin"), Some(10.0));
    assert!(
        there.says.iter().any(|note| note.key == "above-time-origin"),
        "{:?}",
        there.says
    );

    let back = bag(
        cloud(times.clone()),
        "resqml22.PointSetRepresentation",
        &time,
        &depth,
    );
    let back = plan(&back, linear(into(&depth), 1800.0, 0.0, "two-way")).expect("time to depth");
    for (was, is) in [35.0, -20.0].iter().zip(written(&back)) {
        assert!((is[2] - was).abs() < 1e-9, "{was} came back as {}", is[2]);
    }
}

// -- the plane ----------------------------------------------------------------------

/// **A turned, offset frame writes numbers that place back where the nodes
/// were** — the reference model's frame: 30° from grid north, its origin at the
/// platform, depths from 35 m above NGF. To break: the azimuth ignored or
/// turned the other way, the plane's origin forgotten, the depth origin lost.
#[test]
fn a_turned_and_offset_frame_places_every_node_back_where_it_was() {
    let mut frames = Frames::new();
    let elevation = frames.frame(&[]);
    let turned = frames.frame(&[
        ("originEasting", json!(OX)),
        ("originNorthing", json!(OY)),
        ("azimuth", json!(30.0)),
        ("axis", json!("depth")),
        ("originVertical", json!(35.0)),
    ]);
    let (sin, cos) = 30f64.to_radians().sin_cos();
    let nodes = vec![
        [OX, OY, 25.0],                             // the origin, 10 m under its zero
        [OX + 100.0 * sin, OY + 100.0 * cos, 35.0], // 100 m along the local Y, N30E
        [OX + 50.0 * cos, OY - 50.0 * sin, -5.0],   // 50 m along the local X, N120E
        [X0, Y0, -40.0],                            // 600 m west, 150 m south
    ];
    let pieces = bag(
        surface(nodes.clone(), vec![[0, 1, 2], [0, 2, 3]]),
        "resqml22.TriangulatedSetRepresentation",
        &elevation,
        &turned,
    );
    let planned = plan(&pieces, into(&turned)).expect("planned");
    let output = written(&planned);

    // What the azimuth and the origins say, node by node.
    for (at, (is, want)) in output
        .iter()
        .zip([[0.0, 0.0, 10.0], [0.0, 100.0, 0.0], [50.0, 0.0, 40.0]])
        .enumerate()
    {
        for axis in 0..3 {
            assert!(
                (is[axis] - want[axis]).abs() < 1e-6,
                "node {at}: {is:?}, where the frame puts {want:?}"
            );
        }
    }
    // Every node, through the other reading of both frames.
    let (source, target) = (frames.read(&elevation), frames.read(&turned));
    for (was, is) in nodes.iter().zip(&output) {
        let (a, b) = (source.to_projected(*was), target.to_projected(*is));
        for axis in 0..3 {
            assert!((a[axis] - b[axis]).abs() < 1e-6, "{a:?} placed back at {b:?}");
        }
    }
    // The frame did move the numbers — otherwise the agreement would prove
    // nothing about which ones are local.
    assert!(
        (output[3][0] - nodes[3][0]).abs() > 1_000.0 && (output[3][1] - nodes[3][1]).abs() > 1_000.0,
        "{:?}",
        output[3]
    );
    // Same topology, same order.
    assert_eq!(
        planned.shapes[0].geometry.patches[0].topology,
        Topology::Triangles(vec![[0, 1, 2], [0, 2, 3]])
    );
    assert_eq!(
        representation(&planned)["TrianglePatch"][0]["Geometry"]["LocalCrs"]["Uuid"],
        json!(turned.crs.as_ref().unwrap().uuid)
    );
}

/// **A grid moves its origin, and refuses a turned frame.** `grid` writes a
/// lattice along the frame's +x then +y: an offset keeps it — the origin moves,
/// the spacings and the order stay — and a turn would need turned axes. To
/// break: write the grid in the turned frame anyway, or keep the old origin.
#[test]
fn a_grid_moves_its_origin_and_refuses_a_turned_frame() {
    let mut frames = Frames::new();
    let elevation = frames.frame(&[]);
    let turned = frames.frame(&[
        ("originEasting", json!(OX)),
        ("originNorthing", json!(OY)),
        ("azimuth", json!(30.0)),
        ("axis", json!("depth")),
    ]);
    let moved = frames.frame(&[
        ("originEasting", json!(OX)),
        ("originNorthing", json!(OY)),
        ("axis", json!("depth")),
    ]);
    let top = |i: usize, j: usize| -30.0 - i as f64 - 10.0 * j as f64;

    let why = refused(
        &bag(lattice(top), "resqml22.Grid2dRepresentation", &elevation, &turned),
        into(&turned),
    );
    assert_eq!(why.slot.as_deref(), Some("shape"), "{}", why.message);
    assert!(why.message.contains("turned 30°"), "{}", why.message);

    let planned = plan(
        &bag(lattice(top), "resqml22.Grid2dRepresentation", &elevation, &moved),
        into(&moved),
    )
    .expect("an offset keeps the lattice");
    let supporting = &representation(&planned)["Geometry"]["Points"]["SupportingGeometry"];
    let origin = &supporting["Origin"];
    assert!(
        (origin["Coordinate1"].as_f64().unwrap() - (X0 - OX)).abs() < 1e-9,
        "{origin}"
    );
    assert!(
        (origin["Coordinate2"].as_f64().unwrap() - (Y0 - OY)).abs() < 1e-9,
        "{origin}"
    );
    // The slow axis first, as `grid` writes it: 40 along y, then 25 along x.
    assert_eq!(supporting["Dimension"][0]["Spacing"]["Value"], json!(40.0));
    assert_eq!(supporting["Dimension"][1]["Spacing"]["Value"], json!(25.0));
    // The heights, as depths from NGF's zero, in the input's order.
    let Numbers::F64(values) = &planned
        .datasets
        .iter()
        .find(|one| one.path.ends_with("/z"))
        .expect("the heights")
        .values
    else {
        panic!("the heights are floats");
    };
    let expected: Vec<f64> = lattice(top).patches[0].points.iter().map(|p| -p[2]).collect();
    assert_eq!(values.len(), expected.len());
    for (is, want) in values.iter().zip(&expected) {
        assert!((is - want).abs() < 1e-9, "{values:?} vs {expected:?}");
    }
}

// -- refused by name ----------------------------------------------------------------

/// **Two projected systems, or two datums, are a reprojection.** To break: a
/// check that compares nothing, or only the compounds' titles.
#[test]
fn two_projected_systems_or_two_datums_are_refused_by_name() {
    let mut frames = Frames::new();
    let elevation = frames.frame(&[]);
    let utm = frames.write(
        Choices::new()
            .set("epsg", 32631)
            .set("vertical", 5720)
            .set("axis", "depth"),
        "crs",
    );
    let msl = frames.write(
        Choices::new()
            .set("epsg", 2154)
            .set("vertical", 5714)
            .set("axis", "depth"),
        "crs",
    );
    let points = || cloud(vec![[X0, Y0, -30.0]]);

    let why = refused(
        &bag(points(), "resqml22.PointSetRepresentation", &elevation, &utm),
        into(&utm),
    );
    assert!(
        why.message.contains("EPSG:2154")
            && why.message.contains("EPSG:32631")
            && why.message.contains("reprojection"),
        "{}",
        why.message
    );
    let why = refused(
        &bag(points(), "resqml22.PointSetRepresentation", &elevation, &msl),
        into(&msl),
    );
    assert!(
        why.message.contains("EPSG:5720") && why.message.contains("EPSG:5714"),
        "{}",
        why.message
    );
}

/// **A time needs a law; a law needs a time, a source and a velocity.** To
/// break: default a law, accept one nothing uses, accept a blank source.
#[test]
fn a_time_needs_a_law_and_a_law_needs_a_time_and_a_source() {
    let mut frames = Frames::new();
    let elevation = frames.frame(&[]);
    let depth = frames.frame(&[("axis", json!("depth"))]);
    let time = frames.frame(&[("axis", json!("time"))]);
    let points = || cloud(vec![[X0, Y0, 40.0]]);
    let kind = "resqml22.PointSetRepresentation";

    let why = refused(&bag(points(), kind, &time, &depth), into(&depth));
    assert_eq!(
        why.slot.as_deref(),
        Some("law"),
        "time to depth with no law: {}",
        why.message
    );
    let why = refused(&bag(points(), kind, &depth, &time), into(&time));
    assert_eq!(
        why.slot.as_deref(),
        Some("law"),
        "depth to time with no law: {}",
        why.message
    );
    let why = refused(
        &bag(points(), kind, &elevation, &depth),
        linear(into(&depth), 1800.0, 0.6, "two-way"),
    );
    assert_eq!(why.slot.as_deref(), Some("law"), "{}", why.message);
    assert!(
        why.message.contains("neither"),
        "a law nothing uses: {}",
        why.message
    );
    let why = refused(
        &bag(points(), kind, &depth, &time),
        linear(into(&time), 1800.0, 0.6, "two-way").set("source", "  "),
    );
    assert_eq!(why.slot.as_deref(), Some("source"), "{}", why.message);
    let why = refused(
        &bag(points(), kind, &depth, &time),
        linear(into(&time), 0.0, 0.6, "two-way"),
    );
    assert_eq!(why.slot.as_deref(), Some("v0"), "{}", why.message);
    // The control: stated whole, the same law is planned — so each refusal
    // above is about what it names.
    plan(
        &bag(points(), kind, &depth, &time),
        linear(into(&time), 1800.0, 0.6, "two-way"),
    )
    .expect("a stated law");
}

/// **A frame that does not say how its third ordinate counts is refused by its
/// name** — a `ProjectedCrs` named directly has no vertical system; a time
/// frame counting in metres, a length frame counting in milliseconds (tampered:
/// `crs` refuses to write either). To break: default an absent vertical to an
/// elevation, or trust the flag over the unit.
#[test]
fn a_frame_that_does_not_say_how_it_counts_is_refused_by_its_name() {
    let mut frames = Frames::new();
    let elevation = frames.frame(&[]);
    let depth = frames.frame(&[("axis", json!("depth"))]);
    let projected = frames.write(
        Choices::new().set("epsg", 2154).set("vertical", 5720),
        "projected-crs",
    );
    let points = || cloud(vec![[X0, Y0, -30.0]]);
    let kind = "resqml22.PointSetRepresentation";

    let why = refused(&bag(points(), kind, &elevation, &projected), into(&projected));
    assert_eq!(why.slot.as_deref(), Some("crs"), "{}", why.message);
    assert!(
        why.message.contains("names no vertical system"),
        "{}",
        why.message
    );

    let mut odd = depth.clone();
    let placement = odd.placement.as_mut().expect("the boundary reads the placement");
    placement.vertical_quantity = VerticalQuantity::Time;
    placement.time_uom = Some("m".into());
    let why = refused(&bag(points(), kind, &elevation, &odd), into(&odd));
    assert!(why.message.contains("no time unit"), "{}", why.message);

    let mut odd = depth.clone();
    odd.placement
        .as_mut()
        .expect("the boundary reads the placement")
        .vertical_uom = Some("ms".into());
    let why = refused(&bag(points(), kind, &elevation, &odd), into(&odd));
    assert!(why.message.contains("IsTime"), "{}", why.message);

    // The control: the frame as written is taken.
    plan(&bag(points(), kind, &elevation, &depth), into(&depth)).expect("a whole frame");
}

/// **The same frame, and a frame nobody resolved, are refused.** To break:
/// fall back on an identity placement when none is known — which is right on
/// every frame of the corpus and wrong on the turned frame.
#[test]
fn the_same_frame_and_a_frame_nobody_resolved_are_refused() {
    let mut frames = Frames::new();
    let elevation = frames.frame(&[]);
    let depth = frames.frame(&[("axis", json!("depth"))]);
    let points = || cloud(vec![[X0, Y0, -30.0]]);
    let kind = "resqml22.PointSetRepresentation";

    let why = refused(&bag(points(), kind, &elevation, &elevation), into(&elevation));
    assert!(why.message.contains("already in"), "{}", why.message);

    let mut blind = elevation.clone();
    blind.placement = None;
    let why = refused(&bag(points(), kind, &blind, &depth), into(&depth));
    assert_eq!(why.slot.as_deref(), Some("shape"), "{}", why.message);
    assert!(why.message.contains("placement is not known"), "{}", why.message);

    let mut unread = depth.clone();
    unread.placement = None;
    let why = refused(&bag(points(), kind, &elevation, &unread), into(&depth));
    assert_eq!(why.slot.as_deref(), Some("crs"), "{}", why.message);
    assert!(why.message.contains("placement is not known"), "{}", why.message);
}

// -- identity and what is represented --------------------------------------------------

/// **The same input is the same object; another law, or other nodes, another
/// one.** `pointset` hangs its uuid on the bag's first piece, here always
/// `shape`: to break, drop the seeded bag — the two clouds then collide.
#[test]
fn the_same_input_is_the_same_object_and_another_content_another() {
    let mut frames = Frames::new();
    let depth = frames.frame(&[("axis", json!("depth"))]);
    let time = frames.frame(&[("axis", json!("time"))]);
    let kind = "resqml22.PointSetRepresentation";
    let first = bag(
        cloud(vec![[X0, Y0, 40.0], [X0 + 25.0, Y0, 42.0]]),
        kind,
        &depth,
        &time,
    );
    let other = bag(
        cloud(vec![[X0, Y0, 41.0], [X0 + 25.0, Y0, 42.0]]),
        kind,
        &depth,
        &time,
    );
    let uuid = |pieces: &Pieces, v0: f64| {
        plan(pieces, linear(into(&time), v0, 0.6, "two-way"))
            .expect("planned")
            .of_role("representation")
            .next()
            .unwrap()
            .uuid
            .clone()
    };
    let once = uuid(&first, 1800.0);
    assert_eq!(once, uuid(&first, 1800.0), "the same input, the same object");
    assert_ne!(
        once,
        uuid(&first, 2000.0),
        "another law is another object, beside the first"
    );
    assert_ne!(
        once,
        uuid(&other, 1800.0),
        "other nodes of the same name and title are another object"
    );
}

/// **H6 in another frame represents H6.** The reference model registers two frames
/// under H6: the input's interpretation is suggested, and nothing is
/// created beside it. To break: drop the suggestion, or let the shared writer
/// create a feature.
#[test]
fn the_same_horizon_in_another_frame_represents_the_same_interpretation() {
    let mut frames = Frames::new();
    let elevation = frames.frame(&[]);
    let depth = frames.frame(&[("axis", json!("depth"))]);
    let planned = plan(
        &bag(
            lattice(chalk),
            "resqml22.Grid2dRepresentation",
            &elevation,
            &depth,
        ),
        into(&depth),
    )
    .expect("planned");
    let object = representation(&planned);
    assert_eq!(object["RepresentedObject"]["Uuid"], json!(H6), "{object}");
    assert_eq!(object["Citation"]["Title"], json!("H6 — top of the Chalk"));
    let roles: Vec<&str> = planned.objects.iter().map(|one| one.role.as_str()).collect();
    assert_eq!(
        roles,
        vec!["representation"],
        "the frame and the interpretation are the package's: {roles:?}"
    );
    assert_eq!(planned.activity.recipe, "change-frame");
}

// -- the whole path ----------------------------------------------------------------------

/// **Written, read back, changed, read back: every node where it was.** The
/// grid is written by `grid` in the boreholes' elevation frame, read by the
/// boundary (`read::pieces_of_object`, the placement included), changed into a
/// depth frame with an origin at the platform, and both grids are read by the
/// scene reader — two readings of where each node is, the operation between
/// them. To break: any of the above, or a writer that reads the frame's
/// placement twice.
#[cfg(feature = "hdf5")]
#[test]
fn a_grid_written_read_and_changed_lands_where_the_reader_placed_it() {
    use energyml_build::pieces::Lattice;

    let dir = std::env::temp_dir().join(format!("energyml-build-change-frame-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let schemas = Arc::new(energyml_schemas::all());
    let mut frames = Frames {
        store: energyml_build::epc::open(None, "h6.epc", schemas.clone()).unwrap(),
    };
    frames.store.external_files_mut().set_root(&dir);
    let elevation = frames.frame(&[]);
    let moved = frames.frame(&[
        ("originEasting", json!(OX)),
        ("originNorthing", json!(OY)),
        ("axis", json!("depth")),
        ("originVertical", json!(35.0)),
    ]);

    // H6 as `interpolate` writes it: a grid in the boreholes' elevation frame.
    let heights: Vec<f64> = (0..6).map(|n| -30.0 - 2.5 * n as f64).collect();
    let pieces = Pieces::empty()
        .with(
            "h6",
            Source::Lattice(Lattice {
                origin: [X0, Y0],
                spacing: [25.0, 40.0],
                counts: [3, 2],
                z: heights,
            }),
        )
        .with("frame", Source::Object(elevation.clone()));
    let choices = Choices::new()
        .set("title", "H6 — top of the Chalk")
        .set("featureType", "resqml22.BoundaryFeature")
        .set("interpretationType", "resqml22.BoundaryFeatureInterpretation");
    let grid = Registry::new()
        .plan("grid", &pieces, &choices, &Identity::derived(), &when())
        .expect("the grid");
    let made = apply(&grid, &mut frames.store, &when());
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(made.refused.is_empty(), "{:?}", made.refused);
    let h6 = grid.of_role("representation").next().unwrap().uuid.clone();

    let read = energyml_build::read::in_slot(
        "shape",
        energyml_build::read::pieces_of_object(&frames.store, &h6).expect("the grid reads back"),
    );
    let mut pieces = Pieces::empty();
    for (name, source) in read {
        pieces = pieces.with(name, source);
    }
    let pieces = pieces.with("crs (object)", Source::Object(moved.clone()));
    let changed = plan(&pieces, into(&moved)).expect("planned");
    let made = apply(&changed, &mut frames.store, &when());
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(made.refused.is_empty(), "{:?}", made.refused);
    let out = dir.join("h6.epc");
    frames
        .store
        .save_as(&out, &energyml_storage::SaveOptions::default())
        .unwrap();

    let reread = EpcStore::open(&out, schemas).unwrap();
    let nodes = |uuid: &str| {
        let uri = reread.find_by_uuid(uuid).expect("in the package").uri.clone();
        let object = reread.get_object(&uri).unwrap();
        let scene =
            energyml_geometry::scene::object_of(object.as_reflect(), &reread, &uri).expect("it reads");
        let placed = scene
            .geometry_in(energyml_geometry::Placement::Projected)
            .expect("it places");
        (
            scene.geometry.patches[0].points.clone(),
            placed.patches[0].points.clone(),
        )
    };
    let (local_before, before) = nodes(&h6);
    let (local_after, after) = nodes(&changed.of_role("representation").next().unwrap().uuid);
    assert_eq!(before.len(), 6);
    assert_eq!(after.len(), before.len());
    for (a, b) in before.iter().zip(&after) {
        for axis in 0..3 {
            assert!((a[axis] - b[axis]).abs() < 1e-6, "{a:?} is now at {b:?}");
        }
    }
    // And the stored numbers did change: depths from 35 m, an origin at the
    // platform — otherwise the agreement would prove nothing.
    for (was, is) in local_before.iter().zip(&local_after) {
        assert!((is[2] - (35.0 - was[2])).abs() < 1e-9, "{was:?} → {is:?}");
        assert!(
            (was[0] - is[0] - OX).abs() < 1e-6 && (was[1] - is[1] - OY).abs() < 1e-6,
            "{was:?} → {is:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
