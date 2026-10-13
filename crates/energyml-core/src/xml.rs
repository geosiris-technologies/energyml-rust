//! XML reading and writing for generated types.
//!
//! Serde is not used for XML. Its data model cannot express namespace prefixes,
//! and EnergyML documents must interoperate with fesapi and other Energistics
//! tooling, so prefixes are managed explicitly here instead.
//!
//! [`XmlSerialize`] is deliberately object-safe (children are written through
//! `&mut XmlWriter`, not a generic `W: Write`) so a `Box<dyn Reflect>` coming
//! out of the registry can still be serialized.

use std::borrow::Cow;
use std::fmt;
use std::io::{BufRead, Write};

use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};

use crate::error::{Error, Result};
use crate::ns::{self, Namespace};

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Write a value as the content of an XML element.
///
/// The element name is supplied by the caller, because the same type is written
/// under different names depending on the field that holds it.
pub trait XmlSerialize {
    /// Push this value's XML attributes onto the (still open) start tag.
    fn xml_attributes(&self, tag: &mut BytesStart<'_>) -> Result<()> {
        let _ = tag;
        Ok(())
    }

    /// Write child elements.
    fn xml_children(&self, w: &mut XmlWriter<'_>) -> Result<()> {
        let _ = w;
        Ok(())
    }

    /// Text content, for types built on `xsd:simpleContent` (every measure type
    /// in the model, e.g. `<Value uom="m">3.4</Value>`).
    fn xml_text(&self) -> Option<Cow<'_, str>> {
        None
    }
}

/// Read a value from an XML element whose start tag has already been consumed.
///
/// Implementations must consume events up to and including the matching end
/// tag.
pub trait XmlDeserialize: Sized {
    /// Read the element body. `start` is the start tag, still available for its
    /// attributes and its `xsi:type`.
    fn xml_read(r: &mut XmlReader<'_>, start: &BytesStart<'_>) -> Result<Self>;
}

/// Conversion to and from the lexical form used in attributes, text content and
/// enumeration values.
pub trait XmlText: Sized {
    /// Lexical representation.
    fn to_xml_text(&self) -> Cow<'_, str>;
    /// Parse from the lexical representation.
    fn from_xml_text(s: &str) -> Result<Self>;
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

/// Options controlling serialized output.
#[derive(Clone, Copy, Debug)]
pub struct XmlWriteOptions {
    /// Emit the `<?xml ... ?>` declaration.
    pub declaration: bool,
    /// Indent nested elements by this many spaces. `None` writes compact XML.
    pub indent: Option<usize>,
}

impl Default for XmlWriteOptions {
    fn default() -> Self {
        Self {
            declaration: true,
            indent: Some(2),
        }
    }
}

impl XmlWriteOptions {
    /// No declaration, no indentation - the form used for byte-exact round trips.
    pub const COMPACT: XmlWriteOptions = XmlWriteOptions {
        declaration: false,
        indent: None,
    };
}

/// Writes EnergyML XML with explicit namespace prefixes.
pub struct XmlWriter<'a> {
    w: quick_xml::Writer<&'a mut (dyn Write + 'a)>,
    pending_ns: Vec<Namespace>,
    root_written: bool,
}

