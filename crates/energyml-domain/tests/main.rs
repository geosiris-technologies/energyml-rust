//! Every integration test of `energyml-domain`, as **one** binary.
//!
//! Each file used to be a binary of its own, and each linked the generated
//! crates it reads. Together they filled the disk of GitLab's small runner
//! during `cargo test --workspace` (01/10: `ld terminated with signal 7 [Bus
//! error]`, then `No space left on device`), after other crates had already been merged the same way.
//!
//! **A new test file is a `mod` line here**, otherwise it does not run. A file
//! that installs a process-wide default (a `set_default`, a global allocator)
//! keeps a binary of its own, as a `[[test]]` in `Cargo.toml`.

mod attachments;
mod borrowed_crs;
mod brt;
mod channel_data_in_hdf5;
mod check_candidates;
mod color;
mod color_inherit;
mod color_scopes;
mod color_versions;
mod compound_crs;
mod corpus;
mod derived_colour;
mod fixtures;
mod geolts;
mod geometry_local_crs;
mod identity_guard;
mod inherit;
mod integrity;
mod metadata_nulls;
mod package_index;
mod package_index_versions;
mod prefetch;
mod property;
mod property_counts_in_check;
mod property_kind;
mod reference_follow;
mod relations;
mod rules;
mod schema_in_check;
mod series;
mod stats;
mod strata_by_md;
mod stratigraphic_column;
mod stratigraphy_versions;
mod subtree;
mod table;
mod trg_2_2;
mod uri_follow;
mod values;
mod well_strata;
mod wellbore_table;
