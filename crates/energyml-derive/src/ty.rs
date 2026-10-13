//! Analysis of the Rust types the generator emits.
//!
//! Cardinality is read off the Rust type (`Option<T>` / `Vec<T>` / `T`) so the
//! generated annotations stay short; `min` / `max` attributes only need to
//! appear when the XSD says something the Rust type cannot.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{GenericArgument, PathArguments, Type};

/// How a field's item type is wrapped.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wrapper {
    /// `T` — exactly one.
    Plain,
    /// `Option<T>` — zero or one.
    Optional,
    /// `Vec<T>` — zero or more.
    Repeated,
}

/// A field's type, unwrapped.
pub struct FieldTy {
    /// Cardinality wrapper.
    pub wrapper: Wrapper,
    /// Item type with `Option`/`Vec` removed. `Box` is kept: it is a layout
    /// detail the serializer forwards through.
    pub inner: Type,
    /// Item type with `Box` also removed, used to name the type for reflection.
    pub bare: Type,
    /// Whether the item is boxed. The generator boxes fields that would
    /// otherwise make a struct infinitely sized.
    pub boxed: bool,
}

fn generic_arg(ty: &Type, wrapper: &str) -> Option<Type> {
    let Type::Path(tp) = ty else { return None };
    if tp.qself.is_some() {
        return None;
    }
    let seg = tp.path.segments.last()?;
    if seg.ident != wrapper {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    args.args.iter().find_map(|a| match a {
        GenericArgument::Type(t) => Some(t.clone()),
        _ => None,
    })
}

/// Split a field type into wrapper and item type.
pub fn analyze(ty: &Type) -> FieldTy {
    let (wrapper, inner) = if let Some(inner) = generic_arg(ty, "Option") {
        (Wrapper::Optional, inner)
    } else if let Some(inner) = generic_arg(ty, "Vec") {
        (Wrapper::Repeated, inner)
    } else {
        (Wrapper::Plain, ty.clone())
    };
    let unboxed = generic_arg(&inner, "Box");
    let boxed = unboxed.is_some();
    let bare = unboxed.unwrap_or_else(|| inner.clone());
    FieldTy {
        wrapper,
        inner,
        bare,
        boxed,
    }
}

/// The `ScalarKind` a primitive Rust type maps to, or `None` for generated
/// types.
pub fn scalar_kind(ty: &Type) -> Option<TokenStream> {
    let Type::Path(tp) = ty else { return None };
    let ident = tp.path.segments.last()?.ident.to_string();
    let kind = match ident.as_str() {
        "bool" => quote!(Bool),
        "i8" | "i16" | "i32" | "i64" | "isize" => quote!(Int),
        "u8" | "u16" | "u32" | "u64" | "usize" => quote!(UInt),
        "f32" | "f64" => quote!(Double),
        "String" | "str" => quote!(String),
        _ => return None,
    };
    Some(kind)
}

/// Whether a type is `String`, which every identity-carrying field is.
pub fn is_string(ty: &Type) -> bool {
    let Type::Path(tp) = ty else { return false };
    tp.path.segments.last().is_some_and(|s| s.ident == "String")
}

/// Map an explicit `scalar = "..."` override onto a `ScalarKind` variant.
pub fn scalar_kind_from_name(name: &str) -> Option<TokenStream> {
    let kind = match name.to_ascii_lowercase().as_str() {
        "bool" | "boolean" => quote!(Bool),
        "int" | "integer" | "long" | "short" | "byte" => quote!(Int),
        "uint" | "unsigned" | "unsignedint" | "unsignedlong" | "positiveinteger" => quote!(UInt),
        "double" | "float" | "decimal" => quote!(Double),
        "string" => quote!(String),
        "datetime" | "date" | "time" => quote!(DateTime),
        "binary" | "base64binary" | "hexbinary" => quote!(Binary),
        _ => return None,
    };
    Some(kind)
}
