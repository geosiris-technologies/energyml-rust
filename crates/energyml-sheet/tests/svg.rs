//! The sheet's picture: `energyml_sheet::picture::of_layout` read as a display list (positions, keys, colours),
//! and `energyml_sheet::svg::write` as the one SVG writer's output.
//!
//! The SVG is still read back by a parser that is not this crate's own code (the triangulation rule, ADR-0011), so
//! a mistake in the escaping or the tag balancing is caught by a reader with no stake in the answer. `quick-xml`
//! is already in the workspace's `Cargo.lock` (`energyml-storage`), so this adds nothing new to it.

use energyml_domain::color::Rgba;
use energyml_sheet::draw::{self, Colour, DisplayList, Element, Item, Paint};
use energyml_sheet::layout::{
    Board, HeightGoal, Interval, IntervalTrack, Layout, MdTrack, Params, Track, compute,
};
use energyml_sheet::picture::{PictureStyle, of_layout};
use quick_xml::Reader;
use quick_xml::events::Event;

fn interval(top: f64, base: f64, text: Option<&str>, colour: Option<Rgba>) -> Interval {
    Interval {
        md_top: top,
        md_base: base,
        text: text.map(str::to_string),
        colour,
        uri: None,
        row: None,
    }
}

/// A small but representative board: an MD column, a filled-and-labelled
/// stratigraphy column with an untruncated and a blank interval, and a
/// text-only column that needs wrapping - enough to exercise every branch
/// the translation has (a fill, a wrapped multi-line text, a blank cell, MD
/// ticks, the header row).
fn sample_board() -> Board {
    let red = Rgba {
        r: 1.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    let strata = IntervalTrack::new(
        "Stratigraphy",
        vec![
            interval(0.0, 10.0, Some("Bartonian"), Some(red)),
            interval(10.0, 20.0, None, None),
        ],
        vec!["fill".to_string(), "text".to_string()],
    );
    let lithology = IntervalTrack::new(
        "Lithology",
        vec![interval(
            0.0,
            20.0,
            Some("Marnes et caillasses du Lutetien, avec des passages sableux & \"argileux\""),
            None,
        )],
        vec!["text".to_string()],
    )
    .with_max_chars(20);
    Board {
        title: Some("Well <GTH-1>".to_string()),
        tracks: vec![
            Track::Md(MdTrack {
                header: "Depth (m)".to_string(),
            }),
            Track::Intervals(strata),
            Track::Intervals(lithology),
        ],
    }
}

fn params(proportion: &str, height: HeightGoal) -> Params {
    Params {
        proportion: proportion.to_string(),
        height,
        page_width_mm: 180.0,
        text_size_pt: 8.0,
    }
}

fn laid(board: &Board, proportion: &str, height: HeightGoal) -> Layout {
    compute(board, &params(proportion, height)).expect("a valid board")
}

fn layer<'a>(list: &'a DisplayList, name: &str) -> &'a [Element] {
    &list.layer(name).unwrap_or_else(|| panic!("a layer `{name}`")).elements
}

fn of_kind<'a>(list: &'a DisplayList, layer_name: &str, kind: &str) -> Vec<&'a Element> {
    layer(list, layer_name)
        .iter()
        .filter(|element| element.kind == kind)
        .collect()
}

/// Every text of an element: its words, its anchor point and its size.
fn texts(element: &Element) -> Vec<(&str, [f64; 2], f64)> {
    element
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Text { at, text, style } => Some((text.as_str(), *at, style.size)),
            _ => None,
        })
        .collect()
}

fn data<'a>(element: &'a Element, name: &str) -> Option<&'a str> {
    element
        .data
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

/// Parse `svg` with `quick-xml`, panicking on the first error - that is the test. Returns the `data-layer` names
/// seen, in order, and checks every tag was balanced.
fn check_well_formed(svg: &str) -> Vec<String> {
    let mut reader = Reader::from_str(svg);
    let mut depth: i64 = 0;
    let mut layers = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(Event::Start(start)) => {
                depth += 1;
                if start.name().as_ref() == b"g" {
                    for attribute in start.attributes().flatten() {
                        if attribute.key.as_ref() == b"data-layer" {
                            layers.push(String::from_utf8_lossy(attribute.value.as_ref()).into_owned());
                        }
                    }
                }
            }
            Ok(Event::End(_)) => depth -= 1,
            Ok(_) => {}
            Err(error) => panic!(
                "the SVG `energyml_sheet::svg::write` produced is not well-formed XML: {error}\n---\n{svg}"
            ),
        }
    }
    assert_eq!(depth, 0, "every start tag must have a matching end tag");
    layers
}

