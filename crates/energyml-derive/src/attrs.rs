//! Parsing of `#[energyml(...)]` attributes.

use std::collections::{HashMap, HashSet};

use syn::spanned::Spanned;
use syn::{Attribute, Lit, Path, Token};

/// A value written as `key = ...`.
pub enum MetaValue {
    /// A literal, e.g. `xml_name = "Citation"`.
    Lit(Lit),
    /// A path, e.g. `base = eml::AbstractObject`.
    Path(Path),
}

/// Everything parsed out of the `#[energyml(...)]` attributes on one item.
#[derive(Default)]
pub struct Meta {
    flags: HashSet<String>,
    values: HashMap<String, MetaValue>,
    lists: HashMap<String, Vec<Path>>,
    span: Option<proc_macro2::Span>,
}

impl Meta {
    /// Collect every `#[energyml(...)]` attribute on an item.
    pub fn parse(attrs: &[Attribute]) -> syn::Result<Self> {
        let mut out = Meta::default();
        for attr in attrs {
            if !attr.path().is_ident("energyml") {
                continue;
            }
            out.span = Some(attr.span());
            attr.parse_nested_meta(|meta| {
                let key = meta
                    .path
                    .get_ident()
                    .ok_or_else(|| meta.error("expected a simple identifier"))?
                    .to_string();

                if meta.input.peek(Token![=]) {
                    let value = meta.value()?;
                    let parsed = if value.peek(Lit) {
                        MetaValue::Lit(value.parse()?)
                    } else {
                        MetaValue::Path(value.parse()?)
                    };
                    out.values.insert(key, parsed);
                } else if meta.input.peek(syn::token::Paren) {
                    let content;
                    syn::parenthesized!(content in meta.input);
                    let paths = content.parse_terminated(Path::parse_mod_style, Token![,])?;
                    out.lists.insert(key, paths.into_iter().collect());
                } else {
                    out.flags.insert(key);
                }
                Ok(())
            })?;
        }
        Ok(out)
    }

    /// Span to attach errors to.
    pub fn span(&self) -> proc_macro2::Span {
        self.span.unwrap_or_else(proc_macro2::Span::call_site)
    }

    /// Whether a bare flag such as `top_level` is present.
    pub fn flag(&self, key: &str) -> bool {
        self.flags.contains(key)
    }

    /// A `key = "string"` value.
    pub fn string(&self, key: &str) -> syn::Result<Option<String>> {
        match self.values.get(key) {
            None => Ok(None),
            Some(MetaValue::Lit(Lit::Str(s))) => Ok(Some(s.value())),
            Some(MetaValue::Lit(l)) => Err(syn::Error::new(l.span(), format!("`{key}` must be a string"))),
            Some(MetaValue::Path(p)) => Err(syn::Error::new(p.span(), format!("`{key}` must be a string"))),
        }
    }

    /// A `key = <integer>` value.
    pub fn u32(&self, key: &str) -> syn::Result<Option<u32>> {
        match self.values.get(key) {
            None => Ok(None),
            Some(MetaValue::Lit(Lit::Int(i))) => Ok(Some(i.base10_parse()?)),
            Some(other) => Err(syn::Error::new(
                value_span(other),
                format!("`{key}` must be an integer"),
            )),
        }
    }

    /// A `key = <number>` value, accepting both integer and float spellings.
    pub fn f64(&self, key: &str) -> syn::Result<Option<f64>> {
        match self.values.get(key) {
            None => Ok(None),
            Some(MetaValue::Lit(Lit::Float(f))) => Ok(Some(f.base10_parse()?)),
            Some(MetaValue::Lit(Lit::Int(i))) => Ok(Some(i.base10_parse::<i64>()? as f64)),
            // Facets are often carried through the generator as strings.
            Some(MetaValue::Lit(Lit::Str(s))) => s
                .value()
                .trim()
                .parse::<f64>()
                .map(Some)
                .map_err(|e| syn::Error::new(s.span(), format!("`{key}` is not a number: {e}"))),
            Some(other) => Err(syn::Error::new(
                value_span(other),
                format!("`{key}` must be a number"),
            )),
        }
    }

    /// A `key = path::To::Type` value.
    pub fn path(&self, key: &str) -> syn::Result<Option<Path>> {
        match self.values.get(key) {
            None => Ok(None),
            Some(MetaValue::Path(p)) => Ok(Some(p.clone())),
            Some(MetaValue::Lit(l)) => Err(syn::Error::new(l.span(), format!("`{key}` must be a type path"))),
        }
    }

    /// A `key(A, B, C)` list of type paths.
    pub fn path_list(&self, key: &str) -> Vec<Path> {
        self.lists.get(key).cloned().unwrap_or_default()
    }

    /// `max = 1`, `max = "unbounded"` or absent.
    pub fn max_occurs(&self) -> syn::Result<Option<MaxOccurs>> {
        match self.values.get("max") {
            None => Ok(None),
            Some(MetaValue::Lit(Lit::Int(i))) => Ok(Some(MaxOccurs::Bounded(i.base10_parse()?))),
            Some(MetaValue::Lit(Lit::Str(s))) => {
                let v = s.value();
                if v.eq_ignore_ascii_case("unbounded") {
                    Ok(Some(MaxOccurs::Unbounded))
                } else {
                    v.parse::<u32>()
                        .map(|n| Some(MaxOccurs::Bounded(n)))
                        .map_err(|_| syn::Error::new(s.span(), "`max` must be an integer or \"unbounded\""))
                }
            }
            Some(other) => Err(syn::Error::new(
                value_span(other),
                "`max` must be an integer or \"unbounded\"",
            )),
        }
    }

    /// Reject anything not in `allowed`, so a typo in generated code fails loudly
    /// instead of silently changing the emitted schema metadata.
    pub fn deny_unknown(&self, allowed: &[&str], context: &str) -> syn::Result<()> {
        let known: HashSet<&str> = allowed.iter().copied().collect();
        for key in self
            .flags
            .iter()
            .chain(self.values.keys())
            .chain(self.lists.keys())
        {
            if !known.contains(key.as_str()) {
                return Err(syn::Error::new(
                    self.span(),
                    format!(
                        "unknown energyml {context} attribute `{key}`; expected one of: {}",
                        allowed.join(", ")
                    ),
                ));
            }
        }
        Ok(())
    }
}

/// Upper cardinality bound, mirroring `energyml_core::reflect::MaxOccurs`.
#[derive(Clone, Copy)]
pub enum MaxOccurs {
    /// A finite bound.
    Bounded(u32),
    /// `maxOccurs="unbounded"`.
    Unbounded,
}

fn value_span(value: &MetaValue) -> proc_macro2::Span {
    match value {
        MetaValue::Lit(l) => l.span(),
        MetaValue::Path(p) => p.span(),
    }
}

/// Concatenate `///` doc comments into a single string literal.
pub fn doc_of(attrs: &[Attribute]) -> String {
    let mut lines = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        if let syn::Meta::NameValue(nv) = &attr.meta
            && let syn::Expr::Lit(expr) = &nv.value
            && let Lit::Str(s) = &expr.lit
        {
            lines.push(s.value().trim().to_string());
        }
    }
    lines.join("\n")
}
