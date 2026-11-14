//! `values::null_value`: the one reading of the `NullValue` of an integer array.
//!
//! Sources: EML TRG 2.3 section 6.41 p. 127 (`IntegerExternalArray`: "The null
//! value must be explicitly provided in the NullValue attribute of this
//! class"), section 6.44 p. 130 (`IntegerXmlArray`), RESQML TRG 2.0.1 section
//! 6.28 p. 73-74 (`IntegerHdf5Array`). The arrays are built by hand.

use energyml_domain::values::null_value;
use energyml_eml_2_3::{FloatingPointXmlArray, IntegerExternalArray, IntegerXmlArray};
use energyml_resqml_2_0_1::IntegerHdf5Array;

/// Break it: make `null_value` return `None` for every array.
#[test]
fn an_integer_array_says_its_null_value() {
    let external = IntegerExternalArray {
        null_value: -999,
        ..Default::default()
    };
    assert_eq!(null_value(&external), Some(-999));

    let inline = IntegerXmlArray {
        null_value: 0,
        values: vec![1],
        ..Default::default()
    };
    assert_eq!(null_value(&inline), Some(0));

    let hdf5 = IntegerHdf5Array {
        null_value: i64::MIN,
        ..Default::default()
    };
    assert_eq!(null_value(&hdf5), Some(i64::MIN));
}

/// Break it: make `null_value` read a `NullValue` field on any array, a float one included.
#[test]
fn a_floating_point_array_has_no_null_value() {
    assert_eq!(null_value(&FloatingPointXmlArray::default()), None);
}
