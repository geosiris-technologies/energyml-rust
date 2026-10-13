//! The journal says **what** was written, not only which objects — part of the
//! package-cache design (ADR-0010). The requirements: every modification should
//! say what it applies to so that the optimised cache structures know whom to
//! send change/refresh request signals to and to what link depth, and the
//! cache must also be updated when data is imported from another epc into the
//! current one.
//!
//! One test per gate: the flags it journals, and the datasets. The rule they
//! hold together is **too wide in one direction only** — a gate may say more
//! than it knows, never less — so most assertions are equalities on the exact
//! flags (a gate that says *more* is as wrong as one that says less, where the
//! gate does know) and a few are containments (where it cannot).
//!
//! No `hdf5` feature: the datasets are written through a recording backend that
//! writes nothing, so what is tested is the journal and not a file format.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use energyml_core::registry::{DynObject, Registry};
use energyml_core::value::OwnedValue;
use energyml_storage::array::{ArrayBuffer, ArrayData, ArrayMetadata, ArrayValues};
use energyml_storage::epc::revision::{Change, DatasetId, Since, What};
use energyml_storage::extract::{Arrays, copy_into};
use energyml_storage::import::{FileCarry, Keep, Request, apply_plan, carry_files, plan};
use energyml_storage::{ArrayBackend, EpcStore};

const RESQML: &str = "http://www.energistics.org/energyml/data/resqmlv2";
const EML: &str = "http://www.energistics.org/energyml/data/commonv2";

const A: &str = "7c000000-0000-4000-8000-00000000000a";
const B: &str = "7c000000-0000-4000-8000-00000000000b";
const C: &str = "7c000000-0000-4000-8000-00000000000c";
const I: &str = "7c000000-0000-4000-8000-0000000000e1";
const OTHER: &str = "7c000000-0000-4000-8000-0000000000f0";

fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

fn feature(uuid: &str, title: &str) -> String {
    format!(
        "<resqml:BoundaryFeature xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" \
         uuid=\"{uuid}\" schemaVersion=\"2.2\">\
         <eml:Citation><eml:Title>{title}</eml:Title>\
         <eml:Originator>test</eml:Originator>\
         <eml:Creation>2026-10-05T00:00:00Z</eml:Creation>\
         <eml:Format>test</eml:Format></eml:Citation>\
         <resqml:IsWellKnown>false</resqml:IsWellKnown>\
         </resqml:BoundaryFeature>"
    )
}

/// An interpretation that **references** a feature — the object whose
/// references an edit can move.
fn interpretation(uuid: &str, feature_uuid: &str) -> String {
    format!(
        "<resqml:HorizonInterpretation xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" \
         uuid=\"{uuid}\" schemaVersion=\"2.2\">\
         <eml:Citation><eml:Title>a horizon</eml:Title>\
         <eml:Originator>test</eml:Originator>\
         <eml:Creation>2026-10-05T00:00:00Z</eml:Creation>\
         <eml:Format>test</eml:Format></eml:Citation>\
         <resqml:Domain>depth</resqml:Domain>\
         <resqml:InterpretedFeature>\
         <eml:Uuid>{feature_uuid}</eml:Uuid>\
         <eml:QualifiedType>resqml22.BoundaryFeature</eml:QualifiedType>\
         <eml:Title>a boundary</eml:Title>\
         </resqml:InterpretedFeature>\
         </resqml:HorizonInterpretation>"
    )
}

/// A synthetic package: `(part name, xml)` pairs, one folder per call (several
/// tests run at once and a shared name was how `revision.rs` met a half-written
/// zip in CI).
fn package(name: &str, parts: &[(String, String)]) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let call = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("energyml-journal-{name}-{}-{call}", std::process::id()));
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
    for (part, xml) in parts {
        zip.start_file(part, options).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
    }
    zip.finish().expect("written");
    path
}

