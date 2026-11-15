//! `energyml_domain::activity`: reading an activity and its template in the four
//! releases, the role of each parameter, and what departs from the template.
//!
//! Source: common Technical Reference Guide 2.3 (Energistics), chapter 3,
//! "Activities". Every object is fabricated here from what the guide says, read
//! through `energyml_core::json::read_value`; no file is read. The last line of
//! each test says how to make it fail.

use energyml_core::identity::address_of_parts;
use energyml_core::json::read_value;
use energyml_core::registry::DynObject;
use energyml_domain::activity::{
    ActivityRefusal, ActivityRole, ActivityRecord, ParameterDirection, ParameterIssue, ParameterKind,
    ParameterValue, RoleBasis, RoleRule, RoleRules, TemplateRecord, conformance, descriptor_of, read_activity,
    read_template, roles,
};
use serde_json::{Value, json};

const T: &str = "11111111-1111-4111-8111-111111111111";
const P: &str = "22222222-2222-4222-8222-222222222222";
const A: &str = "33333333-3333-4333-8333-333333333333";
const OBJ: &str = "44444444-4444-4444-8444-444444444444";
const SERIES: &str = "55555555-5555-4555-8555-555555555555";

fn object(value: &Value) -> Box<dyn DynObject> {
    read_value(value, &energyml_schemas::all()).expect("the fabricated object is read").root
}

fn eml23_ref(qualified: &str, uuid: &str) -> Value {
    json!({ "$type": "eml23.DataObjectReference", "Uuid": uuid, "QualifiedType": qualified, "Title": "t" })
}

fn eml23_citation(originator: &str) -> Value {
    json!({ "$type": "eml23.Citation", "Title": "a", "Originator": originator,
            "Creation": "2026-01-01T00:00:00Z", "Format": "x" })
}

fn eml23_activity(originator: &str, parameters: Vec<Value>) -> Value {
    json!({
        "$type": "eml23.Activity", "uuid": A, "schemaVersion": "2.3",
        "Citation": eml23_citation(originator),
        "ActivityDescriptor": eml23_ref("eml23.ActivityTemplate", T),
        "Parameter": parameters,
    })
}

fn param_object(title: &str, reference: Value) -> Value {
    json!({ "$type": "eml23.DataObjectParameter", "Title": title, "IsUncertain": false, "DataObject": reference })
}

fn param_text(title: &str, value: &str) -> Value {
    json!({ "$type": "eml23.StringParameter", "Title": title, "IsUncertain": false, "Value": value })
}

fn param_double(title: &str, value: f64) -> Value {
    json!({ "$type": "eml23.DoubleQuantityParameter", "Title": title, "IsUncertain": false,
            "Value": value, "Uom": "m" })
}

fn param_integer(title: &str, value: i64) -> Value {
    json!({ "$type": "eml23.IntegerQuantityParameter", "Title": title, "IsUncertain": false, "Value": value })
}

fn template_param(title: &str, allowed: &[&str], is_input: bool, is_output: bool, min: i64, max: i64) -> Value {
    json!({ "$type": "eml23.ParameterTemplate", "Title": title, "AllowedKind": allowed,
            "IsInput": is_input, "IsOutput": is_output, "MinOccurs": min, "MaxOccurs": max })
}

fn eml23_template(parameters: Vec<Value>) -> Value {
    json!({
        "$type": "eml23.ActivityTemplate", "uuid": T, "schemaVersion": "2.3",
        "Citation": { "$type": "eml23.Citation", "Title": "fabricant:test", "Originator": "tester",
                      "Creation": "2026-01-01T00:00:00Z" },
        "Parameter": parameters,
    })
}

fn activity_of(value: &Value) -> ActivityRecord {
    read_activity(object(value).as_reflect()).expect("an activity")
}

fn template_of(value: &Value) -> TemplateRecord {
    read_template(object(value).as_reflect()).expect("a template")
}

