//! A wellbore read as a **table**: one row per node, one column per property,
//! discrete properties rendered by their label.
//!
//! The need: see a complete table for a well, with the discrete properties
//! interpreted through their category lookup tables, so as to show all the
//! properties and values linked to a well at each measured depth, and export the
//! complete well CSVs.
//!
//! # Why here and not in the front end
//!
//! Because the view and the export must render **the same thing**. The
//! repository's rule is clear (ADR-0012: *no face carries logic*), and this
//! table is the one answer to *what is this property worth at this depth*.
//! Written in the front end, the command would have a second one; written in the
//! exporter, the view would have a second one. So it sits above storage and
//! below geometry, with the rest of what reads values.
//!
//! # A code is not a value
//!
//! A discrete property stores integers, and the integer means nothing alone: it
//! is the rank of a row in the table `CategoryLookup` names.
//! [`crate::table::Lookup`] reads it in **both forms** the standard has had -
//! `StringTableLookup` in 2.0.1, `eml23.ColumnBasedTable` in 2.2 - so this
//! module knows neither: it asks for the label.
//!
//! Writing the raw code into a CSV would be worse than an absence: an integer
//! looks like a measurement, and `3` reads as three.
//!
//! # Merging several wells into one file, and knowing when it cannot be done
//!
//! Two boreholes are pasted into one CSV by **uniting their columns**: what a
//! borehole lacks is an empty cell, not a shifted column. Two things prevent it,
//! and they are named rather than silently merged - see [`Conflict`].
//!
//! What does **not** prevent it: two boreholes whose category tables differ.
//! That is precisely what resolution into labels buys - `clay` is `clay`
//! whatever code carried it.

use crate::error::Result;
use crate::property::{self, IndexableElement};
use crate::source::ArraySource;
use crate::table::{Cell, KeyIndex, Lookup, RowMatch, row_named_by};
use crate::values::{self, Values};
use energyml_core::value::Reflect;

/// Where a column comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum From {
    /// The frame's measured depths - the **base** of each pass when the table
    /// is arranged by interval.
    MeasuredDepth,
    /// The **top** of each pass, when the table is arranged by interval.
    TopDepth,
    /// A property, by its uuid.
    Property(String),
    /// What is said of the borehole itself - its head, an alias, its system -
    /// and repeated on every row. The word names what it is.
    Wellbore(&'static str),
}

/// A column of the table.
#[derive(Debug, Clone)]
pub struct Column {
    /// What the header carries - the property's title, or `MD`.
    pub title: String,
    /// Its unit, when it has one.
    pub uom: Option<String>,
    /// The uuid of the property kind, when the property names one.
    pub kind: Option<String>,
    /// The **title** of that property kind - `well name`, `measured depth` -,
    /// which the export prefers as a header when it designates a single column
    /// (property kind names are prioritised as column titles when possible).
    pub kind_title: Option<String>,
    /// Where it comes from.
    pub from: From,
    /// One value per row - node or interval. `None` is an absence **in the
    /// file**, not a zero.
    pub values: Vec<Option<String>>,
}

/// A borehole, tabulated.
#[derive(Debug, Clone)]
pub struct WellTable {
    /// The frame's title.
    pub well: String,
    /// The frame's uuid.
    pub uuid: String,
    /// The depth first, then one column per property.
    pub columns: Vec<Column>,
    /// The number of rows: one per node, or one per interval when the frame
    /// carries pass properties (see [`of_frame`]).
    pub rows: usize,
    /// What could not be read, named.
    pub notes: Vec<String>,
}

/// What prevents two boreholes from fitting in the same file.
///
/// **Named rather than merged.** Putting millidarcies from one borehole and
/// square metres from another in one `PERM` column gives a perfectly well-formed
/// CSV that nothing downstream can detect - the fault this repository refuses
/// everywhere else.
#[derive(Debug, Clone, PartialEq)]
pub enum Conflict {
    /// The same column, two units.
    Uom {
        /// The shared title.
        column: String,
        /// What the first borehole says.
        first: Option<String>,
        /// What the other says.
        other: Option<String>,
    },
    /// The same column, two property kinds.
    Kind {
        /// The shared title.
        column: String,
        /// The first one's kind.
        first: Option<String>,
        /// The other one's.
        other: Option<String>,
    },
}

