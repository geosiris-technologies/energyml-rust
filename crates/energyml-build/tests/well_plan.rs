//! `well-from-plan` — a wellbore from a drilling plan: what it places, what it
//! writes, what it refuses.
//!
//! **The form of the assertions comes from the method and the schema**: the
//! minimum curvature method joins its stations by circular arcs (API Bulletin
//! D20, 1985; the formula as Sawaryn & Thorogood write it, SPE 84246, 2005),
//! so a build of 45° at 15°/30 m is 90 m of hole and ends `R (1 − cos 45°)`
//! across and `R sin 45°` down, `R = 30 / (15° in radians)`; a trajectory's
//! parameter is the measured depth (`crates/energyml-build/ARCHITECTURE.md`: the
//! parameter of a trajectory is the MD); `ParentIntersection.KickoffMd` is
//! *“the measured depth for the start of the child trajectory, as defined
//! within the child”* (RESQML 2.2 `Wells.xsd`). **The numbers are the
//! tests' own**, computed by hand beside each assertion.

use serde_json::{Value, json};

use energyml_build::operation::{Choices, Identity, NewObject, Numbers, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When {
        creation: "2026-09-24T00:00:00Z".into(),
        originator: "test".into(),
    }
}

fn planned(pieces: &Pieces, choices: Choices) -> Result<Plan, String> {
    Registry::new()
        .plan("well-from-plan", pieces, &choices, &Identity::derived(), &when())
        .map_err(|why| why.to_string())
}

/// A frame created here — Lambert-93 over NGF-IGN69, the identity compound,
/// which counts up — and a head given in it.
fn head() -> Choices {
    Choices::new()
        .set("sourceEpsg", 2154)
        .set("project", 2154)
        .set("vertical", 5720)
        .set("title", "GTH-test")
        .set("headX", 651_800.0)
        .set("headY", 6_861_200.0)
}

fn role<'a>(plan: &'a Plan, role: &'a str) -> &'a NewObject {
    plan.of_role(role)
        .next()
        .unwrap_or_else(|| panic!("a `{role}` in the plan"))
}

fn floats(plan: &Plan, path_end: &str) -> Vec<f64> {
    match &plan
        .datasets
        .iter()
        .find(|d| d.path.ends_with(path_end))
        .unwrap_or_else(|| panic!("{path_end}"))
        .values
    {
        Numbers::F64(values) => values.clone(),
        Numbers::I64(_) => panic!("floats at {path_end}"),
    }
}

/// What the activity records under that title.
fn recorded(plan: &Plan, name: &str) -> Option<f64> {
    plan.activity
        .parameters
        .iter()
        .find_map(|parameter| match parameter {
            Parameter::Double { title, value, .. } if title == name => Some(*value),
            _ => None,
        })
}

fn near(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

/// A frame of the package, and what the bag knows of it.
fn frame() -> Ref {
    Ref::new(
        "aaaaaaaa-0000-4000-8000-000000000001",
        "eml23.LocalEngineeringCompoundCrs",
        "Lutetia — test frame",
    )
}

fn in_frame(up: Option<bool>) -> (Pieces, Choices) {
    let pieces = Pieces::empty().with(
        "crs (object)",
        Source::Object(Known {
            reference: Some(frame()),
            crs: Some(frame()),
            vertical_up: up,
            vertical_uom: Some("m".into()),
            ..Default::default()
        }),
    );
    (
        pieces,
        Choices::new().set("crs", frame().dor()).set("title", "GTH-test"),
    )
}

/// A plane, z = 40 + 0.01 (x − 651 000), over a kilometre square: 48 under
/// (651 800, 6 861 200).
fn slope() -> Source {
    let points = vec![
        [651_000.0, 6_861_000.0, 40.0],
        [652_000.0, 6_861_000.0, 50.0],
        [652_000.0, 6_862_000.0, 50.0],
        [651_000.0, 6_862_000.0, 40.0],
    ];
    Source::Shape {
        geometry: Geometry {
            kind: "triangles",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points,
                topology: Topology::Triangles(vec![[0, 1, 2], [0, 2, 3]]),
                closed: Vec::new(),
            }],
        },
        closed: None,
    }
}

