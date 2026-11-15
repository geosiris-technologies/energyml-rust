//! The values between two nodes: a registry of declared methods, refused
//! where they have no sense, never bridging a missing value or a wide gap.

use std::f64::consts::{PI, TAU};

use super::gap::GapRule;

/// The id of the method that takes the closer node (the shallower on a tie).
pub const NEAREST: &str = "nearest";
/// The id of the method that holds the value of the node above.
pub const PREVIOUS: &str = "previous";
/// The id of the linear method.
pub const LINEAR: &str = "linear";
/// The id of the method that is linear in the logarithm of the value.
pub const LOG_LINEAR: &str = "log_linear";
/// The id of the method that goes the short way round an angle.
pub const ANGULAR: &str = "angular";

/// What the values are, filled by the caller from the property's facts (lookup
/// table, family, unit).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Nature {
    /// The values are categories (a lookup table).
    pub categorical: bool,
    /// The values are booleans.
    pub boolean: bool,
    /// The values are integers.
    pub integer: bool,
    /// The values are angles, in radians.
    pub angle: bool,
    /// The values hold over an interval, not at a node.
    pub intervals: bool,
}

/// Why a method does not apply, or refuses a cell.
#[derive(Clone, Copy, Debug, PartialEq, thiserror::Error)]
pub enum InterpRefusal {
    /// Categories cannot be averaged.
    #[error("the values are categories: only nearest and previous apply")]
    Categorical,
    /// Booleans cannot be averaged.
    #[error("the values are booleans: only nearest and previous apply")]
    Boolean,
    /// The angular method needs angles.
    #[error("the values are not angles")]
    NotAnAngle,
    /// A value that holds over an interval is held, not interpolated.
    #[error("the values hold over intervals: only previous applies")]
    IntervalsHoldOnly,
    /// A logarithm needs a positive value.
    #[error("{value} has no logarithm: log-linear needs positive values")]
    NonPositive {
        /// The value that has no logarithm.
        value: f64,
    },
    /// Two opposite angles have no shorter way round.
    #[error("{a} and {b} rad are opposite: the shorter way round is not defined")]
    OppositeAngles {
        /// The angle at the shallower node, in radians.
        a: f64,
        /// The angle at the deeper node, in radians.
        b: f64,
    },
    /// The nodes are further apart than the gap rule allows (S7).
    #[error("the nodes are {gap} apart, wider than the {max} that may be bridged")]
    GapTooWide {
        /// The distance between the nodes.
        gap: f64,
        /// The widest interval that may be bridged.
        max: f64,
    },
    /// The gap is negative or not a number.
    #[error("a gap of {gap} is not a finite non-negative length")]
    BadGap {
        /// The gap given.
        gap: f64,
    },
    /// The fraction is not in `[0, 1]`.
    #[error("t = {t} is outside [0, 1]")]
    TOutOfRange {
        /// The fraction given.
        t: f64,
    },
    /// The two nodes do not hold the same number of values.
    #[error("{lower} value(s) at the shallower node against {upper} at the deeper one")]
    RowsDiffer {
        /// Values at the shallower node.
        lower: usize,
        /// Values at the deeper node.
        upper: usize,
    },
}

/// A note that goes with a result the method does give.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterpNote {
    /// Integers were interpolated: the result may be fractional.
    FractionalInteger,
    /// Angles were interpolated as plain numbers: the wrap at 2 pi is ignored.
    AngleTakenAsLinear,
}

/// Whether a method applies to values of a nature.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Applies {
    /// It applies.
    Yes,
    /// It applies, with a note the caller should show.
    WithNote(InterpNote),
    /// It does not, for this reason.
    No(InterpRefusal),
}

/// A way to find the value between two nodes. Implement it and
/// [`Interpolations::push`] it to add a method: the additions of the caller
/// are tried first.
pub trait Interpolator: Send + Sync {
    /// The stable id of the method (it is written in recipes).
    fn id(&self) -> &'static str;
    /// Whether the method applies to values of this nature.
    fn applies(&self, nature: &Nature) -> Applies;
    /// The value between `a` at the shallower node and `b` at the deeper one;
    /// both finite, `0 < t < 1`.
    fn between(&self, a: f64, b: f64, t: f64) -> Result<f64, InterpRefusal>;
}

struct Nearest;
struct Previous;
struct Linear;
struct LogLinear;
struct Angular;

