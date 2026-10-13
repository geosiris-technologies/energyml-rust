//! `energyml_sheet::read::board_of_frame` against a frame built after the
//! specification, not read from a file - the same pattern
//! `crates/energyml-domain/tests/well_strata.rs` and
//! `crates/energyml-geometry/tests/well.rs` use, and for the same reason
//! (tests follow the specification, not a corpus): the corpus holds no
//! `WellboreFrameRepresentation` with a categorical interval property *and*
//! a per-node one on the same frame, and a value measured in a package would
//! be a value this suite then defends.
//!
//! Every array is inline XML (`*XmlArray`), never external: reading one
//! needs no HDF5 backend, only [`Prefetched`], the in-memory store
//! `energyml_domain::well_strata`'s own tests already use.
//!
//! # What it would take to make each test fail
//!
//! Said in each test's own comment, per the project rule: a test that cannot
//! fail proves nothing.

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::source::Prefetched;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_sheet::layout::{HeightGoal, Params, Track};
use energyml_sheet::read::{self, Options, TrackSelection};
use energyml_storage::ArrayLocation;
use energyml_storage::ObjectSummary;

const FRAME: &str = "f0000000-0000-4000-8000-000000000001";
const TRAJECTORY: &str = "f0000000-0000-4000-8000-000000000002";
const TABLE: &str = "c0000000-0000-4000-8000-000000000001";
const STRATA_PROPERTY: &str = "c0000000-0000-4000-8000-000000000002";
const PRESSURE_KIND: &str = "c0000000-0000-4000-8000-000000000003";
const PRESSURE_PROPERTY: &str = "c0000000-0000-4000-8000-000000000004";
const NODE_PROPERTY: &str = "c0000000-0000-4000-8000-000000000005";

// -- building blocks, after the same pattern as the domain's own tests ------

fn nowhere(_: &dyn Reflect) -> Vec<ArrayLocation> {
    Vec::new()
}

fn boxed<T: DynObject + 'static>(object: T) -> Box<dyn DynObject> {
    Box::new(object)
}

fn held(objects: Vec<Box<dyn DynObject>>) -> Prefetched {
    let mut cache = Prefetched::new();
    for object in objects {
        cache.insert(object, &nowhere);
    }
    cache
}

fn dor(uuid: &str, qualified_type: &str, title: &str) -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: uuid.to_string(),
        qualified_type: qualified_type.to_string(),
        title: title.to_string(),
        ..Default::default()
    }
}

fn citation(title: &str) -> eml23::Citation {
    eml23::Citation {
        title: title.to_string(),
        ..Default::default()
    }
}

fn integers(values: &[i64]) -> eml23::IntegerXmlArray {
    eml23::IntegerXmlArray {
        count_per_value: 1,
        null_value: -1,
        values: values.to_vec(),
        ..Default::default()
    }
}

/// A frame at MD 0, 10, 20, 30 - three intervals, four nodes.
fn frame() -> resqml22::WellboreFrameRepresentation {
    resqml22::WellboreFrameRepresentation {
        uuid: FRAME.to_string(),
        citation: citation("F1 - frame"),
        node_count: 4,
        node_md: Some(eml23::AbstractFloatingPointArray::FloatingPointXmlArray(
            eml23::FloatingPointXmlArray {
                count_per_value: 1,
                values: vec![0.0, 10.0, 20.0, 30.0],
                ..Default::default()
            },
        )),
        trajectory: dor(TRAJECTORY, "resqml22.WellboreTrajectoryRepresentation", "F1"),
        ..Default::default()
    }
}