#[test]
fn a_vertical_plan_keeps_its_head_and_writes_the_plans_depths() {
    // To break: a hold that drifts in plan, a z that goes the wrong way, depths
    // counted from somewhere else than the datum, or frame nodes that are not
    // the stations.
    let plan = planned(
        &Pieces::empty(),
        head().set("ground", 39.0).set("sections", json!(["hold to 200"])),
    )
    .unwrap();
    let mds = floats(&plan, "/controlPointParameters");
    assert_eq!(mds, (0..=20).map(|k| k as f64 * 10.0).collect::<Vec<_>>());
    let points = floats(&plan, "/controlPoints");
    for (at, md) in mds.iter().enumerate() {
        assert_eq!(
            (points[3 * at], points[3 * at + 1]),
            (651_800.0, 6_861_200.0),
            "station {at}"
        );
        // A frame that counts up: each metre of depth is a metre lower.
        assert!(
            near(points[3 * at + 2], 39.0 - md, 1e-9),
            "station {at}: {}",
            points[3 * at + 2]
        );
    }
    assert_eq!(
        floats(&plan, "/nodeMd"),
        mds,
        "the frame's nodes are the stations"
    );

    let roles: Vec<&str> = plan.objects.iter().map(|o| o.role.as_str()).collect();
    assert_eq!(
        roles[roles.len() - 5..],
        ["datum", "feature", "interpretation", "trajectory", "frame"]
    );
    let datum = role(&plan, "datum");
    assert_eq!(datum.json["VerticalCoordinate"], json!(39.0));
    assert_eq!(
        datum.json["HorizontalCoordinates"]["Coordinate1"],
        json!(651_800.0)
    );
    assert_eq!(
        datum.json["HorizontalCoordinates"]["Coordinate2"],
        json!(6_861_200.0)
    );
    let trajectory = role(&plan, "trajectory");
    assert_eq!(trajectory.json["MdInterval"]["Datum"]["Uuid"], json!(datum.uuid));
    assert_eq!(trajectory.json["MdInterval"]["MdMax"], json!(200.0));
    // “For planned trajectories, use IsDrilled=FALSE” (Wells.xsd).
    assert_eq!(role(&plan, "interpretation").json["IsDrilled"], json!(false));

    // The shape is the trajectory: the stations, in order.
    assert_eq!(plan.shapes.len(), 1);
    let drawn: Vec<f64> = plan.shapes[0].geometry.patches[0]
        .points
        .iter()
        .flatten()
        .copied()
        .collect();
    assert_eq!(drawn, points);
    assert_eq!(recorded(&plan, "stationCount"), Some(21.0));
    assert_eq!(recorded(&plan, "departure"), Some(0.0));
}

#[test]
fn a_build_reaches_the_declared_inclination_at_the_declared_depth() {
    // Kick-off at 40, 15°/30 m to 45° toward the east: the arc is 45 / 15 · 30
    // = 90 m, so the direction is reached at MD 130. R = 30 / (15° in radians)
    // = 114.5915590; the build ends R (1 − cos 45°) = 33.5630906 east of the
    // hole and R sin 45° = 81.0284685 below the kick-off.
    let plan = planned(
        &Pieces::empty(),
        head()
            .set("ground", 50.0)
            .set("sections", json!(["hold to 40", "build 15 45 90", "hold 50"])),
    )
    .unwrap();
    let mds = floats(&plan, "/controlPointParameters");
    let points = floats(&plan, "/controlPoints");
    let kick = mds
        .iter()
        .position(|md| *md == 40.0)
        .expect("a station at the kick-off");
    assert_eq!(
        (points[3 * kick], points[3 * kick + 1]),
        (651_800.0, 6_861_200.0),
        "straight down to the kick-off"
    );

    let at = mds
        .iter()
        .position(|md| near(*md, 130.0, 1e-9))
        .expect("a station where the build ends");
    let end = &points[3 * at..3 * at + 3];
    assert!(near(end[0], 651_800.0 + 33.563_090_6, 1e-6), "{end:?}");
    assert!(near(end[1], 6_861_200.0, 1e-9), "{end:?}");
    assert!(near(end[2], 50.0 - 40.0 - 81.028_468_5, 1e-6), "{end:?}");
    // And from there the hole goes on at 45°: as far east as down.
    let next = &points[3 * (at + 1)..3 * (at + 1) + 3];
    let (east, down) = (next[0] - end[0], end[2] - next[2]);
    assert!(
        near(east.atan2(down).to_degrees(), 45.0, 1e-7),
        "east {east}, down {down}"
    );
    assert!(
        near(*mds.last().unwrap(), 180.0, 1e-9),
        "90 m of arc after 40, then 50 of hold: {mds:?}"
    );
}

