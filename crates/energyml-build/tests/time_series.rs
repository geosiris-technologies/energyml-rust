//! `time-series`: a series of instants, and the parentage that chains two.
//!
//! The form of each assertion comes from the schema's documentation
//! (`CommonTypes.xsd` of the common v2.3 schemas, `TimeSeries`):
//! *“Business Rule: If present TimeStep count must match Time count”*;
//! `Time`, *“The list ordering is used by the time index”*; and
//! `TimeSeriesParentage`, *“the series continues from the parent time
//! series”*. The dates are the test's own.

use energyml_build::operation::{Choices, Identity, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use serde_json::{Value, json};

fn when() -> When {
    When { creation: "2026-09-24T00:00:00Z".into(), originator: "test".into() }
}

fn three() -> Value {
    json!(["2015-06-30T00:00:00Z", "2015-09-30T00:00:00Z", "2016-01-01T00:00:00Z"])
}

fn planned(pieces: &Pieces, choices: Choices) -> Result<Plan, String> {
    Registry::new()
        .plan("time-series", pieces, &choices, &Identity::derived(), &when())
        .map_err(|why| why.to_string())
}

fn parent() -> Pieces {
    let known = Known {
        reference: Some(Ref {
            uuid: "4e0c7b1a-8c1d-5e2f-9a3b-6c7d8e9f0a1b".into(),
            qualified_type: "eml23.TimeSeries".into(),
            title: "the drilling".into(),
            object_version: None,
        }),
        ..Default::default()
    };
    Pieces::empty().with("parent", Source::Object(known))
}

#[test]
fn a_series_keeps_its_instants_in_order_and_its_steps_match_them() {
    let plan = planned(&Pieces::empty(), Choices::new().set("title", "T").set("times", three()).set("timeSteps", true))
        .expect("planned");
    assert_eq!(plan.objects.len(), 1);
    let series = &plan.objects[0].json;
    let times: Vec<&str> = series["Time"].as_array().unwrap().iter().map(|t| t["DateTime"].as_str().unwrap()).collect();
    assert_eq!(times, vec!["2015-06-30T00:00:00Z", "2015-09-30T00:00:00Z", "2016-01-01T00:00:00Z"]);
    // “If present TimeStep count must match Time count”.
    let steps = series["TimeStep"]["Values"].as_array().expect("TimeStep values");
    assert_eq!(steps.len(), times.len(), "{series}");
    assert!(series.get("TimeSeriesParentage").is_none_or(Value::is_null), "no parent was given: {series}");
}

#[test]
fn a_series_continues_its_parent_from_the_index_given() {
    let plan = planned(&parent(), Choices::new().set("title", "T").set("times", three()).set("parentIndex", 2))
        .expect("planned");
    let parentage = &plan.objects[0].json["TimeSeriesParentage"];
    assert_eq!(parentage["HasOverlap"], json!(true), "{parentage}");
    assert_eq!(parentage["ParentTimeIndex"]["Index"], json!(2), "{parentage}");
    assert_eq!(
        parentage["ParentTimeIndex"]["TimeSeries"]["Uuid"],
        json!("4e0c7b1a-8c1d-5e2f-9a3b-6c7d8e9f0a1b"),
        "{parentage}"
    );
}

#[test]
fn instants_out_of_order_or_repeated_are_refused_by_name() {
    // An index names a date by its place in the list: a list out of order
    // gives every index a date nobody meant.
    let backwards = json!(["2016-01-01T00:00:00Z", "2015-06-30T00:00:00Z"]);
    let why = planned(&Pieces::empty(), Choices::new().set("title", "T").set("times", backwards)).unwrap_err();
    assert!(why.contains("2015-06-30T00:00:00Z") && why.contains("increasing"), "{why}");

    let twice = json!(["2016-01-01T00:00:00Z", "2016-01-01T00:00:00Z"]);
    assert!(planned(&Pieces::empty(), Choices::new().set("title", "T").set("times", twice)).is_err());
}

#[test]
fn an_instant_that_is_not_a_utc_date_time_is_refused_not_guessed() {
    let local = json!(["2016-01-01T00:00:00+01:00"]);
    let why = planned(&Pieces::empty(), Choices::new().set("title", "T").set("times", local)).unwrap_err();
    assert!(why.contains("2016-01-01T00:00:00+01:00"), "{why}");
}

#[test]
fn an_index_without_a_parent_and_a_parent_without_an_index_are_refused() {
    let without_parent = planned(&Pieces::empty(), Choices::new().set("title", "T").set("times", three()).set("parentIndex", 1));
    assert!(without_parent.unwrap_err().contains("parent"));
    let without_index = planned(&parent(), Choices::new().set("title", "T").set("times", three()));
    assert!(without_index.unwrap_err().contains("the drilling"));
}

#[test]
fn the_same_series_is_the_same_uuid_and_another_parent_another() {
    let one = planned(&Pieces::empty(), Choices::new().set("title", "T").set("times", three())).unwrap();
    let again = planned(&Pieces::empty(), Choices::new().set("title", "T").set("times", three())).unwrap();
    assert_eq!(one.objects[0].uuid, again.objects[0].uuid);
    let continued = planned(&parent(), Choices::new().set("title", "T").set("times", three()).set("parentIndex", 2)).unwrap();
    assert_ne!(one.objects[0].uuid, continued.objects[0].uuid, "a parent is part of what the series is");
}
