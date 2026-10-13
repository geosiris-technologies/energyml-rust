//! Level 2: reading a citation without deserializing the object.
//!
//! An interface listing a two-gigabyte EPC needs a title and a date per object,
//! and nothing else. Deserializing every part to get them would be absurd: a
//! `TriangulatedSetRepresentation` is a few kilobytes of XML whose first two
//! hundred bytes carry everything a listing shows.
//!
//! So this is a **parser that stops**. It reads the root start tag for the
//! identity attributes, walks forward to `Citation`, takes what is in it, and
//! returns the moment the citation closes. What follows is never looked at.
//!
//! It is a free function over bytes already in memory -
//! (ADR-0002) - so the synchronous and the asynchronous paths share it without
//! either owning it.

use quick_xml::events::Event;

/// What the first few hundred bytes of a part say about it.
///
/// Every field is optional because every field is missing somewhere: RESQML
/// 2.0.1 has no `objectVersion`, some producers omit `Originator`, and a part
/// that is not an EnergyML object has no citation at all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PeekedHeader {
    /// The type the root **is**: the local name of the root element,
    /// `obj_Activity` or `Activity` - or, when the root carries an `xsi:type`,
    /// the type that attribute names.
    ///
    /// Kept because it is the fallback when `[Content_Types].xml` types a part
    /// wrongly - which the reference package does.
    ///
    /// # Why the `xsi:type` wins
    ///
    /// PRODML 2.3 gives `FlowTestActivity` a global element and none of its
    /// subtypes, though its own documentation says *"This object is abstract;
    /// you must choose one of the available types"* (FlowTestActivity.xsd
    /// l. 186-190). An injection test is therefore written
    /// `<prodml:FlowTestActivity xsi:type="prodml:InjectionFlowTest">` - the
    /// form of Energistics' own example, `xml_examples/FlowTest/
    /// FlowTestActivity.xml` l. 2. Matched on the element, the part would be
    /// taken for the empty base and everything the test says would be lost.
    pub root_name: String,
    /// The root element's own name, when an `xsi:type` made [`root_name`]
    /// something else - `FlowTestActivity` for an `InjectionFlowTest`.
    ///
    /// [`root_name`]: PeekedHeader::root_name
    pub root_element: Option<String>,
    /// `uuid` attribute of the root element.
    pub uuid: Option<String>,
    /// `schemaVersion` attribute, as written: `2.0`, `2.2`, sometimes `2.0.1`.
    pub schema_version: Option<String>,
    /// Namespace URI the root element is in.
    ///
    /// It says which **package** the object belongs to and nothing about its
    /// version: Energistics gave every version of a package the same namespace,
    /// so `resqmlv2` covers RESQML 2.0.1 and 2.2 alike. That makes it exactly
    /// half of the identification - the half `schemaVersion` does not do, and
    /// the half that tells `resqml20.Activity` from `eml23.Activity`, two types
    /// with the same name in different packages.
    pub namespace: Option<String>,
    /// `objectVersion` attribute. An attribute, not an element, in every
    /// version that has one - eml 2.1 and later.
    pub object_version: Option<String>,
    /// `Citation/Title`.
    pub title: Option<String>,
    /// `Citation/Originator`.
    pub originator: Option<String>,
    /// `Citation/Creation`, lexical.
    pub creation: Option<String>,
    /// `Citation/LastUpdate`, lexical.
    pub last_update: Option<String>,
    /// `Citation/Format` - the producing software, as it named itself.
    pub format: Option<String>,
}

/// The XML Schema instance namespace, the one `xsi:type` is in.
const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";

/// Read the identity and the citation of a part already in memory.
pub fn peek(xml: &[u8]) -> PeekedHeader {
    peek_reader(xml)
}

