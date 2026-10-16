//! The OPC side of an EPC: the archive, its index, and its relationships.
//!
//! Everything here is about the *package*, not about EnergyML. It reads
//! `[Content_Types].xml` and the `.rels` parts, and it is deliberately
//! forgiving - the reference package published with fesapi breaks three rules
//! of the specification at once, and refusing it would make the reader useless.
//! Every departure produces a [`Diagnostic`]; none is swallowed.

use std::collections::{BTreeMap, HashSet};
use std::io::Read;
use std::sync::Mutex;

use quick_xml::events::Event;

use crate::error::{Diagnostic, Error, Result};
use crate::resource::Relationship;

/// Anything an archive can be read from: a file, or bytes already in memory.
///
/// Reading from memory is a requirement, not a convenience: a cloud service
/// receives an EPC as the body of a response and has no disk to put it on.
pub trait ReadSeek: std::io::Read + std::io::Seek {}
impl<T: std::io::Read + std::io::Seek> ReadSeek for T {}

/// The archive plus what its index says about it.
pub(crate) struct Package {
    /// Behind a mutex so the store can stay `&self` and its futures stay
    /// `Send`; a `RefCell` would rule out every threaded caller.
    archive: Mutex<zip::ZipArchive<Box<dyn ReadSeek + Send>>>,
    /// Part name (no leading slash) → content type, from the `Override` entries.
    overrides: BTreeMap<String, String>,
    /// Lowercased extension → content type, from the `Default` entries.
    defaults: BTreeMap<String, String>,
    /// Every entry in the archive, in archive order.
    entries: Vec<String>,
    /// The same names as a set, so [`Package::has`] is one hash lookup rather
    /// than a scan of every entry. `entries` stays the `Vec` because its order
    /// is the archive's and callers walk it in that order.
    names: HashSet<String>,
    /// Part name → uncompressed size, as the central directory declares it.
    ///
    /// Declared, not measured: reading every part to weigh it would inflate the
    /// whole archive. A listing that says so is worth having - it is what tells
    /// somebody which of the three hundred parts is the one worth looking at -
    /// and a forged size misleads a listing rather than costing anything, which
    /// is why [`Package::read`] still refuses to trust the same number.
    sizes: BTreeMap<String, u64>,
}

impl Package {
    /// Open an archive and read its index.
    pub(crate) fn open(source: Box<dyn ReadSeek + Send>, diagnostics: &mut Vec<Diagnostic>) -> Result<Self> {
        let mut archive =
            zip::ZipArchive::new(source).map_err(|e| Error::Package(format!("not a zip: {e}")))?;
        // A name is UTF-8, or else CP437, which takes any byte: `zip` cannot fail
        // to decode one today, but if it ever does the package is refused by
        // name rather than read without the part.
        let entries: Vec<String> = archive
            .file_names()
            .map(|name| name.map(|n| n.into_owned()))
            .collect::<std::result::Result<_, _>>()
            .map_err(|e| Error::Package(format!("a part name cannot be read: {e}")))?;

        // The sizes, read once from the central directory. `by_index_raw` opens
        // no decoder, so this is a walk over metadata the archive has already
        // parsed rather than a pass over the bytes.
        let mut sizes = BTreeMap::new();
        for index in 0..archive.len() {
            if let Ok(entry) = archive.by_index_raw(index) {
                let name = entry
                    .name()
                    .map_err(|e| Error::Package(format!("a part name cannot be read: {e}")))?
                    .into_owned();
                sizes.insert(name, entry.size());
            }
        }

        let mut content_types = Vec::new();
        match archive.by_name("[Content_Types].xml") {
            Ok(mut part) => {
                part.read_to_end(&mut content_types)?;
            }
            Err(_) => {
                // Mandatory in OPC, so its absence is a real problem - but the
                // parts are still there, and the root element of each can type
                // it. Say so and carry on.
                diagnostics.push(Diagnostic::warning(
                    "[Content_Types].xml",
                    "absent, though OPC makes it mandatory; parts will be typed from their root element",
                ));
            }
        }

        let (overrides, defaults) = parse_content_types(&content_types, diagnostics);
        let names: HashSet<String> = entries.iter().cloned().collect();
        Ok(Self {
            archive: Mutex::new(archive),
            overrides,
            defaults,
            entries,
            names,
            sizes,
        })
    }

