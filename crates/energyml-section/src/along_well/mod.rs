//! The geosteering section: a vertical section **along a well**, the
//! abscissa given by a projection of the hole (`md_tvd`, `developed`,
//! `vertical_section`), the ordinate the true vertical depth below the MD
//! datum (S6), with the curtain of a cube laid either side of the hole, the
//! boundaries of a depth of detection, the horizons cut by the curtain and the
//! tops of the hole.
//!
//! ADR-0038 is this module's decision record. It is not the long-section
//! of [`crate::layout`]: a section of a model along a trace has its own
//! geometry, and this one its own model and its own drawing (the shared
//! `energyml_sheet::draw`).
//!
//! # One frame, one door
//!
//! Everything is computed in **one frame**, the hole's: a point is `(md, dn)`,
//! `dn` the offset along the high side, positive up. **One function** carries
//! it into the drawing, `WellProjection::place`: the curtain's cells, the
//! boundaries, the horizons and the markers are consistent by construction (a
//! boundary at radius `r` and the centre of the row at offset `r` are the same
//! point, to the bit).
//!
//! # Three stages, three costs
//!
//! ```text
//! Request ──► build (reads, once) ──► Model ──► layout / draw (View) ──► SVG
//!                                        └────► pick / pixel_of
//! ```
//!
//! - [`build`] is the expensive stage: it reads the frame, the trajectory, the
//!   curtain's columns, the boundaries and the surfaces' maps, and places them.
//! - [`layout`] and [`draw`] only arrange the numbers of a [`Model`] under a
//!   [`View`]: nothing is read or computed again, so a change of window or of
//!   exaggeration costs a drawing, not a reading.
//! - [`pick`] and [`pixel_of`] are the way back and the way there between a
//!   pixel and the data.
//!
//! The drawing's metadata carries the request and the view: [`redraw`] makes
//! the same drawing again, byte for byte, over the same package.
//!
//! # What is refused, by name
//!
//! A curtain whose axes the file does not give (ranks are not offsets), whose
//! source is declared and not built, whose shape is not a matrix of two
//! dimensions per node; markers (no reader exists); a node with no high side is
//! counted, never a refusal of the whole section.
//!
//! No native dependency, no file, no thread: this module compiles to wasm.

pub mod boundaries;
pub mod cells;
pub mod detect;
pub mod horizons;
pub mod image;
pub mod layout;
pub mod markers;
pub mod model;
pub mod pick;
pub mod sources;

pub use boundaries::{ANGLE_TOLERANCE, Boundaries, BoundaryMode, BoundaryPoint, BoundaryRefusal, boundaries};
pub use cells::{CurtainCell, CurtainGeometry, CurtainLayout, CurtainRefusal, curtain_cells};
pub use detect::{BoundaryOffer, Offer, offers};
pub use horizons::{HorizonPoint, HorizonTrace, PlacedMesh, Undecided, computed_tops, horizon_in_curtain};
pub use image::{ImageCounts, curtain_image};
pub use layout::{LegendAsk, PlotFrame, ScaleKindAsk, View, draw, layout, plot_frame, read_metadata, redraw};
pub use markers::{Dip, DipReference, MarkerNote, MarkerRefusal, MarkerTick, WellMarker, marker_ticks};
pub use model::{
    AlongWellRefusal, AxisLinkAsk, AxisSignAsk, BoundaryAsk, CandidateNeedAsk, Curtain, CurtainAsk, FORMAT,
    METADATA_ELEMENT, Model, Note, PlaneAxisAsk, Request, SectionAsk, SurfaceTops, build,
};
pub use pick::{Readout, pick, pixel_of};
pub use sources::{
    CubeOnFrame, CurtainColumns, CurtainSource, CurtainSources, NotBuilt, place_plane, plane_of_roles,
};
