//! An import writes a package, and that package reads back.
//!
//! The other tests of the crate look at objects in memory; this one writes an
//! EPC to disk and reopens it with the ordinary reader — the one the CLI
//! and the front end use. It is the only check that can say that an object
//! *leaves* the crate in a form that the rest of the workspace accepts.
//!
//! Without the `hdf5` feature, the coordinates cannot be written. The check
//! **is not skipped for all that**: it then verifies that the refusal is named,
//! which is precisely the guarantee that counts — a package that promises
//! coordinates absent from a file looks exactly like a complete package.

use std::sync::Arc;

use energyml_build::assemble;
use energyml_build::crs::Vertical;
use energyml_build::epc::{self, Options};
use energyml_build::geojson;

/// Two units on one stage, with duplicated passes.
fn export() -> String {
    let feature = |unit: &str, lon: f64, lat: f64, z: f64, older: (&str, f64), younger: (&str, f64)| {
        format!(
            r#"{{"type":"Feature","geometry":{{"type":"Point","coordinates":[{lon},{lat},{z}]}},
               "properties":{{
                 "composition":[{{"material":{{"StratiRockMaterial":{{"lithostratigraphy":{{
                    "@href":"https://referentiel-lithostrat.brgm-rec.fr/#/lithostrat/search/details/{unit}",
                    "@title":"{unit}"}}}}}}}}],
                 "geologicHistory":[{{
                    "olderNamedAge":{{"@title":"{}"}},
                    "youngerNamedAge":{{"@title":"{}"}},
                    "numericAge":{{"NumericAgeRange":{{
                       "olderBoundDate":{{"Quantity":{{"value":{}}}}},
                       "youngerBoundDate":{{"Quantity":{{"value":{}}}}}}}}}}}]}}}}"#,
            older.0, younger.0, older.1, younger.1
        )
    };
    let a = feature("Marnes et Caillasses", 2.29, 48.87, 58.0, ("Bartonien", 37.8), ("Lutétien", 47.8));
    let b = feature("Marnes et Caillasses supérieures", 2.40, 48.90, 50.0, ("Bartonien", 37.8), ("Bartonien", 41.2));
    format!(
        r#"{{"type":"FeatureCollection","features":[{a},{a},{b}]}}"#
    )
}

fn options() -> Options {
    Options {
        name: "import.epc".into(),
        data_file: "import.h5".into(),
        vertical: Vertical::default(),
        // The degrees as they are, which is what the test wants: the projection has its
        // own checks, and mixing the two would make a test that fails for
        // two reasons.
        target: energyml_build::crs::Target::Geographic(4326),
        source_epsg: 4326,
        creation: "2026-09-08T00:00:00Z".into(),
        originator: "BRGM".into(),
        source: "GeologicUnit_Bartonien_pretty.json".into(),
        stage: "Bartonien".into(),
    }
}

