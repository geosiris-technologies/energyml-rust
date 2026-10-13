//! The SVG, re-read by a parser that is not this crate's code (`quick-xml`) (ADR-0015).

use crate::layout::{catalogue, id, trace, view, well};
use energyml_section::layout::{Metadata, layout};
use energyml_section::model::*;
use energyml_section::svg;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

/// One element, as the parser sees it: name, attributes (unescaped), and its text.
#[derive(Debug, Clone)]
struct El {
    name: String,
    attrs: Vec<(String, String)>,
    text: String,
    /// The names of the ancestors, nearest last.
    parents: Vec<String>,
}

impl El {
    fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
}

fn start(e: &BytesStart, parents: &[String]) -> El {
    El {
        name: String::from_utf8_lossy(e.name().as_ref()).into_owned(),
        attrs: e
            .attributes()
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
        text: String::new(),
        parents: parents.to_vec(),
    }
}

/// Parse the whole document; any error is a failed test — "the document stays valid XML".
fn parse(svg: &str) -> Vec<El> {
    let mut reader = Reader::from_str(svg);
    let mut out: Vec<El> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    loop {
        match reader.read_event().expect("the SVG is well-formed XML") {
            Event::Start(e) => {
                out.push(start(&e, &names));
                stack.push(out.len() - 1);
                names.push(out.last().unwrap().name.clone());
            }
            Event::Empty(e) => out.push(start(&e, &names)),
            Event::Text(t) => {
                if let Some(&i) = stack.last() {
                    out[i].text.push_str(
                        &t.xml_content(quick_xml::XmlVersion::default())
                            .expect("valid text"),
                    );
                }
            }
            Event::GeneralRef(r) => {
                // `&amp;` and friends arrive as references in this parser; resolve the five.
                if let Some(&i) = stack.last() {
                    let name = r.decode().expect("a name").into_owned();
                    out[i].text.push_str(match name.as_str() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        other => panic!("an unexpected reference &{other};"),
                    });
                }
            }
            Event::End(_) => {
                stack.pop();
                names.pop();
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

fn drawn(cat: &Catalogue, v: &View) -> String {
    svg::write(&layout(cat, v))
}

fn data_keys(els: &[El]) -> Vec<String> {
    els.iter()
        .filter(|e| e.name == "g")
        .filter_map(|e| e.attr("data-key"))
        .map(str::to_string)
        .collect()
}

/// The front end's key as the SVG spells it: the NUL becomes U+2400.
fn spelled(key: &str) -> String {
    key.replace('\0', "\u{2400}")
}

#[test]
fn one_group_per_object_drawn_with_its_key_and_kind() {
    let els = parse(&drawn(&catalogue(items()), &view()));
    let mut keys = data_keys(&els);
    keys.sort();
    let mut expected: Vec<String> = ["A", "B"]
        .iter()
        .map(|n| spelled(&id(n, "WellboreFrameRepresentation").key()))
        .chain(
            ["H", "L"]
                .iter()
                .map(|n| spelled(&id(n, "TriangulatedSetRepresentation").key())),
        )
        .collect();
    expected.sort();
    assert_eq!(
        keys, expected,
        "exactly one <g data-key> per object, no duplicate"
    );
    let a = els
        .iter()
        .find(|e| e.attr("data-key") == Some(&spelled(&id("A", "WellboreFrameRepresentation").key())))
        .unwrap();
    assert_eq!(a.attr("data-kind"), Some("well"));
    assert_eq!(a.attr("data-store"), Some("s1"));
    assert_eq!(
        a.attr("data-uri"),
        Some(id("A", "WellboreFrameRepresentation").uri.as_str())
    );
    // And the key survives as XML: no NUL was written.
    assert!(!drawn(&catalogue(items()), &view()).contains('\0'));
}

#[test]
fn a_hidden_object_has_no_group() {
    let mut v = view();
    v.hidden = vec![
        id("B", "WellboreFrameRepresentation").key(),
        id("H", "TriangulatedSetRepresentation").key(),
    ];
    let out = drawn(&catalogue(items()), &v);
    let keys = data_keys(&parse(&out));
    assert_eq!(keys.len(), 2);
    assert!(!keys.contains(&spelled(&id("B", "WellboreFrameRepresentation").key())));
    // Nor a trace of it in the drawing: no element but the metadata (which holds the view, hence
    // the hidden list itself) names its uri.
    let uri = id("B", "WellboreFrameRepresentation").uri;
    for e in parse(&out).iter().filter(|e| e.name != "section-view") {
        assert!(e.attrs.iter().all(|(_, v)| !v.contains(&uri)), "{e:?}");
    }
}

#[test]
fn colours_are_attributes_never_classes() {
    let mut v = view();
    v.well_colouring = WellColouring::Strata;
    v.fills = vec![FillChoice {
        key: id("H", "TriangulatedSetRepresentation").key(),
        up: false,
        down: true,
    }];
    let out = drawn(&catalogue(items()), &v);
    assert!(
        !out.contains("class="),
        "no class: a class is lost when the SVG leaves its document"
    );
    assert!(!out.contains("<style"), "no stylesheet either");
    let els = parse(&out);
    let strokes: Vec<&str> = els
        .iter()
        .filter(|e| e.name == "polyline")
        .filter_map(|e| e.attr("stroke"))
        .collect();
    assert!(
        strokes.contains(&"#aa0000"),
        "the interval's own colour is a stroke attribute: {strokes:?}"
    );
    assert!(
        strokes.iter().all(|s| s.starts_with('#')),
        "every stroke is a literal colour: {strokes:?}"
    );
    let fills: Vec<&str> = els
        .iter()
        .filter(|e| e.name == "polygon")
        .filter_map(|e| e.attr("fill"))
        .collect();
    assert!(
        !fills.is_empty() && fills.iter().all(|f| f.starts_with('#')),
        "{fills:?}"
    );
}

#[test]
fn a_well_has_a_group_per_interval_with_its_unit() {
    let out = drawn(&catalogue(items()), &view());
    let els = parse(&out);
    let units: Vec<&str> = els.iter().filter_map(|e| e.attr("data-unit")).collect();
    // Two wells, two intervals each.
    assert_eq!(units.iter().filter(|u| **u == "Bartonian").count(), 2);
    assert_eq!(units.iter().filter(|u| **u == "Lutetian").count(), 2);
    let iv = els
        .iter()
        .find(|e| e.attr("data-unit") == Some("Bartonian"))
        .unwrap();
    assert_eq!(
        (iv.attr("data-md-from"), iv.attr("data-md-to")),
        (Some("0"), Some("100"))
    );
    // An interval group sits inside its well's group.
    assert!(iv.parents.iter().any(|p| p == "g"));
}

#[test]
fn a_hostile_title_comes_out_escaped_and_the_document_stays_xml() {
    let nasty = "<&\"'> </svg><script>alert(1)</script>\u{1}";
    let mut a = well("A", 150.0, 12.0, Side::Left);
    a.object.title = nasty.to_string();
    let mut h = trace("H", vec![vec![[0.0, -100.0], [1000.0, -100.0]]]);
    h.object.title = nasty.to_string();
    let mut v = view();
    v.background = "red\" onload=\"x".into();
    let out = drawn(&catalogue(vec![Item::Well(a), Item::Trace(h)]), &v);
    assert!(!out.contains("<script>alert"), "no raw markup from a title");
    assert!(
        !out.contains('\u{1}'),
        "a control character XML cannot carry is dropped"
    );
    let els = parse(&out); // would panic on invalid XML
    // The only script is the drawing's own zoom (`View::zoom_script`, on by
    // default), its text the file it is written from — nothing a title brought.
    let scripts: Vec<_> = els.iter().filter(|e| e.name == "script").collect();
    assert_eq!(scripts.len(), 1, "one script, the zoom's");
    assert!(
        out.contains(include_str!("../src/zoom.js")),
        "the script is zoom.js, byte for byte"
    );
    assert!(
        !out.contains("alert(1)</script>"),
        "the title's script never reaches the document raw"
    );
    // No element carries an attribute the title or the background injected.
    assert!(!els.iter().any(|e| e.attr("onload").is_some()));
    // The parser, undoing the escapes, reads the title exactly (minus the control character).
    let clean = "<&\"'> </svg><script>alert(1)</script>";
    assert!(
        els.iter().any(|e| e.name == "title" && e.text == clean),
        "the group's <title>"
    );
    assert!(
        els.iter()
            .any(|e| e.name == "text" && e.text.starts_with("<&\"'> </svg")),
        "the label, cut to fit"
    );
}

#[test]
fn the_metadata_read_back_give_the_same_inputs_and_view() {
    let cat = catalogue(items());
    let mut v = view();
    v.hidden = vec!["s1\0eml:///x".into()];
    v.colours = vec![("a".into(), "#fff".into())];
    let els = parse(&drawn(&cat, &v));
    let meta = els
        .iter()
        .find(|e| e.name == "section-view")
        .expect("<metadata> holds the data");
    assert!(els.iter().any(|e| e.name == "metadata"));
    assert_eq!(meta.attr("version"), Some(env!("CARGO_PKG_VERSION")));
    let back: Metadata =
        serde_json::from_str(&meta.text).expect("the JSON is intact after the XML round trip");
    assert_eq!(back.inputs, cat.inputs);
    assert_eq!(back.view, v);
    assert_eq!(back.format, FORMAT);
}

/// `svg::read_metadata` is the way back an operation takes, so it is held to a
/// reader that is not this crate's code: what `quick-xml` unescapes is what it
/// unescapes, on text that needs every entity.
#[test]
fn the_metadata_reader_agrees_with_an_xml_parser_and_refuses_by_name() {
    let cat = catalogue(items());
    let mut v = view();
    v.hidden = vec!["s1\0eml:///<&\"'>".into()];
    v.background = "#ffffff".into();
    let out = drawn(&cat, &v);
    let els = parse(&out);
    let meta = els.iter().find(|e| e.name == "section-view").unwrap();
    let theirs: Metadata = serde_json::from_str(&meta.text).unwrap();
    let ours = svg::read_metadata(&out).expect("the metadata reads back");
    assert_eq!(ours, theirs);
    assert_eq!(ours.view.hidden, v.hidden);

    // An editor that re-saved the file with character references.
    let referenced = out.replace("&quot;", "&#34;").replace("&lt;", "&#x3C;");
    assert_ne!(referenced, out, "the test rewrites something");
    assert_eq!(
        svg::read_metadata(&referenced).expect("character references read"),
        ours
    );

    // No metadata, another format: refused by name, never a default.
    let start = out.find("<metadata>").unwrap();
    let end = out.find("</metadata>").unwrap() + "</metadata>".len();
    let stripped = format!("{}{}", &out[..start], &out[end..]);
    assert_eq!(svg::read_metadata(&stripped), Err(svg::MetadataError::Missing));
    let other = out.replacen(&format!("format=\"{FORMAT}\""), "format=\"99\"", 1);
    assert_eq!(
        svg::read_metadata(&other),
        Err(svg::MetadataError::Format {
            found: "99".into(),
            known: FORMAT
        })
    );
    let broken = out.replacen("&quot;", "&bogus;", 1);
    assert!(matches!(
        svg::read_metadata(&broken),
        Err(svg::MetadataError::Unreadable(_))
    ));
}

#[test]
fn the_exaggeration_and_the_warnings_are_written_on_the_drawing() {
    let mut cat = catalogue(items());
    cat.objects = vec![ObjectRow {
        object: id("Z", "TriangulatedSetRepresentation"),
        kind: RowKind::Surface,
        outcome: Outcome::Refused {
            reason: "unreadable".into(),
        },
        fillable: false,
    }];
    let l = layout(&cat, &view());
    let els = parse(&svg::write(&l));
    let ex = els
        .iter()
        .find(|e| e.attr("data-role") == Some("exaggeration"))
        .unwrap();
    assert_eq!(ex.text, l.exaggeration_text);
    assert!(ex.text.contains('×'));
    let warned: Vec<&El> = els
        .iter()
        .filter(|e| e.attr("data-role") == Some("warning"))
        .collect();
    assert_eq!(warned.len(), l.warnings.len());
    assert!(warned.iter().any(|w| w.text.contains("1 objects refused")));
}

#[test]
fn the_plan_is_a_drawing_too() {
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
    let els = parse(&drawn(&cat, &v));
    assert!(els.iter().any(|e| e.attr("data-section") == Some("long")));
    assert_eq!(
        data_keys(&els),
        vec![spelled(&id("A", "WellboreFrameRepresentation").key())]
    );
}

#[test]
fn the_size_of_a_big_drawing_is_the_size_of_its_nodes() {
    // 200 wells of 100 nodes and 20 traces of 200 points: the string, not a stopwatch.
    let wiggle = |n: usize, i: usize| -> Vec<[f64; 2]> {
        (0..n)
            .map(|k| {
                [
                    i as f64 * 5.0 + (k as f64 * 0.37).sin() * 3.0,
                    -(k as f64) * 5.0 - (k as f64 * 0.11).cos(),
                ]
            })
            .collect()
    };
    let mut its = Vec::new();
    for i in 0..200 {
        let mut w = well(&format!("W{i}"), i as f64 * 5.0, 10.0, Side::Left);
        w.path = wiggle(100, i);
        w.md = Some((0..100).map(|k| k as f64 * 5.0).collect());
        w.intervals = (0..5)
            .map(|k| Interval {
                from: k as f64 * 100.0,
                to: k as f64 * 100.0 + 90.0,
                colour: Some("#336699".into()),
                unit: format!("U{k}"),
                unit_uri: None,
            })
            .collect();
        its.push(Item::Well(w));
    }
    for i in 0..20 {
        its.push(Item::Trace(trace(
            &format!("T{i}"),
            vec![
                (0..200)
                    .map(|k| [k as f64 * 5.0, -50.0 * i as f64 + (k as f64 * 0.2).sin() * 5.0])
                    .collect(),
            ],
        )));
    }
    let mut v = view();
    v.window = None;
    v.well_colouring = WellColouring::Strata;
    let out = drawn(&catalogue(its), &v);
    let els = parse(&out);
    assert_eq!(data_keys(&els).len(), 220);
    eprintln!("big drawing: {} bytes ({} elements)", out.len(), els.len());
    assert!(out.len() < 4_000_000, "{}", out.len());
}

/// The root carries the map an overlay reads a pointer through —
/// `data-frame`, `data-window`, `data-scale` — and it is the map the drawing
/// was drawn with: well A's head, `(150, 0)` in the section, mapped through
/// those attributes, lands on the first point of a polyline the SVG draws.
///
/// To make it fail: write the window of another view, or flip the vertical.
#[test]
fn the_root_says_how_a_pixel_reads_as_a_place_of_the_section() {
    let out = drawn(&catalogue(items()), &view());
    let els = parse(&out);
    let root = els.iter().find(|e| e.name == "svg").unwrap();
    let numbers = |name: &str| -> Vec<f64> {
        root.attr(name)
            .unwrap_or_else(|| panic!("no {name}"))
            .split(' ')
            .map(|n| n.parse().unwrap())
            .collect()
    };
    let (frame, window, scale) = (
        numbers("data-frame"),
        numbers("data-window"),
        numbers("data-scale"),
    );
    assert_eq!((frame.len(), window.len(), scale.len()), (4, 4, 2));
    let x = frame[0] + (150.0 - window[0]) * scale[0];
    let y = frame[1] + frame[3] - (0.0 - window[2]) * scale[1];
    let lands = els
        .iter()
        .filter(|e| e.name == "polyline")
        .filter_map(|e| e.attr("points"))
        .any(|points| {
            let first: Vec<f64> = points
                .split(' ')
                .next()
                .unwrap()
                .split(',')
                .map(|n| n.parse().unwrap())
                .collect();
            (first[0] - x).abs() < 0.01 && (first[1] - y).abs() < 0.01
        });
    assert!(lands, "({x}, {y}) is the first point of no polyline");
}

// ---------------------------------------------------------------------------
// The origin line of the title band (04/10)

/// The title band of the section drawings (there seems to be some room in the
/// middle) should show the position of the polyline's point 0 with its
/// associated ESG codes.
fn with_origins() -> Catalogue {
    let mut cat = catalogue(items());
    cat.long.trace = vec![[500000.0, 6000000.0], [501000.0, 6000000.0]];
    let mut cross = cat.long.clone();
    cross.key = SectionKey::Cross { index: 1 };
    cross.trace = vec![[500400.0, 5999900.0], [500400.0, 6000100.0]];
    cross.length = 200.0;
    cat.crosses = vec![cross];
    cat.plan.origin = [500000.0, 6000000.0];
    cat
}

fn origin_of(els: &[El]) -> Option<String> {
    els.iter()
        .find(|e| e.attr("data-role") == Some("origin"))
        .map(|e| e.text.clone())
}

#[test]
fn the_title_band_writes_the_first_point_of_each_trace_with_its_epsg_code() {
    let cat = with_origins();
    let long = origin_of(&parse(&drawn(&cat, &view()))).expect("the long section writes its origin");
    assert_eq!(long, "origin (s = 0): X 500000.0 · Y 6000000.0 — EPSG:2154");
    // A cross section's point 0 is its own, not the long section's.
    let mut v = view();
    v.target = Target::Section {
        key: SectionKey::Cross { index: 1 },
    };
    v.window = Some(Window {
        x: [0.0, 200.0],
        y: [-500.0, 0.0],
    });
    let cross = origin_of(&parse(&drawn(&cat, &v))).expect("the cross section writes its origin");
    assert_eq!(cross, "origin (s = 0): X 500400.0 · Y 5999900.0 — EPSG:2154");
    // The plan has a band too, and its (0, 0) is the origin.
    v.target = Target::Plan;
    v.window = None;
    let plan = origin_of(&parse(&drawn(&cat, &v))).expect("the plan writes its origin");
    assert_eq!(plan, long);
}

#[test]
fn a_view_without_the_origin_writes_none() {
    let mut v = view();
    v.show.origin = false;
    assert_eq!(origin_of(&parse(&drawn(&with_origins(), &v))), None);
}

#[test]
fn a_system_less_scene_writes_the_point_without_a_code() {
    let mut cat = with_origins();
    cat.inputs.system = None;
    assert_eq!(
        origin_of(&parse(&drawn(&cat, &view()))).as_deref(),
        Some("origin (s = 0): X 500000.0 · Y 6000000.0")
    );
}

/// The boxes of the three texts of the band, from the SVG as a parser reads it.
fn band_boxes(els: &[El]) -> Vec<(String, f64, f64)> {
    use energyml_sheet::text::string_width_pt;
    let x = |e: &El| e.attr("x").unwrap().parse::<f64>().unwrap();
    let mut out = Vec::new();
    for e in els
        .iter()
        .filter(|e| e.name == "text" && e.attr("y") == Some("16"))
    {
        let size = e.attr("font-size").map_or(11.0, |s| s.parse().unwrap());
        let w = string_width_pt(&e.text, size);
        let x0 = if e.attr("text-anchor") == Some("end") {
            x(e) - w
        } else {
            x(e)
        };
        out.push((e.text.clone(), x0, x0 + w));
    }
    out
}

#[test]
fn the_origin_never_sits_on_the_title_or_the_exaggeration_and_a_narrow_band_shortens_then_counts() {
    let cat = with_origins();
    let mut seen = std::collections::BTreeSet::new();
    for width in [1084.0, 700.0, 560.0, 480.0, 420.0, 380.0, 330.0, 300.0] {
        let mut v = view();
        v.size_px = [width, 700.0];
        let l = layout(&cat, &v);
        let els = parse(&svg::write(&l));
        let mut boxes = band_boxes(&els);
        boxes.sort_by(|a, b| a.1.total_cmp(&b.1));
        for pair in boxes.windows(2) {
            assert!(
                pair[0].2 <= pair[1].1 + 0.01,
                "{:?} runs onto {:?} at {width} px",
                pair[0],
                pair[1]
            );
        }
        let written = origin_of(&els);
        let form = match written.as_deref() {
            Some(t) if t.contains(" X ") => "whole",
            Some(_) => "short",
            None => "none",
        };
        seen.insert(form);
        let said = |part: &str| l.warnings.iter().any(|w| w.contains(part));
        assert_eq!(
            said("origin position shortened"),
            form == "short",
            "{width}: {:?}",
            l.warnings
        );
        assert_eq!(
            said("origin position not written"),
            form == "none",
            "{width}: {:?}",
            l.warnings
        );
        if form == "short" {
            assert_eq!(written.as_deref(), Some("origin: EPSG:2154"));
        }
    }
    assert_eq!(
        seen.into_iter().collect::<Vec<_>>(),
        vec!["none", "short", "whole"],
        "the sweep must reach all three forms"
    );
}

// ---------------------------------------------------------------------------
// Line widths by object (04/10)

/// Line sizes can be set per object or per selection. The stroke widths of an object's polylines, as
/// a parser reads them: the polylines that follow its `<g data-key>` up to the
/// next group.
fn widths_of(els: &[El], key: &str) -> Vec<f64> {
    let want = spelled(key);
    let mut inside = false;
    let mut out = Vec::new();
    for e in els {
        if e.name == "g" {
            inside = e.attr("data-key") == Some(want.as_str());
        } else if inside && e.name == "polyline" {
            out.push(e.attr("stroke-width").unwrap().parse().unwrap());
        }
    }
    out
}

#[test]
fn a_width_set_on_one_object_changes_only_that_object() {
    let cat = catalogue(items());
    let (a, b) = (
        id("A", "WellboreFrameRepresentation").key(),
        id("B", "WellboreFrameRepresentation").key(),
    );
    let h = id("H", "TriangulatedSetRepresentation").key();
    let before = parse(&drawn(&cat, &view()));
    let mut v = view();
    v.widths = vec![(a.clone(), 9.0), (h.clone(), 4.0)];
    let l = layout(&cat, &v);
    let after = parse(&svg::write(&l));
    assert!(widths_of(&after, &a).iter().all(|w| *w == 9.0) && !widths_of(&after, &a).is_empty());
    assert_eq!(widths_of(&after, &h), vec![4.0]);
    assert_eq!(
        widths_of(&after, &b),
        widths_of(&before, &b),
        "another well keeps its width"
    );
    assert_eq!(
        widths_of(&after, &id("L", "TriangulatedSetRepresentation").key()),
        vec![1.5],
        "another surface keeps its own"
    );
    assert!(
        l.warnings.iter().all(|w| !w.contains("line width")),
        "{:?}",
        l.warnings
    );
}

#[test]
fn a_refused_width_falls_back_to_the_default_and_is_named() {
    let cat = catalogue(items());
    let a = id("A", "WellboreFrameRepresentation").key();
    let default = widths_of(&parse(&drawn(&cat, &view())), &a);
    for bad in [0.0, -2.0, f64::NAN, f64::INFINITY, 1000.0] {
        let mut v = view();
        v.widths = vec![(a.clone(), bad)];
        let l = layout(&cat, &v);
        assert_eq!(
            widths_of(&parse(&svg::write(&l)), &a),
            default,
            "{bad} is not drawn"
        );
        let said: Vec<&String> = l.warnings.iter().filter(|w| w.contains("line width")).collect();
        assert_eq!(said.len(), 1, "{bad}: {:?}", l.warnings);
        assert!(
            said[0].contains(&id("A", "WellboreFrameRepresentation").uri),
            "{}",
            said[0]
        );
    }
}

// ---------------------------------------------------------------------------
// Direction chevrons on the plan's section lines (04/10)

/// Small arrows or simple chevrons on the section line show the direction in
/// which the geodesic distance from the starting point is computed.
fn plan_with(long: Vec<[f64; 2]>, cross: Vec<[f64; 2]>) -> Catalogue {
    let mut cat = catalogue(items());
    cat.plan.sections = vec![(SectionKey::Long, long), (SectionKey::Cross { index: 1 }, cross)];
    cat
}

fn plan_view() -> View {
    let mut v = view();
    v.target = Target::Plan;
    v.window = None;
    v
}

/// The chevrons of a section line, each as the unit vector from the middle of its
/// two arms to its tip, in pixels — read from the SVG by a parser.
fn chevrons(els: &[El], section: &str) -> Vec<[f64; 2]> {
    let want = format!("direction-{section}");
    let mut inside = false;
    let mut out = Vec::new();
    for e in els {
        if e.name == "g" {
            inside = e.attr("data-decor") == Some(want.as_str());
        } else if inside && e.name == "polyline" {
            let p: Vec<[f64; 2]> = e
                .attr("points")
                .unwrap()
                .split(' ')
                .map(|pt| {
                    let (x, y) = pt.split_once(',').unwrap();
                    [x.parse().unwrap(), y.parse().unwrap()]
                })
                .collect();
            assert_eq!(p.len(), 3, "a chevron is two arms meeting at a tip");
            let (dx, dy) = (
                p[1][0] - (p[0][0] + p[2][0]) / 2.0,
                p[1][1] - (p[0][1] + p[2][1]) / 2.0,
            );
            let n = dx.hypot(dy);
            out.push([dx / n, dy / n]);
        }
    }
    out
}

/// The direction of a plan segment on the drawing: screen y runs down.
fn on_screen(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    let (dx, dy) = (b[0] - a[0], -(b[1] - a[1]));
    let n = dx.hypot(dy);
    [dx / n, dy / n]
}

fn close(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
}

#[test]
fn the_plan_marks_every_line_with_chevrons_that_follow_its_abscissa() {
    let cat = plan_with(
        vec![[0.0, 0.0], [600.0, 0.0], [600.0, 400.0]],
        vec![[300.0, -200.0], [300.0, 300.0]],
    );
    let els = parse(&drawn(&cat, &plan_view()));
    let long = chevrons(&els, "long");
    // One per segment at least, and a bend shows its new direction.
    assert!(
        long.iter()
            .any(|d| close(*d, on_screen([0.0, 0.0], [600.0, 0.0]))),
        "{long:?}"
    );
    assert!(
        long.iter()
            .any(|d| close(*d, on_screen([600.0, 0.0], [600.0, 400.0]))),
        "{long:?}"
    );
    assert!(long.iter().all(|d| close(*d, on_screen([0.0, 0.0], [600.0, 0.0]))
        || close(*d, on_screen([600.0, 0.0], [600.0, 400.0]))));
    // A cross section follows its own direction from its own point 0.
    let cross = chevrons(&els, "cross-1");
    assert!(
        !cross.is_empty()
            && cross
                .iter()
                .all(|d| close(*d, on_screen([300.0, -200.0], [300.0, 300.0]))),
        "{cross:?}"
    );
}

#[test]
fn a_line_drawn_the_other_way_round_gets_the_opposite_orientation() {
    let there = plan_with(
        vec![[0.0, 0.0], [600.0, 0.0]],
        vec![[300.0, -200.0], [300.0, 300.0]],
    );
    let back = plan_with(
        vec![[600.0, 0.0], [0.0, 0.0]],
        vec![[300.0, 300.0], [300.0, -200.0]],
    );
    for section in ["long", "cross-1"] {
        let a = chevrons(&parse(&drawn(&there, &plan_view())), section);
        let b = chevrons(&parse(&drawn(&back, &plan_view())), section);
        assert!(!a.is_empty() && !b.is_empty());
        assert!(
            a.iter().all(|x| b.iter().all(|y| close([-x[0], -x[1]], *y))),
            "{section}: {a:?} vs {b:?}"
        );
    }
}

#[test]
fn a_long_segment_gets_more_chevrons_and_the_option_turns_them_off() {
    let cat = plan_with(
        vec![[0.0, 0.0], [600.0, 0.0]],
        vec![[300.0, -200.0], [300.0, 300.0]],
    );
    let on = parse(&drawn(&cat, &plan_view()));
    assert!(chevrons(&on, "long").len() >= 3, "one per ~100 px of line");
    let mut v = plan_view();
    v.show.direction = false;
    let off = parse(&drawn(&cat, &v));
    assert!(chevrons(&off, "long").is_empty() && chevrons(&off, "cross-1").is_empty());
    assert!(!drawn(&cat, &v).contains("data-decor=\"direction-"));
}
