//! `draw::hit`: the keyed element under a point. Each test states the rule it
//! checks (the order of drawing, the slop, the clip, the measured text) and
//! what would have to break for it to turn red.

use std::sync::Arc;

use energyml_sheet::draw::text::string_width_pt;
use energyml_sheet::draw::{
    Anchor, Colour, DisplayList, Element, Item, Layer, PageUnit, Paint, Raster, Rect, Stroke, TextStyle, hit,
};

const RED: Colour = Colour::rgb(255, 0, 0);

fn list_of(layers: Vec<Layer>) -> DisplayList {
    let mut list = DisplayList::new([200.0, 100.0], PageUnit::Pixel);
    list.layers = layers;
    list
}

fn layer_of(name: &str, elements: Vec<Element>) -> Layer {
    let mut layer = Layer::new(name);
    layer.elements = elements;
    layer
}

fn filled_rect(x: f64, y: f64, w: f64, h: f64) -> Item {
    Item::Rect {
        rect: Rect { x, y, w, h },
        corner: 0.0,
        fill: Some(Paint::Solid(RED)),
        stroke: None,
    }
}

/// Three bars with a gap of 10 between them (more than the slop of 3).
/// Breaks if: the point is tested against the list's box, not the bars.
#[test]
fn hit_finds_the_bar_under_the_point() {
    let bars = (0..3)
        .map(|i| {
            Element::keyed(
                format!("bar-{i}"),
                "bar",
                vec![filled_rect(f64::from(i) * 30.0, 10.0, 20.0, 40.0)],
            )
        })
        .collect();
    let list = list_of(vec![layer_of("bars", bars)]);
    let found = hit(&list, 40.0, 30.0).expect("the centre of bar-1 is on it");
    assert_eq!(found.key, "bar-1");
    assert_eq!((found.layer, found.element), (0, 1));
    assert_eq!(
        hit(&list, 55.0, 30.0),
        None,
        "a point between two bars hits nothing"
    );
}

/// What is drawn last is on top; a decoration hides nothing.
/// Breaks if: elements are walked first to last, or a keyless element answers.
#[test]
fn the_topmost_keyed_element_wins_and_a_decoration_does_not_block() {
    let elements = vec![
        Element::keyed("under", "bar", vec![filled_rect(0.0, 0.0, 50.0, 50.0)]),
        Element::keyed("over", "bar", vec![filled_rect(10.0, 10.0, 50.0, 50.0)]),
        Element::new(vec![filled_rect(0.0, 0.0, 100.0, 100.0)]),
    ];
    let list = list_of(vec![layer_of("l", elements)]);
    assert_eq!(hit(&list, 30.0, 30.0).map(|h| h.key), Some("over"));
    assert_eq!(hit(&list, 5.0, 5.0).map(|h| h.key), Some("under"));
}

/// A thick polyline of width 10 is hit within 5 + the slop 3 = 8 of its axis.
/// Breaks if: the half width is forgotten (7 would then miss).
#[test]
fn a_point_on_a_thick_polyline_hits_it() {
    let line = Item::Polyline {
        points: vec![[0.0, 50.0], [100.0, 50.0]],
        stroke: Stroke::solid(RED, 10.0),
    };
    let list = list_of(vec![layer_of(
        "l",
        vec![Element::keyed("well", "well", vec![line])],
    )]);
    assert!(hit(&list, 50.0, 57.0).is_some(), "7 <= 5 + 3");
    assert!(hit(&list, 50.0, 59.0).is_none(), "9 > 5 + 3");
}

/// Breaks if: the angle of the sector is ignored.
#[test]
fn a_point_in_a_sector_hits_it_and_one_outside_its_angle_does_not() {
    let sector = Item::Sector {
        centre: [50.0, 50.0],
        inner: 10.0,
        outer: 20.0,
        start: 0.0,
        end: std::f64::consts::FRAC_PI_2,
        fill: Some(Paint::Solid(RED)),
        stroke: None,
    };
    let list = list_of(vec![layer_of(
        "l",
        vec![Element::keyed("s", "sector", vec![sector])],
    )]);
    assert!(hit(&list, 58.0, 58.0).is_some(), "radius 11.3, 45 degrees");
    assert!(hit(&list, 42.0, 58.0).is_none(), "135 degrees is outside");
    assert!(
        hit(&list, 50.0, 50.0).is_none(),
        "the centre is inside the inner radius"
    );
}

