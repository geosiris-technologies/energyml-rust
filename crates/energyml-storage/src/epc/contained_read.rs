//! Reading an object that a part holds **by value** (a WITSML `Channel` inside a
//! `ChannelSet` inside a `Log`): its citation, its bytes, the object itself.
//!
//! A contained object is not a part. It has no entry of its own in the
//! inventory, and the part that holds it is the *container's*; asking that part
//! for "the object at this URI" would return the container, an answer that looks
//! right and is wrong. So a contained object is reached by **the same door as
//! any object**, [`EpcStore::lookup`] (through `locate`), which now may answer
//! with a [`Live`] whose `nested` field is set and whose `archive` is `None`:
//! every path that reads a part (`entry`) then fails with `NotFound` instead of
//! reading the container, and the three paths that must serve a contained object
//! (`header`, `get_object`, raw bytes) branch on `nested` first.
//!
//! # How a contained object is read
//!
//! One road, the ordinary one (ETP 1.2 section 9.1.3.1, p. 179: a contained
//! object is a standalone data object in its own right):
//!
//! 1. the index (`super::contents`) gives the byte range of the element in the
//!    part, `span`;
//! 2. [`extract`] cuts that range and makes a **standalone document** of it: the
//!    root is renamed to the global element of the object's type (the element a
//!    slot names, `FormationMarker`, is not the element the type is written
//!    under as a part, `WellboreMarker`), the namespace declarations in scope at
//!    that place are carried to the new root, `xsi:type` is dropped when the
//!    renaming says it, and `schemaVersion` (required on every
//!    `AbstractObject`, `eml23` `Abstract.xsd` l. 307) is added when the element
//!    lacks it;
//! 3. the document is read by [`Registry::read_xml_as_named`], the reader a part
//!    goes through, in the same passes ([`super::read_passes`]). There is no
//!    second reader and no reflection-based clone of a sub-tree.
//!
//! Every request is **grouped by container**: a part is read once for all the
//! contained objects asked of it ([`per_part`]), and the namespace scopes of all
//! of them come from one pass over it ([`scopes_at`]).
//!
//! # When the index is consulted
//!
//! Never by `resources()`, never for a part (a part of its own right wins over a
//! copy held by value, `nested_index` rule), and only for a lookup that **found
//! no part** and whose type may be contained ([`Registry::may_be_contained`]) or
//! names no type ([`EpcStore::live_for`]). A RESQML 2.0.1 package therefore never
//! reads a part more than before.
//!
//! # What is refused, by name
//!
//! Writing is a later pass; until then the three operations whose result an EPC
//! could only guess are refused with [`Error::Unsupported`]: putting an object
//! that exists only by value, deleting an object held by value, deleting a
//! container whose contained objects live only inside it (they would disappear
//! with it, where an ETP store keeps them: ETP 1.2 section 9.2.3 row 26, p. 192).

use std::collections::{BTreeMap, HashSet};
use std::io::{BufReader, Read};

use energyml_core::identifiers::EtpUri;
use energyml_core::ns::{Namespace, XSI};
use energyml_core::reflect::TypeInfo;
use energyml_core::registry::DynObject;
use quick_xml::events::Event;

use super::contents::ContainedEntry;
use super::{EpcStore, Live, header};
use crate::error::{Diagnostic, Error, Result};
use crate::graph::Edge;
use crate::kinds::{Judged, KindRules, StandardRules};
use crate::raw::{RawFormat, RawObject};
use crate::resource::{ObjectHeader, ObjectSummary};
use crate::storage::Contents;

/// Which store refuses, in the words of [`Error::Unsupported`].
const STORE: &str = "EPC";

/// `(prefix, namespace uri)`, the prefix empty for the default namespace.
type Declarations = Vec<(String, String)>;

// ---------------------------------------------------------------------------
// The scope of a place in a part
// ---------------------------------------------------------------------------

/// What an element inherits from its ancestors, and from nothing else: the
/// namespace declarations in scope where it starts, and the `schemaVersion` of
/// the part's root.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Scope {
    /// Declarations in scope, outermost first. A prefix declared twice appears
    /// twice: the later one hides the earlier.
    pub declarations: Declarations,
    /// `schemaVersion` of the part's root element, when it states one.
    pub schema_version: Option<String>,
}

/// A raw attribute value, unescaped (a namespace uri never needs more).
fn unescape(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    quick_xml::escape::unescape(&text)
        .map(|unescaped| unescaped.into_owned())
        .unwrap_or_else(|_| text.into_owned())
}

/// The [`Scope`] at the start of each wanted element, in the order of `entries`,
/// from **one** pass over `part`; the pass stops at the last wanted start.
///
/// `None` for an entry whose span does not start exactly where an element
/// starts (a span that begins inside a tag or a text run is not an element, and
/// a scope taken "about there" would be a guess).
pub(crate) fn scopes_at(part: &[u8], entries: &[&ContainedEntry]) -> Vec<Option<Scope>> {
    let mut out: Vec<Option<Scope>> = vec![None; entries.len()];
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by_key(|&at| entries[at].span.start);

    let mut reader = quick_xml::Reader::from_reader(part);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut frames: Vec<Declarations> = Vec::new();
    let mut root_schema: Option<String> = None;
    let mut seen_root = false;
    let mut next = 0;

    while next < order.len() {
        // Where the next event begins: everything before it has been consumed.
        let before = usize::try_from(reader.buffer_position()).unwrap_or(usize::MAX);
        while next < order.len() && entries[order[next]].span.start <= before {
            if entries[order[next]].span.start == before {
                out[order[next]] = Some(Scope {
                    declarations: frames.iter().flatten().cloned().collect(),
                    schema_version: root_schema.clone(),
                });
            }
            next += 1;
        }
        if next == order.len() {
            break;
        }
        buf.clear();
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(start)) => {
                let mut own = Declarations::new();
                for attribute in start.attributes().flatten() {
                    let key = attribute.key.as_ref();
                    let value = unescape(&attribute.value);
                    if key == b"xmlns" {
                        own.push((String::new(), value));
                    } else if let Some(bound) = key.strip_prefix(b"xmlns:") {
                        own.push((String::from_utf8_lossy(bound).into_owned(), value));
                    } else if !seen_root && key == b"schemaVersion" {
                        root_schema = Some(value);
                    }
                }
                seen_root = true;
                frames.push(own);
            }
            Ok(Event::End(_)) => {
                frames.pop();
            }
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => {}
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The extraction
// ---------------------------------------------------------------------------

/// What the extraction needs of the **type** of the contained object, taken from
/// its [`TypeInfo`] so that [`extract`] does not depend on a registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Target {
    /// Local name of the global element the type is written under as a part
    /// (`Channel`, `WellboreMarker`; `FlowTestActivity` for a PRODML type that
    /// has no element of its own: [`TypeInfo::root_element`]).
    pub element: &'static str,
    /// The namespace of that element.
    pub namespace: Namespace,
    /// The complex type name, written in `xsi:type` when the type needs it.
    pub type_name: &'static str,
    /// Whether `xsi:type` must stay on the root: the type has no element of its
    /// own, so the element alone does not say which type it is.
    pub keep_xsi_type: bool,
    /// `schemaVersion` of the type's release (`2.1`), the last resort when
    /// neither the element nor its part states one.
    pub schema_version: &'static str,
}

impl Target {
    /// The target of a registered type.
    pub(crate) fn of(info: &TypeInfo) -> Self {
        Self {
            element: info.root_element.unwrap_or(info.xml_name),
            namespace: info.xml_ns,
            type_name: info.object_type,
            keep_xsi_type: info.root_element.is_some(),
            schema_version: info.schema_version,
        }
    }
}

/// A contained object as a standalone document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Extracted {
    /// The document: no XML declaration, UTF-8.
    pub bytes: Vec<u8>,
    /// What was supplied that the element did not state, one sentence each;
    /// the caller turns them into diagnostics.
    pub notes: Vec<String>,
}

/// One attribute of a start tag, as written.
struct RawAttr<'t> {
    /// The key, prefix included.
    key: &'t str,
    /// The value between the quotes, still escaped.
    value: &'t str,
    /// The quote it was written with.
    quote: char,
}

/// A start tag, cut apart.
struct StartTag<'t> {
    /// The qualified name.
    name: &'t str,
    /// The attributes, in order.
    attrs: Vec<RawAttr<'t>>,
    /// Written `<x ... />`.
    empty: bool,
    /// Length in bytes of the tag, `>` included.
    head_len: usize,
}

