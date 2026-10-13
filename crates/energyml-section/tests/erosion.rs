//! Erosions suspected from the surfaces' ages (ADR-0015), on traces built by hand — no package is read, except the
//! interpretations the age reader is shown, built in memory.

use energyml_section::erosion::{self, detect};
use energyml_section::layout::{Layout, Shape, layout};
use energyml_section::model::*;

use crate::layout::{catalogue, id, trace, view};

/// A horizon with an age, over the given points.
fn dated(n: &str, age: Option<[f64; 2]>, points: Vec<[f64; 2]>) -> TraceItem {
    TraceItem { age, ..trace(n, vec![points]) }
}

/// The old horizon, flat at -200, and a young one that dips below it between
/// s = 200 and s = 600 (it crosses -200 at those two abscissas exactly).
fn dipping() -> (TraceItem, TraceItem) {
    (
        dated("Old", Some([30.0, 30.0]), vec![[0.0, -200.0], [1000.0, -200.0]]),
        dated("Young", Some([10.0, 10.0]), vec![[0.0, -100.0], [400.0, -300.0], [800.0, -100.0], [1000.0, -100.0]]),
    )
}

fn with_notes(items: Vec<TraceItem>) -> Catalogue {
    let mut cat = catalogue(items.into_iter().map(Item::Trace).collect());
    cat.notes = erosion::notes(&cat);
    cat
}

fn layer<'a>(l: &'a Layout, name: &str) -> Option<&'a energyml_section::layout::Layer> {
    l.layers.iter().find(|x| x.name == name)
}

#[test]
fn an_order_the_ages_respect_gives_nothing() {
    // The younger one lies above the older one everywhere: nothing to say.
    let old = dated("Old", Some([30.0, 30.0]), vec![[0.0, -200.0], [1000.0, -200.0]]);
    let young = dated("Young", Some([10.0, 10.0]), vec![[0.0, -100.0], [1000.0, -150.0]]);
    assert!(detect(SectionKey::Long, &[&old, &young]).is_empty());
}

#[test]
fn the_portion_where_the_younger_passes_below_is_exact() {
    let (old, young) = dipping();
    let found = detect(SectionKey::Long, &[&old, &young]);
    assert_eq!(found.len(), 1, "{found:?}");
    let p = &found[0];
    assert!((p.s0 - 200.0).abs() < 1e-9 && (p.s1 - 600.0).abs() < 1e-9, "[{}, {}]", p.s0, p.s1);
    assert_eq!(p.older.title, "Old");
    assert_eq!(p.younger.title, "Young");
    // The order of the two in the list does not change who is older.
    let swapped = detect(SectionKey::Long, &[&young, &old]);
    assert_eq!((swapped[0].older.title.as_str(), swapped[0].s0, swapped[0].s1), ("Old", p.s0, p.s1));
}

#[test]
fn two_stretches_are_two_portions_and_a_gap_is_not_bridged() {
    // Below on [100, 300], above on [300, 500], below again on [500, 700].
    let old = dated("Old", Some([30.0, 30.0]), vec![[0.0, -200.0], [1000.0, -200.0]]);
    let young = dated(
        "Young",
        Some([10.0, 10.0]),
        vec![[0.0, -150.0], [100.0, -200.0], [200.0, -250.0], [300.0, -200.0], [400.0, -150.0], [500.0, -200.0], [600.0, -250.0], [700.0, -200.0], [800.0, -150.0]],
    );
    let found: Vec<(f64, f64)> = detect(SectionKey::Long, &[&old, &young]).iter().map(|p| (p.s0, p.s1)).collect();
    assert_eq!(found.len(), 2, "{found:?}");
    assert!((found[0].0 - 100.0).abs() < 1e-9 && (found[0].1 - 300.0).abs() < 1e-9);
    assert!((found[1].0 - 500.0).abs() < 1e-9 && (found[1].1 - 700.0).abs() < 1e-9);

    // Where the younger line does not exist, nothing is judged: a span that
    // stops at s = 200 and one that restarts at 300 leave no portion between.
    let cut = TraceItem {
        spans: vec![vec![[0.0, -100.0], [200.0, -300.0]], vec![[300.0, -300.0], [400.0, -100.0]]],
        ..dated("Young", Some([10.0, 10.0]), Vec::new())
    };
    let found = detect(SectionKey::Long, &[&old, &cut]);
    assert!(found.iter().all(|p| p.s1 <= 200.0 + 1e-9 || p.s0 >= 300.0 - 1e-9), "{found:?}");
}

