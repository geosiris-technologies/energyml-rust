//! Changing what a part *says* it is, without reading what it holds.
//!
//! # Why this is a byte edit and not an object edit
//!
//! Every other way of changing a package goes through an object: read it, edit
//! it by reflection, let the writer serialise it back. That is the right shape
//! for changing *data*. It is the wrong shape for changing a **declaration**,
//! and the difference is not stylistic.
//!
//! A part written against `resqml 2.2_dev3` but declaring `schemaVersion="2.2"`
//! is read as the released 2.2 — and the release *succeeds*, because unknown
//! children are skipped rather than rejected. So the object comes back missing
//! whatever the drop has and the release does not (a `ContactPatch` loses its
//! `Count`, which the drop makes required and the release has no field for),
//! and saving that object writes the loss to disk. Reading in order to correct
//! the declaration would destroy the very thing the correction exists to
//! protect.
//!
//! So nothing here parses the document. The root element's tag is located, one
//! attribute inside it is rewritten, and **every other byte of the part is left
//! exactly as it was** — including the parts of the tag this code did not come
//! for: attribute order, quoting style, whitespace, namespace declarations.
//!
//! # What the specification says, and where this goes past it
//!
//! `schemaVersion` is a required attribute of `eml:AbstractObject`, and its
//! documentation is specific:
//!
//! > *The version of the Energistics schema used for a data object. The schema
//! > version is the first 2 digits of an ML version. EXAMPLES: For WITSML v2.0
//! > the schema version is 20; for RESQML v2.0.1 the schema version is 20.*
//!
//! Two things follow. Producers do not obey it — the corpus is full of `2.0`
//! and `2.2`, with the separator — which is why this crate compares versions
//! through [`energyml_core::naming::version_eq`] rather than by equality. And
//! **the specification provides no way at all to name a development drop
//! here.** `2.2_dev3` is not a schema version in the sense that sentence
//! defines; it is Energistics' own name for the drop, taken from the directory
//! and archive they distribute it in (`resqml_v2.2_dev3`).
//!
//! That is a deliberate departure and it is stated rather than hidden: writing
//! it makes the file honest to this workspace and *less* legible to an
//! implementation that only knows the released spellings. It is the right trade
//! only when the file really is a drop, which is why the caller names both the
//! package and the version and nothing here guesses either.

use std::path::Path;
use std::sync::Arc;

use energyml_core::error::{Error, Result};
use energyml_core::registry::Registry;
use quick_xml::events::Event;

/// Save a package whose parts were staged as **bytes**, rebuilding its `.rels`.
///
/// # Why one save is not enough
///
/// Staging bytes is what protects the content: nothing is deserialised, so
/// nothing can be dropped. But a part staged that way has no *object*, and the
/// `.rels` are built from an object's `DataObjectReference`s — so a single save
/// writes parts that are byte-perfect and an index that is **gone**. Measured on
/// a 1 344-object package: 1 335 `.rels` down to 2.
///
/// The second pass reopens what the first wrote. That package now declares the
/// version its parts really are, so its objects read as the right release and
/// their references parse — which is what the writer builds the index from. Same
/// package, measured: **1 345** `.rels`, *more* than the original, because
/// reading it correctly finds links the producer's own index omitted.
///
/// # Why this is here rather than in the caller
///
/// Because it is knowledge about the storage layer, not about a command line.
/// Anything that stages bytes and saves — a repair tool, the front end, an ETP
/// client later — walks into the same trap, and a binary that knows something
/// the library does not is a bug in the layering.
///
/// # Errors
///
/// The temporary is **left behind** when the second pass fails, and its path is
/// in the error: its bytes are right and only its index is missing, which beats
/// deleting the one usable result.
pub fn save_reindexed(
    store: &crate::EpcStore,
    out: &Path,
    registry: Arc<Registry>,
    options: &crate::SaveOptions,
) -> crate::Result<Vec<crate::Diagnostic>> {
    let temporary = out.with_extension("relabel-pass1.epc");
    store.save_as(&temporary, options)?;

    let reopened = match crate::EpcStore::open(&temporary, registry) {
        Ok(store) => store,
        Err(e) => {
            return Err(crate::Error::Package(format!(
                "the relabelled parts were written to {} but it could not be reopened to rebuild the relationships, so that byte-perfect package is left there: {e}",
                temporary.display()
            )));
        }
    };
    let diagnostics = match reopened.save_as(out, options) {
        Ok(diagnostics) => diagnostics,
        Err(e) => {
            return Err(crate::Error::Package(format!(
                "the relabelled parts were written to {} but the relationships could not be rebuilt, so that byte-perfect package is left there: {e}",
                temporary.display()
            )));
        }
    };
    // Only once the real file is written: a failure above must not take the
    // one result that is worth keeping with it.
    let _ = std::fs::remove_file(&temporary);
    Ok(diagnostics)
}

