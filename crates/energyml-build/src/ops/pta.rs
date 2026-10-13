//! `pressure-transient-analysis` — a PRODML `PressureTransientAnalysis` of a
//! flow test: one homogeneous layer, its permeability, thickness, porosity and
//! initial pressure, and — when they are given — the RESQML grid properties
//! that distribute the permeability and the porosity.
//!
//! The reference model's analysis that cites the grid:
//! `PressureTransientAnalysis` → `DistributedParametersSubModel.
//! PermeabilityArrayRefID` / `PorosityArrayRefID` → the grid properties. The
//! red thread between RESQML and PRODML — homogeneous, its
//! `FlowTestActivity` given once and its two array references to the
//! grid.
//!
//! # Numerical when the grid is given, analytical otherwise
//!
//! PRODML has two homogeneous reservoir models. `HomogeneousModel` states
//! constants; `NumericalHomogeneousReservoirModel` *“may have constant value
//! or reference a grid of geometrically distributed values”* and carries the
//! `DistributedParametersSubModel` whose `Is…Gridded` flags say which
//! parameters the grid holds (`pta_models`, PRODML 2.3). So the model is the
//! numerical one exactly when a grid property is given — the only one that
//! can say so — and the analytical one otherwise. The constants are written
//! either way: both models require them.
//!
//! # What is referenced, and what is not read
//!
//! `PermeabilityArrayRefID` and `PorosityArrayRefID` name the **properties**
//! as the reference model does — *“Reference to RESQML grid containing
//! Permeability values”*, and the values are the property's. Lutetia's
//! permeability is three scalar properties (`grid-properties`, kx, ky, kz):
//! the horizontal radial permeability is kx's.
//!
//! `PrincipalTestPeriodRef` names a test period **of the flow test** by its
//! uid, and this operation cannot read the flow test: an object in the bag is
//! a reference, not its content (`Known::json` is a door still to be decided,
//! as an open point). So the uid is **told** (`testPeriod`), and
//! `crates/energyml-build/tests/lutetia.rs` checks, in the written package,
//! that the period named is one the test holds — a second source of the same
//! fact rather than trust.
//!
//! # Every value is invented
//!
//! Like every PRODML object of Lutetia: `synthetic = true`, and the source in
//! `Citation/Description`; a blank source is
//! refused.

use energyml_core::xml::XmlText;
use energyml_domain::provenance::Parameter;
use energyml_eml_2_3 as eml;
use energyml_prodml_2_3 as prodml;

use super::prodml_doublet::{class_of, titled};
use super::prodml_writer::Writer;
use crate::declare::{Inputs, ObjectIn, Params};
use crate::operation::{About, ActivityDraft, Category, Context, Note, Operation, Plan, Refusal};
use crate::pieces::Ref;

/// The operation.
pub struct PressureTransientAnalysis;

/// The PRODML classes a `FlowTestActivity` is (`flow_test_activity`, PRODML 2.3).
const FLOW_TESTS: &[&str] = &[
    "DrillStemTest",
    "FormationTesterStation",
    "InjectionFlowTest",
    "InterwellTest",
    "ProductionFlowTest",
    "ProductionTransientTest",
    "VerticalInterferenceTest",
    "WaterLevelTest",
];

/// What `pressure-transient-analysis` is told.
#[derive(Debug, Params)]
pub struct PtaParams {
    /// the title of the analysis
    pub title: String,
    /// where the values come from — a document with its page or paragraph; a value stated as plausible says so (synthetic, and why)
    pub source: String,
    /// the uid of the test period analysed, a period of the flow test given — `PrincipalTestPeriodRef`
    pub test_period: String,
    /// the model's name — `ModelName`, a description with no semantic meaning
    #[param(default = String::from("homogeneous"), optional)]
    pub model_name: String,
    /// the phases analysed — single phase water, single phase oil, single phase gas, or a multiphase kind of the PRODML list
    #[param(default = String::from("single phase water"), optional)]
    pub phase: String,
    /// the horizontal radial permeability, in mD
    #[param(uom = "mD", min = 0.0)]
    pub permeability: f64,
    /// the total thickness of the layer, in m
    #[param(uom = "m", min = 0.0)]
    pub thickness: f64,
    /// the porosity, a fraction of the rock's volume
    #[param(min = 0.0, max = 1.0)]
    pub porosity: f64,
    /// the initial pressure of the reservoir, in bar
    #[param(uom = "bar", min = 0.0)]
    pub initial_pressure: f64,
}

/// What `pressure-transient-analysis` takes.
#[derive(Debug, Inputs)]
pub struct PtaInputs {
    /// the flow test analysed — its `FlowTestActivity`, given once
    #[input(
        types = "prodml23.InjectionFlowTest, prodml23.DrillStemTest, prodml23.ProductionTransientTest, prodml23.ProductionFlowTest, prodml23.InterwellTest, prodml23.VerticalInterferenceTest, prodml23.WaterLevelTest, prodml23.FormationTesterStation"
    )]
    pub test: ObjectIn,
    /// the permeability distributed on a grid — a property of the grid's cells; the model is then the numerical one
    #[input(types = "resqml22.ContinuousProperty")]
    pub permeability_array: Option<ObjectIn>,
    /// the porosity distributed on a grid — a property of the grid's cells; the model is then the numerical one
    #[input(types = "resqml22.ContinuousProperty")]
    pub porosity_array: Option<ObjectIn>,
}