#[test]
fn the_depths_written_are_the_plans_never_the_chords() {
    // `hold to 43.7` and `hold to 250.3` are written as said, and along the arc
    // the depths are arc lengths. To break: hand the writer `mds: None`, and it
    // measures the chords between stations — which fall short on an arc, so
    // every depth after it comes out smaller.
    let plan = planned(
        &Pieces::empty(),
        head().set("ground", 30.0).set(
            "sections",
            json!(["hold to 43.7", "build 10 30 45", "hold to 250.3"]),
        ),
    )
    .unwrap();
    let mds = floats(&plan, "/controlPointParameters");
    assert!(mds.contains(&43.7), "{mds:?}");
    assert_eq!(*mds.last().unwrap(), 250.3);
    assert_eq!(floats(&plan, "/nodeMd"), mds);
    assert_eq!(
        role(&plan, "trajectory").json["MdInterval"]["MdMax"],
        json!(250.3)
    );

    // What the chords would have said — and that it is not the same is what
    // lets this test fail.
    let points = floats(&plan, "/controlPoints");
    let stations: Vec<&[f64]> = points.chunks(3).collect();
    let chords: f64 = stations
        .windows(2)
        .map(|pair| {
            ((pair[1][0] - pair[0][0]).powi(2)
                + (pair[1][1] - pair[0][1]).powi(2)
                + (pair[1][2] - pair[0][2]).powi(2))
            .sqrt()
        })
        .sum();
    assert!(250.3 - chords > 1e-3, "the chords add up to {chords}");
}

#[test]
fn a_plan_in_degrees_or_without_a_frame_is_refused() {
    let base = Choices::new()
        .set("title", "GTH-test")
        .set("headX", 2.33)
        .set("headY", 48.85)
        .set("ground", 40.0)
        .set("sections", json!(["hold to 100"]));
    // Degrees beside metres of depth: the refusal `well-from-polyline` makes.
    let degrees = planned(&Pieces::empty(), base.clone().set("sourceEpsg", 4326)).unwrap_err();
    assert!(
        degrees.contains("`project`") && degrees.contains("degrees"),
        "{degrees}"
    );
    // No frame at all: which way is down decides every z, so nothing is placed.
    let nowhere = planned(&Pieces::empty(), base).unwrap_err();
    assert!(
        nowhere.contains("`crs`") && nowhere.contains("sourceEpsg"),
        "{nowhere}"
    );
}

