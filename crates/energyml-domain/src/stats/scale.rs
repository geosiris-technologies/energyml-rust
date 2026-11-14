//! The scale of a **value**: linear or log10, with bounds and a direction.
//!
//! A view turns a value into a fraction `0..1` with a [`Scale`], then the
//! fraction into a colour with its colour map. This is not
//! [`crate::color::Scaling`]: that one is what the *file* says about how its
//! values sit on its own colour map (`MinMax`, `UseLogarithmicMapping`,
//! `UseReverseMapping`); a `Scale` is what the *view* uses to place a value,
//! and it never reads or writes a `Scaling`.
//!
//! A logarithmic scale holds positive values only: a value at or below zero has
//! no logarithm, so it is [`Fraction::NotPositive`], never a colour and never a
//! `NaN`.

use super::quantile::{QuantileMethod, QuantileRefusal, quantiles};

/// Proposing a logarithmic scale starts at this many decades (S8).
pub const LOG_DECADES: f64 = 3.0;
/// Automatic lower bound: the first percentile (S8).
pub const AUTO_LOW: f64 = 0.01;
/// Automatic upper bound: the ninety-ninth percentile (S8).
pub const AUTO_HIGH: f64 = 0.99;

/// The shape of a scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaleKind {
    /// Equal steps of the value.
    Linear,
    /// Equal steps of `log10` of the value: decades.
    Log10,
}

/// What the caller asks: the bounds it fixes, the others left to [`resolve`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScaleSpec {
    /// Linear or logarithmic.
    pub kind: ScaleKind,
    /// The lower bound, when the caller fixes it.
    pub min: Option<f64>,
    /// The upper bound, when the caller fixes it.
    pub max: Option<f64>,
    /// Display direction: the fraction runs from 1 at `min` to 0 at `max`.
    pub reverse: bool,
}

/// A resolved scale: finite bounds, `min < max`, both positive for `Log10`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    kind: ScaleKind,
    min: f64,
    max: f64,
    reverse: bool,
}

/// Where a value falls on a scale.
///
/// `Below` and `Above` speak of the **value** against `min` and `max`, even
/// when the scale is reversed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fraction {
    /// Inside the bounds: the fraction, `0..=1`.
    At(f64),
    /// Under `min`.
    Below,
    /// Over `max`.
    Above,
    /// Not a number: a gap, drawn empty.
    Gap,
    /// At or below zero on a logarithmic scale: no logarithm.
    NotPositive,
}

/// Why a scale was refused.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ScaleRefusal {
    /// A bound is missing and there is no value to take it from.
    #[error("no bound given and no value to take one from ({kind:?} scale)")]
    NoValues {
        /// The kind asked for.
        kind: ScaleKind,
    },
    /// A bound is `NaN` or infinite.
    #[error("the bounds {min} and {max} are not both finite")]
    NotFinite {
        /// The lower bound.
        min: f64,
        /// The upper bound.
        max: f64,
    },
    /// `min > max`.
    #[error("the lower bound {min} is above the upper bound {max}")]
    Reversed {
        /// The lower bound.
        min: f64,
        /// The upper bound.
        max: f64,
    },
    /// `min == max`.
    #[error("both bounds are {at}: a scale needs a range")]
    Degenerate {
        /// The single value both bounds have.
        at: f64,
    },
    /// A logarithmic scale with a bound at or below zero.
    #[error("a logarithmic scale needs positive bounds, not {min} and {max}")]
    LogBoundNotPositive {
        /// The lower bound.
        min: f64,
        /// The upper bound.
        max: f64,
    },
    /// A value that has no logarithm.
    #[error("{value} has no logarithm: a logarithmic scale holds positive values only")]
    NotPositive {
        /// The value refused.
        value: f64,
    },
    /// The quantiles behind automatic bounds were refused.
    #[error(transparent)]
    Quantiles(#[from] QuantileRefusal),
}

impl Scale {
    /// A scale over `min..max`. Refused, in this order, for a bound that is not
    /// finite, `min > max`, `min == max`, and a logarithmic scale whose `min`
    /// is not positive. `reverse` is a display direction, never a swap of the
    /// bounds.
    pub fn new(kind: ScaleKind, min: f64, max: f64, reverse: bool) -> Result<Scale, ScaleRefusal> {
        if !min.is_finite() || !max.is_finite() {
            return Err(ScaleRefusal::NotFinite { min, max });
        }
        if min > max {
            return Err(ScaleRefusal::Reversed { min, max });
        }
        if min == max {
            return Err(ScaleRefusal::Degenerate { at: min });
        }
        if kind == ScaleKind::Log10 && min <= 0.0 {
            return Err(ScaleRefusal::LogBoundNotPositive { min, max });
        }
        Ok(Scale {
            kind,
            min,
            max,
            reverse,
        })
    }

