//! Named markers on a set of objects (ADR-0015): the shapes the library declares from the model,
//! where a marker lands, and the legend that says what is drawn.
//!
//! The shape of the assertions is the design's; the values are hand-built
//! catalogues (`tests/layout.rs`). The list of symbols is **RESQML 2.2's
//! `NodeSymbol` enumeration** (Energistics RESQML v2.2 schema,
//! `GraphicalInformationObject.xsd`, simple type `NodeSymbol`): it is typed here,
//! as a second source, so that the registry's reading is confronted with
//! something it did not produce.

use energyml_core::registry::Registry;
use energyml_section::layout::{Attr, Layout, Shape, layout};
use energyml_section::markers::{self, CONTOUR};
use energyml_section::model::*;

use crate::layout::{catalogue, id, surface, view, well};
use crate::strata_legend::elements;

/// The values of RESQML 2.2's `NodeSymbol`.
const NODE_SYMBOL: [&str; 10] = [
    "circle",
    "cross",
    "cube",
    "diamond",
    "plus",
    "point",
    "pyramid",
    "sphere",
    "star",
    "tetrahedron",
];

fn marker(name: &str, shape: &str, at: MarkerPlace, keys: Vec<String>) -> Marker {
    Marker {
        name: name.into(),
        shape: shape.into(),
        colour: "#e91e63".into(),
        at,
        keys,
        size_px: 12.0,
        hidden: false,
    }
}

fn well_key(n: &str) -> String {
    id(n, "WellboreFrameRepresentation").key()
}

fn marked(markers: Vec<Marker>) -> View {
    View { markers, ..view() }
}

/// Well "A": head (150, 0), nodes (150, 0) (150, -200) (200, -500), MD 0 200 520.
fn one_well() -> Catalogue {
    catalogue(vec![Item::Well(well("A", 150.0, 12.0, Side::Left))])
}

fn px(l: &Layout, p: [f64; 2]) -> [f64; 2] {
    [
        l.frame.x + (p[0] - l.window.x[0]) * l.scale[0],
        l.frame.y + l.frame.h - (p[1] - l.window.y[0]) * l.scale[1],
    ]
}

fn symbols(l: &Layout) -> Vec<(String, [f64; 2], String)> {
    let Some(layer) = l.layer("markers") else {
        return Vec::new();
    };
    layer
        .objects
        .iter()
        .flat_map(|o| {
            let name = match &o.attr {
                Attr::Marker { name, .. } => name.clone(),
                other => panic!("a marker's group is about a marker: {other:?}"),
            };
            o.shapes.iter().filter_map(move |s| match s {
                Shape::Symbol { at, kind, .. } => Some((name.clone(), *at, kind.clone())),
                _ => None,
            })
        })
        .collect()
}

fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6
}

fn registry() -> Registry {
    energyml_schemas::all()
}

#[test]
fn the_shapes_are_read_from_the_registry_not_typed_in_the_library() {
    let (shapes, places) = markers::declared(&registry());
    let ids: Vec<&str> = shapes.iter().map(|s| s.id.as_str()).collect();
    for expected in NODE_SYMBOL {
        assert!(
            ids.contains(&expected),
            "{expected} is a NodeSymbol value and is not declared: {ids:?}"
        );
    }
    // The pair a marker of that shape would write — the qualified type of the
    // enumeration the field is typed with, and the value.
    for wanted in ["star", "diamond"] {
        let shape = shapes.iter().find(|s| s.id == wanted).unwrap();
        assert!(
            shape
                .format
                .iter()
                .any(|(ty, value)| ty == "resqml22.NodeSymbolExt" && value == wanted),
            "{wanted}: {:?}",
            shape.format
        );
        assert!(shape.drawn, "{wanted} is drawn");
        assert_eq!(shape.label_key, format!("sectionView.markers.shape.{wanted}"));
        assert!(!shape.label.is_empty());
    }
    // The tool's own shape has no pair; every other one has at least one.
    assert_eq!(
        shapes.iter().find(|s| s.id == CONTOUR).unwrap().format,
        Vec::<(String, String)>::new()
    );
    assert!(
        shapes
            .iter()
            .filter(|s| s.id != CONTOUR)
            .all(|s| !s.format.is_empty())
    );
    // Each shape once, however many releases write it.
    let mut sorted = ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "{ids:?}");

    let place_ids: Vec<(&str, bool)> = places.iter().map(|p| (p.id.as_str(), p.takes_value)).collect();
    assert_eq!(place_ids, [("head", false), ("bottom", false), ("md", true)]);
    assert!(
        places
            .iter()
            .all(|p| p.label_key.starts_with("sectionView.markers.place.") && !p.label.is_empty())
    );
}

