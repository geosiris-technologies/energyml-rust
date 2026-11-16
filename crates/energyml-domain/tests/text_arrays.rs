//! String arrays read from a file: the strings, or a refusal that names the
//! cause - never an empty column that looks like a column with no values.
//!
//! Sources: EML TRG 2.3 section 6.63 p. 149 ("The datatype of the values will
//! be identified by means of the HDF5 API"); Common TRG 2.3 section 9.1 p. 190
//! ("Ordering matters since it will be the same in all columns"). The source
//! is the test's own: a map of datasets, as in `channel_data_in_hdf5`.

use std::collections::BTreeMap;

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::source::{ArraySource, ObjectSource};
use energyml_domain::table::Lookup;
use energyml_domain::values::{self, Values};
use energyml_eml_2_3 as eml;
use energyml_storage::{
    ArrayBuffer, ArrayData, ArrayLocation, ArrayMetadata, ArrayValues, ExternalDataReason,
};

/// Datasets by `(file, path)`, located with the storage's own locator.
struct Held {
    held: BTreeMap<(String, String), ArrayData>,
}

impl Held {
    fn of(items: Vec<(&str, ArrayValues)>) -> Held {
        let mut held = BTreeMap::new();
        for (path, values) in items {
            let dimensions = vec![values.len() as i64];
            let element_type = values.element_type().to_string();
            held.insert(
                ("t.h5".to_string(), path.to_string()),
                ArrayData {
                    metadata: ArrayMetadata::new(dimensions, element_type),
                    values,
                },
            );
        }
        Held { held }
    }
}

impl ObjectSource for Held {
    fn object(&self, _uuid: &str) -> Option<Box<dyn DynObject>> {
        None
    }
}

impl ArraySource for Held {
    fn locate(&self, object: &dyn Reflect) -> Vec<ArrayLocation> {
        energyml_storage::array::locate(object)
    }
    fn metadata(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayMetadata> {
        self.read(location).map(|data| data.metadata)
    }
    fn read(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayData> {
        self.held
            .get(&(location.file.clone(), location.path_in_file.clone()))
            .cloned()
            .ok_or_else(|| {
                energyml_storage::Error::ExternalData {
                    location: format!("{}:{}", location.file, location.path_in_file),
                    reason: ExternalDataReason::PathMissing(location.path_in_file.clone()),
                }
                .into()
            })
    }
    fn read_into(
        &self,
        _location: &ArrayLocation,
        _out: &mut ArrayBuffer<'_>,
    ) -> energyml_domain::Result<ArrayMetadata> {
        Err(energyml_domain::Error::unsupported(
            "reading into a buffer",
            "this test's source reads whole",
        ))
    }
}

fn part(path: &str, count: i64) -> eml::ExternalDataArrayPart {
    eml::ExternalDataArrayPart {
        count: vec![count],
        path_in_external_file: path.into(),
        start_index: vec![0],
        uri: "t.h5".into(),
        mime_type: Some("application/x-hdf5".into()),
    }
}

fn string_array(parts: Vec<eml::ExternalDataArrayPart>) -> eml::StringExternalArray {
    eml::StringExternalArray {
        count_per_value: 1,
        values: eml::ExternalDataArray {
            external_data_array_part: parts,
        },
        ..Default::default()
    }
}

fn texts(values: &[&str]) -> ArrayValues {
    ArrayValues::Text(values.iter().map(|s| (*s).to_string()).collect())
}

fn owned(values: &[&str]) -> Vec<String> {
    values.iter().map(|s| (*s).to_string()).collect()
}

/// Break it: restore `Family::Text => Values::Text(Vec::new())` in `widen`.
#[test]
fn a_string_array_in_a_file_reads_its_strings() {
    let array = string_array(vec![part("/s", 3)]);
    let held = Held::of(vec![("/s", texts(&["a", "", "c"]))]);
    let read = values::read(&array, &held, "column").expect("the strings are read");
    // The empty string is a value, not a hole.
    assert_eq!(read, Values::Text(owned(&["a", "", "c"])));
}

/// Break it: make `widen` answer `Ok(Values::Text(vec![]))` for the `Family::Text` / numbers case.
#[test]
fn a_string_array_read_as_numbers_is_refused_not_emptied() {
    let array = string_array(vec![part("/s", 3)]);
    // What a backend that does not decode strings answers.
    let held = Held::of(vec![("/s", ArrayValues::F64(vec![1.0, 2.0, 3.0]))]);
    let error = values::read(&array, &held, "column").expect_err("a refusal");
    let message = format!("{error}");
    assert!(
        message.contains("string") && message.contains("double"),
        "{message}"
    );
}

/// Break it: let `numbers` answer `Ok(Vec::new())` for `Text`.
#[test]
fn a_numeric_array_whose_file_holds_strings_is_refused() {
    let array = eml::FloatingPointExternalArray {
        count_per_value: 1,
        values: eml::ExternalDataArray {
            external_data_array_part: vec![part("/s", 2)],
        },
        ..Default::default()
    };
    let held = Held::of(vec![("/s", texts(&["a", "b"]))]);
    let error = values::read(&array, &held, "column").expect_err("a refusal");
    let message = format!("{error}");
    assert!(message.contains("holds strings"), "{message}");
}

/// Break it: restore the old `widen`, which emptied the column; the table then has 0 rows.
#[test]
fn one_unreadable_text_column_does_not_empty_the_table() {
    let key = eml::Column {
        title: Some("code".into()),
        value_count_per_row: 1,
        values: Some(eml::AbstractValueArray::IntegerXmlArray(eml::IntegerXmlArray {
            values: vec![1, 2, 3],
            count_per_value: 1,
            ..Default::default()
        })),
        ..Default::default()
    };
    let name = eml::Column {
        title: Some("name".into()),
        value_count_per_row: 1,
        values: Some(eml::AbstractValueArray::StringExternalArray(string_array(vec![
            part("/s", 3),
        ]))),
        ..Default::default()
    };
    let table = eml::ColumnBasedTable {
        uuid: "11111111-2222-4333-8444-555555555555".into(),
        key_column: vec![key],
        column: vec![name],
        ..Default::default()
    };
    // The backend answers numbers for the string dataset, as a browser's can.
    let held = Held::of(vec![("/s", ArrayValues::F64(vec![1.0, 2.0, 3.0]))]);
    let lookup = Lookup::read(&table, &held).expect("the table is read");
    assert_eq!(lookup.rows.rows, 3);
    assert!(lookup.rows.columns[1].is_none());
    let notes: Vec<&String> = lookup.rows.notes.iter().chain(lookup.notes.iter()).collect();
    assert!(notes.iter().any(|note| note.contains("name")), "{notes:?}");
}

/// Break it: remove the `Text` arm of `append`; the joined array is then `["a", "b"]`.
#[test]
fn the_parts_of_a_string_array_are_joined() {
    let array = string_array(vec![part("/s1", 2), part("/s2", 1)]);
    let held = Held::of(vec![("/s1", texts(&["a", "b"])), ("/s2", texts(&["c"]))]);
    let read = values::read(&array, &held, "column").expect("the parts are joined");
    assert_eq!(read, Values::Text(owned(&["a", "b", "c"])));
}
