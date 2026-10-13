//! A package whose producer left something out is still a package.
//!
//! The report that prompted this: a client EPC of 1618 objects whose every
//! `Citation` was written without its `Originator`. One string, mandatory in
//! the schema, read by nothing downstream - and the whole file was unreadable.
//! Not one geometry, not one reference, not one title, no export, no JSON.
//!
//! The reasoning that changed it is in
//! [`ReadMode`][energyml_core::registry::ReadMode], and the short version is
//! that a parser saying *what the document says* and a validator saying *what
//! it should have said* are two jobs. Doing the second inside the first leaves
//! only one way to report it, and that way costs everything else in the file.
//!
//! What this suite pins is that the change did not buy readability with
//! silence, and did not blunt the two things that were relying on strictness.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use energyml_core::registry::{ReadMode, Registry};
use energyml_storage::EpcStore;

fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

fn corpus_file(name: &str) -> Option<PathBuf> {
    crate::fixtures::find(name)
}

/// A copy of a real package with every `Originator` element removed.
///
/// Built rather than checked in: the point is that this is an ordinary package
/// in every other respect, and deriving it from one keeps it that way as the
/// corpus changes.
///
/// **Built once.** Three tests in this file want it, cargo runs them on three
/// threads, and the first version of this wrote the same path from all three -
/// so one thread read a zip another was still writing. It passed alone and
/// failed in the workspace run, which is the worst way for a test to be wrong:
/// the isolated run that a person does while working is the one that lies.
fn without_originator(name: &str) -> Option<PathBuf> {
    static BUILT: OnceLock<Option<PathBuf>> = OnceLock::new();
    BUILT.get_or_init(|| build_without_originator(name)).clone()
}

fn build_without_originator(name: &str) -> Option<PathBuf> {
    let source = corpus_file(name)?;
    let out = std::env::temp_dir().join(format!("energyml-absent-{name}"));

    let bytes = std::fs::read(&source).ok()?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
    let file = std::fs::File::create(&out).ok()?;
    let mut writer = zip::ZipWriter::new(file);
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut stripped = 0usize;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).ok()?;
        let entry_name = entry.name().to_string();
        let mut content = Vec::new();
        std::io::copy(&mut entry, &mut content).ok()?;
        if entry_name.ends_with(".xml") {
            let before = content.len();
            content = strip_element(&content, b"Originator");
            if content.len() != before {
                stripped += 1;
            }
        }
        writer.start_file(entry_name, options).ok()?;
        std::io::Write::write_all(&mut writer, &content).ok()?;
    }
    writer.finish().ok()?;
    assert!(stripped > 0, "{name} carried no Originator to remove");
    Some(out)
}

/// Remove every `<…:Originator>…</…:Originator>` element, prefix or not.
fn strip_element(xml: &[u8], local: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(xml).into_owned();
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    let local = String::from_utf8_lossy(local).into_owned();
    loop {
        // The opening tag, with or without a namespace prefix.
        let Some(open) = rest
            .match_indices('<')
            .find(|(at, _)| {
                let after = &rest[at + 1..];
                let after = after.split_once(':').map(|(_, r)| r).unwrap_or(after);
                after.starts_with(&local) && after[local.len()..].starts_with(['>', ' ', '/'])
            })
            .map(|(at, _)| at)
        else {
            out.push_str(rest);
            return out.into_bytes();
        };
        let closing = format!("{local}>");
        let Some(end) = rest[open..].find(&closing).map(|at| open + at + closing.len()) else {
            out.push_str(rest);
            return out.into_bytes();
        };
        out.push_str(&rest[..open]);
        rest = &rest[end..];
    }
}

#[test]
fn a_package_missing_a_required_element_still_reads() {
    let Some(path) = without_originator("testingPackageCpp22.epc") else {
        eprintln!("skipped: the corpus is not present");
        return;
    };
    let store = EpcStore::open(&path, registry()).expect("the package opens");

    let mut read = 0usize;
    let mut refused = Vec::new();
    for summary in store.resources() {
        match store.get_object(&summary.uri) {
            Ok(_) => read += 1,
            Err(e) => refused.push(format!("{}: {e}", summary.uuid)),
        }
    }
    assert!(
        refused.is_empty(),
        "{} of {} objects refused over an absent element: {:?}",
        refused.len(),
        read + refused.len(),
        &refused[..refused.len().min(3)]
    );
    assert!(read > 100, "only {read} objects read");
}

#[test]
fn what_was_filled_in_is_reported() {
    // Readable is not the same as silent. Every default the reader put there is
    // a value nobody wrote, and a caller that saves the object would put it on
    // disk - so it has to be able to find out.
    let Some(path) = without_originator("testingPackageCpp22.epc") else {
        return;
    };
    let store = EpcStore::open(&path, registry()).expect("the package opens");
    for summary in store.resources() {
        let _ = store.get_object(&summary.uri);
    }
    let said: Vec<String> = store.diagnostics().iter().map(|d| d.to_string()).collect();
    let about_originator = said.iter().filter(|line| line.contains("Originator")).count();
    assert!(
        about_originator > 100,
        "{about_originator} diagnostic(s) mention the element that was filled in; \
         reading past it quietly would be worse than refusing"
    );
}