#[test]
fn a_registry_without_the_type_declares_the_contour_alone() {
    // The control of the test above: the shapes come from what the registry
    // holds. RESQML 2.0.1 has no `GraphicalInformationForNodes`.
    let mut old = Registry::new();
    energyml_resqml_2_0_1::register(&mut old);
    let (shapes, _) = markers::declared(&old);
    assert_eq!(
        shapes.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        [CONTOUR]
    );
}

#[test]
fn the_declared_list_and_the_drawer_agree_on_what_is_drawn() {
    let (shapes, _) = markers::declared(&registry());
    // The control: the loop below has something to loop over.
    assert!(
        shapes.iter().filter(|s| s.drawn && s.id != CONTOUR).count() >= NODE_SYMBOL.len(),
        "{shapes:?}"
    );
    for shape in &shapes {
        assert_eq!(shape.drawn, markers::is_drawn(&shape.id), "{}", shape.id);
        if shape.drawn && shape.id != CONTOUR {
            // And the drawer really draws it: a symbol group in the SVG.
            let view = marked(vec![marker(
                "m",
                &shape.id,
                MarkerPlace::Head,
                vec![well_key("A")],
            )]);
            let svg = energyml_section::draw(&one_well(), &view);
            assert!(
                svg.contains(&format!("data-symbol=\"{}\"", shape.id)),
                "{} is declared drawn and is not written",
                shape.id
            );
        }
    }
}

#[test]
fn a_marker_lands_where_the_well_is() {
    let l = layout(
        &one_well(),
        &marked(vec![
            marker("head", "star", MarkerPlace::Head, vec![well_key("A")]),
            marker("bottom", "diamond", MarkerPlace::Bottom, vec![well_key("A")]),
            // MD 100 is half the first segment (0 → 200): (150, -100).
            marker(
                "at100",
                "cube",
                MarkerPlace::Md { value: 100.0 },
                vec![well_key("A")],
            ),
            // MD 360 is half the second (200 → 520): (175, -350).
            marker(
                "at360",
                "plus",
                MarkerPlace::Md { value: 360.0 },
                vec![well_key("A")],
            ),
        ]),
    );
    let found = symbols(&l);
    let at = |name: &str| {
        found
            .iter()
            .find(|(n, ..)| n == name)
            .unwrap_or_else(|| panic!("{name} not drawn: {found:?}"))
            .1
    };
    assert!(near(at("head"), px(&l, [150.0, 0.0])), "{:?}", at("head"));
    assert!(near(at("bottom"), px(&l, [200.0, -500.0])), "{:?}", at("bottom"));
    assert!(near(at("at100"), px(&l, [150.0, -100.0])), "{:?}", at("at100"));
    assert!(near(at("at360"), px(&l, [175.0, -350.0])), "{:?}", at("at360"));
}

#[test]
fn a_marker_of_a_merged_object_lands_on_the_line_that_stands_for_it() {
    let mut line = well("A", 150.0, 12.0, Side::Left);
    line.merged = vec![id("A-trajectory", "WellboreTrajectoryRepresentation")];
    let cat = catalogue(vec![Item::Well(line)]);
    let key = id("A-trajectory", "WellboreTrajectoryRepresentation").key();
    let l = layout(
        &cat,
        &marked(vec![marker("m", "circle", MarkerPlace::Head, vec![key])]),
    );
    assert_eq!(symbols(&l).len(), 1);
}

