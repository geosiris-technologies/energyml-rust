//! What the written file says is true: the polygons of a `.vtp` carry one value
//! each, the header names the system the points are in, a time axis is placed in
//! GeoJSON, and a GeoJSON feature does not drop what it cannot hold.
//!
//! Every object is fabricated in memory; nothing here reads a file.

use energyml_domain::crs::{AxisReading, Crs, CrsKind, Point, VerticalDirection, VerticalQuantity};
use energyml_domain::reproject::Reprojector;
use energyml_export::{Error, Exporters, Options};
use energyml_geometry::{Attribute, AttributeScope, Geometry, Patch, SceneObject, Topology};

fn crs(epsg: u64, time: bool) -> Crs {
    Crs {
        uuid: "crs".into(),
        kind: "LocalEngineeringCompoundCrs",
        x_offset: 0.0,
        y_offset: 0.0,
        z_offset: 0.0,
        areal_rotation: 0.0,
        vertical_direction: VerticalDirection::Up,
        vertical_quantity: if time {
            VerticalQuantity::Time
        } else {
            VerticalQuantity::Length
        },
        horizontal_uom: None,
        vertical_uom: None,
        time_uom: time.then(|| "ms".to_string()),
        projected: CrsKind::Epsg(epsg),
        vertical: CrsKind::Absent,
        axes: AxisReading::NotStated,
    }
}

fn object(crs: Crs, patches: Vec<(Vec<[f64; 3]>, Topology)>) -> SceneObject {
    SceneObject {
        uuid: "u".into(),
        uri: "eml:///resqml22.Test(u)".into(),
        title: "t".into(),
        qualified_type: "resqml22.Test".into(),
        geometry: Geometry {
            kind: "test",
            patches: patches
                .into_iter()
                .enumerate()
                .map(|(index, (points, topology))| Patch {
                    index,
                    node_offset: 0,
                    points,
                    topology,
                    closed: Vec::new(),
                })
                .collect(),
        },
        crs: Some(crs),
        interpretation: None,
        feature: None,
        metadata: Default::default(),
        attributes: Vec::new(),
    }
}

/// A backend that moves every point 1000 m east, whatever the systems: enough to
/// tell a reprojected point from one that was not.
struct East;

impl Reprojector for East {
    fn reproject(&self, point: Point, _from: u64, _to: u64) -> energyml_domain::Result<Point> {
        Ok([point[0] + 1000.0, point[1], point[2]])
    }
}

fn written(format: &str, object: &SceneObject, options: &Options<'_>) -> String {
    let exporters = Exporters::builtin();
    let mut out = Vec::new();
    exporters
        .get(format)
        .expect("a format")
        .write(object, &mut out, options)
        .unwrap_or_else(|why| panic!("{format}: {why}"));
    String::from_utf8(out).expect("utf-8")
}

/// Two hexahedra sharing a face (twelve points), with one value per cell.
fn two_cells(values: Vec<f64>) -> SceneObject {
    let mut points = Vec::new();
    for layer in 0..3 {
        for (x, y) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
            points.push([x, y, layer as f64]);
        }
    }
    let mut grid = object(
        crs(4326, false),
        vec![(
            points,
            Topology::Hexahedra {
                cells: vec![[0, 1, 2, 3, 4, 5, 6, 7], [4, 5, 6, 7, 8, 9, 10, 11]],
                dimensions: [1, 1, 2],
                index: Vec::new(),
            },
        )],
    );
    grid.attributes = vec![Attribute {
        uuid: "p".into(),
        name: "porosity".into(),
        kind: None,
        uom: None,
        scope: AttributeScope::Cells,
        components: 1,
        values,
        lut: Vec::new(),
    }];
    grid
}

/// The numbers of the `<DataArray>` of the XML block `tag` named `name`.
fn numbers(text: &str, name: &str) -> Vec<f64> {
    let marker = format!("Name=\"{name}\"");
    let at = text
        .find(&marker)
        .unwrap_or_else(|| panic!("no array {name} in:\n{text}"));
    let body = text[at..].lines().nth(1).expect("a line of values").trim();
    body.split_whitespace()
        .map(|v| v.parse().expect("a number"))
        .collect()
}

