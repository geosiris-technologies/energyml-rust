//! Derive macros for generated EnergyML types.
//!
//! `energyml-codegen` emits plain annotated Rust; these macros turn each item
//! into implementations of the `energyml-core` traits. Keeping the behaviour in
//! macros rather than in the generator means a change to (say) how `xsi:type` is
//! written is a change to one file here, not a regeneration of every crate.
//!
//! Three derives cover the whole model:
//!
//! - [`macro@EnergymlObject`] — an XSD `complexType`.
//! - [`macro@EnergymlEnum`] — an XSD `simpleType` enumeration, `open` for the
//!   Energistics `...Ext` types that also accept free text.
//! - [`macro@EnergymlChoice`] — an abstract type or substitution group, selected
//!   on the wire by `xsi:type`.
//!
//! ```ignore
//! #[derive(Default, EnergymlObject, Serialize, Deserialize)]
//! #[energyml(
//!     package = "resqml", version = "2.2",
//!     top_level,
//!     content_type = "application/x-resqml+xml;version=2.2;type=TriangulatedSetRepresentation",
//!     qualified_type = "resqml22.TriangulatedSetRepresentation",
//! )]
//! pub struct TriangulatedSetRepresentation {
//!     #[energyml(attr)]
//!     pub uuid: String,
//!     /// Citation is declared by the common schema, so it keeps the eml namespace.
//!     #[energyml(elem, ns = "eml")]
//!     pub citation: Citation,
//!     #[energyml(elem, min = 1)]
//!     pub triangle_patch: Vec<TrianglePatch>,
//! }
//! ```

use proc_macro::TokenStream;
use syn::{Data, DeriveInput, parse_macro_input};

mod attrs;
mod common;
mod gen_choice;
mod gen_enum;
mod gen_struct;
mod ty;

/// Implement the EnergyML traits for a struct mapped from an XSD `complexType`.
///
/// Container attributes: `package`, `version` (both required), `ns`, `prefix`,
/// `uri`, `xml_name`, `top_level`, `nestable`, `abstract_type`,
/// `content_type`, `qualified_type`, `base = path::To::Type`,
/// `subtypes(A, B, ...)`.
///
/// Field attributes: `attr` / `elem` / `text` (default `elem`), `skip`,
/// `xml_name`, `json_name`, `ns`, `min`, `max`, `scalar`, and the XSD facets
/// `min_inclusive`, `max_inclusive`, `min_exclusive`, `max_exclusive`,
/// `min_length`, `max_length`, `pattern`.
///
/// Cardinality is read off the Rust type — `Option<T>` is optional, `Vec<T>` is
/// repeated — so `min` and `max` are only needed when the schema says something
/// the Rust type cannot express (`minOccurs="2"`, say).
#[proc_macro_derive(EnergymlObject, attributes(energyml))]
pub fn derive_energyml_object(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let result = match &input.data {
        Data::Struct(data) => gen_struct::expand(&input, data),
        _ => Err(syn::Error::new_spanned(
            &input.ident,
            "EnergymlObject applies to structs; use EnergymlEnum or EnergymlChoice for enums",
        )),
    };
    result.unwrap_or_else(syn::Error::into_compile_error).into()
}

/// Implement the EnergyML traits for an XSD `simpleType` enumeration.
///
/// Add `#[energyml(open)]` and a variant marked `#[energyml(other)]` holding a
/// `String` for the Energistics `...Ext` types, which accept values outside the
/// declared set.
#[proc_macro_derive(EnergymlEnum, attributes(energyml))]
pub fn derive_energyml_enum(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let result = match &input.data {
        Data::Enum(data) => gen_enum::expand(&input, data),
        _ => Err(syn::Error::new_spanned(
            &input.ident,
            "EnergymlEnum applies to enums",
        )),
    };
    result.unwrap_or_else(syn::Error::into_compile_error).into()
}

/// Implement the EnergyML traits for an abstract type or substitution group.
///
/// Each variant holds one concrete subtype and declares the `xsi:type` that
/// selects it. Reflection is transparent: callers see the concrete subtype.
#[proc_macro_derive(EnergymlChoice, attributes(energyml))]
pub fn derive_energyml_choice(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let result = match &input.data {
        Data::Enum(data) => gen_choice::expand(&input, data),
        _ => Err(syn::Error::new_spanned(
            &input.ident,
            "EnergymlChoice applies to enums",
        )),
    };
    result.unwrap_or_else(syn::Error::into_compile_error).into()
}
