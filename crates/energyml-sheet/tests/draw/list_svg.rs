//! `draw::list` and `draw::svg`: the display list, its SVG and the metadata
//! read back. The XML is re-read by `quick-xml`, a reader that is not the code
//! under test (two independent readings of one fact, ADR-0011), as in
//! `tests/svg.rs`. Data are made up; each test says what would break it.

use std::sync::Arc;

use energyml_sheet::draw::svg::{DrawError, Metadata, MetadataError, escape, read_metadata, write};
use energyml_sheet::draw::{
    Anchor, Colour, DisplayList, Element, Hatch, Item, Layer, PageUnit, Paint, PathData, Raster, Rect, Segment,
    Stroke, SymbolKind, TextStyle,
};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

const RED: Colour = Colour::rgb(255, 0, 0);
const BLUE: Colour = Colour::rgb(0, 0, 255);

type Attrs = Vec<(String, String)>;

fn attrs_of(start: &BytesStart) -> Attrs {
    start
        .attributes()
        .flatten()
        .map(|a| {
            let key = String::from_utf8_lossy(a.key.as_ref()).to_string();
            let raw = String::from_utf8_lossy(a.value.as_ref()).to_string();
            let value = quick_xml::escape::unescape(&raw).expect("a well-formed attribute").to_string();
            (key, value)
        })
        .collect()
}

/// Every element with its attributes, in document order, and the text of every
/// `<title>`. Panics on the first XML error: that is the test.
fn walk(svg: &str) -> (Vec<(String, Attrs)>, Vec<String>) {
    let mut reader = Reader::from_str(svg);
    let mut nodes = Vec::new();
    let mut titles = Vec::new();
    let mut depth: i64 = 0;
    let mut title: Option<String> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(Event::Start(start)) => {
                depth += 1;
                let name = String::from_utf8_lossy(start.name().as_ref()).to_string();
                if name == "title" {
                    title = Some(String::new());
                }
                nodes.push((name, attrs_of(&start)));
            }
            Ok(Event::Empty(start)) => {
                let name = String::from_utf8_lossy(start.name().as_ref()).to_string();
                nodes.push((name, attrs_of(&start)));
            }
            Ok(Event::End(end)) => {
                depth -= 1;
                if end.name().as_ref() == b"title" {
                    titles.extend(title.take());
                }
            }
            Ok(Event::Text(text)) => {
                if let Some(buffer) = title.as_mut() {
                    buffer.push_str(&text.decode().expect("utf-8 text"));
                }
            }
            Ok(Event::GeneralRef(reference)) => {
                if let Some(buffer) = title.as_mut() {
                    let name = reference.decode().expect("utf-8 reference").to_string();
                    buffer.push(match name.as_str() {
                        "lt" => '<',
                        "gt" => '>',
                        "amp" => '&',
                        "quot" => '"',
                        "apos" => '\'',
                        other => panic!("an unexpected entity `&{other};`"),
                    });
                }
            }
            Ok(_) => {}
            Err(error) => panic!("the SVG is not well-formed XML: {error}\n---\n{svg}"),
        }
    }
    assert_eq!(depth, 0, "every start tag has its end tag");
    (nodes, titles)
}

fn attr<'a>(attrs: &'a Attrs, name: &str) -> Option<&'a str> {
    attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

fn nodes_named<'a>(nodes: &'a [(String, Attrs)], tag: &str) -> Vec<&'a Attrs> {
    nodes.iter().filter(|(t, _)| t == tag).map(|(_, a)| a).collect()
}

fn layer_of(name: &str, elements: Vec<Element>) -> Layer {
    let mut layer = Layer::new(name);
    layer.elements = elements;
    layer
}

fn list_of(layers: Vec<Layer>) -> DisplayList {
    let mut list = DisplayList::new([200.0, 100.0], PageUnit::Pixel);
    list.layers = layers;
    list
}

fn text_style() -> TextStyle {
    TextStyle { size: 10.0, colour: BLUE, anchor: Anchor::Middle, rotate: 0.0 }
}

