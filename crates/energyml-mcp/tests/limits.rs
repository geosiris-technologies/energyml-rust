//! The bounds: an argument lowers them and never raises them, and a cut list
//! says why it was cut.

use energyml_mcp::{Limits, Registry};
use serde_json::json;

use crate::support::{call_with, model};

#[test]
fn the_defaults_are_the_agreed_ones() {
    let limits = Limits::default();
    assert_eq!(limits.max_response_bytes, 65_536);
    assert_eq!(limits.max_list, 500);
    assert_eq!(limits.max_header_scan, 2_000);
    assert_eq!(limits.max_window_elements, 10_000);
    assert_eq!(limits.max_object_bytes, 262_144);
    assert_eq!(limits.call_timeout.as_secs(), 30);
    assert!(
        !limits.reveal_custom_data,
        "values of custom data are hidden by default"
    );
}

#[test]
fn an_argument_may_lower_a_bound_and_never_raise_it() {
    assert_eq!(Limits::effective(Some(10), 50, 500), 10, "lowered");
    assert_eq!(
        Limits::effective(Some(10_000), 50, 500),
        500,
        "never above the ceiling"
    );
    assert_eq!(
        Limits::effective(None, 50, 20),
        20,
        "even the default yields to the ceiling"
    );
    assert_eq!(Limits::effective(None, 50, 500), 50);
}

#[test]
fn a_list_asked_for_more_than_the_server_allows_gets_the_servers_ceiling_and_is_told() {
    let store = model();
    let limits = Limits::default().with_max_list(2);
    let answer = call_with(
        &Registry::builtin(),
        &store,
        &limits,
        "energyml_list_resources",
        json!({ "limit": 500 }),
    )
    .unwrap();
    assert_eq!(answer["returned"], 2, "the argument did not raise the bound");
    assert_eq!(answer["total"], 5);
    assert_eq!(answer["truncated"], true);
    assert_eq!(answer["truncation"]["reason"], "server_max_list");
    assert_eq!(answer["truncation"]["limit"], 2);
    assert!(
        answer["truncation"]["how_to_get_more"]
            .as_str()
            .unwrap()
            .contains("next_cursor")
    );
}

#[test]
fn a_list_asked_for_less_says_it_was_the_callers_limit() {
    let answer = call_with(
        &Registry::builtin(),
        &model(),
        &Limits::default(),
        "energyml_list_resources",
        json!({ "limit": 1 }),
    )
    .unwrap();
    assert_eq!(answer["returned"], 1);
    assert_eq!(answer["truncation"]["reason"], "limit");
    assert_eq!(answer["truncation"]["limit"], 1);
}

#[test]
fn a_list_stops_at_its_byte_budget_and_says_so() {
    // A response bound so small that no item fits: nothing is returned, and the
    // answer says it was cut by bytes and how to continue - not an empty list
    // presented as the whole.
    let limits = Limits::default().with_max_response_bytes(2_048 + 100);
    let answer = call_with(
        &Registry::builtin(),
        &model(),
        &limits,
        "energyml_list_resources",
        json!({}),
    )
    .unwrap();
    assert!(answer["returned"].as_u64().unwrap() < 5);
    assert_eq!(answer["total"], 5);
    assert_eq!(answer["truncated"], true);
    assert_eq!(answer["truncation"]["reason"], "response_bytes");
    assert!(answer["next_cursor"].is_string());
}

#[test]
fn an_argument_out_of_the_schema_range_is_refused_and_not_clamped() {
    for (tool, args) in [
        ("energyml_list_resources", json!({ "limit": 501 })),
        ("energyml_list_resources", json!({ "limit": 0 })),
        ("energyml_list_resources", json!({ "depth": 6 })),
        (
            "energyml_find_resources",
            json!({ "type_contains": "e", "limit": 201 }),
        ),
        (
            "energyml_describe_object",
            json!({ "uuid": "x", "max_bytes": 10 }),
        ),
        ("energyml_store_info", json!({ "diagnostics_limit": 101 })),
    ] {
        let refusal = call_with(
            &Registry::builtin(),
            &model(),
            &Limits::default(),
            tool,
            args.clone(),
        )
        .unwrap_err();
        assert_eq!(
            refusal.code,
            energyml_mcp::RefusalCode::BadArguments,
            "{tool} {args}"
        );
    }
}
