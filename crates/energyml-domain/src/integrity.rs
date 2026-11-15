//! The business rules that need **more than one object** to judge - the code
//! half of the rule set, beside the data half in [`crate::rules`].
//!
//! # Why code, when the rules are data
//!
//! [`crate::rules`] is the default and stays so: a rule the closed vocabulary
//! can say is written in JSON, never here. What lands here is what the
//! vocabulary cannot say, and each check below names the missing word:
//!
//! - **a group of objects**: *one definitive trajectory per wellbore* is a
//!   count over every trajectory of the package, not a property of one;
//! - **a membership kept elsewhere**: what a `DataobjectCollection` holds is
//!   written in a *third* object (`CollectionsToDataobjectsAssociationSet`),
//!   which no path from the collection reaches;
//! - **the values of an array**: *every value of a discrete property is a key
//!   of its table* reads two arrays, which the vocabulary never does
//!   (the rule vocabulary cannot reach array lengths or array contents);
//! - **a disjunction, or a set**: *IsListric is present iff the fault's only
//!   throw kind is normal* asks whether a list holds exactly one distinct
//!   value - `forEach` is a conjunction over the first element of each item.
//!
//! These are the anomalies found by the synthetic reference model audit
//! (ADR-0018). The contact-subject anomaly is a data rule
//! (`trg/contact-element-is-geological` in [`crate::rules::trg`]); the property
//! kind anomalies were already [`crate::property_kind::check`]'s.
//!
//! # The same findings, the same three outcomes
//!
//! Every check answers with a [`Finding`] of [`crate::rules`], so `check` files
//! them in one list and counts them one way: a violation weighs what its check
//! declares ([`Integrity::gravity`]), an uncheckable is neither - never a silent
//! pass. Every one was first a warning; the weights were then reviewed
//! against the `[ERROR]` model of Lutetia, and all
//! of them are errors but `house/time-series-asset-resolves`. None of the texts
//! cited below is an RFC 2119 *MUST* (the Energistics documents that use RFC
//! 2119 - the packaging conventions, ETP - write it in capitals; the reference
//! guides' business rules do not): the weight is that decision, not the
//! specification's.
//!
//! # House rules are named as such
//!
//! A `trg/` id cites a guide or a schema. A `house/` id is a reading this
//! repository chose where the specification is silent - a discrete value absent from its table, a series naming an unknown well - and
//! its documentation says so, and says where the decision is written.
//!
//! Nothing here opens a file: an [`ObjectSource`] lends the objects, an
//! [`ArraySource`] the arrays, and a check that needs arrays with none given
//! says so rather than passing.

use std::collections::{BTreeMap, BTreeSet};

use energyml_core::identity;
use energyml_core::value::{FieldValue, Reflect};

use crate::activity::{
    ActivityRefusal, ParameterIssue, TemplateRecord, conformance, descriptor_of, read_activity, read_template,
};
use crate::property;
use crate::property_kind::Resolver;
use crate::rules::{Finding, Gravity, Outcome, concepts_of};
use crate::source::{ArraySource, NoObjects, ObjectSource, Presence, object_chip};
use crate::table::{
    Cell, CellState, ColumnCells, Convention, LinkDeclaration, LinkReaders, LinkRefusal, LinkReport,
    LinkTarget, Lookup, NamingRefusal, Resolved, RowMatch, Table, TableRefusal, TableSet, TableSheet,
    TargetIndexes, check_links, resolve, resolve_kinds, row_named_by, uri_column,
};
use crate::values::{Family, Values};

/// One cross-object check: its id, what it means, and the code that judges.
pub struct Integrity {
    /// The id findings carry - `trg/…` for a rule of the standard, `house/…`
    /// for one of ours.
    pub id: &'static str,
    /// What the rule means, for the report.
    pub says: &'static str,
    /// How much a violation weighs - see [`Gravity`].
    pub gravity: Gravity,
    judge: fn(&Context<'_>, &Integrity, &mut Vec<Finding>),
}

/// What a check reads from: every object of the package, and where to fetch
/// what is not among them.
struct Context<'a> {
    objects: &'a [(&'a str, &'a dyn Reflect)],
    source: &'a dyn ObjectSource,
    arrays: Option<&'a dyn ArraySource>,
}

/// The shipped checks - the list `check` runs and counts.
///
/// A slice rather than a `match`, like every list here that will grow: a new
/// check is an entry and a function.
pub fn checks() -> &'static [Integrity] {
    CHECKS
}

const CHECKS: &[Integrity] = &[
    Integrity {
        id: "house/discrete-value-in-lookup",
        says: "every value of a property with a category lookup names a row of its table, by its key column or by rank from 0 when it has none (the reading is ours: common TRG 2.3 §9.1 says a KeyColumn makes the table a lookup table, not how a value names a row)",
        gravity: Gravity::Error,
        judge: discrete_value_in_lookup,
    },
    Integrity {
        id: "house/table/link-resolves",
        says: "every non-empty value of a link column names a row of its target table, by its one key column or by rank when it has none (the convention is ours: common TRG 2.3 §9.1 p. 190 has no column naming another table; the declaration is a qualifier facet, §11.11 p. 294)",
        gravity: Gravity::Error,
        judge: table_link_resolves,
    },
    Integrity {
        id: "house/table/link-target-key-unique",
        says: "a key a link names is held by one row of its target (nothing in common TRG 2.3 §9.1 p. 190 requires keys to be unique; the first row is used and the ambiguity reported)",
        gravity: Gravity::Warning,
        judge: table_link_target_key_unique,
    },
    Integrity {
        id: "house/table/link-declared-by-facet",
        says: "a link column is declared by one qualifier facet holding the target table's uri (common TRG 2.3 §11.11 p. 294, §11.3 p. 285); a description or a title is read and reported",
        gravity: Gravity::Warning,
        judge: table_link_declared_by_facet,
    },
    Integrity {
        id: "house/table/uri-value-resolves",
        says: "a uri written as a value names an object of the package (Identifier Specification v4.0 §6.3 p. 14); another dataspace is not judged",
        gravity: Gravity::Warning,
        judge: table_uri_value_resolves,
    },
    Integrity {
        id: "house/time-series-asset-resolves",
        says: "a time series' `asset identifier` names a well this package holds (the convention is ours: TimeSeriesData.xsd l. 67 makes the key text, not a reference; the URI grammar is the Energistics Identifier Specification v4.0 §6.3 p. 14)",
        gravity: Gravity::Warning,
        judge: time_series_asset_resolves,
    },
    Integrity {
        id: "trg/one-definitive-trajectory-per-wellbore",
        says: "a wellbore has at most one definitive trajectory (WITSML TRG 2.1 §16.6 p. 350, on Trajectory.Definitive: \"There can only be one trajectory per wellbore with definitive=true\")",
        gravity: Gravity::Error,
        judge: one_definitive_trajectory_per_wellbore,
    },
    Integrity {
        id: "trg/is-listric-iff-normal",
        says: "IsListric is present if and only if the fault is normal (RESQML Interpretations.xsd l. 659-662, TRG 2.2 §6.20 p. 188: \"Must be present if the fault is normal. Must not be present if the fault is not normal.\"; a fault is normal when normal is its only throw kind)",
        gravity: Gravity::Error,
        judge: is_listric_iff_normal,
    },
    Integrity {
        id: "trg/homogeneous-collection",
        says: "a collection declaring HomogeneousDatatype true holds data objects of one type only (EML 2.3 Collection.xsd l. 66-70: \"If true all data objects in the collection are of the same Energistics data type\")",
        gravity: Gravity::Error,
        judge: homogeneous_collection,
    },
    Integrity {
        id: "trg/homogeneous-association",
        says: "a collection association declaring HomogeneousDatatype true lists data objects of one type only (EML 2.3 Collection.xsd l. 82-86, the same sentence on SingleCollectionAssociation)",
        gravity: Gravity::Error,
        judge: homogeneous_association,
    },
    Integrity {
        id: "trg/wellbore-interpretation-set-holds-wellbores",
        says: "an earth model's WellboreInterpretationSet holds WellboreInterpretations only (RESQML TRG 2.2 §6.19 p. 187: \"the DataobjectCollection must only contain WellboreInterpretation\")",
        gravity: Gravity::Error,
        judge: wellbore_interpretation_set_holds_wellbores,
    },
    Integrity {
        id: "house/property/directional-ratio",
        says: "two properties of one kind on one support, told apart only by a vertical and a lateral direction facet, keep the vertical value at least 0.95 times the lateral one in 95 % of their pairs, for a resistivity (the reading is ours: common TRG 2.3 §11.1 p. 281 names the directions, not the order of their values; ADR-0036)",
        gravity: Gravity::Warning,
        judge: directional_ratio,
    },
    Integrity {
        id: "house/provenance/activity-follows-its-template",
        says: "every parameter of an activity has an equivalent in the template its ActivityDescriptor names, of an allowed kind and in the allowed numbers (common TRG 2.3 §3.1 p. 40: \"Must have an equivalent in the activity descriptor parameters\"; §3.10 p. 49: an empty AllowedKind allows all, a MaxOccurs of -1 is unbounded; reading a data object that names an activity as a sub-activity is ours, ADR-0035)",
        gravity: Gravity::Warning,
        judge: activity_follows_its_template,
    },
];

