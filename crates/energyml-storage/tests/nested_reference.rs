//! A reference to a data object **nested** in another part resolves to that
//! part.
//!
//! PRODML flow-test data references a WITSML channel set and its channels
//! (`AbstractFlowTestData.ChannelSet`, `TimeChannel`, FlowTestActivity.xsd
//! l. 95–103); the usage guide puts the values there: *"Channel is a WITSML
//! object; Channels are collected into Channel Sets and the Channel Set is where
//! the actual values are stored"* (PRODML Technical Usage Guide v2.3 §13.6
//! p. 118). A WITSML `Log` holds its `ChannelSet` and its `Channel`s inline, each
//! with a uuid. Such a reference resolves to the part holding
//! the nested object — the writer links the two parts, and neither the
//! reference check nor the `.rels` check calls it dangling.

use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_storage::extract::empty_package;
use energyml_storage::graph::{self, Related};
use energyml_storage::{EpcStore, SaveOptions};

const LOG: &str = "0c000000-0000-4000-8000-0000000000c1";
const SET: &str = "0c000000-0000-4000-8000-0000000000c2";
const TIME: &str = "0c000000-0000-4000-8000-0000000000c3";
const PRESSURE: &str = "0c000000-0000-4000-8000-0000000000c4";
const TEST: &str = "0c000000-0000-4000-8000-0000000000c5";

fn citation(title: &str) -> String {
    format!(
        "<eml:Citation><eml:Title>{title}</eml:Title><eml:Originator>test</eml:Originator><eml:Creation>2026-09-27T00:00:00Z</eml:Creation><eml:Format>test</eml:Format></eml:Citation>"
    )
}

fn channel(uuid: &str, mnemonic: &str) -> String {
    format!(
        r#"<witsml:Channel uuid="{uuid}" schemaVersion="2.1">{}<witsml:Mnemonic>{mnemonic}</witsml:Mnemonic></witsml:Channel>"#,
        citation(mnemonic)
    )
}

fn log() -> String {
    format!(
        r#"<witsml:Log xmlns:witsml="http://www.energistics.org/energyml/data/witsmlv2" xmlns:eml="http://www.energistics.org/energyml/data/commonv2" uuid="{LOG}" schemaVersion="2.1">{}<witsml:ChannelSet uuid="{SET}" schemaVersion="2.1">{}{}{}</witsml:ChannelSet></witsml:Log>"#,
        citation("GTH-2 — injection test log"),
        citation("GTH-2 — injection test channels"),
        channel(TIME, "TIME"),
        channel(PRESSURE, "BHP"),
    )
}

fn dor(element: &str, uuid: &str, qualified: &str, title: &str) -> String {
    format!(
        "<prodml:{element}><eml:Uuid>{uuid}</eml:Uuid><eml:QualifiedType>{qualified}</eml:QualifiedType><eml:Title>{title}</eml:Title></prodml:{element}>"
    )
}

fn injection_test() -> String {
    format!(
        r#"<prodml:FlowTestActivity xmlns:prodml="http://www.energistics.org/energyml/data/prodmlv2" xmlns:eml="http://www.energistics.org/energyml/data/commonv2" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="prodml:InjectionFlowTest" uuid="{TEST}" schemaVersion="2.3">{}<prodml:IntervalMeasurementSet><prodml:Location/><prodml:MeasuredPressure>{}{}{}</prodml:MeasuredPressure></prodml:IntervalMeasurementSet></prodml:FlowTestActivity>"#,
        citation("GTH-2 — injection test"),
        dor(
            "ChannelSet",
            SET,
            "witsml21.ChannelSet",
            "GTH-2 — injection test channels"
        ),
        dor("TimeChannel", TIME, "witsml21.Channel", "TIME"),
        dor("PressureChannel", PRESSURE, "witsml21.Channel", "BHP"),
    )
}

fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

