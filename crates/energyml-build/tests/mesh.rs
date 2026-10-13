//! Reading a foreign mesh — and reading it **as the former front-end parsers read it**.
//!
//! # What this file holds
//!
//! The need: reading mesh files must go
//! through Rust, to avoid duplicating the readings in a web front end. The
//! two parsers lived there, in TypeScript; this module
//! takes them over, and this file is the **confrontation** before the front end
//! loses them.
//!
//! The fixtures are those of the front end's own tests, copied word for
//! word, and the expectations are those that front end already checked. Two readers
//! of one format that are not confronted end up accepting
//! different things without anything saying so — it is the rule of the repository, and
//! this is the only moment when the two exist at the same time to uphold it.
//!
//! # What would have to break to make it turn red
//!
//! A fan from the wrong corner, a forgotten base 1, a negative index
//! taken for a large number, an `l` chain read as a single edge. Each
//! returns a *plausible* shape — which is why the counts are not enough and
//! the indices themselves are pinned.

use energyml_build::mesh;
use energyml_geometry::geometry::Topology;

/// The triangles of a shape, flat, in the order in which they arrive.
fn triangles(geometry: &energyml_geometry::geometry::Geometry) -> Vec<u32> {
    geometry
        .patches
        .iter()
        .filter_map(|patch| match &patch.topology {
            Topology::Triangles(list) => Some(list.iter().flatten().copied().collect::<Vec<_>>()),
            _ => None,
        })
        .flatten()
        .collect()
}

/// The segments of a shape, flat.
fn segments(geometry: &energyml_geometry::geometry::Geometry) -> Vec<u32> {
    geometry
        .patches
        .iter()
        .filter_map(|patch| match &patch.topology {
            Topology::Segments(list) => Some(list.iter().flatten().copied().collect::<Vec<_>>()),
            _ => None,
        })
        .flatten()
        .collect()
}

/// The number of nodes — the patches share the same points.
fn nodes(geometry: &energyml_geometry::geometry::Geometry) -> usize {
    geometry.patches.first().map(|patch| patch.points.len()).unwrap_or(0)
}

// -- OBJ ---------------------------------------------------------------------

/// The fixture of a front end's artefact test, word for word.
const SQUARE: &str = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf 1 2 3 4\nl 1 2 3\n";

#[test]
fn vertices_faces_and_lines_are_one_based_as_the_format_counts() {
    let read = mesh::read_obj(SQUARE).expect("un OBJ");
    assert_eq!(nodes(&read), 4);
    assert_eq!(triangles(&read), vec![0, 1, 2, 0, 2, 3]);
    // `l 1 2 3` is a **chain**: two segments, and not one edge.
    assert_eq!(segments(&read), vec![0, 1, 1, 2]);
}

