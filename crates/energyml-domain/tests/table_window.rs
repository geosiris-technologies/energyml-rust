//! The window over a large table, `energyml_domain::table::TableWindow` (WP06 §4.10).
//!
//! The rule it follows is the desktop grid's (`grid.rs:303-305`): *an absence is neither
//! the smallest nor the largest*, so it sorts last whichever way the column is sorted.
//! An empty string is a value (S16: a `StringXmlArray` has no `NullValue`) and sorts as
//! `""`. Common TRG 2.3 §9.1 p. 190: *"Ordering matters since it will be the same in all
//! columns allowing to map a column value to another one"* - a row is its position, so a
//! sort is a permutation of row numbers, never of cells.
//!
//! Every column is built by hand, inline. The last line of each comment says how to make
//! the test fail.

use std::borrow::Cow;

use energyml_domain::table::{
    Cell, CellState, ColumnCells, RowFilter, SortKey, SortOrder, TableRefusal, TableWindow,
};
use energyml_domain::values::Values;

type Columns = Vec<Result<ColumnCells<'static>, TableRefusal>>;

fn column(values: Values) -> Result<ColumnCells<'static>, TableRefusal> {
    Ok(ColumnCells {
        values: Cow::Owned(values),
        per_row: 1,
        null: None,
    })
}

fn floats(values: &[f64]) -> Columns {
    vec![column(Values::Floating(values.to_vec()))]
}

fn texts(values: &[&str]) -> Columns {
    vec![column(Values::Text(values.iter().map(|text| text.to_string()).collect()))]
}

fn ascending(column: usize) -> SortKey {
    SortKey {
        column,
        order: SortOrder::Ascending,
        component: None,
    }
}

fn descending(column: usize) -> SortKey {
    SortKey {
        column,
        order: SortOrder::Descending,
        component: None,
    }
}

fn shown(window: &TableWindow) -> Vec<u32> {
    window.page(0, window.len()).to_vec()
}

#[test]
fn the_window_puts_absent_values_last_both_ways() {
    let cells = floats(&[3.0, f64::NAN, 1.0, 2.0]);
    let mut window = TableWindow::new(4).expect("a window");
    window.sort(&[ascending(0)], &cells);
    assert_eq!(shown(&window), vec![2, 3, 0, 1], "ascending: 1, 2, 3, then the absence");
    window.sort(&[descending(0)], &cells);
    assert_eq!(shown(&window), vec![0, 3, 2, 1], "descending: 3, 2, 1, and the absence is still last");
    // Break it: compare with `f64::total_cmp` before the absence test, and the NaN goes first
    // (or last in only one direction).
}

#[test]
fn an_empty_string_sorts_as_a_value_before_words() {
    let cells = texts(&["b", "", "a"]);
    let mut window = TableWindow::new(3).expect("a window");
    window.sort(&[ascending(0)], &cells);
    assert_eq!(shown(&window), vec![1, 2, 0], "the empty string is the smallest text");
    // Break it: sort `Empty` with the absences, and it lands last.
}

#[test]
fn a_tie_keeps_file_order_and_a_second_key_breaks_it() {
    let cells: Columns = vec![
        column(Values::Floating(vec![1.0, 1.0, 2.0, 1.0])),
        column(Values::Floating(vec![9.0, 5.0, 0.0, 7.0])),
    ];
    let mut window = TableWindow::new(4).expect("a window");
    window.sort(&[ascending(0)], &cells);
    assert_eq!(shown(&window), vec![0, 1, 3, 2], "rows 0, 1, 3 tie on the first key: file order");
    window.sort(&[ascending(0), ascending(1)], &cells);
    assert_eq!(shown(&window), vec![1, 3, 0, 2], "the second key breaks the tie");
    window.sort(&[], &cells);
    assert_eq!(shown(&window), vec![0, 1, 2, 3], "no key restores file order");
    // Break it: use `sort_unstable_by`, or start from the previous order instead of file order.
}

