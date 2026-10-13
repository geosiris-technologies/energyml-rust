//! The bridge, exercised **without Python**.
//!
//! This is what the split is for: a pyo3 binding only runs after `maturin
//! develop`, so nothing living in `bindings.rs` is reachable by `cargo test`.
//! Everything that can be wrong is therefore in `session`, and this file is
//! what proves it - were it to reach nothing, the split would be decorative.
//!
//! # On real packages
//!
//! The reference packages are fetched (`scripts/fetch-fixtures.py`), not versioned (`fixtures.rs`).
//! When they are absent outside CI every check **says so and skips** - a red test on a fresh
//! clone stops meaning anything. It is the shape
//! `energyml-build/tests/package.rs` already has.

use std::path::PathBuf;

// The library is called `energyml_rust`, after the distribution; it is
// `[lib] name`, and **not** what Python imports - maturin names the extension
// from `module-name` in `pyproject.toml`, and a caller writes
// `from energyml.rust.epc import Epc`.
//
// It was `energyml` until 20/09, which is what `energyml-cli` calls its binary:
// two targets of one name write one `target/debug/deps/energyml.pdb`, and
// `cargo test --workspace` died on `LNK1201` under Windows as soon as both were
// rebuilt in the same invocation.
use energyml_rust::session::{ArrayRef, Block, Package};

/// A package from the corpus, or `None` - and it says which one is missing.
fn corpus(name: &str) -> Option<PathBuf> {
    crate::fixtures::find_or_skip(name)
}

#[test]
fn a_package_opens_and_returns_its_catalogue() {
    let Some(path) = corpus("testingPackageCpp.epc") else {
        return;
    };
    let package = Package::open(&path).expect("the package opens");
    let objects = package.objects();
    assert!(!objects.is_empty(), "a package of the corpus holds objects");

    // The four fields of a summary are **filled**, not merely present. A
    // catalogue whose type was empty would read as a package of anonymous
    // objects, which is the opposite of the news it carries.
    for one in &objects {
        assert!(one.uri.starts_with("eml:///"), "an ETP uri: {}", one.uri);
        assert!(!one.uuid.is_empty(), "{} has no uuid", one.uri);
        assert!(
            one.qualified_type.contains('.'),
            "a qualified type: {}",
            one.qualified_type
        );
    }

    // And the uuid finds its uri back: that is what lets a caller work with the
    // identifiers it reads in its other tools.
    let first = &objects[0];
    assert_eq!(
        package.uri_of(&first.uuid).expect("one entry").as_deref(),
        Some(first.uri.as_str())
    );
    assert_eq!(package.uri_of("00000000-0000-4000-8000-000000000000"), Ok(None));
}

#[test]
fn an_object_comes_back_as_json_and_carries_its_type() {
    let Some(path) = corpus("testingPackageCpp.epc") else {
        return;
    };
    let package = Package::open(&path).expect("the package opens");
    let first = package.objects().into_iter().next().expect("at least one object");
    let text = package.get(&first.uri).expect("the object reads");
    let value: serde_json::Value = serde_json::from_str(&text).expect("some JSON");

    // **The `$type` is what makes this JSON re-readable.** Without it, `put`
    // would not know which class to build, and a round trip would lose the
    // object in silence - it would leave as an anonymous dictionary.
    assert_eq!(value["$type"].as_str(), Some(first.qualified_type.as_str()));
    assert_eq!(value["uuid"].as_str(), Some(first.uuid.as_str()));
}

#[test]
fn an_object_read_and_put_back_keeps_its_uri() {
    let Some(path) = corpus("testingPackageCpp.epc") else {
        return;
    };
    let mut package = Package::open(&path).expect("the package opens");
    let first = package.objects().into_iter().next().expect("at least one object");
    let text = package.get(&first.uri).expect("the object reads");

    // The full round trip through the Python door: JSON text in, uri out. What
    // would have to break is the reading of the type or of the uuid - and the
    // uri would change with it, so the object would go and sit somewhere else
    // without saying anything.
    let written = package.put(&text).expect("the object is put back");
    assert_eq!(written, first.uri);
}

