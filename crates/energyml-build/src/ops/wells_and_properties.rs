//! `wells-and-properties-from-table` — the boreholes **and** their properties, in
//! un run.
//!
//! The need: an operation that chains the two automatically, and that
//! also appears in the front end for translation and enrichment — after
//! noting that an import from the front end did not lay the properties
//! on them.
//!
//! # A composition, not a third writer
//!
//! The crate's rule: *an operation that writes the same object as another
//! delegates to it*, through its typed function and never through a derived bag. The boreholes
//! come from [`WellsFromTable::plan_ending`], which also returns **the frame on
//! which each borehole ends** and its depths; the properties
//! come from [`properties_from_table::lay`], laid on those frames.
//!
//! This is what makes the composition correct where a chaining of two plans
//! would not be: the second operation does not re-read the package — the new frames
//! are not in it yet — it receives those that the first has just
//! decided, and those of the package that it has recognised.
//!
//! # The passes tied to the units of the BRGM chart
//!
//! Step 3 of the table-to-boreholes design. When `units` names the column that gives
//! the unit of each pass (`STRA`, and `unitLabels` its label, `Nom_STRA`),
//! each frame receives **`IntervalStratigraphicUnits`** (TRG 2.2 §5.34): one
//! unit index per **interval**, in a rank of a stratigraphic
//! column — *“Array length must equal the number of intervals”*,
//! which the passes make possible.
//!
//! Three choices, written here because they decide the shape of the package:
//!
//! - **the column is that of the observed divisions** — exactly those that
//!   the passes name, written by [`super::stratigraphy::write_column`]. Their
//!   uuids come from their URI in the register: the Priabonian of this column
//!   **is** that of a column drawn from the chart by
//!   `stratigraphy-from-geolts`, not a second one;
//! - **one rank per level, hence one correspondence per rank**: `Q` is a
//!   system, `e4`–`e7` are stages. The frame admits several
//!   `IntervalStratigraphicUnits` (0..*); in each, an interval whose
//!   unit is of another level — or of no known division — carries the
//!   null value, which the rule provides for (*“use null value”*);
//! - **the correspondence is in the identity of the frame**: the seed of the
//!   trajectory and of the frame carries the units of each pass. Another
//!   classification of the same passes is therefore a second frame, never the
//!   first one rewritten.
//!
//! A pass whose code and label designate no division of the
//! chart — `n.d.` in a producer's table — stays **without a unit, and counted**: inventing a
//! local one is one more step, not a guess of this operation.
//!
//! # What is not taken over
//!
//! `subdivide`: the new frames are built on the depths of the file
//! itself, so no row has an MD that they do not carry. And `enrich`
//! stays off by default, for the same reason as in `wells-from-table`.

use std::collections::BTreeMap;

use energyml_domain::geolts::{self, Chart};
use energyml_eml_2_3 as eml;
use energyml_resqml_2_2 as resqml;

use crate::declare::{Epsg, Inputs, ObjectIn, Params, Suggestions, TableIn};
use crate::operation::{About, ActivityDraft, Category, Context, Note, Operation, Output, Plan, Refusal};
use crate::pieces::Pieces;

use super::properties_from_table::{self, Frames, PropertiesParams, TextColumns};
use super::well::DatumKind;
use super::wells_from_table::{SetAside, TableInputs, TableParams, WellsFromTable, column_at, group, intervals};

/// The operation.
pub struct WellsAndProperties;