/// A category table keyed `10, 20, 35` - Lutetian, Bartonian, Priabonian -
/// the same shape `energyml-domain/tests/well_strata.rs`'s own
/// `category_package` builds.
fn table() -> eml23::ColumnBasedTable {
    eml23::ColumnBasedTable {
        uuid: TABLE.to_string(),
        citation: citation("STRA"),
        column: vec![eml23::Column {
            title: Some("Nom_STRA".to_string()),
            value_count_per_row: 1,
            values: Some(eml23::AbstractValueArray::StringXmlArray(eml23::StringXmlArray {
                count_per_value: 1,
                values: vec!["Lutetian".into(), "Bartonian".into(), "Priabonian".into()],
                ..Default::default()
            })),
            ..Default::default()
        }],
        key_column: vec![eml23::Column {
            title: Some("STRA".to_string()),
            value_count_per_row: 1,
            values: Some(eml23::AbstractValueArray::IntegerXmlArray(integers(&[
                10, 20, 35,
            ]))),
            ..Default::default()
        }],
        ..Default::default()
    }
}

/// The categorical property: values `[20, 10, 35]`, one per interval - read
/// through the table above by **key**, not by position (the table's own
/// keys are not `0..n`, so a position read would find the wrong rows, or
/// none).
fn strata_property() -> resqml22::DiscreteProperty {
    resqml22::DiscreteProperty {
        uuid: STRATA_PROPERTY.to_string(),
        // The header a caller with no `PropertyKind` reference falls back
        // to: this property's own `Citation.Title`, "STRA" - kept
        // short on purpose, since this test looks the track up by header.
        citation: citation("STRA"),
        indexable_element: eml23::IndexableElement::Intervals,
        value_count_per_indexable_element: vec![1],
        supporting_representation: dor(FRAME, "resqml22.WellboreFrameRepresentation", "F1 - frame"),
        values_for_patch: vec![resqml22::AbstractValueArray::IntegerXmlArray(integers(&[
            20, 10, 35,
        ]))],
        category_lookup: Some(dor(TABLE, "eml23.ColumnBasedTable", "STRA")),
        ..Default::default()
    }
}

/// A `PropertyKind` object titled "Pressure" - what [`PropertyLabel::Kind`]
/// (the default) must resolve to, rather than the property's own
/// title, which repeats the well.
fn pressure_kind() -> eml23::PropertyKind {
    eml23::PropertyKind {
        uuid: PRESSURE_KIND.to_string(),
        schema_version: "2.3".to_string(),
        citation: citation("Pressure"),
        ..Default::default()
    }
}

/// A discrete property with no `CategoryLookup`: one value per interval,
/// drawn as text directly.
fn pressure_property() -> resqml22::DiscreteProperty {
    resqml22::DiscreteProperty {
        uuid: PRESSURE_PROPERTY.to_string(),
        citation: citation("F1 - pressure, yearly - 2022"),
        indexable_element: eml23::IndexableElement::Intervals,
        value_count_per_indexable_element: vec![1],
        property_kind: dor(PRESSURE_KIND, "eml23.PropertyKind", "a stale cached title"),
        supporting_representation: dor(FRAME, "resqml22.WellboreFrameRepresentation", "F1 - frame"),
        values_for_patch: vec![resqml22::AbstractValueArray::FloatingPointXmlArray(
            eml23::FloatingPointXmlArray {
                count_per_value: 1,
                values: vec![10.0, 20.0, 30.0],
                ..Default::default()
            },
        )],
        ..Default::default()
    }
}

/// A property that hangs on the frame's **nodes**, not its intervals - one
/// value per node (four), which must be counted and must create no track.
fn node_property() -> resqml22::DiscreteProperty {
    resqml22::DiscreteProperty {
        uuid: NODE_PROPERTY.to_string(),
        citation: citation("F1 - a per-node log"),
        indexable_element: eml23::IndexableElement::Nodes,
        value_count_per_indexable_element: vec![1],
        supporting_representation: dor(FRAME, "resqml22.WellboreFrameRepresentation", "F1 - frame"),
        values_for_patch: vec![resqml22::AbstractValueArray::FloatingPointXmlArray(
            eml23::FloatingPointXmlArray {
                count_per_value: 1,
                values: vec![1.0, 2.0, 3.0, 4.0],
                ..Default::default()
            },
        )],
        ..Default::default()
    }
}

