//! **An entry of a package is (uuid, qualified type, `objectVersion`)** (ADR-0010).
//!
//! The store used to keep the first part of a uuid and dropped the others with a
//! warning, so a package holding one object in two versions, or one uuid under
//! two releases, lost all but one of them without saying which. These tests
//! fabricate such packages **by hand** - the zip, the part names, the content
//! types, the `objectVersion` attribute - so that what is read does not come
//! from the writer under test, and then pin the rule: accept the short uri when
//! there is no conflict (or when the object has no version), and the form with
//! the version when there is a conflict.
//!
//! The specification behind them:
//!
//! - Identifier Specification v5.0 §3.4.2 (p. 14): the canonical form with a
//!   version is `eml:///{type}(uuid={uuid},version='{version}')`; §3.4.2.1
//!   (p. 15): a URI without a version, when several exist, *implicitly refers to
//!   the most recent version* - and ETP 1.2 says nothing of how to tell which,
//!   so this store names the candidates instead of choosing one;
//! - EPC v1.1 §3.6 (p. 17): *"A data object XML file MUST be contained in a
//!   unique path"*, and *"If this data object is versioned, the unique path
//!   should contain a directory called 'version_id'"*.
//!
//! (The file `versions.rs` beside this one is about **releases** - one namespace
//! for 2.0.1 and 2.2 - and keeps its name.)

use std::io::{Cursor, Read, Write};
use std::sync::Arc;

use energyml_core::registry::{DynObject, ReadMode, Registry};
use energyml_storage::epc::SaveOptions;
use energyml_storage::epc::revision::{Change, Since, What};
use energyml_storage::{EpcStore, Error};

const RESQML: &str = "http://www.energistics.org/energyml/data/resqmlv2";
const EML: &str = "http://www.energistics.org/energyml/data/commonv2";

/// An object held in versions 1 and 2, and a third, unversioned, elsewhere.
const X: &str = "7c900000-0000-4000-8000-000000000001";
/// One uuid under two releases, neither declaring a version.
const Y: &str = "7c900000-0000-4000-8000-000000000002";
/// An object that declares no version, and one that declares `2`.
const Z: &str = "7c900000-0000-4000-8000-000000000003";
/// Two horizons: `H` names X **at version 2**, `H0` names X without a version.
const H: &str = "7c900000-0000-4000-8000-0000000000a1";
const H0: &str = "7c900000-0000-4000-8000-0000000000a2";

const T22: &str = "resqml22.BoundaryFeature";
const T20: &str = "resqml20.obj_BoundaryFeature";
const CT22: &str = "application/x-resqml+xml;version=2.2;type=BoundaryFeature";
const CT20: &str = "application/x-resqml+xml;version=2.0;type=obj_BoundaryFeature";
const CT_HORIZON: &str = "application/x-resqml+xml;version=2.2;type=HorizonInterpretation";

fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

fn uri22(uuid: &str, version: Option<&str>) -> String {
    match version {
        Some(version) => format!("eml:///{T22}(uuid={uuid},version='{version}')"),
        None => format!("eml:///{T22}({uuid})"),
    }
}

fn uri20(uuid: &str) -> String {
    format!("eml:///{T20}({uuid})")
}

fn feature22(uuid: &str, version: Option<&str>, title: &str) -> String {
    let version = version
        .map(|v| format!(" objectVersion=\"{v}\""))
        .unwrap_or_default();
    format!(
        "<resqml:BoundaryFeature xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" \
         uuid=\"{uuid}\" schemaVersion=\"2.2\"{version}>\
         <eml:Citation><eml:Title>{title}</eml:Title>\
         <eml:Originator>test</eml:Originator>\
         <eml:Creation>2026-10-05T00:00:00Z</eml:Creation>\
         <eml:Format>test</eml:Format></eml:Citation>\
         <resqml:IsWellKnown>false</resqml:IsWellKnown>\
         </resqml:BoundaryFeature>"
    )
}

/// RESQML 2.0.1 keeps the version in the citation, `VersionString`.
fn feature20(uuid: &str, title: &str) -> String {
    format!(
        "<resqml2:obj_BoundaryFeature xmlns:resqml2=\"{RESQML}\" xmlns:eml=\"{EML}\" \
         uuid=\"{uuid}\" schemaVersion=\"2.0\">\
         <eml:Citation><eml:Title>{title}</eml:Title>\
         <eml:Originator>test</eml:Originator>\
         <eml:Creation>2026-10-05T00:00:00Z</eml:Creation>\
         <eml:Format>test</eml:Format></eml:Citation>\
         </resqml2:obj_BoundaryFeature>"
    )
}

/// A horizon interpretation naming `feature`, at `version` when given. The
/// reference's elements in the order eml 2.3 writes them.
fn horizon(uuid: &str, feature: &str, version: Option<&str>) -> String {
    let version = version
        .map(|v| format!("<eml:ObjectVersion>{v}</eml:ObjectVersion>"))
        .unwrap_or_default();
    format!(
        "<resqml:HorizonInterpretation xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" \
         uuid=\"{uuid}\" schemaVersion=\"2.2\">\
         <eml:Citation><eml:Title>a horizon</eml:Title>\
         <eml:Originator>test</eml:Originator>\
         <eml:Creation>2026-10-05T00:00:00Z</eml:Creation>\
         <eml:Format>test</eml:Format></eml:Citation>\
         <resqml:Domain>depth</resqml:Domain>\
         <resqml:InterpretedFeature>\
         <eml:Uuid>{feature}</eml:Uuid>{version}\
         <eml:QualifiedType>{T22}</eml:QualifiedType>\
         <eml:Title>a boundary</eml:Title>\
         </resqml:InterpretedFeature>\
         </resqml:HorizonInterpretation>"
    )
}