/// Read the identity and the citation of a part **as it streams**, and stop
/// there.
///
/// Taking a reader rather than a slice is the whole economy: fed the entry of a
/// zip archive, this inflates the first few hundred bytes and abandons the rest.
///
/// Never fails: a part this cannot read yields a header with empty fields
/// rather than an error, because a listing that loses one row over a malformed
/// part is worse than a listing with one thin row. The caller that needs
/// certainty deserializes the object.
pub fn peek_reader<R: std::io::BufRead>(source: R) -> PeekedHeader {
    let mut out = PeekedHeader::default();
    let mut reader = quick_xml::Reader::from_reader(source);
    // Trimming per event would cut the inside of a text run: since quick-xml
    // 0.41 an entity is its own event, so `A &amp; B` arrives in three pieces.
    // The run is accumulated whole and trimmed once, as `energyml-core` does.
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();

    // Where the walk is: before the root, inside it, inside the citation.
    let mut root_seen = false;
    let mut in_citation = false;
    let mut current: Option<String> = None;
    let mut text = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Err(_) | Ok(Event::Eof) => break,
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = local_name(e.name().as_ref());
                if !root_seen {
                    root_seen = true;
                    out.root_name = name;
                    // The root's own prefix, before its declarations are read -
                    // an element is in the namespace its prefix is bound to on
                    // itself or on an ancestor, and the root has no ancestor.
                    let prefix = prefix_of(e.name().as_ref());
                    let mut declarations: Vec<(String, String)> = Vec::new();
                    // `(prefix, value)` of a `…:type` attribute: an `xsi:type`
                    // once its prefix is found bound to the XSI namespace.
                    let mut typed: Option<(String, String)> = None;
                    for attribute in e.attributes().flatten() {
                        let value = unescape_attr(&attribute.value);
                        let key = attribute.key.as_ref();
                        if key == b"xmlns" {
                            declarations.push((String::new(), value.clone()));
                        } else if let Some(bound) = key.strip_prefix(b"xmlns:") {
                            declarations.push((String::from_utf8_lossy(bound).to_string(), value.clone()));
                        }
                        match local_name(key).as_str() {
                            "uuid" => out.uuid = non_empty(value),
                            "schemaVersion" => out.schema_version = non_empty(value),
                            "objectVersion" => out.object_version = non_empty(value),
                            "type" if key.contains(&b':') => typed = Some((prefix_of(key), value)),
                            _ => {}
                        }
                    }
                    if let Some((bound, value)) = typed
                        && declarations.iter().any(|(p, uri)| *p == bound && uri == XSI)
                    {
                        let named = value.rsplit(':').next().unwrap_or(&value).trim().to_string();
                        // A type naming itself - `LocalDepth3dCrs` typed
                        // `obj_LocalDepth3dCrs`, as some producers write - is
                        // no substitution: the element stays the name.
                        let fold =
                            |name: &str| name.strip_prefix("obj_").unwrap_or(name).to_ascii_lowercase();
                        if !named.is_empty() && fold(&named) != fold(&out.root_name) {
                            out.root_element = Some(std::mem::replace(&mut out.root_name, named));
                        }
                    }
                    out.namespace = declarations
                        .into_iter()
                        .find(|(bound, _)| *bound == prefix)
                        .map(|(_, uri)| uri)
                        .filter(|uri| !uri.is_empty());
                } else if name == "Citation" {
                    in_citation = true;
                } else if in_citation {
                    current = Some(name);
                    text.clear();
                }
            }
            Ok(Event::Text(e)) if in_citation && current.is_some() => {
                let raw = e.xml10_content().unwrap_or_default();
                match quick_xml::escape::unescape(&raw) {
                    Ok(unescaped) => text.push_str(&unescaped),
                    Err(_) => text.push_str(&raw),
                }
            }
            Ok(Event::CData(e)) if in_citation && current.is_some() => {
                text.push_str(&String::from_utf8_lossy(&e));
            }
            // `&amp;`, `&#233;`: their own event since quick-xml 0.41.
            Ok(Event::GeneralRef(e)) if in_citation && current.is_some() => {
                if let Ok(name) = e.decode() {
                    let reference = format!("&{name};");
                    if let Ok(resolved) = quick_xml::escape::unescape(&reference) {
                        text.push_str(&resolved);
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = local_name(e.name().as_ref());
                if name == "Citation" {
                    // The whole point: everything a listing needs is now read,
                    // and the rest of the part - which may be megabytes of
                    // inline geometry - is never touched.
                    break;
                }
                if in_citation && current.as_deref() == Some(name.as_str()) {
                    let value = non_empty(std::mem::take(&mut text));
                    match name.as_str() {
                        "Title" => out.title = value,
                        "Originator" => out.originator = value,
                        "Creation" => out.creation = value,
                        "LastUpdate" => out.last_update = value,
                        "Format" => out.format = value,
                        _ => {}
                    }
                    current = None;
                }
            }
            _ => {}
        }
        buf.clear();
    }
    out
}