#[test]
fn which_way_is_down_is_read_from_the_frame_never_guessed() {
    // A frame counting depth: the ground is 45 above its zero, so at −45, and
    // each metre of depth is a metre **more**.
    let (pieces, choices) = in_frame(Some(false));
    let plan = planned(
        &pieces,
        choices
            .set("headX", 100.0)
            .set("headY", 200.0)
            .set("ground", -45.0)
            .set("sections", json!(["hold to 100"])),
    )
    .unwrap();
    let points = floats(&plan, "/controlPoints");
    assert_eq!(points[2], -45.0);
    assert!(near(points[points.len() - 1], 55.0, 1e-9), "{points:?}");
    assert_eq!(
        role(&plan, "trajectory").json["Geometry"]["LocalCrs"]["Uuid"],
        json!(frame().uuid)
    );

    // A frame that does not say: refused rather than drawn upside down.
    let (pieces, choices) = in_frame(None);
    let why = planned(
        &pieces,
        choices
            .set("headX", 100.0)
            .set("headY", 200.0)
            .set("ground", 45.0)
            .set("sections", json!(["hold to 100"])),
    )
    .unwrap_err();
    assert!(why.contains("`crs`") && why.contains("which way"), "{why}");
}

#[test]
fn impossible_plans_are_refused_by_their_section() {
    let refused = |list: &[&str]| {
        planned(
            &Pieces::empty(),
            head().set("ground", 40.0).set("sections", json!(list)),
        )
        .unwrap_err()
    };
    let why = refused(&["hold -5"]);
    assert!(
        why.contains("section 1") && why.contains("length") && why.contains("hold -5"),
        "{why}"
    );
    let why = refused(&["hold to 30", "build 15 190 0"]);
    assert!(why.contains("section 2") && why.contains("inclination"), "{why}");
    let why = refused(&["hold to 30", "build 0 30 0"]);
    assert!(why.contains("section 2") && why.contains("rate"), "{why}");
    let why = refused(&["hold to 50", "hold to 40"]);
    assert!(
        why.contains("section 2") && why.contains("does not come after"),
        "{why}"
    );
    let why = refused(&["hold to 30", "build 10 40 0", "build 10 20 0"]);
    assert!(
        why.contains("section 3") && why.contains("a build raises"),
        "{why}"
    );
    let why = refused(&["bild 15 30 0"]);
    assert!(why.contains("bild 15 30 0") && why.contains("hold to"), "{why}");
    // A ground-level datum above the ground says two things.
    let why = planned(
        &Pieces::empty(),
        head()
            .set("ground", 40.0)
            .set("datumHeight", 3.0)
            .set("sections", json!(["hold to 100"])),
    )
    .unwrap_err();
    assert!(why.contains("`datumKind`"), "{why}");
}

#[test]
fn the_same_plan_is_the_same_objects_and_another_plan_another_trajectory() {
    let plan = |last: &str| {
        planned(
            &Pieces::empty(),
            head()
                .set("ground", 40.0)
                .set("sections", json!(["hold to 40", "build 15 45 90", last])),
        )
        .unwrap()
    };
    let uuids = |plan: &Plan| plan.objects.iter().map(|o| o.uuid.clone()).collect::<Vec<_>>();
    let (one, again) = (plan("hold 50"), plan("hold 50"));
    assert_eq!(uuids(&one), uuids(&again));
    // The well is its name, the datum its place: kept. The trajectory and the
    // frame are their depths: another plan, other objects.
    let longer = plan("hold 60");
    for kept in ["datum", "feature", "interpretation"] {
        assert_eq!(role(&one, kept).uuid, role(&longer, kept).uuid, "{kept}");
    }
    for moved in ["trajectory", "frame"] {
        assert_ne!(role(&one, moved).uuid, role(&longer, moved).uuid, "{moved}");
    }
}

