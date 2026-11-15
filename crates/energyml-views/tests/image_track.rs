//! `image-track`: the pixels against the plane `read_plane` gives, the rows and gaps, the scales, the keys, the
//! refusals. Properties fabricated in memory (inline arrays, no file). Sources: RESQML TRG 2.2 section 7.1 p. 222
//! (several values per element, the first given dimension the slowest), section 12.13 p. 310 (a frame's nodes and the
//! intervals between them).

use std::sync::Arc;

use energyml_core::xml::XmlText;
use energyml_domain::palettes::Reading;
use energyml_domain::property;
use energyml_domain::property::shape::ComponentPick;
use energyml_domain::series::matrix::read_plane;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_sheet::draw::raster::Raster;
use energyml_sheet::draw::{DisplayList, Item, Paint, Rect};
use energyml_views::image_track::ImageTrack;
use energyml_views::{Drawn, ViewAsk, ViewKinds, ViewRefusal, draw_list, value_colours};
use serde_json::json;

use crate::fixtures::{FRAME, NO_MAPS, Opened, frame_uri, package, texts, uri};

const PROP: &str = "44444444-4444-4444-8444-444444444444";
const SCALAR: &str = "55555555-5555-4555-8555-555555555555";

fn prop_uri(uuid: &str) -> String {
    uri("resqml22.ContinuousProperty", uuid)
}

fn property(
    uuid: &str,
    indexable: eml23::IndexableElement,
    per_element: Vec<i64>,
    values: Vec<f64>,
) -> resqml22::ContinuousProperty {
    resqml22::ContinuousProperty {
        uuid: uuid.to_string(),
        schema_version: "2.2".into(),
        citation: eml23::Citation {
            title: "Rh".to_string(),
            ..Default::default()
        },
        indexable_element: indexable,
        value_count_per_indexable_element: per_element,
        supporting_representation: eml23::DataObjectReference {
            uuid: FRAME.to_string(),
            qualified_type: "resqml22.WellboreFrameRepresentation".to_string(),
            title: "a frame".to_string(),
            ..Default::default()
        },
        uom: eml23::UnitOfMeasureExt::from_xml_text("ohm.m").expect("a unit"),
        values_for_patch: vec![resqml22::AbstractValueArray::FloatingPointXmlArray(
            eml23::FloatingPointXmlArray {
                count_per_value: 1,
                values,
                ..Default::default()
            },
        )],
        ..Default::default()
    }
}

/// `10 n + c + 1` for node `n`, component `c` of 3: every cell its own value, 1 to 34.
fn ramp(rows: usize) -> Vec<f64> {
    (0..rows)
        .flat_map(|n| (0..3).map(move |c| (10 * n + c + 1) as f64))
        .collect()
}

/// A package with a frame at `mds` and `object` on it.
fn opened(mds: &[f64], object: resqml22::ContinuousProperty) -> Opened {
    let mut store = package(50.0, mds);
    store.put(Box::new(object)).expect("it goes in");
    Opened::of(store)
}

/// Four nodes 10 m apart carrying `[3]` values each.
fn standard() -> Opened {
    opened(
        &[100.0, 110.0, 120.0, 130.0],
        property(PROP, eml23::IndexableElement::Nodes, vec![3], ramp(4)),
    )
}

fn kinds() -> ViewKinds {
    let mut kinds = ViewKinds::empty();
    kinds.register(Box::new(ImageTrack));
    kinds
}

/// A plot of `w` by `h` pixels: the page leaves 70 left, 110 right, 44 above and 56 below it.
fn ask_sized(uuid: &str, w: f64, h: f64) -> ViewAsk {
    ViewAsk::new("image-track")
        .object(prop_uri(uuid))
        .sized(70.0 + w + 110.0, 44.0 + h + 56.0)
}

fn render(opened: &Opened, ask: ViewAsk) -> Result<Drawn, ViewRefusal> {
    draw_list(&kinds(), &ask, &opened.context(&NO_MAPS)).map(|(drawn, _)| drawn)
}

fn image(list: &DisplayList) -> (Rect, Arc<Raster>) {
    list.layers
        .iter()
        .filter(|layer| layer.name == "image")
        .flat_map(|layer| layer.elements.iter())
        .flat_map(|element| element.items.iter())
        .find_map(|item| match item {
            Item::Image { rect, raster } => Some((*rect, raster.clone())),
            _ => None,
        })
        .expect("an image")
}

