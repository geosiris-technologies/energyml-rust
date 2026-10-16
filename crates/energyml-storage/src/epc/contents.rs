//! Level 2.5: the data objects a part holds **by value**, found without
//! deserializing the part.
//!
//! A WITSML `Log` holds its `ChannelSet`s and a `ChannelSet` its `Channel`s
//! inline, each with a `uuid` of its own (ETP 1.2 section 9.1.3, p. 178: such an
//! object is a data object in its own right, contained by a ByValue
//! reference). They are not parts of the archive, so the inventory read at
//! open (`[Content_Types].xml`, then the citation of each part) does not know
//! them, and the citation parser cannot find them: it stops at the end of
//! `Citation`, and a contained object is written **after** it.
//!
//! This module is the walk that does find them. It streams the bytes of one
//! part through `quick_xml`, guided by the [`ContainedSlot`]s of the part's type
//! (`TypeRelations::contains`, written by the generator from the schema), and
//! keeps, for each element that sits at a slot path and carries a `uuid`:
//! its identity, the title of its citation, the byte range of its element, its
//! path and its **direct** container. Nothing is deserialized and no text is
//! copied beyond the attributes and the title.
//!
//! # When it runs
//!
//! Only on the parts whose type is a container
//! ([`Registry::is_container`](energyml_core::registry::Registry::is_container)).
//! [`ContainedIndex::build`] is handed the parts of the archive and reads
//! **those and no others**: a package whose types hold nothing by value (all of
//! RESQML 2.0.1) reads not one part more than before. The caller builds the
//! index once, on demand, and keeps it (`OnceLock`).
//!
//! # What the table does not say, and this walk does
//!
//! The table is not transitive: a `Log` slot says "a `ChannelSet` is here", the
//! `ChannelSet` entry says "a `Channel` is here". The walk composes them by
//! keeping a stack of frames, one per object opened (the part, then each
//! contained object found), each with the slots of its own type. A `Channel`
//! inside a `ChannelSet` inside a `Log` is therefore found at
//! `ChannelSet.Channel` and its container is the `ChannelSet`, never the `Log`.
//!
//! # Rules for the edge cases
//!
//! - The same object in two containers of one part (a `Channel` in two
//!   `ChannelSet`s) is **one** entry with two `contained_in`: ETP 1.2 section
//!   9.1.3 (p. 178), "MAY be included in one or more container data objects".
//!   The span and the path are those of the first occurrence.
//! - An element whose `uuid` is its container's own is not listed: an object
//!   cannot contain itself, and an edge to itself would loop.
//! - An element at a slot path **without a `uuid`** is no object
//!   (`energyml_core::identity::nested_objects` has the same rule).
//! - An object whose uuid is also that of a part of the archive is **not** in
//!   the index: the part wins, as in `graph::nested_index`. It is kept apart
//!   ([`ContainedIndex::shadowed`]) so the edge to its container is not lost.
//! - The same uuid under two different types is two entries and a warning
//!   naming both: identity is the triple (uuid, type, objectVersion), and
//!   nothing is fused.
//! - An abstract slot (`eml23.Aggregate.AggregateMember`) reads the type from
//!   `xsi:type`; a type the table does not know is **listed** with the type
//!   read, built from the namespace and `schemaVersion`, not dropped.
//!
//! `TODO(verify)`: a byte-order mark at the start of a part, and whether
//! `quick_xml` counts it in `buffer_position` (the spans would then be shifted
//! by three bytes). No test here covers it.
//! `TODO(verify)`: a polymorphic step (`Step::when_type`) and an `xsi:type`
//! are compared by the local name of the type, not by namespace: every version
//! of a package shares its namespace, and the two names a slot can mean differ
//! by package only in the cross-package case this has not met.

// The walk is consumed by `EpcStore` through the patch that wires it; until then
// part of it is unused.

use std::collections::HashMap;
use std::ops::Range;

use energyml_core::identifiers::{self, EtpUri};
use energyml_core::identity::ObjectId;
use energyml_core::naming;
use energyml_core::ns;
use energyml_core::registry::Registry;
use energyml_core::relations::{ContainedSlot, Step, TypeRelations};
use quick_xml::events::{BytesStart, Event};

use crate::error::{Diagnostic, Error, Result};
use crate::resource::ObjectSummary;

/// One data object held by value inside a part, as the scan found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContainedEntry {
    /// Its identity: canonical URI, uuid, qualified type, content type;
    /// `locator` is the part holding it; `contained_in` the containers' URIs.
    pub summary: ObjectSummary,
    /// The title of its citation, when it has one.
    pub title: Option<String>,
    /// The part (archive path) holding it.
    pub part: String,
    /// The byte range of its element in that part, start tag to end tag included.
    pub span: std::ops::Range<usize>,
    /// The XML element name it is written under (`Channel`), and its prefix if any.
    pub element: String,
    /// Dotted path from the part's root element (`ChannelSet.Channel`).
    pub path: String,
}

/// What a scan of one part found, and what it had to say about it.
#[derive(Debug, Default)]
pub(crate) struct Scan {
    /// The contained objects, in document order, the same object once.
    pub entries: Vec<ContainedEntry>,
    /// Departures from the specification met on the way, never dropped.
    pub diagnostics: Vec<Diagnostic>,
}

/// Every contained object of a package, built once on demand.
///
/// Entries are stored **grouped by lowercased uuid** so that
/// [`by_uuid`](ContainedIndex::by_uuid) can hand out a slice (several entries:
/// the same uuid in two types, or under two `objectVersion`s). Archive order,
/// the order a listing wants, is [`in_archive_order`](ContainedIndex::in_archive_order).
#[derive(Debug, Default)]
pub(crate) struct ContainedIndex {
    /// Sorted by lowercased uuid, then by archive order.
    entries: Vec<ContainedEntry>,
    /// Lowercased uuid to its run in `entries`.
    ranges: HashMap<String, Range<usize>>,
    /// For the k-th entry in archive order, its position in `entries`.
    archive: Vec<usize>,
    /// Contained objects left out because their uuid is a part of the archive.
    shadowed: Vec<ContainedEntry>,
    /// The parts that were read to build this, in order. Empty for a package
    /// that holds no container: the proof that nothing was opened.
    scanned: Vec<String>,
}

impl ContainedIndex {
    /// Every entry, grouped by lowercased uuid. Not archive order: see
    /// [`in_archive_order`](ContainedIndex::in_archive_order).
    pub(crate) fn entries(&self) -> &[ContainedEntry] {
        &self.entries
    }

    /// The entries carrying this uuid, in any case. Several: the same uuid under
    /// two types or two `objectVersion`s. Empty when none.
    pub(crate) fn by_uuid(&self, uuid: &str) -> &[ContainedEntry] {
        match self.ranges.get(&uuid.trim().to_ascii_lowercase()) {
            Some(range) => &self.entries[range.clone()],
            None => &[],
        }
    }

