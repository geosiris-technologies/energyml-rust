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

// -- the lines between strata, over any colour ---------------------------------------------

/// The units a well shows give a line across it where one gives way to the next: the shared fixture has
/// Bartonian from MD 0 to 100 and Lutetian from 100 to 300, so one line at MD 100, the first of its class, between
/// the two units **by their titles**. A hole between two intervals gives none (no limit crosses a hole).
///
/// Break it: in `strata_limits`, give every interval the same class (`Class::Unit(String::new())`): no unit changes
/// and no line is drawn.
#[test]
fn the_strata_give_a_line_where_one_unit_gives_way_to_the_next_and_none_over_a_hole() {
    let mut a = well("A", 150.0, 12.0, Side::Left);
    let (limits, unplaced) = energyml_section::limits::strata_limits(&a);
    assert!(unplaced.is_empty(), "{unplaced:?}");
    assert_eq!(limits.len(), 1, "{limits:?}");
    assert_eq!(limits[0].at, LimitAt::Md(100.0));
    assert_eq!(limits[0].rank, 1);
    assert_eq!(limits[0].above.as_deref(), Some("Bartonian"));
    assert_eq!(limits[0].below.as_deref(), Some("Lutetian"));

    // A third unit that starts after a hole: nothing crosses the hole.
    a.intervals.push(Interval {
        from: 350.0,
        to: 400.0,
        colour: None,
        unit: "Priabonian".into(),
        unit_uri: None,
    });
    let (again, _) = energyml_section::limits::strata_limits(&a);
    assert_eq!(again.len(), 1, "no line over the hole: {again:?}");

    // And they are drawn over a well painted by the scene: the paint does not take them away.
    let drawn = svg::write(&layout(
        &catalogue(vec![Item::Well(a.clone())]),
        &View {
            well_colouring: WellColouring::Scene,
            paints: vec![(
                key_of("A"),
                Paint {
                    nodes: vec!["#00ff00ff".into(), "#ff0000ff".into(), "#0000ffff".into()],
                    blend: Blend::Net,
                },
            )],
            limits: vec![(key_of("A"), again)],
            ..view()
        },
    ));
    assert!(
        group(&drawn, "limits").contains("data-limit-md=\"100\""),
        "{drawn}"
    );
}

// -- the legend of categories -----------------------------------------------------------------

/// A section has one legend of categories, on the right: the property's name, then a swatch and a label per category,
/// each its own object, in a group of its own. Nothing is drawn for no category, and it is said.
///
/// Break it: in `category_legend`, take the first entry's colour for every swatch.
#[test]
fn the_legend_of_categories_lists_each_value_painted_with_its_colour() {
    let legend = CategoryLegend {
        title: "Strati".to_string(),
        entries: vec![
            CategoryEntry {
                label: "Lutetian".to_string(),
                colour: "#ff0000".to_string(),
            },
            CategoryEntry {
                label: "Bartonian".to_string(),
                colour: "#00ff00".to_string(),
            },
        ],
    };
    let l = layout(
        &one_well(),
        &View {
            category_legend: Some(legend.clone()),
            ..view()
        },
    );
    let layer = l
        .layers
        .iter()
        .find(|layer| layer.name == "category-legend")
        .expect("one layer");
    let entries: Vec<&LaidObject> = layer
        .objects
        .iter()
        .filter(|o| o.kind == "category-entry")
        .collect();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].title, "Lutetian");
    assert!(texts(entries[0]).contains(&"Lutetian"));
    let fill = |o: &LaidObject| {
        o.shapes.iter().find_map(|s| match s {
            Shape::Polygon { fill, .. } => Some(fill.clone()),
            _ => None,
        })
    };
    assert_eq!(fill(entries[0]).as_deref(), Some("#ff0000"));
    assert_eq!(fill(entries[1]).as_deref(), Some("#00ff00"));
    let text = svg::write(&l);
    assert!(text.contains("id=\"category-legend\""), "{text}");

    // The metadata carries it and reads it back.
    let v = View {
        category_legend: Some(legend),
        ..view()
    };
    let back: View = serde_json::from_str(&serde_json::to_string(&v).unwrap()).unwrap();
    assert_eq!(back, v);

    // No category: nothing is drawn, and the drawing says why.
    let empty = layout(
        &one_well(),
        &View {
            category_legend: Some(CategoryLegend {
                title: "x".into(),
                entries: Vec::new(),
            }),
            ..view()
        },
    );
    assert!(
        empty.warnings.iter().any(|w| w.contains("holds no category")),
        "{:?}",
        empty.warnings
    );
}

