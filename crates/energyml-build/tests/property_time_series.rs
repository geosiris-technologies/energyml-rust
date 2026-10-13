//! `property-time-series` — a property through a time series, in the two forms
//! RESQML 2.2 has for it (step 9 of the reference model;
//! the property series).
//!
//! The form of the assertions comes from the specification and the reference-model description, not
//! from what this crate writes:
//!
//! - `AbstractProperty` states `Time` (`GeologicTime`, 0..1) and
//!   `TimeOrIntervalSeries` (0..1), and no time index — RESQML Technical
//!   Reference Guide v2.2 §7.1 p. 222;
//! - a property naming a series holds **every** step of it, time the slowest
//!   dimension — TRG v2.2 §7, the note of the diagram p. 221: *“[TimeCount] …
//!   [IndexableElementCount] …”*; so one property per step does **not** name
//!   the series;
//! - a time index names an instant by its place in the series' list — *“The
//!   list ordering is used by the time index”*, Energistics common Technical
//!   Reference Guide v2.3 §11.14 p. 297;
//! - under `UseInterval`, `n` instants give `n − 1` values (answered
//!   25/09);
//! - an invented value says so and cites its source (ADR-0019).
//!
//! The numbers — the snapshot, the dates, the change per year — are the tests'
//! own. The dates' distances are counted **from the calendar**, in days, and
//! never by the date parser the operation uses.
//!
//! # What would have to break to make it turn red
//!
//! Laying the time as the fast dimension (`time_law::lay` backwards); dating a
//! property per step by an instant other than its own (shifting the index); making
//! the properties per step name the series; counting `n` intervals for `n`
//! instants; taking the wrong midpoint; accepting a base already in time,
//! qualified, without values, a series that does not increase, a law without a source;
//! dropping the values from the seed.

use serde_json::json;

use energyml_build::operation::{Choices, Identity, NewObject, Numbers, Plan, Registry, When};
use energyml_build::pieces::{Known, KnownProperty, Pieces, Ref, Source};

fn when() -> When {
    When {
        creation: "2026-09-25T00:00:00Z".into(),
        originator: "test".into(),
    }
}

const BASE: &str = "baba0000-0000-4000-8000-00000000ba5e";
const FRAME: &str = "f0f0f0f0-0000-4000-8000-0000000000f0";
const KIND: &str = "e703be06-7c3f-4d1b-8a4c-2d2d4e6c7a10";
const SERIES: &str = "5e5e5e5e-0000-4000-8000-000000005e5e";

/// The snapshot: a pressure in bar at five nodes, the first above the water
/// table — no value there.
fn snapshot() -> Vec<f64> {
    vec![f64::NAN, 0.5, 1.5, 2.5, 3.5]
}

/// The series of the tests, and the days from its first instant to each —
/// 2016 is a leap year: 366 days to 2017, 731 to 2018.
const INSTANTS: [&str; 3] = [
    "2016-01-01T00:00:00Z",
    "2017-01-01T00:00:00Z",
    "2018-01-01T00:00:00Z",
];
const DAYS: [f64; 3] = [0.0, 366.0, 731.0];

/// What the boundary resolves of a pressure on a frame (`read::property_of`).
fn facts(values: Vec<f64>) -> KnownProperty {
    KnownProperty {
        supporting: Some(Ref::new(
            FRAME,
            "resqml22.WellboreFrameRepresentation",
            "GTH-1 — frame",
        )),
        indexable: "nodes".to_string(),
        value_count: vec![1],
        timed: false,
        realizations: false,
        uom: Some("bar".to_string()),
        kind: Some(Ref::new(KIND, "eml23.PropertyKind", "pressure")),
        lookup: None,
        null_value: None,
        facets: Vec::new(),
        values: Ok(energyml_domain::values::Values::Floating(values)),
    }
}

fn base_with(facts: KnownProperty) -> Source {
    Source::Object(Known {
        reference: Some(Ref::new(BASE, "resqml22.ContinuousProperty", "GTH-1 — pressure")),
        property: Some(facts),
        ..Known::default()
    })
}