impl<'a> XmlWriter<'a> {
    /// Create a writer over any sink.
    pub fn new(out: &'a mut (dyn Write + 'a), opts: XmlWriteOptions) -> Self {
        let w = match opts.indent {
            Some(n) => quick_xml::Writer::new_with_indent(out, b' ', n),
            None => quick_xml::Writer::new(out),
        };
        Self {
            w,
            pending_ns: Vec::new(),
            root_written: false,
        }
    }

    /// Declare a namespace on the root element.
    ///
    /// Declarations queued before the first element are emitted on it; anything
    /// queued later is ignored, which keeps output stable.
    pub fn declare(&mut self, namespace: Namespace) -> &mut Self {
        if !self.root_written && !self.pending_ns.iter().any(|n| n.uri == namespace.uri) {
            self.pending_ns.push(namespace);
        }
        self
    }

    /// Write the XML declaration.
    pub fn write_declaration(&mut self) -> Result<()> {
        self.w.write_event(Event::Decl(quick_xml::events::BytesDecl::new(
            "1.0",
            Some("UTF-8"),
            None,
        )))?;
        Ok(())
    }

    /// Write pre-formed XML into the current element, untouched.
    ///
    /// The escape hatch [`crate::any_xml::AnyXml`] uses to put back a subtree
    /// nobody modelled. Nothing else should need it: everywhere else the
    /// writer owns the escaping, and this bypasses it.
    pub fn write_raw(&mut self, xml: &str) -> Result<()> {
        use std::io::Write;
        self.w.get_mut().write_all(xml.as_bytes())?;
        Ok(())
    }

    /// Write one element with the given namespace, local name and content.
    pub fn write_element(
        &mut self,
        namespace: Namespace,
        local: &str,
        value: &dyn XmlSerialize,
    ) -> Result<()> {
        let qname = namespace.qualify(local);
        let mut start = BytesStart::new(qname.clone());

        if !self.root_written {
            self.root_written = true;
            let decls: Vec<(String, &'static str)> = std::mem::take(&mut self.pending_ns)
                .into_iter()
                .map(|n| (format!("xmlns:{}", n.prefix), n.uri))
                .collect();
            for (k, v) in &decls {
                start.push_attribute((k.as_str(), *v));
            }
        }

        value.xml_attributes(&mut start)?;
        self.w.write_event(Event::Start(start))?;

        if let Some(text) = value.xml_text() {
            self.w.write_event(Event::Text(BytesText::new(text.as_ref())))?;
        }
        value.xml_children(self)?;

        self.w.write_event(Event::End(BytesEnd::new(qname)))?;
        Ok(())
    }

    /// Write an optional field, skipping it when absent.
    pub fn write_optional<T: XmlSerialize>(
        &mut self,
        namespace: Namespace,
        local: &str,
        value: &Option<T>,
    ) -> Result<()> {
        match value {
            Some(v) => self.write_element(namespace, local, v),
            None => Ok(()),
        }
    }

    /// Write every element of a repeated field.
    pub fn write_all<T: XmlSerialize>(
        &mut self,
        namespace: Namespace,
        local: &str,
        values: &[T],
    ) -> Result<()> {
        for v in values {
            self.write_element(namespace, local, v)?;
        }
        Ok(())
    }
}

/// Push an attribute onto a start tag, skipping `None`.
pub fn push_attr<T: XmlText>(tag: &mut BytesStart<'_>, name: &str, value: &Option<T>) {
    if let Some(v) = value {
        tag.push_attribute((name, v.to_xml_text().as_ref()));
    }
}

/// Push a mandatory attribute onto a start tag.
pub fn push_attr_required<T: XmlText>(tag: &mut BytesStart<'_>, name: &str, value: &T) {
    tag.push_attribute((name, value.to_xml_text().as_ref()));
}

/// Push the `xsi:type` attribute used to select a concrete subtype.
pub fn push_xsi_type(tag: &mut BytesStart<'_>, xsi_type: &str) {
    tag.push_attribute(("xsi:type", xsi_type));
}

/// Serialize a value to a `String`.
pub fn to_xml_string(
    value: &dyn XmlSerialize,
    namespace: Namespace,
    local: &str,
    declare: &[Namespace],
    opts: XmlWriteOptions,
) -> Result<String> {
    let mut out: Vec<u8> = Vec::new();
    {
        let mut w = XmlWriter::new(&mut out, opts);
        if opts.declaration {
            w.write_declaration()?;
        }
        for n in declare {
            w.declare(*n);
        }
        w.declare(namespace);
        w.declare(ns::XSI);
        w.write_element(namespace, local, value)?;
    }
    Ok(String::from_utf8(out)?)
}

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