#[test]
fn the_ground_is_read_on_the_surface_under_the_head() {
    // 40 + 0.01 · 800 = 48 under the head, and the kelly bushing 3 m above it:
    // the measured depths start at 51.
    let pieces = Pieces::empty().with("surface", slope());
    let plan = planned(
        &pieces,
        head()
            .set("datumHeight", 3.0)
            .set("datumKind", "kelly bushing")
            .set("sections", json!(["hold to 100"])),
    )
    .unwrap();
    let datum = role(&plan, "datum");
    assert_eq!(datum.json["Kind"], json!("kelly bushing"));
    assert!(
        near(datum.json["VerticalCoordinate"].as_f64().unwrap(), 51.0, 1e-9),
        "{}",
        datum.json
    );
    let points = floats(&plan, "/controlPoints");
    assert!(
        near(points[2], 51.0, 1e-9),
        "the first station is the datum: {}",
        points[2]
    );
    assert!(near(recorded(&plan, "groundRead").expect("recorded"), 48.0, 1e-9));
    assert!(plan.says.iter().any(|note| note.key == "ground-read"));

    // Outside the surface: no height to read, and none invented.
    let outside = planned(
        &pieces,
        head()
            .set("headX", 653_000.0)
            .set("sections", json!(["hold to 100"])),
    )
    .unwrap_err();
    assert!(
        outside.contains("`surface`") && outside.contains("outside"),
        "{outside}"
    );
    // Two answers to one question.
    let both = planned(
        &pieces,
        head().set("ground", 40.0).set("sections", json!(["hold to 100"])),
    )
    .unwrap_err();
    assert!(both.contains("`ground`"), "{both}");
    // Neither.
    let neither = planned(&Pieces::empty(), head().set("sections", json!(["hold to 100"]))).unwrap_err();
    assert!(
        neither.contains("`ground`") && neither.contains("surface"),
        "{neither}"
    );
}

/// **The ground's interpretation is not the well's.** The shared suggestion
/// offers the interpretation of the one input that names one — here the
/// surface the ground is read on, a horizon's. Taken, the well's trajectory
/// represented the ground (found on 24/09 playing Lutetia: GTH-1 wrote no
/// feature, no interpretation, and pointed at `le sol`). A sidetrack's parent
/// is no better: a lateral is a wellbore of its own.
#[test]
fn a_well_makes_its_own_interpretation_never_the_grounds() {
    let ground = Ref::new(
        "bbbbbbbb-0000-4000-8000-000000000002",
        "resqml22.BoundaryFeatureInterpretation",
        "the ground",
    );
    let pieces = Pieces::empty().with("surface", slope()).with(
        "surface (object)",
        Source::Object(Known {
            interpretation: Some(ground.clone()),
            ..Default::default()
        }),
    );
    let plan = planned(&pieces, head().set("sections", json!(["hold to 100"]))).unwrap();
    let made = role(&plan, "interpretation");
    assert_eq!(made.qualified_type, "resqml22.WellboreInterpretation");
    let trajectory = role(&plan, "trajectory");
    assert_eq!(
        trajectory.json["RepresentedObject"]["Uuid"],
        json!(made.uuid),
        "{}",
        trajectory.json
    );
    assert_ne!(trajectory.json["RepresentedObject"]["Uuid"], json!(ground.uuid));
}

#[test]
fn a_surface_in_another_frame_is_refused() {
    let (pieces, choices) = in_frame(Some(true));
    let elsewhere = Ref::new(
        "bbbbbbbb-0000-4000-8000-000000000002",
        "eml23.LocalEngineeringCompoundCrs",
        "another frame",
    );
    let pieces = pieces.with("surface", slope()).with(
        "surface (object)",
        Source::Object(Known {
            crs: Some(elsewhere),
            vertical_up: Some(true),
            vertical_uom: Some("m".into()),
            ..Default::default()
        }),
    );
    let why = planned(
        &pieces,
        choices
            .set("headX", 651_800.0)
            .set("headY", 6_861_200.0)
            .set("sections", json!(["hold to 100"])),
    )
    .unwrap_err();
    assert!(
        why.contains("`surface`") && why.contains("another frame"),
        "{why}"
    );
}

#[test]
fn the_depth_limit_is_checked_never_bent_to() {
    // The datum at the ground: a vertical depth is the MD. 250 goes past 200
    // at the station 210.
    let deep = planned(
        &Pieces::empty(),
        head()
            .set("ground", 40.0)
            .set("maxDepth", 200.0)
            .set("sections", json!(["hold to 250"])),
    )
    .unwrap_err();
    assert!(deep.contains("`maxDepth`") && deep.contains("MD 210"), "{deep}");
    let within = planned(
        &Pieces::empty(),
        head()
            .set("ground", 40.0)
            .set("maxDepth", 200.0)
            .set("sections", json!(["hold to 200"])),
    )
    .unwrap();
    assert_eq!(recorded(&within, "depthMax"), Some(200.0));
}

