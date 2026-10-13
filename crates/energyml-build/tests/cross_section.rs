//! `cross-section` — a filled section, and the first operation with two roles.
//!
//! # What this file holds
//!
//! Until now all the
//! operations took **one** input, and what they declared of their
//! cardinality and their order was exercised by nobody: `Slot::least`,
//! `Slot::most` and `Slot::in_order` have existed since 11/09 and none was
//! used. A declaration that nothing uses is a declaration of which
//! nobody knows whether it works.
//!
//! # What would have to break to make it turn red
//!
//! Reading the roles by their topology rather than by their slot name — a
//! trace and a surface cut are two polylines, and nothing distinguishes them
//! except the name. Taking the surfaces in another order than that of the bag —
//! an inverted section looks abnormal to nobody. Keeping quiet about an empty panel,
//! or about a crossing, which are the two pieces of news a section brings.

use energyml_build::operation::{Choices, Fitness, Identity, AnyOperation, Registry, When};
use energyml_build::pieces::{Pieces, Source};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When { creation: "2026-09-12T00:00:00Z".to_string(), originator: "tests".to_string() }
}

/// A straight trace from west to east, at y = 5.
fn trace() -> Source {
    Source::Shape {
        geometry: Geometry {
            kind: "trace",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points: vec![[0.0, 5.0, 0.0], [10.0, 5.0, 0.0]],
                topology: Topology::Polylines(vec![2]),
                closed: Vec::new(),
            }],
        },
        closed: None,
    }
}

/// A horizontal square at altitude `z`, two triangles.
fn plate(z: f64) -> Source {
    Source::Shape {
        geometry: Geometry {
            kind: "plate",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points: vec![[0.0, 0.0, z], [10.0, 0.0, z], [10.0, 10.0, z], [0.0, 10.0, z]],
                topology: Topology::Triangles(vec![[0, 1, 2], [0, 2, 3]]),
                closed: Vec::new(),
            }],
        },
        closed: None,
    }
}

/// An inclined plane that goes down towards the east, from `high` to `low`.
fn ramp(high: f64, low: f64) -> Source {
    Source::Shape {
        geometry: Geometry {
            kind: "ramp",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points: vec![[0.0, 0.0, high], [10.0, 0.0, low], [10.0, 10.0, low], [0.0, 10.0, high]],
                topology: Topology::Triangles(vec![[0, 1, 2], [0, 2, 3]]),
                closed: Vec::new(),
            }],
        },
        closed: None,
    }
}

/// The writing settings: a frame created on the spot, and a title.
fn choices() -> Choices {
    Choices::new()
        .set("sourceEpsg", 2154)
        .set("project", 2154)
        .set("title", "Coupe A")
        .set("dataFile", "data.h5")
}

fn operation() -> &'static dyn AnyOperation {
    // Deliberate leak: the registry lives for the length of the test, and making it
    // static avoids passing a reference at each call.
    Box::leak(Box::new(Registry::new())).get("cross-section").expect("`cross-section` is in the registry")
}

/// The ordinary bag: a trace, two plates, the high one first.
fn bag() -> Pieces {
    Pieces::empty()
        .with("trace", trace())
        .with("surfaces", plate(100.0))
        .with("surfaces 2", plate(50.0))
}

// -- the two roles -----------------------------------------------------------

/// **The role comes from the name of the slot, never from the topology.**
///
/// This is what the declaration of what is taken made possible by making the name of a piece the name
/// of the slot that received it. Without it, a trace and a surface cut are
/// two polylines that nothing distinguishes, and the operation would have to guess.
#[test]
fn without_a_named_trace_the_operation_refuses_saying_so() {
    let no_trace = Pieces::empty().with("surfaces", plate(100.0)).with("surfaces 2", plate(50.0));
    match operation().fitness(&no_trace) {
        Fitness::Unfit(why) => assert!(why.message.contains("trace"), "{why}"),
        Fitness::Fit => panic!("a bag without a trace should not be fit"),
    }

    // And a slot holds only one role: a polyline filed under `surfaces`
    // is not a surface, so it does not count.
    let misfiled = Pieces::empty().with("surfaces", trace()).with("surfaces 2", plate(50.0));
    assert!(matches!(operation().fitness(&misfiled), Fitness::Unfit(_)));
}

