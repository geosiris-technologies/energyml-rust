//! The guard of the identity rule - internally, the full uri and nothing else
//! (ADR-0003: one single entry point to define an identity for an object, in the
//! web page as in Rust).
//!
//! The rule was written four times and broken as often: a uuid lookup here, a
//! hand-written `uuid_of_uri` there, each one a second implementation that
//! answers two things as soon as a package holds an object in two versions. A
//! rule that lives in a sentence is kept by whoever remembers it; this test is
//! the one that **walks the sources** and fails on the forbidden call, so that a
//! new one cannot slip in outside the adapters.
//!
//! What is forbidden, and where it is allowed:
//!
//! | needle | why | allowed in |
//! |---|---|---|
//! | `find_by_uuid(` | a lookup by a bare uuid answers *one of* the versions | the adapters (`energyml-storage`, the faces: `-cli`, `-python`, plus the monorepo-only faces of `PRIVATE_ADAPTERS`), and the exceptions below |
//! | `lookup_uuid(` | the same, refusing when several | the same |
//! | `",version='"` | reading the version out of a uri by hand | `energyml-core`, `energyml-storage` |
//! | `fn is_uuid(` | a second shape of a uuid | `energyml-core` (`identifiers::is_uuid`) |
//! | `fn uuid_of_uri(`, `fn uuid_in(`, `fn uuid_of_key(` | a second reading of an address | nowhere but the exception below |
//!
//! An exception is a **file and a reason**, and it must still be needed: an
//! allowance no source matches any more fails the test too - otherwise the list
//! only ever grows. The comment lines and everything after `#[cfg(test)]` are not
//! read (a unit test builds a uuid by hand on purpose).
//!
//! This reads **our own sources**, not a document of reference: the rule it
//! states is the project's, and the sources are versioned beside it.
//!
//! *What would break it*: write `store.find_by_uuid(uuid)` in
//! `energyml-geometry/src/` - the test names the file and the line.

use std::fs;
use std::path::{Path, PathBuf};

/// One thing no source may contain outside the places that are allowed.
struct Rule {
    needle: &'static str,
    why: &'static str,
    /// Path prefixes, relative to `crates/`, where the needle is the adapter or the definition.
    allowed_under: &'static [&'static str],
    /// Whether every adapter (public or monorepo-only) may also contain the needle.
    adapters_too: bool,
}

/// The adapters that exist in the public tree.
const PUBLIC_ADAPTERS: &[&str] = &[
    "energyml-storage/src/",
    "energyml-cli/src/",
    "energyml-python/src/",
];

/// Faces that exist only in the monorepo (the private repositories). They are
/// adapters like the others, but the public tree does not hold them: a prefix
/// here matches nothing there, which is fine, and no test below requires one to
/// be present. Adding a private face is one entry here; the public tree never
/// has to know it.
const PRIVATE_ADAPTERS: &[&str] = &["energyml-wasm/src/", "energyml-ui/src/"];

fn is_adapter(file: &str) -> bool {
    PUBLIC_ADAPTERS
        .iter()
        .chain(PRIVATE_ADAPTERS)
        .any(|prefix| file.starts_with(prefix))
}

const RULES: &[Rule] = &[
    Rule {
        needle: "find_by_uuid(",
        why: "a lookup by a bare uuid answers one of the versions of the object: follow a reference by `ObjectSource::object_of` / `EpcStore::resolve_reference`, an address by `EpcStore::resolve`",
        allowed_under: &[],
        adapters_too: true,
    },
    Rule {
        needle: "lookup_uuid(",
        why: "a lookup by a bare uuid is the entry adapter of the library, not what internal code asks",
        allowed_under: &[],
        adapters_too: true,
    },
    Rule {
        needle: "\",version='\"",
        why: "the version of a uri is read by `identity::parse_address` (or `EtpUri::parse`), never split out by hand",
        allowed_under: &["energyml-core/src/", "energyml-storage/src/"],
        adapters_too: false,
    },
    Rule {
        needle: "fn is_uuid(",
        why: "the shape of a uuid is `energyml_core::identifiers::is_uuid`, written once",
        allowed_under: &["energyml-core/src/"],
        adapters_too: false,
    },
    Rule {
        needle: "fn uuid_of_uri(",
        why: "an address is read by `identity::parse_address`, written once",
        allowed_under: &["energyml-core/src/"],
        adapters_too: false,
    },
    Rule {
        needle: "fn uuid_in(",
        why: "an address is read by `identity::parse_address`, written once",
        allowed_under: &["energyml-core/src/"],
        adapters_too: false,
    },
    Rule {
        needle: "fn uuid_of_key(",
        why: "an address is read by `identity::parse_address`, written once",
        allowed_under: &["energyml-core/src/"],
        adapters_too: false,
    },
];

