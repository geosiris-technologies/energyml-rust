//! `fault-from-sticks` - a fault's feature, its interpretation, the surface
//! lofted between its sticks and the sticks themselves
//!.
//!
//! The form of the assertions comes from the schema and the reference-model description: a
//! `FaultInterpretation` interprets a `BoundaryFeature` and is what its
//! representations represent; *“IsListric … Must be present if the
//! fault is normal. Must not be present if the fault is not normal”*
//! (Interpretations.xsd l. 659-662); the sticks are `interpretation
//! line`s (Structural.xsd l. 55-58); an invented object carries
//! `synthetic = true` and its source. The
//! numbers are the tests' own, placed where Lutetia's F1 is - at map
//! coordinates, since a defect of 24/09 only showed far from the origin.
//!
//! # What would have to break to make it turn red
//!
//! Binding the representation or the sticks to something other than the fault
//! interpretation; forgetting a field of the fault, or the `IsListric` rule in one
//! direction or the other; not marking an invented object; lofting the sticks
//! in another order, moving a node of one, or keeping quiet about a stick out of order;
//! writing twice the stick shared by two patches; reusing a given uuid
//! for the sticks; creating a second frame for them; pulling the plane
//! upward, or without its offset; suggesting the title of an input; writing a
//! second time sticks already in the package.

use std::collections::BTreeSet;

use serde_json::json;

use energyml_build::operation::{Choices, Fitness, Identity, NewObject, Numbers, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When {
        creation: "2026-09-24T00:00:00Z".into(),
        originator: "test".into(),
    }
}

/// The frame Lutetia's boreholes are written in: a compound, in elevation.
fn frame() -> Ref {
    Ref::new(
        "cccccccc-0000-4000-8000-000000000003",
        "eml23.LocalEngineeringCompoundCrs",
        "Lutetia - boreholes (elevation)",
    )
}

/// What a recipe puts in the bag when a setting names the frame - its
/// companion, as `read::known_of_object` reads a system.
fn framed(up: bool) -> Pieces {
    Pieces::empty().with(
        "crs (object)",
        Source::Object(Known {
            reference: Some(frame()),
            crs: Some(frame()),
            vertical_uom: Some("m".into()),
            vertical_up: Some(up),
            ..Default::default()
        }),
    )
}

/// Where F1 crosses the drain of GTH-2, at −97 m.
const X0: f64 = 651_350.0;
const Y0: f64 = 6_861_145.0;

/// A point of the plane through (X0, Y0, −97) striking N30°E and dipping 65°
/// toward 120°, `along` metres along the strike, at elevation `z`.
fn on_f1(along: f64, z: f64) -> [f64; 3] {
    let strike = 30f64.to_radians();
    let down = 120f64.to_radians();
    let across = (-97.0 - z) / 65f64.to_radians().tan();
    [
        X0 + along * strike.sin() + across * down.sin(),
        Y0 + along * strike.cos() + across * down.cos(),
        z,
    ]
}

const LEVELS: [f64; 3] = [30.0, -60.0, -150.0];

fn along(k: usize) -> f64 {
    -30.0 + 100.0 * k as f64
}

/// A point as the text of a stick writes it - `{}` prints the shortest form
/// that reads back to the same bits.
fn written(point: [f64; 3]) -> String {
    format!("{} {} {}", point[0], point[1], point[2])
}

/// `count` sticks of F1, 100 m apart along the strike, three points each.
fn sticks(count: usize) -> Vec<String> {
    (0..count)
        .map(|k| {
            LEVELS
                .iter()
                .map(|&z| written(on_f1(along(k), z)))
                .collect::<Vec<_>>()
                .join("; ")
        })
        .collect()
}

/// What every sticks test says, without the throw.
fn base(given: Vec<String>) -> Choices {
    Choices::new()
        .set("crs", frame().dor())
        .set("title", "F1")
        .set("sticks", json!(given))
        .set("synthetic", true)
        .set(
            "source",
            "synthetic - the test's own fault, placed where Lutetia's F1 is",
        )
}

