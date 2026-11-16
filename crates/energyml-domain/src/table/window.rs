//! A window over the rows of a **large** table: a sort permutation, a filter mask, a page,
//! and where a row that was jumped to lands.
//!
//! The window holds row numbers only (`u32`, 4 bytes a row), never cells: a million rows is
//! a few megabytes of permutation, and the cells stay in the columns they were read into.
//!
//! **An absence is neither the smallest nor the largest.** `Absent`, `Unreadable` and a
//! cell that is not there sort *last*, whichever way the column is sorted, and keep file
//! order among themselves. An empty string is a value (`CellState::Empty`) and sorts as
//! `""`, before any word, in ascending order.

use std::cmp::Ordering;

use crate::table::{Cell, CellState, ColumnCells, TableRefusal};

/// The direction of a sort.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortOrder {
    /// Smallest first.
    Ascending,
    /// Largest first. Absences stay last.
    Descending,
}

/// One column to sort on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SortKey {
    /// The column.
    pub column: usize,
    /// The direction.
    pub order: SortOrder,
    /// For a vector column, the component sorted on; `None` compares the vectors component
    /// by component.
    pub component: Option<usize>,
}

/// Which rows to keep. Typed: a filter that cannot be built is the caller's refusal, never
/// "everything".
#[derive(Clone, Debug, PartialEq)]
pub enum RowFilter {
    /// The cell's text (`Cell` `Display`) contains `needle`, case-insensitively, in any of
    /// `columns` (all if `None`). An empty needle keeps every row.
    Text {
        /// What to look for.
        needle: String,
        /// The columns looked in; `None` for all.
        columns: Option<Vec<usize>>,
    },
    /// The cell equals this value (`Cell::matches`).
    Equals {
        /// The column.
        column: usize,
        /// The value.
        value: Cell,
    },
    /// Numbers within `[min, max]` (bounds optional, inclusive); a non-number never passes.
    Range {
        /// The column.
        column: usize,
        /// The lower bound, if any.
        min: Option<f64>,
        /// The upper bound, if any.
        max: Option<f64>,
    },
    /// `Absent` cells only.
    AbsentOnly {
        /// The column.
        column: usize,
    },
    /// `Empty` cells only.
    EmptyOnly {
        /// The column.
        column: usize,
    },
    /// Exactly these rows (the dangling rows of a `LinkReport`, for instance).
    Rows(Vec<usize>),
    /// Every filter must keep the row.
    All(Vec<RowFilter>),
}

/// How the window reads cells.
pub trait ColumnAccess {
    /// How many rows the columns share.
    fn rows(&self) -> usize;
    /// How many columns.
    fn columns(&self) -> usize;
    /// `None` beyond the column; an unreadable column answers `Some(Unreadable)`.
    fn state(&self, row: usize, column: usize) -> Option<CellState>;
}

impl<'a> ColumnAccess for [Result<ColumnCells<'a>, TableRefusal>] {
    /// The rows every readable column holds (the fewest, when they disagree: the window
    /// never reads past the end of a column).
    fn rows(&self) -> usize {
        self.iter()
            .filter_map(|column| column.as_ref().ok()?.rows("").ok())
            .min()
            .unwrap_or(0)
    }

    fn columns(&self) -> usize {
        self.len()
    }

    fn state(&self, row: usize, column: usize) -> Option<CellState> {
        match self.get(column)? {
            Ok(cells) => cells.cell(row),
            Err(_) => Some(CellState::Unreadable),
        }
    }
}

// A slice cannot be turned into a `&dyn ColumnAccess` (it is not `Sized`), so the two
// sized spellings a caller holds delegate to it.
impl<'a> ColumnAccess for Vec<Result<ColumnCells<'a>, TableRefusal>> {
    fn rows(&self) -> usize {
        self.as_slice().rows()
    }
    fn columns(&self) -> usize {
        self.as_slice().columns()
    }
    fn state(&self, row: usize, column: usize) -> Option<CellState> {
        self.as_slice().state(row, column)
    }
}

impl<'a, 'b> ColumnAccess for &'b [Result<ColumnCells<'a>, TableRefusal>] {
    fn rows(&self) -> usize {
        (**self).rows()
    }
    fn columns(&self) -> usize {
        (**self).columns()
    }
    fn state(&self, row: usize, column: usize) -> Option<CellState> {
        (**self).state(row, column)
    }
}

/// Why a window cannot be made.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum WindowRefusal {
    /// More rows than a `u32` numbers.
    #[error("{rows} rows is more than a window holds ({max})")]
    TooManyRows {
        /// The rows asked for.
        rows: usize,
        /// The most a window holds.
        max: usize,
    },
}

/// The rows of a table in a chosen order, narrowed by a filter, paged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableWindow {
    /// Every row, in the sort order.
    order: Vec<u32>,
    /// Whether each row (by row number) passes the filter.
    keep: Vec<bool>,
    /// `order` without the rows the filter dropped.
    shown: Vec<u32>,
    /// The place of each row (by row number) in `shown`; `u32::MAX` when filtered out.
    position: Vec<u32>,
}

