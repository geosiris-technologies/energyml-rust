//! The rest of Lutetia's PRODML: the
//! sample chain (`fluid-sample`), the fiber (`dts-installation`,
//! DTS) and the loop's flow model (`product-flow-model`).
//!
//! **The form of each assertion comes from the schema and the reference-model description — which
//! object names which , what a unit's ports are — and the values
//! from the bag.** Every JSON a plan writes is read back into its generated
//! PRODML type before anything is asserted of it, the road `apply` takes.
//!
//! The analysis's arithmetic is confronted with two sources the operation does
//! not own: `energyml_analysis::ion_balance` called directly, and the
//! textbook factors of hardness (2.497 for calcium, 4.118 for magnesium),
//! which are rounded from molar masses nobody here typed.

use serde_json::json;

use energyml_build::operation::{Choices, Identity, NewObject, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_core::registry::DynObject;
use energyml_domain::provenance::Parameter;
use energyml_prodml_2_3 as prodml;

const GTH1: &str = "2b2b2b2b-0000-4000-8000-000000000001";
const GTH2: &str = "2b2b2b2b-0000-4000-8000-000000000002";
const GTH3: &str = "2b2b2b2b-0000-4000-8000-000000000003";
const FLUID: &str = "2b2b2b2b-0000-4000-8000-000000000004";
const FACILITY: &str = "2b2b2b2b-0000-4000-8000-000000000005";
const ENTITY1: &str = "2b2b2b2b-0000-4000-8000-000000000006";
const ENTITY2: &str = "2b2b2b2b-0000-4000-8000-000000000007";

fn when() -> When {
    When { creation: "2026-09-25T00:00:00Z".into(), originator: "test".into() }
}

fn object(uuid: &str, qualified_type: &str, title: &str) -> Source {
    Source::Object(Known { reference: Some(Ref::new(uuid, qualified_type, title)), ..Default::default() })
}

fn planned(operation: &str, pieces: &Pieces, choices: Choices) -> Result<Plan, String> {
    Registry::new().plan(operation, pieces, &choices, &Identity::derived(), &when()).map_err(|why| why.to_string())
}

fn of_type<'a>(plan: &'a Plan, qualified: &str) -> &'a NewObject {
    let found: Vec<&NewObject> = plan.objects.iter().filter(|one| one.qualified_type == qualified).collect();
    assert_eq!(found.len(), 1, "one `{qualified}` in the plan: {:?}", plan.objects.iter().map(|o| &o.qualified_type).collect::<Vec<_>>());
    found[0]
}

/// The JSON of a plan's object, read back into its generated type.
fn read<T: energyml_core::value::Reflect + Clone>(object: &NewObject) -> T {
    let schemas = energyml_schemas::all();
    let document = energyml_core::json::read_value(&object.json, &schemas).unwrap_or_else(|why| panic!("`{}` does not read back: {why}", object.title));
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

fn synthetic(extensions: &[energyml_eml_2_3::ExtensionNameValue]) -> bool {
    extensions.iter().any(|one| one.name == "synthetic" && one.value.value == "true")
}

// ================================================================ fluid-sample

/// A calcium bicarbonate water with gypsum sulfate — the reference-model example in
/// order of magnitude — chosen neutral.
fn ions() -> Vec<&'static str> {
    vec!["Ca+2=150", "Mg+2=15", "Na+=25", "K+=4", "Ba+2=0.05", "HCO3-=330", "SO4-2=150", "Cl-=40"]
}

fn sample_bag() -> Pieces {
    Pieces::empty()
        .with("wellbore", object(GTH1, "witsml21.Wellbore", "GTH-1"))
        .with("fluidSystem", object(FLUID, "prodml23.FluidSystem", "Craie water"))
}

fn sample_choices() -> Choices {
    Choices::new()
        .set("title", "GTH-1 — sample")
        .set("source", "synthetic — a trial source")
        .set("sampledAt", "2025-03-12T09:00:00Z")
        .set("sampleMd", 280.0)
        .set("standardConditions", "15 degC 1 atm")
        .set("components", json!(ions()))
        .set("waterDensity", 999.0)
        .set("waterViscosity", 1.11)
        .set("waterCompressibility", 4.6e-5)
        .set("referenceTemperature", 16.5)
}

