//! `earth-model` — a `Model` and the `EarthModelInterpretation` that gathers
//! what a package says of it: its stratigraphic column, its structural and
//! rock-fluid organisations, its unit occurrences.
//!
//! The synthetic reference model names its model and its earth model
//! interpretation before anything else, and the rule is **one writer** of
//! an `EarthModelInterpretation`, which the operations that build a model from
//! a unit or an organisation then call. This is that writer, taking the
//! organisations as they already are in the package.
//!
//! # What the schema says, and what is written
//!
//! - `EarthModelInterpretation.InterpretedFeature` is the `Model`: *« An earth
//!   model Interpretation interprets only a model feature »* (RESQML TRG 2.2
//!   §6.19, p. 187). The `Model` is written here, its uuid derived from its
//!   title, so that a second earth model of the same title interprets the
//!   **same** model — a new snapshot of it, not a second model;
//! - `StratigraphicColumn` 0..1, `Structure` 0..∞, `Fluid` 0..∞,
//!   `StratigraphicOccurrences` 0..∞, `WellboreInterpretationSet` 0..∞
//!   (Interpretations.xsd l. 637–641): each reference points at an object the
//!   caller gave, typed by the slot that took it. A wellbore set is a
//!   `DataobjectCollection` that *« must only contain WellboreInterpretation »*
//!   (TRG 2.2 §6.19 p. 187) — its members are not read here, a plan being pure;
//! - **the interpretation's uuid comes from what it references.** The same
//!   column and organisations give the same interpretation; another set gives
//!   another interpretation of the same model — the « snap shot » the usage
//!   guide describes, not a rewrite of the first;
//! - an interpretation that references nothing is valid — every field is
//!   optional — and is written, **said** in a note: an empty earth model reads
//!   exactly like a finished one in a tree.

use energyml_domain::provenance::Parameter;
use energyml_eml_2_3 as eml;
use energyml_resqml_2_2 as resqml;

use crate::declare::{Inputs, ObjectIn, Params, Title};
use crate::operation::{About, ActivityDraft, Category, Context, NewObject, Note, Operation, Plan, Refusal};

/// The operation.
pub struct EarthModel;

/// What `earth-model` is told.
#[derive(Debug, Params)]
pub struct EarthModelParams {
    /// the model's title — the same title is the same model
    pub title: Title,
    /// the earth model interpretation's title; defaults to the model's, followed by « earth model »
    pub interpretation_title: Option<String>,
}

/// What `earth-model` takes: what the package already says of the model.
#[derive(Debug, Inputs)]
pub struct EarthModelInputs {
    /// the stratigraphic column of the model
    #[input(types = "resqml22.StratigraphicColumn")]
    pub column: Option<ObjectIn>,
    /// the structural organisations — one per scenario
    #[input(types = "resqml22.StructuralOrganizationInterpretation")]
    pub structures: Vec<ObjectIn>,
    /// the rock-fluid organisations
    #[input(types = "resqml22.RockFluidOrganizationInterpretation")]
    pub fluids: Vec<ObjectIn>,
    /// the unit occurrences — a column read along a well
    #[input(types = "resqml22.GeologicUnitOccurrenceInterpretation")]
    pub occurrences: Vec<ObjectIn>,
    /// the collections of the model's wellbore interpretations — each holds only WellboreInterpretation (TRG 2.2 §6.19)
    #[input(types = "eml23.DataobjectCollection")]
    pub wellbore_sets: Vec<ObjectIn>,
}

/// What an earth model interpretation references, each slot in its order.
#[derive(Debug, Default, Clone)]
pub(crate) struct Gathered {
    pub(crate) column: Option<eml::DataObjectReference>,
    pub(crate) structures: Vec<eml::DataObjectReference>,
    pub(crate) fluids: Vec<eml::DataObjectReference>,
    pub(crate) occurrences: Vec<eml::DataObjectReference>,
    pub(crate) wellbore_sets: Vec<eml::DataObjectReference>,
}

/// The `Model` an earth model interprets.
pub(crate) enum ModelIs {
    /// Written here, its uuid derived from the title: the same title is the
    /// same model, whichever operation names it.
    Titled(String),
    /// Already in the package: referenced, not written.
    Given(eml::DataObjectReference),
}