fn pixel(raster: &Raster, x: u32, y: u32) -> [u8; 4] {
    raster.pixel(x, y).expect("a pixel in the image")
}

fn colours_over(opened: &Opened, uuid: &str, bounds: (f64, f64), log: bool) -> energyml_views::ValueColours {
    value_colours(
        &opened.context(&NO_MAPS),
        "image-track",
        Some(&prop_uri(uuid)),
        None,
        bounds,
        log,
        Reading::Continuous,
    )
    .expect("colours")
}

fn note_keys(drawn: &Drawn) -> Vec<&'static str> {
    drawn.notes.iter().map(|note| note.key).collect()
}

#[test]
fn a_property_with_several_values_per_element_is_offered_and_a_curve_is_not() {
    let mut store = package(50.0, &[100.0, 110.0, 120.0, 130.0]);
    store
        .put(Box::new(property(
            PROP,
            eml23::IndexableElement::Nodes,
            vec![3],
            ramp(4),
        )))
        .expect("in");
    store
        .put(Box::new(property(
            SCALAR,
            eml23::IndexableElement::Nodes,
            vec![1],
            vec![1.0, 2.0, 3.0, 4.0],
        )))
        .expect("in");
    let opened = Opened::of(store);
    let offers = kinds().offers(&opened.index);
    let objects: Vec<&str> = offers.iter().map(|offer| offer.objects[0].as_str()).collect();
    assert_eq!(objects, vec![prop_uri(PROP).as_str()], "{objects:?}");
    assert!(offers[0].missing.is_empty());
    // Break it: offer every property, or drop the test on the values per element.
}

#[test]
fn the_colour_of_a_cell_is_the_colour_of_the_value_read_plane_gives_there() {
    let opened = standard();
    let list = render(
        &opened,
        ask_sized(PROP, 300.0, 400.0)
            .set("scale", json!("linear"))
            .set("min", json!(0.0))
            .set("max", json!(50.0)),
    )
    .expect("drawn")
    .list;
    let (rect, raster) = image(&list);
    assert_eq!((rect.w, rect.h), (300.0, 400.0));
    assert_eq!((raster.width(), raster.height()), (300, 400));

    // The values, from the library's own door, never from the view.
    let object = opened.store.get_object(&prop_uri(PROP)).expect("the property");
    let read = property::read(object.as_reflect()).expect("a property");
    let plane = read_plane(&prop_uri(PROP), &read, 4, &ComponentPick::All, &opened.store).expect("a plane");
    assert_eq!((plane.rows, plane.cols), (4, 3));
    let colours = colours_over(&opened, PROP, (0.0, 50.0), false);

    // 4 nodes over 40 m of window in 400 pixels, 3 components in 300: 100 pixels a cell.
    let mut seen = Vec::new();
    for row in 0..4usize {
        for col in 0..3usize {
            let expected = colours.sample(plane.values[row * 3 + col]).expect("a colour");
            let got = pixel(&raster, col as u32 * 100 + 50, row as u32 * 100 + 50);
            assert_eq!(got, expected, "row {row}, column {col}");
            seen.push(got);
        }
    }
    seen.sort();
    seen.dedup();
    assert_eq!(
        seen.len(),
        12,
        "twelve values, twelve colours: nothing is transposed or repeated"
    );
    // Break it: paint with `values[col * rows + row]` (the plane transposed), or colour the plane's neighbouring row.
}

#[test]
fn the_values_of_a_pixel_row_are_the_node_when_it_covers_and_the_gap_stays_empty() {
    // Nodes 10 m apart, then one 80 m below: the rule bridges 3 median steps, so half of it each side.
    let opened = opened(
        &[100.0, 110.0, 120.0, 200.0],
        property(PROP, eml23::IndexableElement::Nodes, vec![3], ramp(4)),
    );
    let drawn = render(
        &opened,
        ask_sized(PROP, 300.0, 1200.0)
            .set("scale", json!("linear"))
            .set("min", json!(0.0))
            .set("max", json!(50.0)),
    )
    .expect("drawn");
    let (_, raster) = image(&drawn.list);
    // The window is 95 to 215 m, 10 pixels a metre: MD 160 is at pixel 650, in the hole.
    assert_eq!(pixel(&raster, 50, 650)[3], 0, "a hole stays a hole");
    assert_ne!(pixel(&raster, 50, 1050)[3], 0, "MD 200 is drawn");
    assert!(note_keys(&drawn).contains(&"md-gaps"), "{:?}", note_keys(&drawn));
    // Break it: fill a hole with the nearest sample, or lift the 3-step rule.
}

