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
//! # Where the values come from
//!
//! [`crate::frame_values`], the one door for *what is this property worth along
//! this frame* (ADR-0049): its support (nodes or intervals - `intervals from
//! datum` is a pass like `intervals`), its values and the row of its table. This
//! module lays them in columns and writes them as text; it reads neither a
//! property nor a table of its own.
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
use crate::frame_values::{self, Categories, Value};
use crate::property::{self, IndexableElement};
use crate::source::ArraySource;
use crate::table::Cell;
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
///
/// The values of each property are read by [`crate::frame_values`], the one door
/// for *what is this property worth along this frame*: this module lays them in
/// columns and writes them as text.
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
    let mds = frame_values::node_mds(frame, arrays).map_err(|why| match why {
        frame_values::Refusal::Read(error) => error,
        other => crate::error::Error::malformed(&uuid, other.to_string()),
    })?;
    // **One row per interval when the frame carries passes.** A pass property is
    // worth *from here to there*: putting it on a node would shift it by one
    // rank, and that is the fault `properties-from-table` exists to avoid. So
    // the table takes the shape of the file it comes from - a top, a base, a
    // value - and that is what makes the CSV identical to the source one.
    //
    // `intervals` and `intervals from datum` are both passes
    // ([`IndexableElement::is_frame_interval`]): the table used to compare with
    // `intervals` alone and wrote a frame whose properties said `intervals from
    // datum` as a table of nodes, with every one of those properties refused.
    let by_passes = mds.len() >= 2
        && properties
            .iter()
            .any(|object| property::read(*object).is_ok_and(|read| read.indexable.is_frame_interval()));
    let (rows, mut columns) = if by_passes {
        let rows = mds.len() - 1;
        let top = Column {
            title: "top MD".to_string(),
            uom: None,
            kind: None,
            kind_title: Some("top depth".to_string()),
            from: From::TopDepth,
            values: (0..rows).map(|at| md_text(&mds, at)).collect(),
        };
        let base = Column {
            title: "MD".to_string(),
            uom: None,
            kind: None,
            kind_title: Some("measured depth".to_string()),
            from: From::MeasuredDepth,
            values: (0..rows).map(|at| md_text(&mds, at + 1)).collect(),
        };
        (rows, vec![top, base])
    } else {
        let rows = mds.len();
        let md = Column {
            title: "MD".to_string(),
            uom: None,
            kind: None,
            kind_title: Some("measured depth".to_string()),
            from: From::MeasuredDepth,
            values: (0..rows).map(|at| md_text(&mds, at)).collect(),
        };
        (rows, vec![md])
    };

    for object in properties {
        match one_property(*object, &well, &uuid, &mds, by_passes, arrays) {
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
    frame: &str,
    mds: &[f64],
    by_passes: bool,
    arrays: &dyn ArraySource,
) -> std::result::Result<(Vec<Column>, Vec<String>), String> {
    let found = frame_values::of_given(frame, mds, object, arrays).map_err(|why| match why {
        // **Only what hangs on what the rows are**: said below for a property
        // that hangs on the other of the two, and here for one that hangs on
        // neither.
        frame_values::Refusal::Element { property, element } => {
            hangs_elsewhere(&property, element, by_passes)
        }
        other => other.to_string(),
    })?;
    let title = found.title.clone();
    let column = heading(&title, well);

    // **Only what hangs on what the rows are.** An interval property has one
    // fewer value than there are nodes, and putting it on the nodes would shift
    // every value by one rank - the fault `properties-from-table` exists to
    // avoid. The reverse too: a node value has no row in a table of passes.
    if found.support.is_intervals() != by_passes {
        return Err(hangs_elsewhere(&title, found.element, by_passes));
    }
    let rows = found.values.len();

    let kind = found.kind.as_ref().and_then(kind_uuid);
    let kind_title = found.kind.as_ref().and_then(|kind| match kind {
        property::PropertyKindRef::Object { title, .. } => title.clone(),
        property::PropertyKindRef::Standard { name } => Some(name.clone()),
    });

    // The label rather than the code, when a table gives one. **A property that
    // names a lookup it cannot be read through was refused above**, not written
    // as raw codes: the module doc says an integer looks like a measurement, so
    // a column of codes under a label's heading would be worse than an absence.
    let Some(categories) = &found.categories else {
        let cells = found
            .values
            .iter()
            .map(|value| value.as_ref().and_then(value_text))
            .collect();
        return Ok((
            vec![Column {
                title: column,
                uom: found.uom.clone(),
                kind,
                kind_title,
                from: From::Property(found.property.clone()),
                values: cells,
            }],
            Vec::new(),
        ));
    };

    // The table's columns that are not keys: the label alone most of the time,
    // the code and the label when the table groups both.
    let table_columns = categories.value_columns();

    // **A code no row names is an absence, said in the notes**, never the raw
    // number written under the label's heading. A code that is the array's
    // `NullValue` is an absence too, without a note: that is what it is for.
    let mut said = Vec::new();
    let unmatched: Vec<String> = found
        .values
        .iter()
        .flatten()
        .filter(|value| categories.row_of(value).is_none())
        .map(Value::to_string)
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

    if table_columns.len() > 1 {
        let made = table_columns
            .iter()
            .enumerate()
            .map(|(rank, &at_column)| Column {
                title: categories.lookup.titles[at_column]
                    .clone()
                    .unwrap_or_else(|| format!("{column} {}", rank + 1)),
                uom: None,
                // The property's kind goes to the **first** column, the code:
                // it is what the property measures. The following ones give its
                // meaning and have no kind of their own here.
                kind: if rank == 0 { kind.clone() } else { None },
                kind_title: if rank == 0 { kind_title.clone() } else { None },
                from: From::Property(found.property.clone()),
                values: found
                    .values
                    .iter()
                    .map(|value| {
                        value
                            .as_ref()
                            .and_then(|value| cell_of(categories, value, at_column))
                    })
                    .collect(),
            })
            .collect();
        return Ok((made, said));
    }

    let cells = (0..rows)
        .map(|at| {
            found
                .values
                .get(at)
                .and_then(Option::as_ref)
                .and_then(|value| label_of(categories, value))
        })
        .collect();

    Ok((
        vec![Column {
            title: column,
            uom: found.uom.clone(),
            kind,
            kind_title,
            from: From::Property(found.property.clone()),
            values: cells,
        }],
        said,
    ))
}

/// The note of a property that hangs on what the rows are not.
fn hangs_elsewhere(title: &str, element: IndexableElement, by_passes: bool) -> String {
    format!(
        "`{title}` hangs on {} while this table has one row per {}, so it has no row here",
        element.as_str(),
        if by_passes { "interval" } else { "node" }
    )
}

/// A measured depth, as text - and `None` for what the file does not say.
fn md_text(mds: &[f64], at: usize) -> Option<String> {
    let one = *mds.get(at)?;
    one.is_finite().then(|| trim(one))
}

/// A stored value as text, `None` for what the file does not say.
///
/// A `NaN` is **an absence**, not the string `NaN`: that is how properties
/// created from a table write a node without a measurement, and rendering it
/// literally would turn a blank into a word. ([`crate::frame_values`] made it
/// `None` already.)
fn value_text(value: &Value) -> Option<String> {
    match value {
        Value::Number(number) => Some(trim(*number)),
        Value::Code(code) => Some(code.to_string()),
        Value::Boolean(flag) => Some(flag.to_string()),
        Value::Text(text) => (!text.is_empty()).then(|| text.clone()),
    }
}

/// The cell of this table column, at the row the code designates.
fn cell_of(categories: &Categories, value: &Value, column: usize) -> Option<String> {
    let row = categories.row_of(value)?;
    match categories.lookup.rows.cell(row, column)? {
        Cell::Text(text) => Some(text),
        Cell::Number(number) => Some(trim(number)),
        Cell::Boolean(flag) => Some(flag.to_string()),
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
/// that column - it *is* the label. (The row is named by
/// [`Categories::row_of`]: by the key, by the rank in a table that has none.)
///
/// This is not [`Categories::label_column`] (the column of kind `code`, else the
/// only text column): a table whose only value column is a number is written
/// with that number here, as it always was.
fn label_of(categories: &Categories, value: &Value) -> Option<String> {
    let row = categories.row_of(value)?;
    let column = categories.value_columns().last().copied()?;
    match categories.lookup.rows.cell(row, column)? {
        Cell::Text(text) => Some(text),
        Cell::Number(number) => Some(trim(number)),
        Cell::Boolean(flag) => Some(flag.to_string()),
    }
}

// A number written without useless zeros - `6.4` and not `6.4000000000000004`:
// the one copy, in core.
use energyml_core::text::plain_number as trim;

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
    use crate::table::{ColumnMeta, Lookup, Rows};
    use crate::values::Values;

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

    /// The table, as `frame_values` holds it: with its rule for naming a row.
    fn categories(key: Option<Vec<i64>>, labels: Vec<&str>) -> Categories {
        let table = lookup(key, labels);
        let columns: Vec<ColumnMeta> = table
            .titles
            .iter()
            .enumerate()
            .map(|(at, title)| ColumnMeta {
                title: title.clone(),
                description: None,
                uom: None,
                kind: None,
                key: table.key_columns.contains(&at),
                values_per_row: 1,
            })
            .collect();
        Categories::new(String::new(), "ColumnBasedTable".to_string(), table, columns)
    }

    #[test]
    fn a_keyed_category_table_names_its_row_by_key() {
        // Common TRG 2.3 §9.1 p. 190: a key column is what makes the table a
        // lookup; a code names its row by that key, never by its position.
        let table = categories(Some(vec![10, 20, 30]), vec!["a", "b", "c"]);
        assert_eq!(label_of(&table, &Value::Code(20)).as_deref(), Some("b"));
        assert_eq!(
            label_of(&table, &Value::Code(1)),
            None,
            "a keyed table is never read by rank"
        );
    }

    #[test]
    fn a_keyless_category_table_names_its_row_by_rank() {
        let table = categories(None, vec!["a", "b", "c"]);
        assert_eq!(label_of(&table, &Value::Code(1)).as_deref(), Some("b"));
        assert_eq!(label_of(&table, &Value::Code(3)), None);
    }
}
