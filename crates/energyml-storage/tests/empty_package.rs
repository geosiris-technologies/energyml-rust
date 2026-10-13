//! What a **new** package must carry so as not to open on a complaint.
//!
//! Reported as: when a new epc is created, there is always a health error: the
//! core.xml is absent. It was not a defect of the check — it is right, EPC §3.5
//! makes the core properties mandatory — it was **two writers in the repository
//! disagreeing on what a minimal EPC is**: the one that judges knew it, the one
//! that creates did not write it. The first kind of defect this workspace
//! names.

use std::sync::Arc;

use energyml_storage::{EpcStore, Severity, extract::empty_package};

/// What the front end opens when someone creates a package.
fn fresh() -> EpcStore {
    let bytes = empty_package(None).expect("an empty package");
    EpcStore::from_reader(
        Box::new(std::io::Cursor::new(bytes)),
        "new.epc",
        Arc::new(energyml_schemas::all()),
    )
    .expect("it opens")
}

#[test]
fn a_fresh_package_opens_without_a_single_complaint() {
    // **The check that would have gone red before.** Nothing else is checked here:
    // an empty package has no object, so the only verdict it can give
    // concerns its mandatory parts.
    let store = fresh();
    let said: Vec<String> = store
        .diagnostics()
        .iter()
        .filter(|one| one.severity >= Severity::Warning)
        .map(|one| format!("{}: {}", one.part, one.message))
        .collect();
    assert!(said.is_empty(), "a new package complains of: {said:?}");
}

#[test]
fn it_carries_the_three_parts_the_specification_makes_mandatory() {
    let store = fresh();
    // OPC's two, and the one the EPC adds (§3.5).
    for part in ["[Content_Types].xml", "_rels/.rels", "docProps/core.xml"] {
        assert!(store.raw_part(part).is_ok(), "`{part}` is missing");
    }
}

#[test]
fn the_core_properties_say_the_three_things_they_must() {
    // §3.5: a creator, a creation date, the specification version.
    // Read **by the repository's reader**, not by a text search: what
    // matters is that they can be read back, not that they are written somewhere.
    let store = fresh();
    let bytes = store.core_properties().expect("the core properties are there");
    let core = energyml_storage::epc::core_props::CoreProperties::parse(&bytes);
    assert!(!core.creator.is_empty(), "the creator is empty");
    assert!(core.created.ends_with('Z'), "the date is not W3CDTF UTC: {}", core.created);
    assert_eq!(core.version, energyml_storage::epc::core_props::EPC_VERSION);
}

#[test]
fn the_relationship_the_specification_requires_points_at_them() {
    // §3.5.2: the package relationship to the core properties. Without it,
    // a conforming reader does not find them — and the file would open
    // anyway here, which is exactly the kind of gap that only shows
    // with another tool.
    let store = fresh();
    let rels = String::from_utf8(store.raw_part("_rels/.rels").expect("the package rels"))
        .expect("it is text");
    assert!(rels.contains(energyml_storage::epc::core_props::CORE_RELATIONSHIP), "{rels}");
    assert!(rels.contains("docProps/core.xml"), "{rels}");
}

#[test]
fn this_crate_has_exactly_one_clock_and_callers_choose_it() {
    // **The rule learnt again.** `SystemTime::now()` on
    // `wasm32-unknown-unknown` does not return an error: it **panics**, and the
    // message names neither the function nor the crate —
    // *"time not implemented on this platform"*. Nothing catches it at
    // compile time, since the code compiles; nothing catches it in the suite,
    // since it runs natively where the clock exists. The only possible
    // safeguard is to count the clocks.
    //
    // One, in `core_props::now_w3cdtf`, which **native** callers
    // choose explicitly by passing `None`. Everything else receives its date
    // — `SaveOptions::created`, `empty_package(created)` — and that is what
    // lets the front end supply its own.
    //
    // The shape is that of the check in `retp-proto`, which keeps the same rule
    // for the same reason.
    let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
    let mut files = Vec::new();
    collect(root, &mut files);
    assert!(files.len() > 5, "the collection found nothing: {}", root.display());

    let mut readers = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).expect("a source file");
        // **Comment lines are skipped**, and the check demanded it
        // on the first try: it went red on the comment that *explains*
        // the rule. What is counted is a call, never a mention — otherwise
        // the safeguard would punish explaining it, and the answer
        // would be to stop explaining.
        for line in text.lines() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            for clock in ["SystemTime::now", "Instant::now", "chrono::Utc::now"] {
                if code.contains(clock) {
                    readers.push(format!("{} ({clock})", file.display()));
                }
            }
        }
    }
    assert_eq!(
        readers.len(),
        1,
        "this crate must have a single clock, in `core_props`: {readers:?}"
    );
    assert!(readers[0].contains("core_props.rs"), "{readers:?}");
}

fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("a readable directory") {
        let path = entry.expect("an entry").path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}
