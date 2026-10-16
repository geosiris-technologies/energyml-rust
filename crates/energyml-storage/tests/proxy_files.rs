//! Which external file each HDF5 proxy stands for, when a package has several.
//!
//! # Why this suite exists
//!
//! `EpcStore` remembers the `externalResource` of a part's `.rels` rather than
//! re-reading it, because a proxy's own `.rels` carries one relationship **per
//! object that references it**: on the Lyon package that part is 585 KB and
//! 2687 relationships, and it was inflated and parsed once per array pointer -
//! 2694 times, 1.6 GB of XML, to find the single entry naming the file. Nor
//! could an early-exiting parser have helped: in that file the
//! `externalResource` is the **last** relationship of the part, at byte 598 349
//! of 598 464.
//!
//! What a memo gets wrong is its **key**, and it got it wrong once already: the
//! first attempt keyed on `(proxy uuid, asking part)`, which hits every time on
//! the geometry path - where the asking array node has no uuid, so the key is
//! constant - and never hits when the caller is an object, since then every one
//! of the 4484 objects makes a key of its own. The measurement said so at once:
//! the frames went 12.2 s → 0.9 s while the array listing went 34.3 s → 36.1 s.
//!
//! So these tests use **three** proxies with three different answers, asked in
//! an interleaved order. A memo keyed on anything but the part - or one handing
//! back a neighbour's entry - gets a wrong file name here, not a slow one.
//!
//! Built in the test rather than taken from `testdata/`: a package with three
//! proxies and a deliberately absent `externalResource` is not something to
//! keep in a corpus.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_storage::EpcStore;

const ALPHA: &str = "11111111-1111-4111-8111-111111111111";
const BETA: &str = "22222222-2222-4222-8222-222222222222";
/// The one with no `externalResource` at all - the answer that must stay `None`
/// however many times its neighbours have been asked.
const MUTE: &str = "33333333-3333-4333-8333-333333333333";

fn registry() -> Arc<Registry> {
    let mut registry = Registry::new();
    energyml_eml_2_3::register(&mut registry);
    Arc::new(registry)
}

fn part_of(uuid: &str) -> String {
    format!("EpcExternalPartReference_{uuid}.xml")
}

fn proxy(uuid: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<eml:EpcExternalPartReference \
xmlns:eml=\"http://www.energistics.org/energyml/data/commonv2\" \
uuid=\"{uuid}\" schemaVersion=\"2.2\">\
<eml:Citation><eml:Title>External HDF5 proxy</eml:Title>\
<eml:Originator>test</eml:Originator>\
<eml:Creation>2026-09-08T00:00:00Z</eml:Creation>\
<eml:Format>test</eml:Format></eml:Citation>\
<eml:MimeType>application/x-hdf5</eml:MimeType>\
</eml:EpcExternalPartReference>"
    )
}

/// A proxy's `.rels`, shaped like a real one: a crowd of back-references, and
/// the `externalResource` **last** - where the corpus puts it, and the position
/// that makes reading the whole part unavoidable.
fn rels(file: Option<&str>) -> String {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    );
    for i in 0..64 {
        xml.push_str(&format!(
            "<Relationship Id=\"_back{i}\" \
Type=\"http://schemas.energistics.org/package/2012/relationships/externalPartProxyToMl\" \
Target=\"Whatever_{i}.xml\"/>"
        ));
    }
    if let Some(name) = file {
        xml.push_str(&format!(
            "<Relationship Id=\"Hdf5File\" \
Type=\"http://schemas.energistics.org/package/2012/relationships/externalResource\" \
Target=\"{name}\" TargetMode=\"External\"/>"
        ));
    }
    xml.push_str("</Relationships>");
    xml
}

fn content_types() -> String {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" \
ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>",
    );
    for uuid in [ALPHA, BETA, MUTE] {
        xml.push_str(&format!(
            "<Override PartName=\"/{}\" \
ContentType=\"application/x-eml+xml;version=2.2;type=EpcExternalPartReference\"/>",
            part_of(uuid)
        ));
    }
    xml.push_str("</Types>");
    xml
}

/// Three proxies, three `.rels`, two of them naming a different file.
fn package() -> PathBuf {
    // A folder per call, not per process: the tests of this file run in
    // parallel threads, and one removing the folder while another reads the
    // package failed on Linux (where an open file can be unlinked), 04/10.
    static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("energyml-proxy-files-{}-{call}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a temporary folder");
    let path = dir.join("three-proxies.epc");

    let file = std::fs::File::create(&path).expect("the package is creatable");
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("[Content_Types].xml", options).unwrap();
    zip.write_all(content_types().as_bytes()).unwrap();

    for (uuid, named) in [(ALPHA, Some("alpha.h5")), (BETA, Some("beta.h5")), (MUTE, None)] {
        let part = part_of(uuid);
        zip.start_file(&part, options).unwrap();
        zip.write_all(proxy(uuid).as_bytes()).unwrap();
        zip.start_file(format!("_rels/{part}.rels"), options).unwrap();
        zip.write_all(rels(named).as_bytes()).unwrap();
    }
    zip.finish().expect("the package is written");
    path
}

/// Each proxy answers with **its own** file, whatever order they are asked in.
#[test]
fn three_proxies_keep_three_answers() {
    let store = EpcStore::open(package(), registry()).expect("the package opens");

    // Interleaved, and each one asked more than once: the second answer comes
    // from the memo, so this is where a key that collides shows up.
    for _ in 0..3 {
        assert_eq!(store.external_file_of(ALPHA).as_deref(), Some("alpha.h5"));
        assert_eq!(store.external_file_of(BETA).as_deref(), Some("beta.h5"));
        assert_eq!(
            store.external_file_of(MUTE),
            None,
            "this proxy's `.rels` names no external resource, and no number of \
             questions about its neighbours may lend it one"
        );
        assert_eq!(store.external_file_of(BETA).as_deref(), Some("beta.h5"));
        assert_eq!(store.external_file_of(ALPHA).as_deref(), Some("alpha.h5"));
    }
}

/// The counter-proof: the assertions above fail when the answers really do
/// collide, so they are not passing on an empty set.
///
/// `external_file_of` is asked for a uuid **no part carries**. It has to be
/// `None` - and it is the same `None` the mute proxy gives, which is why the
/// test above pins the two apart by asking for both.
#[test]
fn a_proxy_the_package_does_not_hold_answers_nothing() {
    let store = EpcStore::open(package(), registry()).expect("the package opens");
    assert_eq!(store.external_file_of(ALPHA).as_deref(), Some("alpha.h5"));
    assert_eq!(
        store.external_file_of("44444444-4444-4444-8444-444444444444"),
        None,
        "no part carries this uuid; the memo must not answer for it either"
    );
}
