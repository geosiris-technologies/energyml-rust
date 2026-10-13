//! A subtree the generator does not model, kept instead of dropped.
//!
//! The Energistics schemas embed foreign vocabularies — a full GML CRS
//! definition inside `eml:ProjectedGmlCrs`, for instance. Generating Rust for
//! all of GML is out of scope, but *losing* the definition when a file is read
//! and written back is worse than not understanding it: a round trip would
//! quietly turn a georeferenced object into an unreferenced one.
//!
//! [`AnyXml`] holds such a subtree as it was read — attributes and content —
//! and writes it back unchanged. Nothing interprets it; it survives.

use quick_xml::events::{BytesEnd, BytesStart};

use crate::error::Result;
use crate::reflect::{EnergymlType, Roles, TypeInfo, TypeKind};
use crate::value::{AsFieldValue, FieldValue, FromOwnedValue, OwnedValue, Reflect, downcast_owned};
use crate::xml::{XmlDeserialize, XmlEvent, XmlReader, XmlSerialize, XmlWriter};

/// An XML subtree carried verbatim.
///
/// Round-tripping is semantic, not byte-for-byte: the content is rebuilt from
/// the parsed events, so entity spellings, self-closing tags and insignificant
/// whitespace may differ. Every element, attribute and text value is preserved.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AnyXml {
    /// Attributes of the carrying element, name and value as written.
    pub attributes: Vec<(String, String)>,
    /// Everything between the start and end tags, as XML source.
    pub content: String,
    /// Prefix → namespace URI for every prefix the content uses.
    ///
    /// The subtree was written under declarations that lived on an ancestor —
    /// typically the document root — and those do not travel with it. Without
    /// carrying them, writing the subtree back produces XML where `resqml:` is
    /// bound to nothing: well-formed to the eye, unusable to a parser.
    pub namespaces: Vec<(String, String)>,
}

impl AnyXml {
    /// Whether the subtree carries nothing at all.
    pub fn is_empty(&self) -> bool {
        self.attributes.is_empty() && self.content.trim().is_empty()
    }

    /// Namespace URI bound to a prefix inside the kept subtree.
    pub fn namespace(&self, prefix: &str) -> Option<&str> {
        self.namespaces
            .iter()
            .find(|(p, _)| p == prefix)
            .map(|(_, uri)| uri.as_str())
    }

    /// Look up one attribute of the carrying element.
    ///
    /// Enough to read a `srsName` or a `gml:id` out of a CRS definition
    /// without a GML parser.
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

impl XmlSerialize for AnyXml {
    fn xml_attributes(&self, tag: &mut BytesStart<'_>) -> Result<()> {
        for (name, value) in &self.attributes {
            tag.push_attribute((name.as_str(), value.as_str()));
        }
        // Re-declare what the content needs, unless the element already says it.
        for (prefix, uri) in &self.namespaces {
            let declaration = format!("xmlns:{prefix}");
            if !self.attributes.iter().any(|(name, _)| name == &declaration) {
                tag.push_attribute((declaration.as_str(), uri.as_str()));
            }
        }
        Ok(())
    }