/// Local name of a possibly prefixed XML name.
/// The prefix of a qualified name, empty when there is none.
fn prefix_of(qualified: &[u8]) -> String {
    match qualified.iter().position(|byte| *byte == b':') {
        Some(at) => String::from_utf8_lossy(&qualified[..at]).to_string(),
        None => String::new(),
    }
}

fn local_name(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw.rsplit(|b| *b == b':').next().unwrap_or_default()).into_owned()
}

fn unescape_attr(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    quick_xml::escape::unescape(&text)
        .map(|c| c.into_owned())
        .unwrap_or_else(|_| text.into_owned())
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PART: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<resqml:obj_TriangulatedSetRepresentation
    xmlns:resqml="http://www.energistics.org/energyml/data/resqmlv2"
    xmlns:eml="http://www.energistics.org/energyml/data/commonv2"
    schemaVersion="2.0" uuid="f0d2a4ec-1234-4f22-9d1a-000000000001">
  <eml:Citation>
    <eml:Title>A &amp; B</eml:Title>
    <eml:Originator>fesapi</eml:Originator>
    <eml:Creation>2019-05-07T09:12:00Z</eml:Creation>
    <eml:Format>F2I-CONSULTING:fesapi 2.0.0</eml:Format>
    <eml:LastUpdate>2020-01-01T00:00:00Z</eml:LastUpdate>
  </eml:Citation>
  <resqml:Geometry>this must never be read</resqml:Geometry>
</resqml:obj_TriangulatedSetRepresentation>"#;

    #[test]
    fn the_citation_is_read_and_the_rest_is_not() {
        let header = peek(PART);
        assert_eq!(header.root_name, "obj_TriangulatedSetRepresentation");
        assert_eq!(
            header.uuid.as_deref(),
            Some("f0d2a4ec-1234-4f22-9d1a-000000000001")
        );
        assert_eq!(header.schema_version.as_deref(), Some("2.0"));
        assert_eq!(header.object_version, None);
        assert_eq!(header.title.as_deref(), Some("A & B"));
        assert_eq!(header.originator.as_deref(), Some("fesapi"));
        assert_eq!(header.creation.as_deref(), Some("2019-05-07T09:12:00Z"));
        assert_eq!(header.last_update.as_deref(), Some("2020-01-01T00:00:00Z"));
        assert_eq!(header.format.as_deref(), Some("F2I-CONSULTING:fesapi 2.0.0"));
    }

    #[test]
    fn truncating_the_part_after_the_citation_changes_nothing() {
        // Proof that nothing below the citation is needed: cut the bytes there
        // and the answer must be identical.
        let end = PART.windows(14).position(|w| w == b"</eml:Citation").unwrap() + 15;
        assert_eq!(peek(&PART[..end]), peek(PART));
    }

    #[test]
    fn an_object_version_is_an_attribute() {
        let xml = br#"<eml:Activity xmlns:eml="x" uuid="u" schemaVersion="2.3" objectVersion="7">
            <eml:Citation><eml:Title>T</eml:Title></eml:Citation></eml:Activity>"#;
        let header = peek(xml);
        assert_eq!(header.object_version.as_deref(), Some("7"));
        assert_eq!(header.title.as_deref(), Some("T"));
    }

    #[test]
    fn a_part_without_a_citation_still_yields_its_identity() {
        let xml = br#"<Types xmlns="x"><Default Extension="rels"/></Types>"#;
        let header = peek(xml);
        assert_eq!(header.root_name, "Types");
        assert_eq!(header.title, None);
    }
}
