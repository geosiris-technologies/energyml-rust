//! `target`: the sectors, the angle convention, the interpolation, the refusals. Every package is fabricated in memory.
//!
//! The data: a frame with two nodes (MD 20 and 80) on a straight inclined trajectory (`fixtures::package`), and
//! properties of four values per node, one per sector. Four sectors evenly spread have their centres at 0, 90, 180
//! and 270 degrees, and each spans a quarter turn.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

use energyml_core::xml::XmlText;
use energyml_domain::along_md::{GapRule, Interpolations, Nature, interpolate_row};
use energyml_domain::palettes::Reading;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_sheet::draw::{DisplayList, Item, Paint};
use energyml_views::target::Target;
use energyml_views::{Drawn, ViewAsk, ViewKinds, ViewRefusal, draw_list, draw_svg, redraw, value_colours};
use serde_json::json;

use crate::fixtures::{FRAME, NO_MAPS, Opened, package, texts};

const SECTORS: &str = "44444444-4444-4444-8444-444444444441";
const OTHER: &str = "44444444-4444-4444-8444-444444444442";
const ANGLES: &str = "44444444-4444-4444-8444-444444444443";
const SCALAR: &str = "44444444-4444-4444-8444-444444444444";

fn kinds() -> ViewKinds {
    let mut kinds = ViewKinds::empty();
    kinds.register(Box::new(Target));
    kinds
}