/// An event from [`XmlReader`], owned so generated code is free of lifetimes.
///
/// `Empty` elements are normalized into a `Start` followed by a synthetic `End`,
/// so every consumer can use a single loop shape.
#[derive(Debug)]
pub enum XmlEvent {
    /// Element start tag, with the namespace URI its name resolved to.
    ///
    /// The URI matters: WITSML objects declare their own `witsml:Citation`
    /// alongside the `eml:Citation` they inherit from `AbstractObject`. Matching
    /// on the local name alone silently reads one into the other.
    Start(BytesStart<'static>, Option<String>),
    /// Element end tag.
    End(BytesEnd<'static>),
    /// Unescaped text or CDATA content.
    Text(String),
    /// End of document.
    Eof,
}

impl XmlEvent {
    /// Namespace URI of a start tag, empty for anything else.
    pub fn namespace(&self) -> &str {
        match self {
            XmlEvent::Start(_, Some(uri)) => uri.as_str(),
            _ => "",
        }
    }
}

/// A required attribute or element that was absent, and the default that was
/// put in its place by a [lenient][`XmlReader::lenient`] read.
///
/// Every field borrows: the names come from the generated `TypeInfo`
/// constants, so recording one allocates nothing even when a package produces
/// thousands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Defaulted {
    /// `"attribute"`, `"element"` or `"text content"`.
    pub kind: &'static str,
    /// Name of the item that was absent.
    pub name: &'static str,
    /// Element that should have contained it.
    pub parent: &'static str,
}

impl fmt::Display for Defaulted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "required {} `{}` in <{}> was absent; read as its default",
            self.kind, self.name, self.parent
        )
    }
}

/// A value that **was** present and could not be read, and what was put in its
/// place by a [lenient][`XmlReader::lenient`] read.
///
/// Distinct from [`Defaulted`], and the distinction is the whole point: an
/// absent element is a producer forgetting something, while a value of
/// `hello` where a number belongs is a producer writing something *wrong*.
/// The second is worse, so it carries what was found - which means it
/// allocates, and it allocates only for documents that are broken.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Repaired {
    /// `"enumeration"`, `"number"`, `"boolean"`, `"xsi:type"` or `"entity"`.
    pub kind: &'static str,
    /// The type or item whose value it was.
    pub name: String,
    /// What the document said.
    pub found: String,
    /// What was used instead.
    pub used: String,
    /// Why it could not be taken as written.
    pub reason: String,
}

impl fmt::Display for Repaired {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} `{}`: `{}` could not be read ({}); used `{}` instead",
            self.kind, self.name, self.found, self.reason, self.used
        )
    }
}

/// Reads EnergyML XML, resolving namespace prefixes as it goes.
pub struct XmlReader<'a> {
    r: quick_xml::NsReader<&'a mut (dyn BufRead + 'a)>,
    buf: Vec<u8>,
    pending_end: Option<BytesEnd<'static>>,
    /// The event that ended a text run, held back until the text has been
    /// delivered. See [`XmlReader::next_event`].
    pending: Option<XmlEvent>,
    /// Repair a value that is **there and wrong**. Off by default: see
    /// [`XmlReader::lenient`].
    lenient: bool,
    /// Fill an item the schema requires and the document does **not carry**.
    /// On by default: see [`XmlReader::strict`].
    absent_defaults: bool,
    /// What leniency had to invent, in document order. Empty in strict mode,
    /// and empty in lenient mode over a conformant document.
    defaulted: Vec<Defaulted>,
    /// What leniency had to *replace*: values that were there and wrong.
    repaired: Vec<Repaired>,
}

