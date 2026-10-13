//! `fault-throw` — a horizon torn along its faults: the surface with its
//! `SplitEdgePatch`, the new interpretation it represents, and the lips
//! (step 7 of the reference model).
//!
//! The form of the assertions comes from the schema and the reference-model description: a split
//! edge is *“a consecutive pair of nodes”* in *“a 2D array […] with
//! dimensions 2 x numSplitEdges”* (RESQML 2.2 Structural.xsd l. 206–219; TRG
//! 2.2 §11.7 p. 276); a changed set of points represents a **new**
//! interpretation (usage guide 2.2, draft, §7.2.2 p. 72); the lips are
//! `contact` lines (Structural.xsd l. 60–64); an invented object carries
//! `synthetic = true` and its source. The
//! numbers are the tests' own, at map coordinates where Lutetia is.
//!
//! # What would have to break to make it turn red
//!
//! Declaring a split edge that is not one, or keeping quiet about one; filing a
//! triangle on the wrong side; making the surface represent the old
//! interpretation; writing the lips elsewhere than on the nodes of the
//! surface; accepting two lists of different lengths, a set without a vertical
//! sense, an already triangulated surface, two frames; giving the lips a
//! uuid that the surface has taken.

use std::collections::{BTreeSet, HashMap};

use serde_json::json;

use energyml_build::operation::{Choices, Fitness, Identity, NewObject, Numbers, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

const X0: f64 = 651_000.0;
const Y0: f64 = 6_861_000.0;

fn when() -> When {
    When {
        creation: "2026-09-25T00:00:00Z".into(),
        originator: "test".into(),
    }
}

/// The frame Lutetia's boreholes are written in: a compound, in elevation.
fn frame() -> Ref {
    Ref::new(
        "cccccccc-0000-4000-8000-000000000003",
        "eml23.LocalEngineeringCompoundCrs",
        "Lutetia — boreholes (elevation)",
    )
}

fn feature() -> Ref {
    Ref::new(
        "fe000000-0000-4000-8000-000000000006",
        "resqml22.BoundaryFeature",
        "H6 — top of the Chalk — feature",
    )
}

fn old_interpretation() -> Ref {
    Ref::new(
        "1e000000-0000-4000-8000-000000000006",
        "resqml22.HorizonInterpretation",
        "H6 — top of the Chalk — interpretation",
    )
}

/// What the bag knows of an object in the frame.
fn known(reference: Ref, interpretation: Option<Ref>) -> Known {
    Known {
        reference: Some(reference),
        crs: Some(frame()),
        vertical_uom: Some("m".into()),
        vertical_up: Some(true),
        interpretation,
        ..Default::default()
    }
}

/// A flat horizon at −50 m, a 21 × 21 lattice 10 m apart — a grid of the
/// package, as `read::in_slot` puts it in the bag.
fn horizon() -> Source {
    let mut points = Vec::new();
    for j in 0..21 {
        for i in 0..21 {
            points.push([X0 + i as f64 * 10.0, Y0 + j as f64 * 10.0, -50.0]);
        }
    }
    Source::Shape {
        geometry: Geometry {
            kind: "grid",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points,
                topology: Topology::Grid {
                    fastest: 21,
                    slowest: 21,
                },
                closed: Vec::new(),
            }],
        },
        closed: None,
    }
}

/// A plane fault striking north through x = 105 at −50 m, dipping `dip`°
/// east, from 0 to −100 m, from y = 45 to 155.
fn fault(dip: f64) -> Source {
    let at = |z: f64| X0 + 105.0 + (-50.0 - z) / dip.to_radians().tan();
    let points = vec![
        [at(0.0), Y0 + 45.0, 0.0],
        [at(0.0), Y0 + 155.0, 0.0],
        [at(-100.0), Y0 + 45.0, -100.0],
        [at(-100.0), Y0 + 155.0, -100.0],
    ];
    Source::Shape {
        geometry: Geometry {
            kind: "fault",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points,
                topology: Topology::Triangles(vec![[0, 1, 2], [1, 3, 2]]),
                closed: Vec::new(),
            }],
        },
        closed: None,
    }
}

fn bag_with(fault_shape: Source, fault_frame: Ref) -> Pieces {
    let mut fault_known = known(
        Ref::new(
            "fa000000-0000-4000-8000-000000000001",
            "resqml22.TriangulatedSetRepresentation",
            "F1",
        ),
        None,
    );
    fault_known.crs = Some(fault_frame);
    Pieces::empty()
        .with("horizon", horizon())
        .with(
            "horizon (object)",
            Source::Object(known(
                Ref::new(
                    "9d000000-0000-4000-8000-000000000006",
                    "resqml22.Grid2dRepresentation",
                    "H6 — top of the Chalk",
                ),
                Some(old_interpretation()),
            )),
        )
        .with("faults", fault_shape)
        .with("faults (object)", Source::Object(fault_known))
        .with(
            "feature",
            Source::Object(Known {
                reference: Some(feature()),
                ..Default::default()
            }),
        )
}

