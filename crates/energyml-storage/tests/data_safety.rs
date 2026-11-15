//! Where the store could lose, overwrite or misjudge something - the cases of
//! the review of 15/11, one test each, built **in memory** (a package saved to a
//! temporary folder when a file has to exist), never from the corpus.
//!
//! Each test says what to break to make it red.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use energyml_core::registry::{DynObject, Registry};
use energyml_storage::array::{ArrayBuffer, ArrayData, ArrayMetadata, ArrayValues, locate};
use energyml_storage::extract::{Arrays, copy_into, empty_package, extract};
use energyml_storage::graph::{self, Related};
use energyml_storage::resource::{ObjectSummary, Relationship};
use energyml_storage::{ArrayBackend, EpcStore, SaveOptions, rename};

const RESQML: &str = "http://www.energistics.org/energyml/data/resqmlv2";
const EML: &str = "http://www.energistics.org/energyml/data/commonv2";

const A: &str = "7d000000-0000-4000-8000-00000000000a";
const B: &str = "7d000000-0000-4000-8000-00000000000b";
const GHOST: &str = "7d000000-0000-4000-8000-0000000000ff";
const F: &str = "7d000000-0000-4000-8000-0000000000f1";
const F2: &str = "7d000000-0000-4000-8000-0000000000f2";
const H: &str = "7d000000-0000-4000-8000-0000000000a1";
const H2: &str = "7d000000-0000-4000-8000-0000000000a2";

fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

/// A folder of its own per call: the tests of this file run in parallel.
fn folder(name: &str) -> PathBuf {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let call = CALLS.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("energyml-safety-{name}-{}-{call}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a temporary folder");
    dir
}

/// A new, empty package held in memory, with `dir` as the folder its data
/// files live in.
fn fresh(dir: &Path) -> EpcStore {
    let bytes = empty_package(None).expect("an empty package");
    let mut store = EpcStore::from_reader(Box::new(std::io::Cursor::new(bytes)), "new.epc", registry())
        .expect("it opens");
    store.external_files_mut().set_root(dir);
    store
}

/// A `CmpLineFeature` whose `TraceLabels` is an eml 2.3 external array naming
/// `file`: an object that carries the name of a data file in a field of its own.
fn lines(uuid: &str, file: &str) -> Box<dyn DynObject> {
    use energyml_eml_2_3::{Citation, ExternalDataArray, ExternalDataArrayPart, StringExternalArray};
    Box::new(energyml_resqml_2_2::CmpLineFeature {
        uuid: uuid.to_string(),
        schema_version: "2.2".to_string(),
        citation: Citation {
            title: "labels".to_string(),
            ..Default::default()
        },
        trace_labels: Some(StringExternalArray {
            count_per_value: 1,
            values: ExternalDataArray {
                external_data_array_part: vec![ExternalDataArrayPart {
                    count: vec![3],
                    path_in_external_file: "/labels".to_string(),
                    start_index: vec![0],
                    uri: file.to_string(),
                    mime_type: Some("application/x-hdf5".to_string()),
                }],
            },
            ..Default::default()
        }),
        ..Default::default()
    })
}

fn uri_of_lines(uuid: &str) -> String {
    format!("eml:///resqml22.CmpLineFeature({uuid})")
}

/// The file the object's first array names, read through `locate`.
fn file_named(store: &EpcStore, uri: &str) -> String {
    let object = store.get_object(uri).expect("the object reads");
    locate(object.as_reflect())
        .first()
        .expect("it carries an array pointer")
        .file
        .clone()
}

// ---------------------------------------------------------------------------
// [storage-1 S7] a rename never replaces a file the package does not name
// ---------------------------------------------------------------------------