/// One part of a fabricated package: its name, its declared content type, its
/// XML.
struct Part {
    name: String,
    content_type: &'static str,
    xml: String,
}

fn part(name: &str, content_type: &'static str, xml: String) -> Part {
    Part {
        name: name.to_string(),
        content_type,
        xml,
    }
}

/// A zip, by hand: the content types declare every part, as a real package does.
fn zip_of(parts: &[Part]) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut bytes);
        let options: zip::write::SimpleFileOptions =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("[Content_Types].xml", options).unwrap();
        let mut types = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
             <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
             <Default Extension=\"rels\" \
             ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>",
        );
        for one in parts {
            types.push_str(&format!(
                "<Override PartName=\"/{}\" ContentType=\"{}\"/>",
                one.name, one.content_type
            ));
        }
        types.push_str("</Types>");
        zip.write_all(types.as_bytes()).unwrap();
        for one in parts {
            zip.start_file(&one.name, options).unwrap();
            zip.write_all(one.xml.as_bytes()).unwrap();
        }
        zip.finish().expect("written");
    }
    bytes.into_inner()
}

fn open_bytes(bytes: Vec<u8>) -> EpcStore {
    EpcStore::from_reader(Box::new(Cursor::new(bytes)), "versions.epc", registry()).expect("opens")
}

/// The package of the lot's proof: **four entries** of two uuids - X in two
/// versions, Y under two releases - and two horizons that name X.
fn four_entries() -> Vec<Part> {
    vec![
        part(
            "version_1/BoundaryFeature_X.xml",
            CT22,
            feature22(X, Some("1"), "X, first"),
        ),
        part(
            "version_2/BoundaryFeature_X.xml",
            CT22,
            feature22(X, Some("2"), "X, second"),
        ),
        part("obj_BoundaryFeature_Y.xml", CT20, feature20(Y, "Y, 2.0.1")),
        part("BoundaryFeature_Y.xml", CT22, feature22(Y, None, "Y, 2.2")),
        part(
            "HorizonInterpretation_H.xml",
            CT_HORIZON,
            horizon(H, X, Some("2")),
        ),
        part("HorizonInterpretation_H0.xml", CT_HORIZON, horizon(H0, X, None)),
    ]
}

fn four() -> EpcStore {
    open_bytes(zip_of(&four_entries()))
}

/// The uris of the boundary features, sorted.
fn features(store: &EpcStore) -> Vec<String> {
    let mut uris: Vec<String> = store
        .resources()
        .into_iter()
        .filter(|one| one.qualified_type.ends_with("BoundaryFeature"))
        .map(|one| one.uri)
        .collect();
    uris.sort();
    uris
}

fn sorted(mut uris: Vec<String>) -> Vec<String> {
    uris.sort();
    uris
}

fn candidates(refusal: Result<impl std::fmt::Debug, Error>) -> (String, Vec<String>) {
    match refusal {
        Err(Error::Ambiguous { uri, candidates }) => (uri, sorted(candidates)),
        other => panic!("expected `Ambiguous`, got {other:?}"),
    }
}

fn since(store: &EpcStore, from: u64) -> Change {
    match store.changes_since(from) {
        Since::Exact(change) => change,
        Since::Everything => panic!("the journal holds a handful of lines"),
    }
}

/// What the window says of the entry **named by this uri**.
fn what_of(change: &Change, uri: &str) -> Option<What> {
    change
        .objects
        .iter()
        .find(|object| object.uri.as_deref() == Some(uri))
        .map(|object| object.what)
}

/// An object, read from XML - one the store being tested does not hold.
fn object22(uuid: &str, version: Option<&str>, title: &str) -> Box<dyn DynObject> {
    registry()
        .read_xml_as(CT22, feature22(uuid, version, title).as_bytes(), ReadMode::Strict)
        .expect("readable")
        .0
}

// ---------------------------------------------------------------------------
// The inventory
// ---------------------------------------------------------------------------

/// What would have to break for this to go red: the open loop dropping a part
/// whose uuid is already known - **the "first part wins" it had before**.
/// There were then two entries, not four, and the sabotage that proves it is
/// restoring that line.
#[test]
fn four_entries_are_four_entries() {
    let store = four();
    assert_eq!(
        features(&store),
        sorted(vec![
            uri22(X, Some("1")),
            uri22(X, Some("2")),
            uri20(Y),
            uri22(Y, None)
        ]),
        "X in two versions, Y in two releases: four entries, each under the uri that names it alone"
    );
    // The same uuid under two *types* keeps the short form of each - the types
    // differ, so nothing conflicts; X's versions conflict, so both name theirs.
    assert!(
        store
            .diagnostics()
            .iter()
            .all(|d| !d.message.contains("first part wins")),
        "{:?}",
        store.diagnostics()
    );
    // And what the front end lists: every object, once.
    assert_eq!(store.resources().len(), 6);
}

