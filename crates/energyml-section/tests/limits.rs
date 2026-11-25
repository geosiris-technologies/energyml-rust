//! The lines across a well at its limits, their colour and width, and the ramp legend
//! (ADR-0052). Everything is built here: a well of the shared fixture (three nodes, MDs
//! 0, 200 and 520, two strata intervals), limits written by hand or taken from
//! `energyml_domain::limits`, a ramp sampled by a closure. Nothing is read from a package.
//!
//! What each test says to break is in its last lines.

use energyml_domain::frame_values::Support;
use energyml_domain::limits::{Anchor, Class, Classification, LimitKind, Threshold, transitions};
use energyml_domain::stats::scale::{Scale, ScaleKind};
use energyml_section::layout::{Attr, LaidObject, Layout, Shape, layout};
use energyml_section::limits::well_limits;
use energyml_section::model::*;
use energyml_section::{ramp, svg};

use crate::layout::{catalogue, id, view, well};

fn key_of(name: &str) -> String {
    id(name, "WellboreFrameRepresentation").key()
}

fn drawn<'a>(l: &'a Layout, key: &str) -> &'a LaidObject {
    l.layers
        .iter()
        .flat_map(|layer| layer.objects.iter())
        .find(|o| matches!(&o.attr, Attr::Key(k) if k == key))
        .expect("the well is drawn")
}

fn one_well() -> Catalogue {
    catalogue(vec![Item::Well(well("A", 150.0, 12.0, Side::Left))])
}

fn limit(at: LimitAt, rank: usize) -> WellLimit {
    WellLimit {
        at,
        rank,
        above: Some("Bartonian".to_string()),
        below: Some("Lutetian".to_string()),
    }
}

fn with_limits(limits: Vec<WellLimit>) -> View {
    View {
        limits: vec![(key_of("A"), limits)],
        ..view()
    }
}

/// The part of an SVG between the opening of a group and the next closing one.
fn group<'a>(text: &'a str, part: &str) -> &'a str {
    let open = text
        .find(&format!("<g data-part=\"{part}\">"))
        .unwrap_or_else(|| panic!("no group `{part}` in the drawing"));
    let rest = &text[open..];
    &rest[..rest.find("</g>").expect("the group closes")]
}

// -- the style of the lines ---------------------------------------------------------

/// The ticks between two strata are the black line of one pixel they always were: the style they
/// share with the limits has that default, and the same bytes come out.
///
/// Break it: in `LimitStyle::default`, write another colour (`"#111111"`).
#[test]
fn the_ticks_between_strata_keep_the_black_line_of_one_pixel_by_default() {
    let v = View {
        well_colouring: WellColouring::Strata,
        strata_ticks: true,
        ..view()
    };
    let text = svg::write(&layout(&one_well(), &v));
    let ticks = group(&text, "strata-ticks");
    assert!(ticks.contains("stroke=\"#000000\" stroke-width=\"1\""), "{ticks}");
    assert!(!text.contains("data-part=\"limits\""), "no limit was asked for");
    assert!(
        !serde_json::to_string(&v).unwrap().contains("limitStyle"),
        "the default style is not written into the metadata"
    );
}

/// One style for every line across a well: the strata ticks and the limits take the colour and the
/// width the view gives; one that cannot be drawn is said and the default is drawn.
///
/// Break it: in `write_object`, write the ticks with `&LimitStyle::default()`.
#[test]
fn the_colour_and_the_width_of_the_lines_are_the_views_for_ticks_and_limits_alike() {
    let v = View {
        well_colouring: WellColouring::Strata,
        strata_ticks: true,
        limit_style: LimitStyle {
            colour: "#cc0000".into(),
            width_px: 2.5,
        },
        ..with_limits(vec![limit(LimitAt::Md(100.0), 1)])
    };
    let text = svg::write(&layout(&one_well(), &v));
    let wanted = "stroke=\"#cc0000\" stroke-width=\"2.5\"";
    assert!(group(&text, "strata-ticks").contains(wanted));
    assert!(group(&text, "limits").contains(wanted));

    let refused = View {
        limit_style: LimitStyle {
            colour: "red".into(),
            width_px: 0.0,
        },
        ..v
    };
    let l = layout(&one_well(), &refused);
    assert!(
        l.warnings.iter().any(|w| w.contains("limit style not drawn")),
        "{:?}",
        l.warnings
    );
    assert!(
        group(&svg::write(&l), "limits").contains("stroke=\"#000000\" stroke-width=\"1\""),
        "the default is drawn"
    );
}