fn summary(uuid: &str, qualified_type: &str) -> ObjectSummary {
    ObjectSummary {
        uri: uuid.to_string(),
        uuid: uuid.to_string(),
        qualified_type: qualified_type.to_string(),
        content_type: String::new(),
        locator: String::new(),
    }
}

fn package() -> (
    resqml22::WellboreFrameRepresentation,
    Prefetched,
    Vec<ObjectSummary>,
) {
    let source = held(vec![
        boxed(table()),
        boxed(strata_property()),
        boxed(pressure_kind()),
        boxed(pressure_property()),
        boxed(node_property()),
    ]);
    let resources = vec![
        summary(STRATA_PROPERTY, "resqml22.DiscreteProperty"),
        summary(PRESSURE_PROPERTY, "resqml22.DiscreteProperty"),
        summary(NODE_PROPERTY, "resqml22.DiscreteProperty"),
    ];
    (frame(), source, resources)
}

fn track_labelled(board: &energyml_sheet::layout::Board, header: &str) -> Vec<Option<String>> {
    for track in &board.tracks {
        if let Track::Intervals(track) = track
            && track.header == header
        {
            return track
                .intervals
                .iter()
                .map(|interval| interval.text.clone())
                .collect();
        }
    }
    panic!(
        "no track headed `{header}` among {:?}",
        board.tracks.iter().map(track_header).collect::<Vec<_>>()
    );
}

fn track_header(track: &Track) -> &str {
    match track {
        Track::Md(md) => &md.header,
        Track::Intervals(track) => &track.header,
    }
}

/// A CBT's row label reaches the sheet, by key - not by position, and not
/// the row's own key echoed back.
///
/// What would break this: `well_strata::of_frame`'s `Wanted::Category` path
/// stops being called from `read::build_property_track` - the labels would
/// then be `None` (no track at all, since `read.lookup` would go unread) or
/// a wrong row (a position-based lookup would misname every row, since the
/// table's keys 10/20/35 are not `0..n`).
#[test]
fn a_category_property_is_labelled_through_its_lookup_table() {
    let (frame, source, resources) = package();
    let options = Options::new("MD (m)");
    let read = read::board_of_frame(&frame, &resources, &source, &options).expect("the frame reads");

    let labels = track_labelled(&read.board, "STRA");
    assert_eq!(
        labels,
        vec![
            Some("Bartonian".to_string()),
            Some("Lutetian".to_string()),
            Some("Priabonian".to_string())
        ]
    );
}

/// A property column's header is its `PropertyKind`'s title by default -
/// not the property's own, which repeats the well and the
/// context.
///
/// What would break this: `read::column_label`'s default branch reading
/// `read.title` first, or the [`energyml_domain::property_kind::Resolver`]
/// call being skipped - either would show "F1 - pressure, yearly - 2022"
/// instead of "Pressure".
#[test]
fn a_propertys_column_is_headed_by_its_property_kind_title_by_default() {
    let (frame, source, resources) = package();
    let options = Options::new("MD (m)");
    let read = read::board_of_frame(&frame, &resources, &source, &options).expect("the frame reads");

    let values = track_labelled(&read.board, "Pressure");
    assert_eq!(
        values,
        vec![
            Some("10".to_string()),
            Some("20".to_string()),
            Some("30".to_string())
        ]
    );
}

