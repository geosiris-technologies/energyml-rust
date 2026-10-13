//! `witsml-logs` — the WITSML 2.1 side of a wellbore's continuous properties:
//! a `Log`, its `ChannelSet` indexed by measured depth, a `Channel` per
//! property, the data blob inline.
//!
//! **The form of the assertions comes from the standard and from the module's
//! own decisions**, cited where used:
//!
//! - `ChannelSet.Index[0].IndexKind` is one index, MD or time, never both;
//! - `ChannelData.Data` is *"the data blob in JSON form"*, and never a
//!   `FileUri` (generated doc comment, `crates/generated/energyml-witsml-2-1/src/log.rs`);
//! - a `Channel`'s `Wellbore` names the one `witsml-wells` writes for the same
//!   RESQML wellbore interpretation, checked here against a real `witsml-wells`
//!   plan over the same frame, not against a formula copied a second time.
//!
//! **The numbers are the test's own**: a straight vertical frame at a
//! Lambert-93-like place (x ≈ 651 000, y ≈ 6 861 000), because a defect on
//! 24/09 only showed far from the origin. Every JSON the plan writes is read
//! back into the generated WITSML types before anything is asserted of it.

use serde_json::{Value, json};

use energyml_build::operation::{Choices, Identity, Plan, Registry, When};
use energyml_build::pieces::{Known, KnownProperty, Pieces, Ref, Source};
use energyml_core::registry::DynObject;
use energyml_domain::values::Values;
use energyml_eml_2_3 as eml;
use energyml_geometry::geometry::{Geometry, Patch, Topology};
use energyml_witsml_2_1 as witsml;

const FRAME: &str = "10000000-0000-4000-8000-000000000001";
const TRAJECTORY: &str = "10000000-0000-4000-8000-000000000002";
const INTERPRETATION: &str = "10000000-0000-4000-8000-000000000003";
const DATUM: &str = "10000000-0000-4000-8000-000000000004";
const COMPOUND: &str = "10000000-0000-4000-8000-000000000005";
const TEMPERATURE_PROPERTY: &str = "20000000-0000-4000-8000-000000000001";
const PRESSURE_PROPERTY: &str = "20000000-0000-4000-8000-000000000002";
const TEMPERATURE_KIND: &str = "30000000-0000-4000-8000-000000000001";
const PRESSURE_KIND: &str = "30000000-0000-4000-8000-000000000002";
const BUSINESS_ASSOCIATE: &str = "40000000-0000-4000-8000-000000000001";

/// A straight vertical hole: `count` nodes, 10 m of MD apart, from `head`
/// straight down — Lambert-93-like, so a defect that only shows far from the
/// origin would show here too.
const HEAD: [f64; 3] = [651_000.0, 6_861_000.0, 50.0];

fn when() -> When {
    When {
        creation: "2026-09-24T00:00:00Z".into(),
        originator: "test".into(),
    }
}

fn mds(count: usize) -> Vec<f64> {
    (0..count).map(|k| k as f64 * 10.0).collect()
}

/// What `witsml-logs` and `witsml-wells` both need of one frame — the same
/// `Known`, because the two operations read one frame, not two: `witsml-logs`
/// only reads `reference`, `interpretation`, `datum` and `node_mds`; the rest
/// is here only so the same fixture also drives a real `witsml-wells` plan
/// (the wellbore-reference test).
fn frame_known(count: usize) -> Known {
    Known {
        reference: Some(Ref::new(
            FRAME,
            "resqml22.WellboreFrameRepresentation",
            "GTH-1 — frame",
        )),
        crs: Some(Ref::new(
            COMPOUND,
            "eml23.LocalEngineeringCompoundCrs",
            "EPSG:2154 + EPSG:5720",
        )),
        crs_epsg: Some(2154),
        vertical_epsg: Some(5720),
        interpretation: Some(Ref::new(
            INTERPRETATION,
            "resqml22.WellboreInterpretation",
            "GTH-1 — interpretation",
        )),
        vertical_uom: Some("m".to_string()),
        vertical_up: Some(true),
        node_count: Some(count),
        wellhead: Some(HEAD),
        node_mds: Some(mds(count)),
        trajectory: Some(Ref::new(
            TRAJECTORY,
            "resqml22.WellboreTrajectoryRepresentation",
            "GTH-1 — trajectory",
        )),
        datum: Some(Ref::new(
            DATUM,
            "eml23.ReferencePointInALocalEngineeringCompoundCrs",
            "GTH-1 — datum",
        )),
        ..Known::default()
    }
}

