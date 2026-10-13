//! A node with no position must not reach a file — and neither must the cells
//! around it.
//!
//! The report that started this: *"when a z value is null, the cell is
//! supposed not to be represented because it is absent"*. It is right in effect, and the
//! sharpest way to say why is not aesthetic — it is that **`NaN` is not a JSON
//! value**. RFC 7946 is JSON before it is GeoJSON, so one null Z among 34 232
//! made a 10 MB `.geojson` fail `JSON.parse` outright. OBJ has no syntax for a
//! missing coordinate either, and VTK's readers put `nan` at the origin or
//! refuse the file.
//!
//! Since a 2D grid is now drawn as its cells (see `shapes.rs`), the sentence
//! finally means what it says: a hole removes the **four cells** around it, not
//! one dot among three hundred thousand.
//!
//! These tests are on hand-built objects rather than the corpus on purpose: they
//! run in every configuration, with no HDF5 and no test data, and they say
//! exactly which node was undefined. The corpus is what proved the bug; this is
//! what keeps it fixed.

use energyml_export::{Exporters, Options, Shapes};
use energyml_geometry::{Geometry, Patch, Placement, SceneObject, Topology};

/// A four-by-four lattice — nine cells — with one node undefined, and no CRS.
///
/// Four by four and not three by three, because the middle node of a 3 × 3
/// belongs to *every* one of its four cells: the correct answer there is an
/// empty grid, which proves nothing about which cells were dropped. Here the
/// hole at `(1, 1)` touches four cells and leaves five.
///
/// No CRS so the exporters take the local coordinates as they are: this is a
/// test about NaN, and dragging geodesy into it would only add a second reason
/// for it to fail.
fn grid_with_a_hole() -> SceneObject {
    let mut points: Vec<[f64; 3]> = Vec::new();
    for j in 0..4 {
        for i in 0..4 {
            points.push([i as f64, j as f64, 0.0]);
        }
    }
    points[4 + 1] = [1.0, 1.0, f64::NAN];

    SceneObject {
        uuid: "hole".into(),
        uri: "eml:///resqml22.Grid2dRepresentation(hole)".into(),
        title: "a horizon with a hole in it".into(),
        qualified_type: "resqml22.Grid2dRepresentation".into(),
        geometry: Geometry {
            kind: "Grid2dRepresentation",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points,
                topology: Topology::Grid {
                    fastest: 4,
                    slowest: 4,
                },
                closed: Vec::new(),
            }],
        },
        crs: None,
        interpretation: None,
        feature: None,
        metadata: Default::default(),
        attributes: Vec::new(),
    }
}

fn options() -> Options<'static> {
    Options::new().with_native_crs().with_placement(Placement::Local)
}

fn written_with(format: &str, object: &SceneObject, options: &Options<'_>) -> String {
    let exporters = Exporters::builtin();
    let mut buffer = Vec::new();
    exporters
        .get(format)
        .unwrap_or_else(|_| panic!("`{format}` should be a built-in exporter"))
        .write(object, &mut buffer, options)
        .unwrap_or_else(|e| panic!("`{format}` refused an ordinary object: {e}"));
    String::from_utf8(buffer).expect("every exporter here writes utf-8")
}

fn written(format: &str, object: &SceneObject) -> String {
    written_with(format, object, &options())
}

#[test]
fn geojson_writes_no_nan_because_json_has_no_such_value() {
    let text = written("geojson", &grid_with_a_hole());
    assert!(
        !text.contains("NaN") && !text.contains("nan"),
        "a bare NaN makes the whole document unparseable, and this is where it got in:\n{text}"
    );
}

#[test]
fn geojson_that_came_out_can_be_read_back_as_json() {
    // The check that actually matters, done the way a browser would: parse it.
    // A hand-rolled writer is exactly the place a NaN, a trailing comma or an
    // unescaped quote gets in, and only a parser notices all three.
    let text = written("geojson", &grid_with_a_hole());
    let value: serde_json::Value = serde_json::from_str(&text).expect("the document must parse as JSON");
    let geometry = &value["features"][0]["geometry"];
    assert_eq!(geometry["type"], "MultiPolygon");

    let rings = geometry["coordinates"]
        .as_array()
        .expect("a MultiPolygon has an array of polygons");
    assert_eq!(
        rings.len(),
        5,
        "nine cells, four of them touching the hole at (1,1)"
    );
    for polygon in rings {
        let ring = polygon[0].as_array().expect("a polygon has a ring");
        // RFC 7946 §3.1.6: a linear ring closes on its first position.
        assert_eq!(ring.len(), 5, "four corners and the repeat of the first");
        assert_eq!(ring[0], ring[4]);
        for position in ring {
            for number in position.as_array().expect("a position is an array") {
                assert!(
                    number.as_f64().is_some_and(|v| v.is_finite()),
                    "every coordinate must be a finite JSON number, found {number}"
                );
            }
        }
    }
}

