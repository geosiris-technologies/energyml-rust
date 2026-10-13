//! `computed-tops` — where a well crosses horizons: the depth of each crossing, what
//! is written, what is said, what is refused.
//!
//! **The form of the assertions comes from the specification and from
//! geometry**: a marker's depth is the measured depth where the hole meets the
//! surface, so the position the path has there lies on the surface; N
//! boundaries make N+1 intervals (RESQML TRG v2.2 §12.15 p. 312); a boundary
//! references a `BoundaryFeatureInterpretation` (§12.6 p. 302) and a WITSML
//! `WellboreMarker` holding the depth (WITSML TRG v2.1 §24.6 pp. 582–583).
//! **The numbers are the tests' own**, each computed by hand beside its
//! assertion — a straight hole through a plane, a circular arc through a
//! level, a bilinear surface whose height is its formula.

use serde_json::json;

use energyml_build::operation::{Choices, Identity, NewObject, Plan, Registry, When};
use energyml_build::pieces::{Known, KnownPath, Pieces, Ref, Source};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};
use energyml_geometry::parametric::{Path, Vertical};

/// Lambert-93, near Paris: numbers the size a real model has.
const X0: f64 = 651_000.0;
const Y0: f64 = 6_861_000.0;

fn when() -> When {
    When {
        creation: "2026-09-25T00:00:00Z".into(),
        originator: "test".into(),
    }
}

fn frame() -> Ref {
    Ref::new(
        "aaaaaaaa-0000-4000-8000-00000000000f",
        "eml23.LocalEngineeringCompoundCrs",
        "Lutetia — test frame",
    )
}

fn trajectory() -> Ref {
    Ref::new(
        "bbbbbbbb-0000-4000-8000-000000000001",
        "resqml22.WellboreTrajectoryRepresentation",
        "GTH-test",
    )
}

/// A well whose line is `path`, drawn at its stations, in `frame` — an
/// elevation frame in metres, which is what the builders here write.
fn well(path: Path, frame: Ref) -> Vec<(String, Source)> {
    let knots = path.knots().expect("a line with its depths").to_vec();
    let stations: Vec<[f64; 3]> = knots.iter().map(|md| path.at(*md)).collect();
    let count = stations.len() as u32;
    vec![
        (
            "well".to_string(),
            Source::Shape {
                geometry: Geometry {
                    kind: "trajectory",
                    patches: vec![Patch {
                        index: 0,
                        node_offset: 0,
                        points: stations,
                        topology: Topology::Polylines(vec![count]),
                        closed: Vec::new(),
                    }],
                },
                closed: None,
            },
        ),
        (
            "well (object)".to_string(),
            Source::Object(Known {
                reference: Some(trajectory()),
                crs: Some(frame),
                vertical_up: Some(true),
                vertical_uom: Some("m".into()),
                interpretation: Some(Ref::new(
                    "bbbbbbbb-0000-4000-8000-000000000002",
                    "resqml22.WellboreInterpretation",
                    "GTH-test",
                )),
                path: Some(KnownPath {
                    trajectory: trajectory(),
                    path: Ok(path),
                    md_uom: Some("m".into()),
                    datum: Some(Ref::new(
                        "bbbbbbbb-0000-4000-8000-000000000003",
                        "eml23.ReferencePointInALocalEngineeringCompoundCrs",
                        "GTH-test datum",
                    )),
                }),
                ..Default::default()
            }),
        ),
    ]
}

/// A horizon: a 2D grid of `n × n` nodes, `spacing` apart from `(x0, y0)`,
/// whose heights are `f` — exact between the nodes whenever `f` is bilinear,
/// which is what a grid's cell is read as.
fn grid(x0: f64, y0: f64, n: usize, spacing: f64, f: impl Fn(f64, f64) -> f64) -> Geometry {
    let mut points = Vec::new();
    for j in 0..n {
        for i in 0..n {
            let (x, y) = (x0 + i as f64 * spacing, y0 + j as f64 * spacing);
            points.push([x, y, f(x, y)]);
        }
    }
    Geometry {
        kind: "grid",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points,
            topology: Topology::Grid {
                fastest: n,
                slowest: n,
            },
            closed: Vec::new(),
        }],
    }
}

