//! `along-well`: the offers, the hole laid flat at each projection, the exaggeration, the refusals, the redraw.
//!
//! The package is fabricated: a straight hole going 50 m east over 100 m of depth, MD 0 to 100 (the line of
//! `fixtures::package`), a frame with nodes at MD 20 and 80. Worked by hand, the hole at MD `m` is at TVD `m` and
//! `m / 2` metres east: in `md_tvd` its point is `[m, m]`, in `developed` and `vertical_section` (to the east) it is
//! `[m / 2, m]`. The curtain's values cannot be fabricated without an array file, so it is tested as far as the
//! package goes without one: what is offered, what is asked of the caller, what is refused.

use energyml_core::xml::XmlText;
use energyml_eml_2_3 as eml23;
use energyml_geometry::around_well::projection::{WellProjection, WellProjections};
use energyml_resqml_2_2 as resqml22;
use energyml_section::along_well as section;
use energyml_sheet::draw::DisplayList;
use energyml_views::along_well::{AlongWell, ID};
use energyml_views::{ViewAsk, ViewKind, ViewKinds, ViewRefusal, draw_list, draw_svg, read_ask, redraw};
use serde_json::json;

use crate::fixtures::{FRAME, NO_MAPS, Opened, frame_uri, package, polylines, uri};

const MATRIX: &str = "55555555-5555-4555-8555-555555555555";

fn kinds() -> ViewKinds {
    let mut kinds = ViewKinds::empty();
    kinds.register(Box::new(AlongWell));
    kinds
}

/// A matrix of 3 x 3 values per node on the frame: a curtain candidate, with no axis the file states.
fn matrix_property() -> resqml22::ContinuousProperty {
    resqml22::ContinuousProperty {
        uuid: MATRIX.to_string(),
        schema_version: "2.2".into(),
        citation: eml23::Citation {
            title: "a matrix".to_string(),
            ..Default::default()
        },
        indexable_element: eml23::IndexableElement::Nodes,
        value_count_per_indexable_element: vec![3, 3],
        property_kind: eml23::DataObjectReference {
            uuid: "dddddddd-dddd-4ddd-8ddd-dddddddddddd".to_string(),
            qualified_type: "eml23.PropertyKind".to_string(),
            title: "a kind".to_string(),
            ..Default::default()
        },
        supporting_representation: eml23::DataObjectReference {
            uuid: FRAME.to_string(),
            qualified_type: "resqml22.WellboreFrameRepresentation".to_string(),
            title: "a frame".to_string(),
            ..Default::default()
        },
        uom: eml23::UnitOfMeasureExt::from_xml_text("ohm.m").expect("a unit"),
        values_for_patch: vec![resqml22::AbstractValueArray::IntegerConstantArray(
            eml23::IntegerConstantArray {
                value: 1,
                count: 18,
                ..Default::default()
            },
        )],
        ..Default::default()
    }
}

fn hole() -> Opened {
    Opened::of(package(50.0, &[20.0, 80.0]))
}

fn with_curtain() -> Opened {
    let mut store = package(50.0, &[20.0, 80.0]);
    store.put(Box::new(matrix_property())).expect("it goes in");
    Opened::of(store)
}

fn matrix_uri() -> String {
    uri("resqml22.ContinuousProperty", MATRIX)
}

fn draw(opened: &Opened, ask: &ViewAsk) -> Result<DisplayList, ViewRefusal> {
    draw_list(&kinds(), ask, &opened.context(&NO_MAPS)).map(|(drawn, _)| drawn.list)
}

fn base() -> ViewAsk {
    ViewAsk::new(ID).object(frame_uri()).sized(800.0, 600.0)
}

/// What the section itself would draw over the same package: the model and the view the face passes it.
fn section_of(opened: &Opened, projection: &str, ask: Option<section::SectionAsk>) -> (section::Model, section::View) {
    let wanted = section::Request {
        frame: frame_uri(),
        trajectory: None,
        curtain: None,
        boundaries: None,
        surfaces: Vec::new(),
        computed_tops: false,
        markers: Vec::new(),
        md_range: None,
        dn_range: None,
        projection: projection.to_string(),
        section: ask,
        step: None,
    };
    let model = section::build(
        &wanted,
        &opened.store,
        &[],
        &section::CurtainSources::builtin(),
        &WellProjections::standard(),
    )
    .expect("the section is built");
    let view = section::View {
        size: [800.0, 600.0],
        window: None,
        exaggeration: None,
        show_curtain: true,
        show_boundaries: true,
        show_surfaces: true,
        show_tops: true,
        colours: Default::default(),
        legend: None,
        embed_curtain: true,
    };
    (model, view)
}