/// The same two passes, entirely in memory.
///
/// The doc of [`save_reindexed`] names the front end as one of the callers that
/// walks into the trap; this is what it calls. A browser has no filesystem, so
/// the intermediate is a `Vec<u8>` — but the reason for two passes is
/// unchanged: the first writes byte-perfect parts and an index built from
/// objects that read as the wrong release, and the second reopens what the
/// first wrote, now declaring what it really is.
///
/// # Errors
///
/// When the second pass fails, **the first pass's bytes come back with the
/// error** — they are the byte-perfect package, missing only its index, and
/// throwing them away would leave the caller with nothing.
pub fn save_reindexed_bytes(
    store: &crate::EpcStore,
    registry: Arc<Registry>,
    options: &crate::SaveOptions,
) -> std::result::Result<Vec<u8>, (String, Vec<u8>)> {
    let mut first = std::io::Cursor::new(Vec::new());
    if let Err(e) = store.write_to(&mut first, options) {
        return Err((format!("the relabelled package could not be written: {e}"), Vec::new()));
    }
    let bytes = first.into_inner();

    let reopened = match crate::EpcStore::from_reader(
        Box::new(std::io::Cursor::new(bytes.clone())),
        "relabel-pass1.epc",
        registry,
    ) {
        Ok(store) => store,
        Err(e) => {
            return Err((
                format!(
                    "the parts were relabelled but the package could not be reopened to rebuild its relationships, so what comes back is byte-perfect with the index it had: {e}"
                ),
                bytes,
            ));
        }
    };
    let mut second = std::io::Cursor::new(Vec::new());
    if let Err(e) = reopened.write_to(&mut second, options) {
        return Err((
            format!(
                "the parts were relabelled but the relationships could not be rebuilt, so what comes back is byte-perfect with the index it had: {e}"
            ),
            bytes,
        ));
    }
    Ok(second.into_inner())
}

/// What relabelling a whole package would touch.
///
/// **Surveying and applying are two calls**, because the answer to *what would
/// this do* has to be available before anything is written — a front end shows it,
/// a command line prints it, and both need the refusal below without having
/// modified a store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Survey {
    /// The package asked about, lower-cased: `resqml`, `eml`, `witsml`, `prodml`.
    pub package: String,
    /// Part name and qualified type of every part that would be rewritten.
    pub targets: Vec<(String, String)>,
    /// Every release of that package the file holds, in order.
    pub versions: Vec<String>,
    /// Parts whose qualified type this build cannot place in a package.
    ///
    /// Counted rather than skipped: *not yours* and *unknown* are two different
    /// answers, and only one of them is somebody else's problem.
    pub undecided: usize,
}

impl Survey {
    /// Why this cannot be applied, when it cannot.
    ///
    /// One case, and it is the one that matters: **a package holding two
    /// releases**. An EPC may legitimately mix them (EPC §3.2) — the reference
    /// package does — and writing `2.2_dev3` onto a 2.0.1 part makes it readable
    /// as neither, with nothing downstream to report it. Refusing by name beats
    /// picking one.
    pub fn refusal(&self) -> Option<String> {
        (self.versions.len() > 1).then(|| {
            format!(
                "this package holds `{}` parts on {} releases ({}) — an EPC may mix them \
                 (EPC §3.2), so say which release to relabel",
                self.package,
                self.versions.len(),
                self.versions.join(", ")
            )
        })
    }
}

