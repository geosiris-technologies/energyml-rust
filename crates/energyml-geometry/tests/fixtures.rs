//! Where a test finds a reference package.
//!
//! The public suite reads the pinned packages of `tests/fixtures.lock`
//! (`testingPackageCpp*`, `block`, `s_bend`, `tic_tac_toe`, `wren`), fetched
//! into `target/fixtures/` by `scripts/fetch-fixtures.py`. It never reads the
//! private corpus. Lookup order, one directory after the other:
//!
//! 1. `$ENERGYML_FIXTURES_DIR`;
//! 2. `<workspace>/target/fixtures/`;
//! 3. `<workspace>/testdata/` (so the monorepo keeps working).
//!
//! In each, a package is looked for at the root and under `fesapi/` and
//! `resqpy/` (the layout of the lock). Nothing found: **a panic when `$ENERGYML_FIXTURES_REQUIRED` is
//! set** - a missing reference package is a failure in CI, never a skip - and
//! `None` otherwise, the caller printing that it skipped.
//!
//! The same file is in every test crate that reads packages; keep the copies
//! identical.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// The EPC files the lock lists. The only names the public suite may read.
pub const PUBLIC_EPCS: &[&str] = &[
    "testingPackageCpp.epc",
    "testingPackageCpp22.epc",
    "block.epc",
    "s_bend.epc",
    "tic_tac_toe.epc",
    "wren.epc",
];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn in_ci() -> bool {
    std::env::var_os("ENERGYML_FIXTURES_REQUIRED").is_some()
}

fn roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(dir) = std::env::var_os("ENERGYML_FIXTURES_DIR") {
        roots.push(PathBuf::from(dir));
    }
    roots.push(workspace().join("target").join("fixtures"));
    roots.push(workspace().join("testdata"));
    roots
}

fn locate(name: &str) -> Option<PathBuf> {
    for root in roots() {
        for sub in ["", "fesapi", "resqpy"] {
            let candidate = root.join(sub).join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// The path of one reference file (`testingPackageCpp22.epc`,
/// `testingPackageCpp22.h5`, …), or `None` when absent outside CI.
///
/// # Panics
/// When the file is absent and `$ENERGYML_FIXTURES_REQUIRED` is set.
pub fn find(name: &str) -> Option<PathBuf> {
    let found = locate(name);
    if found.is_none() && in_ci() {
        panic!(
            "reference package `{name}` not found (looked in $ENERGYML_FIXTURES_DIR, target/fixtures, testdata): \
             a missing reference package is a failure in CI, never a skip - run scripts/fetch-fixtures.py"
        );
    }
    found
}

/// Every public reference package present, in the order of [`PUBLIC_EPCS`].
/// Empty (outside CI) when none was fetched.
///
/// # Panics
/// When one of them is absent and `$ENERGYML_FIXTURES_REQUIRED` is set.
pub fn public_corpus() -> Vec<PathBuf> {
    PUBLIC_EPCS.iter().filter_map(|name| find(name)).collect()
}

/// [`find`] that also prints the skip, in the style of the tests.
pub fn find_or_skip(name: &str) -> Option<PathBuf> {
    let found = find(name);
    if found.is_none() {
        eprintln!("skipped: {name} is not in the fixtures (scripts/fetch-fixtures.py)");
    }
    found
}

/// The private corpus: every EPC under `testdata/` the lock does
/// not list. Real-file campaign, private. Never panics: the files are not
/// public, so their absence is never a CI failure.
#[cfg(feature = "private-corpus")]
pub fn private_corpus() -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![workspace().join("testdata")];
    while let Some(dir) = stack.pop() {
        // A directory that says it is not corpus is not walked.
        if dir.join("NOT-CORPUS").exists() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("epc"))
                && !PUBLIC_EPCS
                    .iter()
                    .any(|n| path.file_name().is_some_and(|f| f == *n))
            {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// One private package by file name, `None` when absent.
#[cfg(feature = "private-corpus")]
pub fn private_find(name: &str) -> Option<PathBuf> {
    let path = workspace().join("testdata").join(name);
    path.is_file().then_some(path)
}
