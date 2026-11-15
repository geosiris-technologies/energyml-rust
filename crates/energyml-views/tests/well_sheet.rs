//! `well-sheet`: the sheet of a frame, its sections, its keys, its settings, its refusals. A package fabricated in
//! memory: a frame at MD 0, 10, 20, 30, one pressure value per interval, one value per node.

use energyml_domain::source::ObjectSource;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_sheet::draw::{DisplayList, Element, Item};
use energyml_sheet::layout::{self, HeightGoal, Params, Track};
use energyml_sheet::picture::{PictureStyle, of_layout};
use energyml_sheet::read::{self, Options};
use energyml_storage::EpcStore;
use energyml_views::{ViewAsk, ViewKinds, ViewRefusal, draw_list, draw_svg, redraw, style};
use serde_json::json;

use crate::fixtures::{NO_MAPS, Opened, frame_uri, package, trajectory_uri, uri};

const FRAME: &str = crate::fixtures::FRAME;
const PRESSURE_KIND: &str = "55555555-5555-4555-8555-555555555555";
const PRESSURE: &str = "66666666-6666-4666-8666-666666666666";
const PER_NODE: &str = "77777777-7777-4777-8777-777777777777";

fn dor(uuid: &str, qualified_type: &str, title: &str) -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: uuid.to_string(),
        qualified_type: qualified_type.to_string(),
        title: title.to_string(),
        ..Default::default()
    }
}

fn values(values: &[f64]) -> Vec<resqml22::AbstractValueArray> {
    vec![resqml22::AbstractValueArray::FloatingPointXmlArray(
        eml23::FloatingPointXmlArray {
            count_per_value: 1,
            values: values.to_vec(),
            ..Default::default()
        },
    )]
}

fn property(
    uuid: &str,
    title: &str,
    element: eml23::IndexableElement,
    data: &[f64],
) -> resqml22::DiscreteProperty {
    resqml22::DiscreteProperty {
        uuid: uuid.to_string(),
        schema_version: "2.2".into(),
        citation: eml23::Citation {
            title: title.to_string(),
            ..Default::default()
        },
        indexable_element: element,
        value_count_per_indexable_element: vec![1],
        property_kind: dor(PRESSURE_KIND, "eml23.PropertyKind", "Pressure"),
        supporting_representation: dor(FRAME, "resqml22.WellboreFrameRepresentation", "a frame"),
        values_for_patch: values(data),
        ..Default::default()
    }
}

/// The frame of the shared fixture (MD 0, 10, 20, 30), a pressure property on its three intervals, and, when asked,
/// a property on its four nodes.
fn sheet_package(with_node_property: bool) -> EpcStore {
    let mut store = package(50.0, &[0.0, 10.0, 20.0, 30.0]);
    store
        .put(Box::new(eml23::PropertyKind {
            uuid: PRESSURE_KIND.to_string(),
            schema_version: "2.3".to_string(),
            citation: eml23::Citation {
                title: "Pressure".to_string(),
                ..Default::default()
            },
            ..Default::default()
        }))
        .expect("it goes in");
    store
        .put(Box::new(property(
            PRESSURE,
            "a frame - pressure",
            eml23::IndexableElement::Intervals,
            &[10.0, 20.0, 30.0],
        )))
        .expect("it goes in");
    if with_node_property {
        store
            .put(Box::new(property(
                PER_NODE,
                "a frame - a per-node log",
                eml23::IndexableElement::Nodes,
                &[1.0, 2.0, 3.0, 4.0],
            )))
            .expect("it goes in");
    }
    store
}

fn drawn(store: EpcStore, ask: ViewAsk) -> energyml_views::Drawn {
    let opened = Opened::of(store);
    draw_list(&ViewKinds::with_builtins(), &ask, &opened.context(&NO_MAPS))
        .expect("drawn")
        .0
}

fn refused(store: EpcStore, ask: ViewAsk) -> ViewRefusal {
    let opened = Opened::of(store);
    draw_list(&ViewKinds::with_builtins(), &ask, &opened.context(&NO_MAPS)).expect_err("refused")
}

fn cells(list: &DisplayList) -> Vec<&Element> {
    list.layer("tracks")
        .expect("the tracks layer")
        .elements
        .iter()
        .filter(|element| element.kind == "cell")
        .collect()
}

fn rect_of(element: &Element) -> energyml_sheet::draw::Rect {
    match &element.items[0] {
        Item::Rect { rect, .. } => *rect,
        other => panic!("a cell starts with its rectangle, not {other:?}"),
    }
}