    /// Linear or logarithmic.
    pub fn kind(&self) -> ScaleKind {
        self.kind
    }

    /// The lower bound.
    pub fn min(&self) -> f64 {
        self.min
    }

    /// The upper bound.
    pub fn max(&self) -> f64 {
        self.max
    }

    /// Whether the display direction is reversed.
    pub fn reverse(&self) -> bool {
        self.reverse
    }

    /// The value in the space of the scale: itself, or its `log10`. A value at
    /// or below zero on a logarithmic scale is refused by name, never turned
    /// into a `NaN`. A value that is not finite is returned as it is.
    pub fn transform(&self, value: f64) -> Result<f64, ScaleRefusal> {
        match self.kind {
            ScaleKind::Linear => Ok(value),
            ScaleKind::Log10 => {
                if value <= 0.0 {
                    Err(ScaleRefusal::NotPositive { value })
                } else {
                    Ok(value.log10())
                }
            }
        }
    }

    /// Where `value` falls: see [`fraction`].
    pub fn fraction(&self, value: f64) -> Fraction {
        fraction(value, self)
    }

    /// The inverse of [`Scale::fraction`], the direction applied. The fraction
    /// is not clamped: a tick outside the frame can be computed.
    pub fn value_at(&self, fraction: f64) -> f64 {
        let f = if self.reverse { 1.0 - fraction } else { fraction };
        let (lo, hi) = (self.space(self.min), self.space(self.max));
        let t = lo + f * (hi - lo);
        match self.kind {
            ScaleKind::Linear => t,
            ScaleKind::Log10 => 10f64.powf(t),
        }
    }

    /// `log10(max / min)` when both bounds are positive.
    pub fn decades(&self) -> Option<f64> {
        if self.min > 0.0 && self.max > 0.0 {
            Some((self.max / self.min).log10())
        } else {
            None
        }
    }

    /// A bound in the space of the scale (bounds are positive on a log scale).
    fn space(&self, value: f64) -> f64 {
        match self.kind {
            ScaleKind::Linear => value,
            ScaleKind::Log10 => value.log10(),
        }
    }
}

/// Resolve a request: the bounds the caller fixed, completed by `auto` (the
/// automatic bounds, see [`auto_bounds`]) where it fixed none.
pub fn resolve(spec: &ScaleSpec, auto: Option<(f64, f64)>) -> Result<Scale, ScaleRefusal> {
    let min = spec.min.or(auto.map(|a| a.0));
    let max = spec.max.or(auto.map(|a| a.1));
    match (min, max) {
        (Some(min), Some(max)) => Scale::new(spec.kind, min, max, spec.reverse),
        _ => Err(ScaleRefusal::NoValues { kind: spec.kind }),
    }
}

/// Where `value` falls on `scale`.
///
/// - not finite: [`Fraction::Gap`];
/// - a logarithmic scale and `value <= 0`: [`Fraction::NotPositive`];
/// - under `min` or over `max`: [`Fraction::Below`], [`Fraction::Above`];
/// - otherwise `At(f)`, `f` in `0..=1`, mirrored when the scale is reversed.
pub fn fraction(value: f64, scale: &Scale) -> Fraction {
    if !value.is_finite() {
        return Fraction::Gap;
    }
    if scale.kind == ScaleKind::Log10 && value <= 0.0 {
        return Fraction::NotPositive;
    }
    if value < scale.min {
        return Fraction::Below;
    }
    if value > scale.max {
        return Fraction::Above;
    }
    let (lo, hi) = (scale.space(scale.min), scale.space(scale.max));
    let f = (scale.space(value) - lo) / (hi - lo);
    Fraction::At(if scale.reverse { 1.0 - f } else { f })
}

/// The automatic bounds: the first and ninety-ninth percentiles of the finite
/// values, and on a logarithmic scale of the finite values `> 0` (the others
/// are set aside). A constant gives equal bounds, which [`resolve`] refuses.
pub fn auto_bounds(values: &[f64], kind: ScaleKind, method: QuantileMethod) -> Result<(f64, f64), ScaleRefusal> {
    let population: Vec<f64> = values
        .iter()
        .copied()
        .filter(|v| v.is_finite() && (kind == ScaleKind::Linear || *v > 0.0))
        .collect();
    if population.is_empty() {
        return Err(ScaleRefusal::NoValues { kind });
    }
    let q = quantiles(&population, &[AUTO_LOW, AUTO_HIGH], method)?;
    Ok((q.values[0], q.values[1]))
}

/// Whether to propose a logarithmic scale: the positive values span at least
/// [`LOG_DECADES`] decades. Decided on the decades alone, not on the kind of
/// property (S8).
pub fn propose_log(decades: Option<f64>) -> bool {
    decades.is_some_and(|d| d >= LOG_DECADES)
}
