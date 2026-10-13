//! A renamed file is not a lost file.
//!
//! Someone renames `model.epc` and `model.h5` to `project_final.*`. Someone
//! else is sent the `.h5` under whatever name their colleague's export tool
//! chose. Nothing was lost in either case — every number is still on disk,
//! usually in the very folder the package is in — and the only thing standing
//! between the person and their data is a string.
//!
//! So the resolution asks the question that actually matters: **which file
//! holds this dataset**, not which file has the right name. The name orders the
//! search; the content ends it. See `energyml_storage::backend` for the order
//! and `Origin` for what each place in it means.
//!
//! What this suite pins is that the permissiveness works, that it prefers the
//! right answers in the right order, and — the part that keeps it honest —
//! that it never goes anywhere without saying so.

#[cfg(feature = "hdf5")]
use std::path::PathBuf;
#[cfg(feature = "hdf5")]
use std::sync::Arc;

#[cfg(feature = "hdf5")]
use energyml_core::registry::Registry;
#[cfg(feature = "hdf5")]
use energyml_storage::EpcStore;
#[cfg(feature = "hdf5")]
use energyml_storage::Origin;

#[cfg(feature = "hdf5")]
fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

#[cfg(feature = "hdf5")]
fn corpus() -> Option<(PathBuf, PathBuf)> {
    let epc = crate::fixtures::find("testingPackageCpp22.epc")?;
    let h5 = crate::fixtures::find("testingPackageCpp22.h5")?;
    Some((epc, h5))
}