#[test]
fn an_interval_covers_exactly_its_own_depths() {
    // Three intervals between four nodes: 100-110, 110-120, 120-130, 10 pixels a metre.
    let opened = opened(
        &[100.0, 110.0, 120.0, 130.0],
        property(PROP, eml23::IndexableElement::Intervals, vec![3], ramp(3)),
    );
    let list = render(
        &opened,
        ask_sized(PROP, 300.0, 300.0)
            .set("scale", json!("linear"))
            .set("min", json!(0.0))
            .set("max", json!(50.0)),
    )
    .expect("drawn")
    .list;
    let (rect, raster) = image(&list);
    assert_eq!((rect.w, rect.h), (300.0, 300.0));
    let colours = colours_over(&opened, PROP, (0.0, 50.0), false);
    let values = ramp(3);
    for row in 0..3usize {
        let expected = colours.sample(values[row * 3 + 1]).expect("a colour");
        assert_eq!(
            pixel(&raster, 150, row as u32 * 100),
            expected,
            "first pixel row of interval {row}"
        );
        assert_eq!(
            pixel(&raster, 150, row as u32 * 100 + 99),
            expected,
            "last pixel row of interval {row}"
        );
    }
    // Break it: cover intervals like nodes (half the way to the neighbours), and the first pixel row changes.
}

#[test]
fn several_samples_in_one_pixel_row_are_reduced_by_the_id_asked() {
    // 4 nodes in 2 pixel rows of 20 m: rows hold nodes (100, 110) and (120, 130).
    let opened = standard();
    let ask = |reduce: &str| {
        ask_sized(PROP, 300.0, 2.0)
            .set("scale", json!("linear"))
            .set("min", json!(0.0))
            .set("max", json!(50.0))
            .set("reduce", json!(reduce))
    };
    let colours = colours_over(&opened, PROP, (0.0, 50.0), false);

    let mean = render(&opened, ask("mean")).expect("drawn");
    assert_eq!(
        pixel(&image(&mean.list).1, 50, 0),
        colours.sample((1.0 + 11.0) / 2.0).unwrap()
    );
    assert_eq!(
        pixel(&image(&mean.list).1, 50, 1),
        colours.sample((21.0 + 31.0) / 2.0).unwrap()
    );
    assert!(note_keys(&mean).contains(&"rows-reduced"));

    // The nodes 100 and 110 are both 5 m from the pixel row's centre: the first is kept.
    let nearest = render(&opened, ask("nearest")).expect("drawn");
    assert_eq!(
        pixel(&image(&nearest.list).1, 50, 0),
        colours.sample(1.0).unwrap()
    );
    assert_ne!(
        pixel(&image(&mean.list).1, 50, 0),
        pixel(&image(&nearest.list).1, 50, 0),
        "the two reductions differ"
    );
    // Break it: reduce with `max` whatever the id.
}

#[test]
fn a_logarithmic_scale_colours_by_decades_and_graduates_by_decades() {
    let opened = standard();
    let drawn = render(
        &opened,
        ask_sized(PROP, 300.0, 400.0)
            .set("scale", json!("log"))
            .set("min", json!(1.0))
            .set("max", json!(100.0)),
    )
    .expect("drawn");
    let (_, raster) = image(&drawn.list);
    let colours = colours_over(&opened, PROP, (1.0, 100.0), true);
    // Node 1, component 1 holds 12; node 3, component 2 holds 33.
    assert_eq!(pixel(&raster, 150, 150), colours.sample(12.0).unwrap());
    assert_eq!(pixel(&raster, 250, 350), colours.sample(33.0).unwrap());
    let words = texts(&drawn.list);
    assert!(
        words.contains(&"100".to_string()) && words.contains(&"10".to_string()),
        "{words:?}"
    );
    // Break it: graduate the colour bar with the linear ticks.
}

