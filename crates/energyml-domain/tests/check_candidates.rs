//! **`energyml check` resolves a reference by uuid, type and version, and
//! names the candidates when none answers** (ADR-0003).
//!
//! `check::of_store` recognised a target by its uuid alone — *the first version
//! wins* — so a reference to version 3 of an object held in versions 1 and 2 was
//! compared with version 1. The expected message: 'uuid found but type or version should be one of the following: ' followed by the list of uris of objects that have the right uuid but not the right type or version number.
//!
//! The package is fabricated in memory. What would have to break: the first
//! entry answering every reference again — the finding then lists one uri.

use std::io::Cursor;
use std::sync::Arc;

use energyml_core::registry::{DynObject, ReadMode};
use energyml_domain::check::{self, Options};
use energyml_storage::EpcStore;
use energyml_storage::graph::RefIssueKind;

const RESQML: &str = "http://www.energistics.org/energyml/data/resqmlv2";
const EML: &str = "http://www.energistics.org/energyml/data/commonv2";
const X: &str = "7e400000-0000-4000-8000-000000000001";
const H: &str = "7e400000-0000-4000-8000-0000000000a1";
const CT_FEATURE: &str = "application/x-resqml+xml;version=2.2;type=BoundaryFeature";
const CT_HORIZON: &str = "application/x-resqml+xml;version=2.2;type=HorizonInterpretation";

fn read(content_type: &str, xml: String) -> Box<dyn DynObject> {
    energyml_schemas::all().read_xml_as(content_type, xml.as_bytes(), ReadMode::Tolerant).expect("readable").0
}

fn feature(version: &str) -> Box<dyn DynObject> {
    read(
        CT_FEATURE,
        format!(
            "<resqml:BoundaryFeature xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" uuid=\"{X}\" \
             schemaVersion=\"2.2\" objectVersion=\"{version}\"><eml:Citation><eml:Title>a surface</eml:Title>\
             <eml:Originator>test</eml:Originator><eml:Creation>2026-10-05T00:00:00Z</eml:Creation>\
             <eml:Format>test</eml:Format></eml:Citation><resqml:IsWellKnown>false</resqml:IsWellKnown>\
             </resqml:BoundaryFeature>"
        ),
    )
}

/// A horizon naming `X` at version 3 — which the package does not hold.
fn horizon() -> Box<dyn DynObject> {
    read(
        CT_HORIZON,
        format!(
            "<resqml:HorizonInterpretation xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" uuid=\"{H}\" \
             schemaVersion=\"2.2\"><eml:Citation><eml:Title>a horizon</eml:Title>\
             <eml:Originator>test</eml:Originator><eml:Creation>2026-10-05T00:00:00Z</eml:Creation>\
             <eml:Format>test</eml:Format></eml:Citation><resqml:Domain>depth</resqml:Domain>\
             <resqml:InterpretedFeature><eml:Uuid>{X}</eml:Uuid><eml:ObjectVersion>3</eml:ObjectVersion>\
             <eml:QualifiedType>resqml22.BoundaryFeature</eml:QualifiedType><eml:Title>a surface</eml:Title>\
             </resqml:InterpretedFeature></resqml:HorizonInterpretation>"
        ),
    )
}

#[test]
fn the_report_names_both_versions_when_neither_is_the_one_named() {
    let empty = energyml_storage::extract::empty_package(Some(0)).expect("an empty package");
    let mut store =
        EpcStore::from_reader(Box::new(Cursor::new(empty)), "check.epc", Arc::new(energyml_schemas::all())).expect("opens");
    store.put(feature("1")).expect("v1");
    store.put_as_new_version(feature("2")).expect("v2");
    store.put(horizon()).expect("the horizon");
    let mut uris: Vec<String> = store.summaries_of(X).into_iter().map(|one| one.uri).collect();
    uris.sort();
    assert_eq!(uris.len(), 2);

    let report = check::of_store(&store, &Options { schema: false, rules: false, extra_rules: Vec::new() });
    let told: Vec<_> = report.references.iter().filter(|one| matches!(one.kind, RefIssueKind::ObjectVersionMismatch { .. })).collect();
    assert_eq!(told.len(), 1, "one finding: {:?}", report.references);
    match &told[0].kind {
        RefIssueKind::ObjectVersionMismatch { referenced, candidates, .. } => {
            assert_eq!(referenced.as_deref(), Some("3"));
            assert_eq!(candidates, &uris, "both entries of the uuid, by full uri");
        }
        _ => unreachable!(),
    }
    assert!(told[0].is_error());
    assert!(told[0].to_string().contains("should be one of the following"));
}
