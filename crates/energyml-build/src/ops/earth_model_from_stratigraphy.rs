//! `earth-model-from-stratigraphy` — an earth model made from a stratigraphic
//! unit, or from a whole column: the `Model`, its column restricted to what the
//! unit contains, a top and a bottom for every unit, and the
//! `EarthModelInterpretation` that gathers them.
//!
//! The need: to be able to make EarthModelInterpretations from a
//! stratigraphicUnitInterpretation (the Eocene's, for instance) present in a
//! column. The answers to the follow-up questions decide
//! the rest, and each is noted where it acts.
//!
//! # What is written
//!
//! - **the `Model`**, named after the unit when none is given — a model is
//!   created, for example, for "Cretaceous" if that is what was requested.
//!   Its uuid comes from its title, through the writer
//!   `earth-model` uses, so the same title is the same model whichever
//!   operation writes it;
//! - **the column**: the unit's rank and the finer ones, **restricted** to the
//!   units the unit contains (`energyml_domain::stratigraphy::subtree`, which
//!   decides the nesting — never here). A rank whose units are exactly the
//!   subtree's is **taken as it is**; a restricted one interprets the model
//!   and keeps its source's number. A column whose ranks are all taken
//!   as they are is itself taken;
//! - **a top and a bottom for every unit** (one top and one bottom per unit,
//!   with Feature and interpretation only, e.g. "Bartonian
//!   TOP" and "Bartonian BOTTOM"): a `BoundaryFeature` and a
//!   `HorizonInterpretation` each, unless the package already holds one of that
//!   title — compared **without case**, in case the user already
//!   wrote these objects by hand — which is then taken. Two of one title
//!   are refused by name: choosing would be guessing. The switch `boundaries`
//!   turns it off, for when the user already has these objects;
//! - **a structural organisation** of those horizons, ordered by age — only
//!   when every unit states its age: an ordered list whose order nobody
//!   stated would be the one lie this crate refuses to draw. Without ages the
//!   horizons are written and the organisation is not, and the plan says so;
//! - **an occurrence per rank of the column** — a
//!   `GeologicUnitOccurrenceInterpretation` that says which units exist in
//!   *this* model. The occurrence is part of the earth model and describes
//!   what exists in the real world of the model; a given unit may not appear
//!   in a particular model (because of erosion, for example). For now, it
//!   should express the units coming from the stratigraphicColumn. One per rank because
//!   `IsOccurrenceOf` is *« 0..1 To: StratigraphicColumnRankInterpretation »*
//!   (RESQML TRG 2.2 §6.31 p. 199), and because a succession is read at one
//!   rank — the Eocene and the Lutetian are not one after the other. Each lists
//!   its rank's units in the rank's order, under the rank's criterion: the
//!   column as it is, until a person removes an eroded unit or turns an
//!   overturned succession over — the occurrence is the object that will say
//!   it, the column stays the chart;
//! - **the `EarthModelInterpretation`**, by `earth_model::write`, the one writer
//!   of it. `mode = snapshot` makes a new one when the content
//!   is new — the earlier stays; `mode = enrich` rewrites the one given in
//!   `enrich` under its own uuid (the user is asked whether to make a
//!   new one or enrich).
//!
//! # What is not written yet
//!
//! The contacts of a restricted rank are **not copied** — counted in a note;
//! the wells of the model and its sides neither.

use std::collections::BTreeMap;

use energyml_domain::provenance::Parameter;
use energyml_domain::stratigraphy::{self, Columns, Named, SubRank, Subtree};
use energyml_eml_2_3 as eml;
use energyml_resqml_2_2 as resqml;

use super::earth_model::{self, Gathered, ModelIs};
use crate::crs::{reference_of, reference_to};
use crate::declare::{Choice, Inputs, ObjectIn, Params, Suggestions};
use crate::operation::{About, ActivityDraft, Category, Context, Dropped, Identity, NewObject, Note, Operation, Plan, Refusal};
use crate::pieces::{Pieces, Source};

/// The operation.
pub struct EarthModelFromStratigraphy;

/// A new snapshot, or the interpretation given rewritten.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum Mode {
    /// A new earth model interpretation when the content is new; the earlier ones stay.
    Snapshot,
    /// The interpretation given in `enrich` keeps its uuid and holds the new content.
    Enrich,
}

