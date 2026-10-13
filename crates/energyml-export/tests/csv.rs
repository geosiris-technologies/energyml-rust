//! The long CSV export, end to end through the registry.
//!
//! The fixture reproduces what `attributes_for` produces: per-node values (with
//! a NaN — the corpus's "no data here") and per-cell values. The assertions
//! count the rows and read the cells back: a CSV with the right number of rows
//! but the values shifted by one column is exactly the file a spreadsheet loads
//! without complaining.

use energyml_export::{Exporters, Options};
use energyml_geometry::{Attribute, AttributeScope, Geometry, Patch, SceneObject, Topology};

fn a_well() -> SceneObject {
    SceneObject {
        uuid: "u-1".into(),
        uri: "eml:///resqml22.WellboreTrajectoryRepresentation(u-1)".into(),
        title: "AUB, 12".into(), // the comma forces the quoting
        qualified_type: "resqml22.WellboreTrajectoryRepresentation".into(),
        geometry: Geometry {
            kind: "test",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points: vec![[0.0, 0.0, 10.0], [1.0, 0.0, 20.0], [2.0, 0.0, 30.0]],
                topology: Topology::Polylines(vec![3]),
                closed: Vec::new(),
            }],
        },
        crs: None,
        interpretation: None,
        feature: None,
        metadata: Default::default(),
        attributes: vec![
            Attribute {
                uuid: "p-1".into(),
                name: "Gamma".into(),
                kind: Some("gamma ray".into()),
                uom: Some("gAPI".into()),
                scope: AttributeScope::Points,
                components: 1,
                values: vec![55.0, f64::NAN, 80.5],
                lut: Vec::new(),
            },
            Attribute {
                uuid: "p-2".into(),
                name: "Perm".into(),
                kind: None,
                uom: Some("mD".into()),
                scope: AttributeScope::Cells,
                components: 1,
                values: vec![12.5, 13.0],
                lut: Vec::new(),
            },
        ],
    }
}

#[test]
fn a_well_with_logs_writes_one_row_per_node_and_per_cell_value() {
    let exporters = Exporters::builtin();
    let exporter = exporters.get("csv").expect("csv is registered");
    let mut out = Vec::new();
    exporter
        .write(&a_well(), &mut out, &Options::default())
        .expect("writes");
    let text = String::from_utf8(out).expect("utf-8");
    let lines: Vec<&str> = text.lines().collect();

    assert_eq!(
        lines[0],
        "object,uuid,element,index,x,y,z,property,kind,uom,value"
    );
    // 3 nodes × 1 per-node property, + 2 cell values.
    assert_eq!(lines.len(), 1 + 3 + 2);

    // The title's comma is quoted — otherwise every row gains a column and
    // everything shifts with no error.
    assert!(lines[1].starts_with("\"AUB, 12\",u-1,node,0,"));
    let first: Vec<&str> = lines[1].splitn(8, ',').collect();
    assert_eq!(first[0], "\"AUB");

    // The second node's NaN is an empty cell, not the word `NaN`.
    assert!(
        lines[2].ends_with("Gamma,gamma ray,gAPI,"),
        "the NaN row should end with an empty value cell: {}",
        lines[2]
    );
    assert!(lines[3].ends_with(",80.5"));

    // The cells: no coordinates — a cell has not one position but eight — and
    // the index that places it back in the grid.
    assert!(lines[4].contains(",cell,0,,,,Perm,,mD,12.5"), "{}", lines[4]);
    assert!(lines[5].contains(",cell,1,,,,Perm,,mD,13"), "{}", lines[5]);
}

#[test]
fn an_object_with_no_attribute_still_writes_its_nodes() {
    let exporters = Exporters::builtin();
    let exporter = exporters.get("csv").expect("csv is registered");
    let mut bare = a_well();
    bare.attributes.clear();
    let mut out = Vec::new();
    exporter
        .write(&bare, &mut out, &Options::default())
        .expect("writes");
    let text = String::from_utf8(out).expect("utf-8");
    // Geometry alone is an answer: three nodes, empty property columns — not
    // an empty file that looks like a broken export.
    assert_eq!(text.lines().count(), 1 + 3);
    assert!(text.lines().nth(1).expect("row").ends_with(",,,,"));
}

#[test]
fn a_group_shares_one_header() {
    let exporters = Exporters::builtin();
    let exporter = exporters.get("csv").expect("csv is registered");
    let mut second = a_well();
    second.uuid = "u-2".into();
    second.title = "Another".into();
    let mut out = Vec::new();
    exporter
        .write_all(&[a_well(), second], &mut out, &Options::default())
        .expect("writes");
    let text = String::from_utf8(out).expect("utf-8");
    // One header only: the well is a column, not a block — that is what makes
    // a group export pasteable as it stands into a spreadsheet.
    assert_eq!(text.lines().filter(|line| line.starts_with("object,")).count(), 1);
    assert!(text.contains("Another,u-2"));
}

#[test]
fn geojson_refuses_an_object_whose_every_node_is_undefined() {
    // The bug seen from the interface: an object whose every position
    // is NaN passed the gate on its point count, was emptied by the
    // preparation, and came out as a Feature with empty coordinates — a file
    // every reader loads and that shows nothing.
    let exporters = Exporters::builtin();
    let exporter = exporters.get("geojson").expect("geojson is registered");
    let mut ghost = a_well();
    for patch in &mut ghost.geometry.patches {
        patch.points.fill([f64::NAN; 3]);
    }
    assert!(
        !exporter.handles(&ghost),
        "an all-NaN object must be refused at the gate, not written empty"
    );
}