/// What it is told — the settings of the two operations, `dataFile` in common.
#[derive(Debug, Params)]
pub struct BothParams {
    /// the EPSG code the file's coordinates are in — left out, a column must state it
    pub source_epsg: Option<Epsg>,
    /// reproject into this EPSG system, in metres — a borehole cannot be written in degrees
    pub project: Option<Epsg>,
    /// the EPSG code of the vertical system, when known; left out, the package says unknown
    pub vertical: Option<Epsg>,
    /// what the measured depths are counted from — the ground, unless the file says otherwise
    #[param(default = DatumKind::GroundLevel)]
    pub datum_kind: DatumKind,
    /// the external file the coordinates and the property values go into
    #[param(default = String::from("data.h5"))]
    pub data_file: String,
    /// how far apart two wellheads may be and still be the same hole, in metres
    #[param(default = 0.01)]
    pub head_tolerance: f64,
    /// add the new depths to a recognised well's frame instead of writing a second one
    ///
    /// Off, for the reason `wells-from-table` gives: inserting a node
    /// renumbers the frame, and the properties already laid on it then
    /// describe the wrong depths.
    #[param(default = false)]
    pub enrich: bool,
    /// the integer a discrete property uses where the file says nothing
    #[param(default = -9999i64, optional)]
    pub null_value: i64,
    /// the column naming each pass's stratigraphic unit, a code like `STRA` — given, each frame maps its intervals to the units of the BRGM chart
    pub units: Option<String>,
    /// the column giving that code's label, like `Nom_STRA`, which tells apart the chart's divisions sharing a code
    pub unit_labels: Option<String>,
    /// the language the chart's divisions are titled in — the same units as `stratigraphy-from-geolts` writes, so the same setting
    #[param(default = super::stratigraphy::Language::Fr, optional)]
    pub language: super::stratigraphy::Language,
    /// how the text columns are written — one property each, or gathered in a table with one property pointing at its rows
    #[param(default = TextColumns::Separate, optional)]
    pub text_columns: TextColumns,
    /// also carry the columns the template does not name, in a table of their own, rather than leave them out
    #[param(default = false, optional)]
    pub everything: bool,
}

/// A pass, and the unit the file gives it: `(top, base, code, label)`.
type Pass = (f64, f64, String, String);

/// What it takes: the table, the boreholes of the package, the code tables to
/// continue.
#[derive(Debug, Inputs)]
pub struct BothInputs {
    /// the table, with what its columns mean
    #[input(least = 1)]
    pub table: Vec<TableIn>,
    /// the wells already in the package, to recognise rather than duplicate
    ///
    /// Left empty, these are all the frames of the target package — the declaration
    /// of `wells-from-table`, taken over as it is.
    #[input(types = "resqml22.WellboreFrameRepresentation, resqml20.obj_WellboreFrameRepresentation", every)]
    pub wells: Vec<ObjectIn>,
    /// the category tables to continue rather than duplicate
    #[input(types = "eml23.ColumnBasedTable, resqml20.StringTableLookup")]
    pub lookups: Vec<ObjectIn>,
}

impl BothParams {
    fn wells(&self) -> TableParams {
        TableParams {
            source_epsg: self.source_epsg,
            project: self.project,
            vertical: self.vertical,
            datum_kind: self.datum_kind,
            data_file: self.data_file.clone(),
            head_tolerance: self.head_tolerance,
            enrich: self.enrich,
        }
    }

    fn properties(&self) -> PropertiesParams {
        PropertiesParams {
            data_file: self.data_file.clone(),
            null_value: self.null_value,
            subdivide: false,
            text_columns: self.text_columns,
            everything: self.everything,
        }
    }
}

/// The union of what the halves write, **and the units column's** — each
/// half's own list, merged by role, never retyped.
///
/// A role two halves share (`feature`, `interpretation`: the wells' and the
/// chart's) lists both halves' types; its bounds add up. The chart's objects
/// are written only when `units` is given, so none of them is owed (`least`
/// 0).
fn union() -> Vec<Output> {
    let chart = super::stratigraphy::outputs().into_iter().map(|one| Output { least: 0, ..one });
    let parts = [super::wells_from_table::outputs(), super::properties_from_table::outputs(), chart.collect()];
    let mut merged: Vec<Output> = Vec::new();
    for one in parts.into_iter().flatten() {
        match merged.iter_mut().find(|have| have.role == one.role) {
            None => merged.push(one),
            Some(have) => {
                let mut types: Vec<&'static str> = have.types.to_vec();
                types.extend(one.types.iter().copied().filter(|kind| !have.types.contains(kind)));
                // Declarations are static; a merged list is built once and kept.
                have.types = Box::leak(types.into_boxed_slice());
                have.least += one.least;
                have.most = have.most.zip(one.most).map(|(a, b)| a + b);
                have.describe = Box::leak(format!("{}; {}", have.describe, one.describe).into_boxed_str());
            }
        }
    }
    merged
}

