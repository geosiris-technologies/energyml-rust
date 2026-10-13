//! `export-media` — a representation written by an exporter of
//! `energyml-export` and put in the package, related to it
//!.
//!
//! The form of the assertions comes from three documents, never from what the
//! operation produced: the packaging conventions
//! (Energistics Packaging Conventions v1.0 — every
//! part typed in `[Content_Types].xml`, §2.2.1 p. 9; `destinationMedia` from
//! the object and `sourceMedia` back, §3.4.2 pp. 15–16); the reference-model rule that
//! *“the types come from the exporter, not from a second table”*
//! — the content type is compared with `mime()`, not with a
//! string written here; and the formats themselves — one `v` line per vertex
//! and one `f` per face in an OBJ, and the definition of EPSG:2154, whose
//! projection origin (lon 3°E, lat 46.5°N) is written (700 000 m, 6 600 000 m).
//!
//! What would make them fail: a second writer in place of the registry (the
//! bytes stop being the exporter's), a content type from a table (it stops
//! being `mime()`), a part related to nothing, a GeoJSON left in Lambert-93, a
//! bag route that places the numbers differently from the library's reader.

use energyml_build::operation::{Choices, Fitness, Identity, Registry, When};
use energyml_build::ops::export_media::file_name_of;
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_domain::crs::{Crs, CrsKind, VerticalDirection, VerticalQuantity};
use energyml_export::{Exporters, Options};
use energyml_geometry::geometry::{Geometry, Patch, Topology};
use energyml_geometry::{Related, SceneObject};
use serde_json::Value;

const SURFACE: &str = "c0ffee00-1111-4222-8333-444455556666";
const INTERPRETATION: &str = "c0ffee00-1111-4222-8333-777788889999";
const FRAME: &str = "c0ffee00-1111-4222-8333-aaaabbbbcccc";

fn when() -> When {
    When {
        creation: "2026-09-25T00:00:00Z".to_string(),
        originator: "tests/export_media.rs".to_string(),
    }
}

/// The frame as `energyml_domain::crs` reads a compound written by `crs` —
/// the identity over EPSG:2154 + EPSG:5720, metres, elevation.
fn lambert93() -> Crs {
    Crs {
        uuid: FRAME.to_string(),
        kind: "LocalEngineeringCompoundCrs",
        x_offset: 0.0,
        y_offset: 0.0,
        z_offset: 0.0,
        areal_rotation: 0.0,
        vertical_direction: VerticalDirection::Up,
        vertical_quantity: VerticalQuantity::Length,
        horizontal_uom: Some("m".to_string()),
        vertical_uom: Some("m".to_string()),
        time_uom: None,
        projected: CrsKind::Epsg(2154),
        vertical: CrsKind::Epsg(5720),
    }
}

fn surface_ref(title: &str, qualified_type: &str) -> Ref {
    Ref::new(SURFACE, qualified_type, title)
}

/// What the bag says of an object of the package — as `read::pieces_of_object`
/// writes it, named for the slot.
fn known(reference: Ref, placement: Option<Crs>) -> Known {
    Known {
        reference: Some(reference),
        crs: Some(Ref::new(
            FRAME,
            "eml23.LocalEngineeringCompoundCrs",
            "Lambert-93 / NGF-IGN69",
        )),
        interpretation: Some(Ref::new(INTERPRETATION, "resqml22.HorizonInterpretation", "H3")),
        placement,
        ..Default::default()
    }
}

fn bag(geometry: Geometry, known: Option<Known>) -> Pieces {
    let pieces = Pieces::empty().with(
        "shape",
        Source::Shape {
            geometry,
            closed: None,
        },
    );
    match known {
        Some(known) => pieces.with("shape (object)", Source::Object(known)),
        None => pieces,
    }
}

/// Two triangles on four nodes, near Paris.
fn two_triangles() -> Geometry {
    Geometry {
        kind: "surface",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points: vec![
                [652000.0, 6862000.0, -20.0],
                [653000.0, 6862000.0, -21.0],
                [653000.0, 6863000.0, -22.0],
                [652000.0, 6863000.0, -23.0],
            ],
            topology: Topology::Triangles(vec![[0, 1, 2], [0, 2, 3]]),
            closed: Vec::new(),
        }],
    }
}

