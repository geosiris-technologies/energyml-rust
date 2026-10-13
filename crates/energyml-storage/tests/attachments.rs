//! Files that are not objects: putting one in, taking one out, listing them.
//!
//! An EPC is a zip, and §3.6 lets it carry anything — a report, a picture, a
//! note. The reader already copied those through a rewrite; what was missing is
//! the other direction, and the thing worth testing about it is not that a byte
//! survives. It is that **the listing and the file agree after every operation**:
//! a part added exists before the save, a part removed stops existing before the
//! save, and both are still true after the round trip.
//!
//! `testdata/` is git-ignored, so every test skips when the corpus is absent —
//! a test that goes red on a fresh clone teaches people to ignore red tests.

use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_storage::epc::{PartKind, SaveOptions};
use energyml_storage::{EpcStore, LocalPackage};

fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

fn package() -> Option<PathBuf> {
    crate::fixtures::find("testingPackageCpp22.epc")
}

fn open() -> Option<EpcStore> {
    EpcStore::open(package()?, registry()).ok()
}

/// Save to memory and open what came out — the only proof that survives.
fn round_trip(store: &EpcStore) -> EpcStore {
    let mut out = Cursor::new(Vec::new());
    store
        .write_to(&mut out, &SaveOptions::default())
        .expect("the package writes");
    EpcStore::from_reader(Box::new(Cursor::new(out.into_inner())), "rewritten.epc", registry())
        .expect("and reads back")
}

const NOTE: &[u8] = b"%PDF-1.4\n% not really a pdf, but it is bytes\n";

#[test]
fn a_file_put_in_is_listed_before_the_save_and_survives_it() {
    let Some(mut store) = open() else {
        eprintln!("skipped: the corpus is not present");
        return;
    };

    store
        .set_attachment("media/rapport.pdf", NOTE.to_vec(), Some("application/pdf".into()))
        .expect("an attachment goes in");

    // Before the save. A listing that only became right after a round trip
    // would be a listing nobody could act on — the whole point is to show what
    // the package holds *now*.
    let listed = store.parts();
    let entry = listed
        .iter()
        .find(|p| p.part == "media/rapport.pdf")
        .expect("the new part is listed");
    assert_eq!(entry.kind, PartKind::Attachment);
    assert!(entry.added, "a part the archive never had is not marked as added");
    assert_eq!(entry.size, Some(NOTE.len() as u64));
    assert_eq!(entry.content_type.as_deref(), Some("application/pdf"));
    assert!(store.attachments().iter().any(|p| p == "media/rapport.pdf"));
    assert_eq!(store.read_attachment("media/rapport.pdf").unwrap(), NOTE);

    // And after it, with the content type the caller stated — not one guessed
    // from the extension, which is the fallback and not the answer.
    let reopened = round_trip(&store);
    assert_eq!(reopened.read_attachment("media/rapport.pdf").unwrap(), NOTE);
    let entry = reopened
        .parts()
        .into_iter()
        .find(|p| p.part == "media/rapport.pdf")
        .expect("it is in the rewritten package");
    assert_eq!(entry.content_type.as_deref(), Some("application/pdf"));
    assert!(!entry.added, "a part read from an archive is not an addition");
}

#[test]
fn a_file_taken_out_stops_being_listed_and_is_not_written() {
    let Some(mut store) = open() else { return };

    // Whatever this package carries that is not an object. The reference
    // package has some; if it ever has none, the test says so rather than
    // passing over an empty set.
    let Some(victim) = store.attachments().first().cloned() else {
        eprintln!("skipped: this package carries no attachment to remove");
        return;
    };

    store.remove_attachment(&victim).expect("it comes out");
    assert!(
        !store.attachments().contains(&victim),
        "`{victim}` is still listed after being removed"
    );
    let listed = store.parts();
    let entry = listed.iter().find(|p| p.part == victim).expect("still shown, as dropped");
    assert!(entry.dropped, "a removed part is not marked as such");

    let reopened = round_trip(&store);
    assert!(
        reopened.read_attachment(&victim).is_err(),
        "`{victim}` was written out again after being removed"
    );
    // And the objects are all still there: removing a file is not removing the
    // package around it.
    assert_eq!(
        reopened.resources().len(),
        store.resources().len(),
        "removing an attachment changed the object count"
    );
}

#[test]
fn what_may_not_be_written_over_is_refused_by_name() {
    let Some(mut store) = open() else { return };

    // An object's part: a save writes those from the object, so bytes put there
    // would be discarded at the exact moment they were supposed to take effect.
    let object = store.resources().first().expect("a package with objects").locator.clone();
    let refusal = store
        .set_attachment(&object, NOTE.to_vec(), None)
        .expect_err("an object's part was accepted as an attachment");
    let message = format!("{refusal}");
    assert!(message.contains(&object), "{message}");

    // The package's own machinery, all of it regenerated on save.
    for part in ["[Content_Types].xml", "_rels/.rels", "docProps/core.xml"] {
        assert!(
            store.set_attachment(part, NOTE.to_vec(), None).is_err(),
            "`{part}` was accepted, and a save would have discarded it"
        );
    }

    // And removing something that is not there says so, rather than quietly
    // recording a removal that removes nothing.
    assert!(store.remove_attachment("media/nothing.pdf").is_err());
    assert!(
        store.remove_attachment(&object).is_err(),
        "an object was removable as though it were a file"
    );
}

#[test]
fn a_file_can_be_put_back_after_being_removed() {
    let Some(mut store) = open() else { return };

    store
        .set_attachment("media/note.txt", b"un".to_vec(), Some("text/plain".into()))
        .unwrap();
    store.remove_attachment("media/note.txt").unwrap();
    assert!(!store.attachments().iter().any(|p| p == "media/note.txt"));

    // Putting it back is putting it back — not adding a second part named the
    // same, and not leaving it on a removed list that a save would obey.
    store
        .set_attachment("media/note.txt", b"deux".to_vec(), Some("text/plain".into()))
        .unwrap();
    let reopened = round_trip(&store);
    assert_eq!(reopened.read_attachment("media/note.txt").unwrap(), b"deux");
}

#[test]
fn the_listing_is_about_the_package_and_not_about_the_archive() {
    let Some(store) = open() else { return };
    let listed = store.parts();

    // Every kind the reference package has, and the counts that make the view
    // legible. An `Object` count of zero would mean the listing is showing a
    // zip and not a package.
    let objects = listed.iter().filter(|p| p.kind == PartKind::Object).count();
    let rels = listed.iter().filter(|p| p.kind == PartKind::Relationships).count();
    assert!(objects > 100, "{objects} object part(s)");
    assert!(rels > 0, "no `.rels` in the listing");
    assert!(
        listed.iter().any(|p| p.kind == PartKind::ContentTypes),
        "`[Content_Types].xml` is not in the listing"
    );

    // An object part carries its uri, which is what makes the listing a way
    // *into* the package rather than a curiosity.
    let object = listed
        .iter()
        .find(|p| p.kind == PartKind::Object)
        .expect("at least one object");
    assert!(object.uri.as_deref().is_some_and(|u| u.starts_with("eml:///")));
    assert!(object.qualified_type.as_deref().is_some_and(|t| t.contains('.')));
    assert!(object.size.is_some_and(|s| s > 0), "no size for {}", object.part);

    // No directory entries: a zip may hold them and they are not parts.
    assert!(!listed.iter().any(|p| p.part.ends_with('/')));
}