impl Operation for WellsAndProperties {
    type Params = BothParams;
    type Inputs = BothInputs;

    fn gives(&self) -> Vec<Output> {
        // Leaked once: the merged type lists must be `'static`.
        static MERGED: std::sync::OnceLock<Vec<Output>> = std::sync::OnceLock::new();
        MERGED.get_or_init(union).clone()
    }

    fn about(&self) -> About {
        About {
            name: "wells-and-properties-from-table",
            describe: "wellbores and the properties along them from a tabular file and an OSDU column template, in one run: wells-from-table, then properties-from-table on the frames it made or recognised",
            category: Category::Import,
            makes: &[
                // In the compound the run creates — `wells-from-table`'s datum.
                "eml23.ReferencePointInALocalEngineeringCompoundCrs",
                "resqml22.WellboreFeature",
                "resqml22.WellboreInterpretation",
                "resqml22.WellboreTrajectoryRepresentation",
                "resqml22.WellboreFrameRepresentation",
                "resqml22.ContinuousProperty",
                "resqml22.DiscreteProperty",
                // No `CategoricalProperty`: RESQML 2.2 has none
                // `tests/makes.rs`) — the table below is what gives a code
                // its meaning, through `DiscreteProperty.CategoryLookup`.
                "eml23.ColumnBasedTable",
            ],
            recipe: "wells-and-properties-from-table",
            represents: None,
        }
    }

    fn also(&self) -> &'static [Category] {
        &[Category::Wells, Category::Properties]
    }

    /// What `wells-from-table` proposes — the system of the file, and a metric
    /// zone when it is in degrees. The slots carry the same names.
    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        WellsFromTable.suggest(bag, slots);
    }

    /// What its two halves report, under the same titles.
    fn reports(&self) -> Vec<crate::operation::Slot> {
        properties_from_table::PropertiesFromTable.reports()
    }

    /// The content of the table, judged by `wells-from-table`: without the roles
    /// of a borehole, there is nothing to create and therefore nothing to lay on.
    fn check(&self, inputs: &BothInputs) -> Result<(), Refusal> {
        WellsFromTable.check(&TableInputs { table: inputs.table.clone(), wells: Vec::new() })
    }

    fn plan(&self, inputs: BothInputs, params: BothParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        let BothInputs { table, wells, lookups } = inputs;
        // **The units of each pass, read before a frame is written**: they
        // enter its identity (see the module).
        let passes = match &params.units {
            Some(column) => {
                let _span = tracing::info_span!("wells_props.passes", column = column.as_str()).entered();
                Some(passes_of(&table, column, params.unit_labels.as_deref())?)
            }
            None => None,
        };
        let more = |hole: &str| -> Option<String> {
            let passes = passes.as_ref()?.get(hole)?;
            Some(format!(
                "units:{}",
                passes.iter().map(|(_, _, code, label)| format!("{code}/{label}")).collect::<Vec<_>>().join(",")
            ))
        };
        let (first, ends) = {
            let _span = tracing::info_span!("wells_props.wells").entered();
            WellsFromTable.plan_ending(TableInputs { table: table.clone(), wells }, params.wells(), cx, &more)?
        };
        let frames: Vec<(crate::pieces::Ref, Vec<f64>)> =
            ends.iter().map(|(_, frame, mds)| (frame.clone(), mds.clone())).collect();

        // **A table of heads has nothing to lay**: no frame, no MD.
        // This is the first plan, said as it is, and not the refusal of
        // `properties-from-table` — which would state a fault where there is only a
        // table without measurements.
        if frames.is_empty() {
            let mut plan = first;
            plan.says.push(Note {
                key: "no-properties",
                detail: "the table writes no frame, so there is nothing to lay a property on".to_string(),
            });
            return Ok(rename(plan));
        }

        let second = {
            let _span = tracing::info_span!("wells_props.properties", frames = frames.len()).entered();
            properties_from_table::lay(&table, &Frames::given(&frames), &lookups, &params.properties(), cx)?
        };

        let mut plan = first;
        // Carried, the columns no template names are not *left out*: the first
        // half's note would say the opposite of the second's.
        if params.everything {
            plan.says.retain(|note| note.key != "columns-ignored");
        }
        plan.objects.extend(second.objects);
        plan.datasets.extend(second.datasets);
        plan.says.extend(second.says);
        plan.dropped.extend(second.dropped);
        plan.activity.parameters.extend(second.activity.parameters);
        if let (Some(passes), Some(column)) = (&passes, &params.units) {
            let title = format!("{} — {column}", table.first().map(|one| one.file.as_str()).unwrap_or("table"));
            let language = params.language;
            let _span = tracing::info_span!("wells_props.attach_units").entered();
            geolts::with_default(|chart| attach(chart, &mut plan, &ends, passes, &title, language, cx))?;
        }
        Ok(rename(plan))
    }
}