/// What [`write`] made.
pub(crate) struct Written {
    /// The `Model` (when written) then the interpretation.
    pub(crate) objects: Vec<NewObject>,
    pub(crate) interpretation: eml::DataObjectReference,
    pub(crate) interpretation_title: String,
    /// The interpretation's seed — the activity's too.
    pub(crate) seed: String,
    /// How many references the interpretation holds.
    pub(crate) content: usize,
}

/// **The one writer of an `EarthModelInterpretation`**
/// (the one-writer rule): `earth-model` and
/// `earth-model-from-stratigraphy` both call it, so an interpretation is
/// written by one piece of code whatever made its column.
///
/// Its uuid comes from what it references, and the model's title — the seed
/// `earth-model` has always used, so its objects keep their uuids. `keep` is
/// the uuid of an interpretation being **enriched**: it keeps it, and the
/// content replaces what it held.
pub(crate) fn write(
    model: ModelIs,
    interpretation_title: Option<String>,
    gathered: &Gathered,
    keep: Option<&str>,
    variant: Option<&str>,
    cx: &Context<'_>,
) -> Result<Written, Refusal> {
    let citation = |title: &str| eml::Citation {
        title: title.to_string(),
        originator: cx.when.originator.clone(),
        creation: cx.when.creation.clone(),
        format: energyml_core::CITATION_FORMAT.to_string(),
        ..Default::default()
    };
    let mut objects = Vec::new();
    let (model_ref, title) = match model {
        ModelIs::Titled(title) => {
            let model_uuid = cx.identity.uuid(&format!("earth-model:model:{title}"), 0)?;
            let model = resqml::Model {
                uuid: model_uuid.clone(),
                schema_version: "2.2".to_string(),
                citation: citation(&title),
                ..Default::default()
            };
            objects.push(NewObject {
                uuid: model_uuid.clone(),
                qualified_type: "resqml22.Model".to_string(),
                role: "model".to_string(),
                title: title.clone(),
                json: energyml_core::json::write_value(&model),
            });
            (
                crate::crs::reference_to(&model_uuid, "resqml22.Model", &title),
                title,
            )
        }
        ModelIs::Given(reference) => {
            let title = reference.title.clone();
            (reference, title)
        }
    };
    // What the interpretation references is what it is: the uuids, in the
    // order the slots name them.
    let content: Vec<String> = gathered
        .column
        .iter()
        .chain(&gathered.structures)
        .chain(&gathered.fluids)
        .chain(&gathered.occurrences)
        .chain(&gathered.wellbore_sets)
        .map(|dor| dor.uuid.clone())
        .collect();
    let mut seed = format!("earth-model:interpretation:{title}:{}", content.join(","));
    // A second snapshot of the same content, on purpose — Paul's and Peter's
    // readings of one column (TUG p. 68). Appended only when given, so every
    // interpretation written without one keeps its uuid.
    if let Some(variant) = variant.map(str::trim).filter(|one| !one.is_empty()) {
        seed.push_str(":variant:");
        seed.push_str(variant);
    }
    let interpretation_uuid = match keep {
        Some(uuid) => uuid.to_string(),
        None => cx.identity.uuid(&seed, 1)?,
    };
    let interpretation_title = interpretation_title
        .map(|given| given.trim().to_string())
        .filter(|given| !given.is_empty())
        .unwrap_or_else(|| format!("{title} — earth model"));
    let interpretation = resqml::EarthModelInterpretation {
        uuid: interpretation_uuid.clone(),
        schema_version: "2.2".to_string(),
        citation: citation(&interpretation_title),
        interpreted_feature: model_ref.clone(),
        stratigraphic_column: gathered.column.clone(),
        structure: gathered.structures.clone(),
        fluid: gathered.fluids.clone(),
        stratigraphic_occurrences: gathered.occurrences.clone(),
        wellbore_interpretation_set: gathered.wellbore_sets.clone(),
        ..Default::default()
    };
    objects.push(NewObject {
        uuid: interpretation_uuid.clone(),
        qualified_type: "resqml22.EarthModelInterpretation".to_string(),
        role: "interpretation".to_string(),
        title: interpretation_title.clone(),
        json: energyml_core::json::write_value(&interpretation),
    });
    Ok(Written {
        objects,
        interpretation: crate::crs::reference_to(
            &interpretation_uuid,
            "resqml22.EarthModelInterpretation",
            &interpretation_title,
        ),
        interpretation_title,
        seed,
        content: content.len(),
    })
}

