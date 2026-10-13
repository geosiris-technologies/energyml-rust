//! **Nothing of this crate lands outside `energyml.rust`.**
//!
//! The same publisher already ships `energyml.xxx` distributions for RESQML,
//! WITSML and the rest; this bridge must not interfere with that older Python
//! version.
//!
//! `energyml-resqml2-2`, `energyml-witsml2-0`, `energyml-utils` and their
//! siblings all install under `energyml/`. A file of ours landing anywhere but
//! `energyml/rust/` would overwrite one of theirs — silently, at `pip install`
//! time, and it would be their code that broke.
//!
//! This is a promise a comment cannot keep, so it is checked: the module name
//! maturin builds to, and every file of the Python source tree.

use std::path::Path;

/// The crate's `pyproject.toml`, which decides where the wheel puts things.
fn pyproject() -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("pyproject.toml"))
        .expect("the crate has a pyproject.toml")
}

#[test]
fn the_extension_is_built_under_energyml_rust() {
    let text = pyproject();
    let line = text
        .lines()
        .find(|line| line.trim_start().starts_with("module-name"))
        .expect("pyproject states a module-name");
    let value = line.split('=').nth(1).expect("module-name has a value").trim().trim_matches('"');
    assert!(
        value.starts_with("energyml.rust."),
        "the extension must be built under `energyml.rust.`, not `{value}`"
    );

    // And the distribution is not named `energyml`: a distribution of that name
    // would claim the whole folder on an index, which is exactly what must not
    // happen.
    assert!(
        text.contains("name = \"energyml-rust\""),
        "the distribution is `energyml-rust`"
    );
}

#[test]
fn every_python_file_of_this_crate_lives_under_energyml_rust() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("python");
    let under = root.join("energyml").join("rust");
    let mut seen = 0;
    let mut wrong = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(here) = stack.pop() {
        for entry in std::fs::read_dir(&here).expect("the python source tree is readable") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            seen += 1;
            if !path.starts_with(&under) {
                wrong.push(path.display().to_string());
            }
        }
    }

    // **Including `energyml/__init__.py`, which must not exist.** Without it the
    // folder is a namespace package when this bridge is alone and an ordinary
    // sub-folder when one of the other distributions has laid its own down —
    // both work, and nothing of anyone else's is overwritten.
    assert!(wrong.is_empty(), "these would land outside `energyml/rust/`: {wrong:?}");
    assert!(seen >= 2, "the python source tree is not empty: {seen} file(s)");
}