#[test]
fn the_arrays_of_an_object_are_named_before_they_are_read() {
    let Some(path) = corpus("testingPackageCpp.epc") else {
        return;
    };
    let package = Package::open(&path).expect("the package opens");

    // **Naming is not reading**, and that is what lets a multi-gigabyte package
    // be opened without decompressing a byte. So: find an object that carries
    // arrays, and look at what it says about them.
    let found = package
        .objects()
        .into_iter()
        .find_map(|one| match package.arrays(&one.uri) {
            Ok(arrays) if !arrays.is_empty() => Some((one, arrays)),
            _ => None,
        });
    let Some((object, arrays)) = found else {
        eprintln!("skipped: no object of this package names an array");
        return;
    };

    for array in &arrays {
        assert!(
            !array.file.is_empty(),
            "{} names an array with no file",
            object.uri
        );
        assert!(
            array.path_in_file.starts_with('/'),
            "a path inside the file: {}",
            array.path_in_file
        );
    }

    // **Naming works without a backend, reading does not** - so reading has two
    // legitimate outcomes, and the point is that neither of them is an empty
    // array: an absence and a blunder are not the same news, and a build with
    // no HDF5 that answered with no numbers would read as a package that holds
    // none.
    //
    // **Which outcome runs is not this crate's feature to decide**, and saying
    // it with `#[cfg(feature = "hdf5")]` was wrong in the only way that
    // matters - it describes a build that is not the one running. Features
    // unify across a `cargo test --workspace`, and another crate asks for
    // `energyml-storage/hdf5` unconditionally, so the store this test links
    // against **has** a backend while this crate's own flag is off. The guard
    // therefore compiled the *refusal* branch into a build that reads perfectly
    // well, and the test failed for being right about the wrong thing.
    match package.read_array(&arrays[0]) {
        Ok((read, block)) => {
            // The shape is asked for without reading, and it must agree with
            // what is read. Two readers agreeing on a total and disagreeing on
            // what it means is the shape of the bugs this repository hunts:
            // here it is the same total.
            let shape = package.array_shape(&arrays[0]).expect("the shape is asked for");
            assert!(!shape.dimensions.is_empty(), "a shape has at least one dimension");
            let promised: i64 = shape.dimensions.iter().product();
            assert_eq!(
                read.dimensions, shape.dimensions,
                "the announced shape is the one read"
            );
            assert_eq!(
                block.len() as i64,
                promised,
                "as many numbers as the shape promises"
            );
            assert_eq!(read.element_type, shape.element_type);
        }
        Err(why) => assert!(!why.is_empty(), "a refusal carries a sentence"),
    }
}

// Reading and rewriting an `.h5` needs the C library: this check lives in the
// `hdf5` CI pass, like the one in `energyml-build`.
#[cfg(feature = "hdf5")]
#[test]
fn an_array_that_is_written_reads_back_number_for_number() {
    let dir = tempfile::tempdir().expect("a directory for this test");
    // The package **and its data file**: writing into the original `.h5` would
    // damage the corpus, which git ignores and nobody would restore.
    for name in ["testingPackageCpp.epc", "testingPackageCpp.h5"] {
        let Some(from) = corpus(name) else { return };
        std::fs::copy(&from, dir.path().join(name)).expect("the copy");
    }
    let package = Package::open(dir.path().join("testingPackageCpp.epc")).expect("the package");

    let array = ArrayRef {
        file: "testingPackageCpp.h5".to_string(),
        path_in_file: "/python-bridge/trial".to_string(),
    };
    let values = vec![1.5_f64, -2.5, 3.0, 4.25, 5.0, 6.75];
    package
        .write_array(&array, &[2, 3], Block::F64(values.clone()))
        .expect("the array writes");

    let (shape, block) = package.read_array(&array).expect("and reads back");
    assert_eq!(shape.dimensions, vec![2, 3], "the shape crosses the file");
    assert_eq!(block, Block::F64(values), "number for number, with no conversion");
}

// A tool is told where its files are, not what the package calls them. The
// name in the objects is `logical.h5`; the bytes must land in `actual.dat`, and
// nothing may be created under the logical name.
#[cfg(feature = "hdf5")]
#[test]
fn a_pinned_name_writes_and_reads_the_file_it_was_pinned_to() {
    let dir = tempfile::tempdir().expect("a directory for this test");
    let mut package = Package::create("pinned.epc").expect("an empty package");
    package.set_data_root(dir.path());
    let actual = dir.path().join("actual.dat");
    package.pin_data_file("logical.h5", &actual);

    let array = ArrayRef {
        file: "logical.h5".to_string(),
        path_in_file: "/pinned/trial".to_string(),
    };
    let values = vec![0.5_f64, 1.5, 2.5, 3.5];
    package
        .write_array(&array, &[2, 2], Block::F64(values.clone()))
        .expect("the array writes");

    assert!(
        actual.exists(),
        "the bytes are in the file the name was pinned to"
    );
    assert!(
        !dir.path().join("logical.h5").exists(),
        "and no file is created under the logical name"
    );
    let (shape, block) = package
        .read_array(&array)
        .expect("and reads back through the pin");
    assert_eq!(shape.dimensions, vec![2, 2]);
    assert_eq!(block, Block::F64(values));

    // Pinned again, the last path wins: a run reads its input, then writes into its copy.
    let copy = dir.path().join("copy.dat");
    std::fs::copy(&actual, &copy).expect("the copy");
    package.pin_data_file("logical.h5", &copy);
    package
        .write_array(&array, &[2, 2], Block::F64(vec![9.0, 9.0, 9.0, 9.0]))
        .expect("overwrites in the copy");
    let (_, in_copy) = package.read_array(&array).expect("reads the copy");
    assert_eq!(
        in_copy,
        Block::F64(vec![9.0; 4]),
        "the second pin is the one in force"
    );
    package.pin_data_file("logical.h5", &actual);
    let (_, in_first) = package.read_array(&array).expect("reads the first again");
    assert_eq!(
        in_first,
        Block::F64(vec![0.5, 1.5, 2.5, 3.5]),
        "the first file was left alone by the second pin's write"
    );
}