/// A horizon of the package: its shape, and what is known of it — a
/// representation, its horizon interpretation, its frame.
fn horizon(slot: &str, rank: usize, title: &str, geometry: Geometry, frame: Ref) -> Vec<(String, Source)> {
    vec![
        (
            slot.to_string(),
            Source::Shape {
                geometry,
                closed: None,
            },
        ),
        (
            format!("{slot} (object)"),
            Source::Object(Known {
                reference: Some(Ref::new(
                    format!("cccccccc-0000-4000-8000-00000000000{rank}"),
                    "resqml22.Grid2dRepresentation",
                    title,
                )),
                interpretation: Some(Ref::new(
                    format!("dddddddd-0000-4000-8000-00000000000{rank}"),
                    "resqml22.HorizonInterpretation",
                    title,
                )),
                crs: Some(frame),
                vertical_up: Some(true),
                vertical_uom: Some("m".into()),
                ..Default::default()
            }),
        ),
    ]
}

/// The horizons of a run, named as the front end names them: `horizons`,
/// `horizons 2`, …
fn bag(well: Vec<(String, Source)>, horizons: Vec<(&str, Geometry, Ref)>) -> Pieces {
    let mut pieces = Pieces::empty();
    for (name, source) in well {
        pieces = pieces.with(name, source);
    }
    for (rank, (title, geometry, frame)) in horizons.into_iter().enumerate() {
        let slot = if rank == 0 {
            "horizons".to_string()
        } else {
            format!("horizons {}", rank + 1)
        };
        for (name, source) in horizon(&slot, rank + 1, title, geometry, frame) {
            pieces = pieces.with(name, source);
        }
    }
    pieces
}

fn planned(pieces: &Pieces, choices: Choices) -> Result<Plan, String> {
    Registry::new()
        .plan("computed-tops", pieces, &choices, &Identity::derived(), &when())
        .map_err(|why| why.to_string())
}

fn markers(plan: &Plan) -> Vec<&NewObject> {
    plan.of_role("marker").collect()
}

fn md_of(marker: &NewObject) -> f64 {
    marker.json["Md"]["MeasuredDepth"]["_"]
        .as_f64()
        .unwrap_or_else(|| panic!("a depth: {}", marker.json["Md"]))
}

fn recorded(plan: &Plan, name: &str) -> Option<f64> {
    plan.activity
        .parameters
        .iter()
        .find_map(|parameter| match parameter {
            Parameter::Double { title, value, .. } if title == name => Some(*value),
            _ => None,
        })
}

fn said<'a>(plan: &'a Plan, key: &str) -> Vec<&'a str> {
    plan.says
        .iter()
        .filter(|note| note.key == key)
        .map(|note| note.detail.as_str())
        .collect()
}

/// A straight hole, as a linear line through its stations.
fn straight(stations: &[([f64; 3], f64)]) -> Path {
    Path::new(
        1,
        stations.iter().map(|s| s.0).collect(),
        stations.iter().map(|s| s.1).collect(),
        None,
        Vertical::Up,
    )
    .expect("a line")
}

