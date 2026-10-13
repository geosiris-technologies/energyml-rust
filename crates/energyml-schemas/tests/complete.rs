//! The list in `lib.rs` against the generated crates on disk.
//!
//! A new Energistics release is generated into `crates/generated/<crate>/`.
//! Everything downstream hangs on that crate being *registered*: a generated
//! crate has to be handed to a `Registry` by somebody, and until 02/08 that
//! somebody was written out twice in production and eighteen more times across
//! the test suites. Nothing detected an omission: leaving a new crate out
//! produced no compile error and no failing test - only a tool that quietly
//! could not read the new version, and a suite that quietly never covered it.
//!
//! This is the test that makes "one registration line" true. One list, and a
//! failure that names what is missing.
//!
//! **What this test no longer reads is `codegen.toml`.** That file declares the
//! releases to *generate*; it belongs to the private generator repository
//!, and the check "every `[[package]]` of `codegen.toml` is registered
//! and conversely" lives there. What holds in the public tree is the other end
//! of the same chain: the crates that exist under `crates/generated/` and the
//! registry agree.

use std::path::PathBuf;

/// A schema `(package, version)`, as the registry spells it.
type Pair = (String, String);

/// The `(package, version)` a generated crate directory is named after:
/// `energyml-resqml-2-0-1` is `("resqml", "2.0.1")`, `energyml-resqml-2-2-dev3`
/// is `("resqml", "2.2_dev3")`. `None` for a directory that is no schema version
/// (`retp-types`, the ETP messages).
fn pair_of_directory(name: &str) -> Option<Pair> {
    let mut parts = name.strip_prefix("energyml-")?.split('-');
    let package = parts
        .next()
        .filter(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_lowercase()))?;
    let mut numbers: Vec<&str> = Vec::new();
    let mut suffix = None;
    for part in parts {
        if part.chars().all(|c| c.is_ascii_digit()) && !part.is_empty() && suffix.is_none() {
            numbers.push(part);
        } else if part.starts_with("dev") && suffix.is_none() {
            suffix = Some(part);
        } else {
            return None;
        }
    }
    if numbers.is_empty() {
        return None;
    }
    let mut version = numbers.join(".");
    if let Some(drop) = suffix {
        version.push('_');
        version.push_str(drop);
    }
    Some((package.to_string(), version))
}

/// The `(package, version)` pairs of the generated crates present on disk -
/// the directories of `crates/generated/` that hold a `Cargo.toml` and are
/// named after a schema version.
///
/// Read from the directory listing rather than from the generator: the point is
/// to check what *exists* against the registry, and borrowing the generator's
/// own list would let a bug in it hide the disagreement.
fn on_disk() -> Vec<Pair> {
    let generated = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../generated");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&generated)
        .unwrap_or_else(|e| panic!("{} is the home of the generated crates: {e}", generated.display()))
    {
        let path = entry.expect("a directory entry").path();
        if !path.join("Cargo.toml").is_file() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .expect("a utf-8 directory name");
        if let Some(pair) = pair_of_directory(name) {
            out.push(pair);
        }
    }
    out.sort();
    out.dedup();
    out
}

/// What is on one side and not on the other: `(only_on_disk, only_registered)`.
fn disagreements(on_disk: &[Pair], registered: &[Pair]) -> (Vec<Pair>, Vec<Pair>) {
    let missing = on_disk
        .iter()
        .filter(|pair| !registered.contains(pair))
        .cloned()
        .collect();
    let extra = registered
        .iter()
        .filter(|pair| !on_disk.contains(pair))
        .cloned()
        .collect();
    (missing, extra)
}

/// Every generated crate is registered, and every registered version has its
/// crate.
///
/// Both directions matter, and they fail for opposite reasons. A crate on disk
/// missing from the registry is the new-release case: the crate exists,
/// nothing hands it to anybody, and every tool in the workspace silently cannot
/// read it. A registered version with no crate is the reverse - a registration
/// kept alive after its crate was removed.
#[test]
fn every_generated_version_is_registered() {
    let on_disk = on_disk();
    assert!(
        !on_disk.is_empty(),
        "no generated crate found under crates/generated/; the directory reader above is broken, not the registry"
    );
    // A crate whose feature is off is on disk and rightly not registered. A
    // crate the table does not know is kept, so that it is reported here too.
    let on_disk: Vec<Pair> = on_disk
        .into_iter()
        .filter(
            |(package, version)| match energyml_schemas::known_version(package, version) {
                Some(known) => known.is_compiled(),
                None => true,
            },
        )
        .collect();

    let registered: Vec<Pair> = energyml_schemas::registered()
        .into_iter()
        .map(|(package, version)| (package.to_string(), version.to_string()))
        .collect();

    let (missing, extra) = disagreements(&on_disk, &registered);
    assert!(
        missing.is_empty(),
        "crates/generated/ holds {missing:?}, and `energyml_schemas::all` does not register them.
         Add the dependency to crates/energyml-schemas/Cargo.toml and the `register` line to its `lib.rs` - that is the whole of what a new Energistics release costs."
    );
    assert!(
        extra.is_empty(),
        "`energyml_schemas::all` registers {extra:?}, which has no crate under crates/generated/.
         Either the crate was removed and the registration should go with it, or the directory was renamed."
    );
}

