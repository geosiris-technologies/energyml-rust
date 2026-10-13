//! `injection-test` - Lutetia's injection test.
//!
//! **The form of each assertion comes from the schemas, the guides and the
//! decisions of 27/09; the values from the bag.** The test references its
//! measurements in a WITSML channel set (PRODML Technical Usage Guide v2.3
//! §13.6 p. 118), `TimeChannel` names a channel of dates (FlowTestActivity.xsd
//! l. 100-103, a deliberate decision), and the collection lists the test
//! (`FlowTestActivity` 1..n, SimpleProductVolume.xsd l. 519-522). The pressure
//! is checked against the definition of an injectivity index - a settled rate
//! over the pressure it adds - computed here, not read from the operation.

use serde_json::Value;

use energyml_build::operation::{Choices, Identity, NewObject, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_core::registry::DynObject;
use energyml_prodml_2_3 as prodml;
use energyml_witsml_2_1 as witsml;

const GTH2: &str = "2d2d2d2d-0000-4000-8000-000000000002";
const COMPANY: &str = "2d2d2d2d-0000-4000-8000-000000000009";

/// Three steps of 2 h, sampled every 30 min: 12 intervals, 13 samples.
const STEPS: [f64; 3] = [20.0, 40.0, 60.0];
const P0: f64 = 12.0;
const II: f64 = 10.0;

fn when() -> When {
    When {
        creation: "2026-09-27T00:00:00Z".into(),
        originator: "test".into(),
    }
}

fn bag() -> Pieces {
    Pieces::empty().with(
        "wellbore",
        Source::Object(Known {
            reference: Some(Ref::new(GTH2, "witsml21.Wellbore", "GTH-2")),
            ..Default::default()
        }),
    )
}

fn choices() -> Choices {
    Choices::new()
        .set("title", "GTH-2 - injection test")
        .set("source", "synthetic - a trial source; to be sourced")
        .set("start", "2025-06-02T08:00:00Z")
        .set("steps", serde_json::json!(STEPS))
        .set("stepHours", 2.0)
        .set("sampleMinutes", 30)
        .set("staticPressure", P0)
        .set("injectivity", II)
        .set("reservoirTemperature", 30.0)
        .set("injectionTemperature", 12.0)
        .set("nominalPeriod", "day")
        .set("standardConditions", "15 degC 1 atm")
        .set("loggingCompany", serde_json::json!({ "Uuid": COMPANY, "QualifiedType": "eml23.BusinessAssociate", "Title": "Geosiris" }))
}

fn planned(pieces: &Pieces, choices: Choices) -> Result<Plan, String> {
    Registry::new()
        .plan("injection-test", pieces, &choices, &Identity::derived(), &when())
        .map_err(|why| why.to_string())
}

fn of_type<'a>(plan: &'a Plan, qualified: &str) -> &'a NewObject {
    let found: Vec<&NewObject> = plan
        .objects
        .iter()
        .filter(|one| one.qualified_type == qualified)
        .collect();
    assert_eq!(
        found.len(),
        1,
        "one `{qualified}`: {:?}",
        plan.objects.iter().map(|o| &o.qualified_type).collect::<Vec<_>>()
    );
    found[0]
}

fn read<T: energyml_core::value::Reflect + Clone>(object: &NewObject) -> T {
    let schemas = energyml_schemas::all();
    let document = energyml_core::json::read_value(&object.json, &schemas)
        .unwrap_or_else(|why| panic!("`{}` does not read back: {why}", object.title));
    let root: &dyn DynObject = &*document.root;
    root.as_reflect()
        .downcast_ref::<T>()
        .unwrap_or_else(|| panic!("`{}` is no {}", object.title, std::any::type_name::<T>()))
        .clone()
}

/// The log's channel set and its channels by mnemonic.
fn channel_set(plan: &Plan) -> witsml::ChannelSet {
    let log: witsml::Log = read(of_type(plan, "witsml21.Log"));
    assert_eq!(log.channel_set.len(), 1);
    let witsml::AnyChannelSet::ChannelSet(set) = &log.channel_set[0] else {
        panic!("a plain channel set")
    };
    set.clone()
}

fn channel<'a>(set: &'a witsml::ChannelSet, mnemonic: &str) -> &'a witsml::Channel {
    set.channel
        .iter()
        .find_map(|one| match one {
            witsml::AnyChannel::Channel(channel) if channel.mnemonic == mnemonic => Some(channel),
            _ => None,
        })
        .unwrap_or_else(|| panic!("a channel `{mnemonic}`"))
}

fn rows(set: &witsml::ChannelSet) -> Vec<Vec<Value>> {
    let text = set
        .data
        .as_ref()
        .and_then(|data| data.data.clone())
        .expect("the values inline");
    serde_json::from_str(&text).expect("a JSON array of rows")
}