#[test]
fn a_value_with_no_logarithm_is_counted_and_left_transparent() {
    let mut values = ramp(4);
    values[3 + 2] = 0.0; // node 1 (3 values a node), component 2
    let opened = opened(
        &[100.0, 110.0, 120.0, 130.0],
        property(PROP, eml23::IndexableElement::Nodes, vec![3], values),
    );
    let drawn = render(
        &opened,
        ask_sized(PROP, 300.0, 400.0)
            .set("scale", json!("log"))
            .set("min", json!(1.0))
            .set("max", json!(100.0)),
    )
    .expect("drawn");
    let (_, raster) = image(&drawn.list);
    assert_eq!(pixel(&raster, 250, 150)[3], 0, "no colour for zero");
    assert_ne!(pixel(&raster, 150, 150)[3], 0, "its neighbour is painted");
    assert!(note_keys(&drawn).contains(&"cells-not-coloured"));
    // Break it: colour the zero with the lowest colour of the map.
}

#[test]
fn auto_picks_a_logarithmic_scale_when_the_values_span_decades_and_says_so() {
    let wide: Vec<f64> = (0..12).map(|k| 10f64.powf(k as f64 / 2.0)).collect();
    let opened = opened(
        &[100.0, 110.0, 120.0, 130.0],
        property(PROP, eml23::IndexableElement::Nodes, vec![3], wide),
    );
    let drawn = render(&opened, ask_sized(PROP, 300.0, 400.0)).expect("drawn");
    assert!(
        note_keys(&drawn).contains(&"scale-log-proposed"),
        "{:?}",
        note_keys(&drawn)
    );
    // A narrow property keeps the linear scale.
    let narrow = drawn_of_standard();
    assert!(!note_keys(&narrow).contains(&"scale-log-proposed"));
    // And `linear` overrides.
    let forced = render(
        &opened,
        ask_sized(PROP, 300.0, 400.0).set("scale", json!("linear")),
    )
    .expect("drawn");
    assert!(!note_keys(&forced).contains(&"scale-log-proposed"));
    // Break it: propose the log scale for every property.
}

fn drawn_of_standard() -> Drawn {
    render(&standard(), ask_sized(PROP, 300.0, 400.0)).expect("drawn")
}

#[test]
fn the_plane_shown_follows_pick_and_the_default_says_so() {
    // `[3, 2]` per node: 100 n + 10 i + j.
    let values: Vec<f64> = (0..4)
        .flat_map(|n| (0..3).flat_map(move |i| (0..2).map(move |j| (100 * n + 10 * i + j) as f64)))
        .collect();
    let opened = opened(
        &[100.0, 110.0, 120.0, 130.0],
        property(PROP, eml23::IndexableElement::Nodes, vec![3, 2], values),
    );
    let object = opened.store.get_object(&prop_uri(PROP)).expect("the property");
    let read = property::read(object.as_reflect()).expect("a property");
    let colours = colours_over(&opened, PROP, (0.0, 400.0), false);
    let base = |extra: Option<serde_json::Value>| {
        let ask = ask_sized(PROP, 300.0, 400.0)
            .set("scale", json!("linear"))
            .set("min", json!(0.0))
            .set("max", json!(400.0));
        render(
            &opened,
            if let Some(pick) = extra {
                ask.set("pick", pick)
            } else {
                ask
            },
        )
        .expect("drawn")
    };

    // Default: along the slowest dimension at index 0 of the others, and the note says it.
    let default = base(None);
    assert!(note_keys(&default).contains(&"pick-defaulted"));
    let along_first = read_plane(
        &prop_uri(PROP),
        &read,
        4,
        &ComponentPick::Along {
            axis: 0,
            at: vec![0, 0],
        },
        &opened.store,
    )
    .expect("a plane");
    assert_eq!(along_first.cols, 3);
    let raster = image(&default.list).1;
    for row in 0..4usize {
        for col in 0..3usize {
            assert_eq!(
                pixel(&raster, col as u32 * 100 + 50, row as u32 * 100 + 50),
                colours.sample(along_first.values[row * 3 + col]).unwrap(),
                "default, row {row} column {col}"
            );
        }
    }

    // Asked: along the fast dimension at index 2 of the slow one: two columns of 150 pixels.
    let asked = base(Some(json!({"along": 1, "at": [2, 0]})));
    assert!(!note_keys(&asked).contains(&"pick-defaulted"));
    let along_second = read_plane(
        &prop_uri(PROP),
        &read,
        4,
        &ComponentPick::Along {
            axis: 1,
            at: vec![2, 0],
        },
        &opened.store,
    )
    .expect("a plane");
    assert_eq!(along_second.cols, 2);
    let raster = image(&asked.list).1;
    for row in 0..4usize {
        for col in 0..2usize {
            assert_eq!(
                pixel(&raster, col as u32 * 150 + 75, row as u32 * 100 + 50),
                colours.sample(along_second.values[row * 2 + col]).unwrap(),
                "asked, row {row} column {col}"
            );
        }
    }
    // Break it: ignore `along` and always show the slowest dimension.
}