fn features(name: &str, uuids: &[&str]) -> PathBuf {
    let parts: Vec<(String, String)> = uuids
        .iter()
        .map(|uuid| (format!("BoundaryFeature_{uuid}.xml"), feature(uuid, "a boundary")))
        .collect();
    package(name, &parts)
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

/// An object the store being tested does not hold.
fn foreign(uuid: &str, title: &str) -> Box<dyn DynObject> {
    let path = package(
        &format!("foreign-{uuid}"),
        &[(format!("BoundaryFeature_{uuid}.xml"), feature(uuid, title))],
    );
    let other = open(&path);
    let uri = uri_of(&other, uuid);
    other.get_object(&uri).expect("readable")
}

/// The window since `from`, which must be exact.
fn since(store: &EpcStore, from: u64) -> Change {
    match store.changes_since(from) {
        Since::Exact(change) => change,
        Since::Everything => panic!("the journal could not say, and it holds a handful of lines"),
    }
}

/// What the window says of one object — `None` when it does not name it.
fn what_of(change: &Change, uuid: &str) -> Option<What> {
    change.objects.iter().find(|object| object.uuid.eq_ignore_ascii_case(uuid)).map(|object| object.what)
}

// ---------------------------------------------------------------------------
// A recording backend: it handles every file, writes nothing, and says so
// ---------------------------------------------------------------------------

struct Silent {
    written: Arc<Mutex<Vec<(String, String)>>>,
}

impl ArrayBackend for Silent {
    fn name(&self) -> &'static str {
        "silent"
    }
    fn handles(&self, _mime: Option<&str>, _file: &std::path::Path) -> bool {
        true
    }
    fn metadata(&self, _file: &std::path::Path, _path: &str) -> energyml_storage::Result<ArrayMetadata> {
        unreachable!("the journal tests read nothing")
    }
    fn read(&self, _f: &std::path::Path, _p: &str, _s: &[i64], _c: &[i64]) -> energyml_storage::Result<ArrayData> {
        unreachable!("the journal tests read nothing")
    }
    fn read_into(
        &self,
        _f: &std::path::Path,
        _p: &str,
        _s: &[i64],
        _c: &[i64],
        _out: &mut ArrayBuffer<'_>,
    ) -> energyml_storage::Result<ArrayMetadata> {
        unreachable!("the journal tests read nothing")
    }
    fn writes(&self) -> bool {
        true
    }
    fn write(&self, file: &std::path::Path, path: &str, _data: &ArrayData) -> energyml_storage::Result<()> {
        self.written.lock().unwrap().push((file.display().to_string(), path.to_string()));
        Ok(())
    }
}

fn numbers(values: usize, dimensions: Vec<i64>) -> ArrayData {
    ArrayData {
        metadata: ArrayMetadata { dimensions, element_type: "double".to_string() },
        values: ArrayValues::F64(vec![1.0; values]),
    }
}

/// A store with a backend that can write, and a place to write to.
fn writable(name: &str) -> EpcStore {
    let mut store = open(&features(name, &[A]));
    store.add_array_backend(Box::new(Silent { written: Arc::default() }));
    store.external_files_mut().set_root(std::env::temp_dir());
    store
}

fn dataset(file: &str, path: &str) -> DatasetId {
    DatasetId { file: file.to_string(), path: path.to_string() }
}

// ---------------------------------------------------------------------------
// The rule of the front end, descended
// ---------------------------------------------------------------------------

/// The front end's `kindOfEdit` and `movesIdentity`, as the module now says them.
#[test]
fn a_path_says_what_an_edit_of_it_changes() {
    assert_eq!(What::of_edit("Citation"), What::CITATION);
    assert_eq!(What::of_edit("Citation.Title"), What::CITATION);
    assert_eq!(What::of_edit("Domain"), What::BODY | What::LINKS_OUT);
    // `Citation` is a path segment, not a prefix of a name.
    assert_eq!(What::of_edit("CitationNote.Text"), What::BODY | What::LINKS_OUT);
    // What may move the identity says everything, the row being keyed by it.
    for path in ["uuid", "Uuid", "UID", "objectVersion", "schemaVersion", "InterpretedFeature.UUID"] {
        let what = What::of_edit(path);
        assert!(what.contains(What::IDENTITY), "{path}: {what:?}");
        assert!(what.contains(What::BROAD), "{path}: {what:?}");
    }
    assert!(!What::of_edit("Citation.Title").contains(What::IDENTITY));
}