/// 200 x 100 px, two layers and an empty one, one element of every item but `Image`.
fn sample() -> DisplayList {
    let hatch = Hatch { colour: BLUE, spacing: 4.0, angle: 45.0, width: 1.0 };
    let back = layer_of(
        "back",
        vec![
            Element::keyed(
                "eml:///a",
                "bar",
                vec![Item::Rect {
                    rect: Rect { x: 10.0, y: 10.0, w: 20.0, h: 30.0 },
                    corner: 2.0,
                    fill: Some(Paint::Solid(RED)),
                    stroke: Some(Stroke::solid(BLUE, 1.0)),
                }],
            ),
            Element::keyed(
                "eml:///b",
                "cell",
                vec![Item::Polygon {
                    points: vec![[40.0, 10.0], [60.0, 10.0], [50.0, 30.0]],
                    fill: Some(Paint::Hatch(hatch)),
                    stroke: None,
                }],
            ),
            Element::new(vec![Item::Circle {
                centre: [100.0, 20.0],
                radius: 5.0,
                fill: None,
                stroke: Some(Stroke::solid(RED, 1.0)),
            }]),
        ],
    );
    let curves = layer_of(
        "curves",
        vec![Element::keyed(
            "eml:///c",
            "curve",
            vec![
                Item::Polyline { points: vec![[0.0, 50.0], [100.0, 60.0]], stroke: Stroke::solid(RED, 2.0) },
                Item::Path {
                    path: PathData::polyline(&[[0.0, 0.0], [10.0, 10.0]]),
                    fill: None,
                    stroke: Some(Stroke::solid(RED, 1.0)),
                },
                Item::Sector {
                    centre: [150.0, 50.0],
                    inner: 5.0,
                    outer: 15.0,
                    start: 0.0,
                    end: 1.0,
                    fill: Some(Paint::Solid(BLUE)),
                    stroke: None,
                },
                Item::Symbol { at: [20.0, 80.0], kind: SymbolKind::Star, size: 8.0, fill: Some(RED), stroke: BLUE },
                Item::Text { at: [100.0, 90.0], text: "label".to_string(), style: text_style() },
            ],
        )],
    );
    let mut list = list_of(vec![back, curves, Layer::new("empty")]);
    list.background = Some(Colour::rgb(255, 255, 255));
    list
}

/// Breaks if: `data-key` is not written, or an empty layer is dropped.
#[test]
fn the_svg_is_well_formed_and_every_keyed_element_is_a_group() {
    let svg = write(&sample(), None).expect("a valid list");
    let (nodes, _) = walk(&svg);
    let keyed = nodes_named(&nodes, "g").into_iter().filter(|a| attr(a, "data-key").is_some()).count();
    assert_eq!(keyed, 3, "one group per keyed element");
    let layers: Vec<&str> = nodes_named(&nodes, "g").into_iter().filter_map(|a| attr(a, "data-layer")).collect();
    assert_eq!(layers, ["back", "curves", "empty"], "an empty layer is still there");
}

/// Breaks if: `<` is not escaped.
#[test]
fn the_svg_escapes_a_title() {
    let weird = "a<b & \"c\" 'd'";
    let mut list = sample();
    list.title = weird.to_string();
    list.layers[0].elements[0].title = weird.to_string();
    list.layers[0].elements[0].data.push(("note".to_string(), weird.to_string()));
    let svg = write(&list, None).expect("a valid list");
    let (nodes, titles) = walk(&svg);
    assert_eq!(titles, [weird, weird]);
    let group = nodes_named(&nodes, "g").into_iter().find(|a| attr(a, "data-note").is_some()).expect("the note");
    assert_eq!(attr(group, "data-note"), Some(weird));
    assert_eq!(escape("\u{1}x"), "x", "a control character XML 1.0 cannot carry is dropped");
}

/// Breaks if: the key is written as it is (the NUL disappears at escaping).
#[test]
fn a_nul_in_a_key_is_written_as_the_symbol_for_null() {
    let mut list = sample();
    list.layers[0].elements[0].key = Some("store\0uri".to_string());
    let svg = write(&list, None).expect("a valid list");
    let (nodes, _) = walk(&svg);
    let keys: Vec<&str> = nodes_named(&nodes, "g").into_iter().filter_map(|a| attr(a, "data-key")).collect();
    assert!(keys.contains(&"store\u{2400}uri"), "{keys:?}");
}

/// Breaks if: a colour is written as a class.
#[test]
fn colours_are_attributes_never_classes() {
    let mut list = sample();
    list.layers[0].elements.push(Element::keyed(
        "eml:///half",
        "bar",
        vec![Item::Rect {
            rect: Rect { x: 0.0, y: 0.0, w: 5.0, h: 5.0 },
            corner: 0.0,
            fill: Some(Paint::Solid(Colour::rgba(0, 255, 0, 128))),
            stroke: None,
        }],
    ));
    let svg = write(&list, None).expect("a valid list");
    assert!(!svg.contains("class="), "no class anywhere");
    let (nodes, _) = walk(&svg);
    let rects = nodes_named(&nodes, "rect");
    assert!(rects.iter().any(|a| attr(a, "fill") == Some("#ff0000")), "the red rectangle");
    assert!(
        rects
            .iter()
            .any(|a| attr(a, "fill") == Some("#00ff00") && attr(a, "fill-opacity") == Some("0.502")),
        "alpha 128 is 0.502"
    );
}