/// **Two patches on the same points are offset all the same.**
///
/// `Patch::node_offset` is defined as *the running sum of the point counts of
/// the preceding patches*, and an OBJ that declares faces **and**
/// `l` is the only place in the repository where two patches carry the same points.
/// Zero seemed right there — they are the same points — and made everything
/// that indexes *the nodes of the representation* designate those of the first patch
/// believing to name those of the second.
///
/// # What would have to break to make it turn red
///
/// Put `node_offset: 0` back on the second patch. Nothing else reads it
/// today, so nothing else would say so.
#[test]
fn the_second_patch_is_offset_by_the_count_of_the_first() {
    let read = mesh::read_obj(SQUARE).expect("un OBJ");
    assert_eq!(read.patches.len(), 2, "a surface and its lines");
    assert_eq!(read.patches[0].node_offset, 0);
    assert_eq!(read.patches[1].node_offset, read.patches[0].points.len());

    // And a file of a single nature has only one patch, at zero.
    let lines = mesh::read_obj("v 0 0 0
v 1 0 0
l 1 2").expect("un OBJ");
    assert_eq!(lines.patches.len(), 1);
    assert_eq!(lines.patches[0].node_offset, 0);
}

#[test]
fn a_polygon_is_triangulated_as_a_fan() {
    let read = mesh::read_obj("v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf 1 2 3 4").expect("un OBJ");
    assert_eq!(triangles(&read), vec![0, 1, 2, 0, 2, 3]);
}

#[test]
fn the_three_spellings_of_indices_and_negative_indices() {
    // `7/2`, `7//3`: only the first field is the vertex. `-1`: the last
    // declared vertex — the definition of the format, not a tolerance.
    let read = mesh::read_obj("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1/5 2//7 -1").expect("un OBJ");
    assert_eq!(triangles(&read), vec![0, 1, 2]);
}

#[test]
fn an_index_outside_the_buffer_is_refused_naming_the_line() {
    let why = mesh::read_obj("v 0 0 0\nf 1 2 3").expect_err("refused");
    assert!(why.contains("line 2"), "{why}");
    // Zero does not exist in OBJ: saying so is better than a silent shift.
    let zero = mesh::read_obj("v 0 0 0\nf 0 1 1").expect_err("refused");
    assert!(zero.contains("counts from 1"), "{zero}");
}

#[test]
fn a_file_without_a_vertex_is_refused_not_returned_empty() {
    // An empty artefact in the scene is invisible and unreachable — the worst
    // of both states.
    let why = mesh::read_obj("# rien\nusemtl acier").expect_err("refused");
    assert!(why.contains("no vertex"), "{why}");
}

#[test]
fn what_is_real_and_of_no_interest_passes_without_noise() {
    let read = mesh::read_obj("mtllib a.mtl\nv 0 0 0\nvt 0 0\nvn 0 0 1\no truc\nv 1 0 0\nv 0 1 0\nf 1 2 3")
        .expect("un OBJ");
    assert_eq!(triangles(&read).len(), 3);
}

// -- VTK legacy --------------------------------------------------------------

const HEADER: &str = "# vtk DataFile Version 3.0\ntitre [uuid]\nASCII\n";

#[test]
fn polydata_the_polygon_as_a_fan_the_polyline_as_segments() {
    let poly = format!(
        "{HEADER}DATASET POLYDATA\nPOINTS 4 double\n0 0 0\n1 0 0\n1 1 0\n0 1 0\nPOLYGONS 1 5\n4 0 1 2 3\nLINES 1 4\n3 0 1 2\n"
    );
    let read = mesh::read_vtk(&poly).expect("un VTK");
    assert_eq!(nodes(&read), 4);
    assert_eq!(triangles(&read), vec![0, 1, 2, 0, 2, 3]);
    assert_eq!(segments(&read), vec![0, 1, 1, 2]);
}

#[test]
fn numbers_may_spread_over_several_lines() {
    let folded = format!("{HEADER}DATASET POLYDATA\nPOINTS 2 float\n0 0\n0 1\n0 0\n");
    assert_eq!(nodes(&mesh::read_vtk(&folded).expect("un VTK")), 2);
}

#[test]
fn unstructured_grid_the_hexahedron_becomes_six_faces_and_the_polyline_follows() {
    let grid = format!(
        "{HEADER}DATASET UNSTRUCTURED_GRID\nPOINTS 8 double\n0 0 0\n1 0 0\n1 1 0\n0 1 0\n0 0 1\n1 0 1\n1 1 1\n0 1 1\nCELLS 2 12\n8 0 1 2 3 4 5 6 7\n2 0 6\nCELL_TYPES 2\n12\n4\n"
    );
    let read = mesh::read_vtk(&grid).expect("un VTK");
    // Six faces, two triangles each.
    assert_eq!(triangles(&read).len(), 6 * 2 * 3);
    assert_eq!(segments(&read), vec![0, 6]);
}

#[test]
fn a_binary_or_5_1_vtk_is_refused_by_name() {
    let binary = format!("{HEADER}BINARY\nDATASET POLYDATA\n");
    assert!(mesh::read_vtk(&binary).expect_err("refused").contains("BINARY"));

    let modern = format!("{HEADER}DATASET POLYDATA\nPOINTS 1 double\n0 0 0\nOFFSETS vtktypeint64\n0\n");
    assert!(mesh::read_vtk(&modern).expect_err("refused").contains("5.1"));

    let other = format!("{HEADER}DATASET STRUCTURED_POINTS\n");
    let why = mesh::read_vtk(&other).expect_err("refused");
    assert!(why.contains("STRUCTURED_POINTS"), "{why}");
}

#[test]
fn what_is_not_a_legacy_vtk_is_refused_from_the_first_line() {
    let why = mesh::read_vtk("v 0 0 0\nf 1 1 1").expect_err("refused");
    assert!(why.contains("vtk DataFile"), "{why}");
}

// -- the registry of formats -------------------------------------------------

/// **What this module reads is asked for, not spelled elsewhere.**
///
/// A caller — the front end, the CLI — asks for the list rather than writing its
/// own extensions. A format added appears in its file picker
/// without a line changing, which is the rule of the repository for everything that
/// grows.
#[test]
fn readable_formats_can_be_asked_for() {
    assert!(mesh::FORMATS.iter().any(|f| f.name == "obj"));
    assert!(mesh::FORMATS.iter().any(|f| f.name == "vtk"));
    // Each says its extensions and what it reads: a format without a description
    // would put a bare line in a picker.
    for format in mesh::FORMATS {
        assert!(!format.extensions.is_empty(), "`{}` does not say its extensions", format.name);
        assert!(!format.describe.is_empty(), "`{}` does not say what it reads", format.name);
        assert!(format.extensions.iter().all(|e| e.starts_with('.')), "`{}`: the dot included", format.name);
    }
}

#[test]
fn the_format_is_read_from_the_file_name_case_insensitively() {
    assert_eq!(mesh::format_of("Nappe.OBJ").map(|f| f.name), Some("obj"));
    assert_eq!(mesh::format_of("coupe.vtk").map(|f| f.name), Some("vtk"));
    // GeoJSON **is** a readable mesh since 19/09: it was already exported
    // without being able to read it back, and the asymmetry was not a choice.
    assert_eq!(mesh::format_of("export.geojson").map(|f| f.name), Some("geojson"));
    assert_eq!(mesh::format_of("SEMIS.CSV").map(|f| f.name), Some("csv"));
    assert_eq!(mesh::format_of("forme.vtp").map(|f| f.name), Some("vtp"));

    // And `read` refuses naming both ends: the file, and what is known
    // to be readable — a refusal that only said “no” would send one searching at random.
    let why = mesh::read("piece.stl", "solid x").expect_err("refused");
    assert!(why.contains("piece.stl") && why.contains("obj") && why.contains("geojson"), "{why}");
}

#[test]
fn read_chooses_the_reader_by_name() {
    assert!(mesh::read("carre.obj", SQUARE).is_ok());
    let poly = format!("{HEADER}DATASET POLYDATA\nPOINTS 1 double\n0 0 0\n");
    assert!(mesh::read("coupe.vtk", &poly).is_ok());
    // The name decides, so an OBJ named `.vtk` is refused by the VTK reader —
    // and that is good news: sniffing the content would be a second way
    // of deciding the format.
    assert!(mesh::read("menteur.vtk", SQUARE).is_err());
}

// ---------------------------------------------------------------------------
// CSV, GeoJSON and VTK XML — added on 19/09, because these four formats were
// already **exported** without being able to read them back. The asymmetry was not a choice.

#[test]
fn a_csv_is_a_scatter_and_its_separator_is_measured() {
    // A CSV has no standard: comma, semicolon or tab depending on
    // the spreadsheet. Assuming the comma would return a single column and a refusal
    // *“this is not a number”* that helps nobody. The separator retained is
    // the one that splits the most — measured on the line.
    for text in ["x,y,z\n1,2,3\n4,5,6", "x;y;z\n1;2;3\n4;5;6", "x\ty\tz\n1\t2\t3\n4\t5\t6"] {
        let made = mesh::read("semis.csv", text).expect("un semis");
        assert_eq!(nodes(&made), 2, "with `{}`", text.lines().next().unwrap_or(""));
        assert_eq!(made.patches[0].points[1], [4.0, 5.0, 6.0]);
    }
}

#[test]
fn columns_are_named_if_they_are_and_counted_otherwise() {
    // The header wins when it exists, even out of order: a file
    // that says `z,x,y` says so for a reason.
    let named = mesh::read("semis.csv", "z,x,y\n30,10,20").expect("un semis");
    assert_eq!(named.patches[0].points[0], [10.0, 20.0, 30.0]);
    // Without a header, the position — the only order that an unnamed scatter can
    // have, and the one that any export without a title writes.
    let bare = mesh::read("semis.csv", "10,20,30").expect("un semis");
    assert_eq!(bare.patches[0].points[0], [10.0, 20.0, 30.0]);
}

#[test]
fn a_csv_without_z_is_refused_until_the_height_is_given() {
    // **Zero is not neutral**: it is sea level in half the
    // vertical systems and the bottom of the model in the other. Two columns make
    // a map, not a shape, so the format asks for it rather than choosing.
    // **We query what the refusal declares, not its words.** Looking for
    // “height” in the sentence would be exactly the fault that `Refused` exists
    // to remove: the day the sentence changes, the check still passes and
    // the form no longer opens.
    let why = mesh::read_or_ask("semis.csv", "x,y\n10,20", &mesh::Given::default())
        .expect_err("the elevation is missing");
    assert_eq!(why.missing, vec![mesh::Ask::Height]);
    // Given, it is laid — and it is the same reading, not a second one.
    let made = mesh::read_given(
        "semis.csv",
        "x,y\n10,20",
        &mesh::Given { height: Some(-15.0), ..Default::default() },
    )
    .expect("un semis");
    assert_eq!(made.patches[0].points[0], [10.0, 20.0, -15.0]);
    // And the format **declares** that it asks for it, otherwise a front end would have to spell
    // “the csv wants a height” on its side: two truths, one of which drifts.
    let format = mesh::format_of("semis.csv").expect("un format");
    assert!(format.asks.contains(&mesh::Ask::Height));
}

#[test]
fn degrees_in_a_csv_are_refused_naming_the_format_that_reads_them() {
    // A `lon,lat` scatter read as metres lands beside the model and a thousand
    // times too small — the exact defect of the SIGEOL import of 19/09, which passes
    // `check` without saying anything. Refusing it costs a file to rewrite; reading it
    // costs a search of several hours.
    let why = mesh::read("semis.csv", "lon,lat,z\n2.32,48.97,38").expect_err("degrees");
    assert!(why.contains("GeoJSON"), "it names the format that can read them: {why}");
}

#[test]
fn a_geojson_is_carried_in_metres_and_the_zone_is_proposed() {
    // RFC 7946 fixes the system — WGS 84, in degrees — and a scene is in metres.
    // The EPSG code is not guessed for all that: it is **proposed** from the
    // longitude of the data, as `sigeol-boreholes` does.
    let text = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","geometry":{"type":"LineString",
         "coordinates":[[2.32,48.97,10],[2.33,48.98,20]]},"properties":{}}]}"#;
    let made = mesh::read("trace.geojson", text).expect("a trace");
    assert_eq!(nodes(&made), 2);
    // Metres, not degrees: it is the fact on which everything else depends.
    assert!(made.patches[0].points[0][0] > 100_000.0, "{:?}", made.patches[0].points[0]);
    // And the two positions are joined: a trace is a line.
    assert_eq!(segments(&made), vec![0, 1]);
}

