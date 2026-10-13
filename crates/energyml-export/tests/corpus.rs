//! Exporting the reference packages, and reading back what was written.
//!
//! An exporter is easy to get subtly wrong in ways that still produce a file:
//! OBJ indices are 1-based and file-global, GeoJSON rings must close, and a
//! format that silently skips what it does not understand looks complete. So
//! this writes the corpus out and parses the result rather than trusting it.

mod fixtures;

#[cfg(feature = "hdf5")]
use std::path::{Path, PathBuf};
#[cfg(feature = "hdf5")]
use std::sync::Arc;

#[cfg(feature = "hdf5")]
use energyml_core::registry::Registry;
use energyml_export::{Exporter, Exporters, Options};
use energyml_geometry::Placement;
#[cfg(feature = "hdf5")]
use energyml_geometry::{geometry, scene};
#[cfg(feature = "hdf5")]
use energyml_storage::EpcStore;

#[cfg(feature = "hdf5")]
/// Every schema version the workspace generates.
///
/// One line rather than a hand-kept subset: a list written out here goes stale
/// the day a release is added, and nothing would notice - the suite would keep
/// passing while quietly never covering the new version. `energyml-schemas`
/// holds the one list, and its own test fails when it disagrees with
/// `codegen.toml`.
fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

#[cfg(feature = "hdf5")]
/// The public reference packages; see `fixtures/mod.rs`.
fn corpus() -> Vec<PathBuf> {
    crate::fixtures::public_corpus()
}

#[cfg(feature = "hdf5")]
fn scene_of(path: &Path) -> Vec<energyml_geometry::SceneObject> {
    let store = EpcStore::open(path, registry()).expect("the package opens");
    let mut objects = Vec::new();
    for summary in store.resources() {
        let Ok(object) = store.get_object(&summary.uri) else {
            continue;
        };
        if !geometry::is_supported(object.as_reflect()) {
            continue;
        }
        if let Ok(built) = scene::object_of(object.as_reflect(), &store, &summary.uri) {
            objects.push(built);
        }
    }
    // Time-domain representations cannot be placed in metres - the vertical
    // axis is travel time, and `Placement::Metres` refuses them by name, which
    // is correct and exactly what these tests must not trip over. ALWYN
    // carries a seismic model in a `LocalTime3dCrs`; a metres-based exporter
    // legitimately has nothing to say about it.
    objects.retain(|o| {
        o.crs
            .as_ref()
            .is_none_or(|crs| crs.vertical_quantity != energyml_geometry::VerticalQuantity::Time)
    });
    objects
}

#[test]
#[cfg(feature = "hdf5")]
fn obj_comes_out_parseable() {
    let files = corpus();
    if files.is_empty() {
        eprintln!("skipped: no reference package (scripts/fetch-fixtures.py)");
        return;
    }
    let exporters = Exporters::builtin();
    let obj = exporters.get("obj").expect("the built-in obj exporter");
    let mut written = 0usize;
    let mut faces = 0usize;

    for path in &files {
        let objects = scene_of(path);
        if objects.is_empty() {
            continue;
        }
        let mut buffer = Vec::new();
        obj.write_all(&objects, &mut buffer, &Options::new())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let text = String::from_utf8(buffer).expect("OBJ is text");

        // Parse it back the way a viewer would, and check the one thing that is
        // easy to get wrong and impossible to see: indices are 1-based and run
        // across the whole file, not per object.
        let vertices = text.lines().filter(|l| l.starts_with("v ")).count();
        assert!(vertices > 0, "{}: no vertices written", path.display());
        for line in text.lines() {
            let Some(rest) = line.strip_prefix("f ") else {
                continue;
            };
            faces += 1;
            for index in rest.split_whitespace() {
                let index: usize = index.parse().expect("a face index");
                assert!(
                    index >= 1,
                    "{}: OBJ indices are 1-based, got {index}",
                    path.display()
                );
                assert!(
                    index <= vertices,
                    "{}: face index {index} past the {vertices} vertices in the file",
                    path.display()
                );
            }
        }
        for line in text.lines() {
            let Some(rest) = line.strip_prefix("l ") else {
                continue;
            };
            for index in rest.split_whitespace() {
                let index: usize = index.parse().expect("a line index");
                assert!(
                    index >= 1 && index <= vertices,
                    "{}: line index {index}",
                    path.display()
                );
            }
        }
        // The identity has to survive, or the file cannot be traced back.
        assert!(
            text.contains("# uuid "),
            "{}: no uuid in the output",
            path.display()
        );
        written += objects.len();
    }

    eprintln!("{written} object(s) exported to OBJ, {faces} face(s), all indices in range");
    assert!(written > 0);
    assert!(faces > 0, "no triangles were written at all");
}