// common TRG 2.3, section 3.3, p. 42.
#[test]
fn an_eml23_activity_reads_its_template_parent_and_parameters_in_order() {
    let mut value = eml23_activity(
        "tester",
        vec![
            param_object("input", eml23_ref("resqml22.ContinuousProperty", OBJ)),
            param_double("step", 2.5),
            param_integer("count", 3),
            param_text("mode", "idw"),
            json!({ "$type": "eml23.TimeIndexParameter", "Title": "when", "IsUncertain": false,
                    "TimeIndex": { "Index": 4, "TimeSeries": eml23_ref("eml23.TimeSeries", SERIES) } }),
        ],
    );
    value["Parent"] = eml23_ref("eml23.Activity", P);
    let record = activity_of(&value);
    assert_eq!(record.uri, format!("eml:///eml23.Activity({A})"));
    assert_eq!(record.descriptor.as_ref().unwrap().address, format!("eml:///eml23.ActivityTemplate({T})"));
    assert_eq!(record.parent.as_ref().unwrap().address, format!("eml:///eml23.Activity({P})"));
    assert_eq!(record.originator.as_deref(), Some("tester"));
    let kinds: Vec<_> = record.parameters.iter().map(|p| p.value.kind()).collect();
    assert_eq!(
        kinds,
        vec![
            Some(ParameterKind::DataObject),
            Some(ParameterKind::Double),
            Some(ParameterKind::Integer),
            Some(ParameterKind::String),
            Some(ParameterKind::Timestamp),
        ]
    );
    let titles: Vec<&str> = record.parameters.iter().map(|p| p.title.as_str()).collect();
    assert_eq!(titles, vec!["input", "step", "count", "mode", "when"]);
    assert_eq!(record.parameters[1].value, ParameterValue::Double { value: 2.5, uom: "m".into() });
    assert_eq!(record.parameters[2].value, ParameterValue::Integer { value: 3 });
    assert_eq!(record.parameters[3].value, ParameterValue::Text("idw".into()));
    assert!(matches!(record.parameters[4].value, ParameterValue::TimeIndex { index: 4, .. }));
    assert_eq!(record.parameters[0].is_uncertain, Some(false));
    // Break it: do not read `Parent` (the `parent` assertion fails), or swap two parameters.
}

// common TRG 2.3, section 3.3, p. 42; resqml 2.0.1 `activities.rs` (FloatingPointQuantityParameter).
#[test]
fn a_resqml201_activity_reads_like_its_eml23_twin() {
    let reference = |content: &str, uuid: &str| {
        json!({ "$type": "eml20.DataObjectReference", "ContentType": content, "Title": "t", "UUID": uuid })
    };
    let value = json!({
        "$type": "resqml20.obj_Activity", "uuid": A, "schemaVersion": "2.0",
        "Citation": { "$type": "eml20.Citation", "Title": "a", "Originator": "tester",
                      "Creation": "2026-01-01T00:00:00Z", "Format": "x" },
        "ActivityDescriptor": reference("application/x-resqml+xml;version=2.0;type=obj_ActivityTemplate", T),
        "Parameter": [
            { "$type": "resqml20.DataObjectParameter", "Title": "input",
              "DataObject": reference("application/x-resqml+xml;version=2.0;type=obj_ContinuousProperty", OBJ) },
            { "$type": "resqml20.FloatingPointQuantityParameter", "Title": "step", "Value": 2.5, "Uom": "m" },
            { "$type": "resqml20.IntegerQuantityParameter", "Title": "count", "Value": 3 },
            { "$type": "resqml20.StringParameter", "Title": "mode", "Value": "idw" },
        ],
    });
    let record = activity_of(&value);
    let titles: Vec<&str> = record.parameters.iter().map(|p| p.title.as_str()).collect();
    assert_eq!(titles, vec!["input", "step", "count", "mode"]);
    assert_eq!(record.parameters[1].value, ParameterValue::Double { value: 2.5, uom: "m".into() });
    assert_eq!(record.parameters[2].value, ParameterValue::Integer { value: 3 });
    assert_eq!(record.parameters[3].value, ParameterValue::Text("idw".into()));
    assert!(record.parameters.iter().all(|p| p.is_uncertain.is_none()), "resqml 2.0.1 has no IsUncertain");
    assert_eq!(
        record.descriptor.unwrap().qualified_type.as_deref(),
        Some("resqml20.obj_ActivityTemplate"),
        "the qualified type is derived from the ContentType"
    );
    // Break it: read the value by the class name, and `FloatingPointQuantityParameter` is unknown.
}