/// **The sheet's SVG is the one writer's, over the translation: nothing else writes it.**
///
/// To break it: give `svg::write` a serialiser of its own again, or let it change the list on the way.
#[test]
fn the_svg_is_the_one_writers_over_the_translation() {
    let layout = laid(&sample_board(), "readable", HeightGoal::Total(150.0));
    let list = of_layout(&layout, &PictureStyle::default());
    let by_the_writer = draw::svg::write(&list, None).expect("a drawable list");
    assert_eq!(
        energyml_sheet::svg::write(&layout).expect("an SVG"),
        by_the_writer,
        "the sheet's SVG is draw::svg::write on of_layout"
    );

    let layers = check_well_formed(&by_the_writer);
    assert_eq!(
        layers,
        vec!["header", "sections", "ruler", "tracks"],
        "readable is a deformed mode: its ruler is drawn"
    );
    // Break it: serialise the layout in `svg.rs` by hand.
}

#[test]
fn ampersands_and_angle_brackets_in_user_text_are_escaped() {
    // What would break this: the writer missing one of the five XML special characters.
    let layout = laid(&sample_board(), "to-scale", HeightGoal::Total(100.0));
    let svg = energyml_sheet::svg::write(&layout).expect("an SVG");

    // The well-formedness check alone already proves the escaping worked - an unescaped `<` or `&` would
    // desynchronise the parser. The second assertion is the more direct one: the raw text must not appear
    // verbatim in the markup.
    check_well_formed(&svg);
    assert!(
        !svg.contains("Well <GTH-1>"),
        "the title's angle brackets must be escaped"
    );
    assert!(
        svg.contains("Well &lt;GTH-1&gt;"),
        "escaped as entities, not dropped"
    );
    // And the list itself holds the raw words: the escaping is the writer's, once.
    let list = of_layout(&layout, &PictureStyle::default());
    assert_eq!(texts(&layer(&list, "header")[0])[0].0, "Well <GTH-1>");
    // Break it: write a text without going through the writer's `escape`.
}

#[test]
fn every_cell_carries_its_md_bounds_and_a_blank_one_says_so() {
    let layout = laid(&sample_board(), "readable", HeightGoal::Total(150.0));
    let list = of_layout(&layout, &PictureStyle::default());
    let cells = of_kind(&list, "tracks", "cell");
    // Two interval columns over two sections.
    assert_eq!(cells.len(), 4);
    for (cell, section) in cells.iter().zip(layout.sections.iter().cycle()) {
        assert_eq!(
            data(cell, "md-top").and_then(|text| text.parse::<f64>().ok()),
            Some(section.md_top)
        );
        assert_eq!(
            data(cell, "md-base").and_then(|text| text.parse::<f64>().ok()),
            Some(section.md_base)
        );
    }
    // The second stratigraphy interval names neither colour nor text: the second cell, and only it.
    let blank: Vec<usize> = cells
        .iter()
        .enumerate()
        .filter(|(_, cell)| data(cell, "blank") == Some("true"))
        .map(|(at, _)| at)
        .collect();
    assert_eq!(blank, vec![1]);
    // Break it: stop writing `blank`, or write it on every cell.
}

/// **A cell is placed where the layout put its section, and painted with its own colour.**
///
/// The colour is an independent literal (pure red is 255, 0, 0), not read back through the translation's own
/// conversion. To break it: offset the body by the wrong height, or paint a cell with the colour of its neighbour.
#[test]
fn a_cell_sits_on_its_section_and_takes_its_colour() {
    let layout = laid(&sample_board(), "readable", HeightGoal::Total(150.0));
    let list = of_layout(&layout, &PictureStyle::default());
    let cells = of_kind(&list, "tracks", "cell");
    for (cell, section) in cells.iter().zip(layout.sections.iter().cycle()) {
        let Item::Rect { rect, .. } = &cell.items[0] else {
            panic!("a cell starts with its rectangle");
        };
        assert!((rect.y - (layout.header_height_mm + section.y_top_mm)).abs() < 1e-9);
        assert!((rect.h - (section.y_base_mm - section.y_top_mm)).abs() < 1e-9);
    }
    let Item::Rect { fill, .. } = &cells[0].items[0] else {
        unreachable!()
    };
    assert_eq!(*fill, Some(Paint::Solid(Colour::rgba(255, 0, 0, 255))));
    let Item::Rect { fill, .. } = &cells[1].items[0] else {
        unreachable!()
    };
    assert_eq!(*fill, None, "a blank cell is unpainted, never filled in");
    // Break it: translate the body by `y_top_mm` alone, or default a missing colour to white.
}