/// F1 as the reference-model description states it: normal, not listric, not sealed.
fn f1(given: Vec<String>) -> Choices {
    base(given)
        .set("throw", json!(["normal"]))
        .set("listric", false)
        .set("sealed", false)
        .set("maximumThrow", 15.0)
        .set("meanDip", 65.0)
        .set("meanAzimuth", 120.0)
}

fn planned(pieces: &Pieces, choices: &Choices) -> Result<Plan, String> {
    Registry::new()
        .plan(
            "fault-from-sticks",
            pieces,
            choices,
            &Identity::derived(),
            &when(),
        )
        .map_err(|why| why.message())
}

fn one<'a>(plan: &'a Plan, qualified_type: &str) -> &'a NewObject {
    let found: Vec<&NewObject> = plan
        .objects
        .iter()
        .filter(|o| o.qualified_type == qualified_type)
        .collect();
    assert_eq!(
        found.len(),
        1,
        "one {qualified_type}: {:?}",
        plan.objects.iter().map(|o| &o.qualified_type).collect::<Vec<_>>()
    );
    found[0]
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
    match recorded(plan, title).as_slice() {
        [Parameter::Double { value, .. }] => *value,
        other => panic!("`{title}` recorded as {other:?}"),
    }
}

/// A polyline piece - a stick drawn in the scene.
fn line(points: Vec<[f64; 3]>) -> Source {
    let runs = vec![points.len() as u32];
    Source::Shape {
        geometry: Geometry {
            kind: "test",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points,
                topology: Topology::Polylines(runs),
                closed: Vec::new(),
            }],
        },
        closed: None,
    }
}

#[test]
fn a_fault_is_a_feature_an_interpretation_a_surface_and_its_sticks() {
    let plan = planned(&framed(true), &f1(sticks(7))).expect("planned");
    assert_eq!(
        plan.objects.len(),
        4,
        "the frame is named: nothing but the fault is written"
    );
    let feature = one(&plan, "resqml22.BoundaryFeature");
    let interpretation = one(&plan, "resqml22.FaultInterpretation");
    let surface = one(&plan, "resqml22.TriangulatedSetRepresentation");
    let lines = one(&plan, "resqml22.PolylineSetRepresentation");
    // The interpretation interprets the feature.
    assert_eq!(
        interpretation.json["InterpretedFeature"]["Uuid"],
        json!(feature.uuid)
    );
    assert_eq!(
        interpretation.json["InterpretedFeature"]["QualifiedType"],
        json!("resqml22.BoundaryFeature")
    );
    // Both representations represent the fault's interpretation.
    for representation in [surface, lines] {
        assert_eq!(
            representation.json["RepresentedObject"]["Uuid"],
            json!(interpretation.uuid),
            "{}",
            representation.title
        );
        assert_eq!(
            representation.json["RepresentedObject"]["QualifiedType"],
            json!("resqml22.FaultInterpretation")
        );
    }
    // The sticks are interpretation lines, in the frame given.
    assert_eq!(lines.json["LineRole"], json!("interpretation line"));
    assert_eq!(
        lines.json["LinePatch"][0]["Geometry"]["LocalCrs"]["Uuid"],
        json!(frame().uuid)
    );
    assert_eq!(
        lines.json["LinePatch"][0]["NodeCountPerPolyline"]["Values"],
        json!([3, 3, 3, 3, 3, 3, 3])
    );
    assert_eq!(
        surface.json["TrianglePatch"][0]["Geometry"]["LocalCrs"]["Uuid"],
        json!(frame().uuid)
    );
    // Seven sticks of three: 21 nodes, six bands of four triangles.
    assert_eq!(surface.json["TrianglePatch"][0]["NodeCount"], json!(21));
    assert_eq!(number(&plan, "triangleCount"), 24.0);
    assert_eq!(number(&plan, "stickCount"), 7.0);
    assert_eq!(number(&plan, "writtenStickCount"), 7.0);
    assert_eq!(plan.activity.recipe, "fault-from-sticks");
}

