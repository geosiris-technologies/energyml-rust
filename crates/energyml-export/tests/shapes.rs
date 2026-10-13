//! Choosing what form each type comes out in.
//!
//! Three things have to hold for this to be usable rather than clever:
//!
//! 1. the **order** of the rules is the caller's, and the first match wins —
//!    otherwise a general rule and an exception cannot be written on one line;
//! 2. a rule that **cannot be honoured** fails by name, rather than quietly
//!    falling back to something else and teaching the caller it worked;
//! 3. a **typo** fails on the command line, not after twenty files have landed
//!    in a directory.

use energyml_export::shapes::{Selector, Shapes};
use energyml_export::{Exporters, Options};
use energyml_geometry::shape::Shape;
use energyml_geometry::{Geometry, Patch, Placement, SceneObject, Topology};

fn object(qualified_type: &str, topology: Topology, points: Vec<[f64; 3]>) -> SceneObject {
    SceneObject {
        uuid: "u".into(),
        uri: format!("eml:///{qualified_type}(u)"),
        title: "t".into(),
        qualified_type: qualified_type.into(),
        geometry: Geometry {
            kind: "test",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points,
                topology,
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

fn a_grid() -> SceneObject {
    let mut points = Vec::new();
    for j in 0..3 {
        for i in 0..3 {
            points.push([i as f64, j as f64, 0.0]);
        }
    }
    object(
        "resqml20.obj_Grid2dRepresentation",
        Topology::Grid {
            fastest: 3,
            slowest: 3,
        },
        points,
    )
}

fn a_polyline_set() -> SceneObject {
    object(
        "resqml22.PolylineSetRepresentation",
        Topology::Polylines(vec![3]),
        vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 1.0, 0.0]],
    )
}

fn a_triangulated_set() -> SceneObject {
    object(
        "resqml22.TriangulatedSetRepresentation",
        Topology::Triangles(vec![[0, 1, 2]]),
        vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
    )
}

#[test]
fn a_substring_matches_both_releases_of_a_type() {
    // The whole reason the pattern is a substring: `resqml20.obj_Grid2d…` and
    // `resqml22.Grid2d…` are the same concept spelled two ways, and a caller
    // should not have to know that to write one rule.
    let rules = Shapes::new().with(Selector::parse("Grid2d"), Shape::Points);
    assert_eq!(
        rules.shape_for("resqml20.obj_Grid2dRepresentation"),
        Shape::Points
    );
    assert_eq!(
        rules.shape_for("resqml22.Grid2dRepresentation"),
        Shape::Points
    );
    assert_eq!(
        rules.shape_for("resqml22.TriangulatedSetRepresentation"),
        Shape::Auto,
        "a type no rule names keeps its natural shape"
    );
}

#[test]
fn matching_ignores_case() {
    let rules = Shapes::new().with(Selector::parse("GRID2D"), Shape::Surface);
    assert_eq!(
        rules.shape_for("resqml20.obj_grid2drepresentation"),
        Shape::Surface
    );
}

#[test]
fn the_first_matching_rule_wins_so_an_exception_can_precede_a_default() {
    // This is the ordering that makes "grids as polygons, everything else as
    // nodes" one line. Reverse it and the `*` swallows the grid rule — which is
    // exactly why the order has to be the caller's and not sorted here.
    let rules = Shapes::new()
        .parse_rule("Grid2d=surface")
        .unwrap()
        .parse_rule("*=points")
        .unwrap();
    assert_eq!(
        rules.shape_for("resqml22.Grid2dRepresentation"),
        Shape::Surface
    );
    assert_eq!(
        rules.shape_for("resqml22.TriangulatedSetRepresentation"),
        Shape::Points
    );
}

#[test]
fn a_star_before_a_specific_rule_shadows_it_and_that_is_the_callers_choice() {
    // Stated as a test rather than left implicit: first-match-wins means a
    // careless order silently loses a rule, and someone will hit it. The
    // behaviour is predictable, which is the property that matters.
    let rules = Shapes::new()
        .parse_rule("*=points")
        .unwrap()
        .parse_rule("Grid2d=surface")
        .unwrap();
    assert_eq!(
        rules.shape_for("resqml22.Grid2dRepresentation"),
        Shape::Points
    );
}

#[test]
fn a_rule_that_cannot_be_honoured_fails_and_names_the_object() {
    // The case the report asked for by name: a set of polylines is not a
    // surface. A line is not a boundary unless it closes, and RESQML states
    // closure in a separate boolean array this build does not read — so filling
    // one would draw a surface nobody described.
    let options = Options::new()
        .with_native_crs()
        .with_placement(Placement::Local)
        .with_shapes(Shapes::new().parse_rule("PolylineSet=surface").unwrap());

    let exporters = Exporters::builtin();
    let mut buffer = Vec::new();
    let error = exporters
        .get("geojson")
        .unwrap()
        .write(&a_polyline_set(), &mut buffer, &options)
        .expect_err("a polyline set must not silently come out as something else");

    let text = error.to_string();
    assert!(
        text.contains("surface") && text.contains("polylines"),
        "the refusal must name both ends, said: {text}"
    );
}

#[test]
fn every_topology_can_be_asked_for_points() {
    // The one conversion that is always available, because dropping what joins
    // the nodes needs nothing from the file.
    for object in [a_grid(), a_polyline_set(), a_triangulated_set()] {
        assert!(
            object.geometry.can_be(Shape::Points),
            "{} should be reducible to points",
            object.qualified_type
        );
    }
}

#[test]
fn nothing_flat_can_be_asked_for_a_volume() {
    // A triangulated surface is not a solid. Refusing is not pedantry: the
    // alternative is an export that claims to hold cells and holds faces.
    for object in [a_grid(), a_polyline_set(), a_triangulated_set()] {
        assert!(
            !object.geometry.can_be(Shape::Volume),
            "{} must not pass as a volume",
            object.qualified_type
        );
    }
}

#[test]
fn a_grid_asked_for_as_a_surface_has_one_cell_per_lattice_square() {
    let grid = a_grid();
    let surface = grid.geometry.as_shape(Shape::Surface).unwrap();
    match &surface.patches[0].topology {
        Topology::Quads(quads) => assert_eq!(quads.len(), 4, "a 3 × 3 lattice has 2 × 2 cells"),
        other => panic!("expected quadrilaterals, got {other:?}"),
    }
    // The points are untouched, which is what lets a point-scoped property come
    // through a reshape unchanged.
    assert_eq!(surface.patches[0].points, grid.geometry.patches[0].points);
}

#[test]
fn a_grids_cells_are_wound_counterclockwise_by_measurement() {
    // Not by convention: no RESQML file says which way its lattice runs, so the
    // signed area is measured and the ring reversed when it comes out negative.
    // RFC 7946 §3.1.6 asks an exterior ring to be counterclockwise, and a
    // renderer reads that winding as front-facing.
    //
    // This grid is built with its fastest axis running **backwards**, so a ring
    // taken in index order would be clockwise. If the winding were assumed
    // rather than measured, this is the case that would come out inside-out.
    let mut points = Vec::new();
    for j in 0..3 {
        for i in 0..3 {
            points.push([-(i as f64), j as f64, 0.0]);
        }
    }
    let grid = object(
        "resqml22.Grid2dRepresentation",
        Topology::Grid {
            fastest: 3,
            slowest: 3,
        },
        points.clone(),
    );

    let surface = grid.geometry.as_shape(Shape::Surface).unwrap();
    let Topology::Quads(quads) = &surface.patches[0].topology else {
        panic!("expected quadrilaterals");
    };
    for quad in quads {
        let mut area = 0.0;
        for corner in 0..4 {
            let a = points[quad[corner] as usize];
            let b = points[quad[(corner + 1) % 4] as usize];
            area += a[0] * b[1] - b[0] * a[1];
        }
        assert!(area > 0.0, "every ring should be counterclockwise, got {area}");
    }
}

#[test]
fn a_typo_in_a_rule_is_refused_with_the_alternatives() {
    // On the command line, by someone who will not go looking for documentation
    // to find out that `surface` was spelled `srufaec`.
    let error = Shapes::new().parse_rule("Grid2d=srufaec").unwrap_err();
    assert!(error.contains("srufaec"), "{error}");
    assert!(error.contains("surface"), "the alternatives must be listed: {error}");

    let error = Shapes::new().parse_rule("Grid2d").unwrap_err();
    assert!(error.contains("PATTERN=SHAPE"), "{error}");
}

#[test]
fn the_plural_and_the_singular_both_parse() {
    // Refusing `polygons` because the word in the enum is `surface` teaches
    // nothing and costs a round trip through the documentation.
    for spelling in ["surface", "surfaces", "polygon", "polygons", "MultiPolygon"] {
        assert_eq!(Shape::parse(spelling), Some(Shape::Surface), "{spelling}");
    }
    for spelling in ["point", "points", "MultiPoint"] {
        assert_eq!(Shape::parse(spelling), Some(Shape::Points), "{spelling}");
    }
    assert_eq!(Shape::parse("nonsense"), None);
}

#[test]
fn no_rules_at_all_means_every_object_in_its_natural_shape() {
    let rules = Shapes::new();
    assert!(rules.is_empty());
    assert_eq!(rules.shape_for("anything.at.all"), Shape::Auto);
}

// --- Summaries: where the object is, rather than what it is -----------------

/// Nine nodes in an L, so a convex hull is *not* just the bounding box: the
/// inner corner of the L must be dropped, and a test on a square could not tell
/// a hull from a box.
fn an_l_shape() -> SceneObject {
    let coords = [
        (0.0, 0.0), (1.0, 0.0), (2.0, 0.0),
        (0.0, 1.0), (1.0, 1.0), (2.0, 1.0),
        (0.0, 2.0), (1.0, 2.0),
    ];
    object(
        "resqml22.PointSetRepresentation",
        Topology::Points,
        coords.iter().map(|(x, y)| [*x, *y, 0.0]).collect(),
    )
}

#[test]
fn an_outline_is_one_ring_and_it_is_convex() {
    let hull = an_l_shape().geometry.as_shape(Shape::Outline).unwrap();
    assert_eq!(hull.patches.len(), 1, "a summary is one patch");
    let Topology::Polygons(rings) = &hull.patches[0].topology else {
        panic!("expected one polygon, got {:?}", hull.patches[0].topology);
    };
    assert_eq!(rings.len(), 1);

    let corners: Vec<[f64; 2]> = rings[0]
        .iter()
        .map(|i| {
            let p = hull.patches[0].points[*i as usize];
            [p[0], p[1]]
        })
        .collect();

    // Counterclockwise, which is what RFC 7946 asks of an exterior ring.
    let mut area = 0.0;
    for i in 0..corners.len() {
        let a = corners[i];
        let b = corners[(i + 1) % corners.len()];
        area += a[0] * b[1] - b[0] * a[1];
    }
    assert!(area > 0.0, "the ring should be counterclockwise, got {area}");

    // Every original node inside or on it. The check that says "hull" rather
    // than "some polygon": a wrong sweep produces a smaller ring that still
    // looks plausible on a plot.
    for point in &an_l_shape().geometry.patches[0].points {
        let inside = (0..corners.len()).all(|i| {
            let a = corners[i];
            let b = corners[(i + 1) % corners.len()];
            (b[0] - a[0]) * (point[1] - a[1]) - (b[1] - a[1]) * (point[0] - a[0]) >= -1e-9
        });
        assert!(inside, "{point:?} fell outside its own hull");
    }

    // And it is a *hull*, not the bounding box: (2, 2) is a box corner that no
    // node occupies, so it must not be there.
    assert!(
        !corners.iter().any(|c| c[0] == 2.0 && c[1] == 2.0),
        "the bounding-box corner is outside the data and must not appear"
    );
}

#[test]
fn a_centroid_is_one_point_and_it_is_the_mean() {
    let summary = an_l_shape().geometry.as_shape(Shape::Centroid).unwrap();
    assert_eq!(summary.point_count(), 1);
    assert!(matches!(summary.patches[0].topology, Topology::Points));
    // x: 0+1+2+0+1+2+0+1 = 7 over 8 nodes; y: 0+0+0+1+1+1+2+2 = 7 over 8.
    // Both 0.875, and both **off-centre** — which is the point worth pinning:
    // the mean of the nodes is pulled by where the sampling is dense, and it is
    // not the centroid of the area. Documented on `Shape::Centroid`.
    let mean = summary.patches[0].points[0];
    assert!((mean[0] - 0.875).abs() < 1e-12, "{mean:?}");
    assert!((mean[1] - 0.875).abs() < 1e-12, "{mean:?}");
}

#[test]
fn a_summary_of_a_whole_object_is_one_hull_and_not_one_per_patch() {
    // Eleven patches of a cut surface have one outline between them. Summarising
    // per patch would give eleven overlapping polygons and answer a question
    // nobody asked.
    let mut object = an_l_shape();
    let second = Patch {
        index: 1,
        node_offset: 8,
        points: vec![[10.0, 10.0, 0.0], [11.0, 10.0, 0.0], [10.0, 11.0, 0.0]],
        topology: Topology::Points,
        closed: Vec::new(),
    };
    object.geometry.patches.push(second);

    let hull = object.geometry.as_shape(Shape::Outline).unwrap();
    assert_eq!(hull.patches.len(), 1);
    let Topology::Polygons(rings) = &hull.patches[0].topology else {
        panic!("expected one polygon");
    };
    assert_eq!(rings.len(), 1);
    let far = hull.patches[0]
        .points
        .iter()
        .any(|p| p[0] > 9.0 && p[1] > 9.0);
    assert!(far, "the second patch must be inside the outline too");
}

#[test]
fn a_summary_drops_the_properties_because_it_has_nowhere_to_put_them() {
    // The failure this forbids: four hull corners keeping an array of nine node
    // values, lined up against corners it was never measured on.
    use energyml_geometry::scene::{Attribute, AttributeScope};

    let mut object = an_l_shape();
    object.attributes.push(Attribute {
        uuid: "p".into(),
        name: "depth".into(),
        kind: None,
        uom: None,
        scope: AttributeScope::Points,
        components: 1,
        values: (0..8).map(|i| i as f64).collect(),
        lut: Vec::new(),
    });

    let summary = object.as_shape(Shape::Outline).unwrap();
    assert!(
        summary.attributes.is_empty(),
        "a summary carries no property, and must not pretend to"
    );
}

#[test]
fn an_outline_of_a_line_of_points_is_not_a_polygon() {
    // Three collinear nodes bound no area. A degenerate ring would validate,
    // draw as nothing, and quietly claim to be a footprint — so the points
    // themselves are the honest answer, and the geometry type says so.
    let flat = object(
        "resqml22.PointSetRepresentation",
        Topology::Points,
        vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
    );
    let summary = flat.geometry.as_shape(Shape::Outline).unwrap();
    assert!(
        matches!(summary.patches[0].topology, Topology::Points),
        "got {:?}",
        summary.patches[0].topology
    );
}

#[test]
fn geojson_names_the_singular_types_for_a_summary() {
    // OSDU's `SpatialGeometryTypeID` distinguishes a Polygon from a
    // MultiPolygon and a Point from a MultiPoint, so wrapping a lone ring would
    // name the wrong type in the field whose job is to say which one it is.
    let exporters = Exporters::builtin();

    for (shape, expected) in [("outline", "\"Polygon\""), ("centroid", "\"Point\"")] {
        let options = Options::new()
            .with_native_crs()
            .with_placement(Placement::Local)
            .with_shapes(Shapes::new().parse_rule(&format!("*={shape}")).unwrap());
        let mut buffer = Vec::new();
        exporters
            .get("geojson")
            .unwrap()
            .write(&an_l_shape(), &mut buffer, &options)
            .unwrap();
        let text = String::from_utf8(buffer).unwrap();
        assert!(
            text.contains(&format!("\"type\":{expected}")),
            "`{shape}` should produce {expected}, got:\n{text}"
        );
        assert!(
            !text.contains("\"type\":\"Multi"),
            "`{shape}` must not be wrapped in a Multi- type:\n{text}"
        );
    }
}

#[test]
fn every_feature_carries_a_bounding_box() {
    // Optional in RFC 7946 and in OSDU's schema, written anyway: it is what
    // lets a viewer or an index decide whether to open a ten-megabyte file.
    let options = Options::new()
        .with_native_crs()
        .with_placement(Placement::Local);
    let exporters = Exporters::builtin();
    let mut buffer = Vec::new();
    exporters
        .get("geojson")
        .unwrap()
        .write(&an_l_shape(), &mut buffer, &options)
        .unwrap();
    let text = String::from_utf8(buffer).unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let bbox = value["features"][0]["bbox"].as_array().expect("a bbox");
    // RFC 7946 §5: the axes interleave — west, south, low, east, north, high —
    // and not the two corners one after the other.
    assert_eq!(bbox.len(), 6);
    assert_eq!(bbox[0].as_f64(), Some(0.0));
    assert_eq!(bbox[1].as_f64(), Some(0.0));
    assert_eq!(bbox[3].as_f64(), Some(2.0));
    assert_eq!(bbox[4].as_f64(), Some(2.0));
}

// ---------------------------------------------------------------------------
// The wireframe - the way out of the refusal that plan section 6 left "to be
// decided".
//
// Decided: the **cell edges**, each one once, never the triangulation - the
// precedent is a volume's skin, a boundary derived from the data and not an
// invention. These tests defend the three traps of the decision: a quad's
// diagonal is not an edge, a shared edge is drawn only once, and a per-cell
// attribute does not survive - a cell value has no honest place on an edge two
// cells share.

#[test]
fn a_grid_asked_for_lines_gives_its_cell_edges_without_the_diagonals() {
    let grid = a_grid();
    let drawn = grid.as_shape(Shape::Lines).expect("edges are derivable");
    let Topology::Segments(edges) = &drawn.geometry.patches[0].topology else {
        panic!("a wireframe is indexed segments");
    };
    // 3 × 3 nodes, 2 × 2 cells: 12 unique edges.
    assert_eq!(edges.len(), 12);
    for pair in edges {
        let (a, b) = (pair[0] as i64, pair[1] as i64);
        let (ai, aj, bi, bj) = (a % 3, a / 3, b % 3, b / 3);
        // Every edge follows an axis of the grid: a diagonal - the artefact of
        // rendering in triangles - would be at distance 2.
        assert_eq!((ai - bi).abs() + (aj - bj).abs(), 1, "{pair:?} is a diagonal");
    }
}

#[test]
fn a_shared_edge_is_stored_once() {
    let two = object(
        "resqml22.TriangulatedSetRepresentation",
        Topology::Triangles(vec![[0, 1, 2], [1, 3, 2]]),
        vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
        ],
    );
    let drawn = two.as_shape(Shape::Lines).expect("edges");
    let Topology::Segments(edges) = &drawn.geometry.patches[0].topology else {
        panic!("segments");
    };
    // Two triangles, six half-edges, of which 1-2 is shared: five unique.
    assert_eq!(edges.len(), 5);
}

