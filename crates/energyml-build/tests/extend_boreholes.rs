//! `extend-boreholes` - a frame prolonged to at most `maxDepth`, recut at
//! `step`, marked as laid below the base it was logged to.
//!
//! The shape of the assertions comes from the reference model's build plan, the
//! rules on invented objects (ADR-0019) and from `crates/energyml-build/ARCHITECTURE.md` (the sections on enriching
//! without overwriting and on recognising a borehole) - the values are fabricated here,
//! at coordinates near the reference model's own (x ≈ 651 000, y ≈ 6 861 000).

use serde_json::{Value, json};

use energyml_build::operation::{Choices, Identity, Numbers, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

const X: f64 = 651_000.0;
const Y: f64 = 6_861_000.0;
const GROUND: f64 = 40.0;

const FRAME_UUID: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const TRAJECTORY_UUID: &str = "bbbbbbbb-0000-4000-8000-000000000002";
const DATUM_UUID: &str = "cccccccc-0000-4000-8000-000000000003";
const INTERPRETATION_UUID: &str = "dddddddd-0000-4000-8000-000000000004";
const CRS_UUID: &str = "eeeeeeee-0000-4000-8000-000000000005";

fn when() -> When {
    When {
        creation: "2026-09-24T00:00:00Z".into(),
        originator: "test".into(),
    }
}

/// A vertical frame's measured depths and its collar - the BSS shape: `x`,
/// `y` fixed, `z = ground - md`, `vertical_up` true (elevation).
fn vertical_frame(title: &str, mds: &[f64]) -> Pieces {
    vertical_frame_full(title, mds, true, true, true)
}

/// The same, with the datum, the interpretation or the wellhead left out -
/// what the refusals below exercise.
fn vertical_frame_full(
    title: &str,
    mds: &[f64],
    with_datum: bool,
    with_interpretation: bool,
    with_wellhead: bool,
) -> Pieces {
    let points: Vec<[f64; 3]> = mds.iter().map(|md| [X, Y, GROUND - md]).collect();
    let n = points.len() as u32;
    let geometry = Geometry {
        kind: "resqml22.WellboreFrameRepresentation",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points,
            topology: Topology::Polylines(vec![n]),
            closed: Vec::new(),
        }],
    };
    let known = Known {
        reference: Some(Ref::new(
            FRAME_UUID,
            "resqml22.WellboreFrameRepresentation",
            format!("{title} - frame"),
        )),
        crs: Some(Ref::new(
            CRS_UUID,
            "eml23.LocalEngineeringCompoundCrs",
            "The frame",
        )),
        vertical_uom: Some("m".to_string()),
        vertical_up: Some(true),
        node_mds: Some(mds.to_vec()),
        trajectory: Some(Ref::new(
            TRAJECTORY_UUID,
            "resqml22.WellboreTrajectoryRepresentation",
            format!("{title} - trajectory"),
        )),
        datum: with_datum.then(|| {
            Ref::new(
                DATUM_UUID,
                "eml23.ReferencePointInALocalEngineeringCompoundCrs",
                format!("{title} - datum"),
            )
        }),
        interpretation: with_interpretation.then(|| {
            Ref::new(
                INTERPRETATION_UUID,
                "resqml22.WellboreInterpretation",
                format!("{title} - interpretation"),
            )
        }),
        wellhead: with_wellhead.then_some([X, Y, GROUND]),
        ..Known::default()
    };
    Pieces::empty()
        .with(
            "frames",
            Source::Shape {
                geometry,
                closed: None,
            },
        )
        .with("frames (object)", Source::Object(known))
}

fn plan(pieces: &Pieces, answers: Value) -> Result<Plan, String> {
    let mut choices = Choices::new();
    for (name, value) in answers.as_object().expect("an object") {
        choices = choices.set(name, value.clone());
    }
    Registry::new()
        .plan(
            "extend-boreholes",
            pieces,
            &choices,
            &Identity::derived(),
            &when(),
        )
        .map_err(|why| why.to_string())
}

fn settings(max_depth: f64, step: f64, source: &str) -> Value {
    json!({ "maxDepth": max_depth, "step": step, "source": source })
}

fn new_frame(made: &Plan) -> &energyml_build::operation::NewObject {
    let found: Vec<_> = made
        .objects
        .iter()
        .filter(|o| o.qualified_type == "resqml22.WellboreFrameRepresentation")
        .collect();
    assert_eq!(found.len(), 1, "one new frame, got {}", found.len());
    found[0]
}

fn new_trajectory(made: &Plan) -> &energyml_build::operation::NewObject {
    let found: Vec<_> = made
        .objects
        .iter()
        .filter(|o| o.qualified_type == "resqml22.WellboreTrajectoryRepresentation")
        .collect();
    assert_eq!(found.len(), 1, "one new trajectory, got {}", found.len());
    found[0]
}