/// A per-node property creates no section - it is counted in the board's
/// notes instead of silently vanishing or crashing the count check that a
/// four-value array would otherwise fail against three intervals.
///
/// What would break this: the classification loop in `board_of_frame`
/// stopping to special-case `IndexableElement::Nodes` (it would then land in
/// `build_property_track`, which would refuse it by its counts - still no
/// track, but the wrong reason, and no note naming it as a *node* property).
#[test]
fn a_per_node_property_creates_no_track_and_is_counted() {
    let (frame, source, resources) = package();
    let options = Options::new("MD (m)");
    let read = read::board_of_frame(&frame, &resources, &source, &options).expect("the frame reads");

    assert!(
        read.board
            .tracks
            .iter()
            .all(|track| track_header(track) != "F1 - a per-node log"),
        "a per-node property must not become a track: {:?}",
        read.board.tracks.iter().map(track_header).collect::<Vec<_>>()
    );
    assert!(
        read.notes
            .iter()
            .any(|note| note.contains('1') && note.contains("node")),
        "the one node property must be counted in the notes: {:?}",
        read.notes
    );
    // Exactly three tracks: MD, STRA, Pressure - the node property adds none.
    assert_eq!(
        read.board.tracks.len(),
        3,
        "{:?}",
        read.board.tracks.iter().map(track_header).collect::<Vec<_>>()
    );
}

/// The laid-out sections follow the frame's own `[md_top, md_base]` bounds -
/// the union the interval tracks state, not a guess and not the whole
/// `NodeMd` array read as sections one MD apart.
///
/// What would break this: `layout::compute`'s bounds loop reading something
/// other than the interval tracks `read::board_of_frame` built, or this
/// module aligning a property against the wrong window (an off-by-one would
/// shift every boundary by one node).
#[test]
fn sections_follow_the_intervals_bounds() {
    let (frame, source, resources) = package();
    let options = Options::new("MD (m)");
    let read = read::board_of_frame(&frame, &resources, &source, &options).expect("the frame reads");

    let params = Params {
        proportion: "to-scale".to_string(),
        height: HeightGoal::Total(300.0),
        page_width_mm: 200.0,
        text_size_pt: 8.0,
    };
    let layout = energyml_sheet::layout::compute(&read.board, &params).expect("a valid board");

    let bounds: Vec<(f64, f64)> = layout
        .sections
        .iter()
        .map(|section| (section.md_top, section.md_base))
        .collect();
    assert_eq!(bounds, vec![(0.0, 10.0), (10.0, 20.0), (20.0, 30.0)]);
}

/// The MD track carries the caller's own header, never one invented here
/// ("no hardcoded French sentence", and no English one either - a
/// caller states it every time).
#[test]
fn the_md_track_carries_the_callers_header() {
    let (frame, source, resources) = package();
    let options = Options::new("Depth (m)");
    let read = read::board_of_frame(&frame, &resources, &source, &options).expect("the frame reads");
    assert!(matches!(&read.board.tracks[0], Track::Md(md) if md.header == "Depth (m)"));
}

/// `Options::with_tracks` orders and filters the property tracks - the MD
/// column and any stratigraphic-rank track are unaffected, only the
/// property tracks are selected and reordered.
///
/// What would break this: `TrackSelection::Only` being applied before
/// classification (it would then also hide the node property's *count*,
/// which must stay independent of which tracks are drawn).
#[test]
fn track_selection_orders_and_filters_the_property_tracks() {
    let (frame, source, resources) = package();
    let options =
        Options::new("MD (m)").with_tracks(TrackSelection::Only(vec![PRESSURE_PROPERTY.to_string()]));
    let read = read::board_of_frame(&frame, &resources, &source, &options).expect("the frame reads");

    let headers: Vec<&str> = read.board.tracks.iter().map(track_header).collect();
    assert_eq!(
        headers,
        vec!["MD (m)", "Pressure"],
        "only the requested property track is drawn: {headers:?}"
    );
}