/// The reference an object of the bag carries, or a refusal naming its slot.
fn reference(slot: &str, input: &ObjectIn) -> Result<Ref, Refusal> {
    input.known.reference.clone().ok_or_else(|| {
        Refusal::on(
            slot,
            format!("`{}` was given without a reference to write", input.name),
        )
    })
}

/// A finite number, or a refusal naming the slot.
fn finite(slot: &str, value: f64, what: &str) -> Result<f64, Refusal> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(Refusal::on(slot, format!("{value} is not {what}")))
    }
}

/// A unit code as the schema spells it, or a refusal.
fn unit<T: XmlText>(code: &str) -> Result<T, Refusal> {
    T::from_xml_text(code)
        .map_err(|why| Refusal::new("refused", format!("`{code}` is no unit this build writes: {why}")))
}

impl Operation for PressureTransientAnalysis {
    type Params = PtaParams;
    type Inputs = PtaInputs;

    fn about(&self) -> About {
        About {
            name: "pressure-transient-analysis",
            describe: "a pressure transient analysis of a flow test — one homogeneous layer, its permeability, thickness, porosity and initial pressure, numerical when grid properties distribute them — every value stated and sourced",
            category: Category::Synthetic,
            makes: &["prodml23.PressureTransientAnalysis"],
            recipe: "pressure-transient-analysis",
            represents: None,
        }
    }

    fn check(&self, inputs: &PtaInputs) -> Result<(), Refusal> {
        match inputs
            .test
            .known
            .reference
            .as_ref()
            .map(|one| class_of(&one.qualified_type))
        {
            Some(class) if FLOW_TESTS.contains(&class) => Ok(()),
            _ => Err(Refusal::on(
                "test",
                "the object given is no PRODML flow test: an analysis is of one",
            )),
        }
    }

