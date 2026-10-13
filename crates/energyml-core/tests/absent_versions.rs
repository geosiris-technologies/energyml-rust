//! `Registry::declare_absent`: a version the project ships and this build does
//! not carry is **named**, in every spelling a document uses for it.
//!
//! Pure data, so the registry here is empty: what is under test is the table
//! and the way it is asked, not any generated type.
//!
//! Each test says what to break to make it red.

use energyml_core::error::Error;
use energyml_core::registry::{ReadMode, Registry};

const WHY_20: &str = "RESQML 2.0.1 is not compiled in: enable the `resqml20` feature";
const WHY_DEV3: &str = "RESQML 2.2_dev3 is not compiled in: enable the `resqml22dev3` feature";

fn registry() -> Registry {
    let mut registry = Registry::new();
    registry.declare_absent("resqml", "2.0.1", &["20", "201"], WHY_20);
    registry.declare_absent("resqml", "2.2_dev3", &[], WHY_DEV3);
    registry
}

/// `2.0`, `2.0.1`, `20` and `201` are one version, whatever the case of the
/// package. Break it: compare `compact_version(version)` of the registry's own
/// spelling only (drop the `compact` list) and `2.0` - what a RESQML 2.0.1
/// `schemaVersion` says - is no longer absent.
#[test]
fn every_spelling_of_a_version_names_it() {
    let registry = registry();
    for version in ["2.0", "2.0.1", "20", "201", " 2.0 "] {
        assert_eq!(registry.absent("resqml", version), Some(WHY_20), "`{version}`");
    }
    assert_eq!(registry.absent("RESQML", "2.0"), Some(WHY_20));
}

/// Another version of the same package, or the same version of another one, is
/// not absent. Break it: drop the package test in `AbsentVersion::names` and
/// `witsml 2.0` is refused as RESQML 2.0.1.
#[test]
fn only_the_declared_version_of_the_declared_package_is_absent() {
    let registry = registry();
    assert_eq!(registry.absent("resqml", "2.2"), None);
    assert_eq!(registry.absent("witsml", "2.0"), None);
    assert_eq!(
        registry.absent("resqml", "2.0.9"),
        None,
        "a version nobody ships is unknown, not absent"
    );
}

/// The drop and the release are two versions: declaring the drop absent says
/// nothing of `2.2`, and `2.2dev3` is the drop however it is spelled. Break it:
/// ignore `prerelease` in `names`.
#[test]
fn a_development_drop_is_not_its_release() {
    let registry = registry();
    assert_eq!(registry.absent("resqml", "2.2"), None);
    assert_eq!(registry.absent("resqml", "2.2_dev3"), Some(WHY_DEV3));
    assert_eq!(registry.absent("resqml", "2.2dev3"), Some(WHY_DEV3));
    assert_eq!(registry.absent("resqml", "2.2-DEV3"), Some(WHY_DEV3));
}

/// By qualified type: `resqml20.` and `resqml201.` both name RESQML 2.0.1, and
/// a qualified type cannot name a drop, so `resqml22.` is the release (not
/// declared here). Break it: match `resqml22.` against the drop's row.
#[test]
fn a_qualified_type_names_the_release_in_either_spelling() {
    let registry = registry();
    assert_eq!(
        registry.absent_qualified_type("resqml20.obj_Activity"),
        Some(WHY_20)
    );
    assert_eq!(
        registry.absent_qualified_type("resqml201.obj_Activity"),
        Some(WHY_20)
    );
    assert_eq!(registry.absent_qualified_type("resqml22.Activity"), None);
    assert_eq!(registry.absent_qualified_type("eml20.Citation"), None);
    assert_eq!(registry.absent_qualified_type("not a qualified type"), None);
}

/// By content type, with the dotted version.
#[test]
fn a_content_type_names_the_version() {
    let registry = registry();
    for version in ["2.0", "2.0.1"] {
        let content_type = format!("application/x-resqml+xml;version={version};type=obj_Activity");
        assert_eq!(
            registry.absent_content_type(&content_type),
            Some(WHY_20),
            "{content_type}"
        );
    }
    assert_eq!(
        registry.absent_content_type("application/x-resqml+xml;version=2.2;type=Activity"),
        None
    );
}

/// The two lookups that used to say "unknown type (not registered)" now carry
/// the refusal when the version was declared absent - and still say it for a
/// name nobody declared. Break it: build the error with `UnknownType` only.
#[test]
fn creating_and_reading_an_absent_version_is_a_named_refusal() {
    let registry = registry();

    match registry.create("resqml201.obj_Activity") {
        Err(Error::VersionAbsent(why)) => assert_eq!(why, WHY_20),
        other => panic!("expected the refusal, got {:?}", other.err()),
    }
    match registry.read_xml_as(
        "application/x-resqml+xml;version=2.0;type=obj_Activity",
        b"<x/>",
        ReadMode::Tolerant,
    ) {
        Err(Error::VersionAbsent(why)) => assert_eq!(why, WHY_20),
        other => panic!("expected the refusal, got {:?}", other.err()),
    }

    // The converse: nothing declared, nothing named.
    assert!(matches!(
        registry.create("resqml22.Activity"),
        Err(Error::UnknownType(_))
    ));
    assert!(matches!(
        registry.read_xml_as(
            "application/x-resqml+xml;version=9.9;type=Activity",
            b"<x/>",
            ReadMode::Tolerant
        ),
        Err(Error::UnknownType(_))
    ));
}