impl Conflict {
    /// The sentence a caller shows.
    pub fn says(&self) -> String {
        match self {
            Conflict::Uom { column, first, other } => format!(
                "`{column}` is in {} for one wellbore and {} for another - one column cannot hold both",
                first.as_deref().unwrap_or("no unit"),
                other.as_deref().unwrap_or("no unit")
            ),
            Conflict::Kind { column, first, other } => format!(
                "`{column}` is {} for one wellbore and {} for another - same name, two measurements",
                first.as_deref().unwrap_or("of no stated kind"),
                other.as_deref().unwrap_or("of no stated kind")
            ),
        }
    }

    /// The title of the column at fault.
    pub fn column(&self) -> &str {
        match self {
            Conflict::Uom { column, .. } | Conflict::Kind { column, .. } => column,
        }
    }
}

/// The name of a borehole, stripped of the role a writer appends to it.
///
/// `"F1 - frame"`, `"F1 : trajectory"`, `"F1 - STRA"`: a producer puts the role
/// after the name, and the separator is never alphanumeric. What remains in
/// front is the identity.
///
/// **This is not a comfort: it is what makes merging possible.** Without it, a
/// column is called `"F1 - STRA"` for one and `"F2 - STRA"` for the other, so
/// the union of the columns unites nothing and a ten-well file has ten times
/// the same columns, nine tenths empty. Found while exporting - the view alone
/// would not have shown it.
pub fn stem(title: &str) -> &str {
    for cut in [" - ", " : ", " - "] {
        if let Some(at) = title.find(cut) {
            return title[..at].trim_end();
        }
    }
    title
}

/// What a column measures, without the borehole that carries it.
///
/// A property's title identifies **the object** and so carries its borehole;
/// a table's header names **the measurement**, and the `STRA` of two boreholes
/// is the same one. The prefix is removed **only if it is this borehole's**:
/// cutting at the first separator found would turn a legitimate title such as
/// `"porosité - corrigée"` into a column `corrigée`.
fn heading(title: &str, well: &str) -> String {
    for cut in [" - ", " : ", " - "] {
        if let Some(rest) = title.strip_prefix(well).and_then(|rest| rest.strip_prefix(cut)) {
            return rest.to_string();
        }
    }
    title.to_string()
}

/// Whether this property hangs on that representation.
///
/// **One reading**, because two callers deciding it each would end up not
/// exporting the same columns as the view shows.
pub fn hangs_on(object: &dyn Reflect, representation: &str) -> bool {
    property::read(object)
        .ok()
        .and_then(|read| read.supporting_representation)
        .is_some_and(|uuid| uuid == representation)
}

/// A frame and its properties, tabulated.
///
/// `properties` is what the caller found through [`hangs_on`]: this module does
/// not walk a package, it reads what it is given - the rule that keeps it
/// testable without a file.
pub fn of_frame(
    frame: &dyn Reflect,
    properties: &[&dyn Reflect],
    arrays: &dyn ArraySource,
) -> Result<WellTable> {
    let titled = energyml_core::identity::title_of_reflect(frame)
        .unwrap_or("wellbore")
        .to_string();
    let well = stem(&titled).to_string();
    let uuid = energyml_core::identity::uuid_of_reflect(frame)
        .unwrap_or_default()
        .to_string();
    let mut notes = Vec::new();

    // The depth first: it is the axis, and it fixes the number of rows. Without
    // it there is no table - a value without a depth fits nowhere.
    let mds = match frame.field("NodeMd") {
        Some(energyml_core::value::FieldValue::Object(array)) => {
            crate::values::read(array, arrays, "NodeMd")?
        }
        _ => {
            return Err(crate::error::Error::malformed(
                &uuid,
                "this object states no `NodeMd`, so it is not a wellbore frame and its values have no depth to sit at",
            ));
        }
    };
    // **One row per interval when the frame carries passes.** A pass property is
    // worth *from here to there*: putting it on a node would shift it by one
    // rank, and that is the fault `properties-from-table` exists to avoid. So
    // the table takes the shape of the file it comes from - a top, a base, a
    // value - and that is what makes the CSV identical to the source one.
    let by_passes = mds.len() >= 2
        && properties.iter().any(|object| {
            property::read(*object).is_ok_and(|read| read.indexable == IndexableElement::Intervals)
        });
    let (rows, wanted, mut columns) = if by_passes {
        let rows = mds.len() - 1;
        let top = Column {
            title: "top MD".to_string(),
            uom: None,
            kind: None,
            kind_title: Some("top depth".to_string()),
            from: From::TopDepth,
            values: (0..rows).map(|at| cell_text(&mds, at)).collect(),
        };
        let base = Column {
            title: "MD".to_string(),
            uom: None,
            kind: None,
            kind_title: Some("measured depth".to_string()),
            from: From::MeasuredDepth,
            values: (0..rows).map(|at| cell_text(&mds, at + 1)).collect(),
        };
        (rows, IndexableElement::Intervals, vec![top, base])
    } else {
        let rows = mds.len();
        let md = Column {
            title: "MD".to_string(),
            uom: None,
            kind: None,
            kind_title: Some("measured depth".to_string()),
            from: From::MeasuredDepth,
            values: (0..rows).map(|at| cell_text(&mds, at)).collect(),
        };
        (rows, IndexableElement::Nodes, vec![md])
    };

    for object in properties {
        match one_property(*object, &well, rows, wanted, arrays) {
            Ok((made, said)) => {
                columns.extend(made);
                notes.extend(said);
            }
            Err(why) => notes.push(why),
        }
    }
    Ok(WellTable {
        well,
        uuid,
        columns,
        rows,
        notes,
    })
}

