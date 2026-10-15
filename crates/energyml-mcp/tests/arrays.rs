//! The two array tools, on a fake store that serves arrays and counts the reads
//! that reach it.

use energyml_mcp::{Limits, RefusalCode, Registry};
use energyml_storage::contract::FEATURE;
use serde_json::json;

use crate::support::{Fake, MetaFail, SECRET, call, call_with, refusal, uri_of};

fn feature(store: &Fake) -> String {
    uri_of(store, FEATURE)
}

#[test]
fn list_arrays_gives_shape_and_type_and_reads_no_value() {
    let store = Fake::new();
    let answer = call(&store, "energyml_list_arrays", json!({ "uri": feature(&store) })).unwrap();
    assert_eq!(answer["total"], 1);
    assert_eq!(answer["object_has_no_array_fields"], false);
    let array = &answer["arrays"][0];
    assert_eq!(array["index"], 0);
    assert_eq!(array["readable"], "yes");
    assert_eq!(array["dimensions"], json!([1000, 3]));
    assert_eq!(array["element_type"], "double");
    assert_eq!(array["elements"], 3000);
    assert_eq!(array["field_path"], "Geometry.Points.Coordinates");
    assert_eq!(array["dataset_path"], "/resqml22/points");
    assert!(
        array.get("file").is_none(),
        "no file name of the package is shown"
    );
    assert!(store.reads().is_empty(), "listing reads no values");
}

#[test]
fn an_array_that_cannot_be_read_is_listed_with_the_reason_never_dropped() {
    let store = Fake::new().failing(MetaFail::FeatureNotEnabled);
    let answer = call(&store, "energyml_list_arrays", json!({ "uri": feature(&store) })).unwrap();
    // The capability is missing: the array is there, listed, unreadable. Not `[]`.
    assert_eq!(answer["total"], 1);
    assert_eq!(answer["object_has_no_array_fields"], false);
    let array = &answer["arrays"][0];
    assert_eq!(array["readable"], "no");
    assert_eq!(array["unreadable_reason"]["code"], "feature_not_enabled");

    let store = Fake::new().failing(MetaFail::FileMissing);
    let answer = call(&store, "energyml_list_arrays", json!({ "uri": feature(&store) })).unwrap();
    assert_eq!(answer["arrays"][0]["unreadable_reason"]["code"], "external_data");
}

#[test]
fn a_server_failure_while_listing_arrays_is_the_answers_and_leaks_no_secret() {
    let store = Fake::new().failing(MetaFail::Remote);
    let refusal = refusal(&store, "energyml_list_arrays", json!({ "uri": feature(&store) }));
    assert_eq!(refusal.code, RefusalCode::Remote);
    assert!(!refusal.to_value().to_string().contains(SECRET), "{refusal:?}");
}

#[test]
fn a_window_comes_with_statistics_and_a_preview() {
    let store = Fake::new();
    let answer = call(
        &store,
        "energyml_read_array_window",
        json!({
            "uri": feature(&store),
            "array": 0,
            "start": [10, 0],
            "count": [4, 3],
            "preview": 5
        }),
    )
    .unwrap();
    assert_eq!(answer["array"]["dimensions"], json!([1000, 3]));
    assert_eq!(answer["window"]["elements"], 12);
    assert_eq!(answer["window"]["of_total"], 3000);
    assert_eq!(answer["clamped"], false);
    assert_eq!(answer["truncated"], false);
    assert!(answer.get("truncation").is_none(), "nothing cut, nothing said");
    assert_eq!(answer["statistics"]["computed_over"], "window");
    assert_eq!(answer["statistics"]["elements"], 12);
    assert_eq!(answer["statistics"]["nan_count"], 0);
    assert_eq!(answer["preview"].as_array().unwrap().len(), 5);
    assert_eq!(store.reads().len(), 1);
}

#[test]
fn statistics_can_be_left_out() {
    let store = Fake::new();
    let answer = call(
        &store,
        "energyml_read_array_window",
        json!({ "uri": feature(&store), "array": 0, "count": [2, 3], "statistics": false, "preview": 0 }),
    )
    .unwrap();
    assert!(answer.get("statistics").is_none());
    assert_eq!(answer["preview"], json!([]));
}

#[test]
fn a_window_larger_than_the_cap_is_clamped_and_the_backend_never_sees_more() {
    // A billion elements asked of a store whose backend fails the test on a
    // read of more than a million: only the cap may reach it.
    let store = Fake::new().with_dims(vec![1_000_000_000]);
    let limits = Limits::default();
    let answer = call_with(
        &Registry::builtin(),
        &store,
        &limits,
        "energyml_read_array_window",
        json!({ "uri": feature(&store), "array": 0, "start": [0], "count": [1_000_000_000] }),
    )
    .unwrap();
    assert_eq!(answer["clamped"], true);
    assert_eq!(answer["truncated"], true);
    assert_eq!(answer["truncation"]["reason"], "window_elements");
    assert_eq!(answer["truncation"]["limit"], limits.max_window_elements);
    assert_eq!(answer["requested_count"], json!([1_000_000_000]));
    for (_, count) in store.reads() {
        let n: i64 = count.iter().product();
        assert!(
            n as usize <= limits.max_window_elements,
            "the backend received {n} elements, the cap is {}",
            limits.max_window_elements
        );
    }
    assert!(!store.reads().is_empty());
}

#[test]
fn the_cap_can_be_lowered_by_the_server_and_the_window_follows() {
    let store = Fake::new().with_dims(vec![1_000]);
    let limits = Limits::default().with_max_window_elements(100);
    let answer = call_with(
        &Registry::builtin(),
        &store,
        &limits,
        "energyml_read_array_window",
        json!({ "uri": feature(&store), "array": 0, "count": [1_000] }),
    )
    .unwrap();
    assert_eq!(answer["clamped"], true);
    assert!(answer["window"]["elements"].as_u64().unwrap() <= 100);
}

#[test]
fn a_window_that_does_not_match_the_array_is_refused_before_anything_is_read() {
    let store = Fake::new();
    let uri = feature(&store);
    for args in [
        // Wrong rank.
        json!({ "uri": uri, "array": 0, "start": [0] }),
        json!({ "uri": uri, "array": 0, "count": [1] }),
        // Outside the array.
        json!({ "uri": uri, "array": 0, "start": [5000, 0], "count": [1, 1] }),
        json!({ "uri": uri, "array": 0, "start": [999, 0], "count": [2, 1] }),
        // No such array.
        json!({ "uri": uri, "array": 7 }),
    ] {
        let refusal = refusal(&store, "energyml_read_array_window", args.clone());
        assert_eq!(refusal.code, RefusalCode::BadArguments, "{args}: {refusal:?}");
    }
    assert!(store.reads().is_empty(), "nothing was read");
}

#[test]
fn a_missing_capability_is_a_refusal_naming_it_and_not_an_empty_window() {
    let store = Fake::new().failing(MetaFail::FeatureNotEnabled);
    let refusal = refusal(
        &store,
        "energyml_read_array_window",
        json!({ "uri": feature(&store), "array": 0 }),
    );
    assert_eq!(refusal.code, RefusalCode::FeatureNotEnabled);
    assert!(refusal.hint.as_deref().unwrap().contains("hdf5"));
}