/// The two helpers above can fail: a name that is no schema version is refused,
/// the directory spellings map to the registry's, and a registry that lacks a
/// crate (or keeps one too many) is reported on the right side. Without this,
/// the test above could pass on an empty or mis-parsed listing.
#[test]
fn the_disk_reader_and_the_comparison_can_fail() {
    let pair = |p: &str, v: &str| (p.to_string(), v.to_string());
    assert_eq!(
        pair_of_directory("energyml-resqml-2-0-1"),
        Some(pair("resqml", "2.0.1"))
    );
    assert_eq!(
        pair_of_directory("energyml-resqml-2-2-dev3"),
        Some(pair("resqml", "2.2_dev3"))
    );
    assert_eq!(pair_of_directory("energyml-eml-2-3"), Some(pair("eml", "2.3")));
    assert_eq!(pair_of_directory("retp-types"), None);
    assert_eq!(pair_of_directory("schemas.lock"), None);

    let disk = vec![pair("eml", "2.3"), pair("resqml", "2.2")];
    let (missing, extra) = disagreements(&disk, &[pair("eml", "2.3")]);
    assert_eq!((missing, extra), (vec![pair("resqml", "2.2")], vec![]));
    let (missing, extra) = disagreements(
        &disk,
        &[pair("eml", "2.3"), pair("resqml", "2.2"), pair("witsml", "2.1")],
    );
    assert_eq!((missing, extra), (vec![], vec![pair("witsml", "2.1")]));

    // And the listing is not trivially the whole registry by construction: it
    // finds the released RESQML, the drop and the oldest EML.
    let found = on_disk();
    for expected in [
        pair("resqml", "2.2"),
        pair("resqml", "2.2_dev3"),
        pair("eml", "2.0"),
    ] {
        assert!(
            found.contains(&expected),
            "{expected:?} is not among the crates found: {found:?}"
        );
    }
}

/// The registry actually holds objects for every version, not just an empty
/// registration.
///
/// A `register` line that compiles proves nothing about what it put in. This is
/// the difference between "the list is complete" and "the list works".
#[test]
fn every_registered_version_carries_top_level_types() {
    let registry = energyml_schemas::all();
    for (package, version) in energyml_schemas::registered() {
        let count = registry
            .top_level_types()
            .filter(|info| info.package == package && info.version == version)
            .count();
        assert!(
            count > 0,
            "{package} {version} is registered and holds no top-level type"
        );
    }
}

// ---------------------------------------------------------------------------
// Development drops
// ---------------------------------------------------------------------------
//
// Energistics ships `resqml 2.2_dev3` beside the released 2.2, and the drop's
// schemas declare the release's own `version="2.2"`. So the two are
// indistinguishable on the wire, they compute the *same* qualified type, and
// the maps that resolve one hold a single index per key. Everything below is
// about that collision: what the drop must put on the wire, who owns the
// shared key, and that the drop is still reachable anyway.

/// The drop writes its **release** into content and qualified types.
///
/// This is the end-to-end half of a rule spelled twice - `Package::schema_version`
/// in the generator and `schema_version_of` in the derive - which cannot call
/// each other. Testing the two functions would test the copies; this tests what
/// was actually generated, so a drift in either one fails here.
///
/// Getting it wrong is not a cosmetic slip: `version=2.2_dev3` in a content
/// type and `resqml22_dev3.` in a qualified type are spellings no other
/// implementation reads, in files this workspace would then be alone able to
/// open.
#[cfg(feature = "resqml22dev3")]
#[test]
fn a_development_drop_puts_its_release_on_the_wire() {
    let registry = energyml_schemas::all();
    let drop: Vec<_> = registry
        .types()
        .filter(|info| info.package == "resqml" && info.version == "2.2_dev3")
        .collect();
    assert!(
        !drop.is_empty(),
        "resqml 2.2_dev3 registered nothing; the rest of this test would pass over an empty set"
    );

    for info in &drop {
        assert_eq!(
            info.schema_version, "2.2",
            "{} reports schema_version `{}`; the drop's own schemas say 2.2, and that is what a \
             document written against it carries",
            info.name, info.schema_version
        );
    }
    for info in drop.iter().filter(|i| i.is_top_level) {
        let qualified = info.qualified_type.unwrap_or("");
        assert!(
            qualified.starts_with("resqml22."),
            "{} has qualified type `{qualified}`; nothing outside this workspace reads a \
             `resqml22_dev3.` prefix",
            info.name
        );
    }
}