/// **A deviated hole through a tilted plane: a marker at the depth the algebra
/// gives, and everything the two schemas ask of it.**
///
/// The hole starts at `(X0 + 100, Y0 + 100, 40)` and runs along
/// `u = (0.6, 0, −0.8)`, one metre of line per metre of depth. The plane is
/// `z = −300 + 0.1 (x − X0)`. At depth `s`, `40 − 0.8 s = −300 + 0.1 (100 +
/// 0.6 s)`, so `s = 330 / 0.86`. To break: a root taken at a sample, a depth
/// counted from somewhere else, the boundary pointing at another marker.
#[test]
fn a_deviated_hole_gets_its_marker_at_the_algebras_depth() {
    let head = [X0 + 100.0, Y0 + 100.0, 40.0];
    let end = 1000.0;
    let path = straight(&[
        (head, 0.0),
        ([head[0] + 0.6 * end, head[1], head[2] - 0.8 * end], end),
    ]);
    let plane = grid(X0 - 1000.0, Y0 - 1000.0, 5, 1000.0, |x, _| {
        -300.0 + 0.1 * (x - X0)
    });
    let plan = planned(
        &bag(well(path, frame()), vec![("H3 — toit", plane, frame())]),
        Choices::new(),
    )
    .expect("a plan");

    let found = markers(&plan);
    assert_eq!(found.len(), 1);
    let expected = 330.0 / 0.86;
    let md = md_of(found[0]);
    assert!((md - expected).abs() < 1e-9, "{md} against {expected}");
    assert_eq!(found[0].qualified_type, "witsml21.WellboreMarker");
    assert_eq!(found[0].json["Md"]["MeasuredDepth"]["uom"], json!("m"));
    assert_eq!(
        found[0].json["Md"]["Datum"]["Uuid"],
        json!("bbbbbbbb-0000-4000-8000-000000000003"),
        "counted from the trajectory's datum"
    );
    assert_eq!(found[0].json["WellboreMarkerKind"], json!("stratigraphic"));
    assert_eq!(found[0].json["Citation"]["Title"], json!("H3 — toit — GTH-test"));

    let set = plan.of_role("interval-set").next().expect("the interval set");
    assert_eq!(set.qualified_type, "resqml22.WellboreIntervalSet");
    let boundaries = set.json["IntervalBoundaries"].as_array().expect("boundaries");
    assert_eq!(boundaries.len(), 1);
    assert_eq!(boundaries[0]["Marker"]["Uuid"], json!(found[0].uuid));
    assert_eq!(
        boundaries[0]["Interpretation"]["Uuid"],
        json!("dddddddd-0000-4000-8000-000000000001"),
        "the horizon's interpretation"
    );
    assert_eq!(boundaries[0]["GeologicBoundaryKind"], json!("horizon"));
    // N boundaries, N + 1 intervals — TRG v2.2 §12.15 p. 312.
    assert_eq!(set.json["MarkerInterval"].as_array().map(Vec::len), Some(2));
    assert_eq!(
        set.json["RepresentedObject"]["Uuid"],
        json!("bbbbbbbb-0000-4000-8000-000000000002")
    );

    // The position said and drawn is the hole at that depth, on the plane.
    let drawn = plan.shapes[0].geometry.patches[0].points[0];
    let on_line = [head[0] + 0.6 * md, head[1], head[2] - 0.8 * md];
    assert!(
        (0..3).all(|k| (drawn[k] - on_line[k]).abs() < 1e-6),
        "{drawn:?} against {on_line:?}"
    );
    assert!((drawn[2] - (-300.0 + 0.1 * (drawn[0] - X0))).abs() < 1e-6);
    assert_eq!(plan.shapes[0].object.as_deref(), Some(set.uuid.as_str()));
    assert_eq!(recorded(&plan, "markerCount"), Some(1.0));
    assert_eq!(recorded(&plan, "notReachedCount"), Some(0.0));
}

