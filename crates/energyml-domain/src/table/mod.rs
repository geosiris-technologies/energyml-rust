//! Lookup tables, in the two shapes the standard has had.
//!
//! # What a lookup is for
//!
//! A discrete or categorical property stores integers, and those integers mean
//! something: facies 3 is *sandstone*, and a saturation of 0.2 has a relative
//! permeability of 0.05. The table that says so is a separate object, and the
//! property points at it.
//!
//! # Two shapes, one view
//!
//! | | RESQML 2.0.1 | RESQML 2.2 |
//! |---|---|---|
//! | the object | `StringTableLookup`, `DoubleTableLookup` | `eml:ColumnBasedTable` |
//! | the link | `CategoricalProperty.Lookup`, 1..1 | `DiscreteProperty.CategoryLookup`, 0..1 |
//! | the values | pairs, **inline in the XML** | columns, usually in an external file |
//! | how many columns | exactly two: key and value | any number, several of them keys |
//!
//! The two are read into the same [`Table`], so a caller asking "what does 3
//! mean here?" writes it once. What is *not* smoothed over is where the values
//! live: a 2.0.1 lookup is in the document, a column-based table is behind
//! [`crate::values`] and may need an HDF5 backend.
//!
//! # The rules that are normative, and the ones that are ours
//!
//! **Row identity is positional, and that is in the specification** - common
//! TRG §9.1, on `Column.Values`: *"Ordering matters since it will be the same
//! in all columns allowing to map a column value to another one."* Row *i* is
//! index *i* in every column, key columns included.
//!
//! **A key column makes it a lookup** - same section: *"Defines a key column
//! which make the table a lookup table."*
//!
//! **`CategoryLookup` points at a `ColumnBasedTable`** - the XSD lost the
//! association (an untyped `DataObjectReference` with no annotation), both
//! technical reference guides state it. A mixed package may still hold 2.0.1
//! lookups beside it, so the target type is dispatched on and anything else is
//! **refused by name**.
//!
//! What the specification does *not* say: nothing requires keys to be unique,
//! nothing requires them sorted, and nothing says how several key columns
//! compose. So: linear search on the tuple of key columns, **first matching
//! row wins**, and duplicate keys are **reported** rather than silently
//! chosen between.
//!
//! # A trap the corpus contains
//!
//! In `80wells`, the table's three value columns point at `column1`, `column2`
//! and `column3`, and its **key** column points at `column0` - the key is
//! serialized last in the XML and stored first in the file. Deriving the
//! dataset order from the document order gets every value wrong while looking
//! perfectly healthy.
//!
//! # Layout
//!
//! This file keeps the table itself (`Table`, `Column`, `Rows`, `Cell`,
//! `Lookup`) and the reading of the two shapes. The rest is split by trade, and
//! every public name is re-exported here, so `energyml_domain::table::X` is the
//! path whichever file holds `X`:
//!
//! | file | what it holds |
//! |---|---|
//! | `cells` | a cell's four states, `ValueCountPerRow`, the row count |
//! | `sheet` | a table without its values, kept by the package index |
//! | `class` | the class of a column, the MD columns, the curve pairs |
//! | `links` | the link a column declares, by convention |
//! | `keys` | the key index, the one rule "by the key, else by the rank" |
//! | `resolve` | a link cell resolved to a row |
//! | `check` | the links of a table or a set, checked |
//! | `window` | sort, filter and page a big table |

pub mod cells;
pub mod check;
pub mod class;
pub mod keys;
pub mod links;
pub mod resolve;
pub mod sheet;
pub mod window;

pub use cells::{CellState, ColumnCells, TableRefusal, row_count};
pub use check::{ColumnReport, LinkReport, UriColumnReport, check_links, check_links_of_set};
pub use class::{
    ColumnClass, DESCRIPTION, DepthKind, EXPLICIT_INDEX, IDENTIFIER, IntervalColumns,
    MEASURED_DEPTH, TRUE_VERTICAL_DEPTH, classify_column, curve_pairs, descends_from, md_columns,
    resolve_kinds,
};
pub use keys::{KeyIndex, NamingRefusal, RowMatch, row_named_by};
pub use links::{
    Convention, LinkDeclaration, LinkReader, LinkReaders, LinkRefusal, LinkTarget, LinkWarning,
    UriColumn, uri_column,
};
pub use resolve::{DanglingWhy, Resolved, TableSet, TargetIndexes, TargetTable, resolve};
pub use sheet::{ColumnSheet, TableSheet};
pub use window::{
    ColumnAccess, RowFilter, SortKey, SortOrder, TableWindow, WindowRefusal,
};