#[test]
fn strict_still_refuses_what_the_document_does_not_carry() {
    // The mode a caller that will **write the package back** asks for.
    let Some(path) = without_originator("testingPackageCpp22.epc") else {
        return;
    };
    let mut store = EpcStore::open(&path, registry()).expect("the package opens");
    store.set_strict(true);

    let refused = store
        .resources()
        .iter()
        .filter(|summary| store.get_object(&summary.uri).is_err())
        .count();
    assert!(refused > 100, "strict read only refused {refused} object(s)");
}

#[test]
fn a_value_that_is_there_and_wrong_is_still_refused() {
    // The asymmetry, on a real part: absence is read past, nonsense is not. A
    // producer that omits an element forgot; a producer that writes `deux`
    // where a dimension belongs wrote something, and defaulting it throws away
    // what the file said - an IJK grid silently 0 cells wide.
    let Some(path) = corpus_file("testingPackageCpp22.epc") else {
        eprintln!("skipped: the corpus is not present");
        return;
    };
    let store = EpcStore::open(&path, registry()).expect("the package opens");
    let summary = store
        .resources()
        .into_iter()
        .find(|s| s.qualified_type.contains("IjkGrid"))
        .expect("the corpus has an IJK grid");
    let bytes = store.part_bytes(&summary.uri).expect("its part reads");

    let text = String::from_utf8(bytes).expect("a part is UTF-8");
    let broken = text.replace("<resqml:Ni>2</resqml:Ni>", "<resqml:Ni>deux</resqml:Ni>");
    assert_ne!(
        broken, text,
        "the substitution missed; the test would prove nothing"
    );
    let broken = broken.as_bytes();

    let content_type = "application/x-resqml+xml;version=2.2;type=IjkGridRepresentation";
    let registry = registry();
    assert!(
        registry
            .read_xml_as(content_type, broken, ReadMode::Tolerant)
            .is_err(),
        "tolerating what is absent is not tolerating what is wrong"
    );
    let (_, departures) = registry
        .read_xml_as(content_type, broken, ReadMode::Lenient)
        .expect("lenient repairs it");
    assert!(
        departures.repaired.iter().any(|r| r.to_string().contains("deux")),
        "the producer's own text is the only record of what was written: {:?}",
        departures.repaired
    );
}

#[test]
fn the_version_a_part_is_read_as_does_not_change() {
    // The one thing tolerance could have quietly broken. Telling two releases
    // apart works by letting the wrong one **fail**, and an element required on
    // one side and absent on the other is very often the only difference a
    // parser can see. Reading tolerantly on the first pass would blunt exactly
    // that, and a part would come back as a plausible object of the wrong
    // release carrying only the fields the two share.
    //
    // So `get_object` reads strictly first, over the candidates best-first, and
    // only falls back to the caller's mode when no candidate reads strictly at
    // all. This checks the outcome on the corpus rather than the mechanism.
    #[allow(unused_mut)] // pushed to only with `private-corpus`
    let mut packages: Vec<(String, PathBuf)> = ["testingPackageCpp.epc", "testingPackageCpp22.epc"]
        .iter()
        .filter_map(|name| corpus_file(name).map(|path| (name.to_string(), path)))
        .collect();
    // 80wells and ALWYN: real-file campaign, run privately.
    #[cfg(feature = "private-corpus")]
    for name in ["80wells_surf_modified_val_color.epc", "ALWYN-RESQML.epc"] {
        if let Some(path) = crate::fixtures::private_find(name) {
            packages.push((name.to_string(), path));
        }
    }
    for (name, path) in &packages {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            let info = object.as_reflect().type_info();
            let read_as = info.qualified_type.unwrap_or(info.name);
            if read_as == summary.qualified_type {
                continue;
            }
            // **And when they differ, the store must have said so.**
            //
            // The assertion used to be a plain equality, and it held for a
            // reason that stopped being true on 21/09: PRODML 2.3 was not a
            // release this workspace generated, so a part stamped `2.3` fell
            // back to 2.2 on *both* paths and the two agreed by accident.
            // Generating 2.3 separated them, and the separation is correct -
            // fesapi stamps `schemaVersion="2.3"` on PRODML 2.2 content, and
            // `80wells_surf_modified_val_color.epc` has no `[Content_Types].xml`
            // to contradict it, so the inventory believes the stamp while the
            // read **proves** the content by parsing it.
            //
            // Demanding equality would therefore have forced one of two lies:
            // an inventory that claims to know what only a parser can settle,
            // or a reader that accepts the stamp over the bytes. What must hold
            // is the thing this repository actually cares about - *the
            // disagreement is named, not swallowed* - and `get_object` already
            // records it whenever it settles on a lower-ranked candidate.
            let said = store
                .diagnostics()
                .into_iter()
                .any(|d| d.message.contains(&summary.qualified_type) && d.message.contains(read_as));
            assert!(
                said,
                "{name}: {} was inventoried as `{}` and read as `{read_as}`, and the store said nothing",
                summary.uuid, summary.qualified_type
            );
        }
    }
}
