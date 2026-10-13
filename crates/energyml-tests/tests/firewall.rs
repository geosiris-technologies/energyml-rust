//! The dependency firewall of the repository split.
//!
//! The workspace is about to become several repositories: the public library
//! (`energyml-rust`), the public ETP stack (`retp`), and private ones. A public
//! crate that depends on a private one — even optionally, even as a
//! dev-dependency — cannot be published, because Cargo resolves every
//! dependency into the lockfile. And the library must not depend on `retp`,
//! which depends on the library: that would be a cycle between repositories.
//!
//! The rules, checked on `cargo metadata`:
//! - a library crate depends on no private crate and no ETP crate;
//! - an ETP crate depends on no private crate;
//! - `energyml-geostat` depends on no `energyml-*` crate at all — it is the
//!   domain-neutral computation crate.
//!
//! The checker is a pure function so that it can be shown to fail: the first
//! test feeds it a workspace that breaks each rule.

use std::collections::BTreeMap;
use std::process::Command;

/// Where a crate goes once the workspace is split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Repo {
    Library,
    Etp,
    Private,
}

/// The private crates. Everything else is public, in the library or in ETP.
const PRIVATE: &[&str] = &[
    "energyml-codegen",
    "energyml-translate",
    "energyml-translate-epc",
    "energyml-translate-cli",
    "energyml-wasm",
    "energyml-ui",
];

/// The crate that must stay free of any `energyml-*` dependency.
const NEUTRAL: &str = "energyml-geostat";

fn repo_of(name: &str) -> Repo {
    if PRIVATE.contains(&name) {
        Repo::Private
    } else if name.starts_with("retp-") || name == "energyml-etp-store" || name == "etptypes" {
        Repo::Etp
    } else {
        Repo::Library
    }
}

/// Every violation, as `crate -> dependency (kind): rule`.
fn violations(workspace: &BTreeMap<String, Vec<(String, String)>>) -> Vec<String> {
    let mut found = Vec::new();
    for (name, deps) in workspace {
        let from = repo_of(name);
        for (dep, kind) in deps {
            // Judged by name, member or not: once split, a public tree reaches a
            // private crate through a path to a sibling repository, and that crate
            // is not a member of the tree that depends on it.
            if !(dep.starts_with("energyml-") || dep.starts_with("retp-") || dep == "etptypes") {
                continue; // a third-party crate
            }
            let to = repo_of(dep);
            let rule = match (from, to) {
                (Repo::Library, Repo::Private) => Some("a library crate depends on a private crate"),
                (Repo::Library, Repo::Etp) => {
                    Some("the library depends on the ETP repository, which depends on the library")
                }
                (Repo::Etp, Repo::Private) => Some("an ETP crate depends on a private crate"),
                _ => None,
            };
            if let Some(rule) = rule {
                found.push(format!("{name} -> {dep} ({kind}): {rule}"));
            }
            if name == NEUTRAL && dep.starts_with("energyml-") {
                found.push(format!(
                    "{name} -> {dep} ({kind}): the neutral crate depends on an energyml crate"
                ));
            }
        }
    }
    found
}

/// The workspace members and their dependencies (every kind, optional ones included).
fn workspace() -> BTreeMap<String, Vec<(String, String)>> {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps", "--offline"])
        .current_dir(root)
        .output()
        .expect("cargo metadata runs");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let meta: serde_json::Value = serde_json::from_slice(&output.stdout).expect("cargo metadata is JSON");
    let mut workspace = BTreeMap::new();
    for package in meta["packages"].as_array().expect("packages") {
        let name = package["name"].as_str().expect("name").to_string();
        let deps = package["dependencies"]
            .as_array()
            .expect("dependencies")
            .iter()
            .map(|d| {
                let dep = d["name"].as_str().expect("dependency name").to_string();
                let kind = d["kind"].as_str().unwrap_or("normal");
                let optional = if d["optional"].as_bool() == Some(true) {
                    ", optional"
                } else {
                    ""
                };
                (dep, format!("{kind}{optional}"))
            })
            .collect();
        workspace.insert(name, deps);
    }
    workspace
}

#[test]
fn the_checker_names_each_broken_rule() {
    let ws: BTreeMap<String, Vec<(String, String)>> = [
        (
            "energyml-cli",
            vec![
                ("energyml-translate", "normal, optional"),
                ("retp-client", "normal"),
            ],
        ),
        ("energyml-etp-store", vec![("energyml-wasm", "dev")]),
        ("energyml-geostat", vec![("energyml-core", "normal")]),
        ("energyml-translate", vec![("energyml-core", "normal")]),
        ("energyml-wasm", vec![("retp-proto", "normal")]),
        ("retp-client", vec![]),
        ("retp-proto", vec![]),
        ("energyml-core", vec![]),
    ]
    .into_iter()
    .map(|(n, d)| {
        (
            n.to_string(),
            d.into_iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
        )
    })
    .collect();
    let found = violations(&ws);
    assert_eq!(found.len(), 4, "{found:#?}");
    // A private crate that is not a member of the tree is still a private crate.
    let mut lone = BTreeMap::new();
    lone.insert(
        "energyml-cli".to_string(),
        vec![("energyml-translate-cli".to_string(), "normal".to_string())],
    );
    assert_eq!(
        violations(&lone).len(),
        1,
        "a dependency on a private crate outside the workspace went unseen"
    );
    assert!(
        found
            .iter()
            .any(|v| v.starts_with("energyml-cli -> energyml-translate"))
    );
    assert!(found.iter().any(|v| v.starts_with("energyml-cli -> retp-client")));
    assert!(
        found
            .iter()
            .any(|v| v.starts_with("energyml-etp-store -> energyml-wasm"))
    );
    assert!(
        found
            .iter()
            .any(|v| v.starts_with("energyml-geostat -> energyml-core"))
    );
    // Private crates may depend on anything public, and that is not reported.
    assert!(
        !found
            .iter()
            .any(|v| v.starts_with("energyml-wasm") || v.starts_with("energyml-translate"))
    );
}

#[test]
fn no_public_crate_depends_on_what_it_cannot_publish_with() {
    let ws = workspace();
    // A firewall over an empty or truncated workspace would pass for nothing. The
    // same test runs in the monorepo and in the public `energyml-rust` tree, so it
    // asks only for crates that both hold.
    for expected in ["energyml-core", "energyml-cli", "energyml-storage", NEUTRAL] {
        assert!(
            ws.contains_key(expected),
            "`{expected}` is not a workspace member — the check would prove nothing"
        );
    }
    let found = violations(&ws);
    assert!(
        found.is_empty(),
        "dependencies that would break the repository split:\n{}",
        found.join("\n")
    );
}