    /// The entries in the order the archive lists their containers, and in
    /// document order within a part.
    pub(crate) fn in_archive_order(&self) -> impl Iterator<Item = &ContainedEntry> + '_ {
        self.archive.iter().map(|at| &self.entries[*at])
    }

    /// The contained objects left out of the index because a part of the
    /// archive has their uuid. Their `contained_in` still names their
    /// containers, so the containment edge can be stated.
    pub(crate) fn shadowed(&self) -> &[ContainedEntry] {
        &self.shadowed
    }

    /// The parts read to build the index, in order.
    pub(crate) fn scanned_parts(&self) -> &[String] {
        &self.scanned
    }

    /// Build the index from the parts of an archive.
    ///
    /// `containers` are the summaries of the archive's parts, in archive
    /// order. Only those whose type is a container are read; `read_part` is
    /// handed the `locator` of each and returns its **decompressed** bytes.
    /// `is_part` is asked, with a lowercased uuid, whether a part of the
    /// archive has it (the part wins).
    ///
    /// A part that cannot be read or scanned is a warning, not a failure: the
    /// rest of the index stands, and the warning names the part.
    pub(crate) fn build<'a>(
        containers: impl IntoIterator<Item = &'a ObjectSummary>,
        registry: &Registry,
        is_part: &dyn Fn(&str) -> bool,
        read_part: &mut dyn FnMut(&str) -> Result<Vec<u8>>,
    ) -> (ContainedIndex, Vec<Diagnostic>) {
        let mut diagnostics = Vec::new();
        let mut scanned = Vec::new();
        let mut kept: Vec<ContainedEntry> = Vec::new();
        let mut shadowed: Vec<ContainedEntry> = Vec::new();
        // (lowercased uuid, lowercased type, objectVersion) to its place in `kept`.
        let mut seen: HashMap<(String, String, String), usize> = HashMap::new();
        // lowercased uuid to the lowercased types it was met under, each with
        // the part it was first met in. A collision inside one part was
        // already reported by its scan.
        let mut types: HashMap<String, Vec<(String, String)>> = HashMap::new();

        for container in containers {
            let Some(tables) = registry
                .relations_for(&container.qualified_type)
                .filter(|tables| tables.is_container())
            else {
                continue;
            };
            let bytes = match read_part(&container.locator) {
                Ok(bytes) => bytes,
                Err(e) => {
                    diagnostics.push(Diagnostic::warning(
                        &container.locator,
                        format!("its contained objects are not listed, the part could not be read: {e}"),
                    ));
                    continue;
                }
            };
            scanned.push(container.locator.clone());
            let scan = match scan_part_with(&bytes, container, tables, registry) {
                Ok(scan) => scan,
                Err(e) => {
                    diagnostics.push(Diagnostic::warning(
                        &container.locator,
                        format!("its contained objects are not listed, the part could not be scanned: {e}"),
                    ));
                    continue;
                }
            };
            diagnostics.extend(scan.diagnostics);

            for entry in scan.entries {
                let uuid = entry.summary.uuid.to_ascii_lowercase();
                let kind = entry.summary.qualified_type.to_ascii_lowercase();
                if is_part(&uuid) {
                    diagnostics.push(Diagnostic::note(
                        &entry.part,
                        format!(
                            "`{}` is also held by value in `{}` ({}); the part of that uuid is the object, \
                             the copy by value is not listed",
                            entry.summary.uri, entry.part, entry.path
                        ),
                    ));
                    shadowed.push(entry);
                    continue;
                }
                let kinds = types.entry(uuid.clone()).or_default();
                if !kinds.iter().any(|(held, _)| *held == kind) {
                    if let Some((other, other_part)) = kinds.first()
                        && *other_part != entry.part
                    {
                        diagnostics.push(Diagnostic::warning(
                            &entry.part,
                            format!(
                                "the uuid {uuid} is held by value as `{}` and as `{other}` (in `{other_part}`): \
                                 two objects, neither is fused into the other",
                                entry.summary.qualified_type
                            ),
                        ));
                    }
                    kinds.push((kind.clone(), entry.part.clone()));
                }
                let key = (uuid, kind, object_version_of(&entry.summary.uri));
                match seen.get(&key) {
                    Some(&at) => {
                        let held = &mut kept[at];
                        for uri in entry.summary.contained_in {
                            if !held.summary.contained_in.contains(&uri) {
                                held.summary.contained_in.push(uri);
                            }
                        }
                        held.summary.contained_in.sort();
                        if held.part != entry.part {
                            diagnostics.push(Diagnostic::note(
                                &entry.part,
                                format!(
                                    "`{}` is held by value in `{}` and in `{}`; it is read from the first",
                                    held.summary.uri, held.part, entry.part
                                ),
                            ));
                        }
                    }
                    None => {
                        seen.insert(key, kept.len());
                        kept.push(entry);
                    }
                }
            }
        }

        // Group by uuid, stable, so a run is a slice.
        let keys: Vec<String> = kept.iter().map(|e| e.summary.uuid.to_ascii_lowercase()).collect();
        let mut order: Vec<usize> = (0..kept.len()).collect();
        order.sort_by(|a, b| keys[*a].cmp(&keys[*b]));
        let mut archive = vec![0usize; kept.len()];
        for (position, original) in order.iter().enumerate() {
            archive[*original] = position;
        }
        let mut slots: Vec<Option<ContainedEntry>> = kept.into_iter().map(Some).collect();
        let entries: Vec<ContainedEntry> = order.iter().filter_map(|at| slots[*at].take()).collect();
        let mut ranges: HashMap<String, Range<usize>> = HashMap::new();
        for (position, entry) in entries.iter().enumerate() {
            let uuid = entry.summary.uuid.to_ascii_lowercase();
            ranges
                .entry(uuid)
                .and_modify(|range| range.end = position + 1)
                .or_insert(position..position + 1);
        }

        (
            ContainedIndex {
                entries,
                ranges,
                archive,
                shadowed,
                scanned,
            },
            diagnostics,
        )
    }
}

/// The `objectVersion` a canonical URI names, empty when it names none. Read
/// through [`EtpUri::parse`], the one door for a URI.
fn object_version_of(uri: &str) -> String {
    EtpUri::parse(uri)
        .ok()
        .and_then(|parsed| parsed.object_version)
        .unwrap_or_default()
}

/// [`scan_part`], with what it noticed.
pub(crate) fn scan_part_with<'a>(
    xml: &[u8],
    container: &ObjectSummary,
    tables: &'a TypeRelations,
    registry: &'a Registry,
) -> Result<Scan> {
    let mut scanner = Scanner::new(container, tables, registry);
    let mut reader = quick_xml::Reader::from_reader(xml);
    // Trimming per event would cut a text run in pieces; the title is
    // accumulated whole and trimmed once, as `header::peek_reader` does.
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    loop {
        // With text untrimmed every event starts where the last one ended, so
        // this is the offset of the `<` of a start tag.
        let start = reader.buffer_position() as usize;
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|e| Error::Xml(format!("{}: {e}", container.locator)))?;
        match event {
            Event::Eof => break,
            Event::Start(e) => scanner.open(&e, start, None),
            Event::Empty(e) => {
                let end = reader.buffer_position() as usize;
                scanner.open(&e, start, Some(end));
            }
            Event::End(_) => {
                let end = reader.buffer_position() as usize;
                scanner.close(end);
            }
            Event::Text(t) => {
                if let Some(text) = scanner.title.as_mut() {
                    let raw = t.xml10_content().unwrap_or_default();
                    match quick_xml::escape::unescape(&raw) {
                        Ok(unescaped) => text.push_str(&unescaped),
                        Err(_) => text.push_str(&raw),
                    }
                }
            }
            Event::CData(c) => {
                if let Some(text) = scanner.title.as_mut() {
                    text.push_str(&String::from_utf8_lossy(&c));
                }
            }
            // `&amp;`, `&#233;`: their own event since quick-xml 0.41.
            Event::GeneralRef(r) => {
                if let Some(text) = scanner.title.as_mut()
                    && let Ok(name) = r.decode()
                {
                    let reference = format!("&{name};");
                    if let Ok(resolved) = quick_xml::escape::unescape(&reference) {
                        text.push_str(&resolved);
                    }
                }
            }
            _ => {}
        }
        buf.clear();
    }
    if !scanner.elems.is_empty() {
        return Err(Error::Xml(format!(
            "{}: the part ends inside `{}`",
            container.locator,
            scanner.elems.last().map(|e| e.name.as_str()).unwrap_or_default()
        )));
    }
    Ok(Scan {
        entries: scanner.entries,
        diagnostics: scanner.diagnostics,
    })
}

/// An element open at the moment.
struct Elem {
    /// Local name.
    name: String,
    /// The local name of its `xsi:type`, when it has one.
    xsi: Option<String>,
    /// Set when this element is a contained object found by the walk.
    opened: Option<Opened>,
    /// Set when this is the `Title` of the citation of the entry at that index.
    title_of: Option<usize>,
}

/// A contained object whose element is open.
#[derive(Clone, Copy)]
struct Opened {
    /// Its index in the entries.
    entry: usize,
    /// Whether this is the occurrence that made the entry: only that one
    /// closes the span.
    first: bool,
}

/// An object open at the moment: the part itself, or a contained object found.
struct Frame<'a> {
    /// Its slots; `None` when the registry has no table for its type.
    table: Option<&'a TypeRelations>,
    /// Its canonical URI.
    uri: String,
    /// Its lowercased uuid.
    uuid: String,
    /// Index into the open elements of the first element **inside** it.
    base: usize,
    /// Its index in the entries; `None` for the part itself.
    entry: Option<usize>,
}

/// A contained object just recognised.
struct Found<'a> {
    opened: Opened,
    table: Option<&'a TypeRelations>,
    uri: String,
    uuid: String,
}