fn node_mds_of(made: &Plan) -> Vec<f64> {
    let frame = new_frame(made);
    let path = format!("/resqml22/{}/nodeMd", frame.uuid);
    let dataset = made
        .datasets
        .iter()
        .find(|d| d.path == path)
        .expect("the frame's NodeMd dataset");
    let Numbers::F64(values) = &dataset.values else {
        panic!("NodeMd is not floating point")
    };
    values.clone()
}

fn control_points_of(made: &Plan) -> Vec<[f64; 3]> {
    let trajectory = new_trajectory(made);
    let path = format!("/resqml22/{}/controlPoints", trajectory.uuid);
    let dataset = made
        .datasets
        .iter()
        .find(|d| d.path == path)
        .expect("the trajectory's control points");
    let Numbers::F64(values) = &dataset.values else {
        panic!("control points are not floating point")
    };
    values.chunks(3).map(|c| [c[0], c[1], c[2]]).collect()
}

// -- what is kept, what is added, where it stops -----------------------------

#[test]
fn old_measured_depths_are_kept_bit_for_bit_and_the_deepest_node_lands_on_the_limit() {
    // Whole metres throughout: what is checked is that the logged depths
    // survive **bit for bit**, and a decimal step added to a decimal depth
    // would leave a rounding question the test has no business asking.
    let mds = [0.0, 5.0, 12.0];
    let made = plan(
        &vertical_frame("F1", &mds),
        settings(50.0, 10.0, "plausible: BSS boreholes rarely exceed 200 m"),
    )
    .expect("a plan");
    let written = node_mds_of(&made);
    // The three logged depths, untouched - not a value rebuilt from the line.
    assert_eq!(
        written[..3].to_vec(),
        mds.to_vec(),
        "the logged depths must survive bit for bit: {written:?}"
    );
    // Every added node at a `step` of 10 from 12, the last exactly at 50 -
    // never a value nudged to make the step divide evenly.
    assert_eq!(written[3..].to_vec(), vec![22.0, 32.0, 42.0, 50.0], "{written:?}");
}

#[test]
fn positions_continue_the_hole_in_its_last_direction() {
    let mds = [0.0, 20.0];
    let made = plan(
        &vertical_frame("F2", &mds),
        settings(50.0, 10.0, "vertical BSS hole"),
    )
    .expect("a plan");
    let points = control_points_of(&made);
    // A BSS hole is vertical: (x, y) unchanged over every point, old and
    // added, z continuing straight down at one metre of elevation lost per
    // metre of measured depth.
    assert!(
        points.iter().all(|p| p[0] == X && p[1] == Y),
        "the added nodes must stay over the same collar: {points:?}"
    );
    let last = *points.last().unwrap();
    assert_eq!(
        last,
        [X, Y, GROUND - 50.0],
        "the deepest node must sit where the vertical continuation puts it"
    );
}

#[test]
fn the_new_objects_are_marked_synthetic_from_the_logged_base() {
    let mds = [0.0, 5.0, 12.0];
    let made = plan(
        &vertical_frame("F3", &mds),
        settings(50.0, 10.0, "plausible depth for the Paris basin"),
    )
    .expect("a plan");
    for object in [new_frame(&made), new_trajectory(&made)] {
        let marks = object.json["ExtensionNameValue"]
            .as_array()
            .expect("ExtensionNameValue");
        let synthetic = marks
            .iter()
            .find(|m| m["Name"] == "synthetic")
            .expect("a `synthetic` mark");
        assert_eq!(synthetic["Value"]["_"], Value::from("true"));
        let from = marks
            .iter()
            .find(|m| m["Name"] == "syntheticFromMd")
            .expect("a `syntheticFromMd` mark");
        // `f64`'s `Display` prints a whole number with no decimal point.
        assert_eq!(from["Value"]["_"], Value::from("12"));
        assert_eq!(from["Value"]["uom"], Value::from("m"));
        assert!(
            object.json["Citation"]["Description"]
                .as_str()
                .unwrap()
                .contains("plausible depth for the Paris basin"),
            "the source must be readable in the citation: {}",
            object.json["Citation"]["Description"]
        );
    }
}

// -- a second object, never the first rewritten -----------------------------

#[test]
fn the_old_frame_and_trajectory_are_not_rewritten() {
    let mds = [0.0, 5.0, 12.0];
    let made = plan(&vertical_frame("F4", &mds), settings(50.0, 10.0, "test")).expect("a plan");
    assert_ne!(
        new_frame(&made).uuid,
        FRAME_UUID,
        "the old frame's uuid must not be reused"
    );
    assert_ne!(
        new_trajectory(&made).uuid,
        TRAJECTORY_UUID,
        "the old trajectory's uuid must not be reused"
    );
    // And the old pair is not among the written objects at all - nothing here
    // rewrites what the file already had.
    assert!(
        made.objects
            .iter()
            .all(|o| o.uuid != FRAME_UUID && o.uuid != TRAJECTORY_UUID)
    );
}

