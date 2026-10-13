//! **A reference resolves exactly, and when nothing answers it names the
//! candidates** - lot I0 of the identity review. The requirement:
//!
//! > In validation, the checks on DORs should be able to detect that a
//! > reference is missing because the uri generated from the DOR […] has the
//! > wrong version or content type. That would make it possible to say
//! > 'uuid found but type or version should be one of the following: ' with
//! > the list of uris of the objects that have the right uuid but not the
//! > right type or version number.
//!
//! The package is fabricated in memory: `X` held in versions 1 and 2, and
//! horizons whose `InterpretedFeature` names it in every wrong way there is.
//!
//! What would have to break: resolving by uuid alone again (the first entry
//! answers every reference, so a reference to version 3 is compared with
//! version 1 and the finding lists one uri - or none).
//!
//! The DOT export is checked here too: it cut the uri by hand and drew two
//! versions as one box.

use std::io::Cursor;
use std::sync::Arc;

use energyml_core::registry::{DynObject, ReadMode};
use energyml_core::value::OwnedValue;
use energyml_storage::EpcStore;
use energyml_storage::graph::{self, RefIssue, RefIssueKind};

const RESQML: &str = "http://www.energistics.org/energyml/data/resqmlv2";
const EML: &str = "http://www.energistics.org/energyml/data/commonv2";
const X: &str = "7e300000-0000-4000-8000-000000000001";
const NOBODY: &str = "7e300000-0000-4000-8000-0000000000ff";
const CT_FEATURE: &str = "application/x-resqml+xml;version=2.2;type=BoundaryFeature";
const CT_HORIZON: &str = "application/x-resqml+xml;version=2.2;type=HorizonInterpretation";
const FEATURE: &str = "resqml22.BoundaryFeature";

const H_EXACT: &str = "7e300000-0000-4000-8000-0000000000a1";
const H_V3: &str = "7e300000-0000-4000-8000-0000000000a2";
const H_TYPE: &str = "7e300000-0000-4000-8000-0000000000a3";
const H_BARE: &str = "7e300000-0000-4000-8000-0000000000a4";
const H_RELEASE: &str = "7e300000-0000-4000-8000-0000000000a5";
const H_LOST: &str = "7e300000-0000-4000-8000-0000000000a6";

fn read(content_type: &str, xml: String) -> Box<dyn DynObject> {
    energyml_schemas::all()
        .read_xml_as(content_type, xml.as_bytes(), ReadMode::Tolerant)
        .expect("readable")
        .0
}

fn feature(version: &str, title: &str) -> Box<dyn DynObject> {
    read(
        CT_FEATURE,
        format!(
            "<resqml:BoundaryFeature xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" uuid=\"{X}\" \
             schemaVersion=\"2.2\" objectVersion=\"{version}\">\
             <eml:Citation><eml:Title>{title}</eml:Title><eml:Originator>test</eml:Originator>\
             <eml:Creation>2026-10-05T00:00:00Z</eml:Creation><eml:Format>test</eml:Format></eml:Citation>\
             <resqml:IsWellKnown>false</resqml:IsWellKnown></resqml:BoundaryFeature>"
        ),
    )
}

/// A horizon whose `InterpretedFeature` names `target`, at `version` when given,
/// declaring `declared` as its type.
fn horizon(uuid: &str, target: &str, version: Option<&str>, declared: &str) -> Box<dyn DynObject> {
    let version = version
        .map(|v| format!("<eml:ObjectVersion>{v}</eml:ObjectVersion>"))
        .unwrap_or_default();
    read(
        CT_HORIZON,
        format!(
            "<resqml:HorizonInterpretation xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" uuid=\"{uuid}\" \
             schemaVersion=\"2.2\"><eml:Citation><eml:Title>a horizon</eml:Title>\
             <eml:Originator>test</eml:Originator><eml:Creation>2026-10-05T00:00:00Z</eml:Creation>\
             <eml:Format>test</eml:Format></eml:Citation><resqml:Domain>depth</resqml:Domain>\
             <resqml:InterpretedFeature><eml:Uuid>{target}</eml:Uuid>{version}\
             <eml:QualifiedType>{declared}</eml:QualifiedType><eml:Title>a boundary</eml:Title>\
             </resqml:InterpretedFeature></resqml:HorizonInterpretation>"
        ),
    )
}

