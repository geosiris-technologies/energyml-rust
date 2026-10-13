//! Copying objects, and what they need, from one package into another.
//!
//! The operation resqpy has and this workspace did not, and it is the one that
//! would fail *quietly*: a copy that took the surface and left its CRS behind
//! produces a package that opens, lists an object, and places it nowhere.
//!
//! So the checks are about what came **along**, and one of them walks the
//! arrived package with `references` - each object's own DORs - rather than
//! with the graph the copy closed on. Two walks, written apart, and the second
//! is what would catch the first getting it wrong.

#![cfg(feature = "hdf5")]

use std::path::PathBuf;

use energyml_rust::session::Package;

fn corpus(name: &str) -> Option<PathBuf> {
    crate::fixtures::find_or_skip(name)
}

fn source() -> Option<Package> {
    let path = corpus("testingPackageCpp22.epc")?;
    Some(Package::open(&path).expect("the package opens"))
}

#[test]
fn a_surface_arrives_with_what_it_needs_and_the_rest_is_said() {
    let Some(from) = source() else { return };
    let Some(surface) = from
        .find(Some("TriangulatedSetRepresentation"), None)
        .into_iter()
        .next()
    else {
        eprintln!("skipped: no triangulated set in this package");
        return;
    };

    let dir = tempfile::tempdir().expect("a directory for this test");
    let mut into = Package::create("copy.epc").expect("an empty package");
    into.set_data_root(dir.path());

    let done = into
        .copy_from(&from, std::slice::from_ref(&surface.uri), "copy")
        .expect("the copy is made");

    // **The point of the whole operation.** One object was asked for and more
    // than one arrived; a copy that took only what was named would leave a
    // representation with no interpretation and no CRS - an object that opens
    // and means nothing.
    assert!(done.objects.contains(&surface.uri), "what was asked for is there");
    assert!(
        !done.added.is_empty(),
        "and its closure came along: {:?}",
        done.added
    );
    assert!(
        !done.added.contains(&surface.uri),
        "what was asked for is not *added*"
    );
    assert_eq!(
        done.objects.len(),
        done.added.len() + 1,
        "everything copied was either asked for or added"
    );
    assert!(done.arrays > 0, "the points travelled too");

    let arrived: Vec<String> = into.objects().into_iter().map(|one| one.uri).collect();
    for uri in &done.objects {
        assert!(
            arrived.contains(uri),
            "{uri} was reported copied and is not in the package"
        );
    }
}

#[test]
fn what_arrives_is_closed_and_a_second_reader_says_so() {
    let Some(from) = source() else { return };
    let Some(surface) = from
        .find(Some("TriangulatedSetRepresentation"), None)
        .into_iter()
        .next()
    else {
        return;
    };

    let dir = tempfile::tempdir().expect("a directory for this test");
    let mut into = Package::create("copy.epc").expect("an empty package");
    into.set_data_root(dir.path());
    into.copy_from(&from, std::slice::from_ref(&surface.uri), "copy")
        .expect("the copy is made");

    // The copy closed the set with `graph::closure`; this asks the piece of
    // code that **only judges** whether the result holds together. Two walks,
    // written apart on purpose, and the second one is the one that would catch
    // the first getting it wrong.
    for uri in into.objects().into_iter().map(|one| one.uri) {
        for link in into.references(&uri).expect("references answer") {
            // A reference into nothing is only acceptable if the *source* had
            // it too - a copy is not answerable for a package that was already
            // naming what it did not hold.
            if !link.present {
                let in_source = from.find(None, None).into_iter().any(|one| one.uuid == link.uuid);
                assert!(
                    !in_source,
                    "{uri} needs {} and the source had it: the copy left it behind",
                    link.uri
                );
            }
        }
    }
}

#[test]
fn the_numbers_that_arrive_are_the_numbers_that_left() {
    let Some(from) = source() else { return };
    let Some(surface) = from
        .find(Some("TriangulatedSetRepresentation"), None)
        .into_iter()
        .next()
    else {
        return;
    };
    let arrays = from.arrays(&surface.uri).expect("it names its arrays");
    let Some(first) = arrays.first() else {
        eprintln!("skipped: this surface names no array");
        return;
    };

    let dir = tempfile::tempdir().expect("a directory for this test");
    let mut into = Package::create("copy.epc").expect("an empty package");
    into.set_data_root(dir.path());
    into.copy_from(&from, std::slice::from_ref(&surface.uri), "copy")
        .expect("the copy is made");

    // Number for number, through two different files. A copy that wrote the
    // right count of wrong numbers - a transposed shape, a widened type -
    // would pass every other check here.
    let (there, before) = from.read_array(first).expect("the source reads");
    let (here, after) = into.read_array(first).expect("the copy reads");
    assert_eq!(here.dimensions, there.dimensions, "the shape travelled");
    assert_eq!(here.element_type, there.element_type, "and the type");
    // `assert_eq!` on the block compares the variant too, so this says the
    // type did not widen on the way - the triangles of this corpus are
    // `int8`, and an `int64` copy of them would still hold the right numbers.
    assert_eq!(after, before, "and every value, in the same type");
}

#[test]
fn an_unknown_array_handling_is_refused_with_the_two_that_exist() {
    let Some(from) = source() else { return };
    let mut into = Package::create("copy.epc").expect("an empty package");
    let why = into
        .copy_from(&from, &[], "move")
        .expect_err("`move` is not one of them");
    assert!(why.contains("copy") && why.contains("leave"), "{why}");
}

#[test]
fn copying_nothing_copies_nothing() {
    let Some(from) = source() else { return };
    let mut into = Package::create("copy.epc").expect("an empty package");
    let done = into
        .copy_from(&from, &[], "copy")
        .expect("an empty ask is not an error");
    assert!(done.objects.is_empty(), "nothing asked for, nothing copied");
    assert!(into.objects().is_empty(), "and the package stayed empty");
}