#[test]
fn a_flat_geojson_is_refused_rather_than_laying_nans() {
    // RFC 7946 makes the altitude **optional**, so it is the ordinary case. A
    // `NaN` is not visible: it draws nothing and poisons every bounding
    // box that meets it — an empty scene, and nothing to say so.
    let text = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","geometry":{"type":"Point","coordinates":[2.32,48.97]},"properties":{}}]}"#;
    let why = mesh::read_or_ask("plat.geojson", text, &mesh::Given::default())
        .expect_err("no elevation");
    assert_eq!(why.missing, vec![mesh::Ask::Height]);
    let made = mesh::read_given(
        "plat.geojson",
        text,
        &mesh::Given { height: Some(42.0), projection: None },
    )
    .expect("un point");
    assert_eq!(made.patches[0].points[0][2], 42.0);
}

#[test]
fn a_file_that_carries_everything_asks_for_nothing() {
    // **The feedback of 19/09**: *“they ask for an epsg and a Z whereas
    // I think they are inside”*. That was right — the BRGM exports
    // write `[2.2953, 48.875, 58]`, third ordinate included, and the UTM zone
    // is deduced from their longitude.
    //
    // `Format::asks` says what a format **may** demand; it does not say what
    // this file does not carry. It is the reader that knows, and it alone,
    // so the order is: we try, and we only ask if it refuses for that reason.
    let text = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","geometry":{"type":"Point","coordinates":[2.2953,48.875,58]},"properties":{}}]}"#;
    let made = mesh::read_or_ask("unites.geojson", text, &mesh::Given::default()).expect("lisible");
    assert_eq!(made.patches[0].points[0][2], 58.0, "its own elevation, not an elevation asked for");
    // And the format still declares that it *may* ask for both: it is
    // the distinction between what a format sometimes demands and what is missing
    // here that is the whole subject.
    let format = mesh::format_of("unites.geojson").expect("un format");
    assert_eq!(format.asks, &[mesh::Ask::Projection, mesh::Ask::Height]);
}

