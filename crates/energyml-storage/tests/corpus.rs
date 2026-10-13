//! The reader, against the EPC files it will actually be given.
//!
//! The pinned reference packages are fetched, never committed, so every test
//! here **skips** when they are absent outside CI (and fails in CI, where
//! `fixtures.rs` panics). A test that goes red on a fresh clone teaches
//! contributors to ignore red tests.
//!
//! What is checked is deliberately not "it parses": it is that the three levels
//! of ADR-0002 agree with each other. The identity a
//! listing shows must be the identity the deserialized object carries - if they
//! can disagree, the cheap levels are worthless.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use energyml_core::identity;
use energyml_core::json;
use energyml_core::registry::Registry;
use energyml_core::validate;
use energyml_storage::epc::SaveOptions;
use energyml_storage::{EnergymlStorage, EpcStore, LocalPackage, graph};

/// Every schema version the workspace generates.
///
/// One line rather than a hand-kept subset: a list written out here goes stale
/// the day a release is added, and nothing would notice - the suite would keep
/// passing while quietly never covering the new version. `energyml-schemas`
/// holds the one list, and its own test fails when it disagrees with
/// `codegen.toml`.
fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

/// The public reference packages, found by `fixtures.rs`: the
/// fesapi pair and the four resqpy ones, each beside its `.h5` companion -
/// which is what lets an array pointer naming `testingPackageCpp22.h5` be
/// resolved relative to its package.
fn corpus() -> Vec<PathBuf> {
    crate::fixtures::public_corpus()
}

/// Prints what was skipped, so a silent pass is never mistaken for coverage.
fn skip_if_empty(files: &[PathBuf], what: &str) -> bool {
    if files.is_empty() {
        eprintln!("skipped `{what}`: no reference package (scripts/fetch-fixtures.py)");
        return true;
    }
    false
}

#[test]
fn every_package_opens_and_lists_its_objects() {
    let files = corpus();
    if skip_if_empty(&files, "every_package_opens_and_lists_its_objects") {
        return;
    }
    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        let resources = store.resources();
        assert!(
            !resources.is_empty(),
            "{} holds no EnergyML object at all - the inventory is wrong",
            path.display()
        );
        for summary in &resources {
            assert!(
                identity::parse_qualified_type(&summary.qualified_type).is_ok(),
                "{}: `{}` is not a qualified type",
                path.display(),
                summary.qualified_type
            );
            assert!(
                summary.uri.starts_with("eml:///"),
                "{}: `{}`",
                path.display(),
                summary.uri
            );
        }
        eprintln!(
            "{}: {} objects, {} diagnostics",
            path.file_name().unwrap().to_string_lossy(),
            resources.len(),
            store.diagnostics().len()
        );
        for diagnostic in store.diagnostics() {
            eprintln!("    {diagnostic}");
            // A part whose root element names nothing this build knows is read
            // as the index declares and comes back **blank**. Saying so is
            // right; saying so about a healthy package would be noise a reader
            // learns to ignore, which is how a real warning gets missed.
            assert!(
                !diagnostic.message.contains("no type this build knows"),
                "{}: `{}` - nothing in this corpus is an alien part",
                path.display(),
                diagnostic.message
            );
        }
    }
}

#[test]
fn the_three_levels_agree() {
    let files = corpus();
    if skip_if_empty(&files, "the_three_levels_agree") {
        return;
    }
    for path in &files {
        let mut store = EpcStore::open(path, registry()).expect("the package opens");
        // The corpus carries genuinely defective packages -
        // `80wells_surf_modified_val_color.epc` has a `FluidCharacterization`
        // missing a required element. This test is about the three levels
        // agreeing on identity, not about strictness (which has its own
        // tests), so it reads the way a repair tool would.
        store.set_lenient(true);
        for summary in store.resources() {
            // Level 2 must not invent an identity of its own.
            let header = store.header(&summary.uri).expect("the citation is readable");
            assert_eq!(header.summary.uuid, summary.uuid);
            assert!(!header.display_name().is_empty());

            // Level 3, where the registry knows the type. An unregistered
            // version is a gap in the workspace, not a failure of the reader.
            let object = match store.get_object(&summary.uri) {
                Ok(object) => object,
                Err(energyml_storage::Error::UnknownType(_)) => continue,
                Err(e) => panic!("{}: {} - {e}", path.display(), summary.uri),
            };
            let key = identity::key_of_reflect(object.as_reflect())
                .unwrap_or_else(|| panic!("{}: {} carries no identity", path.display(), summary.uri));
            assert!(
                key.uuid.eq_ignore_ascii_case(&summary.uuid),
                "{}: the index says {} and the object says {}",
                path.display(),
                summary.uuid,
                key.uuid
            );
            // **The index guesses, the read proves, and the disagreement is said.**
            //
            // This was a plain equality, and it held for a reason that stopped
            // being true once PRODML 2.3 was generated: until then, 2.3 not being generated, a
            // part stamped `2.3` fell back on 2.2 on **both** sides and the
            // two agreed by accident. Generating 2.3 separates them, and the
            // separation is right - the `TimeSeriesData` of
            // `80wells_surf_modified_val_color.epc` declares `2.3` on 2.2
            // content, and the package has no `[Content_Types].xml` to
            // contradict it. The index believes the stamp, without parsing; the read
            // **proves** the content by parsing it, and it is the one that is right.
            //
            // Requiring equality therefore forced a lie on one side or the
            // other: an index claiming to know what only a parser can
            // settle, or a reader believing the stamp against the bytes.
            // What must hold is that the disagreement is **named**, and
            // `get_object` already records it.
            if !key.qualified_type().eq_ignore_ascii_case(&summary.qualified_type) {
                let read_as = key.qualified_type();
                let said = store
                    .diagnostics()
                    .into_iter()
                    .any(|d| d.message.contains(&summary.qualified_type) && d.message.contains(&read_as));
                assert!(
                    said,
                    "{}: the index says `{}` and the object says `{read_as}` for {}, and the store said nothing",
                    path.display(),
                    summary.qualified_type,
                    summary.uuid
                );
            }
        }
    }
}

