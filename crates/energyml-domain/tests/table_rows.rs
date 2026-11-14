//! Rows of a table: several values per row (D2) and the keys of a 2.0.1
//! `DoubleTableLookup` (D3). Every object is built by hand.
//!
//! Common TRG 2.3 §9.1 Column p. 190: *"If this value is greater than 1, then
//! the fastest dimension of the column Values array must be this value.
//! EXAMPLE: If this value is 3 for a column of 10 rows, then the corresponding
//! array would be [10, 3]"*. RESQML TRG 2.0.1 §6.24 `DoubleLookup` p. 72
//! (`Key: double`) and §6.40 `StringLookup` p. 79 (`Key: integer`).

use energyml_domain::source::NoObjects;
use energyml_domain::table::{Cell, Lookup};
use energyml_domain::values::Values;
use energyml_eml_2_3::{
    AbstractValueArray, Column, ColumnBasedTable, FloatingPointXmlArray, IntegerXmlArray,
};

fn integers(values: Vec<i64>) -> Option<AbstractValueArray> {
    Some(AbstractValueArray::IntegerXmlArray(IntegerXmlArray {
        values,
        count_per_value: 1,
        ..Default::default()
    }))
}

fn floats(values: Vec<f64>) -> Option<AbstractValueArray> {
    Some(AbstractValueArray::FloatingPointXmlArray(FloatingPointXmlArray {
        values,
        count_per_value: 1,
        ..Default::default()
    }))
}

/// A table of a key `code` and a column `range` of `per_row` values per row.
fn table(keys: Vec<i64>, range: Vec<f64>, per_row: i64) -> ColumnBasedTable {
    ColumnBasedTable {
        uuid: "00000000-0000-0000-0000-00000000000a".into(),
        key_column: vec![Column {
            title: Some("code".into()),
            value_count_per_row: 1,
            values: integers(keys),
            ..Default::default()
        }],
        column: vec![Column {
            title: Some("range".into()),
            value_count_per_row: per_row,
            values: floats(range),
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[test]
fn a_column_of_two_values_per_row_reads_two_values_per_row() {
    let object = table(vec![1, 2, 3], vec![10.0, 11.0, 20.0, 21.0, 30.0, 31.0], 2);
    let lookup = Lookup::read(&object, &NoObjects).expect("a lookup that reads");
    let rows = &lookup.rows;
    assert_eq!(rows.rows, 3);
    assert_eq!(rows.per_row, vec![1, 2]);
    assert_eq!(
        rows.values_of(1, 1),
        Some(vec![Cell::Number(20.0), Cell::Number(21.0)])
    );
    assert_eq!(rows.cell(1, 1), None, "a vector column has no scalar cell");
    assert_eq!(rows.cell(1, 0), Some(Cell::Number(2.0)));
    assert_eq!(
        rows.row_values(2),
        vec![
            Some(vec![Cell::Number(3.0)]),
            Some(vec![Cell::Number(30.0), Cell::Number(31.0)]),
        ]
    );
    assert!(lookup.notes.is_empty(), "{:?}", lookup.notes);
}

#[test]
fn a_column_that_is_not_a_whole_number_of_rows_is_named() {
    let object = table(vec![1, 2], vec![10.0, 11.0, 20.0, 21.0, 30.0], 2);
    let lookup = Lookup::read(&object, &NoObjects).expect("a lookup that reads");
    assert_eq!(lookup.rows.rows, 2);
    assert!(
        lookup
            .notes
            .iter()
            .any(|note| note.contains("range") && note.contains("ValueCountPerRow")),
        "{:?}",
        lookup.notes
    );
}

#[test]
fn double_table_lookup_keys_keep_their_fraction() {
    use energyml_resqml_2_0_1::{DoubleLookup, DoubleTableLookup};
    let object = DoubleTableLookup {
        uuid: "00000000-0000-0000-0000-00000000000b".into(),
        value: vec![
            DoubleLookup { key: 0.5, value: 10.0 },
            DoubleLookup { key: 0.7, value: 20.0 },
        ],
        ..Default::default()
    };
    let lookup = Lookup::read(&object, &NoObjects).expect("a lookup that reads");
    assert_eq!(lookup.rows.columns[0], Some(Values::Floating(vec![0.5, 0.7])));
    assert_eq!(lookup.of(0.5), Some(vec![Some(Cell::Number(10.0))]));
    assert_eq!(lookup.of(0.7), Some(vec![Some(Cell::Number(20.0))]));
    assert!(lookup.duplicate_keys().is_empty());
}

#[test]
fn string_table_lookup_keys_stay_integers() {
    use energyml_resqml_2_0_1::{StringLookup, StringTableLookup};
    let object = StringTableLookup {
        uuid: "00000000-0000-0000-0000-00000000000c".into(),
        value: vec![
            StringLookup { key: 1, value: "sand".into() },
            StringLookup { key: 2, value: "shale".into() },
        ],
        ..Default::default()
    };
    let lookup = Lookup::read(&object, &NoObjects).expect("a lookup that reads");
    assert_eq!(lookup.rows.columns[0], Some(Values::Integer(vec![1, 2])));
}