/// What `earth-model-from-stratigraphy` is told.
#[derive(Debug, Params)]
pub struct FromStratigraphyParams {
    /// the earth model interpretation's title; left out, the unit's followed by « earth model »
    pub title: Option<String>,
    /// a new snapshot, or the interpretation given in `enrich` rewritten under its uuid
    #[param(default = Mode::Snapshot, optional)]
    pub mode: Mode,
    /// write a top and a bottom — a boundary feature and a horizon interpretation — for every unit, taking those the package already holds under the same title
    #[param(default = true, optional)]
    pub boundaries: bool,
    /// a second snapshot of the same content, on purpose — left out for the ordinary case
    pub variant: Option<String>,
}

/// What `earth-model-from-stratigraphy` takes.
#[derive(Debug, Inputs)]
pub struct FromStratigraphyInputs {
    /// the stratigraphic unit the model is made for — or a whole stratigraphic column
    #[input(types = "resqml22.StratigraphicUnitInterpretation, resqml22.StratigraphicColumn")]
    pub from: ObjectIn,
    /// the model the earth model interprets; left empty, a model named after the unit is written
    #[input(types = "resqml22.Model")]
    pub model: Option<ObjectIn>,
    /// the column to read the unit in, when several columns list it and say different things
    #[input(types = "resqml22.StratigraphicColumn")]
    pub column: Option<ObjectIn>,
    /// the earth model interpretation to rewrite — `mode = enrich` only
    #[input(types = "resqml22.EarthModelInterpretation")]
    pub enrich: Option<ObjectIn>,
    /// the boundary features and horizon interpretations of the package: a « <unit> TOP » or « <unit> BOTTOM » found here, whatever its case, is taken
    #[input(types = "resqml22.BoundaryFeature, resqml22.HorizonInterpretation", every)]
    pub existing: Vec<ObjectIn>,
}

/// The reference an input stands for, or a refusal naming its slot.
fn dor(slot: &str, input: &ObjectIn) -> Result<eml::DataObjectReference, Refusal> {
    let Some(known) = input.known.reference.as_ref() else {
        return Err(Refusal::on(slot, format!("`{}` was given without a reference to write", input.name)));
    };
    Ok(reference_of(known))
}

/// `A`'s title, or its uuid.
fn label(named: &Named) -> &str {
    if named.title.is_empty() { &named.uuid } else { &named.title }
}

/// The criterion a rank states, as the schema spells it.
fn criterion(text: &str) -> Option<resqml::OrderingCriteria> {
    match text.trim() {
        "age" => Some(resqml::OrderingCriteria::Age),
        "apparent depth" => Some(resqml::OrderingCriteria::ApparentDepth),
        _ => None,
    }
}

/// Which side of a unit a boundary is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Top,
    Bottom,
}

impl Side {
    fn word(self) -> &'static str {
        match self {
            Side::Top => "TOP",
            Side::Bottom => "BOTTOM",
        }
    }
}

/// A boundary of the model: whose, which side, and the interpretation that
/// stands for it — taken from the package or written here.
struct Limit {
    unit: Named,
    side: Side,
    rank: Option<i64>,
    interpretation: eml::DataObjectReference,
}

/// What the package holds under one title, per type — lowercased keys, so « bartonien top » finds « Bartonien TOP ».
#[derive(Default)]
struct Existing {
    features: BTreeMap<String, Vec<eml::DataObjectReference>>,
    interpretations: BTreeMap<String, Vec<eml::DataObjectReference>>,
}

impl Existing {
    fn of(inputs: &[ObjectIn]) -> Existing {
        let mut out = Existing::default();
        for input in inputs {
            let Some(known) = input.known.reference.as_ref() else { continue };
            let key = known.title.trim().to_lowercase();
            let dor = reference_of(known);
            if known.qualified_type.ends_with("HorizonInterpretation") {
                out.interpretations.entry(key).or_default().push(dor);
            } else if known.qualified_type.ends_with("BoundaryFeature") {
                out.features.entry(key).or_default().push(dor);
            }
        }
        out
    }

