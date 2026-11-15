//! The cells of a table: four states, `ValueCountPerRow`, the row count (WP06 §4.2).
//!
//! Common TRG 2.3 §9.1 Column p. 190: *"If this value is greater than 1, then the
//! fastest dimension of the column Values array must be this value. EXAMPLE: If
//! this value is 3 for a column of 10 rows, then the corresponding array would be
//! [10, 3]"*; `Column.Values`: *"Ordering matters since it will be the same in all
//! columns allowing to map a column value to another one."* EML TRG 2.3 §6.44
//! p. 130: `IntegerXmlArray.NullValue` is mandatory; a `StringXmlArray` has none.
//! Every object is built by hand.

use std::borrow::Cow;

use energyml_domain::source::NoObjects;
use energyml_domain::table::{
    Cell, CellState, Column, ColumnCells, Table, TableRefusal, row_count,
};
use energyml_domain::values::{Family, Values};
use energyml_eml_2_3 as eml23;

fn cells(values: Values, per_row: u64) -> ColumnCells<'static> {
    ColumnCells {
        values: Cow::Owned(values),
        per_row,
        null: None,
    }
}

/// A column holding `values` inline, `per_row` values per row.
fn column(title: Option<&str>, per_row: u64, key: bool, inline: Option<Values>) -> Column<'static> {
    Column {
        title: title.map(String::from),
        description: None,
        uom: None,
        kind: None,
        values_per_row: per_row,
        facets: Vec::new(),
        key,
        array: None,
        inline,
    }
}

fn table(columns: Vec<Column<'static>>) -> Table<'static> {
    Table {
        uuid: "t1".into(),
        title: None,
        qualified_type: "eml23.ColumnBasedTable".into(),
        columns,
    }
}

#[test]
fn a_vector_column_gives_n_values_per_row() {
    // TRG 2.3 §9.1 p. 190: 3 per row, 2 rows -> [2, 3], row 1 is values 3..6.
    let column = cells(Values::Floating(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]), 3);
    assert_eq!(
        column.cell(1),
        Some(CellState::Vector(vec![
            Some(Cell::Number(4.0)),
            Some(Cell::Number(5.0)),
            Some(Cell::Number(6.0)),
        ]))
    );
    assert_eq!(column.rows("v").expect("a multiple"), 2);
    assert_eq!(column.cell(2), None, "past the last row");
}

#[test]
fn a_vector_column_whose_length_is_not_a_multiple_is_refused() {
    let column = cells(Values::Floating(vec![0.0; 7]), 3);
    assert!(matches!(
        column.rows("v"),
        Err(TableRefusal::VectorShape {
            per_row: 3,
            values: 7,
            ..
        })
    ));
    // `read` refuses it too, naming the column.
    let read = ColumnCells::read(
        &self::column(Some("v"), 3, false, Some(Values::Floating(vec![0.0; 7]))),
        &NoObjects,
    );
    match read {
        Err(TableRefusal::VectorShape { column, .. }) => assert_eq!(column, "v"),
        other => panic!("expected VectorShape, got {other:?}"),
    }
}

#[test]
fn an_integer_null_value_is_absent_not_a_number() {
    // EML TRG 2.3 §6.44 p. 130: `NullValue` is mandatory on an `IntegerXmlArray`.
    let object = eml23::ColumnBasedTable {
        uuid: "00000000-0000-0000-0000-00000000c001".into(),
        column: vec![eml23::Column {
            title: Some("n".into()),
            value_count_per_row: 1,
            values: Some(eml23::AbstractValueArray::IntegerXmlArray(
                eml23::IntegerXmlArray {
                    values: vec![1, -999, 3],
                    null_value: -999,
                    count_per_value: 1,
                    ..Default::default()
                },
            )),
            ..Default::default()
        }],
        ..Default::default()
    };
    let table = Table::read(&object).expect("a table that reads");
    let read = table.read_cells(&NoObjects);
    let column = read[0].as_ref().expect("the column reads");
    assert_eq!(column.null, Some(-999));
    assert_eq!(column.cell(1), Some(CellState::Absent));
    assert_eq!(
        column.cell(0),
        Some(CellState::Value(Cell::Number(1.0)))
    );
}

