//! Parts no parser will touch, and how to fix them anyway.
//!
//! Every other way of changing a package goes through an object: read it, edit
//! it by reflection, let the writer serialise it back. That whole path assumes
//! the part **parses**, and the audit of 01/08 left three cases where it does
//! not — markup that does not close, a part of zero length, a byte that is not
//! UTF-8. Those were named refusals, which was honest and left the file
//! unfixable.
//!
//! This is the third level of tolerance: the bytes themselves. Take them out,
//! repair them, put them back.
//!
//! # The case that used to be invisible
//!
//! Worse than an unreadable part is one the **inventory drops**. When the
//! damage reaches the root element, and the part name carries no uuid either,
//! there is nothing to address the part by — so it did not appear in `list`, in
//! `check`'s object section, or anywhere else. It was in the archive and in no
//! listing. `unreadable()` reports it by part name, and the part name is enough
//! to read and replace it.
//!
//! Everything here is built in the test rather than taken from `testdata/`,
//! because a corpus of deliberately broken packages is exactly what nobody
//! wants to keep in a repository.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_storage::{EpcStore, SaveOptions};

const UUID: &str = "1a4112fa-c4ef-4c8d-aed0-47d9273bebc5";
const PART: &str = "TriangulatedSetRepresentation_1a4112fa-c4ef-4c8d-aed0-47d9273bebc5.xml";

fn registry() -> Arc<Registry> {
    let mut registry = Registry::new();
    energyml_eml_2_3::register(&mut registry);
    energyml_resqml_2_2::register(&mut registry);
    Arc::new(registry)
}

const NEIGHBOUR: &str = "2b5223fb-d5f0-5d9e-bfe1-58ea384cacd6";
const NEIGHBOUR_PART: &str = "TriangulatedSetRepresentation_2b5223fb-d5f0-5d9e-bfe1-58ea384cacd6.xml";

/// A `TriangulatedSetRepresentation` that reads: the repair target.
///
/// It carries a `RepresentedObject` pointing at the package's other part,
/// because a relationship is only written for a reference that has somewhere to
/// go — and one of the tests below is entirely about the relationships.
fn healthy() -> String {
    object(UUID, Some(NEIGHBOUR))
}

fn object(uuid: &str, references: Option<&str>) -> String {
    let reference = references
        .map(|target| {
            format!(
                "<resqml:RepresentedObject>\
<eml:QualifiedType>resqml22.TriangulatedSetRepresentation</eml:QualifiedType>\
<eml:Title>neighbour</eml:Title><eml:Uuid>{target}</eml:Uuid>\
</resqml:RepresentedObject>"
            )
        })
        .unwrap_or_default();
    format!(
        "<resqml:TriangulatedSetRepresentation \
xmlns:resqml=\"http://www.energistics.org/energyml/data/resqmlv2\" \
xmlns:eml=\"http://www.energistics.org/energyml/data/commonv2\" \
xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" \
uuid=\"{uuid}\" schemaVersion=\"2.2\">\
<eml:Citation><eml:Title>Fault1 Interp1 TriRep</eml:Title>\
<eml:Originator>test</eml:Originator>\
<eml:Creation>2026-08-01T00:00:00Z</eml:Creation>\
<eml:Format>test</eml:Format></eml:Citation>\
{reference}\
<resqml:SurfaceRole>map</resqml:SurfaceRole>\
</resqml:TriangulatedSetRepresentation>"
    )
}

fn content_types(part: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Override PartName=\"/{part}\" \
ContentType=\"application/x-resqml+xml;version=2.2;type=TriangulatedSetRepresentation\"/>\
<Override PartName=\"/{NEIGHBOUR_PART}\" \
ContentType=\"application/x-resqml+xml;version=2.2;type=TriangulatedSetRepresentation\"/>\
</Types>"
    )
}

/// A one-object package whose single part holds exactly these bytes.
///
/// `declare` says whether `[Content_Types].xml` mentions the part at all —
/// without it, and with damage at the root, nothing can address the part, which
/// is the invisible case.
fn package(name: &str, part: &str, bytes: &[u8], declare: bool) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("energyml-repair-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a temporary folder");
    let path = dir.join(format!("{name}.epc"));

    let file = std::fs::File::create(&path).expect("the package is creatable");
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("[Content_Types].xml", options).unwrap();
    let index = if declare {
        content_types(part)
    } else {
        content_types("Nothing.xml")
    };
    zip.write_all(index.as_bytes()).unwrap();
    zip.start_file(part, options).unwrap();
    zip.write_all(bytes).unwrap();
    // A second, healthy object, so the victim's reference has somewhere to go.
    zip.start_file(NEIGHBOUR_PART, options).unwrap();
    zip.write_all(object(NEIGHBOUR, None).as_bytes()).unwrap();
    zip.finish().expect("the package is written");
    path
}