    /// The one object of that title, if there is one; two are refused by name.
    fn one(map: &BTreeMap<String, Vec<eml::DataObjectReference>>, title: &str, what: &str) -> Result<Option<eml::DataObjectReference>, Refusal> {
        match map.get(&title.to_lowercase()).map(Vec::as_slice) {
            None | Some([]) => Ok(None),
            Some([one]) => Ok(Some(one.clone())),
            Some(many) => Err(Refusal::on(
                "existing",
                format!(
                    "{} {what}s of the package are titled « {title} » (compared without case): {} — which one is this model's is not said; retitle one, or turn `boundaries` off and link them by hand",
                    many.len(),
                    many.iter().map(|one| format!("{} ({})", one.title, one.uuid)).collect::<Vec<_>>().join(", ")
                ),
            )),
        }
    }
}

/// Pick the subtree to model when several columns list the unit.
fn chosen(trees: Vec<Subtree>, column: Option<&ObjectIn>, unit: &str) -> Result<Subtree, Refusal> {
    if let Some(column) = column {
        let wanted = column.known.reference.as_ref().map(|one| one.uuid.to_ascii_lowercase()).unwrap_or_default();
        return trees
            .into_iter()
            .find(|tree| tree.column.as_ref().is_some_and(|one| one.uuid == wanted))
            .ok_or_else(|| Refusal::on("column", format!("`{}` does not list `{unit}`", column.name)));
    }
    let content = |tree: &Subtree| -> Vec<Vec<String>> {
        tree.ranks.iter().map(|rank| rank.units.iter().map(|unit| unit.uuid.clone()).collect()).collect()
    };
    let first = content(&trees[0]);
    if trees.iter().all(|tree| content(tree) == first) {
        return Ok(trees.into_iter().next().expect("subtree never answers an empty list"));
    }
    Err(Refusal::on(
        "column",
        format!(
            "`{unit}` is listed by {} columns that do not contain the same units under it: {} — name the one to read in `column`",
            trees.len(),
            trees
                .iter()
                .map(|tree| tree.column.as_ref().map_or_else(|| "a rank no column lists".to_string(), |one| format!("`{}`", label(one))))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    ))
}

impl Operation for EarthModelFromStratigraphy {
    type Params = FromStratigraphyParams;
    type Inputs = FromStratigraphyInputs;

    fn about(&self) -> About {
        About {
            name: "earth-model-from-stratigraphy",
            describe: "an earth model made from a stratigraphic unit or a whole column: the model, the column restricted to what the unit contains, an occurrence of each rank, a top and a bottom per unit, and the earth model interpretation",
            category: Category::Create,
            makes: &[
                "resqml22.Model",
                "resqml22.StratigraphicColumnRankInterpretation",
                "resqml22.StratigraphicColumn",
                "resqml22.GeologicUnitOccurrenceInterpretation",
                "resqml22.BoundaryFeature",
                "resqml22.HorizonInterpretation",
                "resqml22.StructuralOrganizationInterpretation",
                "resqml22.EarthModelInterpretation",
            ],
            recipe: "earth-model-from-stratigraphy",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        // The unit's title, read from the bag — a suggestion reads nothing else.
        let unit = bag.iter().find_map(|(_, source)| match source {
            Source::Object(known) if known.stratigraphy.is_some() => known.reference.as_ref().map(|one| one.title.clone()),
            _ => None,
        });
        if let Some(unit) = unit {
            slots.set("title", serde_json::Value::String(format!("{unit} — earth model")));
        }
    }

    fn plan(&self, inputs: FromStratigraphyInputs, params: FromStratigraphyParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        if let Identity::Given(_) = cx.identity {
            return Err(Refusal::new(
                "refused",
                "an earth model is named from its content — the same unit and the same column give the same objects; it does not take given uuids",
            ));
        }
        let Some(from) = inputs.from.known.reference.clone() else {
            return Err(Refusal::on("from", "the unit was given without a reference"));
        };
        if !from.qualified_type.starts_with("resqml22.") {
            return Err(Refusal::on(
                "from",
                format!("this operation writes RESQML 2.2; `{}` is a {} — translate the package first", from.title, from.qualified_type),
            ));
        }
        let Some(columns) = inputs.from.known.stratigraphy.as_ref() else {
            return Err(Refusal::on(
                "from",
                format!("nothing is known of the stratigraphy around `{}` — it was not read from a package", from.title),
            ));
        };
        let whole = from.qualified_type.ends_with("StratigraphicColumn");
        let tree = if whole {
            let column = columns
                .columns
                .iter()
                .find(|one| one.column.uuid.eq_ignore_ascii_case(&from.uuid))
                .ok_or_else(|| Refusal::on("from", format!("column `{}` is not in the package's stratigraphy", from.title)))?;
            stratigraphy::whole_column(column)
        } else {
            let trees = stratigraphy::subtree(columns, &from.uuid).map_err(|why| Refusal::on("from", why))?;
            chosen(trees, inputs.column.as_ref(), &from.title)?
        };
        let root_title = from.title.trim().to_string();

        // -- the model -------------------------------------------------------------------
        let (model_is, model) = match inputs.model.as_ref() {
            Some(given) => {
                let reference = dor("model", given)?;
                (ModelIs::Given(reference.clone()), reference)
            }
            None => {
                let uuid = cx.identity.uuid(&format!("earth-model:model:{root_title}"), 0)?;
                (ModelIs::Titled(root_title.clone()), reference_to(&uuid, "resqml22.Model", &root_title))
            }
        };
        let citation = |title: &str, description: Option<String>| eml::Citation {
            title: title.to_string(),
            originator: cx.when.originator.clone(),
            creation: cx.when.creation.clone(),
            format: energyml_core::CITATION_FORMAT.to_string(),
            description,
            ..Default::default()
        };
        let mut objects: Vec<NewObject> = Vec::new();
        let mut says: Vec<Note> = Vec::new();
        let mut dropped: Vec<Dropped> = tree.unplaced.iter().map(|(_, why)| Dropped { count: 1, why: why.clone() }).collect();

        // -- the ranks -----------------------------------------------------------------------
        let mut ranks: Vec<eml::DataObjectReference> = Vec::new();
        let mut criteria: Vec<resqml::OrderingCriteria> = Vec::new();
        let mut restricted = 0usize;
        let mut contacts_left = 0usize;
        for (at, sub) in tree.ranks.iter().enumerate() {
            let read = criteria_of(sub, &mut says);
            criteria.push(read.clone());
            if sub.whole {
                ranks.push(reference_to(&sub.source.uuid, "resqml22.StratigraphicColumnRankInterpretation", label(&sub.source)));
                continue;
            }
            ranks.push(write_rank(sub, read, &model, &root_title, at, cx, &citation, &mut objects)?);
            restricted += 1;
            contacts_left += sub.contacts.len();
        }
        if contacts_left > 0 {
            says.push(Note {
                key: "contacts-not-copied",
                detail: format!("{contacts_left} contact(s) between kept units of the restricted ranks are not copied yet — they stay on the source ranks"),
            });
        }

        // -- the column ----------------------------------------------------------------------
        let taken = tree.ranks.iter().all(|sub| sub.whole)
            && tree.column.as_ref().is_some_and(|column| {
                columns.columns.iter().find(|one| one.column.uuid == column.uuid).is_some_and(|one| one.ranks.len() == tree.ranks.len())
            });
        let column = if taken {
            let column = tree.column.as_ref().expect("checked above");
            reference_to(&column.uuid, "resqml22.StratigraphicColumn", label(column))
        } else {
            let keys: Vec<String> = ranks.iter().map(|one| one.uuid.to_ascii_lowercase()).collect();
            let uuid = cx.identity.uuid(&format!("earth-model:column:{}:{}", model.uuid.to_ascii_lowercase(), keys.join(",")), 50)?;
            let title = format!("{root_title} — column");
            let written = resqml::StratigraphicColumn {
                uuid: uuid.clone(),
                schema_version: "2.2".to_string(),
                citation: citation(&title, None),
                ranks: ranks.clone(),
                ..Default::default()
            };
            objects.push(NewObject {
                uuid: uuid.clone(),
                qualified_type: "resqml22.StratigraphicColumn".to_string(),
                role: "column".to_string(),
                title: title.clone(),
                json: energyml_core::json::write_value(&written),
            });
            reference_to(&uuid, "resqml22.StratigraphicColumn", &title)
        };

        // -- what exists in this model: an occurrence of each rank ---------------------------
        let mut occurrences: Vec<eml::DataObjectReference> = Vec::new();
        for (at, ((sub, rank), criteria)) in tree.ranks.iter().zip(&ranks).zip(criteria).enumerate() {
            occurrences.push(write_occurrence(sub, rank, criteria, &model, &root_title, at, cx, &citation, &mut objects)?);
        }
        says.push(Note {
            key: "occurrences",
            detail: format!(
                "{} occurrence(s), one per rank, each listing every unit of its rank: what exists in this model is the column as it is until a unit is removed from its occurrence",
                occurrences.len()
            ),
        });

        // -- the tops and bottoms, and their organisation ------------------------------------
        let mut structure: Option<eml::DataObjectReference> = None;
        let mut boundaries_written = 0usize;
        let mut boundaries_taken = 0usize;
        if params.boundaries {
            let existing = Existing::of(&inputs.existing);
            let ages = ages_of(columns);
            // Every unit of the model once, at the coarsest rank it appears in.
            let mut units: Vec<(Named, Option<i64>)> = Vec::new();
            for sub in &tree.ranks {
                for unit in &sub.units {
                    if !units.iter().any(|(one, _)| one.uuid == unit.uuid) {
                        units.push((unit.clone(), sub.number));
                    }
                }
            }
            let mut limits: Vec<Limit> = Vec::new();
            for (unit, rank) in &units {
                for side in [Side::Top, Side::Bottom] {
                    let (interpretation, wrote) = boundary(unit, side, &existing, cx, &citation, &mut objects)?;
                    if wrote {
                        boundaries_written += 1;
                    } else {
                        boundaries_taken += 1;
                    }
                    limits.push(Limit { unit: unit.clone(), side, rank: *rank, interpretation });
                }
            }
            match ordered_by_age(&mut limits, &ages) {
                Ok(()) => {
                    let (top, bottom) = frontiers(&tree, &limits);
                    let keys: Vec<String> = limits.iter().map(|one| one.interpretation.uuid.to_ascii_lowercase()).collect();
                    let uuid =
                        cx.identity.uuid(&format!("earth-model:structure:{}:{}", model.uuid.to_ascii_lowercase(), keys.join(",")), 60)?;
                    let title = format!("{root_title} — structure");
                    let organisation = resqml::StructuralOrganizationInterpretation {
                        uuid: uuid.clone(),
                        schema_version: "2.2".to_string(),
                        citation: citation(&title, None),
                        interpreted_feature: model.clone(),
                        ascending_ordering_criteria: resqml::OrderingCriteria::Age,
                        top_frontier: top.into_iter().collect(),
                        bottom_frontier: bottom.into_iter().collect(),
                        ordered_boundary_feature_interpretation: limits
                            .iter()
                            .map(|one| resqml::BoundaryFeatureInterpretationPlusItsRank {
                                stratigraphic_rank: one.rank,
                                boundary_feature_interpretation: Some(one.interpretation.clone()),
                                feature_interpretation_set: None,
                            })
                            .collect(),
                        ..Default::default()
                    };
                    objects.push(NewObject {
                        uuid: uuid.clone(),
                        qualified_type: "resqml22.StructuralOrganizationInterpretation".to_string(),
                        role: "organisation".to_string(),
                        title: title.clone(),
                        json: energyml_core::json::write_value(&organisation),
                    });
                    structure = Some(reference_to(&uuid, "resqml22.StructuralOrganizationInterpretation", &title));
                }
                Err(undated) => says.push(Note {
                    key: "structure-unordered",
                    detail: format!(
                        "{undated} unit(s) state no age, so the order of their tops and bottoms is not stated anywhere: the horizons are written, no structural organisation is"
                    ),
                }),
            }
            says.push(Note {
                key: "boundaries",
                detail: format!(
                    "{} top(s) and bottom(s): {boundaries_written} written, {boundaries_taken} taken from the package by their title",
                    boundaries_written + boundaries_taken
                ),
            });
        }

        // -- the earth model interpretation --------------------------------------------------
        let keep = match (params.mode, inputs.enrich.as_ref()) {
            (Mode::Enrich, Some(given)) => Some(dor("enrich", given)?.uuid),
            (Mode::Enrich, None) => {
                return Err(Refusal::on("enrich", "`mode = enrich` rewrites an earth model interpretation: name it in `enrich`"));
            }
            (Mode::Snapshot, Some(given)) => {
                return Err(Refusal::on(
                    "enrich",
                    format!("`{}` is given to enrich while `mode` is `snapshot` — say `mode = enrich`, or leave `enrich` empty", given.name),
                ));
            }
            (Mode::Snapshot, None) => None,
        };
        let gathered = Gathered {
            column: Some(column.clone()),
            structures: structure.into_iter().collect(),
            occurrences,
            ..Default::default()
        };
        let made = earth_model::write(model_is, params.title, &gathered, keep.as_deref(), params.variant.as_deref(), cx)?;
        let known_before = columns.columns.iter().any(|one| one.models.iter().any(|emi| emi.uuid.eq_ignore_ascii_case(&made.interpretation.uuid)));
        if known_before && keep.is_none() {
            says.push(Note {
                key: "unchanged",
                detail: format!("the package already holds `{}` with this content: the same objects are written again, no new snapshot", made.interpretation_title),
            });
        }
        // The model first — everything interprets it — then what the
        // interpretation references, then the interpretation.
        let (model_objects, interpretation_objects): (Vec<_>, Vec<_>) =
            made.objects.into_iter().partition(|one| one.qualified_type == "resqml22.Model");
        let mut ordered = model_objects;
        ordered.extend(objects);
        ordered.extend(interpretation_objects);

        says.insert(
            0,
            Note {
                key: "contains",
                detail: format!(
                    "{} rank(s) — {} taken as they are, {restricted} restricted — holding {} unit(s)",
                    tree.ranks.len(),
                    tree.ranks.len() - restricted,
                    tree.ranks.iter().map(|one| one.units.len()).sum::<usize>()
                ),
            },
        );
        if !tree.unplaced.is_empty() {
            dropped.dedup();
        }

        let number = |title: &str, value: usize| Parameter::Double { title: title.to_string(), value: value as f64, uom: None };
        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            objects: ordered,
            datasets: Vec::new(),
            activity: ActivityDraft {
                recipe: "earth-model-from-stratigraphy",
                title: format!("earth-model-from-stratigraphy → {}", made.interpretation_title),
                seed: format!("earth-model-from-stratigraphy:{}:{}", from.uuid.to_ascii_lowercase(), made.seed),
                parameters: vec![
                    number("rankCount", tree.ranks.len()),
                    number("restrictedRankCount", restricted),
                    number("boundaryCount", boundaries_written + boundaries_taken),
                ],
            },
            says,
            dropped,
            shapes: Vec::new(),
        })
    }

    /// What was counted while planning.
    fn reports(&self) -> Vec<crate::operation::Slot> {
        let count = |name: &'static str, about: &'static str| {
            crate::operation::Slot::required(name, crate::operation::Need::Number { uom: None, least: None, most: None }, about)
        };
        vec![
            count("rankCount", "the ranks of the model's column"),
            count("restrictedRankCount", "the ranks written restricted to the unit's content"),
            count("boundaryCount", "the tops and bottoms of the model's units, written or taken"),
        ]
    }
}

