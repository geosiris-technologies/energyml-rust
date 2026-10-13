//! Reading a whole package into shapes - the piece three faces had each.
//!
//! `energyml export`, `energyml geometry` and the Python bridge assembled this
//! themselves, and the three had begun to differ: the colour ramps were
//! resolved by one of them and reachable from neither of the others. What is
//! checked here is what those three now share.
//!
//! # What makes these falsifiable
//!
//! Counting objects would pass on almost any mistake - a walk that attached no
//! property, or one that attached every property to every shape, both return
//! the same number of shapes. So each check names the **thing that would be
//! wrong**: an attribute whose values are not the count of the elements it
//! hangs on, a colour ramp that never arrives, a type filter that lets another
//! type through, a cheap default that is not cheap.

// Everything here needs a build that can read an HDF5 dataset, so the whole
// file is behind the feature - imports and helpers included, or a build without
// it warns about what it compiled for nothing. It runs in the `hdf5` CI step, like
// `tests/corpus.rs` beside it.
#[cfg(feature = "hdf5")]
use std::path::PathBuf;
#[cfg(feature = "hdf5")]
use std::sync::Arc;

#[cfg(feature = "hdf5")]
use energyml_core::registry::Registry;
#[cfg(feature = "hdf5")]
use energyml_geometry::scene::{self, Properties, Request};
#[cfg(feature = "hdf5")]
use energyml_storage::EpcStore;

#[cfg(feature = "hdf5")]
fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

/// One reference package, or `None` - the packages are fetched, not
/// versioned, so a fresh clone must skip rather than fail (and CI must not).
#[cfg(feature = "hdf5")]
fn package(name: &str) -> Option<PathBuf> {
    let found = crate::fixtures::find(name);
    if found.is_none() {
        eprintln!("skipped: {name} is absent from the fixtures");
    }
    found
}

#[test]
#[cfg(feature = "hdf5")]
fn a_package_comes_back_as_shapes_that_name_themselves() {
    let Some(path) = package("testingPackageCpp22.epc") else {
        return;
    };
    let store = EpcStore::open(&path, registry()).expect("the package opens");
    let resources = store.resources();

    let read = scene::of_package(&resources, &store, &Request::default());
    assert!(
        !read.objects.is_empty(),
        "no shape at all out of a package of them"
    );

    for object in &read.objects {
        // **A qualified type, not a class name.** It was the Rust type name
        // until 20/09, so an OBJ comment, a GeoJSON property and a VTK field
        // all carried `Grid2dRepresentation` where the package says
        // `resqml22.Grid2dRepresentation` - a name that does not say which
        // release it came from, in the one line of an export meant to lead
        // back to the package.
        assert!(
            object.qualified_type.contains('.'),
            "`{}` is a class name, not a qualified type",
            object.qualified_type
        );
        assert!(
            object.uri.contains(&object.qualified_type),
            "{} and {} disagree about what this object is",
            object.uri,
            object.qualified_type
        );
        assert!(!object.uuid.is_empty(), "{} has no uuid", object.uri);
    }

    // A sound package of the corpus reads whole. Not a count of objects - a
    // count of **failures**, which is the number a caller acts on.
    assert!(read.unreadable.is_empty(), "parts refused: {:?}", read.unreadable);
}

#[test]
#[cfg(feature = "hdf5")]
fn nothing_is_read_that_was_not_asked_for() {
    let Some(path) = package("testingPackageCpp22.epc") else {
        return;
    };
    let store = EpcStore::open(&path, registry()).expect("the package opens");
    let resources = store.resources();

    // The default is the cheap one, and that is the point: attaching the
    // properties means reading every property of the package and its external
    // arrays, which an export of pure geometry has no reason to pay for.
    let bare = scene::of_package(&resources, &store, &Request::default());
    assert!(
        bare.objects.iter().all(|one| one.attributes.is_empty()),
        "properties were read without being asked for"
    );

    let filtered = scene::of_package(
        &resources,
        &store,
        &Request {
            of_type: Some("TriangulatedSet"),
            ..Default::default()
        },
    );
    assert!(!filtered.objects.is_empty(), "the filter kept nothing at all");
    assert!(
        filtered
            .objects
            .iter()
            .all(|one| one.qualified_type.contains("TriangulatedSet")),
        "the filter let another type through"
    );
    assert!(
        filtered.objects.len() < bare.objects.len(),
        "the filter kept everything, so it filtered nothing"
    );
}

#[test]
#[cfg(feature = "hdf5")]
fn the_properties_land_on_the_shapes_they_belong_to() {
    let Some(path) = package("testingPackageCpp22.epc") else {
        return;
    };
    let store = EpcStore::open(&path, registry()).expect("the package opens");
    let resources = store.resources();

    let read = scene::of_package(
        &resources,
        &store,
        &Request {
            properties: Properties::All,
            ..Default::default()
        },
    );
    let attached: Vec<_> = read
        .objects
        .iter()
        .flat_map(|one| one.attributes.iter().map(move |a| (one, a)))
        .collect();
    assert!(!attached.is_empty(), "no property was attached to anything");

    for (object, attribute) in &attached {
        // **The count is the check.** A property lined up against the wrong
        // elements draws a plausible picture of nothing, which is the one
        // outcome worth ruling out - and it is what `attributes_for` refuses
        // on, so a walk that skipped that refusal would show up here.
        let elements = match attribute.scope {
            scene::AttributeScope::Points => object.geometry.point_count(),
            scene::AttributeScope::Cells => object.geometry.cell_count(),
        };
        assert_eq!(
            attribute.values.len(),
            elements * attribute.components as usize,
            "`{}` on {} has {} value(s) for {elements} element(s) × {} component(s)",
            attribute.name,
            object.uuid,
            attribute.values.len(),
            attribute.components
        );
    }

    // And **the colours arrive**, which is the half only `export` could reach
    // before: the cascade needs the whole package, so an exporter - handed one
    // object - cannot resolve it, and neither could anything that read a
    // package shape by shape.
    let with_a_ramp = attached.iter().filter(|(_, a)| !a.lut.is_empty()).count();
    assert!(
        with_a_ramp > 0,
        "not one of {} attached properties carries a colour ramp, on a package that states them",
        attached.len()
    );
}

#[test]
#[cfg(feature = "hdf5")]
fn asking_for_one_property_attaches_one_property() {
    let Some(path) = package("testingPackageCpp22.epc") else {
        return;
    };
    let store = EpcStore::open(&path, registry()).expect("the package opens");
    let resources = store.resources();

    let all = scene::of_package(
        &resources,
        &store,
        &Request {
            properties: Properties::All,
            ..Default::default()
        },
    );
    let (uuid, name) = all
        .objects
        .iter()
        .flat_map(|one| one.attributes.iter())
        .map(|a| (a.uuid.clone(), a.name.clone()))
        .next()
        .expect("a package of the corpus states properties");

    let only = [uuid.clone()];
    let one = scene::of_package(
        &resources,
        &store,
        &Request {
            properties: Properties::Only(&only),
            ..Default::default()
        },
    );
    let names: Vec<&str> = one
        .objects
        .iter()
        .flat_map(|object| object.attributes.iter())
        .map(|a| a.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec![name.as_str()],
        "asking for one property brought back {names:?}"
    );
}
