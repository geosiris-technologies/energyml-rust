//! The sheet of a table: titles, kinds, facets, families and counts without
//! reading one array, and the package index that keeps one per table (WP06 §4.3).
//!
//! Common TRG 2.3 §9.1 Column p. 190 (`KeyColumn`, `ValueCountPerRow`, `Facet`),
//! §11.3 p. 285 (`qualifier`), §11.11 p. 294 (`PropertyKindFacet`). The packages
//! are built in memory, never read from a file.

use std::io::Cursor;
use std::sync::Arc;

use energyml_core::registry::DynObject;
use energyml_domain::package_index::{PackageIndex, SheetKind};
use energyml_domain::table::{Table, TableSheet};
use energyml_domain::values::Family;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_storage::EpcStore;

const TABLE: &str = "00000000-0000-0000-0000-00000000a501";
const OTHER: &str = "00000000-0000-0000-0000-00000000a502";
const KIND: &str = "311dcaba-6512-4144-aa7c-79a9de19b4ab";

fn citation(title: &str) -> eml23::Citation {
    eml23::Citation {
        title: title.to_string(),
        originator: "test".to_string(),
        creation: "2026-11-14T00:00:00Z".to_string(),
        format: "test".to_string(),
        ..Default::default()
    }
}

fn kind_reference() -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: KIND.to_string(),
        qualified_type: "eml23.PropertyKind".to_string(),
        title: "identifier".to_string(),
        ..Default::default()
    }
}

fn integers(values: &[i64]) -> Option<eml23::AbstractValueArray> {
    Some(eml23::AbstractValueArray::IntegerXmlArray(eml23::IntegerXmlArray {
        values: values.to_vec(),
        null_value: -999,
        count_per_value: 1,
        ..Default::default()
    }))
}

fn strings(values: &[&str]) -> Option<eml23::AbstractValueArray> {
    Some(eml23::AbstractValueArray::StringXmlArray(eml23::StringXmlArray {
        values: values.iter().map(|v| v.to_string()).collect(),
        count_per_value: 1,
        ..Default::default()
    }))
}

/// A key `code` (3 integers, kind `identifier`) and a column `name` of 3
/// strings with one `qualifier` facet.
fn table() -> eml23::ColumnBasedTable {
    eml23::ColumnBasedTable {
        uuid: TABLE.to_string(),
        citation: citation("lithologies"),
        key_column: vec![eml23::Column {
            title: Some("code".into()),
            value_count_per_row: 1,
            property_kind: kind_reference(),
            values: integers(&[1, 2, 3]),
            ..Default::default()
        }],
        column: vec![eml23::Column {
            title: Some("name".into()),
            value_count_per_row: 1,
            property_kind: kind_reference(),
            values: strings(&["a", "b", "c"]),
            facet: vec![eml23::PropertyKindFacet {
                facet: eml23::FacetExt::Other("eml:///eml23.ColumnBasedTable".to_string()),
                kind: eml23::FacetKind::Qualifier,
            }],
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[test]
fn a_table_sheet_says_keys_kinds_facets_and_counts_without_an_array() {
    let object = table();
    let sheet = TableSheet::of(&object).expect("a table that reads");
    assert!(
        sheet.uri.starts_with("eml:///eml23.ColumnBasedTable("),
        "{}",
        sheet.uri
    );
    assert!(sheet.uri.contains(TABLE), "{}", sheet.uri);
    assert_eq!(sheet.columns.len(), 2);

    let key = &sheet.columns[0];
    assert!(key.key);
    assert_eq!(key.family, Some(Family::Integer));
    assert_eq!(key.values, Some(3));
    assert!(!key.external);
    assert!(matches!(
        &key.kind,
        Some(SheetKind::Object { uuid, .. }) if &**uuid == KIND
    ));

    let name = &sheet.columns[1];
    assert!(!name.key);
    assert_eq!(name.family, Some(Family::Text));
    assert_eq!(name.facets[0].kind, "qualifier");
    assert_eq!(sheet.row_count(), Some(3));
    assert_eq!(sheet.key_columns(), vec![0]);
}

#[test]
fn an_external_column_has_no_count_and_no_row_count() {
    // The values live in a file: counting them needs its header, so the sheet
    // says nothing rather than a guessed 0.
    let mut object = table();
    object.column[0].values = Some(eml23::AbstractValueArray::FloatingPointExternalArray(
        eml23::FloatingPointExternalArray {
            count_per_value: 1,
            ..Default::default()
        },
    ));
    let sheet = TableSheet::of_table(&Table::read(&object).expect("it reads"), "eml:///x");
    let column = &sheet.columns[1];
    assert_eq!(column.values, None);
    assert!(column.external);
    assert_eq!(column.family, Some(Family::Floating));
    assert_eq!(sheet.row_count(), None);
}

#[test]
fn a_sheet_recognises_a_table_by_its_column_titles() {
    let mut object = table();
    object.key_column[0].title = Some("nwdf_uid".into());
    object.column[0].title = Some("role".into());
    object.column.push(eml23::Column {
        title: Some("epc_uri".into()),
        value_count_per_row: 1,
        property_kind: kind_reference(),
        values: strings(&["x", "y", "z"]),
        ..Default::default()
    });
    let sheet = TableSheet::of(&object).expect("it reads");
    assert!(sheet.has_titles(&["role", "epc_uri"]));
    assert!(!sheet.has_titles(&["role", "missing"]));
    assert!(!sheet.has_titles(&[]), "an empty list recognises nothing");
    assert_eq!(sheet.column_titled("epc_uri"), Some(2));
}

// -- the index ----------------------------------------------------------------

fn store() -> EpcStore {
    EpcStore::from_reader(
        Box::new(Cursor::new(
            energyml_storage::extract::empty_package(None).expect("an empty package"),
        )),
        "table_sheet.epc",
        Arc::new(energyml_schemas::all()),
    )
    .expect("it opens")
}

fn put(store: &mut EpcStore, object: impl DynObject + 'static) {
    store.put(Box::new(object)).expect("it goes in");
}

#[test]
fn the_index_keeps_a_sheet_per_table_and_drops_it_with_the_table() {
    let mut store = store();
    let object = table();
    // The title is read from the generated struct's own field, not through
    // `TableSheet::of`: two readings that share nothing.
    let expected_title = object.citation.title.clone();
    put(&mut store, object);
    put(
        &mut store,
        resqml22::WellboreFeature {
            uuid: OTHER.to_string(),
            schema_version: "2.2".into(),
            citation: citation("not a table"),
            ..Default::default()
        },
    );

    let mut index = PackageIndex::build(&store, 1);
    assert_eq!(index.tables().count(), 1, "the feature is not a table");
    let sheet = index.table_sheet(TABLE).expect("the table has a sheet");
    assert_eq!(sheet.title.as_deref(), Some(expected_title.as_str()));
    assert_eq!(sheet.columns.len(), 2);
    assert!(index.table_sheet(OTHER).is_none());

    let uri = store
        .find_by_uuid(TABLE)
        .expect("the table is in the store")
        .uri
        .clone();
    store.remove(&uri).expect("it is removed");
    index.update(&store, &[TABLE.to_string()], 2);
    assert_eq!(index.tables().count(), 0, "the sheet goes with the table");
    assert!(index.table_sheet(TABLE).is_none());
}
