//! Container-level attributes shared by the three derives.

use proc_macro2::TokenStream;
use quote::quote;
use syn::DeriveInput;

use crate::attrs::{MaxOccurs, Meta, doc_of};

pub const CONTAINER_ATTRS: &[&str] = &[
    "package",
    "version",
    "schema_version",
    "ns",
    "prefix",
    "uri",
    "xml_name",
    "object_type",
    "json_name",
    "top_level",
    "nestable",
    "object_ref",
    "abstract_type",
    "open",
    "other_pattern",
    "content_type",
    "qualified_type",
    "root_element",
    "base",
    "subtypes",
];

/// The `#[energyml(...)]` attributes on a type.
pub struct Container {
    pub name: String,
    pub package: String,
    pub version: String,
    /// Version as identifiers spell it: `2.0.1` becomes `2.0`.
    pub schema_version: String,
    pub xml_name: String,
    /// Complex type name, verbatim - `obj_Activity` keeps its prefix. Defaults
    /// to `xml_name`, which is right for every package that does not use the
    /// prefix.
    pub object_type: String,
    /// `object_type` without `obj_`: the cross-version grouping key.
    pub type_key: String,
    pub doc: String,
    pub ns: TokenStream,
    pub content_type: TokenStream,
    pub qualified_type: TokenStream,
    /// `Some("FlowTestActivity")` for a part written under an ancestor's
    /// element with `xsi:type` - see `TypeInfo::root_element`.
    pub root_element: TokenStream,
    pub top_level: bool,
    pub nestable: bool,
    /// `eml:DataObjectReference` and the types extending it.
    pub object_ref: bool,
    pub is_abstract: bool,
    pub is_open: bool,
    /// What an open enumeration's free text must still match - the widening
    /// union member's own pattern. `None` when the schema puts none.
    pub other_pattern: Option<String>,
    pub base: TokenStream,
    pub subtypes: TokenStream,
}