/// **A reading says in which system it laid the degrees, and who
/// decided it** — the defect of the note of 24/09 (*“the geojson import won't
/// show me anything in the scene unless I give by hand the epsg to
/// reproject to”*).
///
/// Without a given code, the degrees went into the UTM zone of the first
/// position, silently: a scene in Lambert-93 received the file more than
/// a thousand kilometres from its objects, and the front end could not say so.
///
/// # Where the expectations come from
///
/// From the definition of the systems, not from a file: UTM zone 31 covers
/// 0° E–6° E (EPSG:32631), and at 49° N the UTM ordinate (counted from
/// the equator) and that of Lambert-93 (false northing of 6,600,000 m at 46.5° N,
/// EPSG:2154) differ by some 1,450 km — hence the threshold of a thousand.
///
/// # What would have to break to make it turn red
///
/// Make `proposed` always false (the proposal becomes mute again), or
/// ignore the given code (the two readings land in the same place, and
/// the gap that is the whole defect disappears from the assertion).
#[test]
fn a_geojson_says_in_which_system_it_laid_its_degrees() {
    let text = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","geometry":{"type":"LineString",
         "coordinates":[[2.32,48.97,10],[2.33,48.98,20]]},"properties":{}}]}"#;

    // Nothing given: the zone of its longitude, **said as a proposal**.
    let proposed = mesh::read_placed("trace.geojson", text, &mesh::Given::default()).expect("a trace");
    assert_eq!(
        proposed.placed,
        Some(mesh::Placed { epsg: 32631, proposed: true, degrees: false }),
    );

    // The code of the scene: **said as an answer**.
    let given = mesh::read_placed(
        "trace.geojson",
        text,
        &mesh::Given { height: None, projection: Some(2154) },
    )
    .expect("a trace");
    assert_eq!(given.placed, Some(mesh::Placed { epsg: 2154, proposed: false, degrees: false }));

    // And the gap that made the artefact disappear: the same positions, more
    // than a thousand kilometres apart depending on the system chosen.
    let north = |read: &mesh::Read| read.geometry.patches[0].points[0][1];
    assert!(
        (north(&given) - north(&proposed)).abs() > 1_000_000.0,
        "UTM {} contre Lambert-93 {}",
        north(&proposed),
        north(&given)
    );

    // Degrees kept as they are are named as such: laid in a
    // scene in metres, they land thousands of kilometres from anything.
    let kept = mesh::read_placed(
        "trace.geojson",
        text,
        &mesh::Given { height: None, projection: Some(4326) },
    )
    .expect("a trace");
    assert_eq!(kept.placed, Some(mesh::Placed { epsg: 4326, proposed: false, degrees: true }));
    assert_eq!(kept.geometry.patches[0].points[0], [2.32, 48.97, 10.0]);

    // A format without degrees says nothing: nothing was decided in its place.
    let obj = mesh::read_placed("carre.obj", SQUARE, &mesh::Given::default()).expect("un OBJ");
    assert!(obj.placed.is_none(), "{:?}", obj.placed);
    // And the earlier reading returns the same shape: `read_or_ask` is only a view.
    let before = mesh::read_or_ask("trace.geojson", text, &mesh::Given::default()).expect("a trace");
    assert_eq!(before.patches[0].points, proposed.geometry.patches[0].points);
}

