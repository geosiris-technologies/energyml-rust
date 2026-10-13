//! Runtime support for generated EnergyML crates.
//!
//! This crate holds everything the generated `eml` / `resqml` / `witsml` /
//! `prodml` crates lean on, and nothing schema-specific. It is deliberately free
//! of native dependencies so generated code stays usable from `wasm32`.
//!
//! What a generated type gets:
//!
//! - [`xml`] — namespace-aware XML reading and writing, with explicit control of
//!   prefixes and `xsi:type`. Serde handles JSON only.
//! - [`reflect`] — a `&'static TypeInfo` per type: fields, XSD cardinality,
//!   facets, documentation, accepted types for polymorphic fields.
//! - [`value`] — object-safe dynamic access, so a UI can walk and complete an
//!   instance it has no compile-time knowledge of.
//! - [`path`] — `Citation.Title` style access, optionally case-insensitive.
//! - [`validate`] — structural validation from the schema, plus a hook for
//!   business rules.
//! - [`registry`] — create and read objects by content type or qualified type.
//! - [`identifiers`] — UUID, content type, qualified type and ETP URI parsing.
//!
//! Generated code is written by `energyml-codegen`; the `energyml-derive` macros
//! turn the annotated structs it emits into implementations of the traits above.

#![forbid(unsafe_code)]

pub mod any_xml;
#[cfg(feature = "json")]
pub mod docs;
pub mod error;
pub mod identifiers;
pub mod identity;
#[cfg(feature = "json")]
pub mod json;
pub mod lexical;
pub mod measure;
pub mod naming;
pub mod ns;
pub mod path;
pub mod reflect;
pub mod registry;
pub mod validate;
pub mod value;
pub mod xml;

pub mod prelude;

pub use error::{Error, Result};

/// What every object this software writes says in `Citation.Format`.
///
/// EML 2.3 `Abstract.xsd` l. 393–399 documents the field as *"Software or
/// service that was used to originate the object and the file format created
/// [...] including the organization name, software application name and
/// software version [...] provided by the application that creates or edits
/// the data object"*, in the form
/// `organizationName:applicationName:applicationVersion/format`. The field is
/// mandatory (`minOccurs="1"`), but `String2000` has no `minLength`, so an
/// empty one passes the schema. Lutetia wrote 924 empty ones out of 1 350 until
/// 06/10/2026, from writers that each spelled their own value or none.
///
/// The `/format` suffix is left out on purpose. A citation is written before
/// anyone knows whether the object will be stored as an EPC part (XML) or pushed
/// as JSON over ETP, so any suffix would be a guess.
pub const CITATION_FORMAT: &str = concat!("Geosiris:energyml-rust:", env!("CARGO_PKG_VERSION"));

// Re-exported so generated code can name these types through `energyml_core`
// alone, without every generated crate having to depend on quick-xml directly.
pub use quick_xml;

/// The derive macros, re-exported so `use energyml_core::prelude::*` is the
/// only import generated code needs.
#[cfg(feature = "derive")]
pub use energyml_derive::{EnergymlChoice, EnergymlEnum, EnergymlObject};