/// Markup cut in half: the inventory still addresses it, the parser does not.
fn truncated() -> Vec<u8> {
    let whole = healthy();
    whole.as_bytes()[..whole.len() / 2].to_vec()
}

// ---------------------------------------------------------------------------

/// The worklist names the part, its size and what the reader said.
/// The fourth copy a reference carries: the revision of its target.
///
/// Added on 10/09, after `energyml check` learned to compare it and found our
/// own translations saying two things — every object stamped `objectVersion=0`
/// and every reference silent. What would have to break for this to go red: the
/// version left out of the drift check, written where the target has none, or a
/// reference naming a revision the target lacks quietly cleared.
#[test]
fn a_reference_takes_the_revision_its_target_carries() {
    use energyml_storage::graph::{RepairOutcome, Target, repair_references};

    let registry = registry();
    let read = |xml: &str| {
        registry
            .read_xml(
                "application/x-resqml+xml;version=2.2;type=TriangulatedSetRepresentation",
                xml.as_bytes(),
            )
            .expect("the object reads")
    };
    let summary = energyml_storage::ObjectSummary {
        uri: format!("eml:///resqml22.TriangulatedSetRepresentation({UUID})"),
        uuid: UUID.to_string(),
        qualified_type: "resqml22.TriangulatedSetRepresentation".to_string(),
        content_type: String::new(),
        locator: PART.to_string(),
    };
    let target = |version: Option<&str>| Target {
        qualified_type: "resqml22.TriangulatedSetRepresentation".to_string(),
        title: Some("neighbour".to_string()),
        object_version: version.map(str::to_string),
    };
    let version_of = |object: &dyn energyml_core::value::Reflect| {
        energyml_core::identity::collect_refs(object)
            .first()
            .and_then(|edge| edge.target.object_version.map(str::to_string))
    };

    // The reference says nothing, the target is at `0`: brought in line.
    let mut object = read(&healthy());
    let found = repair_references(&summary, object.as_reflect_mut(), &|_| Some(target(Some("0"))));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        matches!(
            &found[0].outcome,
            RepairOutcome::Done { now_version: Some(version), .. } if version == "0"
        ),
        "{:?}",
        found[0].outcome
    );
    assert_eq!(version_of(object.as_reflect()).as_deref(), Some("0"));
    assert!(found[0].to_string().contains("version → `0`"), "{}", found[0]);

    // Run again: nothing left to say. A repair that keeps finding work is a
    // repair that rewrites the package at every pass.
    let again = repair_references(&summary, object.as_reflect_mut(), &|_| Some(target(Some("0"))));
    assert!(again.is_empty(), "{again:?}");

    // The target moved on: the copy follows.
    let moved = repair_references(&summary, object.as_reflect_mut(), &|_| Some(target(Some("3"))));
    assert_eq!(moved.len(), 1);
    assert_eq!(version_of(object.as_reflect()).as_deref(), Some("3"));

    // And a target that states no revision leaves the reference alone: which
    // half is wrong is not something the package says.
    let silent = repair_references(&summary, object.as_reflect_mut(), &|_| Some(target(None)));
    assert!(silent.is_empty(), "{silent:?}");
    assert_eq!(version_of(object.as_reflect()).as_deref(), Some("3"));
}

#[test]
fn an_unreadable_part_is_listed_with_its_reason() {
    let path = package("truncated", PART, &truncated(), true);
    let store = EpcStore::open(&path, registry()).expect("the package opens");

    let broken = store.unreadable();
    assert_eq!(broken.len(), 1, "one part, and it is the broken one");
    let item = &broken[0];
    assert_eq!(item.part, PART);
    assert_eq!(
        item.uri.as_deref(),
        Some(format!("eml:///resqml22.TriangulatedSetRepresentation({UUID})").as_str()),
        "the part name carries the uuid, so this one is still addressable"
    );
    assert_eq!(item.bytes, truncated().len());
    assert!(
        item.reason.contains("tag not closed") || item.reason.contains("syntax"),
        "the reason is the reader's own words: {}",
        item.reason
    );
}

/// A part of zero length reports zero, which is a diagnosis of its own.
#[test]
fn an_empty_part_says_it_is_empty() {
    let path = package("empty", PART, b"", true);
    let store = EpcStore::open(&path, registry()).expect("the package opens");
    let broken = store.unreadable();
    assert_eq!(broken.len(), 1);
    assert_eq!(broken[0].bytes, 0, "nothing in it at all");
}

