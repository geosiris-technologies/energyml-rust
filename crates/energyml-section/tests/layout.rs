//! The layout, in numbers (ADR-0015): everything that can
//! be wrong in the drawing is a number here, and these tests state it on
//! hand-built catalogues — no package is read.

use energyml_section::layout::{Attr, Layout, Shape, layout};
use energyml_section::model::*;
use energyml_sheet::text;

pub fn id(n: &str, kind: &str) -> ObjectId {
    ObjectId {
        store: "s1".into(),
        uri: format!("eml:///resqml22.{kind}({n})"),
        uuid: format!("{n}-uuid"),
        title: n.to_string(),
        qualified_type: format!("resqml22.{kind}"),
    }
}

pub fn well(n: &str, s: f64, d: f64, side: Side) -> WellItem {
    WellItem {
        object: id(n, "WellboreFrameRepresentation"),
        s,
        d,
        side,
        head: [s, 0.0],
        path: vec![[s, 0.0], [s, -200.0], [s + 50.0, -500.0]],
        md: Some(vec![0.0, 200.0, 520.0]),
        intervals: vec![
            Interval { from: 0.0, to: 100.0, colour: Some("#aa0000".into()), unit: "Bartonian".into(), unit_uri: None },
            Interval { from: 100.0, to: 300.0, colour: None, unit: "Lutetian".into(), unit_uri: None },
        ],
        strata: None,
        merged: Vec::new(),
    }
}

pub fn trace(n: &str, spans: Vec<Vec<[f64; 2]>>) -> TraceItem {
    TraceItem { object: id(n, "TriangulatedSetRepresentation"), role: TraceRole::Horizon, spans, time: false, notes: Vec::new(), colour: None, age: None, feature: None, sources: Vec::new() }
}

pub fn catalogue(items: Vec<Item>) -> Catalogue {
    let section = Section {
        key: SectionKey::Long,
        trace: vec![[0.0, 0.0], [1000.0, 0.0]],
        length: 1000.0,
        marks: vec![
            Mark::Vertex { s: 0.0, index: 0 },
            Mark::Crossing { s: 400.0, other: SectionKey::Cross { index: 1 } },
            Mark::Vertex { s: 1000.0, index: 1 },
        ],
        band: Vec::new(),
        items,
        z_range: Some([-500.0, 0.0]),
    };
    Catalogue {
        format: FORMAT,
        inputs: Inputs {
            trace: vec![[500000.0, 6000000.0], [501000.0, 6000000.0]],
            system: Some(System { epsg: 2154, unit: "m".into() }),
            left: 100.0,
            right: 100.0,
            crosses: Crosses::None,
            objects: vec![Asked { store: "s1".into(), revision: 4, wells: vec!["w".into()], surfaces: Vec::new() }],
            time_factor: None,
        },
        long: section,
        crosses: Vec::new(),
        plan: Plan { origin: [500000.0, 6000000.0], system: None, sections: Vec::new(), bands: Vec::new(), wells: Vec::new() },
        objects: Vec::new(),
        notes: Vec::new(),
    }
}

pub fn view() -> View {
    View {
        target: Target::Section { key: SectionKey::Long },
        window: Some(Window { x: [0.0, 1000.0], y: [-500.0, 0.0] }),
        portion: None,
        size_px: [1084.0, 700.0],
        background: "#ffffff".into(),
        hidden: Vec::new(),
        colours: Vec::new(),
        well_colouring: WellColouring::Side,
        widths: Vec::new(),
        well_width_px: 4.0,
        fills: Vec::new(),
        show: Show { crossings: true, vertices: true, distances: true, names: true, rejected: true, bands: true, surface_names: true, notes: true, origin: true, direction: true },
        axis: AxisMode::Projected,
        highlight: Vec::new(),
        labels: energyml_section::model::LabelMode::Full,
        paints: Vec::new(), joints: Default::default(), zoom_script: true, strata_legend: false, strata_ticks: false, unit_colours: Vec::new(), draw_unstratified: true, unstratified: Default::default(), store_names: Vec::new(), markers: Vec::new(),
    }
}

fn two_wells() -> Catalogue {
    catalogue(vec![Item::Well(well("A", 150.0, 12.0, Side::Left)), Item::Well(well("B", 700.0, 30.0, Side::Right))])
}

fn object<'a>(l: &'a Layout, key: &str) -> Option<&'a energyml_section::layout::LaidObject> {
    l.layers.iter().flat_map(|x| x.objects.iter()).find(|o| matches!(&o.attr, Attr::Key(k) if k == key))
}

fn key(n: &str, kind: &str) -> String {
    id(n, kind).key()
}

fn px_x(l: &Layout, s: f64) -> f64 {
    l.frame.x + (s - l.window.x[0]) * l.scale[0]
}

fn xs(shape: &Shape) -> Vec<f64> {
    match shape {
        Shape::Polyline { points, .. } | Shape::Polygon { points, .. } => points.iter().map(|p| p[0]).collect(),
        _ => Vec::new(),
    }
}

#[test]
fn the_exaggeration_written_is_the_one_the_scales_give() {
    let l = layout(&two_wells(), &view());
    // From the frame and the window, not from `scale`: 500 m over frame.h pixels,
    // 1000 m over frame.w pixels.
    let expected = (l.frame.h / 500.0) / (l.frame.w / 1000.0);
    let got = l.exaggeration.expect("a section has one");
    assert!((got - expected).abs() < 1e-9, "{got} vs {expected}");
    assert!((l.scale[1] / l.scale[0] - got).abs() < 1e-9);
    // And it is what is printed: the three significant figures of the same number.
    let printed: f64 = l.exaggeration_text.trim_start_matches("vertical exaggeration ×").parse().expect("a number");
    assert!((printed - got).abs() / got < 0.01, "{} vs {got}", l.exaggeration_text);
}

#[test]
fn zooming_one_axis_changes_the_exaggeration_and_the_text() {
    let mut v = view();
    let before = layout(&two_wells(), &v);
    v.window = Some(Window { x: [0.0, 1000.0], y: [-250.0, 0.0] });
    let after = layout(&two_wells(), &v);
    let (a, b) = (before.exaggeration.unwrap(), after.exaggeration.unwrap());
    assert!((b / a - 2.0).abs() < 1e-9, "twice the zoom in z: {a} -> {b}");
    assert_ne!(before.exaggeration_text, after.exaggeration_text);
}