use energyml_core::identity;
use energyml_core::value::{FieldValue, Reflect, Scalar};

use crate::error::{Error, Result};
use crate::property::{Facet, PropertyKindRef, read_facets};
use crate::source::ArraySource;
use crate::values::{self, Values};
// The one arithmetic of "the values of a row", written in `cells`.
use cells::span;

/// One column of a table.
pub struct Column<'a> {
    /// `Title`, which is what a person reads.
    pub title: Option<String>,
    /// `Description`.
    pub description: Option<String>,
    /// The column's own unit, which **overrides** the one its property kind
    /// implies - the one override the standard does state.
    pub uom: Option<String>,
    /// What the column measures. Mandatory in a `ColumnBasedTable`; absent in
    /// a 2.0.1 lookup, which has no notion of it.
    pub kind: Option<PropertyKindRef>,
    /// Values per row: 1 for a scalar column, more for a vector one. With `n`
    /// greater than 1, the fastest dimension of the array is `n`.
    pub values_per_row: u64,
    /// Qualifiers on the column.
    pub facets: Vec<Facet>,
    /// Whether this is a `KeyColumn` - what makes the table a lookup.
    pub key: bool,
    /// The array, in whichever of the eighteen forms it is. `None` for a
    /// 2.0.1 lookup, whose values are inline pairs rather than an array.
    pub array: Option<&'a dyn Reflect>,
    /// Values read straight from the document, for the 2.0.1 pairs.
    pub inline: Option<Values>,
}

impl Column<'_> {
    /// The values of this column.
    ///
    /// Reads the array where there is one, and hands back the inline pairs
    /// where there is not - the caller does not have to know which shape the
    /// table had.
    pub fn read(&self, source: &dyn ArraySource) -> Result<Values> {
        if let Some(inline) = &self.inline {
            return Ok(inline.clone());
        }
        let array = self.array.ok_or_else(|| {
            Error::malformed(
                self.title.as_deref().unwrap_or("a column"),
                "carries neither an array nor inline values, so it has no contents at all",
            )
        })?;
        values::read(array, source, self.title.as_deref().unwrap_or("column"))
    }
}

/// A lookup table, whichever shape it was written in.
pub struct Table<'a> {
    /// Uuid of the table object.
    pub uuid: String,
    /// `Citation.Title`.
    pub title: Option<String>,
    /// Qualified type of the object it was read from, so a caller can say
    /// which shape it got.
    pub qualified_type: String,
    /// Key columns first, in document order, then the value columns.
    ///
    /// Ordered this way on purpose: a lookup is *searched* by its keys, and
    /// putting them first makes the tuple of keys the head of every row.
    pub columns: Vec<Column<'a>>,
}

impl<'a> Table<'a> {
    /// Read a lookup object.
    ///
    /// Accepts `eml23.ColumnBasedTable` - the normative target of
    /// `CategoryLookup` - and the two RESQML 2.0.1 lookups, which a mixed
    /// package may still carry. Anything else is refused **naming the type it
    /// found**, because "not a table" without the name is the kind of message
    /// that sends someone reading source code.
    pub fn read(object: &'a dyn Reflect) -> Result<Table<'a>> {
        let name = object.type_info().name;
        let kind = name.trim_start_matches("obj_");
        let uuid = identity::uuid_of_reflect(object)
            .ok_or_else(|| Error::malformed(name, "a lookup table with no uuid"))?
            .to_string();
        let title = object
            .field("Citation")
            .and_then(|f| f.as_object())
            .and_then(|citation| text(citation, "Title"));
        let qualified_type = object.type_info().qualified_type.unwrap_or(name).to_string();

        let columns = match kind {
            "ColumnBasedTable" => read_columns(object),
            "StringTableLookup" | "DoubleTableLookup" => read_pairs(object, &uuid, kind)?,
            other => {
                return Err(Error::unsupported(
                    format!("`{other}` as a lookup table"),
                    format!(
                        "a `CategoryLookup` points at an `eml23.ColumnBasedTable` (both technical \
                         reference guides say so), and a mixed package may carry a RESQML 2.0.1 \
                         `StringTableLookup` or `DoubleTableLookup`; `{uuid}` is neither"
                    ),
                ));
            }
        };

        if columns.is_empty() {
            return Err(Error::malformed(
                &uuid,
                "a lookup table with no column at all; the schema requires at least one",
            ));
        }

        Ok(Table {
            uuid,
            title,
            qualified_type,
            columns,
        })
    }

    /// The key columns - what makes it a lookup rather than a list.
    pub fn keys(&self) -> Vec<&Column<'a>> {
        self.columns.iter().filter(|column| column.key).collect()
    }

