//! The reference graph, both directions, and the closure.
//!
//! Each check confronts one direction with the other: what an object *says* it
//! points at, against what a scan of every object says points back. A graph
//! that agreed with itself would prove nothing — these two are computed by
//! different walks over different inputs.

use std::path::PathBuf;

use energyml_rust::session::Package;

fn corpus(name: &str) -> Option<PathBuf> {
    crate::fixtures::find_or_skip(name)
}

fn package() -> Option<Package> {
    let path = corpus("testingPackageCpp22.epc")?;
    Some(Package::open(&path).expect("the package opens"))
}

#[test]
fn what_an_object_points_at_points_back_at_it() {
    let Some(package) = package() else { return };

    // One object with references is enough to make the point, and looking for
    // it rather than naming one keeps this working on another corpus.
    let found = package.objects().into_iter().find_map(|one| {
        let links = package.references(&one.uri).ok()?;
        links.iter().any(|link| link.present).then_some((one, links))
    });
    let Some((object, links)) = found else {
        eprintln!("skipped: no object of this package references another one it holds");
        return;
    };

    let mut checked = 0;
    for link in links.iter().filter(|link| link.present) {
        // **The confrontation.** `references` read one object and believed its
        // DORs; `referenced_by` read every object and kept the ones that name
        // this uuid. They have to meet, path included.
        let back = package
            .referenced_by(&link.uri)
            .expect("the other direction answers");
        let mine: Vec<&str> = back
            .iter()
            .filter(|one| one.uri == object.uri)
            .map(|one| one.path.as_str())
            .collect();
        assert!(
            mine.contains(&link.path.as_str()),
            "{} says it points at {} through `{}`, and looking back gives {mine:?}",
            object.uri,
            link.uri,
            link.path
        );
        checked += 1;
    }
    assert!(checked > 0, "nothing was actually compared");
}

#[test]
fn an_object_is_never_its_own_dependent() {
    let Some(package) = package() else { return };
    let Some(first) = package.objects().into_iter().next() else {
        return;
    };
    for one in package.referenced_by(&first.uri).expect("the question answers") {
        assert_ne!(one.uri, first.uri, "an object listed as pointing at itself");
    }
}

#[test]
fn a_reference_into_nothing_is_said_to_be_absent() {
    let Some(package) = package() else { return };

    // This corpus package names objects it does not hold — the save says so in
    // its diagnostics. The point here is that `references` reports it as a
    // fact on the link rather than dropping the edge: an object that quietly
    // lost its dangling references would look self-contained.
    let mut present = 0;
    let mut absent = 0;
    for object in package.objects() {
        for link in package.references(&object.uri).expect("references answer") {
            if link.present {
                present += 1;
            } else {
                absent += 1;
                assert!(!link.uuid.is_empty(), "an absent target is still named");
                assert!(!link.path.is_empty(), "and the path to it is still known");
            }
        }
    }
    assert!(present > 0, "a package of references has some that resolve");
    assert!(
        absent > 0,
        "this corpus package is known to name what it does not hold"
    );
    eprintln!("{present} reference(s) resolve, {absent} point outside the package");
}

#[test]
fn what_an_object_needs_is_a_set_that_closes() {
    let Some(package) = package() else { return };
    let found = package
        .objects()
        .into_iter()
        .find(|one| one.qualified_type.contains("Representation"));
    let Some(object) = found else {
        eprintln!("skipped: no representation in this package");
        return;
    };

    let (needed, _missing) = package
        .needs(std::slice::from_ref(&object.uri))
        .expect("the closure answers");
    assert!(needed.contains(&object.uri), "the seed is part of what it needs");
    assert!(
        needed.len() > 1,
        "a representation needs at least its interpretation or its CRS"
    );

    // **Closed means closed**, and this checks it the other way round: for
    // every object kept, every reference it holds that the package *has* must
    // be kept too. The closure walked the graph's edges; this walks each
    // object's own DORs.
    for uri in &needed {
        for link in package.references(uri).expect("references answer") {
            if link.present {
                assert!(
                    needed.contains(&link.uri),
                    "{uri} needs {} through `{}`, and the closure left it out",
                    link.uri,
                    link.path
                );
            }
        }
    }
}

#[test]
fn what_is_missing_from_a_closure_is_named() {
    let Some(package) = package() else { return };

    // The whole package as the seed: whatever it names and does not hold is
    // what a copy of it could not take along.
    let all: Vec<String> = package.objects().into_iter().map(|one| one.uri).collect();
    let (needed, missing) = package.needs(&all).expect("the closure answers");
    assert_eq!(needed.len(), all.len(), "asking for everything keeps everything");

    // Not asserted as a number: what matters is that each one is a uri and
    // that a well-known property kind is **not** in the list — the mistake
    // this repository has already made once, where four references to the
    // Energistics catalogue were reported as dangling.
    for one in &missing {
        assert!(one.starts_with("eml:///"), "a missing target is a uri: {one}");
        assert!(
            !one.contains("PropertyKind"),
            "a well-known kind is not missing, it is defined elsewhere: {one}"
        );
    }
    eprintln!("{} reference target(s) missing from the package", missing.len());
}
