//! From an EnergyML representation to points a program can draw.
//!
//! The storage layer answers "what is in this package" and "give me that
//! array". This one answers the question an application actually has: **where
//! is this surface, and what shape is it?**
//!
//! # The thing nobody expects
//!
//! An EnergyML array is not a file offset. `AbstractPoint3dArray` has half a
//! dozen concrete forms and only one of them is "read it from HDF5":
//!
//! | Form | Where the numbers are |
//! |---|---|
//! | `Point3dExternalArray` / `Point3dHdf5Array` | in the external file |
//! | `Point3dLatticeArray` | **nowhere** — an origin, direction vectors and spacings, from which the points are *computed* |
//! | `Point3dZValueArray` | x and y from a supporting geometry (usually a lattice), z from an array |
//! | `Point3dParametricArray` | on parametric lines, evaluated from control points |
//! | `Point3dFromRepresentationLatticeArray` | in *another* representation |
//!
//! On the reference corpus that is 71 external arrays against 19 that are not,
//! so a reader that only knows HDF5 silently loses every 2D grid and every
//! wellbore trajectory — and loses them as *empty geometry*, which looks like
//! a working program showing nothing. The same is true of plain numeric arrays:
//! `DoubleConstantArray` and `DoubleLatticeArray` carry no data either.
//!
//! So [`values`] and [`points`] resolve **every** form, and the ones that are
//! not implemented yet fail by name instead of returning nothing.
//!
//! # Two places a reader quietly gets it wrong
//!
//! Both were found by running this crate over the reference corpus, and both
//! produce a plausible-looking result rather than an error:
//!
//! - **Patch node numbering.** A triangle's indices may be numbered inside its
//!   own patch or across the whole representation — FESAPI writes one, resqpy
//!   the other. [`geometry`] decides from the numbers and refuses what fits
//!   neither, and [`Patch::node_offset`] records the patch's place.
//! - **Lattice axis order.** Dimensions are listed slowest first, so the *last*
//!   one varies fastest. Reversing it keeps the point count exactly right and
//!   transposes the surface.
//!
//! # Where it sits
//!
//! Above the storage layer, never inside it (ADR-0002). Everything here takes objects already in memory plus a [`ValueSource`]
//! for the arrays, so an ETP dataspace works exactly as a file does.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod clip;
pub mod connection;
pub mod defined;
pub mod derived;
pub mod error;
pub mod fence;
pub mod follow;
pub mod geometry;
pub mod heads;
pub mod ijk;
pub mod parametric;
pub mod parent;
pub mod plane;
pub mod points;
pub mod readers;
pub mod scene;
pub mod section;
pub mod shape;
pub mod source;
pub mod min_curvature;
pub mod stations;
pub mod transfer;
pub mod tube;
pub mod unstructured;
pub mod values;
pub mod well;
pub mod wells;

pub use energyml_domain::{Crs, CrsKind, VerticalDirection, VerticalQuantity};
/// Units and coordinate reference systems, shared with every other consumer.
///
/// Re-exported so a caller working on geometry does not have to name a second
/// crate for the CRS of the surface it just read. The definitions live in
/// [`energyml_domain`] because a WITSML well needs them just as much, and a
/// crate called *geometry* is the wrong place to keep them.
pub use energyml_domain::{crs, reproject, units};
pub use error::{Error, Result};
pub use geometry::{Geometry, Patch, Polyhedra, Topology};
pub use points::Point;
pub use readers::{Readers, RepresentationReader};
pub use scene::{Attribute, AttributeScope, Placement, Related, SceneObject};
pub use source::ValueSource;