impl Interpolator for Nearest {
    fn id(&self) -> &'static str {
        NEAREST
    }
    fn applies(&self, nature: &Nature) -> Applies {
        if nature.intervals {
            Applies::No(InterpRefusal::IntervalsHoldOnly)
        } else {
            Applies::Yes
        }
    }
    fn between(&self, a: f64, b: f64, t: f64) -> Result<f64, InterpRefusal> {
        Ok(if t > 0.5 { b } else { a })
    }
}

impl Interpolator for Previous {
    fn id(&self) -> &'static str {
        PREVIOUS
    }
    fn applies(&self, _nature: &Nature) -> Applies {
        Applies::Yes
    }
    fn between(&self, a: f64, _b: f64, _t: f64) -> Result<f64, InterpRefusal> {
        Ok(a)
    }
}

/// The refusals and notes `linear` and `log_linear` share.
fn applies_to_a_measure(nature: &Nature) -> Applies {
    if nature.categorical {
        Applies::No(InterpRefusal::Categorical)
    } else if nature.boolean {
        Applies::No(InterpRefusal::Boolean)
    } else if nature.intervals {
        Applies::No(InterpRefusal::IntervalsHoldOnly)
    } else if nature.angle {
        Applies::WithNote(InterpNote::AngleTakenAsLinear)
    } else if nature.integer {
        Applies::WithNote(InterpNote::FractionalInteger)
    } else {
        Applies::Yes
    }
}

impl Interpolator for Linear {
    fn id(&self) -> &'static str {
        LINEAR
    }
    fn applies(&self, nature: &Nature) -> Applies {
        applies_to_a_measure(nature)
    }
    fn between(&self, a: f64, b: f64, t: f64) -> Result<f64, InterpRefusal> {
        Ok(a + t * (b - a))
    }
}

impl Interpolator for LogLinear {
    fn id(&self) -> &'static str {
        LOG_LINEAR
    }
    fn applies(&self, nature: &Nature) -> Applies {
        applies_to_a_measure(nature)
    }
    fn between(&self, a: f64, b: f64, t: f64) -> Result<f64, InterpRefusal> {
        if a <= 0.0 {
            return Err(InterpRefusal::NonPositive { value: a });
        }
        if b <= 0.0 {
            return Err(InterpRefusal::NonPositive { value: b });
        }
        let (la, lb) = (a.ln(), b.ln());
        Ok((la + t * (lb - la)).exp())
    }
}

impl Interpolator for Angular {
    fn id(&self) -> &'static str {
        ANGULAR
    }
    fn applies(&self, nature: &Nature) -> Applies {
        if !nature.angle {
            Applies::No(InterpRefusal::NotAnAngle)
        } else if nature.categorical {
            Applies::No(InterpRefusal::Categorical)
        } else if nature.intervals {
            Applies::No(InterpRefusal::IntervalsHoldOnly)
        } else {
            Applies::Yes
        }
    }
    /// In radians: the shorter way round, the result in `[0, 2 pi)`.
    fn between(&self, a: f64, b: f64, t: f64) -> Result<f64, InterpRefusal> {
        let mut d = (b - a).rem_euclid(TAU);
        if d > PI {
            d -= TAU;
        }
        if (d.abs() - PI).abs() <= 1e-12 {
            return Err(InterpRefusal::OppositeAngles { a, b });
        }
        Ok((a + t * d).rem_euclid(TAU))
    }
}

/// The registry of methods. The additions of the caller are tried before the
/// built-in ones.
pub struct Interpolations {
    entries: Vec<Box<dyn Interpolator>>,
}

impl Default for Interpolations {
    fn default() -> Self {
        Self::builtin()
    }
}

impl Interpolations {
    /// A registry with no method.
    pub fn none() -> Self {
        Interpolations { entries: Vec::new() }
    }

    /// The five built-in methods: nearest, previous, linear, log_linear, angular.
    pub fn builtin() -> Self {
        Interpolations {
            entries: vec![
                Box::new(Nearest),
                Box::new(Previous),
                Box::new(Linear),
                Box::new(LogLinear),
                Box::new(Angular),
            ],
        }
    }

    /// Adds a method, tried before the others: a method of the same id
    /// replaces the built-in one.
    pub fn push(&mut self, method: Box<dyn Interpolator>) -> &mut Self {
        self.entries.insert(0, method);
        self
    }

    /// The method of this id (the caller's addition first).
    pub fn get(&self, id: &str) -> Option<&dyn Interpolator> {
        self.entries.iter().find(|m| m.id() == id).map(|m| m.as_ref())
    }

