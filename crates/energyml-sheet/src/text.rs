//! Measuring text against the font the sheet is written in, and wrapping it.
//!
//! Moved to [`crate::draw::text`], which every drawing shares; re-exported here
//! so that `energyml_sheet::text::*` keeps working.

pub use crate::draw::text::{Style, Wrapped, average_char_width_pt, char_width_pt, string_width_pt, wrap};