/// **The decoy**, and it is what really tells the two readings apart.
///
/// A bag where the trace is named and where a **second polyline** is filed
/// elsewhere. Read by name, the section follows the trace and returns material; read
/// by topology, it is the last polyline found that wins — this one
/// passes a hundred metres from the plates, and the section returns nothing.
///
/// This is the useful form of the check: the two readings **agree** on an
/// ordinary bag, and the first attempt of this file therefore did not tell them apart.
#[test]
fn one_more_polyline_does_not_become_the_trace() {
    let elsewhere = Source::Shape {
        geometry: Geometry {
            kind: "decoy",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points: vec![[0.0, 500.0, 0.0], [10.0, 500.0, 0.0]],
                topology: Topology::Polylines(vec![2]),
                closed: Vec::new(),
            }],
        },
        closed: None,
    };
    let pieces = bag().with("decoy", elsewhere);
    let plan = operation()
        .plan(&pieces, &choices(), &Identity::derived(), &when())
        .expect("the section follows the named trace, not the decoy");
    assert_eq!(plan.shapes.iter().filter(|made| made.parent.is_some()).count(), 1);
}

/// **Cardinality, at last exercised on a real operation.**
#[test]
fn a_single_surface_is_refused_with_the_count() {
    let one_surface = Pieces::empty().with("trace", trace()).with("surfaces", plate(100.0));
    match operation().fitness(&one_surface) {
        Fitness::Unfit(energyml_build::operation::Refusal { message: why, .. }) => {
            assert!(why.contains("two"), "{why}");
            assert!(why.contains("1 given"), "{why}");
        }
        Fitness::Fit => panic!("a filled section asks for two surfaces"),
    }
    assert_eq!(operation().fitness(&bag()), Fitness::Fit);
}

/// The slot is declared multiple **and ordered**.
#[test]
fn surfaces_are_declared_multiple_and_ordered() {
    let takes = operation().takes();
    let trace = takes.iter().find(|slot| slot.name == "trace").expect("`trace`");
    let surfaces = takes.iter().find(|slot| slot.name == "surfaces").expect("`surfaces`");

    assert_eq!(trace.least, 1);
    assert_eq!(trace.most, Some(1));
    // A single trace has no order: declaring it would put in the template a
    // flag that a form would then have to decide to ignore.
    assert!(!trace.ordered);

    assert_eq!(surfaces.least, 2);
    assert_eq!(surfaces.most, None);
    assert!(surfaces.takes_many());
    assert!(surfaces.ordered, "the order of a section is data");
}

// -- what the plan computes --------------------------------------------------

/// **One panel per consecutive pair**, and the tree carries them.
#[test]
fn three_surfaces_make_two_panels_under_a_root() {
    let pieces = bag().with("surfaces 3", plate(10.0));
    let plan = operation()
        .plan(&pieces, &choices(), &Identity::derived(), &when())
        .expect("the plan succeeds");

    let roots: Vec<_> = plan.shapes.iter().filter(|made| made.parent.is_none()).collect();
    assert_eq!(roots.len(), 1, "one root, which is the whole result");
    let panels: Vec<_> = plan.shapes.iter().filter(|made| made.parent.is_some()).collect();
    assert_eq!(panels.len(), 2, "two consecutive pairs, two panels");

    // **The panels have no object of their own**, and that is said: they are the
    // patches of the representation that the root names. Inventing one for them
    // would make one believe they can be promoted separately.
    assert!(roots[0].object.is_some());
    assert!(panels.iter().all(|made| made.object.is_none()));

    // **And the root draws nothing.** Leaving it the merged geometry
    // would draw each panel twice — once through it, once through its
    // child — and two surfaces exactly superimposed fight pixel by
    // pixel with nothing saying why. It is a **group** root.
    assert!(roots[0].geometry.patches.is_empty(), "the root groups, it does not draw");
    assert!(panels.iter().all(|made| !made.geometry.patches.is_empty()));

    // The title of a panel names its two bounds: it is what one reads on the
    // line of the scene, and a “panel 2” would teach nothing.
    assert!(panels[0].title.contains("surfaces"), "{}", panels[0].title);

    // A single object written, and it carries the two panels as patches.
    let written = plan.objects.iter().filter(|o| o.qualified_type.contains("Triangulated")).count();
    assert_eq!(written, 1);
}

