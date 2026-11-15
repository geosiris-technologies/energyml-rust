//! Checking the links of a table and of a set (`table::check`).
//!
//! Tables are `eml23.ColumnBasedTable` objects built by hand (common TRG 2.3
//! section 9.1 p. 190; section 11.11 p. 294 for the facet of convention B),
//! sheeted and read as cells by the table's own readers; the declarations are
//! written by hand, or read from a real facet where the test is about the
//! facet. Nothing is read from a file.

use std::cell::Cell as StdCell;

use energyml_core::registry::DynObject;
use energyml_domain::property_kind::Resolver;
use energyml_domain::source::{NoObjects, ObjectSource};
use energyml_domain::table::{
    Cell, ColumnCells, Convention, LinkDeclaration, LinkReaders, LinkRefusal, LinkTarget, LinkWarning, Lookup, Table,
    TableRefusal, TableSet, TableSheet, check_links, check_links_of_set, resolve_kinds,
};
use energyml_eml_2_3 as eml23;

const OWN: &str = "eml:///eml23.ColumnBasedTable(00000000-0000-0000-0000-00000000b001)";
const TEXT_TARGET: &str = "eml:///eml23.ColumnBasedTable(00000000-0000-0000-0000-00000000b002)";
const INT_TARGET: &str = "eml:///eml23.ColumnBasedTable(00000000-0000-0000-0000-00000000b003)";
const KEYLESS: &str = "eml:///eml23.ColumnBasedTable(00000000-0000-0000-0000-00000000b004)";
const LENGTH: &str = "4364b378-899a-403a-8401-b06abd4fc0cf";

fn strings(values: &[&str]) -> eml23::AbstractValueArray {
    eml23::AbstractValueArray::StringXmlArray(eml23::StringXmlArray {
        values: values.iter().map(|s| s.to_string()).collect(),
        count_per_value: 1,
        ..Default::default()
    })
}

fn integers(values: &[i64]) -> eml23::AbstractValueArray {
    eml23::AbstractValueArray::IntegerXmlArray(eml23::IntegerXmlArray {
        values: values.to_vec(),
        null_value: -999,
        count_per_value: 1,
        ..Default::default()
    })
}

fn column(title: &str, values: eml23::AbstractValueArray) -> eml23::Column {
    eml23::Column {
        title: Some(title.to_string()),
        value_count_per_row: 1,
        values: Some(values),
        ..Default::default()
    }
}

fn object(uuid_tail: &str, keys: Vec<eml23::Column>, columns: Vec<eml23::Column>) -> eml23::ColumnBasedTable {
    eml23::ColumnBasedTable {
        uuid: format!("00000000-0000-0000-0000-{uuid_tail}"),
        key_column: keys,
        column: columns,
        ..Default::default()
    }
}

fn lookup(table: &eml23::ColumnBasedTable) -> Lookup {
    Lookup::read(table, &NoObjects).expect("the table reads")
}

fn sheeted(table: &eml23::ColumnBasedTable, uri: &str) -> (TableSheet, Vec<Result<ColumnCells<'static>, TableRefusal>>) {
    let read = Table::read(table).expect("the table reads");
    (TableSheet::of_table(&read, uri), read.read_cells(&NoObjects))
}

fn key_link(target: &str) -> (usize, Result<LinkDeclaration, LinkRefusal>) {
    (
        0,
        Ok(LinkDeclaration {
            column: 0,
            target: LinkTarget::Key {
                table_uri: target.to_string(),
            },
            convention: Convention::KindAndFacet,
            warnings: Vec::new(),
            key_named: None,
        }),
    )
}

/// The targets: a table keyed by the text `a`, `b`, and one keyed by the integers 1, 2, 3.
fn targets() -> TableSet {
    let mut set = TableSet::new();
    set.insert(
        TEXT_TARGET,
        None,
        lookup(&object(
            "00000000b002",
            vec![column("k", strings(&["a", "b"]))],
            vec![column("v", strings(&["A", "B"]))],
        )),
    );
    set.insert(
        INT_TARGET,
        None,
        lookup(&object(
            "00000000b003",
            vec![column("k", integers(&[1, 2, 3]))],
            vec![column("v", strings(&["A", "B", "C"]))],
        )),
    );
    set
}

