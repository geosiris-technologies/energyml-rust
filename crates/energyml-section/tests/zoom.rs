//! The SVG that zooms itself (a line keeps the same width in pixels
//! even when zoomed; it has to work outside a front end, when the user
//! opens the SVG in a browser; the option that adds the script to the SVG files
//! produced is on by default).
//!
//! What a Rust test can hold: the markup. That the script runs is the front end's
//! vitest (its own test suite), which loads `src/zoom.js` itself.

use crate::layout::{catalogue, id, trace, view, well};
use energyml_section::layout::layout;
use energyml_section::model::*;
use energyml_section::svg;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

/// The script as the crate embeds it.
const SCRIPT: &str = include_str!("../src/zoom.js");

#[derive(Debug, Clone)]
struct El {
    name: String,
    attrs: Vec<(String, String)>,
    /// Inside the `<g data-zoom="plot">` the script transforms.
    in_plot: bool,
    /// The direct parent's attributes' `clip-path`, when it has one.
    parent_clip: Option<String>,
    /// Text of a CDATA section directly inside.
    cdata: String,
}

impl El {
    fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
}

fn start(e: &BytesStart) -> (String, Vec<(String, String)>) {
    (
        String::from_utf8_lossy(e.name().as_ref()).into_owned(),
        e.attributes()
            .map(|a| {
                let a = a.expect("a well-formed attribute");
                (
                    String::from_utf8_lossy(a.key.as_ref()).into_owned(),
                    a.normalized_value(quick_xml::XmlVersion::default())
                        .expect("a valid value")
                        .into_owned(),
                )
            })
            .collect(),
    )
}

