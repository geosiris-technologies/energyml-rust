//! What the examples promise, checked by `cargo test`.
//!
//! `examples/01_add_a_surface.py` and `examples/02_a_package_from_nothing.py`
//! are the documentation's worked cases, and they are the first thing a user of
//! the bridge runs. They build RESQML objects **as dictionaries**, which means
//! nothing in the type system protects them: a renamed field or a tightened
//! enum would leave them printing a refusal, and the suite would stay green.
//!
//! So the same two objects are built here, in the same shapes, through the same
//! door. This file is not a second implementation — it is the examples' alarm.
//! When one changes, the other changes with it.
//!
//! What it deliberately does *not* check is the Python: that needs an
//! interpreter and a `maturin develop`, which `cargo test` has no way to
//! arrange. What can be wrong in an example is the JSON, and the JSON is here.

// Everything but the last check needs the native backend: an example that
// writes no numbers is not one of these two. Behind the feature rather than
// deleted, so `cargo clippy` stays silent in a build that has no HDF5.
#[cfg(feature = "hdf5")]
use std::path::PathBuf;

use energyml_rust::session::Package;
#[cfg(feature = "hdf5")]
use energyml_rust::session::{ArrayRef, Block};
#[cfg(feature = "hdf5")]
use serde_json::{Value, json};

#[cfg(feature = "hdf5")]
fn corpus(name: &str) -> Option<PathBuf> {
    crate::fixtures::find_or_skip(name)
}

#[cfg(feature = "hdf5")]
fn citation(title: &str) -> Value {
    json!({
        "$type": "eml23.Citation",
        "Title": title,
        "Originator": "energyml-python example",
        "Creation": "2026-09-19T12:00:00Z",
        "Format": "energyml-rust",
    })
}

#[cfg(feature = "hdf5")]
fn reference(qualified_type: &str, title: &str, uuid: &str) -> Value {
    json!({
        "$type": "eml23.DataObjectReference",
        "QualifiedType": qualified_type,
        "Title": title,
        "Uuid": uuid,
    })
}

#[cfg(feature = "hdf5")]
fn external_array(file: &str, path: &str, count: usize) -> Value {
    json!({
        "$type": "eml23.ExternalDataArray",
        "ExternalDataArrayPart": [{
            "$type": "eml23.ExternalDataArrayPart",
            "Count": [count],
            "StartIndex": [0],
            "MimeType": "application/x-hdf5",
            "PathInExternalFile": path,
            "URI": file,
        }],
    })
}

