//! The series readers of `energyml_domain::series`, against the writers of
//! this crate: what `witsml-logs`, `injection-test` and `prodml-doublet`
//! write, read back and compared **to the inputs the test gave** — the
//! depths, the steps, the instants, the frozen window — never to what the
//! writer says it wrote.
//!
//! Two code paths meet here and neither checks itself: the writer lays rows
//! out by its own module doc (`witsml_logs.rs`, *The row layout*), the reader
//! reads them by its own (`channel_set.rs`, *inferred*). A reader that
//! swapped two columns, or a writer that did, turns these red; a reader and a
//! writer that agreed on a wrong layout would not — which is why the layout
//! is also pinned by hand-written rows in `energyml-domain/tests/series.rs`,
//! and why the values compared here are the test's own.
//!
//! The arithmetic of the pressure is the definition of an injectivity index,
//! computed here (`P0 + q / II` once a step has settled); the volumes are
//! `energyml_analysis::flow_law`'s, called here directly.

use serde_json::json;

use energyml_analysis::flow_law::{self, Law};
use energyml_build::operation::{Choices, Identity, NewObject, Plan, Registry, When};
use energyml_build::pieces::{Known, KnownProperty, Pieces, Ref, Source};
use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::series::decimate::decimate;
use energyml_domain::series::time_series::{self, Content};
use energyml_domain::series::{self, IndexKind, Quality, Readout, Refused, channel_set, flow_test, instant, production};
use energyml_domain::source::NoObjects;
use energyml_domain::values::Values;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

/// A plan's objects, read back through their JSON — the road `apply` takes —
/// each with its uuid.
fn objects_of(plan: &Plan) -> Vec<(String, Box<dyn DynObject>)> {
    let schemas = energyml_schemas::all();
    plan.objects
        .iter()
        .map(|object| {
            let document = energyml_core::json::read_value(&object.json, &schemas)
                .unwrap_or_else(|why| panic!("`{}` does not read back: {why}", object.title));
            (object.uuid.clone(), document.root)
        })
        .collect()
}

fn pairs(held: &[(String, Box<dyn DynObject>)]) -> Vec<(&str, &dyn Reflect)> {
    held.iter().map(|(uuid, object)| (uuid.as_str(), object.as_reflect())).collect()
}

fn of_type<'a>(plan: &'a Plan, qualified: &str) -> Vec<&'a NewObject> {
    plan.objects.iter().filter(|one| one.qualified_type == qualified).collect()
}

// ---------------------------------------------------------------------------
// witsml-logs: a Log in measured depth
// ---------------------------------------------------------------------------

const FRAME: &str = "10000000-0000-4000-8000-000000000001";
const INTERPRETATION: &str = "10000000-0000-4000-8000-000000000003";
const DATUM: &str = "10000000-0000-4000-8000-000000000004";
const COMPANY: &str = "40000000-0000-4000-8000-000000000001";

fn mds() -> Vec<f64> {
    vec![0.0, 10.0, 20.0, 30.0, 40.0]
}

fn frame_bag() -> Pieces {
    let head = [651_000.0, 6_861_000.0, 50.0];
    let points: Vec<[f64; 3]> = mds().iter().map(|md| [head[0], head[1], head[2] - md]).collect();
    let n = points.len() as u32;
    let known = Known {
        reference: Some(Ref::new(FRAME, "resqml22.WellboreFrameRepresentation", "GTH-1 — frame")),
        interpretation: Some(Ref::new(INTERPRETATION, "resqml22.WellboreInterpretation", "GTH-1 — interpretation")),
        node_count: Some(n as usize),
        node_mds: Some(mds()),
        datum: Some(Ref::new(DATUM, "eml23.ReferencePointInALocalEngineeringCompoundCrs", "GTH-1 — datum")),
        ..Known::default()
    };
    let shape = Source::Shape {
        geometry: Geometry {
            kind: "polyline",
            patches: vec![Patch { index: 0, node_offset: 0, points, topology: Topology::Polylines(vec![n]), closed: Vec::new() }],
        },
        closed: None,
    };
    Pieces::empty().with("frames", shape).with("frames (object)", Source::Object(known))
}

fn property(uuid: &str, title: &str, kind_uuid: &str, kind: &str, uom: &str, values: Vec<f64>) -> Known {
    Known {
        reference: Some(Ref::new(uuid, "resqml22.ContinuousProperty", title)),
        property: Some(KnownProperty {
            supporting: Some(Ref::new(FRAME, "resqml22.WellboreFrameRepresentation", "GTH-1 — frame")),
            indexable: "nodes".to_string(),
            value_count: vec![1],
            timed: false,
            realizations: false,
            uom: Some(uom.to_string()),
            kind: Some(Ref::new(kind_uuid, "eml23.PropertyKind", kind)),
            lookup: None,
            null_value: None,
            facets: Vec::new(),
            values: Ok(Values::Floating(values)),
        }),
        ..Known::default()
    }
}