/// A closed ring whose first node is the origin of EPSG:2154.
fn ring() -> Geometry {
    Geometry {
        kind: "polylines",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points: vec![
                [700000.0, 6600000.0, 35.0],
                [701000.0, 6600000.0, 35.0],
                [701000.0, 6601000.0, 35.0],
                [700000.0, 6601000.0, 35.0],
            ],
            topology: Topology::Polylines(vec![4]),
            closed: vec![true],
        }],
    }
}

/// One hexahedron — the shape an IJK grid is read as.
fn one_cell() -> Geometry {
    let mut points = Vec::new();
    for z in [0.0, -10.0] {
        for (x, y) in [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)] {
            points.push([652000.0 + x, 6862000.0 + y, z]);
        }
    }
    Geometry {
        kind: "ijk",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points,
            topology: Topology::Hexahedra {
                cells: vec![[0, 1, 2, 3, 4, 5, 6, 7]],
                dimensions: [1, 1, 1],
                index: Vec::new(),
            },
            closed: Vec::new(),
        }],
    }
}

fn plan(
    pieces: &Pieces,
    choices: Choices,
) -> Result<energyml_build::operation::Plan, energyml_build::operation::Error> {
    Registry::new().plan("export-media", pieces, &choices, &Identity::derived(), &when())
}

fn refusals(pieces: &Pieces, choices: Choices) -> Vec<(Option<String>, String)> {
    plan(pieces, choices)
        .expect_err("it was accepted")
        .refusals()
        .iter()
        .map(|one| (one.slot.clone(), one.message.clone()))
        .collect()
}

fn lines_starting(text: &str, head: &str) -> usize {
    text.lines().filter(|line| line.starts_with(head)).count()
}

#[test]
fn an_obj_is_the_registrys_bytes_typed_by_its_mime_and_related_to_the_object() {
    let title = "H3 — top of the Yprésien";
    let pieces = bag(
        two_triangles(),
        Some(known(
            surface_ref(title, "resqml22.TriangulatedSetRepresentation"),
            Some(lambert93()),
        )),
    );
    let plan = plan(&pieces, Choices::new().set("format", "obj")).unwrap_or_else(|e| panic!("{e}"));

    assert!(plan.objects.is_empty(), "an export is a part, not an object");
    let [media] = plan.media.as_slice() else {
        panic!("one part, got {:?}", plan.media)
    };
    let exporters = Exporters::builtin();
    let obj = exporters.get("obj").unwrap();
    assert_eq!(media.content_type, obj.mime(), "the exporter's own type");
    assert_eq!(
        media.path, "media/export/h3-top-of-the-ypresien.obj",
        "the agreed folder, the title as a file name"
    );
    assert_eq!(media.related_to, [SURFACE]);

    // The format's own grammar: a `v` per node, an `f` per triangle, and the
    // uuid in a comment — counted against the bag written above.
    let text = String::from_utf8(media.bytes.clone()).expect("an OBJ is text");
    assert_eq!(lines_starting(&text, "v "), 4, "{text}");
    assert_eq!(lines_starting(&text, "f "), 2, "{text}");
    assert!(
        text.contains(SURFACE) && text.contains(INTERPRETATION),
        "the identity travels: {text}"
    );

    // And the bytes are the registry's: the same exporter on the same object.
    let object = SceneObject {
        uuid: SURFACE.to_string(),
        uri: format!("eml:///resqml22.TriangulatedSetRepresentation({SURFACE})"),
        title: title.to_string(),
        qualified_type: "resqml22.TriangulatedSetRepresentation".to_string(),
        geometry: two_triangles(),
        crs: Some(lambert93()),
        interpretation: Some(Related {
            uuid: INTERPRETATION.to_string(),
            title: "H3".to_string(),
            qualified_type: "resqml22.HorizonInterpretation".to_string(),
        }),
        feature: None,
        metadata: Default::default(),
        attributes: Vec::new(),
    };
    let mut direct = Vec::new();
    obj.write(&object, &mut direct, &Options::new()).unwrap();
    assert_eq!(media.bytes, direct);

    // The activity says which part, of which type, from which object.
    let parameters = format!("{:?}", plan.activity.parameters);
    assert!(
        parameters.contains("media/export/h3-top-of-the-ypresien.obj") && parameters.contains(SURFACE),
        "{parameters}"
    );
}

