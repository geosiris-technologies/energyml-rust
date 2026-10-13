//! XML namespaces used by the Energistics schemas.
//!
//! Namespace URIs do **not** identify a package version: every RESQML version
//! shares `.../resqmlv2`, every common version shares `.../commonv2`. Version is
//! carried by [`crate::reflect::TypeInfo::version`] and by the
//! contentType/qualifiedType strings, never by the URI alone.

/// A namespace binding: the prefix generated documents use, and its URI.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Namespace {
    /// Prefix written in serialized documents, e.g. `resqml`.
    pub prefix: &'static str,
    /// Namespace URI.
    pub uri: &'static str,
}

impl Namespace {
    /// Build a namespace binding.
    pub const fn new(prefix: &'static str, uri: &'static str) -> Self {
        Self { prefix, uri }
    }

    /// Look up one of the well-known Energistics namespaces by prefix.
    ///
    /// Generated code uses the short form (`ns = "eml"`); anything else must be
    /// given as an explicit prefix/URI pair.
    pub fn well_known(prefix: &str) -> Option<Namespace> {
        WELL_KNOWN.iter().copied().find(|n| n.prefix == prefix)
    }

    /// Qualify a local name with this namespace's prefix.
    pub fn qualify(&self, local: &str) -> String {
        if self.prefix.is_empty() {
            local.to_string()
        } else {
            format!("{}:{}", self.prefix, local)
        }
    }
}

/// Energistics common (a.k.a. `eml`) namespace, shared by all common versions.
pub const EML: Namespace = Namespace::new("eml", "http://www.energistics.org/energyml/data/commonv2");
/// RESQML namespace, shared by 2.0.1, 2.2_dev3 and 2.2.
pub const RESQML: Namespace = Namespace::new("resqml", "http://www.energistics.org/energyml/data/resqmlv2");
/// WITSML namespace, shared by 2.0 and 2.1.
pub const WITSML: Namespace = Namespace::new("witsml", "http://www.energistics.org/energyml/data/witsmlv2");
/// PRODML namespace, shared by 2.0 and 2.2.
pub const PRODML: Namespace = Namespace::new("prodml", "http://www.energistics.org/energyml/data/prodmlv2");
/// XML Schema instance namespace, used for `xsi:type` polymorphism.
pub const XSI: Namespace = Namespace::new("xsi", "http://www.w3.org/2001/XMLSchema-instance");
/// XML Schema namespace.
pub const XSD: Namespace = Namespace::new("xsd", "http://www.w3.org/2001/XMLSchema");
/// GML 3.2.1, vendored inside each common version.
pub const GML: Namespace = Namespace::new("gml", "http://www.opengis.net/gml/3.2");
/// XLink 1.0, vendored inside each common version.
pub const XLINK: Namespace = Namespace::new("xlink", "http://www.w3.org/1999/xlink");
/// ISO 19139 geographic common extension.
pub const GCO: Namespace = Namespace::new("gco", "http://www.isotc211.org/2005/gco");
/// ISO 19139 metadata.
pub const GMD: Namespace = Namespace::new("gmd", "http://www.isotc211.org/2005/gmd");

const WELL_KNOWN: &[Namespace] = &[EML, RESQML, WITSML, PRODML, XSI, XSD, GML, XLINK, GCO, GMD];

/// All namespaces generated code may reference.
pub fn well_known_namespaces() -> &'static [Namespace] {
    WELL_KNOWN
}
