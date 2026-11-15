//! `md-coverage`: the bars against the extents the door of extents gives, the bands of references that cannot be
//! compared, the alignments the person declares, the hatching, the cross, what is apart, the refusals. Packages
//! fabricated in memory. Sources: common TRG 2.3 section 6.48 p. 134 (MDs with one datum and one unit are comparable;
//! another datum is never moved onto), RESQML TRG 2.2 section 12.13 p. 310 (a frame's nodes carry MDs counted as its
//! trajectory's).

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use std::sync::Arc;

use energyml_core::registry::DynObject;
use energyml_domain::md_extent::{self, Bar, Coverage, CoverageGroup, ExtentAxis, ExtentBasis, ExtentReaders};
use energyml_domain::{ObjectSource, units};
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_sheet::draw::{DisplayList, Element, Item, Paint, Rect};
use energyml_storage::EpcStore;
use energyml_views::md_coverage::{CoverageSpec, MdCoverage, coverage_list};
use energyml_views::{Drawn, ViewAsk, ViewKinds, ViewRefusal, draw_list, draw_svg, redraw};
use serde_json::json;

use crate::fixtures::{NO_MAPS, Opened, texts, uri};

const WELL: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const INTERPRETATION: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const DATUM: &str = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
const EMPTY_WELL: &str = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee";
const T1: &str = "c1111111-1111-4111-8111-111111111111";
const T2: &str = "c2222222-2222-4222-8222-222222222222";
const T3: &str = "c3333333-3333-4333-8333-333333333333";
const T4: &str = "c4444444-4444-4444-8444-444444444444";
const F1: &str = "f1111111-1111-4111-8111-111111111111";

fn trajectory_uri(uuid: &str) -> String {
    uri("resqml22.WellboreTrajectoryRepresentation", uuid)
}

fn frame_uri(uuid: &str) -> String {
    uri("resqml22.WellboreFrameRepresentation", uuid)
}

fn well_uri() -> String {
    uri("resqml22.WellboreFeature", WELL)
}

fn reference(qualified_type: &str, uuid: &str) -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: uuid.to_string(),
        qualified_type: qualified_type.to_string(),
        ..Default::default()
    }
}

fn citation(title: &str) -> eml23::Citation {
    eml23::Citation {
        title: title.to_string(),
        ..Default::default()
    }
}

/// A trajectory that states only its MD interval (no geometry: its extent is the interval it declares).
fn trajectory(
    uuid: &str,
    title: &str,
    (min, max): (f64, f64),
    uom: eml23::LengthUomExt,
    datum: Option<&str>,
) -> resqml22::WellboreTrajectoryRepresentation {
    resqml22::WellboreTrajectoryRepresentation {
        uuid: uuid.to_string(),
        schema_version: "2.2".into(),
        citation: citation(title),
        md_interval: eml23::MdInterval {
            md_min: min,
            md_max: max,
            uom,
            datum: datum.map(|datum| reference("resqml22.MdDatum", datum)),
            ..Default::default()
        },
        represented_object: Some(reference("resqml22.WellboreInterpretation", INTERPRETATION)),
        ..Default::default()
    }
}

fn frame(uuid: &str, title: &str, on: &str, nodes: &[f64]) -> resqml22::WellboreFrameRepresentation {
    resqml22::WellboreFrameRepresentation {
        uuid: uuid.to_string(),
        schema_version: "2.2".into(),
        citation: citation(title),
        node_count: nodes.len() as i64,
        node_md: Some(eml23::AbstractFloatingPointArray::FloatingPointXmlArray(
            eml23::FloatingPointXmlArray {
                count_per_value: 1,
                values: nodes.to_vec(),
                ..Default::default()
            },
        )),
        trajectory: reference("resqml22.WellboreTrajectoryRepresentation", on),
        represented_object: Some(reference("resqml22.WellboreInterpretation", INTERPRETATION)),
        ..Default::default()
    }
}

fn put(store: &mut EpcStore, object: impl DynObject + 'static) {
    store.put(Box::new(object)).expect("it goes in");
}