/// Run every check over a package's objects.
///
/// `objects` is every object that read, with its uuid; `source` lends what a
/// reference names; `arrays`, when given, reads the arrays - without it, the
/// one check that needs values says *uncheckable* for each object it would
/// have judged.
pub fn run(
    objects: &[(&str, &dyn Reflect)],
    source: &dyn ObjectSource,
    arrays: Option<&dyn ArraySource>,
) -> Vec<Finding> {
    let context = Context {
        objects,
        source,
        arrays,
    };
    let mut out = Vec::new();
    for check in CHECKS {
        (check.judge)(&context, check, &mut out);
    }
    out
}

// ---------------------------------------------------------------------------
// A discrete value absent from its table
// ---------------------------------------------------------------------------

/// **House rule**, and the reading is written down: the value is looked up in the
/// key column, or by its rank from 0 when the table has none - because neither
/// the common TRG 2.3 §9.1-9.2 (p. 190-191) nor the RESQML TRG 2.2 §7.7
/// (p. 230) says how a value designates a row, beyond *"Defines a key column
/// which make the table a lookup table"*. The one door is
/// [`crate::table::row_named_by`] (`house/table/row-rank-without-key`,
/// ADR-0030); a value that names no row is a value no legend can name.
///
/// A value equal to the array's own `NullValue` is not a value, and `NaN` is
/// how floating arrays say the same thing.
fn discrete_value_in_lookup(context: &Context<'_>, check: &Integrity, out: &mut Vec<Finding>) {
    'objects: for (uuid, object) in context.objects {
        if !concepts_of(*object).contains(&"AbstractProperty") {
            continue;
        }
        let Ok(read) = property::read(*object) else {
            continue;
        };
        let Some(table_uuid) = read.lookup.as_deref() else {
            continue;
        };
        let cant = |why: String| unverifiable(check, uuid, why);
        let Some(arrays) = context.arrays else {
            out.push(cant(
                "its values are an array, and this check was given nothing to read arrays with".into(),
            ));
            continue;
        };
        // By the uri the property names: the version it names is the table read.
        let found = match read.lookup_uri.as_deref() {
            Some(uri) => context.source.object_at(uri),
            None => context.source.object(table_uuid),
        };
        let Some(table) = found else {
            out.push(cant(format!("its lookup `{table_uuid}` is not in this package")));
            continue;
        };
        let lookup = match Lookup::read(table.as_reflect(), arrays) {
            Ok(lookup) => lookup,
            Err(why) => {
                out.push(cant(format!("its lookup `{table_uuid}` does not read: {why}")));
                continue;
            }
        };
        // The index is built once per table, and `row_named_by` is the one door.
        let index = lookup.key_index();
        if let Err(NamingRefusal::SeveralKeyColumns { count }) = row_named_by(&index, &Cell::Number(0.0)) {
            out.push(cant(format!(
                "its lookup `{table_uuid}` has {count} key column(s), and a value can name a row by one"
            )));
            continue;
        }
        // A keyed table is read by its key; the checks on the key column apply to it only.
        let key = index.key_columns().first().copied();
        if let Some(key) = key {
            match lookup.rows.columns.get(key) {
                Some(Some(Values::Text(_))) => {
                    out.push(cant(format!(
                        "the key column of `{table_uuid}` holds text, and the property's values are numbers"
                    )));
                    continue;
                }
                Some(Some(_)) => {}
                _ => {
                    out.push(cant(format!(
                        "the key column of `{table_uuid}` does not read: {}",
                        lookup.notes.join("; ")
                    )));
                    continue;
                }
            }
        }
        let key_title = key.and_then(|key| lookup.titles.get(key).cloned().flatten());

        // Value → how many elements carry it, in order of first appearance.
        let mut missing: Vec<(f64, usize)> = Vec::new();
        for patch in &read.patches {
            let values = match patch.read(arrays) {
                Ok(values) => values,
                Err(why) => {
                    out.push(cant(format!("patch {} does not read: {why}", patch.position)));
                    continue 'objects;
                }
            };
            let Some(numbers) = values.to_f64() else {
                out.push(cant(format!("patch {} does not hold numbers", patch.position)));
                continue 'objects;
            };
            let null = crate::values::null_value(patch.array).map(|n| n as f64);
            for value in numbers {
                if value.is_nan() || Some(value) == null {
                    continue;
                }
                if matches!(
                    row_named_by(&index, &Cell::Number(value)),
                    Ok(RowMatch::One(_) | RowMatch::Several(_))
                ) {
                    continue;
                }
                match missing.iter_mut().find(|(known, _)| *known == value) {
                    Some((_, count)) => *count += 1,
                    None => missing.push((value, 1)),
                }
            }
        }
        if !missing.is_empty() {
            let listed: Vec<String> = missing
                .iter()
                .map(|(value, count)| format!("{value} ({count} element(s))"))
                .collect();
            let read = if key.is_some() {
                format!(
                    "{} not in the key column{} of `{table_uuid}`",
                    listed.join(", "),
                    key_title.map(|title| format!(" `{title}`")).unwrap_or_default()
                )
            } else {
                format!(
                    "{} not a row of `{table_uuid}` ({} row(s), named by rank from 0: house/table/row-rank-without-key)",
                    listed.join(", "),
                    index.row_count()
                )
            };
            out.push(violation(check, uuid, read));
        }
    }
}

// ---------------------------------------------------------------------------
// Links between tables, and uris written as values
// ---------------------------------------------------------------------------

/// The object types a link column can sit in: the ones the table checks judge.
const TABLE_TYPE: &str = "ColumnBasedTable";

/// A table read for the link checks: its sheet, and what each of its columns
/// declares. Nothing here reads an array.
struct LinkedTable {
    sheet: TableSheet,
    declarations: Vec<(usize, std::result::Result<LinkDeclaration, LinkRefusal>)>,
}

/// The sheet of a table object and the declarations of its columns: the kinds
/// are resolved through the package then the dictionary, and `LinkReaders`
/// reads the conventions (ADR-0030). `None` when the table does not read: that
/// is the business of `Table::read`, not of a link check.
fn table_context(object: &dyn Reflect, source: &dyn ObjectSource) -> Option<LinkedTable> {
    let sheet = TableSheet::of(object).ok()?;
    let resolver = Resolver::new(source);
    let kinds = resolve_kinds(&sheet, &resolver);
    let declarations = LinkReaders::builtin().declarations(&sheet, &kinds);
    Some(LinkedTable { sheet, declarations })
}

/// The title of a column, or its rank when it has none.
fn column_name(sheet: &TableSheet, column: usize) -> String {
    sheet
        .columns
        .get(column)
        .and_then(|c| c.title.as_deref())
        .map(str::to_owned)
        .unwrap_or_else(|| column.to_string())
}