    /// The columns that are not keys.
    pub fn values(&self) -> Vec<&Column<'a>> {
        self.columns.iter().filter(|column| !column.key).collect()
    }

    /// Read every column, with the row count checked across them.
    ///
    /// Row identity is positional and normative, so columns of different
    /// lengths are a real defect - reported, never repaired by truncating in
    /// silence, because truncating shifts every row of the shorter column
    /// against the longer.
    ///
    /// A column that cannot be read does **not** silence the table: it comes
    /// back as `None`, named in the notes with the reader's own words, and the
    /// row count is taken across the columns that read. The real case is a
    /// `ColumnBasedTable` with one external column in a build - or a browser -
    /// without the array backend: its string columns are right there in the
    /// XML, and refusing them because a neighbouring column lives in a file
    /// nobody can open turned a readable table into nothing, silently.
    /// (`Result` is kept for the callers' sake; this path no longer fails.)
    pub fn read_all(&self, source: &dyn ArraySource) -> Result<Rows> {
        let mut columns = Vec::new();
        let mut notes = Vec::new();
        for column in &self.columns {
            match column.read(source) {
                Ok(values) => columns.push(Some(values)),
                Err(cause) => {
                    notes.push(format!(
                        "column `{}` could not be read: {cause}",
                        column.title.as_deref().unwrap_or("untitled")
                    ));
                    columns.push(None);
                }
            }
        }
        let per_row: Vec<u64> = self.columns.iter().map(|c| c.values_per_row.max(1)).collect();
        // Rows per column that read: a column of `ValueCountPerRow` values per
        // row holds `len / n` rows (common TRG 2.3 §9.1 p. 190).
        let mut row_counts: Vec<usize> = Vec::new();
        for ((column, values), n) in self.columns.iter().zip(&columns).zip(&per_row) {
            let Some(values) = values else { continue };
            let n = *n as usize;
            let len = values.len();
            if len % n != 0 {
                notes.push(format!(
                    "column `{}` holds {len} value(s), which is not a whole number of rows of {n} \
                     (`ValueCountPerRow`; common TRG 2.3 §9.1: \"the fastest dimension of the \
                     column Values array must be this value\"); {} row(s) are read, the {} \
                     value(s) left over are not",
                    column.title.as_deref().unwrap_or("untitled"),
                    len / n,
                    len % n
                ));
            }
            row_counts.push(len / n);
        }
        let rows = row_counts.iter().copied().min().unwrap_or(0);
        if row_counts.iter().any(|count| *count != rows) {
            notes.push(format!(
                "the columns do not have the same number of rows ({}), and row identity is \
                 positional (common TRG §9.1: \"ordering matters since it will be the same in all \
                 columns\"); {rows} row(s) can be read across all of them, the rest of the longer \
                 columns is unmatched",
                row_counts
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let nulls: Vec<Option<i64>> = self
            .columns
            .iter()
            .map(|column| column.array.and_then(values::null_value))
            .collect();
        Ok(Rows {
            columns,
            per_row,
            nulls,
            rows,
            notes,
        })
    }
}

/// The values of a table, column by column, with the rows lined up.
pub struct Rows {
    /// One entry per column of the table, in the same order. `None` is a
    /// column that could not be read - named in the notes - and its cells
    /// answer `None`, which is not the same as a column of empty strings.
    pub columns: Vec<Option<Values>>,
    /// Values per row of each column, in the same order - `ValueCountPerRow`,
    /// 1 for a scalar column and for both columns of a 2.0.1 lookup.
    pub per_row: Vec<u64>,
    /// The `NullValue` each column's array declares, in the same order: `None`
    /// for a column in line, a floating-point or text column, or one that did
    /// not read. [`Rows::cells`] gives it to [`ColumnCells`].
    pub nulls: Vec<Option<i64>>,
    /// How many rows can be read across every column **that read**.
    pub rows: usize,
    /// What did not line up, and what could not be read.
    pub notes: Vec<String>,
}

impl Rows {
    /// One column as cells, borrowing its values: what [`ColumnCells`] cuts
    /// into rows with `ValueCountPerRow` and the array's `NullValue`. `None` for
    /// a column that could not be read and for one past the table.
    pub fn cells(&self, column: usize) -> Option<ColumnCells<'_>> {
        let values = self.columns.get(column)?.as_ref()?;
        Some(ColumnCells {
            values: std::borrow::Cow::Borrowed(values),
            per_row: self.per_row_of(column),
            null: self.nulls.get(column).copied().flatten(),
        })
    }

    /// Values per row of a column; 1 when the column is not known.
    fn per_row_of(&self, column: usize) -> u64 {
        self.per_row.get(column).copied().unwrap_or(1).max(1)
    }

    /// One cell of a **scalar** column, by row and column index; `None` for a
    /// column of several values per row (read it with [`Rows::values_of`]),
    /// for an unreadable column, and past its end.
    pub fn cell(&self, row: usize, column: usize) -> Option<Cell> {
        if self.per_row_of(column) != 1 {
            return None;
        }
        Cell::at(self.columns.get(column)?.as_ref()?, row)
    }

    /// The `n` values of one row of a column, in storage order. `None` when
    /// unreadable or past the end.
    pub fn values_of(&self, row: usize, column: usize) -> Option<Vec<Cell>> {
        let values = self.columns.get(column)?.as_ref()?;
        span(self.per_row_of(column), row)
            .map(|at| Cell::at(values, at))
            .collect::<Option<Vec<_>>>()
    }

    /// The whole of one row, in column order, scalar cells (`cell` per column).
    pub fn row(&self, row: usize) -> Vec<Option<Cell>> {
        (0..self.columns.len())
            .map(|column| self.cell(row, column))
            .collect()
    }

    /// The whole of one row, every column as its values.
    pub fn row_values(&self, row: usize) -> Vec<Option<Vec<Cell>>> {
        (0..self.columns.len())
            .map(|column| self.values_of(row, column))
            .collect()
    }
}

/// One cell of a table, compared the way a lookup needs.
#[derive(Clone, PartialEq, Debug)]
pub enum Cell {
    /// A number, whatever width it was stored at.
    Number(f64),
    /// A truth value.
    Boolean(bool),
    /// Text.
    Text(String),
}

impl Cell {
    fn at(values: &Values, row: usize) -> Option<Cell> {
        Some(match values {
            Values::Floating(v) => Cell::Number(*v.get(row)?),
            Values::Integer(v) => Cell::Number(*v.get(row)? as f64),
            Values::Boolean(v) => Cell::Boolean(*v.get(row)?),
            Values::Text(v) => Cell::Text(v.get(row)?.clone()),
        })
    }

    /// Whether two cells are the same key.
    ///
    /// Numbers are compared as numbers whatever they were stored as - a key
    /// column of `int32` and a query of `3.0` are the same key - and **exactly**:
    /// `-0.0` is `0.0` and a `NaN` is never a key. A hashed index needs an
    /// equivalence relation, and a tolerance is not transitive (the former
    /// `f64::EPSILON` window only ever applied under 1 in absolute value).
    /// Text is compared exactly, because a facies name is a name.
    pub fn matches(&self, other: &Cell) -> bool {
        match (self, other) {
            (Cell::Number(a), Cell::Number(b)) => a == b,
            (Cell::Boolean(a), Cell::Boolean(b)) => a == b,
            (Cell::Text(a), Cell::Text(b)) => a == b,
            // A number against text is not a mismatch of value but of column;
            // saying "no" here is right, and the caller sees an empty result
            // rather than a wrong row.
            _ => false,
        }
    }
}

impl std::fmt::Display for Cell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Cell::Number(v) => write!(f, "{v}"),
            Cell::Boolean(v) => write!(f, "{v}"),
            Cell::Text(v) => write!(f, "{v}"),
        }
    }
}

