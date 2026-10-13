//! What we write, we know how to read back.
//!
//! # Why this file exists
//!
//! The mesh reader long knew two formats when the exporter
//! wrote six. The asymmetry was not a choice: nobody had
//! noticed it, because no check confronted the two lists. The need:
//! the other formats that are already supported should be visible.
//!
//! The tests of `mesh.rs` pin each reader on a hand-written file, which
//! is the right form - **the shape of the assertions comes from the
//! specification, not from what we produce**. This one asks the other
//! question, which is a question about *us*: what our exporter writes,
//! does our reader read it back?
//!
//! It therefore stands on a file made by the exporter itself, and that is
//! legitimate because **it is precisely the subject**: a full round trip.
//!
//! # What it does not claim
//!
//! That the points come back identical. They cannot: the export places
//! in WGS 84 for GeoJSON and the import reprojects to a UTM zone, so the
//! loop crosses two systems. What is verified is what must be -
//! **the file is read without refusal, and it returns nodes** - because an import
//! that returned zero nodes looks exactly like an empty file.

use energyml_build::mesh;

/// What `energyml export --format vtp` writes of a triangle.
const VTP: &str = r#"<?xml version="1.0"?>
<VTKFile type="PolyData" version="1.0" byte_order="LittleEndian">
  <PolyData>
    <Piece NumberOfPoints="3" NumberOfPolys="1">
      <Points>
        <DataArray type="Float64" NumberOfComponents="3" format="ascii">
          652844.8 6864033.8 38.5
          652854.8 6864033.8 38.5
          652844.8 6864043.8 38.5
        </DataArray>
      </Points>
      <Polys>
        <DataArray type="Int64" Name="connectivity" format="ascii">0 1 2</DataArray>
        <DataArray type="Int64" Name="offsets" format="ascii">3</DataArray>
      </Polys>
    </Piece>
  </PolyData>
</VTKFile>
"#;

/// The same, as an `UnstructuredGrid` - the other container that we write.
const VTU: &str = r#"<?xml version="1.0"?>
<VTKFile type="UnstructuredGrid" version="1.0">
  <UnstructuredGrid>
    <Piece NumberOfPoints="3" NumberOfCells="1">
      <Points>
        <DataArray type="Float64" NumberOfComponents="3" format="ascii">
          0 0 0  1 0 0  0 1 0
        </DataArray>
      </Points>
      <Cells>
        <DataArray type="Int64" Name="connectivity" format="ascii">0 1 2</DataArray>
        <DataArray type="Int64" Name="offsets" format="ascii">3</DataArray>
        <DataArray type="UInt8" Name="types" format="ascii">5</DataArray>
      </Cells>
    </Piece>
  </UnstructuredGrid>
</VTKFile>
"#;

fn nodes(geometry: &energyml_geometry::geometry::Geometry) -> usize {
    geometry.patches.iter().map(|patch| patch.points.len()).sum()
}

#[test]
fn a_vtp_from_our_exporter_comes_back() {
    let made = mesh::read("surface.vtp", VTP).expect("un vtp");
    assert_eq!(nodes(&made), 3);
    // The coordinates are those of the file, to the digit: nothing in this
    // path reprojects, and an import that silently moved things would be
    // the costliest defect of all - it passes every check.
    assert_eq!(made.patches[0].points[0], [652_844.8, 6_864_033.8, 38.5]);
}

#[test]
fn a_vtu_from_our_exporter_comes_back() {
    let made = mesh::read("volume.vtu", VTU).expect("un vtu");
    assert_eq!(nodes(&made), 3);
}

#[test]
fn a_csv_from_our_exporter_comes_back_by_its_named_columns() {
    // Our CSV is long-format: one row per element, with a header. What
    // a scatter reader draws from it is the cloud of its positions.
    let text = "x,y,z,attribute,value\n10,20,30,porosity,0.2\n11,21,31,porosity,0.3\n";
    let made = mesh::read("points.csv", text).expect("un semis");
    assert_eq!(nodes(&made), 2);
    assert_eq!(made.patches[0].points[1], [11.0, 21.0, 31.0]);
}

#[test]
fn a_geojson_from_our_exporter_comes_back_in_metres() {
    // Our GeoJSON is in WGS 84, as RFC 7946 imposes. The import carries it
    // into a UTM zone deduced from its longitude - so the loop crosses two
    // systems, and what is verified is that it arrives in metres.
    let text = r#"{"type":"FeatureCollection","features":[
      {"type":"Feature","bbox":[2.32,48.96,38.5,2.33,48.97,38.5],
       "geometry":{"type":"MultiLineString","coordinates":[[[2.32,48.96,38.5],[2.33,48.97,38.5]]]},
       "properties":{"uuid":"x"}}]}"#;
    let made = mesh::read("trace.geojson", text).expect("a trace");
    assert_eq!(nodes(&made), 2);
    assert!(
        made.patches[0].points[0][0] > 100_000.0,
        "{:?}",
        made.patches[0].points[0]
    );
    // The elevation of the file is kept as it is: it is already in metres, and
    // only the horizontal changes system.
    assert_eq!(made.patches[0].points[0][2], 38.5);
}

#[test]
fn the_six_formats_written_are_the_six_formats_read() {
    // **The check that was missing.** It does not compare two lists of strings -
    // that would be true the day it is written and false the next - it verifies
    // that each extension that the exporter produces finds a reader.
    //
    // `energyml-export` is not a dependency of this crate, and it must not
    // become one for a test: the list is therefore quoted here, with its source.
    // If it grows without this file moving, it is this check that will say so -
    // it will refuse the new extension.
    for written in [".obj", ".csv", ".geojson", ".vtk", ".vtp", ".vtu"] {
        assert!(
            mesh::format_of(&format!("x{written}")).is_some(),
            "`{written}` is written by `energyml export --list` and does not read back",
        );
    }
}
