//! `well-logs` — a temperature and a hydrostatic pressure laid on wellbore
//! frames by a sourced law of the vertical depth below the ground
//! below the ground.
//!
//! The form of the assertions comes from what the reference-model description and the schemas state:
//! a property on a frame is positional, one value per node
//! (`crates/energyml-build/ARCHITECTURE.md`); the depth is vertical and counted from
//! **the ground under the head** (*“vertical depth … below the ground at the
//! head”*, §2.4), not from the datum, which the doublet puts on a rig 3 m above
//! it, nor along the hole; a hydrostatic pressure is about a bar per ten
//! metres of water; the kinds are the dictionary's `temperature` and
//! `pressure` (§6.6); an invented value says so and cites its source (§3, rules
//! 4 and 5). The numbers — the heads, the slope of the ground, the law's
//! coefficients — are the tests' own, in Lambert-93 on purpose: a defect of
//! `heights` only showed far from the origin (24/09).
//!
//! # What would have to break to make it turn red
//!
//! Counting the depth from the datum, along the hole, or under each node
//! rather than under the head (`the_law_is_read_at_the_vertical_depth_below_the_ground_at_the_head`);
//! reading the vertical sense backwards or ignoring it (the frame in depth
//! turns red); writing an array of another length than the frame; extrapolating
//! above the ground or the start of the law; accepting a law without a source, two
//! grounds or none, a ground of another frame, a frame whose numbers are
//! not where its datum puts them; dropping the value from the seed (the
//! same uuid for another gradient); giving each property the first
//! uuid of a given list.

use serde_json::json;

use energyml_build::operation::{Choices, Identity, Numbers, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When {
        creation: "2026-09-24T00:00:00Z".into(),
        originator: "test".into(),
    }
}

const UP: &str = "cccccccc-0000-4000-8000-000000000003";
const DOWN: &str = "dddddddd-0000-4000-8000-000000000004";
const OTHER: &str = "abababab-0000-4000-8000-00000000000a";
const GROUND: &str = "eeeeeeee-0000-4000-8000-000000000005";
const GTH1: &str = "11111111-0000-4000-8000-000000000001";
const GTH3: &str = "33333333-0000-4000-8000-000000000003";
const SHORT: &str = "44444444-0000-4000-8000-000000000004";
const WELL_INTERPRETATION: &str = "99999999-0000-4000-8000-000000000009";

/// The head of GTH-1 as the reference model places it, in Lambert-93.
const HEAD: [f64; 2] = [651_620.0, 6_861_170.0];
/// The ground under that head, and how it slopes: 2 cm per metre toward the
/// east — so that a ground read under a deviated node is not the head's.
const GROUND_AT_HEAD: f64 = 45.0;
const SLOPE: f64 = 0.02;

/// The test's ground, as an elevation, at easting `x`.
fn ground_at(x: f64) -> f64 {
    GROUND_AT_HEAD + SLOPE * (x - HEAD[0])
}

/// The two frames of reference of the tests: one counting up (an elevation),
/// one counting down (a depth) — identity compounds both, as every frame this
/// crate writes.
fn crs(up: bool) -> Ref {
    if up {
        Ref::new(
            UP,
            "eml23.LocalEngineeringCompoundCrs",
            "Lambert-93 + NGF-IGN69 — elevation",
        )
    } else {
        Ref::new(
            DOWN,
            "eml23.LocalEngineeringCompoundCrs",
            "Lambert-93 + NGF-IGN69 — depth",
        )
    }
}

/// A wellbore frame of the test: its datum and its nodes **placed** —
/// easting, northing, elevation — and the measured depth of each node.
struct Well {
    uuid: &'static str,
    title: &'static str,
    datum: [f64; 3],
    mds: Vec<f64>,
    nodes: Vec<[f64; 3]>,
}

