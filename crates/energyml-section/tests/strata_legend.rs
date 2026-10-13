//! The legend of the wells' stratigraphy (an optional colour legend for the
//! boreholes' stratigraphy, on the left of the section drawings, in a group of
//! its own so that the user can easily move it when editing the SVG with
//! Inkscape, for instance).
//!
//! The legend is confronted with the **drawing's own wells**: the colours it
//! shows are those of the strata paths in the same SVG, never a constant.

use crate::layout::{catalogue, id, view};
use energyml_section::layout::layout;
use energyml_section::model::*;
use energyml_section::svg;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

pub fn interval(from: f64, to: f64, colour: Option<&str>, unit: &str) -> Interval {
    Interval { from, to, colour: colour.map(str::to_string), unit: unit.to_string(), unit_uri: None }
}

pub fn well_with(n: &str, s: f64, intervals: Vec<Interval>) -> Item {
    Item::Well(WellItem {
        object: id(n, "WellboreFrameRepresentation"),
        s,
        d: 10.0,
        side: Side::Left,
        head: [s, 0.0],
        path: vec![[s, 0.0], [s, -200.0], [s + 50.0, -500.0]],
        md: Some(vec![0.0, 200.0, 520.0]),
        intervals,
        strata: None,
        merged: Vec::new(),
    })
}

/// Two wells that share the middle of the column: A sees Top, Mid, Bottom, B sees
/// Mid, Bottom, Deep — and a unit with no colour of its own.
pub fn catalogue_of_two() -> Catalogue {
    catalogue(vec![
        well_with(
            "A",
            150.0,
            vec![
                interval(0.0, 100.0, Some("#aa0000"), "Top"),
                interval(100.0, 200.0, Some("#00aa00"), "Mid"),
                interval(200.0, 300.0, Some("#0000aa"), "Bottom"),
                interval(300.0, 350.0, None, "Colourless"),
            ],
        ),
        well_with(
            "B",
            700.0,
            vec![
                interval(0.0, 150.0, Some("#00aa00"), "Mid"),
                interval(150.0, 300.0, Some("#0000aa"), "Bottom"),
                interval(300.0, 400.0, Some("#ffcc00"), "Deep"),
            ],
        ),
    ])
}

pub fn on() -> View {
    let mut v = view();
    v.well_colouring = WellColouring::Strata;
    v.strata_legend = true;
    v
}

#[derive(Debug, Clone)]
pub struct El {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    /// Names of the ancestors' `id` or `data-zoom`, outermost first.
    pub path: Vec<String>,
    pub text: String,
}

impl El {
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
}

