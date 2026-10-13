# 0007 - `core` has no native dependency

Status: accepted

## Context

The library must run in a browser (WebAssembly) as well as natively. Native
libraries such as HDF5 or PROJ cannot be linked in a WebAssembly build.

## Decision

- `energyml-core` and every crate below the storage layer's array backends are
  free of native dependencies. WebAssembly is a hard requirement checked in CI.
- HDF5 and similar libraries live in the storage layer, behind optional features
  (`hdf5`), behind the `ArrayBackend` trait. A WebAssembly host supplies its own
  backend.
- Coordinate reprojection is pure Rust (`proj4rs`), not a binding to PROJ.
  `energyml-proj` refuses a definition that carries no datum rather than applying
  one silently.
- Computation crates (interpolation, section view, well sheet) compute over plain
  slices and return `String`s or numbers; they read no files.

## Consequences

- One code base serves the command line, Python and the browser.
- Anything needing a native library is optional and must say so when missing
  (see ADR 0005).
- A pure-Rust reprojection must be validated against PROJ; it agrees to the
  millimetre on the systems tested.