/// A property, read into columns - or the reason it was not read.
///
/// **Several columns when its code table has several.** A table that carries the
/// code **and** its label (`STRA`, `Nom_STRA`) renders both: that is the shape of
/// the original CSV, and writing only one would lose either the code another
/// tool expects or the word a geologist reads.
fn one_property(
    object: &dyn Reflect,
    well: &str,
    rows: usize,
    wanted: IndexableElement,
    arrays: &dyn ArraySource,
) -> std::result::Result<(Vec<Column>, Vec<String>), String> {
    let read = property::read(object).map_err(|why| why.to_string())?;
    let title = read.title.clone().unwrap_or_else(|| read.uuid.clone());
    let column = heading(&title, well);

    // **Only what hangs on what the rows are.** An interval property has one
    // fewer value than there are nodes, and putting it on the nodes would shift
    // every value by one rank - the fault `properties-from-table` exists to
    // avoid. The reverse too: a node value has no row in a table of passes.
    if read.indexable != wanted {
        return Err(format!(
            "`{title}` hangs on {} while this table has one row per {}, so it has no row here",
            read.indexable.as_str(),
            if wanted == IndexableElement::Intervals {
                "interval"
            } else {
                "node"
            }
        ));
    }
    let Some(patch) = read.patches.first() else {
        return Err(format!("`{title}` states no values"));
    };
    let values = patch.read(arrays).map_err(|why| format!("`{title}`: {why}"))?;
    if values.len() != rows {
        return Err(format!(
            "`{title}` holds {} value(s) where the frame has {rows} {} - reading it here would shift every one",
            values.len(),
            if wanted == IndexableElement::Intervals {
                "interval(s)"
            } else {
                "node(s)"
            }
        ));
    }

    // The `NullValue` of the integer array: a code equal to it is no code.
    let null = values::null_value(patch.array);

    // The label rather than the code, when a table gives one.
    //
    // **A property that names a lookup it cannot be read through is refused**,
    // not written as raw codes: the module doc says an integer looks like a
    // measurement, so a column of codes under a label's heading would be worse
    // than an absence. The note carries the reason.
    let labels = match read.lookup.as_deref() {
        None => None,
        Some(uuid) => {
            // By the uri the property names: the version named is the version read.
            let object = match read.lookup_uri.as_deref() {
                Some(uri) => arrays.object_at(uri),
                None => arrays.object(uuid),
            };
            let Some(object) = object else {
                return Err(format!(
                    "`{title}` names a category lookup ({}) that is not here, so its codes have no label; \
                     they are not written as if they were values",
                    read.lookup_uri.as_deref().unwrap_or(uuid)
                ));
            };
            match Lookup::read(object.as_reflect(), arrays) {
                Ok(table) => Some(table),
                Err(why) => {
                    return Err(format!(
                        "`{title}` names a category lookup that cannot be read ({why}), so its codes have no label; \
                         they are not written as if they were values"
                    ));
                }
            }
        }
    };
    let kind = read.kind.as_ref().and_then(kind_uuid);
    let kind_title = read.kind.as_ref().and_then(|kind| match kind {
        property::PropertyKindRef::Object { title, .. } => title.clone(),
        property::PropertyKindRef::Standard { name } => Some(name.clone()),
    });

    // The table's columns that are not keys: the label alone most of the time,
    // the code and the label when the table groups both.
    let table_columns: Vec<usize> = labels
        .as_ref()
        .map(|table| {
            (0..table.titles.len())
                .filter(|one| !table.key_columns.contains(one))
                .collect()
        })
        .unwrap_or_default();
    // The table's key index, built **once** and asked for every cell: a code
    // names its row by the table's key, or by its rank when it has none.
    let index: Option<KeyIndex> = labels.as_ref().map(Lookup::key_index);

    // **A code no row names is an absence, said in the notes**, never the raw
    // number written under the label's heading. A code that is the array's
    // `NullValue` is an absence too, without a note: that is what it is for.
    let mut said = Vec::new();
    if let Some(index) = &index {
        let unmatched: Vec<String> = (0..rows)
            .filter(|at| !is_absent(&values, *at, null))
            .filter(|at| row_of(index, &values, *at).is_none())
            .filter_map(|at| cell_text(&values, at))
            .collect();
        if !unmatched.is_empty() {
            let mut distinct: Vec<&str> = Vec::new();
            for code in &unmatched {
                if !distinct.contains(&code.as_str()) {
                    distinct.push(code);
                }
            }
            said.push(format!(
                "`{title}`: {} code(s) name no row of their category lookup, and are left empty \
                 rather than written as values (code(s) {})",
                unmatched.len(),
                distinct.iter().take(5).copied().collect::<Vec<_>>().join(", ")
            ));
        }
    }

    if let (Some(table), Some(index), true) = (&labels, &index, table_columns.len() > 1) {
        let made = table_columns
            .iter()
            .enumerate()
            .map(|(rank, &at_column)| Column {
                title: table.titles[at_column]
                    .clone()
                    .unwrap_or_else(|| format!("{column} {}", rank + 1)),
                uom: None,
                // The property's kind goes to the **first** column, the code:
                // it is what the property measures. The following ones give its
                // meaning and have no kind of their own here.
                kind: if rank == 0 { kind.clone() } else { None },
                kind_title: if rank == 0 { kind_title.clone() } else { None },
                from: From::Property(read.uuid.clone()),
                values: (0..rows)
                    .map(|at| cell_of(table, index, &values, at, at_column))
                    .collect(),
            })
            .collect();
        return Ok((made, said));
    }

    let cells = (0..rows)
        .map(|at| match (&labels, &index) {
            (Some(table), Some(index)) => label_at(table, index, &values, at),
            _ => code_text(&values, at, null),
        })
        .collect();

    Ok((
        vec![Column {
            title: column,
            uom: read.uom.clone(),
            kind,
            kind_title,
            from: From::Property(read.uuid.clone()),
            values: cells,
        }],
        said,
    ))
}