#[test]
fn no_age_no_overlap_no_fault_no_judgement() {
    let (old, young) = dipping();
    let undated = TraceItem { age: None, ..young.clone() };
    assert!(detect(SectionKey::Long, &[&old, &undated]).is_empty(), "no age is the rule, not an error");
    assert!(detect(SectionKey::Long, &[&undated, &undated.clone()]).is_empty());
    // Overlapping spans of age order nothing; neither do two equal ages.
    let overlapping = TraceItem { age: Some([20.0, 5.0]), ..old.clone() };
    let inside = TraceItem { age: Some([15.0, 10.0]), ..young.clone() };
    assert!(detect(SectionKey::Long, &[&overlapping, &inside]).is_empty());
    let same = TraceItem { age: old.age, ..young.clone() };
    assert!(detect(SectionKey::Long, &[&old, &same]).is_empty());
    // A fault does not stack.
    let fault = TraceItem { role: TraceRole::Fault, ..young };
    assert!(detect(SectionKey::Long, &[&old, &fault]).is_empty());
}

#[test]
fn a_disjoint_span_orders_the_pair() {
    // [40, 35] Ma is older than [20, 10] Ma whatever the points of the span.
    let (old, young) = dipping();
    let old = TraceItem { age: Some([40.0, 35.0]), ..old };
    let young = TraceItem { age: Some([20.0, 10.0]), ..young };
    assert_eq!(detect(SectionKey::Long, &[&old, &young]).len(), 1);
}

#[test]
fn the_note_names_the_two_surfaces_the_portion_and_the_proposal() {
    let (old, young) = dipping();
    let cat = with_notes(vec![old, young]);
    assert_eq!(cat.notes.len(), 1);
    let note = &cat.notes[0];
    assert_eq!(note.code, "erosion");
    assert_eq!(note.sections, vec![SectionKey::Long]);
    assert_eq!(note.objects.iter().map(|o| o.title.as_str()).collect::<Vec<_>>(), vec!["Old", "Young"]);
    for wanted in ["`Old`", "`Young`", "s = 200.0", "s = 600.0", "should stop against", "30 Ma", "10 Ma", "nothing was changed"] {
        assert!(note.said.contains(wanted), "`{wanted}` is not in: {}", note.said);
    }
}

#[test]
fn assemble_carries_the_age_and_the_notes() {
    use energyml_section::assemble::assemble;
    let spans = |points: Vec<[f64; 2]>| CutShape::Trace { spans: vec![points] };
    let read = |n: &str, age: Option<[f64; 2]>, points: Vec<[f64; 2]>| SurfaceRead {
        object: id(n, "TriangulatedSetRepresentation"),
        role: TraceRole::Horizon,
        time: false,
        colour: None,
        age,
        feature: None,
        cuts: vec![SurfaceCut { section: SectionKey::Long, shape: spans(points), notes: Vec::new(), sources: Vec::new() }],
    };
    let inputs = Inputs {
        trace: vec![[0.0, 0.0], [1000.0, 0.0]],
        system: None,
        left: 20.0,
        right: 20.0,
        crosses: Crosses::None,
        objects: Vec::new(),
        time_factor: None,
    };
    let (old, young) = dipping();
    let contribution = Contribution {
        store: "s1".to_string(),
        surfaces: vec![read("Old", old.age, old.spans[0].clone()), read("Young", young.age, young.spans[0].clone())],
        ..Contribution::default()
    };
    let cat = assemble(&inputs, &[contribution]).unwrap();
    let ages: Vec<Option<[f64; 2]>> = cat
        .long
        .items
        .iter()
        .filter_map(|i| if let Item::Trace(t) = i { Some(t.age) } else { None })
        .collect();
    assert_eq!(ages, vec![Some([30.0, 30.0]), Some([10.0, 10.0])]);
    assert_eq!(cat.notes.iter().filter(|n| n.code == "erosion").count(), 1);
}

#[test]
fn a_document_written_before_ages_still_reads() {
    let (old, _) = dipping();
    let mut json = serde_json::to_value(&old).unwrap();
    json.as_object_mut().unwrap().remove("age");
    let back: TraceItem = serde_json::from_value(json).unwrap();
    assert_eq!(back.age, None);
}