fn sector_list(end: f64) -> DisplayList {
    let sector = Item::Sector {
        centre: [50.0, 50.0],
        inner: 10.0,
        outer: 20.0,
        start: 0.0,
        end,
        fill: Some(Paint::Solid(RED)),
        stroke: None,
    };
    list_of(vec![layer_of("l", vec![Element::keyed("s", "sector", vec![sector])])])
}

/// SVG 1.1 section 8.3.8 (elliptical arcs, sweep flag). Breaks if: the sweep flag of the arcs is inverted.
#[test]
fn a_sector_path_has_two_arcs() {
    let svg = write(&sector_list(std::f64::consts::FRAC_PI_2), None).expect("a valid list");
    let (nodes, _) = walk(&svg);
    let path = nodes_named(&nodes, "path")[0];
    assert_eq!(attr(path, "d"), Some("M70,50 A20,20 0 0 1 50,70 L50,60 A10,10 0 0 0 60,50 Z"));
    assert_eq!(attr(path, "fill-rule"), None);
}

/// A full circle is two half arcs: an arc whose end is its start draws nothing.
/// Breaks if: the ring is written with one arc per circle.
#[test]
fn a_whole_ring_is_two_half_arcs_and_even_odd() {
    let svg = write(&sector_list(std::f64::consts::TAU), None).expect("a valid list");
    let (nodes, _) = walk(&svg);
    let path = nodes_named(&nodes, "path")[0];
    assert_eq!(attr(path, "d").expect("d").matches('A').count(), 4);
    assert_eq!(attr(path, "fill-rule"), Some("evenodd"));
}

/// Breaks if: a pattern is written for each item instead of each style.
#[test]
fn a_hatch_pattern_is_written_once_per_style() {
    let one = Hatch { colour: BLUE, spacing: 4.0, angle: 45.0, width: 1.0 };
    let two = Hatch { colour: RED, spacing: 6.0, angle: 0.0, width: 1.0 };
    let triangle = |hatch: Hatch| Item::Polygon {
        points: vec![[0.0, 0.0], [10.0, 0.0], [5.0, 10.0]],
        fill: Some(Paint::Hatch(hatch)),
        stroke: None,
    };
    let list = list_of(vec![layer_of(
        "l",
        vec![
            Element::keyed("a", "cell", vec![triangle(one)]),
            Element::keyed("b", "cell", vec![triangle(two)]),
            Element::keyed("c", "cell", vec![triangle(one)]),
        ],
    )]);
    let svg = write(&list, None).expect("a valid list");
    let (nodes, _) = walk(&svg);
    let ids: Vec<&str> = nodes_named(&nodes, "pattern").into_iter().filter_map(|a| attr(a, "id")).collect();
    assert_eq!(ids, ["hatch-0", "hatch-1"]);
    let fills: Vec<&str> = nodes_named(&nodes, "polygon").into_iter().filter_map(|a| attr(a, "fill")).collect();
    assert_eq!(fills, ["url(#hatch-0)", "url(#hatch-1)", "url(#hatch-0)"]);
}

fn test_meta() -> Metadata {
    Metadata {
        element: "test-view".to_string(),
        namespace: "urn:energyml:test-view".to_string(),
        format: 3,
        version: "0.3.0".to_string(),
        body: r#"{"a":"<&>\"'"}"#.to_string(),
    }
}

/// Breaks if: the body is not unescaped on reading.
#[test]
fn metadata_reads_back_what_was_written() {
    let meta = test_meta();
    let svg = write(&sample(), Some(&meta)).expect("a valid list");
    assert_eq!(read_metadata(&svg, "test-view"), Ok(meta));
}

/// Breaks if: only the five predefined entities are recognised (an editor that
/// saved the file again may have written character references).
#[test]
fn read_metadata_unescapes_character_references() {
    let svg = r#"<svg><metadata><test-view xmlns="urn:x" format="1" version="0.1">a&#60;b&#x3E;c</test-view></metadata></svg>"#;
    let meta = read_metadata(svg, "test-view").expect("readable");
    assert_eq!(meta.body, "a<b>c");
    assert_eq!((meta.format, meta.version.as_str(), meta.namespace.as_str()), (1, "0.1", "urn:x"));
}

/// Breaks if: the element is searched as a prefix (`<test-view-x` is not `<test-view`).
#[test]
fn read_metadata_says_what_is_missing() {
    assert_eq!(
        read_metadata("<svg/>", "test-view"),
        Err(MetadataError::Missing { element: "test-view".to_string() })
    );
    let other = r#"<svg><metadata><test-view-x xmlns="urn:x" format="1" version="0.1">b</test-view-x></metadata></svg>"#;
    assert_eq!(
        read_metadata(other, "test-view"),
        Err(MetadataError::Missing { element: "test-view".to_string() })
    );
}