/// GeoJSON refuses rather than writing projected metres as longitude.
#[test]
#[cfg(feature = "hdf5")]
fn geojson_refuses_without_a_reprojector() {
    let files = corpus();
    if files.is_empty() {
        return;
    }
    let exporters = Exporters::builtin();
    let geojson = exporters.get("geojson").expect("the built-in geojson exporter");
    let mut refused = 0usize;

    for path in &files {
        for object in scene_of(path) {
            if !geojson.handles(&object) {
                continue;
            }
            let mut buffer = Vec::new();
            let error = geojson
                .write(&object, &mut buffer, &Options::new())
                .expect_err("no backend, so it must refuse");
            let message = format!("{error}");
            // The message has to say what is missing and why it matters, or the
            // next person will "fix" it by writing the metres.
            assert!(message.contains("Reprojector"), "{message}");
            refused += 1;
        }
    }
    eprintln!("{refused} object(s) correctly refused GeoJSON without a geodesy backend");
    assert!(
        refused > 0,
        "no object reached the refusal; the test proves nothing"
    );
}

/// With a backend plugged in, the document parses and the identity is there.
#[test]
#[cfg(feature = "hdf5")]
fn geojson_writes_a_document_when_it_can() {
    let files = corpus();
    if files.is_empty() {
        return;
    }

    // A stand-in for a geodesy library: it does not compute a real datum shift,
    // and says so. What is under test here is the document, not the geodesy -
    // the real conversion is `proj4rs`'s job, behind this same trait.
    struct Pretend;
    impl energyml_domain::reproject::Reprojector for Pretend {
        fn reproject(&self, point: [f64; 3], _from: u64, _to: u64) -> energyml_domain::Result<[f64; 3]> {
            // Something in the shape of degrees, so the output is checkable.
            Ok([point[0] / 100_000.0, point[1] / 100_000.0, point[2]])
        }
    }

    let exporters = Exporters::builtin();
    let geojson = exporters.get("geojson").unwrap();
    let backend = Pretend;
    let options = Options::new().with_reprojector(&backend);
    let mut features = 0usize;

    for path in &files {
        let objects: Vec<_> = scene_of(path)
            .into_iter()
            .filter(|o| geojson.handles(o))
            .collect();
        if objects.is_empty() {
            continue;
        }
        let mut buffer = Vec::new();
        geojson
            .write_all(&objects, &mut buffer, &options)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let text = String::from_utf8(buffer).expect("GeoJSON is text");

        // One enclosing collection, not several concatenated - the mistake the
        // `write_all` override exists to prevent.
        assert_eq!(
            text.matches("\"FeatureCollection\"").count(),
            1,
            "{}: several collections in one file",
            path.display()
        );
        assert!(
            text.contains("\"uuid\""),
            "{}: no identity in the output",
            path.display()
        );
        // Brackets must balance, which is the cheapest proof that the document
        // is not truncated halfway through.
        assert_eq!(
            text.matches('[').count(),
            text.matches(']').count(),
            "{}: unbalanced brackets",
            path.display()
        );
        assert_eq!(text.matches('{').count(), text.matches('}').count());
        features += objects.len();
    }

    eprintln!("{features} feature(s) written to GeoJSON");
    assert!(features > 0);
}

