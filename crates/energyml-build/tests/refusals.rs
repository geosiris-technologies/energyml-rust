//! Refusals travel **structured** — a key, the slot, the message — from
//! `settle` and from `fitness` to the caller (ADR 0017).
//! §5.7 and §7).
//!
//! `settle` finds every wrong field of a form at once; joined into one
//! sentence, a front end could only show them in one place. Each is now a
//! `Refusal`, and the front end puts it under its field.
//!
//! # What would have to break to make it turn red
//!
//! Making `settle` return a refusal without its slot, or all under a single key;
//! stopping at the first faulty field; letting `fitness` lose the input
//! slot that the binder had named; making `message()` say anything other
//! than the sequence of refusals.

use energyml_build::operation::{Choices, Fitness, Identity, Need, Purpose, Refusal, Registry, Slot, When};
use energyml_build::pieces::{Pieces, Source};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn cloud() -> Pieces {
    let points: Vec<[f64; 3]> = (0..3)
        .flat_map(|j| (0..3).map(move |i| [i as f64 * 10.0, j as f64 * 10.0, 5.0 + i as f64]))
        .collect();
    Pieces::empty().with(
        "shape",
        Source::Shape {
            geometry: Geometry {
                kind: "cloud",
                patches: vec![Patch { index: 0, node_offset: 0, points, topology: Topology::Points, closed: Vec::new() }],
            },
            closed: None,
        },
    )
}

fn pairs(refusals: &[Refusal]) -> Vec<(&str, Option<&str>)> {
    refusals.iter().map(|one| (one.key, one.slot.as_deref())).collect()
}

/// **Every** wrong field, each under its own slot and key — in the order the
/// form declares them — and the sentence is exactly their messages.
#[test]
fn each_faulty_field_files_its_refusal_under_its_slot() {
    let slots = Registry::new().get("interpolate").unwrap().needs(&cloud());
    let why = Choices::new()
        .set("method", "kriging")
        .set("model", "spherical")
        .set("spacing", "wide")
        .set("kernel", "linear")
        .settle_for(&slots, Purpose::Geometry)
        .unwrap_err();
    assert_eq!(
        pairs(why.refusals()),
        [
            ("value", Some("spacing")),
            ("required", Some("nugget")),
            ("required", Some("sill")),
            ("required", Some("range")),
            ("inapplicable", Some("kernel")),
        ]
    );
    for one in why.refusals() {
        let slot = one.slot.as_deref().unwrap();
        assert!(one.message.contains(&format!("`{slot}`")), "the message names its slot: {one:?}");
    }
    let sentence = why.refusals().iter().map(|one| one.message.clone()).collect::<Vec<_>>().join("; ");
    assert_eq!(why.message(), sentence);
    assert_eq!(why.to_string(), sentence, "the CLI prints the same sentence");
}

/// A list answer: the shape, the count, and **each** wrong element under the
/// list's slot.
#[test]
fn a_list_is_refused_by_shape_by_count_then_element_by_element() {
    let slots = vec![Slot::many("codes", Need::Epsg, "the codes", 2)];
    let settle = |value: serde_json::Value| Choices::new().set("codes", value).settle(&slots).unwrap_err();

    assert_eq!(pairs(settle(serde_json::json!(2154)).refusals()), [("shape", Some("codes"))]);
    assert_eq!(pairs(settle(serde_json::json!([2154])).refusals()), [("count", Some("codes"))]);
    let why = settle(serde_json::json!(["a", 2154, "b"]));
    assert_eq!(pairs(why.refusals()), [("value", Some("codes")), ("value", Some("codes"))]);
    assert!(why.refusals()[0].message.contains("`codes`[0]") && why.refusals()[1].message.contains("`codes`[2]"), "{why}");
}

/// The binder's refusal keeps the **input** slot it is about through
/// `fitness`, and through the registry's refusal to plan.
#[test]
fn an_unfit_bag_names_its_input_slot() {
    let alone = Pieces::empty().with("top", cloud().get("shape").unwrap().clone());
    let Fitness::Unfit(why) = Registry::new().get("thickness").unwrap().fitness(&alone) else {
        panic!("a thickness needs two surfaces")
    };
    assert_eq!((why.key, why.slot.as_deref()), ("unfit", Some("base")), "{why:?}");

    let when = When { creation: "2026-09-12T00:00:00Z".into(), originator: "test".into() };
    let refused = Registry::new().plan("thickness", &alone, &Choices::new(), &Identity::derived(), &when).unwrap_err();
    assert_eq!(pairs(refused.refusals()), [("unfit", Some("base"))]);
    assert!(refused.message().contains("`thickness` is not fit"), "{refused}");
}

/// A refusal raised **inside** `plan` — a spacing that would make too many
/// nodes — reaches the caller under its slot too: the operation says which
/// field, and the registry keeps it.
#[test]
fn a_refusal_of_the_computation_keeps_its_field() {
    let when = When { creation: "2026-09-12T00:00:00Z".into(), originator: "test".into() };
    let choices = Choices::new().set("spacing", 1e-4).set("title", "t");
    let why = Registry::new()
        .plan_for("interpolate", &cloud(), &choices, &Identity::derived(), &when, Purpose::Geometry)
        .unwrap_err();
    assert_eq!(pairs(why.refusals()), [("value", Some("spacing"))], "{why}");
    assert!(why.message().contains("widen the spacing"), "{why}");
}
