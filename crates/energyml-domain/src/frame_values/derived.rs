//! A numeric column of a category table, as a property of its own.
//!
//! A table that names its categories can say more than their names: each facies
//! its porosity, each unit its density. The property stores the **code**; the
//! column is read through it, row by row, and the result is a continuous
//! property of the same support, in the unit and of the kind **the column**
//! states - without writing anything. Nothing is guessed on the way: a code that
//! names no row, a table that repeats a key, a column that is not a plain number
//! each refuse by name.

use crate::table::Cell;
use crate::values::Values;

use super::{FrameValues, Value};

/// What a derived property was made from.
#[derive(Clone, Debug, PartialEq)]
pub struct Derivation {
    /// The uuid of the property whose codes were read.
    pub property: String,
    /// The uuid of the table.
    pub table: String,
    /// The column of the table, by position.
    pub column: usize,
    /// The column's title.
    pub title: Option<String>,
}

/// Why a column cannot be turned into a property.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum DeriveRefusal {
    /// The property names no table.
    #[error("`{property}` names no category table, so there is no column to derive a property from")]
    NoTable {
        /// The property's title.
        property: String,
    },
    /// The table is a function of a real key, not a list of categories.
    #[error(
        "the table is a `{table_type}`: a function of a real key, read between its keys, not a list of categories whose columns are read per code"
    )]
    FunctionTable {
        /// The type of the table.
        table_type: String,
    },
    /// The table has no such column.
    #[error("the table has no column {column}: it has {columns}")]
    NoSuchColumn {
        /// The column asked for.
        column: usize,
        /// How many columns the table has.
        columns: usize,
    },
    /// The column holds several values per row.
    #[error(
        "column `{title}` holds {per_row} values per row, and a derived property has one value per element"
    )]
    SeveralValuesPerRow {
        /// The column's title.
        title: String,
        /// `ValueCountPerRow`.
        per_row: u64,
    },
    /// The column could not be read.
    #[error("column `{title}` could not be read")]
    Unreadable {
        /// The column's title.
        title: String,
    },
    /// The column holds text or truth values.
    #[error(
        "column `{title}` is not numeric (it holds text or truth values), and a derived property is a number"
    )]
    NotNumeric {
        /// The column's title.
        title: String,
    },
    /// The table repeats a key.
    #[error(
        "the table repeats the key {key} (rows {rows}): which row a code names is not decided, so nothing is derived"
    )]
    DuplicateKeys {
        /// The first key that is repeated.
        key: String,
        /// The rows that hold it, joined.
        rows: String,
    },
    /// A value of the property is not a code.
    #[error("the value at element {at} is a truth value or a text, not a code or a number")]
    NotACode {
        /// The element.
        at: usize,
    },
    /// A code names no row.
    #[error("the value {code} at element {at} names no row of the table, so its column has no value for it")]
    KeyAbsent {
        /// The element.
        at: usize,
        /// The code.
        code: String,
    },
}

impl FrameValues {
    /// A column of the table this property names, as a continuous property of the
    /// same support: element `i` is the cell of the row that its code names.
    ///
    /// The result takes the unit and the kind of the **column**, and a title that
    /// says what it is made of (`derived from <property> by <column>`); a cell
    /// that is absent in the column (`NullValue`, `NaN`) is an absent value, and
    /// a value that was absent stays so. Refusals: [`DeriveRefusal`].
    pub fn derive(&self, column: usize) -> Result<FrameValues, DeriveRefusal> {
        let Some(categories) = &self.categories else {
            return Err(DeriveRefusal::NoTable {
                property: self.title.clone(),
            });
        };
        if categories.table_type.ends_with("DoubleTableLookup") {
            return Err(DeriveRefusal::FunctionTable {
                table_type: categories.table_type.clone(),
            });
        }
        let Some(meta) = categories.columns.get(column) else {
            return Err(DeriveRefusal::NoSuchColumn {
                column,
                columns: categories.columns.len(),
            });
        };
        let title = meta.title.clone().unwrap_or_else(|| format!("column {column}"));
        if meta.values_per_row > 1 {
            return Err(DeriveRefusal::SeveralValuesPerRow {
                title,
                per_row: meta.values_per_row,
            });
        }
        match categories.lookup.rows.columns.get(column) {
            Some(Some(Values::Floating(_) | Values::Integer(_))) => {}
            Some(Some(_)) => return Err(DeriveRefusal::NotNumeric { title }),
            _ => return Err(DeriveRefusal::Unreadable { title }),
        }
        if let Some((key, rows)) = categories.duplicate_keys().into_iter().next() {
            return Err(DeriveRefusal::DuplicateKeys {
                key: key.iter().map(Cell::to_string).collect::<Vec<_>>().join(", "),
                rows: rows.iter().map(usize::to_string).collect::<Vec<_>>().join(", "),
            });
        }

        let mut derived = Vec::with_capacity(self.values.len());
        for (at, value) in self.values.iter().enumerate() {
            let Some(value) = value else {
                derived.push(None);
                continue;
            };
            if value.as_f64().is_none() {
                return Err(DeriveRefusal::NotACode { at });
            }
            let Some(row) = categories.row_of(value) else {
                return Err(DeriveRefusal::KeyAbsent {
                    at,
                    code: value.to_string(),
                });
            };
            derived.push(match categories.lookup.rows.cell(row, column) {
                Some(Cell::Number(number)) if number.is_finite() => Some(Value::Number(number)),
                _ => None,
            });
        }

        let range = FrameValues::range_of(&derived);
        let mut notes = self.notes.clone();
        notes.push(format!(
            "derived from `{}` by column `{title}` of table {}: nothing is stored",
            self.title, categories.table
        ));
        Ok(FrameValues {
            frame: self.frame.clone(),
            property: self.property.clone(),
            title: format!("derived from {} by {title}", self.title),
            element: self.element,
            support: self.support.clone(),
            values: derived,
            continuous: true,
            uom: meta.uom.clone(),
            kind: meta.kind.clone(),
            kind_uri: None,
            range,
            categories: None,
            time: self.time.clone(),
            realization: self.realization,
            derivation: Some(Derivation {
                property: self.property.clone(),
                table: categories.table.clone(),
                column,
                title: meta.title.clone(),
            }),
            notes,
        })
    }
}