/// The rule, the half about the short form: an entry that declares no
/// version keeps the short uri, **even when another entry of its uuid and type
/// declares one**; the versioned one takes the long form.
#[test]
fn the_entry_without_a_version_keeps_the_short_uri_beside_a_versioned_one() {
    let store = open_bytes(zip_of(&[
        part("BoundaryFeature_Z.xml", CT22, feature22(Z, None, "Z, no version")),
        part(
            "version_2/BoundaryFeature_Z.xml",
            CT22,
            feature22(Z, Some("2"), "Z, version 2"),
        ),
    ]));
    assert_eq!(
        features(&store),
        sorted(vec![uri22(Z, None), uri22(Z, Some("2"))])
    );
    // The short uri names the entry that declares none: not ambiguous.
    assert_eq!(
        store.resolve(&uri22(Z, None)).unwrap().locator,
        "BoundaryFeature_Z.xml"
    );
    assert_eq!(
        store.resolve(&uri22(Z, Some("2"))).unwrap().locator,
        "version_2/BoundaryFeature_Z.xml"
    );
}

/// **An object that declares a version is spelled with it**, namesake or not -
/// internally the uri always carries the version number when there is one
/// (ADR-0003). The short form still **names** it: an address
/// given by a user, resolved at the door to the full one.
#[test]
fn a_lone_versioned_object_is_spelled_with_its_version() {
    let store = open_bytes(zip_of(&[part(
        "BoundaryFeature_X.xml",
        CT22,
        feature22(X, Some("7"), "X alone"),
    )]));
    assert_eq!(features(&store), vec![uri22(X, Some("7"))]);
    assert_eq!(
        store.resolve(&uri22(X, None)).unwrap().uri,
        uri22(X, Some("7")),
        "the short form, at the door"
    );
    assert!(matches!(
        store.resolve(&uri22(X, Some("8"))),
        Err(Error::NotFound(_))
    ));
}

/// Only the **whole** identity repeated is a duplicate: same uuid, same type,
/// same version. That one is dropped, and said.
#[test]
fn an_exact_duplicate_is_dropped_and_said() {
    let store = open_bytes(zip_of(&[
        part(
            "version_1/BoundaryFeature_X.xml",
            CT22,
            feature22(X, Some("1"), "X"),
        ),
        part(
            "copy/BoundaryFeature_X.xml",
            CT22,
            feature22(X, Some("1"), "X again"),
        ),
    ]));
    assert_eq!(
        features(&store),
        vec![uri22(X, Some("1"))],
        "one entry, the first part"
    );
    let said: Vec<String> = store.diagnostics().into_iter().map(|d| d.message).collect();
    assert!(
        said.iter()
            .any(|m| m.contains("same uuid, same type, same objectVersion")),
        "{said:?}"
    );
}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

#[test]
fn a_versioned_uri_names_exactly_its_entry_or_nothing() {
    let store = four();
    assert_eq!(
        store.resolve(&uri22(X, Some("1"))).unwrap().locator,
        "version_1/BoundaryFeature_X.xml"
    );
    assert_eq!(
        store.resolve(&uri22(X, Some("2"))).unwrap().locator,
        "version_2/BoundaryFeature_X.xml"
    );
    assert!(matches!(
        store.resolve(&uri22(X, Some("3"))),
        Err(Error::NotFound(_))
    ));
    // Each reads as itself: the bytes of its own part.
    let first = store.get_object(&uri22(X, Some("1"))).expect("reads");
    let second = store.get_object(&uri22(X, Some("2"))).expect("reads");
    assert_eq!(
        energyml_core::identity::title_of_reflect(first.as_reflect()),
        Some("X, first")
    );
    assert_eq!(
        energyml_core::identity::title_of_reflect(second.as_reflect()),
        Some("X, second")
    );
}

/// A short uri that conflicts is **refused by name** - never answered with
/// whichever came first. The candidates are the uris that resolve.
#[test]
fn a_short_uri_that_conflicts_is_refused_naming_each_candidate() {
    let store = four();
    let (uri, named) = candidates(store.resolve(&format!("eml:///{T22}({X})")));
    assert_eq!(uri, format!("eml:///{T22}({X})"));
    assert_eq!(named, sorted(vec![uri22(X, Some("1")), uri22(X, Some("2"))]));
    // …and every name it gives resolves.
    for name in &named {
        assert!(store.resolve(name).is_ok(), "{name}");
    }
    // The same refusal from every door that takes a uri.
    assert!(matches!(
        store.get_object(&format!("eml:///{T22}({X})")),
        Err(Error::Ambiguous { .. })
    ));
    assert!(matches!(
        store.header(&format!("eml:///{T22}({X})")),
        Err(Error::Ambiguous { .. })
    ));
    assert!(matches!(
        store.part_bytes(&format!("eml:///{T22}({X})")),
        Err(Error::Ambiguous { .. })
    ));
}