/// Cut the start tag at the beginning of `text`.
fn parse_start_tag(text: &str) -> std::result::Result<StartTag<'_>, &'static str> {
    let bytes = text.as_bytes();
    let space = |b: u8| b.is_ascii_whitespace();
    if bytes.first() != Some(&b'<') || matches!(bytes.get(1), None | Some(b'/' | b'!' | b'?')) {
        return Err("the span does not start at a start tag");
    }
    let mut at = 1;
    while bytes
        .get(at)
        .is_some_and(|b| !space(*b) && *b != b'>' && *b != b'/')
    {
        at += 1;
    }
    let name = &text[1..at];
    let mut attrs = Vec::new();
    loop {
        while bytes.get(at).is_some_and(|b| space(*b)) {
            at += 1;
        }
        match bytes.get(at).copied() {
            None => return Err("the start tag is not closed"),
            Some(b'>') => {
                return Ok(StartTag {
                    name,
                    attrs,
                    empty: false,
                    head_len: at + 1,
                });
            }
            Some(b'/') => {
                return if bytes.get(at + 1) == Some(&b'>') {
                    Ok(StartTag {
                        name,
                        attrs,
                        empty: true,
                        head_len: at + 2,
                    })
                } else {
                    Err("a `/` that does not close the start tag")
                };
            }
            Some(_) => {
                let key_start = at;
                while bytes
                    .get(at)
                    .is_some_and(|b| !space(*b) && *b != b'=' && *b != b'>' && *b != b'/')
                {
                    at += 1;
                }
                let key = &text[key_start..at];
                while bytes.get(at).is_some_and(|b| space(*b)) {
                    at += 1;
                }
                if bytes.get(at) != Some(&b'=') {
                    return Err("an attribute without a value");
                }
                at += 1;
                while bytes.get(at).is_some_and(|b| space(*b)) {
                    at += 1;
                }
                let quote = match bytes.get(at).copied() {
                    Some(q) if q == b'"' || q == b'\'' => q,
                    _ => return Err("an attribute value that is not quoted"),
                };
                at += 1;
                let value_start = at;
                while bytes.get(at).is_some_and(|b| *b != quote) {
                    at += 1;
                }
                if at >= bytes.len() {
                    return Err("an attribute value that is not closed");
                }
                attrs.push(RawAttr {
                    key,
                    value: &text[value_start..at],
                    quote: char::from(quote),
                });
                at += 1;
            }
        }
    }
}

/// An extraction failure, naming the object and its part.
fn fail(entry: &ContainedEntry, why: impl std::fmt::Display) -> Error {
    Error::Xml(format!(
        "contained object `{}` in `{}`: {why}",
        entry.summary.uri, entry.part
    ))
}

/// The uri `prefix` is bound to, the nearest declaration winning.
fn resolve<'d>(bound: &'d [(String, String)], prefix: &str) -> Option<&'d str> {
    bound
        .iter()
        .rev()
        .find(|(declared, _)| declared == prefix)
        .map(|(_, uri)| uri.as_str())
}

/// A prefix bound to `uri` and not hidden since, the nearest first.
fn prefix_for<'d>(bound: &'d [(String, String)], uri: &str) -> Option<&'d str> {
    bound
        .iter()
        .rev()
        .find(|(prefix, declared)| declared == uri && resolve(bound, prefix) == Some(uri))
        .map(|(prefix, _)| prefix.as_str())
}

/// A prefix near `wanted` that nothing binds yet.
fn fresh_prefix(bound: &[(String, String)], wanted: &str) -> String {
    let base = if wanted.is_empty() { "ns" } else { wanted };
    if resolve(bound, base).is_none() {
        return base.to_string();
    }
    (1..)
        .map(|n| format!("{base}{n}"))
        .find(|candidate| resolve(bound, candidate).is_none())
        .unwrap_or_else(|| base.to_string())
}

/// Escape a value written between double quotes.
fn escape_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('"', "&quot;")
}

/// Write one namespace declaration.
fn push_declaration(out: &mut String, prefix: &str, uri: &str) {
    out.push_str(" xmlns");
    if !prefix.is_empty() {
        out.push(':');
        out.push_str(prefix);
    }
    out.push_str("=\"");
    out.push_str(&escape_attr(uri));
    out.push('"');
}

/// The contained object `entry` as a standalone document.
///
/// `part` is the container's bytes, `target` the type of the object, `scope` the
/// scope at the start of its element ([`scopes_at`]). The root is renamed to
/// the global element of the type, the declarations in scope are carried over
/// (those the element redeclares keep the element's own), `xsi:type` is dropped
/// unless the type has no element of its own (then it is added when missing),
/// and `schemaVersion` is supplied when the element lacks it (noted).
///
/// Nothing is repaired or guessed beyond that: a span that is not exactly one
/// element is refused.
pub(crate) fn extract(
    part: &[u8],
    entry: &ContainedEntry,
    target: &Target,
    scope: &Scope,
) -> Result<Extracted> {
    let inner = part
        .get(entry.span.clone())
        .ok_or_else(|| fail(entry, "its span lies outside its part"))?;
    let text = std::str::from_utf8(inner)
        .map_err(|_| fail(entry, "its part is not UTF-8 (TODO(verify): UTF-16 packages)"))?;
    let tag = parse_start_tag(text).map_err(|why| fail(entry, why))?;

    // The end tag, checked against the start tag; an empty element has none.
    let children = if tag.empty {
        None
    } else {
        let close = text
            .rfind("</")
            .filter(|close| *close >= tag.head_len)
            .ok_or_else(|| fail(entry, "its end tag is missing"))?;
        let closing = text[close + 2..].trim_end().strip_suffix('>').map(str::trim_end);
        if closing != Some(tag.name) {
            return Err(fail(entry, "its end tag does not match its start tag"));
        }
        Some(&text[tag.head_len..close])
    };

    // Declarations: the ancestors', then the element's own.
    let own: Declarations = tag
        .attrs
        .iter()
        .filter_map(|attr| {
            if attr.key == "xmlns" {
                Some((String::new(), unescape(attr.value.as_bytes())))
            } else {
                attr.key
                    .strip_prefix("xmlns:")
                    .map(|prefix| (prefix.to_string(), unescape(attr.value.as_bytes())))
            }
        })
        .collect();
    let mut bound: Declarations = scope.declarations.clone();
    bound.extend(own.iter().cloned());

    // Carried: what the ancestors declare, nearest wins, unless the element
    // redeclares the prefix itself.
    let mut carried: Declarations = Vec::new();
    for (prefix, uri) in scope.declarations.iter().rev() {
        if own.iter().any(|(declared, _)| declared == prefix)
            || carried.iter().any(|(declared, _)| declared == prefix)
        {
            continue;
        }
        carried.push((prefix.clone(), uri.clone()));
    }
    carried.reverse();

    // The root: the prefix the type's namespace has in scope, or one declared here.
    let mut added: Declarations = Vec::new();
    let root_prefix = match prefix_for(&bound, target.namespace.uri) {
        Some(prefix) => prefix.to_string(),
        None => {
            let prefix = fresh_prefix(&bound, target.namespace.prefix);
            added.push((prefix.clone(), target.namespace.uri.to_string()));
            bound.push((prefix.clone(), target.namespace.uri.to_string()));
            prefix
        }
    };
    let root = if root_prefix.is_empty() {
        target.element.to_string()
    } else {
        format!("{root_prefix}:{}", target.element)
    };

    // xsi:type: dropped when the root element already says the type, kept (or
    // added) when the type has no element of its own.
    let is_xsi_type = |key: &str| {
        key.split_once(':')
            .is_some_and(|(prefix, local)| local == "type" && resolve(&bound, prefix) == Some(XSI.uri))
    };
    let had_type = tag.attrs.iter().any(|attr| is_xsi_type(attr.key));
    let kept: Vec<&RawAttr<'_>> = tag
        .attrs
        .iter()
        .filter(|attr| target.keep_xsi_type || !is_xsi_type(attr.key))
        .collect();

    let mut supplied: Vec<(String, String)> = Vec::new();
    let mut notes = Vec::new();
    if target.keep_xsi_type && !had_type {
        let xsi = match prefix_for(&bound, XSI.uri).filter(|prefix| !prefix.is_empty()) {
            Some(prefix) => prefix.to_string(),
            None => {
                let prefix = fresh_prefix(&bound, XSI.prefix);
                added.push((prefix.clone(), XSI.uri.to_string()));
                bound.push((prefix.clone(), XSI.uri.to_string()));
                prefix
            }
        };
        let value = if root_prefix.is_empty() {
            target.type_name.to_string()
        } else {
            format!("{root_prefix}:{}", target.type_name)
        };
        supplied.push((format!("{xsi}:type"), value));
    }
    // `schemaVersion` is required on every AbstractObject (eml23 Abstract.xsd
    // l. 307). TODO(verify): on a real WITSML file, that a contained object
    // carries its own (the XSD requires it; a producer may not write it).
    if !tag.attrs.iter().any(|attr| attr.key == "schemaVersion") {
        let (version, whose) = match &scope.schema_version {
            Some(version) => (version.clone(), "its container's"),
            None => (target.schema_version.to_string(), "its type's"),
        };
        notes.push(format!(
            "the element states no schemaVersion (required by the schema); {whose} `{version}` was written in the extract"
        ));
        supplied.push(("schemaVersion".to_string(), version));
    }

    let mut out = String::with_capacity(text.len() + 256);
    out.push('<');
    out.push_str(&root);
    for (prefix, uri) in carried.iter().chain(added.iter()) {
        push_declaration(&mut out, prefix, uri);
    }
    for attr in kept {
        out.push(' ');
        out.push_str(attr.key);
        out.push('=');
        out.push(attr.quote);
        out.push_str(attr.value);
        out.push(attr.quote);
    }
    for (key, value) in &supplied {
        out.push(' ');
        out.push_str(key);
        out.push_str("=\"");
        out.push_str(&escape_attr(value));
        out.push('"');
    }
    match children {
        None => out.push_str("/>"),
        Some(children) => {
            out.push('>');
            out.push_str(children);
            out.push_str("</");
            out.push_str(&root);
            out.push('>');
        }
    }
    Ok(Extracted {
        bytes: out.into_bytes(),
        notes,
    })
}