// `find` filters on the type before reading any header, so it must agree with
// the filter a caller would write over `objects()` - the cheaper path is not
// allowed to answer something else. Built from nothing: two types, two titles,
// one object version.
#[test]
fn find_answers_what_the_filter_over_objects_answers() {
    use serde_json::json;

    let mut package = Package::create("find.epc").expect("an empty package");
    let put = |package: &mut Package, kind: &str, uuid: &str, title: &str, version: Option<&str>| {
        let mut object = json!({
            "$type": kind,
            "uuid": uuid,
            "schemaVersion": "2.3",
            "Existence": "actual",
            "Citation": {
                "$type": "eml23.Citation",
                "Title": title,
                "Originator": "test",
                "Format": "test",
                "Creation": "2026-10-08T00:00:00Z",
            },
        });
        if let Some(version) = version {
            object["objectVersion"] = json!(version);
        }
        package.put(&object.to_string()).expect("the object is put");
    };
    put(
        &mut package,
        "eml23.DataobjectCollection",
        "6a3ba3a0-6d4e-4a3b-9f10-1111aaaa0001",
        "first",
        None,
    );
    put(
        &mut package,
        "eml23.DataobjectCollection",
        "6a3ba3a0-6d4e-4a3b-9f10-1111aaaa0002",
        "second",
        Some("v2"),
    );
    put(
        &mut package,
        "eml23.ActivityTemplate",
        "6a3ba3a0-6d4e-4a3b-9f10-1111aaaa0003",
        "first template",
        None,
    );

    let all = package.objects();
    let by_type: Vec<_> = all
        .iter()
        .filter(|one| one.qualified_type == "eml23.DataobjectCollection")
        .collect();
    let found = package.find(Some("eml23.DataobjectCollection"), None);
    assert_eq!(found.len(), 2, "both collections, and not the template");
    for one in &found {
        let same = by_type
            .iter()
            .find(|row| row.uri == one.uri)
            .expect("a row of objects()");
        assert_eq!(one.uuid, same.uuid);
    }
    let second = found
        .iter()
        .find(|one| one.uuid.ends_with("0002"))
        .expect("the second collection");
    assert_eq!(
        second.title, "second",
        "the title comes from the citation, for an object put in this session too"
    );
    assert_eq!(
        second.object_version.as_deref(),
        Some("v2"),
        "and so does the version"
    );

    let titled = package.find(None, Some("first"));
    assert_eq!(
        titled.len(),
        2,
        "the title filter runs on what the type kept - here everything: {titled:?}"
    );
    let both = package.find(Some("ActivityTemplate"), Some("first"));
    assert_eq!(both.len(), 1, "type and title together");
    assert!(package.find(Some("Grid2dRepresentation"), None).is_empty());
}

#[test]
fn a_shape_that_lies_is_refused_naming_both_counts() {
    let Some(path) = corpus("testingPackageCpp.epc") else {
        return;
    };
    let dir = tempfile::tempdir().expect("a directory for this test");
    std::fs::copy(&path, dir.path().join("p.epc")).expect("the copy");
    let package = Package::open(dir.path().join("p.epc")).expect("the package");

    // **The check that prevents a silent re-read.** A shape promising more
    // numbers than there are writes without error in some formats and reads
    // back with zeros at the end - plausible values that nothing flags. The
    // refusal names both counts, because it is their difference that says what
    // to fix.
    let array = ArrayRef {
        file: "p.h5".to_string(),
        path_in_file: "/trial".to_string(),
    };
    let why = package
        .write_array(&array, &[4, 4], Block::F64(vec![1.0, 2.0, 3.0]))
        .expect_err("sixteen promised, three carried");
    assert!(why.contains("16"), "the promised count: {why}");
    assert!(why.contains('3'), "and the one carried: {why}");
}

#[test]
fn a_package_written_elsewhere_reads_back_with_the_same_objects() {
    let Some(path) = corpus("testingPackageCpp.epc") else {
        return;
    };
    let dir = tempfile::tempdir().expect("a directory for this test");
    let package = Package::open(&path).expect("the package opens");
    let before: Vec<String> = package.objects().into_iter().map(|one| one.uri).collect();

    let out = dir.path().join("copy.epc");
    package.save_as(&out).expect("the package writes");

    // **The full round trip**, which is the only real test of this bridge: what
    // goes out must come back, and with the same objects. A rewritten package
    // that lost one would open perfectly and say nothing.
    let again = Package::open(&out).expect("and reads back");
    let after: Vec<String> = again.objects().into_iter().map(|one| one.uri).collect();
    assert_eq!(after, before, "the same objects, in the same order");
}

#[test]
fn a_path_that_does_not_exist_refuses_by_naming_it() {
    // A refusal that only said "no" would send the caller searching at random.
    // And there is nothing to skip here: this check asks for no corpus, which
    // makes it the only one that runs on a fresh clone.
    let why = match Package::open("there-is-no-file-here.epc") {
        Ok(_) => panic!("a file that does not exist opened"),
        Err(why) => why,
    };
    assert!(
        why.contains("there-is-no-file-here.epc"),
        "the refusal names the path: {why}"
    );
}