// common TRG 2.3, section 3.3, p. 42.
#[test]
fn an_eml21_and_an_eml22_activity_read_alike() {
    for (release, prefix) in [("2.1", "eml21"), ("2.2", "eml22")] {
        let reference = |kind: &str, uuid: &str| {
            json!({ "$type": format!("{prefix}.DataObjectReference"),
                    "ContentType": format!("application/x-eml+xml;version={release};type={kind}"),
                    "Title": "t", "Uuid": uuid })
        };
        let value = json!({
            "$type": format!("{prefix}.Activity"), "uuid": A, "schemaVersion": release,
            "Citation": { "$type": format!("{prefix}.Citation"), "Title": "a", "Originator": "tester",
                          "Creation": "2026-01-01T00:00:00Z", "Format": "x" },
            "ActivityDescriptor": reference("ActivityTemplate", T),
            "Parameter": [
                { "$type": format!("{prefix}.DoubleQuantityParameter"), "Title": "step", "Value": 2.5, "Uom": "m" },
                { "$type": format!("{prefix}.IntegerQuantityParameter"), "Title": "count", "Value": 3 },
                { "$type": format!("{prefix}.StringParameter"), "Title": "mode", "Value": "idw" },
            ],
        });
        let record = activity_of(&value);
        assert_eq!(record.parameters[0].value, ParameterValue::Double { value: 2.5, uom: "m".into() }, "{release}");
        assert_eq!(record.parameters[1].value, ParameterValue::Integer { value: 3 }, "{release}");
        assert_eq!(record.parameters[2].value, ParameterValue::Text("idw".into()), "{release}");
        assert!(record.parameters.iter().all(|p| p.is_uncertain.is_none()), "{release}");
    }
    // Break it: read `IsUncertain` as `false` when absent, and `is_uncertain` is `Some(false)`.
}

// common TRG 2.3, section 3.10, p. 49 (AllowedKind).
#[test]
fn a_template_parameter_without_allowed_kind_allows_every_kind() {
    let template = template_of(&eml23_template(vec![template_param("x", &[], true, false, 0, -1)]));
    assert!(template.parameters[0].allowed.is_empty());
    let activity = activity_of(&eml23_activity("tester", vec![param_text("x", "a"), param_integer("x", 1)]));
    let issues = conformance(&activity, Some(&template));
    assert!(
        !issues.iter().any(|issue| matches!(issue, ParameterIssue::KindDiffers { .. })),
        "{issues:?}"
    );
    // Break it: treat an empty `AllowedKind` as "no kind allowed", and `KindDiffers` appears.
}

// triangulation: two components, the writer of `provenance` and this reader.
#[test]
fn what_the_writer_writes_the_reader_reads_back() {
    use energyml_domain::provenance::{Parameter, Record, activity};
    let record = Record {
        recipe: "offset",
        uuid: A,
        creation: "2026-09-07T00:00:00Z",
        title: None,
        output_title: Some("Top"),
        descriptor: eml23_ref("eml23.ActivityTemplate", T),
        parameters: vec![
            Parameter::Double { title: "dx".into(), value: 12.5, uom: Some("m".into()) },
            Parameter::Text { title: "mode".into(), value: "line".into() },
            Parameter::Object { title: "input".into(), reference: eml23_ref("resqml22.ContinuousProperty", OBJ) },
            Parameter::Integer { title: "count".into(), value: 7 },
        ],
    };
    let written = activity(&record).expect("written");
    let read = activity_of(&written);
    assert_eq!(read.originator.as_deref(), Some("energyml-rust"));
    assert_eq!(read.parameters[0].value, ParameterValue::Double { value: 12.5, uom: "m".into() });
    assert_eq!(read.parameters[1].value, ParameterValue::Text("line".into()));
    assert!(matches!(read.parameters[2].value, ParameterValue::Object(_)));
    assert_eq!(read.parameters[3].value, ParameterValue::Integer { value: 7 });
    assert_eq!(read.parameters[3].title, "count");
    // Break it: read an `Integer` as a `Double`.
}

