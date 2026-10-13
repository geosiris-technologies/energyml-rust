//! `prodml-doublet` — the PRODML 2.3 side of Lutetia's geothermal doublet
//!: what it writes, how
//! the objects name their well and their series, and what it refuses.
//!
//! **The arithmetic is cross-checked against `energyml_analysis::flow_law`
//! directly** (the crate's law, not the operation grading its own work) —
//! the triangulation `crates/energyml-build/ARCHITECTURE.md` asks for. Every JSON
//! the plan writes is read back into its generated PRODML type before
//! anything is asserted of it, the road `apply` takes.

use serde_json::json;

use energyml_analysis::flow_law::{self, Law};
use energyml_build::operation::{Choices, Identity, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_core::registry::DynObject;
use energyml_domain::provenance::Parameter;
use energyml_prodml_2_3 as prodml;
use energyml_storage::epc::core_props::unix_seconds;

const PRODUCER: &str = "1a1a1a1a-0000-4000-8000-000000000001";
const INJECTOR: &str = "1a1a1a1a-0000-4000-8000-000000000002";
const OBSERVATION: &str = "1a1a1a1a-0000-4000-8000-000000000003";
const SERIES: &str = "1a1a1a1a-0000-4000-8000-000000000004";
const ORGANISATION: &str = "1a1a1a1a-0000-4000-8000-000000000005";

fn when() -> When {
    When { creation: "2026-09-25T00:00:00Z".into(), originator: "test".into() }
}

fn object(uuid: &str, qualified_type: &str, title: &str) -> Source {
    Source::Object(Known { reference: Some(Ref::new(uuid, qualified_type, title)), ..Default::default() })
}

fn bag() -> Pieces {
    Pieces::empty()
        .with("producer", object(PRODUCER, "witsml21.Wellbore", "GTH-1"))
        .with("injector", object(INJECTOR, "witsml21.Wellbore", "GTH-2"))
        .with("series", object(SERIES, "eml23.TimeSeries", "Monthly 2025"))
}

/// The monthly series: 13 monthly instants, 12 periods —
/// 13 dates give 12 values (confirmed).
fn monthly_2025() -> Vec<&'static str> {
    vec![
        "2025-01-01T00:00:00Z",
        "2025-02-01T00:00:00Z",
        "2025-03-01T00:00:00Z",
        "2025-04-01T00:00:00Z",
        "2025-05-01T00:00:00Z",
        "2025-06-01T00:00:00Z",
        "2025-07-01T00:00:00Z",
        "2025-08-01T00:00:00Z",
        "2025-09-01T00:00:00Z",
        "2025-10-01T00:00:00Z",
        "2025-11-01T00:00:00Z",
        "2025-12-01T00:00:00Z",
        "2026-01-01T00:00:00Z",
    ]
}

/// The instants above, as hours since the first — what `flow_law::periods`
/// takes, computed independently of the operation (it goes through
/// `unix_seconds` too, but never through `prodml_doublet` itself).
fn monthly_hours() -> Vec<f64> {
    let seconds: Vec<i64> = monthly_2025().iter().map(|text| unix_seconds(text).expect("a date")).collect();
    let anchor = seconds[0];
    seconds.iter().map(|s| (s - anchor) as f64 / 3600.0).collect()
}

const RATE: f64 = 40.0;
const DECLINE: f64 = 0.05;

fn law() -> Law {
    Law::new(RATE, DECLINE).unwrap()
}

fn choices() -> Choices {
    Choices::new()
        .set("facility", "Lutetia heat exchanger")
        .set("fluid", "Chalk water")
        .set("source", "synthetic — a trial source")
        .set("times", json!(monthly_2025()))
        .set("rate", RATE)
        .set("decline", DECLINE)
        .set("standardConditions", "15 degC 1 atm")
        .set("producerHeadTemperature", 16.5)
        .set("producerHeadPressure", 3.2)
        .set("assetIdentifier", "eml://lutetia.geosiris.com/well({wellbore})/wellbore(01)/wellhead")
        .set("samplesFrom", "2025-06-02T00:00:00Z")
        .set("sampleDays", 2)
        .set("sampleStep", 60)
}

fn planned(pieces: &Pieces, choices: Choices) -> Result<Plan, String> {
    Registry::new().plan("prodml-doublet", pieces, &choices, &Identity::derived(), &when()).map_err(|why| why.to_string())
}