// ---------------------------------------------------------------------------
// The object gates
// ---------------------------------------------------------------------------

#[test]
fn an_edit_in_the_citation_says_citation_and_nothing_more() {
    let mut store = open(&features("citation", &[A, B]));
    let uri = uri_of(&store, A);
    let before = store.revision();
    let object = store.edit_at(&uri, "Citation.Title").expect("borrowed");
    energyml_core::path::set(object, "Citation.Title", OwnedValue::Str("renamed".into())).expect("set");

    let change = since(&store, before);
    assert_eq!(what_of(&change, A), Some(What::CITATION));
    assert_eq!(change.objects.len(), 1, "one object, one line: {change:?}");
    assert_eq!(change.objects[0].uri.as_deref(), Some(uri.as_str()), "the store's spelling");
    assert!(change.datasets.is_empty() && !change.files);
}

#[test]
fn an_edit_that_moved_no_reference_does_not_say_links() {
    let mut store = open(&features("body", &[A]));
    let uri = uri_of(&store, A);
    let before = store.revision();
    let object = store.edit_at(&uri, "IsWellKnown").expect("borrowed");
    energyml_core::path::set(object, "IsWellKnown", OwnedValue::Bool(true)).expect("set");
    // The gate said `BODY | LINKS_OUT` because a path cannot tell; the object
    // makes the references it made, and the journal says so when it is read.
    assert_eq!(what_of(&since(&store, before), A), Some(What::BODY));
}

#[test]
fn an_edit_that_moved_a_reference_keeps_links() {
    let parts = vec![
        (format!("BoundaryFeature_{A}.xml"), feature(A, "a boundary")),
        (format!("HorizonInterpretation_{I}.xml"), interpretation(I, A)),
    ];
    let mut store = open(&package("moved", &parts));
    let uri = uri_of(&store, I);
    let before = store.revision();
    // The path names an ordinary field, and the caller changes the reference
    // with it: a gate that trusted the path would miss the move. (Through the
    // front end's own rule, the reference's `Uuid` is an identity path: this is the
    // case the *comparison* exists for.)
    let object = store.edit_at(&uri, "Domain").expect("borrowed");
    energyml_core::path::set(object, "InterpretedFeature.Uuid", OwnedValue::Str(OTHER.into())).expect("set");
    let what = what_of(&since(&store, before), I).expect("named");
    assert_eq!(what, What::BODY | What::LINKS_OUT, "the reference moved: {what:?}");
}

#[test]
fn an_edit_that_may_move_the_identity_says_so_and_is_not_narrowed() {
    let mut store = open(&features("identity", &[A]));
    let uri = uri_of(&store, A);
    let before = store.revision();
    // A uuid edit may move the identity. (An `objectVersion` edit of an entry with
    // no namesake does not: it keeps its slot and its short uri, and says the
    // citation — see `object_versions.rs`.)
    store.edit_at(&uri, "uuid").expect("borrowed");
    let what = what_of(&since(&store, before), A).expect("named");
    assert!(what.contains(What::IDENTITY) && what.contains(What::BROAD), "{what:?}");
}

#[test]
fn an_in_place_borrow_with_no_path_says_everything_broad() {
    let mut store = open(&features("borrow", &[A]));
    let uri = uri_of(&store, A);
    let before = store.revision();
    store.edit(&uri).expect("borrowed");
    assert_eq!(what_of(&since(&store, before), A), Some(What::BROAD));
}

#[test]
fn a_put_of_a_new_uuid_says_created() {
    let mut store = open(&features("put-new", &[A]));
    let before = store.revision();
    let uri = store.put(foreign(B, "new")).expect("put");
    let change = since(&store, before);
    assert_eq!(what_of(&change, B), Some(What::CREATED));
    assert_eq!(change.objects[0].uri.as_deref(), Some(uri.as_str()));
}

