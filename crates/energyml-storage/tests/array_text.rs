//! `ArrayValues::Text`: strings read from a file travel as strings.
//!
//! Source: EML TRG 2.3 section 6.63 p. 149 and RESQML TRG 2.0.1 section 6.39
//! p. 78: "The datatype of the values will be identified by means of the HDF5
//! API". The data are built by hand, no file is read.

use energyml_storage::array::{ArrayData, ArrayMetadata, ArrayValues};
use energyml_storage::window::stats_of;

fn texts(values: &[&str]) -> ArrayValues {
    ArrayValues::Text(values.iter().map(|s| (*s).to_string()).collect())
}

/// Break it: make `to_f64` return `vec![0.0; v.len()]` for `Text`; the `is_nan` assertion fails.
#[test]
fn a_text_block_counts_its_strings_and_lends_no_bytes() {
    let values = texts(&["sand", "", "shale"]);
    assert_eq!(values.len(), 3);
    assert_eq!(values.element_type(), "string");
    assert!(values.as_bytes().is_empty());
    let numbers = values.to_f64();
    assert_eq!(numbers.len(), 3);
    assert!(numbers.iter().all(|x| x.is_nan()));
    // The empty string is a value, not a hole.
    assert_eq!(
        values.text(),
        Some(&["sand".to_string(), String::new(), "shale".to_string()][..])
    );
    // Numbers are not text.
    assert_eq!(ArrayValues::F64(vec![1.0]).text(), None);
}

/// Break it: make the `Text` branch of `stats_of` return `Some(fold(std::iter::empty()))`.
#[test]
fn a_text_window_has_no_statistics() {
    let data = ArrayData {
        metadata: ArrayMetadata::new(vec![2], "string"),
        values: texts(&["a", "b"]),
    };
    assert!(stats_of(&data).is_none());
    assert_eq!(data.text().map(<[String]>::len), Some(2));
}