/// [export-providers-cache-rules S3] A hexahedron is six polygons in a `.vtp`,
/// so the `CellData` has to count polygons: each cell's value is repeated over
/// its six (it was one value per cell, for `6 n` polygons, which ParaView
/// refuses or misaligns). A `.vtu` keeps one cell per cell and one value each.
#[test]
fn a_vtp_repeats_each_cell_value_over_the_polygons_of_the_cell() {
    let grid = two_cells(vec![10.0, 20.0]);
    let vtp = written("vtp", &grid, &Options::new());
    assert!(vtp.contains("NumberOfPolys=\"12\""), "{vtp}");
    let values = numbers(&vtp, "porosity");
    let mut want = vec![10.0; 6];
    want.extend(vec![20.0; 6]);
    assert_eq!(values, want, "one value per polygon, cell by cell");

    let vtu = written("vtu", &grid, &Options::new());
    assert!(vtu.contains("NumberOfCells=\"2\""), "{vtu}");
    assert_eq!(numbers(&vtu, "porosity"), vec![10.0, 20.0]);
    // Break it: write `attribute.values` as they stand in `write_attributes`
    // for the `PolyData` flavour (the `repeats` branch removed).
}

/// A cell property that does not have one value per cell cannot be spread over
/// the polygons: refused by name, not written misaligned.
#[test]
fn a_vtp_refuses_a_cell_property_that_does_not_fit_its_cells() {
    let grid = two_cells(vec![10.0, 20.0, 30.0]);
    let exporters = Exporters::builtin();
    let mut out = Vec::new();
    let why = exporters
        .get("vtp")
        .unwrap()
        .write(&grid, &mut out, &Options::new())
        .expect_err("3 values for 2 cells");
    assert!(matches!(why, Error::Unsuitable { .. }), "{why}");
    assert!(why.to_string().contains("porosity"), "{why}");
    // Break it: drop the `tuples.len() != repeats.len()` check.
}

fn a_point_in_lambert() -> SceneObject {
    object(
        crs(2154, false),
        vec![(vec![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]], Topology::Points)],
    )
}

/// [export-providers-cache-rules S4] The header names the system the points are
/// **in**: with `--target-epsg` that is the target, not the source's EPSG.
#[test]
fn the_obj_and_vtk_headers_name_the_target_of_a_reprojection() {
    let object = a_point_in_lambert();
    let options = Options::new().with_reprojector(&East).with_target_epsg(3857);
    let obj = written("obj", &object, &options);
    assert!(obj.contains("# crs crs EPSG:3857, metres, z up"), "{obj}");
    assert!(!obj.contains("EPSG:2154"), "{obj}");
    assert!(obj.contains("v 1001 2 3"), "the points were reprojected: {obj}");

    let vtk = written("vtk", &object, &options);
    assert!(vtk.contains("crs EPSG:3857"), "{vtk}");
    assert!(!vtk.contains("EPSG:2154"), "{vtk}");

    // Not reprojected: the CRS's own system, as before.
    let plain = written("obj", &object, &Options::new());
    assert!(plain.contains("# crs crs EPSG:2154, metres, z up"), "{plain}");
    // Break it: format `crs.projected.epsg()` in `header` and `pairs`, ignoring `target_epsg`.
}

/// [export-providers-cache-rules S4] A time axis with a stated equivalence **and**
/// a target system gets both: the equivalence on the vertical, the reprojection
/// on the horizontals. `target_epsg` used to be dropped without a word.
#[test]
fn a_time_axis_with_an_equivalence_is_also_reprojected_when_a_target_is_asked() {
    let seismic = object(
        crs(2154, true),
        vec![(vec![[1.0, 2.0, 1000.0]], Topology::Points)],
    );
    let options = Options::new()
        .with_reprojector(&East)
        .with_target_epsg(3857)
        .with_time_equivalence(2000.0);
    let obj = written("obj", &seismic, &options);
    // 1000 ms = 1 s, at 2000 m/s: 2000; the point went 1000 east.
    let vertex: Vec<f64> = obj
        .lines()
        .find(|line| line.starts_with("v "))
        .expect("a vertex")
        .split_whitespace()
        .skip(1)
        .map(|v| v.parse().expect("a number"))
        .collect();
    assert!(
        (vertex[0] - 1001.0).abs() < 1e-6
            && (vertex[1] - 2.0).abs() < 1e-6
            && (vertex[2] - 2000.0).abs() < 1e-6,
        "{vertex:?} in {obj}"
    );
    assert!(obj.contains("EPSG:3857"), "{obj}");
    // Break it: test `time_equivalence` before `target_epsg` in `Options::points`, as before.
}