/// **A curved hole is followed along its arc, not along its chord.**
///
/// A build from the vertical to the horizontal, radius `R = 600`: two
/// stations, `R π / 2` apart in depth, their tangents down then east — a
/// minimum-curvature line (kind 5), which the reader evaluates as the arc
/// `x = R (1 − cos(s/R))`, `z = −R sin(s/R)` from the head. A level `d` below
/// the head is met at `s = R asin(d / R)`. The chord between the two stations
/// meets it elsewhere — `s = (R π / 2) · d / R` along it —, so straightening the
/// line makes this red.
#[test]
fn a_curved_hole_is_followed_between_its_stations() {
    let r = 600.0;
    let head = [X0 + 100.0, Y0 + 100.0, 40.0];
    let end = r * std::f64::consts::FRAC_PI_2;
    let path = Path::new(
        5,
        vec![head, [head[0] + r, head[1], head[2] - r]],
        vec![0.0, end],
        Some(vec![[0.0, 0.0, -1.0], [1.0, 0.0, 0.0]]),
        Vertical::Up,
    )
    .expect("an arc");
    let d = 250.0;
    let level = grid(X0 - 1000.0, Y0 - 1000.0, 3, 1500.0, |_, _| head[2] - d);
    let plan = planned(
        &bag(well(path, frame()), vec![("H2", level, frame())]),
        Choices::new(),
    )
    .expect("a plan");
    let md = md_of(markers(&plan)[0]);
    let expected = r * (d / r).asin();
    assert!((md - expected).abs() < 1e-6, "{md} against {expected}");
}

/// **Each marker's position lies on its horizon** — the confrontation this
/// operation is judged by, and it asks nothing of the code that found them.
///
/// The hole is the arc above, re-evaluated **from the marker's depth by its own
/// formula**; the horizon is a grid of the bilinear
/// `f = −200 + 0.05 (x − X0) − 0.02 (y − Y0) + 1e-5 (x − X0)(y − Y0)`, whose
/// height anywhere *is* `f` — a grid's cell is read bilinearly, and `f` is
/// bilinear. So `|z(md) − f(x(md), y(md))|` is measured with two formulas and
/// no reader.
///
/// **The tolerance, 1e-6 m.** The depth is bisected to adjacent doubles (1e-13
/// m at a kilometre); the path moves at most one metre per metre of depth and
/// the surface rises at most `0.05 + 0.02 + 1e-5 · 2000 ≈ 0.09` per metre, so
/// the depth alone costs under 1e-12 m. What remains is the rounding of
/// map-scale coordinates — a northing of 6.86e6 m carries ~1e-9 m — and the
/// reader's closed form of the arc against the formula here, ~1e-10 m on a
/// 600 m radius. 1e-6 m is three orders above all of it, and three below a
/// millimetre, finer than any depth is ever stated.
#[test]
fn each_markers_position_lies_on_its_horizon() {
    let r = 600.0;
    let head = [X0 + 300.0, Y0 + 400.0, 40.0];
    let end = r * std::f64::consts::FRAC_PI_2;
    let arc = |s: f64| {
        [
            head[0] + r * (1.0 - (s / r).cos()),
            head[1],
            head[2] - r * (s / r).sin(),
        ]
    };
    let path = Path::new(
        5,
        vec![arc(0.0), arc(end)],
        vec![0.0, end],
        Some(vec![[0.0, 0.0, -1.0], [1.0, 0.0, 0.0]]),
        Vertical::Up,
    )
    .expect("an arc");
    let f = |x: f64, y: f64| -200.0 + 0.05 * (x - X0) - 0.02 * (y - Y0) + 1e-5 * (x - X0) * (y - Y0);
    let g = |x: f64, y: f64| -420.0 + 0.03 * (x - X0) + 0.01 * (y - Y0);
    let plan = planned(
        &bag(
            well(path, frame()),
            vec![
                ("H1", grid(X0, Y0, 21, 100.0, f), frame()),
                ("H2", grid(X0, Y0, 11, 200.0, g), frame()),
            ],
        ),
        Choices::new(),
    )
    .expect("a plan");
    let found = markers(&plan);
    assert_eq!(found.len(), 2, "{:#?}", plan.says);
    let mut checked = 0;
    for (marker, height) in found.iter().zip([&f as &dyn Fn(f64, f64) -> f64, &g]) {
        let md = md_of(marker);
        let p = arc(md);
        let off = (p[2] - height(p[0], p[1])).abs();
        assert!(
            off < 1e-6,
            "`{}` at MD {md}: {off} m off its horizon",
            marker.title
        );
        checked += 1;
    }
    assert_eq!(checked, 2);
    // In the order of the hole: H1 is the shallower here.
    assert!(md_of(found[0]) < md_of(found[1]));
}