/// The criterion a rank of the subtree states; age, and said, when this build
/// does not know it.
fn criteria_of(sub: &SubRank, says: &mut Vec<Note>) -> resqml::OrderingCriteria {
    criterion(&sub.criteria).unwrap_or_else(|| {
        says.push(Note {
            key: "criterion-unread",
            detail: format!(
                "rank `{}` states the criterion « {} », which this build does not know: what is written from it is ordered by age",
                label(&sub.source),
                sub.criteria
            ),
        });
        resqml::OrderingCriteria::Age
    })
}

/// A rank restricted to the subtree: it interprets the model and keeps its
/// source's number and criterion.
#[allow(clippy::too_many_arguments)]
fn write_rank(
    sub: &SubRank,
    criteria: resqml::OrderingCriteria,
    model: &eml::DataObjectReference,
    root_title: &str,
    at: usize,
    cx: &Context<'_>,
    citation: &dyn Fn(&str, Option<String>) -> eml::Citation,
    objects: &mut Vec<NewObject>,
) -> Result<eml::DataObjectReference, Refusal> {
    let number = sub.number.unwrap_or(at as i64);
    let keys: Vec<String> = sub.units.iter().map(|one| one.uuid.clone()).collect();
    let uuid = cx.identity.uuid(
        &format!("earth-model:rank:{}:{}:{}", model.uuid.to_ascii_lowercase(), sub.source.uuid, keys.join(",")),
        10 + at,
    )?;
    let title = format!("{} — {root_title}", label(&sub.source));
    let rank = resqml::StratigraphicColumnRankInterpretation {
        uuid: uuid.clone(),
        schema_version: "2.2".to_string(),
        citation: citation(
            &title,
            Some(format!(
                "Rank `{}` restricted to the {} unit(s) inside `{root_title}`, by earth-model-from-stratigraphy.",
                label(&sub.source),
                sub.units.len()
            )),
        ),
        interpreted_feature: model.clone(),
        ascending_ordering_criteria: criteria,
        rank_in_stratigraphic_column: number,
        stratigraphic_units: sub
            .units
            .iter()
            .map(|unit| reference_to(&unit.uuid, "resqml22.StratigraphicUnitInterpretation", label(unit)))
            .collect(),
        ..Default::default()
    };
    objects.push(NewObject {
        uuid: uuid.clone(),
        qualified_type: "resqml22.StratigraphicColumnRankInterpretation".to_string(),
        role: "rank".to_string(),
        title: title.clone(),
        json: energyml_core::json::write_value(&rank),
    });
    Ok(reference_to(&uuid, "resqml22.StratigraphicColumnRankInterpretation", &title))
}

