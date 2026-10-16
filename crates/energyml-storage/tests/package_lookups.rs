//! Lookups in a package are hash lookups, not scans (05/10, the cache, first wave).
//!
//! Opening a 72 808-object package took 121 s in `mediaLinks`: `Package::has`
//! was a `Vec::contains` over every part name, and the duplicate-`Id` check of a
//! `.rels` scanned the relationships already read - one `.rels` held 70 127 of
//! them. Both were quadratic.
//!
//! **Work is counted, not timed.** `parse_relationships_counting` returns how
//! many `Id` lookups the parse made; a linear parse makes exactly one per
//! relationship that carries an `Id`. The same answers are checked too: the
//! duplicate is named, once, and the order of the relationships is the
//! document's. The package is fabricated, so nothing here can skip.

use std::io::{Cursor, Write};
use std::sync::Arc;

use energyml_storage::EpcStore;
use energyml_storage::epc::package::parse_relationships_counting;

const REL: &str = "http://schemas.energistics.org/package/2012/relationships/destinationObject";

/// A `.rels` of `count` relationships `r0..`, with `Id="r{dup}"` written a
/// second time at the end.
fn rels(count: usize, dup: usize) -> String {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    );
    for i in 0..count {
        xml.push_str(&format!(
            "<Relationship Id=\"r{i}\" Type=\"{REL}\" Target=\"p{i}.xml\"/>"
        ));
    }
    xml.push_str(&format!(
        "<Relationship Id=\"r{dup}\" Type=\"{REL}\" Target=\"again.xml\"/>"
    ));
    xml.push_str("</Relationships>");
    xml
}

#[test]
fn the_duplicate_id_is_named_once_and_the_order_is_the_documents() {
    let (found, said, _) = parse_relationships_counting(rels(50, 7).as_bytes(), "_rels/a.xml.rels");
    assert_eq!(found.len(), 51);
    for (i, relationship) in found.iter().take(50).enumerate() {
        assert_eq!(
            relationship.id,
            format!("r{i}"),
            "the order of the document is kept"
        );
    }
    assert_eq!(found[50].id, "r7");
    let duplicates: Vec<_> = said
        .iter()
        .filter(|d| d.message.contains("more than once"))
        .collect();
    assert_eq!(duplicates.len(), 1, "one duplicate, one note: {said:?}");
    assert!(duplicates[0].message.contains("r7"), "{}", duplicates[0].message);
    assert_eq!(duplicates[0].part, "_rels/a.xml.rels");
}

#[test]
fn the_duplicate_check_makes_one_lookup_per_relationship() {
    // The quadratic scan would compare 0 + 1 + … + n−1 pairs: ~50 million for
    // 10 000. One lookup each is n + 1, whatever n is.
    for count in [100usize, 1_000, 10_000] {
        let (found, said, probes) =
            parse_relationships_counting(rels(count, count / 2).as_bytes(), "_rels/b.rels");
        assert_eq!(found.len(), count + 1);
        assert_eq!(
            probes,
            (count + 1) as u64,
            "{count} relationships, {probes} lookups"
        );
        assert_eq!(
            said.iter()
                .filter(|d| d.message.contains("more than once"))
                .count(),
            1
        );
    }
}

#[test]
fn a_relationship_without_an_id_is_not_looked_up() {
    let xml = format!(
        "<Relationships><Relationship Type=\"{REL}\" Target=\"a.xml\"/>\
         <Relationship Type=\"{REL}\" Target=\"b.xml\"/></Relationships>"
    );
    let (found, said, probes) = parse_relationships_counting(xml.as_bytes(), "_rels/c.rels");
    assert_eq!(found.len(), 2);
    assert_eq!(probes, 0);
    assert!(
        said.iter().all(|d| !d.message.contains("more than once")),
        "{said:?}"
    );
}

/// A package of `parts` media files and a root `.rels` of the same size with a
/// duplicate `Id`.
fn package(parts: usize) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options: zip::write::SimpleFileOptions =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut put = |name: &str, bytes: &[u8]| {
        zip.start_file(name, options).expect("start");
        zip.write_all(bytes).expect("write");
    };
    put(
        "[Content_Types].xml",
        b"<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
          <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
          <Default Extension=\"bin\" ContentType=\"application/octet-stream\"/></Types>",
    );
    put("_rels/.rels", rels(parts, parts / 2).as_bytes());
    for i in 0..parts {
        put(&format!("media/f{i}.bin"), b"x");
    }
    zip.finish().expect("finish").into_inner()
}

#[test]
fn a_package_of_many_parts_opens_and_says_the_same_things() {
    let store = EpcStore::from_reader(
        Box::new(Cursor::new(package(2_000))),
        "many.epc",
        Arc::new(energyml_schemas::all()),
    )
    .expect("opens");
    // `has` answers for a part present, with or without the leading slash, and
    // for one absent - read through the public door, `raw_part`.
    assert!(store.raw_part("media/f0.bin").is_ok());
    assert!(store.raw_part("/media/f1999.bin").is_ok());
    assert!(store.raw_part("media/f2000.bin").is_err());
    assert!(store.media_links().is_empty(), "no object, so no link");
}