fn series_with(instants: &[&str]) -> Source {
    Source::Object(Known {
        reference: Some(Ref::new(SERIES, "eml23.TimeSeries", "operation")),
        instants: Some(instants.iter().map(|one| one.to_string()).collect()),
        ..Known::default()
    })
}

fn bag() -> Pieces {
    Pieces::empty()
        .with("base", base_with(facts(snapshot())))
        .with("series", series_with(&INSTANTS))
}

/// The law as the tests state it: half a bar less per year.
fn steps() -> Choices {
    Choices::new()
        .set("form", "steps")
        .set("change", -0.5)
        .set("source", "synthetic: the test's own values")
}

fn series(use_interval: bool) -> Choices {
    Choices::new()
        .set("form", "series")
        .set("useInterval", use_interval)
        .set("change", -0.5)
        .set("source", "synthetic: the test's own values")
}

fn planned(pieces: &Pieces, choices: &Choices) -> Result<Plan, String> {
    planned_as(pieces, choices, &Identity::derived())
}

fn planned_as(pieces: &Pieces, choices: &Choices, identity: &Identity) -> Result<Plan, String> {
    Registry::new()
        .plan("property-time-series", pieces, choices, identity, &when())
        .map_err(|why| why.to_string())
}

/// The values the plan writes for a property, and the dimensions it declares.
fn written(plan: &Plan, property: &NewObject) -> (Vec<f64>, Vec<i64>) {
    let path = format!("/resqml22/{}/values", property.uuid);
    let dataset = plan
        .datasets
        .iter()
        .find(|one| one.path == path)
        .expect("the property's values");
    match &dataset.values {
        Numbers::F64(values) => (values.clone(), dataset.dimensions.clone()),
        other => panic!("not floating point: {other:?}"),
    }
}

/// The drift after `days`, from the calendar: −0.5 bar per Julian year.
fn drift(days: f64) -> f64 {
    -0.5 * days / 365.25
}

fn same(a: f64, b: f64) -> bool {
    (a.is_nan() && b.is_nan()) || (a - b).abs() <= 1e-9 * (1.0 + b.abs())
}

// -- the steps: one property per instant, dated by its own ---------------------------

#[test]
fn each_step_is_a_property_dated_by_its_own_instant_and_naming_no_series() {
    let plan = planned(&bag(), &steps()).expect("a plan");
    assert_eq!(plan.objects.len(), 3, "one property per instant");
    for (index, property) in plan.objects.iter().enumerate() {
        assert_eq!(
            property.json["Time"]["DateTime"],
            json!(INSTANTS[index]),
            "property {index} is dated by instant {index}"
        );
        assert!(
            property.json.get("TimeOrIntervalSeries").is_none(),
            "a property per step names no series: {}",
            property.json
        );
        assert_eq!(property.json["SupportingRepresentation"]["Uuid"], json!(FRAME));
        assert_eq!(property.json["PropertyKind"]["Uuid"], json!(KIND));
        assert_eq!(property.json["Uom"], json!("bar"));
        assert_eq!(property.json["IndexableElement"], json!("nodes"));
        let (values, dimensions) = written(&plan, property);
        assert_eq!(dimensions, vec![5], "one value per node, no time dimension");
        for (node, value) in values.iter().enumerate() {
            let expected = snapshot()[node] + drift(DAYS[index]);
            assert!(
                same(*value, expected),
                "step {index}, node {node}: {value} where {expected}"
            );
        }
    }
    assert_eq!(plan.objects[1].title, "GTH-1 — pressure — 2017-01-01T00:00:00Z");
}

// -- the series: one property, time the slowest dimension -------------------------