/// ADR-0005, refuse rather than draw. Breaks if: a NaN is written as 0.
#[test]
fn a_non_finite_coordinate_is_refused_by_name() {
    let ok = Element::keyed("ok", "curve", vec![Item::Polyline { points: vec![[0.0, 0.0], [1.0, 1.0]], stroke: Stroke::solid(RED, 1.0) }]);
    let bad = Element::keyed(
        "bad",
        "curve",
        vec![Item::Polyline { points: vec![[0.0, 0.0], [f64::NAN, 1.0]], stroke: Stroke::solid(RED, 1.0) }],
    );
    let list = list_of(vec![layer_of("curves", vec![ok, bad])]);
    assert_eq!(
        write(&list, None),
        Err(DrawError::BadItem { layer: "curves".to_string(), element: 1, why: "a coordinate is not finite" })
    );
    assert!(list.validate().is_err(), "validate says it before write does");
}

/// Breaks if: only the first character of a name is checked.
#[test]
fn a_bad_data_attribute_name_is_refused() {
    for name in ["Value", "key", "a b", "x\"y", "kind", "layer", ""] {
        let mut list = sample();
        list.layers[0].elements[0].data.push((name.to_string(), "1".to_string()));
        assert!(
            matches!(write(&list, None), Err(DrawError::BadName { what: "data attribute", .. })),
            "`{name}` must be refused"
        );
    }
    let mut list = sample();
    list.layers[0].elements[0].data.push(("row-index".to_string(), "1".to_string()));
    assert!(write(&list, None).is_ok(), "a good name passes");
}

/// Breaks if: the pairs are separated by a comma or the letter is separated from its pair.
#[test]
fn path_commands_are_the_svg_d() {
    let path = PathData {
        segments: vec![
            Segment::MoveTo([10.0, 20.0]),
            Segment::LineTo([30.0, 40.256]),
            Segment::CubicTo { c1: [1.0, 2.0], c2: [3.0, 4.0], to: [5.0, 6.0] },
            Segment::Close,
        ],
    };
    assert_eq!(path.to_commands(2), "M10,20 L30,40.26 C1,2 3,4 5,6 Z");
    let list = list_of(vec![layer_of(
        "l",
        vec![Element::keyed("p", "edge", vec![Item::Path { path, fill: None, stroke: Some(Stroke::solid(RED, 1.0)) }])],
    )]);
    let svg = write(&list, None).expect("a valid list");
    let (nodes, _) = walk(&svg);
    assert_eq!(attr(nodes_named(&nodes, "path")[0], "d"), Some("M10,20 L30,40.26 C1,2 3,4 5,6 Z"));
}

/// Breaks if: the layers are walked backwards.
#[test]
fn keys_come_in_drawing_order() {
    let rect = || Item::Rect { rect: Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }, corner: 0.0, fill: None, stroke: None };
    let list = list_of(vec![
        layer_of("one", vec![Element::keyed("a", "x", vec![rect()]), Element::new(vec![rect()]), Element::keyed("b", "x", vec![rect()])]),
        layer_of("two", vec![Element::keyed("c", "x", vec![rect()])]),
    ]);
    assert_eq!(list.keys(), ["a", "b", "c"]);
    assert_eq!(list.layer("two").map(|l| l.elements.len()), Some(1));
    assert!(list.layer("three").is_none());
}

fn raster_list() -> DisplayList {
    let raster = Raster::from_rgba(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]).expect("2 x 1 x 4 bytes");
    let image = Item::Image { rect: Rect { x: 0.0, y: 0.0, w: 20.0, h: 10.0 }, raster: Arc::new(raster) };
    list_of(vec![layer_of("l", vec![Element::keyed("img", "image", vec![image])])])
}

/// ADR-0005: a missing backend is `FeatureNotEnabled`, never an empty result.
/// Breaks if: the group is written without its image.
#[cfg(not(feature = "png"))]
#[test]
fn a_raster_without_the_png_feature_is_refused() {
    assert!(matches!(
        write(&raster_list(), None),
        Err(DrawError::FeatureNotEnabled { feature: "png", .. })
    ));
}

/// PNG specification section 5.2: the signature, in base64 `iVBORw0KGgo`.
/// Breaks if: `xmlns:xlink` is not declared (the XML stops reading).
#[cfg(feature = "png")]
#[test]
fn a_raster_is_embedded_as_a_png_image() {
    let svg = write(&raster_list(), None).expect("a valid list");
    let (nodes, _) = walk(&svg);
    let root = &nodes[0].1;
    assert_eq!(attr(root, "xmlns:xlink"), Some("http://www.w3.org/1999/xlink"));
    let image = nodes_named(&nodes, "image")[0];
    assert!(attr(image, "xlink:href").expect("href").starts_with("data:image/png;base64,iVBORw0KGgo"));
    assert_eq!(attr(image, "preserveAspectRatio"), Some("none"));
}
