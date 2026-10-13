//! `energyml_domain::series` — the readers of WITSML channel sets and PRODML
//! time series, the flow-test resolver, the decimation and the production
//! periods, each refusal by its variant.
//!
//! **The form comes from the specification, the values are the test's own.**
//! Every object here is built by hand, and each citation is in a comment where
//! a reader can check it (ADR-0011: a reference document is cited, not read at
//! run time). A refusal is asserted by its
//! **variant**, never by words in its message.
//!
//! The round trip against the writers of `energyml-build` —
//! `witsml-logs`, `injection-test`, `prodml-doublet` — is in
//! `crates/energyml-build/tests/series_readback.rs`: build depends on domain,
//! not the reverse.

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_core::xml::XmlText;
use energyml_domain::series::decimate::decimate;
use energyml_domain::series::time_series::{self, Content};
use energyml_domain::series::{
    self, Channel, Direction, Index, IndexKind, Quality, Readout, Refused, Series, channel_set, flow_test,
    instant, production,
};
use energyml_domain::source::NoObjects;
use energyml_domain::values::Values;
use energyml_eml_2_3 as eml;
use energyml_prodml_2_3 as prodml;
use energyml_witsml_2_1 as witsml;

// ---------------------------------------------------------------------------
// Building by hand
// ---------------------------------------------------------------------------

fn citation(title: &str) -> eml::Citation {
    eml::Citation {
        title: title.into(),
        originator: "test".into(),
        creation: "2026-09-28T00:00:00Z".into(),
        format: "test".into(),
        ..Default::default()
    }
}

fn dor(uuid: &str, qualified_type: &str, title: &str) -> eml::DataObjectReference {
    eml::DataObjectReference {
        uuid: uuid.into(),
        qualified_type: qualified_type.into(),
        title: title.into(),
        ..Default::default()
    }
}

fn unit(code: &str) -> eml::UnitOfMeasureExt {
    eml::UnitOfMeasureExt::from_xml_text(code).expect("a unit")
}

fn index(
    kind: eml::DataIndexKind,
    mnemonic: &str,
    uom: &str,
    direction: eml::IndexDirection,
) -> witsml::ChannelIndex {
    witsml::ChannelIndex {
        index_kind: kind,
        uom: unit(uom),
        direction,
        mnemonic: mnemonic.into(),
        ..Default::default()
    }
}

fn md() -> witsml::ChannelIndex {
    index(
        eml::DataIndexKind::MeasuredDepth,
        "MD",
        "m",
        eml::IndexDirection::Increasing,
    )
}

fn time() -> witsml::ChannelIndex {
    index(
        eml::DataIndexKind::DateTime,
        "TIME",
        "s",
        eml::IndexDirection::Increasing,
    )
}

fn channel(
    uuid: &str,
    mnemonic: &str,
    kind: witsml::ChannelDataKind,
    uom: &str,
    own: witsml::ChannelIndex,
) -> witsml::AnyChannel {
    witsml::AnyChannel::Channel(witsml::Channel {
        citation: citation(mnemonic),
        uuid: uuid.into(),
        schema_version: "2.1".into(),
        mnemonic: mnemonic.into(),
        global_mnemonic: mnemonic.into(),
        data_kind: kind,
        uom: unit(uom),
        channel_property_kind: dor(
            "30000000-0000-4000-8000-000000000001",
            "eml23.PropertyKind",
            "temperature",
        ),
        index: vec![own],
        ..Default::default()
    })
}

fn double(uuid: &str, mnemonic: &str, uom: &str) -> witsml::AnyChannel {
    channel(uuid, mnemonic, witsml::ChannelDataKind::Double, uom, md())
}

const SET: &str = "50000000-0000-4000-8000-000000000001";
const T_CHANNEL: &str = "50000000-0000-4000-8000-000000000011";
const P_CHANNEL: &str = "50000000-0000-4000-8000-000000000012";

fn set(
    indexes: Vec<witsml::ChannelIndex>,
    channels: Vec<witsml::AnyChannel>,
    data: &str,
) -> witsml::ChannelSet {
    witsml::ChannelSet {
        citation: citation("a set"),
        uuid: SET.into(),
        schema_version: "2.1".into(),
        index: indexes,
        channel: channels,
        data: Some(witsml::ChannelData {
            data: Some(data.into()),
            file_uri: None,
        }),
        ..Default::default()
    }
}

/// An MD set of two channels, temperature and pressure.
fn md_set(data: &str) -> witsml::ChannelSet {
    set(
        vec![md()],
        vec![double(T_CHANNEL, "T", "degC"), double(P_CHANNEL, "P", "bar")],
        data,
    )
}

fn refused(result: Result<Series, Refused>) -> Refused {
    result.expect_err("refused")
}

// ---------------------------------------------------------------------------
// Instants
// ---------------------------------------------------------------------------