    fn plan(&self, inputs: PtaInputs, params: PtaParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        self.check(&inputs)?;
        let title = titled("title", &params.title)?;
        let mut writer = Writer::new(cx, "pressure-transient-analysis", &params.source)?;
        let test = reference("test", &inputs.test)?;
        let period = params.test_period.trim();
        if period.is_empty() {
            return Err(Refusal::on(
                "testPeriod",
                "is empty: an analysis names the period of the test it analyses",
            ));
        }
        let phase = prodml::FluidPhaseKind::from_xml_text(params.phase.trim()).map_err(|_| {
            Refusal::on(
                "phase",
                format!(
                    "`{}` is none of the PRODML fluid phase kinds (single phase water, single phase oil, …)",
                    params.phase.trim()
                ),
            )
        })?;

        let k = finite("permeability", params.permeability, "a permeability")?;
        let h = finite("thickness", params.thickness, "a thickness")?;
        let phi = finite("porosity", params.porosity, "a porosity")?;
        let pi = finite("initialPressure", params.initial_pressure, "a pressure")?;
        if k <= 0.0 || h <= 0.0 {
            return Err(Refusal::on(
                if k <= 0.0 { "permeability" } else { "thickness" },
                "is 0: a layer that holds or passes nothing has no transient to analyse",
            ));
        }
        if !(phi > 0.0 && phi <= 1.0) {
            return Err(Refusal::on(
                "porosity",
                format!("{phi} is no fraction of a volume in (0, 1]"),
            ));
        }

        let permeability_array = inputs
            .permeability_array
            .as_ref()
            .map(|one| reference("permeabilityArray", one))
            .transpose()?;
        let porosity_array = inputs
            .porosity_array
            .as_ref()
            .map(|one| reference("porosityArray", one))
            .transpose()?;
        let numerical = permeability_array.is_some() || porosity_array.is_some();

        // -- the constants, which both models require --------------------------------------
        //
        // **Each `Abbreviation` is the one the schema fixes** (`fixed=` in prodml
        // 2.3 PtaParameters.xsd: `K`, `h`, `k.h`, `Phi`, `Pi`). This wrote `k`,
        // `kh` and `phi` until 04/10/2026, when `fixed` became a facet that
        // `Validate` reads — the first thing it found.
        let permeability = prodml::HorizontalRadialPermeability {
            uid: "k".to_string(),
            abbreviation: "K".to_string(),
            permeability: eml::PermeabilityRockMeasureExt {
                value: k,
                uom: unit("mD")?,
            },
            ..Default::default()
        };
        let thickness = prodml::TotalThickness {
            uid: "h".to_string(),
            abbreviation: "h".to_string(),
            length: eml::LengthMeasure {
                value: h,
                uom: unit("m")?,
            },
            ..Default::default()
        };
        let kh = prodml::PermeabilityThicknessProduct {
            uid: "kh".to_string(),
            abbreviation: "k.h".to_string(),
            permeability_length: eml::PermeabilityLengthMeasureExt {
                value: k * h,
                uom: unit("mD.m")?,
            },
            ..Default::default()
        };
        let porosity = prodml::Porosity {
            uid: "phi".to_string(),
            abbreviation: "Phi".to_string(),
            volume_per_volume: eml::VolumePerVolumeMeasure {
                value: phi,
                uom: unit("Euc")?,
            },
            ..Default::default()
        };
        let initial = prodml::InitialPressure {
            uid: "pi".to_string(),
            abbreviation: "Pi".to_string(),
            pressure: eml::PressureMeasure {
                value: pi,
                uom: unit("bar")?,
            },
            ..Default::default()
        };

        let reservoir = if numerical {
            let grid_ref = |one: &Option<Ref>| {
                one.as_ref().map(|one| prodml::ResqmlModelRef {
                    resqml_model_ref: crate::crs::reference_of(one),
                })
            };
            prodml::ReservoirBaseModel::NumericalHomogeneousReservoirModel(
                prodml::NumericalHomogeneousReservoirModel {
                    horizontal_radial_permeability: permeability,
                    total_thickness: thickness,
                    permeability_thickness_product: kh,
                    porosity,
                    initial_pressure: initial,
                    distributed_parameters_sub_model: prodml::DistributedParametersSubModel {
                        is_permeability_gridded: permeability_array.is_some(),
                        permeability_array_ref_id: grid_ref(&permeability_array),
                        is_porosity_gridded: porosity_array.is_some(),
                        porosity_array_ref_id: grid_ref(&porosity_array),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
        } else {
            prodml::ReservoirBaseModel::HomogeneousModel(prodml::HomogeneousModel {
                horizontal_radial_permeability: permeability,
                total_thickness: thickness,
                permeability_thickness_product: kh,
                porosity,
                initial_pressure: initial,
                ..Default::default()
            })
        };

        // -- the analysis ---------------------------------------------------------------------
        let uuid = writer.uuid(&format!(
            "pta:{}:{period}:{title}",
            test.uuid.to_ascii_lowercase()
        ))?;
        let analysis = prodml::PressureTransientAnalysis {
            citation: writer.citation(
                &title,
                &format!(
                    "an invented pressure transient analysis of `{}`, one homogeneous layer",
                    test.title
                ),
            )?,
            extension_name_value: vec![writer.synthetic("an invented pressure transient analysis")],
            uuid: uuid.clone(),
            schema_version: "2.3".to_string(),
            model_name: params.model_name.trim().to_string(),
            principal_test_period_ref: vec![period.to_string()],
            layer_model: vec![prodml::LayerModel {
                name: format!("{title} — layer"),
                aggregate_layers_model: false,
                reservoir_model: Some(reservoir),
                ..Default::default()
            }],
            fluid_phase_analysis_kind: phase,
            pressure_non_linear_transform_kind:
                prodml::PressureNonLinearTransformKind::PressureUnMinusTransformed,
            time_non_linear_transform_kind: prodml::TimeNonLinearTransformKind::TimeUnMinusTransformed,
            is_numerical_analysis: Some(numerical),
            flow_test_activity: crate::crs::reference_of(&test),
            ..Default::default()
        };
        writer.push(
            &uuid,
            "prodml23.PressureTransientAnalysis",
            "analysis",
            &title,
            &analysis,
        );

        // -- what is said ---------------------------------------------------------------------
        let gridded: Vec<String> = [
            ("permeability", &permeability_array),
            ("porosity", &porosity_array),
        ]
        .into_iter()
        .filter_map(|(what, one)| one.as_ref().map(|one| format!("{what} → `{}`", one.title)))
        .collect();
        let mut says = vec![Note {
            key: "model",
            detail: if numerical {
                format!(
                    "NumericalHomogeneousReservoirModel: {} distributed on the grid (DistributedParametersSubModel), the constants beside them",
                    gridded.join(", ")
                )
            } else {
                "HomogeneousModel: no grid property given, so the constants are the model".to_string()
            },
        }];
        says.push(Note {
            key: "kh",
            detail: format!("k·h = {k} mD × {h} m = {} mD.m", k * h),
        });
        says.push(Note {
            key: "test-period",
            detail: format!(
                "the period analysed is `{period}` of `{}` — told, not read: an object in the bag is a reference, not its content",
                test.title
            ),
        });
        says.push(Note {
            key: "synthetic",
            detail: "the analysis is invented: `synthetic = true` and the source in Citation/Description"
                .to_string(),
        });

        let mut parameters = vec![Parameter::Object {
            title: "input".to_string(),
            reference: test.dor(),
        }];
        for one in permeability_array.iter().chain(porosity_array.iter()) {
            parameters.push(Parameter::Object {
                title: "input".to_string(),
                reference: one.dor(),
            });
        }
        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            activity: ActivityDraft {
                recipe: "pressure-transient-analysis",
                title: format!("pressure-transient-analysis → {title}"),
                seed: writer.activity_seed(),
                parameters,
            },
            objects: writer.objects,
            datasets: Vec::new(),
            says,
            dropped: Vec::new(),
            shapes: Vec::new(),
        })
    }
}