#[test]
fn arrays_are_located_in_both_spellings() {
    let files = corpus();
    if skip_if_empty(&files, "arrays_are_located_in_both_spellings") {
        return;
    }
    let mut old_form = 0usize;
    let mut new_form = 0usize;
    let mut with_mime = 0usize;
    let mut unresolved = Vec::new();

    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            for location in store.arrays_of(object.as_reflect()) {
                assert!(
                    !location.path_in_file.is_empty(),
                    "{}: {} points at a dataset with no path",
                    path.display(),
                    summary.uri
                );
                if location.proxy_uuid.is_some() {
                    old_form += 1;
                    // The older form declares its format on the proxy object,
                    // not in the part. Losing it would silently call every
                    // external file HDF5, and a producer writing something else
                    // would have no way to say so.
                    if let Some(mime) = &location.mime {
                        with_mime += 1;
                        assert!(
                            !mime.is_empty(),
                            "{}: an empty mime is not a mime",
                            path.display()
                        );
                    }
                } else {
                    new_form += 1;
                }
                if !location.is_resolved() {
                    unresolved.push(format!("{} {}", summary.uri, location.field_path));
                }
            }
        }
    }

    eprintln!(
        "array pointers: {old_form} through a proxy ({with_mime} with the proxy's MIME type), \
         {new_form} carrying their own URI"
    );
    assert!(
        with_mime > 0,
        "not one proxy declared its format - the MIME type on `EpcExternalPartReference` is being dropped"
    );
    assert!(
        old_form + new_form > 0,
        "the corpus holds no external array at all - suspicious"
    );
    assert!(
        unresolved.is_empty(),
        "{} array pointers could not be resolved to a file: {:?}",
        unresolved.len(),
        &unresolved[..unresolved.len().min(5)]
    );
}

#[test]
fn json_round_trips_on_every_object_of_the_corpus() {
    // The strongest test the JSON encoding can have: 425 objects written by two
    // independent implementations, each written out, read back and written
    // again. Anything the encoding cannot express - a substituted subtype, a
    // foreign XML subtree, a jagged array of doubles - comes out as a
    // difference between the two documents.
    let files = corpus();
    if skip_if_empty(&files, "json_round_trips_on_every_object_of_the_corpus") {
        return;
    }
    let registry = registry();
    let mut checked = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry.clone()).expect("the package opens");
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };

            // **The expected type is the object's, not the URI's.**
            //
            // The assertion compared against `summary.qualified_type`, that is,
            // what the index **guessed** without parsing - and it passed as long as
            // the two could not diverge. They diverge since
            // PRODML 2.3 exists: `80wells_surf_modified_val_color.epc` carries
            // a `TimeSeriesData` stamped `schemaVersion="2.3"` whose
            // content is 2.2 (its `Key`s are `xs:string`, which only
            // 2.2 allows - 2.3 wants a required `keyword` there), and the package
            // has no `[Content_Types].xml` to contradict the stamp.
            //
            // What this test means is *the document names its own type,
            // and names it first* - a property of the JSON encoding. Making it
            // depend on the index made it, by accident, a test of
            // version identification, which has its own.
            let expected = identity::key_of_reflect(object.as_reflect())
                .map(|key| key.qualified_type())
                .unwrap_or_else(|| summary.qualified_type.clone());
            let written = json::write_string(object.as_reflect(), false).expect("json out");
            assert!(
                written.starts_with(&format!("{{\"$type\":\"{expected}\"")),
                "{}: the document must name its own type first: {}",
                summary.uri,
                &written[..written.len().min(120)]
            );

            let back = json::read_string(&written, &registry)
                .unwrap_or_else(|e| panic!("{}: reading back its own JSON: {e}", summary.uri));
            let again = json::write_string(back.root.as_reflect(), false).expect("json out again");
            assert_eq!(
                again, written,
                "{}: the round trip changed the document",
                summary.uri
            );
            checked += 1;
        }
    }
    eprintln!("{checked} objects round-tripped through JSON");
    assert!(
        checked > 100,
        "only {checked} objects - the corpus is not what it should be"
    );
}

#[test]
fn every_validation_path_of_the_corpus_points_at_something() {
    // A validation message is only useful if the interface can put it on the
    // widget showing the offending value, and that means following the path
    // back down the object. Checked here on everything two independent
    // implementations wrote, rather than on a fixture built to pass.
    let files = corpus();
    if skip_if_empty(&files, "every_validation_path_of_the_corpus_points_at_something") {
        return;
    }
    let registry = registry();
    let mut issues = 0usize;
    let mut indexed = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry.clone()).expect("the package opens");
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            let report = validate::validate_with(object.as_validate(), &[]);
            for issue in &report.issues {
                issues += 1;
                if issue.path.contains('[') {
                    indexed += 1;
                }
                energyml_core::path::get(object.as_reflect(), &issue.path).unwrap_or_else(|e| {
                    panic!(
                        "{}: `{}` says `{}` but the path does not resolve: {e}",
                        summary.uri, issue.path, issue.message
                    )
                });
            }
        }
    }

    eprintln!("{issues} validation issue(s) on the corpus, {indexed} inside a repeated field");
    assert!(
        issues > 0,
        "no issue at all on the corpus - the check proves nothing"
    );
    assert!(
        indexed > 0,
        "no issue inside a repeated field - the case that was broken is not covered"
    );
}

#[test]
fn a_reference_can_carry_its_target() {
    // `_data`: the object a reference names, inlined, so a document stands on
    // its own without a package around it.
    let files = corpus();
    if skip_if_empty(&files, "a_reference_can_carry_its_target") {
        return;
    }
    let registry = registry();

    for path in &files {
        let store = EpcStore::open(path, registry.clone()).expect("the package opens");
        let summaries = store.resources();
        let objects: Vec<_> = summaries
            .iter()
            .filter_map(|s| store.get_object(&s.uri).ok())
            .collect();
        let source = json::ObjectsInMemory::new(objects.iter().map(|o| o.as_reflect()));

        // Any object holding at least one reference the package can resolve.
        let Some(object) = objects.iter().find(|object| {
            energyml_core::identity::collect_refs(object.as_reflect())
                .iter()
                .any(|edge| store.find_by_uuid(edge.target.uuid).is_some())
        }) else {
            continue;
        };

        let text = json::write_string_with(object.as_reflect(), &source, false).expect("json out");
        assert!(text.contains("\"_data\""), "the reference carried its target");

        let document = json::read_string(&text, &registry).expect("json in");
        assert!(
            !document.referenced.is_empty(),
            "reading gives the referenced objects back, beside the root"
        );
        return;
    }
}