#[test]
fn the_sample_chain_names_its_wellbore_its_fluid_and_one_another() {
    // The acquisition names the wellbore given and the fluid
    // system; the job, the container, the sample and the analysis name one
    // another. To break: a DOR to the wrong object of the plan, or the
    // acquisition's uid not the one the sample names back.
    let plan = planned("fluid-sample", &sample_bag(), sample_choices()).expect("a plan");
    let job: prodml::FluidSampleAcquisitionJob = read(of_type(&plan, "prodml23.FluidSampleAcquisitionJob"));
    let container = of_type(&plan, "prodml23.FluidSampleContainer");
    let sample_object = of_type(&plan, "prodml23.FluidSample");
    let sample: prodml::FluidSample = read(sample_object);
    let analysis_object = of_type(&plan, "prodml23.WaterAnalysis");
    let analysis: prodml::WaterAnalysis = read(analysis_object);
    let characterization: prodml::FluidCharacterization = read(of_type(&plan, "prodml23.FluidCharacterization"));

    assert_eq!(job.fluid_system.uuid, FLUID, "FluidSystem 1..1 → the fluid system given");
    assert_eq!(job.fluid_sample_acquisition.len(), 1, "one acquisition per sample (Usage Guide v2.3 §10.3 p. 77)");
    let prodml::AnyFluidSampleAcquisition::DownholeSampleAcquisition(acquisition) = &job.fluid_sample_acquisition[0] else {
        panic!("a downhole acquisition: the schema names `DownholeSampleAcquisition.Wellbore`")
    };
    assert_eq!(acquisition.wellbore.uuid, GTH1, "Wellbore 1..1 → the wellbore given");
    assert_eq!(acquisition.top_md.value, 280.0);
    assert_eq!(acquisition.fluid_sample_container.uuid, container.uuid);
    assert_eq!(acquisition.fluid_sample.uuid, sample_object.uuid);

    let source = sample.fluid_sample_acquisition_job_source.as_ref().expect("the sample names its acquisition");
    assert_eq!(source.fluid_sample_acquisition_job_reference.uuid, job.uuid);
    assert_eq!(source.fluid_sample_acquisition_reference, acquisition.uid, "the sample names the acquisition by its uid");
    assert_eq!(sample.original_sample_container.as_ref().map(|one| one.uuid.as_str()), Some(container.uuid.as_str()));
    assert_eq!(sample.fluid_system.as_ref().map(|one| one.uuid.as_str()), Some(FLUID));
    assert_eq!(sample.sample_kind, Some(prodml::FluidSampleKindExt::Water));

    assert_eq!(analysis.fluid_sample.uuid, sample_object.uuid, "FluidSample 1..1");
    let from = characterization.source.first().expect("the characterization says where it comes from");
    assert_eq!(from.fluid_analysis.as_ref().map(|one| one.uuid.as_str()), Some(analysis_object.uuid.as_str()));
    let test = analysis.water_analysis_test.first().expect("a test");
    assert_eq!(from.fluid_analysis_test_reference, vec![test.uid.clone()], "the test named is the analysis's own");
    assert_eq!(characterization.fluid_system.as_ref().map(|one| one.uuid.as_str()), Some(FLUID));

    // §3, rule 4: every object says it is invented.
    assert!(synthetic(&job.extension_name_value) && synthetic(&sample.extension_name_value));
    assert!(synthetic(&analysis.extension_name_value) && synthetic(&characterization.extension_name_value));
}

#[test]
fn each_ion_is_written_as_the_anion_or_cation_it_is_in_mg_per_litre() {
    // To break: a cation written as an anion, a value in the wrong field or
    // the wrong unit.
    let plan = planned("fluid-sample", &sample_bag(), sample_choices()).expect("a plan");
    let analysis: prodml::WaterAnalysis = read(of_type(&plan, "prodml23.WaterAnalysis"));
    assert_eq!(analysis.water_sample_component.len(), ions().len());
    for (written, stated) in analysis.water_sample_component.iter().zip(ions()) {
        let (formula, value) = stated.split_once('=').unwrap();
        let value: f64 = value.parse().unwrap();
        let named = match (&written.anion, &written.cation) {
            (Some(anion), None) => {
                assert!(formula.ends_with('-') || formula.contains('-'), "{formula} written as an anion");
                anion.as_xml_str().to_string()
            }
            (None, Some(cation)) => {
                assert!(formula.contains('+'), "{formula} written as a cation");
                cation.as_xml_str().to_string()
            }
            other => panic!("{formula}: one of anion or cation, not {other:?}"),
        };
        assert_eq!(named, formula);
        let concentration = written.volume_concentration.as_ref().expect("a mass per volume");
        assert_eq!(concentration.uom, energyml_eml_2_3::MassPerVolumeUomExt::MgPerL);
        assert_eq!(concentration.value, value);
        assert!(written.mass_concentration.is_none(), "g/L is a mass per volume, never a MassConcentration");
    }
    assert_eq!(analysis.analysis_quality, prodml::SampleQuality::Unknown, "nobody judged this water");
}