// ---------------------------------------------------------------------------
// Grouping: one read of a part for every object asked of it
// ---------------------------------------------------------------------------

/// The wanted `(position in the request, entry)` pairs, grouped by the part that
/// holds them, in part-name order.
pub(crate) fn group_by_part<'e>(
    wanted: &[(usize, &'e ContainedEntry)],
) -> BTreeMap<&'e str, Vec<(usize, &'e ContainedEntry)>> {
    let mut groups: BTreeMap<&str, Vec<(usize, &ContainedEntry)>> = BTreeMap::new();
    for (at, entry) in wanted {
        groups.entry(entry.part.as_str()).or_default().push((*at, *entry));
    }
    groups
}

/// Run `each` over the `entries` of **one** part, loading the part once and
/// computing every scope in one pass over it. One result per entry, in order;
/// a part that cannot be loaded fails each of its entries, and no other.
pub(crate) fn per_part<T>(
    entries: &[&ContainedEntry],
    load: impl FnOnce() -> Result<Vec<u8>>,
    each: impl Fn(&[u8], &ContainedEntry, &Scope) -> Result<T>,
) -> Vec<Result<T>> {
    let bytes = match load() {
        Ok(bytes) => bytes,
        Err(error) => {
            let why = error.to_string();
            return entries
                .iter()
                .map(|entry| Err(fail(entry, format!("its part cannot be read: {why}"))))
                .collect();
        }
    };
    let scopes = scopes_at(&bytes, entries);
    entries
        .iter()
        .zip(scopes)
        .map(|(entry, scope)| match scope {
            Some(scope) => each(&bytes, entry, &scope),
            None => Err(fail(entry, "its span does not start at an element of its part")),
        })
        .collect()
}

/// The citation of the contained object, from its own span: `ObjectHeader` as
/// [`EpcStore::header`] builds it for a part.
pub(super) fn header_from_peek(summary: &ObjectSummary, peeked: header::PeekedHeader) -> ObjectHeader {
    let mut header = ObjectHeader::new(summary.clone());
    header.title = peeked.title;
    header.originator = peeked.originator;
    header.creation = peeked.creation;
    header.last_update = peeked.last_update;
    header.format = peeked.format;
    header.object_version = peeked.object_version;
    header.schema_version = peeked.schema_version;
    header
}