/// [export-providers-cache-rules S5] GeoJSON places a time-domain object through
/// the equivalence, in WGS 84 as in native mode: it aborted the whole export in
/// the first, and wrote raw milliseconds as elevations in the second.
#[test]
fn geojson_applies_the_time_equivalence() {
    let seismic = object(
        crs(2154, true),
        vec![(vec![[1.0, 2.0, 1000.0], [3.0, 4.0, 2000.0]], Topology::Points)],
    );
    let exporters = Exporters::builtin();
    let geojson = exporters.get("geojson").unwrap();

    let wgs84 = Options::new()
        .with_reprojector(&East)
        .with_time_equivalence(2000.0);
    assert!(geojson.handles_with(&seismic, &wgs84));
    let text = written("geojson", &seismic, &wgs84);
    let json: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    let coordinates = &json["features"][0]["geometry"]["coordinates"];
    assert_eq!(coordinates[0][0], 1001.0, "{text}");
    assert_eq!(coordinates[0][2], 2000.0, "1 s at 2000 m/s: {text}");
    assert_eq!(coordinates[1][2], 4000.0, "{text}");
    assert!(
        json["features"][0]["properties"]["vertical_axis"]
            .as_str()
            .is_some_and(|note| note.contains("TRAVEL TIME")),
        "the equivalence is stated beside the numbers: {text}"
    );

    let native = Options::new().with_native_crs().with_time_equivalence(2000.0);
    let text = written("geojson", &seismic, &native);
    let json: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    let coordinates = &json["features"][0]["geometry"]["coordinates"];
    assert_eq!(coordinates[0][2], 2000.0, "{text}");
    assert!(text.contains("vertical_axis"), "{text}");
    // Break it: `object.points_in(placement)` in the native branch and `Placement::Metres` in `points_in_epsg`.
}

fn wgs84(patches: Vec<(Vec<[f64; 3]>, Topology)>) -> SceneObject {
    object(crs(4326, false), patches)
}

/// [export-providers-cache-rules S6] One GeoJSON feature has one kind of
/// geometry. Patches of different topologies are refused by name (the writer
/// followed the first patch and dropped the others), and so is an index that
/// names no position (it was filtered out without a count).
#[test]
fn geojson_refuses_what_it_would_have_dropped() {
    let exporters = Exporters::builtin();
    let geojson = exporters.get("geojson").unwrap();
    let ask = |object: &SceneObject| {
        let mut out = Vec::new();
        geojson.write(object, &mut out, &Options::new()).map(|_| out)
    };

    let triangle = (
        vec![[2.0, 48.0, 0.0], [3.0, 48.0, 0.0], [2.5, 49.0, 0.0]],
        Topology::Triangles(vec![[0, 1, 2]]),
    );
    assert!(ask(&wgs84(vec![triangle.clone()])).is_ok());

    let line = (
        vec![[2.0, 48.0, 0.0], [3.0, 48.0, 0.0]],
        Topology::Polylines(vec![2]),
    );
    let mixed = ask(&wgs84(vec![triangle.clone(), line]));
    assert!(matches!(&mixed, Err(Error::Unsuitable { .. })), "{mixed:?}");
    let why = mixed.unwrap_err().to_string();
    assert!(why.contains("triangles") && why.contains("lines"), "{why}");

    let outside = (
        vec![[2.0, 48.0, 0.0], [3.0, 48.0, 0.0], [2.5, 49.0, 0.0]],
        Topology::Triangles(vec![[0, 1, 9]]),
    );
    let why = ask(&wgs84(vec![outside])).unwrap_err().to_string();
    assert!(why.contains("node 9"), "{why}");
    // Break it: `.filter(|t| t.iter().all(|i| (*i as usize) < points.len()))` and the
    // first patch's topology deciding, as before.
}
