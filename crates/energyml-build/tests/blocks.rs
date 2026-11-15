//! The blocks the operations share (`ops/blocks.rs`) and `Shape::title`: one
//! rule each, so one test each, on data made here.
//!
//! # What would have to break to make it turn red
//!
//! A title that stays empty where a name could stand; a description limit moved
//! or counted in bytes; a frame compared by case; a flag written where none was
//! given; a seed text that changes (every uuid laid by values would change).

use energyml_build::declare::{CrsRef, Shape};
use energyml_build::ops::blocks;
use energyml_build::pieces::{Known, Ref};
use energyml_geometry::geometry::{Geometry, Patch, Topology};
use serde_json::{Value, json};

fn shape(name: &str, title: Option<&str>, points: Vec<[f64; 3]>) -> Shape {
    Shape {
        name: name.to_string(),
        geometry: Geometry {
            kind: "test",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points,
                topology: Topology::Points,
                closed: Vec::new(),
            }],
        },
        closed: None,
        known: title.map(|title| Known {
            reference: Some(Ref::new("u", "resqml22.PointSetRepresentation", title)),
            ..Default::default()
        }),
    }
}

/// The title of the object behind a shape, else its name - and an empty title is
/// none, so every operation names a shape the same way.
///
/// Break it: drop the `.filter` in `Shape::title`, or return the name first.
#[test]
fn a_shape_is_named_by_its_title_else_by_its_name() {
    assert_eq!(shape("bag-name", Some("Top"), vec![]).title(), "Top");
    assert_eq!(shape("bag-name", Some(""), vec![]).title(), "bag-name");
    assert_eq!(shape("bag-name", None, vec![]).title(), "bag-name");
}

/// A `Citation/Description` holds 2000 characters, counted as characters.
///
/// Break it: compare `len()` (bytes) or move the limit.
#[test]
fn a_description_holds_two_thousand_characters_not_bytes() {
    assert!(blocks::checked_description(&"é".repeat(2000)).is_ok());
    let why = blocks::checked_description(&"é".repeat(2001)).expect_err("2001 characters");
    assert_eq!(why.slot.as_deref(), Some("source"));
    assert!(why.message.contains("2001 characters"), "{}", why.message);
}

/// The `synthetic` mark names what laid the object.
///
/// Break it: change the `Name` or drop `Description`.
#[test]
fn the_synthetic_mark_says_what_laid_the_object() {
    let mark = blocks::synthetic_extension("laid by `x`");
    assert_eq!(mark[0]["Name"], "synthetic");
    assert_eq!(mark[0]["Value"]["_"], "true");
    assert_eq!(mark[0]["Description"], "laid by `x`");
}

/// A named frame is the inputs' frame when the uuid matches, whatever the case,
/// as a `DataObjectReference` or as a bare uuid.
///
/// Break it: compare with `==`.
#[test]
fn a_named_frame_is_compared_by_uuid_without_case() {
    let by_reference = CrsRef(json!({ "Uuid": "ABCDEF00-0000-0000-0000-000000000001" }));
    let bare = CrsRef(json!("abcdef00-0000-0000-0000-000000000001"));
    let frame = "abcdef00-0000-0000-0000-000000000001";
    assert!(blocks::names_frame(&by_reference, frame));
    assert!(blocks::names_frame(&bare, frame));
    assert!(!blocks::names_frame(
        &bare,
        "abcdef00-0000-0000-0000-000000000002"
    ));
}

/// A seed's text is exact: `{:?}` of every value, `NaN` included.
///
/// Break it: format with `{}` (1.0 becomes 1) or skip the NaN.
#[test]
fn a_digest_is_the_exact_text_of_the_values() {
    assert_eq!(blocks::digest(&[1.0, f64::NAN, -0.0, 0.1]), "1.0,NaN,-0.0,0.1");
    assert_eq!(blocks::digest(&[]), "");
}