impl<'a> XmlReader<'a> {
    /// Create a reader over any buffered source.
    pub fn new(input: &'a mut (dyn BufRead + 'a)) -> Self {
        let mut r = quick_xml::NsReader::from_reader(input);
        // Trimming per event would eat the spaces *around* an entity, since
        // `Fault &amp; Horizon` arrives as three events. The run is trimmed
        // once, whole, in `next_event`.
        r.config_mut().trim_text(false);
        r.config_mut().expand_empty_elements = false;
        Self {
            r,
            buf: Vec::new(),
            pending_end: None,
            pending: None,
            lenient: false,
            absent_defaults: true,
            defaulted: Vec::new(),
            repaired: Vec::new(),
        }
    }

    /// Repair a value that is present and unreadable, instead of refusing the
    /// document.
    ///
    /// This is the **second** of two tolerances, and the two are deliberately
    /// separate. An item the document does not carry is handled without asking -
    /// see [`strict`][Self::strict] for why, and for how to turn that off.
    /// What this switch adds is repair of what the document *does* carry and
    /// gets wrong.
    ///
    /// **Values that are there and wrong**: a closed enumeration outside its
    /// list, a number that is not a number, a boolean spelled `yes`, an entity
    /// nothing defines, an `xsi:type` naming an unknown subtype of a *concrete*
    /// base. Each takes the default - or the base - and is recorded as
    /// [`Repaired`], **with the string the document carried**, since that is
    /// the only record of what the producer wrote.
    ///
    /// It is off by default because the two are not the same news. Silence
    /// means the producer forgot; `hello` where a number belongs means the
    /// producer wrote something, and dropping it loses information that was in
    /// the file. A caller should have to ask for that.
    ///
    /// **Covered by neither**: a malformed document, bad encoding, and an
    /// `xsi:type` naming an unknown subtype of a purely abstract choice. The
    /// last is the interesting one: with no concrete base there is nothing to
    /// fall back *to*, and picking some other subtype would put an object in
    /// memory that the document does not describe. Those are errors in every
    /// mode.
    #[must_use]
    pub fn lenient(mut self) -> Self {
        self.lenient = true;
        self
    }

    /// Whether values that are present and unreadable are being repaired.
    pub fn is_lenient(&self) -> bool {
        self.lenient
    }

    /// Refuse a document that omits something the schema requires.
    ///
    /// Off by default, and that default is a deliberate reversal.
    ///
    /// # Why absence is not an error here
    ///
    /// A parser's job is to say what the document says. Whether the document
    /// says everything it *should* is a different question, this workspace
    /// already answers it - [`crate::validate::Validate`], on the object, with
    /// a path and a rule - and answering it twice, once in a place that can
    /// only answer by refusing, costs everything the document did carry.
    ///
    /// The arithmetic is stark. A package of 1618 objects whose producer
    /// omitted `Citation.Originator` - one string, that nothing downstream
    /// reads - was **1618 objects unreadable**: no geometry, no references, no
    /// titles, no export, no JSON. Every byte of real content in that file was
    /// held hostage to a field nobody wanted. Refusing there does not protect a
    /// caller from bad data; it hands them nothing and calls it safety.
    ///
    /// What is filled is never silent: each one is recorded as [`Defaulted`],
    /// and the layers above turn them into diagnostics. A caller that will
    /// **write the object back** is the one that has to care - an absent
    /// required element returns as an empty string or a zero, and saving that
    /// puts on disk a value nobody wrote. That caller is the one that should
    /// ask for strictness, and now it can.
    #[must_use]
    pub fn strict(mut self) -> Self {
        self.absent_defaults = false;
        self
    }

    /// Whether an absent required item takes its default.
    pub fn is_absent_defaulting(&self) -> bool {
        self.absent_defaults
    }

    /// What leniency invented so far, in document order.
    pub fn defaulted(&self) -> &[Defaulted] {
        &self.defaulted
    }

    /// Take what leniency invented, leaving the reader empty.
    pub fn take_defaulted(&mut self) -> Vec<Defaulted> {
        std::mem::take(&mut self.defaulted)
    }

    /// What leniency had to replace so far, in document order.
    pub fn repaired(&self) -> &[Repaired] {
        &self.repaired
    }

    /// Take what leniency replaced, leaving the reader empty.
    pub fn take_repaired(&mut self) -> Vec<Repaired> {
        std::mem::take(&mut self.repaired)
    }

    /// Record a replacement. Called by generated code and by this module.
    pub fn record_repair(&mut self, repaired: Repaired) {
        self.repaired.push(repaired);
    }

    /// Parse a lexical value, or - in lenient mode - replace what cannot be
    /// parsed and say so.
    ///
    /// This is where leniency stops being about cardinality. A closed
    /// enumeration outside its list, a number that is not a number, a boolean
    /// spelled `yes`: strictly each of these is a refusal, and strictly that is
    /// right, because the value is *wrong* rather than missing. But for a tool
    /// whose job is to open a broken file and repair it, refusing the whole
    /// object over one bad word loses the ninety-nine that were fine.
    ///
    /// So in lenient mode the field takes its default and the departure is
    /// recorded **with the string that was there** - the only record of what
    /// the producer actually wrote, and the thing a repair needs.
    pub fn value<T: XmlText + Default>(&mut self, text: &str, kind: &'static str, name: &str) -> Result<T> {
        match T::from_xml_text(text) {
            Ok(value) => Ok(value),
            Err(e) if self.lenient => {
                let used = T::default();
                self.repaired.push(Repaired {
                    kind,
                    name: name.to_string(),
                    found: text.to_string(),
                    used: used.to_xml_text().into_owned(),
                    reason: short_reason(&e),
                });
                Ok(used)
            }
            Err(e) => Err(e),
        }
    }

    /// A required attribute, parsed through [`XmlReader::value`].
    pub fn attr_required<T: XmlText + Default>(
        &mut self,
        start: &BytesStart<'_>,
        name: &str,
        parent: &'static str,
    ) -> Result<T> {
        match attr_value(start, name)? {
            Some(text) => self.value(text.as_str(), "attribute", name),
            None if self.absent_defaults => {
                self.defaulted.push(Defaulted {
                    kind: "attribute",
                    name: leak(name),
                    parent,
                });
                Ok(T::default())
            }
            None => Err(Error::MissingRequired {
                kind: "attribute",
                name: name.to_string(),
                parent: parent.to_string(),
            }),
        }
    }

    /// An optional attribute, parsed through [`XmlReader::value`].
    ///
    /// An attribute that is present and unparseable is **not** the same as an
    /// absent one: strictly it fails, and leniently it is repaired and
    /// recorded. Reading it as `None` would erase the difference.
    pub fn attr_optional<T: XmlText + Default>(
        &mut self,
        start: &BytesStart<'_>,
        name: &str,
    ) -> Result<Option<T>> {
        match attr_value(start, name)? {
            Some(text) => Ok(Some(self.value(text.as_str(), "attribute", name)?)),
            None => Ok(None),
        }
    }

    /// Resolve a required field from the slot the deserializer filled.
    ///
    /// Called once per required field by generated code - around a thousand
    /// impls - so the body lives here and each call site stays one line.
    pub fn required<T: Default>(
        &mut self,
        slot: Option<T>,
        kind: &'static str,
        name: &'static str,
        parent: &'static str,
    ) -> Result<T> {
        match slot {
            Some(value) => Ok(value),
            None if self.absent_defaults => {
                self.defaulted.push(Defaulted { kind, name, parent });
                Ok(T::default())
            }
            None => Err(Error::MissingRequired {
                kind,
                name: name.to_string(),
                parent: parent.to_string(),
            }),
        }
    }

    /// Next meaningful event. Declarations, comments and processing
    /// instructions are skipped.
    ///
    /// Text is **coalesced**: quick-xml reports `&amp;` as its own
    /// `GeneralRef` event and splits the surrounding characters into separate
    /// `Text` events, so `Fault &amp; Horizon` arrives in three pieces. They
    /// are joined, entities resolved, and delivered as one `Text` - otherwise
    /// every title holding an ampersand comes back mangled.
    pub fn next_event(&mut self) -> Result<XmlEvent> {
        if let Some(event) = self.pending.take() {
            return Ok(event);
        }
        if let Some(end) = self.pending_end.take() {
            return Ok(XmlEvent::End(end));
        }
        // Text accumulated so far in the current run.
        let mut text = String::new();
        loop {
            self.buf.clear();
            let (resolved, event) = self.r.read_resolved_event_into(&mut self.buf)?;
            let namespace = match resolved {
                quick_xml::name::ResolveResult::Bound(ns) => {
                    Some(String::from_utf8_lossy(ns.as_ref()).into_owned())
                }
                _ => None,
            };

            // A text-ish event extends the run; anything else ends it.
            let structural = match event {
                Event::Text(e) => {
                    // 0.41 splits this into EOL normalization then entity
                    // unescaping; the reader no longer does either for us.
                    let raw = e.xml10_content()?;
                    text.push_str(&quick_xml::escape::unescape(&raw)?);
                    continue;
                }
                Event::CData(e) => {
                    text.push_str(&String::from_utf8(e.into_inner().into_owned())?);
                    continue;
                }
                // `&amp;`, `&#233;`: their own event since 0.41. Re-form the
                // reference and let the unescaper resolve it, which covers the
                // predefined entities and numeric character references alike.
                Event::GeneralRef(e) => {
                    let name = e.decode()?;
                    let reference = format!("&{name};");
                    match quick_xml::escape::unescape(&reference) {
                        Ok(resolved) => text.push_str(&resolved),
                        // An entity nothing defines. A DOCTYPE is ignored by
                        // this reader, so `&nbsp;` and every other named
                        // entity outside the predefined five arrive here.
                        // Strictly that is a malformed document; leniently the
                        // character becomes U+FFFD - the replacement
                        // character, which is what it is - and the reference
                        // is recorded verbatim so a repair can put back what
                        // was meant.
                        Err(e) if self.lenient => {
                            self.repaired.push(Repaired {
                                kind: "entity",
                                name: name.to_string(),
                                found: reference.clone(),
                                used: "\u{fffd}".to_string(),
                                reason: e.to_string(),
                            });
                            text.push('\u{fffd}');
                        }
                        Err(e) => return Err(e.into()),
                    }
                    continue;
                }
                Event::Start(e) => XmlEvent::Start(e.into_owned(), namespace),
                Event::Empty(e) => {
                    let owned = e.into_owned();
                    self.pending_end = Some(BytesEnd::new(
                        String::from_utf8_lossy(owned.name().as_ref()).into_owned(),
                    ));
                    XmlEvent::Start(owned, namespace)
                }
                Event::End(e) => XmlEvent::End(e.into_owned()),
                Event::Eof => XmlEvent::Eof,
                _ => continue,
            };

            // Whitespace-only runs are indentation between elements, not
            // content - the reader has always dropped those. What survives is
            // trimmed as a whole, which is what the per-event trimming used to
            // achieve for the common case of one event per value.
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return Ok(structural);
            }
            let value = trimmed.to_string();
            self.pending = Some(structural);
            return Ok(XmlEvent::Text(value));
        }
    }

    /// Consume the remainder of the element whose start tag was just read.
    ///
    /// Used for forward compatibility: elements a generated type does not know
    /// about are skipped rather than treated as errors.
    pub fn skip_element(&mut self) -> Result<()> {
        let mut depth = 1usize;
        while depth > 0 {
            match self.next_event()? {
                XmlEvent::Start(..) => depth += 1,
                XmlEvent::End(_) => depth -= 1,
                XmlEvent::Text(_) => {}
                XmlEvent::Eof => return Err(Error::Eof),
            }
        }
        Ok(())
    }

    /// Read the text content of the current element and consume its end tag.
    pub fn read_text(&mut self) -> Result<String> {
        let mut out = String::new();
        let mut depth = 1usize;
        loop {
            match self.next_event()? {
                XmlEvent::Text(t) if depth == 1 => out.push_str(&t),
                XmlEvent::Text(_) => {}
                XmlEvent::Start(..) => depth += 1,
                XmlEvent::End(_) => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(out);
                    }
                }
                XmlEvent::Eof => return Err(Error::Eof),
            }
        }
    }

    /// Advance to the document's root start tag.
    pub fn read_root(&mut self) -> Result<BytesStart<'static>> {
        match self.next_event()? {
            XmlEvent::Start(e, _) => Ok(e),
            XmlEvent::Eof => Err(Error::Eof),
            other => Err(Error::Xml(format!("expected a root element, found {other:?}"))),
        }
    }
}