    /// Every entry in the archive.
    pub(crate) fn entries(&self) -> &[String] {
        &self.entries
    }

    /// The uncompressed size the archive declares for a part.
    pub(crate) fn size(&self, part: &str) -> Option<u64> {
        self.sizes.get(&normalize(part)).copied()
    }

    /// The bytes of one part.
    pub(crate) fn read(&self, part: &str) -> Result<Vec<u8>> {
        let name = normalize(part);
        let mut archive = self
            .archive
            .lock()
            .expect("the package lock is never held across a panic");
        let mut entry = archive
            .by_name(&name)
            .map_err(|_| Error::Package(format!("no part `{name}` in the package")))?;
        // The declared size is a hint from the file, not a fact about it: a
        // forged package can claim four gigabytes for a part holding twelve
        // bytes, and reserving it first would abort the process before a single
        // byte was inflated. Reserve what a real part plausibly needs and let
        // `read_to_end` grow past it - the cost of being wrong is a few
        // reallocations, and the cost of trusting it is the whole process.
        const PREALLOCATION_CAP: usize = 8 * 1024 * 1024;
        let mut bytes = Vec::with_capacity((entry.size() as usize).min(PREALLOCATION_CAP));
        entry.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    /// Run `f` over a part **as a stream**, without decompressing all of it.
    ///
    /// This is what makes the level-2 header cheap: the citation sits in the
    /// first few hundred bytes, `f` stops there, and the rest of the entry is
    /// never inflated. [`Package::read`] would have inflated the whole part to
    /// throw most of it away.
    pub(crate) fn with_part<T>(&self, part: &str, f: impl FnOnce(&mut dyn Read) -> T) -> Result<T> {
        let name = normalize(part);
        let mut archive = self
            .archive
            .lock()
            .expect("the package lock is never held across a panic");
        let mut entry = archive
            .by_name(&name)
            .map_err(|_| Error::Package(format!("no part `{name}` in the package")))?;
        Ok(f(&mut entry))
    }

    /// Whether the archive holds this part.
    pub(crate) fn has(&self, part: &str) -> bool {
        let name = normalize(part);
        self.names.contains(&name)
    }

    /// Content type declared for a part, `Override` first then `Default` by
    /// extension, as OPC prescribes.
    pub(crate) fn content_type(&self, part: &str) -> Option<&str> {
        let name = normalize(part);
        if let Some(ct) = self.overrides.get(&name) {
            return Some(ct.as_str());
        }
        let extension = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
        self.defaults.get(&extension).map(String::as_str)
    }

    /// Relationships recorded for a part, and what was wrong with the `.rels`.
    ///
    /// An absent `.rels` is not an error: the specification only requires one
    /// when the part has relationships.
    ///
    /// The diagnostics come back rather than being recorded here, because
    /// `Package` has nowhere to put one: it is the zip and nothing else. The
    /// caller that owns a diagnostics list - the store - records them, and
    /// `record` deduplicates, so a `.rels` read a hundred times reports its
    /// defect once.
    pub(crate) fn relationships_reporting(&self, part: &str) -> Result<(Vec<Relationship>, Vec<Diagnostic>)> {
        let rels_part = rels_name_of(part);
        if !self.has(&rels_part) {
            return Ok((Vec::new(), Vec::new()));
        }
        let bytes = self.read(&rels_part)?;
        Ok(parse_relationships_reporting(&bytes, &rels_part))
    }
}

/// Part names are written with and without a leading slash - the OPC spec
/// wants one, the fesapi package omits it on half its entries. Compare on the
/// form the archive itself uses.
pub(crate) fn normalize(part: &str) -> String {
    part.trim_start_matches('/').to_string()
}

/// `a/b/Foo.xml` → `a/b/_rels/Foo.xml.rels`, and the package itself → `_rels/.rels`.
pub(crate) fn rels_name_of(part: &str) -> String {
    let name = normalize(part);
    match name.rsplit_once('/') {
        Some((folder, file)) => format!("{folder}/_rels/{file}.rels"),
        None if name.is_empty() => "_rels/.rels".to_string(),
        None => format!("_rels/{name}.rels"),
    }
}

/// Read `[Content_Types].xml` into its two maps.
fn parse_content_types(
    xml: &[u8],
    diagnostics: &mut Vec<Diagnostic>,
) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
    let mut overrides: BTreeMap<String, String> = BTreeMap::new();
    let mut defaults: BTreeMap<String, String> = BTreeMap::new();
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut buf = Vec::new();

