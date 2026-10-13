//! `thickness` — the isochore and the isopach between two horizons, as a
//! property on the top.
//!
//! # What would have to break to make it turn red
//!
//! Reading the roles by altitude rather than by name (the reversed bag would return the
//! same thickness, positive); laying the values on the base; writing a
//! difference of two frames or of two units; writing in metres a
//! difference of time; forgetting the kind of the dictionary.

use serde_json::json;

use energyml_build::operation::{Choices, Fitness, Identity, Numbers, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When {
        creation: "2026-09-12T00:00:00Z".into(),
        originator: "test".into(),
    }
}

fn crs(uuid: &str, title: &str) -> Ref {
    Ref::new(uuid, "eml23.LocalEngineeringCompoundCrs", title)
}

/// A square of side 100, split in two triangles, at `z(x, y)`.
fn plate(z: impl Fn(f64, f64) -> f64) -> Geometry {
    let corners = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]];
    let points = corners.iter().map(|&[x, y]| [x, y, z(x, y)]).collect();
    Geometry {
        kind: "surface",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points,
            topology: Topology::Triangles(vec![[0, 1, 2], [0, 2, 3]]),
            closed: Vec::new(),
        }],
    }
}

/// A regular 5 × 5 top over the plate, edge to edge, on `z(x, y)`,
/// triangulated — so that either can be the other's base.
fn top_grid(z: impl Fn(f64, f64) -> f64) -> Geometry {
    let mut points = Vec::new();
    for j in 0..5 {
        for i in 0..5 {
            let (x, y) = (25.0 * i as f64, 25.0 * j as f64);
            points.push([x, y, z(x, y)]);
        }
    }
    let mut faces = Vec::new();
    for j in 0..4u32 {
        for i in 0..4u32 {
            let k = j * 5 + i;
            faces.push([k, k + 1, k + 6]);
            faces.push([k, k + 6, k + 5]);
        }
    }
    Geometry {
        kind: "surface",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points,
            topology: Topology::Triangles(faces),
            closed: Vec::new(),
        }],
    }
}

fn known(uuid: &str, title: &str, frame: &Ref, uom: &str, nodes: usize) -> Source {
    Source::Object(Known {
        reference: Some(Ref::new(uuid, "resqml22.TriangulatedSetRepresentation", title)),
        crs: Some(frame.clone()),
        vertical_uom: Some(uom.into()),
        vertical_up: Some(false),
        node_count: Some(nodes),
        ..Default::default()
    })
}

const TOP: &str = "11111111-0000-4000-8000-000000000001";
const BASE: &str = "22222222-0000-4000-8000-000000000002";

/// Two horizons of the package, in depth, the base 20 below the top, both
/// dipping at `slope` along x.
fn pair(slope: f64, uom: &str) -> Pieces {
    let frame = crs("cccccccc-0000-4000-8000-000000000003", "The frame");
    Pieces::empty()
        .with(
            "top",
            Source::Shape {
                geometry: top_grid(|x, _| 1000.0 + slope * x),
                closed: None,
            },
        )
        .with("top (object)", known(TOP, "Toit", &frame, uom, 25))
        .with(
            "base",
            Source::Shape {
                geometry: plate(|x, _| 1020.0 + slope * x),
                closed: None,
            },
        )
        .with("base (object)", known(BASE, "Mur", &frame, uom, 4))
}

fn values_of(plan: &Plan) -> Vec<f64> {
    match &plan.datasets[0].values {
        Numbers::F64(values) => values.clone(),
        other => panic!("not floating point: {other:?}"),
    }
}

fn choices(what: &str) -> Choices {
    Choices::new().set("what", what)
}