#[test]
fn an_instant_agrees_with_the_storage_reader_where_both_read() {
    // Two implementations of the same calendar: `core_props::unix_seconds`
    // (storage, `Z` and whole seconds only) and this one. The epoch and a leap
    // day are where an era or a sign slip shows.
    for text in [
        "1970-01-01T00:00:00Z",
        "2024-02-29T12:30:45Z",
        "2025-06-02T08:00:00Z",
        "1969-12-31T23:59:59Z",
        "2100-03-01T00:00:00Z",
    ] {
        let theirs = energyml_storage::epc::core_props::unix_seconds(text).expect("storage reads it") as f64;
        assert_eq!(instant::unix_seconds(text), Ok(theirs), "{text}");
    }
}

#[test]
fn an_offset_and_a_fraction_place_the_same_instant() {
    // XML Schema 1.0 Part 2 §3.2.7: the offset is subtracted to reach UTC.
    let utc = instant::unix_seconds("2025-06-02T08:00:00Z").unwrap();
    assert_eq!(instant::unix_seconds("2025-06-02T10:00:00+02:00"), Ok(utc));
    assert_eq!(instant::unix_seconds("2025-06-02T03:30:00-04:30"), Ok(utc));
    assert_eq!(instant::unix_seconds("2025-06-02T08:00:00.25Z"), Ok(utc + 0.25));
    // 24:00:00 is midnight of the next day (§3.2.7).
    assert_eq!(
        instant::unix_seconds("2025-06-01T24:00:00Z"),
        instant::unix_seconds("2025-06-02T00:00:00Z")
    );
}

#[test]
fn an_instant_that_cannot_be_placed_is_refused() {
    for text in [
        "2025-06-02T08:00:00",  // no zone: placing it is choosing one
        "2025-06-30T23:59:60Z", // a leap second
        "2023-02-29T00:00:00Z", // not a leap year
        "2025-13-01T00:00:00Z", // no thirteenth month
        "2025-06-02 08:00:00Z", // no `T`
        "2025-06-02T08:00Z",    // no seconds
        "2025-06-02T08:00:00+15:00",
    ] {
        assert!(instant::unix_seconds(text).is_err(), "`{text}` must be refused");
    }
}

// ---------------------------------------------------------------------------
// Channel sets
// ---------------------------------------------------------------------------

#[test]
fn a_row_is_the_index_then_one_value_per_channel_in_the_order_listed() {
    // The inferred layout (module doc of `channel_set`): [MD, T, P].
    let series = channel_set::read(&md_set("[[0,10.5,1.0],[10,null,2.0],[20,11.5,3.0]]")).expect("read");
    assert_eq!(series.index.kind, IndexKind::MeasuredDepth);
    assert_eq!(series.index.values, [0.0, 10.0, 20.0]);
    assert_eq!(series.index.uom.as_deref(), Some("m"));
    let t = series.channel("T").expect("T");
    let p = series.channel("P").expect("P");
    assert_eq!(t.uuid.as_deref(), Some(T_CHANNEL));
    assert_eq!(
        p.values,
        Values::Floating(vec![1.0, 2.0, 3.0]),
        "the second column is P, not T"
    );
    // JSON Style Guide p. 8: "the arrays may have NULLs" — absent, not zero.
    assert_eq!(t.quality, [Quality::Good, Quality::Absent, Quality::Good]);
    assert!(t.numbers().unwrap()[1].is_nan());
    assert_eq!(t.tally().min, Some(10.5));
    assert_eq!(t.tally().absent, 1);
}

#[test]
fn a_date_time_index_is_read_in_unix_seconds() {
    let data = r#"[["2025-06-02T08:00:00Z","2025-06-02T08:00:00Z",20],["2025-06-02T08:30:00+00:00","2025-06-02T08:30:00Z",40]]"#;
    let series = channel_set::read(&set(
        vec![time()],
        vec![
            channel(T_CHANNEL, "TIME", witsml::ChannelDataKind::DateTime, "s", time()),
            channel(P_CHANNEL, "Q", witsml::ChannelDataKind::Double, "m3/h", time()),
        ],
        data,
    ))
    .expect("read");
    let start = instant::unix_seconds("2025-06-02T08:00:00Z").unwrap();
    assert_eq!(series.index.values, [start, start + 1800.0]);
    assert_eq!(
        series.channel("TIME").unwrap().values,
        Values::Floating(vec![start, start + 1800.0])
    );
}

#[test]
fn a_row_of_the_wrong_length_is_refused() {
    assert!(matches!(
        refused(channel_set::read(&md_set("[[0,1,2],[10,1]]"))),
        Refused::RowLength {
            row: 2,
            found: 2,
            expected: 3,
            indexes: 1,
            channels: 2
        }
    ));
}

#[test]
fn an_index_that_does_not_run_its_way_is_refused() {
    // WITSML TRG v2.1 §7.5 p. 149: "either increasing or decreasing".
    assert!(matches!(
        refused(channel_set::read(&md_set("[[0,1,1],[10,1,1],[5,1,1]]"))),
        Refused::NotMonotone { at: 3, .. }
    ));
    // Strictly: two rows at one depth are two values at one place.
    assert!(matches!(
        refused(channel_set::read(&md_set("[[0,1,1],[0,1,1]]"))),
        Refused::NotMonotone { at: 2, .. }
    ));
    // Decreasing, declared so, reads.
    let up = index(
        eml::DataIndexKind::MeasuredDepth,
        "MD",
        "m",
        eml::IndexDirection::Decreasing,
    );
    let series = channel_set::read(&set(
        vec![up.clone()],
        vec![channel(
            T_CHANNEL,
            "T",
            witsml::ChannelDataKind::Double,
            "degC",
            up,
        )],
        "[[20,1],[10,2],[0,3]]",
    ))
    .expect("a decreasing index, declared so");
    assert_eq!(series.index.direction, Direction::Decreasing);
    let unordered = index(
        eml::DataIndexKind::MeasuredDepth,
        "MD",
        "m",
        eml::IndexDirection::Unordered,
    );
    assert!(matches!(
        refused(channel_set::read(&set(vec![unordered], vec![], "[[1]]"))),
        Refused::UnorderedIndex { .. }
    ));
}