#[test]
fn the_throw_and_what_the_reference_model_states_are_where_the_schema_puts_them() {
    let plan = planned(&framed(true), &f1(sticks(4))).expect("planned");
    let fault = &one(&plan, "resqml22.FaultInterpretation").json;
    assert_eq!(
        fault["ThrowInterpretation"][0]["Throw"],
        json!(["normal"]),
        "{fault}"
    );
    assert_eq!(fault["IsListric"], json!(false), "present, and false for F1");
    assert_eq!(fault["IsSealed"], json!(false));
    assert_eq!(fault["MaximumThrow"]["_"], json!(15.0));
    assert_eq!(fault["MaximumThrow"]["uom"], json!("m"));
    assert_eq!(fault["MeanDip"]["_"], json!(65.0));
    assert_eq!(fault["MeanDip"]["uom"], json!("dega"));
    assert_eq!(fault["MeanAzimuth"]["_"], json!(120.0));
    assert_eq!(
        fault["Domain"],
        json!("depth"),
        "an elevation in metres is the depth domain, not time"
    );
    assert!(
        fault.get("HasOccurredDuring").is_none(),
        "no bound given, no date: {fault}"
    );
    // Its uuid and title are the ones the pair was made with.
    assert_eq!(
        fault["uuid"],
        json!(one(&plan, "resqml22.FaultInterpretation").uuid)
    );
    assert_eq!(fault["Citation"]["Title"], json!("F1 - interpretation"));
}

#[test]
fn is_listric_is_required_on_a_normal_fault_and_refused_on_any_other() {
    // Normal, and not said whether listric: refused, naming the rule.
    let why = planned(&framed(true), &base(sticks(3)).set("throw", json!(["normal"])))
        .expect_err("IsListric missing");
    assert!(
        why.contains("Must be present if the fault is normal") && why.contains("l. 659-662"),
        "{why}"
    );
    // Reverse, and said: refused.
    let why = planned(
        &framed(true),
        &base(sticks(3))
            .set("throw", json!(["reverse"]))
            .set("listric", false),
    )
    .expect_err("reverse");
    assert!(
        why.contains("Must not be present") && why.contains("reverse"),
        "{why}"
    );
    // Two plays - F3's “normal et strike-slip”: not *the* normal fault.
    let why = planned(
        &framed(true),
        &base(sticks(3))
            .set("throw", json!(["normal", "strike-slip"]))
            .set("listric", false),
    )
    .expect_err("two plays");
    assert!(why.contains("normal and strike-slip"), "{why}");
    // No throw stated: its presence would say the fault is normal.
    let why = planned(&framed(true), &base(sticks(3)).set("listric", true)).expect_err("no throw");
    assert!(why.contains("not stated"), "{why}");

    // And the lawful cases write what they were told, and nothing more.
    let reverse = planned(
        &framed(true),
        &base(sticks(3))
            .set("throw", json!(["reverse"]))
            .set("sealed", true),
    )
    .expect("F4");
    let fault = &one(&reverse, "resqml22.FaultInterpretation").json;
    assert!(fault.get("IsListric").is_none(), "{fault}");
    assert_eq!(fault["IsSealed"], json!(true));
    let two = planned(
        &framed(true),
        &base(sticks(3)).set("throw", json!(["normal", "strike-slip"])),
    )
    .expect("F3");
    let fault = &one(&two, "resqml22.FaultInterpretation").json;
    assert_eq!(
        fault["ThrowInterpretation"].as_array().map(Vec::len),
        Some(1),
        "one throw naming both plays: {fault}"
    );
    assert_eq!(
        fault["ThrowInterpretation"][0]["Throw"],
        json!(["normal", "strike-slip"])
    );
    let listric = planned(
        &framed(true),
        &base(sticks(3))
            .set("throw", json!(["normal"]))
            .set("listric", true),
    )
    .expect("F2");
    assert_eq!(
        one(&listric, "resqml22.FaultInterpretation").json["IsListric"],
        json!(true)
    );
}