#[test]
fn the_balance_and_the_hardness_agree_with_two_sources_the_operation_does_not_own() {
    let plan = planned("fluid-sample", &sample_bag(), sample_choices()).expect("a plan");

    // The balance, by the analysis crate called directly.
    let pairs: Vec<(&str, f64)> = ions().iter().map(|one| {
        let (formula, value) = one.split_once('=').unwrap();
        (formula, value.parse().unwrap())
    }).collect();
    let direct = energyml_analysis::ion_balance::balance(&pairs).unwrap();
    assert!((double_param(&plan, "balanceError") - direct.error).abs() < 1e-12);
    assert!((double_param(&plan, "cationCharge") - direct.cations).abs() < 1e-12);
    assert!(direct.error.abs() < 5.0, "the fixture is neutral within 5 %: {direct:?}");

    // The hardness, by the textbook factors: 2.497 × Ca + 4.118 × Mg, mg/L
    // as CaCO3 — rounded from molar masses, so an agreement within 0.05 %.
    let analysis: prodml::WaterAnalysis = read(of_type(&plan, "prodml23.WaterAnalysis"));
    let hardness = analysis.water_analysis_test[0].total_hardness_per_volume.as_ref().expect("a hardness, computed").value;
    let textbook = 2.497 * 150.0 + 4.118 * 15.0;
    assert!((hardness - textbook).abs() / textbook < 5e-4, "{hardness} against {textbook}");
    assert!((double_param(&plan, "totalHardness") - hardness).abs() < 1e-12);
}

#[test]
fn an_analysis_that_does_not_balance_is_refused_with_both_sums() {
    // Without its bicarbonate the water carries far more positive charge.
    let unbalanced: Vec<&str> = ions().into_iter().filter(|one| !one.starts_with("HCO3")).collect();
    let why = planned("fluid-sample", &sample_bag(), sample_choices().set("components", json!(unbalanced))).unwrap_err();
    assert!(why.contains("`components`") && why.contains("do not balance") && why.contains("meq/L"), "{why}");
    // A wider tolerance lets the same ions through: the refusal is the
    // tolerance's, not a fixed rule.
    let wide = planned(
        "fluid-sample",
        &sample_bag(),
        sample_choices().set("components", json!(ions().into_iter().filter(|one| !one.starts_with("Cl")).collect::<Vec<_>>())).set("balanceTolerance", 10.0),
    );
    assert!(wide.is_ok(), "{wide:?}");
}

#[test]
fn nitrate_an_unknown_ion_and_a_malformed_component_are_refused_by_name() {
    let mut with_nitrate = ions();
    with_nitrate.push("NO3-=10");
    let why = planned("fluid-sample", &sample_bag(), sample_choices().set("components", json!(with_nitrate))).unwrap_err();
    assert!(why.contains("NO3-2") && why.contains("nitrate"), "{why}");

    let mut unknown = ions();
    unknown.push("Xx+=3");
    let why = planned("fluid-sample", &sample_bag(), sample_choices().set("components", json!(unknown))).unwrap_err();
    assert!(why.contains("`Xx+`"), "{why}");

    let mut malformed = ions();
    malformed.push("Ca+2 150");
    let why = planned("fluid-sample", &sample_bag(), sample_choices().set("components", json!(malformed))).unwrap_err();
    assert!(why.contains("formula=mg/L"), "{why}");
}

#[test]
fn a_well_is_refused_where_a_sample_is_taken_in_a_wellbore() {
    let with_well = Pieces::empty()
        .with("wellbore", object(GTH1, "witsml21.Well", "GTH-1"))
        .with("fluidSystem", object(FLUID, "prodml23.FluidSystem", "Craie water"));
    let why = planned("fluid-sample", &with_well, sample_choices()).unwrap_err();
    assert!(why.contains("`wellbore`") && why.contains("`Well`"), "{why}");
}

#[test]
fn the_same_sample_keeps_its_uuids_and_another_analysis_is_a_second_object() {
    let one = planned("fluid-sample", &sample_bag(), sample_choices()).expect("a plan");
    let again = planned("fluid-sample", &sample_bag(), sample_choices()).expect("a plan");
    let uuids = |plan: &Plan| plan.objects.iter().map(|o| o.uuid.clone()).collect::<Vec<_>>();
    assert_eq!(uuids(&one), uuids(&again), "the same content, the same objects");

    let mut other_ions = ions();
    other_ions[2] = "Na+=26";
    let other = planned("fluid-sample", &sample_bag(), sample_choices().set("components", json!(other_ions))).expect("a plan");
    assert_eq!(of_type(&one, "prodml23.FluidSample").uuid, of_type(&other, "prodml23.FluidSample").uuid, "the same sample");
    assert_ne!(of_type(&one, "prodml23.WaterAnalysis").uuid, of_type(&other, "prodml23.WaterAnalysis").uuid, "another analysis of it");
}

