//! `fluid-sample` — a water sample taken down a wellbore, and what is drawn
//! from it: the acquisition job, the container, the sample, its water analysis
//! and a fluid characterization without equation of state.
//!
//! The reference model's sample chain: the
//! chain of custody of a sample of a well's water: `FluidSampleAcquisitionJob`
//! (`DownholeSampleAcquisition.Wellbore` 1..1) → `FluidSampleContainer` →
//! `FluidSample` → rock-fluid unit; its analysis — `WaterAnalysis` → test
//! → components (Ca, Na, Cl, SO₄, HCO₃, Ba); salinity, TDS, hardness — and
//! a simplified `FluidCharacterization`: groundwater, without an equation of state.
//!
//! | object | what it says |
//! |---|---|
//! | `FluidSampleAcquisitionJob` | one downhole acquisition in the wellbore given, at the MD given (`Wellbore` 1..1, `FluidSystem` 1..1) |
//! | `FluidSampleContainer` | the bottle the sample went into — nothing stated of it but that it exists |
//! | `FluidSample` | kind `water`, its container, its fluid system, its acquisition (its rock-fluid unit when given) |
//! | `WaterAnalysis` | one component per ion stated, a test with the hardness and what else was stated (`FluidSample` 1..1) |
//! | `FluidCharacterization` | a parameter set — density, viscosity, compressibility of the aqueous phase — and no model |
//!
//! # One acquisition per sample, in one job
//!
//! PRODML Technical Usage Guide v2.3, §10.3 p. 77: *« The sample acquisition
//! job represents the operation to collect one or more fluid samples. Fluid
//! sample acquisition elements repeat, one per sample acquired, within one job
//! »*, and a downhole one is where *« measured depth for downhole samples »* is
//! captured (p. 78). The job here holds one: the acquisition names the
//! container and the sample by reference, the sample names the job back by the
//! acquisition's `uid` (`FluidSampleAcquisitionJobSource`).
//!
//! # The analysis must balance before it is written
//!
//! The ions are stated as `formula=mg/L`. Before anything is written, their
//! charges are balanced by `energyml_analysis::ion_balance`: a list of
//! concentrations that is not electrically neutral is no water, and an
//! analysis written from it would read exactly like one. An error beyond
//! `balanceTolerance` is refused, with both sums. The total hardness is
//! **computed** from the calcium and magnesium stated, never stated beside
//! them: two numbers for one fact would be free to disagree.
//!
//! The concentrations go in `VolumeConcentration`, which PRODML types as a
//! `MassPerVolumeMeasure` — the reference model's concentration, of the order of g/L,
//! is a mass per volume, and PRODML's `MassConcentration` is
//! a mass per **mass**. The plan says so.
//!
//! `AnalysisQuality` is written `unknown`: the schema requires it (FluidAnalysis
//! `SampleQuality`, one of `invalid`, `unknown`, `valid`), and nobody analysed
//! this water — `valid` would be a judgement of a laboratory that does not exist.
//!
//! # A characterization without an equation of state
//!
//! Usage Guide v2.3, §11.1.6.3 p. 107: the *« Fluid Characterization Parameter
//! Option »*, *« based on the requirements of a constant compressibility
//! pressure transient analysis »* — which is what a fresh water needs: its
//! density, its viscosity, its compressibility, each on the `aqueous` phase. No
//! `ModelSpecification` is written; the remark says why.
//!
//! # Every object is synthetic, and sourced
//!
//! As `prodml-doublet` does: `synthetic = true` and the source in
//! `Citation/Description` (the reference-model rules on invented objects, ADR-0019), through [`super::prodml_writer`].
//!
//! # Identity
//!
//! The job, the container and the sample come from the wellbore, the instant
//! and the depth — the same sample from one run to the next; the analysis from
//! the sample and the concentrations; the characterization from the sample
//! and its parameters. Another analysis of the same sample is a second object,
//! never a rewrite of the first.
//!
//! # References
//!
//! - The charge balance error `100 · (Σ cations − Σ anions) / (Σ cations + Σ anions)` and the total hardness as CaCO3 (APHA *Standard Methods*, 2340 B), as cited in `energyml-analysis` (`ion_balance`).
//!   TODO(reference): verify — cited from memory in `energyml-analysis`; no copy was available.