/// One well with:
/// - T1: 0 to 100 m, datum D;
/// - F1: a frame on T1, nodes 20 and 80 (counted as T1's);
/// - T3: 300 to 330 ft, datum D (the same counting as T1 up to the unit);
/// - T4: 90 down to 60 m, datum D (written the wrong way round);
/// - T2: 150 to 200 m, **no datum** (cannot be compared with T1);
///
/// and a second well that has no representation at all.
fn package() -> Opened {
    let mut store = EpcStore::from_reader(
        Box::new(Cursor::new(
            energyml_storage::extract::empty_package(None).expect("an empty package"),
        )),
        "views.epc",
        Arc::new(energyml_schemas::all()),
    )
    .expect("it opens");
    for (uuid, title) in [(WELL, "A well"), (EMPTY_WELL, "A well with nothing")] {
        put(
            &mut store,
            resqml22::WellboreFeature {
                uuid: uuid.to_string(),
                schema_version: "2.2".into(),
                citation: citation(title),
                ..Default::default()
            },
        );
    }
    put(
        &mut store,
        resqml22::WellboreInterpretation {
            uuid: INTERPRETATION.to_string(),
            schema_version: "2.2".into(),
            citation: citation("An interpretation"),
            interpreted_feature: reference("resqml22.WellboreFeature", WELL),
            ..Default::default()
        },
    );
    put(
        &mut store,
        trajectory(T1, "T1", (0.0, 100.0), eml23::LengthUomExt::M, Some(DATUM)),
    );
    put(&mut store, frame(F1, "F1", T1, &[20.0, 80.0]));
    put(
        &mut store,
        trajectory(T3, "T3", (300.0, 330.0), eml23::LengthUomExt::Ft, Some(DATUM)),
    );
    put(
        &mut store,
        trajectory(T4, "T4", (90.0, 60.0), eml23::LengthUomExt::M, Some(DATUM)),
    );
    put(
        &mut store,
        trajectory(T2, "T2", (150.0, 200.0), eml23::LengthUomExt::M, None),
    );
    Opened::of(store)
}

fn kinds() -> ViewKinds {
    let mut kinds = ViewKinds::empty();
    kinds.register(Box::new(MdCoverage));
    kinds
}

fn drawn(opened: &Opened, ask: ViewAsk) -> Result<Drawn, ViewRefusal> {
    draw_list(&kinds(), &ask, &opened.context(&NO_MAPS)).map(|(drawn, _)| drawn)
}

fn well_ask() -> ViewAsk {
    ViewAsk::new("md-coverage").object(well_uri())
}

/// Every bar of the picture: the band it is in, its element, its rectangle.
fn bars(list: &DisplayList) -> Vec<(usize, &Element, Rect)> {
    let mut out = Vec::new();
    for layer in &list.layers {
        let Some(band) = layer.name.strip_prefix("bars-") else {
            continue;
        };
        let band: usize = band.parse().expect("a band number");
        for element in &layer.elements {
            let rect = element.items.iter().find_map(|item| match item {
                Item::Rect { rect, .. } => Some(*rect),
                _ => None,
            });
            out.push((band, element, rect.expect("a bar is a rectangle")));
        }
    }
    out
}

fn bar<'a>(list: &'a DisplayList, key: &str) -> (usize, &'a Element, Rect) {
    bars(list)
        .into_iter()
        .find(|(_, element, _)| element.key.as_deref() == Some(key))
        .unwrap_or_else(|| panic!("no bar keyed {key}"))
}

fn note_keys(drawn: &Drawn) -> Vec<&'static str> {
    drawn.notes.iter().map(|note| note.key).collect()
}

fn has_cross(element: &Element) -> bool {
    element
        .items
        .iter()
        .any(|item| matches!(item, Item::Symbol { .. }))
}

/// The extent the door of extents gives for an object of the package.
fn extent_of(opened: &Opened, uri: &str) -> md_extent::Extent {
    let object = opened.store.read_object_at(uri).expect("the object is there");
    md_extent::of_object(object.as_reflect(), &opened.store, &ExtentReaders::builtin()).expect("an extent")
}

/// The page y of an MD in a band, from two bars whose extents are known: the scale of a band is affine.
struct Scale {
    top_md: f64,
    top_y: f64,
    per_md: f64,
}

impl Scale {
    /// The scale read on a bar: its page rectangle against the extent `of_object` gave, in the band's unit.
    fn of(rect: Rect, md: [f64; 2]) -> Scale {
        let (low, high) = (md[0].min(md[1]), md[0].max(md[1]));
        Scale {
            top_md: low,
            top_y: rect.y,
            per_md: rect.h / (high - low),
        }
    }

    fn at(&self, md: f64) -> f64 {
        self.top_y + (md - self.top_md) * self.per_md
    }
}

