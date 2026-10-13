//! Tables, against the one the corpus contains.
//!
//! `866841eb` - the KrPc table of `testingPackageCpp22` and `80wells` - is the
//! only `ColumnBasedTable` in the whole corpus, and it carries the trap worth
//! pinning: its three value columns point at `column1`, `column2` and
//! `column3`, and its **key** column points at `column0`. The key is written
//! last in the XML and stored first in the file, so anything that derives the
//! dataset order from the document order reads every value against the wrong
//! key while looking perfectly healthy.
//!
//! Reading the values needs a backend, so the parts that do are behind the
//! `hdf5` feature; the shape of the table is checked without one.

use std::path::PathBuf;
use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_domain::table::Table;
use energyml_storage::EpcStore;

/// Every schema version the workspace generates.
///
/// One line rather than a hand-kept subset: a list written out here goes stale
/// the day a release is added, and nothing would notice - the suite would keep
/// passing while quietly never covering the new version. `energyml-schemas`
/// holds the one list, and its own test fails when it disagrees with
/// `codegen.toml`.
fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

/// The public reference packages; see `fixtures.rs`.
fn corpus() -> Vec<PathBuf> {
    crate::fixtures::public_corpus()
}

/// Every object of the corpus that is a lookup table of either shape.
fn tables() -> Vec<(PathBuf, EpcStore, String)> {
    let registry = registry();
    let mut out = Vec::new();
    for path in corpus() {
        let Ok(store) = EpcStore::open(&path, registry.clone()) else {
            continue;
        };
        let found: Vec<String> = store
            .resources()
            .iter()
            .filter(|summary| {
                let ty = &summary.qualified_type;
                ty.ends_with("ColumnBasedTable")
                    || ty.ends_with("StringTableLookup")
                    || ty.ends_with("DoubleTableLookup")
            })
            .map(|summary| summary.uri.clone())
            .collect();
        for uri in found {
            out.push((
                path.clone(),
                EpcStore::open(&path, registry.clone()).unwrap(),
                uri,
            ));
        }
        let _ = store;
    }
    out
}

#[test]
fn every_table_of_the_corpus_reads_and_declares_its_keys() {
    let tables = tables();
    if tables.is_empty() {
        eprintln!("skipped: no lookup table under testdata/");
        return;
    }
    for (path, store, uri) in &tables {
        let object = store.get_object(uri).expect("the table deserializes");
        let table = Table::read(object.as_reflect())
            .unwrap_or_else(|e| panic!("{}: {uri} did not read: {e}", path.display()));

        assert!(!table.columns.is_empty());
        // A key column is what makes it a lookup rather than a list; the one
        // in the corpus has exactly one.
        assert!(
            !table.keys().is_empty(),
            "{}: {} has no key column, so nothing can be looked up in it",
            path.display(),
            table.uuid
        );
        // Keys come first, whatever order the document wrote them in - the
        // corpus writes the key **last**.
        let first_value = table.columns.iter().position(|c| !c.key).unwrap_or(0);
        let last_key = table.columns.iter().rposition(|c| c.key).unwrap_or(0);
        assert!(
            last_key < first_value || table.values().is_empty(),
            "{}: the key columns must come first",
            path.display()
        );
        eprintln!(
            "{}: `{}` - {} key(s), {} value column(s)",
            path.file_name().unwrap().to_string_lossy(),
            table.title.as_deref().unwrap_or(&table.uuid),
            table.keys().len(),
            table.values().len()
        );
    }
}

#[test]
fn each_column_names_what_it_measures() {
    // `Column.PropertyKind` is mandatory in a `ColumnBasedTable`: without it a
    // column of numbers means nothing at all.
    let tables = tables();
    if tables.is_empty() {
        eprintln!("skipped: no lookup table under testdata/");
        return;
    }
    for (path, store, uri) in &tables {
        let object = store.get_object(uri).expect("it deserializes");
        let table = Table::read(object.as_reflect()).expect("it reads");
        if !table.qualified_type.ends_with("ColumnBasedTable") {
            continue;
        }
        for column in &table.columns {
            assert!(
                column.kind.is_some(),
                "{}: a column of `{}` names no property kind",
                path.display(),
                table.uuid
            );
            assert!(column.values_per_row >= 1);
        }
    }
}