/// A table with its values read, ready to be searched.
pub struct Lookup {
    /// Column titles, in the table's order - keys first.
    pub titles: Vec<Option<String>>,
    /// Which of them are keys.
    pub key_columns: Vec<usize>,
    /// The values.
    pub rows: Rows,
    /// What was wrong with the table.
    pub notes: Vec<String>,
}

impl Lookup {
    /// Read a table and its values in one go.
    pub fn read(object: &dyn Reflect, source: &dyn ArraySource) -> Result<Lookup> {
        let table = Table::read(object)?;
        let mut rows = table.read_all(source)?;
        let notes = std::mem::take(&mut rows.notes);
        Ok(Lookup {
            titles: table.columns.iter().map(|c| c.title.clone()).collect(),
            key_columns: table
                .columns
                .iter()
                .enumerate()
                .filter(|(_, c)| c.key)
                .map(|(at, _)| at)
                .collect(),
            rows,
            notes,
        })
    }

    /// The row whose key columns hold this tuple, and every duplicate of it.
    ///
    /// The first element is the row a caller should use - *first match wins*,
    /// which is this repository's rule and not the standard's, because nothing
    /// anywhere requires keys to be unique. Anything after the first is a
    /// **duplicate key**, handed back rather than hidden, so a caller can say
    /// so instead of quietly using one of them.
    ///
    /// A key column of several values per row finds nothing: a key is one
    /// value per row.
    ///
    /// A caller that looks up several keys builds the index once
    /// ([`Lookup::key_index`]) and asks it.
    pub fn find(&self, key: &[Cell]) -> Vec<usize> {
        self.key_index().rows_of(key).to_vec()
    }

