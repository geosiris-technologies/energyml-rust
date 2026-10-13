//! Every value array of the reference packages, read through one function.
//!
//! The point of the module under test is that a caller should not have to know
//! whether the numbers are in a file, computed from a start and a step, written
//! inline, or derived from another array. This walks the corpus and reads every
//! one of them, so "covers every form" is a count rather than a claim.

use std::path::PathBuf;
use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_core::value::{FieldValue, Reflect};
use energyml_domain::values;
use energyml_storage::EpcStore;

/// Every schema version the workspace generates.
///
/// One line rather than a hand-kept subset: a list written out here goes stale
/// the day a release is added, and nothing would notice — the suite would keep
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

/// Walk an object and hand every value array to `visit`.
fn walk(object: &dyn Reflect, at: &str, depth: usize, visit: &mut dyn FnMut(&dyn Reflect, &str)) {
    if depth > 32 {
        return;
    }
    if values::family(object).is_some() {
        visit(object, at);
        return;
    }
    for field in object.type_info().fields {
        match object.field(field.name) {
            Some(FieldValue::Object(child)) => {
                walk(child, &format!("{at}.{}", field.xml_name), depth + 1, visit)
            }
            Some(FieldValue::List(items)) => {
                for (i, item) in items.iter().enumerate() {
                    if let Some(child) = item.as_object() {
                        walk(child, &format!("{at}.{}[{i}]", field.xml_name), depth + 1, visit);
                    }
                }
            }
            _ => {}
        }
    }
}

#[test]
#[cfg(feature = "hdf5")]
fn every_value_array_of_the_corpus_reads() {
    let files = corpus();
    if files.is_empty() {
        eprintln!("skipped: no EPC in testdata/");
        return;
    }
    let mut forms: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut families: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut refused: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut absent = 0usize;
    let mut total = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            let mut found: Vec<(String, String)> = Vec::new();
            walk(object.as_reflect(), "", 0, &mut |array, at| {
                found.push((array.type_info().name.to_string(), at.to_string()));
            });
            for (name, at) in found {
                // Re-find it: the closure above cannot hold the borrow.
                let mut done = false;
                walk(object.as_reflect(), "", 0, &mut |array, here| {
                    if done || here != at {
                        return;
                    }
                    done = true;
                    match values::read(array, &store, here) {
                        Ok(read) => {
                            *forms.entry(name.clone()).or_default() += 1;
                            *families.entry(format!("{:?}", read.family())).or_default() += 1;
                            total += read.len();
                            // The declared count and the values must agree, or
                            // something downstream indexes past the end.
                            if let Ok(counted) = values::count(array, &store, here) {
                                assert_eq!(
                                    counted,
                                    read.len(),
                                    "{}: {name} at {here} counts {counted} but yields {}",
                                    path.display(),
                                    read.len()
                                );
                            }
                        }
                        Err(e) if e.is_missing_file() => absent += 1,
                        Err(e) => {
                            *refused.entry(format!("{name}: {e}")).or_default() += 1;
                        }
                    }
                });
            }
        }
    }

    eprintln!(
        "{} value array(s) read, {total} value(s) total",
        forms.values().sum::<usize>()
    );
    eprintln!("by form:   {forms:?}");
    eprintln!("by family: {families:?}");
    if absent > 0 {
        eprintln!("{absent} skipped: their package's `.h5` is not in testdata/");
    }
    if !refused.is_empty() {
        eprintln!("not read: {refused:?}");
    }

    assert!(!forms.is_empty(), "the corpus holds no value array at all");
    // The whole claim of this module is that it covers the computed forms too,
    // not only the file-backed ones. Booleans especially: the corpus has 45 of
    // them and the previous reader handled none.
    assert!(
        families.contains_key("Boolean"),
        "no boolean array was read; the corpus has 45: {families:?}"
    );
    assert!(
        families.contains_key("Integer") && families.contains_key("Floating"),
        "{families:?}"
    );
    assert!(
        forms.keys().any(|k| k.ends_with("ConstantArray")),
        "no constant array read: {forms:?}"
    );
    assert!(
        forms.keys().any(|k| k.ends_with("LatticeArray")),
        "no lattice read: {forms:?}"
    );
    // Anything that failed for a reason other than a missing file is a gap
    // worth seeing rather than a tolerated silence.
    assert!(refused.is_empty(), "some arrays could not be read: {refused:?}");
}