/// Two releases of one uuid are two types: each short uri names its own.
#[test]
fn one_uuid_under_two_releases_is_named_by_its_type() {
    let store = four();
    assert_eq!(store.resolve(&uri22(Y, None)).unwrap().qualified_type, T22);
    assert_eq!(store.resolve(&uri20(Y)).unwrap().qualified_type, T20);
    // A **bare uuid** names no type, so it cannot choose between them.
    let (_, named) = candidates(store.resolve(Y));
    assert_eq!(named, sorted(vec![uri22(Y, None), uri20(Y)]));
    // And one that names a single entry is answered.
    let lone = open_bytes(zip_of(&[part(
        "BoundaryFeature_Y.xml",
        CT22,
        feature22(Y, None, "Y"),
    )]));
    assert_eq!(lone.resolve(Y).unwrap().uri, uri22(Y, None));
}

// ---------------------------------------------------------------------------
// References
// ---------------------------------------------------------------------------

/// A reference resolves by (uuid, type, version when it carries one) with the
/// rules of a uri; one that names no version of an object held in several is
/// refused by name - and one that names a version nobody holds is not found.
#[test]
fn a_reference_is_resolved_by_what_it_says_or_refused_by_name() {
    let store = four();
    assert_eq!(
        store.resolve_reference(X, Some(T22), Some("2")).unwrap().uri,
        uri22(X, Some("2"))
    );
    assert_eq!(
        store.resolve_reference(X, Some(T22), Some("1")).unwrap().uri,
        uri22(X, Some("1"))
    );
    let (_, named) = candidates(store.resolve_reference(X, Some(T22), None));
    assert_eq!(named, sorted(vec![uri22(X, Some("1")), uri22(X, Some("2"))]));
    assert!(matches!(
        store.resolve_reference(X, Some(T22), Some("9")),
        Err(Error::NotFound(_))
    ));
    // By type: Y named as a 2.0.1 object is the 2.0.1 one.
    assert_eq!(store.resolve_reference(Y, Some(T20), None).unwrap().uri, uri20(Y));
    assert_eq!(
        store.resolve_reference(Y, Some(T22), None).unwrap().uri,
        uri22(Y, None)
    );
    // *A wrong version is not a wrong type*: a lone object answers to a type that
    // differs only by its release.
    let lone = open_bytes(zip_of(&[part(
        "BoundaryFeature_Y.xml",
        CT22,
        feature22(Y, None, "Y"),
    )]));
    assert_eq!(
        lone.resolve_reference(Y, Some(T20), None).unwrap().uri,
        uri22(Y, None)
    );
}

/// The graph draws each reference to the entry it names; one that names no
/// version of an object held twice reaches **both** (too wide, never too
/// narrow: a closure that missed one would break a package).
#[test]
fn the_graph_sends_a_reference_to_the_entry_it_names() {
    let store = four();
    let read = store.read_objects(None).0;
    let graph = energyml_storage::graph::build(read.iter().map(|(s, o)| (s, o.as_reflect())));
    let edges_of = |from_uuid: &str| -> Vec<String> {
        sorted(
            graph
                .edges
                .iter()
                .filter(|edge| edge.from.contains(from_uuid))
                .map(|edge| edge.to.clone())
                .collect(),
        )
    };
    assert_eq!(edges_of(H), vec![uri22(X, Some("2"))], "H names version 2");
    assert_eq!(
        edges_of(H0),
        sorted(vec![uri22(X, Some("1")), uri22(X, Some("2"))]),
        "H0 names no version"
    );
    // Nothing is dangling in this package, and the nodes are all present.
    assert!(graph.nodes.iter().all(|node| node.present), "{:?}", graph.nodes);
    // The checker compares a reference's `ObjectVersion` with the entry it names:
    // H says `2` and names the entry that carries `2` - no mismatch; H0 says
    // nothing of a version and names objects that carry one - the warning the
    // checker already gives for that, and no error.
    let issues = energyml_storage::graph::check_all_references(
        read.iter().map(|(s, o)| (s, o.as_reflect())),
        &|uuid| store.find_by_uuid(uuid),
    );
    assert!(
        issues
            .iter()
            .all(|issue| !issue.source.contains(H) || !issue.is_error()),
        "{issues:?}"
    );
    assert!(
        !issues.iter().any(|issue| issue.source.contains(H)
            && matches!(
                issue.kind,
                energyml_storage::graph::RefIssueKind::ObjectVersionMismatch { .. }
            )),
        "H names version 2 and the entry it names carries 2: {issues:?}"
    );
}

// ---------------------------------------------------------------------------
// Writing, and the journal
// ---------------------------------------------------------------------------

/// The journal names the entry that was written - by the uri that names it - and
/// **not** its namesake.
#[test]
fn an_edit_is_journaled_under_the_uri_of_the_entry_it_touched() {
    let mut store = four();
    let from = store.revision();
    let second = uri22(X, Some("2"));
    store.edit_at(&second, "Citation.Title").expect("borrowed");
    let change = since(&store, from);
    assert_eq!(
        change.objects.len(),
        1,
        "one entry, one line: {:?}",
        change.objects
    );
    assert_eq!(change.objects[0].uuid, X, "the uuid the reader knows");
    assert_eq!(change.objects[0].uri.as_deref(), Some(second.as_str()));
    assert_eq!(change.objects[0].what, What::CITATION);
    assert_eq!(
        what_of(&change, &uri22(X, Some("1"))),
        None,
        "version 1 was not touched"
    );
    // Editing a field does not move the key: the same uris, the same four entries.
    assert_eq!(
        features(&store),
        sorted(vec![
            uri22(X, Some("1")),
            uri22(X, Some("2")),
            uri20(Y),
            uri22(Y, None)
        ])
    );
}