pub fn elements(svg: &str) -> Vec<El> {
    let mut reader = Reader::from_str(svg);
    let mut out: Vec<El> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    loop {
        match reader.read_event().expect("well-formed XML") {
            ev @ (Event::Start(_) | Event::Empty(_)) => {
                let (e, empty): (&BytesStart, bool) = match &ev {
                    Event::Start(e) => (e, false),
                    Event::Empty(e) => (e, true),
                    _ => unreachable!(),
                };
                let attrs: Vec<(String, String)> = e
                    .attributes()
                    .map(|a| {
                        let a = a.expect("an attribute");
                        (
                            String::from_utf8_lossy(a.key.as_ref()).into_owned(),
                            a.normalized_value(quick_xml::XmlVersion::default()).expect("a value").into_owned(),
                        )
                    })
                    .collect();
                let mut path: Vec<String> = stack.last().map(|&i| out[i].path.clone()).unwrap_or_default();
                if let Some(&i) = stack.last() {
                    let p = &out[i];
                    if let Some(id) = p.attr("id") {
                        path.push(format!("id={id}"));
                    }
                    if let Some(z) = p.attr("data-zoom") {
                        path.push(format!("zoom={z}"));
                    }
                    if let Some(z) = p.attr("data-layer") {
                        path.push(format!("layer={z}"));
                    }
                    if let Some(z) = p.attr("data-kind") {
                        path.push(format!("kind={z}"));
                    }
                }
                out.push(El { name: String::from_utf8_lossy(e.name().as_ref()).into_owned(), attrs, path, text: String::new() });
                if !empty {
                    stack.push(out.len() - 1);
                }
            }
            Event::Text(t) => {
                if let Some(&i) = stack.last() {
                    out[i].text.push_str(&t.decode().expect("text"));
                }
            }
            Event::End(_) => {
                stack.pop();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    out
}

pub fn inside(e: &El, what: &str) -> bool {
    e.path.iter().any(|p| p == what)
}

/// `(unit, colour)` of every strata path of the drawing's wells, in the order drawn.
pub fn strata_of_the_wells(els: &[El]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (i, e) in els.iter().enumerate() {
        if e.name == "g" && e.attr("data-unit").is_some() && inside(e, "layer=wells") {
            let line = els[i + 1..].iter().find(|x| x.name == "polyline").expect("a path under the interval");
            out.push((e.attr("data-unit").unwrap().to_string(), line.attr("stroke").unwrap().to_string()));
        }
    }
    out
}

/// `(title, swatch colour)` of every entry of the legend, in the order written.
pub fn entries_of_the_legend(els: &[El]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (i, e) in els.iter().enumerate() {
        if e.name == "g" && e.attr("data-kind") == Some("strata-entry") && inside(e, "id=strata-legend") {
            let swatch = els[i + 1..].iter().find(|x| x.name == "polygon").expect("a swatch");
            let title = els[i + 1..].iter().find(|x| x.name == "text").expect("a title");
            out.push((title.text.clone(), swatch.attr("fill").unwrap().to_string()));
        }
    }
    out
}

#[test]
fn the_legend_shows_the_units_the_wells_show_in_the_colours_the_wells_are_drawn_in() {
    let svg = svg::write(&layout(&catalogue_of_two(), &on()));
    let els = elements(&svg);
    let wells = strata_of_the_wells(&els);
    assert!(!wells.is_empty(), "the control: the wells' strata are in the drawing");
    let legend = entries_of_the_legend(&els);

    // Exactly the units drawn with a colour of their own, each once, each with the
    // colour of its paths in the same SVG.
    let mut expected: Vec<(String, String)> = Vec::new();
    for (unit, colour) in &wells {
        if unit != "Colourless" && !expected.contains(&(unit.clone(), colour.clone())) {
            expected.push((unit.clone(), colour.clone()));
        }
    }
    let mut sorted_legend = legend.clone();
    sorted_legend.sort();
    expected.sort();
    assert_eq!(sorted_legend, expected, "the units and colours of the wells, no more, no fewer");
    // The colourless unit is drawn in the well's colour: it says nothing of the unit, and is not in the legend.
    assert!(wells.iter().any(|(u, _)| u == "Colourless"));
    assert!(!legend.iter().any(|(u, _)| u == "Colourless"));
}

#[test]
fn the_legend_is_in_stratigraphic_order_the_wells_agree_on() {
    let svg = svg::write(&layout(&catalogue_of_two(), &on()));
    let titles: Vec<String> = entries_of_the_legend(&elements(&svg)).into_iter().map(|(t, _)| t).collect();
    assert_eq!(titles, ["Top", "Mid", "Bottom", "Deep"], "top down along the wells");
}

#[test]
fn the_order_follows_the_wells_not_the_alphabet_when_the_wells_disagree_it_does_not_loop() {
    // A: X over Y; B: Y over X. No order satisfies both: it must still terminate
    // and show each unit once, the first met first.
    let cat = catalogue(vec![
        well_with("A", 150.0, vec![interval(0.0, 100.0, Some("#111111"), "X"), interval(100.0, 200.0, Some("#222222"), "Y")]),
        well_with("B", 700.0, vec![interval(0.0, 100.0, Some("#222222"), "Y"), interval(100.0, 200.0, Some("#111111"), "X")]),
    ]);
    let titles: Vec<String> = entries_of_the_legend(&elements(&svg::write(&layout(&cat, &on())))).into_iter().map(|(t, _)| t).collect();
    assert_eq!(titles, ["X", "Y"]);
}

#[test]
fn the_legend_is_one_group_an_editor_can_move_outside_the_zoomed_plot_and_on_the_left() {
    let l = layout(&catalogue_of_two(), &on());
    let svg = svg::write(&l);
    let els = elements(&svg);
    let root = els.iter().find(|e| e.name == "svg").unwrap();
    assert_eq!(root.attr("xmlns:inkscape"), Some("http://www.inkscape.org/namespaces/inkscape"));
    let groups: Vec<&El> = els.iter().filter(|e| e.attr("data-legend") == Some("strata")).collect();
    assert_eq!(groups.len(), 1, "one group, not one per unit");
    let g = groups[0];
    assert_eq!(g.attr("id"), Some("strata-legend"));
    assert_eq!(g.attr("inkscape:groupmode"), Some("layer"));
    assert_eq!(g.attr("inkscape:label"), Some("Strata legend"));
    // Never in the plot group the script scales, never clipped.
    assert!(!inside(g, "zoom=plot"), "the legend is not zoomed");
    assert!(g.path.iter().all(|p| !p.starts_with("layer=")), "not inside any layer");
    // Nothing of the legend is a counter-scaled dot: it never moves.
    for e in els.iter().filter(|e| inside(e, "id=strata-legend")) {
        assert!(e.attr("data-zoom-fixed").is_none() && e.attr("data-zoom-anchor").is_none(), "{e:?}");
    }
    // On the left: its box ends before the frame, the frame is pushed right by it,
    // and the vertical title (the axes' own) moved with it.
    let frame = l.frame;
    assert!(frame.x > 64.0 + 100.0 - 1.0, "the room is reserved: {}", frame.x);
    assert!(l.left_shift > 0.0 && (frame.x - (64.0 + l.left_shift)).abs() < 1e-9);
    let polygons: Vec<&El> = els.iter().filter(|e| e.name == "polygon" && inside(e, "id=strata-legend")).collect();
    assert!(!polygons.is_empty());
    for p in polygons {
        for pair in p.attr("points").unwrap().split(' ') {
            let x: f64 = pair.split(',').next().unwrap().parse().unwrap();
            assert!(x >= 0.0 && x < frame.x - 64.0, "{x} is left of the axes' margin");
        }
    }
    let title = els.iter().find(|e| e.attr("data-role") == Some("y-title")).unwrap();
    let x: f64 = title.attr("x").unwrap().parse().unwrap();
    assert!(x > l.left_shift, "the vertical title is right of the legend: {x}");
    // The drawing is still the same width: the plot gave the room.
    assert_eq!(l.size, [1084.0, 700.0]);
}

#[test]
fn off_the_drawing_is_what_it_was() {
    let mut off = on();
    off.strata_legend = false;
    let svg = svg::write(&layout(&catalogue_of_two(), &off));
    assert!(!svg.contains("inkscape") && !svg.contains("strata-legend"), "no trace of the option");
    assert!(!svg.contains("strataLegend"), "not even in the metadata: the bytes are those of before");
    let l = layout(&catalogue_of_two(), &off);
    assert_eq!(l.left_shift, 0.0);
    assert_eq!(l.frame.x, 64.0);
    // A view written before the option existed reads as off.
    let mut json = serde_json::to_value(&off).unwrap();
    assert!(json.as_object_mut().unwrap().remove("strataLegend").is_none());
    assert!(!serde_json::from_value::<View>(json).unwrap().strata_legend);
}

#[test]
fn on_it_is_in_the_metadata_so_a_redraw_keeps_it() {
    let svg = svg::write(&layout(&catalogue_of_two(), &on()));
    let meta = svg::read_metadata(&svg).expect("metadata");
    assert!(meta.view.strata_legend);
}

#[test]
fn no_strata_to_show_is_no_legend_and_a_note_never_an_empty_box() {
    // Units with no colour of their own only.
    let none = catalogue(vec![well_with("A", 150.0, vec![interval(0.0, 100.0, None, "Colourless")])]);
    let l = layout(&none, &on());
    let svg = svg::write(&l);
    assert!(!svg.contains("strata-legend"), "no group at all");
    assert_eq!(l.left_shift, 0.0, "no room taken");
    assert!(l.warnings.iter().any(|w| w.starts_with("strata legend not drawn")), "{:?}", l.warnings);

    // Wells not coloured by strata: the intervals take the well's colour.
    let mut side = on();
    side.well_colouring = WellColouring::Side;
    let l = layout(&catalogue_of_two(), &side);
    assert!(!svg::write(&l).contains("strata-legend"));
    assert!(l.warnings.iter().any(|w| w.contains("not coloured by strata")), "{:?}", l.warnings);

    // No well at all.
    let l = layout(&catalogue(Vec::new()), &on());
    assert!(!svg::write(&l).contains("strata-legend"));
    assert!(l.warnings.iter().any(|w| w.contains("no well is drawn")), "{:?}", l.warnings);

    // A hidden well shows nothing: its units are not in the legend.
    let mut hidden = on();
    hidden.hidden = vec![id("B", "WellboreFrameRepresentation").key()];
    let titles: Vec<String> =
        entries_of_the_legend(&elements(&svg::write(&layout(&catalogue_of_two(), &hidden)))).into_iter().map(|(t, _)| t).collect();
    assert_eq!(titles, ["Top", "Mid", "Bottom"], "only the units the drawn wells show");
}

#[test]
fn the_plan_says_it_draws_no_legend() {
    let mut v = on();
    v.target = Target::Plan;
    v.window = None;
    let l = layout(&catalogue_of_two(), &v);
    assert!(!svg::write(&l).contains("strata-legend"));
    assert!(l.warnings.iter().any(|w| w.contains("plan shows no strata")), "{:?}", l.warnings);
}

#[test]
fn a_title_is_escaped_in_the_legend() {
    let cat = catalogue(vec![well_with("A", 150.0, vec![interval(0.0, 100.0, Some("#123456"), "<b>\"x\" & y</b>")])]);
    let svg = svg::write(&layout(&cat, &on()));
    assert!(!svg.contains("<b>"), "a unit's title is someone else's text");
    let titles: Vec<String> = entries_of_the_legend(&elements(&svg)).into_iter().map(|(t, _)| t).collect();
    assert_eq!(titles.len(), 1);
}

// ---------------------------------------------------------------------------
// The portions of a well with no strata information (an option to
// say whether or not to draw the portions without stratigraphy information, on
// by default).

/// The polylines of the well `key`'s group that are **not** inside an interval
/// group (the line under the intervals), and the `(from, to)` of its intervals.
fn well_parts(svg: &str, title_of_well: &str) -> (usize, Vec<(f64, f64)>, usize) {
    let els = elements(svg);
    let key = id(title_of_well, "WellboreFrameRepresentation").key().replace('\0', "\u{2400}");
    let start = els.iter().position(|e| e.name == "g" && e.attr("data-key") == Some(key.as_str()) && e.attr("data-kind") == Some("well")).expect("the well is drawn");
    let depth = els[start].path.len();
    let mut loose = 0;
    let mut circles = 0;
    let mut intervals = Vec::new();
    for e in &els[start + 1..] {
        if e.path.len() <= depth {
            break;
        }
        match e.name.as_str() {
            "g" if e.attr("data-unit").is_some() => {
                intervals.push((e.attr("data-md-from").unwrap().parse().unwrap(), e.attr("data-md-to").unwrap().parse().unwrap()))
            }
            "polyline" => loose += 1,
            "circle" => circles += 1,
            _ => {}
        }
    }
    // Every interval group holds one polyline: what is left is the line under them.
    (loose - intervals.len(), intervals, circles)
}

fn without_unstratified() -> View {
    let mut v = view();
    v.well_colouring = WellColouring::Strata;
    v.draw_unstratified = false;
    v
}

#[test]
fn without_the_portions_with_no_strata_a_well_is_drawn_only_where_a_unit_is_known() {
    let cat = catalogue_of_two();
    let l = layout(&cat, &without_unstratified());
    let svg = svg::write(&l);
    for (name, from_catalogue) in [("A", 4usize), ("B", 3usize)] {
        let Some(Item::Well(w)) = cat.long.items.iter().find(|i| matches!(i, Item::Well(w) if w.object.title == name)) else { panic!() };
        let (loose, drawn, circles) = well_parts(&svg, name);
        assert_eq!(loose, 0, "{name}: no line under the intervals");
        assert_eq!(circles, 1, "{name}: the head stays");
        let known: Vec<(f64, f64)> = w.intervals.iter().map(|i| (i.from, i.to)).collect();
        assert_eq!(known.len(), from_catalogue);
        assert_eq!(drawn, known, "{name}: exactly the catalogue's intervals");
    }
    // The control: with the option on, the same well has its line under them.
    let mut on = without_unstratified();
    on.draw_unstratified = true;
    let (loose, _, _) = well_parts(&svg::write(&layout(&cat, &on)), "A");
    assert_eq!(loose, 1, "the control: the line is there when the option is on");
    // Both wells stop at 350 and 400 of 520: both lost portions, and it is said.
    assert!(l.warnings.iter().any(|w| w.starts_with("2 well(s) drawn only where")), "{:?}", l.warnings);
}

#[test]
fn a_well_wholly_covered_loses_nothing_and_a_well_with_no_strata_keeps_its_head_and_label() {
    let covered = catalogue(vec![well_with("A", 150.0, vec![interval(0.0, 520.0, Some("#123456"), "All")])]);
    let l = layout(&covered, &without_unstratified());
    assert!(!l.warnings.iter().any(|w| w.contains("drawn only where")), "{:?}", l.warnings);

    let bare = catalogue(vec![well_with("A", 150.0, Vec::new())]);
    let l = layout(&bare, &without_unstratified());
    let svg = svg::write(&l);
    let (loose, drawn, circles) = well_parts(&svg, "A");
    assert_eq!((loose, drawn.len(), circles), (0, 0, 1), "only the head");
    assert!(!l.layer("labels").unwrap().objects.is_empty(), "the label stays");
    assert!(l.warnings.iter().any(|w| w.starts_with("1 well(s) drawn only where")));
}

#[test]
fn with_the_option_on_the_drawing_is_unchanged_and_the_metadata_does_not_say_it() {
    let cat = catalogue_of_two();
    let mut v = view();
    v.well_colouring = WellColouring::Strata;
    assert!(v.draw_unstratified, "on by default");
    let svg = svg::write(&layout(&cat, &v));
    assert!(!svg.contains("drawUnstratified"));
    assert!(!svg.contains("drawn only where"));
    let mut json = serde_json::to_value(&v).unwrap();
    json.as_object_mut().unwrap().remove("drawUnstratified");
    assert!(serde_json::from_value::<View>(json).unwrap().draw_unstratified, "a view written before reads as on");
    // Off is in the metadata, so a redraw keeps it.
    let off = svg::write(&layout(&cat, &without_unstratified()));
    assert!(!svg::read_metadata(&off).unwrap().view.draw_unstratified);
}
