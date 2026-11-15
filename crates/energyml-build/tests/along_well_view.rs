//! `along-well-view`: a geosteering section stored in the package - its SVG
//! attached to the frame of the curtain, and an activity naming the objects it
//! was computed from - and read back by `stored_views`.
//!
//! The form of the assertions comes from the rule of `section-view` (the
//! drawing is the input, the recipe is read from its metadata) and from the
//! packaging conventions (EPC v1.0 section 3.4.2 pp. 15-16: a part related to
//! its `Attachment`), never from what the writer computes. The drawings are
//! made here with the writer of SVG of `energyml-sheet`, the metadata body is
//! a `Request` and a `View` typed in the test.
//!
//! What would make them fail: the images attached to the trajectory, an object
//! the drawing names ignored when the package lacks it, a setting that
//! disagrees with the drawing taken in its place, a stored view found by its
//! title, a second reader of the activities beside `stored_sections`.

use std::collections::BTreeMap;
use std::sync::Arc;

use energyml_build::operation::{Choices, Identity, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_domain::provenance::Parameter;
use energyml_section::along_well::{Request, View};
use energyml_sheet::draw::list::{DisplayList, PageUnit};
use energyml_sheet::draw::svg::{self, Metadata};
use serde_json::json;

const FRAME: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const TRAJECTORY: &str = "aaaaaaaa-0000-4000-8000-000000000002";
const SURFACE: &str = "bbbbbbbb-0000-4000-8000-000000000003";

fn when() -> When {
    When {
        creation: "2026-11-15T00:00:00Z".into(),
        originator: "tests/along_well_view.rs".into(),
    }
}

fn frame() -> Ref {
    Ref::new(FRAME, "resqml22.WellboreFrameRepresentation", "the frame")
}
fn trajectory() -> Ref {
    Ref::new(TRAJECTORY, "resqml22.WellboreTrajectoryRepresentation", "the trajectory")
}
fn surface() -> Ref {
    Ref::new(SURFACE, "resqml22.TriangulatedSetRepresentation", "the horizon")
}

fn request(projection: &str) -> Request {
    Request {
        frame: frame().uri(),
        trajectory: Some(trajectory().uri()),
        curtain: None,
        boundaries: None,
        surfaces: vec![surface().uri()],
        computed_tops: false,
        markers: Vec::new(),
        md_range: Some([0.0, 1000.0]),
        dn_range: None,
        projection: projection.to_string(),
        section: None,
        step: Some(1.0),
    }
}

fn view() -> View {
    View {
        size: [800.0, 400.0],
        window: None,
        exaggeration: None,
        show_curtain: true,
        show_boundaries: true,
        show_surfaces: true,
        show_tops: true,
        colours: BTreeMap::new(),
        legend: None,
        embed_curtain: false,
    }
}

/// A drawing whose metadata is `along-well-view`, as `along_well::draw` writes it.
fn drawing(request: &Request) -> String {
    let body = serde_json::to_string(&(request, view())).expect("the body");
    svg::write(
        &DisplayList::new([100.0, 100.0], PageUnit::Pixel),
        Some(&Metadata {
            element: "along-well-view".into(),
            namespace: "urn:energyml:along-well-view".into(),
            format: 1,
            version: "test".into(),
            body,
        }),
    )
    .expect("an SVG")
}

fn held(reference: Ref) -> Source {
    Source::Object(Known {
        reference: Some(reference),
        ..Default::default()
    })
}

/// The drawing, and the objects of the package it was computed from.
fn bag(svg_text: &str, objects: Vec<Ref>) -> Pieces {
    let mut pieces = Pieces::empty().with(
        "svg",
        Source::Document {
            name: "section.svg".to_string(),
            content_type: None,
            bytes: svg_text.as_bytes().to_vec(),
        },
    );
    for (at, one) in objects.into_iter().enumerate() {
        let name = if at == 0 { "objects".to_string() } else { format!("objects {}", at + 1) };
        pieces = pieces.with(&name, held(one));
    }
    pieces
}

fn everything() -> Vec<Ref> {
    vec![frame(), trajectory(), surface()]
}

fn planned(pieces: &Pieces, choices: Choices) -> Result<Plan, String> {
    Registry::new()
        .plan("along-well-view", pieces, &choices, &Identity::derived(), &when())
        .map_err(|why| why.to_string())
}

fn developed() -> Choices {
    Choices::new().set("projection", "developed")
}

#[test]
fn a_section_is_stored_with_its_inputs_and_its_drawing() {
    let plan = planned(&bag(&drawing(&request("developed")), everything()), developed()).expect("planned");
    assert_eq!(plan.activity.recipe, "along-well-view");
    // The attachment is on the frame, form C: its part is in the package.
    let attachment = plan
        .objects
        .iter()
        .find(|one| one.qualified_type == "eml23.Attachment")
        .expect("an attachment");
    assert_eq!(attachment.json["ObjectReference"]["Uuid"], json!(FRAME), "attached to the frame");
    assert_eq!(plan.media.len(), 1, "the SVG's part");
    let inputs: Vec<&str> = plan
        .activity
        .parameters
        .iter()
        .filter_map(|p| match p {
            Parameter::Object { title, reference } if title == "input" => reference["Uuid"].as_str(),
            _ => None,
        })
        .collect();
    assert_eq!(inputs, vec![FRAME, TRAJECTORY, SURFACE]);
    let part = plan.activity.parameters.iter().find_map(|p| match p {
        Parameter::Text { title, value } if title == "svgPart" => Some(value.clone()),
        _ => None,
    });
    assert_eq!(part.as_deref(), Some(plan.media[0].path.as_str()));
}

#[test]
fn an_object_the_package_does_not_hold_is_refused_by_its_uri() {
    let without_surface = vec![frame(), trajectory()];
    let why = planned(&bag(&drawing(&request("developed")), without_surface), developed()).expect_err("refused");
    assert!(why.contains("`objects`"), "{why}");
    assert!(why.contains(&surface().uri()), "it names the uri: {why}");
}

#[test]
fn a_setting_that_disagrees_with_the_drawing_is_refused() {
    let why = planned(
        &bag(&drawing(&request("developed")), everything()),
        Choices::new().set("projection", "md_tvd"),
    )
    .expect_err("refused");
    assert!(why.contains("`projection`"), "{why}");
    assert!(why.contains("developed") && why.contains("md_tvd"), "both values: {why}");
}

#[test]
fn stored_views_finds_it_by_its_template_never_its_title() {
    let section = planned(&bag(&drawing(&request("developed")), everything()), developed()).expect("planned");
    let mut other = Registry::new()
        .plan(
            "crs",
            &Pieces::default(),
            &Choices::default().set("epsg", 2154),
            &Identity::derived(),
            &when(),
        )
        .unwrap_or_else(|e| panic!("{:?}", e.refusals()));
    other.activity.title = section.activity.title.clone();
    let mut store = energyml_build::epc::open(None, "view.epc", Arc::new(energyml_schemas::all()))
        .expect("an empty package");
    for plan in [&section, &other] {
        let made = energyml_build::apply::apply_with(plan, &mut store, &when(), false);
        assert!(made.refused.is_empty(), "{:?}", made.refused);
    }
    let listed = energyml_build::read::stored_views(&store, "along-well-view");
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0].recipe, "along-well-view");
    assert_eq!(listed[0].title, section.activity.title);
    assert_eq!(
        listed[0].inputs,
        vec![frame().uri(), trajectory().uri(), surface().uri()],
        "the entries the activity names"
    );
    assert_eq!(listed[0].drawings, vec![section.media[0].path.clone()]);
    // A recipe nobody wrote lists nothing, rather than everything.
    assert!(energyml_build::read::stored_views(&store, "no-such-recipe").is_empty());
}