#[test]
fn the_same_inputs_write_the_same_uuids() {
    let mds = [0.0, 5.0, 12.0];
    let bag = vertical_frame("F5", &mds);
    let one = plan(&bag, settings(50.0, 10.0, "same run twice")).expect("a plan");
    let two = plan(&bag, settings(50.0, 10.0, "same run twice")).expect("a plan");
    assert_eq!(
        new_frame(&one).uuid,
        new_frame(&two).uuid,
        "two runs of the same extension must agree on the frame's uuid"
    );
    assert_eq!(
        new_trajectory(&one).uuid,
        new_trajectory(&two).uuid,
        "and on the trajectory's"
    );
}

#[test]
fn a_different_source_is_the_same_extension() {
    // The identity comes from the content - the frame continued and the depths
    // added - and the `source` text is not content: it says where the invented
    // values come from. Rewording it rewrites the same object (13/10/2026; the
    // pair across operations is `source_not_in_identity`).
    let mds = [0.0, 5.0, 12.0];
    let bag = vertical_frame("F6", &mds);
    let one = plan(&bag, settings(50.0, 10.0, "a first plausibility")).expect("a plan");
    let two = plan(&bag, settings(50.0, 10.0, "a second, different plausibility")).expect("a plan");
    assert_eq!(
        new_frame(&one).uuid,
        new_frame(&two).uuid,
        "a different source must not rename the frame"
    );
    assert_eq!(
        new_trajectory(&one).uuid,
        new_trajectory(&two).uuid,
        "nor the trajectory"
    );
    // And a different depth is still a second extension.
    let deeper = plan(&bag, settings(60.0, 10.0, "a first plausibility")).expect("a plan");
    assert_ne!(
        new_frame(&one).uuid,
        new_frame(&deeper).uuid,
        "other depths are another object"
    );
}

// -- refusals, each able to fail on its own ----------------------------------

#[test]
fn a_frame_already_at_the_limit_is_refused() {
    let mds = [0.0, 20.0, 50.0];
    let why = plan(&vertical_frame("F7", &mds), settings(50.0, 10.0, "test")).expect_err("nothing to extend");
    assert!(
        why.contains("F7") && why.contains("50") && why.contains("limit"),
        "{why}"
    );
}

#[test]
fn a_frame_deeper_than_the_limit_is_refused() {
    let mds = [0.0, 20.0, 80.0];
    let why =
        plan(&vertical_frame("F8", &mds), settings(50.0, 10.0, "test")).expect_err("already past the limit");
    assert!(why.contains("F8") && why.contains("limit"), "{why}");
}

#[test]
fn a_frame_with_no_datum_is_refused() {
    let mds = [0.0, 5.0];
    let bag = vertical_frame_full("F9", &mds, false, true, true);
    let why = plan(&bag, settings(50.0, 10.0, "test")).expect_err("no datum to count from");
    assert!(why.contains("datum"), "{why}");
}

#[test]
fn a_frame_with_no_interpretation_is_refused() {
    let mds = [0.0, 5.0];
    let bag = vertical_frame_full("F10", &mds, true, false, true);
    let why =
        plan(&bag, settings(50.0, 10.0, "test")).expect_err("nothing to hang the second trajectory off");
    assert!(why.contains("interpretation"), "{why}");
}

#[test]
fn a_frame_with_no_wellhead_is_refused() {
    let mds = [0.0, 5.0];
    let bag = vertical_frame_full("F11", &mds, true, true, false);
    let why = plan(&bag, settings(50.0, 10.0, "test")).expect_err("nothing says where it is collared");
    assert!(why.contains("wellhead"), "{why}");
}

#[test]
fn an_empty_source_is_refused() {
    let mds = [0.0, 5.0];
    let why =
        plan(&vertical_frame("F12", &mds), settings(50.0, 10.0, "  ")).expect_err("blank is not a source");
    assert!(why.contains("source"), "{why}");
}

#[test]
fn a_non_positive_step_is_refused() {
    let mds = [0.0, 5.0];
    let why = plan(&vertical_frame("F13", &mds), settings(50.0, 0.0, "test"))
        .expect_err("a zero step never reaches maxDepth");
    assert!(why.contains("step"), "{why}");
}

#[test]
fn no_frames_is_refused() {
    let why = plan(&Pieces::empty(), settings(50.0, 10.0, "test")).expect_err("nothing to extend");
    assert!(why.contains("frames"), "{why}");
}
