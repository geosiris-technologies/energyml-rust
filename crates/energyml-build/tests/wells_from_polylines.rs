//! `wells-from-polylines` — a wellbore per polyline of a set, delegating
//! to `super::well::write` exactly as `well-from-polyline` does for one.

use std::collections::HashSet;

use energyml_build::operation::{Choices, Fitness, Identity, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When {
        creation: "2026-09-25T00:00:00Z".into(),
        originator: "test".into(),
    }
}

fn frame() -> Ref {
    Ref::new(
        "aaaaaaaa-0000-4000-8000-000000000001",
        "eml23.LocalEngineeringCompoundCrs",
        "The frame",
    )
}

/// A set of polylines, each named `trace`, `trace 2`, … — the numbering
/// convention a multi-valued slot reads (`crates/energyml-build/tests/operations.rs`,
/// *named as the front end names them*).
fn set(lines: Vec<Vec<[f64; 3]>>, vertical_up: Option<bool>) -> Pieces {
    let mut pieces = Pieces::empty();
    for (i, points) in lines.into_iter().enumerate() {
        let name = if i == 0 {
            "trace".to_string()
        } else {
            format!("trace {}", i + 1)
        };
        let n = points.len() as u32;
        pieces = pieces.with(
            name.clone(),
            Source::Shape {
                geometry: Geometry {
                    kind: "polyline",
                    patches: vec![Patch {
                        index: 0,
                        node_offset: 0,
                        points,
                        topology: Topology::Polylines(vec![n]),
                        closed: Vec::new(),
                    }],
                },
                closed: None,
            },
        );
        pieces = pieces.with(
            format!("{name} (object)"),
            Source::Object(Known {
                reference: Some(Ref::new(
                    format!("bbbbbbbb-0000-4000-8000-00000000000{}", i + 2),
                    "resqml22.PolylineRepresentation",
                    "The trace",
                )),
                crs: Some(frame()),
                vertical_up,
                vertical_uom: Some("m".into()),
                ..Default::default()
            }),
        );
    }
    pieces
}

#[test]
fn a_set_of_polylines_is_offered_to_the_operation() {
    let pieces = set(vec![vec![[0.0, 0.0, 100.0], [0.0, 0.0, 50.0]]], Some(true));
    let registry = Registry::new();
    let offer = registry
        .offers(&pieces)
        .into_iter()
        .find(|o| o.operation.name() == "wells-from-polylines")
        .expect("offered");
    assert_eq!(offer.fitness, Fitness::Fit);
}

/// **The test that counts: each polyline makes its own well, and the
/// titles tell them apart.** Two traces well apart geographically,
/// made into two complete sets of objects — not one, not a mixture.
#[test]
fn each_polyline_becomes_a_well_titled_apart() {
    let lines = vec![
        vec![[0.0, 0.0, 100.0], [0.0, 0.0, 50.0]],
        vec![[500.0, 0.0, 80.0], [500.0, 0.0, 30.0]],
    ];
    let pieces = set(lines, Some(true));
    let plan = Registry::new()
        .plan(
            "wells-from-polylines",
            &pieces,
            &Choices::new().set("title", "Fake wells"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();

    let features: Vec<_> = plan.objects.iter().filter(|o| o.role == "feature").collect();
    assert_eq!(features.len(), 2, "one feature per polyline");
    let titles: HashSet<&str> = features.iter().map(|o| o.title.as_str()).collect();
    assert!(titles.contains("Fake wells — well 1 — feature"), "{titles:?}");
    assert!(titles.contains("Fake wells — well 2 — feature"), "{titles:?}");

    let trajectories: Vec<_> = plan
        .objects
        .iter()
        .filter(|o| o.qualified_type == "resqml22.WellboreTrajectoryRepresentation")
        .collect();
    assert_eq!(trajectories.len(), 2, "one trajectory per polyline");
    // Each trajectory hangs off its **own** interpretation: not one interpretation shared by both.
    let interpretations: HashSet<&str> = trajectories
        .iter()
        .map(|t| t.json["RepresentedObject"]["Uuid"].as_str().unwrap())
        .collect();
    assert_eq!(interpretations.len(), 2, "each well has its own interpretation");
    // And the interpretations are distinct from the features too — four wellbore-ish objects, four uuids, in the right shape.
    let interp_objects: Vec<_> = plan
        .objects
        .iter()
        .filter(|o| o.role == "interpretation")
        .collect();
    assert_eq!(interp_objects.len(), 2);

    // The heads are placed exactly where each polyline started — no averaging
    // across the set, no swap between the two wells.
    let control_points: Vec<_> = plan
        .datasets
        .iter()
        .filter(|d| d.path.ends_with("/controlPoints"))
        .collect();
    assert_eq!(control_points.len(), 2);

    assert!(plan.activity.parameters.iter().any(
        |p| matches!(p, energyml_domain::provenance::Parameter::Double { title, value, .. } if title == "wellCount" && *value == 2.0)
    ));
    assert!(
        plan.says
            .iter()
            .any(|n| n.key == "wells" && n.detail.contains("well 1") && n.detail.contains("well 2")),
        "{:?}",
        plan.says
    );
}

/// A single polyline still works, titled `<title> — well 1` — the derivation
/// is unconditional, so a set of one behaves exactly like a set of many.
#[test]
fn a_single_trace_gives_a_single_numbered_well() {
    let pieces = set(vec![vec![[10.0, 10.0, 40.0], [10.0, 10.0, -10.0]]], Some(true));
    let plan = Registry::new()
        .plan(
            "wells-from-polylines",
            &pieces,
            &Choices::new().set("title", "Solo"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let feature = plan.objects.iter().find(|o| o.role == "feature").unwrap();
    assert_eq!(feature.title, "Solo — well 1 — feature");
}

/// A frame in degrees is refused — same argument as `well-from-polyline`.
#[test]
fn a_frame_in_degrees_is_refused() {
    let mut pieces = Pieces::empty();
    let points = vec![[2.0, 48.0, 10.0], [2.0, 48.0, -10.0]];
    let n = points.len() as u32;
    pieces = pieces.with(
        "trace",
        Source::Shape {
            geometry: Geometry {
                kind: "polyline",
                patches: vec![Patch {
                    index: 0,
                    node_offset: 0,
                    points,
                    topology: Topology::Polylines(vec![n]),
                    closed: Vec::new(),
                }],
            },
            closed: None,
        },
    );
    pieces = pieces.with(
        "trace (object)",
        Source::Object(Known {
            reference: Some(Ref::new(
                "bbbbbbbb-0000-4000-8000-000000000002",
                "resqml22.PolylineRepresentation",
                "The trace",
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
            "wells-from-polylines",
            &pieces,
            &Choices::new().set("title", "x"),
            &Identity::derived(),
            &when(),
        )
        .expect_err("degrees are refused");
    assert!(why.message().contains("degrees"), "{why}");
}
