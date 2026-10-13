//! `loft` — a surface between polylines, in their order
//!. The stitch is tested in
//! `energyml_analysis::loft`; this holds the seam.
//!
//! # What would have to break to make it turn red
//!
//! Taking the lines in another order than that of the slot; pairing the
//! polylines of one piece; silently flipping a line of opposite sense;
//! writing something other than a triangulated surface.

use energyml_build::operation::{Choices, Identity, Plan, Purpose, Registry, When};
use energyml_build::pieces::{Pieces, Source};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When { creation: "2026-09-14T00:00:00Z".into(), originator: "test".into() }
}

fn stick(points: Vec<[f64; 3]>) -> Source {
    let runs = vec![points.len() as u32];
    Source::Shape { geometry: Geometry { kind: "test", patches: vec![Patch { index: 0, node_offset: 0, points, topology: Topology::Polylines(runs), closed: Vec::new() }] }, closed: None }
}

fn planned(bag: &Pieces) -> Result<Plan, energyml_build::operation::Error> {
    Registry::new().plan_for("loft", bag, &Choices::new(), &Identity::derived(), &when(), Purpose::Geometry)
}

#[test]
fn a_surface_between_two_sticks_in_the_order_of_the_slot() {
    let bag = Pieces::empty()
        .with("lines", stick(vec![[0.0, 0.0, 0.0], [0.0, 0.0, -10.0]]))
        .with("lines 2", stick(vec![[5.0, 0.0, 0.0], [5.0, 0.0, -5.0], [5.0, 0.0, -10.0]]))
        .with("lines 3", stick(vec![[10.0, 0.0, 0.0], [10.0, 0.0, -10.0]]));
    let made = planned(&bag).unwrap_or_else(|why| panic!("planned: {why}"));
    let patch = &made.shapes[0].geometry.patches[0];
    assert_eq!(patch.points.len(), 7, "every vertex of every stick");
    let Topology::Triangles(triangles) = &patch.topology else { panic!("triangles") };
    assert_eq!(triangles.len(), 6, "two bands");
    // The middle stick's nodes are 2, 3, 4: every triangle touches it — the
    // order of the slot, 1 then 2 then 3, and never 1 with 3.
    assert!(triangles.iter().all(|t| t.iter().any(|v| (2..=4).contains(v))), "{triangles:?}");
    assert_eq!(made.objects.last().unwrap().qualified_type, "resqml22.TriangulatedSetRepresentation");
    assert_eq!(made.activity.recipe, "loft");
}

#[test]
fn two_sticks_of_opposite_sense_are_refused_naming_them() {
    let bag = Pieces::empty()
        .with("lines", stick(vec![[0.0, 0.0, 0.0], [0.0, 0.0, -10.0]]))
        .with("lines 2", stick(vec![[5.0, 0.0, -10.0], [5.0, 0.0, 0.0]]));
    let why = planned(&bag).expect_err("twisted");
    assert!(why.message().contains("opposite ways") && why.message().contains("reverse"), "{why}");
}

#[test]
fn a_piece_of_several_polylines_is_refused() {
    let two = Source::Shape {
        geometry: Geometry {
            kind: "test",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points: vec![[0.0; 3], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0], [1.0, 0.0, -1.0]],
                topology: Topology::Polylines(vec![2, 2]),
                closed: Vec::new(),
            }],
        },
        closed: None,
    };
    let bag = Pieces::empty().with("lines", two).with("lines 2", stick(vec![[5.0, 0.0, 0.0], [5.0, 0.0, -1.0]]));
    let why = planned(&bag).expect_err("several in one piece");
    assert!(why.message().contains("several polylines"), "{why}");
}