#[test]
fn a_package_survives_being_rewritten() {
    // The acceptance test of §10 of the design: read an EPC, write it back,
    // read the result, compare. The comparison is each object's canonical JSON,
    // which is complete - a field lost anywhere in the chain shows up as a
    // difference.
    let files = corpus();
    if skip_if_empty(&files, "a_package_survives_being_rewritten") {
        return;
    }
    let registry = registry();
    let options = SaveOptions {
        creator: Some("energyml-rust tests".to_string()),
        // Fixed, so the same input gives the same bytes twice.
        created: Some("2026-07-31T12:00:00Z".to_string()),
        ..Default::default()
    };

    for path in &files {
        let source = EpcStore::open(path, registry.clone()).expect("the package opens");
        let before: Vec<(String, String)> = source
            .resources()
            .iter()
            .filter_map(|summary| source.get_object(&summary.uri).ok())
            .map(|object| {
                (
                    identity::uuid_of_reflect(object.as_reflect())
                        .unwrap()
                        .to_ascii_lowercase(),
                    json::write_string(object.as_reflect(), false).unwrap(),
                )
            })
            .collect();

        let mut buffer = Cursor::new(Vec::new());
        let written = source
            .write_to(&mut buffer, &options)
            .expect("the package writes");

        let package = EpcStore::from_reader(
            Box::new(Cursor::new(buffer.into_inner())),
            "rewritten.epc",
            registry.clone(),
        )
        .expect("what was written reads back");

        // A package we wrote ourselves must be conformant: no fallback, no
        // duplicate override, no missing core properties. Notes are another
        // matter - "prodml23 is not registered" is as true of what we wrote as
        // of what we read, and saying so is the honest answer.
        let departures: Vec<_> = package
            .diagnostics()
            .into_iter()
            .filter(|d| d.severity == energyml_storage::Severity::Warning)
            .collect();
        assert!(
            departures.is_empty(),
            "{}: the package we wrote departs from the specification: {departures:?}",
            path.display()
        );

        // Nothing is lost, not even what this build cannot read: an
        // unreadable part is copied verbatim rather than dropped.
        assert_eq!(
            package.resources().len(),
            source.resources().len(),
            "{}: the rewritten package holds the same objects, readable or not",
            path.display()
        );

        for (uuid, json_before) in &before {
            let object = package
                .get_object(uuid)
                .unwrap_or_else(|e| panic!("{}: {uuid} did not survive: {e}", path.display()));
            let json_after = json::write_string(object.as_reflect(), false).unwrap();
            assert_eq!(json_after, *json_before, "{}: {uuid} changed", path.display());
        }

        // Parts the specification does not describe belong to the producer, and
        // rewriting one away is a loss nothing would ever mention. fesapi's
        // `docProps/extendedCore.xml` is the real case, in both reference
        // packages - `docProps/` is package metadata rather than an attachment,
        // which is exactly how it fell through.
        if let Ok(original) = source.read_attachment("docProps/extendedCore.xml") {
            let kept = package
                .read_attachment("docProps/extendedCore.xml")
                .unwrap_or_else(|e| panic!("{}: extendedCore.xml was dropped: {e}", path.display()));
            assert_eq!(
                kept,
                original,
                "{}: extendedCore.xml came back changed",
                path.display()
            );
        }

        // Both directions, as §3.4.1 demands: every relationship the source
        // side declares has its mirror on the destination side.
        let mut pairs = 0usize;
        for summary in package.resources() {
            for relationship in package.relationships(&summary.locator).unwrap() {
                if relationship.short_kind() != "destinationObject" {
                    continue;
                }
                pairs += 1;
                let target = relationship.target.rsplit('/').next().unwrap();
                let back = package
                    .relationships(target)
                    .unwrap_or_else(|e| panic!("{}: no rels for {target}: {e}", path.display()));
                assert!(
                    back.iter().any(|r| r.short_kind() == "sourceObject"
                        && summary.locator.ends_with(r.target.rsplit('/').next().unwrap())),
                    "{}: {} → {target} has no matching sourceObject",
                    path.display(),
                    summary.locator
                );
            }
        }

        eprintln!(
            "{}: {} objects rewritten, {pairs} relationship pairs, {} diagnostic(s) while writing",
            path.file_name().unwrap().to_string_lossy(),
            before.len(),
            written.len()
        );
    }
}

#[test]
fn an_open_package_is_modifiable() {
    // §12: open, edit, save. The archive is untouched until the save, and the
    // edit survives it - through the type-agnostic path, since that is what an
    // editing interface built on reflection will use.
    let files = corpus();
    if skip_if_empty(&files, "an_open_package_is_modifiable") {
        return;
    }
    let registry = registry();
    let path = &files[0];

    let mut store = EpcStore::open(path, registry.clone()).expect("the package opens");
    assert!(!store.is_dirty(), "opening changes nothing");

    let target = store.resources()[0].clone();
    let before = store.header(&target.uri).unwrap().display_name().to_string();

    energyml_core::path::set(
        store.edit(&target.uri).expect("it can be edited"),
        "Citation.Title",
        energyml_core::value::OwnedValue::Str("Renamed by a test".to_string()),
    )
    .expect("the title is writable through reflection");

    assert!(store.is_dirty());
    assert_eq!(
        store.header(&target.uri).unwrap().display_name(),
        "Renamed by a test",
        "the layer answers, not the archive"
    );
    assert_ne!(before, "Renamed by a test", "the test would be vacuous otherwise");

    // Remove another object, and check both effects land in the written file.
    let removed = store.resources()[1].clone();
    store.remove(&removed.uri).expect("it can be removed");
    assert!(
        store.get_object(&removed.uri).is_err(),
        "gone from the layer's point of view"
    );

    let mut buffer = Cursor::new(Vec::new());
    store
        .write_to(&mut buffer, &SaveOptions::default())
        .expect("the package writes");
    let written = EpcStore::from_reader(
        Box::new(Cursor::new(buffer.into_inner())),
        "edited.epc",
        registry.clone(),
    )
    .expect("it reads back");

    assert_eq!(
        written.header(&target.uri).unwrap().display_name(),
        "Renamed by a test",
        "the edit reached the file"
    );
    assert!(
        written.find_by_uuid(&removed.uuid).is_none(),
        "the removal did too"
    );
    assert_eq!(written.resources().len(), store.resources().len());
}