fn bag() -> Pieces {
    bag_with(fault(60.0), frame())
}

/// What every test says, but whether the throw is invented.
fn base() -> Choices {
    Choices::new()
        .set("crs", frame().dor())
        .set("title", "H6 — top of the Chalk, faulted")
        .set("maximumThrow", json!([10.0]))
        .set("throw", json!(["normal"]))
        .set("profile", "parabolic")
        .set("reach", 50.0)
}

fn choices() -> Choices {
    base()
        .set("synthetic", true)
        .set("source", "synthetic — the test's own throw")
}

fn planned(pieces: &Pieces, choices: &Choices) -> Result<Plan, String> {
    Registry::new()
        .plan("fault-throw", pieces, choices, &Identity::derived(), &when())
        .map_err(|why| why.message())
}

fn one<'a>(plan: &'a Plan, qualified_type: &str) -> &'a NewObject {
    let found: Vec<&NewObject> = plan
        .objects
        .iter()
        .filter(|o| o.qualified_type == qualified_type)
        .collect();
    assert_eq!(
        found.len(),
        1,
        "one {qualified_type}: {:?}",
        plan.objects.iter().map(|o| &o.qualified_type).collect::<Vec<_>>()
    );
    found[0]
}

fn floats(plan: &Plan, path: &str) -> Vec<[f64; 3]> {
    let found = plan
        .datasets
        .iter()
        .find(|d| d.path == path)
        .unwrap_or_else(|| panic!("no dataset {path}"));
    match &found.values {
        Numbers::F64(values) => values.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect(),
        other => panic!("{path}: {other:?}"),
    }
}

fn integers(plan: &Plan, path: &str) -> Vec<i64> {
    let found = plan
        .datasets
        .iter()
        .find(|d| d.path == path)
        .unwrap_or_else(|| panic!("no dataset {path}"));
    match &found.values {
        Numbers::I64(values) => values.clone(),
        other => panic!("{path}: {other:?}"),
    }
}

/// The path of an `ExternalDataArray`.
fn path_in(external: &serde_json::Value) -> String {
    external["ExternalDataArrayPart"][0]["PathInExternalFile"]
        .as_str()
        .expect("an external array")
        .to_string()
}

/// The path of an integer array whose values are external.
fn path_of(array: &serde_json::Value) -> String {
    path_in(&array["Values"])
}

fn number(plan: &Plan, title: &str) -> f64 {
    let found: Vec<f64> = plan
        .activity
        .parameters
        .iter()
        .filter_map(|p| match p {
            Parameter::Double { title: t, value, .. } if t == title => Some(*value),
            _ => None,
        })
        .collect();
    assert_eq!(found.len(), 1, "`{title}` recorded once: {found:?}");
    found[0]
}

/// The surface as written: its points and triangles, read from its datasets.
fn written_surface(plan: &Plan) -> (Vec<[f64; 3]>, Vec<[i64; 3]>) {
    let surface = one(plan, "resqml22.TriangulatedSetRepresentation");
    let patch = &surface.json["TrianglePatch"][0];
    let points = floats(plan, &path_in(&patch["Geometry"]["Points"]["Coordinates"]));
    let triangles = integers(plan, &path_of(&patch["Triangles"]))
        .chunks_exact(3)
        .map(|t| [t[0], t[1], t[2]])
        .collect();
    (points, triangles)
}

/// The split edges as declared, one list per `EdgePatch`.
fn split_edges(plan: &Plan) -> Vec<Vec<[i64; 2]>> {
    let surface = one(plan, "resqml22.TriangulatedSetRepresentation");
    surface.json["TrianglePatch"][0]["SplitEdgePatch"]
        .as_array()
        .expect("a SplitEdgePatch list")
        .iter()
        .map(|edge_patch| {
            let array = &edge_patch["SplitEdges"];
            let count = &array["Values"]["ExternalDataArrayPart"][0]["Count"];
            let pairs: Vec<[i64; 2]> = integers(plan, &path_of(array))
                .chunks_exact(2)
                .map(|p| [p[0], p[1]])
                .collect();
            assert_eq!(
                count,
                &json!([pairs.len(), 2]),
                "numSplitEdges × 2, as the triangles are numTriangles × 3"
            );
            pairs
        })
        .collect()
}

