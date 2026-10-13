//! `properties-from-table` — the columns of a CSV laid on frames.
//!
//! The second half of the table-to-boreholes design: `wells-from-table`
//! **creates** the boreholes, this one **enriches** — it makes no frame and
//! lays the values on those the package already holds.
//!
//! # What makes this operation delicate: an array is positional
//!
//! A property on a frame is an array where the value *i* belongs to node *i*.
//! An array shorter than the frame **is not noticed**: it reads fine, and each
//! value is assigned to the wrong node. The requirement:
//! when properties are linked, empty rows must be interleaved wherever the model has MDs and the CSV has none:
//!
//! > otherwise the enrichment properties will have fewer rows and
//! > shift.
//!
//! Hence the rule that governs the whole module: **an array written here has
//! exactly as many values as the frame has nodes**, whatever happens.
//! What the file does not say is a **declared null**, never an
//! invented value and never a missing cell.
//!
//! # Three counts, and they are reported
//!
//! `filled` (nodes filled), `added` (MDs added to the frame), `dropped`
//! (values set aside for lack of a node). A property that is 90% nulls is the sign
//! that the template or the file does not match the model, and it is a
//! number that says so.
//!
//! # What is not there yet
//!
//! Columns that reference another CSV are not handled yet. The grouping into
//! `eml23.ColumnBasedTable` is
//! done since 01/10: `textColumns` and `everything`, see
//! `crates/energyml-build/ARCHITECTURE.md` — the default remains one property per
//! column.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::declare::{Choice, Inputs, ObjectIn, Params, Shape, Suggestions, TableIn};
use crate::operation::{
    About, ActivityDraft, Category, Context, Dataset, Identity, Need, NewObject, Note, Numbers, Operation, Output,
    Plan, Refusal, Slot,
};
use energyml_domain::provenance::Parameter;
use crate::osdu::{Column, ValueType};
use crate::pieces::Pieces;

use super::support;
use super::wells_from_table::{SetAside, column_at, group, intervals, role};

/// The operation.
pub struct PropertiesFromTable;

/// What it is set to.
#[derive(Debug, Params)]
pub struct PropertiesParams {
    /// the HDF5 file the value arrays go into
    #[param(default = "data.h5".to_string(), group = "naming")]
    pub data_file: String,
    /// the integer a discrete property uses where the file says nothing
    ///
    /// A continuous property has `NaN`; a discrete one has no natural null and
    /// RESQML makes it state one. Writing `0` would be a value nobody measured,
    /// which is why this is declared rather than assumed.
    #[param(default = -9999i64, optional, group = "operation")]
    pub null_value: i64,
    /// add the file's measured depths that the frame does not carry
    ///
    /// Left off, the frame is not touched and a value whose depth the model
    /// does not have is set aside and counted — never moved to the nearest
    /// node, which would be inventing. This is the *over-splitting* switch.
    #[param(default = false, optional, group = "operation")]
    pub subdivide: bool,
    /// how the text columns are written — one property each, or gathered in a table with one property pointing at its rows
    ///
    /// `separate` is what this operation always wrote: one `DiscreteProperty`
    /// and one single-column table per text column. `shared-table` gathers
    /// every text column into **one** `ColumnBasedTable` for the whole run, a
    /// row per distinct combination of values, and each frame gets **one**
    /// `DiscreteProperty` whose value is the rank of its row. `table-per-well`
    /// writes one table per frame instead, a row per element that carries a
    /// value. Continuous columns stay properties of their own in every mode.
    #[param(default = TextColumns::Separate, optional, group = "operation")]
    pub text_columns: TextColumns,
    /// also carry the columns the template does not name, in a table of their own, rather than leave them out
    ///
    /// They have no property kind, so they get the dictionary's `unknown`
    /// (*“a specific Property Code cannot be determined from the information
    /// provided”*), and their table and its pointing property say so in their
    /// titles. Gathered as `textColumns` says — a table per frame with
    /// `table-per-well`, one for the run otherwise.
    #[param(default = false, optional, group = "operation")]
    pub everything: bool,
}

/// How the text columns of a table are written. Decided on 01/10/2026:
/// both are possible choices, the default being what was
/// written before, so that packages written then are written the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum TextColumns {
    /// One discrete property and one single-column table per text column.
    Separate,
    /// One table for the run, a row per distinct combination; one property per frame.
    SharedTable,
    /// One table per frame, a row per element with a value; one property per frame.
    TablePerWell,
}

/// The property kind of the columns no template names — the dictionary's
/// `unknown`, concrete, *“used to assert that a specific Property Code cannot
/// be determined from the information provided”*. The root `property` is
/// abstract and may not type a property.
const UNKNOWN_KIND: (&str, &str) = ("73f28137-03ef-45b4-8e85-31869c7d36fd", "unknown");

/// The property kind of a property whose value is the rank of a row of a
/// multi-column table — the dictionary's `multi use property`, *“whose
/// meaning can only be interpreted within the context of its use”*: the
/// context is the table its `CategoryLookup` names.
const POINTER_KIND: (&str, &str) = ("47a4319d-c8a4-45dc-8600-f96d4b289138", "multi use property");

/// What it takes: tables, and the frames to enrich.
#[derive(Debug, Inputs)]
pub struct PropertiesInputs {
    /// the tables whose columns become properties, with what their columns mean
    #[input(least = 1)]
    pub table: Vec<TableIn>,
    /// the wellbore frames to lay the values on
    ///
    /// Their MDs arrive resolved (`Known::node_mds`): they are what
    /// says at which array rank a row of the file lands.
    ///
    /// **Left empty, these are all the frames of the target package** — the same
    /// declaration as `wells` of `wells-from-table`, and for the same reason:
    /// they are found by the name of the borehole, so those that no row
    /// names receive nothing.
    #[input(least = 1, types = "resqml22.WellboreFrameRepresentation, resqml20.obj_WellboreFrameRepresentation", every)]
    pub frames: Vec<Shape>,
    /// the category tables to continue rather than duplicate
    ///
    /// An `eml23.ColumnBasedTable` already in the package, given here, is
    /// **extended**: its rows keep their rank, so the codes already written
    /// keep their meaning, and the new values are added at the end. Without it,
    /// a second batch writes its own table and the same integer designates two
    /// things — measured on two batches of the BRGM corpus.
    #[input(types = "eml23.ColumnBasedTable, resqml20.StringTableLookup")]
    pub lookups: Vec<ObjectIn>,
}

/// What a run of `properties-from-table` writes, by role. Reused by
/// `wells-and-properties-from-table`, which runs it second.
///
/// Which of them a run writes depends on `textColumns` and `everything`: a
/// `separate` run writes one `category-lookup` per text column and no
/// `gathered-table`; the two gathering modes write `gathered-table`s (one for
/// the run, or one per frame) and **no** `category-lookup`.
pub(crate) fn outputs() -> Vec<Output> {
    vec![
        Output {
            role: "continuous-property",
            types: &["resqml22.ContinuousProperty"],
            least: 0,
            most: None,
            describe: "one continuous property per numeric column and frame, its values aligned on the frame's measured depths",
        },
        Output {
            role: "discrete-property",
            types: &["resqml22.DiscreteProperty"],
            least: 0,
            most: None,
            describe: "one discrete property per text column and frame (a code), or per frame pointing at a row of a gathered table",
        },
        Output {
            role: "category-lookup",
            types: &["eml23.ColumnBasedTable"],
            least: 0,
            most: None,
            describe: "the table giving each code of a text column its meaning (`separate` mode); a given table is continued, not duplicated",
        },
        Output {
            role: "gathered-table",
            types: &["eml23.ColumnBasedTable"],
            least: 0,
            most: None,
            describe: "the table gathering the text columns (`shared-table`, one for the run; `table-per-well`, one per frame) or the columns the template does not name (`everything`)",
        },
    ]
}