#[test]
fn a_synthetic_fault_says_so_on_every_object_and_names_its_source() {
    let plan = planned(&framed(true), &f1(sticks(4))).expect("planned");
    for object in &plan.objects {
        let marks = &object.json["ExtensionNameValue"];
        assert_eq!(
            marks[0]["Name"],
            json!("synthetic"),
            "{}: {marks}",
            object.qualified_type
        );
        assert_eq!(marks[0]["Value"]["_"], json!("true"), "{}", object.qualified_type);
        let description = object.json["Citation"]["Description"]
            .as_str()
            .unwrap_or_default();
        assert!(
            description.contains("the test's own fault") && description.starts_with("Synthetic"),
            "{}: {description}",
            object.qualified_type
        );
    }
    assert!(
        plan.says.iter().any(|note| note.key == "synthetic"),
        "{:?}",
        plan.says
    );

    // Invented, and no source: refused on `source`.
    let silent = Choices::new()
        .set("crs", frame().dor())
        .set("title", "F1")
        .set("sticks", json!(sticks(3)))
        .set("synthetic", true)
        .set("throw", json!(["reverse"]));
    let why = Registry::new()
        .plan(
            "fault-from-sticks",
            &framed(true),
            &silent,
            &Identity::derived(),
            &when(),
        )
        .expect_err("no source");
    assert_eq!(why.refusals()[0].slot.as_deref(), Some("source"), "{why}");

    // Not invented: no mark.
    let real = planned(&framed(true), &f1(sticks(3)).set("synthetic", false)).expect("planned");
    assert!(
        real.objects
            .iter()
            .all(|object| object.json.get("ExtensionNameValue").is_none()),
        "a fault stated as real is not marked"
    );
}

/// The loft keeps every node, in the order of the sticks: read on the shape
/// the plan carries **and** on the array it writes - two readings of one
/// fact, at map coordinates.
#[test]
fn the_surface_is_the_sticks_in_their_order_bit_for_bit() {
    let plan = planned(&framed(true), &f1(sticks(5))).expect("planned");
    let surface = one(&plan, "resqml22.TriangulatedSetRepresentation");
    let made = plan
        .shapes
        .iter()
        .find(|shape| shape.object.as_deref() == Some(surface.uuid.as_str()))
        .expect("the surface's shape");
    let expected: Vec<[f64; 3]> = (0..5)
        .flat_map(|k| LEVELS.iter().map(move |&z| on_f1(along(k), z)))
        .collect();
    let patch = &made.geometry.patches[0];
    assert_eq!(patch.points.len(), expected.len());
    for (at, (got, want)) in patch.points.iter().zip(&expected).enumerate() {
        assert!(
            got.iter().zip(want).all(|(a, b)| a.to_bits() == b.to_bits()),
            "node {at}: {got:?} for {want:?}"
        );
    }
    let Numbers::F64(written) = &plan.datasets[0].values else {
        panic!("points are floating point")
    };
    let flat: Vec<f64> = expected.iter().flatten().copied().collect();
    assert_eq!(written.len(), flat.len());
    assert!(
        written.iter().zip(&flat).all(|(a, b)| a.to_bits() == b.to_bits()),
        "the array written is the shape"
    );
    // Every triangle joins two neighbouring sticks, never a stick with the
    // one after next.
    let Topology::Triangles(triangles) = &patch.topology else {
        panic!("triangles")
    };
    for triangle in triangles {
        let sticks: BTreeSet<u32> = triangle.iter().map(|node| node / 3).collect();
        let (low, high) = (*sticks.first().unwrap(), *sticks.last().unwrap());
        assert!(
            sticks.len() == 2 && high - low == 1,
            "{triangle:?} joins sticks {sticks:?}"
        );
    }
    // The stated dip and the lofted one agree: the sticks were drawn on it.
    assert!(
        (number(&plan, "measuredDip") - 65.0).abs() < 1e-6,
        "{}",
        number(&plan, "measuredDip")
    );
    assert!(
        (number(&plan, "measuredDipAzimuth") - 120.0).abs() < 1e-6,
        "{}",
        number(&plan, "measuredDipAzimuth")
    );
}