/// An ordinary `put` replaces the entry of the object's version - and only that
/// one. It never makes a second version.
#[test]
fn an_ordinary_put_replaces_the_entry_of_its_version_and_makes_no_new_one() {
    let mut store = four();
    let from = store.revision();
    let uri = store
        .put(object22(X, Some("2"), "X, second, rewritten"))
        .expect("put");
    assert_eq!(uri, uri22(X, Some("2")));
    assert_eq!(features(&store).len(), 4, "still four");
    let change = since(&store, from);
    assert_eq!(
        what_of(&change, &uri22(X, Some("2"))),
        Some(What::BROAD),
        "{change:?}"
    );
    assert_eq!(what_of(&change, &uri22(X, Some("1"))), None);
    let read = store.get_object(&uri22(X, Some("2"))).unwrap();
    assert_eq!(
        energyml_core::identity::title_of_reflect(read.as_reflect()),
        Some("X, second, rewritten")
    );
    let untouched = store.get_object(&uri22(X, Some("1"))).unwrap();
    assert_eq!(
        energyml_core::identity::title_of_reflect(untouched.as_reflect()),
        Some("X, first")
    );
}

/// …and one that names a version none of several holds is **refused**: it could
/// be any of them.
#[test]
fn an_ordinary_put_of_a_version_nobody_holds_is_refused_when_several_exist() {
    let mut store = four();
    let (_, named) = candidates(store.put(object22(X, Some("3"), "X, third")));
    assert_eq!(named, sorted(vec![uri22(X, Some("1")), uri22(X, Some("2"))]));
    let (_, named) = candidates(store.put(object22(X, None, "X, no version")));
    assert_eq!(named.len(), 2);
    assert_eq!(features(&store).len(), 4, "nothing was written");
}

/// The gate that adds a version: a new entry, the uri of its namesakes moved
/// and said, the exact duplicate refused by name.
#[test]
fn the_gate_adds_a_version_and_refuses_the_exact_identity() {
    let mut store = open_bytes(zip_of(&[part(
        "BoundaryFeature_Z.xml",
        CT22,
        feature22(Z, None, "Z"),
    )]));
    assert_eq!(features(&store), vec![uri22(Z, None)]);
    let from = store.revision();

    let uri = store
        .put_as_new_version(object22(Z, Some("2"), "Z, second"))
        .expect("a new version");
    assert_eq!(
        uri,
        uri22(Z, Some("2")),
        "the copy takes the form with its version"
    );
    assert_eq!(
        features(&store),
        sorted(vec![uri22(Z, None), uri22(Z, Some("2"))]),
        "the original declares none: it keeps the short uri"
    );
    let change = since(&store, from);
    assert_eq!(
        what_of(&change, &uri22(Z, Some("2"))),
        Some(What::CREATED),
        "{change:?}"
    );
    // Each reads as itself.
    let original = store.get_object(&uri22(Z, None)).unwrap();
    assert_eq!(
        energyml_core::identity::title_of_reflect(original.as_reflect()),
        Some("Z")
    );
    let copy = store.get_object(&uri22(Z, Some("2"))).unwrap();
    assert_eq!(
        energyml_core::identity::title_of_reflect(copy.as_reflect()),
        Some("Z, second")
    );

    // The exact identity again: refused by name, nothing written.
    let refused = store
        .put_as_new_version(object22(Z, Some("2"), "Z, again"))
        .expect_err("refused");
    assert!(refused.to_string().contains("already held"), "{refused}");
    // And the unversioned one is an identity too.
    let refused = store
        .put_as_new_version(object22(Z, None, "Z, again"))
        .expect_err("refused");
    assert!(refused.to_string().contains("already held"), "{refused}");
    assert_eq!(features(&store).len(), 2);

    // The short uri of Z still names the one that declares none.
    assert_eq!(store.resolve(&uri22(Z, None)).unwrap().uri, uri22(Z, None));
}

/// A second version of an object **moves no uri**: each entry is spelled with
/// its own version from the start (internally the uri always carries the
/// version number when there is one, ADR-0003), so the
/// journal names the newcomer only - and a removal, the removed one only.
#[test]
fn a_second_version_moves_no_uri_and_the_journal_names_the_newcomer_only() {
    let mut store = open_bytes(zip_of(&[part(
        "BoundaryFeature_X.xml",
        CT22,
        feature22(X, Some("1"), "X"),
    )]));
    assert_eq!(
        features(&store),
        vec![uri22(X, Some("1"))],
        "alone, spelled with its version"
    );
    let from = store.revision();
    store
        .put_as_new_version(object22(X, Some("2"), "X, second"))
        .expect("a new version");
    assert_eq!(
        features(&store),
        sorted(vec![uri22(X, Some("1")), uri22(X, Some("2"))])
    );
    let change = since(&store, from);
    assert_eq!(
        what_of(&change, &uri22(X, Some("1"))),
        None,
        "the first is not named: {change:?}"
    );
    assert_eq!(what_of(&change, &uri22(X, Some("2"))), Some(What::CREATED));
    // Removing it again moves nothing either.
    let from = store.revision();
    store.remove(&uri22(X, Some("2"))).expect("removed");
    assert_eq!(features(&store), vec![uri22(X, Some("1"))]);
    let change = since(&store, from);
    assert_eq!(
        what_of(&change, &uri22(X, Some("2"))),
        Some(What::REMOVED),
        "{change:?}"
    );
    assert_eq!(
        what_of(&change, &uri22(X, Some("1"))),
        None,
        "the survivor is not named: {change:?}"
    );
    // And it can come back: the same slot, a `CREATED`.
    let from = store.revision();
    store
        .put_as_new_version(object22(X, Some("2"), "X, second, again"))
        .expect("recreated");
    assert_eq!(
        what_of(&since(&store, from), &uri22(X, Some("2"))),
        Some(What::CREATED)
    );
}