/// Local name of an element, without its namespace prefix.
pub fn local_name(start: &BytesStart<'_>) -> String {
    String::from_utf8_lossy(start.local_name().as_ref()).into_owned()
}

/// Value of an attribute, matched on local name so any prefix is accepted.
pub fn attr_value(start: &BytesStart<'_>, name: &str) -> Result<Option<String>> {
    for attr in start.attributes() {
        let attr = attr?;
        if attr.key.local_name().as_ref() == name.as_bytes() {
            return Ok(Some(
                attr.normalized_value(quick_xml::XmlVersion::default())?
                    .into_owned(),
            ));
        }
    }
    Ok(None)
}

/// Local part of a QName or qualified type.
///
/// `resqml:PointGeometry`, `resqml22.PointGeometry` and `PointGeometry` all
/// reduce to `PointGeometry` - documents disagree on the prefix, never on this.
pub fn local_part(value: &str) -> &str {
    value.rsplit([':', '.']).next().unwrap_or(value)
}

/// Value of the `xsi:type` attribute, if present.
pub fn xsi_type(start: &BytesStart<'_>) -> Result<Option<String>> {
    attr_value(start, "type")
}

/// Parse a required attribute.
pub fn required_attr<T: XmlText>(start: &BytesStart<'_>, name: &str, parent: &str) -> Result<T> {
    match attr_value(start, name)? {
        Some(v) => T::from_xml_text(&v),
        None => Err(Error::MissingRequired {
            kind: "attribute",
            name: name.to_string(),
            parent: parent.to_string(),
        }),
    }
}

