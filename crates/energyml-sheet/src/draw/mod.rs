//! The drawing every view shares - and nothing of EnergyML.
//!
//! [`DisplayList`] is what a view decides to draw: keyed elements in layers,
//! in user units (pixels or millimetres), y down. [`svg::write`] serialises it,
//! [`png`] rasterises it, [`hit()`] says what is under a point. [`ticks`],
//! [`spread`] and [`text`] are what a layout needs to place graduations and
//! labels; [`raster`] paints value grids through a colour the caller passes;
//! [`layered`] places a graph in layers.
//!
//! # No format here
//!
//! Nothing in this module names a format crate of the workspace (the core,
//! domain or storage crates) or another module of this crate (it reaches only
//! itself, by `crate::draw`): it is written to be moved into a crate of its
//! own without a rewrite (`tests/draw/no_format.rs` holds it). A colour is a
//! closure of the caller, never a colour map read here: the one door to
//! colours stays with the caller.

pub mod hit;
pub mod layered;
pub mod list;
pub mod png;
pub mod raster;
pub mod spread;
pub mod svg;
pub mod text;
pub mod ticks;

pub use hit::{Hit, hit};
pub use list::{
    Anchor, Colour, DisplayList, Element, FontWeight, Hatch, Item, Layer, LineCap, LineJoin, PageUnit, Paint,
    PathData, Rect, Segment, Stroke, SymbolKind, TextStyle,
};
pub use raster::Raster;
pub use svg::{DrawError, Metadata, MetadataError};