/// Removing one version leaves the other, and a short uri that was ambiguous
/// stops being so.
#[test]
fn removing_one_version_leaves_the_other() {
    let mut store = four();
    store.remove(&uri22(X, Some("2"))).expect("removed");
    assert!(matches!(
        store.resolve(&uri22(X, Some("2"))),
        Err(Error::NotFound(_))
    ));
    assert_eq!(
        store.resolve(&format!("eml:///{T22}({X})")).unwrap().uri,
        uri22(X, Some("1")),
        "no longer ambiguous"
    );
    assert!(store.get_object(&uri22(X, Some("1"))).is_ok());
}

// ---------------------------------------------------------------------------
// The package, written and read back
// ---------------------------------------------------------------------------

fn written(store: &EpcStore) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    let notes = store
        .write_to(&mut bytes, &SaveOptions::default())
        .expect("written");
    assert!(notes.iter().all(|d| !d.message.contains("share")), "{notes:?}");
    bytes.into_inner()
}

fn names_in(bytes: &[u8]) -> Vec<String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("a zip");
    sorted(
        (0..zip.len())
            .map(|at| {
                zip.by_index(at)
                    .unwrap()
                    .name()
                    .expect("a part name")
                    .into_owned()
            })
            .collect(),
    )
}

fn text_in(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("a zip");
    let mut text = String::new();
    zip.by_name(name)
        .unwrap_or_else(|_| panic!("{name} is in the package"))
        .read_to_string(&mut text)
        .unwrap();
    text
}

