//! A ring the file calls closed comes out closed, in every format.
//!
//! RESQML states it beside the counts (`ClosedPolylines`, read into
//! `Patch::closed`), and none of the three formats has a flag for it: GeoJSON
//! (RFC 7946 §3.1.4), an OBJ `l` and a VTK polyline cell only close by
//! repeating the first vertex. Found on Lutetia's outline: the exported
//! GeoJSON looked like a polygon but was not closed, so an open line was drawn.
//!
//! The other half matters as much: an **open** run and a run the file says
//! **nothing** about stay open — drawing the closing edge there would invent a
//! ring.

use energyml_domain::crs::{Crs, CrsKind, VerticalDirection, VerticalQuantity};
use energyml_export::{Exporters, Options};
use energyml_geometry::{Geometry, Patch, SceneObject, Topology};

/// Two runs: a triangle said closed, then a two-node line said open.
fn two_runs(closed: Vec<bool>) -> SceneObject {
    SceneObject {
        uuid: "ring".into(),
        uri: "eml:///resqml22.PolylineSetRepresentation(ring)".into(),
        title: "outline".into(),
        qualified_type: "resqml22.PolylineSetRepresentation".into(),
        geometry: Geometry {
            kind: "PolylineSetRepresentation",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points: vec![
                    [2.0, 48.0, 0.0],
                    [3.0, 48.0, 0.0],
                    [2.5, 49.0, 0.0],
                    [4.0, 48.0, 0.0],
                    [5.0, 48.0, 0.0],
                ],
                topology: Topology::Polylines(vec![3, 2]),
                closed,
            }],
        },
        // Already in degrees: the GeoJSON exporter needs no reprojector.
        crs: Some(Crs {
            uuid: "crs".into(),
            kind: "LocalEngineeringCompoundCrs",
            x_offset: 0.0,
            y_offset: 0.0,
            z_offset: 0.0,
            areal_rotation: 0.0,
            vertical_direction: VerticalDirection::Up,
            vertical_quantity: VerticalQuantity::Length,
            horizontal_uom: None,
            vertical_uom: None,
            time_uom: None,
            projected: CrsKind::Epsg(4326),
            vertical: CrsKind::Absent,
        }),
        interpretation: None,
        feature: None,
        metadata: Default::default(),
        attributes: Vec::new(),
    }
}

fn written(format: &str, object: &SceneObject) -> String {
    let exporters = Exporters::builtin();
    let exporter = exporters
        .get(format)
        .unwrap_or_else(|why| panic!("{format}: {why}"));
    let mut out = Vec::new();
    exporter
        .write(object, &mut out, &Options::default())
        .expect("writes");
    String::from_utf8(out).expect("utf-8")
}

/// The lines of the GeoJSON's MultiLineString, as (x, y) pairs — read by a
/// real parser, not by looking for brackets.
fn geojson_lines(object: &SceneObject) -> Vec<Vec<(f64, f64)>> {
    let json: serde_json::Value = serde_json::from_str(&written("geojson", object)).expect("valid JSON");
    let geometry = if json["type"] == "FeatureCollection" {
        &json["features"][0]["geometry"]
    } else {
        &json["geometry"]
    };
    assert_eq!(geometry["type"], "MultiLineString");
    geometry["coordinates"]
        .as_array()
        .expect("lines")
        .iter()
        .map(|line| {
            line.as_array()
                .expect("positions")
                .iter()
                .map(|p| (p[0].as_f64().expect("x"), p[1].as_f64().expect("y")))
                .collect()
        })
        .collect()
}

#[test]
fn a_closed_ring_repeats_its_first_position_in_geojson() {
    let lines = geojson_lines(&two_runs(vec![true, false]));
    assert_eq!(lines.len(), 2);
    assert_eq!(
        lines[0].len(),
        4,
        "three nodes and the first one again: {:?}",
        lines[0]
    );
    assert_eq!(lines[0].first(), lines[0].last());
    assert_eq!(lines[1].len(), 2, "the open run stays open");
}

#[test]
fn a_ring_the_file_says_nothing_about_stays_open() {
    // `ClosedPolylines` absent is not *all closed*: nothing is invented.
    for lines in [
        geojson_lines(&two_runs(Vec::new())),
        geojson_lines(&two_runs(vec![false, false])),
    ] {
        assert_eq!(lines[0].len(), 3);
        assert_ne!(lines[0].first(), lines[0].last());
    }
}

#[test]
fn a_closed_ring_ends_on_its_first_vertex_in_obj() {
    let text = written("obj", &two_runs(vec![true, false]));
    let lines: Vec<&str> = text.lines().filter(|l| l.starts_with("l ")).collect();
    // OBJ counts from 1: the triangle is vertices 1 2 3, the open line 4 5.
    assert_eq!(lines, ["l 1 2 3 1", "l 4 5"]);
    let open = written("obj", &two_runs(Vec::new()));
    let open: Vec<&str> = open.lines().filter(|l| l.starts_with("l ")).collect();
    assert_eq!(open, ["l 1 2 3", "l 4 5"]);
}

#[test]
fn a_closed_ring_ends_on_its_first_point_in_legacy_vtk() {
    let text = written("vtk", &two_runs(vec![true, false]));
    let mut rows = text.lines().skip_while(|l| !l.starts_with("LINES "));
    // Two cells, and a size that counts the repeated index: (1 + 4) + (1 + 2).
    assert_eq!(rows.next(), Some("LINES 2 8"));
    assert_eq!(rows.next(), Some("4 0 1 2 0"));
    assert_eq!(rows.next(), Some("2 3 4"));
}
