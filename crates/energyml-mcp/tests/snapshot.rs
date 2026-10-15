//! A snapshot of `tools/list`: names, order, and the shape of every input schema.
//!
//! The contract of the server is what a client sees: a tool that disappears or
//! changes its name, an argument that becomes required, an argument that
//! vanishes. Any of those fails here and has to be written down on purpose
//! (design note `mcp-generique.md` §4.0, "stabilite du contrat"). A new
//! optional argument is added to the table; nothing else is allowed to move.
//!
//! The expected values are written out by hand from the design, not produced by
//! the code under test.

use energyml_mcp::{Registry, StoreInfo};
use serde_json::Value;

/// `(name, properties sorted, required sorted, alternative that must be given)`
/// where the alternative is `""`, `"anyOf"` or `"oneOf"`.
const SNAPSHOT: [(&str, &[&str], &[&str], &str); 8] = [
    (
        "energyml_store_info",
        &["diagnostics_limit", "include_types"],
        &[],
        "",
    ),
    ("energyml_list_dataspaces", &[], &[], ""),
    (
        "energyml_list_resources",
        &["cursor", "depth", "direction", "limit", "scope", "types"],
        &[],
        "",
    ),
    (
        "energyml_find_resources",
        &[
            "cursor",
            "limit",
            "scope",
            "title_contains",
            "type_contains",
            "uuid_prefix",
        ],
        &[],
        "anyOf",
    ),
    (
        "energyml_describe_object",
        &[
            "format",
            "max_bytes",
            "max_depth",
            "max_list",
            "type",
            "uri",
            "uuid",
        ],
        &[],
        "oneOf",
    ),
    (
        "energyml_get_references",
        &["depth", "direction", "limit", "uri"],
        &["uri"],
        "",
    ),
    (
        "energyml_list_arrays",
        &["cursor", "limit", "type", "uri", "uuid"],
        &[],
        "oneOf",
    ),
    (
        "energyml_read_array_window",
        &["array", "count", "preview", "start", "statistics", "uri"],
        &["array", "uri"],
        "",
    ),
];

fn sorted_keys(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    keys.sort();
    keys
}

fn sorted_strings(value: Option<&Value>) -> Vec<String> {
    let mut items: Vec<String> = value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| item.as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default();
    items.sort();
    items
}

#[test]
fn tools_list_is_the_snapshot() {
    let registry = Registry::builtin();
    let listed: Vec<_> = registry.tools().collect();
    assert_eq!(
        listed.iter().map(|tool| tool.name()).collect::<Vec<_>>(),
        SNAPSHOT.iter().map(|row| row.0).collect::<Vec<_>>(),
        "names or their order moved"
    );
    for (tool, (name, properties, required, alternative)) in listed.iter().zip(SNAPSHOT) {
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object", "{name}");
        assert_eq!(
            schema["additionalProperties"], false,
            "{name}: unknown arguments must be refused"
        );
        assert_eq!(
            sorted_keys(&schema["properties"]),
            properties,
            "{name}: its arguments moved"
        );
        assert_eq!(
            sorted_strings(schema.get("required")),
            required,
            "{name}: a required argument moved"
        );
        let found = ["anyOf", "oneOf"]
            .into_iter()
            .find(|key| schema.get(*key).is_some())
            .unwrap_or("");
        assert_eq!(found, alternative, "{name}");
    }
}

#[test]
fn every_tool_is_described_for_the_model_and_annotated() {
    let registry = Registry::builtin();
    for tool in registry.tools() {
        let name = tool.name();
        assert!(!tool.title().is_empty(), "{name}");
        assert!(tool.description().len() > 40, "{name}");
        assert!(
            tool.output_schema().is_none(),
            "{name}: the builtin tools promise no output schema yet"
        );

        let local = tool.annotations(&StoreInfo::new("EPC", false));
        let remote = tool.annotations(&StoreInfo::new("etp", true));
        assert!(
            local.read_only_hint && remote.read_only_hint,
            "{name}: every tool is read-only"
        );
        assert!(!local.open_world_hint, "{name}: a file is a closed world");
        assert!(remote.open_world_hint, "{name}: a remote server is an open one");
        assert_eq!(local.title, tool.title());
        let value = local.to_value();
        assert_eq!(value["readOnlyHint"], true);
        assert_eq!(value["openWorldHint"], false);
    }
}

#[test]
fn a_changed_description_would_not_pass_unnoticed() {
    // The descriptions are the model's instructions. They are not frozen
    // word for word (that would forbid improving them), but their length and
    // the sentence about untrusted data are: a description reduced to a stub
    // or stripped of the warning fails.
    for tool in Registry::builtin().tools() {
        assert!(
            tool.description().contains("data, not instructions"),
            "{}",
            tool.name()
        );
        assert!(
            tool.description().len() < 1_500,
            "{}: a description for a model stays short",
            tool.name()
        );
    }
}

#[test]
fn the_snapshot_can_fail() {
    // A guard that cannot fail proves nothing: the same comparison, against a
    // schema with an argument renamed, must differ.
    let mut schema = Registry::builtin()
        .get("energyml_list_resources")
        .unwrap()
        .input_schema();
    let properties = schema["properties"].as_object_mut().unwrap();
    let limit = properties.remove("limit").unwrap();
    properties.insert("max".into(), limit);
    let row = SNAPSHOT[2];
    assert_ne!(sorted_keys(&schema["properties"]), row.1);
}
