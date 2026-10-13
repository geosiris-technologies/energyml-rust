//! Domain-neutral interpolation and meshing algorithms.
//!
//! Nothing in this crate knows EnergyML: inputs are plain samples, query
//! points, coordinate triples and node indices, so it compiles to wasm with no
//! feature flags and can be used in hydrogeology, environmental work or civil
//! engineering as well as on EnergyML models. `energyml-analysis` re-exports
//! every module, so existing `energyml_analysis::…` paths keep resolving.
//!
//! Contents: interpolation (`idw`, `kriging`, `variogram`, `rbf`, `mba`,
//! `mba_frame`), meshing (`triangulate`, `isocontour`, `resample`, `densify`,
//! `loft`, `outline`, `cells`, `nodes`, `extend`) and tools (`measure`,
//! `barrier`, the private `solve`).
//!
//! House rules, applied:
//! - **A `NaN` sample is set aside, never averaged** — it is how the data
//!   says *no data here*, and the count of what was set aside is part of the
//!   answer, not a silent repair.
//! - **Refusals are named.** No samples, mismatched lengths, a power that
//!   is not a positive number — each says what is wrong, because an
//!   interpolated `NaN` and "the call was malformed" are different news.
//!
//! # References
//!
//! Each algorithm module cites, in its own docs, the publication it follows.
//! Those citations are pending verification (see `README.md`); modules with
//! no published source carry a `TODO(reference): verify` line.

pub mod barrier;
pub mod cells;
pub mod densify;
pub mod extend;
pub mod idw;
pub mod isocontour;
pub mod kriging;
pub mod loft;
pub mod mba;
pub mod mba_frame;
pub mod measure;
pub mod nodes;
pub mod outline;
pub mod rbf;
pub mod resample;
mod solve;
pub mod triangulate;
pub mod variogram;

pub use barrier::Barriers;
pub use densify::{Along, Densified, Placement, densify};
pub use idw::{Idw, IdwAnswer};
pub use isocontour::{At, Contour, Polyline, Segment, chain, contour, contours, levels, levels_spaced};
pub use kriging::{KrigingAnswer, Ordinary};
pub use rbf::{Kernel, Rbf, RbfAnswer};
pub use resample::{plan_distance, resample};
pub use triangulate::{Triangulated, triangulate_xy, triangulate_xy_constrained};
pub use variogram::{Empirical, Model, ModelKind, empirical};

/// What this crate refuses, by name.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum Error {
    #[error("{count} sample position(s) for {values} value(s) — one value per sample")]
    MismatchedLengths { count: usize, values: usize },
    #[error("no usable sample: {given} given, {set_aside} set aside for NaN")]
    NoSample { given: usize, set_aside: usize },
    #[error("the power must be a finite number > 0, not {given}")]
    BadPower { given: f64 },
    #[error("the radius must be a finite number > 0, not {given}")]
    BadRadius { given: f64 },
    #[error("neighbours = 0 would interpolate from nothing")]
    NoNeighbours,
    #[error("0 distance classes — the variogram needs at least one")]
    NoClasses,
    #[error("no triangle: nothing has a boundary")]
    NoTriangle,
    #[error("the boundary is not a set of simple loops: node {node} has more than two boundary edges")]
    NotSimple { node: u32 },
    #[error("a hull needs at least 3 points with a plan-view position; {given} given")]
    TooFewPoints { given: usize },
    #[error("{usable} usable sample(s) where {needed} are needed")]
    TooFewSamples { usable: usize, needed: usize },
    #[error("no sample pair within {reach} of each other — no variogram to measure")]
    TooFewPairs { reach: f64 },
    #[error("{held} variogram class(es) hold pairs where fitting needs {needed}")]
    TooFewClasses { held: usize, needed: usize },
    #[error("not a variogram model: {what}")]
    BadModel { what: String },
    #[error(
        "samples {first} and {second} share one position — the kriging system is singular; deduplicate first (which value wins is the caller's call)"
    )]
    DuplicateSamplePositions { first: usize, second: usize },
    #[error("the kriging system is singular — the samples do not determine the weights")]
    SingularSystem,
    #[error("not a fault trace: {what}")]
    BadBarrier { what: String },
    #[error("a fault trace is plan-view positions `[x, y, …]` — {given} value(s) do not make pairs")]
    NotPairs { given: usize },
    #[error(
        "the samples do not span the space (all on one line, in 2D) — the kernel's polynomial term cannot be determined; use a constant-term kernel or add an off-line sample"
    )]
    DegenerateGeometry,

    // -- Iso-contours -----------------------------------------------------
    #[error("0 levels — there is nothing to contour at")]
    NoLevels,
    #[error("the level spacing must be a finite number > 0, not {given}")]
    BadSpacing { given: f64 },
    #[error("the range {min}..{max} is not two finite numbers")]
    BadRange { min: f64, max: f64 },
    #[error("the level must be a finite number, not {given}")]
    BadLevel { given: f64 },
    #[error("that spacing asks for {wanted} levels where {most} is the most a picture can hold")]
    TooManyLevels { wanted: usize, most: usize },
    #[error("{given} coordinate(s) — points are `[x, y, z, …]`, so a multiple of three")]
    NotTriples { given: usize },
    #[error("{given} index(es) — triangles are three node indices each")]
    NotTriangles { given: usize },
    #[error("a triangle names node {at} where the surface has {nodes}")]
    NodeOutOfRange { at: usize, nodes: usize },

    // -- Thickness --------------------------------------------------------
    #[error("{given} index(es) — quadrilaterals are four node indices each")]
    NotQuads { given: usize },
    #[error("{nodes} node(s) for a lattice of {fastest} by {slowest}: a lattice holds one node per cell corner, the fast axis first")]
    NotALattice { nodes: usize, fastest: usize, slowest: usize },
    #[error("the {which} has no face — its heights are read {how}, and points or lines have none")]
    NoFace { which: &'static str, how: &'static str },
    #[error(
        "no face of the {which} can be read: all {set_aside} are set aside — a corner that is not a finite point, no area in plan, or a cell that is not convex"
    )]
    NoUsableFace { which: &'static str, set_aside: usize },

    // -- Measures and groups -------------------------------------
    #[error("{what} {element} names node {at} where the shape has {nodes}")]
    ElementOutOfRange { what: &'static str, element: usize, at: usize, nodes: usize },
    #[error("the polylines add up to {total} node(s) where the shape has {nodes} — every line after a wrong count would be shifted")]
    RunsPastEnd { total: usize, nodes: usize },
    #[error("{keys} group key(s) for {values} value(s) — one key per value, on the same elements")]
    KeysAndValues { keys: usize, values: usize },

    // -- Fake data -----------------------------------------------------
    #[error("cuts must be at least 1, not {given}")]
    BadCuts { given: f64 },
    #[error("a range of cuts must be at least 1..1, and increasing — not {min}..{max}")]
    BadCutsRange { min: f64, max: f64 },
    #[error("the length must be a finite number >= 0, not {given}")]
    BadLength { given: f64 },
    #[error("a length range must be two finite numbers >= 0, increasing — not {min}..{max}")]
    BadLengthRange { min: f64, max: f64 },
    #[error("the spacing fluctuation must be a finite number in [0, 1], not {given}")]
    BadFluctuation { given: f64 },
    #[error("no point to start a polyline from — the cloud is empty")]
    NoHeads,
}