/// **The isochore of two parallel horizons in depth**: 20 at every node of
/// the top, on the top, under the dictionary's `true vertical thickness`, in
/// the frame's unit — and the vertical convention suggested from the frame.
#[test]
fn the_isochore_is_the_vertical_gap_laid_on_the_top() {
    let plan = Registry::new()
        .plan(
            "thickness",
            &pair(0.1, "m"),
            &choices("isochore"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let values = values_of(&plan);
    assert_eq!(
        values.len(),
        25,
        "one value per node of the top — not of the base"
    );
    assert!(values.iter().all(|v| (v - 20.0).abs() < 1e-9), "{values:?}");
    let object = &plan.objects[0].json;
    assert_eq!(object["SupportingRepresentation"]["Uuid"], json!(TOP));
    assert_eq!(object["PropertyKind"]["Title"], json!("true vertical thickness"));
    assert_eq!(
        object["PropertyKind"]["Uuid"],
        json!("193d165b-bfec-4d11-b093-21e0bdc77433")
    );
    assert_eq!(object["Uom"], json!("m"));
    assert_eq!(plan.objects[0].title, "Toit — Mur isochore");
}

/// **The roles are the names.** The same two surfaces with the names swapped
/// put the base above the top: the thickness is negative at every node, and
/// counted as crossed rather than blanked or silently flipped.
#[test]
fn roles_are_read_by_name_and_a_reversed_bag_still_crosses() {
    let right = pair(0.0, "m");
    let mut swapped = Pieces::empty();
    for (name, source) in right.iter() {
        let renamed = match name {
            "top" => "base".to_string(),
            "base" => "top".to_string(),
            "top (object)" => "base (object)".to_string(),
            "base (object)" => "top (object)".to_string(),
            other => other.to_string(),
        };
        swapped = swapped.with(renamed, source.clone());
    }
    let plan = Registry::new()
        .plan(
            "thickness",
            &swapped,
            &choices("isochore"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let values = values_of(&plan);
    assert_eq!(values.len(), 4, "on the plate, now the top");
    assert!(values.iter().all(|v| (v + 20.0).abs() < 1e-9), "{values:?}");
    assert!(plan.says.iter().any(|n| n.key == "crossed"), "{:?}", plan.says);

    // Without a `base`, unfit — and the reason names the role.
    let alone = Pieces::empty().with(
        "top",
        Source::Shape {
            geometry: top_grid(|_, _| 0.0),
            closed: None,
        },
    );
    match Registry::new().get("thickness").unwrap().fitness(&alone) {
        Fitness::Unfit(why) => assert!(why.message.contains("`base`"), "{why}"),
        Fitness::Fit => panic!("a thickness needs two surfaces"),
    }
}

/// **The isopach** is the isochore times the cosine of the dip, under the
/// dictionary's `true stratigraphic thickness` — and the plan says it used
/// the parallel-beds formula.
#[test]
fn the_isopach_is_the_isochore_times_the_cosine_of_the_dip_and_says_so() {
    let slope: f64 = 0.25;
    let plan = Registry::new()
        .plan(
            "thickness",
            &pair(slope, "m"),
            &choices("isopach"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let expected = 20.0 * slope.atan().cos();
    assert!(values_of(&plan).iter().all(|v| (v - expected).abs() < 1e-9));
    assert_eq!(
        plan.objects[0].json["PropertyKind"]["Title"],
        json!("true stratigraphic thickness")
    );
    assert!(plan.says.iter().any(|n| n.key == "parallel-beds"));
}

/// **In time, the isochore is an isochron and the isopach is refused.**
#[test]
fn in_time_the_isochore_is_an_isochron_and_the_isopach_is_refused() {
    let pieces = pair(0.0, "ms");
    let plan = Registry::new()
        .plan(
            "thickness",
            &pieces,
            &choices("isochore"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert_eq!(
        plan.objects[0].json["PropertyKind"]["Title"],
        json!("vertical time thickness")
    );
    assert_eq!(plan.objects[0].json["Uom"], json!("ms"));
    assert!(plan.says.iter().any(|n| n.key == "isochron"));

    let why = Registry::new()
        .plan(
            "thickness",
            &pieces,
            &choices("isopach"),
            &Identity::derived(),
            &when(),
        )
        .unwrap_err();
    assert!(why.message().contains("not an angle"), "{}", why.message());

    // A length unit answered over a frame that counts in time is refused too.
    let why = Registry::new()
        .plan(
            "thickness",
            &pieces,
            &choices("isochore").set("uom", "m"),
            &Identity::derived(),
            &when(),
        )
        .unwrap_err();
    assert!(why.message().contains("wrong family"), "{}", why.message());
}

/// **Two frames, or two units: no difference.**
#[test]
fn two_frames_or_two_units_are_not_subtracted() {
    let other = crs("dddddddd-0000-4000-8000-000000000004", "The other frame");
    let mut two_frames = Pieces::empty();
    let mut two_units = Pieces::empty();
    for (name, source) in pair(0.0, "m").iter() {
        let (a, b) = if name == "base (object)" {
            let frame = crs("cccccccc-0000-4000-8000-000000000003", "The frame");
            (
                known(BASE, "Mur", &other, "m", 4),
                known(BASE, "Mur", &frame, "ft", 4),
            )
        } else {
            (source.clone(), source.clone())
        };
        two_frames = two_frames.with(name, a);
        two_units = two_units.with(name, b);
    }
    let why = Registry::new()
        .plan(
            "thickness",
            &two_frames,
            &choices("isochore"),
            &Identity::derived(),
            &when(),
        )
        .unwrap_err();
    assert!(
        why.message().contains("two origins") && why.message().contains("The other frame"),
        "{}",
        why.message()
    );
    let why = Registry::new()
        .plan(
            "thickness",
            &two_units,
            &choices("isochore"),
            &Identity::derived(),
            &when(),
        )
        .unwrap_err();
    assert!(why.message().contains("`ft`"), "{}", why.message());
}

/// **A base given as a grid is read bilinearly**, and a top past its edge is
/// counted, not extrapolated.
#[test]
fn a_grid_base_is_read_and_an_overflowing_top_is_counted() {
    // A 2 × 2-cell grid over [0, 100]², z = 1000 + x·y / 100.
    let mut points = Vec::new();
    for j in 0..3 {
        for i in 0..3 {
            let (x, y) = (50.0 * i as f64, 50.0 * j as f64);
            points.push([x, y, 1000.0 + x * y / 100.0]);
        }
    }
    let quads = vec![[0, 1, 4, 3], [1, 2, 5, 4], [3, 4, 7, 6], [4, 5, 8, 7]];
    let grid = Geometry {
        kind: "grid",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points,
            topology: Topology::Quads(quads),
            closed: Vec::new(),
        }],
    };
    let top = Geometry {
        kind: "points",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points: vec![[25.0, 25.0, 900.0], [150.0, 25.0, 900.0]],
            topology: Topology::Points,
            closed: Vec::new(),
        }],
    };
    let pieces = Pieces::empty()
        .with(
            "top",
            Source::Shape {
                geometry: top,
                closed: None,
            },
        )
        .with(
            "base",
            Source::Shape {
                geometry: grid,
                closed: None,
            },
        );
    let answers = choices("isochore").set("vertical", "depth").set("uom", "m").set(
        "representation",
        Ref::new(TOP, "resqml22.PointSetRepresentation", "Toit").dor(),
    );
    let plan = Registry::new()
        .plan("thickness", &pieces, &answers, &Identity::derived(), &when())
        .unwrap();
    let values = values_of(&plan);
    // Bilinear at the centre of the first cell: 1000 + 25·25/100 = 1006.25.
    assert!((values[0] - 106.25).abs() < 1e-9, "{values:?}");
    assert!(values[1].is_nan());
    assert!(plan.says.iter().any(|n| n.key == "outside"));
}

/// **A grid read as a lattice has faces too** — its cells, bilinear. Both
/// `thickness` and `project` called a `Grid2dRepresentation` read as
/// `Topology::Grid` *a surface without a face*, while the scene drew it; found
/// on 14/09 by confronting the front end's projected picking with `project`.
#[test]
fn a_lattice_base_is_read_like_its_cells() {
    let mut points = Vec::new();
    for j in 0..3 {
        for i in 0..3 {
            let (x, y) = (50.0 * i as f64, 50.0 * j as f64);
            points.push([x, y, 1000.0 + x * y / 100.0]);
        }
    }
    let lattice = Geometry {
        kind: "grid",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points,
            topology: Topology::Grid {
                fastest: 3,
                slowest: 3,
            },
            closed: Vec::new(),
        }],
    };
    let top = Geometry {
        kind: "points",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points: vec![[25.0, 25.0, 900.0]],
            topology: Topology::Points,
            closed: Vec::new(),
        }],
    };
    let pieces = Pieces::empty()
        .with(
            "top",
            Source::Shape {
                geometry: top,
                closed: None,
            },
        )
        .with(
            "base",
            Source::Shape {
                geometry: lattice,
                closed: None,
            },
        );
    let answers = choices("isochore").set("vertical", "depth").set("uom", "m").set(
        "representation",
        Ref::new(TOP, "resqml22.PointSetRepresentation", "Toit").dor(),
    );
    let plan = Registry::new()
        .plan("thickness", &pieces, &answers, &Identity::derived(), &when())
        .unwrap_or_else(|why| panic!("{why}"));
    // The same cell as with listed quadrilaterals: 1000 + 25·25/100.
    assert!(
        (values_of(&plan)[0] - 106.25).abs() < 1e-9,
        "{:?}",
        values_of(&plan)
    );
}