#[test]
fn a_transaction_rolls_back_for_real() {
    // §13: a rollback that undoes nothing would be a lie. Nothing here touches
    // a disk, which is precisely why the layer can undo it.
    let files = corpus();
    if skip_if_empty(&files, "a_transaction_rolls_back_for_real") {
        return;
    }
    let mut store = EpcStore::open(&files[0], registry()).expect("the package opens");
    let target = store.resources()[0].clone();
    let original = store.header(&target.uri).unwrap().display_name().to_string();

    store.start_transaction().expect("it opens");
    assert!(
        store.start_transaction().is_err(),
        "ETP v1.2 does not nest transactions"
    );

    energyml_core::path::set(
        store.edit(&target.uri).unwrap(),
        "Citation.Title",
        energyml_core::value::OwnedValue::Str("Inside the transaction".to_string()),
    )
    .unwrap();
    assert_eq!(
        store.header(&target.uri).unwrap().display_name(),
        "Inside the transaction"
    );
    assert!(
        store.save(&SaveOptions::default()).is_err(),
        "saving mid-transaction is refused: the disk would hold a state the rollback undoes"
    );

    store.rollback_transaction().expect("it rolls back");
    assert!(!store.in_transaction());
    assert_eq!(
        store.header(&target.uri).unwrap().display_name(),
        original,
        "the rollback actually undid it"
    );
    assert!(!store.is_dirty(), "and left nothing behind");
}

#[test]
fn references_are_checked_against_what_the_package_holds() {
    let files = corpus();
    if skip_if_empty(&files, "references_are_checked_against_what_the_package_holds") {
        return;
    }
    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        let summaries = store.resources();
        let objects: Vec<_> = summaries
            .iter()
            .filter_map(|summary| store.get_object(&summary.uri).ok().map(|o| (summary, o)))
            .collect();
        let pairs = objects
            .iter()
            .map(|(summary, object)| (*summary, object.as_reflect()));
        let issues = graph::check_all_references(pairs, &|uuid| store.find_by_uuid(uuid));

        for issue in &issues {
            // A finding must name something a human can act on, or it is noise.
            assert!(
                !issue.path.is_empty(),
                "{}: an issue with no path",
                path.display()
            );
            assert!(
                !issue.target_uuid.is_empty(),
                "{}: an issue with no target",
                path.display()
            );
        }

        let name = path.file_name().unwrap().to_string_lossy();
        eprintln!("{name}: {} reference issue(s)", issues.len());
        for issue in issues.iter().take(3) {
            eprintln!("    {issue}");
        }

        // resqpy writes self-contained packages; the assertion is here so that
        // a regression in the *checker* shows up as a false positive rather
        // than hiding among the fesapi findings, which are real.
        if path.parent().is_some_and(|p| p.ends_with("resqpy")) {
            assert!(issues.is_empty(), "{name} should be self-contained: {issues:?}");
        }
    }
}

#[test]
fn a_proxy_named_by_a_file_name_still_resolves() {
    // fesapi 2.14 writes the HDF5 file name into the DOR's `UUID` element for
    // the two RESQML 2.0.1 objects it carries inside `testingPackageCpp22.epc`.
    // Nothing about that is legal, and a reader that requires a uuid there
    // loses both arrays. Pinned down because it is the kind of tolerance that
    // gets "cleaned up" by someone who has not seen the file.
    let files = corpus();
    if skip_if_empty(&files, "a_proxy_named_by_a_file_name_still_resolves") {
        return;
    }
    let Some(path) = files.iter().find(|p| p.ends_with("testingPackageCpp22.epc")) else {
        eprintln!("skipped: testingPackageCpp22.epc is not in the corpus");
        return;
    };

    let store = EpcStore::open(path, registry()).expect("the package opens");
    let summary = store
        .resources()
        .into_iter()
        .find(|s| s.uuid.starts_with("657d5e6b"))
        .expect("the wellbore marker frame is in this package");
    let object = store.get_object(&summary.uri).expect("it deserializes");

    let locations = store.arrays_of(object.as_reflect());
    assert!(!locations.is_empty(), "it points at an array");
    for location in &locations {
        assert_eq!(location.proxy_uuid.as_deref(), Some("testingPackageCpp22.h5"));
        assert_eq!(location.file, "testingPackageCpp22.h5", "resolved anyway");
    }
    assert!(
        store
            .diagnostics()
            .iter()
            .any(|d| d.message.contains("where a uuid belongs")),
        "the departure is recorded, not swallowed"
    );
}

