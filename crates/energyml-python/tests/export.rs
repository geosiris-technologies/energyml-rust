//! Writing a representation out, and the formats there are.
//!
//! The bridge does not write anything: `energyml-export` does, through its
//! registry. What is checked here is that the door leads to it — and that what
//! comes out agrees with what the *geometry* reader says of the same object,
//! which is a second path over the same file.

#![cfg(feature = "hdf5")]

use std::path::PathBuf;

use energyml_rust::session::{Package, formats};

fn corpus(name: &str) -> Option<PathBuf> {
    crate::fixtures::find_or_skip(name)
}

fn package() -> Option<Package> {
    let path = corpus("testingPackageCpp22.epc")?;
    Some(Package::open(&path).expect("the package opens"))
}

#[test]
fn the_formats_are_the_registrys_own_list() {
    let all = formats();
    assert!(all.len() >= 6, "obj, csv, geojson and the three vtk flavours: {}", all.len());
    for one in &all {
        assert!(!one.name.is_empty() && !one.extension.is_empty(), "{one:?}");
        assert!(one.mime.contains('/'), "a mime type: {}", one.mime);
        assert!(!one.describes.is_empty(), "{} says nothing about itself", one.name);
    }
    assert!(all.iter().any(|one| one.name == "obj"), "obj is in the list");
}

#[test]
fn a_surface_written_as_obj_has_the_vertices_its_geometry_has() {
    let Some(package) = package() else { return };
    let Some(one) = package.find(Some("TriangulatedSetRepresentation"), None).into_iter().next()
    else {
        eprintln!("skipped: no triangulated set in this package");
        return;
    };

    let bytes = package.export(std::slice::from_ref(&one.uri), "obj", None, false, None).expect("obj writes");
    let text = String::from_utf8(bytes).expect("OBJ is text");
    let vertices = text.lines().filter(|line| line.starts_with("v ")).count();
    let faces = text.lines().filter(|line| line.starts_with("f ")).count();

    // **The confrontation.** The exporter walked the scene object; this asks
    // the geometry reader the same question by another call. A merge that lost
    // a patch, or a triangulation that doubled one, shows up here.
    let shape = package.geometry(&one.uri, "metres").expect("the shape reads");
    let points: usize = shape.patches.iter().map(|patch| patch.points.len() / 3).sum();
    assert_eq!(vertices, points, "as many `v` lines as the shape has points");
    assert!(faces > 0, "a surface writes faces");
}

#[test]
fn the_format_is_asked_for_by_name_and_an_unknown_one_lists_what_there_is() {
    let Some(package) = package() else { return };
    let Some(one) = package.find(Some("Representation"), None).into_iter().next() else { return };

    let why = package
        .export(std::slice::from_ref(&one.uri), "dxf", None, false, None)
        .expect_err("this build writes no DXF");
    assert!(why.contains("dxf"), "the refusal names what was asked: {why}");
    assert!(why.contains("obj"), "and lists what there is: {why}");
}

#[test]
fn a_format_that_cannot_write_an_object_refuses_rather_than_skipping_it() {
    let Some(package) = package() else { return };

    // GeoJSON needs an EPSG code to put longitudes and latitudes in; a package
    // holds objects it cannot place. Whichever object that is here, the point
    // is the same: **an export that skipped it would hand back a file that
    // looks complete**, and the caller would never learn what is not in it.
    let refused = package
        .find(Some("Representation"), None)
        .into_iter()
        .find_map(|one| package.export(std::slice::from_ref(&one.uri), "geojson", None, false, None).err());
    let Some(why) = refused else {
        eprintln!("skipped: every representation of this package writes as GeoJSON");
        return;
    };
    assert!(!why.is_empty(), "a refusal carries a sentence: {why}");
}

#[test]
fn several_objects_go_into_one_document() {
    let Some(package) = package() else { return };
    let surfaces: Vec<String> = package
        .find(Some("TriangulatedSetRepresentation"), None)
        .into_iter()
        .map(|one| one.uri)
        .take(2)
        .collect();
    if surfaces.len() < 2 {
        eprintln!("skipped: fewer than two triangulated sets");
        return;
    }

    let one = package.export(&surfaces[..1], "obj", None, false, None).expect("one writes");
    let both = package.export(&surfaces, "obj", None, false, None).expect("two write");
    let count = |bytes: &[u8]| {
        String::from_utf8_lossy(bytes).lines().filter(|line| line.starts_with("v ")).count()
    };
    assert!(
        count(&both) > count(&one),
        "two objects in one document hold more than one does"
    );
}

