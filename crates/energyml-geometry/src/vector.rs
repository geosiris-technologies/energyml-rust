//! Three-dimensional vectors on [`Point`](crate::points::Point), and their
//! plan-view companions in [`plan`]: the one place the cross product and the
//! normalisation are written.
//!
//! The functions live in `energyml_geostat::vector`, the lowest crate that
//! every caller can depend on (it depends on no EnergyML crate, is pure Rust and
//! compiles to wasm); this module re-exports them, so the path
//! `energyml_geometry::vector` keeps resolving. See
//! `docs/adr/0044-vectors-in-geostat.md`.
//!
//! The handedness is the one of the placed frames: right-handed, `x × y = z`,
//! z up.

pub use energyml_geostat::vector::*;