/// The occurrence of one rank in the model: the rank it is an occurrence of —
/// taken or restricted —, its units in the rank's order, its criterion. It
/// interprets the model, as the restricted rank and the structure do.
///
/// Its uuid comes from the model, the rank and the units it lists: an
/// occurrence a unit was removed from is another occurrence.
#[allow(clippy::too_many_arguments)]
fn write_occurrence(
    sub: &SubRank,
    rank: &eml::DataObjectReference,
    criteria: resqml::OrderingCriteria,
    model: &eml::DataObjectReference,
    root_title: &str,
    at: usize,
    cx: &Context<'_>,
    citation: &dyn Fn(&str, Option<String>) -> eml::Citation,
    objects: &mut Vec<NewObject>,
) -> Result<eml::DataObjectReference, Refusal> {
    let keys: Vec<String> = sub.units.iter().map(|one| one.uuid.to_ascii_lowercase()).collect();
    let uuid = cx.identity.uuid(
        &format!(
            "earth-model:occurrence:{}:{}:{}",
            model.uuid.to_ascii_lowercase(),
            rank.uuid.to_ascii_lowercase(),
            keys.join(",")
        ),
        70 + at,
    )?;
    let title = format!("{root_title} — occurrence of {}", label(&sub.source));
    let occurrence = resqml::GeologicUnitOccurrenceInterpretation {
        uuid: uuid.clone(),
        schema_version: "2.2".to_string(),
        citation: citation(
            &title,
            Some(format!(
                "The {} unit(s) of rank `{}` that exist in `{root_title}`, by earth-model-from-stratigraphy.",
                sub.units.len(),
                label(&sub.source)
            )),
        ),
        interpreted_feature: model.clone(),
        ascending_ordering_criteria: criteria,
        geologic_unit: sub
            .units
            .iter()
            .map(|unit| reference_to(&unit.uuid, "resqml22.StratigraphicUnitInterpretation", label(unit)))
            .collect(),
        is_occurrence_of: Some(rank.clone()),
        ..Default::default()
    };
    objects.push(NewObject {
        uuid: uuid.clone(),
        qualified_type: "resqml22.GeologicUnitOccurrenceInterpretation".to_string(),
        role: "occurrence".to_string(),
        title: title.clone(),
        json: energyml_core::json::write_value(&occurrence),
    });
    Ok(reference_to(&uuid, "resqml22.GeologicUnitOccurrenceInterpretation", &title))
}