#[test]
fn a_geojson_is_in_wgs84_and_the_origin_of_lambert93_lands_where_epsg_2154_puts_it() {
    let pieces = bag(
        ring(),
        Some(known(
            surface_ref("Lutetia — model extent", "resqml22.PolylineSetRepresentation"),
            Some(lambert93()),
        )),
    );
    let plan = plan(&pieces, Choices::new().set("format", "geojson")).unwrap_or_else(|e| panic!("{e}"));
    let [media] = plan.media.as_slice() else {
        panic!("one part")
    };
    assert_eq!(
        media.content_type,
        Exporters::builtin().get("geojson").unwrap().mime()
    );
    assert_eq!(media.path, "media/export/lutetia-model-extent.geojson");

    let document: Value = serde_json::from_slice(&media.bytes).expect("RFC 7946 is JSON first");
    assert_eq!(document["type"], "FeatureCollection");
    let first = &document["features"][0]["geometry"]["coordinates"][0][0];
    let (lon, lat) = (first[0].as_f64().unwrap(), first[1].as_f64().unwrap());
    // EPSG:2154: false easting 700 000 m and false northing 6 600 000 m at the
    // origin lon 3°E, lat 46.5°N.
    assert!(
        (lon - 3.0).abs() < 1e-7 && (lat - 46.5).abs() < 1e-7,
        "({lon}, {lat})"
    );
}