    fn xml_children(&self, w: &mut XmlWriter<'_>) -> Result<()> {
        w.write_raw(&self.content)
    }
}

/// Append one element — its own start tag included — to a wildcard's content.
///
/// The counterpart of [`AnyXml::xml_read`] for an `xsd:any`: there, the
/// carrying element is known and only its *inside* is kept; here the element
/// itself is one of the unclaimed children, so its tag is part of what must be
/// written back.
///
/// Used by generated code in the arm that would otherwise skip an unknown
/// child. A struct without a wildcard field still skips — this is only reached
/// when the schema said `xsd:any`.
pub fn capture_into(
    into: &mut AnyXml,
    r: &mut XmlReader<'_>,
    start: &BytesStart<'_>,
    ns: Option<&str>,
) -> Result<()> {
    remember_prefix(into, start.name().as_ref(), ns.map(str::to_string));
    write_start(&mut into.content, start)?;

    let mut depth = 1usize;
    loop {
        match r.next_event()? {
            XmlEvent::Start(e, uri) => {
                depth += 1;
                remember_prefix(into, e.name().as_ref(), uri);
                write_start(&mut into.content, &e)?;
            }
            XmlEvent::End(e) => {
                depth -= 1;
                write_end(&mut into.content, &e);
                if depth == 0 {
                    break;
                }
            }
            XmlEvent::Text(t) => into.content.push_str(&escape_text(&t)),
            XmlEvent::Eof => return Err(crate::error::Error::Eof),
        }
    }
    Ok(())
}

/// Record the namespace a captured element's prefix was bound to.
///
/// The declaration lived on an ancestor and does not travel with the subtree;
/// without this, the re-written XML binds `foo:` to nothing.
fn remember_prefix(into: &mut AnyXml, name: &[u8], uri: Option<String>) {
    let Some(uri) = uri else { return };
    let name = String::from_utf8_lossy(name);
    if let Some((prefix, _)) = name.split_once(':')
        && !into.namespaces.iter().any(|(p, _)| p == prefix)
    {
        into.namespaces.push((prefix.to_string(), uri));
    }
}

impl XmlDeserialize for AnyXml {
    fn xml_read(r: &mut XmlReader<'_>, start: &BytesStart<'_>) -> Result<Self> {
        let mut attributes = Vec::new();
        let mut namespaces: Vec<(String, String)> = Vec::new();
        for attribute in start.attributes() {
            let attribute = attribute.map_err(|e| crate::error::Error::Xml(e.to_string()))?;
            let name = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
            let value = attribute
                .normalized_value(quick_xml::XmlVersion::default())?
                .into_owned();
            // A declaration is not an attribute of the data: it goes in the
            // namespace list, which the writer re-emits without ever doubling
            // one the surrounding document already made.
            if let Some(prefix) = name.strip_prefix("xmlns:") {
                if !namespaces.iter().any(|(p, _)| p == prefix) {
                    namespaces.push((prefix.to_string(), value));
                }
                continue;
            }
            if name == "xmlns" {
                continue;
            }
            attributes.push((name, value));
        }

        let mut content = String::new();
        let mut depth = 1usize;
        loop {
            match r.next_event()? {
                XmlEvent::Start(e, uri) => {
                    depth += 1;
                    // The reader resolved the prefix; remember the binding so
                    // the subtree can be written somewhere else and still parse.
                    if let Some(uri) = uri {
                        let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                        if let Some((prefix, _)) = name.split_once(':')
                            && !namespaces.iter().any(|(p, _)| p == prefix)
                        {
                            namespaces.push((prefix.to_string(), uri));
                        }
                    }
                    write_start(&mut content, &e)?;
                }
                XmlEvent::End(e) => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    write_end(&mut content, &e);
                }
                XmlEvent::Text(t) => content.push_str(&escape_text(&t)),
                XmlEvent::Eof => return Err(crate::error::Error::Eof),
            }
        }
        // Carry only what the subtree actually uses. A declaration that came
        // along for the ride would be re-emitted next to the identical one the
        // surrounding document already made — and two `xmlns:eml` on one tag is
        // not valid XML.
        namespaces.retain(|(prefix, _)| {
            content.contains(&format!("<{prefix}:"))
                || content.contains(&format!(" {prefix}:"))
                || attributes.iter().any(|(name, value)| {
                    name.starts_with(&format!("{prefix}:")) || value.starts_with(&format!("{prefix}:"))
                })
        });
        Ok(AnyXml {
            attributes,
            content,
            namespaces,
        })
    }
}

fn write_start(out: &mut String, e: &BytesStart<'_>) -> Result<()> {
    out.push('<');
    out.push_str(&String::from_utf8_lossy(e.name().as_ref()));
    for attribute in e.attributes() {
        let attribute = attribute.map_err(|err| crate::error::Error::Xml(err.to_string()))?;
        out.push(' ');
        out.push_str(&String::from_utf8_lossy(attribute.key.as_ref()));
        out.push_str("=\"");
        out.push_str(&escape_attr(
            &attribute.normalized_value(quick_xml::XmlVersion::default())?,
        ));
        out.push('"');
    }
    out.push('>');
    Ok(())
}

fn write_end(out: &mut String, e: &BytesEnd<'_>) {
    out.push_str("</");
    out.push_str(&String::from_utf8_lossy(e.name().as_ref()));
    out.push('>');
}

fn escape_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_attr(value: &str) -> String {
    escape_text(value).replace('"', "&quot;")
}

// ---------------------------------------------------------------------------
// Reflection: present, but with nothing to say
// ---------------------------------------------------------------------------