fn titled<'a>(plan: &'a Plan, qualified: &str, title: &str) -> &'a energyml_build::operation::NewObject {
    plan.objects
        .iter()
        .find(|object| object.qualified_type == qualified && object.title == title)
        .unwrap_or_else(|| panic!("a `{qualified}` titled `{title}` in the plan: {:?}", plan.objects.iter().map(|o| &o.title).collect::<Vec<_>>()))
}

/// The JSON of a plan's object, read back into its generated type — the road
/// `apply` takes. A field misspelt, a value of the wrong kind, and this panics.
fn read<T: energyml_core::value::Reflect + Clone>(object: &energyml_build::operation::NewObject) -> T {
    let schemas = energyml_schemas::all();
    let document = energyml_core::json::read_value(&object.json, &schemas)
        .unwrap_or_else(|why| panic!("`{}` does not read back: {why}", object.title));
    let root: &dyn DynObject = &*document.root;
    root.as_reflect()
        .downcast_ref::<T>()
        .unwrap_or_else(|| panic!("`{}` does not read back as {}", object.title, std::any::type_name::<T>()))
        .clone()
}

fn double_param(plan: &Plan, title: &str) -> f64 {
    plan.activity
        .parameters
        .iter()
        .find_map(|p| match p {
            Parameter::Double { title: t, value, .. } if t == title => Some(*value),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no `{title}` parameter recorded"))
}

// ---------------------------------------------------------------- the periods

#[test]
fn thirteen_monthly_instants_make_twelve_periods_each_referencing_its_own_wellbore() {
    // To break: `windows(2)` becomes a map over every instant (13 periods),
    // or a period's `ReportingEntity` names the other hole.
    let plan = planned(&bag(), choices()).expect("a plan");

    let producer_entity = titled(&plan, "prodml23.ReportingEntity", "GTH-1");
    let injector_entity = titled(&plan, "prodml23.ReportingEntity", "GTH-2");

    let producer_report: prodml::WellProductionParameters = read(titled(&plan, "prodml23.WellProductionParameters", "GTH-1 — production"));
    let injector_report: prodml::WellProductionParameters = read(titled(&plan, "prodml23.WellProductionParameters", "GTH-2 — injection"));

    assert_eq!(producer_report.production_period.len(), 12, "13 instants make 12 periods — n instants make n − 1 periods");
    assert_eq!(injector_report.production_period.len(), 12);

    for period in &producer_report.production_period {
        assert_eq!(period.reporting_entity.uuid, producer_entity.uuid, "a producer period names GTH-1, never GTH-2");
    }
    for period in &injector_report.production_period {
        assert_eq!(period.reporting_entity.uuid, injector_entity.uuid, "an injector period names GTH-2, never GTH-1");
    }

    // The instants themselves, in order — not reshuffled, not dropped.
    let starts: Vec<&str> = producer_report.production_period.iter().map(|p| p.start_time.as_str()).collect();
    assert_eq!(starts, monthly_2025()[..12].to_vec());
}

#[test]
fn a_periods_mean_rate_times_its_duration_gives_back_the_laws_volume() {
    // Cross-checked against `energyml_analysis::flow_law` directly — a second
    // source of the same fact, never the operation's own arithmetic re-read.
    let plan = planned(&bag(), choices()).expect("a plan");
    let producer_report: prodml::WellProductionParameters = read(titled(&plan, "prodml23.WellProductionParameters", "GTH-1 — production"));

    let expected = flow_law::periods(&law(), &monthly_hours()).unwrap();
    assert_eq!(producer_report.production_period.len(), expected.len());

    let mut total = 0.0;
    for (found, want) in producer_report.production_period.iter().zip(&expected) {
        assert_eq!(found.duration.uom, energyml_eml_2_3::TimeUom::H, "a period's duration is in hours");
        assert!((found.duration.value - want.hours()).abs() < 1e-9, "{} against {}", found.duration.value, want.hours());
        let rate = found.product_rate.first().expect("a rate");
        assert_eq!(rate.product_fluid_kind, prodml::ProductFluidKindExt::WaterMinusProcessed, "water - processed");
        let flow = rate.volume_flow_rate.as_ref().expect("a flow rate");
        assert_eq!(flow.flow_rate.uom, energyml_eml_2_3::VolumePerTimeUomWithLegacy::M3PerH);
        assert!((flow.flow_rate.value - want.mean_rate).abs() < 1e-9, "{} against {}", flow.flow_rate.value, want.mean_rate);
        // Volume = rate × duration, in the units the two fields carry.
        assert!((flow.flow_rate.value * found.duration.value - want.volume).abs() < 1e-6);
        total += flow.flow_rate.value * found.duration.value;
    }

    let recorded = double_param(&plan, "producedVolume");
    assert!((recorded - total).abs() < 1e-6, "the activity's producedVolume ({recorded}) is rate × duration summed ({total})");
}

#[test]
fn the_injector_returns_what_the_producer_pumps_by_default() {
    let plan = planned(&bag(), choices()).expect("a plan");
    let produced = double_param(&plan, "producedVolume");
    let injected = double_param(&plan, "injectedVolume");
    assert!((produced - injected).abs() < 1e-6, "reinjected defaults to 1 — the doublet's loop");

    // A partial return is a different, smaller volume — not the same law
    // written twice.
    let partial = planned(&bag(), choices().set("reinjected", 0.6)).expect("a plan");
    let injected_partial = double_param(&partial, "injectedVolume");
    assert!((injected_partial - produced * 0.6).abs() < 1e-6);
}

// ---------------------------------------------------------------- the asset identifier

#[test]
fn the_head_series_name_their_wellbores_by_the_reference_models_uri_convention() {
    // The convention of the synthetic model — the asset identifier is a URI built
    // from the wellbore's own title, not a made-up asset name.
    let plan = planned(&bag(), choices()).expect("a plan");

    let pressure: prodml::TimeSeriesData = read(titled(&plan, "prodml23.TimeSeriesData", "GTH-1 — wellhead pressure"));
    let temperature: prodml::TimeSeriesData = read(titled(&plan, "prodml23.TimeSeriesData", "GTH-1 — wellhead temperature"));
    let rate: prodml::TimeSeriesData = read(titled(&plan, "prodml23.TimeSeriesData", "GTH-2 — injection rate"));

    let asset_of = |series: &prodml::TimeSeriesData| -> String {
        series
            .key
            .iter()
            .find(|k| k.keyword == prodml::TimeSeriesKeyword::AssetIdentifier)
            .expect("an asset identifier key")
            .value
            .clone()
    };
    assert_eq!(asset_of(&pressure), "eml://lutetia.geosiris.com/well(GTH-1)/wellbore(01)/wellhead");
    assert_eq!(asset_of(&temperature), "eml://lutetia.geosiris.com/well(GTH-1)/wellbore(01)/wellhead");
    assert_eq!(asset_of(&rate), "eml://lutetia.geosiris.com/well(GTH-2)/wellbore(01)/wellhead");

    // §3, rule 4: invented values are qualified `simulated`, not `measured`.
    let qualifier_of = |series: &prodml::TimeSeriesData| -> String {
        series.key.iter().find(|k| k.keyword == prodml::TimeSeriesKeyword::Qualifier).expect("a qualifier key").value.clone()
    };
    assert_eq!(qualifier_of(&pressure), "simulated");
    assert_eq!(qualifier_of(&rate), "simulated");

    // §7.6: `product` is written for the rate only.
    assert!(rate.key.iter().any(|k| k.keyword == prodml::TimeSeriesKeyword::Product && k.value == "water"));
    assert!(!pressure.key.iter().any(|k| k.keyword == prodml::TimeSeriesKeyword::Product), "meaningless for a pressure");
}

#[test]
fn a_percent_encoded_title_still_names_the_right_wellbore() {
    let with_sidetrack = Pieces::empty()
        .with("producer", object(PRODUCER, "witsml21.Wellbore", "GTH-3 ST1"))
        .with("injector", object(INJECTOR, "witsml21.Wellbore", "GTH-2"))
        .with("series", object(SERIES, "eml23.TimeSeries", "Monthly 2025"));
    let plan = planned(&with_sidetrack, choices()).expect("a plan");
    let pressure: prodml::TimeSeriesData = read(titled(&plan, "prodml23.TimeSeriesData", "GTH-3 ST1 — wellhead pressure"));
    let asset = pressure.key.iter().find(|k| k.keyword == prodml::TimeSeriesKeyword::AssetIdentifier).unwrap();
    assert_eq!(asset.value, "eml://lutetia.geosiris.com/well(GTH-3%20ST1)/wellbore(01)/wellhead");
}

// ---------------------------------------------------------------- the fluid system and the observing hole

#[test]
fn the_fluid_system_references_the_rock_fluid_organisation_when_one_is_given() {
    let with_organisation = bag().with(
        "fluidOrganization",
        object(ORGANISATION, "resqml22.RockFluidOrganizationInterpretation", "Chalk water table"),
    );
    let plan = planned(&with_organisation, choices()).expect("a plan");
    let fluid: prodml::FluidSystem = read(titled(&plan, "prodml23.FluidSystem", "Chalk water"));
    assert_eq!(fluid.reservoir_fluid_kind, prodml::ReservoirFluidKind::Unknown, "none of the six hydrocarbon kinds fits fresh water");
    assert_eq!(fluid.solution_gor.value, 0.0, "negligible, declared — not NaN");
    assert_eq!(
        fluid.rock_fluid_organization_interpretation.as_ref().expect("a reference").uuid,
        ORGANISATION
    );

    let without = planned(&bag(), choices()).expect("a plan");
    let fluid_alone: prodml::FluidSystem = read(titled(&without, "prodml23.FluidSystem", "Chalk water"));
    assert!(fluid_alone.rock_fluid_organization_interpretation.is_none());
    assert!(
        without.says.iter().any(|note| note.key == "fluid-organization"),
        "the plan says it names no organisation, rather than staying silent"
    );
}

#[test]
fn an_observing_wellbore_gets_its_own_reporting_entity_and_no_production() {
    let with_observation = bag().with("observation", object(OBSERVATION, "witsml21.Wellbore", "GTH-3"));
    let plan = planned(&with_observation, choices()).expect("a plan");

    let observing = titled(&plan, "prodml23.ReportingEntity", "GTH-3");
    let entity: prodml::ReportingEntity = read(observing);
    assert_eq!(entity.kind, prodml::ReportingEntityKind::Wellbore);
    assert_eq!(entity.associated_object.as_ref().expect("a reference").uuid, OBSERVATION);

    // No `WellProductionParameters` names GTH-3: an observing hole neither
    // produces nor injects.
    let reports: Vec<prodml::WellProductionParameters> = plan
        .objects
        .iter()
        .filter(|o| o.qualified_type == "prodml23.WellProductionParameters")
        .map(read::<prodml::WellProductionParameters>)
        .collect();
    for report in &reports {
        for period in &report.production_period {
            assert_ne!(period.reporting_entity.uuid, OBSERVATION);
        }
    }
    assert_eq!(reports.len(), 2, "still exactly the producer's and the injector's");
}

// ---------------------------------------------------------------- the frozen sensor and its statistic

#[test]
fn a_frozen_rate_window_repeats_the_last_reading_and_is_set_aside_from_the_daily_statistic() {
    // The window: hour 2 to hour 4 of the sample window (samplesFrom + 2h,
    // 120 minutes), well inside the first day (per_day = 24 at a 60-minute
    // step) and not at the first sample, which `frozen_range` would refuse.
    let plan = planned(
        &bag(),
        choices().set("frozen", "rate").set("frozenFrom", "2025-06-02T02:00:00Z").set("frozenMinutes", 120),
    )
    .expect("a plan");

    let rate: prodml::TimeSeriesData = read(titled(&plan, "prodml23.TimeSeriesData", "GTH-2 — injection rate"));
    let injector_law = law(); // reinjected = 1.0 by default: the same law.
    // One law for the whole doublet, anchored on the first stated instant
    // (2025-01-01): the head samples of 2025-06-02 lie on the same curve, 152
    // days on — the law is read in hours since the anchor, not since the
    // window (the first version of this test read it from the window).
    let since_anchor = 152.0 * 24.0;

    let held = {
        let prodml::AbstractValue::DoubleValue(sample) = &rate.data_value[1] else { panic!("a double sample") };
        sample.double_value.value
    };
    for rank in [2usize, 3] {
        let prodml::AbstractValue::DoubleValue(sample) = &rate.data_value[rank] else { panic!("a double sample") };
        assert_eq!(sample.double_value.status, Some(prodml::ValueStatus::Frozen), "sample {rank} is frozen");
        assert_eq!(sample.double_value.value, held, "a frozen sensor repeats its last reading, sample {rank}");
    }
    // The rank just before and just after the gap are ordinary readings of
    // the declining law — not held.
    let prodml::AbstractValue::DoubleValue(rank4) = &rate.data_value[4] else { panic!("a double sample") };
    assert_eq!(rank4.double_value.status, None);
    assert!((rank4.double_value.value - injector_law.rate_at(since_anchor + 4.0)).abs() < 1e-9);

    // The daily statistic: the first day (ranks 0..24) with the two frozen
    // ranks set aside — computed independently through `flow_law::summary`.
    let day: Vec<f64> = (0..24).filter(|rank| !(2..4).contains(rank)).map(|rank| injector_law.rate_at(since_anchor + rank as f64)).collect();
    let want = flow_law::summary(&day).expect("a summary");

    let statistic: prodml::TimeSeriesStatistic = read(titled(&plan, "prodml23.TimeSeriesStatistic", "GTH-2 — injection rate — daily statistic"));
    assert_eq!(statistic.uom, Some(energyml_eml_2_3::UnitOfMeasureExt::M3PerH));
    assert_eq!(statistic.minimum.as_ref().unwrap().uom, energyml_eml_2_3::DimensionlessUom::Euc, "typed as the schema requires");
    assert!((statistic.minimum.as_ref().unwrap().value - want.minimum).abs() < 1e-9);
    assert!((statistic.maximum.as_ref().unwrap().value - want.maximum).abs() < 1e-9);
    assert!((statistic.mean.as_ref().unwrap().value - want.mean).abs() < 1e-9);

    let frozen_count = double_param(&plan, "frozenCount");
    assert_eq!(frozen_count, 2.0);
}

// ---------------------------------------------------------------- identity

#[test]
fn the_same_inputs_make_the_same_uuids_and_a_different_facility_makes_others() {
    let first = planned(&bag(), choices()).expect("a plan");
    let second = planned(&bag(), choices()).expect("a plan");
    let uuids = |plan: &Plan| -> Vec<(String, String)> { plan.objects.iter().map(|o| (o.role.clone(), o.uuid.clone())).collect() };
    assert_eq!(uuids(&first), uuids(&second), "the same run, twice, writes the same uuids");

    let renamed = planned(&bag(), choices().set("facility", "Another heat exchanger")).expect("a plan");
    let facility_before = titled(&first, "prodml23.Facility", "Lutetia heat exchanger").uuid.clone();
    let facility_after = titled(&renamed, "prodml23.Facility", "Another heat exchanger").uuid.clone();
    assert_ne!(facility_before, facility_after, "a different facility title derives a different uuid");
}

// ---------------------------------------------------------------- everything reads back, and is declared synthetic

#[test]
fn every_object_written_reads_back_into_its_generated_type_and_is_declared_synthetic() {
    let plan = planned(
        &bag()
            .with("observation", object(OBSERVATION, "witsml21.Wellbore", "GTH-3"))
            .with("fluidOrganization", object(ORGANISATION, "resqml22.RockFluidOrganizationInterpretation", "Chalk water table")),
        choices(),
    )
    .expect("a plan");

    // One of each, plus three reporting entities, two production reports and
    // three head series.
    assert_eq!(plan.objects.iter().filter(|o| o.qualified_type == "prodml23.Facility").count(), 1);
    assert_eq!(plan.objects.iter().filter(|o| o.qualified_type == "prodml23.ReportingEntity").count(), 3);
    assert_eq!(plan.objects.iter().filter(|o| o.qualified_type == "prodml23.WellProductionParameters").count(), 2);
    assert_eq!(plan.objects.iter().filter(|o| o.qualified_type == "prodml23.TimeSeriesData").count(), 3);
    assert_eq!(plan.objects.iter().filter(|o| o.qualified_type == "prodml23.TimeSeriesStatistic").count(), 1);
    assert_eq!(plan.objects.iter().filter(|o| o.qualified_type == "prodml23.FluidSystem").count(), 1);

    for object in &plan.objects {
        let synthetic_and_sourced = match object.qualified_type.as_str() {
            "prodml23.Facility" => {
                let typed: prodml::Facility = read(object);
                assert_eq!(typed.uuid, object.uuid);
                typed.extension_name_value.iter().any(|e| e.name == "synthetic")
                    && typed.citation.description.as_deref().is_some_and(|d| d.contains("a trial source"))
            }
            "prodml23.ReportingEntity" => {
                let typed: prodml::ReportingEntity = read(object);
                typed.extension_name_value.iter().any(|e| e.name == "synthetic")
            }
            "prodml23.WellProductionParameters" => {
                let typed: prodml::WellProductionParameters = read(object);
                typed.extension_name_value.iter().any(|e| e.name == "synthetic")
            }
            "prodml23.TimeSeriesData" => {
                let typed: prodml::TimeSeriesData = read(object);
                typed.extension_name_value.iter().any(|e| e.name == "synthetic")
            }
            "prodml23.TimeSeriesStatistic" => {
                let typed: prodml::TimeSeriesStatistic = read(object);
                typed.extension_name_value.iter().any(|e| e.name == "synthetic")
            }
            "prodml23.FluidSystem" => {
                let typed: prodml::FluidSystem = read(object);
                typed.extension_name_value.iter().any(|e| e.name == "synthetic")
            }
            other => panic!("an object of an unexpected type: {other}"),
        };
        assert!(synthetic_and_sourced, "`{}` ({}) is not declared synthetic", object.title, object.qualified_type);
    }
}

// ---------------------------------------------------------------- refusals, by name

#[test]
fn the_producer_and_the_injector_must_be_two_holes() {
    let same = Pieces::empty()
        .with("producer", object(PRODUCER, "witsml21.Wellbore", "GTH-1"))
        .with("injector", object(PRODUCER, "witsml21.Wellbore", "GTH-1"))
        .with("series", object(SERIES, "eml23.TimeSeries", "Monthly 2025"));
    let why = planned(&same, choices()).unwrap_err();
    assert!(why.contains("`injector`"), "{why}");
}

#[test]
fn a_well_is_refused_where_a_doublet_reports_per_wellbore() {
    let with_well = Pieces::empty()
        .with("producer", object(PRODUCER, "witsml21.Well", "GTH-1"))
        .with("injector", object(INJECTOR, "witsml21.Wellbore", "GTH-2"))
        .with("series", object(SERIES, "eml23.TimeSeries", "Monthly 2025"));
    let why = planned(&with_well, choices()).unwrap_err();
    assert!(why.contains("`producer`") && why.contains("`Well`"), "{why}");
}

#[test]
fn fewer_than_two_instants_and_instants_out_of_order_are_refused_by_name() {
    let one_instant = planned(&bag(), choices().set("times", json!(["2025-01-01T00:00:00Z"]))).unwrap_err();
    assert!(one_instant.contains("`times`"), "{one_instant}");

    let backwards = planned(
        &bag(),
        choices().set("times", json!(["2025-01-01T00:00:00Z", "2024-01-01T00:00:00Z", "2026-01-01T00:00:00Z"])),
    )
    .unwrap_err();
    assert!(backwards.contains("`times`") && backwards.contains("come after"), "{backwards}");
}

#[test]
fn a_reinjected_share_outside_zero_to_one_is_refused() {
    let too_much = planned(&bag(), choices().set("reinjected", 1.5)).unwrap_err();
    assert!(too_much.contains("`reinjected`"), "{too_much}");
    let none = planned(&bag(), choices().set("reinjected", 0.0)).unwrap_err();
    assert!(none.contains("`reinjected`"), "{none}");
}

#[test]
fn a_sample_step_that_does_not_divide_a_day_is_refused() {
    let why = planned(&bag(), choices().set("sampleStep", 7)).unwrap_err();
    assert!(why.contains("`sampleStep`") && why.contains("divide"), "{why}");
}

#[test]
fn an_asset_identifier_pattern_without_the_wellbore_placeholder_is_refused() {
    let why = planned(&bag(), choices().set("assetIdentifier", "eml://lutetia.geosiris.com/facility(GTH)/wellhead")).unwrap_err();
    assert!(why.contains("`assetIdentifier`") && why.contains("{wellbore}"), "{why}");
}

#[test]
fn an_asset_identifier_that_is_not_an_energistics_uri_is_refused() {
    let why = planned(&bag(), choices().set("assetIdentifier", "http://lutetia/well({wellbore})")).unwrap_err();
    assert!(why.contains("`assetIdentifier`") && why.contains("eml:"), "{why}");
}

#[test]
fn a_frozen_window_starting_at_the_first_sample_is_refused() {
    let why = planned(
        &bag(),
        choices().set("frozen", "pressure").set("frozenFrom", "2025-06-02T00:00:00Z").set("frozenMinutes", 60),
    )
    .unwrap_err();
    assert!(why.contains("`frozenFrom`") && why.contains("first sample"), "{why}");
}

#[test]
fn a_frozen_window_outside_the_head_series_is_refused() {
    let why = planned(
        &bag(),
        choices().set("frozen", "temperature").set("frozenFrom", "2025-06-10T00:00:00Z").set("frozenMinutes", 60),
    )
    .unwrap_err();
    assert!(why.contains("`frozenFrom`"), "{why}");
}

#[test]
fn a_blank_source_is_refused_by_name() {
    let why = planned(&bag(), choices().set("source", "  ")).unwrap_err();
    assert!(why.contains("`source`") && why.contains("invented"), "{why}");
}