/// The activity carries **the recipe of this run**, not that of its first half:
/// it is what a second run will read again to take up the same settings.
fn rename(mut plan: Plan) -> Plan {
    let seed = plan.activity.seed.clone();
    plan.activity = ActivityDraft {
        recipe: "wells-and-properties-from-table",
        title: format!("wells-and-properties-from-table → {seed}"),
        seed,
        parameters: std::mem::take(&mut plan.activity.parameters),
    };
    plan
}

/// The passes of each borehole, with the code and the label of their unit.
fn passes_of(tables: &[TableIn], column: &str, labels: Option<&str>) -> Result<BTreeMap<String, Vec<Pass>>, Refusal> {
    let mut out: BTreeMap<String, Vec<Pass>> = BTreeMap::new();
    let mut aside = SetAside::default();
    for input in tables {
        let table = &input.table;
        let at = |name: &str, slot: &str| -> Result<usize, Refusal> {
            table.column(name).ok_or_else(|| {
                Refusal::on(slot, format!("`{}` has no column `{name}` (it names: {})", input.file, table.headers.join(", ")))
            })
        };
        let unit_at = at(column, "units")?;
        let label_at = labels.map(|name| at(name, "unitLabels")).transpose()?;
        let (Some(name_at), Some(top_at), Some(md_at)) = (
            column_at(table, &input.template, "wellName")?,
            column_at(table, &input.template, "topDepth")?,
            column_at(table, &input.template, "measuredDepth")?,
        ) else {
            return Err(Refusal::on(
                "units",
                format!(
                    "`{}`: a unit is laid on the **intervals** of a frame, so the template must declare the well name, the measured depth and where each pass starts — its `top depth`",
                    input.file
                ),
            ));
        };
        for hole in group(table, name_at, &mut aside) {
            let found = intervals(table, &hole.rows, top_at, md_at, &hole.title).map_err(|why| Refusal::on("table", why))?;
            let cell = |row: usize, at: usize| table.rows[row].get(at).map(|one| one.trim().to_string()).unwrap_or_default();
            out.insert(
                hole.title.clone(),
                found
                    .iter()
                    .map(|(top, base, row)| (*top, *base, cell(*row, unit_at), label_at.map(|at| cell(*row, at)).unwrap_or_default()))
                    .collect(),
            );
        }
    }
    Ok(out)
}