/// The package, saved and opened again: what a reader of the file sees.
fn saved() -> (EpcStore, std::path::PathBuf) {
    let registry = registry();
    let mut store = EpcStore::from_reader(
        Box::new(std::io::Cursor::new(empty_package(None).unwrap())),
        "nested.epc",
        registry.clone(),
    )
    .expect("an empty package");
    store
        .put(
            registry
                .read_xml("application/x-witsml+xml;version=2.1;type=Log", log().as_bytes())
                .expect("the log reads"),
        )
        .unwrap();
    store
        .put(
            registry
                .read_xml(
                    "application/x-prodml+xml;version=2.3;type=InjectionFlowTest",
                    injection_test().as_bytes(),
                )
                .expect("the test reads"),
        )
        .unwrap();
    let dir = std::env::temp_dir().join(format!("energyml-nested-reference-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("nested.epc");
    let said = store.save_as(&path, &SaveOptions::default()).expect("it saves");
    assert!(
        said.iter()
            .all(|one| !one.message.contains("not in this package")),
        "the writer found the nested targets: {said:?}"
    );
    (EpcStore::open(&path, registry).expect("it reopens"), dir)
}

#[test]
fn the_nested_objects_of_a_log_are_its_channel_set_and_channels() {
    let object = registry()
        .read_xml("application/x-witsml+xml;version=2.1;type=Log", log().as_bytes())
        .unwrap();
    let mut found: Vec<String> = energyml_core::identity::nested_objects(object.as_reflect())
        .into_iter()
        .map(|one| format!("{} {}", one.key.qualified_type(), one.key.uuid))
        .collect();
    found.sort();
    assert_eq!(
        found,
        [
            format!("witsml21.Channel {TIME}"),
            format!("witsml21.Channel {PRESSURE}"),
            format!("witsml21.ChannelSet {SET}")
        ]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
    );
}

#[test]
fn a_reference_to_a_nested_channel_is_no_dangling_reference() {
    let (store, dir) = saved();
    let summaries: Vec<_> = store.resources();
    let objects: Vec<_> = summaries
        .iter()
        .map(|one| store.get_object(&one.uri).expect("it reads"))
        .collect();
    let pairs = summaries
        .iter()
        .zip(&objects)
        .map(|(summary, object)| (summary, object.as_reflect()));
    let known = |uuid: &str| summaries.iter().find(|one| one.uuid.eq_ignore_ascii_case(uuid));
    let issues = graph::check_all_references(pairs, &known);
    assert!(issues.is_empty(), "{issues:#?}");

    // The writer linked the test to the part holding the channels, and the
    // `.rels` check expects exactly that.
    let test = summaries.iter().find(|one| one.uuid == TEST).unwrap();
    let log = summaries.iter().find(|one| one.uuid == LOG).unwrap();
    let rels = store.relationships(&test.locator).expect("its .rels");
    assert!(
        rels.iter()
            .any(|one| one.short_kind() == "destinationObject" && one.target.ends_with(&log.locator)),
        "{rels:#?}"
    );
    let all_rels: Vec<_> = summaries
        .iter()
        .map(|one| store.relationships(&one.locator).unwrap_or_default())
        .collect();
    let related: Vec<Related<'_>> = summaries
        .iter()
        .zip(&objects)
        .zip(&all_rels)
        .map(|((summary, object), relationships)| Related {
            summary,
            object: object.as_reflect(),
            relationships,
        })
        .collect();
    let disagreements = graph::check_relationships(&related);
    assert!(disagreements.is_empty(), "{disagreements:#?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_uuid_nested_nowhere_is_still_dangling() {
    // The resolution must not swallow a genuine dangling reference: the check
    // this file relaxes is still the one that finds a missing object.
    let registry = registry();
    let lonely = registry
        .read_xml(
            "application/x-prodml+xml;version=2.3;type=InjectionFlowTest",
            injection_test().as_bytes(),
        )
        .unwrap();
    let summary = energyml_storage::ObjectSummary {
        uri: format!("eml:///prodml23.InjectionFlowTest({TEST})"),
        uuid: TEST.to_string(),
        qualified_type: "prodml23.InjectionFlowTest".to_string(),
        content_type: String::new(),
        locator: "InjectionFlowTest.xml".to_string(),
    };
    let issues = graph::check_all_references([(&summary, lonely.as_reflect())], &|_| None);
    assert_eq!(
        issues.len(),
        3,
        "the channel set and two channels are nowhere: {issues:#?}"
    );
}