#[test]
fn a_vtu_of_a_grid_cell_is_typed_by_the_vtk_exporter() {
    let pieces = bag(
        one_cell(),
        Some(known(
            surface_ref("Ypresian–Chalk grid", "resqml22.IjkGridRepresentation"),
            Some(lambert93()),
        )),
    );
    let plan = plan(
        &pieces,
        Choices::new()
            .set("format", "vtu")
            .set("path", "media/export/grille-craie.vtu"),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let [media] = plan.media.as_slice() else {
        panic!("one part")
    };
    assert_eq!(media.path, "media/export/grille-craie.vtu", "the path given");
    assert_eq!(
        media.content_type,
        Exporters::builtin().get("vtu").unwrap().mime()
    );
    let text = String::from_utf8(media.bytes.clone()).unwrap();
    assert!(
        text.contains("NumberOfPoints=\"8\"") && text.contains("NumberOfCells=\"1\""),
        "{text}"
    );
}

#[test]
fn what_cannot_be_exported_is_refused_on_its_slot() {
    let obj = || Choices::new().set("format", "obj");
    let object = || {
        known(
            surface_ref("S", "resqml22.TriangulatedSetRepresentation"),
            Some(lambert93()),
        )
    };

    // A format the registry does not hold, and the list of what it holds.
    let unknown = refusals(
        &bag(two_triangles(), Some(object())),
        Choices::new().set("format", "gltf"),
    );
    assert!(
        unknown.iter().any(|(slot, why)| slot.as_deref() == Some("format")
            && why.contains("obj")
            && why.contains("geojson")),
        "{unknown:?}"
    );

    // An artefact is no object: unfit, and refused on its slot.
    let artefact = bag(two_triangles(), None);
    let registry = Registry::new();
    let operation = registry.get("export-media").expect("registered");
    assert!(
        matches!(operation.fitness(&artefact), Fitness::Unfit(why) if why.message.contains("no object of the package"))
    );
    let none = refusals(&artefact, obj());
    assert!(
        none.iter().any(|(slot, _)| slot.as_deref() == Some("shape")),
        "{none:?}"
    );

    // A frame named and not given — the shape was read placed.
    let placed = refusals(
        &bag(
            two_triangles(),
            Some(known(
                surface_ref("S", "resqml22.TriangulatedSetRepresentation"),
                None,
            )),
        ),
        obj(),
    );
    assert!(
        placed
            .iter()
            .any(|(slot, why)| slot.as_deref() == Some("shape") && why.contains("Lambert-93")),
        "{placed:?}"
    );

    // A part name the package keeps for itself (EPC §2.2.2 p. 9).
    let rels = refusals(
        &bag(two_triangles(), Some(object())),
        obj().set("path", "media/_rels/x.obj"),
    );
    assert!(
        rels.iter()
            .any(|(slot, why)| slot.as_deref() == Some("path") && why.contains("_rels")),
        "{rels:?}"
    );

    // A GeoJSON of a frame with no EPSG code.
    let mut unknown_frame = lambert93();
    unknown_frame.projected = CrsKind::Unknown;
    let no_code = refusals(
        &bag(
            ring(),
            Some(known(
                surface_ref("S", "resqml22.PolylineSetRepresentation"),
                Some(unknown_frame),
            )),
        ),
        Choices::new().set("format", "geojson"),
    );
    assert!(
        no_code
            .iter()
            .any(|(slot, why)| slot.as_deref() == Some("format") && why.contains("EPSG")),
        "{no_code:?}"
    );

    // A vertical axis in travel time, which an export here does not convert.
    let mut time = lambert93();
    time.vertical_quantity = VerticalQuantity::Time;
    time.vertical_uom = Some("ms".to_string());
    let in_time = refusals(
        &bag(
            two_triangles(),
            Some(known(
                surface_ref("S", "resqml22.TriangulatedSetRepresentation"),
                Some(time),
            )),
        ),
        obj(),
    );
    assert!(
        in_time
            .iter()
            .any(|(slot, why)| slot.as_deref() == Some("format") && why.contains("travel time")),
        "{in_time:?}"
    );
}

#[test]
fn a_title_becomes_a_file_name_and_nothing_left_falls_back_to_the_uuid() {
    assert_eq!(file_name_of("H3 — top of the Yprésien"), "h3-top-of-the-ypresien");
    assert_eq!(file_name_of("  Ypresian–Chalk grid "), "ypresian-chalk-grid");
    assert_eq!(
        file_name_of("a/b\\c"),
        "a-b-c",
        "no folder slips in through a title"
    );
    assert_eq!(file_name_of("——"), "");
    let pieces = bag(
        two_triangles(),
        Some(known(
            surface_ref("——", "resqml22.TriangulatedSetRepresentation"),
            Some(lambert93()),
        )),
    );
    let plan = plan(&pieces, Choices::new().set("format", "obj")).unwrap();
    assert_eq!(plan.media[0].path, format!("media/export/{SURFACE}.obj"));
}

/// **Written, saved, and read back by two routes that do not share a line.**
///
/// A surface is written by `triangles` with its arrays in HDF5, saved, and the
/// saved package reopened. The export goes through the bag the recipe builds
/// (`read::pieces_of_object`, named for the slot), `apply` writes the part,
/// and the package is saved again. Then the **zip** is opened with the `zip`
/// crate, not the store:
///
/// - the part's bytes equal the exporter run on what `energyml-geometry`
///   reads from the saved package — `scene::object_of`, the route `energyml
///   export` takes — so the bag rebuilds the object the library reads;
/// - the OBJ's `v` and `f` lines equal the `NodeCount` and the triangle
///   `Count` the representation's own XML declares — a count the OBJ writer
///   never saw;
/// - `[Content_Types].xml` types the part with `mime()`, and both `.rels` hold
///   their half of the relationship (EPC §3.4.2 p. 16).
#[cfg(feature = "hdf5")]
#[test]
fn an_export_applied_and_saved_is_in_the_zip_with_both_relations() {
    use std::io::Read;
    use std::sync::Arc;

    use energyml_storage::{EpcStore, SaveOptions};

    let dir = std::env::temp_dir().join(format!("energyml-build-export-media-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let registry = Arc::new(energyml_schemas::all());
    let operations = Registry::new();

    let mut store = energyml_build::epc::open(None, "first.epc", registry.clone()).unwrap();
    store.external_files_mut().set_root(&dir);
    let surface = operations
        .plan(
            "triangles",
            &Pieces::empty().with(
                "shape",
                Source::Shape {
                    geometry: two_triangles(),
                    closed: None,
                },
            ),
            &Choices::new()
                .set("sourceEpsg", 2154)
                .set("project", 2154)
                .set("vertical", 5720)
                .set("title", "H3 — top of the Yprésien"),
            &Identity::derived(),
            &when(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
    let made = energyml_build::apply::apply(&surface, &mut store, &when());
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(made.refused.is_empty(), "{:?}", made.refused);
    let uuid = surface
        .of_role("representation")
        .next()
        .expect("the surface")
        .uuid
        .clone();
    store
        .save_as(dir.join("first.epc"), &SaveOptions::default())
        .unwrap();

    let mut reopened = EpcStore::open(dir.join("first.epc"), registry.clone()).unwrap();
    let read = energyml_build::read::pieces_of_object(&reopened, &uuid).unwrap();
    let mut pieces = Pieces::empty();
    for (name, source) in energyml_build::read::in_slot("shape", read) {
        pieces = pieces.with(name, source);
    }
    let export = plan(&pieces, Choices::new().set("format", "obj")).unwrap_or_else(|e| panic!("{e}"));
    let part = export.media[0].path.clone();
    let made = energyml_build::apply::apply(&export, &mut reopened, &when());
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(made.refused.is_empty(), "{:?}", made.refused);
    assert_eq!(made.media, std::slice::from_ref(&part));

    // The other route: the library's reader on the saved package.
    let summary = reopened.find_by_uuid(&uuid).cloned().unwrap();
    let object = reopened.get_object(&summary.uri).unwrap();
    let scene = energyml_geometry::scene::object_of(object.as_reflect(), &reopened, &summary.uri).unwrap();
    let exporters = Exporters::builtin();
    let obj = exporters.get("obj").unwrap();
    let mut direct = Vec::new();
    obj.write(&scene, &mut direct, &Options::new()).unwrap();
    let locator = summary.locator.clone();

    reopened
        .save_as(dir.join("second.epc"), &SaveOptions::default())
        .unwrap();
    let file = std::fs::File::open(dir.join("second.epc")).unwrap();
    let mut zip = zip::ZipArchive::new(file).expect("an EPC is a zip");
    let mut entry = |name: &str| -> Vec<u8> {
        let mut bytes = Vec::new();
        zip.by_name(name)
            .unwrap_or_else(|e| panic!("`{name}`: {e}"))
            .read_to_end(&mut bytes)
            .unwrap();
        bytes
    };
    let bytes = entry(&part);
    let index = String::from_utf8(entry("[Content_Types].xml")).unwrap();
    let forward = String::from_utf8(entry(&format!("_rels/{locator}.rels"))).unwrap();
    let (folder, name) = part.rsplit_once('/').unwrap();
    let backward = String::from_utf8(entry(&format!("{folder}/_rels/{name}.rels"))).unwrap();
    let xml = String::from_utf8(entry(&locator)).unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(
        bytes, direct,
        "the part is what the exporter writes of the object the library reads"
    );

    let text = String::from_utf8(bytes).unwrap();
    // The first number of the first element `tag` after `from` — the XML as
    // the archive holds it, read by nothing of this workspace.
    let declared = |from: &str, tag: &str| -> usize {
        let start = xml.find(from).unwrap_or_else(|| panic!("`{from}` in {xml}"));
        let open = start
            + xml[start..]
                .find(&format!(":{tag}>"))
                .unwrap_or_else(|| panic!("`{tag}` after `{from}` in {xml}"));
        let rest = &xml[open + tag.len() + 2..];
        rest[..rest.find('<').unwrap()]
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap()
    };
    // `NodeCount` of the patch; the triangles' array is `[count, 3]`, its
    // `Count` a list whose first value is the number of triangles.
    assert_eq!(
        lines_starting(&text, "v "),
        declared(":TrianglePatch", "NodeCount"),
        "one vertex per node the XML declares"
    );
    assert_eq!(
        lines_starting(&text, "f "),
        declared(":Triangles", "Count"),
        "one face per triangle the XML declares"
    );

    assert!(
        index.contains(&format!("PartName=\"/{part}\" ContentType=\"{}\"", obj.mime())),
        "the part typed by the exporter: {index}"
    );
    const BASE: &str = "http://schemas.energistics.org/package/2012/relationships/";
    assert!(
        forward.contains(&format!("Type=\"{BASE}destinationMedia\""))
            && forward.contains(&format!("Target=\"{part}\"")),
        "{forward}"
    );
    assert!(
        backward.contains(&format!("Type=\"{BASE}sourceMedia\""))
            && backward.contains(&format!("{locator}\"")),
        "{backward}"
    );
}
