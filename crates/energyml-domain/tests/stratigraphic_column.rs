//! The stratigraphic column, read from the reference package - both
//! versions, which write the same model and do not write it the same way.
//!
//! # Why this test confronts two sources
//!
//! A column reader cannot check itself: returning a stack of units in one
//! order is indistinguishable from returning it in another, and no assertion
//! downstream complains. The defence is triangulation (`crossaudit.rs`, the
//! same rule): here, **the two versions of the same model**.
//!
//! RESQML 2.0.1 writes `StratigraphicUnitInterpretationIndex { Index, Unit }`
//! and `OrderingCriteria`; 2.2 writes a run of `DataObjectReference`s where
//! the position orders, and `AscendingOrderingCriteria`. Rank 0's contact is
//! even written **the other way round** there (`Subject=B, Object=A` against
//! `Subject=A, Object=B`) with other qualifiers (`proportional` against
//! `conformable`), and the verb changes from `stops at` to `stops`. If the
//! reader does its job, the **column** comes out identical from both.
//!
//! The shape of the assertions comes from the specification; the values come
//! from the file. Only the first is authoritative - a producer may write
//! something non-conformant, and pinning a measured value would make it a rule.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_core::value::Reflect;
use energyml_domain::stratigraphy::{self, Columns};
use energyml_storage::EpcStore;

fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

/// A package's whole stratigraphy, read the way an application reads it.
fn read(path: &Path) -> Columns {
    let store = EpcStore::open(path, registry()).expect("the package opens");
    let held: Vec<_> = store
        .resources()
        .iter()
        .filter_map(|summary| store.get_object(&summary.uri).ok())
        .collect();
    stratigraphy::columns(held.iter().map(|object| object.as_reflect() as &dyn Reflect))
}

fn paired() -> Option<(PathBuf, PathBuf)> {
    let (old, new) = (
        crate::fixtures::find("testingPackageCpp.epc")?,
        crate::fixtures::find("testingPackageCpp22.epc")?,
    );
    Some((old, new))
}

/// The column reads, in both versions, with the same content.
#[test]
fn the_two_versions_draw_the_same_column() {
    let Some((old, new)) = paired() else {
        eprintln!("skipped: the paired fesapi packages are not in the fixtures");
        return;
    };

    // Flattened into lines of text rather than nested tuples: a discrepancy then
    // reads in the failure message instead of asking anyone to count
    // parentheses.
    let shape = |all: &Columns| {
        let mut out: Vec<String> = all
            .columns
            .iter()
            .map(|column| {
                let ranks: Vec<String> = column
                    .ranks
                    .iter()
                    .map(|rank| {
                        let units: Vec<&str> = rank.units.iter().map(|u| u.unit.title.as_str()).collect();
                        format!("#{:?} [{}] {}", rank.number, rank.criteria, units.join(" / "))
                    })
                    .collect();
                format!("{} — {}", column.column.title, ranks.join(" | "))
            })
            .collect();
        out.sort();
        out
    };

    let (before, after) = (read(&old), read(&new));
    assert!(!before.columns.is_empty(), "the 2.0.1 package holds columns");
    assert_eq!(
        shape(&before),
        shape(&after),
        "both versions of the same model must give the same column"
    );
}

/// The order is the one the file states, and it descends.
///
/// Rank 0 of the reference package lists `Unit A interp` at index 0 and
/// `Unit B interp` at index 1, with `apparent depth` as the criterion - so A
/// is **above** B. A reader returning the units in uuid order, or in an order
/// coming from the reference graph, would put `Unit B` first on this precise
/// file and the picture would stay credible.
#[test]
fn the_order_is_the_one_the_file_states_and_it_descends() {
    let Some((old, new)) = paired() else {
        eprintln!("skipped: the paired fesapi packages are not in the fixtures");
        return;
    };
    for path in [old, new] {
        let all = read(&path);
        let column = all
            .columns
            .iter()
            .find(|c| c.ranks.len() > 1)
            .unwrap_or_else(|| panic!("{path:?} holds a column with several ranks"));

        // The ranks come out coarsest to finest.
        let numbers: Vec<_> = column.ranks.iter().map(|r| r.number).collect();
        let mut sorted = numbers.clone();
        sorted.sort();
        assert_eq!(numbers, sorted, "{path:?}: the ranks are numbered in order");

        let coarse = &column.ranks[0];
        assert!(
            coarse.descends(),
            "{path:?}: \"{}\" is one of the schema's three criteria, which all descend",
            coarse.criteria
        );
        let titles: Vec<_> = coarse.units.iter().map(|u| u.unit.title.as_str()).collect();
        assert_eq!(
            titles,
            ["Unit A interp", "Unit B interp"],
            "{path:?}: the increasing index descends, so A comes first"
        );
        // The indices are the file's, not invented positions.
        assert_eq!(coarse.units.iter().map(|u| u.index).collect::<Vec<_>>(), [0, 1]);

        // And the fine rank, which is the one where the assertion can really
        // fail: sorted by uuid it would come out `Unit A, Unit B2, Unit B1` - the uuids are
        // `1a91...`, `7d7a...` (Unit B1), `34c3...` (Unit B2) - and the picture would stay
        // credible. Rank 0 would have said nothing, its two uuids being in order.
        let fine = &column.ranks[1];
        assert_eq!(
            fine.units
                .iter()
                .map(|u| u.unit.title.as_str())
                .collect::<Vec<_>>(),
            ["Unit A interp", "Unit B1 interp", "Unit B2 interp"],
            "{path:?}: the order comes from the file, not from the uuid or the graph"
        );
    }
}

