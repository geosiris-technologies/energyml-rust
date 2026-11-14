//! String datasets read from HDF5, proved with datasets created by the `hdf5`
//! crate itself, never by the backend under test.
//!
//! Source: EML TRG 2.3 section 6.63 p. 149 and RESQML TRG 2.0.1 section 6.39
//! p. 78: "The datatype of the values will be identified by means of the HDF5
//! API". Needs `--features hdf5`.

#![cfg(feature = "hdf5")]

use std::path::PathBuf;
use std::str::FromStr;

use energyml_storage::array::{ArrayBuffer, ArrayData, ArrayMetadata, ArrayValues};
use energyml_storage::backend::ArrayBackend;
use energyml_storage::hdf5::Hdf5Backend;
use hdf5::types::{FixedAscii, VarLenUnicode};

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("energyml-strings-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a temporary folder");
    dir
}

fn varlen(values: &[&str]) -> Vec<VarLenUnicode> {
    values.iter().map(|s| VarLenUnicode::from_str(s).expect("a valid string")).collect()
}

fn text(values: &[&str]) -> ArrayValues {
    ArrayValues::Text(values.iter().map(|s| (*s).to_string()).collect())
}

/// The file the tests read: the writing handle is dropped before the backend opens it.
fn fixture(name: &str) -> PathBuf {
    let dir = folder(name);
    let path = dir.join("strings.h5");
    {
        let file = hdf5::File::create(&path).expect("an HDF5 file");
        file.create_group("t").expect("a group");

        let words = ["sandstone", "", "shale", "gr\u{e8}s"];
        file.new_dataset::<VarLenUnicode>()
            .shape(4)
            .create("/t/varlen")
            .expect("a dataset")
            .write_raw(&varlen(&words))
            .expect("the values");

        let sixteen = [
            FixedAscii::<16>::from_ascii(b"0123456789abcdef").expect("16 characters"),
            FixedAscii::<16>::from_ascii(b"ab").expect("2 characters"),
        ];
        file.new_dataset::<FixedAscii<16>>()
            .shape(2)
            .create("/t/fixed16")
            .expect("a dataset")
            .write_raw(&sixteen)
            .expect("the values");

        let twenty = [FixedAscii::<20>::from_ascii(b"twenty-characters!!!").expect("20 characters")];
        file.new_dataset::<FixedAscii<20>>()
            .shape(1)
            .create("/t/fixed20")
            .expect("a dataset")
            .write_raw(&twenty)
            .expect("the values");

        let long = [FixedAscii::<2000>::from_ascii(b"x").expect("one character")];
        file.new_dataset::<FixedAscii<2000>>()
            .shape(1)
            .create("/t/long")
            .expect("a dataset")
            .write_raw(&long)
            .expect("the values");
    }
    path
}

/// Break it: make the `VarLenUnicode` arm of `read` return `Vec::new()`.
#[test]
fn variable_length_strings_read_as_text_and_an_empty_one_is_a_value() {
    let path = fixture("varlen");
    let read = Hdf5Backend.read(&path, "/t/varlen", &[], &[]).expect("a read");
    assert_eq!(read.values, text(&["sandstone", "", "shale", "gr\u{e8}s"]));
    assert_eq!(read.metadata.element_type, "string");
    assert_eq!(read.metadata.dimensions, vec![4]);
}

/// Break it: declare `FixedAscii<20>` datasets in a `0..=15` bucket arm
/// (a 16-byte bucket truncates the 20 characters and the assertion fails).
#[test]
fn fixed_length_strings_are_read_whole_at_a_bucket_edge() {
    let path = fixture("fixed");
    let edge = Hdf5Backend.read(&path, "/t/fixed16", &[], &[]).expect("a read");
    assert_eq!(edge.values, text(&["0123456789abcdef", "ab"]));
    let over = Hdf5Backend.read(&path, "/t/fixed20", &[], &[]).expect("a read");
    assert_eq!(over.values, text(&["twenty-characters!!!"]));
    assert_eq!(over.metadata.element_type, "string");
}

/// Break it: make `strings` ignore `selection` and read the whole dataset.
#[test]
fn a_window_of_strings_reads_the_selected_rows() {
    let path = fixture("window");
    let read = Hdf5Backend.read(&path, "/t/varlen", &[1], &[2]).expect("a read");
    assert_eq!(read.values, text(&["", "shale"]));
    assert_eq!(read.metadata.dimensions, vec![2]);
}

/// Break it: make the last arm of `fixed_ascii` return `Ok(Vec::new())`.
#[test]
fn strings_longer_than_the_backend_decodes_are_refused_by_name() {
    let path = fixture("long");
    let error = Hdf5Backend.read(&path, "/t/long", &[], &[]).unwrap_err();
    let message = format!("{error}");
    assert!(message.contains("2000") && message.contains("1024"), "{message}");
}

/// Break it: move the `Text` guard of `write_into` after the `unlink`.
#[test]
fn strings_are_not_written_and_an_existing_dataset_survives() {
    let dir = folder("write");
    let path = dir.join("out.h5");
    let backend = Hdf5Backend;
    let numbers = ArrayData {
        metadata: ArrayMetadata::new(vec![2], "double"),
        values: ArrayValues::F64(vec![1.0, 2.0]),
    };
    backend.write(&path, "/t/kept", &numbers).expect("the numbers are written");

    let strings = ArrayData {
        metadata: ArrayMetadata::new(vec![1], "string"),
        values: text(&["x"]),
    };
    let error = backend.write(&path, "/t/kept", &strings).unwrap_err();
    assert!(format!("{error}").contains("strings"), "{error}");

    let after = backend.read(&path, "/t/kept", &[], &[]).expect("still readable");
    assert_eq!(after.values, ArrayValues::F64(vec![1.0, 2.0]));
}

/// Break it: let `read_into` skip the element type comparison.
#[test]
fn strings_do_not_read_into_a_numeric_buffer() {
    let path = fixture("into");
    let mut slots = [0.0_f64; 4];
    let error = Hdf5Backend
        .read_into(&path, "/t/varlen", &[], &[], &mut ArrayBuffer::F64(&mut slots))
        .unwrap_err();
    assert!(format!("{error}").contains("string"), "{error}");
}