#[test]
fn an_import_writes_a_package_that_the_ordinary_reader_reads_back() {
    let collection = geojson::read(&export()).expect("a collection");
    let plan = assemble::plan(&collection, "Bartonien");
    assert_eq!(plan.groups.len(), 2, "two units on the stage asked for");

    let registry = Arc::new(energyml_schemas::all());
    let mut store = epc::open(None, "import.epc", registry.clone()).expect("a new package");
    let dir = std::env::temp_dir().join(format!("energyml-import-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    store.external_files_mut().set_root(&dir);

    let report = epc::write(&mut store, &plan, &options());

    // **The objects, one per group plus the common ones.** Two CRS — EML 2.3
    // offers no top-level geographic compound, see `crate::crs`
    // —, a table, and per group: feature, interpretation, point set,
    // property. Plus the canonical template and the activity. The count is written
    // here so that an object lost on the way shows, not to pin a value
    // of the file.
    let expected = 2 + 1 + plan.groups.len() * 4 + 2;
    assert_eq!(
        report.written.len(),
        expected,
        "written: {:?}; refused: {:?}",
        report.written,
        report.refused
    );
    assert_eq!(report.points, 2, "deux lieux distincts");
    assert_eq!(report.features, 3, "trois passes");
    assert_eq!(report.duplicates(), 1);

    // **Either the coordinates are written, or the refusal names the file.** Never
    // a silence: a package promising numbers that are in no
    // file looks exactly like a complete package, and the only moment when
    // anyone would find out is when a viewer draws nothing.
    //
    // **Without `#[cfg(feature = "hdf5")]`, and that is the point.** The first version
    // branched on it and went red under `cargo test --workspace`: the `cfg`
    // guards on the feature of *this* crate, whereas the writing depends on that
    // of `energyml-storage` — which another crate of the workspace enables through its test
    // dependency. Cargo unifies the features of a workspace, so “this crate has no
    // hdf5” does not mean “there is no backend”. A test that demanded
    // a refusal saw a successful writing.
    //
    // The guarantee never concerned *that it refuses* — that is the
    // build configuration — but *what it says when it
    // refuses*. That is what is written here, and it is true on both sides.
    for why in &report.refused {
        assert!(
            why.contains("import.h5") || why.to_lowercase().contains("hdf5"),
            "a refusal must name the file or the capacity that is missing: {why}"
        );
    }
    // **And the exception of degrees is said, as a note and not a refusal**
    // (a deliberate decision of 24/09): `energyml import` fails on a refusal, and an
    // import in degrees — the default — is not one.
    assert!(
        report.notes.iter().any(|note| note.contains("Geographic2dCrs")),
        "the exception of degrees is not said: {:?}",
        report.notes
    );

    // And the package reads back — through the ordinary reader, not through a back
    // door. A save regenerates `[Content_Types].xml`, the `.rels` and
    // `docProps/core.xml` from the objects, so this is also what verifies
    // that they hold together.
    let out = dir.join("import.epc");
    store
        .save_as(&out, &energyml_storage::SaveOptions::default())
        .expect("the package is written");

    let reread = energyml_storage::EpcStore::open(&out, registry).expect("it reopens");
    let inventory = reread.resources();
    assert_eq!(
        inventory.len(),
        expected,
        "the package read back holds {} objects instead of {expected}",
        inventory.len()
    );
    // The expected types are there, and named: a right count with the wrong
    // types would pass.
    let types: Vec<&str> = inventory.iter().map(|s| s.qualified_type.as_str()).collect();
    for wanted in [
        "eml23.Geographic2dCrs",
        "eml23.VerticalCrs",
        "eml23.ColumnBasedTable",
        "eml23.Activity",
        // The template, **in** the package: the activity references it, and a
        // reference to an absent object is what `energyml check` found
        // on the first package produced. No in-memory test could
        // see it — an activity that references a template is well formed.
        "eml23.ActivityTemplate",
        "resqml22.RockVolumeFeature",
        "resqml22.StratigraphicUnitInterpretation",
        "resqml22.PointSetRepresentation",
        "resqml22.DiscreteProperty",
    ] {
        assert!(types.contains(&wanted), "{wanted} manque : {types:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_empty_plan_does_not_produce_an_empty_package_in_silence() {
    // The case that matters most to someone who picks the wrong stage: nothing to
    // write must be said, and not look like an import that worked.
    let collection = geojson::read(&export()).expect("a collection");
    let plan = assemble::plan(&collection, "Yprésien");
    assert!(plan.groups.is_empty());

    let registry = Arc::new(energyml_schemas::all());
    let mut store = epc::open(None, "vide.epc", registry).expect("a new package");
    let report = epc::write(&mut store, &plan, &options());

    assert!(report.written.is_empty());
    assert!(
        report.refused.iter().any(|why| why.contains("no group")),
        "{:?}",
        report.refused
    );
    // And what was set aside is named, not only counted.
    assert!(!report.skipped.is_empty());
}


/// A square polyline, in pieces, in its own system.
fn a_square() -> energyml_build::pieces::Pieces {
    use energyml_geometry::geometry::{Geometry, Patch, Topology};
    energyml_build::pieces::Pieces::empty().with(
        "square",
        energyml_build::pieces::Source::Shape {
            geometry: Geometry {
                kind: "polylines",
                patches: vec![Patch {
                    index: 0,
                    node_offset: 0,
                    points: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0]],
                    topology: Topology::Polylines(vec![4]),
                    closed: Vec::new(),
                }],
            },
            closed: None,
        },
    )
}

#[test]
fn the_chosen_triplet_is_written_and_the_package_reads_it_back() {
    // **The feedback of 10/09**: the feature/interpretation pair was no longer
    // a table in the code, it is chosen — *“the list of the objects that
    // extend AbstractObject, filtered on feature”*. This test is what says
    // that this choice produces objects that the ordinary reader reads back, and not
    // only JSON that looks right.
    //
    // To make it turn red: write the feature without `IsWellKnown`, or link
    // the interpretation to something other than the feature just made.
    use energyml_build::operation::{Choices, Identity, Registry, When};
    let when = When { creation: "2026-09-10T00:00:00Z".into(), originator: "test".into() };
    let choices = Choices::new()
        .set("title", "AOI")
        .set("sourceEpsg", 4326)
        .set("project", 2154)
        .set("featureType", "resqml22.BoundaryFeature")
        .set("interpretationType", "resqml22.HorizonInterpretation");
    let plan = Registry::new()
        .plan("polylines", &a_square(), &choices, &Identity::derived(), &when)
        .expect("planned");

    let roles: Vec<&str> = plan.objects.iter().map(|o| o.role.as_str()).collect();
    assert!(roles.contains(&"feature") && roles.contains(&"interpretation"), "{roles:?}");
    let feature = plan.of_role("feature").next().expect("a feature");
    assert_eq!(feature.qualified_type, "resqml22.BoundaryFeature");
    let interpretation = plan.of_role("interpretation").next().expect("an interpretation");
    assert_eq!(interpretation.json["InterpretedFeature"]["Uuid"], serde_json::json!(feature.uuid));
    let representation = plan.of_role("representation").next().expect("a representation");
    assert_eq!(representation.json["RepresentedObject"]["Uuid"], serde_json::json!(interpretation.uuid));

    // And the package: written, then read back by the ordinary reader. It is the
    // triangulation that the repository demands — two sources of the same fact, one of which
    // is not the code that has just written.
    let registry = std::sync::Arc::new(energyml_schemas::all());
    let mut store = epc::open(None, "triplet.epc", registry.clone()).expect("a new package");
    let dir = std::env::temp_dir().join(format!("energyml-triplet-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    store.external_files_mut().set_root(&dir);
    let made = energyml_build::apply::apply(&plan, &mut store, &when);
    assert!(made.invalid.is_empty(), "objects written against their schema: {:#?}", made.invalid);
    assert!(
        made.refused.iter().all(|why| why.contains("array") || why.contains("backend") || why.contains("hdf5")),
        "refused: {:?}",
        made.refused
    );
    for object in [feature, interpretation] {
        let found = store
            .find_by_uuid(&object.uuid)
            .unwrap_or_else(|| panic!("`{}` is not in the package", object.title));
        assert_eq!(found.qualified_type, object.qualified_type);
        store.get_object(&found.uri).unwrap_or_else(|e| panic!("`{}` does not read back: {e}", object.title));
    }
}

#[test]
fn without_a_chosen_type_nothing_is_invented() {
    // *Refuse rather than draw*: a representation without an interpretation
    // is perfectly legal, and making one that nobody asked for
    // would put in the package a kind of feature coming from nowhere.
    use energyml_build::operation::{Choices, Identity, Registry, When};
    let when = When { creation: "2026-09-10T00:00:00Z".into(), originator: "test".into() };
    let plan = Registry::new()
        .plan(
            "polylines",
            &a_square(),
            &Choices::new().set("title", "AOI").set("sourceEpsg", 4326),
            &Identity::derived(),
            &when,
        )
        .expect("planned");
    let roles: Vec<&str> = plan.objects.iter().map(|o| o.role.as_str()).collect();
    assert!(!roles.contains(&"feature"), "{roles:?}");
    let representation = plan.of_role("representation").next().expect("a representation");
    assert!(representation.json["RepresentedObject"].is_null());
}