/// A property of `per_node` values per node on the frame; `values` is node by node.
fn property(uuid: &str, title: &str, uom: &str, per_node: i64, values: Vec<f64>) -> resqml22::ContinuousProperty {
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
        value_count_per_indexable_element: vec![per_node],
        uom: eml23::UnitOfMeasureExt::from_xml_text(uom).expect("a unit"),
        supporting_representation: eml23::DataObjectReference {
            uuid: FRAME.to_string(),
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

/// The package of the fixtures with these properties in it, and their uris in order.
fn world(east: f64, properties: Vec<resqml22::ContinuousProperty>) -> (Opened, Vec<String>) {
    let mut store = package(east, &[20.0, 80.0]);
    let uris = properties
        .into_iter()
        .map(|object| store.put(Box::new(object)).expect("the package takes it"))
        .collect();
    (Opened::of(store), uris)
}

/// Four sectors, 10 to 40 at the first node and `second` at the second.
fn sectors_property(second: [f64; 4]) -> resqml22::ContinuousProperty {
    let mut values = vec![10.0, 20.0, 30.0, 40.0];
    values.extend(second);
    property(SECTORS, "Rv", "ohm.m", 4, values)
}

/// A request under one convention; sectors declared even; the colours over 0 to 100 in a named palette.
fn ask(uri: &str, md: f64, origin: &str, sense: &str, viewed: &str) -> ViewAsk {
    ViewAsk::new("target")
        .object(uri)
        .set("md", json!(md))
        .set("origin", json!(origin))
        .set("sense", json!(sense))
        .set("viewed_from", json!(viewed))
        .set("regular_sectors", json!(true))
        .set("palette", json!("Viridis"))
        .set("min", json!(0.0))
        .set("max", json!(100.0))
}

fn drawn(opened: &Opened, ask: &ViewAsk) -> Drawn {
    draw_list(&kinds(), ask, &opened.context(&NO_MAPS))
        .expect("drawn")
        .0
}

fn refused(opened: &Opened, ask: &ViewAsk) -> ViewRefusal {
    draw_list(&kinds(), ask, &opened.context(&NO_MAPS)).expect_err("refused")
}

/// What a test reads of one drawn sector.
struct Sector {
    key: String,
    centre: [f64; 2],
    start: f64,
    end: f64,
    inner: f64,
    outer: f64,
    fill: Option<[u8; 4]>,
    value: Option<f64>,
}

impl Sector {
    fn middle(&self) -> f64 {
        (self.start + self.end) / 2.0
    }
}

fn sectors(list: &DisplayList) -> Vec<Sector> {
    list.layers
        .iter()
        .filter(|layer| layer.name == "rings")
        .flat_map(|layer| layer.elements.iter())
        .filter(|element| element.kind == "sector")
        .filter_map(|element| match element.items.first()? {
            Item::Sector {
                centre,
                inner,
                outer,
                start,
                end,
                fill,
                ..
            } => Some(Sector {
                key: element.key.clone().unwrap_or_default(),
                centre: *centre,
                start: *start,
                end: *end,
                inner: *inner,
                outer: *outer,
                fill: match fill {
                    Some(Paint::Solid(colour)) => Some([colour.r, colour.g, colour.b, colour.a]),
                    _ => None,
                },
                value: element
                    .data
                    .iter()
                    .find(|(name, _)| name == "value")
                    .and_then(|(_, value)| value.parse().ok()),
            }),
            _ => None,
        })
        .collect()
}

fn same_angle(actual: f64, expected: f64, what: &str) {
    let difference = actual - expected;
    assert!(
        difference.sin().abs() < 1.0e-9 && difference.cos() > 0.0,
        "{what}: {actual} rad on the page, {expected} expected"
    );
}

#[test]
fn only_properties_with_a_value_per_sector_on_the_nodes_are_offered() {
    let (opened, uris) = world(
        50.0,
        vec![
            sectors_property([10.0; 4]),
            property(SCALAR, "a scalar", "m", 1, vec![1.0, 2.0]),
            property(OTHER, "two values", "m", 2, vec![1.0, 2.0, 3.0, 4.0]),
        ],
    );
    let offers = kinds().offers(&opened.index);
    let objects: Vec<&str> = offers.iter().map(|offer| offer.objects[0].as_str()).collect();
    let expected = opened.index.uri_of(&uris[0]).expect("in the index");
    assert_eq!(objects, vec![expected], "{objects:?}");
    assert!(
        offers[0].missing.iter().any(|need| need.contains("angle convention")),
        "the convention is never in the file: {:?}",
        offers[0].missing
    );
    // Break it: offer every property, whatever its shape.
}

#[test]
fn a_convention_nobody_declared_is_refused_by_the_placement_and_never_guessed() {
    let (opened, uris) = world(50.0, vec![sectors_property([10.0; 4])]);
    let bare = ViewAsk::new("target")
        .object(&uris[0])
        .set("md", json!(20.0))
        .set("regular_sectors", json!(true));
    let refusal = refused(&opened, &bare);
    assert!(
        matches!(&refusal, ViewRefusal::Computing { why, .. } if why.contains("no angle convention is declared")),
        "{refusal}"
    );
    // A convention given in part is not completed: the missing word is named.
    let partial = bare.clone().set("origin", json!("high_side"));
    let refusal = refused(&opened, &partial);
    assert!(
        matches!(&refusal, ViewRefusal::MissingSetting { name, .. } if name == "sense"),
        "{refusal}"
    );
    // Break it: default the origin to the high side and the sense to clockwise.
}

#[test]
fn the_sectors_turn_as_the_convention_says() {
    let (opened, uris) = world(50.0, vec![sectors_property([10.0, 20.0, 30.0, 40.0])]);
    // On the page the high side is up (-pi/2), the right is +x (0), the low side is down (pi/2), the left is pi.
    let cases = [
        // Zero at the high side, clockwise as an observer looking down the hole sees it: up, right, down, left.
        ("high_side", "clockwise", "looking_downhole", 0.0, [-FRAC_PI_2, 0.0, FRAC_PI_2, PI]),
        // The other sense is the mirror.
        ("high_side", "anticlockwise", "looking_downhole", 0.0, [-FRAC_PI_2, PI, FRAC_PI_2, 0.0]),
        // Seen from downhole, the same clockwise is mirrored again.
        ("high_side", "clockwise", "looking_uphole", 0.0, [-FRAC_PI_2, PI, FRAC_PI_2, 0.0]),
        // Zero at the right: right, down, left, up.
        ("right", "clockwise", "looking_downhole", 0.0, [0.0, FRAC_PI_2, PI, -FRAC_PI_2]),
        // An offset of a quarter turn moves where the file's zero sits.
        ("high_side", "clockwise", "looking_downhole", 90.0, [0.0, FRAC_PI_2, PI, -FRAC_PI_2]),
    ];
    for (origin, sense, viewed, offset, expected) in cases {
        let list = drawn(
            &opened,
            &ask(&uris[0], 20.0, origin, sense, viewed).set("offset", json!(offset)),
        )
        .list;
        let found = sectors(&list);
        assert_eq!(found.len(), 4, "{origin} {sense} {viewed}");
        for (k, sector) in found.iter().enumerate() {
            same_angle(sector.middle(), expected[k], &format!("{origin} {sense} {viewed} +{offset}: sector {k}"));
            assert!(
                ((sector.end - sector.start) - FRAC_PI_2).abs() < 1.0e-9,
                "four even sectors span a quarter turn each: {}",
                sector.end - sector.start
            );
        }
    }
    // Break it: place the angles with the sense swapped, and every mirrored case above fails.
}

#[test]
fn the_colour_of_a_sector_is_that_of_the_value_the_interpolation_gives_at_the_md() {
    let (opened, uris) = world(50.0, vec![sectors_property([30.0, 60.0, 70.0, 80.0])]);
    let list = drawn(&opened, &ask(&uris[0], 50.0, "high_side", "clockwise", "looking_downhole")).list;
    let found = sectors(&list);
    assert_eq!(found.len(), 4);

    // By hand: halfway between the nodes 20 and 80, `a + t (b - a)` with t = 1/2.
    let by_hand = [20.0, 40.0, 50.0, 60.0];
    // By the door of the domain: the two rows, the gap of the frame, the linear method.
    let registry = Interpolations::builtin();
    let rule = GapRule::of_axis(&[20.0, 80.0]).expect("a rule");
    let door = interpolate_row(
        &[10.0, 20.0, 30.0, 40.0],
        &[30.0, 60.0, 70.0, 80.0],
        0.5,
        60.0,
        &rule,
        registry.get("linear").expect("linear"),
        &Nature::default(),
    )
    .expect("interpolated");
    // The colours of the door of the views, over the same range.
    let cx = opened.context(&NO_MAPS);
    let colours = value_colours(&cx, "test", None, Some("Viridis"), (0.0, 100.0), false, Reading::Continuous)
        .expect("colours");

    for (k, sector) in found.iter().enumerate() {
        assert!((door.values[k] - by_hand[k]).abs() < 1.0e-9, "the door and the hand agree");
        let value = sector.value.expect("a value");
        assert!((value - door.values[k]).abs() < 1.0e-9, "sector {k} shows {value}");
        assert_eq!(sector.fill, colours.sample(door.values[k]), "sector {k}");
    }
    let fills: Vec<[u8; 4]> = found.iter().map(|sector| sector.fill.expect("a colour")).collect();
    for (a, fill) in fills.iter().enumerate() {
        assert!(!fills[a + 1..].contains(fill), "four values, four colours");
    }
    // Break it: take the row of the nearest node instead of interpolating, and the values are 10 20 30 40 or 30 60 70 80.
}

#[test]
fn a_log_scale_interpolates_by_the_geometric_mean_and_the_method_asked_wins() {
    let (opened, uris) = world(
        50.0,
        vec![property(
            SECTORS,
            "Rv",
            "ohm.m",
            4,
            vec![10.0, 10.0, 10.0, 10.0, 1000.0, 1000.0, 1000.0, 1000.0],
        )],
    );
    let base = ask(&uris[0], 50.0, "high_side", "clockwise", "looking_downhole")
        .set("scale", json!("log"))
        .set("min", json!(1.0))
        .set("max", json!(1000.0));
    let log = drawn(&opened, &base).list;
    for sector in sectors(&log) {
        let value = sector.value.expect("a value");
        assert!((value - 100.0).abs() < 1.0e-6, "log-linear halfway between 10 and 1000: {value}");
    }
    // The default of a log scale is log-linear; a method named is the caller's choice.
    let linear = drawn(&opened, &base.set("interpolation", json!("linear"))).list;
    for sector in sectors(&linear) {
        let value = sector.value.expect("a value");
        assert!((value - 505.0).abs() < 1.0e-6, "linear halfway: {value}");
    }
    // Break it: ignore `scale` when choosing the default method.
}

#[test]
fn an_md_on_a_node_shows_that_node_and_says_so() {
    let (opened, uris) = world(50.0, vec![sectors_property([30.0, 60.0, 70.0, 80.0])]);
    let list = drawn(&opened, &ask(&uris[0], 20.0, "high_side", "clockwise", "looking_downhole")).list;
    let values: Vec<f64> = sectors(&list).iter().map(|s| s.value.expect("a value")).collect();
    assert_eq!(values, vec![10.0, 20.0, 30.0, 40.0]);
    assert!(
        texts(&list).iter().any(|text| text.contains("node 0") && text.contains("no interpolation")),
        "{:?}",
        texts(&list)
    );
    let between = drawn(&opened, &ask(&uris[0], 50.0, "high_side", "clockwise", "looking_downhole")).list;
    assert!(
        texts(&between).iter().any(|text| text.contains("interpolated") && text.contains("t = 0.5")),
        "{:?}",
        texts(&between)
    );
    // Break it: say `node` for every MD.
}

#[test]
fn an_md_outside_the_nodes_is_refused_by_name() {
    let (opened, uris) = world(50.0, vec![sectors_property([30.0, 60.0, 70.0, 80.0])]);
    for md in [5.0, 500.0] {
        let refusal = refused(&opened, &ask(&uris[0], md, "high_side", "clockwise", "looking_downhole"));
        assert!(
            matches!(&refusal, ViewRefusal::Computing { why, .. } if why.contains("outside the support")),
            "{md}: {refusal}"
        );
    }
    // Break it: clamp the MD to the nearest node.
}

#[test]
fn the_angles_come_from_a_paired_property_and_none_is_invented() {
    let angles = |values: Vec<f64>| property(ANGLES, "angles", "dega", 4, values);
    let (opened, uris) = world(
        50.0,
        vec![
            sectors_property([10.0, 20.0, 30.0, 40.0]),
            angles([45.0, 135.0, 225.0, 315.0].repeat(2)),
        ],
    );
    // With the angles of the file: the first sector is centred at 45 degrees clockwise from the high side.
    let given = ask(&uris[0], 20.0, "high_side", "clockwise", "looking_downhole")
        .set("regular_sectors", json!(false))
        .set("angles", json!(uris[1]));
    let found = sectors(&drawn(&opened, &given).list);
    for (k, expected) in [-FRAC_PI_4, FRAC_PI_4, 3.0 * FRAC_PI_4, 5.0 * FRAC_PI_4].into_iter().enumerate() {
        same_angle(found[k].middle(), expected, &format!("sector {k}"));
    }
    // Without them, and without the declaration that the sectors are even: refused, the missing setting named.
    let none = ask(&uris[0], 20.0, "high_side", "clockwise", "looking_downhole").set("regular_sectors", json!(false));
    let refusal = refused(&opened, &none);
    assert!(
        matches!(&refusal, ViewRefusal::MissingSetting { name, .. } if name == "angles"),
        "{refusal}"
    );
    // Break it: fall back on even sectors when no angle is found.
}

#[test]
fn grid_north_points_left_for_a_well_going_east_and_a_north_origin_follows_it() {
    // The well goes east and down; looking down it with the high side up, the right hand is south.
    let (opened, uris) = world(50.0, vec![sectors_property([10.0, 20.0, 30.0, 40.0])]);
    let list = drawn(&opened, &ask(&uris[0], 20.0, "high_side", "clockwise", "looking_downhole")).list;
    let centre = sectors(&list)[0].centre;
    let arrow = list
        .layers
        .iter()
        .filter(|layer| layer.name == "marks")
        .flat_map(|layer| layer.elements.iter())
        .find(|element| element.kind == "north")
        .and_then(|element| match element.items.first() {
            Some(Item::Polyline { points, .. }) => Some(points.clone()),
            _ => None,
        })
        .expect("a north arrow");
    assert!(arrow[1][0] < arrow[0][0] && arrow[0][0] < centre[0], "north is to the left: {arrow:?}");
    assert!((arrow[1][1] - arrow[0][1]).abs() < 1.0e-6, "and level with the centre: {arrow:?}");
    // Angles counted from north: sector 0 points where the arrow does, to the left.
    let from_north = drawn(&opened, &ask(&uris[0], 20.0, "north", "clockwise", "looking_downhole")).list;
    same_angle(sectors(&from_north)[0].middle(), PI, "the first sector, from north");
    // Break it: take the right of the frame as the east, and the arrow points right.
}

#[test]
fn a_vertical_hole_has_no_high_side_and_says_so_but_no_north_means_no_north_origin() {
    let (opened, uris) = world(0.0, vec![sectors_property([10.0, 20.0, 30.0, 40.0])]);
    let list_and_notes = drawn(&opened, &ask(&uris[0], 20.0, "high_side", "clockwise", "looking_downhole"));
    assert_eq!(sectors(&list_and_notes.list).len(), 4, "the sectors are still drawn");
    assert!(
        list_and_notes.notes.iter().any(|note| note.key == "hole-frame-unavailable"),
        "{:?}",
        list_and_notes.notes
    );
    assert!(
        list_and_notes
            .list
            .layers
            .iter()
            .flat_map(|layer| layer.elements.iter())
            .all(|element| element.kind != "high-side" && element.kind != "north"),
        "no mark of a high side that does not exist"
    );
    let refusal = refused(&opened, &ask(&uris[0], 20.0, "north", "clockwise", "looking_downhole"));
    assert!(
        matches!(&refusal, ViewRefusal::Computing { why, .. } if why.contains("north has no direction")),
        "{refusal}"
    );
    // Break it: mark the high side at the top whatever the hole.
}

#[test]
fn several_properties_make_concentric_rings_keyed_by_their_uri_and_their_md() {
    let (opened, uris) = world(
        50.0,
        vec![
            sectors_property([30.0, 60.0, 70.0, 80.0]),
            property(OTHER, "Rh", "ohm.m", 4, vec![5.0, 15.0, 25.0, 35.0, 45.0, 55.0, 65.0, 75.0]),
        ],
    );
    let request = ask(&uris[0], 50.0, "high_side", "clockwise", "looking_downhole").object(&uris[1]);
    let found = sectors(&drawn(&opened, &request).list);
    assert_eq!(found.len(), 8);
    let (first_uri, second_uri) = (
        opened.index.uri_of(&uris[0]).expect("in the index"),
        opened.index.uri_of(&uris[1]).expect("in the index"),
    );
    let (first, second) = found.split_at(4);
    assert!(first.iter().all(|s| s.key.starts_with(first_uri)), "the first property is the first ring");
    assert!(second.iter().all(|s| s.key.starts_with(second_uri)));
    assert!(
        second[0].inner >= first[0].outer,
        "the second ring is outside the first: {} against {}",
        second[0].inner,
        first[0].outer
    );
    assert_eq!(found[2].key, format!("{first_uri}#md=50#col=2"));
    // Break it: give every ring the same radii.
}

#[test]
fn a_sector_without_a_value_is_left_empty_and_said() {
    let (opened, uris) = world(50.0, vec![sectors_property([30.0, 60.0, f64::NAN, 80.0])]);
    let result = drawn(&opened, &ask(&uris[0], 50.0, "high_side", "clockwise", "looking_downhole"));
    let found = sectors(&result.list);
    assert_eq!(found.len(), 4);
    assert!(found[2].fill.is_none() && found[2].value.is_none(), "an empty sector");
    assert!(found.iter().enumerate().all(|(k, s)| k == 2 || s.fill.is_some()));
    assert!(
        result.notes.iter().any(|note| note.key == "cells-without-value"),
        "{:?}",
        result.notes
    );
    // Break it: fill the hole with the value of the other node.
}

#[test]
fn a_drawing_is_made_again_from_its_own_metadata() {
    let (opened, uris) = world(50.0, vec![sectors_property([30.0, 60.0, 70.0, 80.0])]);
    let cx = opened.context(&NO_MAPS);
    let first = draw_svg(
        &kinds(),
        &ask(&uris[0], 50.0, "high_side", "clockwise", "looking_downhole"),
        &cx,
    )
    .expect("drawn");
    assert_eq!(first.ask.settings.get("origin"), Some(&json!("high_side")), "the convention is in the file");
    let again = redraw(&kinds(), &first.svg, &cx).expect("drawn again");
    assert_eq!(first.svg, again.svg);
    // Break it: leave the convention out of the metadata.
}