// -- the wells that carry a paint are drawn over the others ---------------------------------------

/// The frames of one wellbore lie at one place; the well that carries the paint is drawn **after** the ones that do not,
/// whatever their order in the catalogue - none is dropped, and a well drawn in its own colour does not hide the paint.
///
/// Break it: in `layout`, draw `wells` in the catalogue's order (drop the two `filter`s of the loop over the wells).
#[test]
fn a_painted_well_is_drawn_after_the_wells_at_the_same_place() {
    // A comes first in the catalogue and carries the paint; B, at the same place, has none.
    let catalogue = catalogue(vec![
        Item::Well(well("A", 150.0, 12.0, Side::Left)),
        Item::Well(well("B", 150.0, 12.0, Side::Left)),
    ]);
    let painted = View {
        well_colouring: WellColouring::Scene,
        paints: vec![(
            key_of("A"),
            Paint {
                nodes: vec!["#00ff00ff".into(), "#ff0000ff".into(), "#0000ffff".into()],
                blend: Blend::Net,
            },
        )],
        ..view()
    };
    let l = layout(&catalogue, &painted);
    let layer = l
        .layers
        .iter()
        .find(|layer| layer.name == "wells")
        .expect("the wells");
    let at = |name: &str| {
        layer
            .objects
            .iter()
            .position(|o| matches!(&o.attr, Attr::Key(k) if *k == key_of(name)))
            .expect("the well is drawn")
    };
    assert_eq!(layer.objects.len(), 2, "no well is dropped");
    assert!(at("B") < at("A"), "the painted well comes last, over the other");

    // With no paint, the catalogue's order is kept.
    let plain = layout(&catalogue, &view());
    let layer = plain
        .layers
        .iter()
        .find(|layer| layer.name == "wells")
        .expect("the wells");
    let first = layer
        .objects
        .iter()
        .position(|o| matches!(&o.attr, Attr::Key(k) if *k == key_of("A")));
    assert_eq!(first, Some(0));
}

// -- the wells the scene paints nothing along keep their strata -------------------------------------

fn painted_a(strata_for_unpainted: bool) -> View {
    View {
        well_colouring: WellColouring::Scene,
        strata_for_unpainted,
        strata_legend: true,
        paints: vec![(
            key_of("A"),
            Paint {
                nodes: vec!["#00ff00ff".into(), "#ff0000ff".into(), "#0000ffff".into()],
                blend: Blend::Net,
            },
        )],
        ..view()
    }
}

fn two_wells() -> Catalogue {
    catalogue(vec![
        Item::Well(well("A", 150.0, 12.0, Side::Left)),
        Item::Well(well("B", 400.0, 12.0, Side::Left)),
    ])
}

/// In a view that paints one well and asks that the others keep their strata, the second is drawn in the colours of its
/// units (the shared fixture's Bartonian is `#aa0000`) and the strata legend lists them; without the request it is drawn in
/// the scene's colour, as it always was.
///
/// Break it: in `strata_for`, answer `false` under `WellColouring::Scene` whatever the view asks.
#[test]
fn an_unpainted_well_keeps_its_strata_beside_a_painted_one() {
    let intervals_of = |l: &Layout, name: &str| -> Vec<String> {
        drawn(l, &key_of(name))
            .intervals
            .iter()
            .map(|iv| iv.colour.clone())
            .collect()
    };

    let kept = layout(&two_wells(), &painted_a(true));
    assert!(
        intervals_of(&kept, "B").iter().any(|colour| colour == "#aa0000"),
        "{:?}",
        intervals_of(&kept, "B")
    );
    let legend = kept
        .layers
        .iter()
        .find(|layer| layer.name == "strata-legend")
        .expect("the legend of the strata the unpainted well shows");
    assert!(
        legend.objects.iter().any(|o| o.title == "Bartonian"),
        "{:?}",
        legend.objects.iter().map(|o| &o.title).collect::<Vec<_>>()
    );

    let flat = layout(&two_wells(), &painted_a(false));
    assert!(
        intervals_of(&flat, "B").iter().all(|colour| colour != "#aa0000"),
        "{:?}",
        intervals_of(&flat, "B")
    );
    assert!(
        flat.layers
            .iter()
            .find(|layer| layer.name == "strata-legend")
            .is_none_or(|layer| layer.objects.is_empty()),
        "no strata legend: nothing is coloured by strata"
    );
}