const TEMPERATURES: [f64; 5] = [f64::NAN, 13.1, 13.4, 13.7, 14.0];
const PRESSURES: [f64; 5] = [0.0, 1.0, 2.0, 3.0, 4.0];

#[test]
fn what_witsml_logs_writes_reads_back_as_the_depths_and_values_given() {
    let pieces = frame_bag()
        .with("properties", Source::Object(property("20000000-0000-4000-8000-000000000001", "GTH-1 — temperature", "30000000-0000-4000-8000-000000000001", "temperature", "degC", TEMPERATURES.to_vec())))
        .with("properties 2", Source::Object(property("20000000-0000-4000-8000-000000000002", "GTH-1 — pressure", "30000000-0000-4000-8000-000000000002", "pressure", "bar", PRESSURES.to_vec())));
    let choices = Choices::new().set(
        "loggingCompany",
        json!({ "$type": "eml23.DataObjectReference", "Uuid": COMPANY, "QualifiedType": "eml23.BusinessAssociate", "Title": "a company" }),
    );
    let plan = Registry::new()
        .plan("witsml-logs", &pieces, &choices, &Identity::derived(), &When { creation: "2026-09-28T00:00:00Z".into(), originator: "test".into() })
        .expect("a plan");
    let held = objects_of(&plan);
    let objects = pairs(&held);
    let log = objects.iter().find(|(_, object)| channel_set::reads_log(*object)).expect("a Log").1;

    let sets = channel_set::read_log(log).expect("the log reads");
    let [series] = sets.as_slice() else { panic!("one channel set, got {}", sets.len()) };
    assert_eq!(series.index.kind, IndexKind::MeasuredDepth);
    assert_eq!(series.index.values, mds(), "the frame's own depths");
    assert_eq!(series.index.datum.as_deref(), Some(DATUM));

    let temperature = series.channel("temperature").expect("a channel named after its kind");
    let pressure = series.channel("pressure").expect("a channel named after its kind");
    assert_eq!(temperature.uom.as_deref(), Some("degC"));
    // The NaN the property holds above the ground comes back absent — the
    // writer's `null`, the reader's `Quality::Absent` — and nothing else does.
    assert_eq!(temperature.quality[0], Quality::Absent);
    assert!(temperature.quality[1..].iter().all(Quality::is_good));
    assert_eq!(&temperature.numbers().unwrap()[1..], &TEMPERATURES[1..]);
    assert_eq!(pressure.values, Values::Floating(PRESSURES.to_vec()));

    // The same through the one entry point, by the nested set's uuid.
    let Ok(Readout::Sets(by_uuid)) = series::read(&series.uuid, &objects, &NoObjects) else { panic!("the nested set by its uuid") };
    // By their JSON: a `NaN` is not equal to itself.
    assert_eq!(by_uuid[0].to_json(), series.to_json());
}

// ---------------------------------------------------------------------------
// injection-test: a Log in date time, and the test that points at it
// ---------------------------------------------------------------------------

const GTH2: &str = "2d2d2d2d-0000-4000-8000-000000000002";
const STEPS: [f64; 3] = [20.0, 40.0, 60.0];
const P0: f64 = 12.0;
const II: f64 = 10.0;
const START: &str = "2025-06-02T08:00:00Z";

fn injection_plan() -> Plan {
    let pieces = Pieces::empty().with(
        "wellbore",
        Source::Object(Known { reference: Some(Ref::new(GTH2, "witsml21.Wellbore", "GTH-2")), ..Default::default() }),
    );
    let choices = Choices::new()
        .set("title", "GTH-2 — injection test")
        .set("source", "synthetic — a test of the series readers")
        .set("start", START)
        .set("steps", json!(STEPS))
        .set("stepHours", 2.0)
        .set("sampleMinutes", 30)
        .set("staticPressure", P0)
        .set("injectivity", II)
        .set("reservoirTemperature", 30.0)
        .set("injectionTemperature", 12.0)
        .set("nominalPeriod", "day")
        .set("standardConditions", "15 degC 1 atm")
        .set("loggingCompany", json!({ "Uuid": COMPANY, "QualifiedType": "eml23.BusinessAssociate", "Title": "Geosiris" }));
    Registry::new()
        .plan("injection-test", &pieces, &choices, &Identity::derived(), &When { creation: "2026-09-28T00:00:00Z".into(), originator: "test".into() })
        .expect("a plan")
}

/// The instants the test asked for: from START, every 30 minutes, three steps
/// of two hours and the last instant — 13.
fn asked_instants() -> Vec<f64> {
    let start = instant::unix_seconds(START).unwrap();
    (0..13).map(|k| start + k as f64 * 1800.0).collect()
}

