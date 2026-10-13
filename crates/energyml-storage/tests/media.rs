//! Files in the package, and the objects they show — EPC §3.4.2.
//!
//! `Energistics_Packaging_Conventions_v1.0_Doc_v1.2.pdf`:
//!
//! - other files "As a guideline … stored in a folder named `media`" (§3.2,
//!   pp. 11–12), "An optional media folder may contain any type of media
//!   file--no restrictions" (§3.3 p. 14);
//! - `[Content_Types].xml` gives "a specific mime content type for each part in
//!   the package" (§2.2.1 p. 9);
//! - `destinationMedia`: "The object in Target is a media representation for
//!   the current object"; `sourceMedia`: "The current object is a media
//!   representation for the object in Target" (§3.4.2 pp. 15–16).
//!
//! **The package is read back with the `zip` crate and `quick-xml`**, not with
//! the store: a writer and its own reader agreeing on a relationship proves
//! only that they agree (ADR-0011: never ask a component to check itself).
//! The store's reading is checked too, after, against
//! the same bytes.
//!
//! No corpus: the package is built here, so none of these can skip.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_storage::extract::empty_package;
use energyml_storage::{EpcStore, LocalPackage, SaveOptions, Severity};
use quick_xml::events::Event;

const BASE: &str = "http://schemas.energistics.org/package/2012/relationships/";

const FEATURE: &str = "0f4c1a0e-5b3d-4c1e-9a7b-2d6e8f0a1b2c";
const OTHER: &str = "7a1e2c3d-4b5f-4e6a-8b9c-0d1e2f3a4b5c";

/// Bytes that are not text: a PDF header and every byte value, so a writer
/// that went through a `String` anywhere would be caught.
fn binary() -> Vec<u8> {
    let mut bytes = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    bytes.extend(0u8..=255);
    bytes
}

const README: &[u8] = b"# Lutetia\n\nA synthetic model. What is real, what is invented.\n";

fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

fn feature(uuid: &str, title: &str) -> Box<dyn energyml_core::registry::DynObject> {
    let json = format!(
        r#"{{"$type": "resqml22.BoundaryFeature", "uuid": "{uuid}", "schemaVersion": "2.2",
            "Citation": {{"$type": "eml23.Citation", "Title": "{title}", "Originator": "energyml-rust tests",
                          "Creation": "2026-09-25T00:00:00Z", "Format": "energyml-rust"}},
            "IsWellKnown": false}}"#
    );
    energyml_core::json::read_string(&json, &registry()).expect("the feature reads").root
}

/// A fresh package holding two features.
fn store() -> EpcStore {
    let bytes = empty_package(Some(1_790_000_000)).expect("an empty package");
    let mut store = EpcStore::from_reader(Box::new(Cursor::new(bytes)), "media.epc", registry()).expect("it opens");
    store.put(feature(FEATURE, "shown")).expect("the first feature goes in");
    store.put(feature(OTHER, "not shown")).expect("the second too");
    store
}

fn write(store: &EpcStore) -> (Vec<u8>, Vec<energyml_storage::Diagnostic>) {
    let mut out = Cursor::new(Vec::new());
    let said = store.write_to(&mut out, &SaveOptions::default()).expect("the package writes");
    (out.into_inner(), said)
}

fn reopen(bytes: &[u8]) -> EpcStore {
    EpcStore::from_reader(Box::new(Cursor::new(bytes.to_vec())), "again.epc", registry()).expect("it reads back")
}

// ------------------------------------------------------------- independent read

/// Every entry of the archive, by name — the `zip` crate, nothing of ours.
fn entries(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("a zip");
    let mut out = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("an entry");
        let mut content = Vec::new();
        entry.read_to_end(&mut content).expect("it inflates");
        out.insert(entry.name().to_string(), content);
    }
    out
}

