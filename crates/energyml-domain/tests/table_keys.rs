//! The key index and the rule "a value names a row by the key, else by the rank"
//! (WP06 §4.6, decisions T1 to T4, S16, S17).
//!
//! Common TRG 2.3 §9.1 Column p. 190: `KeyColumn`: *"Defines a key column which
//! make the table a lookup table."*; `Column.Values`: *"Ordering matters since it
//! will be the same in all columns allowing to map a column value to another
//! one."* RESQML TRG 2.0.1 §6.24 `DoubleLookup` p. 72 (`Key: double`). Every
//! object is built by hand.

use energyml_domain::source::NoObjects;
use energyml_domain::table::{Cell, KeyIndex, Lookup, NamingRefusal, RowMatch, Rows, row_named_by};
use energyml_domain::values::Values;

/// A table of `keys.len()` key columns (each of `keys[i]`) and one value column of `names`.
fn lookup(keys: Vec<Values>, names: Vec<&str>) -> Lookup {
    let rows = names.len();
    let mut columns: Vec<Option<Values>> = keys.into_iter().map(Some).collect();
    let key_columns: Vec<usize> = (0..columns.len()).collect();
    columns.push(Some(Values::Text(names.into_iter().map(String::from).collect())));
    let width = columns.len();
    Lookup {
        titles: (0..width).map(|at| Some(format!("c{at}"))).collect(),
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

fn texts(values: &[&str]) -> Values {
    Values::Text(values.iter().map(|v| v.to_string()).collect())
}

fn index_of(table: &Lookup) -> KeyIndex {
    table.key_index()
}

#[test]
fn a_key_index_finds_what_the_linear_search_found() {
    // Two paths to the same answer: the hash index, and a linear scan written
    // here with `Cell::matches`.
    let table = lookup(vec![texts(&["a", "b", "a", "c"])], vec!["1", "2", "3", "4"]);
    let index = index_of(&table);
    for wanted in ["a", "b", "c", "z"] {
        let key = [Cell::Text(wanted.to_string())];
        let linear: Vec<usize> = (0..table.rows.rows)
            .filter(|row| table.rows.cell(*row, 0).is_some_and(|cell| cell.matches(&key[0])))
            .collect();
        assert_eq!(index.rows_of(&key), linear.as_slice(), "key {wanted}");
    }
}

#[test]
fn a_duplicate_key_resolves_to_the_first_row_and_is_reported() {
    let table = lookup(vec![texts(&["a", "b", "a"])], vec!["1", "2", "3"]);
    let index = index_of(&table);
    assert_eq!(
        index.find(&[Cell::Text("a".into())]),
        RowMatch::Several(vec![0, 2])
    );
    assert_eq!(
        index.duplicates(),
        vec![(vec![Cell::Text("a".into())], vec![0, 2])]
    );
    assert_eq!(index.find(&[Cell::Text("b".into())]), RowMatch::One(1));
}

#[test]
fn a_double_lookup_keeps_fractional_keys() {
    // RESQML TRG 2.0.1 §6.24 p. 72: `DoubleLookup.Key` is a double.
    use energyml_resqml_2_0_1::{DoubleLookup, DoubleTableLookup};
    let object = DoubleTableLookup {
        uuid: "00000000-0000-0000-0000-00000000000b".into(),
        value: vec![
            DoubleLookup {
                key: 0.15,
                value: 1.0,
            },
            DoubleLookup { key: 0.2, value: 2.0 },
        ],
        ..Default::default()
    };
    let table = Lookup::read(&object, &NoObjects).expect("a lookup that reads");
    let index = table.key_index();
    assert_eq!(index.find(&[Cell::Number(0.15)]), RowMatch::One(0));
    assert_eq!(index.find(&[Cell::Number(0.2)]), RowMatch::One(1));
}

#[test]
fn minus_zero_is_zero_and_nan_is_never_a_key() {
    let table = lookup(vec![Values::Floating(vec![0.0, 1.0])], vec!["zero", "one"]);
    let index = index_of(&table);
    assert_eq!(index.find(&[Cell::Number(-0.0)]), RowMatch::One(0));
    assert_eq!(index.find(&[Cell::Number(f64::NAN)]), RowMatch::None);
    // The exact comparison is the one `Cell::matches` makes too (T1).
    assert!(Cell::Number(-0.0).matches(&Cell::Number(0.0)));
    assert!(!Cell::Number(f64::NAN).matches(&Cell::Number(f64::NAN)));
}

#[test]
fn a_rank_link_on_a_keyless_table_counts_from_zero() {
    // S17: no key column, so a value names a row by its rank from 0.
    let table = lookup(vec![], vec!["a", "b", "c"]);
    let index = index_of(&table);
    assert_eq!(row_named_by(&index, &Cell::Number(0.0)), Ok(RowMatch::One(0)));
    assert_eq!(row_named_by(&index, &Cell::Number(3.0)), Ok(RowMatch::None));
    assert_eq!(row_named_by(&index, &Cell::Number(1.5)), Ok(RowMatch::None));
    assert_eq!(row_named_by(&index, &Cell::Number(-1.0)), Ok(RowMatch::None));
    assert_eq!(
        row_named_by(&index, &Cell::Text("1".into())),
        Ok(RowMatch::None),
        "a text is not a rank"
    );
    assert!(index.incomplete_rows().is_empty(), "no key, no incomplete row");
}

#[test]
fn a_keyed_table_is_never_read_by_rank() {
    // T4: a keyed table is named by its key; the rank 1 is not the key 1.
    let table = lookup(vec![Values::Integer(vec![10, 20, 30])], vec!["a", "b", "c"]);
    let index = index_of(&table);
    assert_eq!(row_named_by(&index, &Cell::Number(1.0)), Ok(RowMatch::None));
    assert_eq!(row_named_by(&index, &Cell::Number(20.0)), Ok(RowMatch::One(1)));
}

#[test]
fn two_key_columns_refuse_to_name_a_row() {
    // T3: a value does not name a tuple.
    let table = lookup(
        vec![Values::Integer(vec![1, 2]), Values::Integer(vec![5, 6])],
        vec!["a", "b"],
    );
    let index = index_of(&table);
    assert_eq!(
        row_named_by(&index, &Cell::Number(1.0)),
        Err(NamingRefusal::SeveralKeyColumns { count: 2 })
    );
    // The two-part key still finds its row through the index.
    assert_eq!(
        index.find(&[Cell::Number(2.0), Cell::Number(6.0)]),
        RowMatch::One(1)
    );
}

#[test]
fn a_row_whose_key_is_empty_or_absent_is_not_indexed_and_is_counted() {
    // T2, S16: an empty text is a value, but it names nothing.
    let table = lookup(vec![texts(&["a", "", "b"])], vec!["1", "2", "3"]);
    let index = index_of(&table);
    assert_eq!(index.incomplete_rows(), &[1]);
    assert_eq!(index.find(&[Cell::Text(String::new())]), RowMatch::None);
    assert_eq!(index.find(&[Cell::Text("b".into())]), RowMatch::One(2));
    assert_eq!(index.row_count(), 3);
}