const FILTERED_OUT: u32 = u32::MAX;

impl TableWindow {
    /// Every row, in file order. Refused above `u32::MAX` rows.
    pub fn new(rows: usize) -> Result<TableWindow, WindowRefusal> {
        let max = u32::MAX as usize;
        if rows > max {
            return Err(WindowRefusal::TooManyRows { rows, max });
        }
        let mut window = TableWindow {
            order: (0..rows as u32).collect(),
            keep: vec![true; rows],
            shown: Vec::new(),
            position: Vec::new(),
        };
        window.refresh();
        Ok(window)
    }

    /// Recompute `shown` and the inverse table from `order` and `keep`.
    fn refresh(&mut self) {
        self.shown = self
            .order
            .iter()
            .copied()
            .filter(|row| self.keep[*row as usize])
            .collect();
        self.position = vec![FILTERED_OUT; self.keep.len()];
        for (at, row) in self.shown.iter().enumerate() {
            self.position[*row as usize] = at as u32;
        }
    }

    /// Stable sort by these keys, the first deciding first; an empty list restores file
    /// order. The sort always starts from file order, so a tie keeps file order.
    pub fn sort(&mut self, keys: &[SortKey], cells: &dyn ColumnAccess) {
        let rows = self.keep.len();
        let mut order: Vec<u32> = (0..rows as u32).collect();
        if !keys.is_empty() {
            // One prepared value per row and key, so the comparator allocates nothing.
            let prepared: Vec<Vec<Option<Sortable>>> = keys
                .iter()
                .map(|key| {
                    (0..rows)
                        .map(|row| {
                            cells
                                .state(row, key.column)
                                .and_then(|state| Sortable::of(state, key.component))
                        })
                        .collect()
                })
                .collect();
            order.sort_by(|a, b| {
                for (key, column) in keys.iter().zip(&prepared) {
                    let (a, b) = (&column[*a as usize], &column[*b as usize]);
                    let ordering = match (a, b) {
                        (None, None) => Ordering::Equal,
                        // An absence is last, both ways.
                        (None, Some(_)) => return Ordering::Greater,
                        (Some(_), None) => return Ordering::Less,
                        (Some(a), Some(b)) => {
                            let ordering = a.compare(b);
                            match key.order {
                                SortOrder::Ascending => ordering,
                                SortOrder::Descending => ordering.reverse(),
                            }
                        }
                    };
                    if ordering != Ordering::Equal {
                        return ordering;
                    }
                }
                Ordering::Equal
            });
        }
        self.order = order;
        self.refresh();
    }

    /// Keep the rows the filter keeps (`None`: every row), in the current order.
    pub fn filter(&mut self, filter: Option<&RowFilter>, cells: &dyn ColumnAccess) {
        let rows = self.keep.len();
        self.keep = match filter {
            None => vec![true; rows],
            Some(filter) => {
                let compiled = Compiled::of(filter);
                (0..rows).map(|row| compiled.keeps(row, cells)).collect()
            }
        };
        self.refresh();
    }

    /// Rows shown.
    pub fn len(&self) -> usize {
        self.shown.len()
    }

    /// Whether no row is shown.
    pub fn is_empty(&self) -> bool {
        self.shown.is_empty()
    }

    /// The rows of a page, as row numbers of the table; clipped at the end.
    pub fn page(&self, offset: usize, len: usize) -> &[u32] {
        let start = offset.min(self.shown.len());
        let end = offset.saturating_add(len).min(self.shown.len());
        &self.shown[start..end]
    }

    /// Where a row is among the rows shown; `None` when filtered out (or not a row).
    pub fn position_of(&self, row: usize) -> Option<usize> {
        match self.position.get(row) {
            Some(at) if *at != FILTERED_OUT => Some(*at as usize),
            _ => None,
        }
    }

    /// The page holding a row: `position_of(row) / page_len`. `None` for a page length of 0.
    pub fn page_of(&self, row: usize, page_len: usize) -> Option<usize> {
        if page_len == 0 {
            return None;
        }
        self.position_of(row).map(|at| at / page_len)
    }
}

// ---------------------------------------------------------------------------
// Sorting
// ---------------------------------------------------------------------------

/// A present value, prepared for comparison.
#[derive(Clone, Debug, PartialEq)]
enum Sortable {
    Number(f64),
    Boolean(bool),
    Text(String),
    /// A vector cell: compared component by component, an absent component last.
    Vector(Vec<Option<Sortable>>),
}

impl Sortable {
    fn of_cell(cell: Cell) -> Sortable {
        match cell {
            Cell::Number(x) => Sortable::Number(x),
            Cell::Boolean(b) => Sortable::Boolean(b),
            Cell::Text(s) => Sortable::Text(s),
        }
    }