/// The case that used to be invisible: damage at the root, no uuid anywhere.
///
/// It is in the archive and in no listing — not in `resources`, not addressable
/// by uri. Only the part name reaches it, which is why `unreadable` reports one
/// and `raw_part` takes one.
#[test]
fn a_part_the_inventory_dropped_is_still_reachable_by_name() {
    let path = package("nameless", "Broken.xml", b"", false);
    let store = EpcStore::open(&path, registry()).expect("the package opens");

    assert!(
        !store.resources().iter().any(|s| s.locator == "Broken.xml"),
        "nothing addressable: that is the whole problem"
    );
    let broken = store.unreadable();
    assert_eq!(broken.len(), 1, "and yet it is reported");
    assert_eq!(broken[0].part, "Broken.xml");
    assert!(
        broken[0].uri.is_none(),
        "no uri — the part name is the only handle there is"
    );
    assert_eq!(store.raw_part("Broken.xml").expect("readable as bytes"), b"");
}

/// The whole loop: take the bytes out, put repaired ones back, save, reopen.
#[test]
fn a_repaired_part_reads_after_a_save() {
    let path = package("loop", PART, &truncated(), true);
    let mut store = EpcStore::open(&path, registry()).expect("the package opens");
    assert_eq!(store.unreadable().len(), 1);

    // What a text editor would have produced.
    let kind = store
        .stage_repaired_part(PART, healthy().into_bytes())
        .expect("the repaired bytes parse");
    assert_eq!(kind, "resqml22.TriangulatedSetRepresentation");
    assert!(
        store.unreadable().is_empty(),
        "the store already knows it is fixed, before any save"
    );

    let out = path.with_file_name("repaired.epc");
    store.save_as(&out, &SaveOptions::default()).expect("it saves");

    let reopened = EpcStore::open(&out, registry()).expect("the repaired package opens");
    assert!(reopened.unreadable().is_empty(), "and nothing is broken in it");
    let summary = reopened
        .resources()
        .into_iter()
        .find(|s| s.uuid == UUID)
        .expect("the object is listed");
    let object = reopened.get_object(&summary.uri).expect("and it deserializes");
    assert_eq!(object.type_info().name, "TriangulatedSetRepresentation");
}

/// Repaired bytes that parse rejoin the objects, so the save rebuilds the
/// part's `.rels` from its own references.
///
/// The difference between `stage_repaired_part` and `set_raw_part`, and it is
/// not cosmetic: verbatim bytes carry no references a writer can see, so a part
/// repaired the other way comes out of the save with no relationships at all.
#[test]
fn a_repair_that_parses_gets_its_relationships_back() {
    let path = package("rels", PART, &truncated(), true);
    let rebuilt = {
        let mut store = EpcStore::open(&path, registry()).expect("opens");
        store
            .stage_repaired_part(PART, healthy().into_bytes())
            .expect("parses");
        let out = path.with_file_name("rels-object.epc");
        store.save_as(&out, &SaveOptions::default()).expect("saves");
        out
    };
    let verbatim = {
        let mut store = EpcStore::open(&path, registry()).expect("opens");
        store.set_raw_part(PART, healthy().into_bytes());
        let out = path.with_file_name("rels-bytes.epc");
        store.save_as(&out, &SaveOptions::default()).expect("saves");
        out
    };

    let names = |path: &PathBuf| -> Vec<String> {
        let file = std::fs::File::open(path).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().to_string())
            .collect()
    };
    let rels = format!("_rels/{PART}.rels");
    assert!(
        names(&rebuilt).contains(&rels),
        "through the object layer, the relationships are regenerated"
    );
    assert!(
        !names(&verbatim).contains(&rels),
        "through the byte layer, they are not — which is why the two exist"
    );
}

/// Bytes that still do not parse are refused, and nothing is staged.
///
/// The store has to be left exactly as it was: a repair attempt that made
/// things worse and then reported failure would be the worst of both.
#[test]
fn a_repair_that_does_not_parse_changes_nothing() {
    let path = package("refused", PART, &truncated(), true);
    let mut store = EpcStore::open(&path, registry()).expect("opens");

    let error = store
        .stage_repaired_part(PART, b"<still broken".to_vec())
        .expect_err("it does not parse");
    assert!(error.to_string().contains(PART), "the error names the part");
    assert!(store.staged_parts().is_empty(), "nothing was staged");
    assert_eq!(
        store.raw_part(PART).expect("still readable as bytes"),
        truncated(),
        "and the bytes are the ones that were there"
    );
    assert!(!store.is_dirty(), "a refused repair leaves a clean store");
}