/// `fs::rename` replaces its destination. A file already named `to` beside the
/// package is not this package's: refused in the quote, and again at the
/// bill, because a plan can be stale.
///
/// Break it: remove the `disk_refusal` calls of `rename::plan` and
/// `rename::apply`; the file `stray.h5` is replaced and the first assertion fails.
#[test]
fn a_rename_never_replaces_a_file_the_package_does_not_name() {
    let dir = folder("rename-overwrite");
    std::fs::write(dir.join("alpha.h5"), b"the package's data").unwrap();
    std::fs::write(dir.join("stray.h5"), b"somebody else's data").unwrap();
    let mut store = fresh(&dir);
    let uri_a = store.put(lines(A, "alpha.h5")).expect("put");

    // The quote says no.
    let quote = rename::plan(&store, "alpha.h5", "stray.h5");
    assert!(!quote.is_allowed(), "the quote allowed it: {quote:?}");
    assert!(
        quote.refusals.iter().any(|line| line.contains("stray.h5")),
        "{:?}",
        quote.refusals
    );
    assert!(rename::apply(&mut store, &quote).is_err());

    // A plan made before the file appeared is checked again at the bill.
    let stale = rename::plan(&store, "alpha.h5", "late.h5");
    assert!(stale.is_allowed(), "{:?}", stale.refusals);
    std::fs::write(dir.join("late.h5"), b"arrived after the quote").unwrap();
    let before = file_named(&store, &uri_a);
    assert!(
        rename::apply(&mut store, &stale).is_err(),
        "the bill replaced a file"
    );

    assert_eq!(
        std::fs::read(dir.join("stray.h5")).unwrap(),
        b"somebody else's data"
    );
    assert_eq!(
        std::fs::read(dir.join("late.h5")).unwrap(),
        b"arrived after the quote"
    );
    assert_eq!(
        std::fs::read(dir.join("alpha.h5")).unwrap(),
        b"the package's data"
    );
    assert_eq!(
        file_named(&store, &uri_a),
        before,
        "and the package was not rewritten either"
    );
}

// ---------------------------------------------------------------------------
// [storage-1 S8] a rename that fails leaves nothing half done
// ---------------------------------------------------------------------------

/// A plan naming an object that has become unreadable: the objects before it
/// must not be rewritten.
///
/// Break it: delete the trial loop of step 0 in `rename::apply`; `A` is then
/// rewritten before `GHOST` fails, and the last assertion fails.
#[test]
fn a_rename_that_fails_midway_rewrites_nothing() {
    let dir = folder("rename-atomic");
    let mut store = fresh(&dir);
    let uri_a = store.put(lines(A, "alpha.h5")).expect("put");
    let uri_b = store.put(lines(B, "alpha.h5")).expect("put");

    let mut plan = rename::plan(&store, "alpha.h5", "new.h5");
    assert!(plan.is_allowed(), "{:?}", plan.refusals);
    assert_eq!(plan.objects.len(), 2);
    // Stale: the third object it names is not there any more.
    plan.objects.push(uri_of_lines(GHOST));

    assert!(rename::apply(&mut store, &plan).is_err());
    for (uuid, uri) in [(A, &uri_a), (B, &uri_b)] {
        assert_eq!(
            file_named(&store, uri),
            "alpha.h5",
            "{uuid} was rewritten by a rename that failed"
        );
    }

    // And the honest plan still goes through.
    let honest = rename::plan(&store, "alpha.h5", "new.h5");
    rename::apply(&mut store, &honest).expect("the rename is carried out");
    assert_eq!(file_named(&store, &uri_a), "new.h5");
}

// ---------------------------------------------------------------------------
// [storage-2 S2] a rollback brings a removed object back whole
// ---------------------------------------------------------------------------

/// An object created and committed, removed inside a transaction that is rolled
/// back, is **found** again - not listed and refused.
///
/// Break it: drop the entry from `made` again in `EpcStore::remove`
/// (`self.made.get_mut(&uuid)...retain`); `get_object` answers `NotFound`.
#[test]
fn a_rollback_brings_back_an_object_removed_in_the_transaction() {
    let dir = folder("rollback-made");
    let mut store = fresh(&dir);
    let uri = store.put(lines(A, "alpha.h5")).expect("put");

    store.start_transaction().expect("opens");
    store.remove(&uri).expect("removed in the transaction");
    assert!(
        store.get_object(&uri).is_err(),
        "gone while the transaction is open"
    );
    store.rollback_transaction().expect("rolls back");

    assert!(
        store.get_object(&uri).is_ok(),
        "listed but refused after the rollback: {:?}",
        store.get_object(&uri).err()
    );
    assert!(store.resources().iter().any(|one| one.uuid == A));
}

// ---------------------------------------------------------------------------
// [storage-2 S14] a rollback puts an attachment back
// ---------------------------------------------------------------------------

fn zip_names(path: &Path) -> Vec<String> {
    let file = std::fs::File::open(path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().expect("a part name").into_owned())
        .collect()
}