#[test]
fn asking_for_points_gives_back_the_surviving_nodes() {
    // The other half of the same rule: the caller who wants a point cloud gets
    // the fifteen nodes that have a position, and not the sixteenth.
    let options = options().with_shapes(Shapes::new().parse_rule("*=points").unwrap());
    let text = written_with("geojson", &grid_with_a_hole(), &options);
    let value: serde_json::Value = serde_json::from_str(&text).expect("must parse as JSON");
    let geometry = &value["features"][0]["geometry"];
    assert_eq!(geometry["type"], "MultiPoint");
    assert_eq!(geometry["coordinates"].as_array().unwrap().len(), 15);
}

#[test]
fn obj_writes_no_vertex_it_cannot_place() {
    let text = written("obj", &grid_with_a_hole());
    let vertices: Vec<&str> = text.lines().filter(|l| l.starts_with("v ")).collect();
    assert_eq!(vertices.len(), 15, "in:\n{text}");
    for vertex in &vertices {
        assert!(
            !vertex.to_lowercase().contains("nan"),
            "`{vertex}` is not something an OBJ reader can place"
        );
    }
    let faces: Vec<&str> = text.lines().filter(|l| l.starts_with("f ")).collect();
    assert_eq!(faces.len(), 5, "in:\n{text}");
    for face in &faces {
        assert_eq!(face.split_whitespace().count(), 5, "`f` plus four corners");
    }
}

#[test]
fn vtk_writes_no_coordinate_it_cannot_place() {
    // Legacy VTK, because it is the flavour whose points are plain text and so
    // the one where a `nan` is visible without decoding anything.
    let text = written("vtk", &grid_with_a_hole());
    assert!(!text.to_lowercase().contains("nan"), "in:\n{text}");
    assert!(
        text.contains("POLYGONS 5 25"),
        "five quadrilaterals, five integers each, in:\n{text}"
    );
}

#[test]
fn a_property_keeps_the_values_of_the_points_that_remain() {
    // The worst outcome of the three, and the reason the compaction carries
    // index maps: dropping points and leaving the values alone colours every
    // survivor with its neighbour's number. It looks right, so nothing
    // downstream ever questions it.
    use energyml_geometry::scene::{Attribute, AttributeScope};

    let mut object = grid_with_a_hole();
    object.attributes.push(Attribute {
        uuid: "p".into(),
        name: "depth".into(),
        kind: None,
        uom: None,
        scope: AttributeScope::Points,
        components: 1,
        // One per node, and each says which node it belongs to.
        values: (0..16).map(|i| i as f64).collect(),
        lut: Vec::new(),
    });

    let drawable = object.drawable_only().expect("node 5 has no position");
    let expected: Vec<f64> = (0..16).filter(|i| *i != 5).map(|i| i as f64).collect();
    assert_eq!(
        drawable.attributes[0].values, expected,
        "value 5 belonged to the node that was dropped; the rest must not shift"
    );
}

#[test]
fn a_grid_with_no_hole_keeps_every_cell() {
    // The rule must cost nothing to the packages that have no nulls — and, more
    // importantly, must not quietly change what they produce.
    let mut object = grid_with_a_hole();
    object.geometry.patches[0].points[5] = [1.0, 1.0, 0.0];
    assert!(
        object.geometry.defined_only().is_none(),
        "nothing to drop means nothing rebuilt"
    );
    let text = written("geojson", &object);
    let value: serde_json::Value = serde_json::from_str(&text).expect("must parse as JSON");
    assert_eq!(
        value["features"][0]["geometry"]["coordinates"]
            .as_array()
            .unwrap()
            .len(),
        9
    );
}
