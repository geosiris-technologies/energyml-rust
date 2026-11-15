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

mod activity;
mod along_md;
mod attachments;
mod borrowed_crs;
mod brt;
mod channel_data_in_hdf5;
mod check_candidates;
mod color;
mod color_inherit;
mod color_log_mapping;
mod color_scopes;
mod color_versions;
mod component_axes;
mod compound_crs;
mod corpus;
mod crs_axes;
mod derived_colour;
mod facets;
mod fixtures;
mod forest;
mod geolts;
mod geometry_local_crs;
mod identity_guard;
mod inherit;
mod integrity;
mod integrity_tables;
mod matrix;
mod md_extent;
mod metadata_nulls;
mod null_value;
mod object_chip;
mod package_index;
mod package_index_versions;
mod prefetch;
mod property;
mod property_counts_in_check;
mod property_describe;
mod property_kind;
mod property_shape;
mod provenance_graph;

mod provenance_verbatim;
mod read_block;
mod reference_follow;
mod relations;
mod rules;
mod schema_in_check;
mod series;
mod slabs;
mod stats;
mod stats_describe;
mod stats_shapes;
mod stats_streaming;
mod strata_by_md;
mod stratigraphic_column;
mod stratigraphy_versions;
mod subtree;
mod table;
mod table_check;
mod table_class;
mod table_links;
mod table_resolve;
mod text_arrays;

mod table_cells;
mod table_keys;
mod table_rows;
mod table_sheet;

mod table_structure;
mod table_window;
mod trg_2_2;
mod units_families;
mod uri_follow;
mod value_count;
mod values;
mod well_strata;
mod wellbore_table;