/// What relabelling came to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Parts whose declaration said something else, with what it said.
    pub replaced: Vec<(String, String)>,
    /// Parts that declared nothing at all. `schemaVersion` is `use="required"`,
    /// so these were already non-conformant.
    pub added: Vec<String>,
    /// Parts already declaring the wanted version. Nothing was written.
    pub already: usize,
    /// Parts that could not be rewritten, each with its reason.
    pub failed: Vec<String>,
}

impl Outcome {
    /// How many parts changed.
    pub fn changed(&self) -> usize {
        self.replaced.len() + self.added.len()
    }
}

/// Which parts of `package` a relabelling would touch.
///
/// `from` restricts to one release, which is how a mixed package is relabelled
/// safely. The version each part is on comes from its **summary**, so this costs
/// no deserialisation: the qualified type is known from the part's first few
/// hundred bytes.
pub fn survey(store: &crate::EpcStore, package: &str, from: Option<&str>) -> Survey {
    let wanted = package.trim().to_ascii_lowercase();
    let mut targets = Vec::new();
    let mut versions: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut undecided = 0usize;

    for summary in store.resources() {
        match energyml_core::identifiers::QualifiedType::parse(&summary.qualified_type) {
            Ok(qt) if qt.domain.eq_ignore_ascii_case(&wanted) => {
                let dotted = qt.dotted_version();
                versions.insert(dotted.clone());
                if let Some(only) = from
                    && !energyml_core::naming::version_eq(&dotted, only)
                {
                    continue;
                }
                targets.push((summary.locator.clone(), summary.qualified_type.clone()));
            }
            Ok(_) => {}
            Err(_) => undecided += 1,
        }
    }
    Survey {
        package: wanted,
        targets,
        versions: versions.into_iter().collect(),
        undecided,
    }
}

/// Rewrite the declaration of every part the survey names.
///
/// Stages bytes; **nothing reaches a file** until the caller saves — with
/// [`save_reindexed`], which is the only save that keeps the staged bytes and
/// still rebuilds the index.
pub fn apply(store: &mut crate::EpcStore, survey: &Survey, version: &str) -> Outcome {
    let mut out = Outcome::default();
    for (part, qualified) in &survey.targets {
        let bytes = match store.raw_part(part) {
            Ok(bytes) => bytes,
            Err(e) => {
                out.failed.push(format!("{part}: cannot be read as bytes: {e}"));
                continue;
            }
        };
        match set_schema_version(&bytes, version) {
            Ok((_, Relabel::Unchanged)) => out.already += 1,
            Ok((written, Relabel::Replaced { was })) => {
                out.replaced.push((qualified.clone(), was));
                store.set_raw_part(part, written);
            }
            Ok((written, Relabel::Added)) => {
                out.added.push(qualified.clone());
                store.set_raw_part(part, written);
            }
            Err(e) => out.failed.push(format!("{part}: {e}")),
        }
    }
    out
}

/// The attribute this module exists to write.
const ATTRIBUTE: &[u8] = b"schemaVersion";

/// What changing one part came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Relabel {
    /// The part already declared this version. Nothing was written.
    Unchanged,
    /// The attribute was there and said something else.
    Replaced {
        /// What the document declared before, verbatim.
        was: String,
    },
    /// The attribute was absent, and has been added.
    ///
    /// Not an unusual repair: `schemaVersion` is `use="required"`, so a part
    /// without one was already non-conformant.
    Added,
}

impl Relabel {
    /// Whether any byte changed.
    pub fn changed(&self) -> bool {
        !matches!(self, Relabel::Unchanged)
    }
}