/// GTH-1 of the test: its datum 3 m above the ground (a kelly bushing),
/// vertical down to MD 20, then straight at 60° from the vertical toward the
/// north-west. Every node is at the end of exactly its MD of hole.
fn deviated() -> Well {
    let datum = [HEAD[0], HEAD[1], GROUND_AT_HEAD + 3.0];
    let mds = vec![0.0, 10.0, 20.0, 40.0, 60.0, 80.0, 100.0, 120.0];
    let (sin, cos) = (60f64.to_radians().sin(), 60f64.to_radians().cos());
    let (east, north) = (-std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2);
    let nodes = mds
        .iter()
        .map(|&md| {
            if md <= 20.0 {
                [datum[0], datum[1], datum[2] - md]
            } else {
                let along = md - 20.0;
                [
                    datum[0] + east * sin * along,
                    datum[1] + north * sin * along,
                    datum[2] - 20.0 - cos * along,
                ]
            }
        })
        .collect();
    Well {
        uuid: GTH1,
        title: "GTH-1 — frame",
        datum,
        mds,
        nodes,
    }
}

/// A vertical well at `(x, y)`, its datum `height` above the test's ground
/// there, a node at each of `mds`.
fn vertical(uuid: &'static str, title: &'static str, x: f64, y: f64, height: f64, mds: Vec<f64>) -> Well {
    let datum = [x, y, ground_at(x) + height];
    let nodes = mds.iter().map(|&md| [x, y, datum[2] - md]).collect();
    Well {
        uuid,
        title,
        datum,
        mds,
        nodes,
    }
}

impl Well {
    /// What the boundary resolves of it (`read::pieces_of_object`): the datum
    /// placed, the MDs read from the file, the frame's vertical sense and unit.
    fn known(&self, up: bool) -> Known {
        Known {
            reference: Some(Ref::new(
                self.uuid,
                "resqml22.WellboreFrameRepresentation",
                self.title,
            )),
            crs: Some(crs(up)),
            vertical_uom: Some("m".into()),
            vertical_up: Some(up),
            // What the property must **not** take: a frame's interpretation.
            interpretation: Some(Ref::new(
                WELL_INTERPRETATION,
                "resqml22.WellboreInterpretation",
                "GTH-1 — interpretation",
            )),
            node_count: Some(self.nodes.len()),
            node_mds: Some(self.mds.clone()),
            wellhead: Some(self.datum),
            datum: Some(Ref::new(
                "dada0000-0000-4000-8000-00000000da7a",
                "eml23.ReferencePointInALocalEngineeringCompoundCrs",
                "GTH-1 — datum",
            )),
            ..Known::default()
        }
    }
}

/// The frame in the bag, under `name`, with what is known of it — its nodes in
/// the frame's own numbers: `z` an elevation when it counts up, a depth when
/// it counts down.
fn put(bag: Pieces, name: &str, well: &Well, up: bool, known: Known) -> Pieces {
    let points: Vec<[f64; 3]> = well
        .nodes
        .iter()
        .map(|&[x, y, e]| [x, y, if up { e } else { -e }])
        .collect();
    let count = points.len() as u32;
    let geometry = Geometry {
        kind: "resqml22.WellboreFrameRepresentation",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points,
            topology: Topology::Polylines(vec![count]),
            closed: Vec::new(),
        }],
    };
    bag.with(
        name,
        Source::Shape {
            geometry,
            closed: None,
        },
    )
    .with(format!("{name} (object)"), Source::Object(known))
}

fn add(bag: Pieces, name: &str, well: &Well, up: bool) -> Pieces {
    put(bag, name, well, up, well.known(up))
}