    /// `None` is an absence.
    fn of(state: CellState, component: Option<usize>) -> Option<Sortable> {
        match state {
            CellState::Value(cell) => Some(Sortable::of_cell(cell)),
            // An empty string is a value: it sorts as `""` (S16).
            CellState::Empty => Some(Sortable::Text(String::new())),
            CellState::Absent | CellState::Unreadable => None,
            CellState::Vector(components) => match component {
                Some(at) => components.into_iter().nth(at).flatten().map(Sortable::of_cell),
                None => Some(Sortable::Vector(
                    components
                        .into_iter()
                        .map(|component| component.map(Sortable::of_cell))
                        .collect(),
                )),
            },
        }
    }

    /// Two families in one column do not happen; when they do, `Number < Boolean < Text <
    /// Vector`.
    fn family(&self) -> u8 {
        match self {
            Sortable::Number(_) => 0,
            Sortable::Boolean(_) => 1,
            Sortable::Text(_) => 2,
            Sortable::Vector(_) => 3,
        }
    }

    fn compare(&self, other: &Sortable) -> Ordering {
        match (self, other) {
            (Sortable::Number(a), Sortable::Number(b)) => a.total_cmp(b),
            (Sortable::Boolean(a), Sortable::Boolean(b)) => a.cmp(b),
            (Sortable::Text(a), Sortable::Text(b)) => a.cmp(b),
            (Sortable::Vector(a), Sortable::Vector(b)) => {
                for (a, b) in a.iter().zip(b.iter()) {
                    let ordering = match (a, b) {
                        (None, None) => Ordering::Equal,
                        (None, Some(_)) => Ordering::Greater,
                        (Some(_), None) => Ordering::Less,
                        (Some(a), Some(b)) => a.compare(b),
                    };
                    if ordering != Ordering::Equal {
                        return ordering;
                    }
                }
                a.len().cmp(&b.len())
            }
            _ => self.family().cmp(&other.family()),
        }
    }
}

// ---------------------------------------------------------------------------
// Filtering
// ---------------------------------------------------------------------------

/// A filter ready to be asked about many rows.
enum Compiled {
    Text {
        needle: String,
        columns: Option<Vec<usize>>,
    },
    Equals {
        column: usize,
        value: Cell,
    },
    Range {
        column: usize,
        min: Option<f64>,
        max: Option<f64>,
    },
    Absent {
        column: usize,
    },
    Empty {
        column: usize,
    },
    /// Sorted, so a row is found by bisection.
    Rows(Vec<usize>),
    All(Vec<Compiled>),
}

impl Compiled {
    fn of(filter: &RowFilter) -> Compiled {
        match filter {
            RowFilter::Text { needle, columns } => Compiled::Text {
                needle: needle.to_lowercase(),
                columns: columns.clone(),
            },
            RowFilter::Equals { column, value } => Compiled::Equals {
                column: *column,
                value: value.clone(),
            },
            RowFilter::Range { column, min, max } => Compiled::Range {
                column: *column,
                min: *min,
                max: *max,
            },
            RowFilter::AbsentOnly { column } => Compiled::Absent { column: *column },
            RowFilter::EmptyOnly { column } => Compiled::Empty { column: *column },
            RowFilter::Rows(rows) => {
                let mut rows = rows.clone();
                rows.sort_unstable();
                rows.dedup();
                Compiled::Rows(rows)
            }
            RowFilter::All(filters) => Compiled::All(filters.iter().map(Compiled::of).collect()),
        }
    }

    fn keeps(&self, row: usize, cells: &dyn ColumnAccess) -> bool {
        match self {
            Compiled::Text { needle, columns } => {
                if needle.is_empty() {
                    return true;
                }
                let all: Vec<usize>;
                let columns: &[usize] = match columns {
                    Some(columns) => columns,
                    None => {
                        all = (0..cells.columns()).collect();
                        &all
                    }
                };
                columns.iter().any(|column| {
                    cells
                        .state(row, *column)
                        .is_some_and(|state| text_of(&state).to_lowercase().contains(needle.as_str()))
                })
            }
            Compiled::Equals { column, value } => matches!(
                cells.state(row, *column),
                Some(CellState::Value(cell)) if cell.matches(value)
            ),
            Compiled::Range { column, min, max } => match cells.state(row, *column) {
                Some(CellState::Value(Cell::Number(x))) => {
                    min.is_none_or(|min| x >= min) && max.is_none_or(|max| x <= max)
                }
                _ => false,
            },
            Compiled::Absent { column } => matches!(cells.state(row, *column), Some(CellState::Absent)),
            Compiled::Empty { column } => matches!(cells.state(row, *column), Some(CellState::Empty)),
            Compiled::Rows(rows) => rows.binary_search(&row).is_ok(),
            Compiled::All(filters) => filters.iter().all(|filter| filter.keeps(row, cells)),
        }
    }
}

/// What a cell reads as, for a text filter. An absence reads as nothing that can match.
fn text_of(state: &CellState) -> String {
    match state {
        CellState::Value(cell) => cell.to_string(),
        CellState::Vector(components) => components
            .iter()
            .flatten()
            .map(|cell| cell.to_string())
            .collect::<Vec<_>>()
            .join(" "),
        CellState::Empty | CellState::Absent | CellState::Unreadable => String::new(),
    }
}
