//! `property-insight`: the four blocks of the figure, each confronted with the computing it lays out. Every package is
//! fabricated in memory: a frame of twenty nodes (`fixtures::package`) and properties with their values inline.
//!
//! The values are the test's own, the forms come from the doors of `energyml-domain` (`stats::describe`,
//! `stats::axes::variation`, `stats::pairs::PairAccumulator`): a bar, a pixel or a width is checked against what the
//! door answers on the same values, never against what the view says.

use energyml_core::xml::XmlText;
use energyml_domain::palettes::Reading;
use energyml_domain::stats::axes::variation;
use energyml_domain::stats::describe::{Plan, describe};
use energyml_domain::stats::pairs::{DENSITY_SIZE, PairAccumulator};
use energyml_domain::stats::scale::ScaleKind;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_sheet::draw::{DisplayList, Item, Paint, Rect};
use energyml_views::property_insight::PropertyInsight;
use energyml_views::{Drawn, ViewAsk, ViewKinds, ViewRefusal, draw_list, draw_svg, redraw, value_colours};
use serde_json::json;

use crate::fixtures::{NO_MAPS, Opened, package, texts};

const KIND: &str = "55555555-5555-4555-8555-555555555500";
const RH: &str = "55555555-5555-4555-8555-555555555501";
const RV: &str = "55555555-5555-4555-8555-555555555502";
const ALONE: &str = "55555555-5555-4555-8555-555555555503";
const FRAME_NODES: usize = 20;

fn kinds() -> ViewKinds {
    let mut kinds = ViewKinds::empty();
    kinds.register(Box::new(PropertyInsight));
    kinds
}