#[test]
fn a_symbol_group_carries_its_marker_and_its_object_and_keeps_its_pixel_size() {
    let view = marked(vec![marker(
        "Core taken",
        "star",
        MarkerPlace::Head,
        vec![well_key("A")],
    )]);
    let svg = energyml_section::draw(&one_well(), &view);
    let els = elements(&svg);
    let groups: Vec<_> = els
        .iter()
        .filter(|e| e.name == "g" && e.attr("data-kind") == Some("marker"))
        .collect();
    assert_eq!(groups.len(), 1, "{svg}");
    assert_eq!(groups[0].attr("data-marker"), Some("Core taken"));
    assert_eq!(
        groups[0].attr("data-key"),
        Some(well_key("A").replace('\0', "\u{2400}").as_str())
    );
    let symbol = els
        .iter()
        .find(|e| e.attr("data-symbol") == Some("star"))
        .expect("the symbol");
    assert!(
        symbol.attr("data-zoom-fixed").is_some(),
        "the zoom script keeps it at its pixel size"
    );
    assert!(
        symbol.path.iter().any(|p| p == "zoom=plot"),
        "inside the zoomed plot: {:?}",
        symbol.path
    );
    // Above the wells: its layer is written after theirs.
    let layer_at = |name: &str| {
        svg.find(&format!("data-layer=\"{name}\""))
            .unwrap_or_else(|| panic!("no {name} layer"))
    };
    assert!(layer_at("wells") < layer_at("markers"));
}

#[test]
fn a_contour_is_a_wide_line_under_the_well_in_the_markers_colour() {
    let mut contour = marker("Cored", CONTOUR, MarkerPlace::Head, vec![well_key("A")]);
    contour.colour = "#00ff00".into();
    let l = layout(&one_well(), &marked(vec![contour]));
    let names: Vec<&str> = l.layers.iter().map(|x| x.name).collect();
    let (contours, traces, wells) = (
        names
            .iter()
            .position(|n| *n == "marker-contours")
            .expect("a contour layer"),
        names.iter().position(|n| *n == "traces").unwrap(),
        names.iter().position(|n| *n == "wells").unwrap(),
    );
    assert!(contours < traces && traces < wells, "{names:?}");
    let drawn = &l.layers[contours].objects[0];
    let Shape::Polyline {
        stroke,
        width,
        points,
        ..
    } = &drawn.shapes[0]
    else {
        panic!("a line")
    };
    assert_eq!(stroke, "#00ff00");
    assert!(*width > 4.0, "wider than the well's 4 px: {width}");
    assert_eq!(points.len(), 3, "the whole path");
    assert!(near(points[0], px(&l, [150.0, 0.0])));
    assert!(symbols(&l).is_empty(), "a contour has no symbol");
    // A contour is listed in the legend like the others.
    assert_eq!(
        l.layer("markers-legend").unwrap().objects.len(),
        2,
        "the box and its entry"
    );
}

#[test]
fn a_surface_carries_a_marker_at_the_ends_of_its_single_span() {
    let cat = catalogue(vec![surface(
        "H",
        TraceRole::Horizon,
        None,
        vec![vec![[100.0, -50.0], [500.0, -80.0], [900.0, -60.0]]],
    )]);
    let key = id("H", "TriangulatedSetRepresentation").key();
    let l = layout(
        &cat,
        &marked(vec![
            marker("first", "circle", MarkerPlace::Head, vec![key.clone()]),
            marker("last", "cross", MarkerPlace::Bottom, vec![key.clone()]),
            marker("depth", "star", MarkerPlace::Md { value: 10.0 }, vec![key]),
        ]),
    );
    let found = symbols(&l);
    assert!(near(
        found.iter().find(|s| s.0 == "first").unwrap().1,
        px(&l, [100.0, -50.0])
    ));
    assert!(near(
        found.iter().find(|s| s.0 == "last").unwrap().1,
        px(&l, [900.0, -60.0])
    ));
    assert!(
        found.iter().all(|s| s.0 != "depth"),
        "a surface has no measured depth"
    );
    assert!(
        l.warnings
            .iter()
            .any(|w| w.contains("marker \"depth\"") && w.contains("no such place")),
        "{:?}",
        l.warnings
    );
}

