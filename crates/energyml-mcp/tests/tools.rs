//! The happy path of each tool, on the contract's model held in an EPC.
//!
//! The model: a feature (FEATURE), two horizons of it (HORIZON_1, HORIZON_2), a
//! column of the first (COLUMN), and a horizon whose feature the package does
//! not hold (DANGLING, of ABSENT).

use energyml_mcp::{Limits, Registry};
use energyml_storage::contract::{COLUMN, DANGLING, FEATURE, HORIZON_1, HORIZON_2};
use serde_json::{Value, json};

use crate::support::{Fake, call, call_with, model, uri_of};

fn uris(answer: &Value, key: &str) -> Vec<String> {
    answer[key]
        .as_array()
        .unwrap_or_else(|| panic!("no `{key}` in {answer}"))
        .iter()
        .map(|item| item["uri"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn store_info_describes_the_store_and_the_server() {
    let answer = call(&model(), "energyml_store_info", json!({})).unwrap();
    assert_eq!(answer["store_kind"], "EPC");
    assert_eq!(answer["read_only"], true);
    assert_eq!(answer["dataspace_count"], 1);
    assert!(answer["supported_types_count"].as_u64().unwrap() > 0);
    assert!(
        answer["capabilities"]["raw_formats"]
            .as_array()
            .unwrap()
            .contains(&json!("xml"))
    );
    assert_eq!(answer["limits"]["max_response_bytes"], 65_536);
    assert_eq!(answer["limits"]["max_list"], 500);
    assert_eq!(answer["limits"]["max_window_elements"], 10_000);
    assert_eq!(answer["limits"]["call_timeout_seconds"], 30);
    assert_eq!(answer["truncated"], false);
    assert!(answer.get("truncation").is_none());
    assert!(answer.get("supported_types").is_none(), "only when asked");
    // The model holds no array: the probe has nothing to try and does not guess.
    assert_eq!(answer["arrays_readable"], "unknown");
    assert!(answer["diagnostics"]["shown"].as_array().unwrap().len() <= 10);
}

#[test]
fn store_info_lists_the_types_when_asked() {
    let answer = call(&model(), "energyml_store_info", json!({ "include_types": true })).unwrap();
    let listed = answer["supported_types"].as_array().unwrap();
    assert!(!listed.is_empty());
    assert!(listed.contains(&json!("resqml22.BoundaryFeature")), "{listed:?}");
}

#[test]
fn store_info_says_how_many_diagnostics_were_left_out() {
    let answer = call(&model(), "energyml_store_info", json!({ "diagnostics_limit": 0 })).unwrap();
    assert_eq!(answer["diagnostics"]["shown"].as_array().unwrap().len(), 0);
    let total = answer["diagnostics"]["total"].as_u64().unwrap();
    assert_eq!(
        answer["truncated"],
        total > 0,
        "the cut is said whenever something was left out"
    );
}

#[test]
fn an_epc_is_one_dataspace() {
    let answer = call(&model(), "energyml_list_dataspaces", json!({})).unwrap();
    assert_eq!(answer["total"], 1);
    assert_eq!(answer["returned"], 1);
    assert_eq!(answer["truncated"], false);
    let dataspace = &answer["dataspaces"][0];
    assert!(dataspace["uri"].as_str().unwrap().starts_with("eml:///"));
    assert!(dataspace.get("custom_data").is_none());
}

#[test]
fn list_resources_lists_the_dataspace_by_default() {
    let answer = call(&model(), "energyml_list_resources", json!({})).unwrap();
    assert_eq!(answer["total"], 5);
    assert_eq!(answer["returned"], 5);
    assert_eq!(answer["truncated"], false);
    assert!(answer.get("next_cursor").is_none());
    assert_eq!(answer["titles"], "not_loaded", "a listing says it has no titles");
    let first = &answer["resources"][0];
    for key in ["uri", "uuid", "qualified_type"] {
        assert!(first[key].is_string(), "{key} in {first}");
    }
}

#[test]
fn list_resources_filters_by_exact_type_and_by_schema_wildcard() {
    let store = model();
    let exact = call(
        &store,
        "energyml_list_resources",
        json!({ "types": ["resqml22.HorizonInterpretation"] }),
    )
    .unwrap();
    assert_eq!(exact["total"], 3);
    let wildcard = call(
        &store,
        "energyml_list_resources",
        json!({ "types": ["resqml22.*"] }),
    )
    .unwrap();
    assert_eq!(wildcard["total"], 5);
    // `types` is exact and case sensitive: a fragment matches nothing here
    // (energyml_find_resources is the substring door), and says 0, not an error.
    let fragment = call(&store, "energyml_list_resources", json!({ "types": ["Horizon"] })).unwrap();
    assert_eq!(fragment["total"], 0);
}

#[test]
fn list_resources_pages_with_an_opaque_cursor() {
    let store = model();
    let first = call(&store, "energyml_list_resources", json!({ "limit": 2 })).unwrap();
    assert_eq!(first["returned"], 2);
    assert_eq!(first["total"], 5);
    assert_eq!(first["truncated"], true);
    let cursor = first["next_cursor"].as_str().expect("a cursor").to_string();
    let second = call(
        &store,
        "energyml_list_resources",
        json!({ "limit": 2, "cursor": cursor }),
    )
    .unwrap();
    let third = call(
        &store,
        "energyml_list_resources",
        json!({ "limit": 2, "cursor": second["next_cursor"] }),
    )
    .unwrap();
    assert_eq!(third["returned"], 1);
    assert_eq!(third["truncated"], false);
    assert!(third.get("next_cursor").is_none());

    let mut all = uris(&first, "resources");
    all.extend(uris(&second, "resources"));
    all.extend(uris(&third, "resources"));
    let mut distinct = all.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(all.len(), 5);
    assert_eq!(distinct.len(), 5, "no object twice, none missed");
}

#[test]
fn list_resources_walks_from_an_object() {
    let store = model();
    let column = uri_of(&store, COLUMN);
    let answer = call(
        &store,
        "energyml_list_resources",
        json!({ "scope": column, "direction": "targets", "depth": 1 }),
    )
    .unwrap();
    assert_eq!(uris(&answer, "resources"), vec![uri_of(&store, HORIZON_1)]);
}

#[test]
fn find_resources_by_type_fragment_title_and_uuid() {
    let store = model();
    let by_type = call(
        &store,
        "energyml_find_resources",
        json!({ "type_contains": "horizon" }),
    )
    .unwrap();
    assert_eq!(by_type["total"], 3, "case-insensitive substring");
    for hit in by_type["hits"].as_array().unwrap() {
        assert_eq!(hit["matched"], json!(["type"]));
        assert!(hit["title"].is_string(), "titles come from the headers: {hit}");
    }
    assert_eq!(by_type["headers_unreadable"], 0);

    let by_title = call(
        &store,
        "energyml_find_resources",
        json!({ "title_contains": "ANOTHER" }),
    )
    .unwrap();
    assert_eq!(uris(&by_title, "hits"), vec![uri_of(&store, HORIZON_2)]);
    assert_eq!(by_title["hits"][0]["title"], "another horizon");
    assert_eq!(by_title["hits"][0]["matched"], json!(["title"]));

    let by_uuid = call(
        &store,
        "energyml_find_resources",
        json!({ "uuid_prefix": FEATURE }),
    )
    .unwrap();
    assert_eq!(uris(&by_uuid, "hits"), vec![uri_of(&store, FEATURE)]);
    assert_eq!(by_uuid["hits"][0]["matched"], json!(["uuid"]));
}

#[test]
fn describe_object_returns_the_json_with_its_type() {
    let store = model();
    let uri = uri_of(&store, HORIZON_1);
    let answer = call(&store, "energyml_describe_object", json!({ "uri": uri })).unwrap();
    assert_eq!(answer["format"], "json");
    assert_eq!(answer["qualified_type"], "resqml22.HorizonInterpretation");
    assert_eq!(answer["title"], "a horizon");
    assert_eq!(answer["object"]["$type"], "resqml22.HorizonInterpretation");
    assert_eq!(answer["references_count"], 1);
    assert!(answer["bytes_total"].as_u64().unwrap() > 0);
}

#[test]
fn describe_object_accepts_a_bare_uuid_at_the_entry_and_answers_with_the_full_uri() {
    let store = model();
    let answer = call(&store, "energyml_describe_object", json!({ "uuid": FEATURE })).unwrap();
    assert_eq!(answer["uri"], uri_of(&store, FEATURE).as_str());
    assert_eq!(answer["object"]["$type"], "resqml22.BoundaryFeature");
}

#[test]
fn describe_object_returns_the_stored_xml_when_the_store_offers_it() {
    let store = model();
    let uri = uri_of(&store, FEATURE);
    let answer = call(
        &store,
        "energyml_describe_object",
        json!({ "uri": uri, "format": "xml" }),
    )
    .unwrap();
    assert_eq!(answer["format"], "xml");
    assert!(answer["text"].as_str().unwrap().contains("BoundaryFeature"));
    assert_eq!(answer["truncated"], false);
}

#[test]
fn get_references_follows_targets_and_sources() {
    let store = model();
    let targets = call(
        &store,
        "energyml_get_references",
        json!({ "uri": uri_of(&store, HORIZON_1), "direction": "targets" }),
    )
    .unwrap();
    assert_eq!(uris(&targets, "objects"), vec![uri_of(&store, FEATURE)]);
    // The store's discovery answers every edge with EITHER end among what it
    // found (ETP 1.2 section 8.3.1, p. 163, 3.c): the feature is found, so the
    // second horizon's edge to it comes too, beside this horizon's own.
    let pairs: Vec<(String, String)> = targets["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edge| {
            (
                edge["from"].as_str().unwrap().to_string(),
                edge["to"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let feature = uri_of(&store, FEATURE);
    assert_eq!(
        pairs,
        vec![
            (uri_of(&store, HORIZON_1), feature.clone()),
            (uri_of(&store, HORIZON_2), feature)
        ],
        "{targets}"
    );
    assert_eq!(targets["dangling"], json!([]));
    // The EPC derives its edges from the objects and does not state their kind:
    // the answer says so and shows no kind.
    assert_eq!(targets["edge_kinds"], false);
    assert!(targets["edges"][0].get("kind").is_none());
    assert_eq!(targets["truncated"], false);

    let sources = call(
        &store,
        "energyml_get_references",
        json!({ "uri": uri_of(&store, FEATURE), "direction": "sources" }),
    )
    .unwrap();
    let mut found = uris(&sources, "objects");
    found.sort();
    let mut expected = vec![uri_of(&store, HORIZON_1), uri_of(&store, HORIZON_2)];
    expected.sort();
    assert_eq!(found, expected);

    let both = call(
        &store,
        "energyml_get_references",
        json!({ "uri": uri_of(&store, HORIZON_1), "direction": "both" }),
    )
    .unwrap();
    assert_eq!(
        both["total"], 2,
        "the feature it points at and the column pointing at it"
    );
}

#[test]
fn a_reference_to_an_object_the_store_lacks_is_dangling_and_not_hidden() {
    let store = model();
    let answer = call(
        &store,
        "energyml_get_references",
        json!({ "uri": uri_of(&store, DANGLING) }),
    )
    .unwrap();
    assert_eq!(uris(&answer, "objects"), Vec::<String>::new());
    let dangling = answer["dangling"].as_array().unwrap();
    assert_eq!(dangling.len(), 1);
    assert!(
        dangling[0]["to"]
            .as_str()
            .unwrap()
            .contains("7f000000-0000-4000-8000-0000000000ff")
    );
}

#[test]
fn list_arrays_of_an_object_without_arrays_says_so() {
    let store = model();
    let answer = call(
        &store,
        "energyml_list_arrays",
        json!({ "uri": uri_of(&store, FEATURE) }),
    )
    .unwrap();
    assert_eq!(answer["arrays"], json!([]));
    assert_eq!(answer["object_has_no_array_fields"], true);
    assert_eq!(answer["total"], 0);
}

#[test]
fn a_tool_runs_the_same_through_a_store_that_wraps_the_model() {
    // The fake adds nothing to the listing: the tools only see the traits.
    let direct = call(&model(), "energyml_list_resources", json!({})).unwrap();
    let wrapped = call(&Fake::new(), "energyml_list_resources", json!({})).unwrap();
    assert_eq!(direct["total"], wrapped["total"]);
}

#[test]
fn no_tool_changes_the_store() {
    let store = model();
    let before: Vec<String> = crate::support::run(energyml_storage::EnergymlStorage::resources(&store))
        .unwrap()
        .into_iter()
        .map(|summary| summary.uri)
        .collect();
    let uri = uri_of(&store, HORIZON_1);
    let registry = Registry::builtin();
    let limits = Limits::default();
    for (name, args) in [
        ("energyml_store_info", json!({ "include_types": true })),
        ("energyml_list_dataspaces", json!({})),
        ("energyml_list_resources", json!({})),
        ("energyml_find_resources", json!({ "type_contains": "e" })),
        ("energyml_describe_object", json!({ "uri": uri })),
        (
            "energyml_get_references",
            json!({ "uri": uri, "direction": "both" }),
        ),
        ("energyml_list_arrays", json!({ "uri": uri })),
    ] {
        call_with(&registry, &store, &limits, name, args).unwrap_or_else(|r| panic!("{name}: {r}"));
    }
    let after: Vec<String> = crate::support::run(energyml_storage::EnergymlStorage::resources(&store))
        .unwrap()
        .into_iter()
        .map(|summary| summary.uri)
        .collect();
    assert_eq!(before, after);
    assert!(
        registry.tools().all(|tool| tool
            .annotations(&energyml_mcp::StoreInfo::new("EPC", false))
            .read_only_hint),
        "every builtin tool is annotated read-only"
    );
}

#[test]
fn a_default_scope_is_what_a_call_naming_none_means() {
    // A server started on one dataspace (`retp mcp --dataspace`) answers a
    // call without `scope` on that dataspace, even when the store holds others.
    let store = model();
    let limits = Limits::default();
    let extensions = energyml_mcp::Extensions::new();
    let other = "eml:///dataspace('another')";
    let ctx = energyml_mcp::ToolContext::new(&store, &limits, &extensions).with_default_scope(Some(other));
    let answer = crate::support::run(Registry::builtin().call("energyml_list_resources", &ctx, json!({})));
    // The model holds no such dataspace: the call went there, and says so by
    // its refusal or its scope, never by answering on the model's own.
    match answer {
        Ok(output) => assert_eq!(output.structured["scope"], other, "{}", output.structured),
        Err(refused) => assert!(format!("{refused:?}").contains("another"), "{refused:?}"),
    }
    // Negative: without a default, the same call answers on the model's only dataspace.
    let plain = call(&store, "energyml_list_resources", json!({})).unwrap();
    assert_ne!(plain["scope"], other);
}
