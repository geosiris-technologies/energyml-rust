//! Every integration test of `energyml-tests`, as **one** binary.
//!
//! Each file used to be a binary of its own, and each linked the generated
//! crates it reads. Together they filled the disk of GitLab's small runner
//! during `cargo test --workspace` (01/10: `ld terminated with signal 7 [Bus
//! error]`, then `No space left on device`), after other crates had already been merged the same way.
//!
//! **A new test file is a `mod` line here**, otherwise it does not run. A file
//! that installs a process-wide default (a `set_default`, a global allocator)
//! keeps a binary of its own, as a `[[test]]` in `Cargo.toml`.

mod construction;
mod content_models;
mod crossaudit;
mod custom_data;
mod firewall;
mod fixtures;
mod generated_resqml;
mod identity;
mod inheritance;
mod introspection;
mod relations;
mod repairs;
mod requirements;
mod rules_trg;
mod validation_audit;