#[test]
fn only_the_frame_is_offered() {
    let opened = Opened::of(sheet_package(false));
    let offers = ViewKinds::with_builtins().offers(&opened.index);
    let sheet: Vec<&str> = offers
        .iter()
        .filter(|offer| offer.view == "well-sheet")
        .map(|offer| offer.objects[0].as_str())
        .collect();
    assert_eq!(sheet, vec![frame_uri().as_str()]);
    // Break it: offer the trajectories too.
}

#[test]
fn the_pressure_column_has_one_cell_per_interval_keyed_by_the_frame() {
    let list = drawn(
        sheet_package(false),
        ViewAsk::new("well-sheet").object(frame_uri()),
    )
    .list;
    let cells = cells(&list);
    assert_eq!(cells.len(), 3, "three intervals, one interval column");
    let frame = frame_uri();
    let keys: Vec<&str> = list.keys();
    // The depth labels, then the cells: a cell of this property names no uri of its own, so the frame's.
    for md in [0, 10, 20, 30] {
        assert!(keys.contains(&format!("{frame}#md={md}").as_str()), "{keys:?}");
    }
    for md in [0, 10, 20] {
        assert!(
            keys.contains(&format!("{frame}#col=1&md={md}").as_str()),
            "{keys:?}"
        );
    }
    // The MD column is the first of the sheet: the pressure column sits to its right.
    assert!(rect_of(cells[0]).x > 0.0);
    // Break it: key the cells by their depth alone, or drop the object from the style.
}

/// **To scale, 4 mm of paper per metre of MD: three intervals of 10 m are three cells of 40 mm.**
///
/// The number is the setting's definition, not read from the sheet. To break it: read `scale` as millimetres per
/// section, or in points.
#[test]
fn to_scale_a_cell_is_as_high_as_its_interval_times_the_scale() {
    let list = drawn(
        sheet_package(false),
        ViewAsk::new("well-sheet")
            .object(frame_uri())
            .set("proportion", json!("to-scale"))
            .set("scale", json!(4.0)),
    )
    .list;
    let cells = cells(&list);
    for cell in &cells {
        assert!((rect_of(cell).h - 40.0).abs() < 1e-9, "{:?}", rect_of(cell));
    }
    let tops: Vec<f64> = cells.iter().map(|cell| rect_of(cell).y).collect();
    assert!((tops[1] - tops[0] - 40.0).abs() < 1e-9 && (tops[2] - tops[1] - 40.0).abs() < 1e-9);
    // The page reaches the bottom of the body (a band of notes, if any, comes under it).
    let body_bottom = rect_of(cells[2]).y + rect_of(cells[2]).h;
    assert!(
        list.size[1] >= body_bottom - 1e-9,
        "{:?} {body_bottom}",
        list.size
    );
    // Break it: double the scale in `Params`.
}

