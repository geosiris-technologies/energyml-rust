//! Refusals: every row of the mapping table that a call can reach, the shape of
//! the answer, and the guards that keep a refusal from being an empty list or a
//! leak.

use energyml_mcp::{Limits, Refusal, RefusalCode, Registry, redact};
use energyml_storage::contract::{ABSENT, FEATURE, HORIZON_1, NEW_1};
use energyml_storage::error::{Error, ExternalDataReason};
use serde_json::json;

use crate::support::{
    Fake, INJECTION, SECRET, call_with, model, package_of_parts, parts_of, refusal, uri_of,
};

// -- the mapping table, one row at a time, through the tools that reach it -----

#[test]
fn not_found_when_the_object_is_not_in_the_store() {
    let store = model();
    // A well-formed URI of an object the package does not hold.
    let absent = uri_of(&store, FEATURE).replace(FEATURE, ABSENT);
    let r = refusal(&store, "energyml_describe_object", json!({ "uri": absent }));
    assert_eq!(r.code, RefusalCode::NotFound);
    assert!(r.hint.is_some());

    let r = refusal(&store, "energyml_describe_object", json!({ "uuid": ABSENT }));
    assert_eq!(
        r.code,
        RefusalCode::NotFound,
        "a bare uuid that resolves to nothing"
    );
}

#[test]
fn ambiguous_when_a_uuid_names_two_versions_and_both_are_named() {
    // The same uuid held under two object versions: nothing picks "the latest".
    let feature = energyml_storage::contract::feature(NEW_1, "versioned");
    let one = feature.xml.replace(
        "schemaVersion=\"2.2\">",
        "schemaVersion=\"2.2\" objectVersion=\"1\">",
    );
    let two = feature.xml.replace(
        "schemaVersion=\"2.2\">",
        "schemaVersion=\"2.2\" objectVersion=\"2\">",
    );
    let store = package_of_parts(&[
        ("BoundaryFeature_v1.xml".to_string(), one),
        ("BoundaryFeature_v2.xml".to_string(), two),
    ]);
    let r = refusal(&store, "energyml_describe_object", json!({ "uuid": NEW_1 }));
    assert_eq!(r.code, RefusalCode::Ambiguous);
    assert_eq!(r.candidates.len(), 2, "{r:?}");
    assert!(
        r.candidates.iter().all(|c| c.starts_with("eml:///")),
        "full URIs: {:?}",
        r.candidates
    );
    // Each candidate resolves.
    for candidate in &r.candidates {
        let answer = call_with(
            &Registry::builtin(),
            &store,
            &Limits::default(),
            "energyml_describe_object",
            json!({ "uri": candidate }),
        );
        assert!(answer.is_ok(), "{candidate}: {answer:?}");
    }
}

#[test]
fn bad_uri_when_the_value_is_not_an_etp_uri_or_names_a_dataspace() {
    let store = model();
    let r = refusal(&store, "energyml_describe_object", json!({ "uri": "not a uri" }));
    assert_eq!(r.code, RefusalCode::BadUri);
    let r = refusal(&store, "energyml_describe_object", json!({ "uri": "eml:///" }));
    assert_eq!(r.code, RefusalCode::BadUri, "a dataspace is not an object");
    let r = refusal(
        &store,
        "energyml_list_resources",
        json!({ "scope": "resqml22.BoundaryFeature" }),
    );
    assert_eq!(r.code, RefusalCode::BadUri);
    // A bare name of a dataspace is not accepted either: URIs come from the listing.
    let r = refusal(
        &store,
        "energyml_find_resources",
        json!({ "type_contains": "e", "scope": "carl/teapot" }),
    );
    assert_eq!(r.code, RefusalCode::BadUri);
}

#[test]
fn unsupported_when_the_store_has_no_xml_and_is_never_an_empty_text() {
    let store = Fake::new();
    let r = refusal(
        &store,
        "energyml_describe_object",
        json!({ "uri": uri_of(&store, FEATURE), "format": "xml" }),
    );
    assert_eq!(r.code, RefusalCode::Unsupported);
    assert_eq!(r.store.as_deref(), Some("fake"));
    assert!(r.operation.is_some());

    // The same store offering it answers.
    let mut with_xml = Fake::new();
    with_xml.raw = true;
    let answer = call_with(
        &Registry::builtin(),
        &with_xml,
        &Limits::default(),
        "energyml_describe_object",
        json!({ "uri": uri_of(&with_xml, FEATURE), "format": "xml" }),
    )
    .unwrap();
    assert!(!answer["text"].as_str().unwrap().is_empty());
}

#[test]
fn feature_not_enabled_is_not_absent_data() {
    let store = Fake::new().failing(crate::support::MetaFail::FeatureNotEnabled);
    let r = refusal(
        &store,
        "energyml_read_array_window",
        json!({ "uri": uri_of(&store, FEATURE), "array": 0 }),
    );
    assert_eq!(r.code, RefusalCode::FeatureNotEnabled);
    assert_ne!(r.code, RefusalCode::NotFound);
}

