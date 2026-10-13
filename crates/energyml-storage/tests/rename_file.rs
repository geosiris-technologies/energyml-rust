//! Renaming the external data file, and the three places that have to agree.
//!
//! The control this suite exists for is the one the design asked for by name:
//! rename, **save**, reopen, and read. A half-done rename passes every other
//! check — the package opens, the objects are all there — and fails only when
//! somebody tries to draw something.
//!
//! # Why reading is not enough on its own
//!
//! This crate finds an external file by **content** when the declared name does
//! not hold the dataset (`tests/renamed.rs`), so a package left naming the old
//! file still reads the renamed one sitting beside it. That permissiveness is
//! deliberate and it makes the obvious assertion worthless: *the arrays still
//! read* would pass on a rename that changed nothing but the file on disk.
//!
//! So the assertion is on what the package **says** — the names it declares,
//! the file each array pointer names — and, where the arrays are read, on the
//! store having gone **nowhere it had to report**. A departure is a warning
//! here, and the absence of one is what says the document is right rather than
//! merely lucky.

use std::path::PathBuf;
use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_storage::EpcStore;
use energyml_storage::rename::{self, FileMove};

fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

/// A copy of a corpus package and its data file, in a folder of its own.
///
/// **Copied, never opened where it lives.** The corpus is re-downloaded rather
/// than versioned, and a test that renames a file in the fixtures takes it away
/// from every other test in the workspace.
fn workbench(case: &str, package: &str, data: &str) -> Option<(PathBuf, PathBuf)> {
    let (Some(source_epc), Some(source_h5)) = (crate::fixtures::find(package), crate::fixtures::find(data))
    else {
        return None;
    };
    let dir = std::env::temp_dir().join(format!("energyml-rename-{case}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).ok()?;
    let epc = dir.join(package);
    let h5 = dir.join(data);
    std::fs::copy(&source_epc, &epc).ok()?;
    std::fs::copy(&source_h5, &h5).ok()?;
    Some((epc, h5))
}

/// Every array pointer of a package, proxies resolved — so both spellings of
/// "where the numbers live" answer the same question.
fn pointers(store: &EpcStore) -> Vec<energyml_storage::ArrayLocation> {
    store
        .resources()
        .iter()
        .filter_map(|summary| store.get_object(&summary.uri).ok())
        .flat_map(|object| store.arrays_of(object.as_reflect()))
        .filter(|location| !location.file.is_empty())
        .collect()
}

/// The file names they state, deduplicated.
fn files_named(store: &EpcStore) -> Vec<String> {
    let mut out: Vec<String> = pointers(store).into_iter().map(|one| one.file).collect();
    out.sort();
    out.dedup();
    out
}

#[test]
fn the_newer_form_names_its_file_in_a_field_of_every_array() {
    // eml 2.3: the name is in `ExternalDataArrayPart.Uri`, one per array, and
    // there is no proxy at all — so a rename that only touched the `.rels`
    // would touch nothing here.
    let Some((epc, _)) = workbench("plan22", "testingPackageCpp22.epc", "testingPackageCpp22.h5") else {
        eprintln!("skipped: the corpus is not present");
        return;
    };
    let store = EpcStore::open(&epc, registry()).expect("the package opens");

    let quote = rename::plan(&store, "testingPackageCpp22.h5", "renamed.h5");
    assert!(quote.is_allowed(), "refused: {:?}", quote.refusals);
    assert!(
        quote.fields > 100,
        "only {} field(s) would be rewritten in a package of {} arrays",
        quote.fields,
        files_named(&store).len()
    );
    assert!(!quote.objects.is_empty(), "no object was named as carrying one");
}

#[test]
fn the_older_form_names_its_file_in_a_relationship() {
    // eml 2.0: the object points at an `EpcExternalPartReference` and the name
    // lives in that part's `.rels` (EPC §3.2.3). Nothing to rewrite in a field,
    // everything to rewrite in an index.
    let Some((epc, _)) = workbench("plan20", "testingPackageCpp.epc", "testingPackageCpp.h5") else {
        eprintln!("skipped: the corpus is not present");
        return;
    };
    let store = EpcStore::open(&epc, registry()).expect("the package opens");

    let quote = rename::plan(&store, "testingPackageCpp.h5", "renamed.h5");
    assert!(quote.is_allowed(), "refused: {:?}", quote.refusals);
    assert!(
        quote.relationships > 0,
        "nothing in the relationships was found to rewrite"
    );
    // **And six fields as well**, which is the package and not the form: this
    // fesapi package carries an `eml23.ColumnBasedTable` and a
    // `resqml22.SeismicWellboreFrameRepresentation` inside an otherwise 2.0.1
    // archive — the mirror of the 2.0.1 objects `external_file_for` documents
    // inside `testingPackageCpp22.epc`. An EPC may mix releases (§3.2), so a
    // rename that handled one spelling and not the other would leave half a
    // package pointing at a file that is gone. The shape of the assertion comes
    // from that rule; the number comes from the file.
    assert_eq!(
        quote.fields, 6,
        "the newer-form parts of this mixed package were not counted"
    );
}

#[test]
fn a_rename_is_refused_before_it_can_half_happen() {
    let Some((epc, _)) = workbench("refusals", "testingPackageCpp22.epc", "testingPackageCpp22.h5") else {
        eprintln!("skipped: the corpus is not present");
        return;
    };
    let mut store = EpcStore::open(&epc, registry()).expect("the package opens");
    let old = "testingPackageCpp22.h5";

    for (to, why) in [
        ("", "empty"),
        ("   ", "blank"),
        ("data/renamed.h5", "a path rather than a bare name"),
        (old, "the name it already has"),
    ] {
        let quote = rename::plan(&store, old, to);
        assert!(!quote.is_allowed(), "`{to}` ({why}) was allowed");
        // And the refusal stops the writing, rather than being decoration on a
        // plan `apply` would carry out anyway.
        assert!(rename::apply(&mut store, &quote).is_err(), "`{to}` was applied");
    }

    // A file this package does not name is a refusal too: renaming nothing is
    // not a rename, and a caller whose button did nothing would not know why.
    let nothing = rename::plan(&store, "somebody_elses.h5", "renamed.h5");
    assert!(!nothing.is_allowed());
    assert!(
        nothing
            .refusals
            .iter()
            .any(|line| line.contains("somebody_elses.h5")),
        "the refusal does not name the file that was asked for: {:?}",
        nothing.refusals
    );
    assert!(!store.is_dirty(), "a refused rename changed the package");
}

#[test]
fn applying_leaves_nothing_naming_the_old_file() {
    let Some((epc, h5)) = workbench("apply22", "testingPackageCpp22.epc", "testingPackageCpp22.h5") else {
        eprintln!("skipped: the corpus is not present");
        return;
    };
    let mut store = EpcStore::open(&epc, registry()).expect("the package opens");

    let quote = rename::plan(&store, "testingPackageCpp22.h5", "renamed.h5");
    let done = rename::apply(&mut store, &quote).expect("the rename is carried out");
    assert_eq!(done.fields, quote.fields, "the quote and the bill disagree");
    assert_eq!(done.relationships, quote.relationships);
    assert_eq!(
        done.file,
        FileMove::Moved(h5.with_file_name("renamed.h5")),
        "the file itself did not move"
    );
    assert!(!h5.exists(), "the old file is still there");

    // Everything the document states, except what a rename cannot follow.
    let stray: Vec<(String, String)> = pointers(&store)
        .into_iter()
        .filter(|one| one.file != "renamed.h5")
        .map(|one| (one.proxy_uuid.unwrap_or_default(), one.field_path))
        .collect();
    assert!(
        stray.iter().all(|(proxy, _)| proxy == "testingPackageCpp22.h5"),
        "something still names the file by its old name: {stray:?}"
    );
    // **Two, and they are the package's own fault.** Two 2.0.1 objects survive
    // in this 2.2 package with the HDF5 file name written where a *uuid*
    // belongs (`EpcStore::external_file_for`, step 2), so the resolution reads
    // that "uuid" literally. A rename follows names written where names go;
    // correcting a producer's mistake is another operation, and guessing that a
    // uuid is really a file name is the kind of guess this workspace refuses.
    // They cost nothing: saved and reopened, the same pointers resolve to the
    // new file (`a_renamed_package_reads_without_having_to_look_around`).
    assert_eq!(
        stray.len(),
        2,
        "more than the two known malformed proxies: {stray:?}"
    );
}

#[test]
fn the_relationships_follow_on_the_older_form() {
    let Some((epc, h5)) = workbench("apply20", "testingPackageCpp.epc", "testingPackageCpp.h5") else {
        eprintln!("skipped: the corpus is not present");
        return;
    };
    let mut store = EpcStore::open(&epc, registry()).expect("the package opens");

    let quote = rename::plan(&store, "testingPackageCpp.h5", "renamed.h5");
    rename::apply(&mut store, &quote).expect("the rename is carried out");
    assert!(h5.with_file_name("renamed.h5").exists());

    let named: Vec<String> = store.external_file_map().into_values().collect();
    assert!(
        !named.is_empty() && named.iter().all(|file| file == "renamed.h5"),
        "the external part references still name {named:?}"
    );
    assert_eq!(files_named(&store), vec!["renamed.h5".to_string()]);
}

/// The control the design asked for: rename, save, reopen, read.
///
/// And it is read **with the departures counted**. This crate finds a file by
/// content when the name is wrong, so *the arrays read* alone would pass on a
/// package that was never corrected; a diagnostic saying it read from somewhere
/// else is what tells the two apart.
#[test]
#[cfg(feature = "hdf5")]
fn a_renamed_package_reads_without_having_to_look_around() {
    let Some((epc, _)) = workbench("saved", "testingPackageCpp22.epc", "testingPackageCpp22.h5") else {
        eprintln!("skipped: the corpus is not present");
        return;
    };
    let mut store = EpcStore::open(&epc, registry()).expect("the package opens");
    let before = store
        .resources()
        .iter()
        .filter_map(|summary| store.get_object(&summary.uri).ok())
        .flat_map(|object| store.arrays_of(object.as_reflect()))
        .filter(|location| store.array_metadata(location).is_ok())
        .count();
    assert!(before > 100, "only {before} arrays read before the rename");

    let quote = rename::plan(&store, "testingPackageCpp22.h5", "renamed.h5");
    rename::apply(&mut store, &quote).expect("the rename is carried out");
    // On Windows an open handle makes a rename fail outright, which is why the
    // cache has a door; here the file has already moved, so this is about the
    // *next* read going to the new name.
    energyml_storage::hdf5::close_cached_files();
    let saved = epc.with_file_name("after_rename.epc");
    store
        .save_as(&saved, &energyml_storage::SaveOptions::default())
        .expect("the package saves");

    let reopened = EpcStore::open(&saved, registry()).expect("the saved package opens");
    let after = reopened
        .resources()
        .iter()
        .filter_map(|summary| reopened.get_object(&summary.uri).ok())
        .flat_map(|object| reopened.arrays_of(object.as_reflect()))
        .filter(|location| reopened.array_metadata(location).is_ok())
        .count();
    assert_eq!(after, before, "arrays went missing across the rename");

    // What the document **says**, which is what a rename changes. Reading alone
    // proves nothing here: the file is found by content when the name is wrong.
    assert_eq!(files_named(&reopened), vec!["renamed.h5".to_string()]);

    let wandered: Vec<String> = reopened
        .diagnostics()
        .iter()
        .map(|one| one.message.clone())
        .filter(|line| line.contains("instead"))
        .collect();
    assert!(
        wandered.is_empty(),
        "the package had to read from a file other than the one it names, so the document is          still wrong: {:?}",
        &wandered[..wandered.len().min(3)]
    );

    // Two diagnostics **do** remain, and they were there before the rename: two
    // 2.0.1 objects of this package carry the HDF5 file name where a uuid
    // belongs (see `EpcStore::external_file_for`). A rename cannot follow a
    // name written in the wrong field, and correcting a producer's mistake is
    // not what it was asked to do — so the pointer resolves by fallback, as it
    // always did, and says so.
    let fesapi = reopened
        .diagnostics()
        .iter()
        .filter(|one| one.message.contains("is not a part of this package"))
        .count();
    assert_eq!(
        fesapi, 2,
        "this package's two malformed proxies changed in number"
    );
}