/// Rewrite the `schemaVersion` of a part's root element.
///
/// Returns the new bytes and what happened. The input is not parsed beyond
/// locating the root tag: everything outside the one attribute is copied.
///
/// # Errors
///
/// A version carrying `"`, `'`, `<` or `&` is **refused rather than escaped**.
/// Escaping would put a value in the file that reads back as something else,
/// and no real schema version contains any of them — so the string is far more
/// likely to be a mistake than an intention.
pub fn set_schema_version(xml: &[u8], version: &str) -> Result<(Vec<u8>, Relabel)> {
    if version.trim().is_empty() {
        return Err(Error::parse(version, "schemaVersion", "must not be empty"));
    }
    if let Some(bad) = version.chars().find(|c| matches!(c, '"' | '\'' | '<' | '&')) {
        return Err(Error::parse(
            version,
            "schemaVersion",
            format!("contains `{bad}`, which an attribute value cannot carry unescaped"),
        ));
    }

    let (open, close) = root_tag(xml)?;
    let tag = &xml[open..close];

    match find_attribute(tag) {
        Some(span) => {
            let held = std::str::from_utf8(&tag[span.clone()])
                .map_err(|_| Error::parse("", "schemaVersion", "value is not UTF-8"))?;
            if held == version {
                return Ok((xml.to_vec(), Relabel::Unchanged));
            }
            let was = held.to_string();
            let mut out = Vec::with_capacity(xml.len() + version.len());
            out.extend_from_slice(&xml[..open + span.start]);
            out.extend_from_slice(version.as_bytes());
            out.extend_from_slice(&xml[open + span.end..]);
            Ok((out, Relabel::Replaced { was }))
        }
        None => {
            // After the element name, before any other attribute: the one
            // position that needs no knowledge of what the others are.
            let at = open + name_end(tag);
            let mut out = Vec::with_capacity(xml.len() + version.len() + 20);
            out.extend_from_slice(&xml[..at]);
            out.extend_from_slice(b" ");
            out.extend_from_slice(ATTRIBUTE);
            out.extend_from_slice(b"=\"");
            out.extend_from_slice(version.as_bytes());
            out.extend_from_slice(b"\"");
            out.extend_from_slice(&xml[at..]);
            Ok((out, Relabel::Added))
        }
    }
}

/// The byte range of the root element's start tag, `<` to `>` inclusive.
///
/// quick_xml does the locating, so a comment, a processing instruction or a
/// doctype before the root is skipped by a parser rather than by a guess about
/// where `<` means what.
fn root_tag(xml: &[u8]) -> Result<(usize, usize)> {
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(_)) | Ok(Event::Empty(_)) => {
                let close = reader.buffer_position() as usize;
                // XML forbids a raw `<` inside an attribute value, so the last
                // one before the tag's `>` opens that tag and nothing else.
                let open = xml[..close]
                    .iter()
                    .rposition(|b| *b == b'<')
                    .ok_or_else(|| Error::parse("", "root element", "no start tag"))?;
                return Ok((open, close));
            }
            Ok(Event::Eof) => {
                return Err(Error::parse("", "root element", "the part has no elements"));
            }
            Ok(_) => buf.clear(),
            Err(e) => return Err(Error::parse("", "root element", e.to_string())),
        }
    }
}

/// Where the element name ends inside a tag: after `<Name`, before whatever
/// follows.
fn name_end(tag: &[u8]) -> usize {
    let mut at = 1; // past `<`
    while at < tag.len() && !tag[at].is_ascii_whitespace() && tag[at] != b'>' && tag[at] != b'/' {
        at += 1;
    }
    at
}