/// The computed forms need no array backend at all.
///
/// The mirror of the geometry crate's lattice test, and the same argument: a
/// constant array and a lattice carry their values in the XML, so a build with
/// no HDF5 — a WebAssembly one, say — must still read them. If this ever needs
/// the feature, something has started going to the file when it should not.
#[test]
fn the_computed_forms_need_no_backend() {
    let files = corpus();
    if files.is_empty() {
        eprintln!("skipped: no EPC in testdata/");
        return;
    }
    let mut read = 0usize;
    let mut families: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            let mut found: Vec<String> = Vec::new();
            walk(object.as_reflect(), "", 0, &mut |array, at| {
                if !values::is_external(array) {
                    found.push(at.to_string());
                }
            });
            for at in found {
                let mut done = false;
                walk(object.as_reflect(), "", 0, &mut |array, here| {
                    if done || here != at {
                        return;
                    }
                    done = true;
                    let values = values::read(array, &store, here)
                        .unwrap_or_else(|e| panic!("{}: {here} — {e}", path.display()));
                    families.insert(format!("{:?}", values.family()));
                    read += 1;
                });
            }
        }
    }

    eprintln!("{read} computed array(s) read with no backend, families {families:?}");
    assert!(read > 0, "the corpus has constants and lattices; none was read");
    assert!(
        families.contains("Boolean"),
        "no boolean constant array read: {families:?}"
    );
}

/// The optimised path gives the file's own type, and only where a file exists.
#[test]
#[cfg(feature = "hdf5")]
fn the_native_path_avoids_the_widening() {
    let files = corpus();
    if files.is_empty() {
        return;
    }
    let mut native = 0usize;
    let mut computed = 0usize;
    let mut narrower = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            let mut seen: Vec<String> = Vec::new();
            walk(object.as_reflect(), "", 0, &mut |array, at| {
                seen.push(at.to_string());
                let _ = array;
            });
            for at in seen {
                let mut done = false;
                walk(object.as_reflect(), "", 0, &mut |array, here| {
                    if done || here != at {
                        return;
                    }
                    done = true;
                    match values::read_native(array, &store, here) {
                        Ok(Some(data)) => {
                            native += 1;
                            // The point of this path: an int32 dataset stays
                            // int32 instead of becoming a Vec<i64> twice the
                            // size. Count the ones where it actually saves.
                            if data.values.element_type() != "double" && data.values.element_type() != "int64"
                            {
                                narrower += 1;
                            }
                            // And it must agree with the widened read.
                            if let Ok(widened) = values::read(array, &store, here) {
                                assert_eq!(
                                    data.values.len(),
                                    widened.len(),
                                    "{}: native and widened disagree at {here}",
                                    path.display()
                                );
                            }
                        }
                        // A computed form has no file, so there is nothing to
                        // read natively — and saying `None` rather than
                        // erroring is what lets a caller try this first.
                        Ok(None) => {
                            computed += 1;
                            assert!(!values::is_external(array));
                        }
                        Err(_) => {}
                    }
                });
            }
        }
    }

    eprintln!("{native} array(s) read natively ({narrower} in a type narrower than f64/i64)");
    eprintln!("{computed} computed array(s), where there is nothing to read natively");
    assert!(native > 0 && computed > 0, "both paths must be exercised");
    assert!(
        narrower > 0,
        "no array came back narrower than the widened read: the native path saves nothing"
    );
}

/// A jagged array unpacks into rows.
#[test]
#[cfg(feature = "hdf5")]
fn jagged_arrays_unpack_into_rows() {
    let files = corpus();
    if files.is_empty() {
        return;
    }
    let mut rows = 0usize;
    let mut arrays = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            let mut found: Vec<String> = Vec::new();
            collect_jagged(object.as_reflect(), "", 0, &mut found);
            for at in found {
                let mut done = false;
                visit_at(object.as_reflect(), "", 0, &at, &mut |array| {
                    if done {
                        return;
                    }
                    done = true;
                    match values::jagged(array, &store, &at) {
                        Ok(unpacked) => {
                            arrays += 1;
                            rows += unpacked.len();
                        }
                        Err(e) if e.is_missing_file() => {}
                        Err(e) => panic!("{}: {at} — {e}", path.display()),
                    }
                });
            }
        }
    }
    eprintln!("{arrays} jagged array(s), {rows} row(s)");
    assert!(arrays > 0, "the corpus has 17 jagged arrays; none was found");
}