// ================================================================ dts-installation

fn dts_choices() -> Choices {
    Choices::new()
        .set("title", "GTH-3 — DTS")
        .set("source", "synthetic — a trial source")
        .set("installedFrom", "2025-01-01T00:00:00Z")
        .set("measuredAt", "2025-06-02T00:00:00Z")
        .set("fiberLength", 203.0)
}

#[test]
fn the_fiber_is_mapped_onto_the_wellbore_given_and_the_system_pairs_what_the_plan_wrote() {
    // FiberFacilityWell.Wellbore 1..1 → the wellbore given. To break:
    // the mapping names another hole, the system names an object the plan
    // did not write, or the survey claims traces.
    let bag = Pieces::empty().with("wellbore", object(GTH3, "witsml21.Wellbore", "GTH-3"));
    let plan = planned("dts-installation", &bag, dts_choices()).expect("a plan");
    let path_object = of_type(&plan, "prodml23.FiberOpticalPath");
    let box_object = of_type(&plan, "prodml23.DtsInstrumentBox");
    let system_object = of_type(&plan, "prodml23.DtsInstalledSystem");
    let path: prodml::FiberOpticalPath = read(path_object);
    let system: prodml::DtsInstalledSystem = read(system_object);
    let measurement: prodml::DtsMeasurement = read(of_type(&plan, "prodml23.DtsMeasurement"));
    let _: prodml::DtsInstrumentBox = read(box_object);

    assert_eq!(path.inventory.segment.len(), 1, "one segment the length of the path (Usage Guide v2.3 §19.3 p. 154)");
    assert_eq!(path.inventory.segment[0].fiber_length.value, 203.0);
    assert_eq!(path.inventory.terminator.termination_type, prodml::TerminationKind::TerminationAtCable);
    let part = &path.facility_mapping[0].fiber_facility_mapping_part[0];
    let Some(prodml::AbstractFiberFacility::FiberFacilityWell(well)) = &part.fiber_facility else {
        panic!("a well facility")
    };
    assert_eq!(well.wellbore.uuid, GTH3);
    assert_eq!((part.optical_path_distance_end.value, part.facility_length_end.value), (203.0, 203.0));

    assert_eq!(system.optical_path.uuid, path_object.uuid);
    assert_eq!(system.instrument_box.uuid, box_object.uuid, "§19.5 p. 162: the system points to the box and the path");
    assert_eq!(measurement.installed_system.uuid, system_object.uuid);
    assert!(measurement.empty_set_flag && !measurement.bad_set_flag, "a header only: empty, not bad");
    assert!(measurement.measurement_trace.is_empty() && measurement.interpretation_log.is_none());
    assert!(synthetic(&path.extension_name_value) && synthetic(&measurement.extension_name_value));
}

#[test]
fn a_survey_before_its_installation_is_refused() {
    let bag = Pieces::empty().with("wellbore", object(GTH3, "witsml21.Wellbore", "GTH-3"));
    let why = planned("dts-installation", &bag, dts_choices().set("measuredAt", "2024-12-31T00:00:00Z")).unwrap_err();
    assert!(why.contains("`measuredAt`") && why.contains("before the installation"), "{why}");
}

#[test]
fn an_object_that_is_no_hole_is_unfit_without_its_title_being_echoed() {
    // The offer names the slot and the kind; the title is the producer's.
    let bag = Pieces::empty().with("wellbore", object(GTH3, "resqml22.TriangulatedSetRepresentation", "Surface v2.0.1"));
    let registry = Registry::new();
    let offer = registry.offers(&bag).into_iter().find(|one| one.operation.name() == "dts-installation").expect("offered");
    let energyml_build::operation::Fitness::Unfit(why) = offer.fitness else { panic!("a surface is no hole") };
    assert!(why.message.contains("no wellbore") && !why.message.contains("v2.0.1"), "{}", why.message);
}

// ================================================================ product-flow-model

fn loop_bag(order: &[(&str, &str, &str)]) -> Pieces {
    let mut bag = Pieces::empty();
    for (at, (uuid, qualified, title)) in order.iter().enumerate() {
        let name = if at == 0 { "units".to_string() } else { format!("units {}", at + 1) };
        bag = bag.with(name, object(uuid, qualified, title));
    }
    bag
}