/// Rows as a list, ten at most, then how many more.
fn rows_listed(rows: &[usize]) -> String {
    const SHOWN: usize = 10;
    let mut text = rows
        .iter()
        .take(SHOWN)
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    if rows.len() > SHOWN {
        text.push_str(&format!(" and {} more", rows.len() - SHOWN));
    }
    text
}

/// The value of a cell as text, when it holds a value.
fn value_at(
    cells: &[std::result::Result<ColumnCells<'static>, TableRefusal>],
    column: usize,
    row: usize,
) -> Option<String> {
    match cells.get(column)?.as_ref().ok()?.cell(row)? {
        CellState::Value(cell) => Some(cell.to_string()),
        _ => None,
    }
}

/// What the links of one table were checked against.
struct LinksChecked {
    report: LinkReport,
    cells: Vec<std::result::Result<ColumnCells<'static>, TableRefusal>>,
    targets: TableSet,
    /// The declared targets that could not be read, with why.
    unread: Vec<(String, String)>,
}

/// Read the table, its targets, and run `check_links`. `Err` is the reason the
/// table could not be checked: an *uncheckable*, never a pass.
fn check_table_links(
    context: &Context<'_>,
    object: &dyn Reflect,
    links: &LinkedTable,
    arrays: &dyn ArraySource,
) -> std::result::Result<LinksChecked, String> {
    let table = Table::read(object).map_err(|why| format!("it does not read: {why}"))?;
    let cells = table.read_cells(arrays);

    let mut declared: Vec<String> = Vec::new();
    let mut by_title = false;
    for (_, declaration) in &links.declarations {
        let Ok(declaration) = declaration else { continue };
        match &declaration.target {
            LinkTarget::Key { table_uri } | LinkTarget::Rank { table_uri } => {
                if !declared.contains(table_uri) {
                    declared.push(table_uri.clone());
                }
            }
            LinkTarget::ByTitleIn { .. } => by_title = true,
            _ => {}
        }
    }
    // A title names a table of the package: every other table is a candidate.
    let mut uris = declared.clone();
    if by_title {
        for (_, other) in context.objects {
            if other.type_info().type_key != TABLE_TYPE {
                continue;
            }
            if let Some(uri) = identity::uri_of_reflect(*other)
                && uri.as_str() != &*links.sheet.uri
                && !uris.contains(&uri)
            {
                uris.push(uri);
            }
        }
    }
    let wanted: Vec<&str> = uris.iter().map(String::as_str).collect();
    let (mut targets, unread) = TableSet::read(&wanted, arrays);
    // The table itself: a self reference lands in it.
    if let Ok(lookup) = Lookup::read(object, arrays) {
        targets.insert(
            &links.sheet.uri,
            links.sheet.title.as_ref().map(|title| title.to_string()),
            lookup,
        );
    }
    let unread = unread
        .into_iter()
        .filter(|(uri, _)| declared.contains(uri))
        .collect();
    let report = check_links(&links.sheet, &cells, &links.declarations, &targets, None);
    Ok(LinksChecked {
        report,
        cells,
        targets,
        unread,
    })
}

/// The tables of the package that declare at least one link they can follow.
fn tables_with_links<'a>(
    context: &'a Context<'a>,
) -> impl Iterator<Item = (&'a str, &'a dyn Reflect, LinkedTable)> {
    context.objects.iter().filter_map(move |(uuid, object)| {
        if object.type_info().type_key != TABLE_TYPE {
            return None;
        }
        let links = table_context(*object, context.source)?;
        if links
            .declarations
            .iter()
            .all(|(_, declaration)| declaration.is_err())
        {
            return None;
        }
        Some((*uuid, *object, links))
    })
}

/// **House rule** (ADR-0030). A link column names a row of its target table by
/// its one key column, or by rank from 0 when the target has none; a non-empty
/// value that names no row is an error, with its rows. An empty string names no
/// row. A target that is not in the package, or a declaration that cannot be
/// followed, makes the table *uncheckable*, never a pass.
fn table_link_resolves(context: &Context<'_>, check: &Integrity, out: &mut Vec<Finding>) {
    for (uuid, object, links) in tables_with_links(context) {
        let Some(arrays) = context.arrays else {
            out.push(unverifiable(
                check,
                uuid,
                "its link columns are values, and this check was given nothing to read arrays with".into(),
            ));
            continue;
        };
        let checked = match check_table_links(context, object, &links, arrays) {
            Ok(checked) => checked,
            Err(why) => {
                out.push(unverifiable(check, uuid, why));
                continue;
            }
        };
        for (uri, _) in &checked.unread {
            out.push(unverifiable(
                check,
                uuid,
                format!("its target `{uri}` is not in this package"),
            ));
        }
        let mut dangling: Vec<String> = Vec::new();
        for column in &checked.report.columns {
            if !column.dangling.is_empty() {
                let values: Vec<String> = column
                    .dangling
                    .iter()
                    .take(10)
                    .filter_map(|row| value_at(&checked.cells, column.column, *row))
                    .map(|value| format!("`{value}`"))
                    .collect();
                dangling.push(format!(
                    "column `{}`: row(s) {} (values {})",
                    column_name(&links.sheet, column.column),
                    rows_listed(&column.dangling),
                    values.join(", ")
                ));
            }
            // A target that is not held is said above, once.
            let refused: Vec<&(usize, LinkRefusal)> = column
                .unresolvable
                .iter()
                .filter(|(_, why)| !matches!(why, LinkRefusal::TargetUnknown { .. }))
                .collect();
            if let Some((_, why)) = refused.first() {
                let rows: Vec<usize> = refused.iter().map(|(row, _)| *row).collect();
                out.push(unverifiable(
                    check,
                    uuid,
                    format!(
                        "column `{}`: row(s) {} cannot be followed: {why}",
                        column_name(&links.sheet, column.column),
                        rows_listed(&rows)
                    ),
                ));
            }
        }
        for (column, why) in &checked.report.refused {
            if !matches!(why, LinkRefusal::TargetUnknown { .. }) {
                out.push(unverifiable(
                    check,
                    uuid,
                    format!("column `{}`: {why}", column_name(&links.sheet, *column)),
                ));
            }
        }
        if !dangling.is_empty() {
            out.push(violation(check, uuid, dangling.join("; ")));
        }
    }
}

/// **House rule** (ADR-0030). Nothing in the specification requires keys to be
/// unique: when the target holds the key a link names on several rows, the
/// first row is used and the link is reported as ambiguous.
fn table_link_target_key_unique(context: &Context<'_>, check: &Integrity, out: &mut Vec<Finding>) {
    for (uuid, object, links) in tables_with_links(context) {
        let Some(arrays) = context.arrays else {
            out.push(unverifiable(
                check,
                uuid,
                "its link columns are values, and this check was given nothing to read arrays with".into(),
            ));
            continue;
        };
        let checked = match check_table_links(context, object, &links, arrays) {
            Ok(checked) => checked,
            Err(why) => {
                out.push(unverifiable(check, uuid, why));
                continue;
            }
        };
        let mut ambiguous: Vec<String> = Vec::new();
        for column in &checked.report.columns {
            let Some(Ok(declaration)) = links
                .declarations
                .iter()
                .find(|(at, _)| *at == column.column)
                .map(|(_, declaration)| declaration)
            else {
                continue;
            };
            // Which keys, held by which rows of which table: the same resolution
            // `check_links` made, asked again for the rows it flagged.
            let mut named: Vec<(String, String, Vec<usize>)> = Vec::new();
            for row in &column.ambiguous {
                let Some(state) = checked
                    .cells
                    .get(column.column)
                    .and_then(|c| c.as_ref().ok())
                    .and_then(|c| c.cell(*row))
                else {
                    continue;
                };
                let sibling = match &declaration.target {
                    LinkTarget::ByTitleIn { column: sibling } => checked
                        .cells
                        .get(*sibling)
                        .and_then(|c| c.as_ref().ok())
                        .and_then(|c| c.cell(*row)),
                    _ => None,
                };
                let Resolved::Ambiguous { table_uri, rows } = resolve(
                    declaration,
                    &state,
                    sibling.as_ref(),
                    &links.sheet.uri,
                    &checked.targets,
                ) else {
                    continue;
                };
                let CellState::Value(key) = &state else { continue };
                let key = key.to_string();
                let target = checked
                    .targets
                    .target(&table_uri)
                    .and_then(|t| t.title.clone())
                    .unwrap_or(table_uri);
                if !named.iter().any(|(k, t, _)| *k == key && *t == target) {
                    named.push((key, target, rows));
                }
            }
            for (key, target, rows) in named {
                ambiguous.push(format!(
                    "column `{}`: key(s) `{key}` held by rows {} of `{target}`",
                    column_name(&links.sheet, column.column),
                    rows_listed(&rows)
                ));
            }
        }
        if !ambiguous.is_empty() {
            out.push(violation(check, uuid, ambiguous.join("; ")));
        }
    }
}