/// The request is a field of the view that is **not written** when it is off - a drawing made without it keeps its bytes and
/// a metadata without it reads as off - and is read back when it is on.
///
/// Break it: in `View`, drop `skip_serializing_if` on `strata_for_unpainted`.
#[test]
fn the_request_is_written_only_when_made_and_read_back() {
    let off = serde_json::to_string(&view()).unwrap();
    assert!(!off.contains("strataForUnpainted"), "{off}");
    let back: View = serde_json::from_str(&off).unwrap();
    assert!(!back.strata_for_unpainted);

    let on = View {
        strata_for_unpainted: true,
        ..view()
    };
    let text = serde_json::to_string(&on).unwrap();
    assert!(text.contains("\"strataForUnpainted\":true"), "{text}");
    let back: View = serde_json::from_str(&text).unwrap();
    assert_eq!(back, on);
}

// -- a stretch the paint has no value for is not an absent well --------------------------------------

/// A well whose property covers only its lower part: the stretch above the first coloured node is drawn **in the colour of the
/// view's wells with no paint** (never left empty), the lower one in the paint. With `strata_for_unpainted`, that
/// stretch takes the well's strata where it has them.
///
/// Break it: in `laid_well`, leave out the polylines laid under the stretches `paint::gaps` names.
#[test]
fn a_stretch_with_no_value_keeps_the_line_of_an_unpainted_well() {
    let cat = catalogue(vec![Item::Well(well("A", 150.0, 12.0, Side::Left))]);
    // Node 0 (MD 0) has no value; nodes 1 and 2 are green.
    let paint = Paint {
        nodes: vec!["#00000000".into(), "#00ff00ff".into(), "#00ff00ff".into()],
        blend: Blend::Net,
    };
    let scene = |strata_for_unpainted| View {
        well_colouring: WellColouring::Scene,
        strata_for_unpainted,
        paints: vec![(key_of("A"), paint.clone())],
        ..view()
    };
    let polylines = |l: &Layout| -> Vec<(String, usize)> {
        drawn(l, &key_of("A"))
            .shapes
            .iter()
            .filter_map(|shape| match shape {
                Shape::Polyline { stroke, points, .. } => Some((stroke.clone(), points.len())),
                _ => None,
            })
            .collect()
    };

    // What an unpainted well of the same view is drawn with.
    let bare = layout(
        &cat,
        &View {
            paints: Vec::new(),
            ..scene(false)
        },
    );
    let neutral = polylines(&bare)
        .first()
        .expect("the line of an unpainted well")
        .0
        .clone();

    let l = layout(&cat, &scene(false));
    assert!(
        polylines(&l)
            .iter()
            .any(|(stroke, points)| *stroke == neutral && *points == 2),
        "the stretch with no value: {:?}",
        polylines(&l)
    );
    assert!(
        drawn(&l, &key_of("A"))
            .intervals
            .iter()
            .any(|iv| iv.colour.starts_with("#00ff00")),
        "the lower part is painted"
    );

    // Keeping the strata: the same stretch is drawn in the colour of the unit it crosses (the fixture's Bartonian, MD 0-100).
    let l = layout(&cat, &scene(true));
    assert!(
        drawn(&l, &key_of("A"))
            .intervals
            .iter()
            .any(|iv| iv.unit == "Bartonian" && iv.colour == "#aa0000"),
        "{:?}",
        drawn(&l, &key_of("A"))
            .intervals
            .iter()
            .map(|iv| (&iv.unit, &iv.colour))
            .collect::<Vec<_>>()
    );
}