#[test]
fn a_refusal_that_no_answer_would_lift_asks_for_nothing() {
    // The half that stops the form lying: a binary VTK is not
    // repaired by filling in a field, so `missing` is empty. Without this
    // distinction, the front end would propose a height for a file that no
    // height will save.
    let text = r#"<VTKFile type="PolyData"><PolyData><Piece>
 <Points><DataArray format="binary">AAAA</DataArray></Points>
</Piece></PolyData></VTKFile>"#;
    let why = mesh::read_or_ask("forme.vtp", text, &mesh::Given::default()).expect_err("binary");
    assert!(why.missing.is_empty(), "{:?}", why.missing);
    assert!(why.why.contains("binary"), "{}", why.why);
}

#[test]
fn an_ascii_vtp_returns_its_triangles_and_its_lines() {
    // What our own exporter writes — `DataArray`s in ascii — so this
    // check is also that of the full round trip: what comes out must go back in.
    let text = r#"<?xml version="1.0"?>
<VTKFile type="PolyData"><PolyData><Piece NumberOfPoints="4">
 <Points><DataArray type="Float64" NumberOfComponents="3" format="ascii">
  0 0 0  1 0 0  0 1 0  1 1 0
 </DataArray></Points>
 <Polys>
  <DataArray type="Int64" Name="connectivity" format="ascii">0 1 2</DataArray>
  <DataArray type="Int64" Name="offsets" format="ascii">3</DataArray>
 </Polys>
 <Lines>
  <DataArray type="Int64" Name="connectivity" format="ascii">2 3</DataArray>
  <DataArray type="Int64" Name="offsets" format="ascii">2</DataArray>
 </Lines>
</Piece></PolyData></VTKFile>"#;
    let made = mesh::read("forme.vtp", text).expect("a shape");
    assert_eq!(nodes(&made), 4);
    assert_eq!(triangles(&made), vec![0, 1, 2]);
    assert_eq!(segments(&made), vec![2, 3]);
}