impl Operation for PropertiesFromTable {
    type Params = PropertiesParams;
    type Inputs = PropertiesInputs;

    fn gives(&self) -> Vec<Output> {
        outputs()
    }

    fn about(&self) -> About {
        About {
            name: "properties-from-table",
            describe: "the columns of a table laid on the wellbore frames of the package, aligned on measured depth and never shifted",
            category: Category::Wells,
            // No `CategoricalProperty`: RESQML 2.2 has none — a code's meaning
            // is `DiscreteProperty.CategoryLookup` — and the name reaches the
            // registry only through the `2.2_dev3` drop (`tests/makes.rs`).
            makes: &["resqml22.ContinuousProperty", "resqml22.DiscreteProperty"],
            recipe: "properties-from-table",
            represents: None,
        }
    }

    fn also(&self) -> &'static [Category] {
        &[Category::Properties]
    }

    fn suggest(&self, _bag: &Pieces, _slots: &mut Suggestions<'_>) {}

    /// What the run **reports**, and it is what makes it replayable.
    ///
    /// The crate's rule: *an activity that does not record its inputs is not
    /// a provenance, it is a receipt*. The registry already writes the settings
    /// that were set (`dataFile`, `nullValue`, `subdivide`); what it cannot
    /// know is **what the columns meant** and **which codes were
    /// assigned**. Without both, a second pass on new CSVs can
    /// neither propose the same mapping nor continue the same code tables.
    fn reports(&self) -> Vec<Slot> {
        vec![
            Slot::optional(
                "mapping",
                Need::Text,
                "what each column of the table was taken to mean, as `column → property kind`",
            ),
            Slot::optional("propertyCount", Need::Number { uom: None, least: None, most: None }, "properties written"),
            Slot::optional("wellCount", Need::Number { uom: None, least: None, most: None }, "wellbores enriched"),
        ]
    }

    fn check(&self, inputs: &PropertiesInputs) -> Result<(), Refusal> {
        if inputs.table.is_empty() {
            return Err(Refusal::on("table", "it takes a table; the input holds none"));
        }
        Ok(())
    }

    fn plan(&self, inputs: PropertiesInputs, params: PropertiesParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        // **Said here and not in `check`**: an empty `frames` is what an offers
        // list sees before the edge fills it (`read::every`), and
        // refusing there would make the operation unfit while it is being chosen. Empty
        // at plan time, it means the package holds none.
        if inputs.frames.is_empty() {
            return Err(Refusal::on(
                "frames",
                "it takes the wellbore frames to enrich; the input holds none, and the package gives none either. \
                 This operation writes no frame — `wells-from-table` creates them",
            ));
        }
        // **The frames, indexed by the name of the borehole.** The link between a
        // row of the file and a node of the package is that name, and nothing else:
        // it is the key, and the case of a heads file and a measurements file
        // is the simplest example of it.
        let known = Frames::of(&inputs.frames)?;
        lay(&inputs.table, &known, &inputs.lookups, &params, cx)
    }
}