/// A unit's top or bottom: the horizon interpretation of that title in the
/// package, or one written here with its feature. `true` when written.
fn boundary(
    unit: &Named,
    side: Side,
    existing: &Existing,
    cx: &Context<'_>,
    citation: &dyn Fn(&str, Option<String>) -> eml::Citation,
    objects: &mut Vec<NewObject>,
) -> Result<(eml::DataObjectReference, bool), Refusal> {
    let title = format!("{} {}", label(unit), side.word());
    if let Some(taken) = Existing::one(&existing.interpretations, &title, "horizon interpretation")? {
        return Ok((taken, false));
    }
    // The uuids come from the unit and the side only: a unit's top is the same
    // object in every model made of it.
    let seed = format!("earth-model:boundary:{}:{}", unit.uuid.to_ascii_lowercase(), side.word().to_lowercase());
    let feature = match Existing::one(&existing.features, &title, "boundary feature")? {
        Some(taken) => taken,
        None => {
            let uuid = cx.identity.uuid(&format!("{seed}:feature"), 100)?;
            let feature = resqml::BoundaryFeature {
                uuid: uuid.clone(),
                schema_version: "2.2".to_string(),
                citation: citation(&title, None),
                is_well_known: false,
                ..Default::default()
            };
            objects.push(NewObject {
                uuid: uuid.clone(),
                qualified_type: "resqml22.BoundaryFeature".to_string(),
                role: "feature".to_string(),
                title: title.clone(),
                json: energyml_core::json::write_value(&feature),
            });
            reference_to(&uuid, "resqml22.BoundaryFeature", &title)
        }
    };
    let uuid = cx.identity.uuid(&format!("{seed}:interpretation"), 101)?;
    let interpretation = resqml::HorizonInterpretation {
        uuid: uuid.clone(),
        schema_version: "2.2".to_string(),
        citation: citation(&title, None),
        interpreted_feature: feature,
        ..Default::default()
    };
    objects.push(NewObject {
        uuid: uuid.clone(),
        qualified_type: "resqml22.HorizonInterpretation".to_string(),
        role: "interpretation".to_string(),
        title: title.clone(),
        json: energyml_core::json::write_value(&interpretation),
    });
    Ok((reference_to(&uuid, "resqml22.HorizonInterpretation", &title), true))
}