/// A property of `per_node` values per node of the frame, in ohm.m; `direction` is its `direction` facet.
fn property(
    uuid: &str,
    title: &str,
    per_node: Vec<i64>,
    values: Vec<f64>,
    direction: Option<&str>,
) -> resqml22::ContinuousProperty {
    resqml22::ContinuousProperty {
        uuid: uuid.to_string(),
        citation: eml23::Citation {
            title: title.to_string(),
            originator: "test".into(),
            creation: "2026-11-15T00:00:00Z".into(),
            format: "test".into(),
            ..Default::default()
        },
        indexable_element: eml23::IndexableElement::Nodes,
        value_count_per_indexable_element: per_node,
        property_kind: eml23::DataObjectReference {
            uuid: KIND.to_string(),
            qualified_type: "eml23.PropertyKind".to_string(),
            title: "resistivity".to_string(),
            ..Default::default()
        },
        facet: direction
            .map(|value| {
                vec![eml23::PropertyKindFacet {
                    facet: match value {
                        "I" => eml23::FacetExt::I,
                        "K" => eml23::FacetExt::K,
                        other => eml23::FacetExt::Other(other.to_string()),
                    },
                    kind: eml23::FacetKind::Direction,
                }]
            })
            .unwrap_or_default(),
        uom: eml23::UnitOfMeasureExt::from_xml_text("ohm.m").expect("a unit"),
        supporting_representation: eml23::DataObjectReference {
            uuid: crate::fixtures::FRAME.to_string(),
            qualified_type: "resqml22.WellboreFrameRepresentation".to_string(),
            title: "a frame".to_string(),
            ..Default::default()
        },
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

/// The package of the fixtures (a frame of twenty nodes) with these properties, and their uris in order.
fn world(properties: Vec<resqml22::ContinuousProperty>) -> (Opened, Vec<String>) {
    let mds: Vec<f64> = (0..FRAME_NODES).map(|k| 5.0 * k as f64).collect();
    let mut store = package(50.0, &mds);
    let uris = properties
        .into_iter()
        .map(|object| store.put(Box::new(object)).expect("the package takes it"))
        .collect();
    (Opened::of(store), uris)
}

/// 1 to 56 234 ohm.m, a quarter of a decade apart: 4.75 decades, one value a node.
fn spread() -> Vec<f64> {
    (0..FRAME_NODES).map(|k| 10f64.powf(k as f64 / 4.0)).collect()
}

fn drawn(opened: &Opened, ask: &ViewAsk) -> Drawn {
    draw_list(&kinds(), ask, &opened.context(&NO_MAPS))
        .expect("drawn")
        .0
}

fn refused(opened: &Opened, ask: &ViewAsk) -> ViewRefusal {
    draw_list(&kinds(), ask, &opened.context(&NO_MAPS)).expect_err("refused")
}

fn has_note(drawn: &Drawn, key: &str) -> bool {
    drawn.notes.iter().any(|note| note.key == key)
}

fn has_layer(list: &DisplayList, name: &str) -> bool {
    list.layers.iter().any(|layer| layer.name == name)
}

/// What a test reads of one drawn bar.
struct Bar {
    at: usize,
    rect: Rect,
    fill: Option<[u8; 4]>,
    count: u64,
}

fn bars(list: &DisplayList) -> Vec<Bar> {
    let mut found: Vec<Bar> = list
        .layers
        .iter()
        .filter(|layer| layer.name == "distribution")
        .flat_map(|layer| layer.elements.iter())
        .filter(|element| element.kind == "bin")
        .map(|element| {
            let at = element
                .key
                .as_deref()
                .and_then(|key| key.rsplit("#bin=").next())
                .and_then(|at| at.parse().ok())
                .expect("a bin key");
            let count = element
                .data
                .iter()
                .find(|(name, _)| name == "count")
                .and_then(|(_, count)| count.parse().ok())
                .expect("a count");
            match &element.items[0] {
                Item::Rect { rect, fill, .. } => Bar {
                    at,
                    rect: *rect,
                    fill: match fill {
                        Some(Paint::Solid(colour)) => Some([colour.r, colour.g, colour.b, colour.a]),
                        _ => None,
                    },
                    count,
                },
                other => panic!("a bar is a rectangle: {other:?}"),
            }
        })
        .collect();
    found.sort_by_key(|bar| bar.at);
    found
}

/// The frame of a plot: the rectangle of the second element of its axes layer.
fn frame(list: &DisplayList, layer: &str) -> Rect {
    let layer = list.layer(layer).expect("an axes layer");
    match &layer.elements[1].items[0] {
        Item::Rect { rect, .. } => *rect,
        other => panic!("the frame is a rectangle: {other:?}"),
    }
}

fn ask(uri: &str) -> ViewAsk {
    ViewAsk::new("property-insight").object(uri)
}

#[test]
fn every_property_without_a_lookup_table_is_offered_and_nothing_else() {
    let (opened, uris) = world(vec![property(RH, "Rh", vec![1], spread(), None)]);
    let offers = kinds().offers(&opened.index);
    let objects: Vec<&str> = offers.iter().map(|offer| offer.objects[0].as_str()).collect();
    assert_eq!(
        objects,
        vec![uris[0].as_str()],
        "the frame and the trajectory are not properties"
    );
    assert!(offers.iter().all(|offer| offer.missing.is_empty()));
    // Break it: offer every object of the package.
}

#[test]
fn the_height_of_each_bar_is_the_count_of_the_door() {
    let values = spread();
    let (opened, uris) = world(vec![property(RH, "Rh", vec![1], values.clone(), None)]);
    let list = drawn(&opened, &ask(&uris[0])).list;
    let found = bars(&list);

    // The door, on the same values, with the plan the view follows by default.
    let door = describe(&values, &[FRAME_NODES as u64], Plan::default()).expect("a description");
    let binned = door.binned.expect("a histogram");
    assert_eq!(found.len(), binned.counts.len(), "one bar per bin");

    let plot = frame(&list, "distribution-axes");
    let most = *binned.counts.iter().max().expect("a count");
    for (bar, count) in found.iter().zip(&binned.counts) {
        assert_eq!(bar.count, *count, "bar {} says its count", bar.at);
        let expected = *count as f64 / most as f64 * plot.h;
        assert!(
            (bar.rect.h - expected).abs() < 1.0e-9,
            "bar {} is {} high, the door's count {count} of {most} makes {expected}",
            bar.at,
            bar.rect.h
        );
        assert!(
            (bar.rect.y + bar.rect.h - (plot.y + plot.h)).abs() < 1.0e-9,
            "bar {} stands on the axis",
            bar.at
        );
    }
    let tallest = found.iter().map(|bar| bar.rect.h).fold(0.0, f64::max);
    assert!(
        (tallest - plot.h).abs() < 1.0e-9,
        "the tallest bar fills the frame"
    );
    // Break it: scale the bars by the number of values instead of by the tallest count, or take the count of a
    // bin from the values read twice.
}

#[test]
fn a_property_spanning_decades_is_drawn_in_log_and_a_narrow_one_in_linear() {
    let (opened, uris) = world(vec![
        property(RH, "Rh", vec![1], spread(), None),
        property(
            RV,
            "Rv",
            vec![1],
            (0..FRAME_NODES).map(|k| 10.0 + k as f64).collect(),
            None,
        ),
    ]);
    let wide = drawn(&opened, &ask(&uris[0])).list;
    let narrow = drawn(&opened, &ask(&uris[1])).list;
    let labels = |list: &DisplayList| texts(list).join("\n");
    assert!(
        labels(&wide).contains("histogram on a logarithmic scale"),
        "{}",
        labels(&wide)
    );
    assert!(
        labels(&wide).contains("logarithmic scale"),
        "the axis title says so"
    );
    assert!(
        labels(&narrow).contains("histogram on a linear scale"),
        "{}",
        labels(&narrow)
    );
    // The graduations of a log axis are powers of ten.
    let ticks: Vec<String> = texts(&wide);
    assert!(
        ticks.iter().any(|tick| tick == "100") && ticks.iter().any(|tick| tick == "1000"),
        "{ticks:?}"
    );
    // The bins are equal steps of the log: the bars have one width.
    let widths: Vec<f64> = bars(&wide).iter().map(|bar| bar.rect.w).collect();
    assert!(widths.iter().all(|w| (w - widths[0]).abs() < 1.0e-9));
    // Asking linear changes it, the observation staying a fact.
    let forced = drawn(&opened, &ask(&uris[0]).set("scale", json!("linear"))).list;
    assert!(labels(&forced).contains("histogram on a linear scale"));
    assert!(
        labels(&forced).contains("a logarithmic scale is proposed"),
        "the proposal is still said"
    );
    // Break it: decide the scale on the kind of the property instead of the decades its values span.
}

#[test]
fn the_colour_of_a_bar_is_that_of_its_middle_value_through_the_one_door() {
    let values = spread();
    let (opened, uris) = world(vec![property(RH, "Rh", vec![1], values.clone(), None)]);
    let list = drawn(&opened, &ask(&uris[0]).set("palette", json!("Viridis"))).list;
    let found = bars(&list);

    let door = describe(&values, &[FRAME_NODES as u64], Plan::default()).expect("a description");
    let binned = door.binned.expect("a histogram");
    let scale = door.scale.expect("a scale");
    let cx = opened.context(&NO_MAPS);
    let colours = value_colours(
        &cx,
        "test",
        None,
        Some("Viridis"),
        (scale.min(), scale.max()),
        scale.kind() == ScaleKind::Log10,
        Reading::Continuous,
    )
    .expect("colours");
    for bar in &found {
        assert_eq!(
            bar.fill,
            colours.sample(binned.center(bar.at)),
            "bar {} is painted at its centre {}",
            bar.at,
            binned.center(bar.at)
        );
    }
    let fills: Vec<[u8; 4]> = found.iter().filter_map(|bar| bar.fill).collect();
    assert!(
        fills.first() != fills.last(),
        "the lowest and the highest bar have two colours"
    );
    assert!(
        texts(&list).iter().any(|text| text.contains("palette Viridis")),
        "the legend says the colours are not the file's"
    );
    // A palette that does not exist is a refusal by its name, not a grey histogram.
    let refusal = refused(&opened, &ask(&uris[0]).set("palette", json!("Nope")));
    assert!(
        matches!(&refusal, ViewRefusal::Palette { asked, .. } if asked == "Nope"),
        "{refusal}"
    );
    // Break it: paint every bar at the left edge of its bin, and the first colours move.
}

#[test]
fn the_modes_and_the_threshold_of_the_door_are_marked_where_it_puts_them() {
    // Two tight populations, ten values near 2 and ten near 200: an empty valley between.
    let mut values: Vec<f64> = (0..10).map(|k| 2.0 + 0.05 * k as f64).collect();
    values.extend((0..10).map(|k| 200.0 + 0.5 * k as f64));
    let (opened, uris) = world(vec![property(RH, "Rh", vec![1], values.clone(), None)]);
    let list = drawn(&opened, &ask(&uris[0])).list;

    let door = describe(&values, &[FRAME_NODES as u64], Plan::default()).expect("a description");
    assert_eq!(door.modes.len(), 2, "the door finds the two populations");
    let marks = list.layer("distribution-marks").expect("marks");
    let modes: Vec<_> = marks.elements.iter().filter(|e| e.kind == "mode").collect();
    assert_eq!(modes.len(), door.modes.len());

    let plot = frame(&list, "distribution-axes");
    let scale = door.scale.expect("a scale");
    for (element, mode) in modes.iter().zip(&door.modes) {
        let x = match &element.items[0] {
            Item::Symbol { at, .. } => at[0],
            other => panic!("a mode is a symbol: {other:?}"),
        };
        let fraction = match scale.fraction(mode.at) {
            energyml_domain::stats::scale::Fraction::At(fraction) => fraction,
            other => panic!("the mode is on the scale: {other:?}"),
        };
        assert!((x - (plot.x + fraction * plot.w)).abs() < 1.0e-9);
    }
    let threshold = door.threshold.expect("a threshold between two populations");
    let line = marks
        .elements
        .iter()
        .find(|element| element.kind == "threshold")
        .expect("a threshold line");
    let x = match &line.items[0] {
        Item::Polyline { points, .. } => points[0][0],
        other => panic!("the threshold is a line: {other:?}"),
    };
    let fraction = match scale.fraction(threshold.at) {
        energyml_domain::stats::scale::Fraction::At(fraction) => fraction,
        other => panic!("{other:?}"),
    };
    assert!((x - (plot.x + fraction * plot.w)).abs() < 1.0e-9);
    // The threshold lies between the two modes on the page too.
    let (low, high) = (
        door.modes[0].at.min(door.modes[1].at),
        door.modes[0].at.max(door.modes[1].at),
    );
    assert!(low < threshold.at && threshold.at < high);
    // Break it: put the mode marks at the bar's index times the bar width of a linear axis.
}

#[test]
fn the_density_has_the_cells_the_pairs_fill_and_a_diagonal() {
    let rh = spread();
    let rv: Vec<f64> = rh.iter().map(|value| value * 2.0).collect();
    let (opened, uris) = world(vec![
        property(RH, "Rh", vec![1], rh.clone(), Some("I")),
        property(RV, "Rv", vec![1], rv.clone(), Some("K")),
    ]);
    let list = drawn(&opened, &ask(&uris[0])).list;

    // The sister was found by its direction facet, not named.
    assert!(
        texts(&list)
            .iter()
            .any(|text| text.contains("told apart by their direction facet")),
        "{:?}",
        texts(&list)
    );
    let relation = list.layer("relation").expect("the relation block");
    let density = relation
        .elements
        .iter()
        .find(|element| element.kind == "density")
        .expect("a density");
    let raster = match &density.items[0] {
        Item::Image { raster, .. } => raster.clone(),
        other => panic!("the density is an image: {other:?}"),
    };
    assert_eq!(
        (raster.width(), raster.height()),
        (DENSITY_SIZE as u32, DENSITY_SIZE as u32)
    );

    // The door, on the same pairs and the scales each property is described with.
    let dims = [FRAME_NODES as u64];
    let x = describe(&rh, &dims, Plan::default())
        .expect("a description")
        .scale
        .expect("a scale");
    let y = describe(&rv, &dims, Plan::default())
        .expect("a description")
        .scale
        .expect("a scale");
    let mut pairs = PairAccumulator::new(x, y, DENSITY_SIZE, None).expect("an accumulator");
    pairs.push(&rh, &rv).expect("pairs");
    let summary = pairs.finish();
    let filled = summary.density.counts.iter().filter(|count| **count > 0).count();
    assert!(filled >= 2, "the pairs spread over cells");
    let painted = raster.rgba().chunks(4).filter(|pixel| pixel[3] > 0).count();
    assert_eq!(
        painted, filled,
        "a pixel is painted exactly where a cell holds a pair"
    );
    assert!(
        density
            .data
            .contains(&("pairs".to_string(), summary.pairs.to_string())),
        "{:?}",
        density.data
    );
    assert!(
        relation.elements.iter().any(|element| element.kind == "diagonal"),
        "the 1:1 diagonal"
    );
    // Break it: flip the rows of the image (row 0 of the grid is the top of the picture) and the painted pixels
    // are the mirror of the cells; the count stays, so also compare one pixel: the lowest cell is at the bottom.
}

#[test]
fn the_lowest_pairs_are_at_the_bottom_left_of_the_picture() {
    let rh = spread();
    let rv: Vec<f64> = rh.iter().map(|value| value * 2.0).collect();
    let (opened, uris) = world(vec![
        property(RH, "Rh", vec![1], rh.clone(), Some("I")),
        property(RV, "Rv", vec![1], rv.clone(), Some("K")),
    ]);
    let list = drawn(&opened, &ask(&uris[0])).list;
    let density = list
        .layer("relation")
        .expect("the relation block")
        .elements
        .iter()
        .find(|element| element.kind == "density")
        .expect("a density");
    let raster = match &density.items[0] {
        Item::Image { raster, .. } => raster.clone(),
        other => panic!("{other:?}"),
    };
    let n = raster.width();
    let painted: Vec<(u32, u32)> = (0..n)
        .flat_map(|y| (0..n).map(move |x| (x, y)))
        .filter(|(x, y)| raster.pixel(*x, *y).is_some_and(|pixel| pixel[3] > 0))
        .collect();
    // Both values grow together: the pixels run from the bottom left to the top right, never the other way.
    let (left_x, left_y) = *painted.iter().min_by_key(|(x, _)| *x).expect("a pixel");
    let (right_x, right_y) = *painted.iter().max_by_key(|(x, _)| *x).expect("a pixel");
    assert!(left_x < right_x);
    assert!(
        left_y > right_y,
        "the left pixel is lower on the page ({left_y} against {right_y})"
    );
    // Break it: write the grid's rows top down, and the line of pixels falls from the top left.
}

#[test]
fn the_share_of_pairs_under_a_ratio_is_the_door_s_count() {
    // 18 pairs where the sister is 1.2 times this one, 2 where it is 0.9 times it: 10 % under 0.95.
    let rh = spread();
    let mut rv: Vec<f64> = rh.iter().map(|value| value * 1.2).collect();
    rv[0] *= 0.9 / 1.2;
    rv[1] *= 0.9 / 1.2;
    let (opened, uris) = world(vec![
        property(RH, "Rh", vec![1], rh.clone(), Some("I")),
        property(RV, "Rv", vec![1], rv.clone(), Some("K")),
    ]);
    let list = drawn(&opened, &ask(&uris[0]).set("ratio_below", json!(0.95))).list;
    let words = texts(&list).join("\n");
    assert!(
        words.contains("ratio below 0.95 in 10 % of the pairs (2 of 20)"),
        "{words}"
    );

    // The door: 2 of 20 by the cell rule, counted again here.
    let dims = [FRAME_NODES as u64];
    let x = describe(&rh, &dims, Plan::default())
        .expect("a description")
        .scale
        .expect("a scale");
    let y = describe(&rv, &dims, Plan::default())
        .expect("a description")
        .scale
        .expect("a scale");
    let mut pairs = PairAccumulator::new(
        x,
        y,
        DENSITY_SIZE,
        Some(energyml_domain::stats::cells::CellRule::Below(0.95)),
    )
    .expect("an accumulator");
    pairs.push(&rh, &rv).expect("pairs");
    let summary = pairs.finish();
    assert_eq!((summary.matched, summary.pairs), (2, 20));
    // Break it: count the pairs below the diagonal of the density instead of the ratio of the values.
}

#[test]
fn the_axis_that_varies_most_is_marked_and_the_bars_follow_the_door() {
    // Twenty nodes of three components: 100 per node, 1 per component.
    let mut values = Vec::new();
    for node in 0..FRAME_NODES {
        for component in 0..3 {
            values.push(100.0 * node as f64 + component as f64);
        }
    }
    let (opened, uris) = world(vec![property(RH, "Rh", vec![3], values.clone(), None)]);
    let list = drawn(&opened, &ask(&uris[0]).set("scale", json!("linear"))).list;

    let door = variation(&values, &[FRAME_NODES as u64, 3], ScaleKind::Linear).expect("a variation");
    let steps: Vec<f64> = door
        .iter()
        .map(|axis| axis.mean_abs_step.expect("a step"))
        .collect();
    assert!(
        steps[0] > steps[1],
        "the door says the nodes vary more than the components"
    );

    let layer = list.layer("variation").expect("the variation block");
    assert_eq!(layer.elements.len(), 2, "one bar per axis");
    let width_of = |element: &energyml_sheet::draw::Element| {
        element
            .items
            .iter()
            .find_map(|item| match item {
                Item::Rect { rect, .. } => Some(rect.w),
                _ => None,
            })
            .expect("a bar")
    };
    let widths: Vec<f64> = layer.elements.iter().map(width_of).collect();
    assert!(
        (widths[1] / widths[0] - steps[1] / steps[0]).abs() < 1.0e-9,
        "the bars are as the door's steps: {widths:?} against {steps:?}"
    );
    let strongest: Vec<bool> = layer
        .elements
        .iter()
        .map(|element| {
            element
                .data
                .contains(&("strongest".to_string(), "true".to_string()))
        })
        .collect();
    assert_eq!(strongest, vec![true, false], "the nodes axis is the strongest");
    assert!(
        layer.elements[0]
            .key
            .as_deref()
            .is_some_and(|key| key.ends_with("#axis=0"))
    );
    // Break it: mark the longest label instead of the door's strongest axis, or give every bar the same width.
}

#[test]
fn a_property_the_description_refuses_keeps_its_facts_and_says_why() {
    // Three values per node declared, thirty held for twenty nodes: the counts fit no shape.
    let (opened, uris) = world(vec![property(
        RH,
        "Rh",
        vec![3],
        (0..30).map(|k| k as f64 + 1.0).collect(),
        None,
    )]);
    let result = drawn(&opened, &ask(&uris[0]));
    assert!(has_note(&result, "description-refused"), "{:?}", result.notes);
    assert!(has_layer(&result.list, "form"), "the shape is still written");
    assert!(
        !has_layer(&result.list, "distribution") && !has_layer(&result.list, "variation"),
        "nothing is drawn from values that were not described"
    );
    assert!(
        texts(&result.list)
            .iter()
            .any(|text| text.contains("3 value(s) per element") || text.contains("[3]")),
        "{:?}",
        texts(&result.list)
    );
    // Break it: draw an empty histogram when the description refuses.
}

#[test]
fn several_values_declared_and_one_held_is_read_as_scalars_and_said() {
    // Three values per node declared, twenty held: one per node, the declaration is what is wrong.
    let (opened, uris) = world(vec![property(RH, "Rh", vec![3], spread(), None)]);
    let result = drawn(&opened, &ask(&uris[0]));
    assert!(has_note(&result, "read-as-scalars"), "{:?}", result.notes);
    assert!(has_layer(&result.list, "distribution"), "and it is drawn");
    let door = describe(&spread(), &[FRAME_NODES as u64], Plan::default()).expect("a description");
    let counts: Vec<u64> = bars(&result.list).iter().map(|bar| bar.count).collect();
    assert_eq!(counts, door.binned.expect("a histogram").counts);
    // Break it: refuse the property because its declared count is wrong.
}

#[test]
fn constant_values_have_no_histogram_and_the_rest_is_drawn() {
    let (opened, uris) = world(vec![property(RH, "Rh", vec![1], vec![5.0; FRAME_NODES], None)]);
    let result = drawn(&opened, &ask(&uris[0]));
    assert!(has_note(&result, "no-histogram"), "{:?}", result.notes);
    assert!(!has_layer(&result.list, "distribution"));
    assert!(
        texts(&result.list)
            .iter()
            .any(|text| text.contains("from 5 to 5")),
        "the moments are still written"
    );
    // Break it: draw one bar for a constant.
}

#[test]
fn no_sister_is_a_note_and_a_sister_by_hand_is_checked_by_the_door() {
    let (opened, uris) = world(vec![
        property(RH, "Rh", vec![1], spread(), None),
        // A sister of another shape: never aligned silently.
        property(
            ALONE,
            "Alone",
            vec![3],
            (0..3 * FRAME_NODES).map(|k| k as f64 + 1.0).collect(),
            None,
        ),
    ]);
    let alone = drawn(&opened, &ask(&uris[0]));
    assert!(has_note(&alone, "no-sibling"), "{:?}", alone.notes);
    assert!(
        has_layer(&alone.list, "distribution"),
        "the other blocks are drawn"
    );

    let chosen = drawn(&opened, &ask(&uris[0]).set("sibling", json!(uris[1])));
    assert!(has_note(&chosen, "relation-not-drawn"), "{:?}", chosen.notes);
    assert!(
        chosen
            .notes
            .iter()
            .any(|note| note.says.contains("never aligned silently") || note.says.contains("reads as")),
        "{:?}",
        chosen.notes
    );

    let none = drawn(&opened, &ask(&uris[0]).set("sibling", json!("none")));
    assert!(!has_note(&none, "relation-not-drawn"));
    // Break it: truncate the longer sister to the shorter and draw the density anyway.
}

#[test]
fn a_bad_setting_is_refused_by_its_name() {
    let (opened, uris) = world(vec![property(RH, "Rh", vec![1], spread(), None)]);
    let bad = refused(&opened, &ask(&uris[0]).set("component", json!({"nope": 1})));
    assert!(
        matches!(&bad, ViewRefusal::BadSetting { name, .. } if name == "component"),
        "{bad}"
    );
    let bins = refused(&opened, &ask(&uris[0]).set("bins", json!(0)));
    assert!(
        matches!(&bins, ViewRefusal::BadSetting { name, .. } if name == "bins"),
        "{bins}"
    );
    // The number of bars asked is the number drawn.
    let eight = drawn(&opened, &ask(&uris[0]).set("bins", json!(8))).list;
    assert_eq!(bars(&eight).len(), 8);
    // Break it: ignore `bins` and take the default 24.
}

#[test]
fn every_bar_and_axis_is_keyed_by_the_full_uri() {
    let values: Vec<f64> = (0..FRAME_NODES * 3).map(|k| k as f64 + 1.0).collect();
    let (opened, uris) = world(vec![property(RH, "Rh", vec![3], values, None)]);
    let list = drawn(&opened, &ask(&uris[0]).set("scale", json!("linear"))).list;
    let keys = list.keys();
    assert!(
        keys.iter().any(|key| *key == format!("{}#bin=0", uris[0])),
        "{keys:?}"
    );
    assert!(keys.iter().any(|key| *key == format!("{}#axis=0", uris[0])));
    assert!(
        keys.iter().all(|key| key.starts_with(uris[0].as_str())),
        "no bare uuid inside: {keys:?}"
    );
    // Break it: key a bar by its index alone.
}

#[test]
fn a_drawing_is_made_again_from_its_own_metadata() {
    let rh = spread();
    let rv: Vec<f64> = rh.iter().map(|value| value * 2.0).collect();
    let (opened, uris) = world(vec![
        property(RH, "Rh", vec![1], rh, Some("I")),
        property(RV, "Rv", vec![1], rv, Some("K")),
    ]);
    let cx = opened.context(&NO_MAPS);
    let first = draw_svg(&kinds(), &ask(&uris[0]).set("bins", json!(12)), &cx).expect("drawn");
    assert_eq!(
        first.ask.settings.get("scale"),
        Some(&json!("auto")),
        "the defaults are in the file"
    );
    assert_eq!(first.ask.settings.get("bins"), Some(&json!(12)));
    let again = redraw(&kinds(), &first.svg, &cx).expect("drawn again");
    assert_eq!(first.svg, again.svg);
    // Break it: leave `bins` out of the metadata, and a later default would change the figure.
}
