//! A shape read from a file is placed in the frame written; a shape of the
//! package, against a frame created from `sourceEpsg`, is refused.
//!
//! The defect of 29/09: `create pointset --from
//! shape=… --set sourceEpsg=3949 --set project=2154` wrote CC49 numbers under
//! a Lambert-93 label, a thousand kilometres from their place, and the package
//! validated clean — only a feature collection went through `energyml-proj`.
//!
//! **The reference is the definition of the two systems, not this code.**
//! EPSG:3949 (RGF93 / CC49) is a Lambert conformal conic with its origin at
//! 3° E, 49° N and a false origin of (1 700 000, 8 200 000) m; EPSG:2154
//! (RGF93 / Lambert-93) has its central meridian at 3° E and a false easting
//! of 700 000 m, on the same datum (EPSG registry, the two projections'
//! parameters). So the CC49 point (1 700 000, 8 200 000) is 3° E, 49° N, and
//! in Lambert-93 its easting is **exactly 700 000 m** — the central meridian
//! maps to the false easting — and its northing a little under 6.88 million
//! (46.5° N maps to 6 600 000, and a degree of latitude is ~111 km). Written
//! raw, as before the fix, the easting is 1 700 000: the test goes red by a
//! million metres.

use serde_json::json;

use energyml_build::operation::{Choices, Identity, Numbers, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When { creation: "2026-09-30T00:00:00Z".into(), originator: "test".into() }
}

/// The CC49 false origin, and a point 1 km east and 1 km north of it, with
/// depths that no horizontal reprojection may touch.
fn cc49_nodes() -> Vec<[f64; 3]> {
    vec![[1_700_000.0, 8_200_000.0, -10.0], [1_701_000.0, 8_201_000.0, -20.0]]
}

fn patch(points: Vec<[f64; 3]>, topology: Topology) -> Patch {
    Patch { index: 0, node_offset: 0, points, topology, closed: Vec::new() }
}

/// A shape read from a file: no object of the package behind it.
fn from_a_file(topology: Topology) -> Pieces {
    Pieces::empty().with("trace.csv", Source::Shape { geometry: Geometry { kind: "mesh", patches: vec![patch(cc49_nodes(), topology)] }, closed: None })
}

/// The same numbers, but as an object of the package, in its own frame.
fn from_the_package() -> Pieces {
    Pieces::empty()
        .with("horizon", Source::Shape { geometry: Geometry { kind: "pointset", patches: vec![patch(cc49_nodes(), Topology::Points)] }, closed: None })
        .with(
            "horizon (object)",
            Source::Object(Known {
                reference: Some(Ref::new("bbbbbbbb-0000-4000-8000-000000000002", "resqml22.PointSetRepresentation", "Toit du Lutétien")),
                crs: Some(Ref::new("aaaaaaaa-0000-4000-8000-000000000001", "eml23.LocalEngineeringCompoundCrs", "CC49 of the package")),
                crs_epsg: Some(3949),
                vertical_uom: Some("m".into()),
                ..Default::default()
            }),
        )
}

fn cc49_to_lambert93() -> Choices {
    Choices::new().set("sourceEpsg", 3949).set("project", 2154).set("vertical", 5720).set("title", "points")
}

/// The first dataset's triplets — the coordinates the plan writes.
fn written(plan: &Plan) -> Vec<[f64; 3]> {
    let Numbers::F64(values) = &plan.datasets[0].values else { panic!("points are written as doubles") };
    values.chunks(3).map(|c| [c[0], c[1], c[2]]).collect()
}

/// The CC49 origin, read back in Lambert-93, against the two definitions.
fn assert_is_the_cc49_origin_in_lambert_93(point: [f64; 3]) {
    assert!((point[0] - 700_000.0).abs() < 0.001, "3° E is Lambert-93's central meridian, so its easting is 700 000 m; got {}", point[0]);
    assert!((6_850_000.0..6_880_000.0).contains(&point[1]), "49° N in Lambert-93 is a little under 6.88 million; got {}", point[1]);
    assert_eq!(point[2], -10.0, "a horizontal reprojection leaves the depth as it was");
}

#[test]
fn a_point_set_from_a_file_is_placed_from_source_epsg_into_project() {
    // To break: copy the shape's nodes as they came (the defect of 29/09).
    let registry = Registry::new();
    let plan = registry.plan("pointset", &from_a_file(Topology::Points), &cc49_to_lambert93(), &Identity::derived(), &when()).unwrap();
    let points = written(&plan);
    assert_eq!(points.len(), 2);
    assert_is_the_cc49_origin_in_lambert_93(points[0]);
    // The second node stays about 1 km east and 1 km north of the first:
    // the two systems are conformal on the same datum, so distances barely move.
    let (dx, dy) = (points[1][0] - points[0][0], points[1][1] - points[0][1]);
    assert!((dx - 1000.0).abs() < 5.0 && (dy - 1000.0).abs() < 5.0, "the offset became ({dx}, {dy})");
    // And the frame written says Lambert-93.
    let frame = plan.objects.iter().find(|o| o.qualified_type.ends_with("ProjectedCrs")).expect("the projected half, created");
    assert_eq!(frame.json["AbstractProjectedCrs"]["EpsgCode"], json!(2154));
}

#[test]
fn a_polyline_set_from_a_file_is_placed_the_same_way() {
    // To break: place the features and forget the shapes, in `polylines`.
    let registry = Registry::new();
    let plan = registry.plan("polylines", &from_a_file(Topology::Polylines(vec![2])), &cc49_to_lambert93(), &Identity::derived(), &when()).unwrap();
    assert_is_the_cc49_origin_in_lambert_93(written(&plan)[0]);
}

#[test]
fn a_shape_of_the_package_against_a_frame_created_from_source_epsg_is_refused_by_name() {
    // To break: reproject a shape whose numbers are in its own frame from a
    // code they are not in — the same defect by another door.
    let registry = Registry::new();
    let why = registry.plan("pointset", &from_the_package(), &cc49_to_lambert93(), &Identity::derived(), &when()).unwrap_err().to_string();
    assert!(why.contains("horizon") && why.contains("CC49 of the package") && why.contains("`crs`"), "{why}");
}

#[test]
fn a_shape_against_a_frame_of_the_package_keeps_its_numbers() {
    // To break: reproject when the frame is one of the package — its numbers
    // are taken as already in it, as they always were.
    let registry = Registry::new();
    let crs = Ref::new("aaaaaaaa-0000-4000-8000-000000000001", "eml23.LocalEngineeringCompoundCrs", "CC49 of the package");
    let choices = Choices::new().set("crs", crs.dor()).set("title", "points");
    let plan = registry.plan("pointset", &from_the_package(), &choices, &Identity::derived(), &when()).unwrap();
    assert_eq!(written(&plan), cc49_nodes());
}