/// Removing an attachment of the archive inside a transaction, then rolling
/// back, leaves it in the saved package.
///
/// Break it: make `Overlay::drop_part` write to `dropped` whatever the
/// transaction state; the saved package then lacks `notes.txt`.
#[test]
fn a_rollback_undoes_the_removal_of_an_attachment() {
    let dir = folder("rollback-attachment");
    let mut first = fresh(&dir);
    first
        .set_attachment("notes.txt", b"hello".to_vec(), Some("text/plain".to_string()))
        .expect("attached");
    let path = dir.join("with-notes.epc");
    first.save_as(&path, &SaveOptions::default()).expect("saved");

    let mut store = EpcStore::open(&path, registry()).expect("reopens");
    assert!(zip_names(&path).contains(&"notes.txt".to_string()));

    store.start_transaction().expect("opens");
    store
        .remove_attachment("notes.txt")
        .expect("removed in the transaction");
    // Inside the transaction it is gone, and a part added there is too transient
    // to survive: both are undone together.
    store
        .set_attachment("extra.txt", b"x".to_vec(), Some("text/plain".to_string()))
        .expect("attached in the transaction");
    store.rollback_transaction().expect("rolls back");

    let out = dir.join("after-rollback.epc");
    store.save_as(&out, &SaveOptions::default()).expect("saved");
    let names = zip_names(&out);
    assert!(
        names.contains(&"notes.txt".to_string()),
        "the attachment was lost: {names:?}"
    );
    assert!(!names.contains(&"extra.txt".to_string()), "{names:?}");
}

// ---------------------------------------------------------------------------
// [storage-2 S5] extract never truncates its source
// ---------------------------------------------------------------------------

/// Extracting onto the package being read is refused before one byte is written.
///
/// Break it: remove the `same_file` guard of `extract::extract`; the source is
/// overwritten by an empty package and the byte comparison fails.
#[test]
fn extract_refuses_the_package_it_reads_as_its_output() {
    let dir = folder("extract-source");
    let mut first = fresh(&dir);
    let uri = first.put(lines(A, "alpha.h5")).expect("put");
    let path = dir.join("source.epc");
    first.save_as(&path, &SaveOptions::default()).expect("saved");
    let original = std::fs::read(&path).unwrap();

    let source = EpcStore::open(&path, registry()).expect("opens");
    let outcome = extract(
        &source,
        &[uri.clone()],
        &path,
        Arrays::Leave,
        &SaveOptions::default(),
    );
    assert!(outcome.is_err(), "extracting onto the source was allowed");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        original,
        "the source package was modified"
    );
    assert!(source.get_object(&uri).is_ok(), "and it still reads");
    assert!(
        !dir.join("source.epc.partial").exists(),
        "no staging file is left behind"
    );

    // A different output works, and leaves no staging file either.
    let other = dir.join("cut.epc");
    extract(&source, &[uri], &other, Arrays::Leave, &SaveOptions::default()).expect("a real cut");
    assert!(other.exists());
    assert!(!dir.join("cut.epc.partial").exists());
}

// ---------------------------------------------------------------------------
// [storage-2 S4] copy_into copies a shared dataset once
// ---------------------------------------------------------------------------

/// A backend that reads and writes nothing real and counts both.
struct Counting {
    reads: Arc<AtomicUsize>,
    writes: Arc<Mutex<Vec<String>>>,
}

fn three() -> ArrayData {
    ArrayData {
        metadata: ArrayMetadata::new(vec![3], "double".to_string()),
        values: ArrayValues::F64(vec![1.0, 2.0, 3.0]),
    }
}

impl ArrayBackend for Counting {
    fn name(&self) -> &'static str {
        "counting"
    }
    fn handles(&self, _mime: Option<&str>, _file: &Path) -> bool {
        true
    }
    fn metadata(&self, _file: &Path, _path: &str) -> energyml_storage::Result<ArrayMetadata> {
        Ok(three().metadata)
    }
    fn read(&self, _f: &Path, _p: &str, _s: &[i64], _c: &[i64]) -> energyml_storage::Result<ArrayData> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        Ok(three())
    }
    fn read_into(
        &self,
        _f: &Path,
        _p: &str,
        _s: &[i64],
        _c: &[i64],
        _out: &mut ArrayBuffer<'_>,
    ) -> energyml_storage::Result<ArrayMetadata> {
        Ok(three().metadata)
    }
    fn writes(&self) -> bool {
        true
    }
    fn write(&self, file: &Path, path: &str, _data: &ArrayData) -> energyml_storage::Result<()> {
        self.writes
            .lock()
            .unwrap()
            .push(format!("{}:{path}", file.display()));
        Ok(())
    }
}