/// The frame, as the `frames` slot of either operation takes it: a shape (its
/// nodes, straight down from `HEAD`) and its companion `Known`.
fn frame_bag(pieces: Pieces, count: usize) -> Pieces {
    let points: Vec<[f64; 3]> = mds(count)
        .iter()
        .map(|md| [HEAD[0], HEAD[1], HEAD[2] - md])
        .collect();
    let n = points.len() as u32;
    let shape = Source::Shape {
        geometry: Geometry {
            kind: "polyline",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points,
                topology: Topology::Polylines(vec![n]),
                closed: Vec::new(),
            }],
        },
        closed: None,
    };
    pieces
        .with("frames", shape)
        .with("frames (object)", Source::Object(frame_known(count)))
}

/// A continuous property laid on [`FRAME`] — [`frame_known`]'s own reference.
fn property(
    uuid: &str,
    title: &str,
    kind_uuid: &str,
    kind_title: &str,
    uom: &str,
    values: Vec<f64>,
) -> Known {
    Known {
        reference: Some(Ref::new(uuid, "resqml22.ContinuousProperty", title)),
        property: Some(KnownProperty {
            supporting: Some(Ref::new(
                FRAME,
                "resqml22.WellboreFrameRepresentation",
                "GTH-1 — frame",
            )),
            indexable: "nodes".to_string(),
            value_count: vec![1],
            timed: false,
            realizations: false,
            uom: Some(uom.to_string()),
            kind: Some(Ref::new(kind_uuid, "eml23.PropertyKind", kind_title)),
            lookup: None,
            null_value: None,
            facets: Vec::new(),
            values: Ok(Values::Floating(values)),
        }),
        ..Known::default()
    }
}

fn temperature() -> Known {
    // One NaN: above the ground the law of `well-logs` states nothing, and
    // this is where a channel's row must read `null`, never a number.
    property(
        TEMPERATURE_PROPERTY,
        "GTH-1 — temperature",
        TEMPERATURE_KIND,
        "temperature",
        "degC",
        vec![f64::NAN, 13.1, 13.4, 13.7, 14.0],
    )
}

fn pressure() -> Known {
    property(
        PRESSURE_PROPERTY,
        "GTH-1 — pressure",
        PRESSURE_KIND,
        "pressure",
        "bar",
        vec![0.0, 1.0, 2.0, 3.0, 4.0],
    )
}

/// The bag `witsml-logs` takes: the frame, and these properties in the slot
/// `properties`.
fn bag(properties: Vec<Known>) -> Pieces {
    let mut pieces = frame_bag(Pieces::empty(), 5);
    for (at, known) in properties.into_iter().enumerate() {
        let name = if at == 0 {
            "properties".to_string()
        } else {
            format!("properties {}", at + 1)
        };
        pieces = pieces.with(name, Source::Object(known));
    }
    pieces
}

fn logging_company() -> Value {
    json!({ "$type": "eml23.DataObjectReference", "Uuid": BUSINESS_ASSOCIATE, "QualifiedType": "eml23.BusinessAssociate", "Title": "energyml-rust — synthetic law" })
}

fn choices() -> Choices {
    Choices::new().set("loggingCompany", logging_company())
}

fn planned(pieces: &Pieces, choices: Choices) -> Result<Plan, String> {
    Registry::new()
        .plan("witsml-logs", pieces, &choices, &Identity::derived(), &when())
        .map_err(|why| why.to_string())
}

