//! Renaming the data file, through the bridge.
//!
//! The operation itself is `energyml-storage`'s and is pinned there
//! (`tests/rename_file.rs`), against the corpus and by sabotage. What this file
//! checks is the only thing this crate adds: that the **quote crosses whole**
//! and that a refusal stays a refusal. A bridge that dropped the refusals would
//! turn a plan a caller must look at into a plan it cannot judge.

use std::path::PathBuf;

use energyml_rust::session::Package;

/// A copy of a corpus package and its data file, in a folder of its own -
/// renaming inside the fixtures would take the file away from every other test.
fn workbench(case: &str, package: &str, data: &str) -> Option<(PathBuf, String)> {
    let (Some(source_epc), Some(source_h5)) = (crate::fixtures::find(package), crate::fixtures::find(data))
    else {
        eprintln!("skipped: {package} is absent from the fixtures");
        return None;
    };
    let dir = std::env::temp_dir().join(format!("energyml-python-rename-{case}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).ok()?;
    let epc = dir.join(package);
    std::fs::copy(&source_epc, &epc).ok()?;
    std::fs::copy(&source_h5, dir.join(data)).ok()?;
    Some((epc, data.to_string()))
}

#[test]
fn the_quote_crosses_whole() {
    let Some((epc, h5)) = workbench("plan", "testingPackageCpp22.epc", "testingPackageCpp22.h5") else {
        return;
    };
    let package = Package::open(&epc).expect("the package opens");

    let quote = package.plan_rename(&h5, "renamed.h5");
    assert!(quote.refusals.is_empty(), "refused: {:?}", quote.refusals);
    assert_eq!(quote.from, h5);
    assert_eq!(quote.to, "renamed.h5");
    assert!(quote.fields > 0, "no field would be rewritten");
    assert!(!quote.objects.is_empty(), "no object was named");
    // And it changed nothing: a quote a caller shows before deciding must be
    // safe to ask for on every keystroke.
    assert!(!package.is_dirty(), "planning modified the package");
}

#[test]
fn a_refusal_arrives_as_words_rather_than_as_an_empty_plan() {
    let Some((epc, h5)) = workbench("refused", "testingPackageCpp22.epc", "testingPackageCpp22.h5") else {
        return;
    };
    let mut package = Package::open(&epc).expect("the package opens");

    let quote = package.plan_rename(&h5, "data/renamed.h5");
    assert!(
        quote.refusals.iter().any(|line| line.contains("bare name")),
        "a path was not refused by name: {:?}",
        quote.refusals
    );
    // And the door refuses too, rather than carrying out a plan that says no.
    let why = package
        .rename_data_file(&h5, "data/renamed.h5")
        .expect_err("a refused rename was carried out");
    assert!(why.contains("refused"), "{why}");
}

#[test]
fn renaming_says_which_of_the_three_things_happened_to_the_file() {
    let Some((epc, h5)) = workbench("done", "testingPackageCpp22.epc", "testingPackageCpp22.h5") else {
        return;
    };
    let mut package = Package::open(&epc).expect("the package opens");

    let done = package
        .rename_data_file(&h5, "renamed.h5")
        .expect("it is carried out");
    assert!(done.fields > 0);
    // Three answers and not two: *moved*, *there was nothing to move*, *this
    // store has no disk*. Here the file is beside the package, so it moved, and
    // the caller is told where.
    assert_eq!(done.file, "moved");
    assert!(
        done.moved_to
            .as_deref()
            .is_some_and(|path| path.ends_with("renamed.h5")),
        "the new path was not reported: {:?}",
        done.moved_to
    );
    assert!(package.is_dirty(), "the package does not know it changed");
}