/// **The sheet is what the other crates make of the frame: the reading, the layout, the translation, nothing more.**
///
/// The package's own listing and the reading are called here directly, with the store's resources rather than the
/// index the view lists from, and the list the view draws must be the one these calls give.
/// To break it: let the view recompute a position, a label or a colour.
#[test]
fn the_view_draws_what_reading_layout_and_picture_give() {
    let opened = Opened::of(sheet_package(false));
    let source = &opened.store;
    let frame = source.read_object_at(&frame_uri()).expect("the frame");
    let resources = opened.store.resources();
    let mut read =
        read::board_of_frame(frame.as_reflect(), &resources, source, &Options::new("MD")).expect("a board");
    read.board.title = Some("a frame".to_string());
    let laid = layout::compute(
        &read.board,
        &Params {
            proportion: "readable".to_string(),
            height: HeightGoal::Scale(4.0),
            page_width_mm: 180.0,
            text_size_pt: 8.0,
        },
    )
    .expect("a layout");
    let expected = of_layout(
        &laid,
        &PictureStyle {
            ink: style::INK,
            guide: style::NEUTRAL,
            background: Some(style::PAPER),
            object: Some(frame_uri()),
            ..PictureStyle::default()
        },
    );

    let list = drawn(
        sheet_package(false),
        ViewAsk::new("well-sheet").object(frame_uri()),
    )
    .list;
    // The drawing is the translation's, layer by layer; the door adds a band of notes under it, if there are any.
    assert_eq!(list.layers[..expected.layers.len()], expected.layers[..]);
    assert_eq!(list.size[0], expected.size[0]);
    assert!(list.size[1] >= expected.size[1]);
    assert_eq!(
        (list.unit, &list.title, list.background),
        (expected.unit, &expected.title, expected.background)
    );

    // And the cells hold the reading's own words, section by section.
    let Some(Track::Intervals(pressure)) = read
        .board
        .tracks
        .iter()
        .find(|track| matches!(track, Track::Intervals(_)))
    else {
        panic!("a pressure column");
    };
    let words: Vec<String> = cells(&list)
        .iter()
        .map(|cell| {
            cell.items
                .iter()
                .filter_map(|item| match item {
                    Item::Text { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    let wanted: Vec<String> = pressure
        .intervals
        .iter()
        .map(|interval| interval.text.clone().unwrap_or_default())
        .collect();
    assert_eq!(words, wanted);
    assert!(wanted.iter().all(|text| !text.is_empty()), "{wanted:?}");
    // Break it: draw the intervals in reverse, or read the values from another property.
}

#[test]
fn a_property_on_the_nodes_is_said_not_drawn() {
    let with = drawn(
        sheet_package(true),
        ViewAsk::new("well-sheet").object(frame_uri()),
    );
    assert_eq!(
        cells(&with.list).len(),
        3,
        "the per-node property makes no column"
    );
    assert!(
        with.notes
            .iter()
            .any(|note| note.key == "sheet-reading" && note.says.contains("nodes")),
        "{:?}",
        with.notes
    );
    // Break it: draw a column per property, whatever it hangs on.
}

#[test]
fn a_selection_that_names_no_interval_property_leaves_no_section_and_is_refused() {
    // The property of the nodes is no interval column: nothing to cut the sheet into sections.
    let why = refused(
        sheet_package(true),
        ViewAsk::new("well-sheet")
            .object(frame_uri())
            .set("tracks", json!(PER_NODE)),
    );
    assert!(
        matches!(&why, ViewRefusal::Computing { view, .. } if view == "well-sheet"),
        "{why}"
    );
    // The pressure property named alone is the same sheet as the default.
    let alone = drawn(
        sheet_package(false),
        ViewAsk::new("well-sheet")
            .object(frame_uri())
            .set("tracks", json!(PRESSURE)),
    )
    .list;
    assert_eq!(cells(&alone).len(), 3);
    // Break it: ignore `tracks`.
}

#[test]
fn a_frame_with_no_interval_data_is_refused_not_drawn_empty() {
    let why = refused(
        package(50.0, &[0.0, 10.0, 20.0]),
        ViewAsk::new("well-sheet").object(frame_uri()),
    );
    assert!(
        matches!(&why, ViewRefusal::Computing { view, why } if view == "well-sheet" && why.contains("section")),
        "{why}"
    );
    // Break it: let an empty board through to the writer.
}

#[test]
fn a_trajectory_is_not_a_frame_and_an_unknown_mode_is_no_mode() {
    let why = refused(
        sheet_package(false),
        ViewAsk::new("well-sheet").object(trajectory_uri()),
    );
    assert!(matches!(why, ViewRefusal::ObjectNotTaken { .. }), "{why}");
    let why = refused(
        sheet_package(false),
        ViewAsk::new("well-sheet")
            .object(frame_uri())
            .set("proportion", json!("sideways")),
    );
    assert!(
        matches!(&why, ViewRefusal::BadSetting { name, .. } if name == "proportion"),
        "{why}"
    );
    // Nothing in the package at that uri.
    let why = refused(
        sheet_package(false),
        ViewAsk::new("well-sheet").object(uri(
            "resqml22.WellboreFrameRepresentation",
            "99999999-9999-4999-8999-999999999999",
        )),
    );
    assert!(matches!(why, ViewRefusal::NotInPackage { .. }), "{why}");
    // Break it: take a trajectory as a frame, or fall back to `readable` for an unknown mode.
}

#[test]
fn the_proportions_offered_are_the_registrys() {
    let registry = layout::ModeRegistry::with_builtins();
    for id in energyml_views::well_sheet::PROPORTIONS {
        assert!(registry.find(id).is_some(), "`{id}` is no proportion mode");
    }
    // Break it: list a mode the registry does not hold.
}

#[test]
fn the_picture_is_written_and_drawn_again_from_its_own_metadata() {
    let opened = Opened::of(sheet_package(false));
    let kinds = ViewKinds::with_builtins();
    let cx = opened.context(&NO_MAPS);
    let picture = draw_svg(&kinds, &ViewAsk::new("well-sheet").object(frame_uri()), &cx).expect("a picture");
    assert!(picture.svg.contains("data-layer=\"tracks\""));
    assert!(picture.svg.contains("width=\"180mm\""), "a page in millimetres");
    let again = redraw(&kinds, &picture.svg, &cx).expect("drawn again");
    assert_eq!(again.svg, picture.svg);
    // Break it: let the view read something other than the request (a clock, a random id).
}