/// The first and the last point of the hole as drawn, brought back to the drawing's units by the section's frame.
fn ends(ask: ViewAsk, projection: &str, vertical: Option<section::SectionAsk>) -> ([f64; 2], [f64; 2]) {
    let opened = hole();
    let list = draw(&opened, &ask).expect("drawn");
    let lines = polylines(&list, "trajectory", "trajectory");
    assert_eq!(lines.len(), 1, "the hole is one line");
    let line = &lines[0];
    let (model, view) = section_of(&opened, projection, vertical);
    let plot = section::plot_frame(&model, &view);
    (plot.to_data(line[0]), plot.to_data(line[line.len() - 1]))
}

fn near(found: [f64; 2], expected: [f64; 2]) {
    assert!(
        (found[0] - expected[0]).abs() < 1.0e-6 && (found[1] - expected[1]).abs() < 1.0e-6,
        "{found:?} is not {expected:?}"
    );
}

#[test]
fn the_hole_and_each_curtain_candidate_are_offered_with_what_is_missing() {
    let opened = with_curtain();
    let offers = kinds().offers(&opened.index);
    let hole_only = offers
        .iter()
        .find(|offer| offer.objects == vec![frame_uri()])
        .expect("the hole is offered");
    assert!(hole_only.missing.is_empty(), "the hole alone needs nothing");
    let curtain = offers
        .iter()
        .find(|offer| offer.objects == vec![frame_uri(), matrix_uri()])
        .expect("the matrix is offered as a curtain");
    assert!(
        curtain.missing.iter().any(|words| words.contains("slow_axis")),
        "the roles are the caller's: {:?}",
        curtain.missing
    );
    assert!(
        curtain.missing.iter().any(|words| words.contains("axes")),
        "the file gives no axis: {:?}",
        curtain.missing
    );
    assert!(offers.iter().all(|offer| offer.view == ID));
    // Break it: offer the matrix with `missing` empty, or leave the frame out of the objects.
}

#[test]
fn in_md_against_tvd_the_hole_runs_from_its_first_to_its_last_node() {
    // Hand worked: the hole is at TVD = MD, so the nodes 20 and 80 are the data points [20, 20] and [80, 80].
    let (first, last) = ends(base().set("projection", json!("md_tvd")), "md_tvd", None);
    near(first, [20.0, 20.0]);
    near(last, [80.0, 80.0]);
    // Triangulation: the section's own projection places the last node at the same point.
    let projections = WellProjections::standard();
    let (model, _) = section_of(&hole(), "md_tvd", None);
    let placed = projections
        .get("md_tvd")
        .expect("the registry holds it")
        .place(&model.axis, 80.0, 0.0)
        .expect("placed");
    near(last, placed);
    // Break it: draw the hole in `developed` whatever the projection asked.
}

#[test]
fn developed_lays_the_hole_by_its_length_on_the_ground() {
    // 50 m east over 100 m of MD: from MD 20 to 80 the plan length grows by 30 m and the depth by 60 m.
    let (first, last) = ends(base().set("projection", json!("developed")), "developed", None);
    near([last[0] - first[0], last[1] - first[1]], [30.0, 60.0]);
    // Break it: use the MD as the abscissa (the width would be 60).
}

#[test]
fn a_vertical_section_to_the_east_gets_its_azimuth_and_its_origin_from_the_caller() {
    let ask = base()
        .set("projection", json!("vertical_section"))
        .set("azimuth", json!(90.0))
        .set("origin", json!([1000.0, 2000.0]));
    let vertical = section::SectionAsk {
        azimuth: std::f64::consts::FRAC_PI_2,
        origin: [1000.0, 2000.0],
    };
    let (first, last) = ends(ask, "vertical_section", Some(vertical));
    near([last[0] - first[0], last[1] - first[1]], [30.0, 60.0]);
    // Break it: take an azimuth of 0 (the hole goes east, not north: the width would be 0).
}

#[test]
fn the_exaggeration_is_the_ratio_of_the_two_scales() {
    // Between the nodes the hole moves 60 units across and 60 down: the pixels' ratio is the exaggeration.
    let list = draw(&hole(), &base().set("exaggeration", json!(2.0))).expect("drawn");
    let line = &polylines(&list, "trajectory", "trajectory")[0];
    let (first, last) = (line[0], line[line.len() - 1]);
    let ratio = (last[1] - first[1]) / (last[0] - first[0]);
    assert!((ratio - 2.0).abs() < 1.0e-9, "{ratio}");
    // Break it: ignore the setting (the ratio is whatever the window and the size give).
}