/// **Down through a horizon and back up through it: two markers, said.**
///
/// Three stations — down 400 m over 500 m of hole, then back up — through the
/// level −150: `−400 s / 500 = −150` at `s = 187.5`, and on the way up
/// `−400 + 400 (s − 500) / 500 = −150` at `s = 812.5`.
#[test]
fn a_hole_that_turns_back_crosses_twice_and_says_so() {
    let head = [X0 + 10.0, Y0 + 10.0, 0.0];
    let path = straight(&[
        (head, 0.0),
        ([head[0] + 300.0, head[1], -400.0], 500.0),
        ([head[0] + 600.0, head[1], 0.0], 1000.0),
    ]);
    let level = grid(X0 - 500.0, Y0 - 500.0, 3, 1000.0, |_, _| -150.0);
    let plan = planned(
        &bag(well(path, frame()), vec![("H4", level, frame())]),
        Choices::new(),
    )
    .expect("a plan");
    let found = markers(&plan);
    assert_eq!(found.len(), 2);
    assert!((md_of(found[0]) - 187.5).abs() < 1e-9 && (md_of(found[1]) - 812.5).abs() < 1e-9);
    assert_eq!(
        found[0].json["Citation"]["Title"],
        json!("H4 — GTH-test (1 of 2)")
    );
    assert_eq!(
        found[1].json["Citation"]["Title"],
        json!("H4 — GTH-test (2 of 2)")
    );
    let again = said(&plan, "crossed-again");
    assert_eq!(again.len(), 1);
    assert!(
        again[0].contains("2 times") && again[0].contains("187.500") && again[0].contains("812.500"),
        "{again:?}"
    );
    let crossings = said(&plan, "crossing");
    assert!(
        crossings[0].contains("from above") && crossings[1].contains("from below"),
        "{crossings:?}"
    );
    let set = plan.of_role("interval-set").next().unwrap();
    assert_eq!(set.json["IntervalBoundaries"].as_array().map(Vec::len), Some(2));
    assert_eq!(set.json["MarkerInterval"].as_array().map(Vec::len), Some(3));
}

/// **A horizon never reached gives no marker, and is counted and named** — with
/// what the hole did instead. The hole ends at −760 (`40 − 0.8 · 1000`); the
/// deeper horizon is at −1000, 240 below its end.
#[test]
fn a_horizon_never_reached_gives_no_marker_and_is_named() {
    let head = [X0 + 100.0, Y0 + 100.0, 40.0];
    let path = straight(&[(head, 0.0), ([head[0] + 600.0, head[1], head[2] - 800.0], 1000.0)]);
    let shallow = grid(X0 - 1000.0, Y0 - 1000.0, 3, 1500.0, |_, _| -300.0);
    let deep = grid(X0 - 1000.0, Y0 - 1000.0, 3, 1500.0, |_, _| -1000.0);
    let beside = grid(X0 + 50_000.0, Y0, 3, 100.0, |_, _| -100.0);
    let plan = planned(
        &bag(
            well(path, frame()),
            vec![
                ("H1", shallow, frame()),
                ("H9", deep, frame()),
                ("far away", beside, frame()),
            ],
        ),
        Choices::new(),
    )
    .expect("a plan");
    assert_eq!(markers(&plan).len(), 1);
    assert_eq!(plan.dropped.len(), 1);
    assert_eq!(plan.dropped[0].count, 2);
    assert_eq!(recorded(&plan, "notReachedCount"), Some(2.0));
    let named = said(&plan, "not-reached");
    assert!(
        named
            .iter()
            .any(|n| n.contains("`H9`") && n.contains("stays above") && n.contains("240.000")),
        "{named:?}"
    );
    assert!(
        named
            .iter()
            .any(|n| n.contains("`far away`") && n.contains("beside")),
        "{named:?}"
    );
}