    /// What a single-key lookup maps a number to: the other columns of the
    /// first matching row.
    ///
    /// The common case - facies 3 → its name, saturation → its permeability -
    /// without the caller building a tuple.
    pub fn of(&self, key: f64) -> Option<Vec<Option<Cell>>> {
        let found = self.find(&[Cell::Number(key)]);
        let row = *found.first()?;
        Some(
            (0..self.rows.columns.len())
                .filter(|column| !self.key_columns.contains(column))
                .map(|column| self.rows.cell(row, column))
                .collect(),
        )
    }

    /// Every key that appears more than once, with the rows carrying it.
    ///
    /// Not an error - nothing forbids it - but a lookup with duplicate keys
    /// answers arbitrarily, and an application that cares should be able to
    /// find out before it does.
    pub fn duplicate_keys(&self) -> Vec<(Vec<Cell>, Vec<usize>)> {
        self.key_index().duplicates()
    }
}

// ---------------------------------------------------------------------------
// Reading the two shapes
// ---------------------------------------------------------------------------

/// `ColumnBasedTable`: keys first, then the value columns.
fn read_columns(object: &dyn Reflect) -> Vec<Column<'_>> {
    let mut columns = Vec::new();
    for (field, key) in [("KeyColumn", true), ("Column", false)] {
        for item in list(object, field) {
            let Some(column) = item.as_object() else { continue };
            columns.push(Column {
                title: text(column, "Title"),
                description: text(column, "Description"),
                // The one override the standard states: a column's unit beats
                // the one its property kind implies.
                uom: text(column, "Uom"),
                kind: column
                    .field("PropertyKind")
                    .and_then(|f| f.as_object())
                    .and_then(|reference| {
                        Some(PropertyKindRef::Object {
                            uuid: uuid_of_reference(reference)?,
                            title: text(reference, "Title"),
                        })
                    }),
                values_per_row: scalar_u64(column, "ValueCountPerRow").unwrap_or(1).max(1),
                facets: read_facets(column),
                key,
                array: column.field("Values").and_then(|f| f.as_object()),
                inline: None,
            });
        }
    }
    columns
}