/// Two objects naming one dataset: it is read and written once, and counted once.
///
/// Break it: remove the `seen` guard of `copy_into`; two reads, two writes.
#[test]
fn copy_into_copies_a_dataset_two_objects_share_once() {
    let source_dir = folder("copy-source");
    std::fs::write(source_dir.join("alpha.h5"), b"x").unwrap();
    let mut first = fresh(&source_dir);
    let uri_a = first.put(lines(A, "alpha.h5")).expect("put");
    let uri_b = first.put(lines(B, "alpha.h5")).expect("put");
    let path = source_dir.join("source.epc");
    first.save_as(&path, &SaveOptions::default()).expect("saved");

    let reads = Arc::new(AtomicUsize::new(0));
    let writes = Arc::new(Mutex::new(Vec::new()));
    let mut source = EpcStore::open(&path, registry()).expect("opens");
    source.add_array_backend(Box::new(Counting {
        reads: reads.clone(),
        writes: writes.clone(),
    }));

    let target_dir = folder("copy-target");
    let mut into = fresh(&target_dir);
    into.add_array_backend(Box::new(Counting {
        reads: reads.clone(),
        writes: writes.clone(),
    }));

    let done = copy_into(&source, &mut into, &[uri_a, uri_b], Arrays::Copy).expect("copied");
    assert_eq!(done.objects.len(), 2, "{done:?}");
    assert_eq!(done.arrays, 1, "one dataset counted once: {done:?}");
    assert_eq!(reads.load(Ordering::Relaxed), 1, "read once");
    assert_eq!(
        writes.lock().unwrap().len(),
        1,
        "written once: {:?}",
        writes.lock().unwrap()
    );
}

// ---------------------------------------------------------------------------
// [storage-2 S3] the relationship check does not merge versions
// ---------------------------------------------------------------------------

fn feature(uuid: &str, version: Option<&str>) -> String {
    let version = version
        .map(|v| format!(" objectVersion=\"{v}\""))
        .unwrap_or_default();
    format!(
        "<resqml:BoundaryFeature xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" \
         uuid=\"{uuid}\" schemaVersion=\"2.2\"{version}>\
         <eml:Citation><eml:Title>a boundary</eml:Title>\
         <eml:Originator>test</eml:Originator>\
         <eml:Creation>2026-11-15T00:00:00Z</eml:Creation>\
         <eml:Format>test</eml:Format></eml:Citation>\
         <resqml:IsWellKnown>false</resqml:IsWellKnown>\
         </resqml:BoundaryFeature>"
    )
}

/// A horizon at `version` naming `feature` (at `feature_version` when given).
fn horizon(uuid: &str, version: &str, feature: &str, feature_version: Option<&str>) -> String {
    let feature_version = feature_version
        .map(|v| format!("<eml:ObjectVersion>{v}</eml:ObjectVersion>"))
        .unwrap_or_default();
    format!(
        "<resqml:HorizonInterpretation xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" \
         uuid=\"{uuid}\" schemaVersion=\"2.2\" objectVersion=\"{version}\">\
         <eml:Citation><eml:Title>a horizon</eml:Title>\
         <eml:Originator>test</eml:Originator>\
         <eml:Creation>2026-11-15T00:00:00Z</eml:Creation>\
         <eml:Format>test</eml:Format></eml:Citation>\
         <resqml:Domain>depth</resqml:Domain>\
         <resqml:InterpretedFeature>\
         <eml:Uuid>{feature}</eml:Uuid>{feature_version}\
         <eml:QualifiedType>resqml22.BoundaryFeature</eml:QualifiedType>\
         <eml:Title>a boundary</eml:Title>\
         </resqml:InterpretedFeature>\
         </resqml:HorizonInterpretation>"
    )
}

fn read(xml: &str, kind: &str) -> Box<dyn DynObject> {
    registry()
        .read_xml(
            &format!("application/x-resqml+xml;version=2.2;type={kind}"),
            xml.as_bytes(),
        )
        .expect("the object reads")
}