/// The step a sample belongs to: four samples per step, the last instant
/// ending the last step.
fn step_of(at: usize) -> usize {
    (at / 4).min(STEPS.len() - 1)
}

#[test]
fn the_injection_log_reads_back_on_the_asked_instants_with_the_asked_steps() {
    let plan = injection_plan();
    let held = objects_of(&plan);
    let objects = pairs(&held);
    let log = objects.iter().find(|(_, object)| channel_set::reads_log(*object)).expect("a Log").1;
    let sets = channel_set::read_log(log).expect("reads");
    let series = &sets[0];
    assert_eq!(series.index.kind, IndexKind::DateTime);
    assert_eq!(series.index.values, asked_instants());
    assert_eq!(series.channel("TIME").unwrap().values, Values::Floating(asked_instants()), "TIME holds the index's instants");
    let rates = series.channel("QINJ").unwrap().numbers().unwrap();
    for (at, rate) in rates.iter().enumerate().skip(1) {
        assert_eq!(*rate, STEPS[step_of(at)], "sample {at}");
    }
}

#[test]
fn the_flow_test_resolves_its_channels_through_the_log_and_the_pressure_is_the_indexs_definition() {
    let plan = injection_plan();
    let held = objects_of(&plan);
    let objects = pairs(&held);
    let test_uuid = of_type(&plan, "prodml23.InjectionFlowTest")[0].uuid.clone();

    let Ok(Readout::FlowTest(measured)) = series::read(&test_uuid, &objects, &NoObjects) else { panic!("the test reads") };
    let roles: Vec<(&str, &str)> = measured.iter().map(|one| (one.group.as_str(), one.role.as_str())).collect();
    assert_eq!(roles, [("MeasuredFlowRate", "FlowChannel"), ("MeasuredPressure", "PressureChannel"), ("OtherData", "DataChannel")]);
    for one in &measured {
        assert_eq!(one.series.index.values, asked_instants(), "{}: against the test's TimeChannel", one.role);
        assert_eq!(one.series.index.mnemonic, "TIME");
        assert_eq!(one.series.index.kind, IndexKind::DateTime, "a channel of dates is a date-time index");
    }
    let pressure = measured[1].series.channels[0].numbers().unwrap();
    assert_eq!(measured[1].series.channels[0].mnemonic, "BHP");
    assert_eq!(pressure[0], P0, "before any water goes in");
    for (at, value) in pressure.iter().enumerate().skip(1) {
        // `responseMinutes` 0: settled at once — the definition of II.
        let expected = P0 + STEPS[step_of(at)] / II;
        assert!((value - expected).abs() < 1e-9, "sample {at}: {value} against {expected}");
    }
    let flow = measured[0].series.channels[0].numbers().unwrap();
    assert_eq!(flow[4], STEPS[1]);
}

#[test]
fn a_flow_test_whose_log_is_missing_is_refused_by_name() {
    let plan = injection_plan();
    let held: Vec<(String, Box<dyn DynObject>)> =
        objects_of(&plan).into_iter().filter(|(_, object)| !channel_set::reads_log(object.as_reflect())).collect();
    let objects = pairs(&held);
    let test = objects.iter().find(|(_, object)| flow_test::reads(*object)).expect("the test").1;
    assert!(matches!(flow_test::read(test, &objects, &NoObjects), Err(Refused::ChannelSetNotFound { .. })));
}

// ---------------------------------------------------------------------------
// prodml-doublet: head series with a frozen window, periods
// ---------------------------------------------------------------------------

const PRODUCER: &str = "1a1a1a1a-0000-4000-8000-000000000001";
const INJECTOR: &str = "1a1a1a1a-0000-4000-8000-000000000002";
const RATE: f64 = 40.0;
const DECLINE: f64 = 0.05;
const MONTHS: [&str; 4] = ["2025-01-01T00:00:00Z", "2025-02-01T00:00:00Z", "2025-03-01T00:00:00Z", "2025-04-01T00:00:00Z"];

