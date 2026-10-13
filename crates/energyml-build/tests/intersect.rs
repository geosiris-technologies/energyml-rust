//! `intersect` - the trace of a fault surface on each horizon.
//!
//! # What would have to break to make it turn red
//!
//! Reading the fault by topology rather than by name; melting the horizons into a
//! single patch; keeping quiet about a horizon that the fault does not cut; giving the
//! root the geometry of its children.

use energyml_build::operation::{AnyOperation, Choices, Fitness, Identity, Registry, When};
use energyml_build::pieces::{Pieces, Source};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When {
        creation: "2026-09-12T00:00:00Z".to_string(),
        originator: "tests".to_string(),
    }
}

fn operation() -> &'static dyn AnyOperation {
    Box::leak(Box::new(Registry::new()))
        .get("intersect")
        .expect("`intersect` is in the registry")
}

fn surface(kind: &'static str, points: Vec<[f64; 3]>, triangles: Vec<[u32; 3]>) -> Source {
    Source::Shape {
        geometry: Geometry {
            kind,
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points,
                topology: Topology::Triangles(triangles),
                closed: Vec::new(),
            }],
        },
        closed: None,
    }
}

fn plate(z: f64) -> Source {
    surface(
        "plate",
        vec![[0.0, 0.0, z], [10.0, 0.0, z], [10.0, 10.0, z], [0.0, 10.0, z]],
        vec![[0, 1, 2], [0, 2, 3]],
    )
}

/// A vertical fault at x = 4, from z = -5 to 15.
fn wall() -> Source {
    surface(
        "wall",
        vec![
            [4.0, -1.0, -5.0],
            [4.0, 11.0, -5.0],
            [4.0, 11.0, 15.0],
            [4.0, -1.0, 15.0],
        ],
        vec![[0, 1, 2], [0, 2, 3]],
    )
}

fn choices() -> Choices {
    Choices::new()
        .set("sourceEpsg", 2154)
        .set("project", 2154)
        .set("title", "Traces")
        .set("dataFile", "data.h5")
}

#[test]
fn the_two_roles_are_read_by_name_and_their_gaps_are_named() {
    let op = operation();
    let no_fault = Pieces::empty().with("surfaces", plate(3.0));
    match op.fitness(&no_fault) {
        Fitness::Unfit(why) => assert!(why.message.contains("fault"), "{why}"),
        Fitness::Fit => panic!("without a fault, unfit"),
    }
    let no_surface = Pieces::empty().with("fault", wall());
    match op.fitness(&no_surface) {
        Fitness::Unfit(why) => assert!(why.message.contains("surfaces"), "{why}"),
        Fitness::Fit => panic!("without a horizon, unfit"),
    }
    // The fault is **the** fault, not one more horizon: a bag where the wall
    // is filed under `surfaces` and a plate under `fault` cuts the plate by
    // the wall - and therefore returns something other than the reverse.
    let ok = Pieces::empty().with("fault", wall()).with("surfaces", plate(3.0));
    assert_eq!(op.fitness(&ok), Fitness::Fit);
    let takes = op.takes();
    let surfaces = takes.iter().find(|s| s.name == "surfaces").unwrap();
    assert!(
        surfaces.takes_many() && !surfaces.ordered,
        "several, without order"
    );
}

/// **One patch per horizon, one child per horizon, a root that groups.**
#[test]
fn three_horizons_make_three_patches_and_three_children() {
    let pieces = Pieces::empty()
        .with("fault", wall())
        .with("surfaces", plate(3.0))
        .with("surfaces 2", plate(7.0))
        .with("surfaces 3", plate(11.0));
    let plan = operation()
        .plan(&pieces, &choices(), &Identity::derived(), &when())
        .expect("the plan succeeds");
    let roots: Vec<_> = plan.shapes.iter().filter(|m| m.parent.is_none()).collect();
    let kids: Vec<_> = plan.shapes.iter().filter(|m| m.parent.is_some()).collect();
    assert_eq!(roots.len(), 1);
    assert_eq!(kids.len(), 3);
    assert!(
        roots[0].geometry.patches.is_empty(),
        "the root groups, it does not draw"
    );
    assert!(roots[0].object.is_some() && kids.iter().all(|k| k.object.is_none()));
    // Each child is at the height of its horizon.
    for (kid, z) in kids.iter().zip([3.0, 7.0, 11.0]) {
        let zs: Vec<f64> = kid.geometry.patches[0].points.iter().map(|p| p[2]).collect();
        assert!(zs.iter().all(|v| (v - z).abs() < 1e-9), "{}: {zs:?}", kid.title);
        assert!(kid.title.contains("surfaces"));
    }
    // A single object written.
    assert_eq!(
        plan.objects
            .iter()
            .filter(|o| o.qualified_type.contains("PolylineSet"))
            .count(),
        1
    );

    // **And one `LinePatch` per horizon in that object** - the assertion that
    // was missing. Without it, this test passed while `polylines` flattened
    // the five traces into a single patch, and the plan, the tree and the doc
    // said *one patch per horizon*.
    let object = plan
        .objects
        .iter()
        .find(|o| o.qualified_type.contains("PolylineSet"))
        .unwrap();
    let written = object.json["LinePatch"].as_array().expect("LinePatches");
    assert_eq!(written.len(), 3, "one patch per horizon, in the object written");
    for (patch, z) in written.iter().zip([3.0, 7.0, 11.0]) {
        let path =
            patch["Geometry"]["Points"]["Coordinates"]["ExternalDataArrayPart"][0]["PathInExternalFile"]
                .as_str()
                .expect("un chemin");
        let dataset = plan.datasets.iter().find(|d| d.path == path).expect("its array");
        let energyml_build::operation::Numbers::F64(values) = &dataset.values else {
            panic!("floats")
        };
        assert!(
            values.as_chunks::<3>().0.iter().all(|p| (p[2] - z).abs() < 1e-9),
            "the patch of z = {z} carries its own nodes"
        );
    }
}

/// A horizon that the fault does not cut is **said**, and the others stay;
/// no horizon cut at all is a refusal.
#[test]
fn a_horizon_outside_the_fault_is_counted_and_none_is_a_refusal() {
    let op = operation();
    let some = Pieces::empty()
        .with("fault", wall())
        .with("surfaces", plate(3.0))
        .with("surfaces 2", plate(40.0));
    let plan = op
        .plan(&some, &choices(), &Identity::derived(), &when())
        .expect("the plan succeeds");
    assert!(plan.says.iter().any(|n| n.key == "no-trace"));
    assert_eq!(plan.shapes.iter().filter(|m| m.parent.is_some()).count(), 1);
    assert!(plan.activity.parameters.iter().any(|p| matches!(
        p,
        energyml_domain::provenance::Parameter::Double { title, value, .. } if title == "uncutSurfaces" && *value == 1.0
    )));

    let none = Pieces::empty()
        .with("fault", wall())
        .with("surfaces", plate(40.0));
    let why = op
        .plan(&none, &choices(), &Identity::derived(), &when())
        .expect_err("refused");
    assert!(why.message().contains("cuts none"), "{}", why.message());
}