#[test]
fn a_well_with_representations_is_offered_and_an_empty_one_is_not() {
    let opened = package();
    let offers = kinds().offers(&opened.index);
    assert_eq!(offers.len(), 1, "{offers:?}");
    assert_eq!(offers[0].view, "md-coverage");
    assert_eq!(offers[0].objects, vec![well_uri()]);
    assert!(offers[0].missing.is_empty());
    // Break it: offer every well, whatever it holds.
}

#[test]
fn every_member_of_the_well_has_a_bar_keyed_by_its_uri() {
    let opened = package();
    let list = drawn(&opened, well_ask()).expect("drawn").list;
    let keys: BTreeSet<String> = bars(&list)
        .iter()
        .filter_map(|(_, element, _)| element.key.clone())
        .collect();
    let expected: BTreeSet<String> = [
        trajectory_uri(T1),
        trajectory_uri(T2),
        trajectory_uri(T3),
        trajectory_uri(T4),
        frame_uri(F1),
    ]
    .into_iter()
    .collect();
    assert_eq!(keys, expected);
    assert_eq!(bars(&list).len(), 5, "one bar each, none twice");
    // Break it: draw only the trajectories of the well.
}

#[test]
fn a_bar_sits_where_the_door_of_extents_puts_its_depths() {
    let opened = package();
    let list = drawn(&opened, well_ask()).expect("drawn").list;

    // The scale of the band is read on T1 (declared 0 to 100 m)...
    let t1 = extent_of(&opened, &trajectory_uri(T1));
    let (band, _, t1_rect) = bar(&list, &trajectory_uri(T1));
    let scale = Scale::of(t1_rect, [t1.from, t1.to]);

    // ... and every other bar of the band is where its own extent says, in metres.
    let f1 = extent_of(&opened, &frame_uri(F1));
    let (f1_band, _, f1_rect) = bar(&list, &frame_uri(F1));
    assert_eq!(f1_band, band, "a frame counts its MDs as its trajectory does");
    assert!((f1_rect.y - scale.at(f1.from)).abs() < 1.0e-6, "the top of the frame");
    assert!(
        (f1_rect.y + f1_rect.h - scale.at(f1.to)).abs() < 1.0e-6,
        "the bottom of the frame"
    );

    // T3 counts in feet from the same datum: its bar is in the band of T1, in metres.
    let t3 = extent_of(&opened, &trajectory_uri(T3));
    assert_eq!(t3.uom.as_deref(), Some("ft"));
    let (t3_band, _, t3_rect) = bar(&list, &trajectory_uri(T3));
    assert_eq!(t3_band, band);
    let metres = |value: f64| units::metres(value, "ft").expect("feet convert");
    assert!((t3_rect.y - scale.at(metres(t3.from))).abs() < 1.0e-6, "T3 top, in metres");
    assert!(
        (t3_rect.y + t3_rect.h - scale.at(metres(t3.to))).abs() < 1.0e-6,
        "T3 bottom, in metres"
    );

    // T4 is written from 90 up to 60: drawn between its two ends, whichever is first.
    let t4 = extent_of(&opened, &trajectory_uri(T4));
    assert!(t4.is_reversed());
    let (t4_band, _, t4_rect) = bar(&list, &trajectory_uri(T4));
    assert_eq!(t4_band, band);
    assert!((t4_rect.y - scale.at(t4.to)).abs() < 1.0e-6);
    assert!((t4_rect.y + t4_rect.h - scale.at(t4.from)).abs() < 1.0e-6);
    // Break it: place a bar from the MD written in the title rather than from `of_object`, or skip the feet's factor.
}

#[test]
fn a_reference_that_cannot_be_compared_has_its_own_band_and_is_said() {
    let opened = package();
    let drawn = drawn(&opened, well_ask()).expect("drawn");
    let (band_1, _, _) = bar(&drawn.list, &trajectory_uri(T1));
    let (band_2, _, _) = bar(&drawn.list, &trajectory_uri(T2));
    assert_ne!(band_1, band_2, "T2 states no datum: never merged onto T1's axis");
    let axes = drawn
        .list
        .layers
        .iter()
        .filter(|layer| layer.name.starts_with("axes-"))
        .count();
    assert_eq!(axes, 2, "one graduated axis per band");
    let keys = note_keys(&drawn);
    assert_eq!(keys.iter().filter(|key| **key == "incomparable-md-reference").count(), 1);
    // Break it: put every bar on the first band's axis.
}

