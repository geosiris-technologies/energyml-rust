//! `draw::text::fit_to_page` and the weight of a text: a text that leaves the page is broken into lines that fit,
//! and a bold text is written bold. Data are made up; each test says what would break it.

use energyml_sheet::draw::svg::write;
use energyml_sheet::draw::text::{max_text_x, styled_width};
use energyml_sheet::draw::{Anchor, DisplayList, Element, FontWeight, Item, Layer, PageUnit, TextStyle};

fn list_with(text: &str, at: [f64; 2], style: TextStyle) -> DisplayList {
    let mut list = DisplayList::new([200.0, 100.0], PageUnit::Pixel);
    let mut layer = Layer::new("title");
    layer.elements.push(Element::new(vec![Item::Text {
        at,
        text: text.to_string(),
        style,
    }]));
    list.layers.push(layer);
    list
}

fn texts(list: &DisplayList) -> Vec<(String, [f64; 2])> {
    list.layers
        .iter()
        .flat_map(|l| &l.elements)
        .flat_map(|e| &e.items)
        .filter_map(|item| match item {
            Item::Text { at, text, .. } => Some((text.clone(), *at)),
            _ => None,
        })
        .collect()
}

const LONG: &str = "A title which is much longer than the two hundred pixels of this page can carry";

/// Break it: make `fit_to_page` return at once; the widest text then crosses 200 and the first assert fails.
#[test]
fn a_title_longer_than_the_page_is_broken_into_lines_inside_it() {
    let mut list = list_with(LONG, [10.0, 20.0], TextStyle::default());
    assert!(
        max_text_x(&list).expect("a text") > 200.0,
        "the fabrication must overflow"
    );
    energyml_sheet::draw::text::fit_to_page(&mut list);
    assert!(max_text_x(&list).expect("a text") <= 200.0);
    let lines = texts(&list);
    assert!(lines.len() > 1, "{lines:?}");
    assert_eq!(lines[0].1, [10.0, 20.0], "the first line keeps its baseline");
    assert!(lines[1].1[1] > lines[0].1[1], "the next line is below");
    assert_eq!(
        lines
            .iter()
            .map(|(t, _)| t.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        LONG,
        "no word is lost"
    );
}

/// Break it: measure a bold text as a regular one (drop the factor in `styled_width`): the bold line then reads
/// narrower than the regular-measured one and the first assert fails.
#[test]
fn a_bold_text_is_measured_wider_and_still_fits() {
    let bold = TextStyle {
        weight: FontWeight::Bold,
        ..TextStyle::default()
    };
    assert!(styled_width("Header", &bold) > styled_width("Header", &TextStyle::default()));
    let mut list = list_with(LONG, [10.0, 20.0], bold);
    energyml_sheet::draw::text::fit_to_page(&mut list);
    assert!(max_text_x(&list).expect("a text") <= 200.0);
}

/// Break it: wrap an `Anchor::Middle` text against the room on its right only; the left half then leaves the page.
#[test]
fn a_centred_and_an_end_anchored_text_fit_on_both_sides() {
    for anchor in [Anchor::Middle, Anchor::End] {
        let style = TextStyle {
            anchor,
            ..TextStyle::default()
        };
        let x = if anchor == Anchor::End { 150.0 } else { 60.0 };
        let mut list = list_with(LONG, [x, 20.0], style);
        energyml_sheet::draw::text::fit_to_page(&mut list);
        for (line, _) in texts(&list) {
            let width = styled_width(&line, &style);
            let [left, right] = energyml_sheet::draw::text::x_span(x, anchor, width);
            assert!(left >= -1e-9 && right <= 200.0 + 1e-9, "{anchor:?} {line:?}");
        }
    }
}

/// Break it: let `fit_to_page` rewrite a text that fits; the one item below becomes two or changes position.
#[test]
fn a_text_that_fits_is_left_alone() {
    let mut list = list_with("short", [10.0, 20.0], TextStyle::default());
    let before = list.clone();
    energyml_sheet::draw::text::fit_to_page(&mut list);
    assert_eq!(list, before);
}

/// Break it: stop writing `font-weight` in `draw::svg`; the bold text then has no such attribute.
#[test]
fn the_svg_writes_bold_and_only_bold() {
    let bold = TextStyle {
        weight: FontWeight::Bold,
        ..TextStyle::default()
    };
    let svg = write(&list_with("Head", [10.0, 20.0], bold), None).expect("a drawable list");
    assert!(svg.contains("font-weight=\"bold\""), "{svg}");
    let plain = write(&list_with("Head", [10.0, 20.0], TextStyle::default()), None).expect("a drawable list");
    assert!(!plain.contains("font-weight"), "{plain}");
}