#[test]
fn a_volume_wireframe_is_the_edges_of_its_boundary() {
    let cube = object(
        "resqml20.obj_IjkGridRepresentation",
        Topology::Hexahedra {
            cells: vec![[0, 1, 2, 3, 4, 5, 6, 7]],
            dimensions: [1, 1, 1],
            index: Vec::new(),
        },
        vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ],
    );
    let drawn = cube.as_shape(Shape::Lines).expect("edges");
    let Topology::Segments(edges) = &drawn.geometry.patches[0].topology else {
        panic!("segments");
    };
    // A cube's wireframe is its skin's: twelve edges.
    assert_eq!(edges.len(), 12);
}

#[test]
fn the_wireframe_keeps_point_values_and_drops_cell_values() {
    use energyml_geometry::scene::{Attribute, AttributeScope};
    let mut grid = a_grid();
    grid.attributes = vec![
        Attribute {
            uuid: "pp".into(),
            name: "per point".into(),
            kind: None,
            uom: None,
            scope: AttributeScope::Points,
            components: 1,
            values: (0..9).map(f64::from).collect(),
            lut: Vec::new(),
        },
        Attribute {
            uuid: "pc".into(),
            name: "per cell".into(),
            kind: None,
            uom: None,
            scope: AttributeScope::Cells,
            components: 1,
            values: (0..4).map(f64::from).collect(),
            lut: Vec::new(),
        },
    ];
    let drawn = grid.as_shape(Shape::Lines).expect("edges");
    let names: Vec<&str> = drawn.attributes.iter().map(|a| a.name.as_str()).collect();
    // The points do not move, so their values stay aligned; the cell count
    // changes (12 edges against 4 cells), so a per-cell value no longer has an
    // honest place and is dropped.
    assert_eq!(names, vec!["per point"], "{names:?}");
}

#[test]
fn obj_writes_a_wireframe_as_l_statements() {
    // End to end: the `Grid2d=lines` rule crosses the exporter, and the file
    // holds one `l` line per edge - the view/export equivalence section 6
    // called for.
    let exporters = Exporters::builtin();
    let exporter = exporters.get("obj").expect("obj exists");
    let options =
        Options::new().with_shapes(Shapes::new().parse_rule("Grid2d=lines").unwrap());
    let mut out = Vec::new();
    exporter
        .write(&a_grid(), &mut out, &options)
        .expect("a grid exports as its edges");
    let text = String::from_utf8(out).expect("utf-8");
    let segments = text.lines().filter(|line| line.starts_with("l ")).count();
    assert_eq!(segments, 12, "{text}");
}