    /// The ids, once each, in the order of the registry.
    pub fn ids(&self) -> Vec<&'static str> {
        let mut ids: Vec<&'static str> = Vec::new();
        for m in &self.entries {
            if !ids.contains(&m.id()) {
                ids.push(m.id());
            }
        }
        ids
    }

    /// Each method (once per id) with whether it applies to this nature.
    pub fn offered(&self, nature: &Nature) -> Vec<(&'static str, Applies)> {
        let mut out: Vec<(&'static str, Applies)> = Vec::new();
        for m in &self.entries {
            if !out.iter().any(|(id, _)| *id == m.id()) {
                out.push((m.id(), m.applies(nature)));
            }
        }
        out
    }
}

/// Why [`default_interpolation`] chose its method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterpChoice {
    /// The values hold over intervals.
    Intervals,
    /// The values are categories or booleans.
    Category,
    /// The values are angles.
    Angle,
    /// The scale is logarithmic.
    LogScale,
    /// A plain measure.
    Measure,
}

/// The method to start from, and why. In this order: intervals, categories
/// and booleans, angles, a logarithmic scale, else linear.
pub fn default_interpolation(nature: &Nature, log_scaled: bool) -> (&'static str, InterpChoice) {
    if nature.intervals {
        (PREVIOUS, InterpChoice::Intervals)
    } else if nature.categorical || nature.boolean {
        (NEAREST, InterpChoice::Category)
    } else if nature.angle {
        (ANGULAR, InterpChoice::Angle)
    } else if log_scaled {
        (LOG_LINEAR, InterpChoice::LogScale)
    } else {
        (LINEAR, InterpChoice::Measure)
    }
}

/// The values of a row between two nodes.
#[derive(Clone, Debug, PartialEq)]
pub struct Interpolated {
    /// One value per cell; `NaN` where absent or refused.
    pub values: Vec<f64>,
    /// How many cells are absent because a neighbour was not a number: never bridged.
    pub missing: usize,
    /// How many cells the method refused (log of a value that is not positive, opposite angles).
    pub refused: usize,
    /// The first refusal, to say why.
    pub first_refusal: Option<InterpRefusal>,
    /// The note of the method, when it gave one.
    pub note: Option<InterpNote>,
}

/// The values at fraction `t` between the rows of two nodes `gap` apart. A
/// method that does not apply to `nature` refuses the whole row; a gap wider
/// than `rule` allows gives an empty row (every cell `NaN`) that says why;
/// a missing neighbour gives a missing cell, never the other neighbour. On a
/// node (`t` of 0 or 1) the value is the node's, whatever the gap. A matrix per
/// node is passed flattened: the same method, cell by cell.
pub fn interpolate_row(
    lower: &[f64],
    upper: &[f64],
    t: f64,
    gap: f64,
    rule: &GapRule,
    method: &dyn Interpolator,
    nature: &Nature,
) -> Result<Interpolated, InterpRefusal> {
    if lower.len() != upper.len() {
        return Err(InterpRefusal::RowsDiffer {
            lower: lower.len(),
            upper: upper.len(),
        });
    }
    if !t.is_finite() || !(0.0..=1.0).contains(&t) {
        return Err(InterpRefusal::TOutOfRange { t });
    }
    if !gap.is_finite() || gap < 0.0 {
        return Err(InterpRefusal::BadGap { gap });
    }
    let mut note = None;
    match method.applies(nature) {
        Applies::No(reason) => return Err(reason),
        Applies::WithNote(n) => note = Some(n),
        Applies::Yes => {}
    }
    let len = lower.len();
    if t > 0.0 && t < 1.0 && !rule.bridges(gap) {
        return Ok(Interpolated {
            values: vec![f64::NAN; len],
            missing: len,
            refused: 0,
            first_refusal: Some(InterpRefusal::GapTooWide {
                gap,
                max: rule.max_gap(),
            }),
            note,
        });
    }
    let mut out = Interpolated {
        values: Vec::with_capacity(len),
        missing: 0,
        refused: 0,
        first_refusal: None,
        note,
    };
    for (&a, &b) in lower.iter().zip(upper) {
        let value = if t == 0.0 {
            a
        } else if t == 1.0 {
            b
        } else if !a.is_finite() || !b.is_finite() {
            // A missing neighbour cuts the curve, it does not bridge it.
            f64::NAN
        } else {
            match method.between(a, b, t) {
                Ok(v) => v,
                Err(r) => {
                    out.refused += 1;
                    out.first_refusal.get_or_insert(r);
                    out.values.push(f64::NAN);
                    continue;
                }
            }
        };
        // A refused cell is counted in `refused` (above); a value that is
        // absent, on a node or between, is counted here.
        if value.is_nan() {
            out.missing += 1;
        }
        out.values.push(value);
    }
    Ok(out)
}