/// **No crossing at all is a refusal naming each horizon** — a plan with
/// nothing to write reads like *nothing to do here*.
#[test]
fn no_crossing_at_all_is_refused_naming_each_horizon() {
    let head = [X0 + 100.0, Y0 + 100.0, 40.0];
    let path = straight(&[(head, 0.0), ([head[0], head[1], head[2] - 500.0], 500.0)]);
    let deep = grid(X0 - 1000.0, Y0 - 1000.0, 3, 1500.0, |_, _| -2000.0);
    let why = planned(
        &bag(well(path, frame()), vec![("H9", deep, frame())]),
        Choices::new(),
    )
    .expect_err("nothing to write");
    assert!(
        why.contains("crosses none") && why.contains("`H9`") && why.contains("stays above"),
        "{why}"
    );
}

/// **A horizon in another frame is refused, naming both.**
#[test]
fn a_horizon_in_another_frame_is_refused_naming_both() {
    let head = [X0 + 100.0, Y0 + 100.0, 40.0];
    let path = straight(&[(head, 0.0), ([head[0], head[1], head[2] - 500.0], 500.0)]);
    let other = Ref::new(
        "aaaaaaaa-0000-4000-8000-0000000000ee",
        "eml23.LocalEngineeringCompoundCrs",
        "Another frame",
    );
    let level = grid(X0 - 1000.0, Y0 - 1000.0, 3, 1500.0, |_, _| -100.0);
    let why = planned(
        &bag(well(path, frame()), vec![("H2", level, other)]),
        Choices::new(),
    )
    .expect_err("two frames");
    assert!(
        why.contains("Another frame") && why.contains("Lutetia — test frame") && why.contains("`H2`"),
        "{why}"
    );
}

/// **A well drawn elsewhere than its line puts it is refused**: the shape read
/// placed for the scene, the line local to its CRS. 500 m of vertical offset
/// here, what a compound whose origin is not zero would give.
#[test]
fn a_well_drawn_away_from_its_line_is_refused() {
    let head = [X0 + 100.0, Y0 + 100.0, 40.0];
    let path = straight(&[(head, 0.0), ([head[0], head[1], head[2] - 500.0], 500.0)]);
    let mut pieces = well(path, frame());
    if let Source::Shape { geometry, .. } = &mut pieces[0].1 {
        for point in &mut geometry.patches[0].points {
            point[2] += 500.0;
        }
    }
    let level = grid(X0 - 1000.0, Y0 - 1000.0, 3, 1500.0, |_, _| -100.0);
    let why = planned(&bag(pieces, vec![("H2", level, frame())]), Choices::new()).expect_err("two frames");
    assert!(why.contains("500.000 away") && why.contains("`well`"), "{why}");
}

/// **A well whose line cannot be followed is refused with the reason the
/// reader gave**, and a shape that is no well is refused as such.
#[test]
fn a_well_without_a_line_is_refused_by_name() {
    let head = [X0 + 100.0, Y0 + 100.0, 40.0];
    let path = straight(&[(head, 0.0), ([head[0], head[1], head[2] - 500.0], 500.0)]);
    let mut pieces = well(path, frame());
    if let Source::Object(known) = &mut pieces[1].1 {
        known.path.as_mut().unwrap().path =
            Err("`GTH-test`: a wellbore trajectory with no `Geometry`".to_string());
    }
    let level = || grid(X0 - 1000.0, Y0 - 1000.0, 3, 1500.0, |_, _| -100.0);
    let why = planned(
        &bag(pieces.clone(), vec![("H2", level(), frame())]),
        Choices::new(),
    )
    .expect_err("no line");
    assert!(
        why.contains("cannot be followed") && why.contains("no `Geometry`"),
        "{why}"
    );

    if let Source::Object(known) = &mut pieces[1].1 {
        known.path = None;
    }
    let why = planned(&bag(pieces, vec![("H2", level(), frame())]), Choices::new()).expect_err("no well");
    assert!(why.contains("neither a wellbore trajectory nor a frame"), "{why}");
}

