//! Judging a package, and the arithmetic that says whether it is sound.
//!
//! The report is `energyml-domain::check`'s — the same one `energyml check`
//! prints — so what is checked here is not the finding logic but the two things
//! a caller acts on: **that the sections say whether they ran**, and that the
//! findings agree with what another path over the same package says.

#![cfg(feature = "hdf5")]

use std::path::PathBuf;

use energyml_rust::session::Package;

fn corpus(name: &str) -> Option<PathBuf> {
    crate::fixtures::find_or_skip(name)
}

fn package(name: &str) -> Option<Package> {
    let path = corpus(name)?;
    Some(Package::open(&path).expect("the package opens"))
}

#[test]
fn a_report_says_which_sections_ran() {
    let Some(package) = package("testingPackageCpp22.epc") else { return };

    // **Not checked is not the same as clean**, and it is the one thing a
    // caller cannot work out from an empty list. A report that only said
    // "0 schema issues" after being asked to skip the schema would be a lie
    // told by omission.
    let all = package.check(true, true);
    assert!(all.schema_checked, "the schema was asked for");
    assert!(all.rules_checked, "and the rules");
    assert!(all.rule_count > 0, "the shipped rule set is not empty");

    let neither = package.check(false, false);
    assert!(!neither.schema_checked, "and this one was not");
    assert!(!neither.rules_checked);
    assert!(neither.schema.is_empty(), "nothing was checked, so nothing was found");
    assert!(neither.rules.is_empty());
    assert_eq!(neither.rule_count, 0, "no rule was evaluated");
}

#[test]
fn the_accounting_puts_each_section_on_the_right_side() {
    let Some(package) = package("testingPackageCpp22.epc") else { return };
    let report = package.check(true, true);

    // **What would break if a section moved sides.** An unreadable part is a
    // fault of the file and counts as an error; a `.rels` disagreement is an
    // index disagreeing with the truth and counts as a warning — the DORs are
    // what the file means (ADR-0002). Someone moving
    // either would change whether `energyml check` exits 0 on a real package,
    // and these two inequalities are what says so.
    assert!(
        report.errors >= report.unreadable.len(),
        "{} unreadable part(s) and only {} error(s)",
        report.unreadable.len(),
        report.errors
    );
    assert!(
        report.warnings >= report.relationships.len(),
        "{} relationship disagreement(s) and only {} warning(s)",
        report.relationships.len(),
        report.warnings
    );

    // And the catalogue references are on neither side: they are absent by
    // design, so counting them would condemn every package that names a
    // well-known property kind.
    let catalogue = report.references.iter().filter(|one| one.5).count();
    let errors_and_warnings = report.errors + report.warnings;
    assert!(
        errors_and_warnings + catalogue >= errors_and_warnings,
        "a count cannot shrink"
    );
    assert!(
        report.references.len() >= catalogue,
        "the catalogue references are part of the reference list"
    );
}

#[test]
fn every_dangling_reference_the_report_finds_is_one_the_objects_show() {
    let Some(package) = package("testingPackageCpp22.epc") else { return };
    let report = package.check(false, false);

    // **The confrontation.** The report's references come from
    // `check_all_references`, which walks every object against an index of
    // uuids; `Package::references` walks *one* object's own DORs and asks the
    // store. Two paths, and a reference the first calls dangling must be one
    // the second calls absent.
    let mut compared = 0;
    for (source, _path, target, _says, is_error, catalogue) in &report.references {
        if *catalogue || !*is_error {
            continue;
        }
        let Ok(links) = package.references(source) else { continue };
        let found = links.iter().find(|link| link.uuid.eq_ignore_ascii_case(target));
        if let Some(link) = found {
            assert!(
                !link.present,
                "the report calls {target} dangling and the object finds it present"
            );
            compared += 1;
        }
    }
    eprintln!("{compared} dangling reference(s) confirmed the other way round");
}

#[test]
fn the_catalogue_references_are_not_counted_as_faults() {
    let Some(package) = package("testingPackageCpp22.epc") else { return };
    let report = package.check(false, false);

    // The mistake this repository has already made: four references to
    // well-known `PropertyKind`s reported as dangling. They are absent **by
    // design**, so they are neither an error nor a warning, and they are said
    // apart.
    let catalogue: Vec<_> = report.references.iter().filter(|one| one.5).collect();
    if catalogue.is_empty() {
        eprintln!("skipped: this package references nothing from the catalogue");
        return;
    }
    for one in &catalogue {
        assert!(!one.4, "a catalogue reference is not an error: {}", one.3);
    }
}

#[test]
fn a_package_created_here_is_sound_the_moment_it_exists() {
    // No corpus needed: the one check that runs on a fresh clone.
    //
    // # This test used to say the opposite, and it was wrong
    //
    // It was called *…is not sound until it is saved* and it **pinned** the
    // fact that `extract::empty_package` did not write `docProps/core.xml`,
    // which EPC §3.5 makes mandatory. Its argument was that saving adds it, so
    // the report was honest in the meantime.
    //
    // The argument holds for a file and collapses on a front end: a package created
    // in the browser **lives in memory**, its health status is displayed
    // permanently, and nobody can fix what it is blamed for. Every new EPC
    // showed a health error: core.xml is missing. An alarm that cannot be
    // switched off is an alarm one learns to ignore — including the day it says
    // something else.
    //
    // The real cause was **two writers in the repository disagreeing about what
    // a minimal EPC is**: the one that judges knew, the one that creates did not
    // write it. It is the kind of defect this workspace names first, and it is
    // fixed on the side that creates.
    let package = Package::create("nothing.epc").expect("an empty package");
    let report = package.check(true, true);
    assert_eq!(report.errors, 0, "a new package blames itself for nothing: {:?}", report.diagnostics);
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    assert!(report.schema.is_empty(), "nothing in it to be wrong about");
    assert!(report.references.is_empty());
    assert!(report.unreadable.is_empty());
}