fn flow_choices() -> Choices {
    Choices::new().set("title", "Lutetia — loop").set("source", "synthetic — a trial source")
}

/// The unit names in the order a network's nodes chain them, from the ports
/// alone: start where no inlet is, follow each outlet's node to the inlet
/// that names it. A second reading of the graph than the order of the list.
fn walked(network: &prodml::ProductFlowNetwork) -> Vec<String> {
    let inlet_node = |unit: &prodml::ProductFlowUnit| {
        unit.port.iter().find(|p| p.direction == prodml::ProductFlowPortType::Inlet).map(|p| p.connected_node[0].node.clone())
    };
    let outlet_node = |unit: &prodml::ProductFlowUnit| {
        unit.port.iter().find(|p| p.direction == prodml::ProductFlowPortType::Outlet).map(|p| p.connected_node[0].node.clone())
    };
    let starts: Vec<&prodml::ProductFlowUnit> = network.unit.iter().filter(|u| inlet_node(u).is_none()).collect();
    assert_eq!(starts.len(), 1, "one unit without inlet: where the flow starts");
    let mut out = vec![];
    let mut here = starts[0];
    loop {
        out.push(here.facility.as_ref().expect("a facility name").value.clone());
        let Some(node) = outlet_node(here) else { break };
        let next: Vec<&prodml::ProductFlowUnit> = network.unit.iter().filter(|u| inlet_node(u).as_deref() == Some(node.as_str())).collect();
        assert_eq!(next.len(), 1, "one inlet on node `{node}`");
        here = next[0];
        assert!(out.len() <= network.unit.len(), "a loop in the chain");
    }
    out
}

#[test]
fn the_units_flow_one_into_the_next_in_the_order_given() {
    // The links are names. To break: an outlet and the next inlet on
    // two different nodes, or the units chained in any order but the slot's.
    let forward = [(ENTITY1, "prodml23.ReportingEntity", "GTH-1"), (FACILITY, "prodml23.Facility", "Exchanger"), (ENTITY2, "prodml23.ReportingEntity", "GTH-2")];
    let plan = planned("product-flow-model", &loop_bag(&forward), flow_choices()).expect("a plan");
    let model: prodml::ProductFlowModel = read(of_type(&plan, "prodml23.ProductFlowModel"));
    assert_eq!(model.network.len(), 1);
    assert_eq!(walked(&model.network[0]), vec!["GTH-1", "Exchanger", "GTH-2"]);
    assert!(synthetic(&model.extension_name_value));

    // The same objects the other way round: the other way through.
    let backward: Vec<(&str, &str, &str)> = forward.iter().rev().copied().collect();
    let reversed = planned("product-flow-model", &loop_bag(&backward), flow_choices()).expect("a plan");
    let model: prodml::ProductFlowModel = read(of_type(&reversed, "prodml23.ProductFlowModel"));
    assert_eq!(walked(&model.network[0]), vec!["GTH-2", "Exchanger", "GTH-1"]);
    assert_ne!(of_type(&plan, "prodml23.ProductFlowModel").uuid, of_type(&reversed, "prodml23.ProductFlowModel").uuid);
}

#[test]
fn a_wellbore_unit_says_it_is_one_and_the_rest_are_not_guessed() {
    let chain = [(GTH1, "witsml21.Wellbore", "GTH-1"), (FACILITY, "prodml23.Facility", "Exchanger"), (GTH2, "witsml21.Wellbore", "GTH-2")];
    let plan = planned("product-flow-model", &loop_bag(&chain), flow_choices()).expect("a plan");
    let model: prodml::ProductFlowModel = read(of_type(&plan, "prodml23.ProductFlowModel"));
    let kinds: Vec<Option<prodml::ReportingFacility>> =
        model.network[0].unit.iter().map(|u| u.facility.as_ref().and_then(|f| f.kind.clone())).collect();
    assert_eq!(kinds, vec![Some(prodml::ReportingFacility::Wellbore), None, Some(prodml::ReportingFacility::Wellbore)]);
}

#[test]
fn a_unit_given_twice_is_refused_by_its_name() {
    let twice = [(ENTITY1, "prodml23.ReportingEntity", "GTH-1"), (FACILITY, "prodml23.Facility", "Exchanger"), (ENTITY1, "prodml23.ReportingEntity", "GTH-1")];
    let why = planned("product-flow-model", &loop_bag(&twice), flow_choices()).unwrap_err();
    assert!(why.contains("`units`") && why.contains("`GTH-1`") && why.contains("unit 1") && why.contains("unit 3"), "{why}");
}