#[test]
fn offsets_are_cell_ends_not_lengths() {
    // **The trap of the format**, and it gives a plausible and wrong mesh: cell
    // `i` goes from `offsets[i-1]` to `offsets[i]`. Read as lengths,
    // the first cells are right and the following ones slide — which
    // no assertion on a number of points would see.
    let text = r#"<VTKFile type="UnstructuredGrid"><UnstructuredGrid><Piece>
 <Points><DataArray format="ascii">0 0 0  1 0 0  0 1 0  1 1 0  2 0 0  2 1 0</DataArray></Points>
 <Cells>
  <DataArray Name="connectivity" format="ascii">0 1 2  3 4 5</DataArray>
  <DataArray Name="offsets" format="ascii">3 6</DataArray>
 </Cells>
</Piece></UnstructuredGrid></VTKFile>"#;
    let made = mesh::read("grille.vtu", text).expect("a grid");
    // Two triangles, the second on 3,4,5 — and not on 3,4,5 shifted.
    assert_eq!(triangles(&made), vec![0, 1, 2, 3, 4, 5]);
}

/// A unit cube as a `.vtu` hexahedron, with the corner order VTK fixes.
///
/// VTK file formats, "Cell types" (VTK_HEXAHEDRON = 12): corners 0-3 run
/// counter-clockwise around the bottom face seen from above, corners 4-7 are the
/// points directly above 0-3 in the same order.
const CUBE_VTU: &str = r#"<VTKFile type="UnstructuredGrid"><UnstructuredGrid><Piece>
 <Points><DataArray format="ascii">0 0 0  1 0 0  1 1 0  0 1 0  0 0 1  1 0 1  1 1 1  0 1 1</DataArray></Points>
 <Cells>
  <DataArray Name="connectivity" format="ascii">0 1 2 3 4 5 6 7</DataArray>
  <DataArray Name="offsets" format="ascii">8</DataArray>
  <DataArray Name="types" format="ascii">12</DataArray>
 </Cells>