/// Every unit's age span, `[old, young]` in Ma, by uuid.
fn ages_of(columns: &Columns) -> BTreeMap<String, [f64; 2]> {
    columns
        .columns
        .iter()
        .flat_map(|column| column.ranks.iter())
        .chain(columns.loose_ranks.iter())
        .flat_map(|rank| rank.units.iter())
        .filter_map(|unit| unit.age.map(|age| (unit.unit.uuid.clone(), age)))
        .collect()
}

/// The limits from the youngest down, when every unit states its age; how
/// many do not, otherwise.
///
/// A top is at its unit's young end, a bottom at its old end. At one age the
/// bottom of the unit above comes before the top of the unit below (both are written, base of A then top of B); among tops the coarser comes
/// first, among bottoms the finer — the outer limit encloses the inner.
fn ordered_by_age(limits: &mut [Limit], ages: &BTreeMap<String, [f64; 2]>) -> Result<(), usize> {
    let undated = {
        let mut seen: Vec<&str> = limits.iter().filter(|one| !ages.contains_key(&one.unit.uuid)).map(|one| one.unit.uuid.as_str()).collect();
        seen.sort_unstable();
        seen.dedup();
        seen.len()
    };
    if undated > 0 {
        return Err(undated);
    }
    let key = |one: &Limit| {
        let [old, young] = ages[&one.unit.uuid];
        let rank = one.rank.unwrap_or(i64::MAX);
        match one.side {
            Side::Bottom => (old, 0, -rank),
            Side::Top => (young, 1, rank),
        }
    };
    limits.sort_by(|a, b| {
        let (ka, kb) = (key(a), key(b));
        ka.0.total_cmp(&kb.0).then(ka.1.cmp(&kb.1)).then(ka.2.cmp(&kb.2))
    });
    Ok(())
}

/// The model's top and bottom: those of its root — the unit, or the first and
/// last units of a whole column's coarsest rank.
fn frontiers(tree: &Subtree, limits: &[Limit]) -> (Option<eml::DataObjectReference>, Option<eml::DataObjectReference>) {
    let Some(coarsest) = tree.ranks.first() else { return (None, None) };
    let find = |unit: Option<&Named>, side: Side| {
        unit.and_then(|unit| limits.iter().find(|one| one.unit.uuid == unit.uuid && one.side == side)).map(|one| one.interpretation.clone())
    };
    (find(coarsest.units.first(), Side::Top), find(coarsest.units.last(), Side::Bottom))
}