#[cfg(feature = "hdf5")]
fn collect_jagged(object: &dyn Reflect, at: &str, depth: usize, out: &mut Vec<String>) {
    if depth > 32 {
        return;
    }
    if object.type_info().name.ends_with("JaggedArray") {
        out.push(at.to_string());
        return;
    }
    for field in object.type_info().fields {
        match object.field(field.name) {
            Some(FieldValue::Object(child)) => {
                collect_jagged(child, &format!("{at}.{}", field.xml_name), depth + 1, out)
            }
            Some(FieldValue::List(items)) => {
                for (i, item) in items.iter().enumerate() {
                    if let Some(child) = item.as_object() {
                        collect_jagged(child, &format!("{at}.{}[{i}]", field.xml_name), depth + 1, out);
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(feature = "hdf5")]
fn visit_at(object: &dyn Reflect, at: &str, depth: usize, wanted: &str, visit: &mut dyn FnMut(&dyn Reflect)) {
    if depth > 32 {
        return;
    }
    if at == wanted {
        visit(object);
        return;
    }
    for field in object.type_info().fields {
        match object.field(field.name) {
            Some(FieldValue::Object(child)) => visit_at(
                child,
                &format!("{at}.{}", field.xml_name),
                depth + 1,
                wanted,
                visit,
            ),
            Some(FieldValue::List(items)) => {
                for (i, item) in items.iter().enumerate() {
                    if let Some(child) = item.as_object() {
                        visit_at(
                            child,
                            &format!("{at}.{}[{i}]", field.xml_name),
                            depth + 1,
                            wanted,
                            visit,
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

#[test]
fn every_value_array_in_an_object_is_found_with_its_path() {
    // # Why the walk is not the file walk
    //
    // `EpcStore::arrays_of` answers *where do the bytes live* and finds only the
    // file-backed forms. An array is a thing in its own right whether or not it
    // touches a file: a property whose values are a `DoubleConstantArray` has
    // values, has a count, and has nothing external at all. A caller built on
    // the file walk alone reports it as having no data.
    //
    // What is asserted: every path leads back to the array it names, no array
    // is listed inside another, and the paths are ones `path::get` accepts.
    let files = corpus();
    if files.is_empty() {
        eprintln!("skipped: no EPC in testdata/");
        return;
    }
    let registry = registry();

    let mut seen = 0usize;
    let mut computed = 0usize;
    for path in files.iter().take(3) {
        let store = EpcStore::open(path, registry.clone()).expect("the package opens");
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else { continue };
            let found = values::locate(object.as_reflect());

            for array in &found {
                // The path is an address, not a description: following it lands
                // on the very object that was reported.
                let held = energyml_core::path::get(object.as_reflect(), &array.path)
                    .unwrap_or_else(|e| panic!("`{}` does not resolve on `{}`: {e}", array.path, summary.uri));
                let energyml_core::value::FieldValue::Object(held) = held else {
                    panic!("`{}` leads to something that is not an object", array.path);
                };
                assert_eq!(held.type_info().name, array.form);
                assert_eq!(values::family(held), Some(array.family));
                assert_eq!(values::is_external(held), array.external);
                if !array.external {
                    computed += 1;
                }
                seen += 1;
            }

            // No array inside another: an array's `Count`, its `HdfProxy`, the
            // integer array a mask is built from — all its own business, and
            // listing them beside it would offer the parts of an answer as
            // though they were answers.
            for array in &found {
                for other in &found {
                    if array.path == other.path {
                        continue;
                    }
                    assert!(
                        !other.path.starts_with(&format!("{}.", array.path)),
                        "`{}` is listed inside `{}` on `{}`",
                        other.path,
                        array.path,
                        summary.uri
                    );
                }
            }
        }
    }

    assert!(seen > 50, "{seen} value array(s) found — the corpus holds far more");
    // The half the file walk cannot see, and the reason this exists.
    assert!(computed > 0, "no computed array found — the walk proves nothing new");
}