#[test]
fn a_table_that_is_not_a_table_is_refused_by_name() {
    // A `CategoryLookup` is an untyped reference in the XSD; following it into
    // the wrong object must say what it found, not "not supported".
    let files = corpus();
    if files.is_empty() {
        eprintln!("skipped: no EPC under testdata/");
        return;
    }
    let registry = registry();
    for path in &files {
        let store = EpcStore::open(path, registry.clone()).expect("the package opens");
        let Some(summary) = store
            .resources()
            .iter()
            .find(|s| s.qualified_type.contains("Citation") || s.qualified_type.ends_with("Property"))
            .cloned()
        else {
            continue;
        };
        let object = store.get_object(&summary.uri).expect("it deserializes");
        let message = match Table::read(object.as_reflect()) {
            Err(e) => e.to_string(),
            Ok(_) => panic!("a property is not a lookup table"),
        };
        assert!(
            message.contains("ColumnBasedTable") && message.contains("2.0.1"),
            "the refusal has to say what a lookup may be: {message}"
        );
        return;
    }
}

#[test]
#[cfg(feature = "hdf5")]
fn the_key_column_is_the_one_the_document_says_not_the_first_dataset() {
    // The trap: in `866841eb` the key is `column0` and the values are
    // `column1..3`, while the XML writes the key **last**. Reading by document
    // order would line every value up against the wrong key.
    use energyml_domain::table::{Cell, Lookup};

    let tables = tables();
    if tables.is_empty() {
        eprintln!("skipped: no lookup table under testdata/");
        return;
    }
    let mut checked = 0usize;

    for (path, store, uri) in &tables {
        let object = store.get_object(uri).expect("it deserializes");
        let table = Table::read(object.as_reflect()).expect("it reads");
        if !table.qualified_type.ends_with("ColumnBasedTable") {
            continue;
        }
        let lookup = Lookup::read(object.as_reflect(), store).expect("its values read");
        assert!(lookup.notes.is_empty(), "{}: {:?}", path.display(), lookup.notes);
        assert!(lookup.rows.rows > 0);
        assert_eq!(lookup.key_columns, vec![0], "the key column comes first");

        // The KrPc table: water saturation → relative permeabilities and
        // capillary pressure. Saturations rise, so the first key is the
        // smallest - which is only true if the key really is `column0`.
        let first = lookup.rows.cell(0, 0).expect("a first key");
        let last = lookup.rows.cell(lookup.rows.rows - 1, 0).expect("a last key");
        if let (Cell::Number(first), Cell::Number(last)) = (first, last) {
            assert!(
                first < last,
                "{}: the key column is not ordered; the wrong dataset was read as the key",
                path.display()
            );
            // And every key of the table finds itself.
            let found = lookup.of(first).expect("the first key finds its own row");
            assert_eq!(found.len(), lookup.rows.columns.len() - 1);
        }
        assert!(
            lookup.duplicate_keys().is_empty(),
            "{}: this table has duplicate keys: {:?}",
            path.display(),
            lookup.duplicate_keys()
        );
        checked += 1;
    }
    assert!(checked > 0, "no column-based table was read with its values");
}

#[test]
#[cfg(feature = "hdf5")]
fn a_property_reaches_its_lookup_and_the_integers_gain_a_meaning() {
    // The end of the chain, and the reason the lot exists: a discrete property
    // whose values are indices, and the table that says what they index.
    use energyml_domain::property;
    use energyml_domain::table::{Cell, Lookup};

    let files = corpus();
    if files.is_empty() {
        eprintln!("skipped: no EPC under testdata/");
        return;
    }
    let registry = registry();
    let mut linked = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry.clone()).expect("the package opens");
        for summary in store.resources() {
            if !summary.qualified_type.ends_with("Property") {
                continue;
            }
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            let Ok(read) = property::read(object.as_reflect()) else {
                continue;
            };
            let Some(lookup_uuid) = read.lookup.as_deref() else {
                continue;
            };
            let target = store.get_object(lookup_uuid).unwrap_or_else(|e| {
                panic!(
                    "{}: {} points at a lookup that is not here: {e}",
                    path.display(),
                    read.uuid
                )
            });
            let lookup = Lookup::read(target.as_reflect(), &store).unwrap_or_else(|e| {
                panic!(
                    "{}: the lookup of {} did not read: {e}",
                    path.display(),
                    read.uuid
                )
            });
            // Every value of the property should be a key of the table - that
            // is what a category lookup is for. Not asserted, because a
            // producer may leave values uncovered, but a value that *is* a key
            // must find its row.
            for patch in &read.patches {
                let Ok(values) = patch.read(&store) else { continue };
                // Whatever family the property stores, a value that *is* a
                // key of the table has to find its row.
                let numbers: Vec<f64> = match (values.floating(), values.integer()) {
                    (Some(v), _) => v.to_vec(),
                    (_, Some(v)) => v.iter().map(|n| *n as f64).collect(),
                    _ => Vec::new(),
                };
                for value in numbers.iter().take(32) {
                    if !lookup.find(&[Cell::Number(*value)]).is_empty() {
                        assert!(lookup.of(*value).is_some());
                    }
                }
            }
            linked += 1;
        }
    }
    eprintln!("{linked} property(ies) reached their lookup table");
}