/// GTH-3 as a parent: vertical at (100, 200) from its datum at 50, a node
/// every 10 m down to MD 200 — or, `bent`, turning between MD 50 and 100.
fn parent(bent: bool) -> Pieces {
    let (mds, points): (Vec<f64>, Vec<[f64; 3]>) = if bent {
        (
            vec![0.0, 50.0, 100.0],
            vec![[100.0, 200.0, 50.0], [100.0, 200.0, 0.0], [130.0, 200.0, -30.0]],
        )
    } else {
        (0..=20)
            .map(|k| (k as f64 * 10.0, [100.0, 200.0, 50.0 - k as f64 * 10.0]))
            .unzip()
    };
    let count = points.len() as u32;
    Pieces::empty()
        .with(
            "parent",
            Source::Shape {
                geometry: Geometry {
                    kind: "polyline",
                    patches: vec![Patch {
                        index: 0,
                        node_offset: 0,
                        points,
                        topology: Topology::Polylines(vec![count]),
                        closed: Vec::new(),
                    }],
                },
                closed: None,
            },
        )
        .with(
            "parent (object)",
            Source::Object(Known {
                reference: Some(Ref::new(
                    "cccccccc-0000-4000-8000-000000000003",
                    "resqml22.WellboreFrameRepresentation",
                    "GTH-3 — frame",
                )),
                crs: Some(frame()),
                vertical_up: Some(true),
                vertical_uom: Some("m".into()),
                node_mds: Some(mds),
                trajectory: Some(Ref::new(
                    "dddddddd-0000-4000-8000-000000000004",
                    "resqml22.WellboreTrajectoryRepresentation",
                    "GTH-3 — trajectory",
                )),
                datum: Some(Ref::new(
                    "eeeeeeee-0000-4000-8000-000000000005",
                    "eml23.ReferencePointInALocalEngineeringCompoundCrs",
                    "GTH-3 — datum",
                )),
                ..Default::default()
            }),
        )
}

/// The bag of a sidetrack: the frame, and the parent in it.
fn with_parent(bent: bool) -> (Pieces, Choices) {
    let (mut pieces, choices) = in_frame(Some(true));
    for (name, source) in parent(bent).iter() {
        pieces = pieces.with(name, source.clone());
    }
    (pieces, choices.set("title", "GTH-3 ST1"))
}

#[test]
fn a_sidetrack_leaves_its_parent_and_counts_from_its_datum() {
    // Kick-off at MD 60 on a vertical parent whose datum is at 50: the first
    // station is at z = 50 − 60 = −10. Build 15°/30 m to 45° west: at MD
    // 60 + 90 = 150 the hole is R (1 − cos 45°) = 33.5630906 west and
    // R sin 45° = 81.0284685 below the kick-off.
    let (pieces, choices) = with_parent(false);
    let plan = planned(
        &pieces,
        choices
            .set("kickoffMd", 60.0)
            .set("sections", json!(["build 15 45 270", "hold 30"])),
    )
    .unwrap();
    let mds = floats(&plan, "/controlPointParameters");
    let points = floats(&plan, "/controlPoints");
    assert_eq!(mds[0], 60.0, "the sidetrack's depths are the parent's");
    assert_eq!(&points[0..3], &[100.0, 200.0, -10.0]);
    let at = mds
        .iter()
        .position(|md| near(*md, 150.0, 1e-9))
        .expect("a station where the build ends");
    assert!(
        near(points[3 * at], 100.0 - 33.563_090_6, 1e-6),
        "{:?}",
        &points[3 * at..3 * at + 3]
    );
    assert!(
        near(points[3 * at + 2], -10.0 - 81.028_468_5, 1e-6),
        "{:?}",
        &points[3 * at..3 * at + 3]
    );

    // No datum of its own: the parent's, which is what its depths count from.
    assert!(plan.of_role("datum").next().is_none());
    let trajectory = role(&plan, "trajectory");
    assert_eq!(
        trajectory.json["MdInterval"]["Datum"]["Uuid"],
        json!("eeeeeeee-0000-4000-8000-000000000005")
    );
    assert_eq!(trajectory.json["MdInterval"]["MdMin"], json!(60.0));
    let intersection: &Value = &trajectory.json["ParentIntersection"];
    assert_eq!(intersection["KickoffMd"], json!(60.0), "{intersection}");
    assert_eq!(
        intersection["ParentTrajectory"]["Uuid"],
        json!("dddddddd-0000-4000-8000-000000000004"),
        "{intersection}"
    );
    assert!(
        intersection.get("ParentMd").is_none(),
        "the same MD in both: left out, as the schema says"
    );
}