fn doublet_plan() -> Plan {
    let object = |uuid: &str, qualified: &str, title: &str| {
        Source::Object(Known { reference: Some(Ref::new(uuid, qualified, title)), ..Default::default() })
    };
    let pieces = Pieces::empty()
        .with("producer", object(PRODUCER, "witsml21.Wellbore", "GTH-1"))
        .with("injector", object(INJECTOR, "witsml21.Wellbore", "GTH-2"))
        .with("series", object("1a1a1a1a-0000-4000-8000-000000000004", "eml23.TimeSeries", "Monthly 2025"));
    let choices = Choices::new()
        .set("facility", "Heat exchanger")
        .set("fluid", "Eau")
        .set("source", "synthetic — a test of the series readers")
        .set("times", json!(MONTHS))
        .set("rate", RATE)
        .set("decline", DECLINE)
        .set("standardConditions", "15 degC 1 atm")
        .set("producerHeadTemperature", 16.5)
        .set("producerHeadPressure", 3.2)
        .set("assetIdentifier", "eml://lutetia.geosiris.com/well({wellbore})/wellbore(01)/wellhead")
        .set("samplesFrom", "2025-06-02T00:00:00Z")
        .set("sampleDays", 1)
        .set("sampleStep", 60)
        .set("frozen", "rate")
        .set("frozenFrom", "2025-06-02T02:00:00Z")
        .set("frozenMinutes", 120);
    Registry::new()
        .plan("prodml-doublet", &pieces, &choices, &Identity::derived(), &When { creation: "2026-09-28T00:00:00Z".into(), originator: "test".into() })
        .expect("a plan")
}

fn witsml_well(uuid: &str, title: &str) -> Box<dyn DynObject> {
    Box::new(energyml_witsml_2_1::Well {
        citation: energyml_eml_2_3::Citation { title: title.into(), ..Default::default() },
        uuid: uuid.into(),
        schema_version: "2.1".into(),
        ..Default::default()
    })
}

#[test]
fn the_frozen_window_asked_comes_back_flagged_on_the_asked_instants() {
    let plan = doublet_plan();
    let mut held = objects_of(&plan);
    held.push(("5e5e5e5e-0000-4000-8000-000000000001".into(), witsml_well("5e5e5e5e-0000-4000-8000-000000000001", "GTH-1")));
    held.push(("5e5e5e5e-0000-4000-8000-000000000002".into(), witsml_well("5e5e5e5e-0000-4000-8000-000000000002", "GTH-2")));
    let objects = pairs(&held);
    let injector_well = objects[objects.len() - 1].1;
    let producer_well = objects[objects.len() - 2].1;

    // The injector's one series, found by its well's name.
    let (_, rate_object) = time_series::of_well(&objects, injector_well, Some("volume per time")).expect("the rate series");
    let rate = time_series::read(rate_object).expect("reads");
    let Content::Samples(series) = &rate.content else { panic!("samples") };
    let start = instant::unix_seconds("2025-06-02T00:00:00Z").unwrap();
    let asked: Vec<f64> = (0..24).map(|hour| start + hour as f64 * 3600.0).collect();
    assert_eq!(series.index.values, asked, "one sample an hour over one day, as asked");
    let frozen: Vec<usize> =
        series.channels[0].quality.iter().enumerate().filter(|(_, q)| **q == Quality::Flagged("frozen".into())).map(|(at, _)| at).collect();
    assert_eq!(frozen, [2, 3], "02:00 and 03:00 — the 120 minutes asked from 02:00");
    // The producer has two series (pressure, temperature): no class, no pick.
    assert!(matches!(time_series::of_well(&objects, producer_well, None), Err(Refused::SeveralMatch { .. })));

    // Decimated, the frozen window stays a gap.
    let out = decimate(series, 0, asked[0], asked[23], 4).expect("decimated");
    assert_eq!(out.gaps.len(), 1);
    assert_eq!((out.gaps[0].from, out.gaps[0].to, out.gaps[0].samples), (asked[2], asked[3], 2));
    assert!(out.points.iter().any(|point| point.sample == 4 && point.starts_run));
}

#[test]
fn a_periods_quantities_sum_to_the_laws_volume() {
    let plan = doublet_plan();
    let held = objects_of(&plan);
    let objects = pairs(&held);
    let seconds: Vec<f64> = MONTHS.iter().map(|text| instant::unix_seconds(text).unwrap()).collect();
    let hours: Vec<f64> = seconds.iter().map(|at| (at - seconds[0]) / 3600.0).collect();
    let expected = flow_law::periods(&Law::new(RATE, DECLINE).unwrap(), &hours).expect("the law's periods");

    let reports: Vec<&dyn Reflect> = objects.iter().filter(|(_, object)| production::reads(*object)).map(|(_, object)| *object).collect();
    assert_eq!(reports.len(), 2, "producer and injector");
    for report in reports {
        let periods = production::read(report).expect("reads");
        assert_eq!(periods.len(), MONTHS.len() - 1, "n instants make n − 1 periods");
        for (period, law) in periods.iter().zip(&expected) {
            let product = &period.products[0];
            assert_eq!(product.quantity_uom.as_deref(), Some("m3"));
            assert!((product.quantity.unwrap() - law.volume).abs() < 1e-6 * law.volume, "{} m3 against {}", product.quantity.unwrap(), law.volume);
        }
        assert_eq!(periods[1].start_seconds, seconds[1]);
    }
}
