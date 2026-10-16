//! Every integration test of `energyml-storage`, as **one** binary.
//!
//! Each file used to be a binary of its own, and each linked the generated
//! crates it reads. Together they filled the disk of GitLab's small runner
//! during `cargo test --workspace` (01/10: `ld terminated with signal 7 [Bus
//! error]`, then `No space left on device`), after other crates had already been merged the same way.
//!
//! **A new test file is a `mod` line here**, otherwise it does not run. A file
//! that installs a process-wide default (a `set_default`, a global allocator)
//! keeps a binary of its own, as a `[[test]]` in `Cargo.toml` - here `hdf5_cache`,
//! which counts the opens of the one HDF5 cache of the process and bounds it.

mod absent;
mod absent_version;
mod agnostic;
mod array_shape;
mod attachments;
mod byte_order;
mod change_journal;
mod contract_epc;
mod contract_witsml;
mod corpus;
mod discovery;
mod drops;
mod empty_package;
mod external_forms;
mod extract;
mod fixtures;
mod hdf5_write;
mod kinds;
mod media;
mod mistyped;
mod nested_reference;
mod object_versions;
mod package_lookups;
mod proxy_files;
mod reference_candidates;
mod reindex;
mod rename_file;
mod renamed;
mod repair;
mod revision;
mod search;
mod substituted_root;
mod versions;
mod window;
mod writer;