#[test]
fn the_plan_is_at_equal_scale_on_both_axes() {
    let mut cat = two_wells();
    cat.plan.sections = vec![(SectionKey::Long, vec![[0.0, 0.0], [1000.0, 100.0]])];
    let mut v = view();
    v.target = Target::Plan;
    v.window = None;
    let l = layout(&cat, &v);
    assert_eq!(l.scale[0], l.scale[1]);
    assert!(l.exaggeration.is_none());
    // The window really drawn has the frame's proportions: nothing stretched.
    let w = (l.window.x[1] - l.window.x[0]) * l.scale[0];
    let h = (l.window.y[1] - l.window.y[0]) * l.scale[1];
    assert!((w - l.frame.w).abs() < 1e-6 && (h - l.frame.h).abs() < 1e-6);
}

#[test]
fn a_portion_keeps_the_abscissas_and_only_what_falls_in_it() {
    let cat = catalogue(vec![
        Item::Well(well("A", 150.0, 5.0, Side::Left)),
        Item::Well(well("B", 700.0, 5.0, Side::Right)),
        Item::Well(well("E", 400.0, 5.0, Side::Right)),
        Item::Trace(trace("H", vec![vec![[0.0, -100.0], [1000.0, -100.0]]])),
    ]);
    let mut v = view();
    v.portion = Some([150.0, 400.0]);
    let l = layout(&cat, &v);
    // The window is still [0, 1000]: A is at 150 m, not at the start of the portion.
    let a = object(&l, &key("A", "WellboreFrameRepresentation")).expect("A is in the portion");
    assert_eq!(a.at, Some([150.0, 0.0]));
    let Shape::Circle { c, .. } = a.shapes.iter().find(|s| matches!(s, Shape::Circle { .. })).unwrap() else { unreachable!() };
    assert!((c[0] - px_x(&l, 150.0)).abs() < 1e-6, "the head is at 150 m of the unchanged axis");
    // Bounds included: a well exactly on the end belongs to the portion.
    assert!(object(&l, &key("E", "WellboreFrameRepresentation")).is_some());
    assert!(object(&l, &key("B", "WellboreFrameRepresentation")).is_none(), "700 m is outside");
    // The trace is cut at the bounds, not just hidden by a clip.
    let h = object(&l, &key("H", "TriangulatedSetRepresentation")).unwrap();
    let all: Vec<f64> = h.shapes.iter().flat_map(xs).collect();
    let (lo, hi) = (all.iter().cloned().fold(f64::INFINITY, f64::min), all.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
    assert!((lo - px_x(&l, 150.0)).abs() < 1e-6 && (hi - px_x(&l, 400.0)).abs() < 1e-6, "{lo} {hi}");
}

#[test]
fn a_hidden_object_is_absent_everywhere() {
    let cat = catalogue(vec![
        Item::Well(well("A", 150.0, 5.0, Side::Left)),
        Item::Trace(trace("H", vec![vec![[0.0, -100.0], [1000.0, -100.0]]])),
        Item::Trace(trace("L", vec![vec![[0.0, -300.0], [1000.0, -300.0]]])),
    ]);
    let mut v = view();
    v.fills = vec![FillChoice { key: key("H", "TriangulatedSetRepresentation"), up: false, down: true }];
    let shown = layout(&cat, &v);
    assert!(shown.layer("fills").unwrap().objects.len() == 1, "the control: H fills down to L");
    v.hidden = vec![key("L", "TriangulatedSetRepresentation"), key("A", "WellboreFrameRepresentation")];
    let l = layout(&cat, &v);
    assert!(object(&l, &key("L", "TriangulatedSetRepresentation")).is_none());
    assert!(object(&l, &key("A", "WellboreFrameRepresentation")).is_none());
    assert!(l.layer("labels").unwrap().objects.is_empty(), "no label for a hidden well");
    // A hidden line is not a neighbour: H has nothing to fill down to.
    assert!(l.layer("fills").unwrap().objects.is_empty());
}

#[test]
fn a_fill_stops_where_one_of_the_two_traces_stops() {
    let cat = catalogue(vec![
        Item::Trace(trace("H", vec![vec![[0.0, -100.0], [1000.0, -100.0]]])),
        Item::Trace(trace("L", vec![vec![[0.0, -300.0], [600.0, -300.0]]])),
    ]);
    let mut v = view();
    v.fills = vec![FillChoice { key: key("H", "TriangulatedSetRepresentation"), up: false, down: true }];
    let l = layout(&cat, &v);
    let fill = &l.layer("fills").unwrap().objects[0];
    assert_eq!(fill.attr, Attr::FillOf(key("H", "TriangulatedSetRepresentation")));
    let all: Vec<f64> = fill.shapes.iter().flat_map(xs).collect();
    let hi = all.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    assert!((hi - px_x(&l, 600.0)).abs() < 1e-6, "ends at 600 m, where L ends: {hi}");
    // The trace without a neighbour in a direction fills nothing there: no floor.
    v.fills = vec![FillChoice { key: key("L", "TriangulatedSetRepresentation"), up: false, down: true }];
    assert!(layout(&cat, &v).layer("fills").unwrap().objects.is_empty());
    v.fills = vec![FillChoice { key: key("H", "TriangulatedSetRepresentation"), up: true, down: false }];
    assert!(layout(&cat, &v).layer("fills").unwrap().objects.is_empty());
}

#[test]
fn a_fill_is_cut_at_a_crossing_instead_of_twisting_through_it() {
    // H is flat at -100; X goes from -50 to -150 and crosses it at 500 m.
    // Down from H, X is the nearest line below only from 500 to 1000; before the
    // crossing it is above H, so the nearest below is L. One polygon per
    // neighbour, cut at the crossing — none twisting through it.
    let cat = catalogue(vec![
        Item::Trace(trace("H", vec![vec![[0.0, -100.0], [1000.0, -100.0]]])),
        Item::Trace(trace("X", vec![vec![[0.0, -50.0], [1000.0, -150.0]]])),
        Item::Trace(trace("L", vec![vec![[0.0, -400.0], [1000.0, -400.0]]])),
    ]);
    let mut v = view();
    v.fills = vec![FillChoice { key: key("H", "TriangulatedSetRepresentation"), up: false, down: true }];
    let l = layout(&cat, &v);
    let found = filled(&l);
    assert_eq!(found.len(), 2, "{found:?}");
    let (first, second) = (&found[0], &found[1]);
    assert!(near(first.0, 0.0) && near(first.1, 500.0), "L from 0 to the crossing: {first:?}");
    assert!(near(second.0, 500.0) && near(second.1, 1000.0), "X from the crossing: {second:?}");
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// The polygons of the `fills` layer, taken back to `(s, z)`.
fn fill_polygons(l: &Layout) -> Vec<Vec<[f64; 2]>> {
    let mut out = Vec::new();
    for object in &l.layer("fills").unwrap().objects {
        for shape in &object.shapes {
            if let Shape::Polygon { points, .. } = shape {
                let poly: Vec<[f64; 2]> = points
                    .iter()
                    .map(|p| {
                        [
                            l.window.x[0] + (p[0] - l.frame.x) / l.scale[0],
                            l.window.y[0] + (l.frame.y + l.frame.h - p[1]) / l.scale[1],
                        ]
                    })
                    .collect();
                out.push(poly);
            }
        }
    }
    out.sort_by(|a, b| s_range(a).0.total_cmp(&s_range(b).0));
    out
}

fn s_range(poly: &[[f64; 2]]) -> (f64, f64) {
    let lo = poly.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
    let hi = poly.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
    (lo, hi)
}

/// The `[s0, s1]` of each filled polygon, in order.
fn filled(l: &Layout) -> Vec<(f64, f64)> {
    fill_polygons(l).iter().map(|p| s_range(p)).collect()
}

fn z_range(poly: &[[f64; 2]]) -> (f64, f64) {
    let lo = poly.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let hi = poly.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
    (lo, hi)
}

const TS: &str = "TriangulatedSetRepresentation";

/// A trace of a role and a feature (`None`: the file names none).
pub fn surface(n: &str, role: TraceRole, feature: Option<&str>, spans: Vec<Vec<[f64; 2]>>) -> Item {
    Item::Trace(TraceItem { role, feature: feature.map(str::to_string), ..trace(n, spans) })
}

fn down(n: &str) -> View {
    let mut v = view();
    v.fills = vec![FillChoice { key: key(n, TS), up: false, down: true }];
    v
}

/// A fault that cuts the interval between a horizon and the one below it used to
/// be that horizon's neighbour — its mean gap was tiny where it stood — and the
/// fill stopped at it (the fill must reach the faults but not run between
/// horizon lines).
#[test]
fn an_oblique_fault_is_never_a_neighbour() {
    let cat = catalogue(vec![
        surface("H", TraceRole::Horizon, Some("fh"), vec![vec![[0.0, -100.0], [1000.0, -100.0]]]),
        surface("F", TraceRole::Fault, Some("ff"), vec![vec![[200.0, -50.0], [600.0, -350.0]]]),
        surface("L", TraceRole::Horizon, Some("fl"), vec![vec![[0.0, -400.0], [1000.0, -400.0]]]),
    ]);
    let l = layout(&cat, &down("H"));
    let found = fill_polygons(&l);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(near(s_range(&found[0]).0, 0.0) && near(s_range(&found[0]).1, 1000.0), "the whole width: {found:?}");
    assert!(near(z_range(&found[0]).0, -400.0), "down to L, not to the fault: {found:?}");
}

#[test]
fn a_fault_is_never_filled_whatever_the_view_says() {
    let cat = catalogue(vec![
        surface("F", TraceRole::Fault, None, vec![vec![[0.0, -100.0], [1000.0, -100.0]]]),
        surface("L", TraceRole::Horizon, None, vec![vec![[0.0, -400.0], [1000.0, -400.0]]]),
    ]);
    let mut v = down("F");
    assert!(layout(&cat, &v).layer("fills").unwrap().objects.is_empty());
    v.fills = vec![FillChoice { key: key("F", TS), up: true, down: true }];
    assert!(layout(&cat, &v).layer("fills").unwrap().objects.is_empty());
    // The control: the same line as a horizon is filled.
    let cat = catalogue(vec![
        surface("F", TraceRole::Horizon, None, vec![vec![[0.0, -100.0], [1000.0, -100.0]]]),
        surface("L", TraceRole::Horizon, None, vec![vec![[0.0, -400.0], [1000.0, -400.0]]]),
    ]);
    assert_eq!(layout(&cat, &down("F")).layer("fills").unwrap().objects.len(), 1);
}

/// Two versions of one horizon — the file says so by their feature — are never
/// each other's neighbour: a band 0.01 m high is not the unit below.
#[test]
fn two_versions_of_one_horizon_are_not_neighbours_but_two_features_are() {
    let versions = |feature_of_copy: &str| {
        catalogue(vec![
            surface("H", TraceRole::Horizon, Some("feat-1"), vec![vec![[0.0, -100.0], [1000.0, -100.0]]]),
            surface("H2", TraceRole::Horizon, Some(feature_of_copy), vec![vec![[0.0, -100.01], [1000.0, -100.01]]]),
            surface("L", TraceRole::Horizon, Some("feat-3"), vec![vec![[0.0, -300.0], [1000.0, -300.0]]]),
        ])
    };
    // Same feature (whatever their interpretations): H goes to L.
    let found = fill_polygons(&layout(&versions("feat-1"), &down("H")));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(near(z_range(&found[0]).0, -300.0), "down to L: {found:?}");
    // The negative control: two features that happen to touch are neighbours.
    let found = fill_polygons(&layout(&versions("feat-2"), &down("H")));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(near(z_range(&found[0]).0, -100.01), "H2 is the nearest line below: {found:?}");
    // No readable feature, no grouping: nothing is guessed from the geometry.
    let unnamed = catalogue(vec![
        surface("H", TraceRole::Horizon, None, vec![vec![[0.0, -100.0], [1000.0, -100.0]]]),
        surface("H2", TraceRole::Horizon, None, vec![vec![[0.0, -100.01], [1000.0, -100.01]]]),
        surface("L", TraceRole::Horizon, Some("feat-3"), vec![vec![[0.0, -300.0], [1000.0, -300.0]]]),
    ]);
    let found = fill_polygons(&layout(&unnamed, &down("H")));
    assert!(near(z_range(&found[0]).0, -100.01), "{found:?}");
}

/// The neighbour is chosen in each abscissa, not once for the whole trace.
#[test]
fn the_neighbour_is_the_nearest_line_in_each_abscissa() {
    let cat = catalogue(vec![
        surface("H", TraceRole::Horizon, Some("a"), vec![vec![[0.0, -100.0], [1000.0, -100.0]]]),
        surface("A", TraceRole::Horizon, Some("b"), vec![vec![[0.0, -200.0], [600.0, -200.0]]]),
        surface("B", TraceRole::Horizon, Some("c"), vec![vec![[0.0, -300.0], [1000.0, -300.0]]]),
    ]);
    let found = fill_polygons(&layout(&cat, &down("H")));
    assert_eq!(found.len(), 2, "{found:?}");
    let (r0, r1) = (s_range(&found[0]), s_range(&found[1]));
    assert!(near(r0.0, 0.0) && near(r0.1, 600.0) && near(z_range(&found[0]).0, -200.0), "A on [0, 600]: {found:?}");
    assert!(near(r1.0, 600.0) && near(r1.1, 1000.0) && near(z_range(&found[1]).0, -300.0), "B on [600, 1000]: {found:?}");
}

/// A horizon cut by a fault is two spans that touch with different elevations:
/// the fill follows each, so it follows the throw.
#[test]
fn a_fill_follows_the_throw_of_a_faulted_horizon() {
    let cat = catalogue(vec![
        surface(
            "H",
            TraceRole::Horizon,
            Some("a"),
            vec![vec![[0.0, -100.0], [500.0, -100.0]], vec![[500.0, -120.0], [1000.0, -120.0]]],
        ),
        surface("L", TraceRole::Horizon, Some("b"), vec![vec![[0.0, -400.0], [1000.0, -400.0]]]),
    ]);
    let found = fill_polygons(&layout(&cat, &down("H")));
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(near(z_range(&found[0]).1, -100.0) && near(s_range(&found[0]).1, 500.0), "{found:?}");
    assert!(near(z_range(&found[1]).1, -120.0) && near(s_range(&found[1]).0, 500.0), "{found:?}");
}

#[test]
fn strata_colour_the_intervals_and_a_missing_colour_is_the_wells_never_transparent() {
    let cat = two_wells();
    let mut v = view();
    v.well_colouring = WellColouring::Strata;
    v.colours = vec![(key("A", "WellboreFrameRepresentation"), "#123456".into())];
    let l = layout(&cat, &v);
    let a = object(&l, &key("A", "WellboreFrameRepresentation")).unwrap();
    assert_eq!(a.intervals.len(), 2);
    assert_eq!(a.intervals[0].colour, "#aa0000");
    assert_eq!(a.intervals[0].unit, "Bartonian");
    assert_eq!(a.intervals[1].colour, "#123456", "no colour of its own: the well's");
    // B has no scene colour: the neutral one, still a colour.
    let b = object(&l, &key("B", "WellboreFrameRepresentation")).unwrap();
    assert!(b.intervals.iter().all(|i| i.colour.starts_with('#')));
    // The piece of line of an interval is cut at its MDs: 0..100 of 0..200 is half the first segment.
    let first = &a.intervals[0].points;
    let (y0, y1) = (first.first().unwrap()[1], first.last().unwrap()[1]);
    let per_m = l.scale[1];
    assert!(((y1 - y0) - 100.0 * per_m).abs() < 1e-6, "100 m of a vertical segment: {}", y1 - y0);
}

#[test]
fn the_colour_of_a_well_follows_the_choice() {
    let cat = two_wells();
    let mut v = view();
    let colour = |l: &Layout, n: &str| {
        let o = object(l, &key(n, "WellboreFrameRepresentation")).unwrap();
        o.shapes.iter().find_map(|s| if let Shape::Polyline { stroke, width, .. } = s { Some((stroke.clone(), *width)) } else { None }).unwrap()
    };
    let by_side = layout(&cat, &v);
    assert_ne!(colour(&by_side, "A").0, colour(&by_side, "B").0, "the two sides differ");
    assert_eq!(colour(&by_side, "A").1, 4.0, "the width is the view's");
    v.well_colouring = WellColouring::Scene;
    v.colours = vec![(key("A", "WellboreFrameRepresentation"), "#00ff00".into())];
    v.well_width_px = 7.0;
    let scene = layout(&cat, &v);
    assert_eq!(colour(&scene, "A"), ("#00ff00".to_string(), 7.0));
}

#[test]
fn labels_do_not_overlap_and_what_does_not_fit_is_folded_and_counted() {
    // Twelve wells within 30 m of one another: at 1 m per 1.08 px they cannot all be named.
    let items: Vec<Item> =
        (0..12).map(|i| Item::Well(well(&format!("Well-number-{i}"), 400.0 + i as f64 * 2.0, 10.0, Side::Left))).collect();
    let l = layout(&catalogue(items), &view());
    let labels = &l.layer("labels").unwrap().objects;
    assert!(l.folded_labels > 0, "twelve names cannot fit in 25 px");
    assert_eq!(labels.len() + l.folded_labels, 12, "every label is either placed or counted");
    assert!(l.warnings.iter().any(|w| w.contains(&format!("{} well labels folded", l.folded_labels))));
    // On one level (same baseline), the boxes, measured with the sheet's table, never overlap.
    let mut boxes: Vec<(f64, f64, f64)> = Vec::new();
    for o in labels {
        for s in &o.shapes {
            if let Shape::Text { at, text: t, size, .. } = s {
                let w = text::string_width_pt(t, *size);
                boxes.push((at[1], at[0] - w / 2.0, at[0] + w / 2.0));
            }
        }
    }
    for (i, a) in boxes.iter().enumerate() {
        for b in &boxes[i + 1..] {
            if (a.0 - b.0).abs() < 1.0 {
                assert!(a.2 <= b.1 || b.2 <= a.1, "{a:?} overlaps {b:?}");
            }
        }
    }
}

#[test]
fn wells_far_apart_are_all_named_with_their_distance_and_side() {
    let l = layout(&two_wells(), &view());
    assert_eq!(l.folded_labels, 0);
    let texts: Vec<String> = l
        .layer("labels")
        .unwrap()
        .objects
        .iter()
        .flat_map(|o| o.shapes.iter())
        .filter_map(|s| if let Shape::Text { text, .. } = s { Some(text.clone()) } else { None })
        .collect();
    assert!(texts.contains(&"A".to_string()) && texts.contains(&"12 m L".to_string()), "{texts:?}");
    assert!(texts.contains(&"30 m R".to_string()));
    let mut v = view();
    v.show.distances = false;
    assert_eq!(layout(&two_wells(), &v).layer("labels").unwrap().objects.len(), 2);
}

#[test]
fn the_ticks_are_round_numbers_inside_the_window() {
    for (a, b) in [(0.0, 1000.0), (123.0, 187.0), (0.0, 0.37), (-5300.0, -4100.0)] {
        let mut v = view();
        v.window = Some(Window { x: [a, b], y: [-500.0, 0.0] });
        let l = layout(&two_wells(), &v);
        let ticks = &l.x_axis.ticks;
        assert!(ticks.len() >= 2, "{a}..{b}: {}", ticks.len());
        let step = ticks[1].value - ticks[0].value;
        let mantissa = step / 10f64.powf(step.log10().floor());
        assert!([1.0, 2.0, 5.0].iter().any(|m| (mantissa - m).abs() < 1e-6), "{a}..{b}: step {step}");
        for t in ticks {
            assert!(t.value >= a - 1e-9 && t.value <= b + 1e-9);
            assert!(((t.value / step).round() * step - t.value).abs() < 1e-9, "a multiple of the step");
            assert!((t.at - px_x(&l, t.value)).abs() < 1e-6);
        }
    }
}

#[test]
fn a_geodesic_axis_falls_back_and_says_so_and_coordinates_are_drawn() {
    let mut v = view();
    v.axis = AxisMode::Geodesic;
    let l = layout(&two_wells(), &v);
    assert_eq!(l.x_axis.mode, AxisMode::Projected);
    assert!(l.warnings.iter().any(|w| w.contains("geodesic")));
    v.axis = AxisMode::Coordinates;
    let l = layout(&two_wells(), &v);
    assert_eq!(l.x_axis.mode, AxisMode::Coordinates);
    let t = l.x_axis.ticks.iter().find(|t| t.value == 0.0).unwrap();
    assert_eq!(t.sublabel.as_deref(), Some("0, 0"), "the trace of the fixture runs along y = 0 from the origin");
}

#[test]
fn what_the_drawing_owes_is_printed() {
    let mut cat = two_wells();
    cat.objects = vec![
        ObjectRow {
            object: id("Z", "TriangulatedSetRepresentation"),
            kind: RowKind::Surface,
            outcome: Outcome::Refused { reason: "unreadable".into() },
            fillable: false,
        },
        ObjectRow { object: id("M", "TriangulatedSetRepresentation"), kind: RowKind::Surface, outcome: Outcome::Missed, fillable: true },
    ];
    cat.plan.wells = vec![PlanWell {
        object: id("F", "WellboreFrameRepresentation"),
        head: [5000.0, 5000.0],
        feet: Vec::new(),
        rejected: Some("too far".into()),
    }];
    let mut t = trace("T", vec![vec![[0.0, -100.0], [1000.0, -100.0]]]);
    t.time = true;
    cat.long.items.push(Item::Trace(t));
    cat.inputs.time_factor = Some(2.5);
    let l = layout(&cat, &view());
    let all = l.warnings.join("\n");
    assert!(all.contains("1 objects refused"), "{all}");
    assert!(all.contains("1 wells set aside"), "{all}");
    assert!(all.contains("in time") && all.contains("2.5"), "{all}");
}

#[test]
fn marks_follow_the_flags_and_the_portion() {
    let mut v = view();
    let marks = |l: &Layout| l.layer("marks").unwrap().objects.iter().flat_map(|o| o.shapes.iter()).filter(|s| matches!(s, Shape::Text { .. })).count();
    assert_eq!(marks(&layout(&two_wells(), &v)), 3, "two vertices and a crossing");
    v.show.vertices = false;
    assert_eq!(marks(&layout(&two_wells(), &v)), 1);
    v.show.crossings = false;
    assert_eq!(marks(&layout(&two_wells(), &v)), 0);
    v.show = Show { crossings: true, vertices: true, ..v.show };
    v.portion = Some([100.0, 500.0]);
    assert_eq!(marks(&layout(&two_wells(), &v)), 1, "only the crossing at 400 m is inside [100, 500]");
}

#[test]
fn the_draw_order_is_the_documented_one_and_highlight_is_marked() {
    let mut v = view();
    v.highlight = vec![key("A", "WellboreFrameRepresentation")];
    let l = layout(&two_wells(), &v);
    let names: Vec<&str> = l.layers.iter().map(|x| x.name).collect();
    assert_eq!(names, ["envelopes", "fills", "outlines", "traces", "wells", "marks", "labels", "legend"]);
    assert!(object(&l, &key("A", "WellboreFrameRepresentation")).unwrap().highlighted);
    assert!(!object(&l, &key("B", "WellboreFrameRepresentation")).unwrap().highlighted);
}

#[test]
fn the_metadata_hold_the_inputs_and_the_view() {
    let cat = two_wells();
    let v = view();
    let l = layout(&cat, &v);
    assert_eq!(l.metadata.format, FORMAT);
    assert_eq!(l.metadata.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(l.metadata.inputs, cat.inputs);
    assert_eq!(l.metadata.view, v);
    let json = serde_json::to_string(&l.metadata).unwrap();
    let back: energyml_section::layout::Metadata = serde_json::from_str(&json).unwrap();
    assert_eq!(back, l.metadata);
}

#[test]
fn a_surface_takes_the_scene_colour_then_the_file_colour_then_its_role_and_carries_its_name() {
    let mut from_file = trace("H1", vec![vec![[0.0, -100.0], [800.0, -100.0]], vec![[900.0, -120.0], [1000.0, -120.0]]]);
    from_file.colour = Some("#aa5500".to_string());
    let plain = trace("H2", vec![vec![[0.0, -300.0], [1000.0, -300.0]]]);
    let cat = catalogue(vec![Item::Trace(from_file), Item::Trace(plain)]);
    let strokes = |l: &Layout, n: &str| -> Vec<String> {
        object(l, &key(n, "TriangulatedSetRepresentation"))
            .unwrap()
            .shapes
            .iter()
            .filter_map(|s| if let Shape::Polyline { stroke, .. } = s { Some(stroke.clone()) } else { None })
            .collect()
    };
    let texts = |l: &Layout, n: &str| -> Vec<(String, String)> {
        object(l, &key(n, "TriangulatedSetRepresentation"))
            .unwrap()
            .shapes
            .iter()
            .filter_map(|s| if let Shape::Text { text, fill, .. } = s { Some((text.clone(), fill.clone())) } else { None })
            .collect()
    };

    let l = layout(&cat, &view());
    assert!(strokes(&l, "H1").iter().all(|c| c == "#aa5500"), "the file's colour");
    assert!(strokes(&l, "H2").iter().all(|c| c != "#aa5500"), "no file colour: the role's");
    // One name per surface, at the end of its longest span, in its colour.
    assert_eq!(texts(&l, "H1"), vec![("H1".to_string(), "#aa5500".to_string())]);

    // The scene's colour wins over the file's.
    let mut v = view();
    v.colours = vec![(key("H1", "TriangulatedSetRepresentation"), "#123456".to_string())];
    assert!(strokes(&layout(&cat, &v), "H1").iter().all(|c| c == "#123456"));

    // Names can be switched off.
    v.show.surface_names = false;
    assert!(texts(&layout(&cat, &v), "H1").is_empty());
}

// ---------------------------------------------------------------------------
// Names without overlap, and codes with a legend

/// Every text of a layer, with the box it covers: (text, left, top, right, bottom).
fn text_boxes(l: &Layout, layer: &str) -> Vec<(String, [f64; 4])> {
    l.layer(layer)
        .unwrap()
        .objects
        .iter()
        .flat_map(|o| o.shapes.iter())
        .filter_map(|s| match s {
            Shape::Text { at, text: t, anchor, size, .. } => {
                let w = text::string_width_pt(t, *size);
                let x0 = match anchor {
                    energyml_section::layout::Anchor::Start => at[0],
                    energyml_section::layout::Anchor::Middle => at[0] - w / 2.0,
                    energyml_section::layout::Anchor::End => at[0] - w,
                };
                Some((t.clone(), [x0, at[1] - size, x0 + w, at[1]]))
            }
            _ => None,
        })
        .collect()
}

fn overlap(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

#[test]
fn two_surface_names_ending_together_do_not_sit_on_each_other() {
    // Two lines a metre apart, both ending at s = 1000: their names would be
    // written in the same place.
    let cat = catalogue(vec![
        Item::Trace(trace("H3 — top of the Ypresian", vec![vec![[0.0, -100.0], [1000.0, -100.0]]])),
        Item::Trace(trace("H4 — top of the Sparnacian", vec![vec![[0.0, -101.0], [1000.0, -101.0]]])),
    ]);
    let l = layout(&cat, &view());
    let names = text_boxes(&l, "traces");
    assert_eq!(names.len(), 2, "both named: {names:?}");
    assert!(!overlap(names[0].1, names[1].1), "{names:?}");
}

#[test]
fn two_wells_side_by_side_keep_their_labels_apart_and_a_moved_one_points_home() {
    let cat = catalogue(vec![Item::Well(well("BSS000NZNM", 500.0, 4.0, Side::Right)), Item::Well(well("BSS000NZNP", 505.0, 5.0, Side::Right))]);
    let l = layout(&cat, &view());
    let labels = text_boxes(&l, "labels");
    assert_eq!(labels.len(), 4, "a name and a distance each: {labels:?}");
    for (i, a) in labels.iter().enumerate() {
        for b in labels.iter().skip(i + 1) {
            assert!(!overlap(a.1, b.1), "{a:?} on {b:?}");
        }
    }
    // The one that had to move has a dotted line back to its head.
    let leaders = l
        .layer("labels")
        .unwrap()
        .objects
        .iter()
        .flat_map(|o| o.shapes.iter())
        .filter(|s| matches!(s, Shape::Polyline { dash: true, .. }))
        .count();
    assert_eq!(leaders, 1);
}

#[test]
fn in_codes_mode_objects_are_named_by_code_with_a_legend_and_keep_their_code_everywhere() {
    let mut cat = catalogue(vec![
        Item::Well(well("A", 150.0, 12.0, Side::Left)),
        Item::Well(well("B", 700.0, 30.0, Side::Right)),
        Item::Trace(trace("Top", vec![vec![[0.0, -100.0], [1000.0, -100.0]]])),
        Item::Trace(trace("Base", vec![vec![[0.0, -300.0], [1000.0, -300.0]]])),
    ]);
    cat.plan.wells = vec![PlanWell { object: id("B", "WellboreFrameRepresentation"), head: [10.0, 10.0], feet: Vec::new(), rejected: None }];
    let mut v = view();
    v.labels = LabelMode::Codes;
    let l = layout(&cat, &v);

    let names: Vec<String> = text_boxes(&l, "labels").into_iter().map(|t| t.0).collect();
    assert!(names.contains(&"W0".to_string()) && names.contains(&"W1".to_string()), "{names:?}");
    // Horizons top first.
    let lines: Vec<String> = text_boxes(&l, "traces").into_iter().map(|t| t.0).collect();
    assert_eq!(lines, vec!["H0".to_string(), "H1".to_string()]);
    // The legend says what each code is.
    let legend: Vec<String> = text_boxes(&l, "legend").into_iter().map(|t| t.0).collect();
    for said in ["Legend", "W0", "A", "W1", "B", "H0", "Top", "H1", "Base"] {
        assert!(legend.contains(&said.to_string()), "{said} missing from {legend:?}");
    }
    // The legend takes its room: the plot is narrower than with full names.
    assert!(l.frame.w < layout(&cat, &view()).frame.w);

    // The plan gives B the code the section gave it.
    let mut p = v.clone();
    p.target = Target::Plan;
    p.window = None;
    let plan_names: Vec<String> = text_boxes(&layout(&cat, &p), "labels").into_iter().map(|t| t.0).collect();
    assert_eq!(plan_names, vec!["W1".to_string()]);
}

/// A catalogue with five things to say under the drawing.
fn talkative() -> Catalogue {
    let mut cat = two_wells();
    cat.objects = (0..3)
        .map(|i| ObjectRow {
            object: id(&format!("Z{i}"), "TriangulatedSetRepresentation"),
            kind: RowKind::Surface,
            outcome: Outcome::Refused { reason: "unreadable".into() },
            fillable: false,
        })
        .collect();
    cat.plan.wells = vec![PlanWell {
        object: id("F", "WellboreFrameRepresentation"),
        head: [5000.0, 5000.0],
        feet: Vec::new(),
        rejected: Some("too far".into()),
    }];
    let mut t = trace("T", vec![vec![[0.0, -100.0], [1000.0, -100.0]]]);
    t.time = true;
    cat.long.items.push(Item::Trace(t));
    cat.inputs.time_factor = Some(2.5);
    cat
}

/// The top-view plot must not be squashed by the legends and the padding. On a small drawing the plot keeps half the height: the notes
/// are folded first, into a line that counts them, then the band of labels is
/// lowered. On a large one nothing is folded.
#[test]
fn a_small_drawing_keeps_half_its_height_for_the_plot_and_counts_the_notes_it_folds() {
    let mut v = view();
    v.axis = AxisMode::Geodesic; // one note more
    let large = layout(&talkative(), &v);
    assert!(large.warnings.len() >= 4, "{:?}", large.warnings);
    assert_eq!(large.written_warnings(), large.warnings, "a large drawing writes them all");

    v.size_px = [800.0, 250.0];
    let small = layout(&talkative(), &v);
    assert!(small.frame.h >= 125.0, "the plot keeps half of 250 px: {}", small.frame.h);
    let written = small.written_warnings();
    assert!(written.len() < small.warnings.len(), "{written:?}");
    let last = written.last().expect("a line says the notes exist");
    let folded = small.warnings.len() - (written.len() - 1);
    let said = if written.len() == 1 { format!("{folded} notes — not shown at this size") } else { format!("+ {folded} more notes — not shown at this size") };
    assert_eq!(last, &said);
    // What is written fits between the plot and the bottom of the drawing.
    assert!(small.frame.y + small.frame.h + 46.0 + 14.0 * (written.len() as f64 - 1.0) <= 250.0);
}

/// The plan band of the front end sits above its section, which says the same
/// notes: a view without notes writes none and gives their room to the plot.
#[test]
fn a_view_without_notes_writes_none_and_its_plot_is_taller() {
    let mut v = view();
    v.target = Target::Plan;
    v.window = None;
    let mut cat = talkative();
    cat.plan.sections = vec![(SectionKey::Long, vec![[0.0, 0.0], [1000.0, 100.0]])];
    let with = layout(&cat, &v);
    assert!(!with.written_warnings().is_empty());
    v.show.notes = false;
    let without = layout(&cat, &v);
    assert!(without.written_warnings().is_empty());
    assert!(without.frame.h > with.frame.h, "{} ≤ {}", without.frame.h, with.frame.h);
    // The plan's labels are inside its frame: no band of 72 px above it.
    assert!(without.frame.y <= 40.0, "{}", without.frame.y);
}

/// A crossing at the end of the trace writes its text on the left of its line:
/// on the right it would run out of the plot and be cut by the clip.
#[test]
fn a_mark_at_the_end_of_the_trace_keeps_its_text_inside_the_plot() {
    let mut cat = two_wells();
    cat.long.marks.push(Mark::Crossing { s: 1000.0, other: SectionKey::Cross { index: 7 } });
    let l = layout(&cat, &view());
    let right = l.frame.x + l.frame.w;
    for (text, [x0, _, x1, _]) in text_boxes(&l, "marks") {
        assert!(x1 <= right + 0.5 && x0 >= l.frame.x - 0.5, "{text} runs out: {x0}..{x1} in ..{right}");
    }
}

/// A surface's name never sits on the text of a mark: both are inside the plot,
/// and "H1 — base × cross 1 Quaternary" read as one.
#[test]
fn a_surface_name_keeps_clear_of_the_marks_texts() {
    // A horizon running out right under the crossing at 400 m, at the top of the plot.
    let mut cat = two_wells();
    cat.long.items.push(Item::Trace(trace("A-rather-long-horizon-name", vec![vec![[0.0, -5.0], [430.0, -5.0]]])));
    let l = layout(&cat, &view());
    let marks = text_boxes(&l, "marks");
    let names = text_boxes(&l, "traces");
    assert!(!names.is_empty(), "the name is placed");
    for (name, b) in &names {
        for (mark, m) in &marks {
            assert!(!overlap(*b, *m), "{name} sits on {mark}");
        }
    }
}

/// In the plan, a well's name never covers a head — the heads are what a plan
/// is read for.
#[test]
fn a_plan_label_never_covers_a_head() {
    let mut cat = two_wells();
    cat.plan.sections = vec![(SectionKey::Long, vec![[0.0, 0.0], [1000.0, 0.0]])];
    cat.plan.wells = (0..8)
        .map(|i| PlanWell {
            object: id(&format!("Well-{i}"), "WellboreFrameRepresentation"),
            head: [400.0 + 15.0 * i as f64, 10.0 * (i % 3) as f64],
            feet: Vec::new(),
            rejected: None,
        })
        .collect();
    let mut v = view();
    v.target = Target::Plan;
    v.window = None;
    v.size_px = [800.0, 300.0];
    let l = layout(&cat, &v);
    let heads: Vec<[f64; 2]> = l
        .layer("wells")
        .unwrap()
        .objects
        .iter()
        .flat_map(|o| o.shapes.iter())
        .filter_map(|s| if let Shape::Circle { c, .. } = s { Some(*c) } else { None })
        .collect();
    assert_eq!(heads.len(), 8);
    let labels = text_boxes(&l, "labels");
    assert!(!labels.is_empty());
    for (text, b) in &labels {
        for h in &heads {
            assert!(!overlap(*b, [h[0] - 3.5, h[1] - 3.5, h[0] + 3.5, h[1] + 3.5]), "{text} covers the head at {h:?}");
        }
    }
}

// Marks' texts without overlap

/// `n` crossings, `px` pixels apart on the drawing, from the first fifth of the trace.
fn crowded(n: usize, px: f64) -> (Catalogue, View) {
    let mut cat = two_wells();
    let v = view();
    let scale = layout(&cat, &v).scale[0];
    cat.long.marks = (1..=n)
        .map(|i| Mark::Crossing { s: 200.0 + i as f64 * px / scale, other: SectionKey::Cross { index: i as u32 } })
        .collect();
    (cat, v)
}

fn mark_lines(l: &Layout) -> usize {
    l.layer("marks").unwrap().objects.iter().flat_map(|o| o.shapes.iter()).filter(|s| matches!(s, Shape::Polyline { .. })).count()
}

#[test]
fn crowded_marks_write_no_text_on_another_and_stay_in_the_frame() {
    let (cat, v) = crowded(9, 20.0);
    let l = layout(&cat, &v);
    let boxes = text_boxes(&l, "marks");
    assert!(boxes.len() >= 5, "most marks still name themselves: {}", boxes.len());
    for (i, (a, ba)) in boxes.iter().enumerate() {
        assert!(ba[0] >= l.frame.x && ba[2] <= l.frame.x + l.frame.w, "{a} leaves the frame");
        assert!(ba[1] >= l.frame.y && ba[3] <= l.frame.y + l.frame.h + 2.0, "{a} leaves the frame");
        for (b, bb) in &boxes[i + 1..] {
            assert!(!overlap(*ba, *bb), "{a} sits on {b}");
        }
    }
}

#[test]
fn the_short_form_is_used_when_the_long_one_does_not_fit() {
    // Whole texts first, in every place; the short form only once none is left.
    // Six crossings in one spot take the six whole places (right and left, three
    // levels); a seventh, 30 px from the end of the plot, has no whole text on
    // its right (it would run out) and none on its left (the stack is there) —
    // but its short form fits on the right.
    let mut cat = two_wells();
    let v = view();
    let l0 = layout(&cat, &v);
    let s_at = |px: f64| l0.window.x[0] + (px - l0.frame.x) / l0.scale[0];
    let right = l0.frame.x + l0.frame.w;
    let mut marks: Vec<Mark> = (1..=6).map(|i| Mark::Crossing { s: s_at(right - 82.0), other: SectionKey::Cross { index: i } }).collect();
    marks.push(Mark::Crossing { s: s_at(right - 30.0), other: SectionKey::Cross { index: 9 } });
    cat.long.marks = marks;
    let texts: Vec<String> = text_boxes(&layout(&cat, &v), "marks").into_iter().map(|(t, _)| t).collect();
    assert_eq!(texts.iter().filter(|t| t.starts_with("× cross ")).count(), 6, "{texts:?}");
    assert!(texts.contains(&"×9".to_string()), "the seventh is short: {texts:?}");
}

#[test]
fn a_mark_with_no_place_keeps_its_line_and_is_counted() {
    let (cat, v) = crowded(60, 3.0);
    let l = layout(&cat, &v);
    let texts = text_boxes(&l, "marks").len();
    assert_eq!(mark_lines(&l), 60, "every line is drawn");
    assert!(texts < 60, "not all can be named");
    let missing = 60 - texts;
    let said = format!("{missing} section marks unlabelled");
    assert!(l.warnings.iter().any(|w| w.contains(&said)), "{missing} not counted in {:?}", l.warnings);
}

#[test]
fn a_wide_drawing_keeps_every_mark_where_it_was() {
    let l = layout(&two_wells(), &view());
    let at = |t: &str| {
        l.layer("marks").unwrap().objects.iter().flat_map(|o| o.shapes.iter()).find_map(|s| match s {
            Shape::Text { at, text, anchor, .. } if text == t => Some((*at, matches!(anchor, energyml_section::layout::Anchor::End))),
            _ => None,
        })
    };
    let bottom = l.frame.y + l.frame.h - 10.0;
    assert_eq!(at("V0"), Some(([px_x(&l, 0.0) + 3.0, bottom], false)));
    assert_eq!(at("× cross 1"), Some(([px_x(&l, 400.0) + 3.0, l.frame.y + 12.0], false)));
    assert_eq!(at("V1"), Some(([px_x(&l, 1000.0) - 3.0, bottom], true)), "the one at the end of the trace is on the left");
    assert!(!l.warnings.iter().any(|w| w.contains("marks unlabelled")));
}

/// A short text on its own level before a long one stacked into the plot: a
/// crossing near the right edge, whose long text fits neither side at the top,
/// writes "×8" at the top rather than "× cross 8" a level down, over the data.
#[test]
fn a_mark_takes_its_short_form_before_going_down_a_level() {
    let mut cat = two_wells();
    cat.long.marks.push(Mark::Crossing { s: 900.0, other: SectionKey::Cross { index: 7 } });
    cat.long.marks.push(Mark::Crossing { s: 960.0, other: SectionKey::Cross { index: 8 } });
    let l = layout(&cat, &view());
    let marks = text_boxes(&l, "marks");
    let seven = marks.iter().find(|(t, _)| t == "× cross 7").expect("the first keeps its whole text");
    let eight = marks.iter().find(|(t, _)| t.contains('8')).expect("the second is labelled");
    assert_eq!(eight.0, "×8", "{marks:?}");
    assert!((eight.1[3] - seven.1[3]).abs() < 1e-9, "on the same level: {eight:?} vs {seven:?}");
}

/// Everything with a coloured background is behind every line: a grid's fill
/// and a surface's fill come before the grid's outline, which comes before the
/// surfaces' traces and the wells.
#[test]
fn every_fill_is_behind_every_line() {
    let mut cat = two_wells();
    let span = vec![[0.0, -100.0], [1000.0, -100.0]];
    cat.long.items.push(Item::Trace(trace("H", vec![span.clone()])));
    cat.long.items.push(Item::Trace(trace("L", vec![vec![[0.0, -300.0], [1000.0, -300.0]]])));
    cat.long.items.push(Item::Envelope(EnvelopeItem {
        object: id("G", "IjkGridRepresentation"),
        top: vec![vec![[0.0, -50.0], [1000.0, -50.0]]],
        bottom: vec![vec![[0.0, -400.0], [1000.0, -400.0]]],
        fills: vec![vec![[0.0, -50.0], [1000.0, -50.0], [1000.0, -400.0], [0.0, -400.0]]],
        faults: Vec::new(),
        time: false,
        colour: None,
    }));
    let mut v = view();
    v.fills = vec![FillChoice { key: key("H", "TriangulatedSetRepresentation"), up: false, down: true }];
    let l = layout(&cat, &v);
    // In the order the layers are written: the last polygon before the first line.
    let mut last_polygon = None;
    let mut first_line = None;
    for (at, shape) in l.layers.iter().filter(|x| x.name != "marks" && x.name != "labels" && x.name != "legend").flat_map(|x| x.objects.iter()).flat_map(|o| o.shapes.iter()).enumerate() {
        match shape {
            Shape::Polygon { .. } => last_polygon = Some(at),
            Shape::Polyline { .. } if first_line.is_none() => first_line = Some(at),
            _ => {}
        }
    }
    let (last_polygon, first_line) = (last_polygon.expect("the grid and H fill"), first_line.expect("lines are drawn"));
    assert!(last_polygon < first_line, "a fill ({last_polygon}) is drawn over a line ({first_line})");
    // The grid is still one object for the front end: its key is on its outline, its fill is a fill.
    assert_eq!(l.keys().iter().filter(|k| k.contains("(G)")).count(), 1);
}