    loop {
        let event = match reader.read_event_into(&mut buf) {
            Ok(event) => event,
            Err(e) => {
                diagnostics.push(Diagnostic::warning(
                    "[Content_Types].xml",
                    format!("unreadable: {e}"),
                ));
                break;
            }
        };
        match event {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) => {
                let local = local_name(e.name().as_ref());
                let (mut key, mut value) = (None, None);
                for attribute in e.attributes().flatten() {
                    let text = String::from_utf8_lossy(&attribute.value).into_owned();
                    match attribute.key.as_ref() {
                        b"PartName" | b"Extension" => key = Some(text),
                        b"ContentType" => value = Some(text),
                        _ => {}
                    }
                }
                match (local.as_str(), key, value) {
                    ("Override", Some(part), Some(ct)) => {
                        let part = normalize(&part);
                        if let Some(previous) = overrides.insert(part.clone(), ct.clone())
                            && previous != ct
                        {
                            // Seen in the fesapi package: the same object is
                            // declared twice, once with a content type that
                            // belongs to another part entirely.
                            diagnostics.push(Diagnostic::warning(
                                part,
                                format!("declared twice, as `{previous}` and `{ct}`; the last wins"),
                            ));
                        }
                    }
                    ("Default", Some(extension), Some(ct)) => {
                        defaults.insert(extension.to_ascii_lowercase(), ct);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        buf.clear();
    }
    (overrides, defaults)
}

/// Read one `.rels` part, and say what was wrong with it.
///
/// A malformed `.rels` used to stop the walk and keep whatever had been read -
/// the relationships after the broken byte simply were not there, and nothing
/// said so. For a package whose `.rels` are how the HDF5 companion is found,
/// "half the relationships" is a package that half works, which is the hardest
/// kind of defect to chase.
///
/// The truncation itself stays: half an index beats none, and the store's own
/// reference check (`identity::collect_refs`) does not depend on it. What
/// changes is that the caller is told.
pub(crate) fn parse_relationships_reporting(xml: &[u8], part: &str) -> (Vec<Relationship>, Vec<Diagnostic>) {
    let (out, diagnostics, _) = parse_relationships_counting(xml, part);
    (out, diagnostics)
}

/// The `Id`s read so far in one `.rels`, answered by a hash lookup.
///
/// It used to be a scan of the relationships already read, for each
/// relationship: quadratic, and one `.rels` of a real package holds 70 127 of
/// them (121 s). `probes` counts the lookups so a test can say *how many* there
/// were, which a stopwatch cannot say reliably.
#[derive(Default)]
struct SeenIds {
    ids: HashSet<String>,
    probes: u64,
}

impl SeenIds {
    /// Record `id` and say whether it had been seen before.
    fn repeats(&mut self, id: &str) -> bool {
        self.probes += 1;
        !self.ids.insert(id.to_string())
    }
}

/// [`parse_relationships_reporting`], and the number of `Id` lookups it made.
///
/// Public only so that `tests/package_lookups.rs` can prove the duplicate
/// check is linear - one lookup per relationship - by count rather than by
/// clock. Not part of the API.
#[doc(hidden)]
pub fn parse_relationships_counting(xml: &[u8], part: &str) -> (Vec<Relationship>, Vec<Diagnostic>, u64) {
    let mut seen = SeenIds::default();
    let mut out: Vec<Relationship> = Vec::new();
    let mut diagnostics = Vec::new();
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Err(e) => {
                diagnostics.push(Diagnostic::warning(
                    part,
                    format!(
                        "unreadable from byte {}: {e}; the {} relationship(s) read before it are kept, \
                         anything after is lost",
                        reader.buffer_position(),
                        out.len()
                    ),
                ));
                break;
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if local_name(e.name().as_ref()) != "Relationship" {
                    buf.clear();
                    continue;
                }
                let (mut id, mut kind, mut target, mut mode) =
                    (String::new(), String::new(), String::new(), String::new());
                for attribute in e.attributes().flatten() {
                    let text = String::from_utf8_lossy(&attribute.value).into_owned();
                    match attribute.key.as_ref() {
                        b"Id" => id = text,
                        b"Type" => kind = text,
                        b"Target" => target = text,
                        b"TargetMode" => mode = text,
                        _ => {}
                    }
                }
                // `Id` is an xsd:ID: unique within the part, by the OPC schema.
                // ALWYN breaks it in 73 of its 203 `.rels`, which is why this
                // is a note and not a refusal - but a duplicate is what makes
                // an editor overwrite one relationship with another.
                if !id.is_empty() && seen.repeats(&id) {
                    diagnostics.push(Diagnostic::note(
                        part,
                        format!("`Id=\"{id}\"` appears more than once; the OPC schema makes it unique"),
                    ));
                }
                if target.is_empty() {
                    diagnostics.push(Diagnostic::note(
                        part,
                        format!("a `Relationship` (Id=`{id}`) has an empty `Target`"),
                    ));
                }
                out.push(Relationship {
                    id,
                    kind,
                    target,
                    external: mode.eq_ignore_ascii_case("external"),
                });
            }
            _ => {}
        }
        buf.clear();
    }
    (out, diagnostics, seen.probes)
}

/// Local name of a possibly prefixed XML name.
fn local_name(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw.rsplit(|b| *b == b':').next().unwrap_or_default()).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rels_names_follow_the_part() {
        assert_eq!(rels_name_of("Foo.xml"), "_rels/Foo.xml.rels");
        assert_eq!(rels_name_of("/Foo.xml"), "_rels/Foo.xml.rels");
        assert_eq!(rels_name_of("resqml20/Foo.xml"), "resqml20/_rels/Foo.xml.rels");
        assert_eq!(rels_name_of(""), "_rels/.rels");
    }