/// **The section boundaries, and the ruler's connectors, come from the layout's own numbers.**
///
/// To break it: draw the boundaries from the cells' text heights, or drop the base line.
#[test]
fn the_boundaries_and_the_ruler_follow_the_layout() {
    for proportion in ["readable", "to-scale"] {
        let layout = laid(&sample_board(), proportion, HeightGoal::Total(150.0));
        let list = of_layout(&layout, &PictureStyle::default());
        let boundaries = of_kind(&list, "sections", "boundary");
        assert_eq!(boundaries.len(), layout.sections.len() + 1, "{proportion}");
        let ys: Vec<f64> = boundaries
            .iter()
            .map(|element| match &element.items[0] {
                Item::Polyline { points, .. } => points[0][1],
                other => panic!("a line, got {other:?}"),
            })
            .collect();
        let mut expected: Vec<f64> = layout
            .sections
            .iter()
            .map(|section| layout.header_height_mm + section.y_top_mm)
            .collect();
        expected.push(layout.header_height_mm + layout.sections.last().expect("a section").y_base_mm);
        assert_eq!(ys, expected, "{proportion}");
        assert_eq!(
            list.layer("ruler").is_some(),
            layout.deformed,
            "{proportion}: a ruler exactly when the mode deforms"
        );
        if let Some(ruler) = list.layer("ruler") {
            let links = ruler.elements.iter().filter(|e| e.kind == "ruler-link").count();
            assert_eq!(links, layout.ruler.as_ref().expect("a ruler").y_true_mm.len());
        }
    }
    // Break it: skip the last boundary, or draw a ruler for every mode.
}

/// The root `<svg>`'s attribute `name`, as a number - `width="210.000mm"` reads 210.
fn root_number(svg: &str, name: &str) -> f64 {
    let at = svg.find("<svg ").expect("a root element");
    let tag = &svg[at..at + svg[at..].find('>').expect("closed")];
    let key = format!(" {name}=\"");
    let from = tag.find(&key).expect("the attribute") + key.len();
    let value = &tag[from..from + tag[from..].find('"').expect("quoted")];
    value.trim_end_matches("mm").parse().expect("a number")
}

/// **The font size is written in millimetres, the unit of the `viewBox`.**
///
/// `font-size="8pt"` under a millimetre `viewBox` is 10.7 mm tall - an SVG turns `pt` into user units as if one
/// were a CSS pixel, then scales like everything else - four times the room the layout reserved (the first sheet
/// drawn from Lutetia). Every text of the list carries 8 pt as 2.822 mm, in a list in millimetres.
/// To break it: size a text in points again.
#[test]
fn the_font_size_is_in_the_viewbox_unit() {
    let layout = laid(&sample_board(), "readable", HeightGoal::Total(150.0));
    let list = of_layout(&layout, &PictureStyle::default());
    assert_eq!(list.unit, draw::PageUnit::Millimetre);
    let mut seen = 0;
    for layer in &list.layers {
        for element in &layer.elements {
            for (words, _, size) in texts(element) {
                assert!(
                    (size - 8.0 * 25.4 / 72.0).abs() < 1e-9,
                    "`{words}`: 8 pt is 2.822 mm, the list says {size}"
                );
                seen += 1;
            }
        }
    }
    assert!(seen > 6, "the texts were read: {seen}");
    // The written SVG says the same, on each text, with the unit of the page.
    let svg = energyml_sheet::svg::write(&layout).expect("an SVG");
    assert!(svg.contains("width=\"180.000mm\""));
    assert!(svg.contains("font-size=\"2.822\""), "{svg}");
    assert!(!svg.contains("pt\""), "no size in points");
}

/// **A row wider than the page widens it; it is never squeezed.**
///
/// The first version scaled every row into the page width: a column cut for 32 characters was drawn at a third of
/// that, and every description ran over its neighbour. Eight text columns of 32 characters cannot fit in 180 mm.
/// To break it: scale the row down to `page_width_mm` again.
#[test]
fn a_row_wider_than_the_page_widens_it() {
    let tracks: Vec<Track> = (0..8)
        .map(|n| {
            Track::Intervals(IntervalTrack::new(
                format!("column {n}"),
                vec![interval(0.0, 10.0, Some("a description"), None)],
                vec!["text".to_string()],
            ))
        })
        .collect();
    let board = Board { title: None, tracks };
    let layout = laid(&board, "to-scale", HeightGoal::Scale(10.0));
    let list = of_layout(&layout, &PictureStyle::default());
    assert!(
        list.size[0] > 180.0,
        "eight 32-character columns in {} mm: squeezed",
        list.size[0]
    );
    // The page width is the SVG's, to the millimetre.
    let svg = energyml_sheet::svg::write(&layout).expect("an SVG");
    assert!((root_number(&svg, "width") - list.size[0]).abs() < 1e-3);
    // And a narrow row still fills the page asked for.
    let narrow = of_layout(
        &laid(&sample_board(), "to-scale", HeightGoal::Scale(10.0)),
        &PictureStyle::default(),
    );
    assert!(narrow.size[0] >= 180.0);
    // Break it: scale the row down to `page_width_mm` again.
}

