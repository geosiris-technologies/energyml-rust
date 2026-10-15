//! Truncation is said when it happens and absent when it does not.

use serde_json::{Value, json};

use crate::support::{call, model, uri_of};
use energyml_storage::contract::HORIZON_1;

fn describe(args: Value) -> Value {
    call(&model(), "energyml_describe_object", args).unwrap()
}

fn horizon() -> String {
    uri_of(&model(), HORIZON_1)
}

#[test]
fn an_object_that_fits_is_not_marked_and_carries_no_truncation_field() {
    let answer = describe(json!({ "uri": horizon() }));
    assert_eq!(answer["truncated"], false);
    assert!(
        answer.get("truncation").is_none(),
        "nothing was cut, so nothing is said: {answer}"
    );
    assert!(
        answer["object"]["Citation"]["Title"].is_string(),
        "the whole object is there"
    );
}

#[test]
fn a_cut_is_marked_and_names_what_was_cut_and_how_to_get_more() {
    // Depth 0 keeps the object's own fields and cuts what nests below them (the
    // Citation, the reference to the feature).
    let answer = describe(json!({ "uri": horizon(), "max_depth": 0 }));
    assert_eq!(answer["truncated"], true, "{answer}");
    let truncation = &answer["truncation"];
    let cut = truncation["cut"].as_array().expect("the cut paths");
    assert!(!cut.is_empty());
    assert!(cut[0]["pointer"].is_string());
    assert!(cut[0]["reason"].is_string());
    assert_eq!(truncation["cut_total"], cut.len());
    assert_eq!(truncation["limit"]["max_depth"], 0);
    assert!(
        truncation["how_to_get_more"]
            .as_str()
            .unwrap()
            .contains("max_depth")
    );
    assert!(truncation["reason"].is_string());
}

#[test]
fn truncated_and_the_truncation_field_always_agree() {
    for depth in 0..=6 {
        let answer = describe(json!({ "uri": horizon(), "max_depth": depth }));
        let truncated = answer["truncated"].as_bool().expect("a boolean");
        assert_eq!(
            answer.get("truncation").is_some(),
            truncated,
            "depth {depth}: `truncated` and `truncation` disagree in {answer}"
        );
    }
}

#[test]
fn the_type_survives_a_cut() {
    let answer = describe(json!({ "uri": horizon(), "max_depth": 0 }));
    assert_eq!(
        answer["truncated"], true,
        "the test must cut to prove anything: {answer}"
    );
    assert_eq!(answer["object"]["$type"], "resqml22.HorizonInterpretation");
}

#[test]
fn a_lowered_budget_never_returns_more_than_a_larger_one() {
    let small = describe(json!({ "uri": horizon(), "max_depth": 0 }));
    let large = describe(json!({ "uri": horizon(), "max_depth": 12 }));
    assert!(small["object"].to_string().len() < large["object"].to_string().len());
    assert_eq!(
        small["bytes_total"], large["bytes_total"],
        "the full size is reported either way"
    );
}