/// The test's ground as a triangulated plate around the doublet, in the frame
/// `up` names — with an interpretation, which no property may take.
fn ground(bag: Pieces, up: bool, frame: Ref) -> Pieces {
    let corners = [
        [651_400.0, 6_860_900.0],
        [651_900.0, 6_860_900.0],
        [651_900.0, 6_861_400.0],
        [651_400.0, 6_861_400.0],
    ];
    let points = corners
        .iter()
        .map(|&[x, y]| [x, y, if up { ground_at(x) } else { -ground_at(x) }])
        .collect();
    let geometry = Geometry {
        kind: "resqml22.TriangulatedSetRepresentation",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points,
            topology: Topology::Triangles(vec![[0, 1, 2], [0, 2, 3]]),
            closed: Vec::new(),
        }],
    };
    bag.with(
        "ground",
        Source::Shape {
            geometry,
            closed: None,
        },
    )
    .with(
        "ground (object)",
        Source::Object(Known {
            reference: Some(Ref::new(
                GROUND,
                "resqml22.TriangulatedSetRepresentation",
                "Lutetia — sol",
            )),
            crs: Some(frame),
            vertical_uom: Some("m".into()),
            vertical_up: Some(up),
            interpretation: Some(Ref::new(
                "51515151-0000-4000-8000-000000000051",
                "resqml22.BoundaryFeatureInterpretation",
                "topographic surface interpretation",
            )),
            ..Known::default()
        }),
    )
}

/// GTH-1 and the ground, in a frame counting up or down.
fn gth1(up: bool) -> Pieces {
    ground(add(Pieces::empty(), "frames", &deviated(), up), up, crs(up))
}

/// The temperature law as the test states it: 12.5 °C at 20 m below the ground,
/// 30 °C per km.
fn temperature() -> Choices {
    Choices::new()
        .set("law", "temperature")
        .set("referenceTemperature", 12.5)
        .set("referenceDepth", 20.0)
        .set("gradient", 30.0)
        .set("source", "synthetic: the test's own values")
}

/// The pressure law as the test states it: a water table at 30 m of
/// elevation, fresh water, the standard gravity, a gauge pressure.
fn hydrostatic() -> Choices {
    Choices::new()
        .set("law", "hydrostatic")
        .set("waterLevel", 30.0)
        .set("source", "synthetic: the test's own values")
}

fn planned(pieces: &Pieces, choices: &Choices) -> Result<Plan, String> {
    planned_as(pieces, choices, &Identity::derived())
}

fn planned_as(pieces: &Pieces, choices: &Choices, identity: &Identity) -> Result<Plan, String> {
    Registry::new()
        .plan("well-logs", pieces, choices, identity, &when())
        .map_err(|why| why.to_string())
}

/// The values the plan writes for the property of that uuid.
fn values(plan: &Plan, uuid: &str) -> Vec<f64> {
    let path = format!("/resqml22/{uuid}/values");
    let dataset = plan
        .datasets
        .iter()
        .find(|one| one.path == path)
        .expect("the property's values");
    match &dataset.values {
        Numbers::F64(values) => values.clone(),
        other => panic!("not floating point: {other:?}"),
    }
}

/// The property written on the frame of that uuid.
fn on_frame<'a>(plan: &'a Plan, frame: &str) -> &'a energyml_build::operation::NewObject {
    plan.objects
        .iter()
        .find(|one| one.json["SupportingRepresentation"]["Uuid"] == json!(frame))
        .unwrap_or_else(|| panic!("no property on {frame}"))
}

fn same(a: f64, b: f64) -> bool {
    (a.is_nan() && b.is_nan()) || (a - b).abs() <= 1e-9 * (1.0 + b.abs())
}

fn recorded(plan: &Plan, title: &str) -> Vec<Parameter> {
    plan.activity
        .parameters
        .iter()
        .filter(|one| match one {
            Parameter::Double { title: t, .. }
            | Parameter::Text { title: t, .. }
            | Parameter::Object { title: t, .. } => t == title,
        })
        .cloned()
        .collect()
}

fn number(plan: &Plan, title: &str) -> f64 {
    match recorded(plan, title).first() {
        Some(Parameter::Double { value, .. }) => *value,
        other => panic!("`{title}` is not recorded as a number: {other:?}"),
    }
}

// -- one value per node, on the frame it names ---------------------------------