    #[test]
    fn a_part_declared_twice_is_reported_and_the_last_wins() {
        let xml = br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
            <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
            <Override PartName="/a.xml" ContentType="application/x-resqml+xml;version=2.0;type=obj_Activity"/>
            <Override PartName="/a.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>
        </Types>"#;
        let mut diagnostics = Vec::new();
        let (overrides, defaults) = parse_content_types(xml, &mut diagnostics);
        assert_eq!(
            overrides.get("a.xml").unwrap(),
            "application/vnd.openxmlformats-package.core-properties+xml"
        );
        assert_eq!(defaults.len(), 1);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    }

    #[test]
    fn an_external_relationship_is_recognised() {
        let xml = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
            <Relationship Id="Hdf5File" TargetMode="External" Target="model.h5"
                Type="http://schemas.energistics.org/package/2012/relationships/externalResource"/>
            <Relationship Id="_x" Target="Other.xml"
                Type="http://schemas.energistics.org/package/2012/relationships/destinationObject"/>
        </Relationships>"#;
        let (rels, problems) = parse_relationships_reporting(xml, "_rels/a.xml.rels");
        assert_eq!(rels.len(), 2);
        assert!(rels[0].is_external_resource());
        assert_eq!(rels[0].target, "model.h5");
        assert_eq!(rels[1].short_kind(), "destinationObject");
        assert!(!rels[1].is_external_resource());
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn a_truncated_rels_keeps_what_it_read_and_says_what_it_lost() {
        // The defect this was blind to: the walk stopped at the broken byte,
        // the relationships after it silently did not exist, and the package
        // half worked.
        let xml = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
            <Relationship Id="Hdf5File" TargetMode="External" Target="model.h5"
                Type="http://schemas.energistics.org/package/2012/relationships/externalResource"/>
            <Relationship Id="_x" Target="Other.xml" Type="htt"#;
        let (rels, problems) = parse_relationships_reporting(xml, "_rels/a.xml.rels");
        assert_eq!(rels.len(), 1, "what was read is kept");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].message.contains("kept"), "{:?}", problems[0]);
    }

    #[test]
    fn a_duplicate_id_is_reported() {
        // ALWYN does this in 73 of its 203 `.rels`. Not fatal - hence a note -
        // but it is what makes an editor overwrite one relationship with
        // another.
        let xml = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
            <Relationship Id="_x" Target="A.xml" Type="t"/>
            <Relationship Id="_x" Target="B.xml" Type="t"/>
        </Relationships>"#;
        let (rels, problems) = parse_relationships_reporting(xml, "_rels/a.xml.rels");
        assert_eq!(rels.len(), 2, "both are kept: the targets differ");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].message.contains("more than once"));
    }
}
