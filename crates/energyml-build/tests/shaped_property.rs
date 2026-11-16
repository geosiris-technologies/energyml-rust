//! `property::write_shaped`: a property whose elements hold several values.
//!
//! The form of the assertions comes from the standard, never from what the
//! code computes: RESQML Technical Reference Guide v2.2 section 7 p. 221 (the
//! note of the figure: the dataset is `[elements, d1, d2, ...]`, time the
//! slowest dimension when there is one) and section 7.1 p. 222
//! (`ValueCountPerIndexableElement`, `LabelPerComponent`).
//!
//! What would make them fail: the shape written as `[1]` and the values flat,
//! the nodes counted against the values instead of the elements, a number of
//! values that is no whole number of elements truncated, the labels dropped or
//! written empty, a scalar given a trailing dimension, the time put last.

use energyml_build::declare::{Field, ObjectRef, PropertyKindRef, Title};
use energyml_build::operation::{Context, Identity, Plan, Purpose, When};
use energyml_build::ops::property::{PropertyParams, Timing, write_shaped, write_timed};
use energyml_build::pieces::{Known, Pieces};
use energyml_domain::property::shape::ElementShape;
use serde_json::{Value, json};

fn when() -> When {
    When {
        creation: "2026-11-15T00:00:00Z".into(),
        originator: "tests/shaped_property.rs".into(),
    }
}

fn params() -> PropertyParams {
    PropertyParams {
        representation: ObjectRef(json!({
            "$type": "eml23.DataObjectReference",
            "Uuid": "aaaaaaaa-0000-4000-8000-000000000001",
            "QualifiedType": "resqml22.WellboreFrameRepresentation",
            "Title": "the frame",
        })),
        property_kind: PropertyKindRef(json!({
            "$type": "eml23.DataObjectReference",
            "Uuid": "bbbbbbbb-0000-4000-8000-000000000001",
            "QualifiedType": "eml23.PropertyKind",
            "Title": "resistivity",
        })),
        uom: "ohm.m".into(),
        title: Title("cube".into()),
        data_file: "data.h5".into(),
    }
}

fn field(values: Vec<f64>) -> Field {
    Field {
        name: "values".into(),
        over: "frame".into(),
        on: "nodes".into(),
        values,
        uom: Some("ohm.m".into()),
    }
}

fn known(nodes: usize) -> Known {
    Known {
        node_count: Some(nodes),
        ..Default::default()
    }
}

/// The values of `0` to `n - 1`, so that an order that moved shows.
fn ramp(n: usize) -> Vec<f64> {
    (0..n).map(|i| i as f64).collect()
}

fn shaped(
    values: Vec<f64>,
    nodes: usize,
    timing: &Timing,
    shape: &ElementShape,
) -> Result<Plan, energyml_build::operation::Refusal> {
    let (identity, when, bag) = (Identity::derived(), when(), Pieces::empty());
    let cx = Context {
        identity: &identity,
        when: &when,
        purpose: Purpose::Writing,
        feedback: &energyml_build::typed::Silent,
        bag: &bag,
    };
    write_shaped(
        &field(values),
        Some(&known(nodes)),
        &params(),
        "seed",
        &cx,
        timing,
        shape,
    )
}

fn counts(plan: &Plan) -> Value {
    plan.objects[0].json["ValueCountPerIndexableElement"].clone()
}

#[test]
fn a_matrix_per_node_is_written_with_its_shape() {
    let shape = ElementShape::of(&[2, 2], &[]).unwrap();
    let plan = shaped(ramp(8), 2, &Timing::Timeless, &shape).expect("planned");
    // TRG 2.2 section 7.1 p. 222: the count per element is the shape.
    assert_eq!(counts(&plan), json!([2, 2]));
    // Section 7 p. 221: `[elements, d1, d2]`, the values in the order given.
    assert_eq!(plan.datasets[0].dimensions, vec![2, 2, 2]);
    match &plan.datasets[0].values {
        energyml_build::operation::Numbers::F64(values) => assert_eq!(values, &ramp(8)),
        other => panic!("{other:?}"),
    }
    assert!(plan.says.iter().any(|note| note.key == "components"));
}

#[test]
fn the_nodes_are_counted_in_whole_elements() {
    let shape = ElementShape::of(&[2, 2], &[]).unwrap();
    // 8 values are 2 elements of 4: the representation holds 3 nodes.
    let why = shaped(ramp(8), 3, &Timing::Timeless, &shape).expect_err("refused");
    let said = why.to_string();
    assert!(said.contains("`field`"), "{said}");
    assert!(
        said.contains('3') && said.contains('2'),
        "it names both counts: {said}"
    );
}

#[test]
fn values_that_do_not_make_whole_elements_are_refused() {
    let shape = ElementShape::of(&[2, 2], &[]).unwrap();
    let why = shaped(ramp(7), 2, &Timing::Timeless, &shape).expect_err("refused");
    assert!(why.to_string().contains("`field`"), "{why}");
}

#[test]
fn labels_are_written_when_given() {
    let labelled = ElementShape::of(&[2], &["Rh".to_string(), "Rv".to_string()]).unwrap();
    let plan = shaped(ramp(4), 2, &Timing::Timeless, &labelled).expect("planned");
    assert_eq!(plan.objects[0].json["LabelPerComponent"], json!(["Rh", "Rv"]));
    // Without labels the field is absent, not an empty list.
    let plain = ElementShape::of(&[2], &[]).unwrap();
    let plan = shaped(ramp(4), 2, &Timing::Timeless, &plain).expect("planned");
    assert!(plan.objects[0].json.get("LabelPerComponent").is_none());
}

#[test]
fn a_scalar_is_written_as_it_always_was() {
    let (identity, when, bag) = (Identity::derived(), when(), Pieces::empty());
    let cx = Context {
        identity: &identity,
        when: &when,
        purpose: Purpose::Writing,
        feedback: &energyml_build::typed::Silent,
        bag: &bag,
    };
    let plan = write_timed(
        &field(ramp(5)),
        Some(&known(5)),
        &params(),
        "seed",
        &cx,
        &Timing::Timeless,
    )
    .expect("planned");
    assert_eq!(counts(&plan), json!([1]));
    assert_eq!(plan.datasets[0].dimensions, vec![5], "a flat array, not [5, 1]");
    assert!(plan.objects[0].json.get("LabelPerComponent").is_none());
}

#[test]
fn a_series_of_matrices_keeps_time_slowest() {
    let shape = ElementShape::of(&[3], &[]).unwrap();
    let series = Timing::Series {
        series: json!({
            "$type": "eml23.DataObjectReference",
            "Uuid": "cccccccc-0000-4000-8000-000000000001",
            "QualifiedType": "eml23.TimeSeries",
            "Title": "times",
        }),
        use_interval: false,
        steps: 2,
    };
    let plan = shaped(ramp(12), 2, &series, &shape).expect("planned");
    // 2 steps, 2 nodes, 3 values each: `[steps, elements, d1]`.
    assert_eq!(plan.datasets[0].dimensions, vec![2, 2, 3]);
    assert_eq!(counts(&plan), json!([3]));
}
