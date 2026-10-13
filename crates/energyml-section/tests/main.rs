//! The section view's tests, in one binary (ADR-0016: one crate, one
//! test binary — a new test file is a `mod` line here, or it does not run).
//!
//! Run them alone: `cargo test --workspace --test section_view <filter>`.

mod assemble;
mod corridor;
mod crosses;
mod erosion;
mod finite;
mod crossings;
mod grids;
mod inputs;
mod layout;
mod markers;
mod panels;
mod model;
mod paint;
mod paint_surfaces;
mod strata_legend;
mod unit_colours;
mod styles;
mod png;
mod surfaces;
mod svg;
mod wells;
mod whole;
mod zoom;