/// The whole document, element by element, each knowing whether it lies in the plot group.
/// (`quick-xml` reports `<a/>` as `Empty` with no `End`, so only a `Start` opens.)
fn parse_tree(svg: &str) -> Vec<El> {
    let mut reader = Reader::from_str(svg);
    let mut out: Vec<El> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    loop {
        match reader.read_event().expect("the SVG is well-formed XML") {
            ev @ (Event::Start(_) | Event::Empty(_)) => {
                let (e, empty) = match &ev {
                    Event::Start(e) => (e, false),
                    Event::Empty(e) => (e, true),
                    _ => unreachable!(),
                };
                let (name, attrs) = start(e);
                let parent = stack.last().map(|&i| &out[i]);
                let in_plot = parent.is_some_and(|p| p.in_plot || p.attr("data-zoom") == Some("plot"));
                let parent_clip = parent.and_then(|p| p.attr("clip-path")).map(str::to_string);
                out.push(El {
                    name,
                    attrs,
                    in_plot,
                    parent_clip,
                    cdata: String::new(),
                });
                if !empty {
                    stack.push(out.len() - 1);
                }
            }
            Event::CData(c) => {
                if let Some(&i) = stack.last() {
                    out[i].cdata.push_str(&String::from_utf8_lossy(&c));
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

fn items() -> Vec<Item> {
    vec![
        Item::Well(well("A", 150.0, 12.0, Side::Left)),
        Item::Well(well("B", 700.0, 30.0, Side::Right)),
        Item::Trace(trace("H", vec![vec![[0.0, -100.0], [1000.0, -100.0]]])),
        Item::Trace(trace("L", vec![vec![[0.0, -300.0], [600.0, -300.0]]])),
    ]
}

fn drawn(zoom_script: bool) -> String {
    let mut v = view();
    v.zoom_script = zoom_script;
    svg::write(&layout(&catalogue(items()), &v))
}

fn els(zoom_script: bool) -> Vec<El> {
    parse_tree(&drawn(zoom_script))
}

#[test]
fn the_script_is_there_by_default_and_only_then() {
    // `View::new` is the way a face builds a view: it must switch the script on.
    let fresh = View::new(
        Target::Section {
            key: SectionKey::Long,
        },
        [1084.0, 700.0],
    );
    assert!(fresh.zoom_script, "the option is on by default");
    // A view saved before the option existed reads as on too.
    let mut json = serde_json::to_value(&fresh).unwrap();
    json.as_object_mut().unwrap().remove("zoomScript");
    assert!(serde_json::from_value::<View>(json).unwrap().zoom_script);

    let on = drawn(true);
    let off = drawn(false);
    assert_eq!(on.matches("<script>").count(), 1, "one script");
    assert!(on.contains("<![CDATA["));
    for needle in [
        "<script",
        "vector-effect",
        "data-zoom",
        "data-ticks",
        "data-tick-at",
        "CDATA",
    ] {
        assert!(
            !off.contains(needle),
            "`{needle}` has no business in a static drawing"
        );
    }
    // The clip is where it always was.
    assert!(off.contains("<g data-layer=\"traces\" clip-path=\"url(#plot)\">"));
}

#[test]
fn every_stroke_in_the_plot_keeps_its_pixel_width() {
    let els = els(true);
    let stroked: Vec<&El> = els
        .iter()
        .filter(|e| e.in_plot)
        .filter(|e| e.attr("stroke").is_some_and(|s| s != "none"))
        .collect();
    // The guard of the guard: wells, intervals and traces are stroked, so a
    // drawing with none in the plot is a test that cannot fail.
    assert!(
        stroked.len() >= 6,
        "{} stroked elements in the plot",
        stroked.len()
    );
    assert!(
        stroked
            .iter()
            .any(|e| e.name == "polyline" && e.attr("data-md-from").is_none())
    );
    for e in &stroked {
        assert_eq!(
            e.attr("vector-effect"),
            Some("non-scaling-stroke"),
            "<{}> {:?}",
            e.name,
            e.attrs
        );
    }
}

#[test]
fn the_group_the_script_transforms_is_not_the_one_that_clips() {
    let els = els(true);
    let plots: Vec<&El> = els
        .iter()
        .filter(|e| e.attr("data-zoom") == Some("plot"))
        .collect();
    assert_eq!(plots.len(), 1, "the clipped layers are one run");
    let plot = plots[0];
    // A clip on the transformed group would be scaled with the plot.
    assert_eq!(plot.attr("clip-path"), None);
    assert_eq!(plot.parent_clip.as_deref(), Some("url(#plot)"));
    // Everything the script scales is inside it; the frame, axes, title and
    // legend are not.
    let layers_in: Vec<&str> = els
        .iter()
        .filter(|e| e.in_plot)
        .filter_map(|e| e.attr("data-layer"))
        .collect();
    assert!(layers_in.contains(&"traces") && layers_in.contains(&"wells"));
    assert!(!layers_in.contains(&"axes") && !layers_in.contains(&"labels") && !layers_in.contains(&"legend"));
    let outside = |f: &dyn Fn(&El) -> bool| els.iter().filter(|e| f(e)).all(|e| !e.in_plot);
    assert!(
        outside(&|e| e.attr("data-role").is_some()),
        "titles and notes stay out"
    );
    assert!(outside(&|e| e.name == "rect"), "the frame stays out");
    assert!(
        els.iter()
            .any(|e| e.name == "rect" && e.attr("stroke") == Some("#444444"))
    );
}

#[test]
fn dots_and_texts_in_the_plot_say_where_they_are_anchored() {
    let els = els(true);
    let circles: Vec<&El> = els.iter().filter(|e| e.name == "circle" && e.in_plot).collect();
    assert!(!circles.is_empty(), "the well heads");
    for c in &circles {
        let at = format!("{} {}", c.attr("cx").unwrap(), c.attr("cy").unwrap());
        assert_eq!(
            c.attr("data-zoom-fixed"),
            Some(at.as_str()),
            "a dot is counter-scaled around its centre"
        );
    }
    for t in els.iter().filter(|e| e.name == "text" && e.in_plot) {
        let at = format!("{} {}", t.attr("x").unwrap(), t.attr("y").unwrap());
        assert_eq!(t.attr("data-zoom-fixed"), Some(at.as_str()));
    }
    // What is outside the plot group is never counter-scaled — it never grows.
    assert!(
        els.iter()
            .filter(|e| !e.in_plot)
            .all(|e| e.attr("data-zoom-fixed").is_none())
    );
    // A label of the unclipped layer carries its anchor, for the script to move it.
    let labels: Vec<&El> = els.iter().filter(|e| e.attr("data-label-of").is_some()).collect();
    assert!(!labels.is_empty(), "the well names");
    assert!(
        labels
            .iter()
            .all(|e| e.attr("data-zoom-anchor").is_some() && !e.in_plot)
    );
}

#[test]
fn the_axes_hand_their_graduations_to_the_script_and_the_root_keeps_its_map() {
    let on = els(true);
    let off = els(false);
    let ticks: Vec<&str> = on.iter().filter_map(|e| e.attr("data-ticks")).collect();
    assert_eq!(ticks, vec!["x", "y"]);
    // The map the script reads is the one the front end reads, the same both ways.
    let root = |v: &[El]| -> Vec<(String, String)> {
        let r = v.iter().find(|e| e.name == "svg").unwrap();
        ["data-frame", "data-window", "data-scale"]
            .iter()
            .map(|k| (k.to_string(), r.attr(k).unwrap().to_string()))
            .collect()
    };
    assert_eq!(root(&on), root(&off));
    // Gridlines and their texts still stand in the static drawing, once each.
    let lines = |v: &[El]| {
        v.iter()
            .filter(|e| e.name == "line" && e.attr("stroke") == Some("#e0e0e0"))
            .count()
    };
    assert!(lines(&off) > 4);
    assert_eq!(lines(&on), lines(&off));
}

#[test]
fn the_script_is_well_formed_inside_its_cdata_and_is_the_file() {
    assert!(!SCRIPT.contains("]]>"), "`]]>` would end the CDATA");
    assert!(
        !SCRIPT.to_ascii_lowercase().contains("</script"),
        "`</script` would end the element"
    );
    let on = els(true);
    let script = on.iter().find(|e| e.name == "script").expect("a <script>");
    assert_eq!(
        script.cdata.trim(),
        SCRIPT.trim(),
        "the CDATA is the embedded file, byte for byte"
    );
    // It is the last thing in the root, after everything it reads.
    assert_eq!(on.last().map(|e| e.name.as_str()), Some("script"));
}

/// Strip what the script adds, and the two drawings are one: turning the
/// option on only ever adds attributes, wrappers and the script.
#[test]
fn switching_the_script_on_adds_and_changes_nothing_else() {
    fn normal(v: Vec<El>) -> Vec<(String, Vec<(String, String)>)> {
        v.into_iter()
            .filter(|e| {
                e.name != "script"
                    && e.attr("data-zoom") != Some("plot")
                    && e.attr("data-ticks").is_none()
                    && e.attr("data-tick-at").is_none()
            })
            .filter(|e| !(e.name == "g" && e.attrs.len() == 1 && e.attrs[0].0 == "clip-path"))
            .map(|e| {
                let attrs = e
                    .attrs
                    .into_iter()
                    .filter(|(k, _)| k != "vector-effect" && !k.starts_with("data-zoom") && k != "clip-path")
                    .collect();
                (e.name, attrs)
            })
            // The metadata's JSON says which way the option was set.
            .filter(|(name, _)| name != "section-view")
            .collect()
    }
    let on = normal(els(true));
    let off = normal(els(false));
    assert!(off.len() > 30, "{} elements compared", off.len());
    assert_eq!(on, off);
}

#[test]
fn the_plan_zooms_too() {
    let mut cat = catalogue(items());
    cat.plan.sections = vec![(SectionKey::Long, vec![[0.0, 0.0], [1000.0, 200.0]])];
    cat.plan.wells = vec![PlanWell {
        object: id("A", "WellboreFrameRepresentation"),
        head: [150.0, 40.0],
        feet: vec![(SectionKey::Long, [150.0, 30.0])],
        rejected: None,
    }];
    let mut v = view();
    v.target = Target::Plan;
    v.window = None;
    let on = parse_tree(&svg::write(&layout(&cat, &v)));
    assert_eq!(
        on.iter().filter(|e| e.attr("data-zoom") == Some("plot")).count(),
        1
    );
    assert!(
        on.iter()
            .filter(|e| e.in_plot && e.attr("stroke").is_some_and(|s| s != "none"))
            .all(|e| e.attr("vector-effect").is_some())
    );
    assert!(on.iter().any(|e| e.name == "script"));
    v.zoom_script = false;
    let off = svg::write(&layout(&cat, &v));
    assert!(!off.contains("<script") && !off.contains("vector-effect"));
}

#[test]
fn a_redraw_from_the_metadata_keeps_the_choice() {
    for choice in [true, false] {
        let svg_text = drawn(choice);
        let back = svg::read_metadata(&svg_text).unwrap();
        assert_eq!(back.view.zoom_script, choice);
    }
}