/// **EPC v1.1 §3.6** (p. 17): two entries cannot share a path, and a versioned
/// one goes under `version_<id>`. Written, then read back: the same four
/// identities, each with its own bytes. Read with the `zip` crate, so that the
/// part names are the archive's own.
#[test]
fn a_package_of_four_entries_is_written_under_unique_paths_and_reads_back_the_same() {
    let store = four();
    let bytes = written(&store);

    let names = names_in(&bytes);
    // The versioned entries sit under `version_<id>/`, the others keep the plain
    // name a package without namesakes always had.
    for expected in [
        "version_1/BoundaryFeature_7c900000-0000-4000-8000-000000000001.xml",
        "version_2/BoundaryFeature_7c900000-0000-4000-8000-000000000001.xml",
        "obj_BoundaryFeature_7c900000-0000-4000-8000-000000000002.xml",
        "BoundaryFeature_7c900000-0000-4000-8000-000000000002.xml",
    ] {
        assert!(
            names.iter().any(|name| name == expected),
            "{expected} in {names:?}"
        );
    }
    // The bytes of each: version 2 in the folder of version 2.
    let second = text_in(
        &bytes,
        "version_2/BoundaryFeature_7c900000-0000-4000-8000-000000000001.xml",
    );
    assert!(
        second.contains("X, second") && second.contains("objectVersion=\"2\""),
        "{second}"
    );

    // Reading it back: the same four identities.
    let again = open_bytes(bytes.clone());
    assert_eq!(features(&again), features(&store));
    assert_eq!(again.resources().len(), 6);
    let title = |uri: &str| {
        let object = again.get_object(uri).expect("reads");
        energyml_core::identity::title_of_reflect(object.as_reflect()).map(str::to_string)
    };
    assert_eq!(title(&uri22(X, Some("1"))).as_deref(), Some("X, first"));
    assert_eq!(title(&uri22(X, Some("2"))).as_deref(), Some("X, second"));
    assert_eq!(title(&uri20(Y)).as_deref(), Some("Y, 2.0.1"));
    assert_eq!(title(&uri22(Y, None)).as_deref(), Some("Y, 2.2"));

    // A relationship names the part of the entry the reference names: H, which
    // says version 2, is related to the part of version 2 and not of version 1.
    let rels = text_in(
        &bytes,
        "_rels/HorizonInterpretation_7c900000-0000-4000-8000-0000000000a1.xml.rels",
    );
    assert!(rels.contains("version_2/BoundaryFeature_"), "{rels}");
    assert!(!rels.contains("version_1/BoundaryFeature_"), "{rels}");
    // Two relationships of one `.rels` never share an `Id` (§3.4.1) - H0 names
    // both versions of X.
    let rels = text_in(
        &bytes,
        "_rels/HorizonInterpretation_7c900000-0000-4000-8000-0000000000a2.xml.rels",
    );
    assert!(
        rels.contains("version_1/BoundaryFeature_") && rels.contains("version_2/BoundaryFeature_"),
        "{rels}"
    );
    let ids: Vec<&str> = rels
        .split("Id=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .collect();
    let mut unique = ids.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(ids.len(), unique.len(), "{ids:?}");

    // A second round changes nothing.
    assert_eq!(names_in(&written(&again)), names);
}

/// A version that is no safe path segment is percent-encoded (§3.6: *"If
/// necessary, use URI encoding (%) to escape special characters"*), and it
/// reads back as the version it was.
#[test]
fn a_version_with_special_characters_is_encoded_in_its_path_and_reads_back() {
    let mut store = open_bytes(zip_of(&[part(
        "BoundaryFeature_Z.xml",
        CT22,
        feature22(Z, None, "Z"),
    )]));
    store
        .put_as_new_version(object22(Z, Some("v 2/b"), "Z odd"))
        .expect("a new version");
    let bytes = written(&store);
    let names = names_in(&bytes);
    assert!(
        names
            .iter()
            .any(|name| name == "version_v%202%2Fb/BoundaryFeature_7c900000-0000-4000-8000-000000000003.xml"),
        "{names:?}"
    );
    let again = open_bytes(bytes);
    assert_eq!(
        features(&again),
        sorted(vec![uri22(Z, None), uri22(Z, Some("v 2/b"))])
    );
    assert!(again.resolve(&uri22(Z, Some("v 2/b"))).is_ok());
}

/// The same uuid and type under two releases whose type names are alike
/// (`witsml20.Well` and `witsml21.Well`) cannot share a flat name either: the
/// second goes under `namespace_<id>` (EPC §3.6).
#[test]
fn two_releases_with_alike_names_get_a_namespace_folder() {
    const WELL: &str = "7c900000-0000-4000-8000-0000000000b1";
    fn well(namespace: &str, version: &str) -> String {
        format!(
            "<witsml:Well xmlns:witsml=\"{namespace}\" xmlns:eml=\"{EML}\" uuid=\"{WELL}\" schemaVersion=\"{version}\">\
             <eml:Citation><eml:Title>a well</eml:Title><eml:Originator>test</eml:Originator>\
             <eml:Creation>2026-10-05T00:00:00Z</eml:Creation><eml:Format>test</eml:Format></eml:Citation>\
             </witsml:Well>"
        )
    }
    let parts = [
        part(
            "twenty/Well.xml",
            "application/x-witsml+xml;version=2.0;type=Well",
            well("http://www.energistics.org/energyml/data/witsmlv2", "2.0"),
        ),
        part(
            "twentyone/Well.xml",
            "application/x-witsml+xml;version=2.1;type=Well",
            well("http://www.energistics.org/energyml/data/witsmlv2", "2.1"),
        ),
    ];
    let store = open_bytes(zip_of(&parts));
    let wells: Vec<String> = store
        .resources()
        .into_iter()
        .map(|one| one.qualified_type)
        .collect();
    assert_eq!(wells.len(), 2, "{wells:?}");
    let bytes = written(&store);
    let names = names_in(&bytes);
    assert!(
        names.iter().any(|name| name.starts_with("namespace_witsml2")),
        "{names:?}"
    );
    assert_eq!(open_bytes(bytes).resources().len(), 2);
}

// ---------------------------------------------------------------------------
// Importing
// ---------------------------------------------------------------------------

/// An object of the same uuid and type under **another version** comes in as a
/// **new entry**, not as a conflict and not over the one that is there
/// (ADR-0010): the plan says so (`new_versions`), the apply
/// writes it through the gate that adds a version and says so too, and the open
/// package then holds both. The same version imported again is the ordinary case
/// (identical here: nothing to write).
#[test]
fn an_import_of_another_version_is_a_new_entry_and_says_so() {
    use energyml_storage::import::{Fate, Request, apply_plan, plan};

    let mut here = open_bytes(zip_of(&[part(
        "BoundaryFeature_Z.xml",
        CT22,
        feature22(Z, None, "Z, here"),
    )]));
    let there = open_bytes(zip_of(&[part(
        "BoundaryFeature_Z.xml",
        CT22,
        feature22(Z, Some("2"), "Z, there"),
    )]));
    let short = uri22(Z, None);
    // The imported object, by the uri **it** has in the imported package: with
    // the version it declares.
    let there_uri = uri22(Z, Some("2"));

    let quote = plan(&there, &here, &Request::default());
    assert!(quote.refusals.is_empty(), "{:?}", quote.refusals);
    assert!(
        quote.conflicts.is_empty(),
        "another version is no conflict: {:?}",
        quote.conflicts
    );
    assert_eq!(quote.new_versions, vec![there_uri.clone()], "named in the plan");
    assert_eq!(quote.objects.len(), 1);
    assert_eq!(quote.objects[0].fate, Fate::New);

    let from = here.revision();
    let done = apply_plan(&there, &mut here, &quote).expect("applied");
    assert_eq!(done.new_versions, vec![there_uri], "and in what was done");
    assert!(
        done.replaced.is_empty(),
        "nothing was replaced: {:?}",
        done.replaced
    );

    assert_eq!(features(&here), sorted(vec![short.clone(), uri22(Z, Some("2"))]));
    let held = here.get_object(&short).unwrap();
    assert_eq!(
        energyml_core::identity::title_of_reflect(held.as_reflect()),
        Some("Z, here"),
        "the one that was here is untouched"
    );
    let brought = here.get_object(&uri22(Z, Some("2"))).unwrap();
    assert_eq!(
        energyml_core::identity::title_of_reflect(brought.as_reflect()),
        Some("Z, there")
    );
    let change = since(&here, from);
    assert_eq!(
        what_of(&change, &uri22(Z, Some("2"))),
        Some(What::CREATED),
        "{change:?}"
    );
}

/// A transaction that added a version and was thrown away leaves the package as
/// it was: the namesake keeps the uri it had all along, the one with its version.
#[test]
fn a_rolled_back_version_leaves_its_namesake_as_it_was() {
    let mut store = open_bytes(zip_of(&[part(
        "BoundaryFeature_X.xml",
        CT22,
        feature22(X, Some("1"), "X"),
    )]));
    store.start_transaction().expect("a transaction");
    store
        .put_as_new_version(object22(X, Some("2"), "X, second"))
        .expect("a new version");
    assert_eq!(
        features(&store),
        sorted(vec![uri22(X, Some("1")), uri22(X, Some("2"))])
    );
    store.rollback_transaction().expect("rolled back");
    assert_eq!(
        features(&store),
        vec![uri22(X, Some("1"))],
        "alone again, as it was"
    );
    assert!(matches!(
        store.resolve(&uri22(X, Some("2"))),
        Err(Error::NotFound(_))
    ));
    assert!(store.get_object(&uri22(X, Some("1"))).is_ok());
}

// ---------------------------------------------------------------------------
// One lookup, three doors; and an edited objectVersion
// ---------------------------------------------------------------------------

fn outcome(found: Result<&energyml_storage::ObjectSummary, Error>) -> Result<String, String> {
    found
        .map(|summary| summary.uri.clone())
        .map_err(|why| why.to_string())
}

/// `resolve` (a URI), `lookup` (a triple) and `lookup_uuid` (a bare uuid) are one
/// lookup: the same entry or the same refusal, for every case of the package.
#[test]
fn the_three_entry_points_agree() {
    let store = four();
    let cases: Vec<(String, &str, Option<&str>, Option<&str>)> = vec![
        (format!("eml:///{T22}({X})"), X, Some(T22), None),
        (uri22(X, Some("1")), X, Some(T22), Some("1")),
        (uri22(X, Some("2")), X, Some(T22), Some("2")),
        (uri22(X, Some("3")), X, Some(T22), Some("3")),
        (uri22(Y, None), Y, Some(T22), None),
        (uri20(Y), Y, Some(T20), None),
    ];
    for (uri, uuid, qualified_type, version) in &cases {
        assert_eq!(
            outcome(store.resolve(uri)),
            outcome(store.lookup(uuid, *qualified_type, *version)),
            "{uri}"
        );
    }
    for uuid in [X, Y, Z] {
        assert_eq!(
            outcome(store.resolve(uuid)),
            outcome(store.lookup_uuid(uuid)),
            "{uuid}"
        );
    }
    // …and the refusals really are refusals, so the agreement is not two `Ok`s.
    assert!(store.lookup_uuid(Y).is_err() && store.lookup(X, Some(T22), None).is_err());
    assert!(store.lookup(X, Some(T22), Some("2")).is_ok());
}

/// An edited `objectVersion` is an **identity change**, alone or not: the uri
/// carries the version (ADR-0003), so it moves with it and the
/// journal says so under the new uri; the entry keeps its slot. The version it
/// now carries resolves, the one it had does not (a versioned uri names exactly
/// its entry, Identifier Specification v5.0 §3.4.2.1). A version a namesake
/// holds is refused by name.
#[test]
fn editing_an_object_version_is_an_identity_change() {
    // Alone.
    let mut store = open_bytes(zip_of(&[part(
        "BoundaryFeature_X.xml",
        CT22,
        feature22(X, Some("1"), "X"),
    )]));
    let from = store.revision();
    let uri = store
        .set_object_version(&uri22(X, Some("1")), "5")
        .expect("edited");
    assert_eq!(uri, uri22(X, Some("5")), "the uri carries the new version");
    assert_eq!(features(&store), vec![uri22(X, Some("5"))]);
    let moved = what_of(&since(&store, from), &uri).expect("named under its new uri");
    assert!(moved.contains(What::IDENTITY), "{moved:?}");
    assert!(
        matches!(store.resolve(&uri22(X, Some("1"))), Err(Error::NotFound(_))),
        "the one it had"
    );
    assert_eq!(
        store.resolve(&uri22(X, None)).unwrap().uri,
        uri,
        "the short form, at the door"
    );

    // With namesakes: the same edit moves the uri.
    let mut store = four();
    let refused = store
        .set_object_version(&uri22(X, Some("1")), "2")
        .expect_err("a namesake holds it");
    assert!(refused.to_string().contains("already carries"), "{refused}");
    let from = store.revision();
    let uri = store
        .set_object_version(&uri22(X, Some("1")), "3")
        .expect("edited");
    assert_eq!(uri, uri22(X, Some("3")));
    let moved = what_of(&since(&store, from), &uri).expect("named under its new uri");
    assert!(moved.contains(What::IDENTITY), "{moved:?}");
    assert_eq!(
        features(&store),
        sorted(vec![
            uri22(X, Some("3")),
            uri22(X, Some("2")),
            uri20(Y),
            uri22(Y, None)
        ])
    );
}