#[test]
fn a_blank_entry_starts_a_second_patch_and_the_stick_they_share_is_written_once() {
    let all = sticks(7);
    let mut given: Vec<String> = all[..4].to_vec();
    given.push(String::new());
    given.extend(all[3..].iter().cloned());
    let plan = planned(&framed(true), &f1(given)).expect("planned");
    let surface = one(&plan, "resqml22.TriangulatedSetRepresentation");
    assert_eq!(
        surface.json["TrianglePatch"].as_array().map(Vec::len),
        Some(2),
        "F1 “en deux patches”"
    );
    let lines = one(&plan, "resqml22.PolylineSetRepresentation");
    assert_eq!(
        lines.json["LinePatch"][0]["NodeCountPerPolyline"]["Values"]
            .as_array()
            .map(Vec::len),
        Some(7)
    );
    assert_eq!(number(&plan, "stickCount"), 8.0);
    assert_eq!(number(&plan, "writtenStickCount"), 7.0);
    assert!(
        plan.says.iter().any(|note| note.key == "shared-stick"),
        "{:?}",
        plan.says
    );
    assert!(
        plan.says.iter().any(|note| note.key == "patches"),
        "{:?}",
        plan.says
    );

    // A blank that ends nothing, or ends the list, is refused.
    let mut leading = vec![String::new()];
    leading.extend(sticks(3));
    assert!(
        planned(&framed(true), &f1(leading))
            .expect_err("leading blank")
            .contains("sticks[0]")
    );
    let mut trailing = sticks(3);
    trailing.push(" ".to_string());
    assert!(
        planned(&framed(true), &f1(trailing))
            .expect_err("trailing blank")
            .contains("last entry is blank")
    );
}

#[test]
fn a_stick_out_of_order_is_said_and_kept_where_it_was_given() {
    let mut given = sticks(5);
    given.swap(2, 3);
    let plan = planned(&framed(true), &f1(given)).expect("planned - said, not refused");
    let note = plan
        .says
        .iter()
        .find(|note| note.key == "stick-order")
        .unwrap_or_else(|| panic!("said: {:?}", plan.says));
    assert!(
        note.detail.contains("`stick 4`") && note.detail.contains("`stick 3`"),
        "{}",
        note.detail
    );
    let in_order = planned(&framed(true), &f1(sticks(5))).expect("planned");
    assert!(
        !in_order.says.iter().any(|note| note.key == "stick-order"),
        "{:?}",
        in_order.says
    );
}

#[test]
fn sticks_given_twice_or_not_at_all_are_refused() {
    let drawn = Pieces::empty()
        .with("lines", line(vec![on_f1(0.0, 30.0), on_f1(0.0, -150.0)]))
        .with("lines 2", line(vec![on_f1(100.0, 30.0), on_f1(100.0, -150.0)]));
    let mut both = framed(true);
    for (name, source) in drawn.iter() {
        both = both.with(name, source.clone());
    }
    let why = planned(&both, &f1(sticks(3))).expect_err("twice");
    assert!(why.contains("given twice"), "{why}");
    let why = planned(&framed(true), &f1(Vec::new())).expect_err("none");
    assert!(why.contains("no stick is given"), "{why}");
    // And the lines alone are sticks.
    let plan = planned(&both, &f1(Vec::new())).expect("planned from lines");
    assert_eq!(number(&plan, "stickCount"), 2.0);
}