#[test]
fn a_torn_horizon_is_a_surface_a_new_interpretation_and_its_lips() {
    let plan = planned(&bag(), &choices()).expect("planned");
    assert_eq!(
        plan.objects.len(),
        3,
        "the frame is named: {:?}",
        plan.objects.iter().map(|o| &o.qualified_type).collect::<Vec<_>>()
    );
    let surface = one(&plan, "resqml22.TriangulatedSetRepresentation");
    let interpretation = one(&plan, "resqml22.HorizonInterpretation");
    let lips = one(&plan, "resqml22.PolylineSetRepresentation");
    // A new interpretation of the same feature, and the surface
    // represents it — not the one the horizon had.
    assert_eq!(
        interpretation.json["InterpretedFeature"]["Uuid"],
        json!(feature().uuid)
    );
    assert_eq!(
        surface.json["RepresentedObject"]["Uuid"],
        json!(interpretation.uuid)
    );
    assert_ne!(interpretation.uuid, old_interpretation().uuid);
    assert_eq!(interpretation.json["Domain"], json!("depth"));
    // The lips are contacts, a patch with two open lines — and, since
    // 02/10 (a reported need: resqml objects without an interpretation are wrong), they
    // represent the torn horizon's new interpretation, as the surface does.
    assert_eq!(lips.json["LineRole"], json!("contact"));
    assert_eq!(lips.json["LinePatch"].as_array().map(Vec::len), Some(1));
    assert_eq!(lips.json["RepresentedObject"]["Uuid"], json!(interpretation.uuid));
    // One EdgePatch per lip.
    assert_eq!(split_edges(&plan).len(), 2);
    // Everything in the frame named, and marked invented.
    assert_eq!(
        surface.json["TrianglePatch"][0]["Geometry"]["LocalCrs"]["Uuid"],
        json!(frame().uuid)
    );
    for object in [surface, interpretation, lips] {
        assert_eq!(
            object.json["ExtensionNameValue"][0]["Name"],
            json!("synthetic"),
            "{}",
            object.title
        );
        let said = object.json["Citation"]["Description"]
            .as_str()
            .unwrap_or_default();
        assert!(
            said.contains("4 s (1 − s)") && said.contains("the test's own throw"),
            "{}: {said}",
            object.title
        );
    }
    assert_eq!(plan.activity.recipe, "fault-throw");
    assert_eq!(number(&plan, "faultCount"), 1.0);
}

