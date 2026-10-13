//! `well-from-polyline` — what it computes, what it writes, what it refuses.
//!
//! The triangulation is the reader this workspace ships: what the trajectory
//! JSON says is what `energyml-geometry`'s parametric reader expects, and the
//! HDF5-gated test at the end reads the written package back through it.

use serde_json::json;

use energyml_build::operation::{Choices, Fitness, Identity, Numbers, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When {
        creation: "2026-09-10T00:00:00Z".into(),
        originator: "test".into(),
    }
}

/// A polyline in a frame of the package whose vertical CRS and direction are known.
fn bag(points: Vec<[f64; 3]>, vertical_up: Option<bool>, with_vertical_crs: bool) -> Pieces {
    bag_in(
        points,
        vertical_up,
        with_vertical_crs,
        Ref::new(
            "aaaaaaaa-0000-4000-8000-000000000001",
            "eml23.LocalEngineeringCompoundCrs",
            "The frame",
        ),
    )
}

/// The same, in the frame given — a `ProjectedCrs` named as it stands, say.
fn bag_in(points: Vec<[f64; 3]>, vertical_up: Option<bool>, with_vertical_crs: bool, frame: Ref) -> Pieces {
    let n = points.len() as u32;
    Pieces::empty()
        .with(
            "path",
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
        )
        .with(
            "path (object)",
            Source::Object(Known {
                reference: Some(Ref::new(
                    "bbbbbbbb-0000-4000-8000-000000000002",
                    "resqml22.PolylineRepresentation",
                    "The trace",
                )),
                crs: Some(frame),
                vertical_crs: with_vertical_crs.then(|| {
                    Ref::new(
                        "dddddddd-0000-4000-8000-000000000004",
                        "eml23.VerticalCrs",
                        "Vertical",
                    )
                }),
                vertical_up,
                vertical_uom: Some("m".into()),
                ..Default::default()
            }),
        )
}

fn projected_frame() -> Ref {
    Ref::new(
        "cccccccc-0000-4000-8000-000000000003",
        "eml23.ProjectedCrs",
        "EPSG:2154",
    )
}

fn deviated() -> Vec<[f64; 3]> {
    vec![
        [100.0, 200.0, 50.0],
        [100.0, 200.0, -50.0],
        [130.0, 240.0, -100.0],
        [190.0, 320.0, -200.0],
    ]
}

fn role<'a>(
    plan: &'a energyml_build::operation::Plan,
    role: &'a str,
) -> &'a energyml_build::operation::NewObject {
    plan.of_role(role)
        .next()
        .unwrap_or_else(|| panic!("a `{role}` in the plan"))
}

fn floats(plan: &energyml_build::operation::Plan, path_end: &str) -> Vec<f64> {
    match &plan
        .datasets
        .iter()
        .find(|d| d.path.ends_with(path_end))
        .unwrap_or_else(|| panic!("{path_end}"))
        .values
    {
        Numbers::F64(v) => v.clone(),
        _ => panic!("floats"),
    }
}