/// **Two columns never share a header.** Two properties of one kind would
/// both be headed `Pressure`, and nobody could tell them apart - Lutetia's
/// `BSS000NSYT` showed two `source`, two `name`, two `identifier`.
/// Each then takes its own title, without the well it repeats, and a note
/// says so.
///
/// To break it: drop `disambiguate` from `board_of_frame` - both headers go back
/// to `Pressure`.
#[test]
fn two_properties_of_one_kind_are_headed_by_their_own_titles() {
    let (frame, _, mut resources) = package();
    let mut second = pressure_property();
    second.uuid = "c0000000-0000-4000-8000-000000000006".to_string();
    second.citation = citation("F1 - pressure, yearly - 2023");
    let mut objects: Vec<Box<dyn DynObject>> = vec![
        boxed(table()),
        boxed(strata_property()),
        boxed(pressure_kind()),
        boxed(pressure_property()),
        boxed(node_property()),
    ];
    objects.push(boxed(second));
    let source = held(objects);
    resources.push(summary(
        "c0000000-0000-4000-8000-000000000006",
        "resqml22.DiscreteProperty",
    ));

    let read =
        read::board_of_frame(&frame, &resources, &source, &Options::new("MD (m)")).expect("the frame reads");
    let headers: Vec<&str> = read.board.tracks.iter().map(track_header).collect();
    assert!(
        !headers.contains(&"Pressure"),
        "a shared kind is no header: {headers:?}"
    );
    assert!(
        headers.contains(&"pressure, yearly - 2022") && headers.contains(&"pressure, yearly - 2023"),
        "{headers:?}"
    );
    assert!(
        read.notes.iter().any(|note| note.contains("Pressure")),
        "the change of header is said: {:?}",
        read.notes
    );
}

/// What a property title keeps once the frame's words are gone - cut on a
/// word, never inside one, and never down to nothing.
#[test]
fn a_title_loses_only_the_whole_words_it_shares_with_its_frame() {
    use read::beyond_shared;
    assert_eq!(
        beyond_shared("BSS000NSYT - source_etage", "BSS000NSYT - frame"),
        "source_etage"
    );
    // `f` is shared with `frame`, but half a word is not a word.
    assert_eq!(
        beyond_shared("BSS000NSYT - formation", "BSS000NSYT - frame"),
        "formation"
    );
    // The frame's title a prefix of the property's.
    assert_eq!(beyond_shared("GTH-1 porosity", "GTH-1"), "porosity");
    // Nothing shared, or nothing left: the title, whole.
    assert_eq!(beyond_shared("porosity", "GTH-1 - frame"), "porosity");
    assert_eq!(beyond_shared("GTH-1 - frame", "GTH-1 - frame"), "GTH-1 - frame");
    assert_eq!(beyond_shared("ABCD porosity", "ABC"), "ABCD porosity");
}

/// A property hangs on the frame its **reference** names - uuid and version.
/// A package holding the frame twice has properties on each; the sheet of
/// version 1 must not draw the curve of a property that names version 2.
///
/// Identity rule: internally only the full URI is used (ADR-0003).
///
/// What would break this: `read::board_of_frame` deciding `matches_frame` from
/// the uuid of the supporting representation alone - the property naming
/// version 2 then lands on the sheet of version 1.
#[test]
fn a_property_on_another_version_of_the_frame_is_not_on_this_sheet() {
    let headers = |board: &energyml_sheet::layout::Board| {
        board
            .tracks
            .iter()
            .map(|track| track_header(track).to_string())
            .collect::<Vec<_>>()
    };
    let mut frame = frame();
    frame.object_version = Some("1".to_string());
    let options = Options::new("MD (m)");

    let named = |version: &str| {
        let mut property = pressure_property();
        property.supporting_representation.object_version = Some(version.to_string());
        let source = held(vec![boxed(pressure_kind()), boxed(property)]);
        let resources = vec![summary(PRESSURE_PROPERTY, "resqml22.DiscreteProperty")];
        read::board_of_frame(&frame, &resources, &source, &options).expect("the frame reads")
    };

    let on_one = named("1");
    assert!(
        headers(&on_one.board).contains(&"Pressure".to_string()),
        "{:?}",
        headers(&on_one.board)
    );
    let on_two = named("2");
    assert!(
        !headers(&on_two.board).contains(&"Pressure".to_string()),
        "{:?}",
        headers(&on_two.board)
    );
}