#[test]
fn secondary_indexes_of_one_family_are_read_and_time_with_depth_is_refused() {
    // §7.5 p. 150: "The first index is the primary index. Any additional
    // indexes are secondary indexes"; §7.7 p. 151: "either in time or in
    // depth".
    let tvd = index(
        eml::DataIndexKind::TrueVerticalDepth,
        "TVD",
        "m",
        eml::IndexDirection::Unordered,
    );
    let series = channel_set::read(&set(
        vec![md(), tvd],
        vec![double(T_CHANNEL, "T", "degC")],
        "[[0,0,1],[10,9.5,2],[20,null,3]]",
    ))
    .expect("MD then TVD");
    assert_eq!(series.secondary.len(), 1);
    assert_eq!(series.secondary[0].values[1], 9.5);
    assert!(series.secondary[0].values[2].is_nan(), "a secondary may be null");
    assert_eq!(
        series.channel("T").unwrap().values,
        Values::Floating(vec![1.0, 2.0, 3.0]),
        "the channel after both indexes"
    );

    let mixed = channel_set::read(&set(
        vec![md(), time()],
        vec![double(T_CHANNEL, "T", "degC")],
        r#"[[0,"2025-06-02T08:00:00Z",1]]"#,
    ));
    assert!(matches!(
        refused(mixed),
        Refused::IndexFamilyMix {
            primary_family: "depth",
            other_family: "time",
            ..
        }
    ));
}

#[test]
fn an_index_neither_time_nor_depth_and_a_pass_indexed_depth_are_refused() {
    let hot = index(
        eml::DataIndexKind::Temperature,
        "TEMP",
        "degC",
        eml::IndexDirection::Increasing,
    );
    assert!(matches!(
        refused(channel_set::read(&set(vec![hot], vec![], "[[1]]"))),
        Refused::IndexNotTimeOrDepth { .. }
    ));
    let pass = index(
        eml::DataIndexKind::PassIndexedDepth,
        "PASS",
        "m",
        eml::IndexDirection::Increasing,
    );
    assert!(matches!(
        refused(channel_set::read(&set(vec![pass], vec![], "[[1]]"))),
        Refused::PassIndexedDepth { .. }
    ));
    let channel = channel(
        T_CHANNEL,
        "PID",
        witsml::ChannelDataKind::PassIndexedDepth,
        "m",
        md(),
    );
    assert!(matches!(
        refused(channel_set::read(&set(vec![md()], vec![channel], "[[1,1]]"))),
        Refused::PassIndexedDepth { .. }
    ));
}

#[test]
fn a_file_uri_is_refused_even_beside_inline_data() {
    // §7.2 p. 146: "If this field is non-null, then the data field is ignored."
    let mut with_file = md_set("[[0,1,1]]");
    with_file.data.as_mut().unwrap().file_uri = Some("./abc.avro".into());
    assert!(
        matches!(refused(channel_set::read(&with_file)), Refused::FileUri { uri } if uri == "./abc.avro")
    );
    let mut nothing = md_set("[]");
    nothing.data = None;
    assert!(matches!(refused(channel_set::read(&nothing)), Refused::NoData));
    let no_index = set(vec![], vec![double(T_CHANNEL, "T", "degC")], "[[1]]");
    assert!(matches!(refused(channel_set::read(&no_index)), Refused::NoIndex));
}

#[test]
fn a_channel_whose_own_index_contradicts_the_sets_is_refused() {
    // §7.7 p. 151: "Each channel has its own index."
    let in_feet = index(
        eml::DataIndexKind::MeasuredDepth,
        "MD",
        "ft",
        eml::IndexDirection::Increasing,
    );
    let bad = set(
        vec![md()],
        vec![channel(
            T_CHANNEL,
            "T",
            witsml::ChannelDataKind::Double,
            "degC",
            in_feet,
        )],
        "[[0,1]]",
    );
    assert!(matches!(
        refused(channel_set::read(&bad)),
        Refused::ChannelIndexContradicts { what: "the unit", .. }
    ));
    let in_time = set(
        vec![md()],
        vec![channel(
            T_CHANNEL,
            "T",
            witsml::ChannelDataKind::Double,
            "degC",
            time(),
        )],
        "[[0,1]]",
    );
    assert!(matches!(
        refused(channel_set::read(&in_time)),
        Refused::ChannelIndexContradicts { what: "the kind", .. }
    ));
    // Another mnemonic for the same index changes no number.
    let renamed = index(
        eml::DataIndexKind::MeasuredDepth,
        "DEPT",
        "m",
        eml::IndexDirection::Increasing,
    );
    assert!(
        channel_set::read(&set(
            vec![md()],
            vec![channel(
                T_CHANNEL,
                "T",
                witsml::ChannelDataKind::Double,
                "degC",
                renamed
            )],
            "[[0,1]]"
        ))
        .is_ok()
    );
}