/// `set_raw_part` stages anything, on purpose.
///
/// A repair can take more than one pass, and a tool that refused to hold an
/// intermediate state would be useless for the files that need it most. What it
/// must not do is pretend: the part is written, and it is still unreadable.
#[test]
fn bytes_that_do_not_parse_can_still_be_written() {
    let path = package("forced", PART, b"", true);
    let mut store = EpcStore::open(&path, registry()).expect("opens");

    store.set_raw_part(PART, b"<half-way there".to_vec());
    assert_eq!(store.staged_parts(), vec![PART.to_string()]);
    assert!(store.staged_part_parses(PART).is_err(), "and it says so");

    let out = path.with_file_name("forced.epc");
    store
        .save_as(&out, &SaveOptions::default())
        .expect("it saves anyway");

    let reopened = EpcStore::open(&out, registry()).expect("the package opens");
    assert_eq!(
        reopened.raw_part(PART).expect("the bytes are there"),
        b"<half-way there",
        "written exactly as given"
    );
    assert_eq!(
        reopened.unreadable().len(),
        1,
        "and still reported as broken, because it is"
    );
}

/// Staged bytes are what a read gives back, before any save.
#[test]
fn a_read_sees_what_was_staged() {
    let path = package("staged", PART, &truncated(), true);
    let mut store = EpcStore::open(&path, registry()).expect("opens");
    store.set_raw_part(PART, b"whatever".to_vec());
    assert_eq!(store.raw_part(PART).unwrap(), b"whatever");
    // Through the uri as well: the two addressings agree.
    let uri = format!("eml:///resqml22.TriangulatedSetRepresentation({UUID})");
    assert_eq!(store.part_bytes(&uri).unwrap(), b"whatever");

    assert!(store.clear_raw_part(PART), "and it can be taken back");
    assert_eq!(store.raw_part(PART).unwrap(), truncated());
}

/// A rollback undoes a staged repair, like every other change.
///
/// The raw layer is a second map, so it had to be taught the transaction
/// separately — and a rollback that undid the objects but kept the bytes would
/// be exactly the kind of half-truth `overlay.rs` refuses to ship.
#[test]
fn a_rollback_undoes_a_staged_repair() {
    let path = package("transaction", PART, &truncated(), true);
    let mut store = EpcStore::open(&path, registry()).expect("opens");

    store.start_transaction().expect("a transaction opens");
    store.set_raw_part(PART, b"provisional".to_vec());
    assert_eq!(store.raw_part(PART).unwrap(), b"provisional");
    store.rollback_transaction().expect("it rolls back");
    assert_eq!(
        store.raw_part(PART).unwrap(),
        truncated(),
        "the staged bytes are gone"
    );
    assert!(store.staged_parts().is_empty());

    // And a commit keeps them.
    store.start_transaction().expect("a second transaction");
    store.set_raw_part(PART, b"kept".to_vec());
    store.commit_transaction().expect("it commits");
    assert_eq!(store.raw_part(PART).unwrap(), b"kept");
}

/// Repairing a nameless part puts it back under its proper name.
///
/// Not something the repair asks for — a consequence of rejoining the object
/// layer. The writer places a part from the object's own type and uuid, so a
/// part called `Broken.xml`, once it parses, comes out of the save as
/// `TriangulatedSetRepresentation_<uuid>.xml` with its `.rels` beside it. The
/// package is not merely readable again, it is laid out the way the packaging
/// conventions ask.
#[test]
fn a_repaired_part_regains_its_proper_name() {
    let path = package("renamed", "Broken.xml", b"", false);
    let mut store = EpcStore::open(&path, registry()).expect("opens");
    assert_eq!(store.unreadable().len(), 1);

    store
        .stage_repaired_part("Broken.xml", healthy().into_bytes())
        .expect("the repaired bytes parse");
    let out = path.with_file_name("renamed.epc");
    store.save_as(&out, &SaveOptions::default()).expect("saves");

    let file = std::fs::File::open(&out).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_string())
        .collect();
    assert!(
        names.contains(&PART.to_string()),
        "the object decides where its part goes: {names:?}"
    );
    assert!(
        !names.contains(&"Broken.xml".to_string()),
        "and the name it had while it was broken is gone"
    );

    let reopened = EpcStore::open(&out, registry()).expect("the repaired package opens");
    assert!(reopened.unreadable().is_empty());
    assert!(
        reopened.resources().iter().any(|s| s.uuid == UUID),
        "and the object is addressable at last"
    );
}