/// The columns of the tables laid on **already known** frames — the body
/// of `plan`, without the reading of the slot.
///
/// `wells-and-properties-from-table` calls it with the frames that
/// `wells-from-table` has just planned: the crate's rule, *an operation
/// that writes the same object as another delegates to it*, through a typed function
/// and never through a derived bag.
pub(crate) fn lay(
    tables: &[TableIn],
    known: &Frames<'_>,
    lookups: &[ObjectIn],
    params: &PropertiesParams,
    cx: &Context<'_>,
) -> Result<Plan, Refusal> {

    let mut objects: Vec<NewObject> = Vec::new();
    let mut datasets: Vec<Dataset> = Vec::new();
    let mut says: Vec<Note> = Vec::new();
    let mut tally = Tally::default();
    let mut aside = SetAside::default();
    let mut rank = 0usize;
    // Which object awaits which table: the rank in `objects`, and the key.
    let mut waiting: Vec<(usize, (String, String))> = Vec::new();
    // **The codes of a text column are shared by the whole run.**
    //
    // The first attempt assigned them per borehole, and that was wrong in a way
    // that could not be seen: measured on the BRGM corpus, code 0 of `STRA`
    // designated **five** things depending on the borehole — `e7` for 75 of them,
    // `e6` for 35, then `n.d.`, `Q`, `e5`. Two boreholes of the same package
    // were therefore not comparable, and the day the `StringTableLookup` that gives
    // them their meaning is written, it would say only one.
    //
    // The key is **(kind, column name)** and not the kind alone: measured
    // too, the uuid is not unique per column — `TYP` and `STRA`
    // share theirs, and so do the three `Nom_*` — so the kind alone
    // would mix two vocabularies in one table.
    let mut codes: BTreeMap<(String, String), Categories> = BTreeMap::new();
    // The run's gathered tables (`shared-table`), keyed by what they gather,
    // and which pointing property waits for which table's uuid.
    let mut shared: BTreeMap<(Group, Vec<String>), Gathered> = BTreeMap::new();
    let mut waiting_shared: Vec<(usize, (Group, Vec<String>))> = Vec::new();
    let mut undeclared_count = 0usize;
    let source = tables.first().map(|one| one.file.clone()).unwrap_or_default();
    // **What exists is taken up before anything is written.** A
    // table given as input keeps its uuid and the order of its rows: a
    // code is the rank of its row, so reordering would make everything that
    // has already been written against it lie.
    for table in lookups {
        let Some(reference) = table.known.reference.as_ref() else { continue };
        let Some(values) = table.known.categories.clone() else {
            return Err(Refusal::on(
                "lookups",
                format!(
                    "`{}` does not read as a category table, so its rows cannot be continued — and continuing an order one has guessed would make every code already written mean something else",
                    reference.title
                ),
            ));
        };
        // **Keyed on the title alone**, because it is all a table
        // of the package says of itself: its column carries a title, and the kind
        // it declares is that of this column. The kind is added to
        // the key of *new* tables, where it tells apart two columns of the
        // same kind; here there is nothing to tell apart, the table exists.
        codes.insert(
            (String::new(), reference.title.clone()),
            Categories {
                order: values,
                lookup: reference.uuid.clone(),
                column: reference.title.clone(),
                kind: None,
                given: true,
            },
        );
    }

    for input in tables {
        let table = &input.table;
        // **Two roles only.** An enrichment table has neither
        // head nor ground: what it must say is *which borehole* and *at
        // which depth*. Requiring the complete structure would refuse a
        // perfectly readable file.
        let Some(name_at) = column_at(table, &input.template, "wellName")? else {
            return Err(Refusal::on(
                "table",
                format!(
                    "`{}` declares no well-name column, so nothing says which wellbore each row belongs to",
                    input.file
                ),
            ));
        };
        let Some(md_at) = column_at(table, &input.template, "measuredDepth")? else {
            return Err(Refusal::on(
                "table",
                format!(
                    "`{}` declares no measured-depth column, so it is a table of heads and carries nothing to \
                     lay along a frame. Its constants belong to `wells-from-table`",
                    input.file
                ),
            ));
        };
        // **Passes, when the template declares their top**: a row
        // holds over the interval `[top, MD]`, and the property is laid on the
        // intervals of the frame — one fewer than it has nodes.
        let top_at = column_at(table, &input.template, "topDepth")?;
        // The columns that become properties: those of the template
        // that the file carries and that are not a role.
        let carried = payload_columns(table, &input.template);
        // What no template column names — carried only with `everything`.
        let undeclared = undeclared_columns(table, &input.template);
        undeclared_count += undeclared.len();
        // **A property kind is required by the schema**, measured in the generated
        // type: `resqml22.ContinuousProperty::property_kind` is
        // not an `Option`. A column that the template attaches to no
        // kind would therefore write an object that `check` refuses, and the plan
        // does not validate — it describes. The refusal falls here, naming the
        // column, because it is the template that must be corrected.
        if let Some(mute) = carried.iter().find(|(column, _)| column.kind.is_none()) {
            return Err(Refusal::on(
                "table",
                format!(
                    "`{}`: the template names no property kind for `{}`, and RESQML makes a property state one. Add its `PropertyType` to the template, keyed on the kind's uuid",
                    input.file, mute.0.name
                ),
            ));
        }
        if carried.is_empty() {
            return Err(Refusal::on(
                "table",
                format!(
                    "`{}`: every column its template names is a structural role, so there is nothing to write \
                     as a property",
                    input.file
                ),
            ));
        }

        for hole in group(table, name_at, &mut aside) {
            let Some(frame) = known.find(&hole.title)? else {
                tally.unmatched.push(hole.title.clone());
                continue;
            };
            tally.holes += 1;
            let (rows, places) = match top_at {
                None => {
                    let rows = rows_by_depth(table, &hole, md_at, &mut aside)?;
                    let places = places_of(&rows, frame.mds, params.subdivide);
                    (rows, places)
                }
                Some(top) => {
                    let passes = intervals(table, &hole.rows, top, md_at, &hole.title)
                        .map_err(|why| Refusal::on("table", why))?;
                    let places = places_of_passes(&passes, frame.mds, params.subdivide);
                    if places.at.is_empty() {
                        return Err(Refusal::on(
                            "table",
                            format!(
                                "`{}`: its frame has {} node(s), so no interval to lay a pass on — a pass lies between two nodes",
                                hole.title,
                                frame.mds.len()
                            ),
                        ));
                    }
                    (passes.iter().map(|(_, base, row)| (*base, *row)).collect(), places)
                }
            };
            tally.added += places.added.len();
            tally.dropped += places.dropped;

            let gathering = params.text_columns != TextColumns::Separate;
            for column in &carried {
                // Gathered, a text column goes into the table below and not
                // into a property of its own; a continuous one is always its own.
                if gathering && !is_numeric(column.0) {
                    continue;
                }
                let made = one_column(
                    table,
                    column,
                    &rows,
                    &places,
                    frame,
                    &hole.title,
                    params,
                    &mut codes,
                    cx.identity,
                    cx.when,
                    &mut rank,
                )?;
                tally.filled += made.filled;
                tally.properties += 1;
                if let Some(key) = made.lookup {
                    waiting.push((objects.len(), key));
                }
                objects.extend(made.objects);
                datasets.extend(made.datasets);
            }
            // **The text columns, gathered** — and, with `everything`, the
            // columns no template names, in a table of their own: they carry no
            // property kind, and mixing them with declared ones would give a
            // declared column's neighbours a meaning nobody stated.
            let mut groups: Vec<(Group, Vec<GatheredColumn>)> = Vec::new();
            if gathering {
                let text: Vec<GatheredColumn> = carried
                    .iter()
                    .filter(|(column, _)| !is_numeric(column))
                    .map(|(column, at)| GatheredColumn::declared(column, *at))
                    .collect();
                if !text.is_empty() {
                    groups.push((Group::Declared, text));
                }
            }
            if params.everything && !undeclared.is_empty() {
                groups.push((Group::Undeclared, undeclared.clone()));
            }
            for (group, columns) in groups {
                let cells = gathered_cells(table, &columns, &places);
                let key = (group, columns.iter().map(|one| one.title.clone()).collect::<Vec<_>>());
                let title = format!("{} — {}", hole.title, group.words());
                let (values, lookup, lookup_title) = match params.text_columns {
                    TextColumns::TablePerWell => {
                        // One table for this frame: a row per element that
                        // carries a value, in the frame's order.
                        let mut rows: Vec<Vec<String>> = Vec::new();
                        let mut values: Vec<i64> = Vec::with_capacity(cells.len());
                        for cell in &cells {
                            match cell {
                                Some(row) => {
                                    values.push(rows.len() as i64);
                                    rows.push(row.clone());
                                }
                                None => {
                                    tally.filled += 1;
                                    values.push(params.null_value);
                                }
                            }
                        }
                        let joined = rows.iter().map(|row| row.join("\u{1f}")).collect::<Vec<_>>().join("\u{1e}");
                        let uuid = support::next_uuid(
                            cx.identity,
                            &format!("gathered-table:{}|{}|{}|{joined}", hole.title, group.words(), key.1.join("\u{1f}")),
                            &mut rank,
                        )?;
                        // Titled apart from the property that points at it:
                        // two objects of one title read as one in a listing.
                        let table_title = format!("{title} (table)");
                        objects.push(gathered_table(&uuid, &table_title, &columns, &rows, cx.when));
                        tally.tables += 1;
                        (values, uuid, table_title)
                    }
                    _ => {
                        // The run's table: a row per distinct combination, its
                        // uuid derived once every frame has contributed.
                        let shared = shared.entry(key.clone()).or_insert_with(|| Gathered {
                            title: format!("{source} — {}", group.words()),
                            columns: columns.clone(),
                            ..Gathered::default()
                        });
                        let mut values: Vec<i64> = Vec::with_capacity(cells.len());
                        for cell in &cells {
                            match cell {
                                Some(row) => values.push(shared.code(row)),
                                None => {
                                    tally.filled += 1;
                                    values.push(params.null_value);
                                }
                            }
                        }
                        (values, PATCH.to_string(), shared.title.clone())
                    }
                };
                let uuid = support::next_uuid(
                    cx.identity,
                    &seed_on(&hole.title, &format!("gathered:{}", group.words()), &integers(&values), places.on),
                    &mut rank,
                )?;
                let path = format!("/resqml22/{uuid}/values");
                if params.text_columns != TextColumns::TablePerWell {
                    waiting_shared.push((objects.len(), key.clone()));
                }
                objects.push(NewObject {
                    uuid: uuid.clone(),
                    qualified_type: "resqml22.DiscreteProperty".to_string(),
                    role: "discrete-property".to_string(),
                    title: title.clone(),
                    json: pointer(&uuid, &title, frame, places.on, &params.data_file, &path, values.len(), params.null_value, (&lookup, &lookup_title), cx.when),
                });
                datasets.push(Dataset {
                    file: params.data_file.clone(),
                    path,
                    dimensions: vec![values.len() as i64],
                    values: Numbers::I64(values),
                });
                tally.properties += 1;
                tally.gathered += 1;
            }
        }
    }

    // **The run's gathered tables, written last** — like the category tables
    // below, and for the same reason: a table emitted after the first frame
    // would hold its rows only. Their uuid comes from what they hold.
    for (key, one) in shared.iter_mut() {
        let joined = one.rows.iter().map(|row| row.join("\u{1f}")).collect::<Vec<_>>().join("\u{1e}");
        one.lookup = support::next_uuid(
            cx.identity,
            &format!("gathered-table:{}|{}|{joined}", key.0.words(), key.1.join("\u{1f}")),
            &mut rank,
        )?;
    }
    for (at, key) in &waiting_shared {
        if let Some(one) = shared.get(key)
            && let Some(object) = objects.get_mut(*at)
        {
            object.json["CategoryLookup"]["Uuid"] = json!(one.lookup);
        }
    }
    for one in shared.values() {
        objects.push(gathered_table(&one.lookup, &one.title, &one.columns, &one.rows, cx.when));
        tally.tables += 1;
    }
    if tally.gathered > 0 {
        says.push(Note {
            key: "gathered",
            detail: format!(
                "{} propert(ies) point at the rows of {} table(s) gathering the text columns ({}) — a value is the rank of a row there",
                tally.gathered,
                tally.tables,
                params.text_columns.word()
            ),
        });
    }
    if params.everything && undeclared_count > 0 {
        says.push(Note {
            key: "columns-carried",
            detail: format!(
                "{undeclared_count} column(s) of the file are not named by the template: carried into a table of their own, with the dictionary's `unknown` property kind"
            ),
        });
    }

    if tally.properties == 0 {
        let why = aside.0.iter().map(|(why, n)| format!("{n} × {why}")).collect::<Vec<_>>().join("; ");
        return Err(Refusal::on(
            "table",
            format!(
                "no property could be written: {} well(s) of the table name no frame of the package{}",
                tally.unmatched.len(),
                if why.is_empty() { String::new() } else { format!("; {why}") }
            ),
        ));
    }

    // **The tables are written at the end**, when all the boreholes have
    // contributed: a table emitted after the first borehole would carry only
    // its values, and the codes of the following ones would point outside it.
    // It is also the only moment when the uuid of a table can be derived,
    // since it comes from what it carries. The properties named it
    // by `PATCH` in the meantime, and it is here that they receive it:
    // a patch-up, said rather than hidden, and the marker is a
    // constant so that an omission reads as a dangling reference.
    for (key, table) in codes.iter_mut().filter(|(_, one)| !one.order.is_empty()) {
        if !table.given {
            let joined = table.order.join("\u{1f}");
            table.lookup = support::next_uuid(
                cx.identity,
                &format!("table-lookup:{}|{joined}", key.1),
                &mut rank,
            )?;
        }
    }
    for (at, key) in &waiting {
        if let Some(table) = codes.get(key)
            && let Some(object) = objects.get_mut(*at)
        {
            object.json["CategoryLookup"]["Uuid"] = json!(table.lookup);
        }
    }
    for table in codes.values().filter(|one| !one.order.is_empty()) {
        objects.push(lookup_table(table, cx.when));
    }
    if !codes.is_empty() {
        says.push(Note {
            key: "lookups",
            detail: format!(
                "{} category table(s) written: a discrete value is the rank of its row there",
                codes.len()
            ),
        });
    }

    says.push(Note {
        key: "properties",
        detail: format!("{} propert(ies) on {} wellbore(s)", tally.properties, tally.holes),
    });
    // **The filling is always said.** An array full of nulls and an
    // array full of measurements have exactly the same shape in the
    // file; only this count tells them apart.
    says.push(Note {
        key: "filled",
        detail: format!("{} element(s) had no value in the file and were written as null", tally.filled),
    });
    if tally.added > 0 {
        says.push(Note {
            key: "depths-added",
            detail: format!("{} measured depth(s) of the file were added to the frames", tally.added),
        });
    }
    if tally.dropped > 0 {
        says.push(Note {
            key: "depths-dropped",
            detail: format!(
                "{} row(s) state a measured depth no frame carries, and were left out — turn `subdivide` on to \
                 add them instead",
                tally.dropped
            ),
        });
    }
    if !tally.unmatched.is_empty() {
        let named = tally.unmatched.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
        says.push(Note {
            key: "wells-unmatched",
            detail: format!(
                "{} well(s) of the table name no frame of the package: {named}{}",
                tally.unmatched.len(),
                if tally.unmatched.len() > 3 { ", and more" } else { "" }
            ),
        });
    }
    for (why, count) in &aside.0 {
        says.push(Note { key: "rows-aside", detail: format!("{count} × {why}") });
    }

    // **The mapping, written out in full.** It is what a second
    // pass needs to take up and that the registry does not know: it
    // records only the settings.
    //
    // The **codes** are not in it, and that is deliberate: since a
    // `ColumnBasedTable` carries them, noting them here would be two homes for
    // one fact. The second, in addition, did not hold — measured on the
    // corpus, the list was 3,070 and 6,659 characters where
    // `Parameter.Value` allows 2,000, and `energyml check` refused
    // the activity.
    let mapping = tables
        .iter()
        .flat_map(|one| one.template.columns.iter())
        .filter_map(|column| column.kind.as_deref().map(|kind| format!("{} → {kind}", column.name)))
        .collect::<Vec<_>>()
        .join("; ");
    // `Parameter.Value` is bounded to 2,000 by `Activity.xsd`. A wide template
    // would exceed it, and a refused activity would no longer be read at
    // all: we cut **and say so**, rather than lose the whole object.
    let mapping = if mapping.chars().count() > 1_900 {
        let kept: String = mapping.chars().take(1_860).collect();
        format!("{kept}… (truncated: `Parameter.Value` allows 2000 characters)")
    } else {
        mapping
    };
    Ok(Plan {
        media: Vec::new(),
        blocked: None,
        objects,
        datasets,
        // **No shape.** This operation computes no geometry:
        // it lays values on nodes that already exist. Returning a
        // shape would make an artefact appear in the scene that is
        // only a copy of the frame.
        shapes: Vec::new(),
        activity: ActivityDraft {
            recipe: "properties-from-table",
            title: format!("properties-from-table → {source}"),
            seed: source,
            parameters: vec![
                Parameter::Text { title: "mapping".to_string(), value: mapping },
                Parameter::Double {
                    title: "propertyCount".to_string(),
                    value: tally.properties as f64,
                    uom: None,
                },
                Parameter::Double { title: "wellCount".to_string(), value: tally.holes as f64, uom: None },
            ],
        },
        says,
        dropped: aside.into_dropped(),
    })
}