#[test]
fn sticks_already_in_the_package_are_not_written_a_second_time() {
    let from_package = |uuid: &str| {
        Source::Object(Known {
            reference: Some(Ref::new(uuid, "resqml22.PolylineSetRepresentation", "Stick read")),
            crs: Some(frame()),
            vertical_uom: Some("m".into()),
            vertical_up: Some(true),
            ..Default::default()
        })
    };
    let bag = framed(true)
        .with("lines", line(vec![on_f1(0.0, 30.0), on_f1(0.0, -150.0)]))
        .with(
            "lines (object)",
            from_package("aaaaaaaa-0000-4000-8000-000000000001"),
        )
        .with("lines 2", line(vec![on_f1(100.0, 30.0), on_f1(100.0, -150.0)]))
        .with(
            "lines 2 (object)",
            from_package("aaaaaaaa-0000-4000-8000-000000000002"),
        );
    let plan = planned(&bag, &f1(Vec::new())).expect("planned");
    assert!(
        !plan
            .objects
            .iter()
            .any(|o| o.qualified_type == "resqml22.PolylineSetRepresentation"),
        "no copy of what is there"
    );
    assert!(
        plan.says.iter().any(|note| note.key == "sticks-in-package"),
        "{:?}",
        plan.says
    );
    assert_eq!(number(&plan, "writtenStickCount"), 0.0);
}

/// F4 of the reference model - reverse, sealed, a plane dipping 55° toward 222° - and F3,
/// vertical: the sticks drawn go **down** the dip by the extent over its
/// tangent, and are not written.
#[test]
fn a_plane_draws_its_sticks_down_the_dip_and_writes_none_of_them() {
    let start = [651_520.0, 6_861_450.0, 30.0];
    let strike = 132f64.to_radians();
    let trace: Vec<[f64; 3]> = (0..3)
        .map(|k| {
            [
                start[0] + 100.0 * k as f64 * strike.sin(),
                start[1] + 100.0 * k as f64 * strike.cos(),
                30.0,
            ]
        })
        .collect();
    let text = trace.iter().map(|p| written(*p)).collect::<Vec<_>>().join("; ");
    let choices = Choices::new()
        .set("crs", frame().dor())
        .set("title", "F4")
        .set("geometry", "plane")
        .set("trace", text.as_str())
        .set("dip", 55.0)
        .set("dipAzimuth", 222.0)
        .set("verticalExtent", 180.0)
        .set("throw", json!(["reverse"]))
        .set("sealed", true);
    let plan = planned(&framed(true), &choices).expect("planned");
    assert!(
        !plan
            .objects
            .iter()
            .any(|o| o.qualified_type == "resqml22.PolylineSetRepresentation"),
        "computed sticks are not written"
    );
    let surface = one(&plan, "resqml22.TriangulatedSetRepresentation");
    let made = plan
        .shapes
        .iter()
        .find(|shape| shape.object.as_deref() == Some(surface.uuid.as_str()))
        .expect("its shape");
    let points = &made.geometry.patches[0].points;
    let across = 180.0 / 55f64.to_radians().tan();
    for (k, top) in trace.iter().enumerate() {
        let bottom = points[2 * k + 1];
        assert_eq!(points[2 * k], *top);
        assert!(
            (bottom[0] - (top[0] + across * 222f64.to_radians().sin())).abs() < 1e-6,
            "{bottom:?}"
        );
        assert!(
            (bottom[1] - (top[1] + across * 222f64.to_radians().cos())).abs() < 1e-6,
            "{bottom:?}"
        );
        assert_eq!(bottom[2], -150.0, "180 m below 30 m, in elevation");
    }
    assert!((number(&plan, "measuredDip") - 55.0).abs() < 1e-6);
    assert!((number(&plan, "measuredDipAzimuth") - 222.0).abs() < 1e-6);

    // F3, vertical: straight down, no offset at all.
    let vertical = choices
        .clone()
        .set("title", "F3")
        .set("dip", 90.0)
        .set("dipAzimuth", 42.0)
        .set("throw", json!(["normal", "strike-slip"]));
    let plan = planned(&framed(true), &vertical).expect("planned");
    let surface = one(&plan, "resqml22.TriangulatedSetRepresentation");
    let made = plan
        .shapes
        .iter()
        .find(|shape| shape.object.as_deref() == Some(surface.uuid.as_str()))
        .expect("its shape");
    let points = &made.geometry.patches[0].points;
    for k in 0..trace.len() {
        assert_eq!(
            [points[2 * k + 1][0], points[2 * k + 1][1]],
            [points[2 * k][0], points[2 * k][1]],
            "stick {k}"
        );
    }
}

