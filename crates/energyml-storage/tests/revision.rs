//! The revision counter and its log: what changed since a reader last looked.
//!
//! Two groups. The first tests `Revisions` alone. The second tests the wiring
//! in `EpcStore`, one test per write door — see the marker below.

use std::collections::BTreeSet;

use energyml_storage::epc::revision::{Changed, Revisions};

fn set(uuids: &[&str]) -> Changed {
    Changed::Some(uuids.iter().map(|u| u.to_string()).collect::<BTreeSet<_>>())
}

#[test]
fn a_fresh_log_is_at_zero_and_has_nothing() {
    let revisions = Revisions::new(8);
    assert_eq!(revisions.current(), 0);
    assert_eq!(revisions.changed_since(0), Changed::Nothing);
}

#[test]
fn a_touch_advances_and_is_named() {
    let mut revisions = Revisions::new(8);
    assert_eq!(revisions.touch("aaa"), 1);
    assert_eq!(revisions.touch("bbb"), 2);
    assert_eq!(revisions.current(), 2);
    assert_eq!(revisions.changed_since(0), set(&["aaa", "bbb"]));
    assert_eq!(revisions.changed_since(1), set(&["bbb"]));
    assert_eq!(revisions.changed_since(2), Changed::Nothing);
}

#[test]
fn uuids_compare_without_case() {
    let mut revisions = Revisions::new(8);
    revisions.touch("ABCDEF");
    revisions.touch("abcdef");
    assert_eq!(revisions.changed_since(0), set(&["abcdef"]), "one object, named once");
}

#[test]
fn the_same_uuid_touched_twice_appears_once_but_advances_twice() {
    let mut revisions = Revisions::new(8);
    revisions.touch("a");
    revisions.touch("a");
    assert_eq!(revisions.current(), 2);
    assert_eq!(revisions.changed_since(0), set(&["a"]));
}

#[test]
fn overflow_answers_everything_for_a_reader_older_than_the_log() {
    let mut revisions = Revisions::new(2);
    revisions.touch("a"); // 1
    revisions.touch("b"); // 2
    revisions.touch("c"); // 3: "a" is forgotten
    assert_eq!(revisions.changed_since(0), Changed::Everything, "it missed a, which is gone");
    assert_eq!(revisions.changed_since(1), set(&["b", "c"]), "it saw a, so the rest is known");
    assert_eq!(revisions.changed_since(2), set(&["c"]));
    assert_eq!(revisions.changed_since(3), Changed::Nothing);
}

#[test]
fn a_zero_capacity_log_never_names_anything() {
    let mut revisions = Revisions::new(0);
    revisions.touch("a");
    assert_eq!(revisions.changed_since(0), Changed::Everything);
    assert_eq!(revisions.changed_since(1), Changed::Nothing);
}

#[test]
fn touch_all_after_since_means_everything() {
    let mut revisions = Revisions::new(8);
    revisions.touch("a"); // 1
    revisions.touch_all(); // 2
    revisions.touch("b"); // 3
    assert_eq!(revisions.current(), 3);
    assert_eq!(revisions.changed_since(0), Changed::Everything);
    assert_eq!(revisions.changed_since(1), Changed::Everything, "touch_all came after it");
}

#[test]
fn touch_all_before_since_is_irrelevant() {
    let mut revisions = Revisions::new(8);
    revisions.touch_all(); // 1
    revisions.touch("b"); // 2
    assert_eq!(revisions.changed_since(1), set(&["b"]));
    assert_eq!(revisions.changed_since(2), Changed::Nothing);
}

#[test]
fn touch_all_alone_is_not_nothing() {
    let mut revisions = Revisions::new(8);
    let at = revisions.current();
    revisions.touch_all();
    assert_eq!(revisions.changed_since(at), Changed::Everything);
}

#[test]
fn a_revision_from_the_future_is_everything() {
    let mut revisions = Revisions::new(8);
    revisions.touch("a");
    assert_eq!(revisions.changed_since(99), Changed::Everything, "a reader of another store");
}

// =========================================================================
// Needs the wiring in EpcStore: `store.revision() -> u64` and
// `store.changed_since(rev) -> Changed`.
// =========================================================================

mod wiring {
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::{Changed, set};
    use energyml_core::registry::{DynObject, Registry};
    use energyml_storage::EpcStore;

    const RESQML: &str = "http://www.energistics.org/energyml/data/resqmlv2";
    const EML: &str = "http://www.energistics.org/energyml/data/commonv2";

    const A: &str = "7e000000-0000-4000-8000-00000000000a";
    const B: &str = "7e000000-0000-4000-8000-00000000000b";
    const C: &str = "7e000000-0000-4000-8000-00000000000c";

    fn registry() -> Arc<Registry> {
        let mut registry = Registry::new();
        energyml_eml_2_3::register(&mut registry);
        energyml_resqml_2_2::register(&mut registry);
        Arc::new(registry)
    }