fn package() -> EpcStore {
    let empty = energyml_storage::extract::empty_package(Some(0)).expect("an empty package");
    let mut store = EpcStore::from_reader(
        Box::new(Cursor::new(empty)),
        "refs.epc",
        Arc::new(energyml_schemas::all()),
    )
    .expect("opens");
    store.put(feature("1", "X, first")).expect("v1");
    store.put_as_new_version(feature("2", "X, second")).expect("v2");
    store
        .put(horizon(H_EXACT, X, Some("2"), FEATURE))
        .expect("names v2, rightly");
    store
        .put(horizon(H_V3, X, Some("3"), FEATURE))
        .expect("names v3, which is not held");
    store
        .put(horizon(H_TYPE, X, Some("2"), "resqml22.HorizonInterpretation"))
        .expect("names X as a horizon");
    store
        .put(horizon(H_BARE, X, None, FEATURE))
        .expect("names X without a version");
    store
        .put(horizon(H_RELEASE, X, None, "resqml20.obj_BoundaryFeature"))
        .expect("names X in another release");
    store
        .put(horizon(H_LOST, NOBODY, None, FEATURE))
        .expect("names nobody");
    store
}

fn findings(store: &EpcStore) -> Vec<RefIssue> {
    let read = store.read_objects(None).0;
    graph::check_all_references(
        read.iter()
            .map(|(summary, object)| (summary, object.as_reflect())),
        &|uuid| store.find_by_uuid(uuid),
    )
}

/// The two entries of `X`, by the uri that names each alone.
fn x_uris(store: &EpcStore) -> Vec<String> {
    let mut uris: Vec<String> = store.summaries_of(X).into_iter().map(|one| one.uri).collect();
    uris.sort();
    uris
}

/// What `horizon` is told, minus the title copies (`TitleMismatch` is another
/// question).
fn about<'a>(all: &'a [RefIssue], horizon: &str) -> Vec<&'a RefIssue> {
    all.iter()
        .filter(|one| one.source.contains(horizon) && !matches!(one.kind, RefIssueKind::TitleMismatch { .. }))
        .collect()
}

#[test]
fn a_reference_to_a_version_nobody_holds_lists_the_two_uris() {
    let store = package();
    let uris = x_uris(&store);
    assert_eq!(uris.len(), 2);
    assert!(
        uris.iter().all(|one| one.contains("version=")),
        "each carries its version: {uris:?}"
    );

    let all = findings(&store);
    let told = about(&all, H_V3);
    assert_eq!(told.len(), 1, "one finding: {told:?}");
    let issue = told[0];
    match &issue.kind {
        RefIssueKind::ObjectVersionMismatch {
            referenced,
            candidates,
            ..
        } => {
            assert_eq!(referenced.as_deref(), Some("3"));
            assert_eq!(
                candidates, &uris,
                "exactly the two entries of the uuid, by full uri"
            );
        }
        other => panic!("expected a version finding, got {other:?}"),
    }
    assert!(issue.is_error(), "a named version that is absent is an error");
    let text = issue.to_string();
    assert!(
        text.contains("uuid found but type or version should be one of the following"),
        "{text}"
    );
    for uri in &uris {
        assert!(text.contains(uri.as_str()), "{text} names {uri}");
    }
}

#[test]
fn a_reference_to_a_neighbouring_type_gives_the_type_finding_with_its_candidates() {
    let store = package();
    let uris = x_uris(&store);
    let all = findings(&store);
    let told = about(&all, H_TYPE);
    assert_eq!(told.len(), 1, "one finding: {told:?}");
    match &told[0].kind {
        RefIssueKind::ConceptMismatch { candidates, .. } => assert_eq!(candidates, &uris),
        other => panic!("expected the concept finding, got {other:?}"),
    }
    assert!(told[0].is_error(), "another type is an error");
    assert!(told[0].to_string().contains("should be one of the following"));
}

#[test]
fn another_release_is_a_warning_and_names_the_candidates_too() {
    let store = package();
    let uris = x_uris(&store);
    let all = findings(&store);
    let told = about(&all, H_RELEASE);
    assert_eq!(told.len(), 1, "one finding: {told:?}");
    match &told[0].kind {
        RefIssueKind::VersionMismatch { candidates, .. } => assert_eq!(candidates, &uris),
        other => panic!("expected the release finding, got {other:?}"),
    }
    assert!(!told[0].is_error(), "a wrong release is not a wrong type");
}

