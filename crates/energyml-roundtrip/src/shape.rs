//! The comparable shape of an XML document.
//!
//! Comparing two serializations byte for byte would fail on things that carry
//! no meaning: the prefix bound to a namespace, the order of attributes, `2.0`
//! written as `2`, a line break inside an array of numbers. What must not
//! change is the *information*: which elements exist, in which namespace, how
//! many times, with which attributes and which values.
//!
//! So a document is reduced to three multisets keyed by element path - the
//! elements themselves, their attributes, their text - and two shapes are
//! compared entry by entry. A missing entry on the round-tripped side is a
//! loss; an extra one is an invention. Both are reported.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use quick_xml::events::Event;
use quick_xml::name::ResolveResult;

/// One document, reduced to what a reader must preserve.
#[derive(Debug, Default)]
pub struct Shape {
    /// `{uri}Local/{uri}Local/…` → how many times that path occurs.
    pub elements: BTreeMap<String, usize>,
    /// `path@attribute` → the values seen, sorted.
    pub attributes: BTreeMap<String, Vec<String>>,
    /// `path` → the text values seen, sorted.
    pub texts: BTreeMap<String, Vec<String>>,
    /// Namespace URIs actually used by an element or an attribute. Prefixes are
    /// deliberately not recorded: they are a spelling, not information.
    pub namespaces: Vec<String>,
    /// `{uri}Local` of the root element.
    pub root: String,
    /// The root's `schemaVersion`, when it carries one. Every version of a
    /// package shares one namespace URI, so this is the only thing in the
    /// document that says which release wrote it.
    pub schema_version: Option<String>,
}

/// One difference between two shapes.
#[derive(Debug, Clone)]
pub struct Diff {
    /// What kind of difference.
    pub kind: DiffKind,
    /// Where, as an element path.
    pub path: String,
    /// What the original document had.
    pub expected: String,
    /// What the round trip produced.
    pub actual: String,
}

/// The kinds of difference, worst first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiffKind {
    /// An element present in the original is missing or rarer after the round trip.
    LostElement,
    /// An attribute present in the original is missing after the round trip.
    LostAttribute,
    /// A text value present in the original is missing after the round trip.
    LostText,
    /// A namespace the original used is absent after the round trip.
    LostNamespace,
    /// The root element came back under a different name.
    ///
    /// Not a loss: RESQML 2.0.1 declares `<element name="DiscreteProperty"
    /// type="obj_DiscreteProperty"/>` and files in the wild use either spelling
    /// as the root tag. Writing the one the schema declares is a correction.
    RootRenamed,
    /// An `xsi:type` naming the element's own declared type was not rewritten.
    ///
    /// Not a loss. fesapi writes one on *every* element, down to
    /// `xsi:type="xsd:string"` on a title; they are redundant by construction
    /// and any conformant reader ignores them. A substitution that really does
    /// select a subtype cannot hide here: dropping it would take the subtype's
    /// own children with it, and those show up as `LostElement`.
    XsiTypeDropped,
    /// The round trip produced an element the original did not have.
    AddedElement,
    /// The round trip produced an attribute the original did not have.
    AddedAttribute,
    /// The round trip produced a text value the original did not have.
    AddedText,
}

impl DiffKind {
    /// Whether this difference means information was lost.
    pub fn is_loss(&self) -> bool {
        matches!(
            self,
            DiffKind::LostElement | DiffKind::LostAttribute | DiffKind::LostText | DiffKind::LostNamespace
        )
    }

    /// Short label for the report.
    pub fn label(&self) -> &'static str {
        match self {
            DiffKind::LostElement => "element lost",
            DiffKind::LostAttribute => "attribute lost",
            DiffKind::LostText => "value lost",
            DiffKind::LostNamespace => "namespace lost",
            DiffKind::XsiTypeDropped => "xsi:type not rewritten",
            DiffKind::RootRenamed => "root tag renamed",
            DiffKind::AddedElement => "element added",
            DiffKind::AddedAttribute => "attribute added",
            DiffKind::AddedText => "value added",
        }
    }
}

