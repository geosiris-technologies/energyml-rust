//! The `source` text of a step says where invented values come from; it is not
//! what the object is, so it never enters a derived uuid.
//!
//! Two runs with the same inputs and two different `source` texts must write
//! the same objects under the same uuids (and the same activity seed), while the
//! `Citation/Description` they write differs. Rewording a citation — or
//! translating it — must not rename anything: it did once, when the
//! `extend-boreholes` seed carried the text (13/10/2026, translating the
//! `source = "…"` of the reference recipe made the extended frames vanish
//! under their old uuids).
//!
//! Rule: ADR-0019 (synthetic data is flagged and sourced) says the source is
//! written in the object; the identity rules of `energyml-build` say the uuid
//! comes from what the object is.
//!
//! # What would have to break to make each test turn red
//!
//! Put `params.source` back into the `format!` that builds `seed` in
//! `ops/extend_boreholes.rs` (`extend_one`) and
//! `extend_boreholes_keeps_its_uuids_when_the_source_changes` turns red.
//! Add `self.source` to the format of `Writer::uuid` or of
//! `Writer::activity_seed` in `ops/prodml_writer.rs` and
//! `a_prodml_writer_keeps_its_uuids_when_the_source_changes` turns red.
//! Both also turn red if the description stopped carrying the source — the
//! `differs` assertions are what keep them from passing on an empty comparison.

use serde_json::{json, Value};

use energyml_build::operation::{Choices, Identity, NewObject, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When { creation: "2026-10-13T00:00:00Z".into(), originator: "test".into() }
}

/// Every object of a plan as `(role, uuid)`, in the plan's order, then the
/// activity seed: everything an identity is made of.
fn identities(plan: &Plan) -> Vec<String> {
    let mut all: Vec<String> = plan.objects.iter().map(|one| format!("{}={}", one.role, one.uuid)).collect();
    all.push(format!("activity-seed={}", plan.activity.seed));
    all
}

/// The `Citation/Description` of every object that has one.
fn descriptions(plan: &Plan) -> Vec<String> {
    plan.objects.iter().filter_map(|one: &NewObject| one.json["Citation"]["Description"].as_str().map(str::to_string)).collect()
}

/// Same uuids, different words — the two halves of the rule, each guarding
/// the other against an empty comparison.
fn assert_same_identity_other_words(first: &Plan, second: &Plan, what: &str) {
    assert!(!first.objects.is_empty(),"{what}: the plan writes objects");
    assert_eq!(identities(first), identities(second), "{what}: the source text must not take part in any uuid or seed");
    let (a, b) = (descriptions(first), descriptions(second));
    assert!(!a.is_empty(), "{what}: the objects carry a description");
    assert_ne!(a, b, "{what}: the written descriptions must differ, or this test compares nothing");
}

// -- a geometric operation: extend-boreholes ---------------------------------

const X: f64 = 651_000.0;
const Y: f64 = 6_861_000.0;
const GROUND: f64 = 40.0;

fn vertical_frame(title: &str, mds: &[f64]) -> Pieces {
    let points: Vec<[f64; 3]> = mds.iter().map(|md| [X, Y, GROUND - md]).collect();
    let n = points.len() as u32;
    let geometry = Geometry {
        kind: "resqml22.WellboreFrameRepresentation",
        patches: vec![Patch { index: 0, node_offset: 0, points, topology: Topology::Polylines(vec![n]), closed: Vec::new() }],
    };
    let known = Known {
        reference: Some(Ref::new("aaaaaaaa-0000-4000-8000-000000000001", "resqml22.WellboreFrameRepresentation", format!("{title} — frame"))),
        crs: Some(Ref::new("eeeeeeee-0000-4000-8000-000000000005", "eml23.LocalEngineeringCompoundCrs", "The frame")),
        vertical_uom: Some("m".to_string()),
        vertical_up: Some(true),
        node_mds: Some(mds.to_vec()),
        trajectory: Some(Ref::new("bbbbbbbb-0000-4000-8000-000000000002", "resqml22.WellboreTrajectoryRepresentation", format!("{title} — trajectory"))),
        datum: Some(Ref::new("cccccccc-0000-4000-8000-000000000003", "eml23.ReferencePointInALocalEngineeringCompoundCrs", format!("{title} — datum"))),
        interpretation: Some(Ref::new("dddddddd-0000-4000-8000-000000000004", "resqml22.WellboreInterpretation", format!("{title} — interpretation"))),
        wellhead: Some([X, Y, GROUND]),
        ..Known::default()
    };
    Pieces::empty().with("frames", Source::Shape { geometry, closed: None }).with("frames (object)", Source::Object(known))
}

fn extended(source: &str) -> Plan {
    let mut choices = Choices::new();
    for (name, value) in json!({ "maxDepth": 50.0, "step": 10.0, "source": source }).as_object().expect("an object") {
        choices = choices.set(name, value.clone());
    }
    Registry::new()
        .plan("extend-boreholes", &vertical_frame("F1", &[0.0, 5.0, 12.0]), &choices, &Identity::derived(), &when())
        .unwrap_or_else(|why| panic!("a plan: {why}"))
}

#[test]
fn extend_boreholes_keeps_its_uuids_when_the_source_changes() {
    let first = extended("plausible: boreholes of this area rarely exceed 200 m");
    let second = extended("vraisemblable : les forages de cette zone dépassent rarement 200 m");
    assert_same_identity_other_words(&first, &second, "extend-boreholes");
    // The two objects that carry the extension are the ones the seed names.
    assert!(first.objects.iter().any(|one| one.role == "frame") && first.objects.iter().any(|one| one.role == "trajectory"));
}

// -- a PRODML writer: pressure-transient-analysis ----------------------------

const TEST: &str = "2d2d2d2d-0000-4000-8000-000000000011";

fn analysed(source: &str) -> Plan {
    let known = Known { reference: Some(Ref::new(TEST, "prodml23.InjectionFlowTest", "GTH-2 — injection test")), ..Default::default() };
    let pieces = Pieces::empty().with("test", Source::Object(known));
    let choices = Choices::new()
        .set("title", "GTH-2 — injection test analysis")
        .set("source", Value::from(source))
        .set("testPeriod", "step-rate")
        .set("permeability", 5.0)
        .set("thickness", 95.0)
        .set("porosity", 0.35)
        .set("initialPressure", 12.0);
    Registry::new()
        .plan("pressure-transient-analysis", &pieces, &choices, &Identity::derived(), &when())
        .unwrap_or_else(|why| panic!("a plan: {why}"))
}

#[test]
fn a_prodml_writer_keeps_its_uuids_when_the_source_changes() {
    let first = analysed("synthetic — a first wording of the source");
    let second = analysed("synthétique — une autre formulation de la source");
    assert_same_identity_other_words(&first, &second, "pressure-transient-analysis");
}