#[test]
fn one_value_per_node_on_the_frame_it_names() {
    let plan = planned(&gth1(true), &temperature()).expect("a plan");
    assert_eq!(plan.objects.len(), 1, "one property per frame");
    let property = &plan.objects[0];
    assert_eq!(property.qualified_type, "resqml22.ContinuousProperty");
    assert_eq!(property.json["SupportingRepresentation"]["Uuid"], json!(GTH1));
    assert_eq!(property.json["IndexableElement"], json!("nodes"));
    assert_eq!(property.json["Uom"], json!("degC"));
    // The kind of the standard dictionary, found by its title — §6.6 names it:
    // `temperature` 08260920.
    assert_eq!(
        property.json["PropertyKind"]["Uuid"],
        json!("08260920-c22a-4000-af25-60a8f28e19f7")
    );
    assert_eq!(property.json["PropertyKind"]["Title"], json!("temperature"));

    // Positional: exactly as many values as the frame has nodes, the empty
    // ones included — a shorter array would put each value on the wrong node.
    let nodes = deviated().nodes.len();
    assert_eq!(values(&plan, &property.uuid).len(), nodes);
    assert_eq!(plan.datasets.len(), 1);
    assert_eq!(plan.datasets[0].dimensions, vec![nodes as i64]);
}

// -- the depth: vertical, below the ground at the head -------------------------

#[test]
fn the_law_is_read_at_the_vertical_depth_below_the_ground_at_the_head() {
    let plan = planned(&gth1(true), &temperature()).expect("a plan");
    let laid = values(&plan, &plan.objects[0].uuid);
    let well = deviated();

    // What the law gives, from the test's own geometry: the ground under the
    // head is 45, each node's depth is 45 minus its elevation, and the law
    // starts at 20 m.
    let expected: Vec<f64> = well
        .nodes
        .iter()
        .map(|node| {
            let depth = GROUND_AT_HEAD - node[2];
            if depth < 20.0 {
                f64::NAN
            } else {
                12.5 + 0.030 * (depth - 20.0)
            }
        })
        .collect();
    for (at, (got, want)) in laid.iter().zip(&expected).enumerate() {
        assert!(
            same(*got, *want),
            "node {at} (MD {}): {got} where the law gives {want}",
            well.mds[at]
        );
    }
    // Five nodes lie deep enough; the datum is in the air, two are shallower
    // than the reference depth — said, not filled.
    assert_eq!(laid.iter().filter(|v| v.is_finite()).count(), 5);
    assert_eq!(number(&plan, "aboveGroundCount"), 1.0);
    assert_eq!(number(&plan, "shallowCount"), 2.0);
    assert!(same(number(&plan, "groundRead"), GROUND_AT_HEAD));

    // And not any of the three near misses, at the deepest node — MD 120,
    // 67 m below the ground at the head, 61 m east of it along the slope.
    let last = well.nodes.len() - 1;
    let deepest = laid[last];
    let from_datum = 12.5 + 0.030 * (well.datum[2] - well.nodes[last][2] - 20.0);
    let along_the_hole = 12.5 + 0.030 * (well.mds[last] - 20.0);
    let under_the_node = 12.5 + 0.030 * (ground_at(well.nodes[last][0]) - well.nodes[last][2] - 20.0);
    for (what, wrong) in [
        ("the datum", from_datum),
        ("the measured depth", along_the_hole),
        ("the ground under the node", under_the_node),
    ] {
        assert!(
            (deepest - wrong).abs() > 1e-3,
            "the deepest node reads like a depth from {what}: {deepest}"
        );
    }
}

#[test]
fn a_depth_frame_gives_the_temperatures_of_an_elevation_frame() {
    let up = planned(&gth1(true), &temperature()).expect("counting up");
    let down = planned(&gth1(false), &temperature()).expect("counting down");
    let (up, down) = (
        values(&up, &up.objects[0].uuid),
        values(&down, &down.objects[0].uuid),
    );
    assert_eq!(up.len(), down.len());
    for (at, (a, b)) in up.iter().zip(&down).enumerate() {
        assert!(same(*a, *b), "node {at}: {a} counting up, {b} counting down");
    }
    // The control: the up frame's numbers read as a depth are not the same well.
    let flipped = ground(
        put(
            Pieces::empty(),
            "frames",
            &deviated(),
            true,
            Known {
                vertical_up: Some(false),
                ..deviated().known(true)
            },
        ),
        true,
        crs(true),
    );
    assert!(
        planned(&flipped, &temperature()).is_err(),
        "the same numbers read the other way up are not where the datum is"
    );
}