// -- the frames of the package ----------------------------------------------

/// A frame of the package, with what is needed to lay values on it.
struct KnownFrame<'a> {
    reference: &'a crate::pieces::Ref,
    mds: &'a [f64],
}

/// The frames, findable by the name of the borehole.
pub(crate) struct Frames<'a> {
    known: Vec<(String, KnownFrame<'a>)>,
    /// Those whose depths cannot be read, by title.
    mute: Vec<String>,
}

impl<'a> Frames<'a> {
    /// **What is not a frame is refused by its type**, and a frame
    /// whose depths cannot be read is a fault only if a row
    /// of the file names it — see [`Frames::find`]. Since `frames` takes
    /// by default all those of the package, refusing one that no row
    /// names would block the import over a borehole that has nothing to do with it.
    fn of(frames: &'a [Shape]) -> Result<Frames<'a>, Refusal> {
        let frame = crate::operation::Takes::object(&["resqml22.WellboreFrameRepresentation"]);
        let mut known = Vec::new();
        let mut mute = Vec::new();
        let mut strangers = Vec::new();
        for one in frames {
            let Some(what) = one.known.as_ref() else {
                mute.push(one.name.clone());
                continue;
            };
            if let Some(other) = what.reference.as_ref().filter(|r| !frame.accepts(&r.qualified_type)) {
                strangers.push(format!("`{}` is `{}` ({})", one.name, other.qualified_type, other.title));
                continue;
            }
            match (what.reference.as_ref(), what.node_mds.as_deref()) {
                (Some(reference), Some(mds)) if !mds.is_empty() => {
                    known.push((reference.title.clone(), KnownFrame { reference, mds }));
                }
                (Some(reference), _) => mute.push(reference.title.clone()),
                (None, _) => mute.push(one.name.clone()),
            }
        }
        if !strangers.is_empty() {
            return Err(Refusal::on(
                "frames",
                format!(
                    "{} of the {} given are no wellbore frame, and values are laid on a frame's measured depths: {}. Give the `WellboreFrameRepresentation`s — or nothing, and every frame of the package is taken",
                    strangers.len(),
                    frames.len(),
                    strangers.join(", ")
                ),
            ));
        }
        Ok(Frames { known, mute })
    }

    /// **Given** frames, with their depths — those that a plan of
    /// `wells-from-table` has just written or recognised. None is
    /// mute: each carries the MDs that define it.
    pub(crate) fn given(frames: &'a [(crate::pieces::Ref, Vec<f64>)]) -> Frames<'a> {
        Frames {
            known: frames
                .iter()
                .map(|(reference, mds)| (reference.title.clone(), KnownFrame { reference, mds }))
                .collect(),
            mute: Vec::new(),
        }
    }

    /// The frame of a borehole — **exact first, then by prefix**.
    ///
    /// A writer titles its frame `“<borehole> — frame”`, and the file only
    /// carries `<borehole>`. The prefix catches that; ambiguity, for its part, is
    /// **refused** rather than settled on the first found — designating the wrong
    /// frame would lay each value on another borehole, which nothing on screen
    /// would show.
    fn find(&self, well: &str) -> Result<Option<&KnownFrame<'a>>, Refusal> {
        if let Some((_, exact)) = self.known.iter().find(|(title, _)| title == well) {
            return Ok(Some(exact));
        }
        // **A name followed by a letter or a digit is another name.**
        // Measured on the BRGM corpus: `CONVERGENCE-MELCHIOR-SP1` is a
        // prefix of `…-SP102` and of `…-SP103`, so a bare prefix returned
        // three candidates where there is only one. What separates a title from
        // its writer suffix — `“F1 — frame”`, `“F1 : trajectory”` —
        // is never alphanumeric.
        let named = |title: &str| {
            title.starts_with(well) && title[well.len()..].chars().next().is_none_or(|next| !next.is_alphanumeric())
        };
        let near: Vec<&(String, KnownFrame<'a>)> = self.known.iter().filter(|(title, _)| named(title)).collect();
        match near.as_slice() {
            // A mute frame that bears this name is the one thing that cannot
            // be kept quiet: the values of this borehole would go nowhere,
            // and the report would say *no frame* of a borehole that has one.
            [] => match self.mute.iter().find(|title| named(title)) {
                Some(title) => Err(Refusal::on(
                    "frames",
                    format!(
                        "`{well}` is `{title}` of the package, whose node depths cannot be read, so there is nothing to align its values on. A `WellboreFrameRepresentation` states them in `NodeMd`; a build with no array backend cannot read it"
                    ),
                )),
                None => Ok(None),
            },
            [one] => Ok(Some(&one.1)),
            many => Err(Refusal::on(
                "frames",
                format!(
                    "`{well}` names {} frames of the package — {} — and which one carries these values cannot be \
                     guessed",
                    many.len(),
                    many.iter().map(|(title, _)| title.as_str()).collect::<Vec<_>>().join(", ")
                ),
            )),
        }
    }
}

// -- aligner -----------------------------------------------------------------

/// The rows of a borehole, **sorted by depth**, duplicates refused.
///
/// A column is never sorted: **rows** are sorted, once, and everything
/// follows. Sorting an MD column while forgetting to apply the same
/// permutation to the values changes the depth of each measurement without anything
/// saying so.
fn rows_by_depth(
    table: &crate::tabular::Table,
    hole: &super::wells_from_table::Hole,
    md_at: usize,
    aside: &mut SetAside,
) -> Result<Vec<(f64, usize)>, Refusal> {
    let mut found: Vec<(f64, usize)> = Vec::new();
    for rank in &hole.rows {
        let cell = table.rows[*rank].get(md_at).map(|s| s.trim()).unwrap_or_default();
        if cell.is_empty() {
            aside.add("a row with no measured depth: nothing says where its values sit");
            continue;
        }
        let Some(value) = table.dialect.number(cell) else {
            return Err(Refusal::on(
                "table",
                format!(
                    "`{}`: line {} has `{cell}` where a measured depth is wanted, which is not a number",
                    hole.title,
                    rank + 1
                ),
            ));
        };
        found.push((value, *rank));
    }
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    for pair in found.windows(2) {
        if (pair[0].0 - pair[1].0).abs() <= f64::EPSILON {
            return Err(Refusal::on(
                "table",
                format!(
                    "`{}`: two rows state the measured depth {} (lines {} and {}) — a frame's nodes are distinct, \
                     and which of the two holds cannot be guessed",
                    hole.title,
                    pair[0].0,
                    pair[0].1 + 1,
                    pair[1].1 + 1
                ),
            ));
        }
    }
    Ok(found)
}

/// Where each node of the frame takes its value.
struct Places {
    /// For each element (in the order of the frame), the row of the file — or
    /// nothing. A node, or an interval between two nodes: see `on`.
    at: Vec<Option<usize>>,
    /// `nodes` or `intervals` — the `IndexableElement` that the property declares.
    on: &'static str,
    /// The MDs of the file added to the frame.
    added: Vec<f64>,
    /// The rows whose MD exists in no node.
    dropped: usize,
}

/// What the frame carries, and where each cell comes from.
///
/// Two settings, and the array is **always** the length of the
/// frame: it is the only thing that prevents the silent shift.
fn places_of(rows: &[(f64, usize)], mds: &[f64], subdivide: bool) -> Places {
    // A node and a row are recognised at the same depth, to within one
    // millionth of a metre. Comparing two floats for identity would make
    // the matching depend on the path each number took — the file
    // for one, the HDF5 for the other.
    let same = |a: f64, b: f64| (a - b).abs() <= 1e-6;
    let mut nodes: Vec<f64> = mds.to_vec();
    let mut added = Vec::new();
    if subdivide {
        for (depth, _) in rows {
            if !nodes.iter().any(|node| same(*node, *depth)) {
                nodes.push(*depth);
                added.push(*depth);
            }
        }
        nodes.sort_by(|a, b| a.total_cmp(b));
    }
    let mut at = vec![None; nodes.len()];
    let mut dropped = 0usize;
    for (depth, row) in rows {
        match nodes.iter().position(|node| same(*node, *depth)) {
            Some(index) => at[index] = Some(*row),
            None => dropped += 1,
        }
    }
    Places { at, on: "nodes", added, dropped }
}

/// Where each **interval** of the frame takes its value, for passes.
///
/// Interval *k* goes from node *k* to node *k + 1*. A pass covers the
/// intervals between the node of its top and that of its base — only one when the
/// frame was written from those passes, several when another source
/// cut it more finely, and the value of the pass then holds on each:
/// it is what a pass says, *from here to there*. A pass one of whose limits is not
/// a node is set aside and counted, except with `subdivide`, which adds its limits.
fn places_of_passes(passes: &[(f64, f64, usize)], mds: &[f64], subdivide: bool) -> Places {
    let same = |a: f64, b: f64| (a - b).abs() <= 1e-6;
    let mut nodes: Vec<f64> = mds.to_vec();
    let mut added = Vec::new();
    if subdivide {
        for (top, base, _) in passes {
            for depth in [*top, *base] {
                if !nodes.iter().any(|node| same(*node, depth)) {
                    nodes.push(depth);
                    added.push(depth);
                }
            }
        }
        nodes.sort_by(|a, b| a.total_cmp(b));
    }
    let mut at = vec![None; nodes.len().saturating_sub(1)];
    let mut dropped = 0usize;
    for (top, base, row) in passes {
        let (Some(from), Some(to)) = (
            nodes.iter().position(|node| same(*node, *top)),
            nodes.iter().position(|node| same(*node, *base)),
        ) else {
            dropped += 1;
            continue;
        };
        for slot in &mut at[from..to] {
            *slot = Some(*row);
        }
    }
    Places { at, on: "intervals", added, dropped }
}

// -- writing a column --------------------------------------------------------

/// The uuid that a table of categories will carry, as long as it is not known.
///
/// A property names its table **before** the table is complete — all the
/// boreholes must have contributed to know what it carries, and therefore
/// what its uuid is. This marker holds the place and `plan` replaces it. It is
/// a constant rather than a string written twice, because a
/// patch-up that fails reads as a dangling reference and not as a
/// oubli.
const PATCH: &str = "“table”";

/// The seed of a property: **what it is**, and nothing of the file.
///
/// Identical values, same uuid, hence a rewrite to the identical and nothing
/// lost; different values, different uuid, hence a second object beside it.
/// The name of the file is not in it — it identifies nothing of the borehole, and putting it there
/// made two exports of the same producer two sets of objects.
fn seed_of(well: &str, column: &str, values: &str) -> String {
    format!("table-property:{well}|{column}|{values}")
}

/// The seed, and **what the values index** when they are not nodes.
///
/// On nodes, the earlier seed, unchanged: properties already written
/// keep their uuid. On intervals, the word is added — the same
/// codes laid on passes are another object.
fn seed_on(well: &str, column: &str, values: &str, on: &str) -> String {
    if on == "nodes" { seed_of(well, column, values) } else { format!("{}|{on}", seed_of(well, column, values)) }
}

/// The values, as **exact** text.
///
/// `{:?}` and not `{}`: the first writing is the one that reads back the same
/// double, so two arrays that differ in the last bit give two different
/// seeds. A `NaN` is an absence, and it is written as such.
fn numbers(values: &[f64]) -> String {
    let mut out = String::with_capacity(values.len() * 8);
    for one in values {
        if one.is_nan() {
            out.push('·');
        } else {
            out.push_str(&format!("{one:?}"));
        }
        out.push(',');
    }
    out
}

/// The codes, as text — and **the labels will come with the table**, whose
/// uuid depends on them.
fn integers(values: &[i64]) -> String {
    let mut out = String::with_capacity(values.len() * 4);
    for one in values {
        out.push_str(&one.to_string());
        out.push(',');
    }
    out
}

/// What a column produced.
struct Written {
    objects: Vec<NewObject>,
    datasets: Vec<Dataset>,
    filled: usize,
    /// The table whose uuid this column awaits, when it is discrete.
    lookup: Option<(String, String)>,
}

/// The columns of the template that the file carries and that are not a role.
///
/// A role is a **structure** — the name of the borehole, its head, its depth —
/// and writing it as a property would repeat it at every node. Everything else is a
/// measurement, and that is what is laid.
fn payload_columns<'a>(
    table: &crate::tabular::Table,
    template: &'a crate::osdu::Template,
) -> Vec<(&'a Column, usize)> {
    let roles: Vec<&str> = super::wells_from_table::ROLES.iter().map(|one| one.uuid).collect();
    template
        .columns
        .iter()
        .filter(|column| !column.kind.as_deref().is_some_and(|uuid| roles.contains(&uuid)))
        .filter_map(|column| table.column(&column.name).map(|at| (column, at)))
        .collect()
}

/// Lay a column on a frame.
#[allow(clippy::too_many_arguments)]
fn one_column(
    table: &crate::tabular::Table,
    (column, at): &(&Column, usize),
    rows: &[(f64, usize)],
    places: &Places,
    frame: &KnownFrame<'_>,
    well: &str,
    params: &PropertiesParams,
    codes: &mut BTreeMap<(String, String), Categories>,
    identity: &Identity,
    when: &crate::operation::When,
    rank: &mut usize,
) -> Result<Written, Refusal> {
    let _ = rows;
    let title = format!("{well} — {}", column.name);
    let cell = |row: Option<usize>| -> Option<&str> {
        let value = table.rows[row?].get(*at)?.trim();
        // An empty cell **and** the template's absence value say the same
        // thing: the file says nothing here.
        if value.is_empty() || column.absent.as_deref() == Some(value) { None } else { Some(value) }
    };

    let mut filled = 0usize;
    let numeric = matches!(column.value_type, ValueType::Number | ValueType::Integer);
    // **The uuid comes from what the property carries**, so it can only be
    // derived after the computation. These two await the values.
    let uuid;
    let path;
    // The key of the code table, when there is one: read outside the block,
    // because the caller needs it for the patch-up.
    let mut key = (String::new(), String::new());
    let (object, dataset) = if numeric {
        let mut values: Vec<f64> = Vec::with_capacity(places.at.len());
        for place in &places.at {
            match cell(*place).and_then(|text| table.dialect.number(text)) {
                Some(number) => values.push(number),
                None => {
                    filled += 1;
                    values.push(f64::NAN);
                }
            }
        }
        uuid = support::next_uuid(identity, &seed_on(well, &column.name, &numbers(&values), places.on), rank)?;
        path = format!("/resqml22/{uuid}/values");
        (
            continuous(&uuid, &title, column, frame, places.on, &params.data_file, &path, values.len(), when),
            Dataset {
                file: params.data_file.clone(),
                path: path.clone(),
                dimensions: vec![values.len() as i64],
                values: Numbers::F64(values),
            },
        )
    } else {
        // **Text becomes a discrete property, and its code comes from the
        // table of the run**, never from a local counter: see `plan`.
        // A **given** table is recognised by the title of its column; otherwise
        // the key is (kind, column), which tells apart two columns of the same
        // kind — `TYP` and `STRA` share theirs.
        let given = (String::new(), column.name.clone());
        key = if codes.get(&given).is_some_and(|one| one.given) {
            given
        } else {
            (column.kind.clone().unwrap_or_default(), column.name.clone())
        };
        // **The uuid of the table also awaits knowing what it carries.**
        // Deriving it from the column alone would be worse than a duplicate: a second
        // run would rewrite it with *its* values, and the properties of the first —
        // which name it — would read their codes against a table that
        // lacks rows. It is therefore set at the end (`plan`), and the
        // properties receive it through a patch-up, for lack of being able to
        // know it before all the boreholes have contributed.
        let fresh = !codes.contains_key(&key);
        let table_of_codes = codes.entry(key.clone()).or_default();
        if fresh {
            table_of_codes.column = column.name.clone();
            table_of_codes.kind = column.kind.clone();
        }
        let mut values: Vec<i64> = Vec::with_capacity(places.at.len());
        for place in &places.at {
            match cell(*place) {
                Some(text) => values.push(table_of_codes.code(text)),
                None => {
                    filled += 1;
                    values.push(params.null_value);
                }
            }
        }
        uuid = support::next_uuid(identity, &seed_on(well, &column.name, &integers(&values), places.on), rank)?;
        path = format!("/resqml22/{uuid}/values");
        (
            discrete(&uuid, &title, column, frame, places.on, &params.data_file, &path, values.len(), params.null_value, PATCH, when),
            Dataset {
                file: params.data_file.clone(),
                path,
                dimensions: vec![values.len() as i64],
                values: Numbers::I64(values),
            },
        )
    };

    Ok(Written {
        lookup: (!numeric).then(|| key.clone()),
        objects: vec![NewObject {
            uuid,
            qualified_type: if numeric {
                "resqml22.ContinuousProperty".to_string()
            } else {
                "resqml22.DiscreteProperty".to_string()
            },
            role: if numeric { "continuous-property".to_string() } else { "discrete-property".to_string() },
            title,
            json: object,
        }],
        datasets: vec![dataset],
        filled,
    })
}

/// The common citation.
fn cite(title: &str, when: &crate::operation::When) -> Value {
    json!({
        "$type": "eml23.Citation",
        "Title": title,
        "Originator": when.originator,
        "Format": energyml_core::CITATION_FORMAT,
        "Creation": when.creation,
    })
}

/// The array of a property, in the form that `ops::property` writes.
///
/// **`ValuesForPatch`, not `Values`**, and `ValueCountPerIndexableElement` is
/// a *list*. Written from memory the first time, that gave 1,680 objects that
/// `energyml check` all refuses — `ValuesForPatch requires at least 1
/// occurrence, found 0` — and nothing in the plan could say so: a plan does not
/// validate, it describes. That is why the form is taken from the writer that
/// exists rather than reinvented.
fn values_for_patch(file: &str, path: &str, count: usize, float: bool, null_value: i64) -> Value {
    let array = support::external(file, path, &[count as i64]);
    if float {
        json!([{
            "$type": "eml23.FloatingPointExternalArray",
            "ArrayFloatingPointType": "arrayOfDouble64LE",
            "CountPerValue": 1,
            "Values": array,
        }])
    } else {
        json!([{
            "$type": "eml23.IntegerExternalArray",
            "ArrayIntegerType": "arrayOfInt64LE",
            "NullValue": null_value,
            "CountPerValue": 1,
            "Values": array,
        }])
    }
}

/// A continuous property on the nodes — or the intervals — of a frame.
#[allow(clippy::too_many_arguments)]
fn continuous(
    uuid: &str,
    title: &str,
    column: &Column,
    frame: &KnownFrame<'_>,
    on: &str,
    file: &str,
    path: &str,
    count: usize,
    when: &crate::operation::When,
) -> Value {
    let mut object = json!({
        "$type": "resqml22.ContinuousProperty",
        "uuid": uuid,
        "schemaVersion": "2.2",
        "Citation": cite(title, when),
        "IndexableElement": on,
        "ValueCountPerIndexableElement": [1],
        "SupportingRepresentation": frame.reference.dor(),
        "ValuesForPatch": values_for_patch(file, path, count, true, 0),
    });
    // **The uom comes from the template, never from here.** A column with no unit
    // declares one that nobody wrote if one is invented.
    if let Some(uom) = &column.uom {
        object["Uom"] = json!(uom);
    }
    if let Some(kind) = kind_reference(column) {
        object["PropertyKind"] = kind;
    }
    object
}

/// A discrete property, and the null it declares.
#[allow(clippy::too_many_arguments)]
fn discrete(
    uuid: &str,
    title: &str,
    column: &Column,
    frame: &KnownFrame<'_>,
    on: &str,
    file: &str,
    path: &str,
    count: usize,
    null_value: i64,
    lookup: &str,
    when: &crate::operation::When,
) -> Value {
    let mut object = json!({
        "$type": "resqml22.DiscreteProperty",
        "uuid": uuid,
        "schemaVersion": "2.2",
        "Citation": cite(title, when),
        "IndexableElement": on,
        "ValueCountPerIndexableElement": [1],
        "SupportingRepresentation": frame.reference.dor(),
        "ValuesForPatch": values_for_patch(file, path, count, false, null_value),
    });
    if let Some(kind) = kind_reference(column) {
        object["PropertyKind"] = kind;
    }
    // **A code only has meaning against its table**, and RESQML 2.2 says so through
    // `CategoryLookup` — a `DataObjectReference` that the schema does not type, and
    // that `csv-to-energyml` points at an `eml23.ColumnBasedTable` for
    // its 480 properties. That reading is the authoritative one here: the XSD says
    // only *a reference*.
    object["CategoryLookup"] = json!({
        "$type": "eml23.DataObjectReference",
        "Uuid": lookup,
        "QualifiedType": "eml23.ColumnBasedTable",
        "Title": column.name,
    });
    object
}

/// The table that gives the codes of a column their meaning.
///
/// **One row per distinct value, in the order of the codes**: the integer value
/// of a discrete property is the rank of its row here. The values
/// are **in the XML** and not in the HDF5: a text column has a few
/// dozen distinct values, and an external array for six strings
/// would cost a file to open to read what the document can say.
fn lookup_table(table: &Categories, when: &crate::operation::When) -> NewObject {
    let title = table.column.clone();
    let kind = json!({
        "$type": "eml23.DataObjectReference",
        "Uuid": table.kind.clone().unwrap_or_default(),
        "QualifiedType": "eml23.PropertyKind",
        "Title": title,
    });
    NewObject {
        uuid: table.lookup.clone(),
        qualified_type: "eml23.ColumnBasedTable".to_string(),
        role: "category-lookup".to_string(),
        title: title.clone(),
        json: json!({
            "$type": "eml23.ColumnBasedTable",
            "uuid": table.lookup,
            "schemaVersion": "2.3",
            "Citation": cite(&title, when),
            "Column": [{
                "Title": title,
                "ValueCountPerRow": 1,
                "PropertyKind": kind,
                "Values": {
                    "$type": "eml23.StringXmlArray",
                    "CountPerValue": 1,
                    "Values": table.order,
                },
            }],
        }),
    }
}

/// The property kind reference that the template names, if it names one.
fn kind_reference(column: &Column) -> Option<Value> {
    let uuid = column.kind.as_deref()?;
    Some(json!({
        "$type": "eml23.DataObjectReference",
        "Uuid": uuid,
        "QualifiedType": "eml23.PropertyKind",
        "Title": column.label.clone().unwrap_or_else(|| column.name.clone()),
    }))
}

/// The distinct values of a text column, and the code of each.
///
/// **The order of encounter, never the alphabet**: an alphabetical order would make
/// the codes depend on the dataset, so two packages of the same producer
/// would not have the same ones — and a code is what a `StringTableLookup`
/// traduira.
#[derive(Default)]
struct Categories {
    order: Vec<String>,
    /// The uuid of the `ColumnBasedTable` that will give them their meaning, derived once —
    /// **or that of the table being continued**, which does not change.
    lookup: String,
    /// What the table carries, to cite it and attach it to its kind.
    column: String,
    kind: Option<String>,
    /// Does it come from the package? A given table keeps its uuid and its order.
    given: bool,
}

impl Categories {
    /// The code of this value, assigned at first encounter.
    fn code(&mut self, text: &str) -> i64 {
        match self.order.iter().position(|one| one == text) {
            Some(at) => at as i64,
            None => {
                self.order.push(text.to_string());
                (self.order.len() - 1) as i64
            }
        }
    }
}

/// The counts of the run.
#[derive(Default)]
struct Tally {
    properties: usize,
    holes: usize,
    filled: usize,
    added: usize,
    dropped: usize,
    unmatched: Vec<String>,
    /// Pointing properties written for gathered tables.
    gathered: usize,
    /// Gathered tables written.
    tables: usize,
}

// -- the columns gathered into a table ---------------------------------------

/// Is this column a number, so a continuous property of its own in every mode?
fn is_numeric(column: &Column) -> bool {
    matches!(column.value_type, ValueType::Number | ValueType::Integer)
}

/// What a gathered table holds: the template's text columns, or the columns no
/// template names. Two tables, never one — see `lay`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Group {
    Declared,
    Undeclared,
}