/// The registry is what makes a format addable, so it gets its own test.
#[test]
fn a_format_can_be_added_without_touching_the_crate() {
    struct Xyz;
    impl energyml_export::Exporter for Xyz {
        fn name(&self) -> &'static str {
            "xyz"
        }
        fn extension(&self) -> &'static str {
            "xyz"
        }
        fn mime(&self) -> &'static str {
            "text/plain"
        }
        fn describe(&self) -> &'static str {
            "one point per line"
        }
        fn placement(&self) -> Placement {
            Placement::Local
        }
        fn handles(&self, object: &energyml_geometry::SceneObject) -> bool {
            !object.geometry.is_empty()
        }
        fn write(
            &self,
            object: &energyml_geometry::SceneObject,
            out: &mut dyn std::io::Write,
            options: &Options<'_>,
        ) -> energyml_export::Result<()> {
            let placement = options.placement.unwrap_or_else(|| self.placement());
            for point in object.points_in(placement)? {
                writeln!(out, "{} {} {}", point[0], point[1], point[2])?;
            }
            Ok(())
        }
    }

    let mut exporters = Exporters::builtin();
    let before = exporters.len();
    exporters.push(Box::new(Xyz));
    assert_eq!(exporters.len(), before + 1);
    assert_eq!(exporters.get("xyz").unwrap().name(), "xyz");
    // Found by extension too, and a leading dot is forgiven.
    assert_eq!(exporters.get(".XYZ").unwrap().name(), "xyz");
    // And the built-ins still work.
    assert_eq!(exporters.get("obj").unwrap().name(), "obj");
    // An unknown format names what there is rather than failing blankly.
    let error = match exporters.get("dxf") {
        Ok(_) => panic!("`dxf` should not be in the registry"),
        Err(e) => format!("{e}"),
    };
    assert!(error.contains("obj") && error.contains("xyz"), "{error}");
}

// ---------------------------------------------------------------------------
// Metadata and VTK
// ---------------------------------------------------------------------------

/// A dotted path crosses references into other objects.
///
/// The reason the metadata module exists: the name a person recognises for a
/// horizon is on the *feature*, which is two references away from the
/// representation. Hard-coding that chain works until somebody wants a fourth
/// thing, so it is configuration - and this proves the configuration reaches
/// through both hops on real files.
#[test]
#[cfg(feature = "hdf5")]
fn a_path_reaches_through_two_references() {
    let Some(package) = fixtures::find("testingPackageCpp.epc") else {
        eprintln!("skipped: the fesapi package is not in the fixtures");
        return;
    };
    let spec = energyml_domain::metadata::Spec::from_json(
        r#"{
            "uuid": "uuid",
            "title": "Citation.Title",
            "contentType": "contentType",
            "cached": "RepresentedInterpretation.Title",
            "resolved": "RepresentedInterpretation.Citation.Title",
            "feature": "RepresentedInterpretation.InterpretedFeature.Citation.Title",
            "nowhere": "Citation.NoSuchFieldAtAll"
        }"#,
    )
    .expect("the spec parses");

    let store = EpcStore::open(&package, registry()).expect("the package opens");
    let mut reached_the_feature = 0usize;
    let mut cached_and_resolved_agree = 0usize;

    for summary in store.resources() {
        let Ok(object) = store.get_object(&summary.uri) else {
            continue;
        };
        if !geometry::is_supported(object.as_reflect()) {
            continue;
        }
        let Ok(built) = scene::object_of_with(
            energyml_geometry::readers::default_registry(),
            object.as_reflect(),
            &store,
            &summary.uri,
            Some(&spec),
        ) else {
            continue;
        };

        // Identity always resolves.
        assert_eq!(built.metadata.get("uuid"), Some(built.uuid.as_str()));
        assert!(
            built
                .metadata
                .get("contentType")
                .is_some_and(|t| t.contains("resqml"))
        );
        // A path that leads nowhere is a miss, never a wrong value.
        assert_eq!(built.metadata.get("nowhere"), None);
        assert!(built.metadata.misses().contains(&"nowhere".to_string()));

        if let Some(feature) = built.metadata.get("feature") {
            assert!(!feature.is_empty());
            reached_the_feature += 1;
        }
        // The reference caches a title and the target has its own. They should
        // agree in a well-formed package - and the point of supporting both is
        // that the cheap one is free while the other costs a read.
        if let (Some(cached), Some(resolved)) = (built.metadata.get("cached"), built.metadata.get("resolved"))
        {
            assert_eq!(cached, resolved, "{}: the cached title has drifted", built.uuid);
            cached_and_resolved_agree += 1;
        }
    }

    eprintln!("{reached_the_feature} object(s) resolved a feature title two references away");
    eprintln!("{cached_and_resolved_agree} where the reference's cached title matched the object's");
    assert!(
        reached_the_feature > 0,
        "no path crossed two references; the feature chain is not being followed"
    );
    assert!(cached_and_resolved_agree > 0);
}

