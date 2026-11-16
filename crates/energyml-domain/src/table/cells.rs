//! The cells of a table: what one row holds in one column.
//!
//! A cell has four states, not two: a value, an absence (`NullValue`, `NaN`), an
//! empty string, and a column that could not be read. The empty string is **a
//! value** (a `StringXmlArray` has no `NullValue`), never an absence. A column
//! with `ValueCountPerRow` greater than 1 holds several values per row (common
//! TRG 2.3 §9.1 p. 190).

use std::borrow::Cow;
use std::ops::Range;

use crate::source::ArraySource;
use crate::table::{Cell, Column, Table};
use crate::values::{Family, Values};

/// The values of one column, with what is needed to cut them into rows.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnCells<'a> {
    /// The column's values, flat, in the family the schema declares.
    pub values: Cow<'a, Values>,
    /// `ValueCountPerRow`: 1 for a scalar column (common TRG 2.3 §9.1 p. 190).
    pub per_row: u64,
    /// The array's `NullValue`, for an integer column that declares one.
    pub null: Option<i64>,
}

/// What one cell of a column holds.
#[derive(Clone, Debug, PartialEq)]
pub enum CellState {
    /// A value. `Cell::Text("")` never appears here: it is `Empty`.
    Value(Cell),
    /// A row of a vector column (`ValueCountPerRow > 1`): its `per_row` values in
    /// order; `None` is an absent component (`NullValue`, `NaN`).
    Vector(Vec<Option<Cell>>),
    /// No value: the integer equals the array's `NullValue`, or the float is `NaN`.
    Absent,
    /// An empty string - a value (a `StringXmlArray` has no `NullValue`).
    Empty,
    /// The column could not be read; the refusal says why.
    Unreadable,
}

/// Why a column, or the rows of a table, cannot be read as cells.
#[derive(Debug, thiserror::Error)]
pub enum TableRefusal {
    /// The column has `ValueCountPerRow = per_row` and a number of values that is
    /// not a multiple of it.
    #[error(
        "column `{column}` has {values} value(s), which is not a multiple of its ValueCountPerRow {per_row} (common TRG 2.3 §9.1 p. 190: the fastest dimension of the Values array must be this value)"
    )]
    VectorShape {
        /// The column's title, or `column <i>`.
        column: String,
        /// `ValueCountPerRow`.
        per_row: u64,
        /// How many values the column holds.
        values: usize,
    },
    /// The column's values could not be read; the reader's own words.
    #[error("column `{column}` could not be read: {cause}")]
    Unreadable {
        /// The column's title, or `column <i>`.
        column: String,
        /// The reader's refusal.
        #[source]
        cause: crate::Error,
    },
    /// The readable columns do not have the same number of rows.
    #[error(
        "the columns do not have the same number of rows ({}); row identity is positional (common TRG 2.3 §9.1 p. 190), {common} row(s) can be read across all of them",
        rows_text(.rows)
    )]
    RowCountsDisagree {
        /// Rows of each column, in the table's order; `None` for an unreadable one.
        rows: Vec<Option<usize>>,
        /// The rows every readable column holds.
        common: usize,
    },
    /// No column of the table could be read.
    #[error("no column of the table could be read")]
    NothingReadable,
}