#[test]
fn a_report_counts_found_dangling_empty_and_absent() {
    // House rule `house/table/link-resolves`: every non-empty value of a link column is a key.
    // A text column cannot be absent and an integer column cannot be empty: one of each.
    let table = object(
        "00000000b001",
        Vec::new(),
        vec![
            column("by_text", strings(&["a", "b", "", "z"])),
            column("by_number", integers(&[1, -999, 2, 3])),
        ],
    );
    let (sheet, cells) = sheeted(&table, OWN);
    let declarations = vec![
        key_link(TEXT_TARGET),
        (
            1,
            Ok(LinkDeclaration {
                column: 1,
                target: LinkTarget::Key {
                    table_uri: INT_TARGET.to_string(),
                },
                convention: Convention::KindAndFacet,
                warnings: Vec::new(),
                key_named: None,
            }),
        ),
    ];
    let report = check_links(&sheet, &cells, &declarations, &targets(), None);

    let text = &report.columns[0];
    assert_eq!(text.checked, 4);
    assert_eq!(text.found, 2);
    assert_eq!(text.dangling, vec![3]);
    assert_eq!(text.empty, 1, "an empty string is not a dangling key");
    assert_eq!(text.absent, 0);

    let number = &report.columns[1];
    assert_eq!(number.checked, 4);
    assert_eq!(number.found, 3);
    assert_eq!(number.absent, 1, "the array's NullValue is absence, not key -999");
    assert!(number.dangling.is_empty());

    assert!(!report.is_clean(), "a dangling key is not clean");
}

#[test]
fn a_duplicated_target_key_is_ambiguous_and_said() {
    let mut set = TableSet::new();
    set.insert(
        TEXT_TARGET,
        None,
        lookup(&object(
            "00000000b002",
            vec![column("k", strings(&["a", "a", "b"]))],
            vec![column("v", strings(&["A", "A2", "B"]))],
        )),
    );
    let table = object("00000000b001", Vec::new(), vec![column("l", strings(&["a", "b"]))]);
    let (sheet, cells) = sheeted(&table, OWN);
    let report = check_links(&sheet, &cells, &[key_link(TEXT_TARGET)], &set, None);
    assert_eq!(report.columns[0].ambiguous, vec![0], "'first match wins' is said, not silent");
    assert_eq!(report.columns[0].found, 1);
    assert!(!report.is_clean());
}

#[test]
fn a_kind_outside_identifier_is_followed_and_warned() {
    // Convention B on a real facet; the column's kind is `length`, which is no identifier.
    let facet = eml23::PropertyKindFacet {
        facet: eml23::FacetExt::Other(TEXT_TARGET.to_string()),
        kind: eml23::FacetKind::Qualifier,
    };
    let mut linking = column("l", strings(&["a", "b"]));
    linking.facet = vec![facet];
    linking.property_kind = eml23::DataObjectReference {
        uuid: LENGTH.to_string(),
        qualified_type: "eml23.PropertyKind".to_string(),
        title: "length".to_string(),
        ..Default::default()
    };
    let table = object("00000000b001", Vec::new(), vec![linking]);
    let (sheet, cells) = sheeted(&table, OWN);
    let kinds = resolve_kinds(&sheet, &Resolver::new(&NoObjects));
    let declarations = LinkReaders::builtin().declarations(&sheet, &kinds);
    assert_eq!(declarations.len(), 1);

    let report = check_links(&sheet, &cells, &declarations, &targets(), None);
    assert!(report.refused.is_empty(), "{:?}", report.refused);
    assert_eq!(report.columns[0].found, 2, "followed, not refused");
    assert!(
        report.columns[0].warnings.contains(&LinkWarning::KindNotAnIdentifier),
        "{:?}",
        report.columns[0].warnings
    );
}