/// Every VTK flavour writes a document with its metadata in it.
#[test]
#[cfg(feature = "hdf5")]
fn every_vtk_flavour_writes_itself() {
    let files = corpus();
    if files.is_empty() {
        return;
    }
    let exporters = Exporters::builtin();
    let mut written = 0usize;

    for path in &files {
        let objects = scene_of(path);
        let Some(object) = objects.iter().find(|o| !o.geometry.is_empty()) else {
            continue;
        };
        for (name, marker) in [
            ("vtk", "# vtk DataFile Version"),
            ("vtp", "type=\"PolyData\""),
            ("vtu", "type=\"UnstructuredGrid\""),
        ] {
            let exporter = exporters.get(name).expect("a built-in vtk flavour");
            let mut buffer = Vec::new();
            exporter
                .write(object, &mut buffer, &Options::new())
                .unwrap_or_else(|e| panic!("{}: {name} - {e}", path.display()));
            let text = String::from_utf8(buffer).expect("VTK ASCII is text");
            assert!(text.contains(marker), "{name}: no `{marker}` in the output");
            // The identity has to be there whether or not a spec was supplied:
            // a file that cannot be traced back is the failure this layer
            // exists to prevent.
            assert!(text.contains(&object.uuid), "{name}: no uuid in the output");
            if name != "vtk" {
                // The XML flavours must balance their tags, which is the
                // cheapest proof the document is not truncated.
                assert_eq!(
                    text.matches("<DataArray").count(),
                    text.matches("</DataArray>").count(),
                    "{name}: unbalanced DataArray tags"
                );
                assert!(text.contains("</VTKFile>"), "{name}: no closing tag");
            }
            written += 1;
        }
    }
    eprintln!("{written} VTK document(s) written across three flavours");
    assert!(written >= 3);
}

/// A VTK file holds one dataset, and saying so beats writing a broken one.
#[test]
#[cfg(feature = "hdf5")]
fn vtk_refuses_several_objects_in_one_file() {
    let files = corpus();
    if files.is_empty() {
        return;
    }
    let exporters = Exporters::builtin();
    let vtk = exporters.get("vtk").unwrap();
    for path in &files {
        let objects: Vec<_> = scene_of(path).into_iter().filter(|o| vtk.handles(o)).collect();
        if objects.len() < 2 {
            continue;
        }
        let mut buffer = Vec::new();
        let error = vtk
            .write_all(&objects, &mut buffer, &Options::new())
            .expect_err("several datasets in one legacy VTK is not a thing");
        assert!(format!("{error}").contains("one dataset"), "{error}");
        return;
    }
    panic!("no package had two exportable objects; the test proves nothing");
}

/// The file-format version is what the file claims to be.
#[test]
fn a_vtk_version_can_be_chosen() {
    use energyml_export::vtk::VtkExporter;
    // A reader older than the version it is handed refuses the file, so this is
    // not cosmetic.
    let old = VtkExporter::poly_data().with_version("0.1");
    assert_eq!(old.name(), "vtp");
    let new = VtkExporter::poly_data().with_version("2.2");
    assert_eq!(new.extension(), "vtp");
    assert_eq!(VtkExporter::legacy().name(), "vtk");
    assert_eq!(VtkExporter::unstructured_grid().name(), "vtu");
}

/// A hand-built object whose CRS actually moves things.
///
/// The corpus cannot test this: every fesapi package has a CRS origin of
/// `(0, 0)`, no rotation and metres throughout, so placing the points twice
/// gives exactly the same answer as placing them once. That is precisely the
/// shape of bug that survives a full-corpus test suite.
fn object_with_an_offset_crs() -> energyml_geometry::SceneObject {
    use energyml_domain::crs::{Crs, CrsKind, VerticalDirection, VerticalQuantity};
    use energyml_geometry::{Geometry, Patch, SceneObject, Topology};

    SceneObject {
        uuid: "offset-crs".into(),
        uri: "eml:///resqml22.PointSetRepresentation(offset-crs)".into(),
        title: "one point, a long way from the origin".into(),
        qualified_type: "resqml22.PointSetRepresentation".into(),
        geometry: Geometry {
            kind: "PointSetRepresentation",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points: vec![[10.0, 20.0, 0.0]],
                topology: Topology::Points,
                closed: Vec::new(),
            }],
        },
        crs: Some(Crs {
            uuid: "crs".into(),
            kind: "LocalDepth3dCrs",
            x_offset: 500_000.0,
            y_offset: 4_650_000.0,
            z_offset: 0.0,
            areal_rotation: 0.0,
            vertical_direction: VerticalDirection::Up,
            vertical_quantity: VerticalQuantity::Length,
            horizontal_uom: None,
            vertical_uom: None,
            time_uom: None,
            projected: CrsKind::Epsg(23031),
            vertical: CrsKind::Absent,
        }),
        interpretation: None,
        feature: None,
        metadata: Default::default(),
        attributes: Vec::new(),
    }
}