use energyml_analysis::ion_balance;
use energyml_core::xml::XmlText;
use energyml_domain::provenance::Parameter;
use energyml_eml_2_3 as eml;
use energyml_prodml_2_3 as prodml;
use energyml_storage::epc::core_props::w3cdtf;

use super::prodml_doublet::{Standard, of_class, titled, wellbore_because};
use super::prodml_writer::{Writer, instant};
use crate::declare::{Inputs, ObjectIn, Params};
use crate::operation::{About, ActivityDraft, Category, Context, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::Ref;

/// The operation.
pub struct FluidSample;

/// Why a sample wants a wellbore, after *« is a `Well`, and »*.
const NOT_A_WELL: &str = "a downhole sample is taken in a wellbore (FluidSampleAcquisitionJob.xsd, `DownholeSampleAcquisition.Wellbore` 1..1): give the wellbore";

/// Why a sample wants a wellbore, after *« is a `…`, and »*.
const NOT_A_HOLE: &str = "a downhole sample is taken in a wellbore: a WITSML `Wellbore`, or the RESQML `WellboreFeature` of the hole";

/// The one acquisition of the job, as the sample names it back.
const ACQUISITION: &str = "acquisition-1";

/// The one test of the analysis.
const TEST: &str = "test-1";

/// Why the characterization has parameters and no model.
const NO_EOS: &str = "Water without equation of state: the density, the viscosity and the compressibility of the aqueous phase as a parameter set — the « Fluid Characterization Parameter Option » of the PRODML Technical Usage Guide v2.3, §11.1.6.3 p. 107 (groundwater, no EOS).";

// ---------------------------------------------------------------- the declaration

/// What `fluid-sample` is told.
#[derive(Debug, Params)]
pub struct FluidSampleParams {
    /// the title of the sample — its job, its container, its analysis and its characterization are named after it
    pub title: String,
    /// where the values come from — a document with its page or paragraph; a value stated as plausible says so (synthetic, and why)
    pub source: String,
    /// when the sample was taken, a UTC date-time as 2025-03-12T09:00:00Z
    pub sampled_at: String,
    /// how long the acquisition lasted, in hours
    #[param(default = 1.0, optional, min = 0.0, uom = "h")]
    pub duration: f64,
    /// the measured depth the sample was taken at, in m along the wellbore from its datum
    #[param(min = 0.0, uom = "m")]
    pub sample_md: f64,
    /// the reference conditions the analysis and the characterization are stated at
    pub standard_conditions: Standard,
    /// the dissolved ions, each as formula=mg/L — Ca+2=110, HCO3-=320
    #[param(least = 2)]
    pub components: Vec<String>,
    /// the largest charge balance error accepted, in percent — beyond it the ions stated are no water
    #[param(default = 5.0, optional, min = 0.0, max = 100.0)]
    pub balance_tolerance: f64,
    /// the total dissolved solids, in mg/L — left out, none is written
    #[param(min = 0.0, uom = "mg/L")]
    pub dissolved_solids: Option<f64>,
    /// the salinity, in mg/L — left out, none is written
    #[param(min = 0.0, uom = "mg/L")]
    pub salinity: Option<f64>,
    /// the density of the water, in kg/m3
    #[param(min = 0.0, uom = "kg/m3")]
    pub water_density: f64,
    /// the viscosity of the water, in mPa.s
    #[param(min = 0.0, uom = "mPa.s")]
    pub water_viscosity: f64,
    /// the compressibility of the water, in 1/bar
    #[param(min = 0.0, uom = "1/bar")]
    pub water_compressibility: f64,
    /// the temperature the characterization is stated at, in degC
    #[param(uom = "degC")]
    pub reference_temperature: f64,
}

/// What `fluid-sample` takes.
#[derive(Debug, Inputs)]
pub struct FluidSampleInputs {
    /// the wellbore the sample was taken in — a WITSML `Wellbore`, or the RESQML `WellboreFeature` of the hole
    #[input(types = "witsml21.Wellbore, resqml22.WellboreFeature")]
    pub wellbore: ObjectIn,
    /// the fluid system the sample belongs to
    #[input(types = "prodml23.FluidSystem")]
    pub fluid_system: ObjectIn,
    /// the rock-fluid unit the sample was taken from
    #[input(types = "resqml22.RockFluidUnitInterpretation")]
    pub rock_fluid_unit: Option<ObjectIn>,
}

// ---------------------------------------------------------------- the ions

/// One ion stated: its formula, its concentration in mg/L.
#[derive(Debug, Clone)]
struct Stated {
    formula: String,
    mg_per_l: f64,
}

/// `Ca+2=110` — the formula and its mg/L, or why the text is not that.
fn stated(rank: usize, text: &str) -> Result<Stated, Refusal> {
    let text = text.trim();
    let Some((formula, value)) = text.split_once('=') else {
        return Err(Refusal::on(
            "components",
            format!("component {rank} `{text}` is not formula=mg/L, as `Ca+2=110`"),
        ));
    };
    let formula = formula.trim();
    let value = value.trim().trim_end_matches("mg/L").trim();
    let Ok(mg_per_l) = value.parse::<f64>() else {
        return Err(Refusal::on(
            "components",
            format!("component {rank} `{text}`: `{value}` is not a number of mg/L"),
        ));
    };
    if formula.starts_with("NO3") {
        return Err(Refusal::on(
            "components",
            format!(
                "component {rank} `{text}`: nitrate is `NO3-`, one negative charge, and PRODML 2.3 spells it `NO3-2` (AnionKind, FluidAnalysis.xsd) — which of the two is meant decides the balance, so it is not written"
            ),
        ));
    }
    Ok(Stated { formula: formula.to_string(), mg_per_l })
}

/// The component PRODML writes for an ion, its formula checked against the
/// closed list of the schema — `AnionKind` or `CationKind` — before its
/// extensible twin is written.
fn component(rank: usize, ion: &Stated) -> Result<prodml::WaterSampleComponent, Refusal> {
    let (anion, cation) = if prodml::AnionKind::from_xml_text(&ion.formula).is_ok() {
        (Some(prodml::AnionKindExt::from_xml_text(&ion.formula).map_err(|why| Refusal::on("components", why.to_string()))?), None)
    } else if prodml::CationKind::from_xml_text(&ion.formula).is_ok() {
        (None, Some(prodml::CationKindExt::from_xml_text(&ion.formula).map_err(|why| Refusal::on("components", why.to_string()))?))
    } else {
        return Err(Refusal::on(
            "components",
            format!("`{}` is neither an anion nor a cation PRODML 2.3 names (AnionKind, CationKind — FluidAnalysis.xsd)", ion.formula),
        ));
    };
    Ok(prodml::WaterSampleComponent {
        anion,
        cation,
        volume_concentration: Some(eml::MassPerVolumeMeasureExt { value: ion.mg_per_l, uom: eml::MassPerVolumeUomExt::MgPerL }),
        uid: format!("component-{rank}"),
        ..Default::default()
    })
}

// ---------------------------------------------------------------- the operation

impl Operation for FluidSample {
    type Params = FluidSampleParams;
    type Inputs = FluidSampleInputs;

    fn about(&self) -> About {
        About {
            name: "fluid-sample",
            describe: "a water sample taken down a wellbore and its chain — acquisition job, container, sample, water analysis balanced before it is written, and a characterization without equation of state — every value stated and sourced",
            category: Category::Synthetic,
            makes: &[
                "prodml23.FluidSampleAcquisitionJob",
                "prodml23.FluidSampleContainer",
                "prodml23.FluidSample",
                "prodml23.WaterAnalysis",
                "prodml23.FluidCharacterization",
            ],
            recipe: "fluid-sample",
            represents: None,
        }
    }

    fn check(&self, inputs: &FluidSampleInputs) -> Result<(), Refusal> {
        wellbore_because("wellbore", &inputs.wellbore, NOT_A_WELL, NOT_A_HOLE)?;
        of_class("fluidSystem", &inputs.fluid_system, &["FluidSystem"], "a sample is drawn from a fluid system (`FluidSystem` 1..1)")?;
        if let Some(unit) = &inputs.rock_fluid_unit {
            of_class(
                "rockFluidUnit",
                unit,
                &["RockFluidUnitInterpretation"],
                "a sample comes from a rock-fluid unit",
            )?;
        }
        Ok(())
    }

    fn plan(&self, inputs: FluidSampleInputs, params: FluidSampleParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        // -- what the run is told ---------------------------------------------------
        let title = titled("title", &params.title)?;
        let mut writer = Writer::new(cx, "fluid-sample", &params.source)?;
        let (sampled_text, sampled) = instant("sampledAt", &params.sampled_at)?;
        if !(params.duration.is_finite() && params.duration > 0.0) {
            return Err(Refusal::on("duration", format!("{} h: an acquisition lasts a while, so its end comes after its start", params.duration)));
        }
        let ended = w3cdtf(sampled + (params.duration * 3600.0).round() as i64);
        if !(params.sample_md.is_finite() && params.sample_md >= 0.0) {
            return Err(Refusal::on("sampleMd", format!("{} m is not a measured depth", params.sample_md)));
        }
        for (slot, value) in [
            ("waterDensity", params.water_density),
            ("waterViscosity", params.water_viscosity),
            ("waterCompressibility", params.water_compressibility),
        ] {
            if !(value.is_finite() && value > 0.0) {
                return Err(Refusal::on(slot, format!("{value} is not a property of a water: a finite number > 0")));
            }
        }

        let hole = wellbore_because("wellbore", &inputs.wellbore, NOT_A_WELL, NOT_A_HOLE)?.clone();
        let system = of_class("fluidSystem", &inputs.fluid_system, &["FluidSystem"], "a sample is drawn from a fluid system (`FluidSystem` 1..1)")?.clone();
        let unit: Option<Ref> = match &inputs.rock_fluid_unit {
            Some(one) => Some(
                of_class("rockFluidUnit", one, &["RockFluidUnitInterpretation"], "a sample comes from a rock-fluid unit")?
                    .clone(),
            ),
            None => None,
        };

        // -- the ions, balanced before anything is written ---------------------------
        let ions: Vec<Stated> = params.components.iter().enumerate().map(|(at, text)| stated(at + 1, text)).collect::<Result<_, _>>()?;
        let pairs: Vec<(&str, f64)> = ions.iter().map(|one| (one.formula.as_str(), one.mg_per_l)).collect();
        let balance = ion_balance::balance(&pairs).map_err(|why| Refusal::on("components", why))?;
        let tolerance = params.balance_tolerance;
        if !(tolerance.is_finite() && (0.0..=100.0).contains(&tolerance)) {
            return Err(Refusal::on("balanceTolerance", format!("{tolerance} is not a percentage")));
        }
        if balance.error.abs() > tolerance {
            return Err(Refusal::on(
                "components",
                format!(
                    "the ions do not balance: {:.3} meq/L of cations against {:.3} meq/L of anions, an error of {:.2} % beyond the {tolerance} % accepted — a water is electrically neutral, so an ion is missing or a concentration is wrong",
                    balance.cations, balance.anions, balance.error
                ),
            ));
        }
        let hardness = ion_balance::total_hardness(&pairs);
        let sum = ion_balance::sum_of_ions(&pairs);
        let components: Vec<prodml::WaterSampleComponent> =
            ions.iter().enumerate().map(|(at, ion)| component(at + 1, ion)).collect::<Result<_, _>>()?;

        let conditions = || {
            eml::AbstractTemperaturePressure::ReferenceTemperaturePressure(eml::ReferenceTemperaturePressure {
                reference_temp_pres: params.standard_conditions.written(),
            })
        };
        let dor = |known: &Ref| crate::crs::reference_of(known);

        // -- the uuids, before the objects: they name one another ----------------------
        let base = format!("{}:{sampled_text}:{}", hole.uuid.to_ascii_lowercase(), params.sample_md);
        let job_uuid = writer.uuid(&format!("job:{base}"))?;
        let container_uuid = writer.uuid(&format!("container:{base}"))?;
        let sample_uuid = writer.uuid(&format!("sample:{base}"))?;
        let concentrations = ions.iter().map(|one| format!("{}={}", one.formula, one.mg_per_l)).collect::<Vec<_>>().join(",");
        let analysis_uuid = writer.uuid(&format!(
            "analysis:{sample_uuid}:{concentrations}:{:?}:{:?}:{}",
            params.dissolved_solids,
            params.salinity,
            params.standard_conditions.word()
        ))?;
        let characterization_uuid = writer.uuid(&format!(
            "characterization:{sample_uuid}:{analysis_uuid}:{}:{}:{}:{}:{}",
            params.water_density,
            params.water_viscosity,
            params.water_compressibility,
            params.reference_temperature,
            params.standard_conditions.word()
        ))?;
        let job_title = format!("{title} — acquisition job");
        let container_title = format!("{title} — container");
        let analysis_title = format!("{title} — water analysis");
        let characterization_title = format!("{title} — characterization");
        let job_ref = Ref::new(job_uuid.clone(), "prodml23.FluidSampleAcquisitionJob", job_title.clone());
        let container_ref = Ref::new(container_uuid.clone(), "prodml23.FluidSampleContainer", container_title.clone());
        let sample_ref = Ref::new(sample_uuid.clone(), "prodml23.FluidSample", title.clone());
        let analysis_ref = Ref::new(analysis_uuid.clone(), "prodml23.WaterAnalysis", analysis_title.clone());

        // The acquisition job: one downhole acquisition.
        let job = prodml::FluidSampleAcquisitionJob {
            uuid: job_uuid.clone(),
            schema_version: "2.3".to_string(),
            citation: writer.citation(
                &job_title,
                &format!("the job that took the invented sample `{title}` down `{}` at MD {} m on {sampled_text}", hole.title, params.sample_md),
            )?,
            extension_name_value: vec![writer.synthetic("an acquisition job of an invented sample")],
            start_time: Some(sampled_text.clone()),
            end_time: Some(ended.clone()),
            fluid_system: dor(&system),
            fluid_sample_acquisition: vec![prodml::AnyFluidSampleAcquisition::DownholeSampleAcquisition(prodml::DownholeSampleAcquisition {
                start_time: sampled_text.clone(),
                end_time: ended.clone(),
                remark: format!("synthetic — one downhole acquisition at MD {} m; no pressure, temperature or volume of acquisition is stated", params.sample_md),
                fluid_sample_container: dor(&container_ref),
                fluid_sample: dor(&sample_ref),
                uid: ACQUISITION.to_string(),
                wellbore: dor(&hole),
                sampling_run: 1,
                top_md: eml::LengthMeasure { value: params.sample_md, uom: eml::LengthUom::M },
                ..Default::default()
            })],
            ..Default::default()
        };
        writer.push(&job_uuid, "prodml23.FluidSampleAcquisitionJob", "acquisition-job", &job_title, &job);

        // The container: that it exists, nothing more.
        let container = prodml::FluidSampleContainer {
            uuid: container_uuid.clone(),
            schema_version: "2.3".to_string(),
            citation: writer.citation(&container_title, &format!("the container the invented sample `{title}` went into"))?,
            extension_name_value: vec![writer.synthetic("the container of an invented sample")],
            remark: Some("synthetic — no make, model, capacity or rating is stated: none is written".to_string()),
            ..Default::default()
        };
        writer.push(&container_uuid, "prodml23.FluidSampleContainer", "container", &container_title, &container);

        // The sample.
        let sample = prodml::FluidSample {
            uuid: sample_uuid.clone(),
            schema_version: "2.3".to_string(),
            citation: writer.citation(&title, &format!("a water sample of `{}` taken at MD {} m on {sampled_text}", hole.title, params.sample_md))?,
            extension_name_value: vec![writer.synthetic("an invented water sample")],
            sample_kind: Some(prodml::FluidSampleKindExt::Water),
            rock_fluid_unit_interpretation: unit.as_ref().map(dor),
            original_sample_container: Some(dor(&container_ref)),
            fluid_system: Some(dor(&system)),
            fluid_sample_acquisition_job_source: Some(prodml::FluidSampleAcquisitionJobSource {
                fluid_sample_acquisition_reference: ACQUISITION.to_string(),
                fluid_sample_acquisition_job_reference: dor(&job_ref),
            }),
            ..Default::default()
        };
        writer.push(&sample_uuid, "prodml23.FluidSample", "sample", &title, &sample);

        // The water analysis.
        let test = prodml::WaterAnalysisTest {
            test_number: 1,
            uid: TEST.to_string(),
            total_dissolved_solids_per_volume: params
                .dissolved_solids
                .map(|value| eml::MassPerVolumeMeasureExt { value, uom: eml::MassPerVolumeUomExt::MgPerL }),
            salinity_per_volume: params.salinity.map(|value| eml::MassPerVolumeMeasureExt { value, uom: eml::MassPerVolumeUomExt::MgPerL }),
            total_hardness_per_volume: hardness.map(|value| eml::MassPerVolumeMeasureExt { value, uom: eml::MassPerVolumeUomExt::MgPerL }),
            remark: Some(match hardness {
                Some(_) => "synthetic — the total hardness is computed from the calcium and magnesium stated, in mg/L as CaCO3".to_string(),
                None => "synthetic — no calcium nor magnesium is stated, so no hardness is written".to_string(),
            }),
            ..Default::default()
        };
        let analysis = prodml::WaterAnalysis {
            uuid: analysis_uuid.clone(),
            schema_version: "2.3".to_string(),
            citation: writer.citation(
                &analysis_title,
                &format!(
                    "the analysis of the invented sample `{title}`: {} ion(s), balanced to {:.2} %",
                    ions.len(),
                    balance.error
                ),
            )?,
            extension_name_value: vec![writer.synthetic("an invented water analysis")],
            standard_conditions: Some(conditions()),
            analysis_quality: prodml::SampleQuality::Unknown,
            remark: Some(format!(
                "synthetic — nobody analysed this water, so its quality is `unknown`; the concentrations are mass per volume (VolumeConcentration, mg/L); cations {:.3} meq/L, anions {:.3} meq/L",
                balance.cations, balance.anions
            )),
            water_analysis_test: vec![test],
            water_sample_component: components,
            fluid_sample: dor(&sample_ref),
            ..Default::default()
        };
        writer.push(&analysis_uuid, "prodml23.WaterAnalysis", "water-analysis", &analysis_title, &analysis);

        // The characterization: parameters, no model.
        let parameter = |property: prodml::OutputFluidPropertyExt, value: f64, uom: &str| prodml::FluidCharacterizationParameter {
            property,
            phase: Some(prodml::ThermodynamicPhase::Aqueous),
            value,
            uom: uom.to_string(),
            ..Default::default()
        };
        let characterization = prodml::FluidCharacterization {
            uuid: characterization_uuid.clone(),
            schema_version: "2.3".to_string(),
            citation: writer.citation(
                &characterization_title,
                &format!("the water of the invented sample `{title}` as a parameter set, without equation of state"),
            )?,
            extension_name_value: vec![writer.synthetic("an invented fluid characterization")],
            rock_fluid_unit_interpretation: unit.as_ref().map(dor),
            standard_conditions: Some(conditions()),
            source: vec![prodml::FluidCharacterizationSource {
                fluid_analysis_test_reference: vec![TEST.to_string()],
                fluid_analysis: Some(dor(&analysis_ref)),
            }],
            model: vec![prodml::FluidCharacterizationModel {
                uid: "model-1".to_string(),
                name: Some("water, no EOS".to_string()),
                reference_temperature: Some(eml::ThermodynamicTemperatureMeasure {
                    value: params.reference_temperature,
                    uom: eml::ThermodynamicTemperatureUom::DegC,
                }),
                remark: Some(NO_EOS.to_string()),
                fluid_characterization_parameter_set: Some(prodml::FluidCharacterizationParameterSet {
                    fluid_characterization_parameter: vec![
                        parameter(prodml::OutputFluidPropertyExt::Density, params.water_density, "kg/m3"),
                        parameter(prodml::OutputFluidPropertyExt::Viscosity, params.water_viscosity, "mPa.s"),
                        parameter(prodml::OutputFluidPropertyExt::Compressibility, params.water_compressibility, "1/bar"),
                    ],
                }),
                ..Default::default()
            }],
            remark: Some(NO_EOS.to_string()),
            fluid_system: Some(dor(&system)),
            ..Default::default()
        };
        writer.push(&characterization_uuid, "prodml23.FluidCharacterization", "characterization", &characterization_title, &characterization);

        // -- what the plan says -------------------------------------------------------
        let mut says = vec![
            Note {
                key: "balance",
                detail: format!(
                    "{:.3} meq/L of cations against {:.3} meq/L of anions: a balance error of {:.2} %, within the {tolerance} % accepted (energyml_analysis::ion_balance)",
                    balance.cations, balance.anions, balance.error
                ),
            },
            Note {
                key: "concentration",
                detail: "the ions are written as VolumeConcentration, a mass per volume in mg/L: the reference model's g/L concentration is a mass per volume, and PRODML's MassConcentration is a mass per mass".to_string(),
            },
            Note {
                key: "analysis-quality",
                detail: "the analysis is written `unknown` in AnalysisQuality: the schema requires it, and no laboratory judged this water".to_string(),
            },
            Note { key: "no-eos", detail: NO_EOS.to_string() },
            Note {
                key: "synthetic",
                detail: "every object written is invented: `synthetic = true` and the source in Citation/Description".to_string(),
            },
        ];
        says.push(Note {
            key: "hardness",
            detail: match hardness {
                Some(value) => format!("the total hardness, {value:.1} mg/L as CaCO3, is computed from the calcium and magnesium stated — never stated beside them"),
                None => "no calcium nor magnesium is stated: no hardness is written".to_string(),
            },
        });
        if unit.is_none() {
            says.push(Note {
                key: "rock-fluid-unit",
                detail: "the sample and the characterization name no rock-fluid unit: the reference model links them to a rock-fluid unit not written by this operation".to_string(),
            });
        }
        if params.dissolved_solids.is_none() {
            says.push(Note { key: "dissolved-solids", detail: "no total dissolved solids is written: none was given".to_string() });
        }
        if params.salinity.is_none() {
            says.push(Note { key: "salinity", detail: "no salinity is written: none was given".to_string() });
        }

        // -- the activity -------------------------------------------------------------
        let mut parameters = vec![
            // In meq/L, which the Energistics unit dictionary does not name: an
            // activity parameter's `Uom` outside it must be `authority:unit`,
            // so the unit is said by the template's words, not by a made-up code.
            Parameter::Double { title: "cationCharge".to_string(), value: balance.cations, uom: None },
            Parameter::Double { title: "anionCharge".to_string(), value: balance.anions, uom: None },
            Parameter::Double { title: "balanceError".to_string(), value: balance.error, uom: Some("%".to_string()) },
            Parameter::Double { title: "sumOfIons".to_string(), value: sum, uom: Some("mg/L".to_string()) },
        ];
        if let Some(value) = hardness {
            parameters.push(Parameter::Double { title: "totalHardness".to_string(), value, uom: Some("mg/L".to_string()) });
        }
        for input in std::iter::once(&hole).chain(std::iter::once(&system)).chain(unit.iter()) {
            parameters.push(Parameter::Object { title: "input".to_string(), reference: input.dor() });
        }
        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            activity: ActivityDraft { recipe: "fluid-sample", title: format!("fluid-sample → {title}"), seed: writer.activity_seed(), parameters },
            objects: writer.objects,
            datasets: Vec::new(),
            says,
            dropped: Vec::new(),
            // No geometry: a sample and its analysis have no shape.
            shapes: Vec::new(),
        })
    }

    /// What the run computed from the ions.
    fn reports(&self) -> Vec<Slot> {
        let number = |uom: Option<&'static str>| Need::Number { uom, least: None, most: None };
        vec![
            Slot::required("cationCharge", number(None), "the charges the cations stated carry, in milliequivalents per litre"),
            Slot::required("anionCharge", number(None), "the charges the anions stated carry, in milliequivalents per litre"),
            Slot::required("balanceError", number(Some("%")), "how far the two are apart, relative to their sum"),
            Slot::required("sumOfIons", number(Some("mg/L")), "what the concentrations stated add up to"),
            Slot::optional("totalHardness", number(Some("mg/L")), "the calcium and magnesium stated, as CaCO3"),
        ]
    }
}