/// **Every header line sits inside the header the layout reserved.**
///
/// The column headers were written one line too low - below the header's height, over the body's first row (the
/// first sheet drawn from Lutetia). To break it: start the column headers one line lower again.
#[test]
fn the_header_lines_stay_inside_the_header() {
    let layout = laid(&sample_board(), "readable", HeightGoal::Total(150.0));
    let list = of_layout(&layout, &PictureStyle::default());
    let mut seen = 0;
    for element in layer(&list, "header") {
        for (words, at, _) in texts(element) {
            assert!(
                at[1] <= layout.header_height_mm + 1e-9,
                "`{words}` at y = {} mm, the header is {} mm",
                at[1],
                layout.header_height_mm
            );
            seen += 1;
        }
    }
    assert!(seen >= 4, "the title, the mode note and the column headers were read: {seen}");
    // Break it: start the column headers one line lower again.
}

/// **Every depth label lies inside the picture, the well's base included.**
///
/// A depth label hangs below its line, and the last line is the page's bottom edge: the base of the well was
/// written below the picture and nobody saw it (the Lutetia sheet of `BSS000NSYT`). To break it: hang the last
/// label below its line again.
#[test]
fn every_depth_label_lies_inside_the_body() {
    for proportion in ["readable", "to-scale"] {
        let layout = laid(&sample_board(), proportion, HeightGoal::Scale(10.0));
        let list = of_layout(&layout, &PictureStyle::default());
        let mut depths = Vec::new();
        for tick in of_kind(&list, "tracks", "tick") {
            let (words, at, _) = texts(tick)[0];
            let md: f64 = data(tick, "md").expect("a depth").parse().expect("a number");
            assert!(
                at[1] > layout.header_height_mm
                    && at[1] <= layout.header_height_mm + layout.body_height_mm + 1e-9,
                "{proportion}: the label `{words}` of MD {md} at y = {} mm, the body spans {} to {}",
                at[1],
                layout.header_height_mm,
                layout.header_height_mm + layout.body_height_mm
            );
            depths.push(md);
        }
        assert_eq!(
            depths,
            vec![0.0, 10.0, 20.0],
            "{proportion}: every bound is labelled, the base included"
        );
    }
    // Break it: hang the last label below its line again.
}

/// **What a user can point at is found by a full key; the rest is a decoration.**
///
/// A cell names the uri of the interval it was drawn from; a depth label, the object the sheet is of. Two cells
/// never share a key. To break it: drop the column or the depth from the key, or key the boundaries.
#[test]
fn keys_are_full_uris_with_what_tells_two_elements_apart() {
    let frame = "eml:///resqml22.WellboreFrameRepresentation(33333333-3333-4333-8333-333333333333)";
    let unit = "eml:///resqml22.StratigraphicUnitInterpretation(44444444-4444-4444-8444-444444444444)";
    let mut with_uri = interval(0.0, 10.0, Some("Bartonian"), None);
    with_uri.uri = Some(unit.to_string());
    with_uri.row = Some("12".to_string());
    let board = Board {
        title: None,
        tracks: vec![
            Track::Md(MdTrack {
                header: "MD".to_string(),
            }),
            Track::Intervals(IntervalTrack::new(
                "Units",
                vec![with_uri, interval(10.0, 20.0, Some("Lutetian"), None)],
                vec!["text".to_string()],
            )),
        ],
    };
    let layout = laid(&board, "to-scale", HeightGoal::Total(100.0));

    let keyed = of_layout(
        &layout,
        &PictureStyle {
            object: Some(frame.to_string()),
            ..PictureStyle::default()
        },
    );
    let expected: Vec<String> = vec![
        format!("{frame}#md=0"),
        format!("{frame}#md=10"),
        format!("{frame}#md=20"),
        format!("{unit}#col=1&md=0&row=12"),
        format!("{frame}#col=1&md=10"),
    ];
    assert_eq!(
        keyed.keys(),
        expected.iter().map(String::as_str).collect::<Vec<&str>>(),
        "the depth labels by the object, a cell by its own uri else the object"
    );
    let all = keyed.keys();
    let distinct: std::collections::BTreeSet<&&str> = all.iter().collect();
    assert_eq!(distinct.len(), all.len(), "no two elements share a key");

    // Without an object, only what names its own uri is keyed.
    let bare = of_layout(&layout, &PictureStyle::default());
    assert_eq!(bare.keys(), vec![format!("{unit}#col=1&md=0&row=12").as_str()]);
    // The SVG carries the key where a face finds it.
    let svg = draw::svg::write(&keyed, None).expect("an SVG");
    assert!(svg.contains(&format!("data-key=\"{unit}#col=1&amp;md=0&amp;row=12\"")));
    // Break it: build the cell key from the depth alone.
}