#[test]
fn external_data_when_the_file_cannot_be_reached() {
    let store = Fake::new().failing(crate::support::MetaFail::FileMissing);
    let r = refusal(
        &store,
        "energyml_read_array_window",
        json!({ "uri": uri_of(&store, FEATURE), "array": 0 }),
    );
    assert_eq!(r.code, RefusalCode::ExternalData);
}

#[test]
fn too_large_when_an_answer_would_exceed_a_bound() {
    let store = model();
    // The XML of an object is refused, not cut, when it exceeds the ceiling.
    let limits = Limits::default().with_max_object_bytes(64);
    let r = call_with(
        &Registry::builtin(),
        &store,
        &limits,
        "energyml_describe_object",
        json!({ "uri": uri_of(&store, FEATURE), "format": "xml" }),
    )
    .unwrap_err();
    assert_eq!(r.code, RefusalCode::TooLarge);
    assert!(r.hint.is_some());

    // A title search that would read every header of a large store.
    let limits = Limits::default().with_max_header_scan(2);
    let r = call_with(
        &Registry::builtin(),
        &store,
        &limits,
        "energyml_find_resources",
        json!({ "title_contains": "horizon" }),
    )
    .unwrap_err();
    assert_eq!(r.code, RefusalCode::TooLarge);
    // Narrowed, it goes through.
    assert!(
        call_with(
            &Registry::builtin(),
            &store,
            &limits,
            "energyml_find_resources",
            json!({ "title_contains": "horizon", "type_contains": "Horizon" }),
        )
        .is_ok()
    );
}

#[test]
fn remote_when_the_server_fails_and_its_words_stay_in_the_detail_without_the_secret() {
    let mut store = Fake::new();
    store.fail_dataspaces = true;
    let r = refusal(&store, "energyml_store_info", json!({}));
    assert_eq!(r.code, RefusalCode::Remote);
    assert_eq!(r.store.as_deref(), Some("fake"));

    let whole = r.to_value().to_string();
    assert!(
        !whole.contains(SECRET),
        "the credential reached the answer: {whole}"
    );
    assert!(
        r.detail.as_deref().unwrap().contains("Unauthorized"),
        "the rest of what the server said is kept"
    );
    // The server's text is data: the sentence of the refusal is ours and does
    // not repeat it.
    assert!(!r.message.contains(INJECTION));
    assert!(r.detail.as_deref().unwrap().contains(INJECTION));
}

#[test]
fn a_store_that_fails_is_a_refusal_and_never_a_count_of_zero() {
    let mut store = Fake::new();
    store.fail_dataspaces = true;
    for name in [
        "energyml_store_info",
        "energyml_list_dataspaces",
        "energyml_list_resources",
    ] {
        let r = call_with(&Registry::builtin(), &store, &Limits::default(), name, json!({})).unwrap_err();
        assert_eq!(r.code, RefusalCode::Remote, "{name}");
    }
}

#[test]
fn bad_arguments_for_an_unknown_missing_or_ill_typed_argument() {
    let store = model();
    let cases = [
        ("energyml_list_resources", json!({ "limt": 3 })),
        ("energyml_list_resources", json!({ "limit": "3" })),
        ("energyml_list_resources", json!({ "types": "resqml22.*" })),
        ("energyml_list_resources", json!({ "direction": "sideways" })),
        ("energyml_list_resources", json!({ "cursor": "garbage" })),
        ("energyml_find_resources", json!({})),
        ("energyml_describe_object", json!({})),
        ("energyml_describe_object", json!({ "uri": "x", "uuid": "y" })),
        (
            "energyml_describe_object",
            json!({ "type": "resqml22.BoundaryFeature" }),
        ),
        ("energyml_get_references", json!({})),
        ("energyml_list_arrays", json!({})),
        ("energyml_read_array_window", json!({ "uri": "x" })),
    ];
    for (tool, args) in cases {
        let r = call_with(
            &Registry::builtin(),
            &store,
            &Limits::default(),
            tool,
            args.clone(),
        )
        .unwrap_err();
        assert_eq!(r.code, RefusalCode::BadArguments, "{tool} {args}: {r:?}");
    }
}

#[test]
fn an_unknown_argument_is_refused_by_every_tool_and_not_ignored() {
    let store = model();
    for tool in Registry::builtin().tools() {
        let r = call_with(
            &Registry::builtin(),
            &store,
            &Limits::default(),
            tool.name(),
            json!({ "no_such_argument": true }),
        )
        .unwrap_err();
        assert_eq!(r.code, RefusalCode::BadArguments, "{}", tool.name());
        assert_eq!(r.detail.as_deref(), Some("no_such_argument"));
    }
}

#[test]
fn a_dataspace_scope_refuses_a_direction() {
    let store = model();
    let r = refusal(
        &store,
        "energyml_list_resources",
        json!({ "direction": "targets" }),
    );
    assert_eq!(r.code, RefusalCode::BadArguments);
}

// -- the conversion itself, for the rows no tool call reaches ------------------