impl Group {
    /// The words a title uses.
    fn words(self) -> &'static str {
        match self {
            Group::Declared => "text columns",
            Group::Undeclared => "columns not in the template",
        }
    }
}

/// A column of a gathered table: its title, the kind its rows mean, where the
/// file has it, and the value that means *nothing* there.
#[derive(Debug, Clone)]
struct GatheredColumn {
    title: String,
    kind: Value,
    at: usize,
    absent: Option<String>,
}

impl GatheredColumn {
    /// A column the template names, with the kind it states.
    fn declared(column: &Column, at: usize) -> Self {
        GatheredColumn {
            title: column.name.clone(),
            // Every text column reaching here has a kind: `lay` refuses one
            // without before any is written.
            kind: kind_reference(column).unwrap_or(Value::Null),
            at,
            absent: column.absent.clone(),
        }
    }
}

/// The columns of the file no template column names, in the file's order —
/// each with the dictionary's `unknown` kind.
fn undeclared_columns(table: &crate::tabular::Table, template: &crate::osdu::Template) -> Vec<GatheredColumn> {
    let named: Vec<String> = template.columns.iter().map(|one| one.name.trim().to_ascii_lowercase()).collect();
    table
        .headers
        .iter()
        .enumerate()
        .filter(|(_, header)| !header.trim().is_empty() && !named.contains(&header.trim().to_ascii_lowercase()))
        .map(|(at, header)| GatheredColumn {
            title: header.trim().to_string(),
            kind: json!({
                "$type": "eml23.DataObjectReference",
                "Uuid": UNKNOWN_KIND.0,
                "QualifiedType": "eml23.PropertyKind",
                "Title": UNKNOWN_KIND.1,
            }),
            at,
            absent: None,
        })
        .collect()
}