#[test]
fn a_reversed_interval_is_marked_and_said_and_only_that_one() {
    let opened = package();
    let drawn = drawn(&opened, well_ask()).expect("drawn");
    for (_, element, _) in bars(&drawn.list) {
        let reversed = element.key.as_deref() == Some(trajectory_uri(T4).as_str());
        assert_eq!(has_cross(element), reversed, "{:?}", element.key);
        assert_eq!(
            element.data.iter().any(|(name, _)| name == "reversed"),
            reversed,
            "{:?}",
            element.key
        );
    }
    assert!(note_keys(&drawn).contains(&"reversed-interval"));
    // Break it: swap the ends of a reversed interval silently.
}

#[test]
fn an_alignment_the_person_declares_puts_the_bar_on_the_other_axis_and_says_so() {
    let opened = package();
    let ask = well_ask().set(
        "align",
        json!([{ "item": trajectory_uri(T2), "onto": trajectory_uri(T1) }]),
    );
    let drawn = drawn(&opened, ask).expect("drawn");
    let (band_1, _, t1_rect) = bar(&drawn.list, &trajectory_uri(T1));
    let (band_2, element, t2_rect) = bar(&drawn.list, &trajectory_uri(T2));
    assert_eq!(band_1, band_2, "aligned onto T1: counted as T1's");
    assert!(element.data.contains(&("aligned".to_string(), "true".to_string())));
    let dashed = element.items.iter().any(|item| {
        matches!(item, Item::Rect { stroke: Some(stroke), .. } if !stroke.dash.is_empty())
    });
    assert!(dashed, "an aligned bar is outlined with a dash");
    let keys = note_keys(&drawn);
    assert!(keys.contains(&"aligned-by-assumption"));
    assert!(!keys.contains(&"incomparable-md-reference"));

    // Where it lands is where its own extent says, on T1's scale.
    let t1 = extent_of(&opened, &trajectory_uri(T1));
    let t2 = extent_of(&opened, &trajectory_uri(T2));
    let scale = Scale::of(t1_rect, [t1.from, t1.to]);
    assert!((t2_rect.y - scale.at(t2.from)).abs() < 1.0e-6);
    // Break it: draw an alignment nobody declared, or place the item at its own MD without the other's reference.
}

#[test]
fn an_alignment_onto_something_not_drawn_is_said_and_ignored() {
    let opened = package();
    let ask = ViewAsk::new("md-coverage")
        .object(trajectory_uri(T1))
        .object(trajectory_uri(T2))
        .set(
            "align",
            json!([{ "item": trajectory_uri(T2), "onto": trajectory_uri(T3) }]),
        );
    let drawn = drawn(&opened, ask).expect("drawn");
    let (band_1, _, _) = bar(&drawn.list, &trajectory_uri(T1));
    let (band_2, _, _) = bar(&drawn.list, &trajectory_uri(T2));
    assert_ne!(band_1, band_2);
    assert!(note_keys(&drawn).contains(&"unknown-alignment"));
    // Break it: apply an alignment whose target is not on the picture.
}

#[test]
fn a_bad_alignment_is_refused_by_its_setting() {
    let opened = package();
    for bad in [
        json!("T2 onto T1"),
        json!([{ "item": trajectory_uri(T2) }]),
        json!([{ "item": trajectory_uri(T2), "onto": "eml:///resqml22.WellboreFrameRepresentation(99999999-9999-4999-8999-999999999999)" }]),
    ] {
        let refused = drawn(&opened, well_ask().set("align", bad.clone())).expect_err("refused");
        assert!(
            matches!(&refused, ViewRefusal::BadSetting { name, .. } if name == "align"),
            "{bad}: {refused}"
        );
    }
    // Break it: guess the alignment from the titles when `align` does not read.
}

#[test]
fn a_well_with_nothing_that_carries_a_depth_is_refused_by_name() {
    let opened = package();
    let ask = ViewAsk::new("md-coverage").object(uri("resqml22.WellboreFeature", EMPTY_WELL));
    let refused = drawn(&opened, ask).expect_err("refused");
    assert!(
        matches!(&refused, ViewRefusal::Computing { view, why } if view == "md-coverage" && why.contains("has no trajectory")),
        "{refused}"
    );
    // Break it: draw an empty page for it.
}

