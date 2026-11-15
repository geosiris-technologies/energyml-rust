//! A drilling sheet: a well's 2D cross-section, drawn the way the BRGM's
//! Banque du Sous-Sol draws one - a depth column, bands of stratigraphy or
//! lithology beside it, and labels - computed in Rust and written as SVG.
//!
//! # Why this crate exists
//!
//! The front end lacked a 2D well view, and it should look like the BSS
//! validated well sections - a proportional
//! depth axis, sections coloured by stratigraphy, and property columns a
//! user can pick and order. The whole design is written down and settled
//! in the sheet design; that design is this crate's specification, not a
//! summary of it, and every non-trivial choice in the code states the rule
//! that decided it.
//!
//! # What this increment is, and is not
//!
//! The work is ordered in seven steps; this crate currently covers the
//! first four:
//!
//! 1. [`layout`] - the page layout, in data, tested in numbers: a
//!    [`layout::Board`] plus [`layout::Params`] become a [`layout::Layout`],
//!    through the two registries (track renderers and proportion modes).
//! 2. [`text`] - measuring and wrapping against a real font's widths,
//!    which [`layout`] uses to size a column and cut its text.
//! 3. [`picture`] translates a [`layout::Layout`] into a display list, and [`svg`] writes it out as an SVG string - a
//!    layout untested against a real serialiser is a layout no one has
//!    looked at.
//! 4. [`read`] - [`read::board_of_frame`] turns a package's
//!    `WellboreFrameRepresentation` into a [`layout::Board`]: the MD column,
//!    one track per stratigraphic rank, one track per interval property -
//!    reusing `energyml_domain::well_strata` and `energyml_domain::table`
//!    rather than reading a frame or a lookup table a second time.
//! 5. [`draw`] - the drawing every view shares: a keyed display list, its
//!    SVG and PNG, graduations, label spreading, text measurement, rasters
//!    painted through a caller's colour, a layered graph layout and the hit
//!    test. It knows no EnergyML type, so that it can leave this crate later.
//!
//! Not here yet, on purpose: curves (decimated properties and WITSML
//! channels), PDF export, lithology patterns (still under a licensing review), the
//! front end, and the CLI command.
//!
//! # No native dependency
//!
//! Like every crate below a WebAssembly front end, this one carries none: `layout`
//! and `text` are pure computation, and `svg` writes a `String`. wasm is a
//! hard requirement, and nothing here has a reason to
//! break it.
//! The `png` feature adds `resvg` with its raster images, pure Rust; the PNG is
//! always the SVG rasterised.

pub mod colour;
pub mod draw;
pub mod layout;
pub mod picture;
pub mod read;
pub mod svg;
pub mod text;