/// Parse an optional attribute.
pub fn optional_attr<T: XmlText>(start: &BytesStart<'_>, name: &str) -> Result<Option<T>> {
    match attr_value(start, name)? {
        Some(v) => Ok(Some(T::from_xml_text(&v)?)),
        None => Ok(None),
    }
}

/// Deserialize from a string.
///
/// An item the document does not carry takes its default; a value that is
/// there and unreadable still refuses. See [`XmlReader::strict`] and
/// [`XmlReader::lenient`] for the two switches, and
/// [`from_xml_str_strict`] for the reading that refuses both.
pub fn from_xml_str<T: XmlDeserialize>(input: &str) -> Result<T> {
    let mut bytes = input.as_bytes();
    let mut reader = XmlReader::new(&mut bytes);
    let root = reader.read_root()?;
    T::xml_read(&mut reader, &root)
}

/// Deserialize from bytes already in memory - the shape a cloud service gets
/// back from a download.
pub fn from_xml_bytes<T: XmlDeserialize>(input: &[u8]) -> Result<T> {
    let mut cursor = input;
    let mut reader = XmlReader::new(&mut cursor);
    let root = reader.read_root()?;
    T::xml_read(&mut reader, &root)
}

/// Deserialize from a string, accepting **only** what the document actually
/// says: an absent required item refuses the document.
///
/// What a caller that will write the object back should use, and what the
/// round-trip check uses. See [`XmlReader::strict`].
pub fn from_xml_str_strict<T: XmlDeserialize>(input: &str) -> Result<T> {
    let mut bytes = input.as_bytes();
    let mut reader = XmlReader::new(&mut bytes).strict();
    let root = reader.read_root()?;
    T::xml_read(&mut reader, &root)
}

