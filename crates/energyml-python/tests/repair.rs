//! Repairing the declared data file names, through the bridge.
//!
//! The survey, the rewrite and the verdict are `energyml_storage::repair`'s
//! (pinned in `energyml-storage`). What this file checks is that the bridge
//! asks, converts and adds nothing: the answers are the library's, and the
//! verdict (`lost`) is read back from the written package.
//!
//! The scenario is fabricated from a fixture package: its data file is copied
//! under another name and the original removed, so the package declares a name
//! the search finds only under the other one.

#![cfg(feature = "hdf5")]

use energyml_rust::session::Package;

#[test]
fn a_renamed_data_file_is_surveyed_and_repaired_by_the_librarys_answers() {
    let (Some(source_epc), Some(source_h5)) = (
        crate::fixtures::find("testingPackageCpp22.epc"),
        crate::fixtures::find("testingPackageCpp22.h5"),
    ) else {
        eprintln!("skipped: the fixture package is absent");
        return;
    };
    let dir = std::env::temp_dir().join("energyml-python-repair");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch folder");
    let epc = dir.join("testingPackageCpp22.epc");
    std::fs::copy(&source_epc, &epc).expect("copy the package");
    std::fs::copy(&source_h5, dir.join("moved.h5")).expect("copy the data file under another name");

    let mut package = Package::open(&epc).expect("the package opens");
    let files = package.array_files();
    assert!(!files.is_empty(), "the package declares external arrays");
    assert!(
        files
            .iter()
            .any(|file| file.found.as_ref().is_some_and(|found| !found.agrees)),
        "a name the search found under another is not reported as agreeing: {files:?}"
    );
    assert!(!package.is_dirty(), "surveying modified the package");

    let out = dir.join("fixed.epc");
    let done = package
        .repair_arrays(&out)
        .expect("the repair runs")
        .expect("something was rewritten");
    assert!(done.declarations > 0);
    assert!(
        done.renamed.iter().any(|(_, now)| now == "moved.h5"),
        "{:?}",
        done.renamed
    );
    assert_eq!(done.lost, 0, "the written package lost array pointers");

    // The written package now says the right thing: a second repair has
    // nothing to do.
    let mut again = Package::open(&out).expect("the repaired package opens");
    assert!(
        again
            .repair_arrays(dir.join("twice.epc"))
            .expect("it runs")
            .is_none(),
        "a repaired package was rewritten again"
    );
    // Break it: make `Package::repair_arrays` return `Ok(None)` before calling
    // `energyml_storage::repair::fix`; the first repair then finds nothing.
}