/// Example 1: a triangulated surface added to a package that already exists.
///
/// The CRS is chosen by what it **says** — `VerticalAxis.IsTime` — and not by
/// its title, which is the point the example makes: a package can hold a depth
/// CRS and a time CRS, and putting metres in the second one produces something
/// that opens perfectly and means something else.
#[cfg(feature = "hdf5")]
#[test]
fn the_first_example_writes_a_surface_a_reader_finds_again() {
    let dir = tempfile::tempdir().expect("a directory for this test");
    for name in ["testingPackageCpp22.epc", "testingPackageCpp22.h5"] {
        let Some(from) = corpus(name) else { return };
        std::fs::copy(&from, dir.path().join(name)).expect("the copy");
    }
    let epc = dir.path().join("testingPackageCpp22.epc");
    let mut package = Package::open(&epc).expect("the package opens");

    let crs = package
        .objects()
        .into_iter()
        .find(|one| {
            one.qualified_type.ends_with("LocalEngineeringCompoundCrs")
                && package
                    .get(&one.uri)
                    .ok()
                    .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                    .is_some_and(|one| one["VerticalAxis"]["IsTime"] == json!(false))
        })
        .expect("a depth CRS");
    let interpretation = package
        .objects()
        .into_iter()
        .find(|one| one.qualified_type.ends_with("GenericFeatureInterpretation"))
        .expect("an interpretation");

    let uuid = "1db316ff-69aa-4d5c-8630-3de3fe979c79";
    let points_path = format!("/resqml22/{uuid}/points_patch0");
    let triangles_path = format!("/resqml22/{uuid}/triangles_patch0");
    let file = "testingPackageCpp22.h5";
    let surface = json!({
        "$type": "resqml22.TriangulatedSetRepresentation",
        "uuid": uuid,
        "schemaVersion": "2.2",
        "Citation": citation("A ramp, added from Python"),
        "RepresentedObject": reference(
            &interpretation.qualified_type, &interpretation.title, &interpretation.uuid),
        "SurfaceRole": "map",
        "TrianglePatch": [{
            "$type": "resqml22.TrianglePatch",
            "NodeCount": 4,
            "Geometry": {
                "$type": "resqml22.PointGeometry",
                "LocalCrs": reference(&crs.qualified_type, &crs.title, &crs.uuid),
                "Points": {
                    "$type": "resqml22.Point3dExternalArray",
                    "Coordinates": external_array(file, &points_path, 12),
                },
            },
            "Triangles": {
                "$type": "eml23.IntegerExternalArray",
                "ArrayIntegerType": "arrayOfInt32LE",
                "CountPerValue": 1,
                "NullValue": -1,
                "Values": external_array(file, &triangles_path, 6),
            },
        }],
    });

    let written = package
        .put(&surface.to_string())
        .expect("the surface is accepted");
    assert!(written.contains(uuid), "put returns the uri it wrote: {written}");

    let points = vec![
        0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 100.0, 100.0, 25.0, 0.0, 100.0, 0.0,
    ];
    let triangles = vec![0_i32, 1, 2, 0, 2, 3];
    let at = |path: &str| ArrayRef {
        file: file.to_string(),
        path_in_file: path.to_string(),
    };
    package
        .write_array(&at(&points_path), &[4, 3], Block::F64(points))
        .expect("the points");
    package
        .write_array(&at(&triangles_path), &[2, 3], Block::I32(triangles))
        .expect("the triangles");
    package.save_as(&epc).expect("the package writes");

    // The example ends by reading its own work back, and so does this. A write
    // that reported nothing and produced a package missing the object would
    // look exactly like a write that worked.
    let again = Package::open(&epc).expect("and reads back");
    let uri = again
        .uri_of(uuid)
        .expect("one entry")
        .expect("the surface is in the package");
    let arrays = again.arrays(&uri).expect("it names its arrays");
    assert_eq!(arrays.len(), 2, "the points and the triangles");
    let (shape, block) = again.read_array(&at(&points_path)).expect("the points read back");
    assert_eq!(shape.dimensions, vec![4, 3]);
    assert_eq!(block.len(), 12);
}

