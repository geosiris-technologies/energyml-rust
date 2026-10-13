//! The colour of an object **at a version** — the walk of the colour is keyed by
//! the object's uri, not by its uuid (ADR-0003).
//!
//! A bare uuid is accepted only at the library's entry, for the user's convenience; internally only full uris are used, which avoids sources of bugs. A
//! package may hold one representation in two `objectVersion`s (the identity of
//! an entry is (uuid, type, version), ADR-0010), and a
//! `GraphicalInformationSet` entry names the object **with** the version of the
//! reference (`eml:TargetObject` carries an `ObjectVersion`): it paints that
//! version and no other.
//!
//! The packages are built in memory — the rule being tested is the project's, not
//! a corpus's. What would have to break: the targets read by uuid alone, the
//! walk's key a uuid, or the ladder's rung found by uuid — the two versions would
//! then share one colour, the last entry's.

use std::io::Cursor;
use std::sync::Arc;

use energyml_core::any_xml::AnyXml;
use energyml_core::registry::DynObject;
use energyml_domain::color::{Cascade, GraphicalSet, Paint, Request, Scope};
use energyml_domain::color_walks::{Ask, ColourCache};
use energyml_domain::package_index::PackageIndex;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_storage::EpcStore;

use crate::inherit::{citation, u};

const REPRESENTATION: u32 = 1;
const SET: u32 = 9;

fn store() -> EpcStore {
    EpcStore::from_reader(
        Box::new(Cursor::new(energyml_storage::extract::empty_package(None).expect("an empty package"))),
        "color-versions.epc",
        Arc::new(energyml_schemas::all()),
    )
    .expect("it opens")
}

fn representation(version: &str) -> Box<dyn DynObject> {
    Box::new(resqml22::WellboreTrajectoryRepresentation {
        uuid: u(REPRESENTATION),
        schema_version: "2.2".into(),
        object_version: Some(version.to_string()),
        citation: citation(&format!("representation, version {version}")),
        ..Default::default()
    })
}

/// A `DefaultGraphicalInformation` of the whole object, in `hue`, whose target
/// names the representation at `version` (or at none).
fn entry(version: Option<&str>, hue: f64) -> AnyXml {
    let version = version.map(|v| format!("<eml23:ObjectVersion>{v}</eml23:ObjectVersion>")).unwrap_or_default();
    let content = format!(
        "<eml23:TargetObject><eml23:Uuid>{}</eml23:Uuid>{version}\
         <eml23:QualifiedType>resqml22.WellboreTrajectoryRepresentation</eml23:QualifiedType>\
         <eml23:Title>t</eml23:Title></eml23:TargetObject>\
         <resqml22:ViewerKind>3d</resqml22:ViewerKind>\
         <resqml22:IndexableElementInfo xsi:type=\"resqml22:GraphicalInformationForWholeObject\">\
           <resqml22:IsVisible>true</resqml22:IsVisible>\
           <resqml22:ConstantColor><resqml22:Alpha>1</resqml22:Alpha><resqml22:Hue>{hue}</resqml22:Hue>\
           <resqml22:Saturation>1</resqml22:Saturation><resqml22:Value>1</resqml22:Value></resqml22:ConstantColor>\
         </resqml22:IndexableElementInfo>",
        u(REPRESENTATION)
    );
    AnyXml {
        attributes: vec![("xsi:type".to_string(), "resqml22:DefaultGraphicalInformation".to_string())],
        content,
        namespaces: Vec::new(),
    }
}

/// The package: the representation in versions 1 and 2, and a set that colours
/// version 1 red (hue 0) and version 2 green (hue 120).
fn package(entries: Vec<AnyXml>) -> (EpcStore, Vec<String>) {
    let mut store = store();
    store.put(representation("1")).expect("version 1");
    store.put_as_new_version(representation("2")).expect("version 2");
    store
        .put(Box::new(eml23::GraphicalInformationSet {
            uuid: u(SET),
            schema_version: "2.3".into(),
            citation: citation("the set"),
            graphical_information: entries,
            ..Default::default()
        }))
        .expect("the set");
    let mut uris: Vec<String> = store
        .resources()
        .iter()
        .filter(|summary| summary.uuid == u(REPRESENTATION))
        .map(|summary| summary.uri.clone())
        .collect();
    uris.sort();
    (store, uris)
}

fn the_set(store: &EpcStore) -> GraphicalSet {
    let object = store.get_object(&u(SET)).expect("the set reads");
    GraphicalSet::read(object.as_reflect())
}