#[test]
fn a_series_is_one_property_with_time_the_slowest_dimension() {
    let plan = planned(&bag(), &series(false)).expect("a plan");
    assert_eq!(plan.objects.len(), 1);
    let property = &plan.objects[0];
    let timed = &property.json["TimeOrIntervalSeries"];
    assert_eq!(timed["UseInterval"], json!(false));
    assert_eq!(timed["TimeSeries"]["Uuid"], json!(SERIES));
    assert!(
        property.json.get("Time").is_none(),
        "a series is no single instant"
    );
    assert_eq!(
        property.json["ValueCountPerIndexableElement"],
        json!([1]),
        "time is no component"
    );
    let (values, dimensions) = written(&plan, property);
    assert_eq!(
        dimensions,
        vec![3, 5],
        "[TimeCount][IndexableElementCount], the slowest first"
    );
    assert_eq!(
        property.json["ValuesForPatch"][0]["Values"]["ExternalDataArrayPart"][0]["Count"],
        json!([3, 5]),
        "the document declares what the dataset holds"
    );
    for step in 0..3 {
        for node in 0..5 {
            let expected = snapshot()[node] + drift(DAYS[step]);
            assert!(
                same(values[step * 5 + node], expected),
                "step {step}, node {node}"
            );
        }
    }
}

#[test]
fn over_intervals_n_instants_give_n_minus_one_means() {
    let plan = planned(&bag(), &series(true)).expect("a plan");
    let property = &plan.objects[0];
    assert_eq!(property.json["TimeOrIntervalSeries"]["UseInterval"], json!(true));
    let (values, dimensions) = written(&plan, property);
    assert_eq!(dimensions, vec![2, 5], "three dates give two values");
    // The mean of a linear drift over an interval is its value at the middle:
    // 183 days in for 2016, 366 + 182.5 for 2017.
    for (step, middle) in [183.0, 548.5].into_iter().enumerate() {
        for node in 0..5 {
            let expected = snapshot()[node] + drift(middle);
            assert!(
                same(values[step * 5 + node], expected),
                "interval {step}, node {node}"
            );
        }
    }
    assert!(
        plan.says
            .iter()
            .any(|note| note.key == "series" && note.detail.contains("3 instants make 2 values"))
    );
}

#[test]
fn an_anchor_moves_the_instant_the_snapshot_holds_at() {
    // Anchored at the last instant, the first step is a year and a day before it.
    let plan = planned(&bag(), &series(false).set("anchor", "2018-01-01T00:00:00Z")).expect("a plan");
    let (values, _) = written(&plan, &plan.objects[0]);
    assert!(same(values[5 + 1], 0.5 + drift(366.0 - 731.0)));
    assert!(same(values[2 * 5 + 1], 0.5), "the snapshot itself, at its anchor");
    let why = planned(&bag(), &series(false).set("anchor", "01/01/2018")).expect_err("no date-time");
    assert!(why.contains("anchor"), "{why}");
}

// -- what is said of what is laid -----------------------------------------------------

#[test]
fn every_property_says_it_is_synthetic_and_cites_its_law_and_source() {
    for choices in [steps(), series(true)] {
        let plan = planned(&bag(), &choices.set("source", "a sourced law — synthetic")).expect("a plan");
        for property in &plan.objects {
            let extension = &property.json["ExtensionNameValue"][0];
            assert_eq!(extension["Name"], json!("synthetic"));
            assert_eq!(extension["Value"]["_"], json!("true"));
            let description = property.json["Citation"]["Description"]
                .as_str()
                .expect("a description");
            assert!(description.contains("not measured"), "{description}");
            assert!(description.contains("a sourced law — synthetic"), "{description}");
            assert!(
                description.contains("GTH-1 — pressure"),
                "the base is named: {description}"
            );
        }
        assert_eq!(
            plan.says
                .iter()
                .find(|note| note.key == "empty")
                .map(|note| note.detail.starts_with("1 element")),
            Some(true)
        );
    }
}

// -- what is refused, by name --------------------------------------------------------

#[test]
fn a_law_without_a_source_is_refused() {
    let why = planned(&bag(), &steps().set("source", "  ")).expect_err("no source");
    assert!(why.contains("source"), "{why}");
}