#[test]
fn geojson_places_the_points_once_and_not_twice() {
    use std::sync::Mutex;

    // Records what the backend was handed, which is the only place the bug was
    // visible: the document that came out was well-formed either way.
    struct Recorder(Mutex<Vec<[f64; 3]>>);
    impl energyml_domain::reproject::Reprojector for Recorder {
        fn reproject(&self, point: [f64; 3], _from: u64, _to: u64) -> energyml_domain::Result<[f64; 3]> {
            self.0.lock().unwrap().push(point);
            Ok([point[0] / 100_000.0, point[1] / 100_000.0, point[2]])
        }
    }

    let backend = Recorder(Mutex::new(Vec::new()));
    let options = Options::new().with_reprojector(&backend);
    let exporters = Exporters::builtin();
    let mut buffer = Vec::new();
    exporters
        .get("geojson")
        .unwrap()
        .write(&object_with_an_offset_crs(), &mut buffer, &options)
        .unwrap();

    let seen = backend.0.lock().unwrap().clone();
    assert_eq!(seen.len(), 1, "{seen:?}");
    // Placed once: 500000 + 10. Placed twice it would be 1000010, and the
    // feature would land in the Sahara rather than the Mediterranean.
    assert_eq!(
        seen[0],
        [500_010.0, 4_650_020.0, 0.0],
        "the CRS was applied twice"
    );
}

#[test]
fn a_target_epsg_reaches_the_other_formats_too() {
    // `--epsg` is not GeoJSON-only: OBJ and VTK take it as well, which is what
    // routing every exporter through `Options::points` buys.
    struct Fixed;
    impl energyml_domain::reproject::Reprojector for Fixed {
        fn reproject(&self, point: [f64; 3], from: u64, to: u64) -> energyml_domain::Result<[f64; 3]> {
            assert_eq!((from, to), (23031, 3857));
            Ok([point[0] + 1.0, point[1] + 2.0, point[2]])
        }
    }

    let backend = Fixed;
    let options = Options::new().with_reprojector(&backend).with_target_epsg(3857);
    let exporters = Exporters::builtin();
    let mut buffer = Vec::new();
    exporters
        .get("obj")
        .unwrap()
        .write(&object_with_an_offset_crs(), &mut buffer, &options)
        .unwrap();
    let text = String::from_utf8(buffer).unwrap();
    assert!(text.contains("v 500011 4650022 0"), "{text}");
}

#[test]
fn a_target_epsg_without_a_backend_is_refused_by_name() {
    let options = Options::new().with_target_epsg(3857);
    let exporters = Exporters::builtin();
    let mut buffer = Vec::new();
    let error = exporters
        .get("obj")
        .unwrap()
        .write(&object_with_an_offset_crs(), &mut buffer, &options)
        .unwrap_err();
    let message = format!("{error}");
    assert!(message.contains("Reprojector"), "{message}");
    assert!(message.contains("23031") && message.contains("3857"), "{message}");
}