/// A row of the gathered table for each element of the frame — `None` where
/// every cell is empty, which is a null of the pointing property and no row.
fn gathered_cells(table: &crate::tabular::Table, columns: &[GatheredColumn], places: &Places) -> Vec<Option<Vec<String>>> {
    places
        .at
        .iter()
        .map(|place| {
            let row = (*place)?;
            let cells: Vec<String> = columns
                .iter()
                .map(|column| {
                    let value = table.rows[row].get(column.at).map(|cell| cell.trim()).unwrap_or("");
                    if column.absent.as_deref() == Some(value) { String::new() } else { value.to_string() }
                })
                .collect();
            cells.iter().any(|cell| !cell.is_empty()).then_some(cells)
        })
        .collect()
}

/// A gathered table under construction (`shared-table`): its rows are the
/// distinct combinations met, **in the order met** — the rule of
/// `Categories::code`, for the same reason.
#[derive(Default)]
struct Gathered {
    title: String,
    columns: Vec<GatheredColumn>,
    rows: Vec<Vec<String>>,
    index: std::collections::HashMap<Vec<String>, usize>,
    lookup: String,
}

impl Gathered {
    /// The rank of this combination's row, added at first meeting.
    fn code(&mut self, row: &[String]) -> i64 {
        if let Some(at) = self.index.get(row) {
            return *at as i64;
        }
        self.rows.push(row.to_vec());
        self.index.insert(row.to_vec(), self.rows.len() - 1);
        (self.rows.len() - 1) as i64
    }
}