impl EnergymlType for AnyXml {
    fn type_info() -> &'static TypeInfo {
        static INFO: TypeInfo = TypeInfo {
            name: "AnyXml",
            package: "eml",
            version: "",
            schema_version: "",
            xml_name: "AnyXml",
            object_type: "AnyXml",
            type_key: "AnyXml",
            xml_ns: crate::ns::EML,
            content_type: None,
            qualified_type: None,
            root_element: None,
            doc: "An XML subtree kept verbatim: the generator does not model this vocabulary.",
            kind: TypeKind::Struct,
            is_abstract: false,
            is_top_level: false,
            is_nestable: false,
            is_object_ref: false,
            roles: Roles::EMPTY,
            base: None,
            fields: &[],
            variants: &[],
            subtypes: &[],
            is_open: false,
            other_pattern: None,
            // An empty kept-verbatim subtree. Buildable, so a generator that
            // reaches an unmodelled field produces an object that round-trips
            // rather than stopping.
            make: Some(|| Box::new(AnyXml::default()) as Box<dyn Reflect>),
        };
        &INFO
    }
}

impl Reflect for AnyXml {
    fn type_info(&self) -> &'static TypeInfo {
        <Self as EnergymlType>::type_info()
    }

    fn field(&self, _name: &str) -> Option<FieldValue<'_>> {
        None
    }

    fn set_field(&mut self, name: &str, _value: OwnedValue) -> Result<()> {
        Err(crate::error::Error::UnknownField {
            ty: "AnyXml",
            field: name.to_string(),
        })
    }

    fn field_object_mut(&mut self, _name: &str, _index: Option<usize>) -> Option<&mut dyn Reflect> {
        None
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

impl AsFieldValue for AnyXml {
    fn as_field_value(&self) -> FieldValue<'_> {
        FieldValue::Object(self)
    }
}

impl FromOwnedValue for AnyXml {
    fn from_owned_value(value: OwnedValue) -> Result<Self> {
        downcast_owned::<Self>(value, "AnyXml")
    }
}

impl crate::validate::Validate for AnyXml {
    fn validate_into(&self, _ctx: &mut crate::validate::ValidationCtx<'_>) {
        // Nothing to check: the schema for this subtree is not the one we read.
    }
}

// ---------------------------------------------------------------------------
// Typing a subtree that only looks foreign

impl AnyXml {
    /// The type this subtree declares, when the registry knows it.
    ///
    /// # Why a subtree that is not really foreign exists at all
    ///
    /// A GML CRS definition is genuinely foreign: nothing here models GML. But
    /// `GraphicalInformationSet.GraphicalInformation` is declared in EML 2.3 as
    /// `AbstractGraphicalInformation`, and its concrete classes —
    /// `DefaultGraphicalInformation`, `ColorInformation` — are **generated**,
    /// in RESQML 2.2. The field falls back to a subtree only because a package
    /// cannot name the classes of one that depends on it, which is a fact about
    /// crate layering and not about the data.
    ///
    /// So the `xsi:type` the subtree carries is often a type this build knows
    /// in full. This resolves it through the prefix declarations the subtree
    /// kept, never by guessing a package from a name: two releases spell the
    /// same class the same way, and only the namespace URI tells them apart.
    pub fn declared_type(&self, registry: &crate::registry::Registry) -> Option<&'static TypeInfo> {
        let declared = self
            .attributes
            .iter()
            .find(|(name, _)| name == "xsi:type" || name.ends_with(":type") || name == "type")
            .map(|(_, value)| value.as_str())?;
        let (prefix, local) = declared.split_once(':')?;
        let uri = self.namespace(prefix)?;
        // The package from the **namespace URI**, through the same table the
        // writers use: a prefix is whatever the document chose, and two
        // documents spell one namespace differently as often as not.
        let package = crate::ns::well_known_namespaces()
            .iter()
            .find(|known| known.uri == uri)
            .map(|known| known.prefix)?;
        // The namespace names the **package**, never the release: every version
        // of RESQML shares one URI. So the candidates are ranked, and the rank
        // is not cosmetic — picking the wrong release picks a different class.
        // Measured on 04/09: `2.2_dev3` was chosen ahead of `2.2`, its
        // `TargetObject` is a reference of the older shape, and a round trip
        // through it **dropped `QualifiedType` and invented an empty
        // `ContentType`**. A development drop is never the answer when a
        // release spells the same name, and among releases the newest is.
        let mut candidates: Vec<&'static TypeInfo> = registry
            .types()
            .filter(|info| info.xml_name == local && info.package == package && !info.version.is_empty())
            .collect();
        candidates.sort_by_key(|info| {
            (
                crate::naming::is_prerelease(info.version),
                std::cmp::Reverse(version_rank(info.version)),
            )
        });
        candidates.into_iter().next()
    }

    /// This subtree read as the type it declares.
    ///
    /// Lenient on purpose: a subtree that a producer wrote loosely is still
    /// worth editing, and refusing it here would put the front end back where it
    /// was — a list of indices. What could not be read is the caller's to
    /// report, through the departures.
    pub fn typed(
        &self,
        registry: &crate::registry::Registry,
    ) -> Option<crate::error::Result<(Box<dyn crate::registry::DynObject>, crate::registry::Departures)>>
    {
        let info = self.declared_type(registry)?;
        let declarations: String = self
            .namespaces
            .iter()
            .map(|(prefix, uri)| format!(" xmlns:{prefix}=\"{uri}\""))
            .collect();
        // Wrapped in an element named after the **type**, not after the field:
        // the reader matches on the type it was asked for, and the field's own
        // name (`GraphicalInformation`) is not one of its spellings.
        let namespace = crate::ns::Namespace::well_known(info.package)?;
        let opening = format!("{}:{}", namespace.prefix, info.xml_name);
        let extra = if self.namespace(namespace.prefix).is_some() {
            String::new()
        } else {
            format!(" xmlns:{}=\"{}\"", namespace.prefix, namespace.uri)
        };
        let xml = format!("<{opening}{declarations}{extra}>{}</{opening}>", self.content);
        Some(registry.read_xml_as_named(info, xml.as_bytes(), crate::registry::ReadMode::Lenient))
    }

    /// The same subtree, rebuilt from an object of the type it declares.
    ///
    /// Keeps the attributes and the declarations it already had: they say what
    /// the subtree *is* and which prefixes its text uses, and neither is the
    /// object's to restate. Only the content changes, which is the whole point
    /// — editing one value must not rewrite the shape of the element around it.
    pub fn rebuilt(&self, object: &dyn crate::registry::DynObject) -> crate::error::Result<AnyXml> {
        let info = object.as_reflect().type_info();
        let namespace = crate::ns::Namespace::well_known(info.package).ok_or_else(|| {
            crate::error::Error::UnknownType(format!("no namespace for package `{}`", info.package))
        })?;
        let written = crate::xml::to_xml_string(
            object.as_xml(),
            namespace,
            info.xml_name,
            &[namespace, crate::ns::EML, crate::ns::XSI],
            crate::xml::XmlWriteOptions {
                declaration: false,
                ..Default::default()
            },
        )?;
        Ok(AnyXml {
            content: inner_of(&written),
            ..self.clone()
        })
    }
}