#[test]
fn the_ground_read_and_the_ground_stated_agree() {
    // Two answers to one question: the ground under the head is 45, and the
    // datum stands 3 m above it. Read on the surface, or counted from the datum,
    // the values must be the same — in either sense of the axis.
    for up in [true, false] {
        let read = planned(&gth1(up), &temperature()).expect("the ground read");
        let stated = planned(
            &add(Pieces::empty(), "frames", &deviated(), up),
            &temperature().set("datumHeight", 3.0),
        )
        .expect("the ground stated");
        let (read, stated) = (
            values(&read, &read.objects[0].uuid),
            values(&stated, &stated.objects[0].uuid),
        );
        for (at, (a, b)) in read.iter().zip(&stated).enumerate() {
            assert!(same(*a, *b), "node {at} (up: {up}): {a} read, {b} stated");
        }
    }
}

// -- the hydrostatic pressure ---------------------------------------------------

#[test]
fn a_hydrostatic_pressure_is_the_weight_of_the_water_above_each_node_in_bar() {
    let plan = planned(&gth1(true), &hydrostatic()).expect("a plan");
    let property = &plan.objects[0];
    assert_eq!(property.json["Uom"], json!("bar"));
    // §6.6: `pressure` e703be06.
    assert_eq!(
        property.json["PropertyKind"]["Uuid"],
        json!("e703be06-9568-4bc6-ab1c-db15dd012e77")
    );
    assert_eq!(property.json["PropertyKind"]["Title"], json!("pressure"));

    let laid = values(&plan, &property.uuid);
    let well = deviated();
    for (at, (got, node)) in laid.iter().zip(&well.nodes).enumerate() {
        let water = 30.0 - node[2];
        let want = if node[2] > GROUND_AT_HEAD || water < 0.0 {
            f64::NAN
        } else {
            1000.0 * 9.80665 * water / 1e5
        };
        assert!(
            same(*got, want),
            "node {at}: {got} bar where {water} m of water weigh {want}"
        );
    }
    // The order of magnitude of “~1 bar per 10 m of water”: between the nodes
    // at MD 40 and MD 60, 10 m apart vertically.
    let (a, b) = (laid[3], laid[4]);
    assert!(((b - a) - 1.0).abs() < 0.05, "{} bar for 10 m of water", b - a);
    // The datum is in the air; the node at MD 10 is under the ground but above
    // the water.
    assert_eq!(number(&plan, "aboveGroundCount"), 1.0);
    assert_eq!(number(&plan, "shallowCount"), 1.0);
}

// -- what is refused -------------------------------------------------------------

#[test]
fn a_frame_that_does_not_say_which_way_is_up_is_refused() {
    let well = deviated();
    let silent = ground(
        put(
            Pieces::empty(),
            "frames",
            &well,
            true,
            Known {
                vertical_up: None,
                ..well.known(true)
            },
        ),
        true,
        crs(true),
    );
    let why = planned(&silent, &temperature()).expect_err("up or down is not guessed");
    assert!(why.contains("GTH-1") && why.contains("which way"), "{why}");

    let unitless = ground(
        put(
            Pieces::empty(),
            "frames",
            &well,
            true,
            Known {
                vertical_uom: None,
                ..well.known(true)
            },
        ),
        true,
        crs(true),
    );
    let why = planned(&unitless, &temperature()).expect_err("a unit is not guessed");
    assert!(why.contains("GTH-1") && why.contains("unit"), "{why}");

    let timed = ground(
        put(
            Pieces::empty(),
            "frames",
            &well,
            true,
            Known {
                vertical_uom: Some("ms".into()),
                ..well.known(true)
            },
        ),
        true,
        crs(true),
    );
    let why = planned(&timed, &temperature()).expect_err("a time is no depth");
    assert!(why.contains("`ms`"), "{why}");
}