#[test]
fn a_sidetrack_is_refused_where_its_parent_bends_or_when_half_said() {
    // Between MD 50 and 100 the parent's nodes are 42.4 apart for 50 m of
    // hole: it curves there, and its direction at MD 70 is not in its nodes.
    let (pieces, choices) = with_parent(true);
    let bent = planned(
        &pieces,
        choices
            .set("kickoffMd", 70.0)
            .set("sections", json!(["build 15 45 270"])),
    )
    .unwrap_err();
    assert!(
        bent.contains("`kickoffMd`") && bent.contains("not straight"),
        "{bent}"
    );

    let (pieces, choices) = with_parent(false);
    let no_depth = planned(
        &pieces,
        choices.clone().set("sections", json!(["build 15 45 270"])),
    )
    .unwrap_err();
    assert!(no_depth.contains("`kickoffMd`"), "{no_depth}");
    let off = planned(
        &pieces,
        choices
            .clone()
            .set("kickoffMd", 250.0)
            .set("sections", json!(["build 15 45 270"])),
    )
    .unwrap_err();
    assert!(off.contains("`kickoffMd`") && off.contains("200"), "{off}");
    let headed = planned(
        &pieces,
        choices
            .set("kickoffMd", 60.0)
            .set("headX", 1.0)
            .set("headY", 2.0)
            .set("sections", json!(["build 15 45 270"])),
    )
    .unwrap_err();
    assert!(headed.contains("`headX`"), "{headed}");

    // And a kick-off with no parent.
    let lonely = planned(
        &Pieces::empty(),
        head()
            .set("ground", 40.0)
            .set("kickoffMd", 60.0)
            .set("sections", json!(["hold to 100"])),
    )
    .unwrap_err();
    assert!(
        lonely.contains("`kickoffMd`") && lonely.contains("parent"),
        "{lonely}"
    );
}

/// **The trajectory is RESQML's minimum-curvature line, its tangents the
/// stations' directions.** `LineKindIndex` 5 with one `TangentVectors` row per
/// control point; at the end of a build to 45° toward the east in a frame
/// counting up, the direction is (sin 45°, 0, −cos 45°). Written as a linear
/// spline the same stations would carry no tangent and bow nowhere.
#[test]
fn a_planned_well_is_written_as_the_minimum_curvature_line_through_its_stations() {
    let plan = planned(
        &Pieces::empty(),
        head()
            .set("ground", 50.0)
            .set("sections", json!(["hold to 40", "build 15 45 90", "hold 50"])),
    )
    .unwrap();
    let trajectory = role(&plan, "trajectory");
    assert_eq!(
        trajectory.json["Geometry"]["LineKindIndex"],
        json!(5),
        "{}",
        trajectory.json
    );
    assert!(
        plan.says.iter().any(|note| note.key == "minimum-curvature"),
        "{:?}",
        plan.says
    );
    let mds = floats(&plan, "/controlPointParameters");
    let tangents = floats(&plan, "/tangentVectors");
    assert_eq!(tangents.len(), 3 * mds.len(), "one tangent per station");
    let at = mds
        .iter()
        .position(|md| near(*md, 130.0, 1e-9))
        .expect("a station where the build ends");
    let t = &tangents[3 * at..3 * at + 3];
    let half = std::f64::consts::FRAC_1_SQRT_2;
    assert!(
        near(t[0], half, 1e-9) && near(t[1], 0.0, 1e-9) && near(t[2], -half, 1e-9),
        "{t:?}"
    );
    let first = &tangents[0..3];
    assert!(
        near(first[2], -1.0, 1e-12),
        "straight down at the head: {first:?}"
    );
}