/// Whether the value at `at` is the file's absence: an integer equal to the
/// array's `NullValue`, a floating value that is not a number.
fn is_absent(values: &Values, at: usize, null: Option<i64>) -> bool {
    match values {
        Values::Integer(v) => v.get(at).is_none_or(|one| Some(*one) == null),
        Values::Floating(v) => v.get(at).is_none_or(|one| !one.is_finite()),
        Values::Boolean(_) | Values::Text(_) => false,
    }
}

/// A stored value as text, `None` when it is the file's absence ([`is_absent`]).
fn code_text(values: &Values, at: usize, null: Option<i64>) -> Option<String> {
    if is_absent(values, at, null) {
        return None;
    }
    cell_text(values, at)
}

/// The cell of this table column, at the row the code designates.
fn cell_of(table: &Lookup, index: &KeyIndex, values: &Values, at: usize, column: usize) -> Option<String> {
    let row = row_of(index, values, at)?;
    match table.rows.cell(row, column)? {
        Cell::Text(text) => Some(text),
        Cell::Number(number) => Some(trim(number)),
        Cell::Boolean(flag) => Some(flag.to_string()),
    }
}

/// The row of the code table that the value at `at` names: **by the table's
/// key** when it has one, by its rank from 0 when it has none
/// (`house/table/row-rank-without-key`, [`row_named_by`]) - never the rank of a
/// keyed table. A duplicated key names its first row ("first match wins"). A
/// value no row holds, and a refused table, name no row: the caller falls back
/// on the stored value.
fn row_of(index: &KeyIndex, values: &Values, at: usize) -> Option<usize> {
    let value = match values {
        Values::Integer(v) => *v.get(at)? as f64,
        Values::Floating(v) => *v.get(at)?,
        _ => return None,
    };
    match row_named_by(index, &Cell::Number(value)) {
        Ok(RowMatch::One(row)) => Some(row),
        Ok(RowMatch::Several(rows)) => rows.first().copied(),
        Ok(RowMatch::None) | Err(_) => None,
    }
}