#[test]
fn a_base_that_cannot_be_laid_through_time_is_refused_by_what_it_is() {
    let refused = |facts: KnownProperty, word: &str| {
        let bag = Pieces::empty()
            .with("base", base_with(facts))
            .with("series", series_with(&INSTANTS));
        let why = planned(&bag, &steps()).expect_err(word);
        assert!(why.contains(word), "{word}: {why}");
    };
    refused(
        KnownProperty {
            timed: true,
            ..facts(snapshot())
        },
        "already indexed in time",
    );
    refused(
        KnownProperty {
            realizations: true,
            ..facts(snapshot())
        },
        "realizations",
    );
    refused(
        KnownProperty {
            value_count: vec![3],
            ..facts(snapshot())
        },
        "vector",
    );
    refused(
        KnownProperty {
            facets: vec![json!({ "Facet": "P10" })],
            ..facts(snapshot())
        },
        "facet",
    );
    refused(
        KnownProperty {
            values: Err("no backend".to_string()),
            ..facts(snapshot())
        },
        "no backend",
    );
    refused(
        KnownProperty {
            uom: None,
            ..facts(snapshot())
        },
        "unit",
    );

    // And an object that is no continuous property, at the offer.
    let registry = Registry::new();
    let operation = registry.get("property-time-series").expect("in the registry");
    let wrong = Pieces::empty()
        .with(
            "base",
            Source::Object(Known {
                reference: Some(Ref::new(BASE, "resqml22.DiscreteProperty", "codes")),
                ..Known::default()
            }),
        )
        .with("series", series_with(&INSTANTS));
    assert!(matches!(
        operation.fitness(&wrong),
        energyml_build::operation::Fitness::Unfit(_)
    ));
}

#[test]
fn a_series_whose_instants_are_not_known_or_do_not_increase_is_refused() {
    let unread = Pieces::empty().with("base", base_with(facts(snapshot()))).with(
        "series",
        Source::Object(Known {
            reference: Some(Ref::new(SERIES, "eml23.TimeSeries", "operation")),
            ..Known::default()
        }),
    );
    let why = planned(&unread, &steps()).expect_err("no instants");
    assert!(why.contains("not read"), "{why}");

    let back = Pieces::empty()
        .with("base", base_with(facts(snapshot())))
        .with("series", series_with(&[INSTANTS[1], INSTANTS[0]]));
    let why = planned(&back, &steps()).expect_err("decreasing");
    assert!(why.contains("instant 2"), "{why}");

    let one = Pieces::empty()
        .with("base", base_with(facts(snapshot())))
        .with("series", series_with(&INSTANTS[..1]));
    let why = planned(&one, &series(true)).expect_err("no interval");
    assert!(why.contains("from one instant to the next"), "{why}");
    assert_eq!(
        planned(&one, &series(false))
            .expect("one instant, one step")
            .objects
            .len(),
        1
    );
}

// -- identity -----------------------------------------------------------------------

#[test]
fn the_same_inputs_give_the_same_uuids_and_another_change_other_properties() {
    let uuids = |plan: &Plan| {
        plan.objects
            .iter()
            .map(|one| one.uuid.clone())
            .collect::<Vec<_>>()
    };
    let first = uuids(&planned(&bag(), &steps()).expect("a plan"));
    assert_eq!(first.len(), 3);
    assert_eq!(first, uuids(&planned(&bag(), &steps()).expect("a plan")));
    let steeper = uuids(&planned(&bag(), &steps().set("change", -0.6)).expect("a plan"));
    // The first step holds the snapshot itself at any change: it is the same
    // property, and the others are new ones.
    assert_eq!(steeper[0], first[0]);
    assert!(
        steeper[1..].iter().all(|uuid| !first.contains(uuid)),
        "{first:?} {steeper:?}"
    );
    let reworded =
        uuids(&planned(&bag(), &steps().set("source", "synthetic, said otherwise")).expect("a plan"));
    assert_eq!(first, reworded, "a reworded source rewrites the same properties");

    // The two forms never share an object.
    let one = uuids(&planned(&bag(), &series(false)).expect("a plan"));
    assert!(!first.contains(&one[0]));
}