/// **The same run names the same objects; a moved horizon is a second marker.**
#[test]
fn a_moved_horizon_is_a_second_marker() {
    let head = [X0 + 100.0, Y0 + 100.0, 40.0];
    let path = || straight(&[(head, 0.0), ([head[0], head[1], head[2] - 500.0], 500.0)]);
    let at = |z: f64| grid(X0 - 1000.0, Y0 - 1000.0, 3, 1500.0, move |_, _| z);
    let uuid = |plan: &Plan| markers(plan)[0].uuid.clone();
    let first = planned(
        &bag(well(path(), frame()), vec![("H2", at(-100.0), frame())]),
        Choices::new(),
    )
    .unwrap();
    let again = planned(
        &bag(well(path(), frame()), vec![("H2", at(-100.0), frame())]),
        Choices::new(),
    )
    .unwrap();
    let moved = planned(
        &bag(well(path(), frame()), vec![("H2", at(-110.0), frame())]),
        Choices::new(),
    )
    .unwrap();
    assert_eq!(uuid(&first), uuid(&again));
    assert_ne!(uuid(&first), uuid(&moved));
}

/// What the HDF5 end-to-end reads of an object of the saved package.
#[cfg(feature = "hdf5")]
fn json_of(store: &energyml_storage::EpcStore, uuid: &str) -> serde_json::Value {
    let summary = store
        .find_by_uuid(uuid)
        .cloned()
        .unwrap_or_else(|| panic!("{uuid} in the package"));
    let object = store.get_object(&summary.uri).unwrap();
    energyml_core::json::write_value(object.as_reflect())
}