/// The uuid of a property kind, when the reference names one.
fn kind_uuid(kind: &property::PropertyKindRef) -> Option<String> {
    match kind {
        property::PropertyKindRef::Object { uuid, .. } => Some(uuid.clone()),
        // A 2.0.1 kind is a **name** of the RESQML vocabulary, not a uuid:
        // returning it here would pass a name off as an identifier.
        property::PropertyKindRef::Standard { .. } => None,
    }
}

/// The label the table gives to this code.
///
/// **The last column that is not a key**, because that is where the label lives
/// in both forms: a `StringTableLookup` has `Key` and `Value`, a category
/// `ColumnBasedTable` has the code then its name. A single-column table returns
/// that column - it *is* the label.
fn label_at(table: &Lookup, index: &KeyIndex, values: &Values, at: usize) -> Option<String> {
    let row = row_of(index, values, at)?;
    let column = (0..table.titles.len())
        .rev()
        .find(|one| !table.key_columns.contains(one))?;
    match table.rows.cell(row, column)? {
        Cell::Text(text) => Some(text),
        Cell::Number(number) => Some(trim(number)),
        Cell::Boolean(flag) => Some(flag.to_string()),
    }
}

/// A value, as text - and `None` for what the file does not say.
///
/// A `NaN` is **an absence**, not the string `NaN`: that is how properties
/// created from a table write a node without a measurement, and rendering it
/// literally would turn a blank into a word.
fn cell_text(values: &Values, at: usize) -> Option<String> {
    match values {
        Values::Floating(v) => {
            let one = *v.get(at)?;
            one.is_finite().then(|| trim(one))
        }
        Values::Integer(v) => Some(v.get(at)?.to_string()),
        Values::Boolean(v) => Some(v.get(at)?.to_string()),
        Values::Text(v) => {
            let one = v.get(at)?;
            (!one.is_empty()).then(|| one.clone())
        }
    }
}

/// A number written without useless zeros - `6.4` and not `6.4000000000000004`.
fn trim(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        let text = format!("{value}");
        text
    }
}

/// Several boreholes, ready for a single file.
#[derive(Debug, Clone)]
pub struct Merged {
    /// The column titles, in order: `well`, `MD`, then the union.
    pub headers: Vec<String>,
    /// One row per node of each borehole, the borehole name first.
    pub rows: Vec<Vec<Option<String>>>,
    /// What prevents merging, named.
    pub conflicts: Vec<Conflict>,
}

