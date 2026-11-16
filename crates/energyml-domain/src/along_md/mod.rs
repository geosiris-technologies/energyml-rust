//! Along the measured depth: where an MD falls between the nodes of a support,
//! how to step from one MD to the next, and what a value is between two nodes.
//!
//! This lives in the domain because the domain already carries the MDs
//! (`strata_by_md`) and the series, has no native dependency and builds for
//! `wasm32-unknown-unknown`, so a web face has the same arithmetic as the
//! library. Decisions S7 (gaps and seams) and S8 are recorded in ADR-0027.
//!
//! Two MDs are compared only after [`crate::strata_by_md::comparable`] has
//! brought them to one unit and one datum. This module receives MDs **already
//! brought** there: it never compares references itself.
//!
//! | Item | What it answers |
//! |---|---|
//! | [`median_step`], [`GapRule`] | how far apart two nodes may be before the space between is a hole |
//! | [`bracket`], [`nearest_node`], [`union_axis`] | where an MD falls, which node is closest, the common axis of several supports |
//! | [`MdStepper`] | the MD `n` notches deeper, on the nodes or on a fine grid |
//! | [`Interpolations`], [`interpolate_row`] | the values between two nodes, by a declared method, refused where it has no sense |

mod bracket;
mod gap;
mod interpolate;
mod stepper;

pub use bracket::{Bracket, BracketRefusal, bracket, nearest_node, union_axis};
pub use gap::{GapRule, median_step};
pub use interpolate::{
    ANGULAR, Applies, InterpChoice, InterpNote, InterpRefusal, Interpolated, Interpolations, Interpolator,
    LINEAR, LOG_LINEAR, NEAREST, Nature, PREVIOUS, default_interpolation, interpolate_row,
};
pub use stepper::{MdStepper, StepMode, StepRefusal, Stepped};

/// S7: a gap wider than `GAP_RATIO` times the median step is never bridged.
pub const GAP_RATIO: f64 = 3.0;

/// S7: a jump of the value wider than `JUMP_RATIO` times the median step of
/// its variation is a seam, and is said.
pub const JUMP_RATIO: f64 = 10.0;

/// An MD within this fraction of a step of a node is that node (a tolerance of
/// `SNAP_FRACTION` times the step, never an absolute length).
pub const SNAP_FRACTION: f64 = 1e-6;

/// The most fine steps a stepper accepts over one axis.
pub const MAX_FINE_STEPS: u64 = 1_000_000;