#[test]
fn axis_bounds_that_leave_no_interval_are_refused() {
    let opened = package();
    let refused = drawn(
        &opened,
        well_ask().set("md_from", json!(50.0)).set("md_to", json!(40.0)),
    )
    .expect_err("refused");
    assert!(matches!(refused, ViewRefusal::Computing { .. }), "{refused}");
    // Break it: swap the two bounds without saying.
}

#[test]
fn drawn_again_from_its_own_metadata_the_bytes_are_the_same() {
    let opened = package();
    let kinds = kinds();
    let cx = opened.context(&NO_MAPS);
    let first = draw_svg(&kinds, &well_ask(), &cx).expect("drawn");
    let again = redraw(&kinds, &first.svg, &cx).expect("drawn again");
    assert_eq!(first.svg, again.svg);
    // Break it: leave `align` out of the metadata, or draw from a default that changed.
}

fn band_of_unstated_references() -> Coverage {
    Coverage {
        groups: vec![CoverageGroup {
            reference: None,
            uom: Some("m".to_string()),
            bars: vec![
                Bar {
                    key: "a".to_string(),
                    from: 10.0,
                    to: 20.0,
                    basis: ExtentBasis::ThroughLink { column: 1 },
                    aligned: false,
                },
                Bar {
                    key: "b".to_string(),
                    from: 30.0,
                    to: 40.0,
                    basis: ExtentBasis::Declared,
                    aligned: false,
                },
            ],
        }],
        notes: Vec::new(),
        applied: Vec::new(),
    }
}

fn spec<'a>(
    coverage: &'a Coverage,
    titles: &'a BTreeMap<String, String>,
    off_axis: &'a [(String, ExtentAxis)],
) -> CoverageSpec<'a> {
    CoverageSpec {
        coverage,
        titles,
        off_axis,
        size: [400.0, 400.0],
        from: None,
        to: None,
        heading: "a table",
    }
}

#[test]
fn an_extent_inherited_through_a_link_is_hatched_and_the_others_are_not() {
    let coverage = band_of_unstated_references();
    let titles = BTreeMap::new();
    let list = coverage_list(&spec(&coverage, &titles, &[])).expect("laid out");
    let fill_of = |key: &str| {
        bar(&list, key)
            .1
            .items
            .iter()
            .find_map(|item| match item {
                Item::Rect { fill, .. } => *fill,
                _ => None,
            })
            .expect("a fill")
    };
    assert!(matches!(fill_of("a"), Paint::Hatch(_)));
    assert!(matches!(fill_of("b"), Paint::Solid(_)));
    let a = bar(&list, "a").1;
    assert!(a.data.contains(&("basis".to_string(), "through-link".to_string())));
    // The band that no reference states says so.
    assert!(
        texts(&list).iter().any(|text| text.contains("reference not stated")),
        "{:?}",
        texts(&list)
    );
    // Break it: hatch by `aligned` instead of by the basis.
}

#[test]
fn what_is_off_the_md_axis_is_written_apart_and_has_no_bar() {
    let coverage = band_of_unstated_references();
    let titles = BTreeMap::from([("log".to_string(), "A time log".to_string())]);
    let apart = [("log".to_string(), ExtentAxis::Time)];
    let list = coverage_list(&spec(&coverage, &titles, &apart)).expect("laid out");
    assert_eq!(bars(&list).len(), 2, "the time log has no bar on the MD axis");
    let layer = list.layer("off-axis").expect("a layer of what is apart");
    let said: Vec<&str> = layer
        .elements
        .iter()
        .flat_map(|element| element.items.iter())
        .filter_map(|item| match item {
            Item::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(said.contains(&"A time log - time"), "{said:?}");
    assert!(layer.elements.iter().any(|element| element.key.as_deref() == Some("log")));
    // Nothing apart: no such layer.
    let alone = coverage_list(&spec(&coverage, &titles, &[])).expect("laid out");
    assert!(alone.layer("off-axis").is_none());
    // Break it: draw the time log as a bar on the MD axis.
}

#[test]
fn bands_that_do_not_fit_the_page_are_refused_not_squeezed() {
    let one = band_of_unstated_references().groups.remove(0);
    let coverage = Coverage {
        groups: vec![one.clone(); 12],
        notes: Vec::new(),
        applied: Vec::new(),
    };
    let titles = BTreeMap::new();
    let refused = coverage_list(&spec(&coverage, &titles, &[])).expect_err("refused");
    assert!(refused.contains("bands do not fit"), "{refused}");
    // Break it: shrink the plot width below 16 pixels instead of refusing.
}
