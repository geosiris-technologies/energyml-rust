//! EnergyML concepts that are the same in every version.
//!
//! The layer between the storage of objects and what an application wants to
//! do with them. Everything here reads the *generated* types through
//! reflection, so one implementation covers RESQML 2.0.1 and 2.2, WITSML 2.0
//! and 2.1, PRODML, and the next release to the extent it spells things the way
//! its predecessors did.
//!
//! | Module | What it answers |
//! |---|---|
//! | [`units`] | how much is `1000 ft` in metres |
//! | [`crs`] | where is this object, and which way does z count |
//! | [`reproject`] | how do I get that into WGS 84 (a seam — see the module) |
//! | [`values`] | give me the numbers, whichever of the twenty array forms this is |
//! | [`stats`] | what is in those numbers — extremes, distribution, how much is `NaN` |
//! | [`metadata`] | give me that value, even if it lives three objects away |
//! | [`property`] | what do these numbers hang on, and how many should there be |
//! | [`property_kind`] | what kind of quantity is it, and what is it a kind *of* |
//! | [`color`] | what colour is this drawn in, and who said so |
//! | [`table`] | what does the integer 3 mean in this property |
//! | [`well_strata`] | which unit is this well in, from which MD to which |
//! | [`strata_by_md`] | and on its other frames and trajectories, looked up by MD |
//! | [`series`] | what does this log or time series say, sample by sample |
//!
//! # The rule the whole crate follows
//!
//! **Read by shape, never by version.** A field is found by its name and its
//! binding, not by matching a concrete generated type. Two hard-won reasons:
//!
//! 1. The same concept is spelled differently across versions — RESQML 2.0.1's
//!    `XOffset` is eml 2.3's `OriginProjectedCoordinate1`, two objects away —
//!    and a reader that knows only one spelling returns a **plausible empty
//!    answer** rather than an error.
//! 2. The generated Rust identifiers are not stable even when the wire format
//!    is. `LengthUom::Mm` is the megametre in eml 2.0 and the millimetre in
//!    eml 2.3; the string `"Mm"` means the same thing in both. Matching the
//!    variant is a factor of 10⁹ waiting to happen. See [`units`].
//!
//! # And the rule that follows from it
//!
//! **What cannot be read is named, never defaulted.** A unit outside the
//! table, a system with no EPSG code, an azimuth from an unsupported
//! reference: each is an error that says what was missing. The alternative —
//! returning the value untouched — is indistinguishable from success, and puts
//! models in the wrong place with every coordinate still looking reasonable.
//!
//! # Layering
//!
//! Above `energyml-storage`, below `energyml-geometry`. It has no native
//! dependencies and builds for `wasm32-unknown-unknown`, which is checked in
//! CI rather than assumed.

#![doc(html_no_source)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod attachments;
pub mod check;
pub mod color;
/// The colour cascade under the inheritance order: the walk of `inherit`, asked
/// *"does this object carry a colour?"*, and the path that answered.
pub mod color_inherit;
/// The colour of an object per target — walked once, resumed on demand, kept
/// between questions and invalidated by the objects a change touched.
pub mod color_walks;
pub mod crs;
/// The narrow XML reader and the `PropertyKindDictionary` scan built on it.
///
/// Private, and both files are included verbatim by `build.rs` so the
/// build-time compaction and the run-time loader can never drift apart. They
/// share one module here for the same reason: in the build script they are one
/// scope, so they must be one scope here too.
#[allow(dead_code)]
mod scan {
    include!("raw_xml.rs");
    include!("dictionary_xml.rs");
}
pub mod error;
pub mod geolts;
pub mod geolts_rank;
/// What an object inherits, by which path — the order of ADR-0009, over the
/// package index.
pub mod inherit;
pub mod integrity;
pub mod metadata;
/// Recording that an operation happened: the canonical activity templates, and
/// the activity built from one. See the module for why it is here rather than
/// in the JavaScript face.
pub mod provenance;
/// What the module keeps of a package: links and property sheets, never the
/// objects (ADR-0010).
pub mod package_index;
pub mod property;
pub mod property_kind;
pub mod reproject;
pub mod rules;
pub mod select;
pub mod series;
pub mod source;
pub mod stats;
pub mod stratigraphy;
pub mod table;
pub mod wellbore_table;
/// Which stratigraphic unit each interval of a wellbore frame belongs to, and
/// the colour the file gives it — what a well is coloured by, section by
/// section.
pub mod well_strata;
/// A wellbore's stratigraphy carried by measured depth onto its other
/// representations — the frames that state no unit, and the trajectories.
pub mod strata_by_md;
pub mod units;
pub mod values;
pub mod water_analysis;

pub use crs::{Crs, CrsKind, VerticalDirection, VerticalQuantity};
pub use error::{Error, Result};
pub use metadata::{Metadata, Spec};
pub use source::{ArraySource, ObjectSource};
pub use values::{Family, Values};
