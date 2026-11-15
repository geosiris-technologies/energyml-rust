//! Writing a [`crate::layout::Layout`] as SVG - the sheet's one point of output: a web page inserts these bytes
//! unchanged, the CLI writes them unchanged, and a PNG export reads *this* string rather than the layout a second
//! time.
//!
//! There is **one SVG writer** in this crate, [`crate::draw::svg::write`]. The sheet goes through it:
//! [`crate::picture::of_layout`] translates the layout into a display list, and this module only calls the two in
//! turn. The units (millimetres, the font size in user units), the horizontal layout, the groups (one
//! `<g data-layer>` per layer: `header`, `sections`, `ruler`, `tracks`) and the keys are in [`crate::picture`];
//! the escaping, the font family and the number formats are the writer's.
//!
//! # What the one writer changed in the document
//!
//! - no XML declaration (`<?xml ...?>`) and no `font-size` on the root: each text carries its own size;
//! - `<g id="header">` and `<g id="body" transform="translate(...)">` are the layers `header` and `sections`,
//!   `ruler`, `tracks`, whose coordinates are the page's own (no translation);
//! - `data-md-top`, `data-md-base`, `data-uri`, `data-row` and `data-blank` are kept on every cell group, and a
//!   `data-key` joins them; the depth labels carry `data-md`;
//! - the title and the column headers are bold again (`font-weight="bold"`, [`crate::draw::FontWeight`]); the mode note is
//!   upright, no longer italic: a display list's text has a weight and no slant.

use crate::draw::DrawError;
use crate::layout::Layout;
use crate::picture::{PictureStyle, of_layout};

/// Write `layout` as a complete SVG document: black on transparent, with no object to key the depth labels by
/// ([`PictureStyle::default`]).
///
/// # Errors
///
/// The display list is not drawable: a sheet with no section has no height, and is refused by name by the
/// writer rather than drawn as an empty page.
pub fn write(layout: &Layout) -> Result<String, DrawError> {
    write_styled(layout, &PictureStyle::default())
}

/// [`write()`], with the caller's ink, background and object.
///
/// # Errors
///
/// As [`write()`].
pub fn write_styled(layout: &Layout, style: &PictureStyle) -> Result<String, DrawError> {
    crate::draw::svg::write(&of_layout(layout, style), None)
}
