//! A shape that lies about how many numbers there are is refused.
//!
//! The failure this prevents is silent on both ends: a shape promising more
//! values than the block holds writes **without error** in some formats and
//! reads back with zeros at the tail - plausible numbers that nothing flags,
//! in a file that opens everywhere. The other direction is worse still: a
//! shape promising fewer quietly drops the rest.
//!
//! It is checked in the store rather than in each backend, and rather than in
//! whichever caller thought of it first: the file would be wrong, not the
//! caller. Every writer gets it - the command line, `energyml-build`, the
//! Python bridge - and none of them has to remember.
//!
//! No `hdf5` feature: the refusal comes before a backend is looked for, which
//! is itself worth pinning. A build with no backend must say *the shape is
//! wrong*, not *there is no backend* - the second sends someone to install a
//! library that would not have helped.

use std::sync::Arc;

use energyml_storage::EpcStore;
use energyml_storage::array::{ArrayData, ArrayLocation, ArrayMetadata, ArrayValues};

fn empty_store() -> EpcStore {
    let bytes = energyml_storage::extract::empty_package(None).expect("an empty package");
    EpcStore::from_reader(
        Box::new(std::io::Cursor::new(bytes)),
        "shapes.epc",
        Arc::new(energyml_schemas::all()),
    )
    .expect("it opens")
}

fn at(path: &str) -> ArrayLocation {
    ArrayLocation {
        file: "shapes.h5".to_string(),
        path_in_file: path.to_string(),
        ..Default::default()
    }
}

fn data(dimensions: Vec<i64>, values: usize) -> ArrayData {
    ArrayData {
        metadata: ArrayMetadata {
            dimensions,
            element_type: "double".to_string(),
        },
        values: ArrayValues::F64(vec![1.0; values]),
    }
}

#[test]
fn a_shape_promising_more_than_it_holds_is_refused_naming_both_counts() {
    let store = empty_store();
    let why = store
        .write_array(&at("/trial"), &data(vec![4, 4], 3))
        .expect_err("sixteen promised, three held");
    let said = format!("{why}");
    assert!(said.contains("16"), "the promised count: {said}");
    assert!(said.contains('3'), "and the one held: {said}");
    // The message must not send anyone looking for a backend.
    assert!(
        !said.contains("feature"),
        "this is not a missing capability: {said}"
    );
}

#[test]
fn a_shape_promising_fewer_is_refused_too() {
    let store = empty_store();
    // The direction that loses data rather than inventing it, and the one a
    // check written only against the first case would let through.
    let why = store
        .write_array(&at("/trial"), &data(vec![2], 6))
        .expect_err("two promised, six held");
    let said = format!("{why}");
    assert!(said.contains('2') && said.contains('6'), "both counts: {said}");
}

#[test]
fn a_shape_that_agrees_gets_past_the_check() {
    let store = empty_store();
    // It fails - there is no backend here - but it must fail for *that*, which
    // is what says the check let it through. Without this, a check that refused
    // everything would look just as green.
    let why = store
        .write_array(&at("/trial"), &data(vec![2, 3], 6))
        .expect_err("no backend in this build");
    let said = format!("{why}");
    assert!(
        !said.contains("promises"),
        "an agreeing shape was refused as if it disagreed: {said}"
    );
}