impl AnyXml {
    /// A subtree built from an object, for a slot that holds subtrees.
    ///
    /// The counterpart of [`AnyXml::typed`], and the reason it is needed: a
    /// list of subtrees written back **whole** — an element added, one removed
    /// — arrives as typed objects, and without this every one of them would
    /// become an empty subtree. Silently, which is the part that matters.
    ///
    /// The `xsi:type` is stamped from the object's own type, and the three
    /// declarations its content can use are declared. Neither is a guess: an
    /// element of an abstract slot has to say which class it is, and a prefix
    /// the content uses has to be bound or the subtree is unusable to a parser.
    pub fn of(object: &dyn crate::registry::DynObject) -> crate::error::Result<AnyXml> {
        let info = object.as_reflect().type_info();
        let namespace = crate::ns::Namespace::well_known(info.package).ok_or_else(|| {
            crate::error::Error::UnknownType(format!("no namespace for package `{}`", info.package))
        })?;
        let blank = AnyXml {
            attributes: vec![("xsi:type".to_string(), namespace.qualify(info.xml_name))],
            content: String::new(),
            namespaces: [namespace, crate::ns::EML, crate::ns::XSI]
                .iter()
                .map(|n| (n.prefix.to_string(), n.uri.to_string()))
                .collect(),
        };
        blank.rebuilt(object)
    }
}

/// Everything between the first `>` and the last `<` — the element's content,
/// which is what an [`AnyXml`] holds.
///
/// A self-closing root has no content, and neither does an empty one; both come
/// back as the empty string rather than as a slice of the tag.
fn inner_of(xml: &str) -> String {
    let Some(open) = xml.find('>') else {
        return String::new();
    };
    if xml[..open].ends_with('/') {
        return String::new();
    }
    let Some(close) = xml.rfind("</") else {
        return String::new();
    };
    if close <= open {
        return String::new();
    }
    xml[open + 1..close].to_string()
}

/// A dotted version as numbers, so `2.2` sorts after `2.0` and `2.10` after
/// `2.9`. A segment that is not a number ends the comparison there, which is
/// what keeps a development drop from sorting as a release.
fn version_rank(version: &str) -> Vec<u32> {
    version
        .split('.')
        .map(|part| part.parse::<u32>().unwrap_or(0))
        .collect()
}