#[cfg(feature = "hdf5")]
#[test]
fn the_corpus_goes_out_as_geojson_through_the_real_backend() {
    // End to end with `energyml_proj::Proj4`, no stand-in: read the packages,
    // place, reproject, write, parse the result, and convert every position
    // back to check it lands where it started.
    let files = corpus();
    if files.is_empty() {
        return;
    }
    let backend = energyml_proj::Proj4::new();
    let options = Options::new().with_reprojector(&backend);
    let exporters = Exporters::builtin();
    let geojson = exporters.get("geojson").unwrap();
    let mut positions = 0usize;
    let mut worst = 0.0f64;

    for path in &files {
        let objects: Vec<_> = scene_of(path)
            .into_iter()
            .filter(|o| geojson.handles(o))
            .collect();
        if objects.is_empty() {
            continue;
        }
        let mut buffer = Vec::new();
        geojson
            .write_all(&objects, &mut buffer, &options)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));

        for object in &objects {
            let Some(source) = object.epsg() else { continue };
            let placed = object.points_in(Placement::Metres).unwrap();
            let out = options.points_in_epsg(object, 4326).unwrap();
            assert_eq!(placed.len(), out.len());

            // Longitude and latitude, not eastings. The corpus sits at easting
            // 0-750 m in EPSG:23031 - which really is off the coast of Africa,
            // because these packages carry no georeferencing at all - so a
            // range check would prove nothing. The round trip does.
            let back =
                energyml_domain::reproject::Reprojector::reproject_all(&backend, &out, 4326, source).unwrap();
            for (before, after) in placed.iter().zip(&back) {
                if !(before[0].is_finite() && before[1].is_finite()) {
                    continue; // sentinel coordinates cannot round-trip meaningfully
                }
                let offset = (before[0] - after[0]).hypot(before[1] - after[1]);
                worst = worst.max(offset);
                assert!(
                    offset < 0.01,
                    "{}: {before:?} came back as {after:?}",
                    path.display()
                );
                // And the depth is untouched by any of it - bitwise, because
                // real files carry NaN as an "absent" sentinel and NaN != NaN.
                assert_eq!(before[2].to_bits(), after[2].to_bits());
            }
            positions += placed.len();
        }
    }

    eprintln!("{positions} position(s) reprojected and returned, worst {worst:.6} m");
    assert!(positions > 0);
}

#[test]
fn native_mode_writes_what_wgs84_mode_refuses() {
    // The escape hatch for a package under repair: no reprojector, no EPSG
    // code - and still a file one can look at. What the rule forbids is
    // *silent* wrongness; this is asked for and labelled.
    let mut orphan = object_with_an_offset_crs();
    orphan.crs = None; // no CRS at all - the 21 BRGM PointSets

    // Default mode: not handled, and refused by name if forced through.
    let exporters = Exporters::builtin();
    let geojson = exporters.get("geojson").unwrap();
    let strict = Options::new();
    assert!(!geojson.handles_with(&orphan, &strict));
    let mut buffer = Vec::new();
    assert!(geojson.write(&orphan, &mut buffer, &strict).is_err());

    // Native mode: written, as stored.
    let native = Options::new().with_native_crs();
    assert!(geojson.handles_with(&orphan, &native));
    let mut buffer = Vec::new();
    geojson.write(&orphan, &mut buffer, &native).unwrap();
    let text = String::from_utf8(buffer).unwrap();
    assert!(text.contains("\"FeatureCollection\""));
    // No CRS anywhere → no legacy member: the file honestly says nothing.
    assert!(!text.contains("\"crs\":{"), "{text}");
    // The local coordinates, untouched: no CRS means no placement either.
    assert!(text.contains("10") && text.contains("20"), "{text}");
}

#[test]
fn native_mode_labels_the_file_with_the_shared_epsg() {
    let exporters = Exporters::builtin();
    let geojson = exporters.get("geojson").unwrap();
    let native = Options::new().with_native_crs();

    let mut buffer = Vec::new();
    geojson
        .write(&object_with_an_offset_crs(), &mut buffer, &native)
        .unwrap();
    let text = String::from_utf8(buffer).unwrap();
    // The legacy 2008 member RFC 7946 removed - QGIS and GDAL honour it.
    assert!(text.contains("urn:ogc:def:crs:EPSG::23031"), "{text}");
    // And the coordinates are the *placed* ones, not reprojected: the CRS
    // offsets applied, no geodesy involved.
    assert!(text.contains("500010"), "{text}");
}

#[test]
fn native_mode_refuses_no_label_over_a_wrong_one() {
    // Two objects naming two different systems: a single `crs` member would
    // be wrong for one of them, so none is written.
    let one = object_with_an_offset_crs();
    let mut other = object_with_an_offset_crs();
    other.uuid = "other".into();
    if let Some(crs) = &mut other.crs {
        crs.projected = energyml_geometry::CrsKind::Epsg(32631);
    }
    let exporters = Exporters::builtin();
    let geojson = exporters.get("geojson").unwrap();
    let native = Options::new().with_native_crs();
    let mut buffer = Vec::new();
    geojson.write_all(&[one, other], &mut buffer, &native).unwrap();
    let text = String::from_utf8(buffer).unwrap();
    assert!(!text.contains("urn:ogc:def:crs"), "{text}");
}