/// A 2.0.1 lookup: pairs in the document, turned into two columns.
///
/// Key and value, in that order, so the shape a caller sees is the same as a
/// column-based table's - which is the whole point of reading them into one
/// type.
fn read_pairs<'a>(object: &dyn Reflect, uuid: &str, kind: &str) -> Result<Vec<Column<'a>>> {
    let mut keys_float: Vec<f64> = Vec::new();
    let mut keys_integer: Vec<i64> = Vec::new();
    let mut values_numeric = Vec::new();
    let mut values_text = Vec::new();
    let mut textual = false;

    for (at, item) in list(object, "Value").into_iter().enumerate() {
        let Some(pair) = item.as_object() else { continue };
        let key = pair.field("Key").and_then(|f| f.as_scalar());
        let Some(key) = key else {
            return Err(Error::malformed(
                uuid,
                format!("pair {at} has no `Key`, so it maps nothing"),
            ));
        };
        if kind == "DoubleTableLookup" {
            // `DoubleLookup.Key` is a double (RESQML TRG 2.0.1 §6.24 p. 72):
            // its fraction is part of the key.
            let Some(key) = key.as_f64() else {
                return Err(Error::malformed(
                    uuid,
                    format!("pair {at} has a `Key` that is not a number, so it maps nothing"),
                ));
            };
            keys_float.push(key);
        } else {
            // `StringLookup.Key` is an integer (§6.40 p. 79).
            let integer = match &key {
                Scalar::Int(v) => Some(*v),
                Scalar::UInt(v) => i64::try_from(*v).ok(),
                Scalar::Double(v)
                    if v.is_finite()
                        && v.fract() == 0.0
                        && *v >= i64::MIN as f64
                        && *v <= i64::MAX as f64 =>
                {
                    Some(*v as i64)
                }
                _ => None,
            };
            let Some(integer) = integer else {
                return Err(Error::malformed(
                    uuid,
                    format!(
                        "pair {at} has the key {key}, and a `StringTableLookup` key is an integer (`StringLookup.Key`)"
                    ),
                ));
            };
            keys_integer.push(integer);
        }

        match pair.field("Value").and_then(|f| f.as_scalar()) {
            Some(Scalar::Str(text)) => {
                textual = true;
                values_text.push(text.to_string());
            }
            Some(other) => values_numeric.push(other.as_f64().unwrap_or(f64::NAN)),
            None => {
                return Err(Error::malformed(
                    uuid,
                    format!("pair {at} has no `Value`, so it maps to nothing"),
                ));
            }
        }
    }

    Ok(vec![
        Column {
            title: Some("Key".to_string()),
            description: None,
            uom: None,
            kind: None,
            values_per_row: 1,
            facets: Vec::new(),
            key: true,
            array: None,
            inline: Some(if kind == "DoubleTableLookup" {
                Values::Floating(keys_float)
            } else {
                Values::Integer(keys_integer)
            }),
        },
        Column {
            title: Some("Value".to_string()),
            description: None,
            uom: None,
            kind: None,
            values_per_row: 1,
            facets: Vec::new(),
            key: false,
            array: None,
            inline: Some(if textual {
                Values::Text(values_text)
            } else {
                Values::Floating(values_numeric)
            }),
        },
    ])
}

// ---------------------------------------------------------------------------
// Reflection helpers
// ---------------------------------------------------------------------------

fn list<'a>(object: &'a dyn Reflect, field: &str) -> Vec<FieldValue<'a>> {
    match object.field(field) {
        Some(FieldValue::List(items)) => items,
        Some(FieldValue::Null) | None => Vec::new(),
        Some(other) => vec![other],
    }
}

fn text(object: &dyn Reflect, field: &str) -> Option<String> {
    match object.field(field)?.as_scalar()? {
        Scalar::Str(s) if !s.is_empty() => Some(s.to_string()),
        Scalar::Str(_) => None,
        other => Some(other.to_string()),
    }
}

fn scalar_u64(object: &dyn Reflect, field: &str) -> Option<u64> {
    match object.field(field)?.as_scalar()? {
        Scalar::UInt(v) => Some(v),
        Scalar::Int(v) if v >= 0 => Some(v as u64),
        _ => None,
    }
}