#[test]
fn a_plane_goes_down_in_the_frame_s_own_sense_and_is_refused_where_the_frame_does_not_say() {
    let text = [[651_000.0, 6_861_500.0, 10.0], [651_100.0, 6_861_500.0, 10.0]]
        .iter()
        .map(|p| written(*p))
        .collect::<Vec<_>>()
        .join("; ");
    let choices = Choices::new()
        .set("crs", frame().dor())
        .set("title", "Fd")
        .set("geometry", "plane")
        .set("trace", text.as_str())
        .set("dip", 60.0)
        .set("dipAzimuth", 180.0)
        .set("verticalExtent", 100.0);
    // A depth frame: down is a larger z.
    let plan = planned(&framed(false), &choices).expect("planned");
    let surface = one(&plan, "resqml22.TriangulatedSetRepresentation");
    let made = plan
        .shapes
        .iter()
        .find(|shape| shape.object.as_deref() == Some(surface.uuid.as_str()))
        .expect("its shape");
    assert_eq!(made.geometry.patches[0].points[1][2], 110.0);
    // A frame nobody said anything of: refused on `crs`, never guessed.
    let why = Registry::new()
        .plan(
            "fault-from-sticks",
            &Pieces::empty(),
            &choices,
            &Identity::derived(),
            &when(),
        )
        .expect_err("which way is down");
    assert_eq!(why.refusals()[0].slot.as_deref(), Some("crs"), "{why}");
}

#[test]
fn a_frame_created_here_is_the_one_the_sticks_name_and_nothing_is_reprojected() {
    let created = |choices: Choices| choices.set("sourceEpsg", 2154);
    let own = Choices::new()
        .set("title", "F1")
        .set("sticks", json!(sticks(3)))
        .set("throw", json!(["normal"]))
        .set("listric", false);
    let plan = planned(&Pieces::empty(), &created(own.clone())).expect("planned");
    let frames: Vec<&NewObject> = plan
        .objects
        .iter()
        .filter(|o| o.qualified_type.contains("Crs"))
        .collect();
    assert_eq!(
        frames.len(),
        4,
        "one frame, made once: {:?}",
        frames.iter().map(|o| &o.qualified_type).collect::<Vec<_>>()
    );
    let compound = plan.of_role("crs").next().expect("the compound");
    let lines = one(&plan, "resqml22.PolylineSetRepresentation");
    assert_eq!(
        lines.json["LinePatch"][0]["Geometry"]["LocalCrs"]["Uuid"],
        json!(compound.uuid)
    );
    let surface = one(&plan, "resqml22.TriangulatedSetRepresentation");
    assert_eq!(
        surface.json["TrianglePatch"][0]["Geometry"]["LocalCrs"]["Uuid"],
        json!(compound.uuid)
    );
    // Degrees to Lambert-93 would move every node: refused on `project`.
    let why = Registry::new()
        .plan(
            "fault-from-sticks",
            &Pieces::empty(),
            &own.set("sourceEpsg", 4326).set("project", 2154),
            &Identity::derived(),
            &when(),
        )
        .expect_err("no reprojection");
    assert_eq!(why.refusals()[0].slot.as_deref(), Some("project"), "{why}");
}

#[test]
fn the_same_title_is_the_same_fault_and_another_title_another() {
    let uuids = |plan: &Plan| {
        plan.objects
            .iter()
            .map(|o| (o.qualified_type.clone(), o.uuid.clone()))
            .collect::<Vec<_>>()
    };
    let first = planned(&framed(true), &f1(sticks(4))).expect("planned");
    // A stick corrected, one added: the same fault, rewritten.
    let corrected = planned(&framed(true), &f1(sticks(5))).expect("planned");
    assert_eq!(uuids(&first), uuids(&corrected));
    let other = planned(&framed(true), &f1(sticks(4)).set("title", "F2")).expect("planned");
    for ((kind, a), (_, b)) in uuids(&first).iter().zip(uuids(&other).iter()) {
        assert_ne!(a, b, "{kind}");
    }
}