#[test]
fn the_portion_is_marked_behind_the_lines_with_its_own_colour() {
    let (old, young) = dipping();
    let cat = with_notes(vec![old, young]);
    let l = layout(&cat, &view());
    let names: Vec<&str> = l.layers.iter().map(|x| x.name).collect();
    let marks = names.iter().position(|n| *n == "erosions").expect("the erosions have a layer");
    assert!(marks < names.iter().position(|n| *n == "traces").unwrap(), "a background goes behind the lines: {names:?}");
    let group = &layer(&l, "erosions").unwrap().objects;
    assert_eq!(group.len(), 1);
    let px = |s: f64| l.frame.x + (s - l.window.x[0]) * l.scale[0];
    let Shape::Polygon { points, fill, .. } = &group[0].shapes[0] else { panic!("a polygon first: {:?}", group[0].shapes) };
    let (lo, hi) = points.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p[0]), b.max(p[0])));
    assert!((lo - px(200.0)).abs() < 0.5 && (hi - px(600.0)).abs() < 0.5, "[{lo}, {hi}] vs [{}, {}]", px(200.0), px(600.0));
    // Its colour is its own: no surface of the section has it.
    for t in &layer(&l, "traces").unwrap().objects {
        for shape in &t.shapes {
            if let Shape::Polyline { stroke, .. } = shape {
                assert_ne!(stroke, fill);
            }
        }
    }
}

#[test]
fn the_warning_counts_what_the_catalogue_noticed_about_the_section() {
    let (old, young) = dipping();
    let mut cat = with_notes(vec![old, young]);
    // One more erosion note, and a crossing mismatch the other module writes.
    let again = cat.notes[0].clone();
    cat.notes.push(again);
    cat.notes.push(Note { code: "crossingMismatch".into(), sections: vec![SectionKey::Long], objects: Vec::new(), said: "x".into() });
    // A note about a cross section does not count on the long one.
    cat.notes.push(Note { code: "erosion".into(), sections: vec![SectionKey::Cross { index: 0 }], objects: Vec::new(), said: "y".into() });
    let l = layout(&cat, &view());
    assert!(l.warnings.contains(&"2 erosions suspected — see the notes".to_string()), "{:?}", l.warnings);
    assert!(l.warnings.contains(&"1 crossing mismatch — see the notes".to_string()), "{:?}", l.warnings);
}

#[test]
fn without_a_note_the_drawing_is_what_it_was() {
    // Ages that the drawing's order respects, or no age at all: the same layout
    // as the catalogue before this lot, and no layer, no warning.
    let old = dated("Old", Some([30.0, 30.0]), vec![[0.0, -200.0], [1000.0, -200.0]]);
    let young = dated("Young", Some([10.0, 10.0]), vec![[0.0, -100.0], [1000.0, -150.0]]);
    let dated_cat = with_notes(vec![old.clone(), young.clone()]);
    assert!(dated_cat.notes.is_empty());
    let plain = catalogue(vec![Item::Trace(TraceItem { age: None, ..old }), Item::Trace(TraceItem { age: None, ..young })]);
    let (a, b) = (layout(&dated_cat, &view()), layout(&plain, &view()));
    assert_eq!(a, b);
    assert!(layer(&a, "erosions").is_none());
    assert!(!a.warnings.iter().any(|w| w.contains("erosion")));
}

#[test]
fn a_hidden_surface_marks_nothing() {
    let (old, young) = dipping();
    let cat = with_notes(vec![old, young.clone()]);
    let mut hidden = view();
    hidden.hidden = vec![young.object.key()];
    assert!(layer(&layout(&cat, &hidden), "erosions").is_none());
}

#[test]
fn the_age_a_horizon_interpretation_states_is_read_as_it_is() {
    use energyml_core::value::Reflect;
    use energyml_resqml_2_2::HorizonInterpretation;
    use energyml_section::read::surfaces::age_of;
    let age = |h: &HorizonInterpretation| age_of(h as &dyn Reflect);
    let time = |years: i64| energyml_eml_2_3::GeologicTime { age_offset_attribute: Some(years), date_time: "2000-01-01T00:00:00Z".into() };

    // Nothing stated: nothing invented.
    assert_eq!(age(&HorizonInterpretation::default()), None);
    // A point.
    let point = HorizonInterpretation { absolute_age: Some(time(10_000_000)), ..Default::default() };
    assert_eq!(age(&point), Some([10.0, 10.0]));
    // A span, whichever order the file wrote it in.
    let span = HorizonInterpretation { older_possible_age: Some(20_000_000), younger_possible_age: Some(5_000_000), ..Default::default() };
    assert_eq!(age(&span), Some([20.0, 5.0]));
    // Half a span is not an age.
    let half = HorizonInterpretation { older_possible_age: Some(20_000_000), ..Default::default() };
    assert_eq!(age(&half), None);
    // An interval of time wins over the rest.
    let interval = HorizonInterpretation {
        has_occurred_during: Some(energyml_resqml_2_2::AbstractTimeInterval::GeologicTimeBasedTimeInterval(
            energyml_resqml_2_2::GeologicTimeBasedTimeInterval { start: time(30_000_000), end: time(25_000_000) },
        )),
        absolute_age: Some(time(10_000_000)),
        ..Default::default()
    };
    assert_eq!(age(&interval), Some([30.0, 25.0]));
}