/// A copy of the reference package under new names, in a folder of its own.
///
/// `h5` is where the data file goes, which is what lets one test put it beside
/// the package and another put it somewhere the package could never find.
#[cfg(feature = "hdf5")]
fn renamed(case: &str, epc_name: &str, h5: Option<&str>) -> Option<(PathBuf, PathBuf)> {
    let (source_epc, source_h5) = corpus()?;
    let dir = std::env::temp_dir().join(format!("energyml-renamed-{case}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).ok()?;

    let epc = dir.join(epc_name);
    std::fs::copy(&source_epc, &epc).ok()?;
    let data = match h5 {
        Some(name) => {
            let path = dir.join(name);
            std::fs::copy(&source_h5, &path).ok()?;
            path
        }
        None => {
            let away = std::env::temp_dir().join(format!("energyml-renamed-{case}-away"));
            std::fs::create_dir_all(&away).ok()?;
            let path = away.join("whatever_they_called_it.h5");
            std::fs::copy(&source_h5, &path).ok()?;
            path
        }
    };
    Some((epc, data))
}

/// How many array pointers a store can actually read.
#[cfg(feature = "hdf5")]
fn readable(store: &EpcStore) -> usize {
    store
        .resources()
        .iter()
        .filter_map(|summary| store.get_object(&summary.uri).ok())
        .flat_map(|object| store.arrays_of(object.as_reflect()))
        .filter(|location| store.array_metadata(location).is_ok())
        .count()
}

#[test]
#[cfg(feature = "hdf5")]
fn a_package_and_its_data_file_both_renamed_still_read() {
    let Some((epc, _)) = renamed("both", "project_final.epc", Some("colleagues_export.h5")) else {
        eprintln!("skipped: the corpus is not present");
        return;
    };
    let store = EpcStore::open(&epc, registry()).expect("the package opens");

    let count = readable(&store);
    assert!(
        count > 200,
        "{count} array pointer(s) readable out of a package whose data file sits right beside it \
         under another name"
    );

    // And it said so. Permissive is only defensible when it is not quiet: the
    // name is also what stops a package reading somebody else's file, so
    // spending it has to leave a record.
    let said: Vec<String> = store.diagnostics().iter().map(|d| d.message.clone()).collect();
    assert!(
        said.iter().any(|line| line.contains("colleagues_export.h5")),
        "nothing in the diagnostics names the file that was actually read: {:?}",
        &said[..said.len().min(3)]
    );
}

#[test]
#[cfg(feature = "hdf5")]
fn a_data_file_nowhere_near_the_package_can_be_offered() {
    // The bypass, and the point of it being **unnamed**: the person holding
    // this file has no idea what the package calls it, and should not have to
    // open a `.rels` in a text editor to find out.
    let Some((epc, away)) = renamed("offered", "project_final.epc", None) else {
        return;
    };

    let store = EpcStore::open(&epc, registry()).expect("the package opens");
    assert_eq!(
        readable(&store),
        0,
        "the data file is in another folder entirely; nothing should be readable yet"
    );

    let mut store = EpcStore::open(&epc, registry()).expect("the package opens");
    store.external_files_mut().offer(&away);
    assert!(
        readable(&store) > 200,
        "offering the file by path did not make it readable"
    );
}

#[test]
#[cfg(feature = "hdf5")]
fn offers_are_tried_in_order_and_an_empty_one_steps_aside() {
    // Two offers, the first of which is a real HDF5 file that simply does not
    // hold this package's datasets. It must not swallow the read: a candidate
    // that cannot answer steps aside for the next one, which is the whole
    // difference between "where is the file" and "which file holds this".
    let Some((epc, away)) = renamed("order", "project_final.epc", None) else {
        return;
    };
    let decoy = away.with_file_name("decoy.h5");
    {
        let file = hdf5::File::create(&decoy).expect("a decoy file is writable");
        file.new_dataset::<f64>()
            .shape([2])
            .create("/nothing/to/see")
            .expect("the decoy holds one dataset of its own");
    }

    let mut store = EpcStore::open(&epc, registry()).expect("the package opens");
    store.external_files_mut().offer(&decoy).offer(&away);
    assert!(
        readable(&store) > 200,
        "the first offer holds none of the datasets and stopped the search"
    );
}

#[test]
#[cfg(feature = "hdf5")]
fn a_pinned_name_outranks_everything() {
    // Pinning is the one case where somebody has stated, about *this* name,
    // where the file is. Nothing the package says outranks that — and the
    // package here says something that would otherwise work.
    let Some((epc, beside)) = renamed("pinned", "project_final.epc", Some("beside.h5")) else {
        return;
    };
    let elsewhere = beside.with_file_name("pinned_copy.h5");
    std::fs::copy(&beside, &elsewhere).expect("a second copy");

    let mut store = EpcStore::open(&epc, registry()).expect("the package opens");
    store
        .external_files_mut()
        .set("testingPackageCpp22.h5", &elsewhere);

    let summary = store
        .resources()
        .into_iter()
        .find(|s| s.qualified_type.contains("IjkGrid"))
        .expect("the corpus has an IJK grid");
    let object = store.get_object(&summary.uri).expect("it reads");
    let location = store
        .arrays_of(object.as_reflect())
        .into_iter()
        .next()
        .expect("it points at an array");

    let resolved = store.resolve_array(&location).expect("the array resolves");
    assert_eq!(resolved.origin, Origin::Pinned);
    assert_eq!(resolved.path, elsewhere);
}

#[test]
#[cfg(feature = "hdf5")]
fn a_package_that_is_right_is_not_searched() {
    // The cost of being permissive has to be zero for everyone who does not
    // need it. A package whose names are correct resolves through its own
    // declaration and records nothing — no departure, no diagnostic, no
    // wandering through the folder.
    let Some((epc, _)) = renamed("intact", "testingPackageCpp22.epc", Some("testingPackageCpp22.h5"))
    else {
        return;
    };
    let store = EpcStore::open(&epc, registry()).expect("the package opens");
    assert!(readable(&store) > 200);

    let said = store.diagnostics();
    let wandered: Vec<&str> = said
        .iter()
        .map(|d| d.message.as_str())
        .filter(|line| line.contains("read from") || line.contains("another file"))
        .collect();
    assert!(
        wandered.is_empty(),
        "a package that names its file correctly should never have been searched: {wandered:?}"
    );
}

#[test]
fn a_neighbouring_package_is_never_a_candidate() {
    // A folder holding two `.epc` files is the ordinary case, and one is never
    // a place to look for the other's numbers. Left in, it costs an open and
    // produces a message about a "file signature not found" that means nothing
    // to the person reading it.
    let mut files = energyml_storage::ExternalFiles::with_root(std::env::temp_dir());
    let dir = std::env::temp_dir().join("energyml-neighbours");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a folder");
    std::fs::write(dir.join("other.epc"), b"PK").expect("a second package");
    std::fs::write(dir.join("data.h5"), b"\x89HDF").expect("a data file");
    files.set_root(&dir);

    let candidates = files.candidates("missing.h5", Some(&dir.join("first.epc")));
    let paths: Vec<String> = candidates
        .iter()
        .map(|c| c.path.display().to_string())
        .collect();
    assert!(
        paths.iter().any(|p| p.ends_with("data.h5")),
        "the data file should be a candidate: {paths:?}"
    );
    assert!(
        !paths.iter().any(|p| p.ends_with(".epc")),
        "no package should be: {paths:?}"
    );
}