#[test]
fn values_travel_with_the_shape_when_they_are_asked_for() {
    let Some(package) = package() else { return };

    // A representation carrying a property, in a format that writes them. The
    // difference between the two exports is the whole point: asked for, the
    // values are in the file; not asked for, they are not — and neither is the
    // cost of reading every property of the package.
    let found = package.find(Some("Representation"), None).into_iter().find_map(|one| {
        let (values, _) = package.properties(&one.uri).ok()?;
        values.first().map(|first| (one, first.name.clone()))
    });
    let Some((object, name)) = found else {
        eprintln!("skipped: no representation of this package carries a property");
        return;
    };

    let bare = package.export(std::slice::from_ref(&object.uri), "vtu", None, false, None).expect("vtu writes");
    let with = package.export(std::slice::from_ref(&object.uri), "vtu", None, true, None).expect("and with values");
    assert!(with.len() > bare.len(), "the values add bytes");
    // VTK names a field array with whitespace replaced, so the comparison is
    // made on the name **the format uses** rather than on the citation title —
    // asserting the raw title would be asserting something the file never
    // claimed.
    let in_file = name.replace(char::is_whitespace, "_");
    assert!(
        String::from_utf8_lossy(&with).contains(&in_file),
        "the property `{in_file}` is named in the file"
    );
    assert!(
        !String::from_utf8_lossy(&bare).contains(&in_file),
        "and it is not there when it was not asked for"
    );
}

#[test]
fn a_property_brings_the_colours_the_package_states() {
    // **The half this bridge could not reach until 20/09.** Resolving a ramp
    // walks the package's `GraphicalInformationSet`, so nothing reading shape
    // by shape can do it — and the bridge read shape by shape. It goes through
    // `scene::of_package` now, which is where `energyml export` got it.
    let Some(package) = package() else { return };

    let mut seen = 0usize;
    let mut with_a_ramp = 0usize;
    for one in package.find(Some("Representation"), None) {
        let Ok((values, _)) = package.properties(&one.uri) else { continue };
        for value in values {
            seen += 1;
            if value.colours.is_empty() {
                continue;
            }
            with_a_ramp += 1;
            // Four components per row, in 0-1: a ramp a caller cannot read as
            // colours is a ramp that is not there.
            for row in &value.colours {
                assert!(
                    row.iter().all(|c| (0.0..=1.0).contains(c)),
                    "`{}` carries {row:?}, which is not an RGBA row in 0-1",
                    value.name
                );
            }
        }
    }
    assert!(seen > 0, "no property at all on this package");
    assert!(
        with_a_ramp > 0,
        "not one of {seen} properties carries a colour ramp, on a package that states them"
    );
}

#[test]
fn a_metadata_spec_reaches_the_document() {
    // The other half the one door unlocked: `energyml export
    // --standard-metadata` wrote these, and nothing here could ask for them.
    let Some(package) = package() else { return };
    let Some(object) = package
        .find(Some("TriangulatedSetRepresentation"), None)
        .into_iter()
        .next()
    else {
        eprintln!("skipped: no triangulated set in this package");
        return;
    };

    let spec = energyml_rust::session::standard_metadata();
    assert!(spec.contains("qualifiedType"), "the standard spec lost its keys: {spec}");

    let bare = package
        .export(std::slice::from_ref(&object.uri), "geojson", None, false, None)
        .expect("geojson writes");
    let rich = package
        .export(std::slice::from_ref(&object.uri), "geojson", None, false, Some(&spec))
        .expect("and with the spec");
    assert!(rich.len() > bare.len(), "the metadata adds nothing to the document");
    assert!(
        String::from_utf8_lossy(&rich).contains("qualifiedType"),
        "the spec was accepted and written nowhere"
    );

    // **A spec that will not read is refused where the typo is**, before a
    // package is walked: a caller who gets bytes back has no way to learn its
    // keys were dropped.
    let why = package
        .export(std::slice::from_ref(&object.uri), "geojson", None, false, Some("{"))
        .expect_err("a broken spec is refused");
    assert!(!why.is_empty(), "the refusal says nothing");
}

#[test]
fn an_object_this_package_does_not_hold_is_refused_by_name() {
    // The walk keeps what it could evaluate, so a uri that named nothing would
    // simply be missing from the result — and a document missing an object
    // somebody asked for by name looks exactly like a complete one.
    let Some(package) = package() else { return };
    let why = package
        .export(
            &["eml:///resqml22.TriangulatedSetRepresentation(00000000-0000-4000-8000-000000000000)".to_string()],
            "obj",
            None,
            false,
            None,
        )
        .expect_err("an absent object is refused");
    assert!(
        why.contains("00000000-0000-4000-8000-000000000000"),
        "the refusal does not name what was asked for: {why}"
    );
}