/// **House rule** (ADR-0030, convention B). A link column is declared by one
/// `qualifier` facet holding the complete uri of the target table. A
/// declaration in the description (A), or a target named by title in a sibling
/// column (D), is read and reported; so is a declaration that is refused.
/// Nothing here reads an array.
fn table_link_declared_by_facet(context: &Context<'_>, check: &Integrity, out: &mut Vec<Finding>) {
    for (uuid, object) in context.objects {
        if object.type_info().type_key != TABLE_TYPE {
            continue;
        }
        let Some(links) = table_context(*object, context.source) else {
            continue;
        };
        let mut read: Vec<String> = Vec::new();
        for (column, declaration) in &links.declarations {
            let name = column_name(&links.sheet, *column);
            match declaration {
                Ok(declaration) if declaration.convention == Convention::DescriptionText => {
                    read.push(format!("column `{name}` (description)"));
                }
                Ok(declaration) if declaration.convention == Convention::SiblingColumn => {
                    read.push(format!("column `{name}` (title in a sibling column)"));
                }
                Ok(_) => {}
                Err(why) => read.push(format!("column `{name}`: {why}")),
            }
        }
        if !read.is_empty() {
            out.push(violation(check, uuid, read.join("; ")));
        }
    }
}

/// **House rule** (ADR-0030, Identifier Specification v4.0 §6.3 p. 14). In a
/// column whose every non-empty value is a complete ETP uri, each uri names an
/// object the package holds. A uri of another dataspace is not judged: an
/// object may legitimately live elsewhere. A column in an array file nobody
/// gave a reader for is *uncheckable*, named.
fn table_uri_value_resolves(context: &Context<'_>, check: &Integrity, out: &mut Vec<Finding>) {
    let nothing = NoObjects;
    let arrays: &dyn ArraySource = match context.arrays {
        Some(arrays) => arrays,
        None => &nothing,
    };
    for (uuid, object) in context.objects {
        if object.type_info().type_key != TABLE_TYPE {
            continue;
        }
        let Some(links) = table_context(*object, context.source) else {
            continue;
        };
        let Ok(table) = Table::read(*object) else {
            continue;
        };
        let mut missing: Vec<String> = Vec::new();
        for (column, cells) in table.read_cells(arrays).into_iter().enumerate() {
            let name = column_name(&links.sheet, column);
            let cells = match cells {
                Ok(cells) => cells,
                Err(why) => {
                    // Only a text column can hold uris: another one, unread, is not this rule's.
                    let text = links.sheet.columns.get(column).and_then(|c| c.family) == Some(Family::Text);
                    if text {
                        out.push(unverifiable(
                            check,
                            uuid,
                            format!("column `{name}` cannot be read: {why}"),
                        ));
                    }
                    continue;
                }
            };
            // A column that declares a link is the link rules' business.
            if links.declarations.iter().any(|(at, _)| *at == column) || uri_column(&cells).is_none() {
                continue;
            }
            let mut rows: Vec<usize> = Vec::new();
            let mut uris: Vec<String> = Vec::new();
            let mut row = 0;
            while let Some(state) = cells.cell(row) {
                if let CellState::Value(Cell::Text(value)) = state
                    && let Ok(chip) = object_chip(&value, None, context.source)
                    && chip.presence == Presence::NotInPackage
                {
                    rows.push(row);
                    uris.push(format!("`{}`", chip.uri));
                }
                row += 1;
            }
            if !rows.is_empty() {
                missing.push(format!(
                    "column `{name}`: row(s) {} ({}) name(s) no object of this package",
                    rows_listed(&rows),
                    uris.iter().take(10).cloned().collect::<Vec<_>>().join(", ")
                ));
            }
        }
        if !missing.is_empty() {
            out.push(violation(check, uuid, missing.join("; ")));
        }
    }
}

// ---------------------------------------------------------------------------
// A series naming a well the package does not hold
// ---------------------------------------------------------------------------

/// **House rule.** A PRODML `Key` is
/// text - TimeSeriesData.xsd l. 67 makes it no reference - so nothing in the
/// schema ties a series to a well; the house convention does, with an
/// Energistics URI (Identifier Specification v4.0 §6.3 p. 14: `path-part =
/// component-type [ "(" identifier ")" ]`, and §6.3.1: *"The actual component
/// name or ID of the component is contained within parenthesis"*).
///
/// So the `well(…)` part of an `asset identifier` is looked for among the
/// package's WITSML wells, by the **name** WITSML 2 gives a well: its
/// `Citation.Title`, or its `NameLegal`. A name is not an identity - this
/// repository's rule - so two wells answering to
/// the name make the series **uncheckable**, never matched to the first.
///
/// What is left alone, and why: a URI with no `well(…)` part names something
/// else (a facility, a tag) and is not this rule's business; the `wellbore(…)`
/// part is not resolved, the convention stating no field its identifier
/// matches.
fn time_series_asset_resolves(context: &Context<'_>, check: &Integrity, out: &mut Vec<Finding>) {
    let wells: Vec<(&str, Vec<String>)> = context
        .objects
        .iter()
        .filter(|(_, object)| object.type_info().type_key == "Well")
        .map(|(uuid, object)| (*uuid, names_of(*object)))
        .collect();

    for (uuid, object) in context.objects {
        if !matches!(
            object.type_info().type_key,
            "TimeSeriesData" | "TimeSeriesStatistic"
        ) {
            continue;
        }
        for key in objects(*object, "Key") {
            if text(key, "keyword").as_deref() != Some("asset identifier") {
                continue;
            }
            let Some(uri) = text(key, "value") else {
                continue;
            };
            let name = match well_named_by(&uri) {
                Ok(Some(name)) => name,
                Ok(None) => continue,
                Err(why) => {
                    out.push(unverifiable(check, uuid, format!("`{uri}`: {why}")));
                    continue;
                }
            };
            let found: Vec<&str> = wells
                .iter()
                .filter(|(_, names)| names.contains(&name))
                .map(|(well, _)| *well)
                .collect();
            match found.as_slice() {
                [_] => {}
                [] => out.push(violation(
                    check,
                    uuid,
                    format!(
                        "`{uri}` names the well `{name}`, and no well of this package is named so ({} well(s) read, by Title and NameLegal)",
                        wells.len()
                    ),
                )),
                several => out.push(unverifiable(
                    check,
                    uuid,
                    format!(
                        "`{uri}` names the well `{name}`, and {} wells answer to that name ({}): a name is not an identity",
                        several.len(),
                        several.join(", ")
                    ),
                )),
            }
        }
    }
}

/// The names WITSML 2 gives a well: its `Citation.Title`, and its
/// `NameLegal` when it has one. Shared with `series::time_series`, which
/// matches a series to a well by the same names - one reading of *which well
/// this name means*, not two.
pub(crate) fn names_of(object: &dyn Reflect) -> Vec<String> {
    let mut names: Vec<String> = identity::title_of_reflect(object)
        .map(str::to_owned)
        .into_iter()
        .collect();
    names.extend(text(object, "NameLegal"));
    names
}