</Piece></UnstructuredGrid></VTKFile>"#;

fn cube_points() -> [[f64; 3]; 8] {
    [
        [0., 0., 0.],
        [1., 0., 0.],
        [1., 1., 0.],
        [0., 1., 0.],
        [0., 0., 1.],
        [1., 0., 1.],
        [1., 1., 1.],
        [0., 1., 1.],
    ]
}

/// Every triangle must lie in one face of the cube and face outward. A fan of
/// the eight corners from corner 0 (the cell read as a polygon) puts triangles
/// across the body, which this rejects.
fn assert_cube_faces(read: &energyml_geometry::geometry::Geometry) {
    let pts = cube_points();
    let tri = triangles(read);
    assert_eq!(tri.len(), 6 * 2 * 3, "six faces, two triangles each");
    for t in tri.chunks(3) {
        let [a, b, c] = [pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]];
        let flat = (0..3).any(|axis| a[axis] == b[axis] && b[axis] == c[axis]);
        assert!(flat, "triangle {t:?} leaves its face: a hexahedron is not a polygon");
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let centre = [(a[0] + b[0] + c[0]) / 3. - 0.5, (a[1] + b[1] + c[1]) / 3. - 0.5, (a[2] + b[2] + c[2]) / 3. - 0.5];
        let outward: f64 = (0..3).map(|k| n[k] * centre[k]).sum();
        assert!(outward > 0.0, "triangle {t:?} faces inward");
    }
}

#[test]
fn a_vtu_hexahedron_is_read_by_its_faces_and_not_as_a_fan() {
    // The grid the front end exports as `.vtu` and reads back: `types` says 12, so
    // the eight points are a hexahedron's corners. Read as a polygon (what this
    // reader did when it ignored `types`) the fan from corner 0 draws diagonals
    // through the cell.
    let made = mesh::read("grille.vtu", CUBE_VTU).expect("a grid");
    assert_cube_faces(&made);
}

#[test]
fn the_same_hexahedron_reads_alike_in_legacy_vtk_and_in_xml() {
    let legacy = "# vtk DataFile Version 3.0
x
ASCII
DATASET UNSTRUCTURED_GRID
POINTS 8 double
0 0 0
1 0 0
1 1 0
0 1 0
0 0 1
1 0 1
1 1 1
0 1 1
CELLS 1 9
8 0 1 2 3 4 5 6 7
CELL_TYPES 1
12
";
    let a = mesh::read_vtk(legacy).expect("classique");
    let b = mesh::read("cube.vtu", CUBE_VTU).expect("xml");
    assert_cube_faces(&a);
    assert_eq!(triangles(&a), triangles(&b));
}

#[test]
fn a_mixed_type_vtu_reads_each_cell_by_its_type() {
    // A triangle (5), then a hexahedron (12), then a line (3): offsets are ends,
    // types decide what the points mean.
    let text = r#"<VTKFile type="UnstructuredGrid"><UnstructuredGrid><Piece>
 <Points><DataArray format="ascii">0 0 0  1 0 0  1 1 0  0 1 0  0 0 1  1 0 1  1 1 1  0 1 1</DataArray></Points>
 <Cells>
  <DataArray Name="connectivity" format="ascii">0 1 2  0 1 2 3 4 5 6 7  0 6</DataArray>
  <DataArray Name="offsets" format="ascii">3 11 13</DataArray>
  <DataArray Name="types" format="ascii">5 12 3</DataArray>
 </Cells>
</Piece></UnstructuredGrid></VTKFile>"#;
    let made = mesh::read("mixte.vtu", text).expect("un vtu");
    assert_eq!(triangles(&made).len(), 3 + 6 * 2 * 3);
    assert_eq!(&triangles(&made)[..3], &[0, 1, 2]);
    assert_eq!(segments(&made), vec![0, 6]);
}

