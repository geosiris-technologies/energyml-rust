//! A plan is asked for its geometry or for its writing.
//!
//! # What this file holds
//!
//! The feedback: the understanding was that an
//! operation created a geometry (an artefact of a front end) and that it is this
//! object that would in a second step allow creating a RESQML object […] for
//! the web it must be well split. The two halves of a plan existed
//! ; what was missing was being able to ask for only one. Keeping
//! an artefact required a frame, a title, a data file - everything that
//! only the writing asks for.
//!
//! # What would have to break to make it turn red
//!
//! Requiring the title or the frame on a geometry plan; letting a missing
//! tolerance through on the pretext of geometry; returning a plan without a frame
//! *not blocked*, which `apply` would then write with a CRS that does not exist.

use energyml_build::operation::{Choices, Identity, Purpose, Registry, When};
use energyml_build::pieces::{Pieces, Source};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When {
        creation: "2026-09-12T00:00:00Z".to_string(),
        originator: "tests".to_string(),
    }
}

fn ring() -> Pieces {
    Pieces::empty().with(
        "shape",
        Source::Shape {
            geometry: Geometry {
                kind: "ring",
                patches: vec![Patch {
                    index: 0,
                    node_offset: 0,
                    points: vec![
                        [0.0, 0.0, 0.0],
                        [10.0, 0.0, 0.0],
                        [10.0, 10.0, 0.0],
                        [0.0, 10.0, 0.0],
                    ],
                    topology: Topology::Polylines(vec![4]),
                    closed: Vec::new(),
                }],
            },
            closed: Some(vec![true]),
        },
    )
}

/// **Nothing but what the computation asks for.** No frame, no title, no
/// file: the geometry comes out, and the writing is said to be blocked.
#[test]
fn a_geometry_plan_asks_for_neither_frame_nor_title() {
    let registry = Registry::new();
    let plan = registry
        .plan_for(
            "polylines",
            &ring(),
            &Choices::new(),
            &Identity::derived(),
            &when(),
            Purpose::Geometry,
        )
        .expect("the geometry comes out with nothing");
    assert_eq!(plan.shapes.len(), 1, "one shape to keep");
    assert!(plan.shapes[0].geometry.point_count() == 4);
    // The title suggested from the piece: it is what the scene will display.
    assert_eq!(plan.shapes[0].title, "shape");
    // And the writing is **blocked, not absent**: the objects are there, `apply`
    // will refuse them naming the frame.
    let why = plan.blocked.as_ref().expect("blocked without a frame");
    assert!(why.message().contains("frame"), "{}", why.message());
    assert!(!plan.objects.is_empty());
}

/// Without a frame, the **writing** plan also comes back blocked, not refused: it is
/// the complement that the earlier rule left as an `Err`, and `apply` is the only one to
/// refuse.
#[test]
fn without_a_frame_the_writing_also_comes_back_blocked() {
    let registry = Registry::new();
    let plan = registry
        .plan(
            "polylines",
            &ring(),
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .expect("the title and the file are suggested, the frame blocks");
    assert!(plan.blocked.is_some());
}

/// **The relaxation, where it lives.** On a real bag almost everything is
/// suggested, so a geometry plan would pass even if the purpose were ignored -
/// that is what the first sabotage showed. Here a required title **without**
/// suggestion: refused for writing, left empty for the geometry; and a
/// required computation setting, refused in both cases.
#[test]
fn the_relaxation_only_touches_what_only_the_writing_asks_for() {
    use energyml_build::operation::{Need, Slot};
    let title = Slot::required("title", Need::Title, "the title").grouped("naming");
    let spacing = Slot::required(
        "spacing",
        Need::Number {
            uom: None,
            least: None,
            most: None,
        },
        "the cell",
    );
    let slots = [title, spacing];
    let empty = Choices::new();

    let why = empty
        .settle_for(&slots, Purpose::Writing)
        .expect_err("writing demands both");
    assert!(
        why.message().contains("`title`") && why.message().contains("`spacing`"),
        "{}",
        why.message()
    );

    let why = empty
        .settle_for(&slots, Purpose::Geometry)
        .expect_err("the computation still demands `spacing`");
    assert!(
        why.message().contains("`spacing`") && !why.message().contains("`title`"),
        "{}",
        why.message()
    );

    let settled = Choices::new()
        .set("spacing", 2.0)
        .settle_for(&slots, Purpose::Geometry)
        .expect("the geometry can be set");
    assert!(
        settled.text("title").is_none(),
        "the title stays empty, it is not invented here"
    );
}

/// **What the computation asks for stays required**, geometry or not: a
/// tolerance that nobody gave is not a geometry that somebody
/// asked for. `interpolate` demands `spacing`, in the group `operation`.
#[test]
fn what_the_computation_asks_for_stays_required_on_a_geometry_plan() {
    let registry = Registry::new();
    let cloud = Pieces::empty().with(
        "shape",
        Source::Shape {
            geometry: Geometry {
                kind: "cloud",
                patches: vec![Patch {
                    index: 0,
                    node_offset: 0,
                    points: vec![
                        [0.0, 0.0, 1.0],
                        [10.0, 0.0, 2.0],
                        [0.0, 10.0, 3.0],
                        [10.0, 10.0, 4.0],
                    ],
                    topology: Topology::Points,
                    closed: Vec::new(),
                }],
            },
            closed: None,
        },
    );
    let why = registry
        .plan_for(
            "interpolate",
            &cloud,
            &Choices::new(),
            &Identity::derived(),
            &when(),
            Purpose::Geometry,
        )
        .expect_err("`spacing` manque");
    assert!(why.message().contains("spacing"), "{}", why.message());
    // And with it, the geometry comes out - still without a frame.
    let plan = registry
        .plan_for(
            "interpolate",
            &cloud,
            &Choices::new().set("spacing", 5.0),
            &Identity::derived(),
            &when(),
            Purpose::Geometry,
        )
        .expect("the grid comes out");
    assert!(!plan.shapes.is_empty());
    assert!(plan.blocked.is_some());
}

/// A geometry plan is not an empty plan: `crs`, which computes nothing,
/// says so as before.
#[test]
fn an_operation_without_geometry_stays_so_whatever_the_aim() {
    let registry = Registry::new();
    let plan = registry
        .plan_for(
            "crs",
            &Pieces::empty(),
            &Choices::new().set("epsg", 2154),
            &Identity::derived(),
            &when(),
            Purpose::Geometry,
        )
        .expect("crs se planifie");
    assert!(plan.shapes.is_empty());
    assert!(plan.blocked.is_none());
}
