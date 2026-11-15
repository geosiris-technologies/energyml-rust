//! Around the well: the physical frame of the hole at a measured depth, polar layers laid in the plane
//! across it, the grid of a per-node matrix, envelopes and ribbons, and the projections that lay a well flat.
//!
//! Everything here works in the **placed** frame of a [`PlacedPath`](crate::well::PlacedPath): metres, z up.
//! Nothing is calculated in the local coordinates of a CRS: a CRS written z down, or with its axes in the order
//! northing then easting, gives the same physical frame (see [`hole_frame`]).
//!
//! What the file does not state (the sense and origin of angles, the axes of a transverse grid, the handedness
//! of a frame) is a **parameter** the caller declares; it is refused by name until then, and never written into
//! a package from here.
//!
//! | module | what it gives |
//! |---|---|
//! | [`hole_frame`] | [`HoleFrame`](hole_frame::HoleFrame): tangent, high side, right, inclination |
//! | [`polar`] | angle conventions, rings in the plane across the hole, sector bounds |
//! | [`transverse`] | the axes of a per-node matrix ([`TransverseGrid`](transverse::TransverseGrid)) |
//! | [`envelope`] | the envelope mesh (variable section) and the ribbon (one column of a cube) |
//! | [`projection`] | the registry of 2D projections of a well, and the locator |

pub mod envelope;
pub mod hole_frame;
pub mod polar;
pub mod projection;
pub mod transverse;
