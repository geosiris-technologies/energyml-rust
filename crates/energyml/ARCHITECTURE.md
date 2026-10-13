# energyml — architecture

Related decision: none of its own; the facade only reflects the layering described in [ARCHITECTURE.md](../../ARCHITECTURE.md).

## Purpose and position

A facade crate: `src/lib.rs` is a list of `pub use energyml_<x> as <x>;`. It sits above every public library crate and beside the faces (`energyml-cli`, `energyml-python`). It exists so that an application takes one dependency and one version.

## Rules

- **No code.** If an item is missing, it is missing in the crate it belongs to. The facade never wraps, renames or adds.
- **A new public library crate is one line here**: a dependency in `Cargo.toml`, a `pub use` in `lib.rs`, a row in the table of the crate documentation.
- **Not re-exported**: the private crates; the derive crates (`energyml-derive`, `energyml-build-derive`); the generated schema crates, which `energyml::schemas` lists; the ETP stack, which is the separate `retp` repository.
- **No default feature, and no default schema version.** The façade depends on `energyml-schemas` with `default-features = false`; one feature per version (`eml20`…`eml23`, `resqml20`, `resqml22`, `resqml22dev3`, `witsml20`, `witsml21`, `prodml20`, `prodml22`, `prodml23`), the families (`eml`, `resqml`, `witsml`, `prodml`) and `all` only forward to the feature of the same name in `energyml-schemas`, which holds the optional dependencies. `build` and `providers` switch on an optional crate (`build` also turns on RESQML 2.2, EML 2.3, WITSML 2.1 and PRODML 2.3, the versions it writes; `providers` implies `build`); `hdf5` and `png` only forward to `energyml-storage` and `energyml-section`.
- **The library target is named `energyml`, like the binary of `energyml-cli`.** The library is an rlib and writes no program database, so the two do not collide at link time; the binary sets `doc = false` so that `cargo doc` does not write both to `target/doc/energyml/`.