#[test]
fn a_put_of_an_existing_object_says_broad_not_created() {
    let mut store = open(&features("put-replace", &[A, B]));
    let before = store.revision();
    store.put(foreign(A, "another A")).expect("put");
    assert_eq!(what_of(&since(&store, before), A), Some(What::BROAD));
}

#[test]
fn a_remove_says_removed_and_keeps_the_uri_it_was_given() {
    let mut store = open(&features("remove", &[A, B]));
    let uri = uri_of(&store, B);
    let before = store.revision();
    store.remove(&uri).expect("removed");
    let change = since(&store, before);
    assert_eq!(what_of(&change, B), Some(What::REMOVED));
    assert_eq!(change.objects[0].uri.as_deref(), Some(uri.as_str()), "a removed object is still named");
}

/// Several writes of one window are **one line per object**, and the order
/// matters: gone-then-back is a replacement, new-then-gone is gone.
#[test]
fn the_writes_of_a_window_merge_per_object() {
    let mut store = open(&features("merge", &[A, B]));
    let uri_a = uri_of(&store, A);
    let uri_b = uri_of(&store, B);
    let before = store.revision();

    store.edit_at(&uri_a, "Citation.Title").expect("a title");
    store.edit_at(&uri_a, "IsWellKnown").expect("a body");
    store.remove(&uri_b).expect("removed");
    store.put(foreign(B, "back")).expect("put again");
    let c_uri = store.put(foreign(C, "short lived")).expect("put");
    store.remove(&c_uri).expect("removed");

    let change = since(&store, before);
    let a = what_of(&change, A).expect("A");
    assert!(a.contains(What::CITATION) && a.contains(What::BODY), "both edits survive: {a:?}");
    let b = what_of(&change, B).expect("B");
    assert!(b.contains(What::CREATED) && b.contains(What::BROAD) && !b.contains(What::REMOVED), "back: {b:?}");
    assert_eq!(what_of(&change, C), Some(What::REMOVED), "a reader never saw it, and is told it is gone");
    assert_eq!(change.objects.len(), 3);
}

/// A reader that looked while the transaction was open saw states that no longer
/// exist: every object it touched is named with every flag.
#[test]
fn an_aborted_transaction_says_everything_for_each_object_it_touched() {
    let mut store = open(&features("rollback", &[A, B]));
    let uri_b = uri_of(&store, B);
    let before = store.revision();
    store.start_transaction().expect("opens");
    store.put(foreign(C, "in a transaction")).expect("put");
    store.remove(&uri_b).expect("removed");
    let during = store.revision();
    store.rollback_transaction().expect("rolls back");

    let undone = since(&store, during);
    assert_eq!(what_of(&undone, B), Some(What::EVERYTHING), "{undone:?}");
    assert_eq!(what_of(&undone, C), Some(What::EVERYTHING), "{undone:?}");
    assert_eq!(undone.objects.len(), 2, "and nothing else");

    // A reader older than the transaction: still told about both.
    let whole = since(&store, before);
    for uuid in [B, C] {
        let what = what_of(&whole, uuid).unwrap_or_else(|| panic!("{uuid} is named"));
        assert!(what.contains(What::BROAD), "{uuid}: {what:?}");
    }
}

#[test]
fn a_part_staged_as_bytes_has_no_name_to_give_so_the_journal_says_everything() {
    let mut store = open(&features("raw", &[A]));
    let before = store.revision();
    store.set_raw_part("BoundaryFeature_x.xml", b"<broken".to_vec());
    assert_eq!(store.changes_since(before), Since::Everything);
}

#[test]
fn a_revision_this_store_never_issued_is_everything_not_nothing() {
    let store = open(&features("future", &[A]));
    assert_eq!(store.changes_since(store.revision() + 50), Since::Everything);
}

