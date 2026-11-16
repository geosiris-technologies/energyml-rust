//! `isosurface`: an iso-surface computed elsewhere, promoted on demand to a
//! `TriangulatedSetRepresentation` with the recipe it was computed by.
//!
//! The operation does not compute: it takes the mesh as it was computed and
//! records the recipe as the caller states it (ADR-0037). What is asserted is
//! therefore what the **writing** owes: one writer of triangulated sets (the
//! one of `triangles`, never a copy), the recipe in the activity, and a refusal
//! by name for what a surface of this kind cannot be - a shape with no triangle,
//! a logarithm of nothing.
//!
//! What would make them fail: a second writer, the cube or the support missing
//! from the activity, a shape of points accepted, a level of 0 under log10.

use energyml_build::operation::{Choices, Identity, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};
use serde_json::{Value, json};
use std::sync::Arc;

const SURFACE: &str = "bbbbbbbb-0000-4000-8000-000000000002";
const CUBE: &str = "dddddddd-0000-4000-8000-000000000001";
const SUPPORT: &str = "aaaaaaaa-0000-4000-8000-000000000001";

fn when() -> When {
    When {
        creation: "2026-11-15T00:00:00Z".into(),
        originator: "tests/isosurface_promotion.rs".into(),
    }
}

fn geometry(topology: Topology) -> Geometry {
    Geometry {
        kind: "isosurface",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points: vec![
                [0.0, 0.0, 0.0],
                [10.0, 0.0, 0.0],
                [10.0, 10.0, 0.0],
                [0.0, 10.0, 0.0],
                [5.0, 5.0, 10.0],
            ],
            topology,
            closed: Vec::new(),
        }],
    }
}

fn pyramid() -> Geometry {
    geometry(Topology::Triangles(vec![
        [0, 1, 4],
        [1, 2, 4],
        [2, 3, 4],
        [3, 0, 4],
    ]))
}

/// The surface and its companion, in a compound frame counting up - as
/// `read::in_slot` hands a shape of the package.
fn surface(geometry: Geometry) -> Pieces {
    Pieces::empty()
        .with(
            "shape",
            Source::Shape {
                geometry: Arc::new(geometry),
                closed: None,
            },
        )
        .with(
            "shape (object)",
            Source::Object(Known {
                reference: Some(Ref::new(
                    SURFACE,
                    "resqml22.TriangulatedSetRepresentation",
                    "the surface",
                )),
                crs: Some(Ref::new(
                    "aaaaaaaa-0000-4000-8000-0000000000c5",
                    "eml23.LocalEngineeringCompoundCrs",
                    "the frame",
                )),
                vertical_uom: Some("m".into()),
                vertical_up: Some(true),
                node_count: Some(5),
                ..Default::default()
            }),
        )
}

fn with_inputs(bag: Pieces) -> Pieces {
    bag.with(
        "cube",
        Source::Object(Known {
            reference: Some(Ref::new(CUBE, "resqml22.ContinuousProperty", "the cube")),
            ..Default::default()
        }),
    )
    .with(
        "support",
        Source::Object(Known {
            reference: Some(Ref::new(
                SUPPORT,
                "resqml22.WellboreFrameRepresentation",
                "the frame",
            )),
            ..Default::default()
        }),
    )
}

fn choices() -> Choices {
    Choices::new()
        .set("title", "iso 20")
        .set("level", 20)
        .set("levelUom", "ohm.m")
        .set("spacing", "log10")
}

fn planned(bag: &Pieces, choices: Choices) -> Result<Plan, String> {
    Registry::new()
        .plan("isosurface", bag, &choices, &Identity::derived(), &when())
        .map_err(|why| why.to_string())
}

fn set_of(plan: &Plan) -> &Value {
    &plan
        .objects
        .iter()
        .find(|one| one.qualified_type == "resqml22.TriangulatedSetRepresentation")
        .expect("a triangulated set")
        .json
}

#[test]
fn an_isosurface_is_promoted_with_its_recipe() {
    let plan = planned(&with_inputs(surface(pyramid())), choices()).expect("planned");
    assert_eq!(
        set_of(&plan)["$type"],
        json!("resqml22.TriangulatedSetRepresentation")
    );
    assert_eq!(plan.activity.recipe, "isosurface");
    let level = plan.activity.parameters.iter().find_map(|p| match p {
        Parameter::Double { title, value, uom } if title == "level" => Some((*value, uom.clone())),
        _ => None,
    });
    assert_eq!(level, Some((20.0, Some("ohm.m".to_string()))));
    let text = |name: &str| {
        plan.activity.parameters.iter().find_map(|p| match p {
            Parameter::Text { title, value } if title == name => Some(value.clone()),
            _ => None,
        })
    };
    assert_eq!(text("spacing").as_deref(), Some("log10"));
    assert_eq!(text("method").as_deref(), Some("marching-tetrahedra"));
    let inputs: Vec<&str> = plan
        .activity
        .parameters
        .iter()
        .filter_map(|p| match p {
            Parameter::Object { title, reference } if title == "input" => reference["Uuid"].as_str(),
            _ => None,
        })
        .collect();
    assert_eq!(
        inputs,
        vec!["aaaaaaaa-0000-4000-8000-0000000000c5", CUBE, SUPPORT],
        "the CRS (recorded by the triangles writer, as for every surface), then the cube and the support"
    );
    assert!(plan.says.iter().any(|note| note.key == "promoted"));
}

#[test]
fn the_surface_is_the_triangles_writers() {
    let promoted = planned(&with_inputs(surface(pyramid())), choices()).expect("planned");
    // The same shape, the same naming, by the operation that writes triangles.
    let direct = Registry::new()
        .plan(
            "triangles",
            &surface(pyramid()),
            &Choices::new().set("title", "iso 20"),
            &Identity::derived(),
            &when(),
        )
        .expect("planned");
    assert_eq!(set_of(&promoted), set_of(&direct), "two doors, one writer");
}

#[test]
fn a_shape_without_triangles_is_refused() {
    let why = planned(&with_inputs(surface(geometry(Topology::Points))), choices()).expect_err("refused");
    assert!(why.contains("shape"), "{why}");
}

#[test]
fn a_log10_level_at_or_below_zero_is_refused() {
    let why = planned(&with_inputs(surface(pyramid())), choices().set("level", 0)).expect_err("refused");
    assert!(why.contains("`level`") && why.contains("log"), "{why}");
}