fn summary(kind: &str, uuid: &str, version: Option<&str>, part: &str) -> ObjectSummary {
    let uri = match version {
        Some(version) => format!("eml:///resqml22.{kind}(uuid={uuid},version='{version}')"),
        None => format!("eml:///resqml22.{kind}({uuid})"),
    };
    ObjectSummary::new(uri, uuid.to_string(), format!("resqml22.{kind}")).with_locator(part.to_string())
}

fn link(kind: &str, id: &str, target: &str) -> Relationship {
    Relationship {
        id: id.to_string(),
        kind: format!("http://schemas.energistics.org/package/2012/relationships/{kind}"),
        target: target.to_string(),
        external: false,
    }
}

/// Two versions of one horizon live under `version_<id>/` with the same file
/// name (what this crate's own writer does), each naming its own feature. Every
/// relationship is right; the check must say nothing.
///
/// Break it: key `by_part` on the file name and `references` on the uuid again;
/// the horizon of version 1 is judged against the edges of version 2.
#[test]
fn versioned_namesakes_are_not_merged_by_the_relationship_check() {
    let summaries = [
        summary("BoundaryFeature", F, None, "BoundaryFeature_F.xml"),
        summary("BoundaryFeature", F2, None, "BoundaryFeature_F2.xml"),
        summary(
            "HorizonInterpretation",
            H,
            Some("1"),
            "version_1/HorizonInterpretation_H.xml",
        ),
        summary(
            "HorizonInterpretation",
            H,
            Some("2"),
            "version_2/HorizonInterpretation_H.xml",
        ),
    ];
    let objects = [
        read(&feature(F, None), "BoundaryFeature"),
        read(&feature(F2, None), "BoundaryFeature"),
        read(&horizon(H, "1", F, None), "HorizonInterpretation"),
        read(&horizon(H, "2", F2, None), "HorizonInterpretation"),
    ];
    let relationships = [
        vec![link(
            "sourceObject",
            "r1",
            "version_1/HorizonInterpretation_H.xml",
        )],
        vec![link(
            "sourceObject",
            "r2",
            "version_2/HorizonInterpretation_H.xml",
        )],
        vec![link("destinationObject", "r3", "../BoundaryFeature_F.xml")],
        vec![link("destinationObject", "r4", "../BoundaryFeature_F2.xml")],
    ];
    let related: Vec<Related<'_>> = summaries
        .iter()
        .zip(&objects)
        .zip(&relationships)
        .map(|((summary, object), relationships)| Related {
            summary,
            object: object.as_reflect(),
            relationships,
        })
        .collect();

    let findings = graph::check_relationships(&related);
    assert!(
        findings.is_empty(),
        "false findings on a consistent package: {findings:#?}"
    );

    // And a relationship that really is wrong is still caught: version 2 declares
    // a link to the first feature, which it does not reference.
    let mut wrong = relationships.clone();
    wrong[3] = vec![link("destinationObject", "r4", "../BoundaryFeature_F.xml")];
    let related: Vec<Related<'_>> = summaries
        .iter()
        .zip(&objects)
        .zip(&wrong)
        .map(|((summary, object), relationships)| Related {
            summary,
            object: object.as_reflect(),
            relationships,
        })
        .collect();
    let findings = graph::check_relationships(&related);
    assert!(
        findings
            .iter()
            .any(|one| one.part == "version_2/HorizonInterpretation_H.xml"),
        "the real defect is masked: {findings:#?}"
    );
}

// ---------------------------------------------------------------------------
// [cross-doors S8] the dependents of one version are not those of another
// ---------------------------------------------------------------------------