/// The state of one walk.
struct Scanner<'a> {
    registry: &'a Registry,
    root_table: &'a TypeRelations,
    root_uri: String,
    root_uuid: String,
    part: String,
    dataspace: Option<String>,
    entries: Vec<ContainedEntry>,
    diagnostics: Vec<Diagnostic>,
    elems: Vec<Elem>,
    frames: Vec<Frame<'a>>,
    /// `(index of the declaring element, prefix, uri)`, innermost last.
    ns: Vec<(usize, String, String)>,
    seen: HashMap<(String, String, String), usize>,
    types: HashMap<String, Vec<String>>,
    /// The text of the `Title` being read, when one is.
    title: Option<String>,
}

impl<'a> Scanner<'a> {
    fn new(container: &ObjectSummary, tables: &'a TypeRelations, registry: &'a Registry) -> Self {
        Scanner {
            registry,
            root_table: tables,
            root_uri: container.uri.clone(),
            root_uuid: container.uuid.to_ascii_lowercase(),
            part: container.locator.clone(),
            // The one door for a URI: the dataspace the container lives in is
            // the one its contents live in.
            dataspace: EtpUri::parse(&container.uri)
                .ok()
                .and_then(|parsed| parsed.dataspace),
            entries: Vec::new(),
            diagnostics: Vec::new(),
            elems: Vec::new(),
            frames: Vec::new(),
            ns: Vec::new(),
            seen: HashMap::new(),
            types: HashMap::new(),
            title: None,
        }
    }

    /// The namespace a prefix is bound to, innermost declaration first.
    fn namespace_of(&self, prefix: &str) -> Option<&str> {
        self.ns
            .iter()
            .rev()
            .find(|(_, bound, _)| bound == prefix)
            .map(|(_, _, uri)| uri.as_str())
    }

    /// Forget the declarations of the elements from `idx` on.
    fn drop_namespaces(&mut self, idx: usize) {
        while self
            .ns
            .last()
            .is_some_and(|(declared_at, _, _)| *declared_at >= idx)
        {
            self.ns.pop();
        }
    }

    fn open(&mut self, e: &BytesStart<'_>, start: usize, empty_end: Option<usize>) {
        let raw_name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
        let name = local_of(&raw_name).to_string();
        let idx = self.elems.len();

        let mut attrs: Vec<(String, String)> = Vec::new();
        for attribute in e.attributes().flatten() {
            let key = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
            let value = unescape_attribute(&attribute.value);
            if key == "xmlns" {
                self.ns.push((idx, String::new(), value.clone()));
            } else if let Some(bound) = key.strip_prefix("xmlns:") {
                self.ns.push((idx, bound.to_string(), value.clone()));
            }
            attrs.push((key, value));
        }
        // `xsi:type`: an attribute named `type` whose prefix is bound to the
        // XSI namespace, wherever the document declared it.
        let xsi_raw = attrs
            .iter()
            .find(|(key, _)| {
                key.split_once(':').is_some_and(|(prefix, local)| {
                    local == "type" && self.namespace_of(prefix) == Some(ns::XSI.uri)
                })
            })
            .map(|(_, value)| value.trim().to_string());
        let xsi = xsi_raw.as_deref().map(|value| local_of(value).to_string());

        self.elems.push(Elem {
            name: name.clone(),
            xsi,
            opened: None,
            title_of: None,
        });

        if idx == 0 {
            self.frames.push(Frame {
                table: Some(self.root_table),
                uri: self.root_uri.clone(),
                uuid: self.root_uuid.clone(),
                base: 1,
                entry: None,
            });
        } else if let Some(found) = self.try_contained(&attrs, xsi_raw.as_deref(), &raw_name, start) {
            match empty_end {
                None => {
                    self.frames.push(Frame {
                        table: found.table,
                        uri: found.uri,
                        uuid: found.uuid,
                        base: self.elems.len(),
                        entry: Some(found.opened.entry),
                    });
                    self.elems[idx].opened = Some(found.opened);
                }
                Some(end) => {
                    if found.opened.first {
                        self.entries[found.opened.entry].span.end = end;
                    }
                }
            }
        } else if let Some(frame) = self.frames.last()
            && let Some(entry) = frame.entry
            && self.elems.len() - frame.base == 2
            && self.elems[frame.base].name == "Citation"
            && name == "Title"
            && empty_end.is_none()
            && self.entries[entry].title.is_none()
        {
            self.elems[idx].title_of = Some(entry);
            self.title = Some(String::new());
        }

        if empty_end.is_some() {
            self.elems.pop();
            self.drop_namespaces(idx);
            if idx == 0 {
                self.frames.pop();
            }
        }
    }

    fn close(&mut self, end: usize) {
        let Some(element) = self.elems.pop() else {
            return;
        };
        let idx = self.elems.len();
        self.drop_namespaces(idx);
        if let Some(entry) = element.title_of {
            let text = self.title.take().unwrap_or_default();
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                self.entries[entry].title = Some(trimmed.to_string());
            }
        }
        if let Some(opened) = element.opened {
            self.frames.pop();
            if opened.first {
                self.entries[opened.entry].span.end = end;
            }
        }
        if idx == 0 {
            self.frames.pop();
        }
    }

    /// Whether the element just opened is a contained object of the object it
    /// sits in; if so, record it.
    fn try_contained(
        &mut self,
        attrs: &[(String, String)],
        xsi_raw: Option<&str>,
        raw_name: &str,
        start: usize,
    ) -> Option<Found<'a>> {
        let (slot, container_uri, container_uuid) = {
            let frame = self.frames.last()?;
            let table = frame.table?;
            let relative = &self.elems[frame.base..];
            let slot = table
                .contains
                .iter()
                .find(|slot| path_matches(slot.path, relative))?;
            (slot, frame.uri.clone(), frame.uuid.clone())
        };
        let attribute = |name: &str| {
            attrs
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.trim())
                .filter(|value| !value.is_empty())
        };
        // No uuid, no object: a slot element without one is plain content.
        let uuid = attribute("uuid")?;
        let path = self.elems[1..]
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>()
            .join(".");

        let qualified = match self.type_at(slot, xsi_raw, attrs) {
            Ok(qualified) => qualified,
            Err(why) => {
                self.diagnostics.push(Diagnostic::warning(
                    &self.part,
                    format!("`{path}` carries the uuid {uuid} but {why}; it is not listed"),
                ));
                return None;
            }
        };
        let uuid_lc = uuid.to_ascii_lowercase();
        if uuid_lc == container_uuid {
            self.diagnostics.push(Diagnostic::note(
                &self.part,
                format!("`{path}` has the uuid of the object it is held by, {uuid}; an object does not contain itself, it is not listed"),
            ));
            return None;
        }
        let kind = qualified.to_ascii_lowercase();
        let version = attribute("objectVersion").map(str::to_string);

        let kinds = self.types.entry(uuid_lc.clone()).or_default();
        if !kinds.contains(&kind) {
            if let Some(other) = kinds.first() {
                self.diagnostics.push(Diagnostic::warning(
                    &self.part,
                    format!(
                        "the uuid {uuid} is held by value as `{qualified}` and as `{other}`: two objects, \
                         neither is fused into the other"
                    ),
                ));
            }
            kinds.push(kind.clone());
        }

        let key = (uuid_lc.clone(), kind, version.clone().unwrap_or_default());
        if let Some(&at) = self.seen.get(&key) {
            let held = &mut self.entries[at].summary.contained_in;
            if held.contains(&container_uri) {
                self.diagnostics.push(Diagnostic::warning(
                    &self.part,
                    format!(
                        "`{path}`: same uuid, same type, same objectVersion twice in `{container_uri}`; the first wins"
                    ),
                ));
            } else {
                held.push(container_uri);
                held.sort();
            }
            let summary = &self.entries[at].summary;
            return Some(Found {
                opened: Opened {
                    entry: at,
                    first: false,
                },
                table: self.registry.relations_for(&summary.qualified_type),
                uri: summary.uri.clone(),
                uuid: uuid_lc,
            });
        }

        let mut id = match ObjectId::from_qualified_type(&qualified, uuid) {
            Ok(id) => id,
            Err(e) => {
                self.diagnostics.push(Diagnostic::warning(
                    &self.part,
                    format!("`{path}`: `{qualified}` is not a qualified type ({e}); it is not listed"),
                ));
                return None;
            }
        };
        id.object_version = version;
        if identifiers::is_nil_uuid(uuid) {
            self.diagnostics
                .push(Diagnostic::nil_uuid(&self.part, Some(&path)));
        }
        let uri = id.etp_uri(self.dataspace.as_deref());
        let summary = ObjectSummary::new(uri.clone(), uuid, id.qualified_type())
            .with_content_type(id.content_type())
            .with_locator(self.part.clone())
            .with_contained_in(vec![container_uri]);
        let table = self.registry.relations_for(&summary.qualified_type);
        let at = self.entries.len();
        self.entries.push(ContainedEntry {
            summary,
            title: None,
            part: self.part.clone(),
            span: start..start,
            element: raw_name.to_string(),
            path,
        });
        self.seen.insert(key, at);
        Some(Found {
            opened: Opened {
                entry: at,
                first: true,
            },
            table,
            uri,
            uuid: uuid_lc,
        })
    }

    /// The qualified type of the object in `slot`, or why none can be told.
    fn type_at(
        &self,
        slot: &ContainedSlot,
        xsi_raw: Option<&str>,
        attrs: &[(String, String)],
    ) -> std::result::Result<String, String> {
        let Some(raw) = xsi_raw else {
            return if slot.abstract_bound {
                Err(format!(
                    "the slot is declared as the abstract `{}` and the element has no xsi:type",
                    slot.declared
                ))
            } else {
                Ok(slot.declared.to_string())
            };
        };
        let wanted = local_of(raw);
        let known = std::iter::once(slot.declared)
            .chain(slot.alternatives.iter().copied())
            .find(|candidate| naming::type_eq_loose(qualified_local(candidate), wanted));
        match known {
            Some(candidate) if slot.admits(candidate) => Ok(candidate.to_string()),
            Some(candidate) => Err(format!(
                "its xsi:type `{raw}` names `{candidate}`, which is not a data object type"
            )),
            None if slot.abstract_bound => self.type_from_namespace(raw, attrs),
            None => Err(format!(
                "its xsi:type `{raw}` is not one of the types admitted there (`{}`{})",
                slot.declared,
                slot.alternatives
                    .iter()
                    .map(|a| format!(", `{a}`"))
                    .collect::<String>()
            )),
        }
    }

    /// A qualified type for an `xsi:type` the slot does not list: the package
    /// from the namespace its prefix is bound to, the version from the
    /// element's own `schemaVersion`.
    ///
    /// `TODO(verify)`: RESQML 2.0.1 and EML 2.0 spell their qualified types
    /// `obj_Name`; the name is used as written, so such a type reads as unknown
    /// rather than as the wrong one.
    fn type_from_namespace(
        &self,
        raw: &str,
        attrs: &[(String, String)],
    ) -> std::result::Result<String, String> {
        let prefix = raw.split_once(':').map_or("", |(prefix, _)| prefix);
        let uri = self
            .namespace_of(prefix)
            .ok_or_else(|| format!("its xsi:type `{raw}` uses a prefix bound to no namespace"))?;
        let domain = [ns::EML, ns::RESQML, ns::WITSML, ns::PRODML]
            .into_iter()
            .find(|known| known.uri == uri)
            .map(|known| known.prefix)
            .ok_or_else(|| {
                format!("its xsi:type `{raw}` is in the namespace `{uri}`, which is no Energistics package")
            })?;
        let version = attrs
            .iter()
            .find(|(key, _)| key == "schemaVersion")
            .map(|(_, value)| naming::compact_version(value.trim()))
            .filter(|version| !version.is_empty())
            .ok_or_else(|| format!("its xsi:type `{raw}` is unknown and the element has no schemaVersion"))?;
        Ok(format!("{domain}{version}.{}", local_of(raw)))
    }
}