/// The uuid a reference names - the identity layer's reading, so every
/// spelling of the field is one answer.
fn uuid_of_reference(object: &dyn Reflect) -> Option<String> {
    energyml_core::identity::uuid_of_reflect(object)
        .filter(|uuid| !uuid.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(keys: Values, values: Values) -> Lookup {
        Lookup {
            titles: vec![Some("Key".into()), Some("Value".into())],
            key_columns: vec![0],
            rows: Rows {
                rows: keys.len().min(values.len()),
                columns: vec![Some(keys), Some(values)],
                per_row: vec![1, 1],
                nulls: vec![None, None],
                notes: Vec::new(),
            },
            notes: Vec::new(),
        }
    }

    #[test]
    fn an_unreadable_column_is_named_and_the_others_still_answer() {
        // The real case: a `ColumnBasedTable` whose string columns are right
        // there in the XML, next to one external column in a build without the
        // array backend. Failing the whole table turned readable rows into
        // nothing, silently - the exact opposite of "half an index beats
        // none". The broken column's cells answer `None`, which is not a
        // column of empty strings.
        let readable = Column {
            title: Some("code".into()),
            description: None,
            uom: None,
            kind: None,
            values_per_row: 1,
            facets: Vec::new(),
            key: true,
            array: None,
            inline: Some(Values::Text(vec!["e6".into(), "e5".into()])),
        };
        let broken = Column {
            title: Some("épaisseur".into()),
            description: None,
            uom: Some("m".into()),
            kind: None,
            values_per_row: 1,
            facets: Vec::new(),
            key: false,
            array: None,
            inline: None, // neither an array nor inline values: `read` refuses
        };
        let table = Table {
            uuid: "t1".into(),
            title: None,
            qualified_type: "eml23.ColumnBasedTable".into(),
            columns: vec![readable, broken],
        };
        let rows = table
            .read_all(&crate::source::NoObjects)
            .expect("the table still answers");
        assert_eq!(rows.rows, 2, "the readable column decides the row count");
        assert_eq!(rows.cell(0, 0), Some(Cell::Text("e6".into())));
        assert_eq!(rows.cell(0, 1), None, "an unreadable cell is None, not empty");
        assert!(
            rows.notes.iter().any(|note| note.contains("épaisseur")),
            "the broken column is named: {:?}",
            rows.notes
        );
    }

    #[test]
    fn a_key_finds_its_row_whatever_width_it_was_stored_at() {
        // An `int32` key column and a query of 3.0 are the same key: the
        // reader widens, so the comparison has to be numeric.
        let table = lookup(
            Values::Integer(vec![1, 2, 3]),
            Values::Text(vec!["shale".into(), "sand".into(), "silt".into()]),
        );
        assert_eq!(table.of(3.0).unwrap(), vec![Some(Cell::Text("silt".to_string()))]);
        assert_eq!(table.of(9.0), None, "a key that is not there maps to nothing");
    }

    #[test]
    fn a_duplicate_key_is_reported_rather_than_chosen_between() {
        // Nothing in the standard forbids it, so it is not an error - but a
        // caller must be able to find out before trusting the answer.
        let table = lookup(
            Values::Integer(vec![1, 2, 2]),
            Values::Text(vec!["a".into(), "b".into(), "c".into()]),
        );
        assert_eq!(
            table.of(2.0).unwrap(),
            vec![Some(Cell::Text("b".to_string()))],
            "first match wins"
        );
        assert_eq!(table.find(&[Cell::Number(2.0)]), vec![1, 2]);
        let duplicates = table.duplicate_keys();
        assert_eq!(duplicates.len(), 1);
        assert_eq!(duplicates[0].1, vec![1, 2]);
    }

    #[test]
    fn a_wrong_shaped_key_finds_nothing_rather_than_the_wrong_row() {
        let table = lookup(
            Values::Integer(vec![1, 2]),
            Values::Text(vec!["a".into(), "b".into()]),
        );
        assert!(table.find(&[Cell::Text("1".into())]).is_empty());
        assert!(
            table.find(&[Cell::Number(1.0), Cell::Number(2.0)]).is_empty(),
            "two keys against a one-key table is a question the table cannot answer"
        );
    }

    #[test]
    fn columns_of_different_lengths_are_reported_and_the_rows_that_line_up_are_kept() {
        let rows = Rows {
            columns: vec![
                Some(Values::Integer(vec![1, 2, 3])),
                Some(Values::Floating(vec![0.1, 0.2])),
            ],
            per_row: vec![1, 1],
            nulls: vec![None, None],
            rows: 2,
            notes: Vec::new(),
        };
        assert_eq!(rows.cell(1, 1), Some(Cell::Number(0.2)));
        assert_eq!(rows.cell(2, 1), None, "the third row has no second column");
    }
}
