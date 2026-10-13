//! What is *computed from* EnergyML shapes — as opposed to what the file
//! describes, which is `energyml-geometry`'s whole job and stays there.
//!
//! Geometry answers "what shape does this document state", analysis answers
//! "what can be derived from it" — wells and grids, structural measures, laws
//! and tests, water chemistry and logs. The domain-neutral algorithms
//! (interpolation, meshing, iso-contours, …) live in `energyml-geostat` and are
//! re-exported here, so `energyml_analysis::kriging::…` and the other paths
//! keep resolving. Nothing here reads a package: the inputs
//! are plain samples and query points, so the crate compiles to wasm with
//! no feature flags and a caller on any storage (EPC, ETP, none at all)
//! uses it the same way. The result of an interpolation is meant to become
//! a legitimate EnergyML object — a `ContinuousProperty` on a `Grid2d` —
//! written into the model, not beside it; that promotion belongs to the
//! caller, which is why the functions here never see a store.
//!
//! House rules, applied:
//! - **A `NaN` sample is set aside, never averaged** — it is how the corpus
//!   says *no data here*, and the count of what was set aside is part of
//!   the answer, not a silent repair.
//! - **Refusals are named.** No samples, mismatched lengths, a power that
//!   is not a positive number — each says what is wrong, because an
//!   interpolated `NaN` and "the call was malformed" are different news.
//!
//! # References
//!
//! The citations in the module docs (Hall 1963, Piper 1944, Stiff 1951, …) are
//! pending verification; algorithms without a published source are marked
//! `TODO(reference): verify`. See `README.md`.

// Domain-neutral modules, moved to `energyml-geostat` and re-exported.
pub use energyml_geostat::{
    barrier, cells, densify, extend, idw, isocontour, kriging, loft, mba, mba_frame, measure, nodes, outline, rbf, resample, triangulate,
    variogram,
};

pub mod fake;
pub mod fault_sticks;
pub mod fault_throw;
pub mod flow_law;
pub mod groups;
pub mod hall;
pub mod heights;
pub mod ijk;
pub mod ijk_faults;
pub mod ion_balance;
pub mod horizon_law;
pub mod profile;
pub mod wells_profile;
pub mod project;
pub mod intersect;
pub mod step_rate;
pub mod surface;
pub mod survey;
pub mod t2d;
pub mod thickness;
pub mod time_law;
pub mod tops;
pub mod water_diagrams;
pub mod well_plan;
pub mod well_logs;

pub use energyml_geostat::Error;
pub use energyml_geostat::{
    Along, At, Barriers, Contour, Densified, Empirical, Idw, IdwAnswer, Kernel, KrigingAnswer, Model, ModelKind, Ordinary, Placement,
    Polyline, Rbf, RbfAnswer, Segment, Triangulated, chain, contour, contours, empirical, levels, levels_spaced, plan_distance,
    triangulate_xy, triangulate_xy_constrained,
};
pub use heights::{Height, Heights};
pub use intersect::{Intersection, Trace, intersect};
pub use surface::{Curvature, Slope, Up, curvature, slope};
pub use t2d::LinearVelocity;
pub use thickness::{Measure, Thickness, thickness};