impl Container {
    pub fn parse(input: &DeriveInput) -> syn::Result<Self> {
        let meta = Meta::parse(&input.attrs)?;
        meta.deny_unknown(CONTAINER_ATTRS, "container")?;

        let name = input.ident.to_string();
        let package = meta.string("package")?.ok_or_else(|| {
            syn::Error::new(
                meta.span(),
                "missing `package = \"...\"` (eml, resqml, witsml or prodml)",
            )
        })?;
        let version = meta
            .string("version")?
            .ok_or_else(|| syn::Error::new(meta.span(), "missing `version = \"...\"`"))?;
        let xml_name = meta.string("xml_name")?.unwrap_or_else(|| name.clone());
        // The complexType name, which is what identifiers are built from. Only
        // RESQML 2.0.1 and common 2.0 need it stated, because only they name
        // the element and the type differently.
        let object_type = meta.string("object_type")?.unwrap_or_else(|| xml_name.clone());
        let type_key = object_type
            .strip_prefix("obj_")
            .unwrap_or(&object_type)
            .to_string();
        let schema_version = meta
            .string("schema_version")?
            .unwrap_or_else(|| schema_version_of(&version));

        let content_type = match meta.string("content_type")? {
            Some(v) => quote!(Some(#v)),
            None => quote!(None),
        };
        let qualified_type = match meta.string("qualified_type")? {
            Some(v) => quote!(Some(#v)),
            None => quote!(None),
        };
        let root_element = match meta.string("root_element")? {
            Some(v) => quote!(Some(#v)),
            None => quote!(None),
        };

        let base = match meta.path("base")? {
            Some(p) => quote!(Some(<#p as ::energyml_core::reflect::EnergymlType>::type_info)),
            None => quote!(None),
        };

        let subtype_paths = meta.path_list("subtypes");
        let subtypes = if subtype_paths.is_empty() {
            quote!(&[])
        } else {
            let entries = subtype_paths
                .iter()
                .map(|p| quote!(<#p as ::energyml_core::reflect::EnergymlType>::type_info));
            quote!(&[#(#entries),*])
        };

        let ns = namespace_expr(&meta, None)?;
        let ns = match ns_is_explicit(&meta)? {
            true => ns,
            false => package_namespace(&package, meta.span())?,
        };

        Ok(Container {
            doc: doc_of(&input.attrs),
            name,
            package,
            version,
            schema_version,
            xml_name,
            object_type,
            type_key,
            ns,
            content_type,
            qualified_type,
            root_element,
            top_level: meta.flag("top_level"),
            nestable: meta.flag("nestable"),
            object_ref: meta.flag("object_ref"),
            is_abstract: meta.flag("abstract_type"),
            is_open: meta.flag("open"),
            other_pattern: meta.string("other_pattern")?,
            base,
            subtypes,
        })
    }

    /// The `TypeInfo { .. }` literal, minus the parts that differ per derive.
    ///
    /// `roles` is `Roles::EMPTY` for everything but structs: only a struct has
    /// fields that could carry identity.
    pub fn type_info_body(
        &self,
        kind: TokenStream,
        fields: TokenStream,
        variants: TokenStream,
        roles: TokenStream,
        make: TokenStream,
    ) -> TokenStream {
        let name = &self.name;
        let xml_name = &self.xml_name;
        let object_type = &self.object_type;
        let type_key = &self.type_key;
        let schema_version = &self.schema_version;
        let ns = &self.ns;
        let package = &self.package;
        let version = &self.version;
        let content_type = &self.content_type;
        let qualified_type = &self.qualified_type;
        let root_element = &self.root_element;
        let doc = &self.doc;
        let is_abstract = self.is_abstract;
        let is_top_level = self.top_level;
        let is_nestable = self.nestable;
        let is_object_ref = self.object_ref;
        let is_open = self.is_open;
        let other_pattern = match &self.other_pattern {
            Some(p) => quote!(::core::option::Option::Some(#p)),
            None => quote!(::core::option::Option::None),
        };
        let base = &self.base;
        let subtypes = &self.subtypes;

        quote! {
            ::energyml_core::reflect::TypeInfo {
                name: #name,
                xml_name: #xml_name,
                xml_ns: #ns,
                package: #package,
                version: #version,
                schema_version: #schema_version,
                object_type: #object_type,
                type_key: #type_key,
                content_type: #content_type,
                qualified_type: #qualified_type,
                root_element: #root_element,
                doc: #doc,
                kind: #kind,
                is_abstract: #is_abstract,
                is_top_level: #is_top_level,
                is_nestable: #is_nestable,
                is_object_ref: #is_object_ref,
                roles: #roles,
                base: #base,
                fields: #fields,
                variants: #variants,
                subtypes: #subtypes,
                is_open: #is_open,
                other_pattern: #other_pattern,
                make: #make,
            }
        }
    }
}

/// Version as identifiers spell it: `2.0.1` becomes `2.0`, `2.2` stays `2.2`,
/// and a development drop reports its release - `2.2_dev3` becomes `2.2`.
///
/// The drop's own schemas declare `version="2.2"`, so that is what a document
/// written against it carries; spelling `2.2_dev3` into content and qualified
/// types would make this workspace the only thing able to read its own output.
///
/// Mirrors `Package::schema_version` in the generator, and the pair is pinned
/// end to end by `energyml-schemas/tests/complete.rs` - a test over what was
/// actually generated, rather than over these two functions agreeing.
fn schema_version_of(version: &str) -> String {
    let bare = version.trim();
    let cut = bare.trim_start_matches(|c: char| !c.is_ascii_digit());
    let trimmed = if cut.contains('.') { cut } else { bare };
    let release = match trimmed.find(|c: char| !c.is_ascii_digit() && c != '.') {
        None => trimmed,
        Some(at) => trimmed[..at].trim_end_matches('.'),
    };
    let parts: Vec<&str> = release.split('.').collect();
    match parts.len() {
        0 => release.to_string(),
        1 => parts[0].to_string(),
        _ => format!("{}.{}", parts[0], parts[1]),
    }
}

fn ns_is_explicit(meta: &Meta) -> syn::Result<bool> {
    Ok(meta.string("ns")?.is_some() || meta.string("uri")?.is_some())
}

/// Resolve `ns = "eml"`, or an explicit `prefix` + `uri` pair, falling back to
/// the container's namespace and then to the package's.
pub fn namespace_expr(meta: &Meta, container: Option<&Container>) -> syn::Result<TokenStream> {
    if let Some(uri) = meta.string("uri")? {
        let prefix = meta.string("prefix")?.unwrap_or_default();
        return Ok(quote!(::energyml_core::ns::Namespace::new(#prefix, #uri)));
    }
    if let Some(short) = meta.string("ns")? {
        return package_namespace(&short, meta.span());
    }
    match container {
        Some(c) => Ok(c.ns.clone()),
        None => Ok(quote!(::energyml_core::ns::EML)),
    }
}

fn package_namespace(short: &str, span: proc_macro2::Span) -> syn::Result<TokenStream> {
    let konst = match short.to_ascii_lowercase().as_str() {
        "eml" | "common" => quote!(EML),
        "resqml" => quote!(RESQML),
        "witsml" => quote!(WITSML),
        "prodml" => quote!(PRODML),
        "xsi" => quote!(XSI),
        "xsd" => quote!(XSD),
        "gml" => quote!(GML),
        "xlink" => quote!(XLINK),
        "gco" => quote!(GCO),
        "gmd" => quote!(GMD),
        other => {
            return Err(syn::Error::new(
                span,
                format!(
                    "unknown namespace `{other}`; use a well-known prefix \
                     (eml, resqml, witsml, prodml, gml, xlink, gco, gmd) \
                     or give `prefix = \"..\", uri = \"..\"`"
                ),
            ));
        }
    };
    Ok(quote!(::energyml_core::ns::#konst))
}

/// Build an `Occurs { .. }` literal.
pub fn occurs_expr(min: u32, max: MaxOccurs) -> TokenStream {
    let max = match max {
        MaxOccurs::Bounded(n) => quote!(::energyml_core::reflect::MaxOccurs::Bounded(#n)),
        MaxOccurs::Unbounded => quote!(::energyml_core::reflect::MaxOccurs::Unbounded),
    };
    quote! {
        ::energyml_core::reflect::Occurs { min: #min, max: #max }
    }
}