#[test]
#[cfg(not(feature = "hdf5"))]
fn reading_an_array_says_which_capability_is_missing() {
    let files = corpus();
    if skip_if_empty(&files, "reading_an_array_says_which_capability_is_missing") {
        return;
    }
    // **Two absences, and they are not the same one.** A package whose data
    // file was never shipped beside it answers `FileMissing` - correctly, and
    // before the question this test is asking is ever reached. Taking the first
    // package in the corpus and demanding `FeatureNotEnabled` therefore turns
    // "somebody added an `.epc` without its `.h5`" into a failure of the array
    // layer, which is what happened the day the corpus grew one.
    //
    // So a missing file means *look further*, and the test still says something:
    // it fails if any package that **does** have its file answers anything other
    // than the missing capability.
    let mut stated = 0usize;
    let mut without_their_file = 0usize;
    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            let Some(location) = store.arrays_of(object.as_reflect()).into_iter().next() else {
                continue;
            };
            // Not "not found", not an empty array: a capability that is absent,
            // stated as such, so the caller can tell the two apart.
            match store.read_array(&location) {
                Err(energyml_storage::Error::ExternalData {
                    reason: energyml_storage::ExternalDataReason::FeatureNotEnabled { feature, .. },
                    ..
                }) => {
                    assert_eq!(feature, "hdf5");
                    stated += 1;
                }
                Err(energyml_storage::Error::ExternalData {
                    reason: energyml_storage::ExternalDataReason::FileMissing,
                    ..
                }) => without_their_file += 1,
                other => panic!("{}: unexpected {other:?}", path.display()),
            }
            break;
        }
    }

    if stated == 0 {
        eprintln!(
            "skipped: none of the {} package(s) of the corpus has its data file beside it",
            files.len()
        );
        return;
    }
    eprintln!("{stated} package(s) named the missing capability, {without_their_file} lack their data file");
}

#[test]
#[cfg(feature = "hdf5")]
fn every_array_of_the_corpus_reads() {
    // The point of the whole external-data layer: the numbers come out. Every
    // pointer in every package, followed to its dataset, with the shape and the
    // element type checked against what the file actually holds.
    let files = corpus();
    if skip_if_empty(&files, "every_array_of_the_corpus_reads") {
        return;
    }
    let mut read = 0usize;
    let mut bytes = 0usize;
    let mut absent = 0usize;
    let mut failures = Vec::new();
    let mut kinds: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();

    // A package whose `.h5` companion was never downloaded is not a failure of
    // the backend - it is a gap in the corpus. Counted and printed, so a pass
    // is never mistaken for coverage.
    let is_absent = |e: &energyml_storage::Error| {
        matches!(
            e,
            energyml_storage::Error::ExternalData {
                reason: energyml_storage::ExternalDataReason::FileMissing,
                ..
            }
        )
    };

    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            for location in store.arrays_of(object.as_reflect()) {
                match store.array_metadata(&location) {
                    Ok(metadata) => {
                        assert!(
                            !metadata.dimensions.is_empty(),
                            "{}: {} has no shape",
                            path.display(),
                            location.path_in_file
                        );
                        *kinds.entry(metadata.element_type.clone()).or_default() += 1;

                        let data = match store.read_array(&location) {
                            Ok(data) => data,
                            Err(e) if is_absent(&e) => {
                                absent += 1;
                                continue;
                            }
                            Err(e) => {
                                failures.push(format!("{} {}: {e}", summary.uuid, location.path_in_file));
                                continue;
                            }
                        };
                        // The values must account for exactly the elements the
                        // shape promises - a silent truncation would pass every
                        // other check in this file.
                        let elements: i64 = data.metadata.dimensions.iter().product();
                        assert_eq!(
                            data.values.len() as i64,
                            elements,
                            "{}: {} gave {} value(s) for a shape of {elements}",
                            path.display(),
                            location.path_in_file,
                            data.values.len()
                        );
                        bytes += data.as_bytes().len();
                        read += 1;
                    }
                    Err(e) if is_absent(&e) => absent += 1,
                    Err(e) => failures.push(format!("{} {}: {e}", summary.uuid, location.path_in_file)),
                }
            }
        }
    }

    eprintln!(
        "{read} arrays read, {} KiB, element types {kinds:?}",
        bytes / 1024
    );
    if absent > 0 {
        eprintln!("{absent} skipped: their package's `.h5` companion is not in testdata/");
    }
    assert!(read > 100, "only {read} arrays read - the corpus has hundreds");
    assert!(
        failures.is_empty(),
        "{} array(s) could not be read: {:#?}",
        failures.len(),
        &failures[..failures.len().min(10)]
    );
}

#[test]
#[cfg(feature = "hdf5")]
fn the_values_are_the_values() {
    // Byte counts prove nothing about content. These two datasets were read
    // with `h5dump` and the numbers written down here, so a decoding mistake -
    // wrong element type, wrong endianness, an off-by-one in a hyperslab -
    // shows up as a wrong number rather than a right length.
    let Some(path) = crate::fixtures::find("testingPackageCpp22.epc") else {
        eprintln!("skipped `the_values_are_the_values`: the reference package is not in the fixtures");
        return;
    };
    let store = EpcStore::open(&path, registry()).expect("the package opens");

    // h5dump: DATASPACE SIMPLE { ( 2 ) }, DATA { (0): 350, 550 }
    let marker_frame = store
        .get_object("657d5e6b-1752-425d-b3e7-237037fa11eb")
        .expect("the wellbore marker frame");
    let location = store
        .arrays_of(marker_frame.as_reflect())
        .into_iter()
        .find(|l| l.path_in_file.ends_with("mdValues"))
        .expect("its measured depths");
    let data = store.read_array(&location).expect("they read");
    assert_eq!(data.metadata.dimensions, vec![2]);
    assert_eq!(data.metadata.element_type, "double");
    // Borrowed, not converted: this is the path a 3D application takes.
    assert_eq!(data.f64().expect("the file holds doubles"), &[350.0, 550.0]);

    // h5dump: DATASPACE SIMPLE { ( 2, 4 ) }, DATA { 300, 300, 350, 350, 300, 300, 350, 350 }
    let grid = store
        .get_object("030a82f6-10a7-4ecf-af03-54749e098624")
        .expect("the 2d grid");
    let location = store
        .arrays_of(grid.as_reflect())
        .into_iter()
        .find(|l| l.path_in_file.ends_with("points_patch0"))
        .expect("its points");
    let data = store.read_array(&location).expect("they read");
    assert_eq!(
        data.metadata.dimensions,
        vec![2, 4],
        "the shape is the file's, not flattened"
    );
    assert_eq!(
        data.f64().expect("doubles"),
        &[300.0, 300.0, 350.0, 350.0, 300.0, 300.0, 350.0, 350.0]
    );
}

