//! Every integration test of `energyml-build`, in **one** binary.
//!
//! Cargo makes a binary per file of `tests/`, and each one is linked against
//! the whole workspace — HDF5 included under `--features hdf5`. Measured on
//! 29/09 in the `hdf5` CI step: 84 binaries, **67 min** for the step, of which
//! **36 s** ran tests; the rest was compiling and linking. One binary is one
//! link (`Cargo.toml`: `autotests = false`, one `[[test]]`).
//!
//! Each former binary is a module here, unchanged: a file's `#![cfg(…)]`
//! still gates the whole file, and its `#[path]` still reads next to it.
//! Run one: `cargo test --workspace --test build lutetia::`.
//!
//! A new test file is a new line below — Cargo no longer finds it alone.
//!
//! **Except a file that installs a process-wide default** — `geolts::set_default`,
//! `property_kind::set_default`, or includes `support/geolts_extract.rs`.
//! Such a file assumes it is the whole process: sharing one made Lutetia read a
//! 30-concept extract of the BRGM chart instead of the chart (29/09,
//! `the chart has no concept 1289`). It keeps a binary of its own
//! (`Cargo.toml`: `contacts_rank`, `stratigraphy`,
//! `earth_model_from_stratigraphy`), and `no_module_here_installs_a_default`
//! below refuses one that would come back.

mod attach;
mod blocked_wellbore;
mod boreholes;
mod business_associate;
mod cardinality;
mod change_frame;
mod clip;
mod collection;
mod colormap_levels;
mod computed_tops;
mod conditions;
mod contacts;
mod contacts_well_origin;
mod cross_section;
mod datum_in_compound;
mod declare;
mod delete_nodes;
mod densify;
mod describe;
mod earth_model;
mod edit_object;
mod entries;
mod error_model;
mod export_media;
mod extend;
mod extend_boreholes;
mod fault_from_sticks;
mod fault_throw;
mod faulted_grid;
mod faults;
mod galaxy;
mod gathered_columns;
mod gives;
mod graphical_information;
mod grid_properties;
mod horizon_law;
mod identity_versions;
mod ijk_grid;
mod injection_test;
mod injectivity;
mod interpolate_onto;
mod intersect;
mod isocontour_bands;
mod kinds;
mod local_grid;
mod loft;
mod lutetia;
mod lutetia_package;
mod makes;
mod manifest;
mod mba;
mod measure;
mod media_parts;
mod mesh;
mod new_version;
mod operations;
mod osdu;
mod package;
mod polyline_through;
mod polylines_from_cloud;
mod prodml_doublet;
mod prodml_rest;
mod project;
mod properties_from_table;
mod property_time_series;
mod pta;
mod purpose;
mod read_versions;
mod recipe;
mod refusals;
mod representation_set;
mod roundtrip_mesh;
mod section_view;
mod selection;
mod series_readback;
mod shape_placement;
mod shapes;
mod sigeol_chain;
mod source_not_in_identity;
mod streaming;
mod structural_organisation;
mod subrepresentation;
mod surface_framework;
mod tabular;
mod terrain;
mod thickness;
mod time_series;
mod transform;
mod well;
mod well_logs;
mod well_plan;
mod wellheads;
mod wells_from_polylines;
mod wells_from_table;
mod witsml_logs;
mod witsml_logs_in_hdf5;
mod witsml_trajectory;
mod witsml_wells;
mod written;
mod zone_statistics;

/// No module of this binary installs a process-wide default — see the module
/// doc. Read as text: the defaults are set once per process, so the failure
/// is order-dependent and a run can pass by luck; this cannot.
///
/// To break: add `mod stratigraphy;` above.
#[test]
fn no_module_here_installs_a_default() {
    let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let main = std::fs::read_to_string(here.join("main.rs")).expect("this file");
    let modules: Vec<&str> = main
        .lines()
        .filter_map(|line| {
            line.trim_start_matches("#[cfg(feature = \"hdf5\")] ")
                .strip_prefix("mod ")
        })
        .map(|rest| rest.trim_end_matches(';'))
        .collect();
    assert!(modules.len() > 50, "the module list was read: {}", modules.len());
    for name in modules {
        let text =
            std::fs::read_to_string(here.join(format!("{name}.rs"))).expect("a declared module has its file");
        for installer in ["set_default(", "mod geolts_extract"] {
            assert!(
                !text.contains(installer),
                "`tests/{name}.rs` has `{installer}`: it needs a binary of its own (see this file's doc)"
            );
        }
    }
}