/// **Two readings of one fact**: which edges are split. The
/// topology says it in its way — an edge bordered by a single triangle, which
/// is not on the border of the lattice, can only be a lip — and
/// `SplitEdgePatch` declares it. The two are compared without one being
/// computed from the other: the first is read in the array of the
/// triangles written, the second in the arrays of the edges.
#[test]
fn the_split_edges_are_the_edges_the_triangles_leave_open() {
    let plan = planned(&bag(), &choices()).expect("planned");
    let (points, triangles) = written_surface(&plan);
    let mut sides: HashMap<(i64, i64), usize> = HashMap::new();
    for t in &triangles {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            *sides.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    // The lattice's border: both ends on the outer rows or columns.
    let on_border = |v: i64| {
        let p = points[v as usize];
        let (i, j) = ((p[0] - X0) / 10.0, (p[1] - Y0) / 10.0);
        i.abs() < 1e-9 || (i - 20.0).abs() < 1e-9 || j.abs() < 1e-9 || (j - 20.0).abs() < 1e-9
    };
    let open: BTreeSet<(i64, i64)> = sides
        .iter()
        .filter(|(edge, count)| **count == 1 && !(on_border(edge.0) && on_border(edge.1)))
        .map(|(edge, _)| *edge)
        .collect();
    let declared: BTreeSet<(i64, i64)> = split_edges(&plan)
        .iter()
        .flatten()
        .map(|[a, b]| (*a.min(b), *a.max(b)))
        .collect();
    assert!(!open.is_empty());
    assert_eq!(
        open, declared,
        "the triangles leave open exactly the edges declared split"
    );
    assert_eq!(number(&plan, "splitEdgeCount"), declared.len() as f64);
}

/// The lips are the surface's nodes, position for position — the footwall's
/// line on the split edges of the first `EdgePatch`, the hanging wall's on the
/// second — and the drop between them is the parabola, computed here from the
/// footwall's `y` alone.
#[test]
fn the_lips_are_the_split_nodes_and_drop_by_the_profile() {
    let plan = planned(&bag(), &choices()).expect("planned");
    let (points, _) = written_surface(&plan);
    let lips = one(&plan, "resqml22.PolylineSetRepresentation");
    let counts = lips.json["LinePatch"][0]["NodeCountPerPolyline"]["Values"].clone();
    let line_points = floats(
        &plan,
        &path_in(&lips.json["LinePatch"][0]["Geometry"]["Points"]["Coordinates"]),
    );
    let edges = split_edges(&plan);
    let chain = |pairs: &Vec<[i64; 2]>| -> Vec<[f64; 3]> {
        std::iter::once(pairs[0][0])
            .chain(pairs.iter().map(|p| p[1]))
            .map(|v| points[v as usize])
            .collect()
    };
    let (footwall, hanging_wall) = (chain(&edges[0]), chain(&edges[1]));
    assert_eq!(counts, json!([footwall.len(), hanging_wall.len()]));
    assert_eq!(
        line_points,
        footwall.iter().chain(&hanging_wall).copied().collect::<Vec<_>>()
    );
    for (f, h) in footwall.iter().zip(&hanging_wall) {
        assert!(
            (f[0] - (X0 + 105.0)).abs() < 1e-6,
            "the footwall's lip is the trace: {f:?}"
        );
        let s = (f[1] - (Y0 + 45.0)) / 110.0;
        assert!(
            ((f[2] - h[2]) - 10.0 * 4.0 * s * (1.0 - s)).abs() < 1e-9,
            "at s = {s}: {} dropped",
            f[2] - h[2]
        );
    }
}

#[test]
fn one_entry_per_fault_and_a_vertical_sense() {
    let refused = planned(&bag(), &choices().set("maximumThrow", json!([10.0, 4.0])))
        .expect_err("two throws for one fault");
    assert!(
        refused.contains("maximumThrow") && refused.contains("2 throw(s) for 1 fault(s)"),
        "{refused}"
    );
    let refused =
        planned(&bag(), &choices().set("throw", json!(["strike-slip"]))).expect_err("no vertical sense");
    assert!(
        refused.contains("strike-slip") && refused.contains("no vertical sense"),
        "{refused}"
    );
    let refused = planned(&bag(), &base().set("synthetic", true)).expect_err("synthetic without source");
    assert!(refused.contains("no constant without a source"), "{refused}");
    // A reverse fault is a sense: accepted.
    planned(&bag(), &choices().set("throw", json!(["reverse"])))
        .expect("a reverse fault raises its hanging wall");
}

#[test]
fn what_the_computation_cannot_tear_is_refused_on_the_faults() {
    let refused = planned(&bag_with(fault(90.0), frame()), &choices()).expect_err("vertical");
    assert!(
        refused.contains("vertical") && refused.contains("no hanging wall"),
        "{refused}"
    );
    let other = Ref::new(
        "cccccccc-0000-4000-8000-000000000099",
        "eml23.LocalEngineeringCompoundCrs",
        "another frame",
    );
    let refused = planned(&bag_with(fault(60.0), other), &choices()).expect_err("two frames");
    assert!(
        refused.contains("two frames") || refused.contains("two origins"),
        "{refused}"
    );
}

#[test]
fn a_triangulated_horizon_is_not_torn_again() {
    let triangulated = Source::Shape {
        geometry: Geometry {
            kind: "triangles",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points: vec![[X0, Y0, -50.0], [X0 + 10.0, Y0, -50.0], [X0, Y0 + 10.0, -50.0]],
                topology: Topology::Triangles(vec![[0, 1, 2]]),
                closed: Vec::new(),
            }],
        },
        closed: None,
    };
    let pieces = Pieces::empty()
        .with("horizon", triangulated)
        .with("faults", fault(60.0))
        .with(
            "feature",
            Source::Object(Known {
                reference: Some(feature()),
                ..Default::default()
            }),
        );
    let fitness = Registry::new()
        .get("fault-throw")
        .expect("registered")
        .fitness(&pieces);
    match fitness {
        Fitness::Unfit(why) => assert!(format!("{why:?}").contains("second skin"), "{why:?}"),
        other => panic!("a triangulated horizon is fit: {other:?}"),
    }
}

/// Correcting a throw rewrites the same objects; renaming makes others. And
/// the lips never take a uuid the surface took.
#[test]
fn the_identity_is_the_horizon_and_the_title() {
    let first = planned(&bag(), &choices()).expect("planned");
    let again = planned(&bag(), &choices().set("maximumThrow", json!([12.0]))).expect("planned");
    let uuids = |plan: &Plan| plan.objects.iter().map(|o| o.uuid.clone()).collect::<Vec<_>>();
    assert_eq!(uuids(&first), uuids(&again));
    let renamed = planned(&bag(), &choices().set("title", "H6 — another")).expect("planned");
    assert!(uuids(&first).iter().all(|u| !uuids(&renamed).contains(u)));
    let distinct: BTreeSet<String> = uuids(&first).into_iter().collect();
    assert_eq!(distinct.len(), first.objects.len());
    // Under given uuids, in the order the objects are written.
    let given: Vec<String> = (1..=3)
        .map(|k| format!("00000000-0000-4000-8000-00000000000{k}"))
        .collect();
    let plan = Registry::new()
        .plan(
            "fault-throw",
            &bag(),
            &choices(),
            &Identity::Given(given.clone()),
            &when(),
        )
        .map_err(|why| why.message())
        .expect("planned");
    assert_eq!(uuids(&plan), given);
}