/// **The order of the bag is the order of the section**, and inverting it shows.
///
/// Two ramps that go down, the high one given first: nothing to report. The
/// same in the other direction: each piece is *inverted*, and the plan says so
/// rather than straightening it. Straightening would be deciding in someone's place —
/// and it is exactly the case that the interpretation workshop must pose.
#[test]
fn the_given_order_decides_top_and_bottom_and_an_inversion_is_said() {
    let right_order = Pieces::empty()
        .with("trace", trace())
        .with("surfaces", ramp(100.0, 80.0))
        .with("surfaces 2", ramp(50.0, 30.0));
    let plan = operation().plan(&right_order, &choices(), &Identity::derived(), &when()).expect("the plan succeeds");
    assert!(
        !plan.says.iter().any(|note| note.key == "inverted"),
        "nothing is inverted when the order is right"
    );

    let inverted_order = Pieces::empty()
        .with("trace", trace())
        .with("surfaces", ramp(50.0, 30.0))
        .with("surfaces 2", ramp(100.0, 80.0));
    let plan = operation().plan(&inverted_order, &choices(), &Identity::derived(), &when()).expect("the plan succeeds");
    let said = plan.says.iter().find(|note| note.key == "inverted").expect("the inversion is said");
    assert!(said.detail.contains("below"), "{}", said.detail);
}

/// **An empty panel is an answer, not a failure** — and it is counted.
///
/// Two surfaces that meet nowhere along the trace make no
/// material. Keeping quiet would make a section with holes a section that looks
/// complete; failing would lose the panels that do have material.
#[test]
fn a_panel_without_material_is_counted_and_the_others_stay() {
    // The third plate is off the trace: the pair 2→3 gives nothing, the
    // pair 1→2 does.
    let off_trace = Source::Shape {
        geometry: Geometry {
            kind: "plate",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points: vec![[100.0, 100.0, 0.0], [110.0, 100.0, 0.0], [110.0, 110.0, 0.0]],
                topology: Topology::Triangles(vec![[0, 1, 2]]),
                closed: Vec::new(),
            }],
        },
        closed: None,
    };
    let pieces = bag().with("surfaces 3", off_trace);
    let plan = operation().plan(&pieces, &choices(), &Identity::derived(), &when()).expect("the plan succeeds");

    assert!(plan.says.iter().any(|note| note.key == "panel-empty"), "the emptiness is said");
    assert_eq!(
        plan.shapes.iter().filter(|made| made.parent.is_some()).count(),
        1,
        "the panel that has material stays"
    );
}

/// No material anywhere: then it is a refusal — an empty plan would read
/// as a computation that succeeded and found nothing to say.
#[test]
fn no_panel_at_all_is_a_refusal_that_counts_the_pairs() {
    let far_plate = Source::Shape {
        geometry: Geometry {
            kind: "plate",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points: vec![[100.0, 100.0, 0.0], [110.0, 100.0, 0.0], [110.0, 110.0, 0.0]],
                topology: Topology::Triangles(vec![[0, 1, 2]]),
                closed: Vec::new(),
            }],
        },
        closed: None,
    };
    let pieces = Pieces::empty().with("trace", trace()).with("surfaces", far_plate.clone()).with("surfaces 2", far_plate);
    let why = operation()
        .plan(&pieces, &choices(), &Identity::derived(), &when())
        .expect_err("no material: refused");
    assert!(why.message().contains("no panel"), "{}", why.message());
}

/// What the operation **reports**, and which is nowhere else.
#[test]
fn the_count_of_panels_goes_into_the_activity() {
    let plan = operation().plan(&bag(), &choices(), &Identity::derived(), &when()).expect("the plan succeeds");
    let named = |title: &str| {
        plan.activity.parameters.iter().any(|p| {
            matches!(p, energyml_domain::provenance::Parameter::Double { title: t, .. } if t == title)
        })
    };
    assert!(named("panelCount"));
    assert!(named("emptyPanels"));
    assert!(named("invertedPatches"));
    assert_eq!(plan.activity.recipe, "cross-section");
}