#[test]
fn a_law_without_a_source_is_refused() {
    let absent = Choices::new()
        .set("law", "temperature")
        .set("referenceTemperature", 12.5)
        .set("gradient", 30.0);
    let why = planned(&gth1(true), &absent).expect_err("no source");
    assert!(why.contains("`source`"), "{why}");

    let blank = temperature().set("source", "   ");
    let why = planned(&gth1(true), &blank).expect_err("a blank source");
    assert!(why.contains("`source`") && why.contains("empty"), "{why}");
}

#[test]
fn the_ground_is_given_once_and_only_once() {
    let both = planned(&gth1(true), &temperature().set("datumHeight", 3.0)).expect_err("two grounds");
    assert!(
        both.contains("datumHeight") && both.contains("give one"),
        "{both}"
    );

    let none =
        planned(&add(Pieces::empty(), "frames", &deviated(), true), &temperature()).expect_err("no ground");
    assert!(
        none.contains("`ground`") && none.contains("`datumHeight`"),
        "{none}"
    );
}

#[test]
fn a_ground_in_another_frame_is_refused() {
    let elsewhere = ground(
        add(Pieces::empty(), "frames", &deviated(), true),
        true,
        Ref::new(OTHER, "eml23.LocalEngineeringCompoundCrs", "Another frame"),
    );
    let why = planned(&elsewhere, &temperature()).expect_err("two frames");
    assert!(why.contains("Another frame") && why.contains("GTH-1"), "{why}");
}

#[test]
fn a_frame_whose_numbers_are_not_where_its_datum_puts_them_is_refused() {
    // The datum placed, the nodes in the frame's local numbers — what an
    // origin of the compound would make of them: 6.9 million metres off.
    let well = deviated();
    let local = Well {
        nodes: well
            .nodes
            .iter()
            .map(|&[x, y, e]| [x - HEAD[0], y - HEAD[1], e])
            .collect(),
        ..deviated()
    };
    let bag = ground(add(Pieces::empty(), "frames", &local, true), true, crs(true));
    let why = planned(&bag, &temperature()).expect_err("off its datum");
    assert!(
        why.contains("GTH-1") && why.contains("node 0") && why.contains("datum"),
        "{why}"
    );

    // And a frame with fewer measured depths than nodes is refused before any
    // value is laid: the array would not say which node is where.
    let short = ground(
        put(
            Pieces::empty(),
            "frames",
            &well,
            true,
            Known {
                node_mds: Some(well.mds[..5].to_vec()),
                ..well.known(true)
            },
        ),
        true,
        crs(true),
    );
    let why = planned(&short, &temperature()).expect_err("MDs and nodes disagree");
    assert!(why.contains("8 node(s) and 5 measured depth(s)"), "{why}");
}

#[test]
fn a_frame_with_no_node_where_the_law_holds_is_set_aside_by_name() {
    // A hole logged from 1 to 3 m, its datum on the ground: the law starts at
    // 20 m, so it has nothing to give.
    let short = vertical(
        SHORT,
        "BSS000TEST — frame",
        651_700.0,
        6_861_200.0,
        0.0,
        vec![1.0, 2.0, 3.0],
    );
    let bag = add(gth1(true), "frames 2", &short, true);
    let plan = planned(&bag, &temperature()).expect("GTH-1 is still laid");
    assert_eq!(plan.objects.len(), 1);
    assert_eq!(number(&plan, "propertyCount"), 1.0);
    assert_eq!(plan.dropped.len(), 1);
    assert!(plan.dropped[0].why.contains("BSS000TEST"), "{:?}", plan.dropped);

    // Alone, it is a refusal: a plan of nothing would read as nothing to do.
    let alone = ground(add(Pieces::empty(), "frames", &short, true), true, crs(true));
    let why = planned(&alone, &temperature()).expect_err("nothing to lay");
    assert!(why.contains("BSS000TEST"), "{why}");
}

// -- what the property says of itself ---------------------------------------------