/// A `ColumnBasedTable` gathering several columns: a `Column` each, with **its
/// own** property kind — what a table is for (`eml23.ColumnBasedTable`: *“the
/// values are arranged against columns that are defined by PropertyKind, UOM
/// and Facet”*). The values are in the XML, as `lookup_table` writes them.
fn gathered_table(uuid: &str, title: &str, columns: &[GatheredColumn], rows: &[Vec<String>], when: &crate::operation::When) -> NewObject {
    let column_values = |at: usize| -> Vec<String> { rows.iter().map(|row| row[at].clone()).collect() };
    let columns: Vec<Value> = columns
        .iter()
        .enumerate()
        .map(|(at, column)| {
            // `Column.Title` is capped at 64 by `ColumnBasedTable.xsd`.
            let title: String = column.title.chars().take(64).collect();
            json!({
                "Title": title,
                "ValueCountPerRow": 1,
                "PropertyKind": column.kind,
                "Values": {
                    "$type": "eml23.StringXmlArray",
                    "CountPerValue": 1,
                    "Values": column_values(at),
                },
            })
        })
        .collect();
    NewObject {
        uuid: uuid.to_string(),
        qualified_type: "eml23.ColumnBasedTable".to_string(),
        role: "gathered-table".to_string(),
        title: title.to_string(),
        json: json!({
            "$type": "eml23.ColumnBasedTable",
            "uuid": uuid,
            "schemaVersion": "2.3",
            "Citation": cite(title, when),
            "Column": columns,
        }),
    }
}