#[test]
fn a_polyhedron_vtu_is_read_by_its_declared_faces() {
    // VTK_POLYHEDRON = 42: the connectivity lists the cell's points, the faces
    // live in `faces` (numFaces, then per face its size and points) and
    // `faceoffsets` gives where each cell's run ends. A tetrahedron.
    let text = r#"<VTKFile type="UnstructuredGrid"><UnstructuredGrid><Piece>
 <Points><DataArray format="ascii">0 0 0  1 0 0  0 1 0  0 0 1</DataArray></Points>
 <Cells>
  <DataArray Name="connectivity" format="ascii">0 1 2 3</DataArray>
  <DataArray Name="offsets" format="ascii">4</DataArray>
  <DataArray Name="types" format="ascii">42</DataArray>
  <DataArray Name="faces" format="ascii">4  3 0 2 1  3 0 1 3  3 1 2 3  3 2 0 3</DataArray>
  <DataArray Name="faceoffsets" format="ascii">17</DataArray>
 </Cells>
</Piece></UnstructuredGrid></VTKFile>"#;
    let made = mesh::read("tetra.vtu", text).expect("un polyedre");
    assert_eq!(triangles(&made), vec![0, 2, 1, 0, 1, 3, 1, 2, 3, 2, 0, 3]);
}

#[test]
fn an_unknown_cell_type_in_a_vtu_is_refused_naming_it() {
    // 11 is VTK_VOXEL, whose corner order differs from the hexahedron's:
    // reading it as one would draw twisted faces, so it is refused.
    let text = CUBE_VTU.replace("12", "11");
    let why = mesh::read("voxel.vtu", &text).expect_err("un voxel");
    assert!(why.contains("11"), "{why}");
}

#[test]
fn a_triangle_strip_is_not_a_fan() {
    // VTK file formats, "Triangle strips": strip `0 1 2 3` is the triangles
    // (0,1,2) and (2,1,3) — every other one reversed — not a fan from point 0.
    let text = r#"<VTKFile type="PolyData"><PolyData><Piece>
 <Points><DataArray format="ascii">0 0 0  1 0 0  0 1 0  1 1 0</DataArray></Points>
 <Strips>
  <DataArray Name="connectivity" format="ascii">0 1 2 3</DataArray>
  <DataArray Name="offsets" format="ascii">4</DataArray>
 </Strips>
</Piece></PolyData></VTKFile>"#;
    let made = mesh::read("bande.vtp", text).expect("a strip");
    assert_eq!(triangles(&made), vec![0, 1, 2, 2, 1, 3]);
}

#[test]
fn a_binary_xml_vtk_is_refused_naming_its_encoding() {
    // `binary` and `appended` exist and we write neither. Reading them
    // needs base64, zlib and the offset of the blocks — three things that would be done
    // by halves. A refusal that names the encoding says what to do; a reader that
    // returned an empty mesh would say that the file is.
    let text = r#"<VTKFile type="PolyData"><PolyData><Piece>
 <Points><DataArray format="binary">AAAA</DataArray></Points>
</Piece></PolyData></VTKFile>"#;
    let why = mesh::read("forme.vtp", text).expect_err("binary");
    assert!(why.contains("binary") && why.contains("ascii"), "{why}");
}

#[test]
fn the_six_formats_are_offered_and_each_says_what_it_asks_for() {
    // The list is the only one: a front end asks it what is readable rather than
    // spelling extensions on its own side. This check is what makes a
    // seventh format appear in the picker without a line changing.
    let names: Vec<&str> = mesh::FORMATS.iter().map(|f| f.name).collect();
    assert_eq!(names, vec!["obj", "vtk", "vtp", "vtu", "csv", "geojson"]);
    // Those that ask for nothing say so too: a non-empty `asks` is an
    // exception, not the ordinary.
    for format in mesh::FORMATS {
        let wanted: &[mesh::Ask] = match format.name {
            "csv" => &[mesh::Ask::Height],
            "geojson" => &[mesh::Ask::Projection, mesh::Ask::Height],
            _ => &[],
        };
        assert_eq!(format.asks, wanted, "`{}`", format.name);
    }
}