#[test]
fn a_laid_property_says_it_is_synthetic_and_cites_its_source() {
    let plan = planned(
        &gth1(true),
        &temperature().set("source", "a sourced law — synthetic"),
    )
    .expect("a plan");
    let property = &plan.objects[0];
    let extension = &property.json["ExtensionNameValue"][0];
    assert_eq!(extension["Name"], json!("synthetic"));
    assert_eq!(extension["Value"]["_"], json!("true"));
    let description = property.json["Citation"]["Description"]
        .as_str()
        .expect("a description");
    assert!(
        description.contains("not measured") && description.contains("a sourced law — synthetic"),
        "{description}"
    );
    assert!(plan.says.iter().any(|note| note.key == "synthetic"));
}

#[test]
fn the_property_is_named_for_the_wellbore_and_takes_nothing_of_the_frame_or_the_ground() {
    let plan = planned(&gth1(true), &temperature()).expect("a plan");
    let property = &plan.objects[0];
    assert_eq!(property.title, "GTH-1 — temperature");
    assert_eq!(property.json["Citation"]["Title"], json!("GTH-1 — temperature"));
    let written = property.json.to_string();
    assert!(
        !written.contains(WELL_INTERPRETATION),
        "the frame's interpretation reached the property: {written}"
    );
    assert!(
        !written.contains("topographic surface interpretation"),
        "the ground's interpretation reached the property: {written}"
    );

    // Nothing to hand over: no slot a shared suggestion could fill with them.
    let registry = Registry::new();
    let operation = registry.get("well-logs").expect("`well-logs` is in the registry");
    let names: Vec<&str> = operation
        .needs(&gth1(true))
        .iter()
        .map(|slot| slot.name)
        .collect();
    assert!(
        !names.contains(&"title") && !names.contains(&"interpretation"),
        "{names:?}"
    );
}

// -- identity -----------------------------------------------------------------------

#[test]
fn the_same_inputs_give_the_same_uuids_and_another_gradient_another_property() {
    let two = add(
        gth1(true),
        "frames 2",
        &vertical(
            GTH3,
            "GTH-3 — frame",
            651_800.0,
            6_861_200.0,
            3.0,
            (0..=6).map(|k| 10.0 * k as f64).collect(),
        ),
        true,
    );
    let uuids = |plan: &Plan| {
        plan.objects
            .iter()
            .map(|one| one.uuid.clone())
            .collect::<Vec<_>>()
    };

    let first = uuids(&planned(&two, &temperature()).expect("a plan"));
    let again = uuids(&planned(&two, &temperature()).expect("a plan"));
    assert_eq!(first.len(), 2);
    assert_ne!(first[0], first[1], "two frames, two properties");
    assert_eq!(first, again, "the same inputs, the same objects");

    // Another gradient is another property, beside the first — never the first
    // rewritten under its uuid.
    let steeper = uuids(&planned(&two, &temperature().set("gradient", 31.0)).expect("a plan"));
    assert!(
        steeper.iter().all(|uuid| !first.contains(uuid)),
        "{first:?} {steeper:?}"
    );

    // A reworded source rewrites the same property.
    let reworded =
        uuids(&planned(&two, &temperature().set("source", "synthetic, said otherwise")).expect("a plan"));
    assert_eq!(first, reworded);
}

#[test]
fn given_uuids_are_handed_out_one_per_property() {
    let two = add(
        gth1(true),
        "frames 2",
        &vertical(
            GTH3,
            "GTH-3 — frame",
            651_800.0,
            6_861_200.0,
            3.0,
            (0..=6).map(|k| 10.0 * k as f64).collect(),
        ),
        true,
    );
    let given = vec![
        "0a0a0a0a-0000-4000-8000-000000000001".to_string(),
        "0b0b0b0b-0000-4000-8000-000000000002".to_string(),
    ];
    let plan = planned_as(&two, &temperature(), &Identity::Given(given.clone())).expect("a plan");
    assert_eq!(
        plan.objects
            .iter()
            .map(|one| one.uuid.clone())
            .collect::<Vec<_>>(),
        given
    );
    assert_eq!(on_frame(&plan, GTH3).uuid, given[1]);
    assert_eq!(values(&plan, &given[1]).len(), 7);

    let why = planned_as(&two, &temperature(), &Identity::Given(given[..1].to_vec())).expect_err("one short");
    assert!(why.contains("at least 2"), "{why}");
}
