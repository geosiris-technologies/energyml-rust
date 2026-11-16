//! `along_well::layout`: the drawing in numbers, its SVG, and the drawing made again from its metadata.

use energyml_geometry::around_well::projection::WellProjections;
use energyml_section::along_well::{
    AlongWellRefusal, CurtainSources, LegendAsk, PlotFrame, ScaleKindAsk, build, draw, layout, plot_frame,
    read_metadata, redraw,
};
use energyml_sheet::draw::list::Item;

use super::{hole_request, inclined, model_on, package_with_well, texts, view};

const NODES: [f64; 4] = [100.0, 200.0, 300.0, 400.0];
const ROWS: [f64; 3] = [-10.0, 0.0, 10.0];

fn values() -> Vec<f64> {
    (1..=12).map(f64::from).collect()
}

/// The abscissa of `developed` is a length on the ground, so its text speaks of an exaggeration; the
/// abscissa of `md_tvd` is a depth along the hole, so its text says the vertical scale is against MD.
///
/// To break it: write one text for both.
#[test]
fn the_exaggeration_text_says_what_the_abscissa_is() {
    let projections = WellProjections::standard();
    let developed = model_on(inclined(60.0), "developed", &NODES, &ROWS, values(), false);
    let words = texts(&layout(&developed, &view(), &projections, None).unwrap());
    assert!(
        words.iter().any(|text| text.contains("vertical exaggeration")),
        "{words:?}"
    );

    let flat = model_on(inclined(60.0), "md_tvd", &NODES, &ROWS, values(), false);
    let words = texts(&layout(&flat, &view(), &projections, None).unwrap());
    assert!(words.iter().any(|text| text.contains("against MD")), "{words:?}");
}

/// The two boundaries are told apart by their pattern (02 section 4.3), never by their colour alone.
///
/// To break it: give both the same dash.
#[test]
fn the_two_boundaries_differ_by_their_pattern() {
    let projections = WellProjections::standard();
    let model = model_on(inclined(60.0), "md_tvd", &NODES, &ROWS, values(), true);
    let list = layout(&model, &view(), &projections, None).unwrap();
    let dash_of = |kind: &str| -> Vec<f64> {
        list.layers
            .iter()
            .flat_map(|layer| layer.elements.iter())
            .filter(|element| element.kind == kind)
            .flat_map(|element| element.items.iter())
            .find_map(|item| match item {
                Item::Polyline { stroke, .. } => Some(stroke.dash.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no line of kind {kind}"))
    };
    let above = dash_of("boundary-above");
    let below = dash_of("boundary-below");
    assert!(!above.is_empty() && !below.is_empty());
    assert_ne!(above, below);
}

/// The curtain is written into the drawing as an image only when asked and when the caller gives a colour:
/// this crate holds no palette.
///
/// To break it: draw an image with no colour (a black curtain).
#[test]
fn the_curtain_is_an_image_only_when_asked_and_coloured() {
    let projections = WellProjections::standard();
    let model = model_on(inclined(60.0), "md_tvd", &NODES, &ROWS, values(), false);
    let mut asked = view();
    asked.embed_curtain = true;
    let colour = |value: f64| Some([(value * 10.0) as u8, 0, 0, 255]);

    let with = layout(&model, &asked, &projections, Some(&colour)).unwrap();
    let images = |list: &energyml_sheet::draw::list::DisplayList| {
        list.layers
            .iter()
            .flat_map(|layer| layer.elements.iter())
            .flat_map(|element| element.items.iter())
            .filter(|item| matches!(item, Item::Image { .. }))
            .count()
    };
    assert_eq!(images(&with), 1);
    assert_eq!(images(&layout(&model, &asked, &projections, None).unwrap()), 0);
    assert_eq!(
        images(&layout(&model, &view(), &projections, Some(&colour)).unwrap()),
        0
    );
}

/// A legend that was asked and has no scale is a named refusal, not a drawing without legend; a legend
/// with a scale is drawn, graduated.
///
/// To break it: ignore the refusal of the scale.
#[test]
fn a_legend_is_drawn_or_refused_by_name() {
    let projections = WellProjections::standard();
    let model = model_on(inclined(60.0), "md_tvd", &NODES, &ROWS, values(), false);
    let colour = |value: f64| Some([(value * 10.0) as u8, 0, 0, 255]);

    let mut asked = view();
    asked.legend = Some(LegendAsk {
        kind: Some(ScaleKindAsk::Linear),
        min: Some(0.0),
        max: Some(12.0),
    });
    let list = layout(&model, &asked, &projections, Some(&colour)).unwrap();
    let legend = list.layer("legend").expect("a layer of legend");
    assert_eq!(legend.elements.len(), 1);

    asked.legend = Some(LegendAsk {
        kind: Some(ScaleKindAsk::Log10),
        min: Some(-1.0),
        max: Some(12.0),
    });
    assert!(matches!(
        layout(&model, &asked, &projections, Some(&colour)),
        Err(AlongWellRefusal::Legend(_))
    ));
}

/// The exaggeration, when fixed, sets the ratio of the two scales exactly and keeps the centre of the window.
///
/// To break it: apply the ratio to the horizontal scale (the centre moves).
#[test]
fn a_fixed_exaggeration_sets_the_ratio_and_keeps_the_centre() {
    let model = model_on(inclined(60.0), "md_tvd", &NODES, &ROWS, values(), false);
    let mut fixed = view();
    fixed.exaggeration = Some(4.0);
    let plot: PlotFrame = plot_frame(&model, &fixed);
    assert!((plot.exaggeration() - 4.0).abs() < 1e-12);
    let free = plot_frame(&model, &view());
    let centre = |p: &PlotFrame| (p.window[1] + p.window[3]) / 2.0;
    assert!((centre(&plot) - centre(&free)).abs() < 1e-9);
    assert_eq!((plot.window[0], plot.window[2]), (free.window[0], free.window[2]));
    // Clamped: nobody asks for a million.
    fixed.exaggeration = Some(1e6);
    assert!((plot_frame(&model, &fixed).exaggeration() - 1000.0).abs() < 1e-9);
}

/// The SVG carries its request and its view, and the drawing made again from them over the same package
/// is the same string, byte for byte (02 test 18).
///
/// To break it: leave the view out of the body of the metadata (the second drawing is the default view).
#[test]
fn a_drawing_made_again_from_its_metadata_is_the_same_bytes() {
    let package = package_with_well(&[20.0, 40.0, 60.0, 80.0]);
    let projections = WellProjections::standard();
    let sources = CurtainSources::builtin();
    let mut shown = view();
    shown.size = [640.0, 480.0];
    shown.exaggeration = Some(2.0);
    shown.colours.insert(super::frame_uri(), "#aa3300".to_string());

    let model = build(&hole_request("md_tvd"), &package, &[], &sources, &projections).unwrap();
    let svg = draw(&model, &shown, &projections, None).unwrap();
    assert!(svg.starts_with("<svg"));

    let (request, view_back) = read_metadata(&svg).unwrap();
    assert_eq!(request, model.request);
    assert_eq!(view_back, shown);

    let again = redraw(&svg, &package, &[], &sources, &projections, None).unwrap();
    assert_eq!(again, svg);
}

/// A drawing with no such metadata is refused by name, not drawn from guesses.
///
/// To break it: fall back to a default request.
#[test]
fn a_drawing_with_no_metadata_is_refused() {
    let refused = read_metadata("<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>");
    assert!(matches!(refused, Err(AlongWellRefusal::Metadata(_))));
}