#[test]
fn the_hole_carries_the_frame_uri_as_its_key() {
    let list = draw(&hole(), &base()).expect("drawn");
    let keys: Vec<&str> = list
        .layers
        .iter()
        .filter(|layer| layer.name == "trajectory")
        .flat_map(|layer| layer.elements.iter())
        .filter_map(|element| element.key.as_deref())
        .collect();
    assert_eq!(keys, vec![frame_uri().as_str()]);
    // Break it: key the hole by its uuid.
}

#[test]
fn what_the_caller_must_declare_is_asked_never_guessed() {
    let opened = with_curtain();
    let missing = |ask: ViewAsk| match draw(&opened, &ask) {
        Err(ViewRefusal::MissingSetting { name, .. }) => name,
        other => panic!("expected a missing setting, got {other:?}"),
    };
    let section_ask = base().set("projection", json!("vertical_section"));
    assert_eq!(missing(section_ask.clone()), "azimuth");
    assert_eq!(missing(section_ask.set("azimuth", json!(90.0))), "origin");
    // A curtain: the roles and signs are the caller's, the file does not state them.
    let curtain = base().object(matrix_uri());
    assert_eq!(missing(curtain.clone()), "slow_axis");
    let declared = curtain
        .set("slow_axis", json!("high_side"))
        .set("slow_sign", json!("positive"))
        .set("fast_axis", json!("right"))
        .set("fast_sign", json!("positive"));
    // Declared, the section's own refusal comes through, by its words: ranks are not offsets.
    match draw(&opened, &declared) {
        Err(ViewRefusal::Computing { why, .. }) => {
            assert!(why.contains("ranks are not offsets"), "{why}");
        }
        other => panic!("expected the section's refusal, got {other:?}"),
    }
    // A source the registry does not hold is refused by the section, by name.
    match draw(&opened, &declared.set("curtain_source", json!("nowhere"))) {
        Err(ViewRefusal::Computing { why, .. }) => assert!(why.contains("no curtain source"), "{why}"),
        other => panic!("expected the section's refusal, got {other:?}"),
    }
    // Break it: default the roles to high side, positive (the curtain would be drawn on a guess).
}

#[test]
fn a_setting_that_does_not_fit_is_refused_by_name() {
    let opened = hole();
    assert!(matches!(
        draw(&opened, &base().set("exaggeration", json!(5000.0))),
        Err(ViewRefusal::BadSetting { name, .. }) if name == "exaggeration"
    ));
    assert!(matches!(
        draw(&opened, &base().set("depth", json!(1.0))),
        Err(ViewRefusal::UnknownSetting { name, .. }) if name == "depth"
    ));
    let ask = base()
        .set("projection", json!("vertical_section"))
        .set("azimuth", json!(90.0))
        .set("origin", json!([1.0, 2.0, 3.0]));
    assert!(matches!(
        draw(&opened, &ask),
        Err(ViewRefusal::BadSetting { name, .. }) if name == "origin"
    ));
    // Break it: read a longer `origin` by its first two numbers.
}

#[test]
fn the_settings_are_the_fields_of_the_section_request_and_view() {
    let settings = AlongWell.settings();
    let names: Vec<&str> = settings.iter().map(|setting| setting.name).collect();
    for field in [
        // `Request` (the frame, the trajectory, the curtain's property and the surfaces are objects).
        "projection",
        "azimuth",
        "origin",
        "step",
        "md_range",
        "dn_range",
        "computed_tops",
        "curtain_source",
        "curtain_links",
        "slow_axis",
        "slow_sign",
        "fast_axis",
        "fast_sign",
        "lateral_index",
        "boundaries",
        // `View` (the size is the request's, the colours are the colour door's).
        "window",
        "exaggeration",
        "show_curtain",
        "show_boundaries",
        "show_surfaces",
        "show_tops",
        "legend",
        "scale",
        "min",
        "max",
        "palette",
    ] {
        assert!(names.contains(&field), "no setting `{field}`: {names:?}");
    }
    // Break it: remove a field from `settings()`.
}

#[test]
fn a_redraw_makes_the_same_bytes_and_the_metadata_is_this_crates() {
    let opened = hole();
    let kinds = kinds();
    let cx = opened.context(&NO_MAPS);
    let first = draw_svg(&kinds, &base().set("projection", json!("developed")), &cx).expect("drawn");
    let again = redraw(&kinds, &first.svg, &cx).expect("drawn again");
    assert_eq!(again.svg, first.svg);
    let ask = read_ask(&first.svg).expect("the ask is in the metadata");
    assert_eq!(ask.view, ID);
    assert_eq!(ask.settings.get("projection"), Some(&json!("developed")));
    assert!(
        !first.svg.contains("along-well-view"),
        "the section's own metadata is not written: one format of redraw"
    );
    // Break it: write the metadata of `energyml_section::along_well::draw` (the element `along-well-view`).
}