/// The reference an input stands for, or a refusal naming its slot.
fn reference(slot: &str, input: &ObjectIn) -> Result<eml::DataObjectReference, Refusal> {
    let Some(known) = input.known.reference.as_ref() else {
        return Err(Refusal::on(
            slot,
            format!("`{}` was given without a reference to write", input.name),
        ));
    };
    Ok(crate::crs::reference_of(known))
}

impl Operation for EarthModel {
    type Params = EarthModelParams;
    type Inputs = EarthModelInputs;

    fn about(&self) -> About {
        About {
            name: "earth-model",
            describe: "a model and the earth model interpretation that gathers its stratigraphic column, its structural and rock-fluid organisations and its unit occurrences",
            category: Category::Create,
            makes: &["resqml22.Model", "resqml22.EarthModelInterpretation"],
            recipe: "earth-model",
            represents: None,
        }
    }

    fn plan(
        &self,
        inputs: EarthModelInputs,
        params: EarthModelParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let title = params.title.0.trim().to_string();
        if title.is_empty() {
            return Err(Refusal::on(
                "title",
                "a model needs a title: it is what makes two runs name the same model",
            ));
        }
        let column = inputs
            .column
            .as_ref()
            .map(|one| reference("column", one))
            .transpose()?;
        let structures = inputs
            .structures
            .iter()
            .map(|one| reference("structures", one))
            .collect::<Result<Vec<_>, _>>()?;
        let fluids = inputs
            .fluids
            .iter()
            .map(|one| reference("fluids", one))
            .collect::<Result<Vec<_>, _>>()?;
        let occurrences = inputs
            .occurrences
            .iter()
            .map(|one| reference("occurrences", one))
            .collect::<Result<Vec<_>, _>>()?;
        let wellbore_sets = inputs
            .wellbore_sets
            .iter()
            .map(|one| reference("wellboreSets", one))
            .collect::<Result<Vec<_>, _>>()?;

        let gathered = Gathered {
            column,
            structures,
            fluids,
            occurrences,
            wellbore_sets,
        };
        let made = write(
            ModelIs::Titled(title.clone()),
            params.interpretation_title,
            &gathered,
            None,
            None,
            cx,
        )?;
        let (column, structures, fluids, occurrences, wellbore_sets) = (
            gathered.column,
            gathered.structures,
            gathered.fluids,
            gathered.occurrences,
            gathered.wellbore_sets,
        );
        let content_len = made.content;
        let interpretation_title = made.interpretation_title.clone();
        let seed = made.seed.clone();

        let mut says = vec![Note {
            key: "gathers",
            detail: format!(
                "{} column, {} structural organisation(s), {} rock-fluid organisation(s), {} occurrence(s), {} wellbore set(s)",
                usize::from(column.is_some()),
                structures.len(),
                fluids.len(),
                occurrences.len(),
                wellbore_sets.len()
            ),
        }];
        if content_len == 0 {
            says.push(Note {
                key: "empty",
                detail: "the earth model interpretation references nothing yet: valid — every field is optional — and it reads like a finished one in a tree".to_string(),
            });
        }

        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            objects: made.objects,
            datasets: Vec::new(),
            activity: ActivityDraft {
                recipe: "earth-model",
                title: format!("earth-model → {interpretation_title}"),
                seed,
                parameters: vec![
                    Parameter::Text {
                        title: "title".to_string(),
                        value: title,
                    },
                    Parameter::Double {
                        title: "references".to_string(),
                        value: content_len as f64,
                        uom: None,
                    },
                ],
            },
            says,
            dropped: Vec::new(),
            shapes: Vec::new(),
        })
    }
}