/// Each writer counts its uuids from the start of the list it is given; the
/// sticks' writer is given what the surface's left.
#[test]
fn given_uuids_name_each_object_once() {
    let given: Vec<String> = (0..4)
        .map(|n| format!("00000000-0000-4000-8000-{n:012}"))
        .collect();
    let plan = Registry::new()
        .plan(
            "fault-from-sticks",
            &framed(true),
            &f1(sticks(3)),
            &Identity::Given(given.clone()),
            &when(),
        )
        .expect("planned");
    let used: BTreeSet<&str> = plan.objects.iter().map(|o| o.uuid.as_str()).collect();
    assert_eq!(
        used.len(),
        plan.objects.len(),
        "two objects under one uuid: {:?}",
        plan.objects
            .iter()
            .map(|o| (&o.qualified_type, &o.uuid))
            .collect::<Vec<_>>()
    );
    assert!(used.iter().all(|uuid| given.iter().any(|one| one == uuid)));
}

#[test]
fn the_dating_takes_both_bounds_or_none() {
    let horizon = |uuid: &str, title: &str| {
        Source::Object(Known {
            reference: Some(Ref::new(uuid, "resqml22.HorizonInterpretation", title)),
            ..Default::default()
        })
    };
    let lower = "bbbbbbbb-0000-4000-8000-000000000133";
    let upper = "bbbbbbbb-0000-4000-8000-000000000131";
    let half = framed(true).with("chronoBottom", horizon(lower, "H3 - interpretation"));
    let why = Registry::new()
        .plan(
            "fault-from-sticks",
            &half,
            &f1(sticks(3)),
            &Identity::derived(),
            &when(),
        )
        .expect_err("one bound");
    assert_eq!(why.refusals()[0].slot.as_deref(), Some("chronoTop"), "{why}");
    let both = half.with("chronoTop", horizon(upper, "H1 - interpretation"));
    let plan = planned(&both, &f1(sticks(3))).expect("planned");
    let dated = &one(&plan, "resqml22.FaultInterpretation").json["HasOccurredDuring"];
    assert_eq!(
        dated["$type"],
        json!("resqml22.GeneticBoundaryBasedTimeInterval"),
        "{dated}"
    );
    assert_eq!(dated["ChronoBottom"]["Uuid"], json!(lower));
    assert_eq!(dated["ChronoTop"]["Uuid"], json!(upper));
    assert!(!plan.says.iter().any(|note| note.key == "not-dated"));
}

/// Neither the title of an input nor its interpretation is offered to the
/// fault: the defect `well-from-plan` had with the ground (24/09).
#[test]
fn nothing_of_an_input_names_the_fault() {
    let bag = Pieces::empty()
        .with("lines", line(vec![on_f1(0.0, 30.0), on_f1(0.0, -150.0)]))
        .with(
            "lines (object)",
            Source::Object(Known {
                reference: Some(Ref::new(
                    "aaaaaaaa-0000-4000-8000-000000000009",
                    "resqml22.PolylineSetRepresentation",
                    "Ground traces",
                )),
                interpretation: Some(Ref::new(
                    "aaaaaaaa-0000-4000-8000-00000000000a",
                    "resqml22.BoundaryFeatureInterpretation",
                    "the ground",
                )),
                crs: Some(frame()),
                ..Default::default()
            }),
        );
    let registry = Registry::new();
    let operation = registry.get("fault-from-sticks").expect("registered");
    assert_eq!(operation.fitness(&bag), Fitness::Fit);
    let slots = operation.needs(&bag);
    let title = slots
        .iter()
        .find(|slot| slot.name == "title")
        .expect("a title slot");
    assert_eq!(title.suggested, None, "the fault is not named after its input");
    assert!(
        !slots.iter().any(|slot| slot.name == "interpretation"),
        "the fault makes its own interpretation"
    );
    let crs = slots.iter().find(|slot| slot.name == "crs").expect("a crs slot");
    assert_eq!(
        crs.suggested.as_ref().map(|v| v["Uuid"].clone()),
        Some(json!(frame().uuid)),
        "the frame of the input is"
    );
}
