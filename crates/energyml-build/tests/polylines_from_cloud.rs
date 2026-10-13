//! `polylines-from-cloud` — fake, dated test data. A vertical polyline
//! descends from each point of a cloud.

use energyml_build::operation::{Choices, Fitness, Identity, Numbers, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When {
        creation: "2026-09-25T00:00:00Z".into(),
        originator: "test".into(),
    }
}

/// A cloud of points, in a frame whose vertical sense is `vertical_up`.
fn cloud(points: Vec<[f64; 3]>, vertical_up: Option<bool>) -> Pieces {
    Pieces::empty()
        .with(
            "shape",
            Source::Shape {
                geometry: Geometry {
                    kind: "cloud",
                    patches: vec![Patch {
                        index: 0,
                        node_offset: 0,
                        points,
                        topology: Topology::Points,
                        closed: Vec::new(),
                    }],
                },
                closed: None,
            },
        )
        .with(
            "shape (object)",
            Source::Object(Known {
                reference: Some(Ref::new(
                    "bbbbbbbb-0000-4000-8000-000000000002",
                    "resqml22.PointSetRepresentation",
                    "Nuage",
                )),
                crs: Some(Ref::new(
                    "aaaaaaaa-0000-4000-8000-000000000001",
                    "eml23.LocalEngineeringCompoundCrs",
                    "The frame",
                )),
                vertical_up,
                vertical_uom: Some("m".into()),
                ..Default::default()
            }),
        )
}

fn points_of(plan: &energyml_build::operation::Plan) -> Vec<[f64; 3]> {
    match &plan
        .datasets
        .iter()
        .find(|d| d.path.ends_with("/points"))
        .expect("a points dataset")
        .values
    {
        Numbers::F64(flat) => flat.chunks(3).map(|c| [c[0], c[1], c[2]]).collect(),
        _ => panic!("floats"),
    }
}

#[test]
fn a_surface_is_offered_to_the_operation() {
    let registry = Registry::new();
    let offer = registry
        .offers(&cloud(vec![[0.0, 0.0, 0.0]], Some(true)))
        .into_iter()
        .find(|o| o.operation.name() == "polylines-from-cloud")
        .expect("offered");
    assert_eq!(offer.fitness, Fitness::Fit);
}

/// **The test that counts: the top node is, bit for bit, the point of the cloud.**
///
/// Nothing recomputes it — each polyline starts from its head as it is — and
/// it is what a point added by mistake would break first: a
/// rounding or a silent reprojection would make the top node drift
/// without any count showing it.
#[test]
fn the_top_node_of_each_polyline_is_bit_for_bit_the_point_of_the_cloud() {
    let heads = vec![[0.0, 0.0, 100.0], [50.0, 0.0, 80.0], [0.0, 50.0, 120.0]];
    let pieces = cloud(heads.clone(), Some(true));
    let plan = Registry::new()
        .plan(
            "polylines-from-cloud",
            &pieces,
            &Choices::new()
                .set("cuts", "fixed")
                .set("count", 3_i64)
                .set("length", "fixed")
                .set("value", 20.0),
            &Identity::derived(),
            &when(),
        )
        .unwrap();

    let object = plan
        .objects
        .iter()
        .find(|o| o.role == "representation")
        .expect("the polyline set");
    assert_eq!(object.qualified_type, "resqml22.PolylineSetRepresentation");
    // 3 heads, 3 cuts each: 4 nodes per line, 12 in all.
    let points = points_of(&plan);
    assert_eq!(points.len(), 12);
    for (line, head) in points.chunks(4).zip(&heads) {
        for axis in 0..3 {
            assert_eq!(
                line[0][axis].to_bits(),
                head[axis].to_bits(),
                "head {head:?}, axis {axis}: not bit for bit"
            );
        }
    }
}