/// The attributes of every element called `name`, in document order.
fn elements(xml: &[u8], name: &str) -> Vec<BTreeMap<String, String>> {
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut buffer = Vec::new();
    let mut out = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer).expect("well-formed XML") {
            Event::Start(element) | Event::Empty(element) if element.local_name().as_ref() == name.as_bytes() => {
                out.push(
                    element
                        .attributes()
                        .map(|attribute| {
                            let attribute = attribute.expect("an attribute");
                            (
                                String::from_utf8_lossy(attribute.key.local_name().as_ref()).into_owned(),
                                quick_xml::escape::unescape(&String::from_utf8_lossy(&attribute.value)).expect("a value").into_owned(),
                            )
                        })
                        .collect(),
                );
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    out
}

/// The part of the object of that uuid, found by its name in the archive.
fn part_of(archive: &BTreeMap<String, Vec<u8>>, uuid: &str) -> String {
    archive
        .keys()
        .find(|name| name.ends_with(&format!("{uuid}.xml")) && !name.ends_with(".rels"))
        .cloned()
        .unwrap_or_else(|| panic!("no part for {uuid} in {:?}", archive.keys().collect::<Vec<_>>()))
}

/// `(type, target, external)` of every relationship of a `.rels`, empty when
/// the archive holds no such `.rels`.
fn relationships(archive: &BTreeMap<String, Vec<u8>>, rels: &str) -> Vec<(String, String, bool)> {
    archive
        .get(rels)
        .map(|xml| {
            elements(xml, "Relationship")
                .into_iter()
                .map(|one| {
                    (
                        one.get("Type").cloned().unwrap_or_default(),
                        one.get("Target").cloned().unwrap_or_default(),
                        one.get("TargetMode").is_some_and(|mode| mode == "External"),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The content type `[Content_Types].xml` gives a part: its `Override`, else
/// the `Default` of its extension — OPC's rule.
fn declared_type(archive: &BTreeMap<String, Vec<u8>>, part: &str) -> Option<String> {
    let index = archive.get("[Content_Types].xml").expect("the index is there");
    let wanted = format!("/{part}");
    if let Some(one) = elements(index, "Override").into_iter().find(|one| one.get("PartName") == Some(&wanted)) {
        return one.get("ContentType").cloned();
    }
    let extension = part.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
    elements(index, "Default")
        .into_iter()
        .find(|one| one.get("Extension").map(|e| e.to_ascii_lowercase()) == Some(extension.clone()))
        .and_then(|one| one.get("ContentType").cloned())
}

/// Both directions of a media relation, as the archive holds them.
fn assert_related(archive: &BTreeMap<String, Vec<u8>>, uuid: &str, media: &str) {
    let object = part_of(archive, uuid);
    let forward = relationships(archive, &format!("_rels/{object}.rels"));
    assert!(
        forward.contains(&(format!("{BASE}destinationMedia"), media.to_string(), false)),
        "the object's .rels does not point at `{media}`: {forward:?}"
    );
    let (folder, file) = media.rsplit_once('/').expect("a media part lives in a folder");
    let up = "../".repeat(folder.split('/').count());
    let backward = relationships(archive, &format!("{folder}/_rels/{file}.rels"));
    assert!(
        backward.contains(&(format!("{BASE}sourceMedia"), format!("{up}{object}"), false)),
        "`{media}`'s .rels does not point back at `{object}`: {backward:?}"
    );
}

// ------------------------------------------------------------------- the tests

#[test]
fn a_media_part_is_written_typed_and_related_both_ways() {
    let mut store = store();
    store
        .set_attachment("media/README.md", README.to_vec(), Some("text/markdown".into()))
        .expect("a README goes in");
    store
        .set_attachment("media/fiche-GTH-1.pdf", binary(), Some("application/pdf".into()))
        .expect("a PDF goes in");
    store.relate_media(FEATURE, "media/README.md").expect("the README shows the feature");
    store.relate_media(FEATURE, "media/fiche-GTH-1.pdf").expect("the PDF too");

    let (bytes, said) = write(&store);
    assert!(
        said.iter().all(|one| one.severity < Severity::Warning),
        "the save complained: {said:?}"
    );
    let archive = entries(&bytes);

    // The bytes, exactly — the binary ones above all.
    assert_eq!(archive.get("media/README.md").map(Vec::as_slice), Some(README));
    assert_eq!(archive.get("media/fiche-GTH-1.pdf"), Some(&binary()));
    // The type the caller stated, not one guessed from the extension.
    assert_eq!(declared_type(&archive, "media/README.md").as_deref(), Some("text/markdown"));
    assert_eq!(declared_type(&archive, "media/fiche-GTH-1.pdf").as_deref(), Some("application/pdf"));
    // Both ways, for both.
    assert_related(&archive, FEATURE, "media/README.md");
    assert_related(&archive, FEATURE, "media/fiche-GTH-1.pdf");
    // And the other feature shows nothing.
    let other = part_of(&archive, OTHER);
    assert!(
        relationships(&archive, &format!("_rels/{other}.rels")).iter().all(|(kind, ..)| !kind.ends_with("Media")),
        "a media relation landed on an object nobody related"
    );

    // The store reads the same thing back.
    let again = reopen(&bytes);
    assert_eq!(again.read_attachment("media/fiche-GTH-1.pdf").expect("the PDF reads"), binary());
    let mut shown = again.media_of(FEATURE).expect("a uuid");
    shown.sort();
    assert_eq!(shown, ["media/README.md", "media/fiche-GTH-1.pdf"]);
    assert!(again.media_of(OTHER).expect("a uuid").is_empty());
}

#[test]
fn a_rewrite_keeps_the_relations_the_archive_held() {
    // A finding of the reference model (ADR-0018): the `.rels` are regenerated from the
    // objects on every save, so a relation read in an archive and not modelled
    // was lost at the next one. Written, reopened with nothing changed, written
    // again: the second package still says it.
    let mut store = store();
    store.set_attachment("media/README.md", README.to_vec(), Some("text/markdown".into())).unwrap();
    store.relate_media(FEATURE, "media/README.md").unwrap();
    let (first, _) = write(&store);

    let (second, said) = write(&reopen(&first));
    assert!(said.iter().all(|one| one.severity < Severity::Warning), "{said:?}");
    let archive = entries(&second);
    assert_related(&archive, FEATURE, "media/README.md");
    assert_eq!(declared_type(&archive, "media/README.md").as_deref(), Some("text/markdown"));
}

#[test]
fn a_relation_read_from_either_end_is_the_same_relation() {
    // A producer may write only one of the two directions — §3.4.1 makes a
    // non-XML part's `.rels` optional. Here the part's side is removed from the
    // archive by hand, and the object's alone must still be read as the link.
    let mut store = store();
    store.set_attachment("media/README.md", README.to_vec(), Some("text/markdown".into())).unwrap();
    store.relate_media(FEATURE, "media/README.md").unwrap();
    let (bytes, _) = write(&store);

    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, content) in entries(&bytes) {
        if name == "media/_rels/README.md.rels" {
            continue;
        }
        out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut out, &content).unwrap();
    }
    let one_way = out.finish().unwrap().into_inner();
    assert!(!entries(&one_way).contains_key("media/_rels/README.md.rels"), "the part's side is gone");

    let again = reopen(&one_way);
    assert_eq!(again.media_of(FEATURE).unwrap(), ["media/README.md"]);
    // And a save puts the missing half back.
    assert_related(&entries(&write(&again).0), FEATURE, "media/README.md");
}

#[test]
fn what_the_specification_forbids_is_refused_by_name() {
    let mut store = store();
    let refused = |result: energyml_storage::Result<()>| result.expect_err("it was accepted").to_string();

    // Not a media type (RFC 2616 §3.7, EPC §2.2.1 p. 9, §3.3 p. 14).
    assert!(refused(store.set_attachment("media/a.md", b"x".to_vec(), Some("markdown".into()))).contains("type/subtype"));
    // The package's own names (§2.2.1, §2.2.2 p. 9).
    assert!(refused(store.set_attachment("media/_rels/a.md", b"x".to_vec(), None)).contains("_rels"));
    // A folder EPC v1.1 §3.6 keeps for data objects.
    assert!(refused(store.set_attachment("namespace_resqml22/a.pdf", b"x".to_vec(), None)).contains("namespace_"));
    // OPC part names.
    assert!(refused(store.set_attachment("media/../a.pdf", b"x".to_vec(), None)).contains(".."));
    // One part twice, as OPC compares names.
    store.set_attachment("media/README.md", README.to_vec(), Some("text/markdown".into())).unwrap();
    assert!(refused(store.set_attachment("media/readme.md", README.to_vec(), None)).contains("differ only by case"));
    // Replacing the same name is not a clash.
    store.set_attachment("media/README.md", README.to_vec(), Some("text/markdown".into())).expect("a replacement");

    // A relation needs both ends in the package.
    assert!(store.relate_media("00000000-0000-4000-8000-000000000000", "media/README.md").is_err());
    let missing = refused(store.relate_media(FEATURE, "media/absent.pdf"));
    assert!(missing.contains("not a file of this package"), "{missing}");
    // An object's own part is not a media representation of anything.
    let object_part = store.find_by_uuid(OTHER).expect("the other feature").locator.clone();
    assert!(refused(store.relate_media(FEATURE, &object_part)).contains("not a file of this package"));
}

#[test]
fn a_rollback_undoes_a_relation() {
    let mut store = store();
    store.set_attachment("media/README.md", README.to_vec(), Some("text/markdown".into())).unwrap();
    store.start_transaction().unwrap();
    store.relate_media(FEATURE, "media/README.md").unwrap();
    assert_eq!(store.media_of(FEATURE).unwrap(), ["media/README.md"], "inside the transaction it holds");
    store.rollback_transaction().unwrap();
    assert!(store.media_of(FEATURE).unwrap().is_empty(), "the rollback left the relation behind");
}

#[test]
fn a_part_removed_takes_its_relations_and_an_object_removed_is_said() {
    let mut store = store();
    store.set_attachment("media/README.md", README.to_vec(), Some("text/markdown".into())).unwrap();
    store.set_attachment("media/fiche-GTH-1.pdf", binary(), Some("application/pdf".into())).unwrap();
    store.relate_media(FEATURE, "media/README.md").unwrap();
    store.relate_media(OTHER, "media/fiche-GTH-1.pdf").unwrap();

    // Removing the file is removing what it said: no complaint.
    store.remove_attachment("media/README.md").unwrap();
    // Removing the object leaves a relation to nothing: named, not written.
    store.remove(OTHER).unwrap();
    let (bytes, said) = write(&store);
    let warned: Vec<&str> =
        said.iter().filter(|one| one.severity >= Severity::Warning).map(|one| one.message.as_str()).collect();
    assert_eq!(warned.len(), 1, "{warned:?}");
    assert!(warned[0].contains(OTHER), "{warned:?}");

    let archive = entries(&bytes);
    assert!(!archive.contains_key("media/README.md"));
    assert!(archive.contains_key("media/fiche-GTH-1.pdf"), "the PDF is still a file of the package");
    let dangling = relationships(&archive, "media/_rels/fiche-GTH-1.pdf.rels");
    assert!(dangling.is_empty(), "a relation to a removed object was written: {dangling:?}");
    let feature = part_of(&archive, FEATURE);
    assert!(
        relationships(&archive, &format!("_rels/{feature}.rels")).iter().all(|(kind, ..)| !kind.ends_with("Media")),
        "the removed README is still related"
    );
}