// 08 section 10, what must be refused.
#[test]
fn a_property_is_not_an_activity() {
    let property = json!({
        "$type": "resqml22.ContinuousProperty", "uuid": OBJ, "schemaVersion": "2.2",
        "Citation": { "$type": "eml23.Citation", "Title": "p", "Originator": "tester",
                      "Creation": "2026-01-01T00:00:00Z" },
    });
    let loaded = object(&property);
    let refusal = read_activity(loaded.as_reflect()).expect_err("refused");
    assert_eq!(refusal, ActivityRefusal::NotAnActivity { qualified_type: "resqml22.ContinuousProperty".into() });
    assert!(matches!(read_template(loaded.as_reflect()), Err(ActivityRefusal::NotATemplate { .. })));
    // Break it: accept any object, and the refusal is not returned.
}

// common TRG 2.3, section 3.10, p. 49 (IsInput, IsOutput).
#[test]
fn a_template_reads_its_four_directions() {
    let template = template_of(&eml23_template(vec![
        template_param("i", &["dataObject"], true, false, 0, 1),
        template_param("o", &["dataObject"], false, true, 0, 1),
        template_param("b", &["dataObject"], true, true, 0, 1),
        template_param("n", &["dataObject"], false, false, 0, 1),
    ]));
    let directions: Vec<_> = template.parameters.iter().map(|p| p.direction).collect();
    assert_eq!(
        directions,
        vec![ParameterDirection::Input, ParameterDirection::Output, ParameterDirection::Both, ParameterDirection::Neither]
    );
    assert_eq!(template.title.as_deref(), Some("fabricant:test"));
    // Break it: confuse `Both` and `Input` (read `IsInput` only).
}

// identity.rs: one rule for versions (`version_fit`).
#[test]
fn the_descriptor_is_found_by_uuid_and_version() {
    let template = |version: &str| TemplateRecord {
        uri: address_of_parts(Some("eml23.ActivityTemplate"), T, Some(version)),
        title: None,
        parameters: Vec::new(),
    };
    let templates = vec![template("1"), template("2")];
    let activity_naming = |version: Option<&str>| {
        let mut record = activity_of(&eml23_activity("tester", vec![param_text("x", "a")]));
        record.descriptor.as_mut().unwrap().address = address_of_parts(Some("eml23.ActivityTemplate"), T, version);
        record
    };
    let found = descriptor_of(&activity_naming(Some("2")), &templates).expect("version 2");
    assert_eq!(found.uri, templates[1].uri);
    let found = descriptor_of(&activity_naming(None), &templates).expect("the first");
    assert_eq!(found.uri, templates[0].uri);
    assert!(descriptor_of(&activity_naming(Some("3")), &templates).is_none(), "3 contradicts both");
    // Break it: compare the addresses as text, and the version 2 is not found.
}

