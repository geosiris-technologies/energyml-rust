//! The EnergyML library in one dependency.
//!
//! `energyml` re-exports the public crates of the
//! [`energyml-rust`](https://github.com/geosiris-technologies/energyml-rust)
//! workspace under short module names, so that an application writes
//! `energyml::storage::EpcStore` instead of adding fifteen dependencies and
//! keeping their versions in step. It contains no code of its own: every item
//! is the item of the crate it comes from, and that crate's documentation is the
//! documentation.
//!
//! | Module | Crate | What it is |
//! |---|---|---|
//! | [`core`] | `energyml-core` | Runtime for the generated code: XML/JSON, reflection, validation, identifiers. No native dependency. |
//! | [`schemas`] | `energyml-schemas` | Every generated schema version (EML, RESQML, WITSML, PRODML) in one registry. |
//! | [`storage`] | `energyml-storage` | `EnergymlStorage` and `EpcStore`: EPC read, edit and rewrite; external arrays through backends. |
//! | [`domain`] | `energyml-domain` | Units, CRS, value arrays, statistics, property kinds, colours, tables. |
//! | [`geometry`] | `energyml-geometry` | Representations to points and topology: surfaces, grids, wells. |
//! | [`export`] | `energyml-export` | Geometry to OBJ, GeoJSON and VTK through a registry. |
//! | [`proj`] | `energyml-proj` | Reprojection between EPSG systems over `proj4rs`; wasm included. |
//! | [`cache`] | `energyml-cache` | What a package keeps of itself, layer by layer. |
//! | [`analysis`] | `energyml-analysis` | Computation on shapes: interpolation, contouring, measures over wells and grids. |
//! | [`geostat`] | `energyml-geostat` | Domain-neutral interpolation and meshing algorithms; depends on no EnergyML crate. |
//! | `build` | `energyml-build` | Create objects from geometry or foreign files: operations, plans, recipes. Feature `build`. |
//! | `providers` | `energyml-providers` | Query public geoscience APIs; request and response are pure. Feature `providers`. |
//! | [`rules`] | `energyml-rules` | The registry of what the library decides where the specification is silent or ambiguous. |
//! | [`sheet`] | `energyml-sheet` | A drilling sheet laid out in Rust and written as SVG. |
//! | [`section`] | `energyml-section` | Long and cross sections of a model along a trace, as SVG. |
//! | [`log`] | `energyml-log` | The one `tracing` subscriber a front end installs. |
//!
//! # Features
//!
//! None is on by default: **choose the schema versions you read.**
//!
//! ```toml
//! # RESQML 2.2 and the EML 2.3 it is built on, and nothing else
//! energyml = { version = "0.1", default-features = false, features = ["resqml22"] }
//! ```
//!
//! | Feature | Compiles | Also enables |
//! |---|---|---|
//! | `eml20` `eml21` `eml22` `eml23` | EML 2.0 to 2.3 | |
//! | `resqml20` | RESQML 2.0.1 (`2.0`, `2.0.1`, `resqml20.`, `resqml201.` all read it) | `eml20` |
//! | `resqml22` | RESQML 2.2 | `eml23` |
//! | `resqml22dev3` | RESQML 2.2_dev3, the development drop | `eml22` |
//! | `witsml20` `witsml21` | WITSML 2.0, 2.1 | `eml21`, `eml23` |
//! | `prodml20` `prodml22` `prodml23` | PRODML 2.0, 2.2, 2.3 | `eml21`, `eml23`, `eml23` |
//! | `eml` `resqml` `witsml` `prodml` | every version of one family | |
//! | `all` | every version | |
//!
//! A version that is not compiled is a **named refusal**, never a half read:
//! `schemas::missing_feature` answers "RESQML 2.2 is not compiled in: enable
//! the `resqml22` feature of energyml-schemas (or of energyml)".
//!
//! - `build` makes `energyml::build` available; it writes RESQML 2.2, EML 2.3,
//!   WITSML 2.1 and PRODML 2.3 and turns those four on;
//! - `providers` makes `energyml::providers` available (implies `build`);
//! - `hdf5` reads and writes external arrays in HDF5 (needs the HDF5 C library
//!   at build time);
//! - `png` renders a section view to PNG.
//!
//! # What is not here
//!
//! The generated schema crates (`energyml-resqml-2-2` and the others) are
//! reached through [`schemas`], which lists every release; the derive crates
//! (`energyml-derive`, `energyml-build-derive`) are the business of the crates
//! that use them; the `energyml` command is `energyml-cli`; the ETP stack is the
//! separate [`retp`](https://github.com/geosiris-technologies/retp) project.

/// Runtime for the generated code (`energyml-core`).
pub use energyml_core as core;

/// Every generated schema version in one registry (`energyml-schemas`).
pub use energyml_schemas as schemas;

/// `EnergymlStorage` and `EpcStore` (`energyml-storage`).
pub use energyml_storage as storage;

/// Units, CRS, value arrays, statistics, property kinds, colours, tables (`energyml-domain`).
pub use energyml_domain as domain;

/// Representations to points and topology (`energyml-geometry`).
pub use energyml_geometry as geometry;

/// Geometry to OBJ, GeoJSON and VTK (`energyml-export`).
pub use energyml_export as export;

/// Reprojection between EPSG systems (`energyml-proj`).
pub use energyml_proj as proj;

/// What a package keeps of itself (`energyml-cache`).
pub use energyml_cache as cache;

/// Computation on shapes (`energyml-analysis`).
pub use energyml_analysis as analysis;

/// Domain-neutral interpolation and meshing (`energyml-geostat`).
pub use energyml_geostat as geostat;

/// Create objects: operations, plans, recipes (`energyml-build`).
#[cfg(feature = "build")]
pub use energyml_build as build;

/// Query public geoscience APIs (`energyml-providers`).
#[cfg(feature = "providers")]
pub use energyml_providers as providers;

/// The registry of the library's own decisions (`energyml-rules`).
pub use energyml_rules as rules;

/// A drilling sheet written as SVG (`energyml-sheet`).
pub use energyml_sheet as sheet;

/// Long and cross sections of a model, as SVG (`energyml-section`).
pub use energyml_section as section;

/// The one `tracing` subscriber a front end installs (`energyml-log`).
pub use energyml_log as log;