/// Merge boreholes into a single table.
///
/// **The union of the columns, in the order they appear**: a borehole that lacks
/// a column has empty cells, never a shifted row. And the first column is the
/// **borehole name**, without which a multi-well file is one where nobody knows
/// which row belongs to which.
///
/// Conflicts are **returned, not raised**: the caller decides whether to write
/// anyway or to split the files, and can only decide if it sees them.
pub fn merge(tables: &[WellTable]) -> Merged {
    let mut headers: Vec<String> = vec!["well".to_string()];
    let mut seen: Vec<(String, Option<String>, Option<String>)> = Vec::new();
    let mut conflicts = Vec::new();
    for table in tables {
        for column in &table.columns {
            match seen.iter().find(|(title, _, _)| *title == column.title) {
                None => {
                    seen.push((column.title.clone(), column.uom.clone(), column.kind.clone()));
                    headers.push(column.title.clone());
                }
                Some((_, uom, kind)) => {
                    if uom != &column.uom {
                        conflicts.push(Conflict::Uom {
                            column: column.title.clone(),
                            first: uom.clone(),
                            other: column.uom.clone(),
                        });
                    }
                    if kind != &column.kind {
                        conflicts.push(Conflict::Kind {
                            column: column.title.clone(),
                            first: kind.clone(),
                            other: column.kind.clone(),
                        });
                    }
                }
            }
        }
    }

    let mut rows = Vec::new();
    for table in tables {
        for at in 0..table.rows {
            let mut row: Vec<Option<String>> = Vec::with_capacity(headers.len());
            row.push(Some(table.well.clone()));
            for title in headers.iter().skip(1) {
                row.push(
                    table
                        .columns
                        .iter()
                        .find(|column| &column.title == title)
                        .and_then(|column| column.values.get(at).cloned().flatten()),
                );
            }
            rows.push(row);
        }
    }
    Merged {
        headers,
        rows,
        conflicts,
    }
}

/// The CSV of a merged table.
///
/// RFC 4180: a field that carries the separator, a quote or a line ending is
/// quoted, and a quote is doubled. Without that, a label such as
/// `argile, sableuse` would split its line in two - and the file would remain
/// perfectly well formed.
pub fn to_csv(merged: &Merged, separator: char) -> String {
    let quote = |cell: &Option<String>| -> String {
        let text = cell.as_deref().unwrap_or("");
        if text.contains(separator) || text.contains('"') || text.contains('\n') || text.contains('\r') {
            format!("\"{}\"", text.replace('"', "\"\""))
        } else {
            text.to_string()
        }
    };
    let mut out = String::new();
    let titles: Vec<Option<String>> = merged.headers.iter().map(|one| Some(one.clone())).collect();
    out.push_str(
        &titles
            .iter()
            .map(quote)
            .collect::<Vec<_>>()
            .join(&separator.to_string()),
    );
    out.push('\n');
    for row in &merged.rows {
        out.push_str(
            &row.iter()
                .map(quote)
                .collect::<Vec<_>>()
                .join(&separator.to_string()),
        );
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::Rows;

    fn lookup(key: Option<Vec<i64>>, labels: Vec<&str>) -> Lookup {
        let mut columns = Vec::new();
        let mut titles = Vec::new();
        let mut key_columns = Vec::new();
        if let Some(key) = key {
            columns.push(Some(Values::Integer(key)));
            titles.push(Some("code".to_string()));
            key_columns.push(0);
        }
        let rows = labels.len();
        columns.push(Some(Values::Text(labels.into_iter().map(String::from).collect())));
        titles.push(Some("name".to_string()));
        let width = columns.len();
        Lookup {
            titles,
            key_columns,
            rows: Rows {
                columns,
                per_row: vec![1; width],
                nulls: vec![None; width],
                rows,
                notes: Vec::new(),
            },
            notes: Vec::new(),
        }
    }

    #[test]
    fn a_keyed_category_table_names_its_row_by_key() {
        // Common TRG 2.3 §9.1 p. 190: a key column is what makes the table a
        // lookup; a code names its row by that key, never by its position.
        let table = lookup(Some(vec![10, 20, 30]), vec!["a", "b", "c"]);
        let index = table.key_index();
        assert_eq!(
            label_at(&table, &index, &Values::Integer(vec![20]), 0).as_deref(),
            Some("b")
        );
        assert_eq!(
            label_at(&table, &index, &Values::Integer(vec![1]), 0),
            None,
            "a keyed table is never read by rank"
        );
    }

    #[test]
    fn a_keyless_category_table_names_its_row_by_rank() {
        let table = lookup(None, vec!["a", "b", "c"]);
        let index = table.key_index();
        assert_eq!(
            label_at(&table, &index, &Values::Integer(vec![1]), 0).as_deref(),
            Some("b")
        );
        assert_eq!(label_at(&table, &index, &Values::Integer(vec![3]), 0), None);
    }
}