/// A ring needs three points, and is flat `[x, y, ...]` in plan.
///
/// Break it: accept two points, or keep the z.
#[test]
fn a_ring_is_three_points_flat_in_plan() {
    let two = shape("ring", None, vec![[0.0; 3], [1.0, 0.0, 5.0]]);
    let why = blocks::ring_corners("outer", &two).expect_err("two points");
    assert_eq!(why.slot.as_deref(), Some("outer"));
    assert!(why.message.contains("holds 2 point(s)"), "{}", why.message);
    let three = shape("ring", None, vec![[0.0; 3], [1.0, 0.0, 5.0], [0.0, 1.0, 7.0]]);
    assert_eq!(
        blocks::ring_corners("outer", &three).unwrap(),
        vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0]
    );
}

/// The pair writes what it is told and nothing more: a table of heads says
/// neither `IsWellKnown` nor `IsDrilled`; the producer-owned extras land on the
/// feature; the interpretation points at the feature.
///
/// Break it: write the flags unconditionally, or point at the wrong uuid.
#[test]
fn a_wellbore_pair_writes_only_the_flags_it_is_given() {
    let cite = |name: &str| json!({ "Title": name });
    let (objects, reference) =
        blocks::wellbore_pair("f", "H1", "i", "H1 - interpretation", cite, Default::default());
    assert_eq!(objects.len(), 2);
    assert!(objects[0].json.get("IsWellKnown").is_none());
    assert!(objects[1].json.get("IsDrilled").is_none());
    assert_eq!(objects[1].json["InterpretedFeature"]["Uuid"], "f");
    assert_eq!(reference["Uuid"], "i");

    let flags = blocks::WellboreFlags {
        is_well_known: Some(true),
        is_drilled: Some(false),
        feature_extras: vec![("Aliases", Value::from(vec![json!("a")]))],
    };
    let (objects, _) = blocks::wellbore_pair("f", "H1", "i", "H1 - interpretation", cite, flags);
    assert_eq!(objects[0].json["IsWellKnown"], true);
    assert_eq!(objects[0].json["Aliases"], json!(["a"]));
    assert_eq!(objects[1].json["IsDrilled"], false);
}

/// A kind the dictionary does not hold is refused by the slot, never invented.
///
/// Break it: return a reference with an empty uuid.
#[test]
fn an_unknown_kind_is_refused_by_the_slot() {
    let why = blocks::standard_kind_dor("no such kind, anywhere", "propertyKind").expect_err("absent");
    assert_eq!(why.slot.as_deref(), Some("propertyKind"));
    assert!(why.message.contains("no such kind, anywhere"), "{}", why.message);
}

/// `[build-ops-1 S7]`: a ring holding several polyline runs is refused by
/// name, not flattened into one polygon joining the end of one line to the
/// start of the next.
///
/// Break it: remove the run count in `blocks::ring_corners`; the two squares
/// are then read as one six-corner polygon.
#[test]
fn a_ring_of_several_runs_is_refused_not_flattened() {
    let mut two_runs = shape(
        "ring",
        None,
        vec![
            [0.0, 0.0, 0.0],
            [4.0, 0.0, 0.0],
            [4.0, 4.0, 0.0],
            [1.0, 1.0, 0.0],
            [2.0, 1.0, 0.0],
            [2.0, 2.0, 0.0],
        ],
    );
    two_runs.geometry.patches[0].topology = Topology::Polylines(vec![3, 3]);
    let why = blocks::ring_corners("outer", &two_runs).expect_err("two runs");
    assert_eq!(why.slot.as_deref(), Some("outer"));
    assert!(why.message.contains("2 polyline runs"), "{}", why.message);

    // One run in one polyline stays a ring.
    let mut one_run = two_runs.clone();
    one_run.geometry.patches[0].topology = Topology::Polylines(vec![6]);
    assert!(blocks::ring_corners("outer", &one_run).is_ok());
}
