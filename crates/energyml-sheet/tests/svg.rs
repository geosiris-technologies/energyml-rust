//! Sheet design: *"the SVG is read back"* — read back by a
//! parser that is not this crate's own code (the triangulation rule, ADR-0011), so a mistake in `energyml_sheet::svg::write`'s
//! escaping or tag balancing is caught by a reader with no stake in the
//! answer. `quick-xml` is already in the workspace's `Cargo.lock`
//! (`energyml-storage`), so this adds nothing new to it.

use energyml_domain::color::Rgba;
use energyml_sheet::layout::{Board, HeightGoal, Interval, IntervalTrack, MdTrack, Params, Track, compute};
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
/// text-only column that needs wrapping — enough to exercise every branch
/// `svg::write` has (a fill, a wrapped multi-line text, a blank cell, MD
/// ticks, the header row).
fn sample_board() -> Board {
    let red = Rgba {
        r: 0.8,
        g: 0.2,
        b: 0.1,
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

/// Parse `svg` with `quick-xml`, panicking on the first error — that is the
/// test. Returns the number of `<g id="…">` elements seen with `id="header"`
/// and with `id="body"`, and whether every tag was balanced.
fn check_well_formed(svg: &str) -> (usize, usize) {
    let mut reader = Reader::from_str(svg);
    let mut depth: i64 = 0;
    let mut header_groups = 0usize;
    let mut body_groups = 0usize;
    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(Event::Start(start)) => {
                depth += 1;
                if start.name().as_ref() == b"g" {
                    for attribute in start.attributes().flatten() {
                        if attribute.key.as_ref() == b"id" {
                            match attribute.value.as_ref() {
                                b"header" => header_groups += 1,
                                b"body" => body_groups += 1,
                                _ => {}
                            }
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
    (header_groups, body_groups)
}

#[test]
fn the_svg_is_well_formed_xml_with_a_header_and_a_body_group() {
    let board = sample_board();
    let params = Params {
        proportion: "readable".to_string(),
        height: HeightGoal::Total(150.0),
        page_width_mm: 180.0,
        text_size_pt: 8.0,
    };
    let layout = compute(&board, &params).expect("a valid board");
    let svg = energyml_sheet::svg::write(&layout);

    let (header_groups, body_groups) = check_well_formed(&svg);
    assert_eq!(
        header_groups, 1,
        "exactly one `<g id=\"header\">`, per the design"
    );
    assert_eq!(body_groups, 1, "exactly one `<g id=\"body\">`, per the design");
    assert!(
        svg.starts_with("<?xml"),
        "the document must open with an XML declaration"
    );
}

#[test]
fn ampersands_and_angle_brackets_in_user_text_are_escaped() {
    // What would break this: `svg::escape` missing one of the five XML
    // special characters, or a data-* attribute value built without going
    // through it.
    let board = sample_board(); // its title and lithology text both carry `<`, `>`, `&`, `"`
    let params = Params {
        proportion: "to-scale".to_string(),
        height: HeightGoal::Total(100.0),
        page_width_mm: 180.0,
        text_size_pt: 8.0,
    };
    let layout = compute(&board, &params).expect("a valid board");
    let svg = energyml_sheet::svg::write(&layout);

    // The well-formedness check alone already proves the escaping worked —
    // an unescaped `<` or `&` would desynchronise the parser above. This
    // second assertion is the more direct one: the raw, unescaped text must
    // not appear verbatim in the markup.
    check_well_formed(&svg);
    assert!(
        !svg.contains("Well <GTH-1>"),
        "the title's angle brackets must be escaped"
    );
    assert!(
        svg.contains("Well &lt;GTH-1&gt;"),
        "escaped as entities, not dropped"
    );
}

#[test]
fn every_cell_carries_its_md_bounds_and_a_blank_one_says_so() {
    let board = sample_board();
    let params = Params {
        proportion: "readable".to_string(),
        height: HeightGoal::Total(150.0),
        page_width_mm: 180.0,
        text_size_pt: 8.0,
    };
    let layout = compute(&board, &params).expect("a valid board");
    let svg = energyml_sheet::svg::write(&layout);
    check_well_formed(&svg);

    assert!(svg.contains("data-md-top="), "data-md-top must be written");
    assert!(svg.contains("data-md-base="), "data-md-base must be written");
    assert!(
        svg.contains("data-blank=\"true\""),
        "the second stratigraphy interval names neither colour nor text"
    );
}

/// The root `<svg>`'s attribute `name`, as a number — `width="210.000mm"`
/// reads 210.
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
/// `font-size="8pt"` under a millimetre `viewBox` is 10.7 mm tall — an SVG
/// turns `pt` into user units as if one were a CSS pixel, then scales like
/// everything else — four times the room the layout reserved (the first
/// sheet drawn from Lutetia). To break it: write `…pt` again.
#[test]
fn the_font_size_is_in_the_viewbox_unit() {
    let params = Params {
        proportion: "readable".to_string(),
        height: HeightGoal::Total(150.0),
        page_width_mm: 180.0,
        text_size_pt: 8.0,
    };
    let svg = energyml_sheet::svg::write(&compute(&sample_board(), &params).expect("a valid board"));
    let size = root_number(&svg, "font-size");
    assert!(
        (size - 8.0 * 25.4 / 72.0).abs() < 1e-3,
        "8 pt is 2.822 mm, the file says {size}"
    );
}

/// **A row wider than the page widens the page; it is never squeezed.**
///
/// The first version scaled every row into the page width: a column cut for
/// 32 characters was drawn at a third of that, and every description ran over
/// its neighbour. Eight text columns of 32 characters cannot fit in 180 mm.
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
    let params = Params {
        proportion: "to-scale".to_string(),
        height: HeightGoal::Scale(10.0),
        page_width_mm: 180.0,
        text_size_pt: 8.0,
    };
    let svg = energyml_sheet::svg::write(&compute(&board, &params).expect("a valid board"));
    let width = root_number(&svg, "width");
    assert!(
        width > 180.0,
        "eight 32-character columns in {width} mm: squeezed"
    );
    // And a narrow row still fills the page asked for.
    let narrow = energyml_sheet::svg::write(&compute(&sample_board(), &params).expect("a valid board"));
    assert!(root_number(&narrow, "width") >= 180.0);
}

/// **Every header line sits inside the header the layout reserved.**
///
/// The column headers were written one line too low — below the header's
/// height, over the body's first row (the first sheet drawn from Lutetia).
/// To break it: start the column headers one line lower again.
#[test]
fn the_header_lines_stay_inside_the_header() {
    let params = Params {
        proportion: "readable".to_string(),
        height: HeightGoal::Total(150.0),
        page_width_mm: 180.0,
        text_size_pt: 8.0,
    };
    let layout = compute(&sample_board(), &params).expect("a valid board");
    let svg = energyml_sheet::svg::write(&layout);
    let header =
        &svg[svg.find("<g id=\"header\">").expect("a header")..svg.find("<g id=\"body\"").expect("a body")];
    let mut seen = 0;
    for part in header.split("<text ").skip(1) {
        let from = part.find(" y=\"").expect("a y") + 4;
        let y: f64 = part[from..from + part[from..].find('"').unwrap()]
            .parse()
            .unwrap();
        assert!(
            y <= layout.header_height_mm + 1e-9,
            "a header line at y = {y} mm, the header is {} mm",
            layout.header_height_mm
        );
        seen += 1;
    }
    assert!(
        seen >= 3,
        "the mode note and the column headers were read: {seen}"
    );
}

/// **Every depth label lies inside the picture, the well's base included.**
///
/// A depth label hangs below its line, and the last line is the page's
/// bottom edge: the base of the well was written below the picture and
/// nobody saw it (the Lutetia sheet of `BSS000NSYT`). To break it: hang
/// the last label below its line again.
#[test]
fn every_depth_label_lies_inside_the_body() {
    for proportion in ["readable", "to-scale"] {
        let params = Params {
            proportion: proportion.to_string(),
            height: HeightGoal::Scale(10.0),
            page_width_mm: 180.0,
            text_size_pt: 8.0,
        };
        let layout = compute(&sample_board(), &params).expect("a valid board");
        let svg = energyml_sheet::svg::write(&layout);
        let mut depths = Vec::new();
        for part in svg
            .split("<text ")
            .skip(1)
            .filter(|part| part.contains("data-md=\""))
        {
            let from = part.find(" y=\"").expect("a y") + 4;
            let y: f64 = part[from..from + part[from..].find('"').unwrap()]
                .parse()
                .unwrap();
            let from = part.find("data-md=\"").unwrap() + 9;
            let md: f64 = part[from..from + part[from..].find('"').unwrap()]
                .parse()
                .unwrap();
            assert!(
                y > 0.0 && y <= layout.body_height_mm + 1e-9,
                "{proportion}: the label of MD {md} at y = {y} mm, the body is {} mm",
                layout.body_height_mm
            );
            depths.push(md);
        }
        assert_eq!(
            depths,
            vec![0.0, 10.0, 20.0],
            "{proportion}: every bound is labelled, the base included"
        );
    }
}