/// The column of the observed divisions, and the correspondence of each frame.
fn attach(
    chart: &Chart,
    plan: &mut Plan,
    ends: &[(String, crate::pieces::Ref, Vec<f64>)],
    passes: &BTreeMap<String, Vec<Pass>>,
    title: &str,
    language: super::stratigraphy::Language,
    cx: &Context<'_>,
) -> Result<(), Refusal> {
    // Each (code, label) pair once: it is what the chart tells apart.
    let mut named: BTreeMap<(String, String), Option<u32>> = BTreeMap::new();
    let mut unknown: BTreeMap<(String, String), usize> = BTreeMap::new();
    for pass in passes.values().flatten() {
        let key = (pass.2.clone(), pass.3.clone());
        if !named.contains_key(&key) {
            let found = if key.1.is_empty() {
                // Without a label, the code alone — unique or refused by
                // `resolve`, never the first that comes.
                chart.resolve(&key.0).ok().map(|one| one.id)
            } else {
                chart.find(&key.0, &key.1).map_err(|why| Refusal::on("units", why))?.map(|one| one.id)
            };
            named.insert(key.clone(), found);
        }
        if named.get(&key).copied().flatten().is_none() {
            *unknown.entry(key).or_default() += 1;
        }
    }
    let mut ids: Vec<u32> = named.values().copied().flatten().collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        plan.says.push(Note {
            key: "units-unknown",
            detail: "no pass names a division of the BRGM chart, so no frame maps its intervals to units".to_string(),
        });
        return Ok(());
    }
    let column = super::stratigraphy::write_column(chart, &ids, &format!("{title} (BRGM chart)"), language, cx)?;

    let mut mapped = 0usize;
    let mut intervals_mapped = 0usize;
    let mut missing: Vec<String> = Vec::new();
    for (hole, frame, mds) in ends {
        let Some(its) = passes.get(hole) else { continue };
        if mds.len() < 2 {
            continue;
        }
        // The unit of each interval of the frame: the pass that covers it.
        let per_interval: Vec<Option<u32>> = mds
            .windows(2)
            .map(|pair| {
                its.iter()
                    .find(|(top, base, _, _)| *top <= pair[0] + 1e-6 && *base >= pair[1] - 1e-6)
                    .and_then(|pass| named.get(&(pass.2.clone(), pass.3.clone())).copied().flatten())
            })
            .collect();
        let mut maps: Vec<serde_json::Value> = Vec::new();
        for rank in &column.ranks {
            let indices: Vec<i64> = per_interval
                .iter()
                .map(|id| id.and_then(|id| rank.units.iter().position(|one| *one == id)).map(|at| at as i64).unwrap_or(-1))
                .collect();
            if indices.iter().all(|one| *one < 0) {
                continue;
            }
            intervals_mapped += indices.iter().filter(|one| **one >= 0).count();
            let count = indices.len() as i64;
            let map = resqml::IntervalStratigraphicUnits {
                // One element per interval: the cumulative length counts 1, 2, …
                unit_indices: eml::JaggedArray {
                    elements: Some(eml::AbstractValueArray::IntegerXmlArray(eml::IntegerXmlArray {
                        count_per_value: 1,
                        null_value: -1,
                        values: indices,
                        ..Default::default()
                    })),
                    cumulative_length: Some(eml::AbstractIntegerArray::IntegerXmlArray(eml::IntegerXmlArray {
                        count_per_value: 1,
                        null_value: -1,
                        values: (1..=count).collect(),
                        ..Default::default()
                    })),
                },
                stratigraphic_organization_interpretation: rank.reference.clone(),
            };
            maps.push(energyml_core::json::write_value(&map));
        }
        if maps.is_empty() {
            continue;
        }
        match plan.objects.iter_mut().find(|one| one.uuid == frame.uuid) {
            Some(object) => {
                object.json["IntervalStratigraphicUnits"] = serde_json::Value::Array(maps);
                mapped += 1;
            }
            None => missing.push(hole.clone()),
        }
    }

    plan.objects.extend(column.objects);
    plan.says.push(Note {
        key: "units",
        detail: format!(
            "{mapped} frame(s) map {intervals_mapped} interval(s) to {} division(s) of the BRGM chart, in {} rank(s): {}",
            column.units,
            column.ranks.len(),
            column.ranks.iter().map(|rank| geolts::level_name(rank.level)).collect::<Vec<_>>().join(", ")
        ),
    });
    if !unknown.is_empty() {
        plan.says.push(Note {
            key: "units-unknown",
            detail: format!(
                "{} pass(es) name no division of the chart and are left without a unit: {}",
                unknown.values().sum::<usize>(),
                unknown
                    .iter()
                    .map(|((code, label), n)| format!("{code}{} × {n}", if label.is_empty() { String::new() } else { format!(" {label}") }))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }
    if !missing.is_empty() {
        plan.says.push(Note {
            key: "units-not-written",
            detail: format!("{} frame(s) are not written by this run, so their units are not either: {}", missing.len(), missing.join(", ")),
        });
    }
    Ok(())
}