// -- the limits -----------------------------------------------------------------------

/// A limit at an MD is a line across the path where the MD is, perpendicular to it, as wide as the
/// well and two pixels more each side; it says its rank, what lies above and below, and its MD.
///
/// Break it: in `tick_through`, take `half` as `TICK_OVERHANG_PX` alone.
#[test]
fn a_limit_is_a_line_across_the_well_at_its_md() {
    let l = layout(&one_well(), &with_limits(vec![limit(LimitAt::Md(100.0), 2)]));
    let o = drawn(&l, &key_of("A"));
    assert_eq!(o.limits.len(), 1);
    let found = &o.limits[0];
    assert_eq!((found.md, found.rank), (Some(100.0), 2));
    let [a, b] = found.ends;
    // The first leg of the well is vertical: the line across it is horizontal, centred on it.
    assert!((a[1] - b[1]).abs() < 1e-9, "{a:?} {b:?}");
    assert!(
        ((b[0] - a[0]).abs() - (4.0 + 4.0)).abs() < 1e-9,
        "well width 4, two px more each side"
    );
    let centre = (a[0] + b[0]) / 2.0;
    assert!((centre - (l.frame.x + (150.0 - l.window.x[0]) * l.scale[0])).abs() < 1e-9);

    let text = svg::write(&l);
    let written = group(&text, "limits");
    assert!(written.contains("data-limit-rank=\"2\""), "{written}");
    assert!(written.contains("data-limit-md=\"100\""), "{written}");
    assert!(written.contains("data-limit-above=\"Bartonian\""), "{written}");
    assert!(written.contains("data-limit-below=\"Lutetian\""), "{written}");
}

