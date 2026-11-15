//! The provenance rule of `energyml_domain::integrity`: an activity against the
//! template its `ActivityDescriptor` names (ADR-0035).
//!
//! Source: common Technical Reference Guide 2.3 (Energistics), section 3.1 p. 40
//! (a parameter's `Title` "must have an equivalent in the activity descriptor
//! parameters") and section 3.10 p. 49 (`AllowedKind`: none given allows all;
//! `MaxOccurs`: -1 is infinite). Every object is fabricated here in JSON and
//! read through `energyml_core::json::read_value`; no file is read. The
//! helpers of `tests/integrity.rs` are imported, not copied.

use crate::integrity::{judge, of, read_of};
use energyml_core::json::read_value;
use energyml_core::registry::DynObject;
use energyml_domain::rules::{Gravity, Outcome};
use serde_json::{Value, json};

const T: &str = "11111111-1111-4111-8111-111111111111";
const A: &str = "33333333-3333-4333-8333-333333333333";
const A_TWIN: &str = "33333333-3333-4333-8333-333333333334";
const CHILD: &str = "22222222-2222-4222-8222-222222222222";
const OBJ: &str = "44444444-4444-4444-8444-444444444444";
const ABSENT: &str = "55555555-5555-4555-8555-555555555555";

const RULE: &str = "house/provenance/activity-follows-its-template";

fn object(value: &Value) -> Box<dyn DynObject> {
    read_value(value, &energyml_schemas::all())
        .expect("the fabricated object is read")
        .root
}

fn reference(qualified: &str, uuid: &str) -> Value {
    json!({ "$type": "eml23.DataObjectReference", "Uuid": uuid, "QualifiedType": qualified, "Title": "t" })
}

fn param_object(title: &str, reference: Value) -> Value {
    json!({ "$type": "eml23.DataObjectParameter", "Title": title, "IsUncertain": false, "DataObject": reference })
}

fn template_param(title: &str, allowed: &[&str]) -> Value {
    json!({ "$type": "eml23.ParameterTemplate", "Title": title, "AllowedKind": allowed,
            "IsInput": true, "IsOutput": false, "MinOccurs": 0, "MaxOccurs": -1 })
}

fn template(parameters: Vec<Value>) -> Value {
    json!({
        "$type": "eml23.ActivityTemplate", "uuid": T, "schemaVersion": "2.3",
        "Citation": { "$type": "eml23.Citation", "Title": "fabricated:test", "Originator": "tester",
                      "Creation": "2026-01-01T00:00:00Z", "Format": "test" },
        "Parameter": parameters,
    })
}

fn activity(uuid: &str, descriptor: &str, parameters: Vec<Value>) -> Value {
    json!({
        "$type": "eml23.Activity", "uuid": uuid, "schemaVersion": "2.3",
        "Citation": { "$type": "eml23.Citation", "Title": "a", "Originator": "tester",
                      "Creation": "2026-01-01T00:00:00Z", "Format": "test" },
        "ActivityDescriptor": reference("eml23.ActivityTemplate", descriptor),
        "Parameter": parameters,
    })
}

/// common TRG 2.3 section 3.1 p. 40: a parameter the template does not declare
/// is found once, as a warning, with its rank and its title.
/// To break it: empty `activity_follows_its_template`.
#[test]
fn an_activity_whose_parameter_the_template_lacks_is_a_warning() {
    let findings = judge(
        vec![
            (
                T,
                object(&template(vec![template_param("output", &["dataObject"])])),
            ),
            (
                A,
                object(&activity(
                    A,
                    T,
                    vec![
                        param_object("input", reference("resqml22.ContinuousProperty", OBJ)),
                        param_object("output", reference("resqml22.ContinuousProperty", OBJ)),
                    ],
                )),
            ),
        ],
        false,
    );
    let found = of(&findings, RULE);
    assert_eq!(found.len(), 1, "{findings:?}");
    assert_eq!(found[0].uuid, A);
    assert_eq!(found[0].gravity, Gravity::Warning);
    let read = read_of(found[0]);
    assert!(
        read.contains("parameter `input` (0) is not in the template"),
        "{read}"
    );
    assert!(!read.contains("`output`"), "{read}");
}

/// The same template, now declaring `input` too: nothing to say.
/// To break it: signal every activity.
#[test]
fn an_activity_that_follows_its_template_has_no_finding() {
    let findings = judge(
        vec![
            (
                T,
                object(&template(vec![
                    template_param("input", &["dataObject"]),
                    template_param("output", &["dataObject"]),
                ])),
            ),
            (
                A,
                object(&activity(
                    A,
                    T,
                    vec![
                        param_object("input", reference("resqml22.ContinuousProperty", OBJ)),
                        param_object("output", reference("resqml22.ContinuousProperty", OBJ)),
                    ],
                )),
            ),
        ],
        false,
    );
    assert!(of(&findings, RULE).is_empty(), "{findings:?}");
}

/// Not having found the template is not having found nothing wrong: the
/// activity is *unverifiable*, naming the template's address, never a pass.
/// To break it: `continue` without pushing the unverifiable finding.
#[test]
fn an_activity_whose_template_is_elsewhere_is_unverifiable_never_a_pass() {
    let findings = judge(
        vec![(
            A,
            object(&activity(
                A,
                ABSENT,
                vec![param_object(
                    "input",
                    reference("resqml22.ContinuousProperty", OBJ),
                )],
            )),
        )],
        false,
    );
    let found = of(&findings, RULE);
    assert_eq!(found.len(), 1, "{findings:?}");
    match &found[0].outcome {
        Outcome::Unverifiable { why } => assert!(why.contains(ABSENT), "{why}"),
        other => panic!("expected an unverifiable finding, got {other:?}"),
    }
}

/// common TRG 2.3 section 3.4 p. 43: a data object naming an activity is read
/// as a sub-activity; one naming anything else is a data object, and the
/// template does not allow it there.
/// To break it: accept any data object under `subActivity` (in `conformance`).
#[test]
fn a_data_object_naming_an_activity_is_a_sub_activity_and_conforms() {
    let findings = judge(
        vec![
            (
                T,
                object(&template(vec![template_param("etape-1", &["subActivity"])])),
            ),
            (
                A,
                object(&activity(
                    A,
                    T,
                    vec![param_object("etape-1", reference("eml23.Activity", CHILD))],
                )),
            ),
            (
                A_TWIN,
                object(&activity(
                    A_TWIN,
                    T,
                    vec![param_object(
                        "etape-1",
                        reference("resqml22.ContinuousProperty", OBJ),
                    )],
                )),
            ),
        ],
        false,
    );
    let found = of(&findings, RULE);
    assert_eq!(found.len(), 1, "{findings:?}");
    assert_eq!(found[0].uuid, A_TWIN);
    assert!(
        read_of(found[0]).contains("is written dataObject"),
        "{:?}",
        found[0]
    );
}