#[test]
fn reading_says_nothing() {
    let store = open(&features("reads", &[A, B]));
    let before = store.revision();
    let _ = store.get_object(&uri_of(&store, A)).expect("readable");
    let _ = store.resources();
    assert!(since(&store, before).is_empty());
}

#[test]
fn changed_since_still_names_the_objects_for_the_index() {
    let mut store = open(&features("index", &[A, B]));
    let before = store.revision();
    store.put(foreign(C, "new")).expect("put");
    store.arrays_written("x.h5", &["/a"]);
    // The package index reads uuids only: the dataset is not one of them, and
    // the set is what it was before the journal grew.
    match store.changed_since(before) {
        energyml_storage::epc::revision::Changed::Some(names) => {
            assert_eq!(names.into_iter().collect::<Vec<_>>(), vec![C.to_string()])
        }
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// The data gates
// ---------------------------------------------------------------------------

#[test]
fn write_array_to_journals_the_dataset_it_wrote() {
    let store = writable("one-array");
    let before = store.revision();
    store.write_array_to("model.h5", "/a/points", &numbers(6, vec![2, 3])).expect("written");
    let change = since(&store, before);
    assert_eq!(change.datasets, vec![dataset("model.h5", "/a/points")]);
    assert!(change.objects.is_empty(), "an array is not an object: {change:?}");
}

#[test]
fn write_arrays_to_journals_the_datasets_it_wrote_and_only_those() {
    let store = writable("many-arrays");
    let before = store.revision();
    let good = numbers(4, vec![2, 2]);
    let lying = numbers(3, vec![2, 2]);
    let results = store.write_arrays_to("model.h5", &[("/b", &good), ("/lie", &lying), ("/c", &good)]);
    assert!(results[0].is_ok() && results[1].is_err() && results[2].is_ok());
    // The shape that disagrees was refused before the file was touched.
    assert_eq!(since(&store, before).datasets, vec![dataset("model.h5", "/b"), dataset("model.h5", "/c")]);
}

#[test]
fn a_refused_write_before_the_file_is_reached_journals_nothing() {
    let store = writable("refused");
    let before = store.revision();
    assert!(store.write_array_to("model.h5", "/lie", &numbers(3, vec![2, 2])).is_err());
    assert!(since(&store, before).is_empty());
}

/// The gate for whoever wrote arrays without this store — the front end, through
/// h5wasm.
#[test]
fn arrays_written_says_what_a_face_wrote_itself() {
    let store = open(&features("outside", &[A]));
    let before = store.revision();
    store.arrays_written("model.h5", &["/p", "/q"]);
    store.arrays_written("other.h5", &["/r"]);
    let change = since(&store, before);
    assert_eq!(
        change.datasets,
        vec![dataset("model.h5", "/p"), dataset("model.h5", "/q"), dataset("other.h5", "/r")]
    );
    assert!(store.revision() > before, "the revision moves: whoever follows it is told");
    // No paths: the file as a whole.
    let mid = store.revision();
    store.arrays_written("whole.h5", &[]);
    assert_eq!(since(&store, mid).datasets, vec![dataset("whole.h5", "")]);
}

#[test]
fn a_dataset_written_twice_is_named_once() {
    let store = writable("twice");
    let before = store.revision();
    store.write_array_to("model.h5", "/p", &numbers(1, vec![1])).expect("written");
    store.write_array_to("model.h5", "/p", &numbers(1, vec![1])).expect("written");
    assert_eq!(since(&store, before).datasets.len(), 1);
}

/// A reader between two consecutive writes of one file still gets both: the
/// shared line takes the newest revision.
#[test]
fn consecutive_writes_into_one_file_share_a_line_without_hiding_the_first() {
    let store = writable("shared-line");
    store.write_array_to("model.h5", "/p", &numbers(1, vec![1])).expect("written");
    let between = store.revision();
    store.write_array_to("model.h5", "/q", &numbers(1, vec![1])).expect("written");
    store.write_array_to("model.h5", "/r", &numbers(1, vec![1])).expect("written");
    let after = since(&store, between).datasets;
    assert!(after.contains(&dataset("model.h5", "/q")) && after.contains(&dataset("model.h5", "/r")), "{after:?}");
    // Too wide, never too narrow: the shared line names `/p` again.
    assert_eq!(since(&store, 0).datasets.len(), 3);
}

#[test]
fn a_file_name_given_to_a_proxy_is_a_line_of_its_own() {
    let mut store = open(&features("renamed", &[A]));
    let before = store.revision();
    store.register_external_file("7C000000-0000-4000-8000-0000000000AA", "moved.h5");
    let change = since(&store, before);
    assert_eq!(change.renamed, vec![("7c000000-0000-4000-8000-0000000000aa".to_string(), "moved.h5".to_string())]);
}

#[test]
fn a_part_or_a_media_relation_moves_the_files_flag() {
    let mut store = open(&features("files", &[A]));
    let before = store.revision();
    store.set_attachment("docs/note.txt", b"hello".to_vec(), Some("text/plain".into())).expect("attached");
    assert!(since(&store, before).files);
    let mid = store.revision();
    let uri = uri_of(&store, A);
    store.relate_media(&uri, "docs/note.txt").expect("related");
    let change = since(&store, mid);
    assert!(change.files && change.objects.is_empty(), "{change:?}");
}

// ---------------------------------------------------------------------------
// Imports: what brings objects from another package goes through these gates
// ---------------------------------------------------------------------------

/// `copy_into` is `put` per object: a new uuid is `CREATED`, a uuid the target
/// holds is replaced, `BROAD`.
#[test]
fn copying_objects_into_a_package_says_created_and_replaced() {
    let from = open(&package(
        "copy-from",
        &[
            (format!("BoundaryFeature_{A}.xml"), feature(A, "from A")),
            (format!("BoundaryFeature_{B}.xml"), feature(B, "from B")),
        ],
    ));
    let mut into = open(&features("copy-into", &[B]));
    let before = into.revision();
    let wanted = vec![uri_of(&from, A), uri_of(&from, B)];
    copy_into(&from, &mut into, &wanted, Arrays::Leave).expect("copied");

    let change = since(&into, before);
    assert_eq!(what_of(&change, A), Some(What::CREATED), "{change:?}");
    assert_eq!(what_of(&change, B), Some(What::BROAD), "{change:?}");
}

/// The front end's import: the plan is applied in memory (objects through `put`), the
/// bytes follow — by `copy_datasets`/`carry_files` on a disk, by h5wasm in a
/// front end, which then says so through `arrays_written`.
#[test]
fn an_import_plan_says_created_and_replaced_and_a_carried_file_says_the_file() {
    let from = open(&package(
        "import-from",
        &[
            (format!("BoundaryFeature_{A}.xml"), feature(A, "imported A")),
            (format!("BoundaryFeature_{B}.xml"), feature(B, "imported B")),
        ],
    ));
    let mut into = open(&features("import-into", &[B]));
    let before = into.revision();

    let request = Request { all: Some(Keep::Imported), ..Default::default() };
    let quote = plan(&from, &into, &request);
    assert!(quote.is_allowed(), "{:?}", quote.refusals);
    apply_plan(&from, &mut into, &quote).expect("applied");

    let change = since(&into, before);
    assert_eq!(what_of(&change, A), Some(What::CREATED), "{change:?}");
    let replaced = what_of(&change, B).expect("B is named");
    assert!(replaced.contains(What::BROAD) && !replaced.contains(What::CREATED), "{replaced:?}");

    // A data file carried beside the package.
    let dir = std::env::temp_dir().join(format!("energyml-journal-carry-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let held = dir.join("imported.h5");
    std::fs::write(&held, b"bytes").unwrap();
    into.external_files_mut().set_root(&dir);
    let mid = into.revision();
    carry_files(
        &into,
        &[FileCarry { from: "imported.h5".into(), held: held.display().to_string(), to: "carried.h5".into() }],
    )
    .expect("carried");
    assert_eq!(since(&into, mid).datasets, vec![dataset("carried.h5", "")]);
}