/// The discrete property whose value is the rank of a row of a gathered table:
/// kind `multi use property`, its meaning given by the table it names.
#[allow(clippy::too_many_arguments)]
fn pointer(
    uuid: &str,
    title: &str,
    frame: &KnownFrame<'_>,
    on: &str,
    file: &str,
    path: &str,
    count: usize,
    null_value: i64,
    (lookup, lookup_title): (&str, &str),
    when: &crate::operation::When,
) -> Value {
    json!({
        "$type": "resqml22.DiscreteProperty",
        "uuid": uuid,
        "schemaVersion": "2.2",
        "Citation": cite(title, when),
        "IndexableElement": on,
        "ValueCountPerIndexableElement": [1],
        "SupportingRepresentation": frame.reference.dor(),
        "ValuesForPatch": values_for_patch(file, path, count, false, null_value),
        "PropertyKind": {
            "$type": "eml23.DataObjectReference",
            "Uuid": POINTER_KIND.0,
            "QualifiedType": "eml23.PropertyKind",
            "Title": POINTER_KIND.1,
        },
        "CategoryLookup": {
            "$type": "eml23.DataObjectReference",
            "Uuid": lookup,
            "QualifiedType": "eml23.ColumnBasedTable",
            "Title": lookup_title,
        },
    })
}

/// The role of a word, for the tests.
pub fn role_of(word: &str) -> &'static str {
    role(word).title
}