/// The same, over bytes.
pub fn from_xml_bytes_strict<T: XmlDeserialize>(input: &[u8]) -> Result<T> {
    let mut cursor = input;
    let mut reader = XmlReader::new(&mut cursor).strict();
    let root = reader.read_root()?;
    T::xml_read(&mut reader, &root)
}

/// Deserialize from a string, repairing values that are there and wrong as
/// well. See [`XmlReader::lenient`].
///
/// The second half of the pair is what had to be invented; an empty one means
/// the document was conformant and the result is identical to
/// [`from_xml_str`].
pub fn from_xml_str_lenient<T: XmlDeserialize>(input: &str) -> Result<(T, Vec<Defaulted>)> {
    from_xml_bytes_lenient(input.as_bytes())
}

/// Deserialize from bytes, filling absent required items with their defaults.
/// See [`from_xml_str_lenient`].
pub fn from_xml_bytes_lenient<T: XmlDeserialize>(input: &[u8]) -> Result<(T, Vec<Defaulted>)> {
    let mut cursor = input;
    let mut reader = XmlReader::new(&mut cursor).lenient();
    let root = reader.read_root()?;
    let object = T::xml_read(&mut reader, &root)?;
    Ok((object, reader.take_defaulted()))
}

// ---------------------------------------------------------------------------
// `xsd:list`
// ---------------------------------------------------------------------------

/// Join the items of an `xsd:list` into its lexical form: values separated by
/// single spaces.
///
/// `eml:FloatingPointXmlArrayList` and its siblings carry inline array data
/// this way - one element holding a whitespace-separated run of values - so the
/// generated field is a `Vec<T>` and this is what puts it back on the wire.
pub fn join_list<T: XmlText>(items: &[T]) -> String {
    let mut out = String::new();
    for item in items {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(item.to_xml_text().as_ref());
    }
    out
}

/// The gist of a parse error, for a one-line record.
fn short_reason(error: &Error) -> String {
    let text = error.to_string();
    match text.find(" (") {
        Some(at) => text[..at].to_string(),
        None => text,
    }
}

/// A name that has to outlive the read, for a `Defaulted` record.
///
/// Attribute names come from generated `&'static str` constants in every real
/// call, so this leaks nothing in practice; it exists so the one path that
/// receives a borrowed name still records rather than staying silent.
fn leak(name: &str) -> &'static str {
    // Interned once per distinct name, so a package with a thousand bad
    // attributes of the same name leaks one string, not a thousand.
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    static NAMES: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let mut names = NAMES
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .expect("the name table lock is never held across a panic");
    if let Some(known) = names.get(name) {
        return known;
    }
    let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
    names.insert(leaked);
    leaked
}

