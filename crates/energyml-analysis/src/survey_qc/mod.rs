//! **What a survey is checked against** - the pure part of the survey quality control: whether a station's azimuth
//! means anything, which azimuth the vertical section takes, how far a survey lies from another path, what the file
//! states against what the method computes, and which trajectory goes with which survey.
//!
//! This module sits above geometry and reads nothing from a package: it takes the stations read by
//! `energyml_geometry::stations` and samples of paths, and answers in numbers and typed outcomes, never in prose.
//! The links a face finds in a package (`WitsmlTrajectory`, `DeviationSurvey`, `Activity`, the MD datum) feed
//! [`pairings`] as plain values: the rule is here, the reading of the links is the caller's.
//!
//! Where a choice is open (which criterion makes an azimuth significant, which source gives the vertical section's
//! azimuth) the answer is a registry - [`Criteria`], [`SectionAzimuths`] - whose caller-registered entries are tried
//! first, and the answer says who answered.

pub mod compare;
pub mod pairing;
pub mod section;
pub mod significance;
pub mod stated;

pub use compare::{
    CompareAt, CompareError, FitError, Frames, MOST_SAMPLES, PathSample, PositionGap, RotationFit,
    SurveyComparison, compare, rotation_fit, sample_mds,
};
pub use pairing::{PairedBy, Pairings, SurveyObjectKind, TrajectoryRefs, pairings};
pub use section::{
    ChosenSection, ClosureAzimuth, DrainMeanAzimuth, SectionAzimuth, SectionAzimuths, SectionInput,
    StatedAzimuth, UserAzimuth, section_azimuth,
};
pub use significance::{
    Criteria, InclinationAtLeast, LateralEffectAtLeast, Significance, SignificanceCriterion, StationContext,
    azimuth_significance,
};
pub use stated::{
    NotCompared, NotComparedWhy, StatedError, StatedField, StatedGap, StatedReport, stated_against_computed,
};

#[cfg(test)]
mod tests;
