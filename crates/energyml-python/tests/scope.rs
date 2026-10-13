//! **This bridge only bridges the original library.**
//!
//! Translation of a package from one version to another is left out of the
//! Python bridge, and so is ETP for now: they are separate packages. Only what
//! belongs in the original library is bridged.
//!
//! Both are workstreams kept **apart**, and the workspace already says so:
//! translation is a separate workstream that no existing crate depends on, and
//! `energyml-etp-store` is the only existing crate to name ETP. A dependency
//! from here to either would bring them into the wheel, into the compile time
//! and into everybody's public surface.
//!
//! It is a boundary a comment does not hold, so it is read in the `Cargo.toml`:
//! a dependency added by distraction goes red here.

use std::path::Path;

/// The crate's own manifest.
fn manifest() -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("the crate has a Cargo.toml")
}

/// The name on the left of every dependency line, in every dependency table.
fn dependencies(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            inside = line.contains("dependencies");
            continue;
        }
        if !inside || line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((name, _)) = line.split_once('=') {
            out.push(name.trim().to_string());
        }
    }
    out
}

#[test]
fn the_bridge_depends_on_no_translation_crate() {
    let found: Vec<String> = dependencies(&manifest())
        .into_iter()
        .filter(|name| name.contains("translate"))
        .collect();
    assert!(
        found.is_empty(),
        "version translation is a separate workshop and stays out of this wheel: {found:?}"
    );
}

#[test]
fn the_bridge_depends_on_no_etp_crate() {
    let found: Vec<String> = dependencies(&manifest())
        .into_iter()
        .filter(|name| name.contains("etp"))
        .collect();
    assert!(
        found.is_empty(),
        "ETP is a separate workshop and stays out of this wheel for now: {found:?}"
    );
}

#[test]
fn what_it_does_depend_on_is_the_library_itself() {
    // The other half of the promise: excluding two things is only meaningful if
    // the rest is *named*, otherwise the next dependency slips in unremarked.
    // These are the crates that read, write and interpret a package — the
    // original library, and nothing else.
    let allowed = [
        "energyml-core",
        "energyml-schemas",
        "energyml-storage",
        "energyml-domain",
        "energyml-geometry",
        "energyml-export",
        "energyml-analysis",
        "energyml-proj",
        "energyml-build",
        "pyo3",
        "numpy",
        "serde_json",
        "tempfile",
    ];
    let unexpected: Vec<String> = dependencies(&manifest())
        .into_iter()
        .filter(|name| !allowed.contains(&name.as_str()))
        .collect();
    assert!(
        unexpected.is_empty(),
        "new dependencies: {unexpected:?} — if they belong here, say so in this list and in \
the Python bridge design notes"
    );
}