#[test]
fn rows_and_columns_are_keyed_by_their_full_uri_and_their_index() {
    let drawn = render(&standard(), ask_sized(PROP, 300.0, 400.0)).expect("drawn");
    let uri = prop_uri(PROP);
    let element = |key: String| {
        drawn
            .list
            .layers
            .iter()
            .flat_map(|layer| layer.elements.iter())
            .find(|element| element.key.as_deref() == Some(key.as_str()))
            .unwrap_or_else(|| panic!("no element `{key}`"))
            .clone()
    };
    assert_eq!(element(uri.clone()).kind, "image");
    let row = element(format!("{uri}#row=2"));
    let Item::Rect { rect, .. } = &row.items[0] else {
        panic!("a rectangle")
    };
    let (area, _) = image(&drawn.list);
    assert_eq!(
        (rect.x, rect.y, rect.w, rect.h),
        (area.x, area.y + 200.0, 300.0, 100.0)
    );
    assert!(
        row.data.contains(&("md".to_string(), "120".to_string())),
        "{:?}",
        row.data
    );
    let column = element(format!("{uri}#col=1"));
    let Item::Rect { rect, .. } = &column.items[0] else {
        panic!("a rectangle")
    };
    assert_eq!((rect.x, rect.w, rect.h), (area.x + 100.0, 100.0, 400.0));
    let keys = drawn.list.keys();
    let rows = keys.iter().filter(|key| key.contains("#row=")).count();
    let cols = keys.iter().filter(|key| key.contains("#col=")).count();
    assert_eq!((rows, cols), (4, 3));
    // Break it: key the rows by their pixel number rather than by the sample's index.
}

/// Pointing at the middle of a row or of a column finds that row or column, not
/// the image under it (`[views-sheet C1]`): the hit elements are areas, and the
/// positions asked are inside them, far from any border.
#[test]
fn pointing_inside_a_row_or_a_column_finds_it() {
    let drawn = render(&standard(), ask_sized(PROP, 300.0, 400.0)).expect("drawn");
    let uri = prop_uri(PROP);
    let (area, _) = image(&drawn.list);
    // Columns are 100 wide and rows 100 high: the middle of row 2 / column 1.
    let (x, y) = (area.x + 150.0, area.y + 250.0);
    let on_top = energyml_sheet::draw::hit(&drawn.list, x, y).expect("something is there");
    assert_eq!(on_top.key, format!("{uri}#col=1"), "the topmost hit element");
    // Without the columns, the row under the same point.
    let mut rows_only = drawn.list.clone();
    for layer in &mut rows_only.layers {
        layer.elements.retain(|element| element.kind != "column");
    }
    let found = energyml_sheet::draw::hit(&rows_only, x, y).expect("the row");
    assert_eq!(found.key, format!("{uri}#row=2"));
    // Break it: give `invisible` in `image_track.rs` neither fill nor stroke
    // and take the area test out of `draw::hit`: the point finds the image.
}

#[test]
fn the_colour_bar_and_its_source_come_from_the_one_door() {
    let opened = standard();
    let drawn = render(
        &opened,
        ask_sized(PROP, 300.0, 400.0)
            .set("scale", json!("linear"))
            .set("min", json!(0.0))
            .set("max", json!(50.0)),
    )
    .expect("drawn");
    let colours = colours_over(&opened, PROP, (0.0, 50.0), false);
    let legend = drawn
        .list
        .layers
        .iter()
        .find(|layer| layer.name == "legend")
        .expect("a legend");
    let first = legend.elements[0]
        .items
        .iter()
        .find_map(|item| match item {
            Item::Rect {
                fill: Some(Paint::Solid(colour)),
                ..
            } => Some([colour.r, colour.g, colour.b, colour.a]),
            _ => None,
        })
        .expect("a band");
    // The top band is the highest hundredth: its centre is 49.75.
    assert_eq!(Some(first), colours.sample(49.75));
    assert!(
        texts(&drawn.list).contains(&colours.source.says()),
        "{:?}",
        texts(&drawn.list)
    );
    assert!(texts(&drawn.list).contains(&"ohm.m".to_string()));
    // Break it: paint the bar from a second ramp of the view's own.
}