/// Split the lexical form of an `xsd:list` and parse every item.
///
/// XSD collapses any run of whitespace - spaces, tabs, newlines - so a value
/// wrapped over several lines in a file reads the same as one written on a
/// single line. Empty or whitespace-only text is an empty list rather than an
/// error: how many items are required is a cardinality question, and that
/// belongs to validation.
pub fn split_list<T: XmlText>(text: &str) -> Result<Vec<T>> {
    text.split_whitespace().map(T::from_xml_text).collect()
}

// ---------------------------------------------------------------------------
// Scalars
// ---------------------------------------------------------------------------

macro_rules! impl_xml_scalar {
    ($ty:ty, $name:literal) => {
        impl XmlText for $ty {
            fn to_xml_text(&self) -> Cow<'_, str> {
                Cow::Owned(self.to_string())
            }
            fn from_xml_text(s: &str) -> Result<Self> {
                s.trim()
                    .parse::<$ty>()
                    .map_err(|e| Error::parse(s, $name, e.to_string()))
            }
        }
        impl XmlSerialize for $ty {
            fn xml_text(&self) -> Option<Cow<'_, str>> {
                Some(XmlText::to_xml_text(self))
            }
        }
        impl XmlDeserialize for $ty {
            fn xml_read(r: &mut XmlReader<'_>, _start: &BytesStart<'_>) -> Result<Self> {
                let text = r.read_text()?;
                r.value::<$ty>(&text, "number", $name)
            }
        }
    };
}

impl_xml_scalar!(i8, "byte");
impl_xml_scalar!(i16, "short");
impl_xml_scalar!(i32, "int");
impl_xml_scalar!(i64, "long");
impl_xml_scalar!(u8, "unsignedByte");
impl_xml_scalar!(u16, "unsignedShort");
impl_xml_scalar!(u32, "unsignedInt");
impl_xml_scalar!(u64, "unsignedLong");
impl_xml_scalar!(f32, "float");
impl_xml_scalar!(f64, "double");

impl XmlText for bool {
    fn to_xml_text(&self) -> Cow<'_, str> {
        Cow::Borrowed(if *self { "true" } else { "false" })
    }
    fn from_xml_text(s: &str) -> Result<Self> {
        // XSD allows the numeric spellings as well as true/false.
        match s.trim() {
            "true" | "1" => Ok(true),
            "false" | "0" => Ok(false),
            other => Err(Error::parse(other, "boolean", "expected true, false, 1 or 0")),
        }
    }
}

impl XmlSerialize for bool {
    fn xml_text(&self) -> Option<Cow<'_, str>> {
        Some(XmlText::to_xml_text(self))
    }
}

impl XmlDeserialize for bool {
    fn xml_read(r: &mut XmlReader<'_>, _start: &BytesStart<'_>) -> Result<Self> {
        let text = r.read_text()?;
        r.value::<bool>(&text, "boolean", "boolean")
    }
}

impl XmlText for String {
    fn to_xml_text(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.as_str())
    }
    fn from_xml_text(s: &str) -> Result<Self> {
        Ok(s.to_string())
    }
}

impl XmlSerialize for String {
    fn xml_text(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(self.as_str()))
    }
}

impl XmlDeserialize for String {
    fn xml_read(r: &mut XmlReader<'_>, _start: &BytesStart<'_>) -> Result<Self> {
        r.read_text()
    }
}

impl<T: XmlSerialize> XmlSerialize for Box<T> {
    fn xml_attributes(&self, tag: &mut BytesStart<'_>) -> Result<()> {
        (**self).xml_attributes(tag)
    }
    fn xml_children(&self, w: &mut XmlWriter<'_>) -> Result<()> {
        (**self).xml_children(w)
    }
    fn xml_text(&self) -> Option<Cow<'_, str>> {
        (**self).xml_text()
    }
}

impl<T: XmlDeserialize> XmlDeserialize for Box<T> {
    fn xml_read(r: &mut XmlReader<'_>, start: &BytesStart<'_>) -> Result<Self> {
        Ok(Box::new(T::xml_read(r, start)?))
    }
}