#[test]
fn an_identifier_link_into_a_keyless_table_is_read_by_rank_and_warned() {
    // The kind says by key, the target has no key: the target decides (by rank), and it is said.
    let mut set = TableSet::new();
    set.insert(
        KEYLESS,
        None,
        lookup(&object(
            "00000000b004",
            Vec::new(),
            vec![column("v", strings(&["p", "q", "r"]))],
        )),
    );
    let table = object("00000000b001", Vec::new(), vec![column("l", integers(&[1]))]);
    let (sheet, cells) = sheeted(&table, OWN);
    let report = check_links(&sheet, &cells, &[key_link(KEYLESS)], &set, None);
    assert_eq!(report.columns[0].found, 1);
    assert!(report.columns[0].warnings.contains(&LinkWarning::KindDisagreesWithTarget));
    // A table with a key, linked by key: nothing to say.
    let report = check_links(&sheet, &cells, &[key_link(INT_TARGET)], &targets(), None);
    assert!(!report.columns[0].warnings.contains(&LinkWarning::KindDisagreesWithTarget));
}

#[test]
fn the_set_check_repeats_the_table_check() {
    let first = object("00000000b001", Vec::new(), vec![column("l", strings(&["a", "z"]))]);
    let second = object("00000000b005", Vec::new(), vec![column("l", integers(&[1, 2, 9]))]);
    let (sheet_one, cells_one) = sheeted(&first, OWN);
    let second_uri = "eml:///eml23.ColumnBasedTable(00000000-0000-0000-0000-00000000b005)";
    let (sheet_two, cells_two) = sheeted(&second, second_uri);
    let declarations_one = vec![key_link(TEXT_TARGET)];
    let declarations_two = vec![key_link(INT_TARGET)];
    let set = targets();

    let together = check_links_of_set(
        &[
            (&sheet_one, &cells_one[..], &declarations_one[..]),
            (&sheet_two, &cells_two[..], &declarations_two[..]),
        ],
        &set,
        None,
    );
    assert_eq!(together.len(), 2);
    assert_eq!(together[0], check_links(&sheet_one, &cells_one, &declarations_one, &set, None));
    assert_eq!(together[1], check_links(&sheet_two, &cells_two, &declarations_two, &set, None));
    // And the two are not the same report: the comparison above can fail.
    assert_ne!(together[0], together[1]);
}

/// A package holding one activity.
struct Package {
    held: eml23::Activity,
    asked: StdCell<usize>,
}

impl ObjectSource for Package {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        self.asked.set(self.asked.get() + 1);
        uuid.eq_ignore_ascii_case(&self.held.uuid)
            .then(|| Box::new(self.held.clone()) as Box<dyn DynObject>)
    }
}

#[test]
fn a_uri_column_reports_its_missing_objects() {
    let held = "00000000-0000-0000-0000-00000000b0a1";
    let absent = "00000000-0000-0000-0000-00000000b0a2";
    let package = Package {
        held: eml23::Activity {
            uuid: held.to_string(),
            ..Default::default()
        },
        asked: StdCell::new(0),
    };
    let table = object(
        "00000000b001",
        Vec::new(),
        vec![column(
            "epc_uri",
            strings(&[
                &format!("eml:///eml23.Activity({held})"),
                &format!("eml:///eml23.Activity({absent})"),
            ]),
        )],
    );
    let (sheet, cells) = sheeted(&table, OWN);
    let none = TableSet::new();

    // Without an object source the uri column is not looked at.
    let blind = check_links(&sheet, &cells, &[], &none, None);
    assert!(blind.uri_columns.is_empty());

    let report = check_links(&sheet, &cells, &[], &none, Some((&package as &dyn ObjectSource, None)));
    assert_eq!(report.uri_columns.len(), 1);
    let uris = &report.uri_columns[0];
    assert_eq!(uris.column, 0);
    assert_eq!(uris.present, 1);
    assert_eq!(uris.missing, vec![1]);
    assert_eq!(uris.other_dataspace, 0);
    assert!(uris.refused.is_empty());
    assert!(!report.is_clean(), "a missing object is not clean");
    // The value is kept as a cell of text: the check read it, it did not guess it.
    assert_eq!(
        cells[0].as_ref().expect("readable").cell(1),
        Some(energyml_domain::table::CellState::Value(Cell::Text(format!(
            "eml:///eml23.Activity({absent})"
        ))))
    );
}