#[test]
fn the_legend_is_what_is_drawn() {
    let cat = one_well();
    let missing = well_key("Z");
    let view = marked(vec![
        marker("Drawn", "star", MarkerPlace::Head, vec![well_key("A")]),
        // Nothing of it is in the drawing: no entry, and a note.
        marker("Absent", "diamond", MarkerPlace::Head, vec![missing]),
        // Refused: a shape no drawer knows.
        marker("Strange", "hexagon", MarkerPlace::Head, vec![well_key("A")]),
    ]);
    let l = layout(&cat, &view);
    let drawn: Vec<String> = symbols(&l).into_iter().map(|s| s.0).collect();
    assert_eq!(drawn, ["Drawn"]);
    let entries: Vec<String> = l
        .layer("markers-legend")
        .expect("a legend")
        .objects
        .iter()
        .filter_map(|o| match &o.attr {
            Attr::Marker { name, key: None } => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(entries, drawn, "one list for both");

    let svg = energyml_section::svg::write(&l);
    let els = elements(&svg);
    let group = els
        .iter()
        .find(|e| e.attr("id") == Some("markers-legend"))
        .expect("the Inkscape layer");
    assert_eq!(group.attr("inkscape:groupmode"), Some("layer"));
    assert!(svg.contains("xmlns:inkscape="), "declared since a group uses it");
    let written: Vec<&str> = els
        .iter()
        .filter(|e| e.attr("data-kind") == Some("markers-entry"))
        .filter_map(|e| e.attr("data-marker"))
        .collect();
    assert_eq!(written, ["Drawn"]);
    // It sits in the right margin, clear of the frame.
    let Shape::Polygon { points, .. } = &l.layer("markers-legend").unwrap().objects[0].shapes[0] else {
        panic!("a box")
    };
    assert!(
        points.iter().all(|p| p[0] > l.frame.x + l.frame.w),
        "{points:?} vs the frame {:?}",
        l.frame
    );
}

/// A marker can be hidden without being deleted. A hidden marker
/// stays in the view and draws nothing — no symbol, no legend entry, no note —
/// and is the same bytes as no marker at all.
///
/// To make it fail: drop the `hidden` filter in `marking::resolve`.
#[test]
fn a_hidden_marker_is_kept_and_draws_nothing() {
    let cat = one_well();
    let mut hidden = marker("Hidden", "star", MarkerPlace::Head, vec![well_key("A")]);
    hidden.hidden = true;
    let view = marked(vec![hidden.clone()]);
    let l = layout(&cat, &view);
    assert!(symbols(&l).is_empty());
    assert!(l.layer("markers-legend").is_none());
    assert!(
        l.warnings.iter().all(|w| !w.contains("Hidden")),
        "{:?}",
        l.warnings
    );
    let none = layout(&cat, &marked(Vec::new()));
    assert_eq!(
        l.layers.iter().map(|x| x.name).collect::<Vec<_>>(),
        none.layers.iter().map(|x| x.name).collect::<Vec<_>>()
    );
    // Kept: the view says it, and writes the flag only when it is on.
    let json = serde_json::to_value(&view).unwrap();
    assert_eq!(json["markers"][0]["hidden"], true);
    hidden.hidden = false;
    assert!(serde_json::to_value(&hidden).unwrap().get("hidden").is_none());
}

#[test]
fn every_cause_is_a_note_and_counted() {
    let mut bad_colour = marker("Pink", "star", MarkerPlace::Head, vec![well_key("A")]);
    bad_colour.colour = "pink".into();
    let mut bad_size = marker("Huge", "star", MarkerPlace::Head, vec![well_key("A")]);
    bad_size.size_px = 500.0;
    let l = layout(
        &one_well(),
        &marked(vec![
            // two keys the drawing does not hold
            marker(
                "Lost",
                "star",
                MarkerPlace::Head,
                vec![well_key("X"), well_key("Y")],
            ),
            // a shape the build does not draw
            marker("Strange", "hexagon", MarkerPlace::Head, vec![well_key("A")]),
            // beyond the well's last MD (520)
            marker(
                "Deep",
                "star",
                MarkerPlace::Md { value: 9000.0 },
                vec![well_key("A")],
            ),
            // no name
            marker("  ", "star", MarkerPlace::Head, vec![well_key("A")]),
            bad_colour,
            bad_size,
        ]),
    );
    let said = |needle: &str| {
        l.warnings
            .iter()
            .filter(|w| w.contains(needle))
            .collect::<Vec<_>>()
    };
    assert_eq!(said("marker \"Lost\"").len(), 1, "{:?}", l.warnings);
    assert!(
        said("marker \"Lost\"")[0].contains("2 objects are not in this drawing"),
        "{:?}",
        said("marker \"Lost\"")
    );
    assert!(said("marker \"Strange\"")[0].contains("shape \"hexagon\" is not drawn"));
    assert!(
        said("marker \"Deep\"")[0].contains("outside 1 well"),
        "{:?}",
        said("marker \"Deep\"")
    );
    assert!(said("marker \"Pink\"")[0].contains("not #rrggbb"));
    assert!(said("marker \"Huge\"")[0].contains("size 500"));
    assert_eq!(said("without a name").len(), 1);
    assert!(said("without a name")[0].starts_with("1 marker(s)"));
    // None of them is drawn, so none is in the legend and there is no legend.
    assert!(symbols(&l).is_empty());
    assert!(l.layer("markers-legend").is_none());
}

#[test]
fn a_well_without_measured_depths_cannot_carry_a_depth_marker() {
    let mut bare = well("A", 150.0, 12.0, Side::Left);
    bare.md = None;
    let l = layout(
        &catalogue(vec![Item::Well(bare)]),
        &marked(vec![marker(
            "D",
            "star",
            MarkerPlace::Md { value: 10.0 },
            vec![well_key("A")],
        )]),
    );
    assert!(
        l.warnings
            .iter()
            .any(|w| w.contains("marker \"D\"") && w.contains("no measured depths")),
        "{:?}",
        l.warnings
    );
    assert!(symbols(&l).is_empty());
}

#[test]
fn a_marker_does_not_squeeze_the_drawing_when_it_is_not_there() {
    // No marker: not one byte of the markers' furniture, and the same layers as before.
    let plain = view();
    let svg = energyml_section::draw(&one_well(), &plain);
    for forbidden in [
        "data-marker",
        "markers-legend",
        "data-symbol",
        "xmlns:inkscape",
        "marker-contours",
        "\"markers\"",
    ] {
        assert!(
            !svg.contains(forbidden),
            "{forbidden} in a drawing with no marker"
        );
    }
    let names: Vec<&str> = layout(&one_well(), &plain)
        .layers
        .iter()
        .map(|l| l.name)
        .collect();
    assert_eq!(
        names,
        [
            "envelopes",
            "fills",
            "outlines",
            "traces",
            "wells",
            "marks",
            "labels",
            "legend"
        ]
    );
    // The same frame as a view whose markers are all refused: the legend's room is only taken when a legend is drawn.
    let refused = marked(vec![marker(
        "Strange",
        "hexagon",
        MarkerPlace::Head,
        vec![well_key("A")],
    )]);
    // (Its note takes a line under the plot, so only the width is the same.)
    assert_eq!(
        layout(&one_well(), &refused).frame.w,
        layout(&one_well(), &plain).frame.w
    );
}

#[test]
fn a_legend_takes_its_room_on_the_right() {
    let with = layout(
        &one_well(),
        &marked(vec![marker(
            "Core taken",
            "star",
            MarkerPlace::Head,
            vec![well_key("A")],
        )]),
    );
    let without = layout(&one_well(), &view());
    assert!(
        with.frame.w < without.frame.w,
        "{} vs {}",
        with.frame.w,
        without.frame.w
    );
}

#[test]
fn the_plan_says_it_draws_no_marker() {
    let cat = one_well();
    let mut v = View::new(Target::Plan, [800.0, 600.0]);
    v.markers = vec![marker("m", "star", MarkerPlace::Head, vec![well_key("A")])];
    let l = layout(&cat, &v);
    assert!(
        l.warnings
            .iter()
            .any(|w| w.contains("markers are drawn in sections only")),
        "{:?}",
        l.warnings
    );
}

#[test]
fn a_view_without_markers_reads_back_empty_and_writes_none() {
    let written = serde_json::to_value(view()).unwrap();
    assert!(
        written.get("markers").is_none(),
        "an empty list is not written: earlier drawings keep their bytes"
    );
    let read: View = serde_json::from_value(written).unwrap();
    assert!(read.markers.is_empty());
}

#[test]
fn a_marker_travels_as_the_json_the_page_writes() {
    let json = serde_json::json!({
        "name": "Core taken", "shape": "star", "colour": "#e91e63",
        "at": { "kind": "md", "value": 1500.0 }, "keys": ["s1\u{0}eml:///x"], "sizePx": 12.0
    });
    let read: Marker = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(read.at, MarkerPlace::Md { value: 1500.0 });
    assert_eq!(serde_json::to_value(&read).unwrap(), json);
    for (place, text) in [(MarkerPlace::Head, "head"), (MarkerPlace::Bottom, "bottom")] {
        assert_eq!(
            serde_json::to_value(place).unwrap(),
            serde_json::json!({ "kind": text })
        );
    }
    // And a drawing keeps the markers it was made with, in its metadata.
    let view = marked(vec![read]);
    let svg = energyml_section::draw(&one_well(), &view);
    let again = energyml_section::svg::read_metadata(&svg).expect("reads").view;
    assert_eq!(again.markers, view.markers);
}

fn row(n: &str, kind: &str) -> ObjectRow {
    ObjectRow {
        object: id(n, kind),
        kind: RowKind::Well,
        outcome: Outcome::Drawn {
            sections: vec![SectionKey::Long],
        },
        fillable: false,
    }
}

/// Wells of a catalogue: BSS trajectories drawn on the long section, refused,
/// merged into their frame's line or drawn on a cross section only, a trajectory
/// of another name, and frames.
fn rows() -> Vec<ObjectRow> {
    vec![
        row("BSS000NUGK trajectory", "WellboreTrajectoryRepresentation"),
        row("BSS000NSYT trajectory", "WellboreTrajectoryRepresentation"),
        row("GTH-1 trajectory", "WellboreTrajectoryRepresentation"),
        row("BSS000NUGK frame", "WellboreFrameRepresentation"),
        // Outside the corridor: in the catalogue, not in the drawing.
        ObjectRow {
            outcome: Outcome::Refused {
                reason: "its head is farther than 200 from the long section".into(),
            },
            ..row("BSS000NTGZ trajectory", "WellboreTrajectoryRepresentation")
        },
        // Folded into its frame's line: the marker lands there.
        ObjectRow {
            outcome: Outcome::Merged {
                into: id("BSS000NUKL frame", "WellboreFrameRepresentation"),
            },
            ..row("BSS000NUKL trajectory", "WellboreTrajectoryRepresentation")
        },
        row("BSS000NUKL frame", "WellboreFrameRepresentation"),
        // Drawn on a cross section only: not in a drawing of the long one.
        ObjectRow {
            outcome: Outcome::Drawn {
                sections: vec![SectionKey::Cross { index: 1 }],
            },
            ..row("BSS000NTCE trajectory", "WellboreTrajectoryRepresentation")
        },
    ]
}

/// The command line's `--marker` (08/10): the type **and** the title choose,
/// case ignored, through the storage's own filters — a frame of a BSS well and a
/// trajectory of another well are both left out, and so are a BSS trajectory the
/// section refused and one drawn on another section only; one merged into a line
/// the section draws is kept.
#[test]
fn a_marker_in_words_takes_the_objects_its_type_and_title_name() {
    let marker = markers::from_spec(
        "Forages BSS:shape=diamond,colour=#1565c0,at=bottom,type=wellboretrajectory,title=bss",
        &rows(),
        Some(SectionKey::Long),
    )
    .unwrap();
    let names: Vec<String> = rows().into_iter().map(|r| r.object.key()).collect();
    assert_eq!(
        marker.keys,
        vec![names[0].clone(), names[1].clone(), names[5].clone()]
    );
    // On any section: the one drawn on a cross section comes in.
    let anywhere = markers::from_spec(
        "BSS:shape=diamond,colour=#1565c0,at=bottom,type=Trajectory,title=BSS",
        &rows(),
        None,
    )
    .unwrap();
    assert_eq!(
        anywhere.keys,
        vec![
            names[0].clone(),
            names[1].clone(),
            names[5].clone(),
            names[7].clone()
        ]
    );
    assert_eq!(
        (
            marker.name.as_str(),
            marker.shape.as_str(),
            marker.colour.as_str()
        ),
        ("Forages BSS", "diamond", "#1565c0")
    );
    assert_eq!(marker.at, MarkerPlace::Bottom);
    assert_eq!(marker.size_px, markers::DEFAULT_SIZE_PX);
    // `A|B`: any of the types.
    let both = markers::from_spec(
        "BSS:shape=star,colour=#000000,at=head,type=Trajectory|Frame,title=BSS",
        &rows(),
        Some(SectionKey::Long),
    )
    .unwrap();
    assert_eq!(both.keys.len(), 5);
    let deep = markers::from_spec(
        "Core:shape=star,colour=#000000,at=md,md=12.5,size=20,title=GTH",
        &rows(),
        Some(SectionKey::Long),
    )
    .unwrap();
    assert_eq!(
        (deep.at, deep.size_px, deep.keys.len()),
        (MarkerPlace::Md { value: 12.5 }, 20.0, 1)
    );
}

/// Every word that cannot become a marker is refused by its name — never a
/// default shape, colour or place, and never a legend entry for nothing.
#[test]
fn a_marker_in_words_refuses_what_it_cannot_draw() {
    let refused = |spec: &str| markers::from_spec(spec, &rows(), Some(SectionKey::Long)).unwrap_err();
    assert!(refused("shape=diamond,colour=#1565c0,at=bottom,title=BSS").contains("expected NAME"));
    assert!(refused(" :shape=diamond,colour=#1565c0,at=bottom,title=BSS").contains("needs a name"));
    assert!(
        refused("A:shape=hexagon,colour=#1565c0,at=bottom,title=BSS").contains("diamond"),
        "the drawn shapes are named"
    );
    assert!(refused("A:shape=diamond,colour=blue,at=bottom,title=BSS").contains("#rrggbb"));
    assert!(refused("A:shape=diamond,colour=#1565c0,title=BSS").contains("at="));
    assert!(refused("A:shape=diamond,colour=#1565c0,at=md,title=BSS").contains("md=VALUE"));
    assert!(
        refused("A:shape=diamond,colour=#1565c0,at=bottom,md=3,title=BSS").contains("goes with `at=md` only")
    );
    assert!(refused("A:shape=diamond,colour=#1565c0,at=bottom").contains("type="));
    assert!(refused("A:shape=diamond,colour=#1565c0,at=bottom,title=nowhere").contains("matches no object"));
    assert!(
        refused("A:shape=diamond,colour=#1565c0,at=bottom,title=BSS,width=3")
            .contains("no parameter `width`")
    );
}