// common TRG 2.3, section 3.10, p. 49.
#[test]
fn the_template_gives_the_role_in_four_states() {
    let template = template_of(&eml23_template(vec![
        template_param("i", &["dataObject"], true, false, 0, 1),
        template_param("o", &["dataObject"], false, true, 0, 1),
        template_param("b", &["dataObject"], true, true, 0, 1),
        template_param("n", &["dataObject"], false, false, 0, 1),
    ]));
    let reference = eml23_ref("resqml22.ContinuousProperty", OBJ);
    let activity = activity_of(&eml23_activity(
        "tester",
        ["i", "o", "b", "n"].iter().map(|title| param_object(title, reference.clone())).collect(),
    ));
    let given = roles(&activity, Some(&template), &RoleRules::empty());
    let found: Vec<_> = given.iter().map(|r| r.role).collect();
    assert_eq!(found, vec![ActivityRole::Input, ActivityRole::Output, ActivityRole::Both, ActivityRole::Neither]);
    assert!(given.iter().all(|r| r.basis == RoleBasis::Template));
    // Break it: read `IsOutput` alone, and `Both` comes out `Output`.
}

// 08 section 2.3, `RoleRefusal::Undeclared`.
#[test]
fn without_template_or_rule_the_role_is_undeclared() {
    let activity = activity_of(&eml23_activity("tester", vec![param_object("input", eml23_ref("eml23.Activity", P))]));
    let given = roles(&activity, None, &RoleRules::empty());
    assert_eq!(given[0].role, ActivityRole::Undeclared);
    assert_eq!(given[0].basis, RoleBasis::None);
    // Break it: guess `Input` from the title.
}

#[test]
fn the_input_output_rule_is_only_for_energyml_rust() {
    let reference = eml23_ref("resqml22.ContinuousProperty", OBJ);
    let ours = activity_of(&eml23_activity("energyml-rust", vec![param_object("input", reference.clone())]));
    let theirs = activity_of(&eml23_activity("FESAPI", vec![param_object("input", reference)]));
    let rules = RoleRules::new();
    let given = roles(&ours, None, &rules);
    assert_eq!(given[0].role, ActivityRole::Input);
    assert_eq!(given[0].basis, RoleBasis::Rule("energyml-build:input-output"));
    assert_eq!(roles(&theirs, None, &rules)[0].role, ActivityRole::Undeclared);
    // Break it: do not look at the originator, and the second assertion fails.
}

struct TestRule;
impl RoleRule for TestRule {
    fn id(&self) -> &'static str {
        "test"
    }
    fn role(&self, _: &ActivityRecord, parameter: &energyml_domain::activity::ActivityParameter) -> Option<ActivityRole> {
        (parameter.title == "input").then_some(ActivityRole::Neither)
    }
}

// ADR-0006: the caller's entries are tried first.
#[test]
fn a_callers_rule_is_tried_before_the_built_in() {
    let activity = activity_of(&eml23_activity(
        "energyml-rust",
        vec![param_object("input", eml23_ref("resqml22.ContinuousProperty", OBJ))],
    ));
    let mut rules = RoleRules::new();
    rules.add(Box::new(TestRule));
    assert_eq!(rules.ids(), vec!["test", "energyml-build:input-output"]);
    let given = roles(&activity, None, &rules);
    assert_eq!(given[0].role, ActivityRole::Neither);
    assert_eq!(given[0].basis, RoleBasis::Rule("test"));
    // Break it: push the caller's rule last, and `Input` wins.
}

// common TRG 2.3, section 3.4, p. 43.
#[test]
fn a_sub_activity_of_the_template_is_a_sub_activity() {
    let template = template_of(&eml23_template(vec![template_param("etape-1", &["subActivity"], false, false, 0, 1)]));
    let activity = activity_of(&eml23_activity("tester", vec![param_object("etape-1", eml23_ref("eml23.Activity", P))]));
    let given = roles(&activity, Some(&template), &RoleRules::new());
    assert_eq!(given[0].role, ActivityRole::SubActivity);
    assert_eq!(given[0].basis, RoleBasis::Template);
    // Break it: ignore `subActivity`, and the role is `Neither`.
}

#[test]
fn the_template_wins_over_a_rule() {
    let template = template_of(&eml23_template(vec![template_param("input", &["dataObject"], false, false, 0, 1)]));
    let activity = activity_of(&eml23_activity(
        "energyml-rust",
        vec![param_object("input", eml23_ref("resqml22.ContinuousProperty", OBJ))],
    ));
    let given = roles(&activity, Some(&template), &RoleRules::new());
    assert_eq!(given[0].role, ActivityRole::Neither);
    assert_eq!(given[0].basis, RoleBasis::Template);
    // Break it: try the rules before the template, and `Input` comes out.
}