/// The sense of z decides the direction, and a fixed length lands exactly.
#[test]
fn the_length_goes_down_in_the_sense_of_the_crs() {
    let up_pieces = cloud(vec![[0.0, 0.0, 100.0]], Some(true));
    let up = Registry::new()
        .plan(
            "polylines-from-cloud",
            &up_pieces,
            &Choices::new()
                .set("cuts", "fixed")
                .set("count", 1_i64)
                .set("length", "fixed")
                .set("value", 30.0),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let bottom = points_of(&up)[1];
    assert!((bottom[2] - 70.0).abs() < 1e-9, "{bottom:?}");

    let down_pieces = cloud(vec![[0.0, 0.0, 100.0]], Some(false));
    let down = Registry::new()
        .plan(
            "polylines-from-cloud",
            &down_pieces,
            &Choices::new()
                .set("cuts", "fixed")
                .set("count", 1_i64)
                .set("length", "fixed")
                .set("value", 30.0),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let bottom = points_of(&down)[1];
    assert!((bottom[2] - 130.0).abs() < 1e-9, "{bottom:?}");
}

/// **A seed reproduces the same draw.** Two runs, same seed: same
/// cut counts, same points. A different seed draws others —
/// otherwise the test could never turn red.
#[test]
fn a_seed_redoes_the_same_polylines() {
    let pieces = cloud(
        vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [20.0, 0.0, 0.0]],
        Some(true),
    );
    let choices = |seed: i64| {
        Choices::new()
            .set("cuts", "random")
            .set("cutsMin", 2_i64)
            .set("cutsMax", 8_i64)
            .set("length", "random")
            .set("lengthMin", 5.0)
            .set("lengthMax", 50.0)
            .set("spacing", "fluctuating")
            .set("fraction", 0.4)
            .set("seed", seed)
    };
    let a = Registry::new()
        .plan(
            "polylines-from-cloud",
            &pieces,
            &choices(7),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let b = Registry::new()
        .plan(
            "polylines-from-cloud",
            &pieces,
            &choices(7),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert_eq!(
        points_of(&a),
        points_of(&b),
        "the same seed remakes the same polylines"
    );

    let c = Registry::new()
        .plan(
            "polylines-from-cloud",
            &pieces,
            &choices(8),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert_ne!(
        points_of(&a),
        points_of(&c),
        "a different seed draws differently — else this proves nothing"
    );
}

/// Fabricated data says so: the note and the name of the recipe, so that a consumer
/// never takes a synthetic head for a measurement.
#[test]
fn fabricated_data_says_so() {
    let pieces = cloud(vec![[0.0, 0.0, 0.0]], Some(true));
    let plan = Registry::new()
        .plan(
            "polylines-from-cloud",
            &pieces,
            &Choices::new()
                .set("cuts", "fixed")
                .set("count", 1_i64)
                .set("length", "fixed")
                .set("value", 10.0),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert_eq!(plan.activity.recipe, "polylines-from-cloud");
    assert!(
        plan.says
            .iter()
            .any(|n| n.key == "fake" && n.detail.contains("synthetic")),
        "{:?}",
        plan.says
    );
}

/// A frame in degrees is refused, as for `well-from-polyline`: a
/// length is linear, not an angle.
#[test]
fn a_frame_in_degrees_is_refused() {
    let pieces = Pieces::empty()
        .with(
            "shape",
            Source::Shape {
                geometry: Geometry {
                    kind: "cloud",
                    patches: vec![Patch {
                        index: 0,
                        node_offset: 0,
                        points: vec![[2.0, 48.0, 0.0]],
                        topology: Topology::Points,
                        closed: Vec::new(),
                    }],
                },
                closed: None,
            },
        )
        .with(
            "shape (object)",
            Source::Object(Known {
                reference: Some(Ref::new(
                    "bbbbbbbb-0000-4000-8000-000000000002",
                    "resqml22.PointSetRepresentation",
                    "Nuage",
                )),
                crs: Some(Ref::new(
                    "cccccccc-0000-4000-8000-000000000003",
                    "eml23.Geographic2dCrs",
                    "WGS84",
                )),
                vertical_up: Some(true),
                vertical_uom: Some("m".into()),
                ..Default::default()
            }),
        );
    let why = Registry::new()
        .plan(
            "polylines-from-cloud",
            &pieces,
            &Choices::new()
                .set("cuts", "fixed")
                .set("count", 1_i64)
                .set("length", "fixed")
                .set("value", 10.0),
            &Identity::derived(),
            &when(),
        )
        .expect_err("degrees are refused");
    assert!(why.message().contains("degrees"), "{why}");
}

/// Without an answer on the sense of z, the operation refuses rather than guesses.
#[test]
fn without_a_known_sense_of_z_the_operation_refuses() {
    let pieces = cloud(vec![[0.0, 0.0, 0.0]], None);
    let why = Registry::new()
        .plan(
            "polylines-from-cloud",
            &pieces,
            &Choices::new()
                .set("cuts", "fixed")
                .set("count", 1_i64)
                .set("length", "fixed")
                .set("value", 10.0),
            &Identity::derived(),
            &when(),
        )
        .expect_err("no vertical sense");
    assert!(why.message().contains("up"), "{why}");
}