/// The `objectVersion` of a uri, **borrowed from it**: `Some(None)` for a uri
/// that names none, `Some(Some(v))` for one that names `v`, `None` when it cannot
/// be borrowed (an unreadable uri, or a version holding a quote, which the uri
/// writes doubled).
///
/// [`Live::version`] borrows; the contained objects keep their version only in
/// their uri.
pub(crate) fn object_version_in(uri: &str) -> Option<Option<&str>> {
    let parsed = EtpUri::parse(uri).ok()?;
    let Some(version) = parsed.object_version else {
        return Some(None);
    };
    let marker = "version='";
    let from = uri.find(marker)? + marker.len();
    let tail = uri.get(from..)?;
    if tail.starts_with(version.as_str()) && tail[version.len()..].starts_with('\'') {
        Some(Some(&tail[..version.len()]))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// The store
// ---------------------------------------------------------------------------

impl EpcStore {
    /// The parts whose archive entries a session has replaced, removed or
    /// restaged: the index was built from the archive and says nothing true
    /// about them, so what it lists of them is not served.
    ///
    /// `TODO(verify)`: that staging bytes ([`EpcStore::set_raw_part`]) makes the
    /// overlay dirty. `TODO(measure)`: a scan of the inventory per call while a
    /// session has unsaved changes.
    fn masked_parts(&self) -> HashSet<&str> {
        if !self.overlay.is_dirty() {
            return HashSet::new();
        }
        self.entries
            .iter()
            .filter(|entry| {
                self.overlay.get(&entry.slot).is_some() || self.overlay.raw(&entry.summary.locator).is_some()
            })
            .map(|entry| entry.summary.locator.as_str())
            .collect()
    }

    /// The index of the archive's contained objects, built on first use.
    ///
    /// The one line that adapts this file to the accessor of `contents.rs`:
    /// `patch-a.md` writes `contained()` infallible; the brief of the lot wrote
    /// `Result<&ContainedIndex>`. With the second, this is `self.contained()`.
    fn contained_index(&self) -> Result<&super::contents::ContainedIndex> {
        Ok(self.contained())
    }

    /// How many parts the index has read so far: zero until a question reaches a
    /// contained object, and zero for a package with no container type. What
    /// the cost rule CONT-10 counts (`resources()` and `headers()` of a part
    /// must leave it at zero).
    #[doc(hidden)]
    pub fn scans(&self) -> u64 {
        self.contained
            .get()
            .map_or(0, |index| index.scanned_parts().len() as u64)
    }

    /// Whether this build can know containment: some registered type has a
    /// relations table. What `Capabilities::contained_objects` says.
    pub(super) fn knows_containment(&self) -> bool {
        self.registry.relations().next().is_some()
    }

    /// The entries of one uuid for [`EpcStore::lookup`]: [`EpcStore::live_of`]
    /// (the parts), plus the objects held by value **when the parts do not
    /// answer**.
    ///
    /// A part of its own right wins and the index is not even asked. The index
    /// is asked only when no part matches and the type may be contained (a
    /// bare uuid names no type, so it always may). A contained object found
    /// twice (two containers, two parts) is one entry, the first. One whose
    /// `objectVersion` cannot be borrowed from its uri is not served and a
    /// diagnostic says why.
    ///
    /// `TODO(verify)`: a bare uuid found among the parts is not checked against
    /// the contained objects (that would build the index for every lookup); an
    /// uuid shared by a part and a contained object of another type is therefore
    /// not reported `Ambiguous`.
    pub(super) fn live_for(&self, uuid: &str, qualified_type: Option<&str>) -> Result<Vec<Live<'_>>> {
        let mut live = self.live_of(uuid);
        let part_answers = match qualified_type {
            Some(wanted) => live.iter().any(|one| one.summary.qualified_type == wanted),
            None => !live.is_empty(),
        };
        if part_answers {
            return Ok(live);
        }
        let may_be_contained = match qualified_type {
            Some(wanted) => self.registry.may_be_contained(wanted),
            None => true,
        };
        if !may_be_contained {
            return Ok(live);
        }
        let index = self.contained_index()?;
        let masked = self.masked_parts();
        let mut seen: HashSet<&str> = HashSet::new();
        for entry in index.by_uuid(uuid) {
            if masked.contains(entry.part.as_str()) || !seen.insert(entry.summary.uri.as_str()) {
                continue;
            }
            let Some(version) = object_version_in(&entry.summary.uri) else {
                self.record(Diagnostic::warning(
                    &entry.part,
                    format!(
                        "contained object `{}` is not addressable: its objectVersion cannot be read back from its uri",
                        entry.summary.uri
                    ),
                ));
                continue;
            };
            live.push(Live {
                // Not a slot of the overlay (those are `uuid` or `uuid\u{1f}...`):
                // the uri, unique and never equal to one.
                slot: &entry.summary.uri,
                summary: &entry.summary,
                version,
                archive: None,
                nested: Some(entry),
            });
        }
        Ok(live)
    }

    /// The contained object a uri names, `None` when it names a part. The
    /// resolution is [`EpcStore::locate`], the one door.
    pub(super) fn contained_entry(&self, uri: &str) -> Result<Option<&ContainedEntry>> {
        Ok(self.locate(uri)?.nested)
    }

    /// Answer a plural request: a part goes the way it always went (`single`),
    /// the contained objects are grouped by container and each container is
    /// handled once (`group`). One result per uri, in order.
    pub(super) fn routed<T>(
        &self,
        uris: &[String],
        single: impl Fn(&str) -> Result<T>,
        group: impl Fn(&[&ContainedEntry]) -> Vec<Result<T>>,
    ) -> Vec<Result<T>> {
        let mut out: Vec<Option<Result<T>>> = uris.iter().map(|_| None).collect();
        let mut wanted: Vec<(usize, &ContainedEntry)> = Vec::new();
        for (at, uri) in uris.iter().enumerate() {
            match self.contained_entry(uri) {
                Ok(Some(entry)) => wanted.push((at, entry)),
                // A part, or a refusal the part's own path will say again.
                _ => out[at] = Some(single(uri)),
            }
        }
        for (_, members) in group_by_part(&wanted) {
            let entries: Vec<&ContainedEntry> = members.iter().map(|(_, entry)| *entry).collect();
            for ((at, _), result) in members.iter().zip(group(&entries)) {
                out[*at] = Some(result);
            }
        }
        out.into_iter()
            .zip(uris)
            .map(|(result, uri)| result.unwrap_or_else(|| Err(Error::NotFound(uri.clone()))))
            .collect()
    }

    /// The registered type of a contained object.
    ///
    /// `TODO(verify)`: a development drop that shares a qualified type with its
    /// release (`resqml22` and `resqml22_dev3`) resolves to the release, which is
    /// right for a contained object of a release container and wrong for one of a
    /// drop container; the container's own `TypeInfo` would settle it.
    fn content_info(&self, entry: &ContainedEntry) -> Result<&'static TypeInfo> {
        self.registry
            .by_qualified_type(&entry.summary.qualified_type)
            .map(|found| found.info)
            .ok_or_else(|| Error::UnknownType(entry.summary.qualified_type.clone()))
    }

    /// The standalone document of one contained object, its notes recorded.
    fn extract_one(
        &self,
        bytes: &[u8],
        entry: &ContainedEntry,
        scope: &Scope,
    ) -> Result<(&'static TypeInfo, Vec<u8>)> {
        let info = self.content_info(entry)?;
        let extracted = extract(bytes, entry, &Target::of(info), scope)?;
        for note in &extracted.notes {
            self.record(Diagnostic::warning(
                &entry.part,
                format!("contained object `{}`: {note}", entry.summary.uri),
            ));
        }
        Ok((info, extracted.bytes))
    }

    /// The objects held by value in **one** part, read with the reader parts go
    /// through, in the same passes.
    pub(super) fn contained_objects(&self, entries: &[&ContainedEntry]) -> Vec<Result<Box<dyn DynObject>>> {
        let Some(first) = entries.first() else {
            return Vec::new();
        };
        per_part(
            entries,
            || self.raw_part(&first.part),
            |bytes, entry, scope| {
                let (info, document) = self.extract_one(bytes, entry, scope)?;
                self.read_extracted(info, &document, entry)
            },
        )
    }

    /// Read a standalone document as `info`: strict first, then the caller's
    /// mode, exactly as [`EpcStore::get_object`] does for a part, with the same
    /// departures recorded.
    fn read_extracted(
        &self,
        info: &'static TypeInfo,
        document: &[u8],
        entry: &ContainedEntry,
    ) -> Result<Box<dyn DynObject>> {
        let mut first: Option<Error> = None;
        for mode in super::read_passes(self.mode()) {
            match self.registry.read_xml_as_named(info, document, *mode) {
                Ok((object, departures)) => {
                    for item in departures.defaulted {
                        self.record(Diagnostic::warning(
                            &entry.part,
                            format!("contained object `{}`: {item}", entry.summary.uri),
                        ));
                    }
                    for item in departures.repaired {
                        self.record(Diagnostic::warning(
                            &entry.part,
                            format!("contained object `{}`: {item}", entry.summary.uri),
                        ));
                    }
                    return Ok(object);
                }
                Err(error) => {
                    if first.is_none() {
                        first = Some(error.into());
                    }
                }
            }
        }
        Err(first.unwrap_or_else(|| Error::UnknownType(entry.summary.content_type.clone())))
    }

    /// The citations of the objects held by value in **one** part: the citation
    /// of the **contained** object, never the container's.
    ///
    /// One object: the part is streamed up to its element and the parser stops
    /// at the end of the citation, as it does for a part. Several: the part is
    /// inflated once and each span is peeked.
    pub(super) fn contained_headers(&self, entries: &[&ContainedEntry]) -> Vec<Result<ObjectHeader>> {
        let Some(first) = entries.first() else {
            return Vec::new();
        };
        if let [only] = entries {
            return vec![self.streamed_header(only)];
        }
        match self.raw_part(&first.part) {
            Ok(bytes) => entries
                .iter()
                .map(|entry| match bytes.get(entry.span.clone()) {
                    Some(slice) => Ok(header_from_peek(&entry.summary, header::peek_reader(slice))),
                    None => Err(fail(entry, "its span lies outside its part")),
                })
                .collect(),
            Err(error) => {
                let why = error.to_string();
                entries
                    .iter()
                    .map(|entry| Err(fail(entry, format!("its part cannot be read: {why}"))))
                    .collect()
            }
        }
    }

    /// One header by streaming: skip to the element, peek, stop at the end of
    /// the citation.
    fn streamed_header(&self, entry: &ContainedEntry) -> Result<ObjectHeader> {
        let skip = entry.span.start as u64;
        let peeked = self.package.with_part(&entry.part, |source| {
            let skipped = std::io::copy(&mut (&mut *source).take(skip), &mut std::io::sink());
            match skipped {
                Ok(count) if count == skip => Ok(header::peek_reader(BufReader::new(source))),
                Ok(_) => Err(fail(entry, "its span lies outside its part")),
                Err(error) => Err(Error::Io(error)),
            }
        })??;
        Ok(header_from_peek(&entry.summary, peeked))
    }

    /// The bytes of the objects held by value in **one** part, each as a
    /// standalone document (what [`EpcStore::get_objects`] reads).
    pub(super) fn contained_raw(&self, entries: &[&ContainedEntry]) -> Vec<Result<RawObject>> {
        let Some(first) = entries.first() else {
            return Vec::new();
        };
        per_part(
            entries,
            || self.raw_part(&first.part),
            |bytes, entry, scope| {
                let (_, document) = self.extract_one(bytes, entry, scope)?;
                Ok(RawObject::new(
                    entry.summary.uri.clone(),
                    RawFormat::Xml,
                    document,
                ))
            },
        )
    }

    // -- The contents of the package ----------------------------------------

    /// The objects the parts hold by value that `resources()` does not list,
    /// and the edges from each to the object that directly holds it (ETP 1.2
    /// section 8.1.1.1.1, p. 156: the contained object is the source).
    ///
    /// A package whose parts have no container type reads nothing here. A
    /// contained object that is also a part of its own right is not listed
    /// again (the part wins: `nested_index`) but keeps its edges. One held by
    /// two containers is one object with two edges. The kind of an edge is the
    /// container's ML's to say ([`KindRules::containment`]), each edge marked
    /// `by_value`.
    ///
    /// `TODO(verify)`: a container created or modified in this session is not in
    /// the archive's index, so the objects it holds are not listed until it is
    /// saved and reopened.
    pub fn contents(&self) -> Result<Contents> {
        let parts = EpcStore::resources(self);
        if !parts
            .iter()
            .any(|part| self.registry.is_container(&part.qualified_type))
        {
            return Ok(Contents::default());
        }
        let index = self.contained_index()?;
        let masked = self.masked_parts();
        let held: HashSet<&str> = parts.iter().map(|part| part.uri.as_str()).collect();
        let rules = StandardRules::over(self.registry.clone());
        let mut seen_objects: HashSet<&str> = HashSet::new();
        let mut seen_edges: HashSet<(&str, &str)> = HashSet::new();
        let mut contents = Contents::default();
        // The objects of the index, in archive order, then those a part of the
        // archive shadows (the part is the object: they are not listed again,
        // only their edge to the container is stated).
        let listed = index.in_archive_order().map(|entry| (entry, true));
        let shadowed = index.shadowed().iter().map(|entry| (entry, false));
        for (entry, own) in listed.chain(shadowed) {
            if masked.contains(entry.part.as_str()) {
                continue;
            }
            let uri = entry.summary.uri.as_str();
            if own && seen_objects.insert(uri) && !held.contains(uri) {
                contents.objects.push(entry.summary.clone());
            }
            for container in &entry.summary.contained_in {
                if !seen_edges.insert((uri, container.as_str())) {
                    continue;
                }
                let judged = match crate::kinds::qualified_type_of(container) {
                    Some(qualified_type) => rules.containment(&qualified_type),
                    None => Judged::by_default(
                        "the container of the edge is not an object URI, so no ML decides: primary by default (maintainer decision of 23/10/2026)",
                    ),
                };
                let mut edge = Edge::new(uri, container.as_str(), "").with_judgement(judged);
                edge.by_value = true;
                contents.edges.push(edge);
            }
        }
        Ok(contents)
    }

    // -- What an EPC refuses, by name ---------------------------------------

    /// Whether the object is held as a part of its own right: the same uuid, the
    /// same type, in the inventory or created in this session.
    fn held_as_part(&self, summary: &ObjectSummary) -> bool {
        self.live_of(&summary.uuid.to_ascii_lowercase())
            .iter()
            .any(|one| one.summary.qualified_type == summary.qualified_type)
    }

    /// Refuse to put an object that exists **only by value** in a container: as
    /// a part it would shadow the copy the container holds, as an update it
    /// would rewrite the container, and which one is meant is not decided
    /// (design note `genre-des-aretes-et-objets-contenus.md` section 4.9,
    /// D1b-9). An object that is also a part is an ordinary upsert.
    pub(super) fn refuse_contained_put(&self, summary: &ObjectSummary) -> Result<()> {
        if !self.registry.may_be_contained(&summary.qualified_type) || self.held_as_part(summary) {
            return Ok(());
        }
        let index = self.contained_index()?;
        let uuid = summary.uuid.to_ascii_lowercase();
        if index
            .by_uuid(&uuid)
            .iter()
            .any(|entry| entry.summary.qualified_type == summary.qualified_type)
        {
            return Err(Error::Unsupported {
                operation: "writing an object that is held by value inside a container (as a part it would shadow that copy, as an update it would rewrite the container)",
                store: STORE,
            });
        }
        Ok(())
    }

    /// Refuse to remove what an EPC cannot remove without losing more than was
    /// asked (D1b-8, D1b-9):
    ///
    /// - an object held by value: it would have to be unjoined, and the
    ///   container's part rewritten;
    /// - a container with objects that live **only inside it**: they would
    ///   disappear with it, where an ETP store keeps them (ETP 1.2 section 9.2.3
    ///   row 26, p. 192). Pruning is the explicit way (`prune_contained`), and
    ///   an EPC does not offer it yet.
    ///
    /// A uri that resolves to nothing is left to `remove` to refuse as it always
    /// did. `TODO(verify)`: a container modified in this session is judged by
    /// the contents it had in the archive.
    pub(super) fn refuse_contained_remove(&self, uri: &str) -> Result<()> {
        let Ok(live) = self.locate(uri) else {
            return Ok(());
        };
        if live.nested.is_some() {
            return Err(Error::Unsupported {
                operation: "deleting an object held by value inside a container (it would have to be unjoined and the container rewritten)",
                store: STORE,
            });
        }
        if !self.registry.is_container(&live.summary.qualified_type) {
            return Ok(());
        }
        let index = self.contained_index()?;
        let part = live.summary.locator.as_str();
        let only_inside = index
            .entries()
            .iter()
            .any(|entry| entry.part == part && !self.held_as_part(&entry.summary));
        if only_inside {
            return Err(Error::Unsupported {
                operation: "deleting a container whose contained objects live only inside it",
                store: STORE,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! Written from the WITSML 2.1 schemas (`witsml/v2.1/xsd_schemas/`):
    //! `Log` (`Log.xsd:690`, element `ChannelSet` at `:822`), `ChannelSet`
    //! (`Log.xsd:531`, element `Channel` at `:650`), `Channel` (global element
    //! at `Log.xsd:209`), `PPFGChannel` (global element at `PPFG.xsd:402`),
    //! `WellboreMarkerSet` (`WellboreMarkers.xsd:170`, element `FormationMarker`
    //! of type `WellboreMarker` at `:186`, global element `WellboreMarker` at
    //! `:74`). Every object extends `AbstractActiveObject`
    //! (`common/v2.3/xsd_schemas/Abstract.xsd:165`) and so carries `Citation`
    //! and `ActiveStatus` (eml), and the attributes `uuid` and `schemaVersion`
    //! (`Abstract.xsd:300`, `:307`, both `use="required"`).
    //!
    //! No file is read: the packages are built here. Each test says how to
    //! break it.

    use std::io::{Cursor, Write};
    use std::sync::Arc;

    use energyml_core::identity;
    use energyml_core::registry::{ReadMode, Registry};

    use super::*;
    use crate::EdgeKind;
    use crate::kinds::KindBasis;

    const WITSML_NS: &str = "http://www.energistics.org/energyml/data/witsmlv2";
    const EML_NS: &str = "http://www.energistics.org/energyml/data/commonv2";
    const XSI_NS: &str = "http://www.w3.org/2001/XMLSchema-instance";

    const LOG: &str = "5a1c0000-0000-4000-8000-000000000001";
    const SET: &str = "5a1c0000-0000-4000-8000-000000000002";
    const GAMMA: &str = "5a1c0000-0000-4000-8000-000000000003";
    const DENSITY: &str = "5a1c0000-0000-4000-8000-000000000004";
    const UNKNOWN: &str = "5a1c0000-0000-4000-8000-0000000000ff";

    fn registry() -> Arc<Registry> {
        Arc::new(energyml_schemas::all())
    }

    /// The `Citation.Title` of an object read back.
    fn title_of(object: &dyn DynObject) -> Option<String> {
        identity::title_of_reflect(object.as_reflect()).map(str::to_string)
    }

    fn citation(title: &str) -> String {
        format!(
            "<eml:Citation><eml:Title>{title}</eml:Title><eml:Originator>test</eml:Originator>\
<eml:Creation>2026-10-15T00:00:00Z</eml:Creation><eml:Format>test</eml:Format></eml:Citation>\
<eml:ActiveStatus>inactive</eml:ActiveStatus>"
        )
    }

    fn channel(uuid: &str, title: &str, mnemonic: &str) -> String {
        format!(
            "<witsml:Channel uuid=\"{uuid}\" schemaVersion=\"2.1\">{}\
<witsml:Mnemonic>{mnemonic}</witsml:Mnemonic><witsml:GlobalMnemonic>{mnemonic}</witsml:GlobalMnemonic>\
<witsml:DataKind>double</witsml:DataKind><witsml:Uom>m</witsml:Uom></witsml:Channel>",
            citation(title)
        )
    }

    /// A `Log` holding one `ChannelSet` holding two `Channel`s, the namespaces
    /// declared on the root only.
    fn log_part() -> String {
        format!(
            "<witsml:Log xmlns:witsml=\"{WITSML_NS}\" xmlns:eml=\"{EML_NS}\" xmlns:xsi=\"{XSI_NS}\" \
uuid=\"{LOG}\" schemaVersion=\"2.1\">{}\
<witsml:ChannelSet uuid=\"{SET}\" schemaVersion=\"2.1\">{}{}{}</witsml:ChannelSet></witsml:Log>",
            citation("Log 1"),
            citation("Set 1"),
            channel(GAMMA, "Gamma Ray", "GR"),
            channel(DENSITY, "Bulk Density", "RHOB"),
        )
    }

    /// The span of the first `open ... close` in `xml`.
    fn span_of(xml: &str, open: &str, close: &str) -> std::ops::Range<usize> {
        let start = xml.find(open).expect("the opening text is in the fixture");
        let end = xml[start..]
            .find(close)
            .map(|at| start + at + close.len())
            .expect("the closing text is in the fixture");
        start..end
    }

    fn entry_in(
        part: &str,
        span: std::ops::Range<usize>,
        qualified_type: &str,
        uuid: &str,
        element: &str,
    ) -> ContainedEntry {
        ContainedEntry {
            summary: ObjectSummary::new(format!("eml:///{qualified_type}({uuid})"), uuid, qualified_type)
                .with_locator(part),
            title: None,
            part: part.to_string(),
            span,
            element: element.to_string(),
            path: element.to_string(),
        }
    }

    fn target(registry: &Registry, qualified_type: &str) -> Target {
        Target::of(
            registry
                .by_qualified_type(qualified_type)
                .expect("the type is registered")
                .info,
        )
    }

    fn extract_one(xml: &str, entry: &ContainedEntry, target: &Target) -> Extracted {
        let scope = scopes_at(xml.as_bytes(), &[entry])
            .pop()
            .flatten()
            .expect("the span starts at an element");
        extract(xml.as_bytes(), entry, target, &scope).expect("the extraction succeeds")
    }

    /// The prefixes used by an element name but not declared on the root tag.
    fn undeclared_prefixes(xml: &str) -> Vec<String> {
        let root_end = xml.find('>').expect("a root tag");
        let root = &xml[..root_end];
        let mut missing = Vec::new();
        let mut rest = xml;
        while let Some(at) = rest.find('<') {
            rest = &rest[at + 1..];
            let named = rest.strip_prefix('/').unwrap_or(rest);
            let name: String = named
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != '>' && *c != '/')
                .collect();
            if let Some((prefix, _)) = name.split_once(':')
                && !root.contains(&format!("xmlns:{prefix}=\""))
            {
                missing.push(prefix.to_string());
            }
        }
        missing
    }

    // -- the extraction -----------------------------------------------------

    /// A `Channel` two levels down is cut out and read by the reader a part goes
    /// through; the object that comes back is the channel, with its uuid and its
    /// title, not the log.
    ///
    /// To break it: make `extract` keep the container's root (cut `0..len`
    /// instead of `entry.span`): the uuid becomes the log's.
    #[test]
    fn a_deep_channel_is_extracted_and_read_by_the_ordinary_reader() {
        let registry = registry();
        let xml = log_part();
        let span = span_of(
            &xml,
            &format!("<witsml:Channel uuid=\"{DENSITY}\""),
            "</witsml:Channel>",
        );
        let entry = entry_in("log.xml", span, "witsml21.Channel", DENSITY, "witsml:Channel");
        let target = target(&registry, "witsml21.Channel");
        let extracted = extract_one(&xml, &entry, &target);
        assert!(extracted.notes.is_empty(), "{:?}", extracted.notes);

        let info = registry
            .by_qualified_type("witsml21.Channel")
            .expect("registered")
            .info;
        let (object, _) = registry
            .read_xml_as_named(info, &extracted.bytes, ReadMode::Tolerant)
            .expect("the standalone document reads");
        assert_eq!(identity::uuid_of_reflect(object.as_reflect()), Some(DENSITY));
        assert_eq!(title_of(object.as_ref()).as_deref(), Some("Bulk Density"));
    }

    /// `eml` is declared only on the root of the log; the channel is two levels
    /// down and writes `eml:Citation`. The extract must carry the declaration or
    /// it is not a namespace-correct document.
    ///
    /// To break it: stop carrying (`carried` empty): `eml` is reported missing.
    #[test]
    fn a_prefix_declared_only_at_the_root_is_carried_to_the_extract() {
        let registry = registry();
        let xml = log_part();
        let span = span_of(
            &xml,
            &format!("<witsml:Channel uuid=\"{GAMMA}\""),
            "</witsml:Channel>",
        );
        let entry = entry_in("log.xml", span, "witsml21.Channel", GAMMA, "witsml:Channel");
        let extracted = extract_one(&xml, &entry, &target(&registry, "witsml21.Channel"));
        let text = String::from_utf8(extracted.bytes).expect("UTF-8");
        assert!(text.contains(&format!("xmlns:eml=\"{EML_NS}\"")), "{text}");
        assert!(text.contains(&format!("xmlns:witsml=\"{WITSML_NS}\"")), "{text}");
        assert_eq!(undeclared_prefixes(&text), Vec::<String>::new(), "{text}");
        // The root tag only: the declarations are not repeated down the tree.
        assert_eq!(text.matches("xmlns:eml=").count(), 1, "{text}");
    }

    /// A document in the default namespace: the extract's root stays unprefixed,
    /// and the default namespace is carried.
    ///
    /// To break it: always write the target's own prefix (`witsml:Channel`): the
    /// root is then in a namespace the extract does not bind to that prefix.
    #[test]
    fn a_default_namespace_is_carried_and_the_root_stays_unprefixed() {
        let registry = registry();
        let xml = format!(
            "<Log xmlns=\"{WITSML_NS}\" xmlns:eml=\"{EML_NS}\" uuid=\"{LOG}\" schemaVersion=\"2.1\">\
<ChannelSet uuid=\"{SET}\" schemaVersion=\"2.1\">\
<Channel uuid=\"{GAMMA}\" schemaVersion=\"2.1\">{}<Mnemonic>GR</Mnemonic></Channel>\
</ChannelSet></Log>",
            citation("Gamma Ray")
        );
        let span = span_of(&xml, "<Channel ", "</Channel>");
        let entry = entry_in("log.xml", span, "witsml21.Channel", GAMMA, "Channel");
        let extracted = extract_one(&xml, &entry, &target(&registry, "witsml21.Channel"));
        let text = String::from_utf8(extracted.bytes).expect("UTF-8");
        assert!(
            text.starts_with(&format!("<Channel xmlns=\"{WITSML_NS}\"")),
            "{text}"
        );
        assert!(text.ends_with("</Channel>"), "{text}");
        assert!(text.contains(&format!("xmlns:eml=\"{EML_NS}\"")), "{text}");
    }

    /// A declaration made by an intermediate ancestor (the `ChannelSet`) is in
    /// the scope of its channels and not of the others.
    ///
    /// To break it: take the scope from the root alone: the first assertion fails.
    #[test]
    fn a_declaration_made_by_an_intermediate_ancestor_is_in_scope() {
        let xml = format!(
            "<witsml:Log xmlns:witsml=\"{WITSML_NS}\" xmlns:eml=\"{EML_NS}\" uuid=\"{LOG}\" schemaVersion=\"2.1\">\
<witsml:ChannelSet xmlns:ext=\"urn:example:ext\" uuid=\"{SET}\" schemaVersion=\"2.1\">\
<witsml:Channel uuid=\"{GAMMA}\" schemaVersion=\"2.1\"/></witsml:ChannelSet>\
<witsml:ChannelSet uuid=\"{DENSITY}\" schemaVersion=\"2.1\">\
<witsml:Channel uuid=\"{UNKNOWN}\" schemaVersion=\"2.1\"/></witsml:ChannelSet></witsml:Log>"
        );
        let inside = entry_in(
            "log.xml",
            span_of(&xml, &format!("<witsml:Channel uuid=\"{GAMMA}\""), "/>"),
            "witsml21.Channel",
            GAMMA,
            "witsml:Channel",
        );
        let outside = entry_in(
            "log.xml",
            span_of(&xml, &format!("<witsml:Channel uuid=\"{UNKNOWN}\""), "/>"),
            "witsml21.Channel",
            UNKNOWN,
            "witsml:Channel",
        );
        // Asked out of document order on purpose: the pass sorts them.
        let scopes = scopes_at(xml.as_bytes(), &[&outside, &inside]);
        let outer = scopes[0].as_ref().expect("a scope");
        let inner = scopes[1].as_ref().expect("a scope");
        assert!(
            inner
                .declarations
                .contains(&("ext".to_string(), "urn:example:ext".to_string()))
        );
        assert!(!outer.declarations.iter().any(|(prefix, _)| prefix == "ext"));
        assert_eq!(inner.schema_version.as_deref(), Some("2.1"));
        assert!(
            inner
                .declarations
                .contains(&("eml".to_string(), EML_NS.to_string()))
        );
    }

    /// A subtype written under the element of its slot (`xsi:type` on a
    /// `Channel` element) is extracted under **its own** global element and
    /// without the `xsi:type`; a `WellboreMarker` held under the element
    /// `FormationMarker` is renamed to `WellboreMarker`. Both read as their type.
    ///
    /// To break it: keep the original element name: the root assertions fail.
    #[test]
    fn a_subtype_and_a_renamed_slot_are_written_under_their_own_global_element() {
        let registry = registry();

        let xml = format!(
            "<witsml:ChannelSet xmlns:witsml=\"{WITSML_NS}\" xmlns:eml=\"{EML_NS}\" xmlns:xsi=\"{XSI_NS}\" \
uuid=\"{SET}\" schemaVersion=\"2.1\">\
<witsml:Channel xsi:type=\"witsml:PPFGChannel\" uuid=\"{GAMMA}\" schemaVersion=\"2.1\">{}</witsml:Channel>\
</witsml:ChannelSet>",
            citation("Pore pressure")
        );
        let span = span_of(&xml, "<witsml:Channel ", "</witsml:Channel>");
        let entry = entry_in("set.xml", span, "witsml21.PPFGChannel", GAMMA, "witsml:Channel");
        let extracted = extract_one(&xml, &entry, &target(&registry, "witsml21.PPFGChannel"));
        let text = String::from_utf8(extracted.bytes.clone()).expect("UTF-8");
        assert!(text.starts_with("<witsml:PPFGChannel "), "{text}");
        assert!(text.ends_with("</witsml:PPFGChannel>"), "{text}");
        assert!(!text.contains("xsi:type"), "{text}");
        let info = registry
            .by_qualified_type("witsml21.PPFGChannel")
            .expect("registered")
            .info;
        let (object, _) = registry
            .read_xml_as_named(info, &extracted.bytes, ReadMode::Tolerant)
            .expect("reads");
        assert_eq!(object.type_info().name, "PPFGChannel");

        let xml = format!(
            "<witsml:WellboreMarkerSet xmlns:witsml=\"{WITSML_NS}\" xmlns:eml=\"{EML_NS}\" \
uuid=\"{SET}\" schemaVersion=\"2.1\">\
<witsml:FormationMarker uuid=\"{DENSITY}\" schemaVersion=\"2.1\">{}</witsml:FormationMarker>\
</witsml:WellboreMarkerSet>",
            citation("Top Chalk")
        );
        let span = span_of(&xml, "<witsml:FormationMarker ", "</witsml:FormationMarker>");
        let entry = entry_in(
            "markers.xml",
            span,
            "witsml21.WellboreMarker",
            DENSITY,
            "witsml:FormationMarker",
        );
        let extracted = extract_one(&xml, &entry, &target(&registry, "witsml21.WellboreMarker"));
        let text = String::from_utf8(extracted.bytes.clone()).expect("UTF-8");
        assert!(text.starts_with("<witsml:WellboreMarker "), "{text}");
        assert!(text.ends_with("</witsml:WellboreMarker>"), "{text}");
        let info = registry
            .by_qualified_type("witsml21.WellboreMarker")
            .expect("registered")
            .info;
        let (object, _) = registry
            .read_xml_as_named(info, &extracted.bytes, ReadMode::Tolerant)
            .expect("reads");
        assert_eq!(title_of(object.as_ref()).as_deref(), Some("Top Chalk"));
    }

    /// A type with no global element of its own stays identified by `xsi:type`
    /// on the root, which is added when the slot did not write one (PRODML 2.3
    /// `InjectionFlowTest` under `FlowTestActivity`, `TypeInfo::root_element`).
    /// The target is written by hand: the point is the rule, not a registry.
    ///
    /// To break it: set `keep_xsi_type` to false: the type is no longer named.
    #[test]
    fn a_type_without_an_element_of_its_own_keeps_its_xsi_type() {
        let target = Target {
            element: "FlowTestActivity",
            namespace: Namespace::new("prodml", "http://www.energistics.org/energyml/data/prodmlv2"),
            type_name: "InjectionFlowTest",
            keep_xsi_type: true,
            schema_version: "2.3",
        };
        let xml = format!(
            "<prodml:Holder xmlns:prodml=\"http://www.energistics.org/energyml/data/prodmlv2\" schemaVersion=\"2.3\">\
<prodml:Test uuid=\"{GAMMA}\"/></prodml:Holder>"
        );
        let entry = entry_in(
            "holder.xml",
            span_of(&xml, "<prodml:Test ", "/>"),
            "prodml23.InjectionFlowTest",
            GAMMA,
            "prodml:Test",
        );
        let extracted = extract_one(&xml, &entry, &target);
        let text = String::from_utf8(extracted.bytes).expect("UTF-8");
        assert!(text.starts_with("<prodml:FlowTestActivity "), "{text}");
        assert!(text.contains(&format!("xmlns:xsi=\"{XSI_NS}\"")), "{text}");
        assert!(text.contains("xsi:type=\"prodml:InjectionFlowTest\""), "{text}");
        // The holder's schemaVersion was supplied, and said.
        assert!(text.contains("schemaVersion=\"2.3\""), "{text}");
        assert_eq!(extracted.notes.len(), 1);
        assert!(
            extracted.notes[0].contains("its container's"),
            "{:?}",
            extracted.notes
        );
    }

    /// Without a `schemaVersion` on the element and none on the container's
    /// root, the type's own release is supplied - and said.
    ///
    /// To break it: drop the `notes.push`: the second assertion fails.
    #[test]
    fn a_missing_schema_version_is_supplied_and_said() {
        let registry = registry();
        let xml = format!(
            "<witsml:ChannelSet xmlns:witsml=\"{WITSML_NS}\" xmlns:eml=\"{EML_NS}\" uuid=\"{SET}\">\
<witsml:Channel uuid=\"{GAMMA}\">{}</witsml:Channel></witsml:ChannelSet>",
            citation("Gamma Ray")
        );
        let entry = entry_in(
            "set.xml",
            span_of(&xml, "<witsml:Channel ", "</witsml:Channel>"),
            "witsml21.Channel",
            GAMMA,
            "witsml:Channel",
        );
        let extracted = extract_one(&xml, &entry, &target(&registry, "witsml21.Channel"));
        let text = String::from_utf8(extracted.bytes).expect("UTF-8");
        assert!(text.contains("schemaVersion=\"2.1\""), "{text}");
        assert_eq!(extracted.notes.len(), 1, "{:?}", extracted.notes);
        assert!(extracted.notes[0].contains("its type's"), "{:?}", extracted.notes);
    }

    /// A span that is not exactly one element is refused, never repaired: one
    /// that starts in the middle of a tag, one whose end tag is another's, one
    /// that lies outside the part.
    ///
    /// To break it: skip the end-tag comparison: the second case is accepted.
    #[test]
    fn a_malformed_span_is_refused() {
        let registry = registry();
        let xml = log_part();
        let target = target(&registry, "witsml21.Channel");
        let scope = Scope::default();

        // Starts one byte into a tag.
        let from = xml.find("<witsml:Channel ").expect("a channel") + 1;
        let torn = entry_in(
            "log.xml",
            from..from + 20,
            "witsml21.Channel",
            GAMMA,
            "witsml:Channel",
        );
        assert!(matches!(
            extract(xml.as_bytes(), &torn, &target, &scope),
            Err(Error::Xml(_))
        ));
        assert_eq!(scopes_at(xml.as_bytes(), &[&torn]), vec![None]);

        // Ends on the wrong end tag: the channel's start, the set's end.
        let start = xml.find("<witsml:Channel ").expect("a channel");
        let end = xml.find("</witsml:ChannelSet>").expect("a set end") + "</witsml:ChannelSet>".len();
        let crossed = entry_in("log.xml", start..end, "witsml21.Channel", GAMMA, "witsml:Channel");
        assert!(matches!(
            extract(xml.as_bytes(), &crossed, &target, &scope),
            Err(Error::Xml(_))
        ));

        // Outside the part.
        let outside = entry_in(
            "log.xml",
            xml.len()..xml.len() + 10,
            "witsml21.Channel",
            GAMMA,
            "witsml:Channel",
        );
        assert!(matches!(
            extract(xml.as_bytes(), &outside, &target, &scope),
            Err(Error::Xml(_))
        ));
    }

    /// A uri carries the `objectVersion` as `version='...'`; a version holding a
    /// quote writes it doubled and cannot be borrowed.
    ///
    /// To break it: return `Some(Some(..))` for the doubled quote.
    #[test]
    fn the_object_version_is_borrowed_from_the_uri_or_not_at_all() {
        let plain = format!("eml:///witsml21.Channel({GAMMA})");
        assert_eq!(object_version_in(&plain), Some(None));
        let versioned = format!("eml:///witsml21.Channel(uuid={GAMMA},version='v1')");
        assert_eq!(object_version_in(&versioned), Some(Some("v1")));
        let quoted = format!("eml:///witsml21.Channel(uuid={GAMMA},version='O''Neil')");
        assert_eq!(object_version_in(&quoted), None);
        assert_eq!(object_version_in("not a uri"), None);
    }

    // -- grouping -----------------------------------------------------------

    /// Two contained objects of one part, asked together, load the part once; a
    /// third from another part loads that one. One result each, in order.
    ///
    /// To break it: load inside `each`: the counter reads 3.
    #[test]
    fn two_contents_of_one_part_are_read_in_one_load() {
        let xml = log_part();
        let one = entry_in(
            "log.xml",
            span_of(
                &xml,
                &format!("<witsml:Channel uuid=\"{GAMMA}\""),
                "</witsml:Channel>",
            ),
            "witsml21.Channel",
            GAMMA,
            "witsml:Channel",
        );
        let two = entry_in(
            "log.xml",
            span_of(
                &xml,
                &format!("<witsml:Channel uuid=\"{DENSITY}\""),
                "</witsml:Channel>",
            ),
            "witsml21.Channel",
            DENSITY,
            "witsml:Channel",
        );
        let elsewhere = entry_in("other.xml", 0..1, "witsml21.Channel", UNKNOWN, "witsml:Channel");

        let wanted = [(0, &one), (1, &elsewhere), (2, &two)];
        let groups = group_by_part(&wanted);
        assert_eq!(groups.len(), 2);
        assert_eq!(
            groups["log.xml"].iter().map(|(at, _)| *at).collect::<Vec<_>>(),
            vec![0, 2]
        );

        let loads = std::cell::Cell::new(0u32);
        let results = per_part(
            &[&one, &two],
            || {
                loads.set(loads.get() + 1);
                Ok(xml.clone().into_bytes())
            },
            |bytes, entry, scope| Ok(entry.span.len() + bytes.len() + scope.declarations.len()),
        );
        assert_eq!(loads.get(), 1);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(Result::is_ok));
    }

    /// A part that cannot be loaded fails its own contents, each one named, and
    /// does not touch another part's.
    ///
    /// To break it: return one error for the whole request.
    #[test]
    fn a_part_that_cannot_be_loaded_fails_its_own_contents_only() {
        let xml = log_part();
        let entry = entry_in("gone.xml", 0..10, "witsml21.Channel", GAMMA, "witsml:Channel");
        let results: Vec<Result<()>> = per_part(
            &[&entry, &entry],
            || Err(Error::Package("no part `gone.xml`".into())),
            |_, _, _| Ok(()),
        );
        assert_eq!(results.len(), 2);
        for result in results {
            let message = result.expect_err("the load failed").to_string();
            assert!(
                message.contains(GAMMA) && message.contains("gone.xml"),
                "{message}"
            );
        }
        // And a part that loads is unaffected.
        let fine = entry_in(
            "log.xml",
            span_of(&xml, "<witsml:ChannelSet ", "</witsml:ChannelSet>"),
            "witsml21.ChannelSet",
            SET,
            "witsml:ChannelSet",
        );
        let results = per_part(&[&fine], || Ok(xml.clone().into_bytes()), |_, _, _| Ok(()));
        assert!(results[0].is_ok());
    }

    // -- the store ----------------------------------------------------------
    //
    // These need the index (`contents.rs`) and the branches of the patch: they
    // are the end-to-end rule, CONT-2 to CONT-9 of the design note in small.

    fn package(extra: &[(&str, String, &str)]) -> Vec<u8> {
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buffer);
            let options: zip::write::SimpleFileOptions =
                zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            let mut overrides = format!(
                "<Override PartName=\"/witsml21/Log_{LOG}.xml\" \
ContentType=\"application/x-witsml+xml;version=2.1;type=Log\"/>"
            );
            for (name, _, content_type) in extra {
                overrides.push_str(&format!(
                    "<Override PartName=\"/{name}\" ContentType=\"{content_type}\"/>"
                ));
            }
            zip.start_file("[Content_Types].xml", options).expect("start");
            zip.write_all(
                format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
{overrides}</Types>"
                )
                .as_bytes(),
            )
            .expect("write");
            zip.start_file(format!("witsml21/Log_{LOG}.xml"), options)
                .expect("start");
            zip.write_all(log_part().as_bytes()).expect("write");
            for (name, body, _) in extra {
                zip.start_file(*name, options).expect("start");
                zip.write_all(body.as_bytes()).expect("write");
            }
            zip.finish().expect("finish");
        }
        buffer.into_inner()
    }

    fn store(extra: &[(&str, String, &str)]) -> EpcStore {
        EpcStore::from_reader(Box::new(Cursor::new(package(extra))), "witsml.epc", registry())
            .expect("the package opens")
    }

    fn uri_of(contents: &Contents, qualified_type: &str, uuid: &str) -> String {
        contents
            .objects
            .iter()
            .find(|object| object.uuid == uuid && object.qualified_type == qualified_type)
            .map(|object| object.uri.clone())
            .unwrap_or_else(|| panic!("{qualified_type}({uuid}) is not in the contents: {contents:?}"))
    }

    /// `resources()` lists the log only; `contents()` lists the set and the two
    /// channels, with an edge from each to the object directly holding it,
    /// primary, by value, labelled as the specification's examples (ETP 1.2
    /// section 8.1.1.1.2, p. 157: ChannelSets into Logs, Channels into
    /// ChannelSets).
    ///
    /// To break it: list the channels in `resources()`, or give `Channel ->
    /// Log` an edge (three edges become four).
    #[test]
    fn contents_lists_what_resources_does_not_and_links_it_to_its_container() {
        let store = store(&[]);
        let parts = store.resources();
        assert_eq!(parts.len(), 1, "{parts:?}");
        assert_eq!(parts[0].qualified_type, "witsml21.Log");

        let contents = store.contents().expect("contents");
        assert_eq!(contents.objects.len(), 3, "{contents:?}");
        let set = uri_of(&contents, "witsml21.ChannelSet", SET);
        let gamma = uri_of(&contents, "witsml21.Channel", GAMMA);
        let density = uri_of(&contents, "witsml21.Channel", DENSITY);
        assert_eq!(contents.edges.len(), 3, "{:?}", contents.edges);
        for (from, to) in [(&gamma, &set), (&density, &set), (&set, &parts[0].uri)] {
            let edge = contents
                .edges
                .iter()
                .find(|edge| &edge.from == from && &edge.to == to)
                .unwrap_or_else(|| panic!("no edge {from} -> {to}: {:?}", contents.edges));
            assert!(edge.by_value);
            assert_eq!(edge.kind, Some(EdgeKind::Primary));
            assert!(
                matches!(edge.basis, Some(KindBasis::EtpSpec { page: 157, .. })),
                "{edge:?}"
            );
        }
    }

    /// A contained object is reached by `lookup`, read as itself, cited as
    /// itself, and served as its own document; two of them asked together come
    /// back in order.
    ///
    /// To break it: let `header` fall through to `entry()` for a contained
    /// object (the container's citation, "Log 1" - a wrong answer that looks
    /// right) or let `get_object` read the part: the titles differ.
    #[test]
    fn a_contained_object_is_looked_up_read_cited_and_served_as_itself() {
        let store = store(&[]);
        let contents = store.contents().expect("contents");
        let gamma = uri_of(&contents, "witsml21.Channel", GAMMA);
        let density = uri_of(&contents, "witsml21.Channel", DENSITY);

        assert_eq!(
            store
                .lookup(GAMMA, Some("witsml21.Channel"), None)
                .expect("found")
                .uri,
            gamma
        );

        let object = store.get_object(&gamma).expect("reads");
        assert_eq!(identity::uuid_of_reflect(object.as_reflect()), Some(GAMMA));
        assert_eq!(title_of(object.as_ref()).as_deref(), Some("Gamma Ray"));

        let header = store.header(&gamma).expect("cites");
        assert_eq!(header.title.as_deref(), Some("Gamma Ray"));
        assert_eq!(header.summary.uri, gamma);

        let both = [density.clone(), gamma.clone()];
        let titles: Vec<_> = store
            .get_objects(&both)
            .into_iter()
            .map(|result| title_of(result.expect("reads").as_ref()))
            .collect();
        assert_eq!(
            titles,
            vec![Some("Bulk Density".to_string()), Some("Gamma Ray".to_string())]
        );
        let cited: Vec<_> = store
            .headers(&both)
            .into_iter()
            .map(|result| result.expect("cites").title)
            .collect();
        assert_eq!(
            cited,
            vec![Some("Bulk Density".to_string()), Some("Gamma Ray".to_string())]
        );

        let raw = store
            .raw_objects(std::slice::from_ref(&gamma), RawFormat::Xml)
            .pop()
            .expect("one")
            .expect("raw");
        let text = String::from_utf8(raw.bytes).expect("UTF-8");
        assert!(text.starts_with("<witsml:Channel "), "{text}");
        assert!(
            text.contains("Gamma Ray") && !text.contains("Bulk Density"),
            "{text}"
        );
    }

    /// A uri of a contained object that is nowhere is `NotFound`; a part still
    /// reads as before.
    ///
    /// To break it: answer the first container's object for any unknown channel.
    #[test]
    fn an_unknown_contained_uri_is_not_found() {
        let store = store(&[]);
        let unknown = format!("eml:///witsml21.Channel({UNKNOWN})");
        assert!(matches!(store.get_object(&unknown), Err(Error::NotFound(_))));
        assert!(matches!(store.header(&unknown), Err(Error::NotFound(_))));
        let raw = store.raw_objects(&[unknown], RawFormat::Xml).pop().expect("one");
        assert!(matches!(raw, Err(Error::NotFound(_))));
        let log = store.resources().pop().expect("the log").uri;
        assert!(store.get_object(&log).is_ok());
    }

    /// A channel that is also a part of its own right: one resource (the
    /// part's), served from the part; its edge to the set stays.
    ///
    /// To break it: let the contained copy win in `live_for`: the title read is
    /// the copy's, "Gamma Ray".
    #[test]
    fn a_contained_object_that_is_also_a_part_is_the_part() {
        let own = format!(
            "<witsml:Channel xmlns:witsml=\"{WITSML_NS}\" xmlns:eml=\"{EML_NS}\" uuid=\"{GAMMA}\" schemaVersion=\"2.1\">{}\
<witsml:Mnemonic>GR</witsml:Mnemonic></witsml:Channel>",
            citation("Gamma Ray (part)")
        );
        let store = store(&[(
            "witsml21/Channel_gamma.xml",
            own,
            "application/x-witsml+xml;version=2.1;type=Channel",
        )]);
        let contents = store.contents().expect("contents");
        assert!(
            contents.objects.iter().all(|object| object.uuid != GAMMA),
            "listed twice: {contents:?}"
        );
        assert_eq!(
            contents
                .edges
                .iter()
                .filter(|edge| edge.from.contains(GAMMA))
                .count(),
            1,
            "{:?}",
            contents.edges
        );
        let uri = store
            .resources()
            .into_iter()
            .find(|part| part.uuid == GAMMA)
            .expect("the part")
            .uri;
        let object = store.get_object(&uri).expect("reads");
        assert_eq!(title_of(object.as_ref()).as_deref(), Some("Gamma Ray (part)"));
    }

    /// The writes an EPC cannot make without guessing are refused by name:
    /// putting an object that exists only by value, deleting one, deleting the
    /// container whose objects live only inside it.
    ///
    /// To break it: remove one `refuse_*` call from `put_entry` or `remove`; the
    /// matching assertion fails (the put is accepted, or the log is gone).
    #[test]
    fn the_writes_that_would_guess_are_refused_by_name() {
        let mut store = store(&[]);
        let contents = store.contents().expect("contents");
        let gamma = uri_of(&contents, "witsml21.Channel", GAMMA);
        let log = store.resources().pop().expect("the log").uri;

        let object = store.get_object(&gamma).expect("reads");
        assert!(matches!(store.put(object), Err(Error::Unsupported { .. })));
        assert!(matches!(store.remove(&gamma), Err(Error::Unsupported { .. })));
        match store.remove(&log) {
            Err(Error::Unsupported { operation, .. }) => {
                assert!(operation.contains("live only inside it"), "{operation}");
            }
            other => panic!("the log held channels that live only inside it: {other:?}"),
        }
        assert_eq!(store.resources().len(), 1, "nothing was removed");
    }
}