fn hex(paint: &Paint) -> String {
    match paint {
        Paint::Constant(rgba) => rgba.to_hex(),
        Paint::Mapped { .. } => panic!("a constant colour was written"),
    }
}

/// An entry names its target **at a version**: two versions of one
/// representation have two colours of the file, and each ladder names the rung
/// of **its own** version.
///
/// Fails on the walk of before 05/10 (keyed by uuid, `Entry.targets` the uuids):
/// the second entry then colours both versions, and the rung of both is the
/// first one listed.
#[test]
fn two_versions_of_one_representation_have_two_colours_of_the_file() {
    let (store, uris) = package(vec![entry(Some("1"), 0.0), entry(Some("2"), 120.0)]);
    assert_eq!(uris.len(), 2, "the package holds the representation twice: {uris:?}");
    assert!(uris[0].contains("version='1'") && uris[1].contains("version='2'"), "{uris:?}");
    let index = PackageIndex::build(&store, store.revision());
    let set = the_set(&store);
    assert_eq!(set.entries.iter().map(|one| one.addresses().len()).collect::<Vec<_>>(), vec![1, 1]);
    assert!(
        set.entries[0].addresses()[0].contains("version='1'"),
        "the entry keeps the version its reference names: {:?}",
        set.entries[0].addresses()
    );

    let cascade = Cascade::new(&set);
    let requests: Vec<Request<'_>> = uris
        .iter()
        .map(|uri| Request { representation: uri, property: None, kind_chain: &[], element: Scope::WholeObject })
        .collect();
    let ladders = cascade.ladders(&index, &store, &requests).expect("the ladders");

    let colours: Vec<String> = ladders
        .iter()
        .map(|ladder| hex(&ladder.walked.resolution.as_ref().expect("the file colours it").value.paint))
        .collect();
    assert_eq!(colours, vec!["#ff0000ff".to_string(), "#00ff00ff".to_string()], "one colour per version");

    // The rung that carried is the one of the version asked, and its uri says so.
    for (ladder, uri) in ladders.iter().zip(&uris) {
        let chosen = ladder.chosen.expect("a rung answered");
        assert_eq!(&ladder.steps[chosen].uri, uri);
        let resolution = ladder.walked.resolution.as_ref().unwrap();
        assert_eq!(&resolution.carrier_uri, uri);
        assert_eq!(&resolution.value.found_on_uri, uri);
    }
}

/// The cache keeps **one walk per uri**: two versions are two walks, and
/// disturbing one version leaves the other exact.
#[test]
fn the_cache_keeps_a_walk_per_version_and_disturbs_one_alone() {
    let (store, uris) = package(vec![entry(Some("1"), 0.0), entry(Some("2"), 120.0)]);
    let index = PackageIndex::build(&store, store.revision());
    let set = the_set(&store);
    let cascade = Cascade::new(&set);
    let scopes = [Scope::WholeObject];
    let asks: Vec<Ask<'_>> = uris.iter().map(|uri| Ask { uri, scopes: &scopes }).collect();
    let mut cache = ColourCache::new();
    cache.resolve(&cascade, &index, &store, &asks);
    assert_eq!(cache.len(), 2, "a walk per version");
    assert!(cache.holds(&uris[0]) && cache.holds(&uris[1]));

    assert_eq!(cache.disturb(std::slice::from_ref(&uris[0])), 1, "only the walk that looked at version 1");
    assert!(!cache.holds(&uris[0]) && cache.holds(&uris[1]), "version 2 is exact still");

    // A bare uuid is the entry adapter, and it names every version.
    assert_eq!(cache.disturb(&[u(REPRESENTATION)]), 1);
    assert!(cache.is_empty());
}

/// An entry whose reference names **no version** contradicts none: it paints
/// every version (a short reference is accepted when nothing contradicts it) —
/// and a version that names another is left alone.
#[test]
fn an_entry_naming_no_version_paints_every_version_and_one_naming_another_paints_none_of_them() {
    let (store, uris) = package(vec![entry(None, 60.0), entry(Some("3"), 240.0)]);
    let index = PackageIndex::build(&store, store.revision());
    let set = the_set(&store);
    let cascade = Cascade::new(&set);
    for uri in &uris {
        let walked = cascade.inherited(&index, &store, uri).expect("it walks");
        let found = walked.resolution.expect("the version-less entry paints it");
        assert_eq!(hex(&found.value.paint), "#ffff00ff", "hue 60 for both, not the entry naming version 3");
        assert_eq!(found.value.entry, 0);
    }
}