#[test]
fn every_error_has_a_code_and_its_words_go_to_the_data_fields() {
    let hostile = format!("{INJECTION} Authorization: Bearer {SECRET}");
    let cases: Vec<(Error, RefusalCode)> = vec![
        (Error::NotFound(hostile.clone()), RefusalCode::NotFound),
        (
            Error::Ambiguous {
                uri: "eml:///x".into(),
                candidates: vec!["a".into(), "b".into()],
            },
            RefusalCode::Ambiguous,
        ),
        (
            Error::BadUri {
                uri: hostile.clone(),
                reason: hostile.clone(),
            },
            RefusalCode::BadUri,
        ),
        (Error::UnknownType(hostile.clone()), RefusalCode::Unsupported),
        (
            Error::Unsupported {
                operation: "something",
                store: "fake",
            },
            RefusalCode::Unsupported,
        ),
        (
            Error::ExternalData {
                location: "m.h5".into(),
                reason: ExternalDataReason::FeatureNotEnabled {
                    feature: "hdf5",
                    mime: None,
                },
            },
            RefusalCode::FeatureNotEnabled,
        ),
        (
            Error::ExternalData {
                location: "m.h5".into(),
                reason: ExternalDataReason::PathMissing(hostile.clone()),
            },
            RefusalCode::ExternalData,
        ),
        (
            Error::Remote {
                store: "etp",
                reason: hostile.clone(),
            },
            RefusalCode::Remote,
        ),
        (
            Error::Io(std::io::Error::other(hostile.clone())),
            RefusalCode::Other,
        ),
        (Error::Package(hostile.clone()), RefusalCode::Other),
        (Error::Xml(hostile.clone()), RefusalCode::Other),
        (
            Error::Transaction {
                store: "fake",
                reason: hostile.clone(),
            },
            RefusalCode::Other,
        ),
    ];
    for (error, code) in cases {
        let shown = format!("{error:?}");
        let r = Refusal::from(error);
        assert_eq!(r.code, code, "{shown}");
        assert!(!r.message.contains(INJECTION), "the sentence is ours: {shown}");
        assert!(!r.message.contains(SECRET), "{shown}");
        assert!(
            !r.to_value().to_string().contains(SECRET),
            "a credential leaked from {shown}"
        );
    }
}

#[test]
fn the_answer_has_the_documented_shape_and_leaves_absent_fields_out() {
    let r = Refusal::new(RefusalCode::NotFound, "no such thing");
    assert_eq!(
        r.to_value(),
        json!({ "refusal": { "code": "not_found", "message": "no such thing" } })
    );
    let r = Refusal::new(RefusalCode::Unsupported, "no")
        .with_store("EPC")
        .with_operation("op")
        .with_hint("h")
        .with_detail("d");
    assert_eq!(
        r.to_value(),
        json!({ "refusal": { "code": "unsupported", "message": "no", "store": "EPC", "operation": "op", "hint": "h", "detail": "d" } })
    );
    for (code, word) in [
        (RefusalCode::NotFound, "not_found"),
        (RefusalCode::Ambiguous, "ambiguous"),
        (RefusalCode::BadUri, "bad_uri"),
        (RefusalCode::Unsupported, "unsupported"),
        (RefusalCode::FeatureNotEnabled, "feature_not_enabled"),
        (RefusalCode::ExternalData, "external_data"),
        (RefusalCode::TooLarge, "too_large"),
        (RefusalCode::Remote, "remote"),
        (RefusalCode::BadArguments, "bad_arguments"),
        (RefusalCode::Timeout, "timeout"),
        (RefusalCode::Other, "other"),
    ] {
        assert_eq!(code.as_str(), word);
    }
}

#[test]
fn redaction_removes_the_word_after_a_credential_marker_and_keeps_the_rest() {
    let cleaned = redact("401 Unauthorized, Authorization: Bearer abc123secret; try later");
    assert!(!cleaned.contains("abc123secret"), "{cleaned}");
    assert!(cleaned.contains("401 Unauthorized"));
    assert!(cleaned.contains("try later"));
    assert!(!redact("password=hunter2&user=bob").contains("hunter2"));
    assert!(!redact("{\"access_token\": \"eyJhbGciOi\"}").contains("eyJhbGciOi"));
    assert_eq!(redact("nothing to hide here"), "nothing to hide here");
}

#[test]
fn the_same_calls_answer_when_nothing_is_wrong() {
    // A control for the tests above: the same store and the same calls do answer
    // when nothing is wrong, so a refusal in them is the guard speaking.
    let store = model();
    let r = call_with(
        &Registry::builtin(),
        &store,
        &Limits::default(),
        "energyml_describe_object",
        json!({ "uri": uri_of(&store, HORIZON_1) }),
    );
    assert!(r.is_ok());
    // And an EPC rebuilt from the same parts answers the same.
    let again = package_of_parts(&parts_of(&energyml_storage::contract::model().objects));
    assert_eq!(uri_of(&again, HORIZON_1), uri_of(&store, HORIZON_1));
}