/// **Two readers of one arc agree between the stations.** A well planned
/// every 30 m is written, saved and read back by `energyml-geometry` — its
/// frame's nodes moved to every metre, so the trajectory is evaluated
/// **between** its stations, where the reader's circular-arc construction
/// (`parametric.rs`) does the work — and set against the same plan laid out
/// every metre by `energyml_analysis::well_plan` (the drilling formula, API
/// Bulletin D20). Two implementations of minimum curvature, neither checking
/// the other. On a 114.6 m radius a 30 m chord strays ~1 m from its arc: a
/// linear spline through the stations fails this by three orders of magnitude.
#[cfg(feature = "hdf5")]
#[test]
fn the_geometry_reader_follows_the_planned_arcs_between_the_stations() {
    use std::sync::Arc;
    let sections = json!([
        "hold to 40",
        "build 15 60 45",
        "hold 30",
        "curve 10 80 120",
        "hold 20"
    ]);
    let fine = planned(
        &Pieces::empty(),
        head()
            .set("ground", 50.0)
            .set("sections", sections.clone())
            .set("step", 1.0),
    )
    .unwrap();
    let fine_mds = floats(&fine, "/controlPointParameters");
    let fine_points = floats(&fine, "/controlPoints");

    let mut coarse = planned(
        &Pieces::empty(),
        head()
            .set("ground", 50.0)
            .set("sections", sections)
            .set("step", 30.0),
    )
    .unwrap();
    // The frame's nodes at every metre of the fine plan.
    let frame_uuid = role(&coarse, "frame").uuid.clone();
    let count = fine_mds.len();
    for object in coarse.objects.iter_mut().filter(|object| object.role == "frame") {
        object.json["NodeCount"] = json!(count);
        object.json["NodeMd"]["Values"]["ExternalDataArrayPart"][0]["Count"] = json!([count]);
    }
    let node_path = format!("/resqml22/{frame_uuid}/nodeMd");
    let dataset = coarse
        .datasets
        .iter_mut()
        .find(|d| d.path == node_path)
        .expect("the frame's node MDs");
    dataset.dimensions = vec![count as i64];
    dataset.values = Numbers::F64(fine_mds.clone());

    let dir = std::env::temp_dir().join(format!("energyml-build-well-plan-arcs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("arcs.epc");
    let registry = Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "arcs.epc", registry.clone()).unwrap();
    store.external_files_mut().set_root(&dir);
    let made = energyml_build::apply::apply(&coarse, &mut store, &when());
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
    let summary = reread.find_by_uuid(&frame_uuid).cloned().unwrap();
    let object = reread.get_object(&summary.uri).unwrap();
    let built = energyml_geometry::scene::object_of(object.as_reflect(), &reread, &summary.uri)
        .expect("the frame reads");
    let read: Vec<[f64; 3]> = built
        .geometry
        .patches
        .iter()
        .flat_map(|p| p.points.iter().copied())
        .collect();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(read.len(), count, "one point per node");
    let mut worst = 0.0f64;
    for (at, point) in read.iter().enumerate() {
        let planned = &fine_points[3 * at..3 * at + 3];
        let gap = ((point[0] - planned[0]).powi(2)
            + (point[1] - planned[1]).powi(2)
            + (point[2] - planned[2]).powi(2))
        .sqrt();
        worst = worst.max(gap);
    }
    assert!(worst < 1e-6, "the reader strays {worst} m from the planned arcs");
}