/// The identifier of the `well(…)` part of an Energistics URI - `None` when
/// the path has no such part, an error when the text is not such a URI.
///
/// Energistics Identifier Specification v4.0 §6.1 p. 13: `emlUri = "eml:" [
/// "//" authority ] "/" path` - the scheme is not case sensitive, the path is
/// (§6.3 p. 14).
pub(crate) fn well_named_by(uri: &str) -> Result<Option<String>, String> {
    let rest = uri.trim();
    let rest = match rest.get(..4) {
        Some(scheme) if scheme.eq_ignore_ascii_case("eml:") => &rest[4..],
        _ => {
            return Err(
                "not an Energistics URI (Identifier Specification v4.0 §6.1: it begins with `eml:`)".into(),
            );
        }
    };
    let path = match rest.strip_prefix("//") {
        Some(authority_and_path) => authority_and_path
            .split_once('/')
            .map(|(_, path)| path)
            .unwrap_or(""),
        None => rest
            .strip_prefix('/')
            .ok_or("an Energistics URI's path begins with `/` (§6.1)")?,
    };
    for part in path.split('/') {
        if let Some(identifier) = part
            .strip_prefix("well(")
            .and_then(|inner| inner.strip_suffix(')'))
        {
            return Ok(Some(percent_decoded(identifier)));
        }
    }
    Ok(None)
}