#[test]
#[cfg(feature = "hdf5")]
fn a_path_need_not_contain_a_uuid() {
    // fesapi writes `/resqml22/<uuid>/Points` and it is only a convention: the
    // schema says `PathInExternalFile` is a string, and nothing more. This
    // builds a file whose dataset lives at a path with no uuid anywhere and
    // reads it through the backend, so that nobody later "improves" the reader
    // by parsing the path.
    use energyml_storage::ArrayBackend;

    let dir = std::env::temp_dir().join("energyml-opaque-path");
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join("plain.h5");
    let _ = std::fs::remove_file(&file);
    {
        let handle = hdf5::File::create(&file).expect("a file can be written");
        let group = handle.create_group("whatever").expect("a group");
        group
            .new_dataset::<f64>()
            .shape([3])
            .create("my points")
            .expect("a dataset")
            .write(&[1.5f64, 2.5, 3.5])
            .expect("values");
    }

    let backend = energyml_storage::hdf5::Hdf5Backend;
    let path = "/whatever/my points";
    let metadata = backend.metadata(&file, path).expect("no uuid needed to find it");
    assert_eq!(metadata.dimensions, vec![3]);
    let data = backend.read(&file, path, &[], &[]).expect("nor to read it");
    assert_eq!(data.f64().expect("doubles"), &[1.5, 2.5, 3.5]);

    let _ = std::fs::remove_file(&file);
}

#[test]
fn a_backend_is_chosen_by_mime_type_not_by_habit() {
    // `ExternalDataArrayPart` carries a MIME type precisely so the data need
    // not be HDF5. This registers a backend for another format and checks the
    // dispatch picks it - the mechanism, without waiting for a real CSV reader.
    use energyml_storage::{ArrayBackend, Backends};

    struct Csv;
    impl ArrayBackend for Csv {
        fn name(&self) -> &'static str {
            "csv"
        }
        fn handles(&self, mime: Option<&str>, file: &Path) -> bool {
            mime.is_some_and(|m| m.contains("csv"))
                || file.extension().is_some_and(|e| e.eq_ignore_ascii_case("csv"))
        }
        fn metadata(&self, _f: &Path, _p: &str) -> energyml_storage::Result<energyml_storage::ArrayMetadata> {
            unreachable!("the dispatch is what is under test")
        }
        fn read(
            &self,
            _f: &Path,
            _p: &str,
            _s: &[i64],
            _c: &[i64],
        ) -> energyml_storage::Result<energyml_storage::ArrayData> {
            unreachable!()
        }
        fn read_into(
            &self,
            _f: &Path,
            _p: &str,
            _s: &[i64],
            _c: &[i64],
            _o: &mut energyml_storage::ArrayBuffer<'_>,
        ) -> energyml_storage::Result<energyml_storage::ArrayMetadata> {
            unreachable!()
        }
    }

    let mut backends = Backends::compiled_in();
    backends.push(Box::new(Csv));

    let csv = energyml_storage::ArrayLocation {
        file: "table.csv".to_string(),
        path_in_file: "column2".to_string(),
        mime: Some("text/csv".to_string()),
        start: Vec::new(),
        count: Vec::new(),
        proxy_uuid: None,
        field_path: "Values".to_string(),
    };
    assert_eq!(
        backends.find(&csv, Path::new("table.csv")).unwrap().name(),
        "csv",
        "the MIME type decides"
    );

    let h5 = energyml_storage::ArrayLocation {
        file: "model.h5".to_string(),
        mime: Some("application/x-hdf5".to_string()),
        ..csv.clone()
    };
    let chosen = backends.find(&h5, Path::new("model.h5"));
    #[cfg(feature = "hdf5")]
    assert_eq!(chosen.unwrap().name(), "hdf5", "and it does not pick the CSV one");
    #[cfg(not(feature = "hdf5"))]
    assert!(chosen.is_err(), "no HDF5 backend is compiled in");
}

#[test]
#[cfg(feature = "hdf5")]
fn an_hdf5_file_is_checked_against_the_package() {
    // EPC §3.2.3: the file carries its proxy's uuid at its root, so a package
    // pointing at somebody else's file is detectable. Nothing else catches it.
    let files = corpus();
    if skip_if_empty(&files, "an_hdf5_file_is_checked_against_the_package") {
        return;
    }
    let mut checked = 0usize;
    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            for location in store.arrays_of(object.as_reflect()) {
                if location.proxy_uuid.is_none() {
                    continue;
                }
                match store.verify_external_file(&location) {
                    Ok(()) => checked += 1,
                    // The companion file is simply not here; nothing to check.
                    Err(energyml_storage::Error::ExternalData {
                        reason: energyml_storage::ExternalDataReason::FileMissing,
                        ..
                    }) => {}
                    Err(e) => panic!("{}: {} - {e}", path.display(), summary.uuid),
                }
            }
        }
    }
    eprintln!("{checked} array pointers cross-validated against the file's own uuid");
}

#[test]
fn the_asynchronous_face_says_what_the_synchronous_one_says() {
    let files = corpus();
    if skip_if_empty(&files, "the_asynchronous_face_says_what_the_synchronous_one_says") {
        return;
    }
    // The acceptance criterion of option D: the same code must be able to run
    // over `dyn EnergymlStorage`. If the wrappers ever drift, this fails.
    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        let synchronous = store.resources();
        let polymorphic: &dyn EnergymlStorage = &store;
        let asynchronous =
            futures_executor::block_on(polymorphic.resources()).expect("resources are listable");
        assert_eq!(synchronous, asynchronous, "{}", path.display());

        let dataspaces = futures_executor::block_on(polymorphic.dataspaces()).unwrap();
        assert_eq!(dataspaces.len(), 1, "an EPC is exactly one dataspace");
        assert_eq!(polymorphic.store_kind(), "EPC");
    }
}