/// The JSON of a plan's object, read back into its generated type — the road
/// `apply` takes.
fn read<T: energyml_core::value::Reflect + Clone>(object: &energyml_build::operation::NewObject) -> T {
    let schemas = energyml_schemas::all();
    let document = energyml_core::json::read_value(&object.json, &schemas)
        .unwrap_or_else(|why| panic!("`{}` does not read back: {why}", object.title));
    let root: &dyn DynObject = &*document.root;
    root.as_reflect()
        .downcast_ref::<T>()
        .unwrap_or_else(|| {
            panic!(
                "`{}` does not read back as {}",
                object.title,
                std::any::type_name::<T>()
            )
        })
        .clone()
}

fn the_log(plan: &Plan) -> witsml::Log {
    let object = plan
        .objects
        .iter()
        .find(|one| one.qualified_type == "witsml21.Log")
        .expect("a Log in the plan");
    read(object)
}

fn the_channel_set(log: &witsml::Log) -> witsml::ChannelSet {
    match log.channel_set.as_slice() {
        [witsml::AnyChannelSet::ChannelSet(set)] => set.clone(),
        other => panic!("one ChannelSet, nested: {other:?}"),
    }
}

fn channels_of(set: &witsml::ChannelSet) -> Vec<witsml::Channel> {
    set.channel
        .iter()
        .map(|any| match any {
            witsml::AnyChannel::Channel(channel) => channel.clone(),
            other => panic!("a plain Channel, not {other:?}"),
        })
        .collect()
}

/// The data blob, parsed — rows in the order the file states them.
fn rows_of(set: &witsml::ChannelSet) -> Vec<Vec<Value>> {
    let text = set
        .data
        .as_ref()
        .and_then(|data| data.data.as_deref())
        .expect("ChannelData.Data, inline");
    assert!(
        set.data
            .as_ref()
            .and_then(|data| data.file_uri.as_deref())
            .is_none(),
        "never a FileUri"
    );
    let parsed: Value =
        serde_json::from_str(text).unwrap_or_else(|why| panic!("Data does not parse as JSON: {why}"));
    parsed
        .as_array()
        .unwrap_or_else(|| panic!("Data is not a JSON array: {text}"))
        .iter()
        .map(|row| {
            row.as_array()
                .unwrap_or_else(|| panic!("a row that is not an array: {row}"))
                .clone()
        })
        .collect()
}

// ---------------------------------------------------------------------------

#[test]
fn a_log_nests_one_channel_set_indexed_by_measured_depth() {
    // To break: a second index (time), the wrong `IndexKind`, a datum copied
    // from somewhere other than the frame's own.
    let plan = planned(&bag(vec![temperature(), pressure()]), choices()).expect("a plan");
    assert_eq!(
        plan.objects.len(),
        1,
        "one Log, nesting its ChannelSet and Channels — none of them a part of its own"
    );
    assert_eq!(plan.objects[0].qualified_type, "witsml21.Log");

    let log = the_log(&plan);
    assert_eq!(log.schema_version, "2.1");
    let set = the_channel_set(&log);
    assert_eq!(set.index.len(), 1, "one index, MD or time, never both");
    let index = &set.index[0];
    assert_eq!(index.index_kind, eml::DataIndexKind::MeasuredDepth);
    assert_eq!(index.direction, eml::IndexDirection::Increasing);
    assert_eq!(index.uom, eml::UnitOfMeasureExt::M);
    assert_eq!(
        index.datum.as_ref().map(|d| d.uuid.as_str()),
        Some(DATUM),
        "the index's datum is the frame's own"
    );

    let rows = rows_of(&set);
    assert_eq!(rows.len(), 5, "one row per node of the frame");
    let read_mds: Vec<f64> = rows
        .iter()
        .map(|row| row[0].as_f64().expect("MD, a number"))
        .collect();
    assert_eq!(
        read_mds,
        mds(5),
        "the index is the frame's own measured depths, exactly"
    );
}