/// Whether the elements open since the start of an object spell a slot's path.
fn path_matches(path: &[Step], relative: &[Elem]) -> bool {
    path.len() == relative.len()
        && path.iter().zip(relative).all(|(step, element)| {
            step.name == element.name
                && step.when_type.is_none_or(|when| {
                    element
                        .xsi
                        .as_deref()
                        .is_some_and(|actual| naming::type_eq_loose(qualified_local(when), actual))
                })
        })
}

/// `Channel` of `witsml21.Channel`.
fn qualified_local(qualified: &str) -> &str {
    qualified.rsplit_once('.').map_or(qualified, |(_, name)| name)
}

/// The part of a possibly prefixed name after the colon.
fn local_of(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, local)| local)
}

fn unescape_attribute(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    quick_xml::escape::unescape(&text)
        .map(|unescaped| unescaped.into_owned())
        .unwrap_or_else(|_| text.into_owned())
}

/// Whether the index holds no contained object. Tests only.
#[cfg(test)]
impl ContainedIndex {
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Scan one container part, dropping the diagnostics; tests only
/// ([`scan_part_with`] is what the index calls, diagnostics kept).
#[cfg(test)]
pub(crate) fn scan_part<'a>(
    xml: &[u8],
    container: &ObjectSummary,
    tables: &'a TypeRelations,
    registry: &'a Registry,
) -> Result<Vec<ContainedEntry>> {
    Ok(scan_part_with(xml, container, tables, registry)?.entries)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashSet;

    use super::*;

    // ---- The tables the walk is guided by --------------------------------
    //
    // Written by hand from the schemas, one slot per `xs:element` that holds a
    // data object by value, and shaped like the generated `relations.rs`
    // (`Step`, `ContainedSlot`). `TODO(verify)`: held against the generated
    // tables of `energyml-witsml-2-1` and `energyml-prodml-2-3` by a test that
    // may depend on those crates; this crate's tests do not.

    /// witsml/v2.1/xsd_schemas/Log.xsd l. 822: `ChannelSet`, `minOccurs=0`,
    /// `maxOccurs=unbounded`, inside the extension of `Log`.
    static LOG_SLOTS: &[ContainedSlot] = &[ContainedSlot {
        path: &[Step::new("ChannelSet")],
        declared: "witsml21.ChannelSet",
        alternatives: &["witsml21.PPFGChannelSet"],
        abstract_bound: false,
        min_occurs: 0,
        max_occurs: None,
        source: "witsml/v2.1/xsd_schemas/Log.xsd#Log.ChannelSet",
    }];

    /// witsml/v2.1/xsd_schemas/Log.xsd l. 650: `Channel` in `ChannelSet`.
    static CHANNEL_SET_SLOTS: &[ContainedSlot] = &[ContainedSlot {
        path: &[Step::new("Channel")],
        declared: "witsml21.Channel",
        alternatives: &["witsml21.PPFGChannel"],
        abstract_bound: false,
        min_occurs: 0,
        max_occurs: None,
        source: "witsml/v2.1/xsd_schemas/Log.xsd#ChannelSet.Channel",
    }];

    /// prodml/v2.3/xsd_schemas/SimpleProductVolume.xsd l. 562 (`Disposition`,
    /// declared `AbstractDisposition`), l. 660 (`Transfer` in
    /// `TransferDisposition`) and l. 613 (`TerminalLifting` in
    /// `TerminalLiftingDisposition`).
    static VOLUMES_SLOTS: &[ContainedSlot] = &[
        ContainedSlot {
            path: &[
                Step::new("ReportingEntityVolumes"),
                Step::when("Disposition", "prodml23.TransferDisposition"),
                Step::new("Transfer"),
            ],
            declared: "prodml23.Transfer",
            alternatives: &[],
            abstract_bound: false,
            min_occurs: 0,
            max_occurs: Some(1),
            source: "prodml/v2.3/xsd_schemas/SimpleProductVolume.xsd#TransferDisposition.Transfer",
        },
        ContainedSlot {
            path: &[
                Step::new("ReportingEntityVolumes"),
                Step::when("Disposition", "prodml23.TerminalLiftingDisposition"),
                Step::new("TerminalLifting"),
            ],
            declared: "prodml23.TerminalLifting",
            alternatives: &[],
            abstract_bound: false,
            min_occurs: 0,
            max_occurs: Some(1),
            source: "prodml/v2.3/xsd_schemas/SimpleProductVolume.xsd#TerminalLiftingDisposition.TerminalLifting",
        },
    ];

    /// common/v2.3/xsd_schemas/Aggregate.xsd l. 27: `AggregateMember`, declared
    /// `eml:AbstractObject`, which is abstract: only `xsi:type` says the type.
    static AGGREGATE_SLOTS: &[ContainedSlot] = &[ContainedSlot {
        path: &[Step::new("AggregateMember")],
        declared: "eml23.AbstractObject",
        alternatives: &["eml23.PropertyKind"],
        abstract_bound: true,
        min_occurs: 1,
        max_occurs: None,
        source: "common/v2.3/xsd_schemas/Aggregate.xsd#Aggregate.AggregateMember",
    }];

    const fn table(
        package: &'static str,
        version: &'static str,
        name: &'static str,
        qualified_type: &'static str,
        contains: &'static [ContainedSlot],
    ) -> TypeRelations {
        TypeRelations {
            package,
            version,
            name,
            qualified_type,
            contains,
            refers: &[],
        }
    }

    static TABLES: &[TypeRelations] = &[
        table("witsml", "2.1", "Log", "witsml21.Log", LOG_SLOTS),
        // `PPFGLog` extends `Log`, so it inherits the slot (PPFG.xsd).
        table("witsml", "2.1", "PPFGLog", "witsml21.PPFGLog", LOG_SLOTS),
        table(
            "witsml",
            "2.1",
            "ChannelSet",
            "witsml21.ChannelSet",
            CHANNEL_SET_SLOTS,
        ),
        // `PPFGChannelSet` extends `ChannelSet` (PPFG.xsd l. 472).
        table(
            "witsml",
            "2.1",
            "PPFGChannelSet",
            "witsml21.PPFGChannelSet",
            CHANNEL_SET_SLOTS,
        ),
        table("witsml", "2.1", "Channel", "witsml21.Channel", &[]),
        table("witsml", "2.1", "PPFGChannel", "witsml21.PPFGChannel", &[]),
        table("witsml", "2.1", "Wellbore", "witsml21.Wellbore", &[]),
        table(
            "prodml",
            "2.3",
            "AssetProductionVolumes",
            "prodml23.AssetProductionVolumes",
            VOLUMES_SLOTS,
        ),
        table("eml", "2.3", "Aggregate", "eml23.Aggregate", AGGREGATE_SLOTS),
    ];

    fn registry() -> Registry {
        let mut registry = Registry::new();
        registry.register_relations(TABLES);
        registry
    }

    // ---- Documents, written from the schemas -----------------------------

    const NAMESPACES: &str = r#"xmlns:witsml="http://www.energistics.org/energyml/data/witsmlv2" xmlns:prodml="http://www.energistics.org/energyml/data/prodmlv2" xmlns:eml="http://www.energistics.org/energyml/data/commonv2" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance""#;

    fn id(n: u32) -> String {
        // Letters in it, so that a test of case-insensitivity can fail.
        format!("abcdef01-0000-4000-8000-{n:012}")
    }

    fn uri(qualified: &str, n: u32) -> String {
        EtpUri::object(None, qualified, &id(n))
    }

    /// `eml:Citation`, eml/v2.3 Abstract.xsd: `Title`, `Originator`, `Creation`, `Format`.
    fn citation(title: &str) -> String {
        format!(
            "<eml:Citation><eml:Title>{title}</eml:Title><eml:Originator>t</eml:Originator>\
             <eml:Creation>2026-01-01T00:00:00Z</eml:Creation><eml:Format>t</eml:Format></eml:Citation>"
        )
    }

    /// An `eml:DataObjectReference`: its identity is in child **elements**, no
    /// `uuid` attribute, so it is no contained object (Log.xsd l. 401).
    fn reference(element: &str, n: u32) -> String {
        format!(
            "<witsml:{element}><eml:Uuid>{}</eml:Uuid><eml:QualifiedType>witsml21.Wellbore</eml:QualifiedType>\
             <eml:Title>W</eml:Title></witsml:{element}>",
            id(n)
        )
    }

    /// `witsml:Channel`, Log.xsd l. 210; `Wellbore` is a reference (l. 401).
    fn channel(n: u32, title: &str) -> String {
        format!(
            r#"<witsml:Channel uuid="{}" schemaVersion="2.1">{}<witsml:Mnemonic>M{n}</witsml:Mnemonic>{}</witsml:Channel>"#,
            id(n),
            citation(title),
            reference("Wellbore", 900)
        )
    }

    /// `witsml:ChannelSet`, Log.xsd l. 532; `Channel` at l. 650.
    fn channel_set(n: u32, title: &str, xsi_type: Option<&str>, channels: &[String]) -> String {
        let typed = xsi_type
            .map(|t| format!(r#" xsi:type="{t}""#))
            .unwrap_or_default();
        format!(
            r#"<witsml:ChannelSet{typed} uuid="{}" schemaVersion="2.1">{}{}{}</witsml:ChannelSet>"#,
            id(n),
            citation(title),
            channels.concat(),
            reference("Wellbore", 900)
        )
    }

    /// `witsml:Log`, Log.xsd l. 691; `ChannelSet` at l. 822.
    fn log(n: u32, title: &str, sets: &[String]) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<witsml:Log {NAMESPACES} uuid="{}" schemaVersion="2.1">{}{}{}</witsml:Log>"#,
            id(n),
            citation(title),
            sets.concat(),
            reference("Wellbore", 900)
        )
    }

    fn summary(qualified: &str, n: u32, locator: &str) -> ObjectSummary {
        ObjectSummary::new(uri(qualified, n), id(n), qualified).with_locator(locator)
    }

    fn scan(xml: &str, container: &ObjectSummary) -> Scan {
        let registry = registry();
        let tables = registry
            .relations_for(&container.qualified_type)
            .expect("a table");
        scan_part_with(xml.as_bytes(), container, tables, &registry).expect("the part scans")
    }

    fn entry(scan: &Scan, n: u32) -> &ContainedEntry {
        scan.entries
            .iter()
            .find(|e| e.summary.uuid == id(n))
            .unwrap_or_else(|| panic!("no entry for {n}: {:?}", scan.entries))
    }

    // ---- The walk ---------------------------------------------------------

    /// Energistics Identifier Specification v5.0, section 2.3.1 (p. 9): an
    /// instance of a business object is identified by a UUID as defined by
    /// RFC 4122, whose section 4.1.7 makes the nil UUID the form that names no
    /// instance. An object held by value with the nil UUID is still listed;
    /// the scan says so, with its path, and a channel with an ordinary uuid
    /// beside it is not mentioned.
    ///
    /// Break it: drop the `is_nil_uuid` check in `try_contained`, and the first
    /// assertion fails.
    #[test]
    fn a_contained_object_with_the_nil_uuid_is_listed_and_warned_about() {
        let nil = "00000000-0000-0000-0000-000000000000";
        let nil_channel = channel(1, "nil").replace(&id(1), nil);
        let xml = log(
            9,
            "log",
            &[channel_set(
                8,
                "set",
                None,
                &[nil_channel, channel(2, "ordinary")],
            )],
        );
        let scan = scan(&xml, &summary("witsml21.Log", 9, "Log.xml"));

        let held = scan
            .entries
            .iter()
            .find(|e| e.summary.uuid == nil)
            .expect("the nil-uuid channel is listed all the same");
        assert_eq!(held.path, "ChannelSet.Channel");
        let nil_warnings: Vec<_> = scan
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("nil UUID"))
            .collect();
        assert_eq!(nil_warnings.len(), 1, "{:?}", scan.diagnostics);
        assert_eq!(nil_warnings[0].severity, crate::error::Severity::Warning);
        assert_eq!(nil_warnings[0].part, "Log.xml");
        assert!(nil_warnings[0].message.starts_with("`ChannelSet.Channel`: "));
        // The ordinary channel is listed and unremarked.
        entry(&scan, 2);
    }

    /// A `Log` holding two `ChannelSet`s that hold `Channel`s: the nesting is
    /// composed from three tables, the container of a `Channel` is its
    /// `ChannelSet`, and the span is the element.
    ///
    /// Break it: take the container from the bottom of the stack (the part)
    /// instead of its top, and `Channel` 3 says it is held by the `Log`; or
    /// drop the frame push, and no `Channel` is found at all.
    #[test]
    fn a_log_holds_channel_sets_which_hold_channels() {
        let xml = log(
            1,
            "Log",
            &[
                channel_set(2, "Set A", None, &[channel(3, "GR"), channel(4, "ROP")]),
                channel_set(5, "Set B", None, &[channel(6, "DT")]),
            ],
        );
        let found = scan(&xml, &summary("witsml21.Log", 1, "Log_1.xml"));
        assert!(found.diagnostics.is_empty(), "{:?}", found.diagnostics);

        let order: Vec<&str> = found.entries.iter().map(|e| e.summary.uuid.as_str()).collect();
        let expected = [2, 3, 4, 5, 6].map(id);
        assert_eq!(order, expected.iter().map(String::as_str).collect::<Vec<_>>());

        let set = entry(&found, 2);
        assert_eq!(set.summary.qualified_type, "witsml21.ChannelSet");
        assert_eq!(set.summary.uri, uri("witsml21.ChannelSet", 2));
        assert_eq!(
            set.summary.content_type,
            "application/x-witsml+xml;version=2.1;type=ChannelSet"
        );
        assert_eq!(set.summary.locator, "Log_1.xml");
        assert_eq!(set.part, "Log_1.xml");
        assert_eq!(set.summary.contained_in, vec![uri("witsml21.Log", 1)]);
        assert_eq!(set.title.as_deref(), Some("Set A"));
        assert_eq!(set.path, "ChannelSet");
        assert_eq!(set.element, "witsml:ChannelSet");

        let gr = entry(&found, 3);
        assert_eq!(gr.summary.qualified_type, "witsml21.Channel");
        assert_eq!(
            gr.summary.contained_in,
            vec![uri("witsml21.ChannelSet", 2)],
            "a Channel is held by its ChannelSet, never by the Log"
        );
        assert_eq!(gr.title.as_deref(), Some("GR"));
        assert_eq!(gr.path, "ChannelSet.Channel");
        assert_eq!(
            entry(&found, 6).summary.contained_in,
            vec![uri("witsml21.ChannelSet", 5)]
        );

        // The span is the element, start tag to end tag.
        let text = &xml[gr.span.clone()];
        assert!(text.starts_with("<witsml:Channel "), "{text}");
        assert!(text.ends_with("</witsml:Channel>"), "{text}");
        assert!(text.contains(&id(3)) && !text.contains(&id(4)));
        // A container's span holds the spans of what it holds.
        assert!(set.span.start < gr.span.start && gr.span.end < set.span.end);
        let outer = &xml[set.span.clone()];
        assert!(outer.starts_with("<witsml:ChannelSet ") && outer.ends_with("</witsml:ChannelSet>"));
    }

    /// A reference to a `Wellbore` is written with child elements and no
    /// `uuid` attribute: it is not a data object held by value, and the
    /// `Log` itself is not in its own list.
    ///
    /// Break it: list any element with a `Uuid` child, and the `Wellbore`
    /// reference (uuid 900) appears.
    #[test]
    fn a_reference_is_not_a_contained_object() {
        let xml = log(1, "Log", &[channel_set(2, "A", None, &[channel(3, "GR")])]);
        let found = scan(&xml, &summary("witsml21.Log", 1, "Log_1.xml"));
        assert!(found.entries.iter().all(|e| e.summary.uuid != id(900)));
        assert!(found.entries.iter().all(|e| e.summary.uuid != id(1)));
        assert_eq!(found.entries.len(), 2);
    }

    /// The same `Channel` in two `ChannelSet`s of one part is one entry held
    /// by both. ETP 1.2 section 9.1.3 (p. 178): an object "MAY be included in
    /// one or more container data objects".
    ///
    /// Break it: key the entry on the container as well, and there are two
    /// `Channel` entries.
    #[test]
    fn the_same_channel_in_two_channel_sets_is_one_entry_with_two_containers() {
        let xml = log(
            1,
            "Log",
            &[
                channel_set(2, "A", None, &[channel(3, "GR")]),
                channel_set(4, "B", None, &[channel(3, "GR")]),
            ],
        );
        let found = scan(&xml, &summary("witsml21.Log", 1, "Log_1.xml"));
        assert!(found.diagnostics.is_empty(), "{:?}", found.diagnostics);
        assert_eq!(found.entries.len(), 3, "two sets and one channel");
        let shared = entry(&found, 3);
        let mut containers = vec![uri("witsml21.ChannelSet", 2), uri("witsml21.ChannelSet", 4)];
        containers.sort();
        assert_eq!(shared.summary.contained_in, containers);
        // The first occurrence gives the span.
        assert!(xml[..shared.span.start].contains(&id(2)) && !xml[..shared.span.start].contains(&id(4)));
    }

    /// The same `Channel` twice in the same `ChannelSet` is a defect of the
    /// document: the first wins, with a warning.
    ///
    /// Break it: stay silent, and the document is read as if it were well formed.
    #[test]
    fn the_same_channel_twice_in_one_channel_set_is_the_first_one_and_a_warning() {
        let xml = log(
            1,
            "Log",
            &[channel_set(
                2,
                "A",
                None,
                &[channel(3, "GR"), channel(3, "again")],
            )],
        );
        let found = scan(&xml, &summary("witsml21.Log", 1, "Log_1.xml"));
        assert_eq!(found.entries.len(), 2);
        assert_eq!(entry(&found, 3).title.as_deref(), Some("GR"));
        assert_eq!(found.diagnostics.len(), 1, "{:?}", found.diagnostics);
        assert!(found.diagnostics[0].message.contains("the first wins"));
    }

    /// A `ChannelSet` slot also takes a `PPFGChannelSet`, selected by `xsi:type`
    /// (Log.xsd l. 822 declares `ChannelSet`; PPFG.xsd l. 472 extends it). Its
    /// type is the subtype, and its `Channel`s are found through the table of
    /// the subtype.
    ///
    /// Break it: ignore `xsi:type`, and the entry says `witsml21.ChannelSet`.
    #[test]
    fn a_polymorphic_channel_set_is_read_by_its_xsi_type() {
        let xml = log(
            1,
            "Log",
            &[channel_set(
                2,
                "PPFG",
                Some("witsml:PPFGChannelSet"),
                &[channel(3, "PP")],
            )],
        );
        let found = scan(&xml, &summary("witsml21.Log", 1, "Log_1.xml"));
        let set = entry(&found, 2);
        assert_eq!(set.summary.qualified_type, "witsml21.PPFGChannelSet");
        assert_eq!(set.summary.uri, uri("witsml21.PPFGChannelSet", 2));
        assert_eq!(
            set.element, "witsml:ChannelSet",
            "the element keeps the name it is written under"
        );
        assert_eq!(
            entry(&found, 3).summary.contained_in,
            vec![uri("witsml21.PPFGChannelSet", 2)]
        );
    }

    /// An `xsi:type` the slot does not admit is not listed, and says why: no
    /// guess about what the element might have been.
    ///
    /// Break it: take the declared type whatever `xsi:type` says, and the
    /// `Wellbore` is listed as a `ChannelSet`.
    #[test]
    fn an_xsi_type_the_slot_does_not_admit_is_refused_by_name() {
        let xml = log(1, "Log", &[channel_set(2, "odd", Some("witsml:Wellbore"), &[])]);
        let found = scan(&xml, &summary("witsml21.Log", 1, "Log_1.xml"));
        assert!(found.entries.is_empty());
        assert_eq!(found.diagnostics.len(), 1);
        assert!(
            found.diagnostics[0]
                .message
                .contains("not one of the types admitted")
        );
    }

    /// An element at a slot path without a `uuid` is no object, and what is
    /// inside it belongs to the object it sits in (the `Log` has no
    /// `ChannelSet.Channel` slot).
    ///
    /// Break it: list slot elements whatever their attributes, and the set
    /// without a uuid appears with an empty uuid.
    #[test]
    fn an_element_without_a_uuid_is_no_contained_object() {
        let set = format!("<witsml:ChannelSet>{}</witsml:ChannelSet>", channel(3, "GR"));
        let xml = log(1, "Log", &[set]);
        let found = scan(&xml, &summary("witsml21.Log", 1, "Log_1.xml"));
        assert!(found.entries.is_empty(), "{:?}", found.entries);
    }

    /// An object does not contain itself: a `Channel` with its `ChannelSet`'s
    /// uuid is left out with a note.
    ///
    /// Break it: drop the test, and the `Channel` lists the `ChannelSet` as its
    /// container while sharing its uuid.
    #[test]
    fn an_element_with_its_containers_uuid_is_not_listed() {
        let xml = log(1, "Log", &[channel_set(2, "A", None, &[channel(2, "loop")])]);
        let found = scan(&xml, &summary("witsml21.Log", 1, "Log_1.xml"));
        assert_eq!(found.entries.len(), 1);
        assert_eq!(found.entries[0].summary.qualified_type, "witsml21.ChannelSet");
        assert_eq!(found.diagnostics.len(), 1);
        assert!(found.diagnostics[0].message.contains("does not contain itself"));
    }

    /// A self-closing element is an object with its own span.
    ///
    /// Break it: read only start events, and it is missed.
    #[test]
    fn an_empty_element_is_a_contained_object_with_its_own_span() {
        let tag = format!(r#"<witsml:Channel uuid="{}" schemaVersion="2.1"/>"#, id(3));
        let set = format!(
            r#"<witsml:ChannelSet uuid="{}" schemaVersion="2.1">{}{tag}</witsml:ChannelSet>"#,
            id(2),
            citation("A")
        );
        let xml = log(1, "Log", &[set]);
        let found = scan(&xml, &summary("witsml21.Log", 1, "Log_1.xml"));
        let channel = entry(&found, 3);
        assert_eq!(&xml[channel.span.clone()], tag);
        assert_eq!(channel.title, None, "no citation, no title");
    }

    /// `objectVersion` is part of the identity, and of the URI.
    ///
    /// Break it: leave it out of the key, and two versions of one `Channel` fuse.
    #[test]
    fn the_object_version_is_part_of_the_identity() {
        let versioned = |version: &str| {
            format!(
                r#"<witsml:Channel uuid="{}" objectVersion="{version}" schemaVersion="2.1">{}</witsml:Channel>"#,
                id(3),
                citation("GR")
            )
        };
        let xml = log(
            1,
            "Log",
            &[channel_set(2, "A", None, &[versioned("a"), versioned("b")])],
        );
        let found = scan(&xml, &summary("witsml21.Log", 1, "Log_1.xml"));
        let channels: Vec<_> = found.entries.iter().filter(|e| e.summary.uuid == id(3)).collect();
        assert_eq!(channels.len(), 2);
        assert!(
            channels[0].summary.uri.contains("version='a'"),
            "{}",
            channels[0].summary.uri
        );
        assert!(channels[1].summary.uri.contains("version='b'"));
    }

    /// The title is the one of the contained object's own citation, entities
    /// resolved, not the container's and not a deeper element's.
    ///
    /// Break it: take the first `Title` met, and `Channel` 3 gets "Set A".
    #[test]
    fn the_title_is_the_one_of_the_objects_own_citation() {
        let titled = format!(
            r#"<witsml:Channel uuid="{}" schemaVersion="2.1"><eml:Citation><eml:Title>A &amp; B</eml:Title></eml:Citation></witsml:Channel>"#,
            id(3)
        );
        let xml = log(1, "Log", &[channel_set(2, "Set A", None, &[titled])]);
        let found = scan(&xml, &summary("witsml21.Log", 1, "Log_1.xml"));
        assert_eq!(entry(&found, 2).title.as_deref(), Some("Set A"));
        assert_eq!(entry(&found, 3).title.as_deref(), Some("A & B"));
    }

    /// The container's dataspace carries to what it contains.
    ///
    /// Break it: build the URI without the dataspace, and the contained
    /// object is in the default one.
    #[test]
    fn a_contained_object_lives_in_its_containers_dataspace() {
        let mut container = summary("witsml21.Log", 1, "Log_1.xml");
        container.uri = EtpUri::object(Some("demo"), "witsml21.Log", &id(1));
        let xml = log(1, "Log", &[channel_set(2, "A", None, &[])]);
        let found = scan(&xml, &container);
        assert_eq!(
            entry(&found, 2).summary.uri,
            EtpUri::object(Some("demo"), "witsml21.ChannelSet", &id(2))
        );
        assert_eq!(entry(&found, 2).summary.contained_in, vec![container.uri.clone()]);
    }

    // ---- A polymorphic path: PRODML's dispositions ------------------------

    fn volumes(dispositions: &str) -> String {
        format!(
            r#"<prodml:AssetProductionVolumes {NAMESPACES} uuid="{}" schemaVersion="2.3">{}<prodml:ReportingEntityVolumes>{dispositions}</prodml:ReportingEntityVolumes></prodml:AssetProductionVolumes>"#,
            id(1),
            citation("Volumes")
        )
    }

    /// A step of a slot path may require an `xsi:type`: only a
    /// `TransferDisposition` holds a `Transfer` (SimpleProductVolume.xsd
    /// l. 652-660), only a `TerminalLiftingDisposition` a `TerminalLifting`.
    ///
    /// Break it: ignore `when_type`, and the `Transfer` written under a
    /// terminal-lifting disposition (uuid 5) is listed too.
    #[test]
    fn a_step_can_require_an_xsi_type() {
        let transfer = |n: u32| {
            format!(
                r#"<prodml:Transfer uuid="{}" schemaVersion="2.3">{}</prodml:Transfer>"#,
                id(n),
                citation("T")
            )
        };
        let dispositions = format!(
            r#"<prodml:Disposition xsi:type="prodml:TransferDisposition">{}</prodml:Disposition>
<prodml:Disposition xsi:type="prodml:TerminalLiftingDisposition"><prodml:TerminalLifting uuid="{}" schemaVersion="2.3">{}</prodml:TerminalLifting>{}</prodml:Disposition>"#,
            transfer(3),
            id(4),
            citation("L"),
            transfer(5)
        );
        let xml = volumes(&dispositions);
        let found = scan(
            &xml,
            &summary("prodml23.AssetProductionVolumes", 1, "Volumes_1.xml"),
        );
        assert_eq!(entry(&found, 3).summary.qualified_type, "prodml23.Transfer");
        assert_eq!(
            entry(&found, 3).path,
            "ReportingEntityVolumes.Disposition.Transfer"
        );
        assert_eq!(
            entry(&found, 4).summary.qualified_type,
            "prodml23.TerminalLifting"
        );
        assert!(
            found.entries.iter().all(|e| e.summary.uuid != id(5)),
            "a Transfer is not held by a terminal-lifting disposition"
        );
        assert_eq!(found.entries.len(), 2);
    }

    /// A disposition with no `xsi:type` matches no typed step.
    ///
    /// Break it: treat a missing `xsi:type` as a match, and the `Transfer` is listed.
    #[test]
    fn a_typed_step_needs_the_xsi_type_to_be_there() {
        let xml = volumes(&format!(
            r#"<prodml:Disposition><prodml:Transfer uuid="{}" schemaVersion="2.3"/></prodml:Disposition>"#,
            id(3)
        ));
        let found = scan(
            &xml,
            &summary("prodml23.AssetProductionVolumes", 1, "Volumes_1.xml"),
        );
        assert!(found.entries.is_empty());
    }

    // ---- An abstract slot: eml23.Aggregate --------------------------------

    /// `AggregateMember` is declared as `eml:AbstractObject`: the type is the
    /// `xsi:type`. One the slot lists is read from the table; one it does not
    /// (a WITSML `Channel`) is listed with the type built from the namespace
    /// and the member's `schemaVersion`; one with no `xsi:type` cannot be
    /// typed and is refused by name.
    ///
    /// Break it: drop the fallback, and the `Channel` member vanishes without a
    /// word; or accept `AbstractObject` as a type, and the untyped member is
    /// listed under a type that is no data object.
    #[test]
    fn an_abstract_slot_takes_its_type_from_xsi_type() {
        let xml = format!(
            r#"<eml:Aggregate {NAMESPACES} uuid="{}" schemaVersion="2.3">{}
<eml:AggregateMember xsi:type="eml:PropertyKind" uuid="{}" schemaVersion="2.3"/>
<eml:AggregateMember xsi:type="witsml:Channel" uuid="{}" schemaVersion="2.1"/>
<eml:AggregateMember uuid="{}" schemaVersion="2.3"/>
</eml:Aggregate>"#,
            id(1),
            citation("Bundle"),
            id(2),
            id(3),
            id(4)
        );
        let found = scan(&xml, &summary("eml23.Aggregate", 1, "Aggregate_1.xml"));
        assert_eq!(entry(&found, 2).summary.qualified_type, "eml23.PropertyKind");
        assert_eq!(entry(&found, 3).summary.qualified_type, "witsml21.Channel");
        assert_eq!(
            entry(&found, 3).summary.contained_in,
            vec![uri("eml23.Aggregate", 1)]
        );
        assert!(found.entries.iter().all(|e| e.summary.uuid != id(4)));
        assert_eq!(found.diagnostics.len(), 1, "{:?}", found.diagnostics);
        assert!(found.diagnostics[0].message.contains("no xsi:type"));
    }

    // ---- The index --------------------------------------------------------

    fn parts(items: &[(&str, String)]) -> HashMap<String, Vec<u8>> {
        items
            .iter()
            .map(|(locator, xml)| (locator.to_string(), xml.clone().into_bytes()))
            .collect()
    }

    fn build(
        summaries: &[ObjectSummary],
        documents: &HashMap<String, Vec<u8>>,
        part_uuids: &HashSet<String>,
    ) -> (ContainedIndex, Vec<Diagnostic>, Vec<String>) {
        let registry = registry();
        let mut read = Vec::new();
        let (index, diagnostics) = ContainedIndex::build(
            summaries,
            &registry,
            &|uuid| part_uuids.contains(uuid),
            &mut |locator| {
                read.push(locator.to_string());
                documents
                    .get(locator)
                    .cloned()
                    .ok_or_else(|| Error::Package(format!("no part `{locator}`")))
            },
        );
        (index, diagnostics, read)
    }

    /// A package that holds no container reads no part to build the index, and
    /// a mixed one reads the containers only. The contract of the lot
    /// (CONT-2/CONT-10): RESQML 2.0.1 opens not one part more.
    ///
    /// Break it: scan every part, and `read` lists the RESQML part and the
    /// `Wellbore`.
    #[test]
    fn only_container_parts_are_read() {
        let mut registry = registry();
        energyml_resqml_2_0_1::register(&mut registry);
        let resqml = summary("resqml20.obj_TriangulatedSetRepresentation", 10, "tsr.xml");
        let wellbore = summary("witsml21.Wellbore", 11, "wellbore.xml");
        let log_summary = summary("witsml21.Log", 1, "log.xml");

        let read: RefCell<Vec<String>> = RefCell::new(Vec::new());
        let mut reader = |locator: &str| -> Result<Vec<u8>> {
            read.borrow_mut().push(locator.to_string());
            Ok(log(1, "Log", &[channel_set(2, "A", None, &[channel(3, "GR")])]).into_bytes())
        };

        let (index, _) = ContainedIndex::build([&resqml, &wellbore], &registry, &|_| false, &mut reader);
        assert!(index.is_empty());
        assert!(index.scanned_parts().is_empty());
        assert!(
            read.borrow().is_empty(),
            "nothing is a container, nothing is opened: {:?}",
            read.borrow()
        );

        let (index, _) = ContainedIndex::build(
            [&resqml, &log_summary, &wellbore],
            &registry,
            &|_| false,
            &mut reader,
        );
        assert_eq!(*read.borrow(), vec!["log.xml".to_string()]);
        assert_eq!(index.scanned_parts(), ["log.xml".to_string()]);
        assert_eq!(index.entries().len(), 2);
    }

    /// The index answers by uuid in any case, and archive order is kept for
    /// listings.
    ///
    /// Break it: key `ranges` on the uuid as written, and an upper-case query finds nothing.
    #[test]
    fn the_index_answers_by_uuid_in_any_case_and_keeps_archive_order() {
        let xml = log(
            1,
            "Log",
            &[
                channel_set(9, "A", None, &[channel(3, "GR")]),
                channel_set(2, "B", None, &[]),
            ],
        );
        let documents = parts(&[("log.xml", xml)]);
        let (index, diagnostics, _) = build(
            &[summary("witsml21.Log", 1, "log.xml")],
            &documents,
            &HashSet::new(),
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(index.by_uuid(&id(3).to_ascii_uppercase()).len(), 1);
        assert!(index.by_uuid(&id(77)).is_empty());
        let archive: Vec<&str> = index
            .in_archive_order()
            .map(|e| e.summary.uuid.as_str())
            .collect();
        assert_eq!(
            archive,
            [id(9), id(3), id(2)]
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
        );
    }

    /// A contained object whose uuid is a part of the archive is the part's:
    /// not in the index, kept apart for its edge, with a note. The rule of
    /// `graph::nested_index`.
    ///
    /// Break it: drop the `is_part` test, and the `Channel` is listed twice,
    /// as a part and as a copy.
    #[test]
    fn a_part_of_the_same_uuid_wins() {
        let xml = log(1, "Log", &[channel_set(2, "A", None, &[channel(3, "GR")])]);
        let documents = parts(&[("log.xml", xml)]);
        let part_uuids: HashSet<String> = [id(3)].into_iter().collect();
        let (index, diagnostics, _) =
            build(&[summary("witsml21.Log", 1, "log.xml")], &documents, &part_uuids);
        assert!(index.by_uuid(&id(3)).is_empty());
        assert_eq!(index.shadowed().len(), 1);
        assert_eq!(
            index.shadowed()[0].summary.contained_in,
            vec![uri("witsml21.ChannelSet", 2)],
            "the edge to its container is not lost"
        );
        assert_eq!(index.entries().len(), 1, "the ChannelSet remains");
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("the part of that uuid is the object"))
        );
    }

    /// The same object held by containers of two parts is one entry, read from
    /// the first part, held by both containers.
    ///
    /// Break it: do not merge across parts, and there are two entries.
    #[test]
    fn the_same_object_in_two_parts_is_one_entry_held_by_both_containers() {
        let first = log(1, "L1", &[channel_set(2, "A", None, &[channel(3, "GR")])]);
        let second = log(10, "L2", &[channel_set(11, "B", None, &[channel(3, "GR")])]);
        let documents = parts(&[("l1.xml", first), ("l2.xml", second)]);
        let (index, diagnostics, _) = build(
            &[
                summary("witsml21.Log", 1, "l1.xml"),
                summary("witsml21.Log", 10, "l2.xml"),
            ],
            &documents,
            &HashSet::new(),
        );
        let shared = index.by_uuid(&id(3));
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0].part, "l1.xml");
        assert_eq!(shared[0].summary.contained_in.len(), 2);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("read from the first"))
        );
    }

    /// One uuid under two types is two entries and a warning naming both.
    /// Never a fusion.
    ///
    /// Break it: key on the uuid alone, and one of the two disappears.
    #[test]
    fn one_uuid_under_two_types_is_two_entries_and_a_warning() {
        // A ChannelSet and a Channel written with the same uuid 5.
        let xml = log(
            1,
            "Log",
            &[
                channel_set(5, "A", None, &[]),
                channel_set(2, "B", None, &[channel(5, "GR")]),
            ],
        );
        let documents = parts(&[("log.xml", xml)]);
        let (index, diagnostics, _) = build(
            &[summary("witsml21.Log", 1, "log.xml")],
            &documents,
            &HashSet::new(),
        );
        let both = index.by_uuid(&id(5));
        assert_eq!(both.len(), 2);
        let mut kinds: Vec<&str> = both.iter().map(|e| e.summary.qualified_type.as_str()).collect();
        kinds.sort();
        assert_eq!(kinds, ["witsml21.Channel", "witsml21.ChannelSet"]);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.severity == crate::error::Severity::Warning
                    && d.message.contains("neither is fused")),
            "{diagnostics:?}"
        );
    }

    /// A part that cannot be read costs its own contents and a warning, not
    /// the rest of the index.
    ///
    /// Break it: propagate the error, and the good part is lost too.
    #[test]
    fn an_unreadable_container_is_a_warning_not_a_failure() {
        let good = log(10, "L2", &[channel_set(11, "B", None, &[])]);
        let documents = parts(&[("good.xml", good)]);
        let (index, diagnostics, _) = build(
            &[
                summary("witsml21.Log", 1, "missing.xml"),
                summary("witsml21.Log", 10, "good.xml"),
            ],
            &documents,
            &HashSet::new(),
        );
        assert_eq!(index.entries().len(), 1);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].part, "missing.xml");
    }

    /// A part cut inside an element is an error of the scan, not a short list.
    ///
    /// Break it: accept the end of input anywhere, and a truncated part lists
    /// the objects before the cut as if it were whole.
    #[test]
    fn a_truncated_part_is_refused() {
        let xml = log(1, "Log", &[channel_set(2, "A", None, &[])]);
        let cut = &xml[..xml.find("</witsml:ChannelSet>").expect("a closing tag")];
        let registry = registry();
        let container = summary("witsml21.Log", 1, "log.xml");
        let tables = registry.relations_for("witsml21.Log").expect("a table");
        let outcome = scan_part(cut.as_bytes(), &container, tables, &registry);
        assert!(matches!(outcome, Err(Error::Xml(_))), "{outcome:?}");
    }
}
