//! What a metadata spec asked for and did not find.
//!
//! Two things went wrong at once in the report that prompted this, and only one
//! of them was the reading:
//!
//! 1. A key that resolved to nothing simply **vanished** from the output. In a
//!    GeoJSON `FeatureCollection` - read as a table - a column that disappears
//!    on the rows where it is empty is a different table.
//! 2. Five of the six keys resolved to nothing on a resqml 2.2 package because
//!    the paths were written with the 2.0.1 spelling. Including the one the
//!    workspace itself ships in `Spec::standard`.

use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_domain::metadata::Spec;
use energyml_domain::source::ObjectSource;

fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

/// A source with nothing in it: enough for paths that stay on the object.
struct Nowhere;
impl ObjectSource for Nowhere {
    fn object(&self, _uuid: &str) -> Option<Box<dyn energyml_core::registry::DynObject>> {
        None
    }
}

#[test]
fn a_key_that_found_nothing_keeps_its_place() {
    let mut spec = Spec::new();
    spec.add("title", "Citation.Title");
    spec.add("author", "Citation.Originator");
    spec.add("nonsense", "No.Such.Path");

    let mut object = energyml_resqml_2_2::structural::TriangulatedSetRepresentation::default();
    object.citation.title = "Top Brent".into();

    let found = spec.read(&object, &Nowhere);
    let entries = found.entries();

    assert_eq!(
        entries.len(),
        3,
        "every key of the spec comes back, found or not: {entries:?}"
    );
    assert_eq!(entries[0], ("title", Some("Top Brent")));
    assert_eq!(entries[1].0, "author");
    assert!(entries[1].1.is_none(), "absent, and said so rather than dropped");
    assert_eq!(entries[2], ("nonsense", None));

    // `pairs` is the other question - what is there - and formats with no null
    // to write still use it.
    assert_eq!(found.pairs(), vec![("title", "Top Brent")]);
    assert_eq!(found.misses(), ["author", "nonsense"]);
}

#[test]
fn a_path_written_for_one_release_works_on_the_other() {
    // `RepresentedInterpretation` is how resqml 2.0.1 spells it and
    // `RepresentedObject` is how 2.2 does. A path is written by a person, once,
    // and then run against packages of every release.
    let mut spec = Spec::new();
    spec.add("interpretation", "RepresentedInterpretation.uuid");
    spec.add("interpretationTitle", "RepresentedInterpretation.Title");

    let object = energyml_resqml_2_2::structural::TriangulatedSetRepresentation {
        represented_object: Some(energyml_eml_2_3::DataObjectReference {
            uuid: "8f1e2d3c-1111-2222-3333-444455556666".into(),
            title: "Top Brent Interp".into(),
            ..Default::default()
        }),
        ..Default::default()
    };

    let found = spec.read(&object, &Nowhere);
    assert_eq!(
        found.get("interpretation"),
        Some("8f1e2d3c-1111-2222-3333-444455556666"),
        "the 2.0.1 spelling has to reach the 2.2 element, or a spec must be forked per version"
    );
    assert_eq!(found.get("interpretationTitle"), Some("Top Brent Interp"));
}

#[test]
fn the_shipped_spec_resolves_on_a_2_2_object() {
    // It did not, and nothing noticed: a path that finds nothing is a result,
    // not an error, so `--standard-metadata` produced no interpretation and no
    // feature on every 2.2 package, quietly.
    let object = energyml_resqml_2_2::structural::TriangulatedSetRepresentation {
        represented_object: Some(energyml_eml_2_3::DataObjectReference {
            uuid: "8f1e2d3c-1111-2222-3333-444455556666".into(),
            title: "Top Brent Interp".into(),
            ..Default::default()
        }),
        citation: energyml_eml_2_3::Citation {
            title: "Top Brent".into(),
            ..Default::default()
        },
        ..Default::default()
    };

    let found = Spec::standard().read(&object, &Nowhere);
    assert!(
        found.get("interpretation").is_some(),
        "the standard spec found no interpretation on a 2.2 object: {:?}",
        found.misses()
    );
    let _ = registry();
}