#[test]
fn the_test_points_at_the_channels_its_log_holds() {
    // To break: a reference to a uuid the log does not hold, or two
    // references swapped (the rate read as the pressure).
    let plan = planned(&bag(), choices()).expect("a plan");
    let set = channel_set(&plan);
    let test: prodml::InjectionFlowTest = read(of_type(&plan, "prodml23.InjectionFlowTest"));
    let measured = &test.interval_measurement_set;
    assert_eq!(
        measured.location.wellbore.as_ref().map(|one| one.uuid.as_str()),
        Some(GTH2)
    );

    let [prodml::AbstractPtaFlowData::MeasuredFlowData(flow)] = measured.measured_flow_rate.as_slice() else {
        panic!("one measured flow")
    };
    let [pressure] = measured.measured_pressure.as_slice() else {
        panic!("one measured pressure")
    };
    let [other] = measured.other_data.as_slice() else {
        panic!("one other data")
    };
    for (name, reference) in [
        ("flow", &flow.channel_set),
        ("pressure", &pressure.channel_set),
        ("other", &other.channel_set),
    ] {
        assert_eq!(reference.uuid, set.uuid, "{name}: the channel set the log holds");
        assert_eq!(
            reference.qualified_type, "witsml21.ChannelSet",
            "{name}: a WITSML one (Usage Guide v2.3 §13.6)"
        );
    }
    for time in [&flow.time_channel, &pressure.time_channel, &other.time_channel] {
        assert_eq!(
            time.uuid,
            channel(&set, "TIME").uuid,
            "TimeChannel names the channel of dates"
        );
    }
    assert_eq!(flow.flow_channel.uuid, channel(&set, "QINJ").uuid);
    assert_eq!(pressure.pressure_channel.uuid, channel(&set, "BHP").uuid);
    assert_eq!(other.data_channel.uuid, channel(&set, "BHT").uuid);

    let tests: prodml::ProductionWellTests = read(of_type(&plan, "prodml23.ProductionWellTests"));
    let test_object = of_type(&plan, "prodml23.InjectionFlowTest");
    assert_eq!(
        tests
            .flow_test_activity
            .iter()
            .map(|one| one.uuid.as_str())
            .collect::<Vec<_>>(),
        [test_object.uuid.as_str()]
    );
    assert_eq!(
        tests.flow_test_activity[0].qualified_type, "prodml23.InjectionFlowTest",
        "the concrete type (27/09)"
    );
    assert_eq!(tests.start_date, "2025-06-02T08:00:00Z");
    assert_eq!(tests.end_date, "2025-06-02T14:00:00Z", "three steps of 2 h");
}

#[test]
fn time_is_a_channel_of_dates_and_the_index_is_date_time() {
    let plan = planned(&bag(), choices()).expect("a plan");
    let set = channel_set(&plan);
    let time = channel(&set, "TIME");
    assert_eq!(time.data_kind, witsml::ChannelDataKind::DateTime);
    assert_eq!(
        time.uom.as_xml_str(),
        "s",
        "a unit of the quantity class `time` (WITSML TRG v2.1 §7.1)"
    );
    assert_eq!(time.channel_property_kind.title, "absolute time");
    assert_eq!(set.index.len(), 1, "one index, never two");
    assert_eq!(set.index[0].index_kind, energyml_eml_2_3::DataIndexKind::DateTime);
    for one in &set.channel {
        let witsml::AnyChannel::Channel(channel) = one else {
            panic!()
        };
        assert_eq!(
            channel.logging_company.uuid, COMPANY,
            "Channel/LoggingCompany is 1..1"
        );
        assert_eq!(channel.wellbore.as_ref().map(|one| one.uuid.as_str()), Some(GTH2));
    }
    // Each row: the index, then TIME, QINJ, BHP, BHT in the order listed.
    let order: Vec<&str> = set
        .channel
        .iter()
        .map(|one| match one {
            witsml::AnyChannel::Channel(c) => c.mnemonic.as_str(),
            _ => "?",
        })
        .collect();
    assert_eq!(order, ["TIME", "QINJ", "BHP", "BHT"]);
    for row in rows(&set) {
        assert_eq!(row.len(), 5);
        assert_eq!(row[0], row[1], "the TIME channel holds the index's instants");
    }
}

#[test]
fn a_settled_step_adds_its_rate_over_the_injectivity_index() {
    // `responseMinutes` 0: each step settles at once, so every sample of step
    // k after the first reads P0 + q_k / II - the definition of the index.
    let plan = planned(&bag(), choices()).expect("a plan");
    let rows = rows(&channel_set(&plan));
    assert_eq!(
        rows.len(),
        13,
        "3 steps × 4 samples of 30 min, and the last instant"
    );
    assert_eq!(rows[0][0], "2025-06-02T08:00:00Z");
    assert_eq!(rows[12][0], "2025-06-02T14:00:00Z");
    assert_eq!(
        rows[0][3].as_f64(),
        Some(P0),
        "before any water goes in, the static pressure"
    );
    for (at, row) in rows.iter().enumerate().skip(1) {
        let step = (at / 4).min(2);
        assert_eq!(
            row[2].as_f64(),
            Some(STEPS[step]),
            "row {at}: the rate of step {step}"
        );
        let expected = P0 + STEPS[step] / II;
        assert!(
            (row[3].as_f64().unwrap() - expected).abs() < 1e-9,
            "row {at}: {} against {expected}",
            row[3]
        );
    }
}

