//! `pressure-transient-analysis` — a PTA of a flow test, one homogeneous layer,
//! citing the grid properties that distribute its permeability and porosity
//!.
//!
//! Every object is read back through `energyml_core::json::read_value` into
//! the generated PRODML 2.3 type before anything is asserted: a field written
//! under the wrong name, or a required one left out, fails there — not in an
//! assertion on a JSON the operation itself shaped.
//!
//! # What would have to break to make each test turn red
//!
//! Writing the analytical model even when a grid property is given
//! turns `a_grid_property_makes_the_model_numerical_and_is_cited` red;
//! forgetting an `Is…Gridded` flag turns it red too; a wrong k·h turns
//! `the_constants_are_written_with_their_units` red; removing the check of
//! the class turns `an_object_that_is_no_flow_test_is_refused` red.

use serde_json::Value;

use energyml_build::operation::{Choices, Identity, NewObject, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_core::registry::DynObject;
use energyml_prodml_2_3 as prodml;

const TEST: &str = "2d2d2d2d-0000-4000-8000-000000000011";
const PERMEABILITY: &str = "2d2d2d2d-0000-4000-8000-000000000412";
const POROSITY: &str = "2d2d2d2d-0000-4000-8000-000000000411";

fn when() -> When {
    When { creation: "2026-09-28T00:00:00Z".into(), originator: "test".into() }
}

fn object(uuid: &str, qualified: &str, title: &str) -> Source {
    Source::Object(Known { reference: Some(Ref::new(uuid, qualified, title)), ..Default::default() })
}

fn bag() -> Pieces {
    Pieces::empty()
        .with("test", object(TEST, "prodml23.InjectionFlowTest", "GTH-2 — injection test"))
        .with("permeabilityArray", object(PERMEABILITY, "resqml22.ContinuousProperty", "grid — permeability (kx)"))
        .with("porosityArray", object(POROSITY, "resqml22.ContinuousProperty", "grid — porosity"))
}

fn choices() -> Choices {
    Choices::new()
        .set("title", "GTH-2 — injection test analysis")
        .set("source", "synthetic — a trial source; to be sourced")
        .set("testPeriod", "step-rate")
        .set("permeability", 5.0)
        .set("thickness", 95.0)
        .set("porosity", 0.35)
        .set("initialPressure", 12.0)
}

fn planned(pieces: &Pieces, choices: Choices) -> Result<Plan, String> {
    let plan = Registry::new().plan("pressure-transient-analysis", pieces, &choices, &Identity::derived(), &when()).map_err(|why| why.to_string())?;
    // What is planned is checked against the schema, every time: this plan wrote
    // `k`, `kh` and `phi` where PtaParameters.xsd fixes `K`, `k.h` and `Phi`
    // (`fixed=`), and no test looked.
    let broken = energyml_build::apply::conformance(&plan, &energyml_schemas::all());
    assert!(broken.is_empty(), "objects planned against their schema: {broken:#?}");
    Ok(plan)
}

fn the_analysis(plan: &Plan) -> (&NewObject, prodml::PressureTransientAnalysis) {
    assert_eq!(plan.objects.len(), 1, "one object: {:?}", plan.objects.iter().map(|o| &o.qualified_type).collect::<Vec<_>>());
    let object = &plan.objects[0];
    assert_eq!(object.qualified_type, "prodml23.PressureTransientAnalysis");
    let schemas = energyml_schemas::all();
    let document = energyml_core::json::read_value(&object.json, &schemas).unwrap_or_else(|why| panic!("the analysis does not read back: {why}"));
    let root: &dyn DynObject = &*document.root;
    let analysis = root.as_reflect().downcast_ref::<prodml::PressureTransientAnalysis>().expect("a PressureTransientAnalysis").clone();
    (object, analysis)
}

fn reservoir(analysis: &prodml::PressureTransientAnalysis) -> &prodml::ReservoirBaseModel {
    assert_eq!(analysis.layer_model.len(), 1, "one layer");
    analysis.layer_model[0].reservoir_model.as_ref().expect("a reservoir model")
}

/// **Given a grid property, the model is the numerical one and cites it.**
/// The flags say which parameters the grid holds, and only those.
#[test]
fn a_grid_property_makes_the_model_numerical_and_is_cited() {
    let plan = planned(&bag(), choices()).unwrap_or_else(|why| panic!("{why}"));
    let (_, analysis) = the_analysis(&plan);
    let prodml::ReservoirBaseModel::NumericalHomogeneousReservoirModel(model) = reservoir(&analysis) else {
        panic!("a numerical homogeneous model: {:?}", reservoir(&analysis));
    };
    let grid = &model.distributed_parameters_sub_model;
    assert!(grid.is_permeability_gridded && grid.is_porosity_gridded);
    assert!(!grid.is_thickness_gridded && !grid.is_depth_gridded && !grid.is_kv_to_kr_gridded && !grid.is_kx_to_ky_gridded);
    assert_eq!(grid.permeability_array_ref_id.as_ref().unwrap().resqml_model_ref.uuid, PERMEABILITY);
    assert_eq!(grid.porosity_array_ref_id.as_ref().unwrap().resqml_model_ref.uuid, POROSITY);
    assert_eq!(analysis.is_numerical_analysis, Some(true));
}

/// **Without one, the model is the analytical `HomogeneousModel`**: only the
/// numerical model can say a parameter is on a grid, so nothing is said.
#[test]
fn without_a_grid_property_the_model_is_analytical() {
    let pieces = Pieces::empty().with("test", object(TEST, "prodml23.InjectionFlowTest", "GTH-2 — injection test"));
    let plan = planned(&pieces, choices()).unwrap_or_else(|why| panic!("{why}"));
    let (_, analysis) = the_analysis(&plan);
    assert!(matches!(reservoir(&analysis), prodml::ReservoirBaseModel::HomogeneousModel(_)), "{:?}", reservoir(&analysis));
    assert_eq!(analysis.is_numerical_analysis, Some(false));
}

/// **The constants, with their units, and k·h their product** — both models
/// require them.
#[test]
fn the_constants_are_written_with_their_units() {
    let plan = planned(&bag(), choices()).unwrap();
    let (object, analysis) = the_analysis(&plan);
    let prodml::ReservoirBaseModel::NumericalHomogeneousReservoirModel(model) = reservoir(&analysis) else { panic!() };
    assert_eq!(model.horizontal_radial_permeability.permeability.value, 5.0);
    assert_eq!(model.total_thickness.length.value, 95.0);
    assert_eq!(model.permeability_thickness_product.permeability_length.value, 5.0 * 95.0);
    assert_eq!(model.porosity.volume_per_volume.value, 0.35);
    assert_eq!(model.initial_pressure.pressure.value, 12.0);
    // The units as the file spells them.
    let text = object.json.to_string();
    for unit in ["\"mD\"", "\"mD.m\"", "\"m\"", "\"Euc\"", "\"bar\""] {
        assert!(text.contains(unit), "{unit} in {text}");
    }
}

/// **The test, once, and the period of it that is analysed**.
#[test]
fn the_analysis_names_its_test_and_the_period() {
    let plan = planned(&bag(), choices()).unwrap();
    let (object, analysis) = the_analysis(&plan);
    assert_eq!(analysis.flow_test_activity.uuid, TEST);
    assert_eq!(analysis.flow_test_activity.qualified_type, "prodml23.InjectionFlowTest");
    assert_eq!(analysis.principal_test_period_ref, vec!["step-rate".to_string()]);
    assert!(matches!(analysis.fluid_phase_analysis_kind, prodml::FluidPhaseKind::SinglePhaseWater));
    // Invented, and saying so.
    let synthetic = &object.json["ExtensionNameValue"];
    assert!(synthetic.as_array().is_some_and(|all| !all.is_empty()), "{synthetic}");
    assert!(object.json["Citation"]["Description"].as_str().unwrap().contains("a trial source"));
    assert!(plan.says.iter().any(|note| note.key == "test-period"), "{:?}", plan.says);
}

/// **An object that is no flow test is refused**, before anything is planned.
#[test]
fn an_object_that_is_no_flow_test_is_refused() {
    let pieces = Pieces::empty().with("test", object(TEST, "prodml23.ProductionWellTests", "GTH-2 — well tests"));
    let why = planned(&pieces, choices()).unwrap_err();
    assert!(why.contains("`test`") && why.contains("no PRODML flow test"), "{why}");
}

/// **What cannot be analysed is refused by its slot**: no period, a porosity
/// that is no fraction, a blank source.
#[test]
fn what_cannot_be_analysed_is_refused_by_its_slot() {
    let why = planned(&bag(), choices().set("testPeriod", "  ")).unwrap_err();
    assert!(why.contains("testPeriod"), "{why}");
    let why = planned(&bag(), choices().set("porosity", 1.5)).unwrap_err();
    assert!(why.contains("porosity"), "{why}");
    let why = planned(&bag(), choices().set("source", " ")).unwrap_err();
    assert!(why.contains("source"), "{why}");
    let why = planned(&bag(), choices().set("phase", "lava")).unwrap_err();
    assert!(why.contains("phase") && why.contains("lava"), "{why}");
}

/// **The same analysis twice is one object**: the uuid comes from the test,
/// the period and the title.
#[test]
fn the_same_analysis_is_the_same_object() {
    let first = planned(&bag(), choices()).unwrap();
    let again = planned(&bag(), choices()).unwrap();
    assert_eq!(first.objects[0].uuid, again.objects[0].uuid);
    let other = planned(&bag(), choices().set("title", "another analysis")).unwrap();
    assert_ne!(first.objects[0].uuid, other.objects[0].uuid);
    let _: &Value = &first.objects[0].json;
}