#[test]
fn a_value_of_the_wrong_kind_is_refused_and_the_guides_spellings_are_read() {
    assert!(matches!(
        refused(channel_set::read(&md_set(r#"[[0,"hot",1]]"#))),
        Refused::ValueKind { row: 1, .. }
    ));
    let ints = set(
        vec![md()],
        vec![channel(T_CHANNEL, "N", witsml::ChannelDataKind::Int, "Euc", md())],
        "[[0,3],[1,3.5]]",
    );
    assert!(matches!(
        refused(channel_set::read(&ints)),
        Refused::ValueKind { row: 2, .. }
    ));
    assert!(matches!(
        refused(channel_set::read(&md_set("[[null,1,1]]"))),
        Refused::NullIndex { row: 1, .. }
    ));
    assert!(matches!(
        refused(channel_set::read(&md_set("{}"))),
        Refused::NotRows { .. }
    ));
    // JSON Style Guide p. 8: "NaN", "+Infinity", "-Infinity" in quotes.
    let series = channel_set::read(&md_set(r#"[[0,"NaN",1],[1,"+Infinity",2]]"#)).expect("read");
    let t = series.channel("T").unwrap();
    assert_eq!(t.quality, [Quality::Absent, Quality::Flagged("+Infinity".into())]);
    // Integers, booleans and strings keep their family; a null is absent.
    let mixed = set(
        vec![md()],
        vec![
            channel(T_CHANNEL, "N", witsml::ChannelDataKind::Long, "Euc", md()),
            channel(P_CHANNEL, "B", witsml::ChannelDataKind::Boolean, "Euc", md()),
            channel(
                "50000000-0000-4000-8000-000000000013",
                "S",
                witsml::ChannelDataKind::String,
                "Euc",
                md(),
            ),
        ],
        r#"[[0,3,true,"a"],[1,null,null,null]]"#,
    );
    let series = channel_set::read(&mixed).expect("read");
    assert_eq!(series.channel("N").unwrap().values, Values::Integer(vec![3, 0]));
    assert_eq!(series.channel("B").unwrap().quality[1], Quality::Absent);
    assert_eq!(
        series.channel("S").unwrap().values,
        Values::Text(vec!["a".into(), String::new()])
    );
    let json = series.to_json();
    assert_eq!(
        json["channels"][0]["values"],
        serde_json::json!([3, null]),
        "a placeholder is written null"
    );
}

// ---------------------------------------------------------------------------
// PRODML time series
// ---------------------------------------------------------------------------

const SERIES: &str = "60000000-0000-4000-8000-000000000001";

fn sample(value: f64, at: Option<&str>, status: Option<prodml::ValueStatus>) -> prodml::AbstractValue {
    prodml::AbstractValue::DoubleValue(prodml::DoubleValue {
        double_value: prodml::TimeSeriesDoubleSample {
            value,
            d_tim: at.map(str::to_string),
            status,
        },
    })
}

fn string_sample(value: &str, at: Option<&str>) -> prodml::AbstractValue {
    prodml::AbstractValue::StringValue(prodml::StringValue {
        string_value: prodml::TimeSeriesStringSample {
            value: value.into(),
            d_tim: at.map(str::to_string),
        },
    })
}

fn time_series(
    uuid: &str,
    well: &str,
    class: eml::MeasureClass,
    values: Vec<prodml::AbstractValue>,
) -> prodml::TimeSeriesData {
    prodml::TimeSeriesData {
        citation: citation(&format!("{well} — series")),
        uuid: uuid.into(),
        schema_version: "2.3".into(),
        key: vec![prodml::KeywordValueStruct {
            value: format!("eml://lutetia.example/well({well})/wellbore(01)/wellhead"),
            keyword: prodml::TimeSeriesKeyword::AssetIdentifier,
        }],
        uom: Some(unit("bar")),
        measure_class: Some(class),
        data_value: values,
        ..Default::default()
    }
}

fn minutes(count: usize) -> Vec<String> {
    let start = instant::unix_seconds("2025-06-02T00:00:00Z").unwrap() as i64;
    (0..count)
        .map(|k| energyml_storage::epc::core_props::w3cdtf(start + 60 * k as i64))
        .collect()
}

#[test]
fn a_frozen_sample_is_flagged_and_a_nan_is_absent() {
    // Usage Guide v2.3 §8.2 p. 31: good only if no status and not NaN.
    let at = minutes(4);
    let object = time_series(
        SERIES,
        "GTH-1",
        eml::MeasureClass::Pressure,
        vec![
            sample(3.2, Some(&at[0]), None),
            sample(3.2, Some(&at[1]), Some(prodml::ValueStatus::Frozen)),
            sample(f64::NAN, Some(&at[2]), None),
            sample(3.3, Some(&at[3]), None),
        ],
    );
    let read = time_series::read(&object).expect("read");
    let Content::Samples(series) = &read.content else {
        panic!("samples")
    };
    let channel = &series.channels[0];
    assert_eq!(
        channel.quality,
        [
            Quality::Good,
            Quality::Flagged("frozen".into()),
            Quality::Absent,
            Quality::Good
        ]
    );
    assert_eq!(series.index.kind, IndexKind::DateTime);
    assert_eq!(series.index.values[3] - series.index.values[0], 180.0);
    assert_eq!(channel.measure_class.as_deref(), Some("pressure"));
    assert_eq!(read.well_named(), Ok(Some("GTH-1".into())));
    let json = read.to_json();
    assert_eq!(
        json["content"]["series"]["channels"][0]["flags"],
        serde_json::json!([{ "from": 1, "to": 2, "quality": "frozen" }])
    );
}

#[test]
fn a_series_without_instants_is_static_only_when_it_holds_one_sample() {
    // TimeSeriesData.xsd l. 100: "If no time is specified then the value is
    // static and only one sample can be defined."
    let one = time_series(
        SERIES,
        "GTH-1",
        eml::MeasureClass::Pressure,
        vec![sample(3.2, None, None)],
    );
    assert!(matches!(
        time_series::read(&one).unwrap().content,
        Content::Static {
            quality: Quality::Good,
            ..
        }
    ));
    let at = minutes(2);
    let two = time_series(
        SERIES,
        "GTH-1",
        eml::MeasureClass::Pressure,
        vec![sample(3.2, Some(&at[0]), None), sample(3.3, None, None)],
    );
    assert!(matches!(
        time_series::read(&two),
        Err(Refused::UntimedSample { at: 2, samples: 2 })
    ));
    let none = time_series(SERIES, "GTH-1", eml::MeasureClass::Pressure, vec![]);
    assert!(matches!(
        time_series::read(&none).unwrap().content,
        Content::Empty
    ));
}

#[test]
fn mixed_kinds_disordered_and_unreadable_instants_are_refused() {
    let at = minutes(3);
    let mixed = time_series(
        SERIES,
        "GTH-1",
        eml::MeasureClass::Pressure,
        vec![sample(1.0, Some(&at[0]), None), string_sample("on", Some(&at[1]))],
    );
    assert!(matches!(
        time_series::read(&mixed),
        Err(Refused::MixedValueKinds { at: 2, .. })
    ));
    let back = time_series(
        SERIES,
        "GTH-1",
        eml::MeasureClass::Pressure,
        vec![sample(1.0, Some(&at[1]), None), sample(1.0, Some(&at[0]), None)],
    );
    assert!(matches!(
        time_series::read(&back),
        Err(Refused::NotMonotone { at: 2, .. })
    ));
    let local = time_series(
        SERIES,
        "GTH-1",
        eml::MeasureClass::Pressure,
        vec![
            sample(1.0, Some("2025-06-02T00:00:00"), None),
            sample(1.0, Some(&at[1]), None),
        ],
    );
    assert!(matches!(time_series::read(&local), Err(Refused::Instant { .. })));
    // Strings read as text, never as numbers.
    let words = time_series(
        SERIES,
        "GTH-1",
        eml::MeasureClass::Pressure,
        vec![
            string_sample("open", Some(&at[0])),
            string_sample("closed", Some(&at[1])),
        ],
    );
    let Content::Samples(series) = time_series::read(&words).unwrap().content else {
        panic!()
    };
    assert_eq!(
        series.channels[0].values,
        Values::Text(vec!["open".into(), "closed".into()])
    );
}

// ---------------------------------------------------------------------------
// A series and its well (house/time-series-asset-resolves)
// ---------------------------------------------------------------------------

fn well(uuid: &str, title: &str, legal: Option<&str>) -> witsml::Well {
    witsml::Well {
        citation: citation(title),
        uuid: uuid.into(),
        schema_version: "2.1".into(),
        name_legal: legal.map(str::to_string),
        ..Default::default()
    }
}

fn boxed<T: DynObject + 'static>(object: T) -> Box<dyn DynObject> {
    Box::new(object)
}

fn pairs(held: &[(String, Box<dyn DynObject>)]) -> Vec<(&str, &dyn Reflect)> {
    held.iter()
        .map(|(uuid, object)| (uuid.as_str(), object.as_reflect()))
        .collect()
}

const W1: &str = "70000000-0000-4000-8000-000000000001";
const W2: &str = "70000000-0000-4000-8000-000000000002";
const S1: &str = "70000000-0000-4000-8000-000000000011";
const S2: &str = "70000000-0000-4000-8000-000000000012";
const S3: &str = "70000000-0000-4000-8000-000000000013";

fn doublet() -> Vec<(String, Box<dyn DynObject>)> {
    let at = minutes(1);
    vec![
        (W1.into(), boxed(well(W1, "GTH-1", None))),
        (W2.into(), boxed(well(W2, "Puits 2", Some("GTH-2")))),
        (
            S1.into(),
            boxed(time_series(
                S1,
                "GTH-1",
                eml::MeasureClass::Pressure,
                vec![sample(3.2, Some(&at[0]), None)],
            )),
        ),
        (
            S2.into(),
            boxed(time_series(
                S2,
                "GTH-1",
                eml::MeasureClass::ThermodynamicTemperature,
                vec![sample(16.5, Some(&at[0]), None)],
            )),
        ),
        (
            S3.into(),
            boxed(time_series(
                S3,
                "GTH-2",
                eml::MeasureClass::VolumePerTime,
                vec![sample(40.0, Some(&at[0]), None)],
            )),
        ),
    ]
}

#[test]
fn a_series_is_found_by_its_wells_name_and_several_are_refused_rather_than_picked() {
    let held = doublet();
    let objects = pairs(&held);
    let gth1 = objects[0].1;
    let gth2 = objects[1].1;
    // By title, with the measure class saying which.
    assert_eq!(
        time_series::of_well(&objects, gth1, Some("pressure")).map(|(uuid, _)| uuid),
        Ok(S1)
    );
    assert_eq!(
        time_series::of_well(&objects, gth1, Some("thermodynamic temperature")).map(|(uuid, _)| uuid),
        Ok(S2)
    );
    // By NameLegal.
    assert_eq!(
        time_series::of_well(&objects, gth2, None).map(|(uuid, _)| uuid),
        Ok(S3)
    );
    // Two series and no class: which one is meant is stated nowhere.
    assert!(matches!(
        time_series::of_well(&objects, gth1, None),
        Err(Refused::SeveralMatch { .. })
    ));
    assert!(matches!(
        time_series::of_well(&objects, gth2, Some("pressure")),
        Err(Refused::NoMatch { .. })
    ));
    // And back: the series to its well.
    assert_eq!(
        time_series::well_of(objects[4].1, &objects).map(|(uuid, _)| uuid),
        Ok(W2)
    );
}

#[test]
fn a_name_two_wells_answer_to_is_no_identity() {
    let mut held = doublet();
    held.push((
        "70000000-0000-4000-8000-000000000003".into(),
        boxed(well("70000000-0000-4000-8000-000000000003", "GTH-1", None)),
    ));
    let objects = pairs(&held);
    assert!(matches!(
        time_series::of_well(&objects, objects[0].1, Some("pressure")),
        Err(Refused::NameNotAnIdentity { .. })
    ));
    assert!(matches!(
        time_series::well_of(objects[2].1, &objects),
        Err(Refused::NameNotAnIdentity { .. })
    ));
}

// ---------------------------------------------------------------------------
// A flow test's channels, through the part that holds them
// ---------------------------------------------------------------------------

const LOG: &str = "80000000-0000-4000-8000-000000000001";
const TEST: &str = "80000000-0000-4000-8000-000000000002";

fn log_holding(set: witsml::ChannelSet) -> witsml::Log {
    witsml::Log {
        citation: citation("a log"),
        uuid: LOG.into(),
        schema_version: "2.1".into(),
        channel_set: vec![witsml::AnyChannelSet::ChannelSet(set)],
        ..Default::default()
    }
}

fn timed_set() -> witsml::ChannelSet {
    set(
        vec![time()],
        vec![
            channel(T_CHANNEL, "TIME", witsml::ChannelDataKind::DateTime, "s", time()),
            channel(P_CHANNEL, "BHP", witsml::ChannelDataKind::Double, "bar", time()),
        ],
        r#"[["2025-06-02T08:00:00Z","2025-06-02T08:00:00Z",12],["2025-06-02T08:30:00Z","2025-06-02T08:30:00Z",14]]"#,
    )
}

fn test_pointing_at(set_uuid: &str, pressure_uuid: &str) -> prodml::InjectionFlowTest {
    prodml::InjectionFlowTest {
        citation: citation("a test"),
        uuid: TEST.into(),
        schema_version: "2.3".into(),
        interval_measurement_set: prodml::FlowTestMeasurementSet {
            measured_pressure: vec![prodml::MeasuredPressureData {
                channel_set: dor(set_uuid, "witsml21.ChannelSet", "a set"),
                time_channel: dor(T_CHANNEL, "witsml21.Channel", "TIME"),
                pressure_channel: dor(pressure_uuid, "witsml21.Channel", "BHP"),
                ..Default::default()
            }],
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn a_flow_tests_channel_is_read_through_the_log_that_nests_its_set() {
    let held = vec![
        (LOG.to_string(), boxed(log_holding(timed_set()))),
        (TEST.to_string(), boxed(test_pointing_at(SET, P_CHANNEL))),
    ];
    let objects = pairs(&held);
    let measured = flow_test::read(objects[1].1, &objects, &NoObjects).expect("read");
    assert_eq!(measured.len(), 1);
    let one = &measured[0];
    assert_eq!(
        (one.group.as_str(), one.role.as_str()),
        ("MeasuredPressure", "PressureChannel")
    );
    assert_eq!(one.channel_set, SET);
    assert_eq!(one.series.channels[0].values, Values::Floating(vec![12.0, 14.0]));
    assert_eq!(
        one.series.index.mnemonic, "TIME",
        "indexed by the test's TimeChannel"
    );
    // The nested set is reachable by its own uuid too.
    assert!(matches!(series::read(SET, &objects, &NoObjects), Ok(Readout::Sets(sets)) if sets.len() == 1));
}

#[test]
fn a_set_nowhere_and_a_channel_not_in_its_set_are_refused_by_name() {
    let elsewhere = "80000000-0000-4000-8000-0000000000ff";
    let held = vec![
        (LOG.to_string(), boxed(log_holding(timed_set()))),
        (TEST.to_string(), boxed(test_pointing_at(elsewhere, P_CHANNEL))),
    ];
    let objects = pairs(&held);
    assert!(
        matches!(flow_test::read(objects[1].1, &objects, &NoObjects), Err(Refused::ChannelSetNotFound { uuid, .. }) if uuid == elsewhere)
    );
    let held = vec![
        (LOG.to_string(), boxed(log_holding(timed_set()))),
        (TEST.to_string(), boxed(test_pointing_at(SET, elsewhere))),
    ];
    let objects = pairs(&held);
    assert!(matches!(
        flow_test::read(objects[1].1, &objects, &NoObjects),
        Err(Refused::ChannelNotInSet { role, channel, .. }) if role == "PressureChannel" && channel == elsewhere
    ));
}

// ---------------------------------------------------------------------------
// Decimation
// ---------------------------------------------------------------------------

/// A series of `values` against 0, 1, 2, …, with the qualities given.
fn numbered(values: Vec<f64>, quality: Vec<Quality>) -> Series {
    Series {
        uuid: "s".into(),
        title: "s".into(),
        index: Index {
            kind: IndexKind::ElapsedTime,
            mnemonic: "t".into(),
            uom: Some("s".into()),
            direction: Direction::Increasing,
            datum: None,
            values: (0..values.len()).map(|k| k as f64).collect(),
        },
        secondary: Vec::new(),
        channels: vec![Channel {
            uuid: None,
            mnemonic: "v".into(),
            uom: None,
            data_kind: None,
            property_kind: None,
            measure_class: None,
            values: Values::Floating(values),
            quality,
        }],
    }
}

#[test]
fn a_one_sample_spike_inside_a_bucket_survives() {
    let mut values = vec![1.0; 1000];
    values[537] = 100.0;
    values[538] = -50.0;
    let series = numbered(values, vec![Quality::Good; 1000]);
    let out = decimate(&series, 0, 0.0, 999.0, 10).expect("decimated");
    assert!(
        out.points
            .iter()
            .any(|point| point.sample == 537 && point.value == 100.0),
        "the spike up"
    );
    assert!(
        out.points
            .iter()
            .any(|point| point.sample == 538 && point.value == -50.0),
        "the spike down"
    );
    assert!(
        out.points.len() <= 40,
        "{} points for 10 buckets",
        out.points.len()
    );
    assert!(
        out.points.windows(2).all(|pair| pair[0].sample < pair[1].sample),
        "in sample order"
    );
    assert_eq!(out.samples, 1000);
}

#[test]
fn a_frozen_window_is_not_bridged() {
    let mut quality = vec![Quality::Good; 100];
    for q in &mut quality[40..60] {
        *q = Quality::Flagged("frozen".into());
    }
    quality[80] = Quality::Absent;
    let values: Vec<f64> = (0..100).map(|k| k as f64).collect();
    let out = decimate(&numbered(values, quality), 0, 0.0, 99.0, 5).expect("decimated");
    // Nothing kept from inside the window, and the sample after it begins a
    // run a chart must not join to the one before.
    assert!(
        out.points
            .iter()
            .all(|point| !(40..60).contains(&point.sample) && point.sample != 80)
    );
    let after = out
        .points
        .iter()
        .find(|point| point.sample == 60)
        .expect("the first good sample after the window");
    assert!(after.starts_run);
    let after_hole = out
        .points
        .iter()
        .find(|point| point.sample == 81)
        .expect("the sample after the null");
    assert!(after_hole.starts_run);
    let starts: Vec<usize> = out
        .points
        .iter()
        .filter(|point| point.starts_run)
        .map(|point| point.sample)
        .collect();
    assert_eq!(
        starts,
        [0, 60, 81],
        "three runs, and a bucket boundary is no break"
    );
    assert_eq!(out.gaps.len(), 2);
    assert_eq!(
        (
            out.gaps[0].from,
            out.gaps[0].to,
            out.gaps[0].samples,
            out.gaps[0].quality.as_str()
        ),
        (40.0, 59.0, 20, "frozen")
    );
    assert_eq!(out.gaps[1].quality, "absent");
}

#[test]
fn the_bound_holds_whatever_the_values() {
    // A deterministic scramble, and gaps every so often.
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % 10_000) as f64 / 100.0
    };
    let count = 20_000;
    let values: Vec<f64> = (0..count).map(|_| next()).collect();
    let quality: Vec<Quality> = (0..count)
        .map(|k| {
            if k % 997 < 3 {
                Quality::Absent
            } else {
                Quality::Good
            }
        })
        .collect();
    let series = numbered(values, quality);
    for buckets in [1, 7, 100, 1000] {
        let out = decimate(&series, 0, 0.0, (count - 1) as f64, buckets).expect("decimated");
        assert!(
            out.points.len() <= 4 * (buckets + out.gaps.len()),
            "{buckets} buckets, {} gaps: {} points",
            out.gaps.len(),
            out.points.len()
        );
        let width = (count - 1) as f64 / buckets as f64;
        let mut used: Vec<usize> = out
            .points
            .iter()
            .map(|point| ((point.index / width).floor() as usize).min(buckets - 1))
            .collect();
        used.dedup();
        assert!(used.len() <= buckets);
    }
    // A window reads only what it covers.
    let out = decimate(&series, 0, 100.0, 199.0, 10).expect("decimated");
    assert_eq!(out.samples, 100);
    assert!(
        out.points
            .iter()
            .all(|point| (100.0..=199.0).contains(&point.index))
    );
}

#[test]
fn what_cannot_be_decimated_is_refused() {
    let series = numbered(vec![1.0, 2.0], vec![Quality::Good; 2]);
    assert!(matches!(
        decimate(&series, 0, 0.0, 1.0, 0),
        Err(Refused::BadWindow { .. })
    ));
    assert!(matches!(
        decimate(&series, 0, 1.0, 1.0, 4),
        Err(Refused::BadWindow { .. })
    ));
    assert!(matches!(
        decimate(&series, 3, 0.0, 1.0, 4),
        Err(Refused::NoSuchChannel { at: 3, channels: 1 })
    ));
    let mut words = series.clone();
    words.channels[0].values = Values::Text(vec!["a".into(), "b".into()]);
    assert!(matches!(
        decimate(&words, 0, 0.0, 1.0, 4),
        Err(Refused::NotNumeric { .. })
    ));
}

// ---------------------------------------------------------------------------
// Production periods
// ---------------------------------------------------------------------------

fn period(
    start: &str,
    hours: f64,
    duration_uom: eml::TimeUom,
    rate: f64,
    rate_uom: eml::VolumePerTimeUomWithLegacy,
) -> prodml::ProductionWellPeriod {
    prodml::ProductionWellPeriod {
        start_time: start.into(),
        duration: eml::TimeMeasure {
            value: hours,
            uom: duration_uom,
        },
        reporting_entity: dor(
            "90000000-0000-4000-8000-000000000001",
            "prodml23.ReportingEntity",
            "GTH-1",
        ),
        product_rate: vec![prodml::ProductRate {
            product_fluid_kind: prodml::ProductFluidKindExt::WaterMinusProcessed,
            volume_flow_rate: Some(eml::FlowRateValue {
                flow_rate: eml::VolumePerTimeMeasure {
                    value: rate,
                    uom: rate_uom,
                },
                ..Default::default()
            }),
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn parameters(periods: Vec<prodml::ProductionWellPeriod>) -> prodml::WellProductionParameters {
    prodml::WellProductionParameters {
        citation: citation("GTH-1 — producing"),
        uuid: "90000000-0000-4000-8000-000000000002".into(),
        schema_version: "2.3".into(),
        production_period: periods,
        ..Default::default()
    }
}

#[test]
fn a_periods_quantity_is_its_rate_times_its_own_duration() {
    // January: 744 h at 40 m3/h. February: 672 h at a rate per **day** — the
    // two times meet through `units::seconds`, never by multiplying the
    // numbers as written (672 × 960 would be 645 120).
    let read = production::read(&parameters(vec![
        period(
            "2025-01-01T00:00:00Z",
            744.0,
            eml::TimeUom::H,
            40.0,
            eml::VolumePerTimeUomWithLegacy::M3PerH,
        ),
        period(
            "2025-02-01T00:00:00Z",
            672.0,
            eml::TimeUom::H,
            960.0,
            eml::VolumePerTimeUomWithLegacy::M3PerD,
        ),
    ]))
    .expect("read");
    assert_eq!(read.len(), 2);
    assert_eq!(read[0].products[0].quantity, Some(29_760.0));
    assert_eq!(read[0].products[0].quantity_uom.as_deref(), Some("m3"));
    assert_eq!(read[1].products[0].quantity, Some(26_880.0), "28 d × 960 m3/d");
    assert_eq!(read[1].duration_uom, "h");
    assert_eq!(read[1].duration_seconds, 28.0 * 86_400.0);
    assert_eq!(read[0].products[0].product, "water - processed");
}

#[test]
fn a_unit_that_cannot_be_checked_is_refused_rather_than_converted() {
    // A julian year is not in this build's time table; a month is not either.
    let year = parameters(vec![period(
        "2025-01-01T00:00:00Z",
        1.0,
        eml::TimeUom::A,
        40.0,
        eml::VolumePerTimeUomWithLegacy::M3PerH,
    )]);
    assert!(matches!(production::read(&year), Err(Refused::Unit { uom, .. }) if uom == "a"));
    let monthly = parameters(vec![period(
        "2025-01-01T00:00:00Z",
        744.0,
        eml::TimeUom::H,
        40.0,
        eml::VolumePerTimeUomWithLegacy::V1000scfPerMo,
    )]);
    assert!(matches!(production::read(&monthly), Err(Refused::Unit { uom, .. }) if uom == "1000scf/mo"));
}