#[test]
fn one_channel_per_property_carries_its_own_uom_and_kind() {
    // To break: channels swapped, a unit copied from the wrong property, a
    // `NaN` written as a number instead of `null`.
    let plan = planned(&bag(vec![temperature(), pressure()]), choices()).expect("a plan");
    let set = the_channel_set(&the_log(&plan));
    let channels = channels_of(&set);
    assert_eq!(channels.len(), 2);

    let by_mnemonic = |name: &str| {
        channels.iter().find(|c| c.mnemonic == name).unwrap_or_else(|| {
            panic!(
                "a channel `{name}`: {:?}",
                channels.iter().map(|c| &c.mnemonic).collect::<Vec<_>>()
            )
        })
    };
    let temperature = by_mnemonic("temperature");
    assert_eq!(temperature.uom, eml::UnitOfMeasureExt::DegC);
    assert_eq!(temperature.data_kind, witsml::ChannelDataKind::Double);
    assert_eq!(temperature.channel_property_kind.uuid, TEMPERATURE_KIND);
    assert_eq!(temperature.logging_company.uuid, BUSINESS_ASSOCIATE);
    assert_eq!(temperature.index.len(), 1);
    assert_eq!(temperature.index[0].index_kind, eml::DataIndexKind::MeasuredDepth);

    let pressure = by_mnemonic("pressure");
    assert_eq!(pressure.uom, eml::UnitOfMeasureExt::Bar);
    assert_eq!(pressure.channel_property_kind.uuid, PRESSURE_KIND);

    // The rows carry the channels in `ChannelSet.Channel`'s own order, the
    // temperature's `NaN` written `null` — the one spelling JSON has for it.
    let temperature_at = channels.iter().position(|c| c.mnemonic == "temperature").unwrap() + 1;
    let pressure_at = channels.iter().position(|c| c.mnemonic == "pressure").unwrap() + 1;
    let rows = rows_of(&set);
    assert_eq!(
        rows[0][temperature_at],
        Value::Null,
        "above the ground the law states nothing: null, not a number"
    );
    assert_eq!(rows[1][temperature_at].as_f64(), Some(13.1));
    assert_eq!(rows[2][pressure_at].as_f64(), Some(2.0));
}

#[test]
fn the_wellbore_reference_is_the_one_witsml_wells_writes_for_the_same_interpretation() {
    // The strongest form of the check: a real `witsml-wells` plan over the
    // very same frame, not a formula copied a second time into this test.
    let wells_bag = frame_bag(Pieces::empty(), 5);
    let wells = Registry::new()
        .plan(
            "witsml-wells",
            &wells_bag,
            &Choices::new().set("existence", "planned"),
            &Identity::derived(),
            &when(),
        )
        .expect("a witsml-wells plan");
    let wellbore = wells
        .objects
        .iter()
        .find(|one| one.qualified_type == "witsml21.Wellbore")
        .expect("a Wellbore");

    let logs = planned(&bag(vec![temperature()]), choices()).expect("a plan");
    let set = the_channel_set(&the_log(&logs));
    let log = the_log(&logs);
    assert_eq!(
        log.wellbore.as_ref().map(|w| w.uuid.as_str()),
        Some(wellbore.uuid.as_str()),
        "the Log names the same Wellbore witsml-wells writes"
    );
    assert_eq!(
        set.wellbore.as_ref().map(|w| w.uuid.as_str()),
        Some(wellbore.uuid.as_str())
    );
    assert_eq!(
        channels_of(&set)[0].wellbore.as_ref().map(|w| w.uuid.as_str()),
        Some(wellbore.uuid.as_str())
    );
    assert_eq!(
        log.wellbore.as_ref().map(|w| w.qualified_type.as_str()),
        Some("witsml21.Wellbore")
    );
}