/// An image on (10, 10, 20, 20) of a 4 x 4 raster: 5 units a pixel.
/// Breaks if: the pixel is rounded instead of truncated.
#[test]
fn a_point_on_an_image_gives_its_pixel() {
    let raster = Raster::from_rgba(4, 4, vec![0; 4 * 4 * 4]).expect("4 x 4 x 4 bytes");
    let image = Item::Image {
        rect: Rect {
            x: 10.0,
            y: 10.0,
            w: 20.0,
            h: 20.0,
        },
        raster: Arc::new(raster),
    };
    let list = list_of(vec![layer_of(
        "l",
        vec![Element::keyed("img", "image", vec![image])],
    )]);
    let found = hit(&list, 26.0, 12.0).expect("inside the image");
    assert_eq!(found.pixel, Some([3, 0]));
    let edge = hit(&list, 30.0, 30.0).expect("the bottom right corner is on the image");
    assert_eq!(edge.pixel, Some([3, 3]), "clamped to the last pixel");
}

/// A rectangle with neither fill nor stroke is a hit area: its middle hits, far
/// from any border; one with a stroke and no fill stays an outline
/// (`[views-sheet C1]`).
/// Breaks if: the unfilled-and-unstroked rectangle is tested on its outline again.
#[test]
fn an_invisible_rectangle_is_hit_inside_and_an_outline_only_on_its_border() {
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    };
    let area = Item::Rect {
        rect,
        corner: 0.0,
        fill: None,
        stroke: None,
    };
    let outline = Item::Rect {
        rect,
        corner: 0.0,
        fill: None,
        stroke: Some(Stroke::solid(RED, 2.0)),
    };
    let list = list_of(vec![layer_of(
        "l",
        vec![Element::keyed("area", "row", vec![area])],
    )]);
    assert_eq!(hit(&list, 50.0, 50.0).map(|h| h.key), Some("area"));
    assert_eq!(hit(&list, 150.0, 50.0), None, "outside is outside");
    let list = list_of(vec![layer_of(
        "l",
        vec![Element::keyed("frame", "frame", vec![outline])],
    )]);
    assert_eq!(
        hit(&list, 50.0, 50.0),
        None,
        "the middle of an outline is not on it"
    );
    assert!(hit(&list, 1.0, 50.0).is_some(), "its border is");
}

/// Breaks if: the clip of the layer is ignored.
#[test]
fn a_point_outside_the_layer_clip_hits_nothing() {
    let mut layer = layer_of(
        "l",
        vec![Element::keyed(
            "r",
            "bar",
            vec![filled_rect(0.0, 0.0, 100.0, 100.0)],
        )],
    );
    layer.clip = Some(Rect {
        x: 0.0,
        y: 0.0,
        w: 50.0,
        h: 50.0,
    });
    let list = list_of(vec![layer]);
    assert_eq!(hit(&list, 75.0, 75.0), None);
    assert!(hit(&list, 25.0, 25.0).is_some());
}

/// "MMMM" and "iiii" have the same length and nowhere near the same width.
/// Breaks if: the box is measured by the number of characters.
#[test]
fn a_text_is_hit_inside_its_measured_width() {
    let text = |s: &str| Item::Text {
        at: [0.0, 50.0],
        text: s.to_string(),
        style: TextStyle {
            size: 10.0,
            colour: RED,
            anchor: Anchor::Start,
            rotate: 0.0,
        },
    };
    let list = list_of(vec![
        layer_of("wide", vec![Element::keyed("MMMM", "label", vec![text("MMMM")])]),
        layer_of(
            "narrow",
            vec![Element::keyed("iiii", "label", vec![text("iiii")])],
        ),
    ]);
    let x = string_width_pt("iiii", 10.0) + 1.0;
    assert!(
        string_width_pt("MMMM", 10.0) > x,
        "the table must tell the two apart"
    );
    assert_eq!(hit(&list, x, 50.0).map(|h| h.key), Some("MMMM"));
    assert_eq!(
        hit(&list, 1.0, 50.0).map(|h| h.key),
        Some("iiii"),
        "inside both: the upper layer wins"
    );
}

/// Breaks if: a non finite coordinate is taken for a point.
#[test]
fn a_non_finite_point_hits_nothing() {
    let list = list_of(vec![layer_of(
        "l",
        vec![Element::keyed(
            "r",
            "bar",
            vec![filled_rect(0.0, 0.0, 100.0, 100.0)],
        )],
    )]);
    assert_eq!(hit(&list, f64::NAN, 5.0), None);
    assert_eq!(hit(&list, 5.0, f64::INFINITY), None);
}
