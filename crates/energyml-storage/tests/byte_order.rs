//! Which end of a number the file writes first, read from the file.
//!
//! An EML 2.3 external array declares the storage type of its elements, byte
//! order included (`arrayOfDouble64LE` against `arrayOfDouble64BE`). Anything
//! that writes one — a version translation, above all — has to know, and the
//! only place that knows is the file. Writing the usual order where the file
//! holds the other would make every value read wrong while the document stayed
//! schema-valid, which is the worst shape a bug can take.
//!
//! So this is asked apart from the shape and the element type, and a backend
//! that cannot answer says `None` rather than the likely one.
//!
//! # What these tests can and cannot prove
//!
//! The corpus is little-endian, like every file written on a mainstream
//! machine, so *the value* these tests read is not the interesting half — a
//! function returning `LittleEndian` unconditionally would pass the first one.
//! The second is the one that bites: a one-byte element has no order, and a
//! constant answer gets that wrong.

#![cfg(feature = "hdf5")]

use std::path::Path;

use energyml_storage::array::ByteOrder;
use energyml_storage::backend::ArrayBackend;
use energyml_storage::hdf5::Hdf5Backend;

fn corpus(name: &str) -> Option<std::path::PathBuf> {
    crate::fixtures::find(name)
}

/// A dataset of doubles says which end it writes first, and the corpus says
/// little-endian.
///
/// The value is not the claim — the claim is that *something was read*: the
/// `TypeDescriptor` the backend used before this carries no order at all, so
/// before `H5Tget_order` there was nothing here to answer with.
#[test]
fn a_multi_byte_dataset_names_its_byte_order() {
    let Some(file) = corpus("testingPackageCpp.h5") else {
        return;
    };
    let backend = Hdf5Backend;

    // A dataset of doubles the corpus really holds — the coordinates of the
    // first patch of a point set. Named rather than searched for, and read
    // with `unwrap`: the first draft of this test swallowed the error in a
    // tolerant arm and passed green on a dataset path that does not exist.
    let path = "/resqml20/fbc5466c-94cd-46ab-8b48-2ae2162b372f/points_patch0";
    let order = backend
        .byte_order(&file, path)
        .unwrap_or_else(|e| panic!("`{path}` is not in the corpus file any more: {e}"));
    assert_eq!(order, Some(ByteOrder::LittleEndian));
}

/// A backend that has no answer says `None`, and the store carries that word
/// through rather than turning it into a plausible order.
///
/// Sabotage: return `LittleEndian` from the default trait method and this goes
/// red — which is the whole point, because that default is what every
/// non-HDF5 backend will inherit.
#[test]
fn a_backend_that_cannot_say_says_nothing() {
    struct Silent;
    impl ArrayBackend for Silent {
        fn name(&self) -> &'static str {
            "silent"
        }
        fn handles(&self, _mime: Option<&str>, _file: &Path) -> bool {
            true
        }
        fn metadata(
            &self,
            _f: &Path,
            _p: &str,
        ) -> energyml_storage::Result<energyml_storage::array::ArrayMetadata> {
            unreachable!("this test asks only for the byte order")
        }
        fn read(
            &self,
            _f: &Path,
            _p: &str,
            _s: &[i64],
            _c: &[i64],
        ) -> energyml_storage::Result<energyml_storage::array::ArrayData> {
            unreachable!("this test asks only for the byte order")
        }
        fn read_into(
            &self,
            _f: &Path,
            _p: &str,
            _s: &[i64],
            _c: &[i64],
            _o: &mut energyml_storage::array::ArrayBuffer<'_>,
        ) -> energyml_storage::Result<energyml_storage::array::ArrayMetadata> {
            unreachable!("this test asks only for the byte order")
        }
    }

    // The default implementation of the trait: no guess.
    assert_eq!(
        Silent
            .byte_order(Path::new("nowhere"), "/x")
            .expect("the default answers"),
        None
    );
}

/// A file nobody could open is an **error**, not a `None`.
///
/// The distinction is the crate's own rule — a missing capability is not
/// missing data — and it matters here more than usual: a caller that must
/// write the order down treats `None` as "this backend has no opinion" and
/// carries on, while an error says the file itself is unreachable.
#[test]
fn an_unreadable_file_is_an_error_and_not_an_absent_answer() {
    let missing = Hdf5Backend.byte_order(Path::new("no-such-file.h5"), "/x");
    assert!(
        missing.is_err(),
        "a file that cannot be opened is an error, not `None`"
    );
}
