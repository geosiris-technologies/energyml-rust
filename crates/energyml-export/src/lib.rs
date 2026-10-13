//! Writing EnergyML geometry out to formats other programs read.
//!
//! # A registry, because the list is never finished
//!
//! OBJ, GeoJSON and VTK — in three flavours — are here. glTF, PLY, shapefiles
//! and whatever a particular site needs are not, and hard-coding a `match` over
//! format names would mean every one of them is a change to this crate. So an exporter is a
//! [`Exporter`] implementation in a [`Exporters`] registry, exactly as an array
//! backend and a representation reader are:
//!
//! ```no_run
//! # use energyml_export::{Exporter, Exporters};
//! # fn example(mine: Box<dyn Exporter>) {
//! let mut exporters = Exporters::builtin();
//! exporters.push(mine);              // found before the built-ins
//! # }
//! ```
//!
//! An application adds a format without touching this crate, and a format added
//! here reaches every caller — including the `energyml export` command, which
//! lists whatever the registry holds rather than a list of its own.
//!
//! # Metadata is configuration, not code
//!
//! What each format carries beyond the geometry is an
//! [`energyml_domain::metadata::Spec`] — a JSON file of key → dotted path,
//! where a path may cross references into other objects:
//!
//! ```json
//! {
//!   "uuid":    "uuid",
//!   "title":   "Citation.Title",
//!   "type":    "contentType",
//!   "feature": "RepresentedInterpretation.InterpretedFeature.Citation.Title"
//! }
//! ```
//!
//! One spec, every format: the same keys come out as GeoJSON `properties`, VTK
//! `FieldData` arrays and OBJ comments. Adding a key is editing a file, not
//! editing this crate — which is the same argument as the registry, one level
//! down.
//!
//! # What an exporter is given
//!
//! A [`SceneObject`]: the shape, the coordinate system, and the identity —
//! uuid, ETP URI, title, and the interpretation and feature above it. The last
//! part matters more than it looks. An exported surface that is only triangles
//! cannot be traced back to what it represents, and tracing back is usually the
//! reason for exporting.
//!
//! # Where the coordinates end up
//!
//! Each exporter states the [`Placement`] it wants, and gets it. OBJ asks for
//! metres, because a mesh viewer wants numbers near the origin in a single
//! unit. GeoJSON asks for metres and then needs *degrees*, which is a datum
//! transformation and not arithmetic — so it goes through
//! [`energyml_domain::reproject`], and **refuses** rather than writing
//! projected metres into a field the format says is longitude. That refusal is
//! the honest answer: a GeoJSON with eastings in the longitude column loads
//! fine and puts the model in the Gulf of Guinea.
//!
//! [`SceneObject`]: energyml_geometry::SceneObject
//! [`Placement`]: energyml_geometry::Placement

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod csv;
pub mod error;
pub mod geojson;
pub mod obj;
pub mod registry;
pub mod shapes;
pub mod vtk;

pub use error::{Error, Result};
pub use registry::{Exporter, Exporters, Options};
pub use shapes::{Selector, Shapes};