/// The **release** owns a qualified type its drop shares - *whatever order
/// they were registered in*.
///
/// `resqml22.TriangulatedSetRepresentation` names a type in both crates, and
/// `by_qualified_type` holds one index per key. Whoever owns it owns every DOR
/// resolution and every JSON `$type` of a released 2.2 document, and losing it
/// to the drop fails nothing: the objects are plausible and mostly identical.
///
/// **The order here is the whole test.** `energyml_schemas::all` registers the
/// drop first, so under the plain "later registration wins" rule the release
/// would take the key back by luck; a test over `all()` therefore passes with
/// the rule in `Registry::register` deleted - measured, on the first attempt at
/// writing this. So the registry is built here by hand with the drop **last**,
/// which is the order that actually asks the question.
///
/// Which release a document is *read* as stays the reader's decision, over
/// ordered candidates; this only settles who owns the shared key.
#[cfg(all(feature = "resqml22", feature = "resqml22dev3"))]
#[test]
fn a_drop_never_takes_the_qualified_type_of_its_release() {
    let mut registry = energyml_core::registry::Registry::new();
    energyml_resqml_2_2::register(&mut registry);
    energyml_resqml_2_2_dev3::register(&mut registry);

    for name in [
        "resqml22.TriangulatedSetRepresentation",
        "resqml22.IjkGridRepresentation",
    ] {
        let entry = registry
            .by_qualified_type(name)
            .unwrap_or_else(|| panic!("{name} resolves to nothing"));
        assert_eq!(
            entry.info.version, "2.2",
            "{name} resolves to resqml {} - the drop was registered last and took the \
             key of the release it precedes",
            entry.info.version
        );
    }
}

/// The shipped registry agrees, which is what applications actually get.
#[cfg(feature = "resqml22")]
#[test]
fn the_shipped_registry_resolves_a_shared_key_to_the_release() {
    let registry = energyml_schemas::all();
    let entry = registry
        .by_qualified_type("resqml22.TriangulatedSetRepresentation")
        .expect("resolves to nothing");
    assert_eq!(entry.info.version, "2.2");
}

/// …and the drop is still reachable, or owning the key would merely have made
/// it useless.
///
/// `by_name` is keyed on `(package, version, name)`, which the drop does not
/// share with anybody, so this is the address that stays its own.
#[cfg(all(feature = "resqml22", feature = "resqml22dev3"))]
#[test]
fn a_drop_is_still_addressable_by_its_own_version() {
    let registry = energyml_schemas::all();
    let entry = registry
        .by_name("resqml", "2.2_dev3", "TriangulatedSetRepresentation")
        .expect("the drop must stay addressable by its directory version");
    assert_eq!(entry.info.version, "2.2_dev3");
    assert_eq!(entry.info.schema_version, "2.2");
}