#[test]
fn attachments_exclude_the_objects_and_the_plumbing() {
    let files = corpus();
    if skip_if_empty(&files, "attachments_exclude_the_objects_and_the_plumbing") {
        return;
    }
    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        let objects: Vec<String> = store.resources().into_iter().map(|s| s.locator).collect();
        for attachment in store.attachments() {
            assert!(!objects.contains(&attachment), "{}: {attachment}", path.display());
            assert!(!attachment.ends_with(".rels"), "{}: {attachment}", path.display());
            assert!(store.read_attachment(&attachment).is_ok());
        }
    }
}

/// A rewrite keeps the relationships of an object the model could not type.
///
/// `GraphicalInformationSet` carries every colour assignment in an EnergyML
/// package, and the generator cannot lower its `AbstractGraphicalInformation`
/// list - so it comes back as `AnyXml`, source text kept verbatim. The text
/// survived a rewrite; the **references inside it** did not, because
/// `collect_refs` walked reflection and reflection saw an opaque blob. And
/// `collect_refs` is what regenerates the `.rels`.
///
/// So `testingPackageCpp22.epc` went in with 18 relationships on that object
/// and came out with **0**: no data lost, the whole colour graph gone from the
/// index. Found on the first run of the `.rels`-against-the-objects check, on
/// the reference corpus, which is the reason that check exists.
#[test]
fn a_rewrite_keeps_the_references_hidden_in_unmodelled_xml() {
    let files = corpus();
    if skip_if_empty(&files, "a_rewrite_keeps_the_references_hidden_in_unmodelled_xml") {
        return;
    }
    let mut checked = 0usize;
    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        let Some(summary) = store
            .resources()
            .into_iter()
            .find(|s| s.qualified_type.ends_with("GraphicalInformationSet"))
        else {
            continue;
        };
        let before = store.relationships(&summary.locator).unwrap_or_default().len();
        if before == 0 {
            continue;
        }

        let out =
            std::env::temp_dir().join(format!("energyml-anyxml-{}-{}.epc", std::process::id(), checked));
        store
            .save_as(&out, &energyml_storage::SaveOptions::default())
            .expect("it saves");
        let rewritten = EpcStore::open(&out, registry()).expect("the rewrite opens");
        let after = rewritten
            .find_by_uuid(&summary.uuid)
            .map(|s| rewritten.relationships(&s.locator).unwrap_or_default().len())
            .unwrap_or(0);
        // Not equality: the writer regenerates the index from the references
        // themselves, so it can come out *more* complete than the producer left
        // it - `testingPackageCpp.epc` gains one. What must never happen is
        // losing them, which is what used to happen, all of them at once.
        assert!(
            after >= before,
            "{}: {} went in with {before} relationship(s) and came out with {after}",
            path.display(),
            summary.uuid
        );
        eprintln!(
            "{}: {before} relationship(s) in, {after} out",
            path.file_name().unwrap().to_string_lossy()
        );
        let _ = std::fs::remove_file(&out);
        checked += 1;
    }
    assert!(
        checked > 0,
        "the corpus holds a GraphicalInformationSet with relationships; none was checked"
    );
    eprintln!("{checked} package(s) kept their unmodelled references across a rewrite");
}

/// The graph carries what a viewer needs, and says what the package lacks.
///
/// The requirement is for something a web or desktop interface can draw
/// *"with some info (title, uuid/uri etc)"*. So the test is about the labels as
/// much as the edges - and about the nodes that stand for objects the package
/// references without holding, because a graph that dropped those would draw a
/// self-contained package where there is none.
#[test]
fn the_graph_labels_its_nodes_and_admits_what_is_missing() {
    let files = corpus();
    if skip_if_empty(&files, "the_graph_labels_its_nodes_and_admits_what_is_missing") {
        return;
    }
    for path in &files {
        let mut store = EpcStore::open(path, registry()).expect("the package opens");
        store.set_lenient(true);
        let summaries = store.resources();
        let objects: Vec<_> = summaries
            .iter()
            .filter_map(|summary| store.get_object(&summary.uri).ok().map(|o| (summary, o)))
            .collect();
        let graph = energyml_storage::graph::build(objects.iter().map(|(s, o)| (*s, o.as_reflect())));

        assert!(!graph.nodes.is_empty(), "{}: an empty graph", path.display());
        assert!(!graph.edges.is_empty(), "{}: no edge at all", path.display());

        // Every edge lands on a node - the property a viewer relies on and the
        // one that breaks it silently when it does not hold.
        let uris: std::collections::HashSet<&str> = graph.nodes.iter().map(|n| n.uri.as_str()).collect();
        for edge in &graph.edges {
            assert!(
                uris.contains(edge.from.as_str()),
                "{}: {} is not a node",
                path.display(),
                edge.from
            );
            assert!(
                uris.contains(edge.to.as_str()),
                "{}: {} is not a node",
                path.display(),
                edge.to
            );
        }

        // Labels: a node standing for an object the package holds knows its
        // concept, and nearly all of them have a title to show.
        let held: Vec<_> = graph.nodes.iter().filter(|n| n.present).collect();
        assert_eq!(held.len(), objects.len());
        assert!(held.iter().all(|n| !n.type_key.is_empty()));
        let titled = held.iter().filter(|n| n.title.is_some()).count();
        assert!(
            titled * 2 > held.len(),
            "{}: only {titled} of {} nodes carry a title",
            path.display(),
            held.len()
        );

        // An absent node whose type the standard publishes in a catalogue says
        // so, and the two questions are answered by the same predicate the
        // reference check uses. Without it a package's only absent nodes are
        // its well-known `PropertyKind`s and every one of them counts as a
        // reference into nothing - measured on a BRGM package: four of four.
        for node in graph.nodes.iter().filter(|n| !n.present) {
            let kind = node.type_key.eq_ignore_ascii_case("PropertyKind");
            assert_eq!(
                node.externally_defined,
                kind,
                "{}: {} ({}) is marked externally defined: {}",
                path.display(),
                node.uuid,
                node.qualified_type,
                node.externally_defined
            );
        }

        // Both renderings parse as themselves.
        let json = graph.to_json();
        assert!(json.starts_with('{') && json.contains("\"concept\""));
        assert_eq!(json.matches("\"externallyDefined\":").count(), graph.nodes.len());
        assert_eq!(json.matches("\"uri\":").count(), graph.nodes.len());
        let dot = graph.to_dot();
        assert!(dot.starts_with("digraph energyml {") && dot.trim_end().ends_with('}'));

        eprintln!(
            "{}: {} node(s) ({} outside the package), {} edge(s)",
            path.file_name().unwrap().to_string_lossy(),
            graph.nodes.len(),
            graph.nodes.iter().filter(|n| !n.present).count(),
            graph.edges.len()
        );
    }
}