/// Attributes that say nothing about the data.
///
/// `xmlns` declarations bind prefixes, which the comparison ignores by design.
/// `schemaLocation` is a hint to a validator, not content - and a writer that
/// does not emit one is not losing anything.
fn is_noise(qname: &str) -> bool {
    qname == "xmlns"
        || qname.starts_with("xmlns:")
        || qname.ends_with(":schemaLocation")
        || qname.ends_with(":noNamespaceSchemaLocation")
}

/// Collapse runs of whitespace, so a value wrapped over several lines compares
/// equal to the same value on one line. This is what XSD does to a list.
fn collapse(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The XML Schema instance namespace, whose `type` attribute is special.
const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";

/// The namespace prefixes in scope, innermost element last.
///
/// One frame per open element, holding what that element declared, so a
/// re-binding stops applying when its subtree ends. The default namespace is
/// kept under the empty prefix, and it has to be kept: an unprefixed *QName
/// value* - `xsi:type="IntegerHdf5Array"` under `xmlns="…/resqmlv2"` - names
/// the default namespace, unlike an unprefixed attribute *name*, which names
/// none. Missing that read every such `xsi:type` as a different type from the
/// one we write back and reported it as lost.
#[derive(Default)]
struct Scopes {
    frames: Vec<Vec<(String, String)>>,
}

impl Scopes {
    fn push(&mut self, declarations: Vec<(String, String)>) {
        self.frames.push(declarations);
    }

    fn pop(&mut self) {
        self.frames.pop();
    }

    /// The URI bound to a prefix, innermost declaration first. `xmlns=""`
    /// undeclares the default namespace and binds it to nothing, so an empty
    /// URI answers `None`.
    fn uri(&self, prefix: &str) -> Option<&str> {
        self.frames
            .iter()
            .rev()
            .flat_map(|frame| frame.iter().rev())
            .find(|(bound, _)| bound == prefix)
            .map(|(_, uri)| uri.as_str())
            .filter(|uri| !uri.is_empty())
    }
}

/// Turn a QName value into `{uri}Local`, resolving an absent prefix against
/// the default namespace, as XSD does for a QName in content.
fn resolve_qname(value: &str, scopes: &Scopes) -> String {
    let (prefix, local) = match value.split_once(':') {
        Some((prefix, local)) => (prefix, local),
        None => ("", value),
    };
    match scopes.uri(prefix) {
        Some(uri) => format!("{{{uri}}}{local}"),
        None => value.to_string(),
    }
}

/// Record one opening element: its path, its namespace bindings, its
/// attributes. Pushes one frame onto `stack` and one onto `scopes`, which the
/// caller pops together when the element ends.
fn push_start(
    start: &quick_xml::events::BytesStart<'_>,
    resolved: ResolveResult<'_>,
    stack: &mut Vec<String>,
    scopes: &mut Scopes,
    namespaces: &mut Vec<String>,
    shape: &mut Shape,
) -> Result<()> {
    let uri = match resolved {
        ResolveResult::Bound(ns) => String::from_utf8_lossy(ns.as_ref()).into_owned(),
        _ => String::new(),
    };
    let local = String::from_utf8_lossy(
        start
            .name()
            .as_ref()
            .rsplit(|b| *b == b':')
            .next()
            .unwrap_or_default(),
    )
    .into_owned();
    let step = format!("{{{uri}}}{local}");
    if !uri.is_empty() && !namespaces.contains(&uri) {
        namespaces.push(uri.clone());
    }

    let is_root = stack.is_empty();
    stack.push(step);
    // Paths are relative to the root: a writer that corrects a
    // non-conformant root tag (`obj_DiscreteProperty` for
    // `DiscreteProperty`) would otherwise make every descendant look
    // lost. The root itself is compared on its own.
    let path = stack[1.min(stack.len())..].join("/");
    *shape.elements.entry(path.clone()).or_insert(0) += 1;

    // Declarations first: an `xsi:type` may sit before the very
    // `xmlns:` that binds the prefix inside its value, and attribute
    // order is not information.
    let mut declarations: Vec<(String, String)> = Vec::new();
    for attribute in start.attributes() {
        let attribute = attribute?;
        let qname = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
        let prefix = match qname.as_str() {
            "xmlns" => Some(String::new()),
            _ => qname.strip_prefix("xmlns:").map(str::to_string),
        };
        if let Some(prefix) = prefix {
            declarations.push((
                prefix,
                String::from_utf8_lossy(attribute.value.as_ref()).into_owned(),
            ));
        }
    }
    scopes.push(declarations);

    for attribute in start.attributes() {
        let attribute = attribute?;
        let qname = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
        if is_noise(&qname) {
            continue;
        }
        // The local name is what identifies an attribute here: the
        // prefix bound to `xsi` is as free as any other.
        let (prefix, local) = match qname.split_once(':') {
            Some((p, l)) => (p, l.to_string()),
            None => ("", qname.clone()),
        };
        let value = collapse(&attribute.normalized_value(quick_xml::XmlVersion::default())?);
        if is_root && prefix.is_empty() && local == "schemaVersion" {
            shape.schema_version = Some(value.clone());
        }
        // An unprefixed attribute is in no namespace, so it is never
        // `xsi:type` however the default namespace happens to be bound.
        let (key, value) = if local == "type" && !prefix.is_empty() && scopes.uri(prefix) == Some(XSI) {
            // The value is a QName, so the prefix inside it is as
            // free as the one on the attribute: `resqml20:Point3dHdf5Array`
            // and `resqml:Point3dHdf5Array` name the same type when both
            // prefixes are bound to the RESQML namespace.
            (format!("{path}@xsi:type"), resolve_qname(&value, scopes))
        } else {
            (format!("{path}@{local}"), value)
        };
        shape.attributes.entry(key).or_default().push(value);
    }
    Ok(())
}

/// Reduce a document to its shape.
pub fn of(xml: &[u8]) -> Result<Shape> {
    let mut reader = quick_xml::NsReader::from_reader(xml);
    let mut shape = Shape::default();
    let mut stack: Vec<String> = Vec::new();
    let mut buf = Vec::new();
    let mut namespaces: Vec<String> = Vec::new();
    // Prefix → URI, so `xsi:type` is recognised by its namespace and not by a
    // prefix the document is free to spell however it likes.
    let mut scopes = Scopes::default();
    // The text of the element being read, joined across events.
    let mut run = String::new();

    loop {
        let position = reader.buffer_position();
        let (resolved, event) = reader
            .read_resolved_event_into(&mut buf)
            .with_context(|| format!("at byte {position}"))?;

        // Text is accumulated, not pushed per event: `&amp;` is an event of
        // its own and splits the characters around it, so pushing each piece
        // would compare `A &amp; B` against `A & B` as five values against one.
        match event {
            Event::Text(text) => {
                let raw = text.xml10_content()?;
                run.push_str(&quick_xml::escape::unescape(&raw)?);
                buf.clear();
                continue;
            }
            Event::CData(cdata) => {
                run.push_str(&String::from_utf8_lossy(cdata.into_inner().as_ref()));
                buf.clear();
                continue;
            }
            Event::GeneralRef(reference) => {
                let name = reference.decode()?;
                run.push_str(&quick_xml::escape::unescape(&format!("&{name};"))?);
                buf.clear();
                continue;
            }
            _ => {}
        }

        let value = collapse(&run);
        run.clear();
        if !value.is_empty() && !stack.is_empty() {
            shape
                .texts
                .entry(stack[1.min(stack.len())..].join("/"))
                .or_default()
                .push(value);
        }

        match event {
            Event::Start(start) => {
                push_start(
                    &start,
                    resolved,
                    &mut stack,
                    &mut scopes,
                    &mut namespaces,
                    &mut shape,
                )?;
                if shape.root.is_empty() {
                    shape.root = stack[0].clone();
                }
            }
            Event::Empty(start) => {
                push_start(
                    &start,
                    resolved,
                    &mut stack,
                    &mut scopes,
                    &mut namespaces,
                    &mut shape,
                )?;
                if shape.root.is_empty() {
                    shape.root = stack[0].clone();
                }
                stack.pop();
                scopes.pop();
            }
            Event::End(_) => {
                stack.pop();
                scopes.pop();
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    for values in shape.attributes.values_mut() {
        values.sort();
    }
    for values in shape.texts.values_mut() {
        values.sort();
    }
    namespaces.sort();
    shape.namespaces = namespaces;
    Ok(shape)
}

/// Whether two lexical values mean the same thing.
///
/// `2.0` and `2` are the same double; `true` and `1` the same boolean. A writer
/// that normalises them is not losing anything, and flagging it would bury the
/// real findings under thousands of lines.
fn same_value(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if let (Ok(x), Ok(y)) = (a.parse::<f64>(), b.parse::<f64>()) {
        return x == y || (x - y).abs() <= f64::EPSILON * x.abs().max(y.abs()).max(1.0);
    }
    let boolean = |v: &str| match v {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    };
    if let (Some(x), Some(y)) = (boolean(a), boolean(b)) {
        return x == y;
    }
    // A list of numbers: compare item by item, so `1.0 2.0` matches `1 2`.
    let (xs, ys): (Vec<&str>, Vec<&str>) = (a.split(' ').collect(), b.split(' ').collect());
    if xs.len() > 1 && xs.len() == ys.len() {
        return xs.iter().zip(&ys).all(|(x, y)| same_value(x, y));
    }
    false
}

/// Compare two lists of values as multisets, under [`same_value`].
fn compare_values(
    kind_lost: DiffKind,
    kind_added: DiffKind,
    path: &str,
    expected: &[String],
    actual: &[String],
    out: &mut Vec<Diff>,
) {
    let mut remaining: Vec<&String> = actual.iter().collect();
    for want in expected {
        match remaining.iter().position(|have| same_value(want, have)) {
            Some(i) => {
                remaining.remove(i);
            }
            None => out.push(Diff {
                kind: kind_lost,
                path: path.to_string(),
                expected: want.clone(),
                actual: String::new(),
            }),
        }
    }
    for extra in remaining {
        out.push(Diff {
            kind: kind_added,
            path: path.to_string(),
            expected: String::new(),
            actual: extra.clone(),
        });
    }
}

/// Every way the round trip differs from the original.
pub fn compare(original: &Shape, round_tripped: &Shape) -> Vec<Diff> {
    let mut out = Vec::new();

    for (path, &count) in &original.elements {
        let after = round_tripped.elements.get(path).copied().unwrap_or(0);
        if after < count {
            out.push(Diff {
                kind: DiffKind::LostElement,
                path: path.clone(),
                expected: count.to_string(),
                actual: after.to_string(),
            });
        }
    }
    for (path, &count) in &round_tripped.elements {
        let before = original.elements.get(path).copied().unwrap_or(0);
        if count > before {
            out.push(Diff {
                kind: DiffKind::AddedElement,
                path: path.clone(),
                expected: before.to_string(),
                actual: count.to_string(),
            });
        }
    }

    let empty = Vec::new();
    for (path, values) in &original.attributes {
        let after = round_tripped.attributes.get(path).unwrap_or(&empty);
        // An `xsi:type` that vanished entirely is redundant typing, not a loss.
        // One that came back *different* is a real disagreement, so it stays a
        // lost attribute and is reported as such.
        let (lost, added) = if path.ends_with("@xsi:type") && after.is_empty() {
            (DiffKind::XsiTypeDropped, DiffKind::AddedAttribute)
        } else {
            (DiffKind::LostAttribute, DiffKind::AddedAttribute)
        };
        compare_values(lost, added, path, values, after, &mut out);
    }
    for (path, values) in &round_tripped.attributes {
        if !original.attributes.contains_key(path) {
            compare_values(
                DiffKind::LostAttribute,
                DiffKind::AddedAttribute,
                path,
                &empty,
                values,
                &mut out,
            );
        }
    }

    for (path, values) in &original.texts {
        let after = round_tripped.texts.get(path).unwrap_or(&empty);
        compare_values(
            DiffKind::LostText,
            DiffKind::AddedText,
            path,
            values,
            after,
            &mut out,
        );
    }
    for (path, values) in &round_tripped.texts {
        if !original.texts.contains_key(path) {
            compare_values(
                DiffKind::LostText,
                DiffKind::AddedText,
                path,
                &empty,
                values,
                &mut out,
            );
        }
    }

    if original.root != round_tripped.root {
        // Only worth a word: the tag is a spelling, and RESQML 2.0.1 files in
        // the wild use both `obj_Foo` and `Foo` as the root of the same object.
        out.push(Diff {
            kind: DiffKind::RootRenamed,
            path: original.root.clone(),
            expected: original.root.clone(),
            actual: round_tripped.root.clone(),
        });
    }

    for uri in &original.namespaces {
        if !round_tripped.namespaces.contains(uri) {
            out.push(Diff {
                kind: DiffKind::LostNamespace,
                path: original.root.clone(),
                expected: uri.clone(),
                actual: String::new(),
            });
        }
    }

    out.sort_by(|a, b| (a.kind, &a.path).cmp(&(b.kind, &b.path)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &[u8] = br#"<r:Root xmlns:r="urn:r" xmlns:e="urn:e" id="1">
        <r:A e:uom="m">1.0</r:A>
        <r:A e:uom="m">2</r:A>
        <r:B/>
    </r:Root>"#;

    #[test]
    fn a_document_equals_itself() {
        let s = of(DOC).unwrap();
        assert!(compare(&s, &of(DOC).unwrap()).is_empty());
    }

    #[test]
    fn prefixes_and_attribute_order_do_not_count() {
        let other = br#"<x:Root xmlns:x="urn:r" xmlns:y="urn:e"><x:A y:uom="m">1</x:A>
            <x:A y:uom="m">2.00</x:A><x:B></x:B></x:Root>"#;
        let mut diffs = compare(&of(DOC).unwrap(), &of(other).unwrap());
        // Only the root's `id`, which the other document really does not have.
        assert_eq!(diffs.len(), 1, "{diffs:?}");
        let only = diffs.pop().unwrap();
        assert_eq!(only.kind, DiffKind::LostAttribute);
        assert!(only.path.ends_with("@id"), "{}", only.path);
    }

    #[test]
    fn a_missing_element_is_a_loss() {
        let other = br#"<r:Root xmlns:r="urn:r" xmlns:e="urn:e" id="1">
            <r:A e:uom="m">1.0</r:A><r:B/></r:Root>"#;
        let diffs = compare(&of(DOC).unwrap(), &of(other).unwrap());
        assert!(diffs.iter().any(|d| d.kind == DiffKind::LostElement));
        assert!(diffs.iter().any(|d| d.kind == DiffKind::LostText));
    }

    /// fesapi, SKUA and Sismage all write the package namespace as the default
    /// one and then an unprefixed `xsi:type`. Reading that as a type in *no*
    /// namespace made every such attribute differ from the prefixed one we
    /// write back, and 105 parts of the corpus were reported as losing their
    /// array types when nothing had been lost.
    #[test]
    fn an_unprefixed_xsi_type_names_the_default_namespace() {
        let default_ns = br#"<Root xmlns="urn:r" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
            <A xsi:type="IntegerHdf5Array"/></Root>"#;
        let prefixed = br#"<r:Root xmlns:r="urn:r" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
            <r:A xsi:type="r:IntegerHdf5Array"/></r:Root>"#;
        let diffs = compare(&of(default_ns).unwrap(), &of(prefixed).unwrap());
        assert!(diffs.is_empty(), "{diffs:?}");
    }

    /// The default namespace is scoped like any other binding, so a subtree
    /// that rebinds it must not change how its siblings resolve.
    #[test]
    fn a_rebound_default_namespace_stops_at_its_subtree() {
        let xsi = "http://www.w3.org/2001/XMLSchema-instance";
        let doc = format!(
            r#"<Root xmlns="urn:r" xmlns:xsi="{xsi}">
                 <Inner xmlns="urn:other"><A xsi:type="Elsewhere"/></Inner>
                 <B xsi:type="Here"/>
               </Root>"#
        );
        let shape = of(doc.as_bytes()).unwrap();
        // `Inner` declares the new default namespace on itself, so it is in it too.
        assert_eq!(
            shape.attributes.get("{urn:other}Inner/{urn:other}A@xsi:type"),
            Some(&vec!["{urn:other}Elsewhere".to_string()])
        );
        assert_eq!(
            shape.attributes.get("{urn:r}B@xsi:type"),
            Some(&vec!["{urn:r}Here".to_string()])
        );
    }

    /// An unprefixed attribute is in no namespace, whatever the default is
    /// bound to - so a plain `type` attribute is never `xsi:type`.
    #[test]
    fn a_plain_type_attribute_is_not_an_xsi_type() {
        let doc = br#"<Root xmlns="http://www.w3.org/2001/XMLSchema-instance"><A type="plain"/></Root>"#;
        let shape = of(doc).unwrap();
        let path = "{http://www.w3.org/2001/XMLSchema-instance}A@type";
        assert_eq!(shape.attributes.get(path), Some(&vec!["plain".to_string()]));
    }

    /// The root's `schemaVersion` is the only thing in a document that says
    /// which release wrote it: every version of a package shares one URI.
    #[test]
    fn the_root_schema_version_is_kept() {
        let doc = br#"<r:Root xmlns:r="urn:r" schemaVersion="2.2"><r:A schemaVersion="9"/></r:Root>"#;
        assert_eq!(of(doc).unwrap().schema_version.as_deref(), Some("2.2"));
    }

    #[test]
    fn a_different_namespace_for_the_same_local_name_is_a_loss() {
        let other = br#"<r:Root xmlns:r="urn:r" xmlns:e="urn:e" id="1">
            <e:A e:uom="m">1.0</e:A><r:A e:uom="m">2</r:A><r:B/></r:Root>"#;
        let diffs = compare(&of(DOC).unwrap(), &of(other).unwrap());
        assert!(
            diffs
                .iter()
                .any(|d| d.kind == DiffKind::LostElement && d.path.contains("{urn:r}A")),
            "{diffs:?}"
        );
    }
}

#[cfg(test)]
mod escape_tests {
    use super::*;

    #[test]
    fn escaped_and_raw_quotes_are_the_same_text() {
        let raw = br#"<a xmlns="urn:a"><v>PROJCRS["ETRS89", ID["EPSG", 9820]]</v></a>"#;
        let escaped =
            br#"<a xmlns="urn:a"><v>PROJCRS[&quot;ETRS89&quot;, ID[&quot;EPSG&quot;, 9820]]</v></a>"#;
        let (x, y) = (of(raw).unwrap(), of(escaped).unwrap());
        println!("raw     : {:?}", x.texts);
        println!("escaped : {:?}", y.texts);
        assert!(compare(&x, &y).is_empty(), "{:?}", compare(&x, &y));
    }
}