#[test]
fn a_nan_is_absent() {
    // A floating-point array has no `NullValue`: its absence is `NaN`.
    let column = cells(Values::Floating(vec![1.0, f64::NAN]), 1);
    assert_eq!(column.cell(1), Some(CellState::Absent));
    assert_eq!(column.cell(0), Some(CellState::Value(Cell::Number(1.0))));
}

#[test]
fn an_empty_string_is_a_value() {
    // EML TRG 2.3 §6.50: a `StringXmlArray` has no `NullValue`; S16.
    let column = cells(Values::Text(vec!["a".into(), String::new()]), 1);
    assert_eq!(column.cell(1), Some(CellState::Empty));
    assert_ne!(column.cell(1), Some(CellState::Absent));
    assert_eq!(
        column.cell(0),
        Some(CellState::Value(Cell::Text("a".into())))
    );
}

#[test]
fn an_empty_component_of_a_vector_is_a_value() {
    let column = cells(Values::Text(vec![String::new(), "b".into()]), 2);
    assert_eq!(
        column.cell(0),
        Some(CellState::Vector(vec![
            Some(Cell::Text(String::new())),
            Some(Cell::Text("b".into())),
        ]))
    );
}

#[test]
fn columns_that_disagree_keep_their_common_rows_in_the_refusal() {
    // TRG 2.3 §9.1 p. 190: ordering matters, row identity is positional.
    let table = table(vec![
        column(Some("a"), 1, true, Some(Values::Integer(vec![1, 2, 3]))),
        column(Some("b"), 1, false, Some(Values::Integer(vec![1, 2]))),
    ]);
    let read = table.read_cells(&NoObjects);
    let titles = vec![Some("a".to_string()), Some("b".to_string())];
    match row_count(&read, &titles) {
        Err(TableRefusal::RowCountsDisagree { rows, common }) => {
            assert_eq!(rows, vec![Some(3), Some(2)]);
            assert_eq!(common, 2);
        }
        other => panic!("expected RowCountsDisagree, got {other:?}"),
    }
}

#[test]
fn an_unreadable_column_is_named_and_does_not_count() {
    // Neither an array nor inline values: `Column::read` refuses it.
    let table = table(vec![
        column(Some("épaisseur"), 1, false, None),
        column(Some("code"), 1, true, Some(Values::Text(vec!["e6".into(), "e5".into()]))),
    ]);
    let read = table.read_cells(&NoObjects);
    match &read[0] {
        Err(TableRefusal::Unreadable { column, .. }) => assert_eq!(column, "épaisseur"),
        other => panic!("expected Unreadable, got {other:?}"),
    }
    let titles = vec![Some("épaisseur".to_string()), Some("code".to_string())];
    assert_eq!(row_count(&read, &titles).expect("the readable column decides"), 2);
}

#[test]
fn rows_cells_borrows_the_values_read_by_read_all() {
    // Two paths of reading must agree: `Rows::cells` (from `read_all`) and
    // `Table::read_cells`.
    let table = table(vec![
        column(Some("a"), 1, true, Some(Values::Integer(vec![1, 2]))),
        column(Some("v"), 2, false, Some(Values::Floating(vec![0.5, 0.6, 0.7, 0.8]))),
    ]);
    let rows = table.read_all(&NoObjects).expect("a table that reads");
    let owned = table.read_cells(&NoObjects);
    for (at, expected) in owned.iter().enumerate() {
        let expected = expected.as_ref().expect("it reads");
        let borrowed = rows.cells(at).expect("a readable column");
        assert!(matches!(borrowed.values, Cow::Borrowed(_)));
        assert_eq!(borrowed.family(), expected.family());
        for row in 0..2 {
            assert_eq!(borrowed.cell(row), expected.cell(row), "column {at} row {row}");
        }
    }
    assert_eq!(owned[1].as_ref().unwrap().family(), Family::Floating);
    assert!(rows.cells(5).is_none(), "past the table");
}