#[test]
fn a_deviated_polyline_becomes_datum_feature_interpretation_trajectory_and_frame() {
    // To break: an object missing from the chain, or a reference between two
    // of them pointing elsewhere.
    let pieces = bag(deviated(), Some(true), true);
    let registry = Registry::new();
    let offer = registry
        .offers(&pieces)
        .into_iter()
        .find(|o| o.operation.name() == "well-from-polyline")
        .unwrap();
    assert_eq!(offer.fitness, Fitness::Fit);
    // The ground is suggested at the highest point: 50.
    assert_eq!(
        offer.needs.iter().find(|s| s.name == "ground").unwrap().suggested,
        Some(json!(50.0))
    );

    let plan = registry
        .plan(
            "well-from-polyline",
            &pieces,
            &Choices::new().set("ground", 60.0),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let roles: Vec<&str> = plan.objects.iter().map(|o| o.role.as_str()).collect();
    assert_eq!(
        roles,
        vec!["datum", "feature", "interpretation", "trajectory", "frame"]
    );
    let trajectory = role(&plan, "trajectory");
    let frame = role(&plan, "frame");
    assert_eq!(
        trajectory.json["MdInterval"]["Datum"]["Uuid"],
        json!(role(&plan, "datum").uuid)
    );
    assert_eq!(
        trajectory.json["RepresentedObject"]["Uuid"],
        json!(role(&plan, "interpretation").uuid)
    );
    assert_eq!(
        role(&plan, "interpretation").json["InterpretedFeature"]["Uuid"],
        json!(role(&plan, "feature").uuid)
    );
    assert_eq!(frame.json["Trajectory"]["Uuid"], json!(trajectory.uuid));
    assert_eq!(
        frame.json["RepresentedObject"]["Uuid"],
        json!(role(&plan, "interpretation").uuid)
    );
    // The datum: at the first point, at the ground, **in the compound** the
    // trajectory is in — decision (b) of 24/09, common TRG v2.3 §13.25 p. 371.
    // Until then it was a `ReferencePointInACrs` whose `ProjectedCoordinates`
    // named this compound: a local position given for a projected one, the
    // type of the reference wrong (a defect found by an audit,
    // §2.4) — and no assertion here looked at that field.
    let datum = role(&plan, "datum");
    assert_eq!(
        datum.qualified_type,
        "eml23.ReferencePointInALocalEngineeringCompoundCrs"
    );
    assert_eq!(
        datum.json["$type"],
        json!("eml23.ReferencePointInALocalEngineeringCompoundCrs")
    );
    assert_eq!(datum.json["VerticalCoordinate"], json!(60.0));
    assert_eq!(
        datum.json["HorizontalCoordinates"]["$type"],
        json!("eml23.HorizontalCoordinates")
    );
    assert_eq!(datum.json["HorizontalCoordinates"]["Coordinate1"], json!(100.0));
    assert_eq!(datum.json["HorizontalCoordinates"]["Coordinate2"], json!(200.0));
    assert_eq!(
        datum.json["Crs"]["Uuid"],
        json!("aaaaaaaa-0000-4000-8000-000000000001")
    );
    assert_eq!(
        datum.json["Crs"]["QualifiedType"],
        json!("eml23.LocalEngineeringCompoundCrs")
    );
    assert!(
        datum.json["VerticalCrs"].is_null(),
        "the compound reaches the vertical system itself"
    );
    assert_eq!(datum.json["Kind"], json!("ground level"));
    // And what the trajectory names is the frame the datum sits in.
    assert_eq!(
        trajectory.json["MdInterval"]["Datum"]["QualifiedType"],
        json!("eml23.ReferencePointInALocalEngineeringCompoundCrs")
    );
    assert_eq!(
        trajectory.json["Geometry"]["LocalCrs"]["Uuid"],
        datum.json["Crs"]["Uuid"]
    );
}

#[test]
fn the_measured_depths_are_the_drop_then_the_chord_length() {
    // 60 → 50 is a drop of 10; then 100 straight down; then 50 in xy and 50 in z
    // → 70.71…; then 100 in xy and 100 in z → 141.42…
    let plan = Registry::new()
        .plan(
            "well-from-polyline",
            &bag(deviated(), Some(true), true),
            &Choices::new().set("ground", 60.0),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let mds = floats(&plan, "/controlPointParameters");
    assert_eq!(mds.len(), 4);
    assert!((mds[0] - 10.0).abs() < 1e-9);
    assert!((mds[1] - 110.0).abs() < 1e-9);
    assert!((mds[2] - (110.0 + 50f64.hypot(50.0))).abs() < 1e-9, "{}", mds[2]);
    assert!(
        (mds[3] - (110.0 + 50f64.hypot(50.0) + 100f64.hypot(100.0))).abs() < 1e-9,
        "{}",
        mds[3]
    );
    let trajectory = role(&plan, "trajectory");
    assert_eq!(trajectory.json["MdInterval"]["MdMin"], json!(mds[0]));
    assert_eq!(trajectory.json["MdInterval"]["MdMax"], json!(mds[3]));
    assert_eq!(trajectory.json["MdInterval"]["Uom"], json!("m"));
    assert_eq!(
        trajectory.json["Geometry"]["LineKindIndex"],
        json!(1),
        "linear by default"
    );
    assert_eq!(trajectory.json["Geometry"]["KnotCount"], json!(4));
    assert!(trajectory.json["Geometry"]["TangentVectors"].is_null());
    // One frame node per control point, at the same depths.
    assert_eq!(floats(&plan, "/nodeMd"), mds);
    assert_eq!(role(&plan, "frame").json["NodeCount"], json!(4));
}

#[test]
fn a_vertical_hole_whose_ground_is_the_systems_zero_is_written_as_the_vertical_kind() {
    // To break: the detection reading the shape choice instead of the points.
    // The ground is at 0 here, so the measured depths **are** the CRS's depths
    // and kind 0 says the same thing as the points do — which is the only case
    // where it may be written, see `a_vertical_hole_drilled_from_a_hill`.
    let pieces = bag(
        vec![
            [100.0, 200.0, 0.0],
            [100.0, 200.0, -500.0],
            [100.0, 200.0, -1000.0],
        ],
        Some(true),
        true,
    );
    let plan = Registry::new()
        .plan(
            "well-from-polyline",
            &pieces,
            &Choices::new().set("ground", 0.0).set("shape", "tangential-cubic"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let trajectory = role(&plan, "trajectory");
    assert_eq!(trajectory.json["Geometry"]["LineKindIndex"], json!(0));
    assert!(trajectory.json["Geometry"]["TangentVectors"].is_null());
    assert_eq!(floats(&plan, "/controlPointParameters"), vec![0.0, 500.0, 1000.0]);
    assert!(plan.says.iter().any(|n| n.key == "vertical"));
    assert_eq!(trajectory.json["MdInterval"]["MdMin"], json!(0.0));
    assert_eq!(trajectory.json["MdInterval"]["MdMax"], json!(1000.0));
}

#[test]
fn a_vertical_hole_drilled_from_a_hill_stays_a_linear_spline_and_says_why() {
    // **The defect of 19/09, and it drew nothing recognisable.** RESQML's kind 0
    // does not interpolate: *"Parameter values are interpreted as depth =>
    // (X,Y,Z), where the depth to Z conversion depends on the vertical CRS
    // direction"* (TRG v2.2 §3.8). A trajectory's parameter is the **measured
    // depth**, counted from the datum — the two agree only when the datum sits
    // at the system's zero. Here the ground is at 20, so MD 20 is the wellhead
    // and kind 0 would read it back at z = -20: the frame forty metres under
    // the trajectory it is supposed to sample.
    //
    // Found on the SIGEOL import of a thousand boreholes, whose heads are at
    // real BRGM elevations. The report: everything
    // works, even the addition to the 3D scene, at least in appearance because nothing shows in the canvas — the
    // map, which uses the wellhead, was right, and the exported OBJ had the
    // trajectory at z = 39.57 and its frame at z = 0, -1.8, -5.2.
    //
    // To break it again: write the depths as parameters instead of the MDs, or
    // take the vertical kind on shape alone.
    let pieces = bag(
        vec![
            [100.0, 200.0, 0.0],
            [100.0, 200.0, -500.0],
            [100.0, 200.0, -1000.0],
        ],
        Some(true),
        true,
    );
    let plan = Registry::new()
        .plan(
            "well-from-polyline",
            &pieces,
            &Choices::new().set("ground", 20.0),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let trajectory = role(&plan, "trajectory");
    assert_eq!(
        trajectory.json["Geometry"]["LineKindIndex"],
        json!(1),
        "a linear spline places the same points"
    );
    assert!(
        plan.says.iter().any(|n| n.key == "vertical-datum-offset"),
        "and it says why: {:?}",
        plan.says
    );
    // **The parameters are the measured depths**, which is what the frame's
    // `NodeMd` indexes into. This is the assertion the bug failed.
    assert_eq!(
        floats(&plan, "/controlPointParameters"),
        vec![20.0, 520.0, 1020.0]
    );
    assert_eq!(trajectory.json["MdInterval"]["MdMin"], json!(20.0));
    assert_eq!(trajectory.json["MdInterval"]["MdMax"], json!(1020.0));
}

#[test]
fn a_vertical_polyline_in_a_frame_of_unknown_direction_stays_a_linear_spline_and_says_why() {
    let pieces = bag(vec![[0.0, 0.0, 0.0], [0.0, 0.0, -100.0]], None, true);
    let plan = Registry::new()
        .plan(
            "well-from-polyline",
            &pieces,
            &Choices::new().set("ground", 0.0),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert_eq!(
        role(&plan, "trajectory").json["Geometry"]["LineKindIndex"],
        json!(1)
    );
    assert!(plan.says.iter().any(|n| n.key == "vertical-unknown-direction"));
}

#[test]
fn a_tangential_cubic_writes_unit_tangents_along_the_line() {
    let plan = Registry::new()
        .plan(
            "well-from-polyline",
            &bag(deviated(), Some(true), true),
            &Choices::new()
                .set("ground", 50.0)
                .set("shape", "tangential-cubic"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert_eq!(
        role(&plan, "trajectory").json["Geometry"]["LineKindIndex"],
        json!(3)
    );
    let tangents = floats(&plan, "/tangentVectors");
    assert_eq!(tangents.len(), 12);
    // The first tangent points straight down, unit.
    assert_eq!(&tangents[..3], &[0.0, 0.0, -1.0]);
    for t in tangents.chunks(3) {
        assert!(((t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt() - 1.0).abs() < 1e-9);
    }
}

#[test]
fn a_spacing_cuts_the_frame_every_so_many_metres_and_keeps_the_last() {
    let plan = Registry::new()
        .plan(
            "well-from-polyline",
            &bag(deviated(), Some(true), true),
            &Choices::new().set("ground", 50.0).set("spacing", 100.0),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let nodes = floats(&plan, "/nodeMd");
    let last = floats(&plan, "/controlPointParameters")[3];
    assert_eq!(nodes[0], 0.0);
    assert_eq!(nodes[1], 100.0);
    assert_eq!(nodes[2], 200.0);
    assert_eq!(*nodes.last().unwrap(), last);
    assert_eq!(role(&plan, "frame").json["NodeCount"], json!(nodes.len()));
}

#[test]
fn an_existing_datum_and_interpretation_are_used_and_not_recreated() {
    let datum = Ref::new(
        "eeeeeeee-0000-4000-8000-000000000005",
        "eml23.ReferencePointInACrs",
        "KB",
    )
    .dor();
    let interpretation = Ref::new(
        "ffffffff-0000-4000-8000-000000000006",
        "resqml22.WellboreInterpretation",
        "W1",
    )
    .dor();
    let plan = Registry::new()
        .plan(
            "well-from-polyline",
            &bag(deviated(), Some(true), false),
            &Choices::new()
                .set("ground", 50.0)
                .set("datum", datum)
                .set("interpretation", interpretation),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let roles: Vec<&str> = plan.objects.iter().map(|o| o.role.as_str()).collect();
    assert_eq!(roles, vec!["trajectory", "frame"]);
    assert_eq!(
        role(&plan, "trajectory").json["MdInterval"]["Datum"]["Uuid"],
        json!("eeeeeeee-0000-4000-8000-000000000005")
    );
    assert_eq!(
        role(&plan, "trajectory").json["RepresentedObject"]["Uuid"],
        json!("ffffffff-0000-4000-8000-000000000006")
    );
}

/// **A datum in a `ProjectedCrs` needs the vertical system beside it; one in a
/// compound does not.**
///
/// This test refused a datum under a *compound* with no vertical CRS known —
/// right while the datum was a `ReferencePointInACrs`, which names its
/// `VerticalCrs` itself (common TRG v2.3 §13.24, p. 370). In the compound
/// (§13.25, p. 371) the vertical system is reached through the `Crs` it
/// names, so there is nothing to refuse: the refusal now belongs to the one
/// frame that still takes the first form, a `ProjectedCrs` named as it stands
/// (decision (c) of 24/09).
#[test]
fn a_datum_in_a_projected_crs_needs_the_vertical_system_and_one_in_a_compound_does_not() {
    // The `ProjectedCrs` is the frame of the input, suggested as `crs` — and
    // the input knows no vertical system for it.
    let why = Registry::new()
        .plan(
            "well-from-polyline",
            &bag_in(deviated(), Some(true), false, projected_frame()),
            &Choices::new().set("ground", 50.0),
            &Identity::derived(),
            &when(),
        )
        .expect_err("no vertical CRS");
    assert!(why.message().contains("vertical CRS"), "{why}");

    // Under the compound of the same bag, the datum is made — in it.
    let plan = Registry::new()
        .plan(
            "well-from-polyline",
            &bag(deviated(), Some(true), false),
            &Choices::new().set("ground", 50.0),
            &Identity::derived(),
            &when(),
        )
        .expect("the compound reaches its vertical system");
    assert_eq!(
        role(&plan, "datum").qualified_type,
        "eml23.ReferencePointInALocalEngineeringCompoundCrs"
    );
}

/// **Under a `ProjectedCrs` named as it stands, the datum is projected — in that
/// `ProjectedCrs`, with its vertical system**, and the plan says the frame is
/// taken with a note. An audit found the same object written under a
/// compound: `ProjectedCoordinates.Crs` pointing at something that is no
/// projected system.
#[test]
fn a_datum_under_a_named_projected_crs_is_given_in_it_and_its_vertical_system() {
    // The input's frame is a `ProjectedCrs` whose vertical system it knows —
    // a BRGM surface, say — and `crs` is suggested from it.
    let plan = Registry::new()
        .plan(
            "well-from-polyline",
            &bag_in(deviated(), Some(true), true, projected_frame()),
            &Choices::new().set("ground", 60.0),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let datum = role(&plan, "datum");
    assert_eq!(datum.qualified_type, "eml23.ReferencePointInACrs");
    assert_eq!(
        datum.json["HorizontalCoordinates"]["$type"],
        json!("eml23.ProjectedCoordinates")
    );
    assert_eq!(
        datum.json["HorizontalCoordinates"]["Crs"]["QualifiedType"],
        json!("eml23.ProjectedCrs")
    );
    assert_eq!(
        datum.json["HorizontalCoordinates"]["Crs"]["Uuid"],
        json!("cccccccc-0000-4000-8000-000000000003")
    );
    assert_eq!(
        datum.json["VerticalCrs"]["QualifiedType"],
        json!("eml23.VerticalCrs")
    );
    assert!(
        plan.says.iter().any(|n| n.key == "frame-projected"),
        "{:?}",
        plan.says
    );
}

/// **A wellbore is refused in degrees**, as `sigeol-boreholes` and
/// `wells-from-table` refuse it — a defect found by an audit: this operation
/// lacked the refusal, and a GeoJSON line left without `project` came out a
/// hole between a hundred and ten thousand times longer than the model is
/// wide, in a package `check` validates.
#[test]
fn a_wellbore_in_degrees_is_refused_naming_what_to_set() {
    let collection = energyml_build::geojson::read(
        r#"{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"LineString","coordinates":[[2.0,48.0,100],[2.0,48.0,-500]]},"properties":{}}]}"#,
    )
    .unwrap();
    let pieces = Pieces::empty().with(
        "well.geojson",
        Source::Features {
            name: "well.geojson".into(),
            features: collection.features,
        },
    );
    let why = Registry::new()
        .plan(
            "well-from-polyline",
            &pieces,
            &Choices::new().set("ground", 100.0),
            &Identity::derived(),
            &when(),
        )
        .expect_err("degrees");
    assert!(
        why.message().contains("degrees") && why.message().contains("`project`"),
        "{why}"
    );
    // And a `Geographic2dCrs` named as the frame, by its slot.
    let geographic = Ref::new(
        "99999999-0000-4000-8000-000000000007",
        "eml23.Geographic2dCrs",
        "EPSG:4326",
    )
    .dor();
    let why = Registry::new()
        .plan(
            "well-from-polyline",
            &bag(deviated(), Some(true), true),
            &Choices::new().set("ground", 50.0).set("crs", geographic),
            &Identity::derived(),
            &when(),
        )
        .expect_err("degrees named");
    assert!(
        why.message().contains("EPSG:4326") && why.message().contains("degrees"),
        "{why}"
    );
}

/// **A half of a system named as the frame blocks the well, and the frame's
/// block reaches the plan.** `well-from-polyline` wrote `blocked: None`
/// whatever `support::frame_in` said — a well under a RESQML 2.0.1 system
/// would have been applied half of one release.
#[test]
fn a_frame_that_blocks_blocks_the_well() {
    let half = Ref::new(
        "dddddddd-0000-4000-8000-000000000004",
        "eml23.VerticalCrs",
        "Vertical",
    )
    .dor();
    let plan = Registry::new()
        .plan(
            "well-from-polyline",
            &bag(deviated(), Some(true), true),
            &Choices::new().set("ground", 50.0).set("crs", half),
            &Identity::derived(),
            &when(),
        )
        .expect("the well is computed; its writing is what is blocked");
    let why = plan
        .blocked
        .as_ref()
        .expect("a VerticalCrs is no frame")
        .to_string();
    assert!(why.contains("eml23.VerticalCrs"), "{why}");
    let old = Ref::new(
        "cccccccc-0000-4000-8000-000000000003",
        "resqml20.obj_LocalDepth3dCrs",
        "Ancien",
    )
    .dor();
    let plan = Registry::new()
        .plan(
            "well-from-polyline",
            &bag(deviated(), Some(true), true),
            &Choices::new().set("ground", 50.0).set("crs", old),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert!(
        plan.blocked
            .as_ref()
            .is_some_and(|why| why.to_string().contains("2.0.1")),
        "{:?}",
        plan.blocked
    );
}

#[test]
fn two_polylines_or_a_surface_are_refused_by_name() {
    let two = Pieces::empty().with(
        "two",
        Source::Shape {
            geometry: Geometry {
                kind: "x",
                patches: vec![Patch {
                    index: 0,
                    node_offset: 0,
                    points: vec![[0.0; 3], [1.0; 3], [2.0; 3], [3.0; 3]],
                    topology: Topology::Polylines(vec![2, 2]),
                    closed: Vec::new(),
                }],
            },
            closed: None,
        },
    );
    let registry = Registry::new();
    match registry
        .offers(&two)
        .into_iter()
        .find(|o| o.operation.name() == "well-from-polyline")
        .unwrap()
        .fitness
    {
        Fitness::Unfit(why) => assert!(why.message.contains("holds 2"), "{why}"),
        Fitness::Fit => panic!("two polylines are two wells"),
    }
    let surface = Pieces::empty().with(
        "s",
        Source::Shape {
            geometry: Geometry {
                kind: "x",
                patches: vec![Patch {
                    index: 0,
                    node_offset: 0,
                    points: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                    topology: Topology::Triangles(vec![[0, 1, 2]]),
                    closed: Vec::new(),
                }],
            },
            closed: None,
        },
    );
    match registry
        .offers(&surface)
        .into_iter()
        .find(|o| o.operation.name() == "well-from-polyline")
        .unwrap()
        .fitness
    {
        Fitness::Unfit(why) => assert!(why.message.contains("one polyline"), "{why}"),
        Fitness::Fit => panic!("a surface is not a polyline"),
    }
}

#[test]
fn a_geojson_line_makes_a_well_in_a_crs_created_here() {
    // The frame created with the plan carries its own vertical CRS, so the
    // datum can be created too — five objects plus the two CRS.
    let collection = energyml_build::geojson::read(
        r#"{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"LineString","coordinates":[[2.0,48.0,100],[2.0,48.0,-500]]},"properties":{}}]}"#,
    )
    .unwrap();
    let pieces = Pieces::empty().with(
        "well.geojson",
        Source::Features {
            name: "well.geojson".into(),
            features: collection.features,
        },
    );
    let plan = Registry::new()
        .plan(
            "well-from-polyline",
            &pieces,
            &Choices::new().set("project", 2154).set("ground", 100.0),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let roles: Vec<&str> = plan.objects.iter().map(|o| o.role.as_str()).collect();
    // The frame created is the compound over Lambert-93 (RESQML TRG 2.2 §3.1
    // p. 22) — four objects where there were two — and the datum sits in it:
    // it names the compound, which reaches the vertical system, where it named
    // the `VerticalCrs` itself beside a `ProjectedCoordinates` in the
    // `ProjectedCrs`.
    assert_eq!(
        roles,
        vec![
            "projected-crs",
            "vertical-crs",
            "local-2d-crs",
            "crs",
            "datum",
            "feature",
            "interpretation",
            "trajectory",
            "frame"
        ]
    );
    assert_eq!(
        role(&plan, "datum").json["Crs"]["Uuid"],
        json!(role(&plan, "crs").uuid)
    );
    assert_eq!(
        role(&plan, "crs").json["VerticalCrs"]["Uuid"],
        json!(role(&plan, "vertical-crs").uuid)
    );
    // Vertical, and the created frame is up — but the ground is at 100, so the
    // measured depths are not the system's depths and kind 0 would sink the
    // whole hole by a hundred metres. A linear spline through the same two
    // points places them exactly, and its parameter is the MD.
    assert_eq!(
        role(&plan, "trajectory").json["Geometry"]["LineKindIndex"],
        json!(1)
    );
}

// -- the whole way, read back by the reader this workspace ships -------------

/// Write a well, save it, read both objects back through this workspace's own
/// geometry reader, and answer with their points.
#[cfg(feature = "hdf5")]
fn read_back(line: &str, ground: f64, tag: &str) -> (Vec<[f64; 3]>, Vec<[f64; 3]>) {
    use std::sync::Arc;
    let collection = energyml_build::geojson::read(&format!(
        r#"{{"type":"FeatureCollection","features":[{{"type":"Feature","geometry":{{"type":"LineString","coordinates":{line}}},"properties":{{}}}}]}}"#
    ))
    .unwrap();
    let pieces = Pieces::empty().with(
        "well.geojson",
        Source::Features {
            name: "well.geojson".into(),
            features: collection.features,
        },
    );
    let plan = Registry::new()
        .plan(
            "well-from-polyline",
            &pieces,
            &Choices::new().set("project", 2154).set("ground", ground),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let dir = std::env::temp_dir().join(format!("energyml-build-well-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("well.epc");
    let registry = Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "well.epc", registry.clone()).unwrap();
    store.external_files_mut().set_root(&dir);
    let made = energyml_build::apply::apply(&plan, &mut store, &when());
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(made.refused.is_empty(), "{:?}", made.refused);
    store
        .save_as(&out, &energyml_storage::SaveOptions::default())
        .unwrap();

    let reread = energyml_storage::EpcStore::open(&out, registry).unwrap();
    let mut answers = Vec::new();
    for what in ["trajectory", "frame"] {
        let summary = reread.find_by_uuid(&role(&plan, what).uuid).cloned().unwrap();
        let object = reread.get_object(&summary.uri).unwrap();
        let built = energyml_geometry::scene::object_of(object.as_reflect(), &reread, &summary.uri)
            .unwrap_or_else(|e| panic!("{what}: {e}"));
        let points: Vec<[f64; 3]> = built
            .geometry
            .patches
            .iter()
            .flat_map(|p| p.points.iter().copied())
            .collect();
        assert!(points.len() >= 2, "{what}: {} point(s)", points.len());
        answers.push(points);
    }
    let _ = std::fs::remove_dir_all(&dir);
    let frame = answers.pop().unwrap();
    (answers.pop().unwrap(), frame)
}

/// **The head a datum states is where the trajectory starts — read by two
/// readers, through a frame that moves things.**
///
/// The datum read by `energyml_geometry::well`
/// (`head_of`: the reference point, placed through the compound it names)
/// against the first node of the trajectory read from its control points and
/// placed through its `LocalCrs` by `energyml_domain::crs`. Two sources — the
/// datum's own numbers in the XML, the control points in the HDF5 — for one
/// fact: the hole is collared where it starts.
///
/// **The frame is turned 30° and offset**, like the grid frame of Lutetia
///: under the identity compound every error of frame would be
/// invisible, local and projected numbers being the same. an audit found —
/// local coordinates written as projected ones — was exactly that. To break:
/// write the datum's coordinates projected while the trajectory's stay local,
/// or name a different frame on the datum.
#[cfg(feature = "hdf5")]
#[test]
fn a_datum_read_by_the_well_reader_is_the_first_node_of_its_trajectory_placed_through_the_compound() {
    use std::sync::Arc;
    let dir = std::env::temp_dir().join(format!("energyml-build-well-turned-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("turned.epc");
    let registry = Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "turned.epc", registry.clone()).unwrap();
    store.external_files_mut().set_root(&dir);

    // The frame: Lambert-93 and NGF-IGN69, the local origin at a platform, the
    // local Y axis N30E.
    let frame_plan = Registry::new()
        .plan(
            "crs",
            &Pieces::empty(),
            &Choices::new()
                .set("epsg", 2154)
                .set("vertical", 5720)
                .set("originEasting", 651_000.0)
                .set("originNorthing", 6_862_000.0)
                .set("azimuth", 30.0),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let made = energyml_build::apply::apply(&frame_plan, &mut store, &when());
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(made.refused.is_empty(), "{:?}", made.refused);
    let compound = frame_plan.of_role("crs").next().expect("the compound");
    let frame = Ref::new(
        compound.uuid.clone(),
        compound.qualified_type.clone(),
        compound.title.clone(),
    );

    // A deviated line **in the local frame**, collared at its first point.
    let line = vec![[10.0, 20.0, 50.0], [10.0, 20.0, -50.0], [40.0, 60.0, -100.0]];
    let plan = Registry::new()
        .plan(
            "well-from-polyline",
            &bag_in(line.clone(), Some(true), false, frame),
            &Choices::new().set("ground", 50.0),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert!(plan.blocked.is_none(), "{:?}", plan.blocked);
    assert_eq!(
        role(&plan, "datum").qualified_type,
        "eml23.ReferencePointInALocalEngineeringCompoundCrs"
    );
    let made = energyml_build::apply::apply(&plan, &mut store, &when());
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(made.refused.is_empty(), "{:?}", made.refused);
    store
        .save_as(&out, &energyml_storage::SaveOptions::default())
        .unwrap();

    let reread = energyml_storage::EpcStore::open(&out, registry).unwrap();
    let summary = reread
        .find_by_uuid(&role(&plan, "trajectory").uuid)
        .cloned()
        .unwrap();
    let trajectory = reread.get_object(&summary.uri).unwrap();

    // Reader one: the datum, placed through the compound it names.
    let head = energyml_geometry::well::head_of(trajectory.as_reflect(), &reread).expect("the datum reads");
    // Reader two: the control points as stored, placed through the trajectory's
    // own `LocalCrs`.
    let built = energyml_geometry::scene::object_of(trajectory.as_reflect(), &reread, &summary.uri)
        .expect("the trajectory reads");
    let first = built.geometry.patches[0].points[0];
    assert_eq!(first, line[0], "the control points are stored local, as written");
    let frame = energyml_domain::crs::of(trajectory.as_reflect(), &reread)
        .expect("the frame reads")
        .expect("one is named");
    let placed = frame.to_projected(first);

    for axis in 0..3 {
        assert!(
            (head[axis] - placed[axis]).abs() < 1e-6,
            "axis {axis}: the datum reads {head:?}, the first node {placed:?}"
        );
    }
    // And the frame did move things — otherwise the agreement would prove
    // nothing about which numbers are local.
    assert!(
        (placed[0] - first[0]).abs() > 1_000.0 && (placed[1] - first[1]).abs() > 1_000.0,
        "{placed:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(feature = "hdf5")]
#[test]
fn a_written_well_reads_back_through_the_geometry_reader() {
    let (trajectory, frame) = read_back(
        "[[2.0,48.0,100],[2.001,48.0,-200],[2.003,48.001,-500]]",
        120.0,
        "deviated",
    );
    assert!(trajectory.len() >= 3 && frame.len() >= 3);
}

#[cfg(feature = "hdf5")]
#[test]
fn a_frames_nodes_land_on_the_trajectory_they_sample() {
    // **The triangulation that was missing on 19/09**, and the only shape of
    // test that could have seen the bug: the old one counted points, and a
    // frame placed forty metres under its own trajectory has exactly as many
    // points as one placed on it.
    //
    // The two objects are read **independently** by the reader this workspace
    // ships — one from `ControlPoints`, one by evaluating the parametric line
    // at each `NodeMd` — so agreeing is a fact about the file, not about a
    // shared code path. A frame created here puts one node on each control
    // point, which is what makes the comparison exact.
    //
    // The vertical case is the one that failed: kind 0 reads its parameter as
    // a depth through the CRS, so a hole collared at 100 m came back with its
    // head at 0.
    for (line, ground, tag) in [
        (
            "[[2.0,48.0,100],[2.0,48.0,-200],[2.0,48.0,-500]]",
            100.0,
            "vertical-from-a-hill",
        ),
        (
            "[[2.0,48.0,0],[2.0,48.0,-300],[2.0,48.0,-600]]",
            0.0,
            "vertical-from-the-zero",
        ),
        (
            "[[2.0,48.0,100],[2.001,48.0,-200],[2.003,48.001,-500]]",
            120.0,
            "deviated",
        ),
    ] {
        let (trajectory, frame) = read_back(line, ground, tag);
        assert_eq!(
            trajectory.len(),
            frame.len(),
            "{tag}: one frame node per control point"
        );
        for (at, (t, f)) in trajectory.iter().zip(&frame).enumerate() {
            let off = ((t[0] - f[0]).powi(2) + (t[1] - f[1]).powi(2) + (t[2] - f[2]).powi(2)).sqrt();
            assert!(
                off < 1e-6,
                "{tag}: node {at} is {off} m off its trajectory: {t:?} vs {f:?}"
            );
        }
    }
}

/// **A well is suggested only a wellbore's interpretation.** The shared
/// suggestion offers the interpretation of the one input that names one; a
/// trace drawn from a horizon's polyline names the horizon's, and the well
/// represented the horizon (the defect `well-from-plan` showed on 24/09 with
/// its ground surface). An operation that imposes what it represents
/// (`WellboreFeature`, `WellboreInterpretation`) takes an interpretation of
/// that type or makes its own; a wellbore's is still taken.
#[test]
fn a_trace_lends_a_wellbore_interpretation_never_a_horizons() {
    let with_interpretation = |interpretation: Ref| {
        let mut pieces = bag(deviated(), Some(true), false);
        pieces.update("path (object)", |source| {
            if let Source::Object(known) = source {
                known.interpretation = Some(interpretation.clone());
            }
        });
        Registry::new()
            .plan(
                "well-from-polyline",
                &pieces,
                &Choices::new().set("ground", 50.0),
                &Identity::derived(),
                &when(),
            )
            .unwrap()
    };
    let horizon = Ref::new(
        "12121212-0000-4000-8000-000000000007",
        "resqml22.HorizonInterpretation",
        "H3",
    );
    let plan = with_interpretation(horizon.clone());
    let made = role(&plan, "interpretation");
    assert_eq!(made.qualified_type, "resqml22.WellboreInterpretation");
    assert_eq!(
        role(&plan, "trajectory").json["RepresentedObject"]["Uuid"],
        json!(made.uuid)
    );

    let wellbore = Ref::new(
        "13131313-0000-4000-8000-000000000008",
        "resqml22.WellboreInterpretation",
        "W1",
    );
    let plan = with_interpretation(wellbore);
    assert!(
        plan.of_role("interpretation").next().is_none(),
        "a wellbore's interpretation is taken, not remade"
    );
    assert_eq!(
        role(&plan, "trajectory").json["RepresentedObject"]["Uuid"],
        json!("13131313-0000-4000-8000-000000000008")
    );
}