/// A contact is read **inside** the object, and it does not say which is above.
///
/// That is the gap this reader fills: an
/// `AbstractContactInterpretationPart` is a field, with neither uuid nor uri,
/// so looking for it in a reference graph never finds it - and the branch
/// that tried ran on no real package.
///
/// The direction is not data: the **same** contact is written
/// `Subject = Unit B` in 2.0.1 and `Subject = Unit A` in 2.2. Hence `joins`,
/// which recognises the pair both ways round, and the order coming elsewhere.
#[test]
fn a_contact_is_read_inside_the_rank_and_names_no_top() {
    let Some((old, new)) = paired() else {
        eprintln!("skipped: the paired fesapi packages are not in the fixtures");
        return;
    };
    let mut subjects = Vec::new();
    for path in [old, new] {
        let all = read(&path);
        let rank = all
            .columns
            .iter()
            .flat_map(|c| &c.ranks)
            .find(|r| !r.contacts.is_empty())
            .unwrap_or_else(|| panic!("{path:?} holds a rank carrying a contact"));

        let a = rank.units[0].unit.uuid.clone();
        let b = rank.units[1].unit.uuid.clone();
        let between = rank.between(&a, &b);
        assert_eq!(between.len(), 1, "{path:?}: one contact between the two units");
        let contact = between[0];
        assert!(!contact.verb.is_empty(), "{path:?}: the verb is returned verbatim");
        // `PartOf` names the horizon in this package; the XSD says nothing about
        // it, so it is returned with its type rather than renamed "surface".
        let part_of = contact.part_of.as_ref().expect("the contact declares a PartOf");
        assert!(
            !part_of.title.is_empty(),
            "{path:?}: the PartOf target is named"
        );
        assert!(
            part_of.kind.contains("Horizon"),
            "{path:?}: PartOf points at a horizon here, and its type says so - {}",
            part_of.kind
        );
        // The pair is recognised both ways round.
        assert!(contact.joins(&b, &a));
        subjects.push(contact.subject.uuid.clone());
    }
    assert_ne!(
        subjects[0], subjects[1],
        "both versions write this contact in opposite directions - which is \
         exactly why the subject cannot serve as the order"
    );
}

/// A rank refining another shares units with it, and that is what makes the
/// two alignable without counting anything.
///
/// The reference package: rank 0 = `[Unit A, Unit B]`, rank 1 = `[Unit A,
/// Unit B1, Unit B2]`. `Unit A` is **in both**, by the same uuid. Nothing in
/// RESQML says "Unit B1 is inside Unit B" - but "A is in both ranks" is a fact of the
/// file, and it is the anchor a nested view needs.
#[test]
fn ranks_share_units_which_is_what_lets_them_be_aligned() {
    let Some((old, new)) = paired() else {
        eprintln!("skipped: the paired fesapi packages are not in the fixtures");
        return;
    };
    for path in [old, new] {
        let all = read(&path);
        let column = all
            .columns
            .iter()
            .find(|c| c.ranks.len() > 1)
            .expect("a column with several ranks");
        let coarse: Vec<_> = column.ranks[0].units.iter().map(|u| &u.unit.uuid).collect();
        let fine: Vec<_> = column.ranks[1].units.iter().map(|u| &u.unit.uuid).collect();
        let shared: Vec<_> = fine.iter().filter(|uuid| coarse.contains(uuid)).collect();
        assert!(
            !shared.is_empty(),
            "{path:?}: the two ranks share at least one unit — without a shared \
             anchor, an alignment would be guesswork"
        );
        assert!(fine.len() > coarse.len(), "{path:?}: rank 1 refines rank 0");
    }
}

/// Nothing leaves the inventory in silence.
///
/// A `StratigraphicColumnRankInterpretation` is a top-level object; nothing
/// obliges a column to list it. Keeping quiet about it would make a
/// stratigraphy the package contains disappear from the screen.
#[test]
fn a_rank_no_column_lists_is_returned_apart_and_named() {
    let Some((_, new)) = paired() else {
        eprintln!("skipped: the paired fesapi packages are not in the fixtures");
        return;
    };
    let all = read(&new);
    // The reference package lists all its ranks, so there is no orphan -
    // and that is what the test must check, or it could not fail. Each
    // orphan, if there is one, carries its note.
    assert_eq!(
        all.loose_ranks.len(),
        all.notes.iter().filter(|n| n.contains("aucune colonne")).count(),
        "a rank returned apart and a note saying so go together"
    );
    for rank in &all.loose_ranks {
        assert!(!rank.rank.uuid.is_empty());
    }
}

/// A package with no column returns an empty list, not an empty column.
/// Public source: the resqpy packages hold geometry and properties but no
/// stratigraphic column.
#[test]
fn a_package_with_no_column_says_nothing_rather_than_drawing_one() {
    for name in ["block.epc", "s_bend.epc", "tic_tac_toe.epc"] {
        let Some(path) = crate::fixtures::find(name) else {
            eprintln!("skipped: {name} is not in the fixtures");
            continue;
        };
        let all = read(&path);
        assert!(all.columns.is_empty(), "{name} has no stratigraphic column");
        assert!(all.loose_ranks.is_empty());
    }
}

/// The same on Volve. Real-file campaign, run privately.
#[cfg(feature = "private-corpus")]
#[test]
fn a_package_with_no_column_says_nothing_private() {
    let Some(volve) = crate::fixtures::private_find("Volve_Horizons_and_Faults_Depth_originEQN.epc") else {
        eprintln!("skipped: Volve is not in testdata/");
        return;
    };
    let all = read(&volve);
    assert!(all.columns.is_empty(), "Volve has no stratigraphic column");
    assert!(all.loose_ranks.is_empty());
}