/// Two versions of a feature, one horizon naming each: asking for the
/// dependents of version 1 answers the one horizon that names version 1.
///
/// Break it: compare on the uuid alone in `dependents_of_uri`; both horizons
/// are returned.
#[test]
fn the_dependents_of_one_version_are_not_those_of_the_other() {
    let summaries = [
        summary("BoundaryFeature", F, Some("1"), "version_1/BoundaryFeature_F.xml"),
        summary("BoundaryFeature", F, Some("2"), "version_2/BoundaryFeature_F.xml"),
        summary("HorizonInterpretation", H, None, "HorizonInterpretation_H.xml"),
        summary("HorizonInterpretation", H2, None, "HorizonInterpretation_H2.xml"),
    ];
    let objects = [
        read(&feature(F, Some("1")), "BoundaryFeature"),
        read(&feature(F, Some("2")), "BoundaryFeature"),
        read(&horizon(H, "0", F, Some("1")), "HorizonInterpretation"),
        read(&horizon(H2, "0", F, Some("2")), "HorizonInterpretation"),
    ];
    let pairs = || summaries.iter().zip(&objects).map(|(s, o)| (s, o.as_reflect()));

    let of_first = graph::dependents_of_uri(pairs(), &summaries[0].uri);
    let uris: Vec<&str> = of_first.iter().map(|one| one.uri.as_str()).collect();
    assert_eq!(uris, [summaries[2].uri.as_str()], "{of_first:#?}");

    let of_second = graph::dependents_of_uri(pairs(), &summaries[1].uri);
    let uris: Vec<&str> = of_second.iter().map(|one| one.uri.as_str()).collect();
    assert_eq!(uris, [summaries[3].uri.as_str()], "{of_second:#?}");

    // The uuid form mixes them, which is why the uri form exists.
    assert_eq!(graph::dependents(pairs(), F).len(), 2);
}

// ---------------------------------------------------------------------------
// [storage-1 S12] a split array is not read as the whole dataset per part
// ---------------------------------------------------------------------------

/// Several `ExternalFileProxy` parts are several flat ranges of one array: each
/// location carries its own range, so a reader reads its slice or refuses, and
/// never reads the whole dataset once per part.
///
/// Break it: remove the `mark_split_parts` call in `walk_value`; both ranges
/// come back empty.
#[test]
fn the_parts_of_a_split_array_carry_their_own_range() {
    use energyml_eml_2_2::{DataObjectReference, ExternalDataset, ExternalDatasetPart};
    let part = |start: i64, count: i64| ExternalDatasetPart {
        count,
        start_index: start,
        path_in_external_file: "/points".to_string(),
        epc_external_part_reference: DataObjectReference {
            uuid: "c2276841-4e15-4db1-83c9-10b8e78749a9".to_string(),
            ..Default::default()
        },
    };
    let split = ExternalDataset {
        external_file_proxy: vec![part(0, 4), part(4, 6)],
    };
    let found = locate(&split);
    assert_eq!(found.len(), 2);
    assert_eq!((&found[0].start, &found[0].count), (&vec![0], &vec![4]));
    assert_eq!((&found[1].start, &found[1].count), (&vec![4], &vec![6]));

    // One part is the whole array, as before.
    let whole = ExternalDataset {
        external_file_proxy: vec![part(3, 7)],
    };
    let found = locate(&whole);
    assert!(
        found[0].start.is_empty() && found[0].count.is_empty(),
        "{found:?}"
    );
}

// ---------------------------------------------------------------------------
// [storage-1 S13] is_array_pointer knows the middle form
// ---------------------------------------------------------------------------

/// Break it: take `MID_FORM` out of `is_array_pointer`; the eml 2.2 type is no
/// longer a pointer.
#[test]
fn every_array_pointer_type_is_one() {
    use energyml_core::reflect::EnergymlType;
    use energyml_storage::array::is_array_pointer;
    assert!(is_array_pointer(
        energyml_eml_2_3::ExternalDataArrayPart::type_info()
    ));
    assert!(is_array_pointer(
        energyml_eml_2_2::ExternalDatasetPart::type_info()
    ));
    assert!(!is_array_pointer(energyml_eml_2_3::Citation::type_info()));
}

// ---------------------------------------------------------------------------
// [cross-duplication C3] text has no numeric reading
// ---------------------------------------------------------------------------

/// Break it: return `Some(vec![f64::NAN; n])` for `Text` in `try_to_f64`.
#[test]
fn text_is_refused_not_turned_into_nan() {
    let text = ArrayValues::Text(vec!["sand".to_string(), "shale".to_string()]);
    assert_eq!(text.try_to_f64(), None);
    assert_eq!(ArrayValues::I32(vec![1, 2]).try_to_f64(), Some(vec![1.0, 2.0]));
    let data = ArrayData {
        metadata: ArrayMetadata::new(vec![2], "string".to_string()),
        values: text,
    };
    assert_eq!(data.try_to_f64(), None);
}
