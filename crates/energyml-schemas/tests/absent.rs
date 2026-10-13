//! The registry `all()` builds knows which shipped versions it does not hold.
//!
//! Holds under **any** feature set: the "compiled" set is a parameter of
//! `declare_absent_versions`, so a workspace build with every feature on can
//! still ask the question of a build with none.
//!
//! What to break, for each test, is said in its doc.

use energyml_core::registry::Registry;
use energyml_schemas::{declare_absent_versions, KNOWN_VERSIONS};

/// With nothing compiled, every row of `KNOWN_VERSIONS` is declared, in the
/// spellings a document uses, with the message naming its feature.
///
/// Break it: skip the `compact` list in `declare_absent_versions`, and the
/// `resqml201` / `2.0` forms stop naming RESQML 2.0.1.
#[test]
fn a_version_that_is_not_compiled_is_declared_absent_under_every_spelling() {
    let mut registry = Registry::new();
    declare_absent_versions(&mut registry, &[]);

    for known in KNOWN_VERSIONS {
        let why = registry
            .absent(known.package, known.version)
            .unwrap_or_else(|| panic!("{} {} is not declared absent", known.package, known.version));
        assert!(why.contains(&format!("`{}`", known.feature)), "`{why}` does not name the feature");
        for compact in known.compact {
            // A qualified type cannot name a drop: its row answers only for the release.
            if !energyml_core::naming::is_prerelease(known.version) {
                assert!(
                    registry.absent_qualified_type(&format!("{}{compact}.Thing", known.package)).is_some(),
                    "{}{compact}. is not refused",
                    known.package
                );
            }
        }
    }

    // The two spellings the plan names, by hand: the `schemaVersion` of a 2.0.1
    // part is `2.0`, and fesapi also writes `resqml201.`.
    let why = registry.absent("resqml", "2.0").expect("2.0 is RESQML 2.0.1");
    assert!(why.contains("`resqml20`"), "{why}");
    assert!(registry.absent_qualified_type("resqml201.obj_Activity").is_some());
    assert!(registry.absent_qualified_type("resqml20.obj_Activity").is_some());
}

/// A version that is compiled is declared nowhere: the registry must not refuse
/// what it holds. Break it: drop the `refusal(...)` filter and every row is
/// declared whatever was compiled.
#[test]
fn a_compiled_version_is_not_declared_absent() {
    let everything: Vec<&str> = KNOWN_VERSIONS.iter().map(|known| known.feature).collect();
    let mut registry = Registry::new();
    declare_absent_versions(&mut registry, &everything);
    for known in KNOWN_VERSIONS {
        assert_eq!(registry.absent(known.package, known.version), None, "{} {}", known.package, known.version);
    }

    // Only RESQML 2.2 compiled: 2.0.1 is absent, 2.2 is not.
    let mut registry = Registry::new();
    declare_absent_versions(&mut registry, &["resqml22", "eml23"]);
    assert!(registry.absent("resqml", "2.0.1").is_some());
    assert_eq!(registry.absent("resqml", "2.2"), None);
}

/// `energyml_schemas::all()` itself declares what its own build lacks: in the
/// default build nothing, and under `--no-default-features --features
/// resqml20` the RESQML 2.2 it did not compile. Break it: remove the call from
/// `all()`.
#[test]
fn all_declares_what_this_build_lacks() {
    let registry = energyml_schemas::all();
    for known in KNOWN_VERSIONS {
        let declared = registry.absent(known.package, known.version).is_some();
        assert_eq!(declared, !known.is_compiled(), "{} {}", known.package, known.version);
    }
}