// common TRG 2.3, section 3.10, p. 49 (AllowedKind).
#[test]
fn an_integer_written_as_a_double_is_signalled_and_still_read() {
    let template = template_of(&eml23_template(vec![template_param("nullValue", &["integer"], true, false, 0, 1)]));
    let activity = activity_of(&eml23_activity("tester", vec![param_double("nullValue", 3.0)]));
    assert_eq!(activity.parameters.len(), 1, "the activity is read");
    assert_eq!(
        conformance(&activity, Some(&template)),
        vec![ParameterIssue::KindDiffers {
            position: 0,
            title: "nullValue".into(),
            declared: vec![ParameterKind::Integer],
            written: ParameterKind::Double,
        }]
    );
    // Break it: refuse the activity, or drop the kind check.
}

// common TRG 2.3, section 3.10, p. 49 (MinOccurs).
#[test]
fn min_occurs_not_held_is_too_few() {
    let template = template_of(&eml23_template(vec![template_param("output", &["dataObject"], false, true, 1, 1)]));
    let activity = activity_of(&eml23_activity("tester", vec![param_text("other", "a")]));
    let issues = conformance(&activity, Some(&template));
    assert!(issues.contains(&ParameterIssue::TooFew { title: "output".into(), min: 1, found: 0 }), "{issues:?}");
    // Break it: ignore `MinOccurs`.
}

// common TRG 2.3, section 3.1, p. 40.
#[test]
fn a_title_the_template_lacks_is_not_in_template() {
    let template = template_of(&eml23_template(vec![template_param("crs", &["dataObject"], true, false, 0, 1)]));
    let activity = activity_of(&eml23_activity("tester", vec![param_object("input", eml23_ref("eml23.Activity", P))]));
    assert_eq!(
        conformance(&activity, Some(&template)),
        vec![ParameterIssue::NotInTemplate { position: 0, title: "input".into() }]
    );
    assert_eq!(conformance(&activity, None), vec![ParameterIssue::TemplateAbsent]);
    // Break it: check the kind only.
}

// common TRG 2.3, section 3.4, p. 43.
#[test]
fn a_root_naming_its_children_by_data_object_conforms() {
    let template = template_of(&eml23_template(vec![template_param("etape-1", &["subActivity"], false, false, 0, -1)]));
    let child = activity_of(&eml23_activity("tester", vec![param_object("etape-1", eml23_ref("eml23.Activity", P))]));
    assert!(conformance(&child, Some(&template)).is_empty());
    let wrong = activity_of(&eml23_activity(
        "tester",
        vec![param_object("etape-1", eml23_ref("resqml22.ContinuousProperty", OBJ))],
    ));
    let issues = conformance(&wrong, Some(&template));
    assert!(matches!(issues.as_slice(), [ParameterIssue::KindDiffers { .. }]), "{issues:?}");
    // Break it: accept any `DataObject` under `subActivity`, and the second assertion fails.
}

// common TRG 2.3, section 3.10, p. 49 (MaxOccurs).
#[test]
fn minus_one_is_unbounded() {
    let template = template_of(&eml23_template(vec![template_param("x", &["string"], true, false, 0, -1)]));
    let five = (0..5).map(|n| param_text("x", &n.to_string())).collect();
    let activity = activity_of(&eml23_activity("tester", five));
    assert!(conformance(&activity, Some(&template)).is_empty());
    let bounded = template_of(&eml23_template(vec![template_param("x", &["string"], true, false, 0, 2)]));
    assert_eq!(
        conformance(&activity, Some(&bounded)),
        vec![ParameterIssue::TooMany { title: "x".into(), max: 2, found: 5 }]
    );
    // Break it: compare `found > -1`, and the first assertion fails.
}
