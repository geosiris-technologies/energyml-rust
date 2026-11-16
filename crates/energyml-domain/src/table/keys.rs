//! Finding a row by its key in constant time, and **the** rule that says which
//! row a value names: by the table's one key column, or by its rank from 0 when
//! the table has none (`house/table/row-rank-without-key`).
//!
//! Numbers are keys by exact equality, with `-0.0 == 0.0` and `NaN` never a key
//! (a hash index needs an equivalence relation; a tolerance is not transitive).
//! An empty string names nothing, and a row whose key cell is absent, empty,
//! unreadable or a vector is left out of the index and counted.

use std::borrow::Cow;
use std::collections::HashMap;

use crate::table::{Cell, CellState, ColumnCells, Lookup};
use crate::values::Values;

/// What a key, or a value, finds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowMatch {
    /// Exactly one row.
    One(usize),
    /// A duplicated key: every row, the first is the one used ("first match wins").
    Several(Vec<usize>),
    /// No row.
    None,
}

/// Why a table cannot say which row a value names.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NamingRefusal {
    /// A value names a row by one key or by its rank, never by a tuple.
    #[error("the table has {count} key columns, and a value names a row by one key or by its rank")]
    SeveralKeyColumns {
        /// How many key columns the table has.
        count: usize,
    },
}

/// One component of a key, hashable.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum KeyPart {
    /// `f64::to_bits` of the number, after `-0.0` became `0.0`.
    Number(u64),
    Boolean(bool),
    Text(String),
}

impl KeyPart {
    /// `None` for what is never a key: a `NaN`, an empty string.
    fn of(cell: &Cell) -> Option<KeyPart> {
        match cell {
            Cell::Number(x) if x.is_nan() => None,
            Cell::Number(x) => {
                let x = if *x == 0.0 { 0.0 } else { *x };
                Some(KeyPart::Number(x.to_bits()))
            }
            Cell::Boolean(b) => Some(KeyPart::Boolean(*b)),
            Cell::Text(s) if s.is_empty() => None,
            Cell::Text(s) => Some(KeyPart::Text(s.clone())),
        }
    }

    fn to_cell(&self) -> Cell {
        match self {
            KeyPart::Number(bits) => Cell::Number(f64::from_bits(*bits)),
            KeyPart::Boolean(b) => Cell::Boolean(*b),
            KeyPart::Text(s) => Cell::Text(s.clone()),
        }
    }
}

/// The key columns of a table, hashed.
#[derive(Clone, Debug)]
pub struct KeyIndex {
    key_columns: Vec<usize>,
    rows: usize,
    map: HashMap<Vec<KeyPart>, Vec<usize>>,
    incomplete: Vec<usize>,
}

impl KeyIndex {
    /// The rows holding this key tuple, first first; empty for none or a wrong-shaped key.
    pub fn rows_of(&self, key: &[Cell]) -> &[usize] {
        if self.key_columns.is_empty() || key.len() != self.key_columns.len() {
            return &[];
        }
        let mut parts = Vec::with_capacity(key.len());
        for cell in key {
            match KeyPart::of(cell) {
                Some(part) => parts.push(part),
                None => return &[],
            }
        }
        self.map.get(&parts).map(Vec::as_slice).unwrap_or(&[])
    }

    /// `rows_of` as a `RowMatch`.
    pub fn find(&self, key: &[Cell]) -> RowMatch {
        match self.rows_of(key) {
            [] => RowMatch::None,
            [one] => RowMatch::One(*one),
            many => RowMatch::Several(many.to_vec()),
        }
    }

    /// Every key held by more than one row, ordered by its first row.
    pub fn duplicates(&self) -> Vec<(Vec<Cell>, Vec<usize>)> {
        let mut found: Vec<(Vec<Cell>, Vec<usize>)> = self
            .map
            .iter()
            .filter(|(_, rows)| rows.len() > 1)
            .map(|(key, rows)| (key.iter().map(KeyPart::to_cell).collect(), rows.clone()))
            .collect();
        found.sort_by_key(|(_, rows)| rows[0]);
        found
    }

    /// The key columns, in the table's order.
    pub fn key_columns(&self) -> &[usize] {
        &self.key_columns
    }

    /// Rows of the table.
    pub fn row_count(&self) -> usize {
        self.rows
    }

    /// Rows left out of the index because a key cell is absent, empty, unreadable or a vector.
    pub fn incomplete_rows(&self) -> &[usize] {
        &self.incomplete
    }

    /// The only constructor besides `Lookup::key_index`; crate-private (contract
    /// §3: no free builder). `columns` holds each key column with its cells.
    pub(crate) fn from_columns(columns: Vec<(usize, ColumnCells<'_>)>, rows: usize) -> KeyIndex {
        let key_columns: Vec<usize> = columns.iter().map(|(at, _)| *at).collect();
        let mut map: HashMap<Vec<KeyPart>, Vec<usize>> = HashMap::new();
        let mut incomplete = Vec::new();
        // A table with no key column has no key to be incomplete.
        let rows_to_index = if columns.is_empty() { 0 } else { rows };
        for row in 0..rows_to_index {
            let mut parts = Vec::with_capacity(columns.len());
            let mut complete = true;
            for (_, cells) in &columns {
                match cells.cell(row) {
                    Some(CellState::Value(cell)) => match KeyPart::of(&cell) {
                        Some(part) => parts.push(part),
                        None => {
                            complete = false;
                            break;
                        }
                    },
                    _ => {
                        complete = false;
                        break;
                    }
                }
            }
            if complete {
                map.entry(parts).or_default().push(row);
            } else {
                incomplete.push(row);
            }
        }
        KeyIndex {
            key_columns,
            rows,
            map,
            incomplete,
        }
    }
}

impl Lookup {
    /// The index of this table's key columns (`rows.cells` of each). Built in one pass.
    ///
    /// A key column that could not be read leaves every row incomplete.
    pub fn key_index(&self) -> KeyIndex {
        let columns: Vec<(usize, ColumnCells<'_>)> = self
            .key_columns
            .iter()
            .map(|at| {
                let cells = self.rows.cells(*at).unwrap_or_else(|| ColumnCells {
                    // No cell at all: every row is incomplete.
                    values: Cow::Owned(Values::Text(Vec::new())),
                    per_row: 1,
                    null: None,
                });
                (*at, cells)
            })
            .collect();
        KeyIndex::from_columns(columns, self.rows.rows)
    }
}

/// **The** rule: the row a value names - by the table's one key column, or by its rank from 0 when it
/// has none (`house/table/row-rank-without-key`). A keyed table is never read by rank.
pub fn row_named_by(index: &KeyIndex, value: &Cell) -> Result<RowMatch, NamingRefusal> {
    match index.key_columns.len() {
        1 => Ok(index.find(std::slice::from_ref(value))),
        0 => Ok(match value {
            Cell::Number(x)
                if x.is_finite() && x.fract() == 0.0 && *x >= 0.0 && (*x as usize) < index.rows =>
            {
                RowMatch::One(*x as usize)
            }
            // A text "3" is not a rank; neither is a fraction, a negative or a number past the end.
            _ => RowMatch::None,
        }),
        count => Err(NamingRefusal::SeveralKeyColumns { count }),
    }
}