/// The byte range of the `schemaVersion` **value** inside a tag, quotes
/// excluded.
///
/// Matched as a whole attribute name — preceded by whitespace and followed by
/// `=` — so an attribute merely *ending* in it, or an element named after it,
/// is not mistaken for the one being looked for.
fn find_attribute(tag: &[u8]) -> Option<std::ops::Range<usize>> {
    let mut at = name_end(tag);
    while at < tag.len() {
        if !tag[at].is_ascii_whitespace() {
            at += 1;
            continue;
        }
        let name = at + 1;
        if !tag[name..].starts_with(ATTRIBUTE) {
            at += 1;
            continue;
        }
        let mut cursor = name + ATTRIBUTE.len();
        while cursor < tag.len() && tag[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= tag.len() || tag[cursor] != b'=' {
            at += 1;
            continue;
        }
        cursor += 1;
        while cursor < tag.len() && tag[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let quote = *tag.get(cursor)?;
        if quote != b'"' && quote != b'\'' {
            return None;
        }
        cursor += 1;
        let end = cursor + tag[cursor..].iter().position(|b| *b == quote)?;
        return Some(cursor..end);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(bytes: &[u8]) -> String {
        String::from_utf8(bytes.to_vec()).expect("utf-8")
    }

    #[test]
    fn an_existing_declaration_is_replaced_and_nothing_else_moves() {
        let xml = br#"<?xml version="1.0"?>
<Feature xmlns="urn:x" uuid="abc" schemaVersion="2.2"><Citation/></Feature>"#;
        let (out, what) = set_schema_version(xml, "2.2_dev3").expect("relabels");
        assert_eq!(
            what,
            Relabel::Replaced {
                was: "2.2".to_string()
            }
        );
        assert!(text(&out).contains(r#"schemaVersion="2.2_dev3""#));
        // Everything the edit did not come for is byte-identical: the
        // declaration, the namespace, the other attribute, the children.
        assert_eq!(
            text(&out).replace(r#"schemaVersion="2.2_dev3""#, r#"schemaVersion="2.2""#),
            text(xml),
            "only the one value changed"
        );
    }

    #[test]
    fn an_absent_declaration_is_added_after_the_element_name() {
        // `schemaVersion` is `use="required"`, so a part without one was
        // already non-conformant; adding it is a repair, not an invention.
        let xml = br#"<Feature xmlns="urn:x" uuid="abc"/>"#;
        let (out, what) = set_schema_version(xml, "2.2_dev3").expect("relabels");
        assert_eq!(what, Relabel::Added);
        assert_eq!(
            text(&out),
            r#"<Feature schemaVersion="2.2_dev3" xmlns="urn:x" uuid="abc"/>"#
        );
    }

    #[test]
    fn a_part_already_saying_it_is_left_untouched() {
        let xml = br#"<Feature schemaVersion="2.2_dev3"/>"#;
        let (out, what) = set_schema_version(xml, "2.2_dev3").expect("relabels");
        assert_eq!(what, Relabel::Unchanged);
        assert!(!what.changed());
        assert_eq!(out, xml.to_vec(), "an unchanged part is copied, not rebuilt");
    }

    #[test]
    fn single_quotes_are_read_and_the_style_is_kept() {
        let xml = br#"<Feature schemaVersion='2.2' uuid='a'/>"#;
        let (out, _) = set_schema_version(xml, "2.2_dev3").expect("relabels");
        assert_eq!(text(&out), r#"<Feature schemaVersion='2.2_dev3' uuid='a'/>"#);
    }

    #[test]
    fn a_comment_before_the_root_does_not_become_the_root() {
        // The reason quick_xml locates the tag instead of a search for `<`.
        let xml = br#"<?xml version="1.0"?><!-- <NotTheRoot schemaVersion="9"/> -->
<Feature schemaVersion="2.2"/>"#;
        let (out, what) = set_schema_version(xml, "2.2_dev3").expect("relabels");
        assert_eq!(
            what,
            Relabel::Replaced {
                was: "2.2".to_string()
            }
        );
        assert!(
            text(&out).contains(r#"<!-- <NotTheRoot schemaVersion="9"/> -->"#),
            "the comment is untouched: {}",
            text(&out)
        );
        assert!(text(&out).contains(r#"<Feature schemaVersion="2.2_dev3"/>"#));
    }

    #[test]
    fn an_attribute_merely_ending_in_the_name_is_not_it() {
        let xml = br#"<Feature xschemaVersion="9" schemaVersion="2.2"/>"#;
        let (out, _) = set_schema_version(xml, "2.2_dev3").expect("relabels");
        assert!(text(&out).contains(r#"xschemaVersion="9""#), "{}", text(&out));
        assert!(text(&out).contains(r#"schemaVersion="2.2_dev3""#));
    }

    #[test]
    fn a_version_an_attribute_cannot_carry_is_refused_not_escaped() {
        let xml = br#"<Feature schemaVersion="2.2"/>"#;
        for bad in ["2.2\"dev", "2.2&dev", "2.2<dev", "  "] {
            assert!(
                set_schema_version(xml, bad).is_err(),
                "`{bad}` must be refused rather than written"
            );
        }
    }

    #[test]
    fn a_part_with_no_element_at_all_is_an_error() {
        assert!(set_schema_version(b"<?xml version=\"1.0\"?>", "2.2").is_err());
        assert!(set_schema_version(b"", "2.2").is_err());
    }
}