fn rows_text(rows: &[Option<usize>]) -> String {
    rows.iter()
        .map(|count| match count {
            Some(count) => count.to_string(),
            None => "unreadable".to_string(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The values of row `row` in a column of `per_row` values per row:
/// `row*per_row .. row*per_row + per_row` (the row's values are the fastest
/// dimension, common TRG 2.3 §9.1 p. 190). The one place this arithmetic is
/// written.
pub(crate) fn span(per_row: u64, row: usize) -> Range<usize> {
    let n = per_row.max(1) as usize;
    let start = row.saturating_mul(n);
    start..start.saturating_add(n)
}

/// What one scalar slot of a column holds, before the row is assembled.
enum Slot {
    Absent,
    Empty,
    Value(Cell),
}

impl ColumnCells<'_> {
    /// Read a column's values and keep what cuts them into rows.
    pub fn read(column: &Column<'_>, source: &dyn ArraySource) -> Result<ColumnCells<'static>, TableRefusal> {
        let name = column
            .title
            .clone()
            .unwrap_or_else(|| "an untitled column".to_string());
        read_named(column, source, name)
    }

    /// How many rows this column holds: `values.len() / per_row`, refused when not a multiple.
    pub fn rows(&self, column: &str) -> Result<usize, TableRefusal> {
        let per_row = self.per_row.max(1) as usize;
        let len = self.values.len();
        if !len.is_multiple_of(per_row) {
            return Err(TableRefusal::VectorShape {
                column: column.to_string(),
                per_row: self.per_row,
                values: len,
            });
        }
        Ok(len / per_row)
    }

    /// The cell at `row`. `None` beyond the column.
    pub fn cell(&self, row: usize) -> Option<CellState> {
        let range = span(self.per_row, row);
        if range.end > self.values.len() {
            return None;
        }
        if self.per_row.max(1) == 1 {
            return Some(match self.slot(range.start)? {
                Slot::Absent => CellState::Absent,
                Slot::Empty => CellState::Empty,
                Slot::Value(cell) => CellState::Value(cell),
            });
        }
        let mut components = Vec::with_capacity(range.len());
        for at in range {
            components.push(match self.slot(at)? {
                Slot::Absent => None,
                // An empty component is a value too (S16).
                Slot::Empty => Some(Cell::Text(String::new())),
                Slot::Value(cell) => Some(cell),
            });
        }
        Some(CellState::Vector(components))
    }

    fn slot(&self, at: usize) -> Option<Slot> {
        Some(match &*self.values {
            Values::Floating(v) => {
                let x = *v.get(at)?;
                if x.is_nan() {
                    Slot::Absent
                } else {
                    Slot::Value(Cell::Number(x))
                }
            }
            Values::Integer(v) => {
                let i = *v.get(at)?;
                if Some(i) == self.null {
                    Slot::Absent
                } else {
                    Slot::Value(Cell::Number(i as f64))
                }
            }
            Values::Boolean(v) => Slot::Value(Cell::Boolean(*v.get(at)?)),
            Values::Text(v) => {
                let s = v.get(at)?;
                if s.is_empty() {
                    Slot::Empty
                } else {
                    Slot::Value(Cell::Text(s.clone()))
                }
            }
        })
    }

    /// The family of the values.
    pub fn family(&self) -> Family {
        self.values.family()
    }

    /// An owned copy (`Cow::Owned`).
    pub fn into_owned(self) -> ColumnCells<'static> {
        ColumnCells {
            values: Cow::Owned(self.values.into_owned()),
            per_row: self.per_row,
            null: self.null,
        }
    }
}

fn read_named(
    column: &Column<'_>,
    source: &dyn ArraySource,
    name: String,
) -> Result<ColumnCells<'static>, TableRefusal> {
    let per_row = column.values_per_row.max(1);
    let values = column.read(source).map_err(|cause| TableRefusal::Unreadable {
        column: name.clone(),
        cause,
    })?;
    let null = column.array.and_then(crate::values::null_value);
    if values.len() % per_row as usize != 0 {
        return Err(TableRefusal::VectorShape {
            column: name,
            per_row,
            values: values.len(),
        });
    }
    Ok(ColumnCells {
        values: Cow::Owned(values),
        per_row,
        null,
    })
}

impl Table<'_> {
    /// Every column as cells, in the table's order (keys first). One entry per column: a column
    /// that cannot be read is an `Err` naming it, never an empty column.
    pub fn read_cells(&self, source: &dyn ArraySource) -> Vec<Result<ColumnCells<'static>, TableRefusal>> {
        self.columns
            .iter()
            .enumerate()
            .map(|(at, column)| {
                let name = column.title.clone().unwrap_or_else(|| format!("column {at}"));
                read_named(column, source, name)
            })
            .collect()
    }
}

/// The number of rows of a table from its columns: every readable column must agree.
pub fn row_count(
    cells: &[Result<ColumnCells<'_>, TableRefusal>],
    titles: &[Option<String>],
) -> Result<usize, TableRefusal> {
    let mut counts: Vec<Option<usize>> = Vec::with_capacity(cells.len());
    for (at, cell) in cells.iter().enumerate() {
        match cell {
            Ok(column) => {
                let name = titles
                    .get(at)
                    .and_then(|title| title.clone())
                    .unwrap_or_else(|| format!("column {at}"));
                counts.push(Some(column.rows(&name)?));
            }
            Err(_) => counts.push(None),
        }
    }
    let readable: Vec<usize> = counts.iter().flatten().copied().collect();
    let Some(&first) = readable.first() else {
        return Err(TableRefusal::NothingReadable);
    };
    if readable.iter().all(|count| *count == first) {
        return Ok(first);
    }
    let common = readable.iter().copied().min().unwrap_or(0);
    Err(TableRefusal::RowCountsDisagree { rows: counts, common })
}