/// RFC 3986 percent-decoding, the one the identifier grammar allows
/// (`pct-encoded`, §6.3). A malformed escape is kept as written.
fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%'
            && let Some(hex) = text.get(at + 1..at + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            at += 3;
            continue;
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---------------------------------------------------------------------------
// Two definitive trajectories on one wellbore
// ---------------------------------------------------------------------------

/// WITSML TRG 2.1 §16.6 p. 350, on `Trajectory.Definitive`: *"There can only
/// be one trajectory per wellbore with definitive=true and it must define the
/// geometry of the whole wellbore (surface to bottom)."*
///
/// Judged per **wellbore**, and the finding is filed under the wellbore's uuid -
/// the object the rule is about, whether the package holds it or not - with
/// the trajectories named in what was read. One finding per wellbore, however
/// many trajectories claim it.
fn one_definitive_trajectory_per_wellbore(context: &Context<'_>, check: &Integrity, out: &mut Vec<Finding>) {
    // Lower-cased wellbore uuid → (the uuid as first written, the claimants).
    let mut by_wellbore: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    for (uuid, object) in context.objects {
        if object.type_info().type_key != "Trajectory"
            || text(*object, "Definitive").as_deref() != Some("true")
        {
            continue;
        }
        // A trajectory with no readable wellbore is `Validate`'s business: the
        // element is mandatory.
        let Some(wellbore) = reference(*object, "Wellbore") else {
            continue;
        };
        let title = identity::title_of_reflect(*object)
            .map(|title| format!(" ({title})"))
            .unwrap_or_default();
        by_wellbore
            .entry(wellbore.uuid.to_ascii_lowercase())
            .or_insert_with(|| (wellbore.uuid.clone(), Vec::new()))
            .1
            .push(format!("{uuid}{title}"));
    }
    for (wellbore, claimants) in by_wellbore.into_values() {
        if claimants.len() > 1 {
            out.push(violation(
                check,
                &wellbore,
                format!(
                    "{} trajectories say they are definitive: {}",
                    claimants.len(),
                    claimants.join(", ")
                ),
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// IsListric on a fault that is not normal
// ---------------------------------------------------------------------------

/// RESQML Interpretations.xsd l. 659-662 (2.2) and the same annotation in
/// 2.0.1: *"Indicates if the normal fault is listric or not. BUSINESS RULE:
/// Must be present if the fault is normal. Must not be present if the fault is
/// not normal."* (TRG 2.2 §6.20 p. 188.)
///
/// **What "normal" means is the one reading here**, and it is the design's:
/// present if and only if the only set is normal - normal is the fault's **only** throw kind, across every
/// `ThrowInterpretation`. An oblique fault (`normal` and `strike-slip`) is
/// therefore not normal, and carries no `IsListric`.
///
/// Two cases are **not** judged:
///
/// - a fault that declares no throw at all: nothing says whether it is normal,
///   and an absent condition does not hold (the house reading of `when`);
/// - a thrust with `IsListric` false - the same page contradicts the business
///   rule: *"A thrust surface is specified as a FaultInterpretation whose
///   FaultThrow kind is "thrust" and which has the attributes: is Listric = 0"*.
///   Two sentences of one guide disagree, so the finding is *uncheckable* and
///   names both, rather than siding with either.
fn is_listric_iff_normal(context: &Context<'_>, check: &Integrity, out: &mut Vec<Finding>) {
    for (uuid, object) in context.objects {
        if !concepts_of(*object).contains(&"FaultInterpretation") {
            continue;
        }
        let mut kinds: Vec<String> = Vec::new();
        for throw in objects(*object, "ThrowInterpretation") {
            for kind in scalars(throw, "Throw") {
                if !kinds.contains(&kind) {
                    kinds.push(kind);
                }
            }
        }
        if kinds.is_empty() {
            continue;
        }
        let normal = kinds.len() == 1 && kinds[0] == "normal";
        match (normal, text(*object, "IsListric")) {
            (true, Some(_)) | (false, None) => {}
            (true, None) => out.push(violation(
                check,
                uuid,
                "the fault's only throw kind is normal, and IsListric is absent".into(),
            )),
            (false, Some(listric)) if kinds.len() == 1 && kinds[0] == "thrust" && listric == "false" => {
                out.push(unverifiable(
                    check,
                    uuid,
                    "a thrust with IsListric false: TRG 2.2 §6.20 p. 188 asks for exactly that (\"is Listric = 0\") in its notes, and forbids it in the business rule on IsListric".into(),
                ))
            }
            (false, Some(listric)) => out.push(violation(
                check,
                uuid,
                format!("IsListric = {listric} on a fault whose throw is {}", kinds.join(" and ")),
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Collections that are not what they say
// ---------------------------------------------------------------------------

/// One member of a collection, as the association names it.
struct Member {
    uuid: String,
    type_key: String,
}

/// Each collection's associations by lower-cased uuid: the version an
/// association names (`None`: none), and the members it lists.
type Memberships = BTreeMap<String, Vec<(Option<String>, Vec<Member>)>>;

/// What each collection holds, read from every
/// `CollectionsToDataobjectsAssociationSet` of the package - the only place
/// EML 2.3 writes membership (Collection.xsd: *"The relationships (between the
/// data objects and the collection) are specified and managed using the
/// SingleCollectionAssociation"*). Keyed by the lower-cased collection uuid,
/// each entry **carrying the version the association names** - two versions of
/// a collection hold two different member lists, and [`members_of`] tells them
/// apart.
///
/// The member's type is read off the reference, never by opening the member:
/// what a reference declares is what a reader of the collection is told.
fn memberships(context: &Context<'_>) -> Memberships {
    let mut out: Memberships = BTreeMap::new();
    for (_, object) in context.objects {
        if object.type_info().type_key != "CollectionsToDataobjectsAssociationSet" {
            continue;
        }
        for association in objects(*object, "SingleCollectionAssociation") {
            let Some(collection) = reference(association, "Collection") else {
                continue;
            };
            let version = field_version(association, "Collection");
            let members: Vec<Member> = objects(association, "Dataobject")
                .into_iter()
                .filter_map(reference_of)
                .collect();
            out.entry(collection.uuid.to_ascii_lowercase())
                .or_default()
                .push((version, members));
        }
    }
    out
}

/// `trg/homogeneous-collection`, on the collection's own flag. The finding is
/// the collection's: it is the object whose claim is false.
fn homogeneous_collection(context: &Context<'_>, check: &Integrity, out: &mut Vec<Finding>) {
    let held = memberships(context);
    for (uuid, object) in context.objects {
        if object.type_info().type_key != "DataobjectCollection"
            || text(*object, "HomogeneousDatatype").as_deref() != Some("true")
        {
            continue;
        }
        let members = members_of(&held, uuid, identity::object_version_of_reflect(*object));
        if members.is_empty() {
            continue;
        }
        if let Some(read) = mixed(&members) {
            out.push(violation(
                check,
                uuid,
                format!("declares HomogeneousDatatype true and holds {read}"),
            ));
        }
    }
}

/// `trg/homogeneous-association`, on the association's own flag. Filed under
/// the association set, the association named by its index.
fn homogeneous_association(context: &Context<'_>, check: &Integrity, out: &mut Vec<Finding>) {
    for (uuid, object) in context.objects {
        if object.type_info().type_key != "CollectionsToDataobjectsAssociationSet" {
            continue;
        }
        for (at, association) in objects(*object, "SingleCollectionAssociation")
            .into_iter()
            .enumerate()
        {
            if text(association, "HomogeneousDatatype").as_deref() != Some("true") {
                continue;
            }
            let members: Vec<Member> = objects(association, "Dataobject")
                .into_iter()
                .filter_map(reference_of)
                .collect();
            if let Some(read) = mixed(&members.iter().collect::<Vec<_>>()) {
                out.push(violation(
                    check,
                    uuid,
                    format!("SingleCollectionAssociation[{at}]: declares HomogeneousDatatype true and lists {read}"),
                ));
            }
        }
    }
}

/// The types among the members, counted, when there are several - `None` when
/// the members are of one type.
fn mixed(members: &[&Member]) -> Option<String> {
    let mut counted: Vec<(&str, usize)> = Vec::new();
    for member in members {
        match counted.iter_mut().find(|(key, _)| *key == member.type_key) {
            Some((_, count)) => *count += 1,
            None => counted.push((member.type_key.as_str(), 1)),
        }
    }
    (counted.len() > 1).then(|| {
        counted
            .iter()
            .map(|(key, count)| format!("{count} {key}"))
            .collect::<Vec<_>>()
            .join(", ")
    })
}

/// `trg/wellbore-interpretation-set-holds-wellbores`. Filed under the earth
/// model, whose association the guide constrains; one finding for every
/// stranger across its sets, each named with the set it sits in.
fn wellbore_interpretation_set_holds_wellbores(
    context: &Context<'_>,
    check: &Integrity,
    out: &mut Vec<Finding>,
) {
    let held = memberships(context);
    for (uuid, object) in context.objects {
        if !concepts_of(*object).contains(&"EarthModelInterpretation") {
            continue;
        }
        let mut strangers = Vec::new();
        for (at, set) in objects(*object, "WellboreInterpretationSet")
            .into_iter()
            .enumerate()
        {
            let Some(collection) = reference_of(set) else {
                continue;
            };
            let members = members_of(&held, &collection.uuid, field_version_of(set));
            for member in members {
                if member.type_key != "WellboreInterpretation" {
                    strangers.push(format!(
                        "`WellboreInterpretationSet[{at}]` ({}) holds {} ({})",
                        collection.uuid, member.uuid, member.type_key
                    ));
                }
            }
        }
        if !strangers.is_empty() {
            out.push(violation(check, uuid, strangers.join(", ")));
        }
    }
}

// ---------------------------------------------------------------------------
// Two directional properties of one quantity
// ---------------------------------------------------------------------------

/// Below this fraction of the lateral value, a vertical value breaks the
/// expected order (ADR-0036).
pub const RATIO_TOLERANCE: f64 = 0.95;

/// Beyond this share of the pairs breaking the order, the rule warns
/// (ADR-0036).
pub const RATIO_SHARE: f64 = 0.05;

/// What a kind expects of its vertical values against its lateral ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectionalOrder {
    /// The vertical value is not below the lateral one (resistivity).
    VerticalNotBelowLateral,
}

/// One kind the rule judges: a table, not a `match` (ADR-0006).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectionalExpectation {
    /// The dictionary uuid of the kind; its descendants are judged too.
    pub kind_uuid: &'static str,
    /// Its title, for the report.
    pub kind_title: &'static str,
    /// The order its values are expected to keep.
    pub order: DirectionalOrder,
}

/// The kinds judged. `resistivity` is the Energistics dictionary's
/// 7c0d1a5d-483e-4d77-81f3-9883527838d2 (`data/property_kinds.tsv`).
/// Permeability is not here: its expectation is weaker and unconfirmed
/// (TODO(verify): ADR-0036).
pub const DIRECTIONAL_EXPECTATIONS: &[DirectionalExpectation] = &[DirectionalExpectation {
    kind_uuid: "7c0d1a5d-483e-4d77-81f3-9883527838d2",
    kind_title: "resistivity",
    order: DirectionalOrder::VerticalNotBelowLateral,
}];

/// What a caller says a property's direction is, where its facet does not (an
/// extension value). Never written into the package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredDirection {
    /// The property, by its full uri.
    pub property: String,
    /// The direction the caller says it is.
    pub role: DirectionRole,
}

/// A direction, as the rule reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectionRole {
    /// Across the layers (`K`, `Z`).
    Vertical,
    /// Along the layers (`I`, `J`, `X`, `Y`).
    Lateral,
}

/// Which axes a direction facet names: a grid's (`I`, `J`, `K`) or the
/// coordinates' (`X`, `Y`, `Z`). A grid's vertical is not the coordinates'
/// lateral, so two families are never paired. A declared direction pairs with
/// any family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axes {
    Grid,
    Coordinates,
    Declared,
}

/// How a direction facet reads.
enum DirectionReading {
    /// A role, in a family of axes.
    Role(DirectionRole, Axes),
    /// An extension value nobody declared: not readable.
    Extension(String),
    /// A standard value that names no axis.
    Unknown(String),
}

/// One property the rule may judge: it has exactly one `direction` facet and a
/// support.
struct Directional<'a> {
    uuid: &'a str,
    uri: String,
    support: String,
    read: property::Property<'a>,
}

const DIRECTIONAL_RATIO: &str = "house/property/directional-ratio";

/// The rule, with what the caller declared: a view of a property runs it again
/// once a person said which sibling is vertical. [`run`] calls it with nothing
/// declared.
///
/// A [`DeclaredDirection`] is read for this one run and never written into the
/// package.
pub fn directional_ratio_declaring(
    objects: &[(&str, &dyn Reflect)],
    source: &dyn ObjectSource,
    arrays: Option<&dyn ArraySource>,
    declared: &[DeclaredDirection],
) -> Vec<Finding> {
    let context = Context {
        objects,
        source,
        arrays,
    };
    let mut out = Vec::new();
    if let Some(check) = CHECKS.iter().find(|check| check.id == DIRECTIONAL_RATIO) {
        judge_directional(&context, check, declared, &mut out);
    }
    out
}

fn directional_ratio(context: &Context<'_>, check: &Integrity, out: &mut Vec<Finding>) {
    judge_directional(context, check, &[], out);
}

/// Read one direction facet. A standard value is the schema's (`I`, `K+`...,
/// EML 2.3 `Facet`; common TRG 2.3 §11.1 p. 281); an extension value is the
/// producer's and is read only when the caller declared it.
fn direction_reading(facet: &property::Facet, uri: &str, declared: &[DeclaredDirection]) -> DirectionReading {
    match &facet.value {
        property::FacetValue::Standard(value) => {
            let upper = value.trim().to_ascii_uppercase();
            match upper.trim_end_matches(['+', '-']) {
                "K" => DirectionReading::Role(DirectionRole::Vertical, Axes::Grid),
                "Z" => DirectionReading::Role(DirectionRole::Vertical, Axes::Coordinates),
                "I" | "J" => DirectionReading::Role(DirectionRole::Lateral, Axes::Grid),
                "X" | "Y" => DirectionReading::Role(DirectionRole::Lateral, Axes::Coordinates),
                _ => DirectionReading::Unknown(value.clone()),
            }
        }
        property::FacetValue::Extension(value) => {
            match declared
                .iter()
                .find(|declaration| declaration.property.eq_ignore_ascii_case(uri))
            {
                Some(declaration) => DirectionReading::Role(declaration.role, Axes::Declared),
                None => DirectionReading::Extension(value.clone()),
            }
        }
    }
}

/// Whether two directions can be paired: one family, or one of them declared.
fn same_axes(a: Axes, b: Axes) -> bool {
    a == b || a == Axes::Declared || b == Axes::Declared
}

/// What the kind of a property expects: `Ok(None)` when the rule does not
/// apply to it, an error when the kind does not resolve.
fn expectation_of(
    resolver: &Resolver<'_>,
    kind: &property::PropertyKindRef,
    kind_uri: Option<&str>,
) -> std::result::Result<Option<DirectionalOrder>, String> {
    let resolution = resolver
        .resolve_at(kind, kind_uri)
        .map_err(|why| format!("its kind does not resolve: {why}"))?;
    Ok(DIRECTIONAL_EXPECTATIONS
        .iter()
        .find(|expectation| {
            resolution
                .chain
                .iter()
                .any(|kind| kind.uuid.eq_ignore_ascii_case(expectation.kind_uuid))
        })
        .map(|expectation| expectation.order))
}

/// The values of a property, every patch in order; the array's `NullValue` and
/// every non-finite value are `NaN`.
fn directional_values(
    read: &property::Property<'_>,
    arrays: &dyn ArraySource,
) -> std::result::Result<Vec<f64>, String> {
    let mut all = Vec::new();
    for patch in &read.patches {
        let values = patch
            .read(arrays)
            .map_err(|why| format!("patch {} does not read: {why}", patch.position))?;
        let numbers = values
            .to_f64()
            .ok_or_else(|| format!("patch {} does not hold numbers", patch.position))?;
        let null = crate::values::null_value(patch.array).map(|n| n as f64);
        all.extend(numbers.into_iter().map(|value| {
            if !value.is_finite() || Some(value) == null {
                f64::NAN
            } else {
                value
            }
        }));
    }
    Ok(all)
}

/// Say once, per property and per reason, that a property cannot be judged.
fn tell(
    out: &mut Vec<Finding>,
    told: &mut BTreeSet<(String, String)>,
    check: &Integrity,
    uuid: &str,
    why: String,
) {
    if told.insert((uuid.to_owned(), why.clone())) {
        out.push(unverifiable(check, uuid, why));
    }
}

/// **House rule** (ADR-0036): the specification names the directions of a
/// property (common TRG 2.3 §11.1 p. 281) and says nothing of the order of
/// their values; this is a reading of the physics of a layered medium, so it
/// is a warning and never an error.
///
/// The siblings are found by [`property::kinship::kinship`], the one rule; this
/// function adds what only it knows: which direction is vertical, which kinds
/// expect an order, and the count of the pairs that break it
/// ([`crate::stats::tally`], the one counter).
#[allow(clippy::needless_range_loop)] // `i` and `j` name the pair, not only the element
fn judge_directional(
    context: &Context<'_>,
    check: &Integrity,
    declared: &[DeclaredDirection],
    out: &mut Vec<Finding>,
) {
    use crate::property::kinship::{KinFacts, Kinship, kinship};
    use crate::stats::{CellRule, tally};

    let mut candidates: Vec<Directional<'_>> = Vec::new();
    for (uuid, object) in context.objects {
        if !concepts_of(*object).contains(&"AbstractProperty") {
            continue;
        }
        let Ok(read) = property::read(*object) else {
            continue;
        };
        let directions = read
            .facets
            .iter()
            .filter(|facet| facet.kind.eq_ignore_ascii_case("direction"))
            .count();
        if directions != 1 {
            continue;
        }
        let Some(support) = read
            .supporting_representation_uri
            .clone()
            .or_else(|| read.supporting_representation.clone())
        else {
            continue;
        };
        let uri = identity::uri_of_reflect(*object).unwrap_or_else(|| (*uuid).to_owned());
        candidates.push(Directional {
            uuid,
            uri,
            support,
            read,
        });
    }

    let resolver = Resolver::new(context.source);
    let mut orders: BTreeMap<String, std::result::Result<Option<DirectionalOrder>, String>> = BTreeMap::new();
    let mut values: BTreeMap<usize, std::result::Result<Vec<f64>, String>> = BTreeMap::new();
    let mut told: BTreeSet<(String, String)> = BTreeSet::new();

    for i in 0..candidates.len() {
        for j in (i + 1)..candidates.len() {
            let (a, b) = (&candidates[i], &candidates[j]);
            let Some(Kinship::Direction { this, other }) = kinship(
                &KinFacts::of_property(&a.uri, &a.support, &a.read),
                &KinFacts::of_property(&b.uri, &b.support, &b.read),
            ) else {
                continue;
            };
            let Some(kind) = &a.read.kind else {
                continue;
            };
            let key = match kind {
                property::PropertyKindRef::Object { uuid, .. } => uuid.to_ascii_lowercase(),
                property::PropertyKindRef::Standard { name } => name.to_ascii_lowercase(),
            };
            let expectation = orders
                .entry(key)
                .or_insert_with(|| expectation_of(&resolver, kind, a.read.kind_uri.as_deref()))
                .clone();
            let order = match expectation {
                Ok(Some(order)) => order,
                Ok(None) => continue,
                Err(why) => {
                    tell(out, &mut told, check, a.uuid, why);
                    continue;
                }
            };

            let reading_a = direction_reading(&this, &a.uri, declared);
            let reading_b = direction_reading(&other, &b.uri, declared);
            for (side, reading) in [(a, &reading_a), (b, &reading_b)] {
                match reading {
                    DirectionReading::Role(..) => {}
                    DirectionReading::Extension(value) => tell(
                        out,
                        &mut told,
                        check,
                        side.uuid,
                        format!(
                            "its direction facet `{value}` is an extension value: which direction it names is not in the specification (common TRG 2.3 §11.1 p. 281); declare it to compare"
                        ),
                    ),
                    DirectionReading::Unknown(value) => tell(
                        out,
                        &mut told,
                        check,
                        side.uuid,
                        format!(
                            "its direction facet `{value}` names no axis of the schema's list (common TRG 2.3 §11.1 p. 281)"
                        ),
                    ),
                }
            }
            let (DirectionReading::Role(role_a, axes_a), DirectionReading::Role(role_b, axes_b)) =
                (&reading_a, &reading_b)
            else {
                continue;
            };
            if role_a == role_b || !same_axes(*axes_a, *axes_b) {
                continue;
            }
            let (v, h) = if *role_a == DirectionRole::Vertical {
                (i, j)
            } else {
                (j, i)
            };
            let (vertical, lateral) = (&candidates[v], &candidates[h]);

            let Some(arrays) = context.arrays else {
                tell(
                    out,
                    &mut told,
                    check,
                    vertical.uuid,
                    "its values are an array, and this check was given nothing to read arrays with".into(),
                );
                continue;
            };
            for index in [v, h] {
                values
                    .entry(index)
                    .or_insert_with(|| directional_values(&candidates[index].read, arrays));
            }
            let (vertical_values, lateral_values) = match (&values[&v], &values[&h]) {
                (Ok(vertical_values), Ok(lateral_values)) => (vertical_values, lateral_values),
                (Err(why), _) => {
                    tell(out, &mut told, check, vertical.uuid, why.clone());
                    continue;
                }
                (_, Err(why)) => {
                    tell(out, &mut told, check, lateral.uuid, why.clone());
                    continue;
                }
            };
            if vertical_values.len() != lateral_values.len() {
                tell(
                    out,
                    &mut told,
                    check,
                    vertical.uuid,
                    format!(
                        "{} values against {}",
                        vertical_values.len(),
                        lateral_values.len()
                    ),
                );
                continue;
            }

            let rule = match order {
                DirectionalOrder::VerticalNotBelowLateral => CellRule::Below(RATIO_TOLERANCE),
            };
            let ratios: Vec<f64> = vertical_values
                .iter()
                .zip(lateral_values)
                .map(|(v, h)| {
                    if v.is_finite() && h.is_finite() && *h != 0.0 {
                        v / h
                    } else {
                        f64::NAN
                    }
                })
                .collect();
            let counted = tally(&ratios, 1, &rule).expect("the tolerance is a finite number");
            let pairs = counted.cells - counted.absent;
            if pairs == 0 {
                tell(
                    out,
                    &mut told,
                    check,
                    vertical.uuid,
                    "no pair of finite values".into(),
                );
                continue;
            }
            let share = counted.matched as f64 / pairs as f64;
            if share > RATIO_SHARE {
                out.push(violation(
                    check,
                    vertical.uuid,
                    format!(
                        "{:.2} % of {pairs} pairs below {RATIO_TOLERANCE} (vertical `{}` against lateral `{}`)",
                        share * 100.0,
                        vertical.uri,
                        lateral.uri
                    ),
                ));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// An activity against its template
// ---------------------------------------------------------------------------

/// How many departures a finding spells out before saying `and N more`.
const ACTIVITY_ISSUES_SHOWN: usize = 10;

/// One departure of an activity from its template, as the report reads it.
fn issue_read(issue: &ParameterIssue) -> Option<String> {
    match issue {
        ParameterIssue::NotInTemplate { position, title } => {
            Some(format!("parameter `{title}` ({position}) is not in the template"))
        }
        ParameterIssue::KindDiffers {
            position,
            title,
            declared,
            written,
        } => Some(format!(
            "parameter `{title}` ({position}) is written {}, the template allows {}",
            written.word(),
            declared
                .iter()
                .map(|kind| kind.word())
                .collect::<Vec<_>>()
                .join(", ")
        )),
        ParameterIssue::TooFew { title, min, found } => {
            Some(format!("`{title}`: {found} for at least {min}"))
        }
        ParameterIssue::TooMany { title, max, found } => {
            Some(format!("`{title}`: {found} for at most {max}"))
        }
        // The template is given to `conformance` here: this cannot happen.
        ParameterIssue::TemplateAbsent => None,
    }
}

/// **House rule** (`house/provenance/activity-follows-its-template`): each
/// activity against the template its `ActivityDescriptor` names, by
/// `crate::activity::conformance` - the one reading of conformance, never a
/// second. A warning: the activity is read all the same. A template that
/// cannot be found is *unverifiable*, never a pass.
fn activity_follows_its_template(context: &Context<'_>, check: &Integrity, out: &mut Vec<Finding>) {
    let templates: Vec<TemplateRecord> = context
        .objects
        .iter()
        .filter_map(|(_, object)| read_template(*object).ok())
        .collect();
    for (uuid, object) in context.objects {
        let activity = match read_activity(*object) {
            Ok(activity) => activity,
            Err(ActivityRefusal::NoIdentity { .. }) => {
                out.push(unverifiable(
                    check,
                    uuid,
                    "it has no uuid, so the template it names cannot be told from another".into(),
                ));
                continue;
            }
            Err(_) => continue,
        };
        let Some(descriptor) = &activity.descriptor else {
            out.push(unverifiable(
                check,
                uuid,
                "it names no ActivityDescriptor, so there is no template to hold it to".into(),
            ));
            continue;
        };
        let fetched: Option<TemplateRecord> = match descriptor_of(&activity, &templates) {
            Some(_) => None,
            None => context
                .source
                .object_at(&descriptor.address)
                .and_then(|found| read_template(found.as_reflect()).ok()),
        };
        let template = match (descriptor_of(&activity, &templates), fetched.as_ref()) {
            (Some(template), _) | (None, Some(template)) => template,
            (None, None) => {
                out.push(unverifiable(
                    check,
                    uuid,
                    format!("its template `{}` is not in this package", descriptor.address),
                ));
                continue;
            }
        };
        let reads: Vec<String> = conformance(&activity, Some(template))
            .iter()
            .filter_map(issue_read)
            .collect();
        if reads.is_empty() {
            continue;
        }
        let mut read = reads
            .iter()
            .take(ACTIVITY_ISSUES_SHOWN)
            .cloned()
            .collect::<Vec<_>>()
            .join("; ");
        if reads.len() > ACTIVITY_ISSUES_SHOWN {
            read.push_str(&format!(" and {} more", reads.len() - ACTIVITY_ISSUES_SHOWN));
        }
        out.push(violation(check, uuid, read));
    }
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

fn violation(check: &Integrity, uuid: &str, read: String) -> Finding {
    Finding {
        rule: check.id.to_owned(),
        uuid: uuid.to_owned(),
        says: check.says.to_owned(),
        outcome: Outcome::Violation { read },
        gravity: check.gravity,
    }
}

fn unverifiable(check: &Integrity, uuid: &str, why: String) -> Finding {
    Finding {
        rule: check.id.to_owned(),
        uuid: uuid.to_owned(),
        says: check.says.to_owned(),
        outcome: Outcome::Unverifiable { why },
        gravity: check.gravity,
    }
}

// The readers of a field are the door's (ADR 0046), under the names this file
// already uses.
use energyml_core::reflect::fields::{objects, text, texts as scalars};

/// The reference held by a field, read with nothing opened.
fn reference(object: &dyn Reflect, name: &str) -> Option<Member> {
    match object.field(name)? {
        FieldValue::Object(child) => reference_of(child),
        _ => None,
    }
}

/// The members of the collection `uuid` at `version`: the associations that
/// name that version, and those that name none (a short reference is accepted
/// when nothing contradicts it). A collection asked for with no version takes
/// every association.
fn members_of<'a>(held: &'a Memberships, uuid: &str, version: Option<&str>) -> Vec<&'a Member> {
    held.get(&uuid.to_ascii_lowercase())
        .into_iter()
        .flatten()
        .filter(|(named, _)| identity::versions_agree(version, named.as_deref()))
        .flat_map(|(_, members)| members)
        .collect()
}

/// The version a reference held by a field names, when it names one.
fn field_version(object: &dyn Reflect, name: &str) -> Option<String> {
    match object.field(name)? {
        FieldValue::Object(child) => field_version_of(child).map(str::to_string),
        _ => None,
    }
}

fn field_version_of(reference: &dyn Reflect) -> Option<&str> {
    identity::object_version_of_reflect(reference)
        .map(str::trim)
        .filter(|version| !version.is_empty())
}

/// What a reference names: its uuid and the concept it declares.
fn reference_of(reference: &dyn Reflect) -> Option<Member> {
    let key = identity::key_of_reflect(reference)?;
    Some(Member {
        uuid: key.uuid.to_owned(),
        type_key: key.type_key().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_well_is_found_in_an_energistics_uri_and_nowhere_else() {
        // Identifier Specification v4.0 §6.3.1 p. 14's own example.
        assert_eq!(
            well_named_by("eml://prodml.mycompany.com/well(123456)/wellbore(01)/bottomhole/pressure meter"),
            Ok(Some("123456".into()))
        );
        // No authority (§6.2 allows it), and an escaped identifier.
        assert_eq!(
            well_named_by("eml:/well(GTH%2D9)/wellhead"),
            Ok(Some("GTH-9".into()))
        );
        // The scheme is not case sensitive (§6.1); `wellbore(…)` is not `well(…)`.
        assert_eq!(well_named_by("EML://a.b/wellbore(01)/wellhead"), Ok(None));
        // Not an Energistics URI: an error, which the check turns into an
        // uncheckable - never a pass.
        assert!(well_named_by("GTH-1 wellhead").is_err());
    }

    #[test]
    fn every_shipped_check_says_where_it_comes_from() {
        for check in checks() {
            assert!(
                check.id.starts_with("trg/") || check.id.starts_with("house/"),
                "`{}` does not say whether it is the standard's or ours",
                check.id
            );
            assert!(!check.says.trim().is_empty(), "`{}` says nothing", check.id);
        }
        let mut ids: Vec<&str> = checks().iter().map(|check| check.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), checks().len(), "two checks share an id");
    }
}
