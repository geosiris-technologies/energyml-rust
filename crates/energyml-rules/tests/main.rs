//! Every integration test of `energyml-rules`, in **one** binary (ADR-0016: one
//! crate, one test binary): `autotests = false`, one
//! `[[test]]`. A new test file is a new `mod` line below — Cargo no longer
//! finds it alone.
//!
//! Run them: `cargo test --workspace --test house`.

// The loader `build.rs` compiles the rules with, included here as well: what
// the build refuses and what these tests try to break are the same code.
#[allow(dead_code)]
#[path = "../build/loader.rs"]
mod loader;

mod crosscheck;
mod loading;
mod registry;