#[test]
fn an_object_created_in_this_session_is_addressable_by_every_accessor() {
    // # The inconsistency this pins, and it was load-bearing
    //
    // `resources()` walked the overlay, so a created object appeared in a
    // listing. `resource()` and `find_by_uuid()` went through `by_uuid`, which
    // is built from the archive and never touched again - so the same object was
    // **listed and not addressable**.
    //
    // Neither of those is an obscure accessor. Spelling a reference starts by
    // asking for the holder's resource, so no reference could be written *from*
    // a created object at all; and a reference resolves through `find_by_uuid`,
    // so every reference *to* one read as dangling. A workshop creating eighteen
    // objects therefore linked none of them, and said only that some links could
    // not be written.
    //
    // Asserted across all four accessors together, because the bug was not that
    // any one of them was wrong - it was that they disagreed.
    let files = corpus();
    if skip_if_empty(&files, "an_object_created_in_this_session_is_addressable") {
        return;
    }
    let registry = registry();
    let mut store = EpcStore::open(files.first().expect("at least one package"), registry.clone())
        .expect("the package opens");

    // Any concrete top-level type this build can make, taken from the registry
    // rather than named - the point is the store, not the type.
    let Some(made) = store
        .resources()
        .first()
        .and_then(|summary| store.get_object(&summary.uri).ok())
    else {
        eprintln!("skipped: the first package holds nothing readable");
        return;
    };
    let mut fresh = made;
    let uuid = "5c1d7e90-2a44-4b31-8f60-1e2d3c4b5a69";
    energyml_core::path::set(
        fresh.as_reflect_mut(),
        "uuid",
        energyml_core::value::OwnedValue::Str(uuid.to_string()),
    )
    .expect("a uuid can be set");

    let uri = store.put(fresh).expect("it becomes a part");

    assert!(
        store.resources().iter().any(|s| s.uri == uri),
        "it is not in the listing"
    );
    assert!(store.get_object(&uri).is_ok(), "it does not read back");
    assert!(
        store.resource(&uri).is_ok(),
        "it is listed but has no resource - the disagreement this test exists for"
    );
    assert_eq!(
        store.find_by_uuid(uuid).map(|s| s.uri.clone()),
        Some(uri.clone()),
        "a reference resolving through the uuid would call it dangling"
    );
    // Case, because a uuid in a document is written either way and a reference
    // must resolve regardless.
    assert!(store.find_by_uuid(&uuid.to_ascii_uppercase()).is_some());

    // And removing it takes it back out of every one of them, rather than
    // leaving a resource for an object nothing else admits to holding.
    store.remove(&uri).expect("it is removed");
    assert!(store.resource(&uri).is_err(), "removed and still addressable");
    assert!(store.find_by_uuid(uuid).is_none(), "removed and still resolvable");
    assert!(!store.resources().iter().any(|s| s.uri == uri));
}

/// **An `externalResource` on an ordinary object survives a rewrite.**
///
/// EPC §3.2.3 puts the external file name in the proxy's `.rels` and nowhere
/// else, and the writer implemented exactly that. Real packages do more:
/// `testingPackageCpp22.epc` puts an `externalResource` straight on **every**
/// object that holds an array - 51 of them - and this reader has always
/// honoured it, as route 3 of `external_file_for`.
///
/// The writer had no counterpart, so a rewrite dropped all 51 of them. Nothing
/// failed and nothing was reported, because the reader finds the file by other
/// means: the package still opened here. Another tool, following the
/// relationships as the package presents them, would have found nothing - and
/// the earlier note recorded this as *"2 pointers out of 211"*, which is what
/// the search recovers, not what was lost.
///
/// Counted on the archive rather than through any API of this crate: what is
/// under test is the bytes a second tool would read.
#[test]
fn an_object_keeps_the_external_file_it_named() {
    let files = corpus();
    if skip_if_empty(&files, "an_object_keeps_the_external_file_it_named") {
        return;
    }
    let registry = registry();

    /// How many `.rels` of an archive carry an `externalResource`.
    fn rels_with_external(bytes: &[u8]) -> usize {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("a readable archive");
        let names: Vec<String> = (0..archive.len())
            .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
            .filter(|name| name.ends_with(".rels"))
            .collect();
        names
            .iter()
            .filter(|name| {
                use std::io::Read;
                let mut body = String::new();
                archive
                    .by_name(name)
                    .ok()
                    .map(|mut f| f.read_to_string(&mut body))
                    .is_some()
                    && body.contains("externalResource")
            })
            .count()
    }

    let mut checked = 0usize;
    for path in &files {
        let original = std::fs::read(path).expect("the package is readable");
        let before = rels_with_external(&original);
        if before == 0 {
            continue;
        }

        let store = EpcStore::open(path, registry.clone()).expect("the package opens");
        let mut out = Cursor::new(Vec::new());
        store
            .write_to(&mut out, &SaveOptions::default())
            .expect("the package writes");
        let after = rels_with_external(out.get_ref());

        assert_eq!(
            after,
            before,
            "{}: {before} `.rels` named an external file and {after} do now",
            path.display()
        );
        checked += 1;
    }

    // The corollary that bites: this passed over an empty set until a package
    // in the corpus was found to use the relationship at all.
    assert!(
        checked > 0,
        "no package in the corpus carries an `externalResource`"
    );
    eprintln!("{checked} package(s) kept every `externalResource` they carried");
}