#[test]
fn given_uuids_are_handed_out_one_per_property() {
    let given: Vec<String> = (1..=3)
        .map(|k| format!("0a0a0a0a-0000-4000-8000-00000000000{k}"))
        .collect();
    let plan = planned_as(&bag(), &steps(), &Identity::Given(given.clone())).expect("a plan");
    assert_eq!(
        plan.objects
            .iter()
            .map(|one| one.uuid.clone())
            .collect::<Vec<_>>(),
        given
    );
}

// -- read back: the instant a property names is the series' own ------------------------

/// Plays a small package — a well, its hydrostatic pressure, a series, and both
/// forms laid through it — writes it, and reads it back **independently**: the
/// instants from the series object as the file holds it, the values from the
/// HDF5 file through `energyml-domain`, and each expected value from the
/// calendar.
#[cfg(feature = "hdf5")]
#[test]
fn read_back_each_step_resolves_to_its_instant_of_the_series() {
    use std::path::Path;
    use std::sync::Arc;

    use energyml_build::entries::Reader;
    use energyml_build::recipe::{self, Recipe};
    use energyml_build::source::Source as Where;
    use energyml_storage::EpcStore;

    struct NoFile;
    impl Reader for NoFile {
        fn text(&mut self, source: &Where) -> Result<(String, Option<String>), String> {
            Err(format!("{source:?}: this test reads no file"))
        }
        fn package(&mut self, path: &str) -> Result<&EpcStore, String> {
            Err(format!("opening {path}: the recipe names no package"))
        }
    }

    let text = r#"
titre = "PTS"
creation = "2026-09-25T00:00:00Z"
auteur = "test"
donnees = "pts.h5"
workflow = "workflow:pts"

[[etape]]
nom = "gth"
operation = "well-from-plan"
set = { sourceEpsg = 2154, project = 2154, vertical = 5720, title = "GTH-T", headX = 651800.0, headY = 6861200.0, ground = 50.0, sections = ["hold to 60"], step = 10.0 }

[[etape]]
nom = "pression"
operation = "well-logs"
entrees = [ { creneau = "frames", etape = "gth", type = "WellboreFrameRepresentation" } ]
set = { law = "hydrostatic", waterLevel = 40.0, datumHeight = 0.0, source = "synthetic: the test's own values" }

[[etape]]
nom = "temps"
operation = "time-series"
set = { title = "operation", times = ["2016-01-01T00:00:00Z", "2017-01-01T00:00:00Z", "2018-01-01T00:00:00Z"] }

[[etape]]
nom = "par-annee"
operation = "property-time-series"
entrees = [
  { creneau = "base", etape = "pression", type = "ContinuousProperty" },
  { creneau = "series", etape = "temps", type = "TimeSeries" },
]
set = { form = "steps", change = -0.5, source = "synthetic: the test's own values" }

[[etape]]
nom = "moyennes"
operation = "property-time-series"
entrees = [
  { creneau = "base", etape = "pression", type = "ContinuousProperty" },
  { creneau = "series", etape = "temps", type = "TimeSeries" },
]
set = { form = "series", useInterval = true, change = -0.5, source = "synthetic: the test's own values" }
"#;
    let recipe = Recipe::parse(text).expect("the recipe reads");
    let dir = std::env::temp_dir().join(format!(
        "energyml-build-property-time-series-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("pts.epc");
    let schemas = Arc::new(energyml_schemas::all());
    recipe::run(
        &recipe,
        Path::new("."),
        &Registry::new(),
        schemas.clone(),
        &out,
        &mut NoFile,
    )
    .unwrap_or_else(|stop| panic!("{stop}"));
    let store = EpcStore::open(&out, schemas).expect("the package reads");

    let json_of =
        |uri: &str| energyml_core::json::write_value(store.get_object(uri).expect("it reads").as_reflect());
    let values_of = |uri: &str| {
        let object = store.get_object(uri).expect("it reads");
        let read = energyml_domain::property::read(object.as_reflect()).expect("a property");
        read.patches[0]
            .read(&store)
            .expect("its values")
            .to_f64()
            .expect("numbers")
    };
    let objects = store.resources();
    let series_uri = objects
        .iter()
        .find(|one| one.qualified_type == "eml23.TimeSeries")
        .expect("the series")
        .uri
        .clone();
    let series = json_of(&series_uri);
    let series_uuid = series["uuid"].as_str().unwrap().to_string();
    let listed: Vec<String> = series["Time"]
        .as_array()
        .unwrap()
        .iter()
        .map(|time| time["DateTime"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(listed.len(), 3);

    let properties: Vec<(String, serde_json::Value)> = objects
        .iter()
        .filter(|one| one.qualified_type == "resqml22.ContinuousProperty")
        .map(|one| (one.uri.clone(), json_of(&one.uri)))
        .collect();
    let (base_uri, base) = properties
        .iter()
        .find(|(_, json)| json.get("Time").is_none() && json.get("TimeOrIntervalSeries").is_none())
        .expect("the snapshot");
    let snapshot = values_of(base_uri);
    let nodes = snapshot.len();
    assert!(
        nodes > 3
            && snapshot.iter().any(|value| value.is_nan())
            && snapshot.iter().any(|value| value.is_finite())
    );

    // The calendar, keyed by the text the series holds — never by a parser.
    let days = |instant: &str| match instant {
        "2016-01-01T00:00:00Z" => 0.0,
        "2017-01-01T00:00:00Z" => 366.0,
        "2018-01-01T00:00:00Z" => 731.0,
        other => panic!("an instant the test did not state: {other}"),
    };

    // The steps: each dated by an instant of the series, each instant once.
    let dated: Vec<&(String, serde_json::Value)> = properties
        .iter()
        .filter(|(_, json)| json.get("Time").is_some())
        .collect();
    assert_eq!(dated.len(), 3, "one property per instant");
    let mut seen = [false; 3];
    for (uri, property) in &dated {
        let instant = property["Time"]["DateTime"].as_str().expect("a date-time");
        let index = listed
            .iter()
            .position(|one| one == instant)
            .unwrap_or_else(|| panic!("{instant} is no instant of the series"));
        assert!(!seen[index], "instant {index} dated twice");
        seen[index] = true;
        assert!(property.get("TimeOrIntervalSeries").is_none());
        assert_eq!(
            property["SupportingRepresentation"]["Uuid"],
            base["SupportingRepresentation"]["Uuid"]
        );
        let values = values_of(uri);
        assert_eq!(values.len(), nodes);
        for node in 0..nodes {
            let expected = snapshot[node] + drift(days(&listed[index]));
            assert!(
                same(values[node], expected),
                "{instant}, node {node}: {} where {expected}",
                values[node]
            );
        }
    }

    // The series: step k is the interval from instant k to instant k + 1.
    let (uri, timed) = properties
        .iter()
        .find(|(_, json)| json.get("TimeOrIntervalSeries").is_some())
        .expect("the series property");
    assert_eq!(
        timed["TimeOrIntervalSeries"]["TimeSeries"]["Uuid"],
        json!(series_uuid)
    );
    assert_eq!(timed["TimeOrIntervalSeries"]["UseInterval"], json!(true));
    let values = values_of(uri);
    assert_eq!(
        values.len(),
        (listed.len() - 1) * nodes,
        "n instants, n − 1 steps, time the slowest"
    );
    for step in 0..listed.len() - 1 {
        let middle = (days(&listed[step]) + days(&listed[step + 1])) / 2.0;
        for node in 0..nodes {
            let expected = snapshot[node] + drift(middle);
            assert!(
                same(values[step * nodes + node], expected),
                "interval {step}, node {node}"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