    fn feature(uuid: &str) -> String {
        format!(
            "<resqml:BoundaryFeature xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" \
             uuid=\"{uuid}\" schemaVersion=\"2.2\">\
             <eml:Citation><eml:Title>a boundary</eml:Title>\
             <eml:Originator>test</eml:Originator>\
             <eml:Creation>2026-10-01T00:00:00Z</eml:Creation>\
             <eml:Format>test</eml:Format></eml:Citation>\
             <resqml:IsWellKnown>false</resqml:IsWellKnown>\
             </resqml:BoundaryFeature>"
        )
    }

    /// A synthetic package holding one boundary feature per uuid.
    fn package(name: &str, uuids: &[&str]) -> PathBuf {
        // **One folder per call.** `foreign(C)` is called by several tests at
        // once, and the same name wiped and rewrote one file while another test
        // was reading it — "not a zip: Could not find EOCD" in CI.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let call = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("energyml-revision-{name}-{}-{call}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temporary folder");
        let path = dir.join(format!("{name}.epc"));

        let file = std::fs::File::create(&path).expect("creatable");
        let mut zip = zip::ZipWriter::new(file);
        let options: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("[Content_Types].xml", options).unwrap();
        zip.write_all(
            b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
              <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
              <Default Extension=\"rels\" \
              ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
              </Types>",
        )
        .unwrap();
        for uuid in uuids {
            zip.start_file(format!("BoundaryFeature_{uuid}.xml"), options).unwrap();
            zip.write_all(feature(uuid).as_bytes()).unwrap();
        }
        zip.finish().expect("written");
        path
    }

    fn open(path: &PathBuf) -> EpcStore {
        EpcStore::open(path, registry()).expect("opens")
    }

    fn uri_of(store: &EpcStore, uuid: &str) -> String {
        store
            .resources()
            .into_iter()
            .find(|r| r.uuid.eq_ignore_ascii_case(uuid))
            .unwrap_or_else(|| panic!("{uuid} is in the package"))
            .uri
    }

    /// An object that the store being tested does not hold.
    fn foreign(uuid: &str) -> Box<dyn DynObject> {
        let other = open(&package(&format!("foreign-{uuid}"), &[uuid]));
        let uri = uri_of(&other, uuid);
        other.get_object(&uri).expect("readable")
    }

    #[test]
    fn put_of_a_new_object_names_it() {
        let mut store = open(&package("put-new", &[A]));
        let before = store.revision();
        store.put(foreign(B)).expect("put");
        assert!(store.revision() > before);
        assert_eq!(store.changed_since(before), set(&[B]));
    }

    #[test]
    fn put_replacing_an_object_names_it() {
        let mut store = open(&package("put-replace", &[A, B]));
        let before = store.revision();
        store.put(foreign(A)).expect("put");
        assert!(store.revision() > before);
        assert_eq!(store.changed_since(before), set(&[A]));
    }

    #[test]
    fn remove_names_the_object() {
        let mut store = open(&package("remove", &[A, B]));
        let uri = uri_of(&store, B);
        let before = store.revision();
        store.remove(&uri).expect("removed");
        assert!(store.revision() > before);
        assert_eq!(store.changed_since(before), set(&[B]));
    }

    #[test]
    fn an_in_place_edit_names_the_object() {
        let mut store = open(&package("edit", &[A, B]));
        let uri = uri_of(&store, A);
        let before = store.revision();
        store.edit(&uri).expect("borrowed for editing");
        assert!(store.revision() > before, "the borrow is the mark");
        assert_eq!(store.changed_since(before), set(&[A]));
    }

    #[test]
    fn a_committed_transaction_names_what_it_touched() {
        let mut store = open(&package("commit", &[A, B]));
        let uri_b = uri_of(&store, B);
        let before = store.revision();
        store.start_transaction().expect("opens");
        store.put(foreign(C)).expect("put");
        store.remove(&uri_b).expect("removed");
        store.commit_transaction().expect("commits");
        assert!(store.revision() > before);
        assert_eq!(store.changed_since(before), set(&[B, C]));
    }

    /// State changed twice: once going in, once coming back out. A reader that
    /// looked while the transaction was open saw an object that no longer
    /// exists, so the rollback must name the uuids it undid.
    #[test]
    fn a_rolled_back_transaction_names_what_it_undid() {
        let mut store = open(&package("rollback", &[A, B]));
        let uri_b = uri_of(&store, B);
        store.start_transaction().expect("opens");
        store.put(foreign(C)).expect("put");
        store.remove(&uri_b).expect("removed");
        let during = store.revision();

        store.rollback_transaction().expect("rolls back");
        assert!(store.revision() > during, "undoing is a change too");
        match store.changed_since(during) {
            Changed::Some(names) => {
                assert!(names.contains(B) && names.contains(C), "it names both: {names:?}");
            }
            Changed::Everything => {} // acceptable, never Nothing
            Changed::Nothing => panic!("a rollback that changed the state said nothing"),
        }
    }

    #[test]
    fn reads_do_not_advance_the_revision() {
        let store = open(&package("reads", &[A, B]));
        let before = store.revision();
        let uri = uri_of(&store, A);
        let _ = store.resources();
        let _ = store.get_object(&uri).expect("readable");
        assert_eq!(store.revision(), before);
        assert_eq!(store.changed_since(before), Changed::Nothing);
    }
}
