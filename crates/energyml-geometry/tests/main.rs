//! Every integration test of `energyml-geometry`, as **one** binary.
//!
//! Each file used to be a binary of its own, and each linked the generated
//! crates it reads. Together they filled the disk of GitLab's small runner
//! during `cargo test --workspace` (01/10: `ld terminated with signal 7 [Bus
//! error]`, then `No space left on device`), after other crates had already been merged the same way.
//!
//! **A new test file is a `mod` line here**, otherwise it does not run. A file
//! that installs a process-wide default (a `set_default`, a global allocator)
//! keeps a binary of its own, as a `[[test]]` in `Cargo.toml`.

mod build;
mod connection;
mod corpus;
mod cut_many;
mod cut_sources;
mod declared;
mod defined;
mod derived;
mod drawable;
mod fixtures;
mod heads;
mod package_scene;
mod plane;
mod properties_of;
mod reference_versions;
mod unstructured;
mod well;
mod well_compound;
mod well_drop;
mod wells_versions;
mod wireframe;
mod witsml_trajectory;