#[test]
fn same_inputs_make_the_same_uuids() {
    // To break: a uuid drawn from a counter rather than from the content, so
    // a second run over the same package writes a second Log.
    let first = planned(&bag(vec![temperature(), pressure()]), choices()).expect("a plan");
    let again = planned(&bag(vec![temperature(), pressure()]), choices()).expect("a plan");
    let log_uuid = |plan: &Plan| {
        plan.objects
            .iter()
            .find(|one| one.qualified_type == "witsml21.Log")
            .unwrap()
            .uuid
            .clone()
    };
    assert_eq!(log_uuid(&first), log_uuid(&again));

    let channel_uuids = |plan: &Plan| -> Vec<String> {
        let set = the_channel_set(&the_log(plan));
        channels_of(&set).into_iter().map(|c| c.uuid).collect()
    };
    assert_eq!(channel_uuids(&first), channel_uuids(&again));
    assert_eq!(
        the_channel_set(&the_log(&first)).uuid,
        the_channel_set(&the_log(&again)).uuid
    );

    // And a changed value writes a second object — the same rule
    // `properties-from-table` and `well-logs` hold: same content, same uuid;
    // different content, a second one, never a silent rewrite.
    let changed = property(
        TEMPERATURE_PROPERTY,
        "GTH-1 — temperature",
        TEMPERATURE_KIND,
        "temperature",
        "degC",
        vec![f64::NAN, 99.0, 13.4, 13.7, 14.0],
    );
    let moved = planned(&bag(vec![changed, pressure()]), choices()).expect("a plan");
    assert_ne!(
        log_uuid(&first),
        log_uuid(&moved),
        "a corrected value writes a second Log, not the first rewritten"
    );
}

#[test]
fn no_frame_is_refused_by_name() {
    let why = planned(&Pieces::empty(), choices()).expect_err("no frame at all");
    assert!(why.contains("frames"), "{why}");
}

#[test]
fn a_frame_with_no_usable_property_is_refused() {
    // Given, but named for a different frame: `witsml-logs` must not write a
    // Log with no channel.
    let mut elsewhere = property(
        TEMPERATURE_PROPERTY,
        "elsewhere — temperature",
        TEMPERATURE_KIND,
        "temperature",
        "degC",
        vec![1.0; 5],
    );
    if let Some(facts) = elsewhere.property.as_mut() {
        facts.supporting = Some(Ref::new(
            "99999999-0000-4000-8000-000000000001",
            "resqml22.WellboreFrameRepresentation",
            "another frame",
        ));
    }
    let why = planned(&bag(vec![elsewhere]), choices()).expect_err("a property that hangs on no given frame");
    assert!(why.contains("frames"), "{why}");
}

#[test]
fn a_property_of_the_wrong_shape_is_set_aside_and_counted_not_silently_dropped() {
    // To break: the frame ending up with no Log at all because the bad
    // property was refused instead of set aside, or the good one lost with it.
    let mut on_intervals = temperature();
    if let Some(facts) = on_intervals.property.as_mut() {
        facts.indexable = "intervals".to_string();
    }
    let plan = planned(&bag(vec![on_intervals, pressure()]), choices())
        .expect("the good property still makes a Log");
    let set = the_channel_set(&the_log(&plan));
    assert_eq!(
        channels_of(&set).len(),
        1,
        "only the property this operation can read becomes a channel"
    );
    assert_eq!(channels_of(&set)[0].mnemonic, "pressure");
    assert!(
        !plan.dropped.is_empty(),
        "the set-aside property is counted, not silently absorbed"
    );
    assert!(
        plan.dropped
            .iter()
            .any(|one| one.why.contains("GTH-1 — temperature")),
        "{:?}",
        plan.dropped
    );
}

#[test]
fn no_logging_company_is_refused_by_name() {
    let why = planned(&bag(vec![temperature()]), Choices::new()).expect_err("loggingCompany is required");
    assert!(why.contains("loggingCompany"), "{why}");
}

#[test]
fn every_object_this_plan_writes_is_a_log_and_reads_back() {
    let plan = planned(&bag(vec![temperature(), pressure()]), choices()).expect("a plan");
    for object in &plan.objects {
        assert_eq!(
            object.qualified_type, "witsml21.Log",
            "witsml-logs writes Logs and nothing else, nested Channels included"
        );
        let log: witsml::Log = read(object);
        assert_eq!(log.uuid, object.uuid);
    }
}