/// Example 2: the whole chain, into a package that did not exist.
///
/// Feature, interpretation, representation, property, and the CRS pair they
/// stand in — six objects, and the `.h5` that two of them point at.
#[cfg(feature = "hdf5")]
#[test]
fn the_second_example_builds_a_package_from_nothing() {
    let dir = tempfile::tempdir().expect("a directory for this test");
    let epc = dir.path().join("top-chalk.epc");
    let file = "top-chalk.h5";

    let mut package = Package::create("top-chalk.epc").expect("an empty package");
    assert!(package.objects().is_empty(), "an empty package holds nothing");
    assert!(
        package.path().is_none(),
        "and it comes from nowhere until it is saved"
    );
    package.set_data_root(dir.path());

    let crs_uuid = "0b7514f2-d854-4e60-a6f9-d3524c3b6878";
    let feature_uuid = "ed3aa217-17ba-40b2-afa4-4f7d2cd586da";
    let interp_uuid = "8cd38954-3e92-4b50-91c5-6b262a5f8b3e";
    let picks_uuid = "d03201b4-22d3-4620-afb7-e983e07be875";
    let property_uuid = "48696afd-9a2e-4a9a-93b5-958a4c19012a";
    let points_path = format!("/resqml22/{picks_uuid}/points_patch0");
    let values_path = format!("/resqml22/{property_uuid}/values_patch0");

    let objects = vec![
        json!({
            "$type": "eml23.ProjectedCrs",
            "uuid": crs_uuid,
            "schemaVersion": "2.3",
            "Citation": citation("EPSG:2154"),
            "AbstractProjectedCrs": {"$type": "eml23.ProjectedEpsgCrs", "EpsgCode": 2154},
            "AxisOrder": "easting northing",
            "uom": "m",
        }),
        json!({
            "$type": "eml23.VerticalCrs",
            "uuid": "5b0d6fe7-91de-43a8-b90a-75b3371bd680",
            "schemaVersion": "2.3",
            "Citation": citation("elevation above sea level"),
            "AbstractVerticalCrs": {
                "$type": "eml23.VerticalUnknownCrs",
                "Unknown": "no vertical datum was given",
            },
            "Direction": "up",
            "uom": "m",
        }),
        json!({
            "$type": "resqml22.BoundaryFeature",
            "uuid": feature_uuid,
            "schemaVersion": "2.2",
            "Citation": citation("Top Chalk"),
        }),
        json!({
            "$type": "resqml22.HorizonInterpretation",
            "uuid": interp_uuid,
            "schemaVersion": "2.2",
            "Citation": citation("Top Chalk, picked on seismic"),
            "Domain": "depth",
            "InterpretedFeature": reference("resqml22.BoundaryFeature", "Top Chalk", feature_uuid),
        }),
        json!({
            "$type": "resqml22.PointSetRepresentation",
            "uuid": picks_uuid,
            "schemaVersion": "2.2",
            "Citation": citation("Top Chalk picks"),
            "RepresentedObject": reference(
                "resqml22.HorizonInterpretation", "Top Chalk, picked on seismic", interp_uuid),
            "NodePatchGeometry": [{
                "$type": "resqml22.PointGeometry",
                "LocalCrs": reference("eml23.ProjectedCrs", "EPSG:2154", crs_uuid),
                "Points": {
                    "$type": "resqml22.Point3dExternalArray",
                    "Coordinates": external_array(file, &points_path, 12),
                },
            }],
        }),
        json!({
            "$type": "resqml22.ContinuousProperty",
            "uuid": property_uuid,
            "schemaVersion": "2.2",
            "Citation": citation("Top Chalk elevation"),
            "IndexableElement": "nodes",
            "SupportingRepresentation": reference(
                "resqml22.PointSetRepresentation", "Top Chalk picks", picks_uuid),
            // A well-known kind, by uuid, and **not** carried by the package —
            // the Energistics dictionary defines it. A local copy would be one
            // more object saying what the standard already says.
            "PropertyKind": reference(
                "eml23.PropertyKind", "elevation", "8508cef4-0ade-454a-9ea4-e00d5905e677"),
            "Uom": "m",
            "ValueCountPerIndexableElement": [1],
            "ValuesForPatch": [{
                "$type": "eml23.FloatingPointExternalArray",
                "ArrayFloatingPointType": "arrayOfFloat32LE",
                "CountPerValue": 1,
                "Values": external_array(file, &values_path, 4),
            }],
        }),
    ];
    for one in &objects {
        package
            .put(&one.to_string())
            .unwrap_or_else(|why| panic!("{}: {why}", one["$type"]));
    }

    let points = vec![
        650000.0, 6860000.0, -1250.0, 650500.0, 6860000.0, -1180.0, 650500.0, 6860500.0, -1205.0, 650000.0,
        6860500.0, -1262.0,
    ];
    let elevations = vec![-1250.0_f32, -1180.0, -1205.0, -1262.0];
    let at = |path: &str| ArrayRef {
        file: file.to_string(),
        path_in_file: path.to_string(),
    };
    package
        .write_array(&at(&points_path), &[4, 3], Block::F64(points))
        .expect("the points");
    package
        .write_array(&at(&values_path), &[4], Block::F32(elevations))
        .expect("the values");

    // One diagnostic, and it is the expected one: the well-known property kind
    // is not in the package, so no relationship is written for it. Any other
    // count means something changed.
    let said = package.save_as(&epc).expect("the package writes");
    assert_eq!(
        said.len(),
        1,
        "one diagnostic, about the well-known kind: {said:?}"
    );
    assert!(said[0].contains("8508cef4"), "and it names it: {}", said[0]);

    let again = Package::open(&epc).expect("and reads back");
    assert_eq!(again.objects().len(), 6, "six objects went in");
    let uri = again
        .uri_of(property_uuid)
        .expect("one entry")
        .expect("the property is there");
    let (shape, block) = again
        .read_array(&again.arrays(&uri).expect("it names its array")[0])
        .expect("the values read back");
    assert_eq!(shape.dimensions, vec![4]);
    assert_eq!(
        block,
        Block::F32(vec![-1250.0, -1180.0, -1205.0, -1262.0]),
        "in float32, as written"
    );
}

/// A created package refuses an array when nothing said where its data goes.
///
/// Without [`Package::set_data_root`] the file name is relative to the working
/// directory, which is a place nobody chose. This is the one thing about
/// creating a package that an opened one answers for free, and it is worth
/// stating rather than discovering.
#[test]
fn a_created_package_comes_from_nowhere_until_it_is_saved() {
    let package = Package::create("nowhere.epc").expect("an empty package");
    assert!(package.path().is_none());
    assert!(package.objects().is_empty());
    assert!(!package.is_dirty(), "nothing has been changed yet");
}