/// **Applied, saved, read back** — a well planned with a build section, a
/// tilted triangulated horizon, both written with their arrays; the markers
/// computed from the package as the front end and the CLI read it, saved, and read
/// back: each marker's depth, evaluated on the trajectory **as read back**,
/// lies on the plane the horizon was made of (`z = −150 + 0.1 (x − X0)`), and
/// every reference of the interval set resolves in the package.
#[cfg(feature = "hdf5")]
#[test]
fn markers_are_applied_saved_and_read_back_on_their_horizon() {
    use std::sync::Arc;
    let dir = std::env::temp_dir().join(format!("energyml-build-computed-tops-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("tops.epc");
    let registry = Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "tops.epc", registry.clone()).unwrap();
    store.external_files_mut().set_root(&dir);

    // The well: Lambert-93 over NGF-IGN69, a hold then a build of 60° at
    // 3°/30 m eastward then a hold — minimum curvature.
    let choices = Choices::new()
        .set("sourceEpsg", 2154)
        .set("project", 2154)
        .set("vertical", 5720)
        .set("title", "GTH-tops")
        .set("headX", X0 + 800.0)
        .set("headY", Y0 + 200.0)
        .set("ground", 50.0)
        .set("sections", json!(["hold to 100", "build 3 60 90", "hold 400"]));
    let well_plan = Registry::new()
        .plan(
            "well-from-plan",
            &Pieces::empty(),
            &choices,
            &Identity::derived(),
            &when(),
        )
        .expect("the well");
    let made = energyml_build::apply::apply(&well_plan, &mut store, &when());
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(made.refused.is_empty(), "{:?}", made.refused);
    let trajectory_uuid = well_plan.of_role("trajectory").next().unwrap().uuid.clone();
    let crs = well_plan.of_role("crs").next().expect("the frame").clone();
    let crs = Ref::new(crs.uuid, crs.qualified_type, crs.title);

    // The horizon: a tilted plane, two triangles over the well, in its frame.
    let plane = |x: f64| -150.0 + 0.1 * (x - X0);
    let corners = [
        [X0 - 2000.0, Y0 - 2000.0],
        [X0 + 4000.0, Y0 - 2000.0],
        [X0 + 4000.0, Y0 + 4000.0],
        [X0 - 2000.0, Y0 + 4000.0],
    ];
    let points: Vec<[f64; 3]> = corners.iter().map(|c| [c[0], c[1], plane(c[0])]).collect();
    let surface = Pieces::empty()
        .with(
            "shape",
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
            },
        )
        .with(
            "shape (object)",
            Source::Object(Known {
                crs: Some(crs.clone()),
                vertical_up: Some(true),
                vertical_uom: Some("m".into()),
                ..Default::default()
            }),
        );
    let choices = Choices::new()
        .set("crs", crs.dor())
        .set("title", "H5 — top of the Lutétien")
        .set("featureType", "resqml22.BoundaryFeature")
        .set("interpretationType", "resqml22.HorizonInterpretation");
    let horizon_plan = Registry::new()
        .plan("triangles", &surface, &choices, &Identity::derived(), &when())
        .expect("the horizon");
    let made = energyml_build::apply::apply(&horizon_plan, &mut store, &when());
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(made.refused.is_empty(), "{:?}", made.refused);
    let horizon_uuid = horizon_plan
        .of_role("representation")
        .next()
        .unwrap()
        .uuid
        .clone();

    // The bag, as the front end and the CLI build it.
    let mut pieces = Pieces::empty();
    for (name, source) in energyml_build::read::in_slot(
        "well",
        energyml_build::read::pieces_of_object(&store, &trajectory_uuid).unwrap(),
    )
    .into_iter()
    .chain(energyml_build::read::in_slot(
        "horizons",
        energyml_build::read::pieces_of_object(&store, &horizon_uuid).unwrap(),
    )) {
        pieces = pieces.with(name, source);
    }
    let tops = Registry::new()
        .plan(
            "computed-tops",
            &pieces,
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .expect("the tops");
    let made = energyml_build::apply::apply(&tops, &mut store, &when());
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
    let summary = reread.find_by_uuid(&trajectory_uuid).cloned().unwrap();
    let object = reread.get_object(&summary.uri).unwrap();
    let path = energyml_geometry::well::path_of(object.as_reflect(), &reread)
        .expect("the trajectory's line reads back");

    let set_uuid = tops.of_role("interval-set").next().unwrap().uuid.clone();
    let set = json_of(&reread, &set_uuid);
    let boundaries = set["IntervalBoundaries"].as_array().expect("boundaries").clone();
    assert!(!boundaries.is_empty());
    let interpretation = horizon_plan
        .of_role("interpretation")
        .next()
        .expect("the horizon's interpretation")
        .uuid
        .clone();
    let mut checked = 0;
    for boundary in &boundaries {
        assert_eq!(boundary["Interpretation"]["Uuid"], json!(interpretation));
        let marker_uuid = boundary["Marker"]["Uuid"].as_str().expect("a marker").to_string();
        let marker = json_of(&reread, &marker_uuid);
        let md = marker["Md"]["MeasuredDepth"]["_"].as_f64().expect("a depth");
        let datum = marker["Md"]["Datum"]["Uuid"]
            .as_str()
            .expect("a datum")
            .to_string();
        assert!(
            reread.find_by_uuid(&datum).is_some(),
            "the datum {datum} is in the package"
        );
        let p = path.at(md);
        let off = (p[2] - plane(p[0])).abs();
        // The tolerance of `each_markers_position_lies_on_its_horizon`: the
        // depth to adjacent doubles, the rounding of map coordinates.
        assert!(off < 1e-6, "MD {md}: {off} m off the plane");
        checked += 1;
    }
    assert_eq!(
        set["MarkerInterval"].as_array().map(Vec::len),
        Some(boundaries.len() + 1)
    );
    let _ = std::fs::remove_dir_all(&dir);
    assert!(checked >= 1);
}