/// The drop is **read**, and never **offered for creation**.
///
/// No object is ever made in 2.2dev3: it is a version supported only for those
/// who built models before 2.2 was published, and almost nobody supports it.
///
/// Before `may_create`, the front end's *new object* chooser offered every type
/// carrying a qualified type, and a search for `model` listed
/// `resqml22.Model` and `resqml22.EarthModelInterpretation` once for the
/// release and once for the drop - under one name, since the drop writes the
/// release's prefix (`a_development_drop_puts_its_release_on_the_wire`).
///
/// Each check stands on a set the test first proves non-empty, and they hold
/// one another: a rule that refused everything would pass the first loop, one
/// that refused nothing would pass the second, and one that looked at the
/// version alone would offer the nested type.
#[cfg(all(feature = "resqml22", feature = "resqml22dev3"))]
#[test]
fn a_development_drop_is_read_but_never_offered() {
    let registry = energyml_schemas::all();

    let drop: Vec<_> = registry
        .top_level_types()
        .filter(|info| energyml_core::naming::is_prerelease(info.version))
        .collect();
    assert!(
        !drop.is_empty(),
        "no development drop is registered; the loop below would pass over an empty set"
    );
    for info in &drop {
        assert!(
            !energyml_schemas::may_create(info),
            "{} of {} {} is offered for creation - a development drop is registered to be read, \
             never to be written",
            info.name,
            info.package,
            info.version
        );
    }

    // The release it precedes is offered, type for type.
    for name in [
        "Model",
        "EarthModelInterpretation",
        "TriangulatedSetRepresentation",
    ] {
        let entry = registry
            .by_name("resqml", "2.2", name)
            .unwrap_or_else(|| panic!("resqml 2.2 has no {name}"));
        assert!(
            energyml_schemas::may_create(entry.info),
            "resqml 2.2 {name} is a release's top-level type, and it is not offered"
        );
    }

    // …and a nested type is not, whatever its version: it names no part.
    let nested = registry
        .types()
        .find(|info| info.package == "resqml" && info.version == "2.2" && info.qualified_type.is_none())
        .expect("resqml 2.2 registers nested types too");
    assert!(
        !energyml_schemas::may_create(nested),
        "{} is nested and offered",
        nested.name
    );

    // Refusing the offer must not have cost the read: the drop's own address
    // still resolves to the drop.
    let kept = registry
        .by_name("resqml", "2.2_dev3", "Model")
        .expect("the drop must stay addressable by its directory version");
    assert_eq!(kept.info.version, "2.2_dev3");
}

/// Every type offered for creation is **what its qualified type creates**.
///
/// The qualified type is the only thing a list passes on: the front end calls
/// `create("resqml22.Model")`, and `Registry::create` resolves that through
/// `by_qualified_type`. So a row whose key resolves to *another* type is a row
/// that makes something other than what it shows - sixty of them while the
/// drop was offered, each creating the release's type under the drop's row.
///
/// It also makes the front end's row key sound. Two offered types sharing a
/// qualified type cannot both resolve to themselves, so this is the same
/// assertion as *no key twice* - and a key twice was the other half of what
/// the front end showed: `resqml22.Model` three times and
/// `resqml22.EarthModelInterpretation` four, for a list holding each twice. A
/// keyed list in a front end patches every old row sharing a key into the one new row
/// holding it, and the others stay in the front end, orphaned where they were - as
/// many as there had been re-renders with the pair on both sides.
#[test]
fn every_creatable_type_is_what_its_qualified_type_creates() {
    let registry = energyml_schemas::all();
    let mut offered = 0usize;
    let mut elsewhere = Vec::new();
    for info in registry.types().filter(|info| energyml_schemas::may_create(info)) {
        offered += 1;
        let qualified = info
            .qualified_type
            .expect("may_create offers only types that name a part");
        match registry.by_qualified_type(qualified) {
            Some(entry) if std::ptr::eq(entry.info, info) => {}
            Some(entry) => elsewhere.push(format!(
                "{qualified} ({} {}) creates {} {} {}",
                info.package, info.version, entry.info.package, entry.info.version, entry.info.name
            )),
            None => elsewhere.push(format!(
                "{qualified} ({} {}) creates nothing",
                info.package, info.version
            )),
        }
    }
    assert!(
        offered > 0,
        "nothing is offered for creation; the loop above proved nothing"
    );
    assert!(
        elsewhere.is_empty(),
        "{} offered rows make something other than what they show:\n{}",
        elsewhere.len(),
        elsewhere.join("\n")
    );
}

/// RESQML 2.0.1 is spelt `resqml20` (its `schemaVersion` is `2.0`) and fesapi
/// still writes `version=2.0.1` in some references, which compacts to
/// `resqml201`. Both spellings resolve to the same type, by qualified type
/// and by content type. Breaks if only the schema-version spelling is
/// registered - the second lookup was `None` on 04/09.
#[cfg(feature = "resqml20")]
#[test]
fn a_release_version_spelling_resolves_like_its_schema_version_spelling() {
    let registry = energyml_schemas::all();
    let by_schema = registry
        .by_qualified_type("resqml20.obj_ActivityTemplate")
        .expect("resqml20");
    let by_release = registry
        .by_qualified_type("resqml201.obj_ActivityTemplate")
        .expect("resqml201");
    assert!(std::ptr::eq(by_schema.info, by_release.info));
    let by_content = registry
        .by_content_type("application/x-resqml+xml;version=2.0.1;type=obj_ActivityTemplate")
        .expect("version=2.0.1");
    assert!(std::ptr::eq(by_schema.info, by_content.info));
    // And a version nothing has is still nothing.
    assert!(
        registry
            .by_qualified_type("resqml209.obj_ActivityTemplate")
            .is_none()
    );
}