#[test]
fn a_palette_is_by_name_and_an_unknown_one_is_refused() {
    let opened = standard();
    let name = opened
        .palettes
        .default_for(Reading::Continuous)
        .expect("a default")
        .name;
    let named = render(&opened, ask_sized(PROP, 300.0, 400.0).set("palette", json!(name))).expect("drawn");
    assert!(texts(&named.list).contains(&format!("colours: palette {name} (not from the file)")));
    let refused = render(
        &opened,
        ask_sized(PROP, 300.0, 400.0).set("palette", json!("no-such-palette")),
    )
    .expect_err("refused");
    assert!(matches!(refused, ViewRefusal::Palette { .. }), "{refused}");
    // Break it: fall back to the default palette when the name is unknown.
}

#[test]
fn the_axis_of_the_components_says_when_it_is_only_ranks_or_names() {
    let standard = drawn_of_standard();
    assert!(
        note_keys(&standard).contains(&"axis-ranks-only"),
        "{:?}",
        note_keys(&standard)
    );
    let mut labelled = property(PROP, eml23::IndexableElement::Nodes, vec![3], ramp(4));
    labelled.label_per_component = vec!["a".into(), "b".into(), "c".into()];
    let opened = opened(&[100.0, 110.0, 120.0, 130.0], labelled);
    let named = render(&opened, ask_sized(PROP, 300.0, 400.0)).expect("drawn");
    assert!(
        note_keys(&named).contains(&"axis-labels-not-drawn"),
        "{:?}",
        note_keys(&named)
    );
    assert!(!note_keys(&named).contains(&"axis-ranks-only"));
    // Break it: say nothing when the axis is only ranks.
}

#[test]
fn what_cannot_be_shown_is_refused_by_name_never_drawn_empty() {
    let mut store = package(50.0, &[100.0, 110.0, 120.0, 130.0]);
    store
        .put(Box::new(property(
            PROP,
            eml23::IndexableElement::Nodes,
            vec![3],
            ramp(4),
        )))
        .expect("in");
    store
        .put(Box::new(property(
            SCALAR,
            eml23::IndexableElement::Nodes,
            vec![1],
            vec![1.0, 2.0, 3.0, 4.0],
        )))
        .expect("in");
    let bad_count = "66666666-6666-4666-8666-666666666666";
    store
        .put(Box::new(property(
            bad_count,
            eml23::IndexableElement::Nodes,
            vec![3],
            vec![1.0; 5],
        )))
        .expect("in");
    let opened = Opened::of(store);
    let why = |ask: ViewAsk| match render(&opened, ask).expect_err("refused") {
        ViewRefusal::Computing { why, .. } => why,
        other => panic!("not a refusal of the computing: {other}"),
    };

    assert!(why(ask_sized(SCALAR, 300.0, 400.0)).contains("one value per element"));
    assert!(why(ask_sized(bad_count, 300.0, 400.0)).contains("neither"));
    // A component the shape does not have.
    let out_of_range = why(ask_sized(PROP, 300.0, 400.0).set("pick", json!({"at": [5]})));
    assert!(!out_of_range.is_empty());
    assert!(
        why(ask_sized(PROP, 300.0, 400.0)
            .set("md_from", json!(130.0))
            .set("md_to", json!(100.0)))
        .contains("empty")
    );
    assert!(
        why(ask_sized(PROP, 300.0, 400.0)
            .set("md_from", json!(500.0))
            .set("md_to", json!(600.0)))
        .contains("no sample lies")
    );
    // A pick that is not one.
    let nonsense = render(
        &opened,
        ask_sized(PROP, 300.0, 400.0).set("pick", json!({"foo": 1})),
    )
    .expect_err("refused");
    assert!(
        matches!(nonsense, ViewRefusal::BadSetting { ref name, .. } if name == "pick"),
        "{nonsense}"
    );
    // The frame itself is not a property.
    let frame = draw_list(
        &kinds(),
        &ViewAsk::new("image-track").object(frame_uri()),
        &opened.context(&NO_MAPS),
    )
    .expect_err("refused");
    assert!(matches!(frame, ViewRefusal::ObjectNotTaken { .. }), "{frame}");
    // Break it: let a one-column plane through (`cols == 1` is a curve), or draw the window empty.
}