/// A limit between two nodes of the path, at a fraction, is where that fraction of the leg is: its MD
/// is the same fraction between the two MDs.
///
/// Break it: in `along_path`, take the point at `a[0] + dx * (1.0 - t)`.
#[test]
fn a_limit_between_two_nodes_is_where_its_fraction_of_the_leg_is() {
    let l = layout(
        &one_well(),
        &with_limits(vec![limit(LimitAt::Node { node: 1, t: 0.5 }, 1)]),
    );
    let found = &drawn(&l, &key_of("A")).limits[0];
    // Leg 1 runs from (150, -200) to (200, -500): its middle is (175, -350). Its MD, from 200 to 520, 360.
    assert_eq!(found.md, Some(360.0));
    let [a, b] = found.ends;
    let (x, y) = ((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0);
    let (want_x, want_y) = (
        l.frame.x + (175.0 - l.window.x[0]) * l.scale[0],
        l.frame.y + l.frame.h - (-350.0 - l.window.y[0]) * l.scale[1],
    );
    assert!(
        (x - want_x).abs() < 1e-9 && (y - want_y).abs() < 1e-9,
        "({x}, {y}) against ({want_x}, {want_y})"
    );
}

/// A limit the path does not hold - an MD outside the well's, a node it does not have - is not drawn
/// where it is not: it is counted in a note.
///
/// Break it: in `along_path`, find the segment with `.unwrap_or(0)` instead of `?`.
#[test]
fn a_limit_off_the_path_is_counted_and_not_drawn() {
    let l = layout(
        &one_well(),
        &with_limits(vec![
            limit(LimitAt::Md(9999.0), 1),
            limit(LimitAt::Node { node: 7, t: 0.5 }, 1),
            limit(LimitAt::Md(100.0), 1),
        ]),
    );
    assert_eq!(
        drawn(&l, &key_of("A")).limits.len(),
        1,
        "only the one on the path"
    );
    assert!(
        l.warnings
            .iter()
            .any(|w| w.contains("2 limit(s)") && w.contains("not on its path")),
        "{:?}",
        l.warnings
    );
}

/// Limits are drawn **over** a well the scene paints: its strata are abandoned under scene colouring,
/// the limits are not - they are what the paint does not say.
///
/// Break it: at the top of `laid_limits`, add `if paint_of(view, w).is_some() { return Vec::new(); }`.
#[test]
fn limits_are_drawn_over_a_well_the_scene_paints() {
    let painted = View {
        well_colouring: WellColouring::Scene,
        paints: vec![(
            key_of("A"),
            Paint {
                nodes: vec!["#ff0000ff".into(), "#0000ffff".into(), "#00ff00ff".into()],
                blend: Blend::Net,
            },
        )],
        ..with_limits(vec![limit(LimitAt::Md(100.0), 1)])
    };
    let l = layout(&one_well(), &painted);
    let o = drawn(&l, &key_of("A"));
    assert!(
        o.intervals.iter().all(|i| i.unit.is_empty()),
        "painted, not stratified"
    );
    assert_eq!(o.limits.len(), 1, "the limit is drawn over the paint");
    let text = svg::write(&l);
    let paint_at = text.rfind("stroke=\"#ff0000\"").expect("the paint is drawn");
    assert!(
        text.find("data-part=\"limits\"").expect("the limits are drawn") > paint_at,
        "after the paint, so over it"
    );
}

// -- from the domain ----------------------------------------------------------------------

/// The limits of `energyml_domain::limits` become what a view carries: the tops (or the bases), placed
/// by the anchor asked, each with its rank and the names of what lies above and below; one that cannot
/// be placed is left out and said.
///
/// Break it: in `well_limits`, keep the limits whose kind is **not** the one asked.
#[test]
fn the_limits_of_the_domain_are_the_limits_of_the_view() {
    let md = vec![0.0, 200.0, 520.0];
    let classified = Classification::new(
        Support::Nodes { md: md.clone() },
        vec![Some(Class::Code(1)), Some(Class::Code(1)), Some(Class::Code(2))],
    )
    .expect("one class per node");
    let found = transitions(&classified);
    let (tops, refused) = well_limits(&found, LimitKind::Top, Anchor::Inside);
    assert!(refused.is_empty());
    assert_eq!(tops.len(), 1);
    assert_eq!(
        tops[0].at,
        LimitAt::Md(520.0),
        "the first node of the run it opens"
    );
    assert_eq!(
        (tops[0].rank, tops[0].above.as_deref(), tops[0].below.as_deref()),
        (1, Some("code 1"), Some("code 2"))
    );
    let (bases, _) = well_limits(&found, LimitKind::Base, Anchor::Inside);
    assert_eq!(
        bases[0].at,
        LimitAt::Md(200.0),
        "the last node of the run it closes"
    );

    // Interpolated, a limit that is not a threshold is not placed - and said.
    let (none, said) = well_limits(&found, LimitKind::Top, Anchor::Interpolated);
    assert!(none.is_empty());
    assert!(
        said[0].contains("not placed") && said[0].contains("threshold"),
        "{said:?}"
    );
}

/// End to end: a threshold on a property along the well, crossed between two nodes, is a line across the
/// well at the MD where the interpolation puts it.
///
/// Break it: in `well_limits`, place every limit with `Anchor::Inside` whatever the caller asked.
#[test]
fn a_threshold_crossing_is_drawn_where_it_is_interpolated() {
    let classified = Classification {
        support: Support::Nodes {
            md: vec![0.0, 200.0, 520.0],
        },
        classes: vec![Some(Class::Below), Some(Class::Below), Some(Class::AtOrAbove)],
        threshold: Some(Threshold {
            at: 6.0,
            values: vec![Some(1.0), Some(3.0), Some(9.0)],
        }),
    };
    let (limits, refused) = well_limits(&transitions(&classified), LimitKind::Top, Anchor::Interpolated);
    assert!(refused.is_empty(), "{refused:?}");
    // 3 at MD 200, 9 at MD 520, threshold 6: half way.
    assert_eq!(limits[0].at, LimitAt::Md(360.0));
    let text = svg::write(&layout(&one_well(), &with_limits(limits)));
    assert!(group(&text, "limits").contains("data-limit-md=\"360\""), "{text}");
}

// -- the ramp legend -----------------------------------------------------------------------

fn red_ramp() -> RampLegend {
    let scale = Scale::new(ScaleKind::Linear, 0.0, 1.0, false).expect("a scale");
    ramp::sampled("Porosity", Some("v/v"), &scale, &|value| {
        Some([(value * 255.0).round() as u8, 0, 0, 255])
    })
}

fn texts(o: &LaidObject) -> Vec<&str> {
    o.shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// A section has one ramp legend, on the right: the property's name, its unit, the ramp from the lower
/// bound at the bottom to the upper at the top, graduated in round numbers.
///
/// Break it: in `ramp_legend`, put the first step at the top (`ramp_top + step as f64 * ramp / steps as f64`).
#[test]
fn the_ramp_legend_names_the_property_and_runs_from_low_at_the_bottom_to_high() {
    let v = View {
        ramp_legend: Some(red_ramp()),
        ..view()
    };
    let l = layout(&one_well(), &v);
    let layers: Vec<_> = l
        .layers
        .iter()
        .filter(|layer| layer.name == "ramp-legend")
        .collect();
    assert_eq!(layers.len(), 1, "one per section");
    assert_eq!(layers[0].objects.len(), 1);
    let o = &layers[0].objects[0];
    let said = texts(o);
    assert!(said.contains(&"Porosity") && said.contains(&"(v/v)"), "{said:?}");
    assert!(
        said.contains(&"0.0") && said.contains(&"1.0"),
        "graduated from 0 to 1, written as the view along a well writes its numbers: {said:?}"
    );

    // Swatches: the lowest step sits lowest on the drawing, the highest highest.
    let swatches: Vec<(&str, f64)> = o
        .shapes
        .iter()
        .skip(1)
        .filter_map(|s| match s {
            Shape::Polygon { points, fill, .. } => Some((
                fill.as_str(),
                points.iter().map(|p| p[1]).fold(f64::MIN, f64::max),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(swatches.len(), ramp::STEPS);
    let lowest = swatches
        .iter()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .expect("a swatch");
    assert_eq!(
        lowest.0,
        v.ramp_legend.as_ref().unwrap().colours[0].as_deref().unwrap()
    );

    // Its margin is reserved on the right of the plot, and it is an Inkscape layer of its own.
    let without = layout(&one_well(), &view());
    assert!(
        l.frame.w < without.frame.w,
        "the legend takes its room on the right"
    );
    let text = svg::write(&l);
    assert!(
        text.contains("id=\"ramp-legend\"") && text.contains("xmlns:inkscape"),
        "{text}"
    );
}

/// A ramp that cannot be graduated (no width, reversed bounds, no colour) is said and not drawn, and takes
/// no room.
///
/// Break it: in `ramp::scale_of`, build the scale over `legend.max + 1.0`.
#[test]
fn a_ramp_that_cannot_be_graduated_is_said_and_not_drawn() {
    let flat = RampLegend {
        min: 1.0,
        max: 1.0,
        ..red_ramp()
    };
    let l = layout(
        &one_well(),
        &View {
            ramp_legend: Some(flat),
            ..view()
        },
    );
    assert!(
        l.warnings.iter().any(|w| w.contains("ramp legend not drawn")),
        "{:?}",
        l.warnings
    );
    assert!(l.layers.iter().all(|layer| layer.name != "ramp-legend"));
    assert_eq!(
        l.frame.w,
        layout(&one_well(), &view()).frame.w,
        "no room reserved"
    );

    let empty = RampLegend {
        colours: Vec::new(),
        ..red_ramp()
    };
    let l = layout(
        &one_well(),
        &View {
            ramp_legend: Some(empty),
            ..view()
        },
    );
    assert!(
        l.warnings.iter().any(|w| w.contains("holds no colour")),
        "{:?}",
        l.warnings
    );
}

/// The ramp and its graduations are the one arithmetic of both legends: the value of a step is at its
/// middle, and a graduation is a fraction of the ramp with its text.
///
/// Break it: in `ramp::value_at_step`, take `step as f64 / STEPS as f64` (the step's low edge).
#[test]
fn the_steps_and_the_graduations_are_the_ramps_own() {
    let scale = Scale::new(ScaleKind::Linear, 0.0, 10.0, false).expect("a scale");
    assert_eq!(ramp::value_at_step(&scale, 0), 10.0 * 0.5 / ramp::STEPS as f64);
    assert_eq!(
        ramp::value_at_step(&scale, ramp::STEPS - 1),
        10.0 * (ramp::STEPS as f64 - 0.5) / ramp::STEPS as f64
    );
    let marks = ramp::graduations(&scale, 200.0);
    assert!(marks.iter().all(|(fraction, _)| (0.0..=1.0).contains(fraction)));
    assert!(
        marks
            .iter()
            .any(|(fraction, name)| *fraction == 0.0 && name == "0"),
        "{marks:?}"
    );
    assert!(
        marks
            .iter()
            .any(|(fraction, name)| *fraction == 1.0 && name == "10"),
        "{marks:?}"
    );
    // And the metadata carries a ramp legend and reads it back.
    let v = View {
        ramp_legend: Some(red_ramp()),
        ..view()
    };
    let back: View = serde_json::from_str(&serde_json::to_string(&v).unwrap()).unwrap();
    assert_eq!(back, v);
}
