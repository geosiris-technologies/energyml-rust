//! Saving a package whose parts were staged as **bytes**.
//!
//! Staging bytes is what protects the content - nothing is deserialised, so
//! nothing can be dropped - and it is the whole point of `relabel`. But the
//! `.rels` are built from an object's `DataObjectReference`s, and a part staged
//! as bytes has no object. So one save writes parts that are byte-perfect and an
//! index that is **gone**.
//!
//! That is not a theory: on a 1 344-object client package, a single save took
//! 1 335 `.rels` down to 2. `save_reindexed` reopens what it wrote and saves
//! again, which rebuilds the index from references that finally parse.
//!
//! The test below is the comparison, on a package small enough to read: the same
//! staging, saved the two ways, and only one of them keeps the link.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_storage::{EpcStore, SaveOptions, relabel};

const FEATURE: &str = "3d5f8c21-0000-4000-8000-000000000001";
const INTERP: &str = "3d5f8c21-0000-4000-8000-000000000002";
const RESQML: &str = "http://www.energistics.org/energyml/data/resqmlv2";
const EML: &str = "http://www.energistics.org/energyml/data/commonv2";

fn registry() -> Arc<Registry> {
    let mut registry = Registry::new();
    energyml_eml_2_3::register(&mut registry);
    energyml_resqml_2_2::register(&mut registry);
    Arc::new(registry)
}

fn citation(title: &str) -> String {
    format!(
        "<eml:Citation><eml:Title>{title}</eml:Title>\
         <eml:Originator>test</eml:Originator>\
         <eml:Creation>2026-09-01T00:00:00Z</eml:Creation>\
         <eml:Format>test</eml:Format></eml:Citation>"
    )
}

/// A horizon that names its feature - the reference the `.rels` must carry.
fn interpretation() -> String {
    format!(
        "<resqml:HorizonInterpretation xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" \
         uuid=\"{INTERP}\" schemaVersion=\"2.2\">{}\
         <resqml:Domain>depth</resqml:Domain>\
         <resqml:InterpretedFeature>\
         <eml:Uuid>{FEATURE}</eml:Uuid>\
         <eml:QualifiedType>resqml22.BoundaryFeature</eml:QualifiedType>\
         <eml:Title>a boundary</eml:Title>\
         </resqml:InterpretedFeature>\
         </resqml:HorizonInterpretation>",
        citation("a horizon")
    )
}

fn feature() -> String {
    format!(
        "<resqml:BoundaryFeature xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" \
         uuid=\"{FEATURE}\" schemaVersion=\"2.2\">{}\
         <resqml:IsWellKnown>false</resqml:IsWellKnown>\
         </resqml:BoundaryFeature>",
        citation("a boundary")
    )
}

fn package(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("energyml-reindex-{name}-{}", std::process::id()));
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
    zip.start_file(format!("HorizonInterpretation_{INTERP}.xml"), options)
        .unwrap();
    zip.write_all(interpretation().as_bytes()).unwrap();
    zip.start_file(format!("BoundaryFeature_{FEATURE}.xml"), options)
        .unwrap();
    zip.write_all(feature().as_bytes()).unwrap();
    zip.finish().expect("written");
    path
}

/// How many `.rels` a saved package carries, read from the zip rather than
/// from this library - two independent views of the same fact.
fn rels(path: &Path) -> usize {
    let file = std::fs::File::open(path).expect("readable");
    let mut zip = zip::ZipArchive::new(file).expect("a zip");
    (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|entry| entry.name().to_string()))
        .filter(|name| name.ends_with(".rels"))
        .count()
}

/// Stage one part's bytes, the way `relabel` does.
fn stage(store: &mut EpcStore) {
    let part = format!("HorizonInterpretation_{INTERP}.xml");
    let bytes = store.raw_part(&part).expect("its bytes");
    let (relabelled, what) =
        relabel::set_schema_version(&bytes, "2.2_dev3").expect("the declaration is rewritten");
    assert!(what.changed(), "the fixture must actually change");
    store.set_raw_part(&part, relabelled);
}

/// **One save loses the index; `save_reindexed` keeps it.**
///
/// The two halves are one test on purpose: the number that matters is not
/// "how many `.rels`" but "how many *more* than the plain save", and a test
/// that only checked the second would pass just as well if staging had never
/// dropped anything.
#[test]
fn a_staged_part_loses_its_relationships_unless_the_package_is_reindexed() {
    let source = package("compare");

    let plain = source.with_file_name("plain.epc");
    {
        let mut store = EpcStore::open(&source, registry()).expect("opens");
        stage(&mut store);
        store.save_as(&plain, &SaveOptions::default()).expect("saves");
    }

    let reindexed = source.with_file_name("reindexed.epc");
    {
        let mut store = EpcStore::open(&source, registry()).expect("opens");
        stage(&mut store);
        relabel::save_reindexed(&store, &reindexed, registry(), &SaveOptions::default())
            .expect("saves and rebuilds");
    }

    // The source is built by hand and carries no `.rels` at all - the writer
    // makes them. So the comparison is between the two *saves*, which is the
    // real question anyway: one save against the same save reindexed.
    let after_plain = rels(&plain);
    let after_reindex = rels(&reindexed);
    assert!(
        after_reindex > after_plain,
        "reindexing must recover relationships the plain save lost: {after_reindex} against          {after_plain}"
    );
    // And the recovered one is the link the staged part makes: an interpretation
    // naming its feature. Without it the package draws a horizon that
    // interprets nothing.
    assert!(
        after_reindex >= 2,
        "both the package relationships and the part's own are expected: {after_reindex}"
    );
}

/// The temporary does not survive a run that worked.
///
/// It carries the same parts under a name nobody asked for; leaving it behind
/// on success would look like the command wrote twice.
#[test]
fn the_intermediate_package_is_cleaned_up() {
    let source = package("cleanup");
    let out = source.with_file_name("out.epc");
    let mut store = EpcStore::open(&source, registry()).expect("opens");
    stage(&mut store);
    relabel::save_reindexed(&store, &out, registry(), &SaveOptions::default()).expect("saves");

    assert!(out.exists(), "the package asked for is written");
    assert!(
        !out.with_extension("relabel-pass1.epc").exists(),
        "and the intermediate is gone"
    );
}

/// What the second pass is *for*: the relabelled declaration survives it.
///
/// Reindexing reopens and saves, which for a part that is no longer staged
/// means re-serialising it. If that lost the new version the whole exercise
/// would be pointless, and the loss would be invisible - the package would
/// simply read as the release again.
#[test]
fn reindexing_keeps_the_version_the_relabelling_wrote() {
    let source = package("keeps");
    let out = source.with_file_name("kept.epc");
    let mut store = EpcStore::open(&source, registry()).expect("opens");
    stage(&mut store);
    relabel::save_reindexed(&store, &out, registry(), &SaveOptions::default()).expect("saves");

    let file = std::fs::File::open(&out).expect("readable");
    let mut zip = zip::ZipArchive::new(file).expect("a zip");
    let mut found = false;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).expect("an entry");
        if !entry.name().starts_with("HorizonInterpretation") {
            continue;
        }
        let mut text = String::new();
        std::io::Read::read_to_string(&mut entry, &mut text).expect("utf-8");
        assert!(
            text.contains("2.2_dev3"),
            "the relabelled version must survive the second pass: {text}"
        );
        found = true;
    }
    assert!(found, "the part must be in the saved package at all");
}