#[test]
fn a_reference_that_names_no_version_of_an_object_held_twice_is_a_warning_with_both() {
    let store = package();
    let uris = x_uris(&store);
    let all = findings(&store);
    let told = about(&all, H_BARE);
    assert_eq!(told.len(), 1, "one finding: {told:?}");
    match &told[0].kind {
        RefIssueKind::ObjectVersionMismatch {
            referenced: None,
            candidates,
            ..
        } => assert_eq!(candidates, &uris),
        other => panic!("expected a warning naming both, got {other:?}"),
    }
    assert!(!told[0].is_error());
}

#[test]
fn the_right_identity_is_told_nothing_and_a_missing_uuid_stays_dangling() {
    let store = package();
    let all = findings(&store);
    assert!(
        about(&all, H_EXACT).is_empty(),
        "uuid, type and version all right: {:?}",
        about(&all, H_EXACT)
    );
    let lost = about(&all, H_LOST);
    assert_eq!(lost.len(), 1);
    assert_eq!(
        lost[0].kind,
        RefIssueKind::Dangling,
        "the uuid itself is absent: no candidate to name"
    );
}

/// An entry the caller could not read is still a candidate: the batch holds
/// version 1 only, the listing holds both, and the finding names both.
#[test]
fn an_entry_that_was_not_read_is_still_named() {
    let store = package();
    let uris = x_uris(&store);
    let read: Vec<_> = store
        .read_objects(None)
        .0
        .into_iter()
        .filter(|(summary, _)| !summary.uri.contains("version='2'"))
        .collect();
    let listing = store.resources();
    let all = graph::check_all_references_with(
        read.iter()
            .map(|(summary, object)| (summary, object.as_reflect())),
        &|uuid| {
            listing
                .iter()
                .filter(|one| one.uuid.eq_ignore_ascii_case(uuid))
                .collect()
        },
    );
    let told = about(&all, H_V3);
    assert_eq!(told.len(), 1, "{told:?}");
    match &told[0].kind {
        RefIssueKind::ObjectVersionMismatch { candidates, .. } => {
            assert_eq!(candidates, &uris, "the unread entry is named beside the read one")
        }
        other => panic!("expected a version finding, got {other:?}"),
    }
}

#[test]
fn the_dot_export_names_both_versions() {
    let store = package();
    let uris = x_uris(&store);
    let read = store.read_objects(None).0;
    let graph = graph::build(
        read.iter()
            .map(|(summary, object)| (summary, object.as_reflect())),
    );
    let dot = graph.to_dot();
    for uri in &uris {
        assert!(
            dot.contains(&format!("\"{uri}\" [label=")),
            "{uri} is a node of its own:\n{dot}"
        );
    }
    // An edge lands on the version it names, not on a box shared by both.
    let exact = format!(
        "-> \"{}\"",
        uris.iter().find(|one| one.contains("version='2'")).expect("v2")
    );
    assert!(dot.contains(&exact), "{dot}");
    let boxes = dot
        .lines()
        .filter(|line| line.contains("[label=\"X, ") && !line.contains("->"))
        .count();
    assert_eq!(boxes, 2, "two versions, two boxes:\n{dot}");
}

#[test]
fn an_edit_of_the_object_version_is_journaled_under_the_uri_it_now_has() {
    let mut store = package();
    let uris = x_uris(&store);
    let second = uris
        .iter()
        .find(|one| one.contains("version='2'"))
        .expect("v2")
        .clone();
    let from = store.revision();
    let object = store.edit_at(&second, "objectVersion").expect("borrowed");
    energyml_core::path::set(object, "objectVersion", OwnedValue::Str("3".to_string())).expect("set");
    let energyml_storage::epc::revision::Since::Exact(change) = store.changes_since(from) else {
        panic!("a handful of lines")
    };
    let named: Vec<&str> = change
        .objects
        .iter()
        .filter_map(|one| one.uri.as_deref())
        .collect();
    assert!(
        named.iter().any(|one| one.contains("version='3'")),
        "the line names the identity the object states now, not the one it was born with: {named:?}"
    );
    assert!(!named.contains(&second.as_str()), "{named:?}");
}