/// A file where a needle stays, and why. Each must still match something.
const EXCEPTIONS: &[(&str, &str, &str)] = &[
    (
        "energyml-build/src/apply.rs",
        "find_by_uuid(",
        "an existence check of the uuid a caller named for a template (`--set template=…`): *is there such an object*, which is what `find_by_uuid` answers, and never *which version*",
    ),
    (
        "energyml-domain/src/package_index.rs",
        "find_by_uuid(",
        "`PackageIndex::update` is given the **uuids** the change journal names; the store holding a uuid twice is decided before (`summaries_of`), and the update then rebuilds",
    ),
    (
        "energyml-build/src/read.rs",
        "lookup_uuid(",
        "a reference that names no type, or a uuid a holder has no reference to: the one entry adapter of the build readers, which refuses a uuid that names several",
    ),
    (
        "energyml-section/src/read/surfaces.rs",
        "fn uuid_of_uri(",
        "a public name the section view's own users import; its body is `identity::parse_address`",
    ),
];

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

/// Every `src/` source of the library, as (path relative to `crates/`, lines to read):
/// the comments and the unit tests left out.
fn sources() -> Vec<(String, Vec<(usize, String)>)> {
    let root = crates_dir();
    let mut found = Vec::new();
    for entry in fs::read_dir(&root).expect("the crates directory reads").flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // The generated crates are what the generator writes, not what is read.
        if name == "generated" || !entry.path().is_dir() {
            continue;
        }
        let mut files = Vec::new();
        rust_sources(&entry.path().join("src"), &mut files);
        for file in files {
            let relative = file
                .strip_prefix(&root)
                .expect("under crates/")
                .to_string_lossy()
                .replace('\\', "/");
            let text = fs::read_to_string(&file).unwrap_or_default();
            let mut lines = Vec::new();
            for (at, line) in text.lines().enumerate() {
                if line.contains("#[cfg(test)]") {
                    break;
                }
                if line.trim_start().starts_with("//") {
                    continue;
                }
                lines.push((at + 1, line.to_string()));
            }
            found.push((relative, lines));
        }
    }
    found
}

fn allowed(rule: &Rule, file: &str) -> bool {
    rule.allowed_under.iter().any(|prefix| file.starts_with(prefix))
        || (rule.adapters_too && is_adapter(file))
        || EXCEPTIONS
            .iter()
            .any(|(path, needle, _)| *path == file && *needle == rule.needle)
}

#[test]
fn no_internal_code_looks_an_object_up_by_a_bare_uuid_or_reads_an_address_by_hand() {
    let sources = sources();
    assert!(
        sources.len() > 100,
        "the walk found {} sources: it is not looking where it thinks",
        sources.len()
    );
    let mut violations = Vec::new();
    for (file, lines) in &sources {
        for rule in RULES {
            if allowed(rule, file) {
                continue;
            }
            for (at, line) in lines {
                if line.contains(rule.needle) {
                    violations.push(format!("{file}:{at}: `{}` - {}", rule.needle, rule.why));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "the identity rule is broken:\n{}",
        violations.join("\n")
    );
}

/// An exception that no source needs any more is removed: the list does not
/// outlive its causes.
#[test]
fn every_exception_is_still_needed() {
    let sources = sources();
    for (path, needle, reason) in EXCEPTIONS {
        let used = sources
            .iter()
            .filter(|(file, _)| file == path)
            .any(|(_, lines)| lines.iter().any(|(_, line)| line.contains(needle)));
        assert!(
            used,
            "`{path}` no longer contains `{needle}`: remove its exception ({reason})"
        );
    }
}

/// The guard can fail: a source that breaks the rule is named. (The walk of the
/// first test is by `sources()`; this one feeds the same rule a line it must
/// refuse - and a line in an adapter it must let through.)
#[test]
fn the_rule_names_a_forbidden_call_and_lets_an_adapter_through() {
    let rule = &RULES[0];
    assert!(
        !allowed(rule, "energyml-geometry/src/scene.rs"),
        "internal code may not look a uuid up"
    );
    assert!(
        allowed(rule, "energyml-cli/src/main.rs"),
        "a face is the entry of the library"
    );
    // A fabricated crate no list names is not an adapter, whatever its name suggests.
    assert!(
        !allowed(rule, "energyml-unlisted-face/src/lib.rs"),
        "only the listed adapters are let through"
    );
    // The same violation seen by the walk: a line in a non-adapter source is refused.
    assert!(
        !is_adapter("energyml-geometry/src/scene.rs") && "store.lookup_uuid(&u)".contains(RULES[1].needle)
    );
    assert!(
        allowed(rule, "energyml-build/src/apply.rs"),
        "an exception names its file"
    );
    assert!(
        !allowed(rule, "energyml-build/src/read.rs"),
        "another file of the same crate is not covered"
    );
    assert!("let found = store.find_by_uuid(&uuid);".contains(rule.needle));
}