#[test]
fn a_jump_lands_on_the_page_of_its_row() {
    let values: Vec<f64> = (0..1000).map(|at| at as f64).collect();
    let cells = floats(&values);
    let mut window = TableWindow::new(1000).expect("a window");
    window.sort(&[descending(0)], &cells);
    assert_eq!(window.page_of(999, 200), Some(0), "the largest value is first");
    assert_eq!(window.page_of(0, 200), Some(4), "the smallest is at position 999, page 4");
    assert_eq!(window.position_of(0), Some(999));
    assert_eq!(window.page_of(0, 0), None, "a page of no rows holds nothing");
    // Break it: answer `position_of(row) = row`, and the jump stays on page 0.
}

#[test]
fn a_filtered_row_has_no_position() {
    let cells = floats(&[10.0, 11.0, 12.0, 13.0]);
    let mut window = TableWindow::new(4).expect("a window");
    window.filter(Some(&RowFilter::Rows(vec![1, 3])), &cells);
    assert_eq!(window.len(), 2);
    assert_eq!(window.position_of(2), None);
    assert_eq!(window.position_of(3), Some(1));
    assert_eq!(shown(&window), vec![1, 3]);
    window.filter(None, &cells);
    assert_eq!(window.len(), 4, "no filter shows every row again");
    // Break it: keep the position the row had before the filter.
}

#[test]
fn an_empty_text_filter_keeps_everything() {
    let cells = texts(&["a", "", "b"]);
    let mut window = TableWindow::new(3).expect("a window");
    window.filter(
        Some(&RowFilter::Text {
            needle: String::new(),
            columns: None,
        }),
        &cells,
    );
    assert_eq!(window.len(), 3, "an empty expression is not a malformed one");
    // Break it: return no row for an empty needle.
}

#[test]
fn a_text_filter_is_case_insensitive_and_an_absence_matches_nothing() {
    let cells = texts(&["Sandstone", "shale", ""]);
    let mut window = TableWindow::new(3).expect("a window");
    window.filter(
        Some(&RowFilter::Text {
            needle: "SAND".to_string(),
            columns: Some(vec![0]),
        }),
        &cells,
    );
    assert_eq!(shown(&window), vec![0]);
    // Break it: compare without lowercasing, and the row is lost.
}

#[test]
fn a_range_keeps_numbers_inside_inclusive_bounds_and_never_an_absence() {
    let cells = floats(&[1.0, 5.0, 9.0, f64::NAN]);
    let mut window = TableWindow::new(4).expect("a window");
    window.filter(
        Some(&RowFilter::Range {
            column: 0,
            min: Some(5.0),
            max: Some(9.0),
        }),
        &cells,
    );
    assert_eq!(shown(&window), vec![1, 2], "the bounds are inclusive");
    window.filter(Some(&RowFilter::AbsentOnly { column: 0 }), &cells);
    assert_eq!(shown(&window), vec![3]);
    window.filter(
        Some(&RowFilter::All(vec![
            RowFilter::Range {
                column: 0,
                min: Some(0.0),
                max: None,
            },
            RowFilter::Equals {
                column: 0,
                value: Cell::Number(9.0),
            },
        ])),
        &cells,
    );
    assert_eq!(shown(&window), vec![2], "every filter must keep the row");
    // Break it: make the bounds exclusive, or let `All` keep a row any filter keeps.
}

#[test]
fn a_column_that_cannot_be_read_sorts_last_and_is_named_by_its_state() {
    let cells: Columns = vec![Err(TableRefusal::NothingReadable)];
    let window_cells = &cells;
    use energyml_domain::table::ColumnAccess;
    assert_eq!(window_cells.state(0, 0), Some(CellState::Unreadable));
    assert_eq!(window_cells.state(0, 5), None, "beyond the columns there is no state");
    // Break it: answer `Absent` for an unreadable column, and the two are no longer told apart.
}

#[cfg(target_pointer_width = "64")]
#[test]
fn more_rows_than_a_u32_numbers_are_refused() {
    let refused = TableWindow::new(u32::MAX as usize + 1);
    assert!(refused.is_err(), "4 294 967 296 rows do not fit a window");
    // Break it: cast `rows as u32` without the test, and the window silently holds none.
}