#[test]
fn a_slow_response_starts_where_the_last_step_left_and_approaches_its_target() {
    let plan = planned(
        &bag(),
        choices().set("responseMinutes", 30.0).set("coolingHours", 3.0),
    )
    .expect("a plan");
    let rows = rows(&channel_set(&plan));
    let pressure = |at: usize| rows[at][3].as_f64().unwrap();
    let temperature = |at: usize| rows[at][4].as_f64().unwrap();
    // Within a step, the pressure moves towards P0 + q / II and never past it.
    for (step, rate) in STEPS.iter().enumerate() {
        let target = P0 + rate / II;
        let first = step * 4 + 1;
        for at in first..first + 3 {
            assert!(
                pressure(at) < pressure(at + 1) + 1e-12 && pressure(at + 1) <= target + 1e-12,
                "step {step}, row {at}"
            );
        }
    }
    assert!(
        pressure(1) > P0 && pressure(1) < P0 + STEPS[0] / II,
        "the first sample has not settled"
    );
    // The bottom cools from the reservoir's towards the injected water's.
    assert_eq!(temperature(0), 30.0);
    assert!((1..13).all(|at| temperature(at) < temperature(at - 1) && temperature(at) > 12.0));
}

#[test]
fn what_cannot_be_a_test_is_refused_by_name() {
    let refused = |choices: Choices, slot: &str| {
        let why = planned(&bag(), choices).expect_err("refused");
        assert!(why.contains(slot), "`{slot}` named: {why}");
    };
    refused(choices().set("sampleMinutes", 25), "sampleMinutes");
    refused(choices().set("injectivity", 0.0), "injectivity");
    refused(choices().set("nominalPeriod", "fortnight"), "nominalPeriod");
    refused(choices().set("source", "  "), "source");
    refused(choices().set("steps", serde_json::json!([20.0, -5.0])), "steps");
    let feature = Pieces::empty().with(
        "wellbore",
        Source::Object(Known {
            reference: Some(Ref::new(GTH2, "resqml22.WellboreFeature", "GTH-2")),
            ..Default::default()
        }),
    );
    let why = planned(&feature, choices()).expect_err("a RESQML feature is no WITSML wellbore");
    assert!(why.contains("wellbore"), "{why}");
}

#[test]
fn written_to_a_package_every_reference_resolves_and_the_test_is_rooted_as_the_schema_declares() {
    let plan = planned(&bag(), choices()).expect("a plan");
    let schemas = std::sync::Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "injection.epc", schemas.clone()).unwrap();
    let made = energyml_build::apply::apply(&plan, &mut store, &when());
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(made.refused.is_empty(), "{:?}", made.refused);
    let dir = std::env::temp_dir().join(format!("energyml-injection-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("injection.epc");
    store
        .save_as(&path, &energyml_storage::SaveOptions::default())
        .unwrap();
    let store = energyml_storage::EpcStore::open(&path, schemas).unwrap();

    let test = store
        .resources()
        .into_iter()
        .find(|one| one.qualified_type == "prodml23.InjectionFlowTest")
        .expect("listed as the concrete type");
    let raw = String::from_utf8(store.raw_part(&test.locator).unwrap()).unwrap();
    assert!(
        raw.contains("<prodml:FlowTestActivity ") && raw.contains(r#"xsi:type="prodml:InjectionFlowTest""#),
        "FlowTestActivity.xsd l. 185"
    );

    let report = energyml_domain::check::of_store(
        &store,
        &energyml_domain::check::Options {
            schema: true,
            rules: false,
            extra_rules: Vec::new(),
        },
    );
    let ours: Vec<String> = plan
        .objects
        .iter()
        .map(|one| one.uuid.to_ascii_lowercase())
        .collect();
    // The wellbore and the company are not in this package: those two are
    // named, and nothing else - the channel set and its channels resolve.
    let dangling: Vec<&energyml_storage::RefIssue> = report
        .references
        .iter()
        .filter(|one| {
            one.is_error()
                && ours
                    .iter()
                    .any(|uuid| one.source.to_ascii_lowercase().contains(uuid))
        })
        .filter(|one| ![GTH2, COMPANY].contains(&one.target_uuid.as_str()))
        .collect();
    assert!(dangling.is_empty(), "{dangling:#?}");
    assert!(report.relationships.is_empty(), "{:#?}", report.relationships);
    assert!(
        report
            .schema
            .iter()
            .all(|one| one.severity != energyml_core::validate::Severity::Error),
        "{:#?}",
        report.schema
    );
    let _ = std::fs::remove_dir_all(&dir);
}
