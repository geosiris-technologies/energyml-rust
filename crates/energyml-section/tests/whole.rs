//! `data-whole`: the extent a drawing would have with no window asked.
//!
//! A front end that holds a zoomed drawing — the detached window, or a definition
//! reopened with its saved window — has no drawing of the whole to clamp its
//! zooms to, and its wheel was dead. The drawing now says it itself, on its
//! root, and says the **same** thing however it was zoomed.

use crate::layout::{catalogue, id, trace, view, well};
use energyml_section::layout::layout;
use energyml_section::model::*;
use energyml_section::svg;

fn items() -> Vec<Item> {
    vec![
        Item::Well(well("A", 150.0, 12.0, Side::Left)),
        Item::Well(well("B", 700.0, 30.0, Side::Right)),
        Item::Trace(trace("H", vec![vec![[0.0, -100.0], [1000.0, -100.0]]])),
        Item::Trace(trace("L", vec![vec![[0.0, -300.0], [600.0, -300.0]]])),
    ]
}

/// The four numbers of a root attribute, read from the text of the drawing.
fn root(svg: &str, name: &str) -> Vec<f64> {
    let tag = &svg[svg.find("<svg").expect("a root")..];
    let tag = &tag[..tag.find('>').expect("a closed root")];
    let key = format!(" {name}=\"");
    let at = tag
        .find(&key)
        .unwrap_or_else(|| panic!("no {name} on the root: {tag}"))
        + key.len();
    tag[at..][..tag[at..].find('"').unwrap()]
        .split(' ')
        .map(|n| n.parse().unwrap())
        .collect()
}

fn window_of(w: &Window) -> Vec<f64> {
    vec![w.x[0], w.x[1], w.y[0], w.y[1]]
}

fn section_view(window: Option<Window>) -> View {
    let mut v = view();
    v.window = window;
    v
}

fn plan_catalogue() -> Catalogue {
    let mut cat = catalogue(items());
    cat.plan.sections = vec![(SectionKey::Long, vec![[0.0, 0.0], [1000.0, 200.0]])];
    cat.plan.wells = vec![PlanWell {
        object: id("A", "WellboreFrameRepresentation"),
        head: [150.0, 40.0],
        feet: vec![(SectionKey::Long, [150.0, 30.0])],
        rejected: None,
    }];
    cat
}

#[test]
fn drawn_without_a_window_the_whole_is_the_window() {
    let out = svg::write(&layout(&catalogue(items()), &section_view(None)));
    assert_eq!(root(&out, "data-whole"), root(&out, "data-window"));
}

#[test]
fn the_whole_is_the_same_whatever_window_was_asked() {
    let cat = catalogue(items());
    let whole = root(&svg::write(&layout(&cat, &section_view(None))), "data-whole");
    for asked in [
        Window {
            x: [0.0, 1000.0],
            y: [-500.0, 0.0],
        },
        // Left of well B: B is not even written, and the whole still counts it.
        Window {
            x: [100.0, 300.0],
            y: [-200.0, -50.0],
        },
        Window {
            x: [650.0, 750.0],
            y: [-40.0, 0.0],
        },
    ] {
        let out = svg::write(&layout(&cat, &section_view(Some(asked))));
        assert_eq!(
            root(&out, "data-window"),
            window_of(&asked),
            "the window is the one asked"
        );
        assert_eq!(
            root(&out, "data-whole"),
            whole,
            "the whole does not follow the window {asked:?}"
        );
    }
}

#[test]
fn the_whole_is_the_same_in_the_plan_and_follows_the_frame_proportions() {
    let cat = plan_catalogue();
    let mut v = view();
    v.target = Target::Plan;
    v.window = None;
    let bare = svg::write(&layout(&cat, &v));
    assert_eq!(root(&bare, "data-whole"), root(&bare, "data-window"));
    v.window = Some(Window {
        x: [100.0, 200.0],
        y: [20.0, 60.0],
    });
    let zoomed = svg::write(&layout(&cat, &v));
    assert_ne!(root(&zoomed, "data-window"), root(&bare, "data-window"));
    assert_eq!(root(&zoomed, "data-whole"), root(&bare, "data-whole"));
}

#[test]
fn only_that_attribute_is_new_on_a_drawing_without_the_script() {
    let mut v = section_view(None);
    v.zoom_script = false;
    let out = svg::write(&layout(&catalogue(items()), &v));
    // Take the attribute out and what is left is the root as it always was.
    let whole = root(&out, "data-whole");
    let spelled = format!(
        " data-whole=\"{} {} {} {}\"",
        whole[0], whole[1], whole[2], whole[3]
    );
    assert!(out.contains(&spelled), "written as the window is: {spelled}");
    let before = out.replacen(&spelled, "", 1);
    assert_eq!(before.matches("data-whole").count(), 0);
    assert!(before.contains("data-window=") && before.contains("data-scale="));
}
