//! A mergeable quantile sketch with a relative-error guarantee: DDSketch.
//!
//! C. Masson, J. E. Rim, H. K. Lee, *DDSketch: A Fast and Fully-Mergeable
//! Quantile Sketch with Relative-Error Guarantees*, PVLDB 12(12), 2019,
//! p. 2195-2205 (arXiv:1908.10693). The guarantee is on the **value**: a
//! quantile read from the sketch is within `relative_error` of the order
//! statistic it stands for, which is what "an error declared with the result"
//! needs. Two sketches of the same accuracy merge exactly, bucket by bucket.
//!
//! This is the unbounded version (no collapsing of the lowest buckets), so the
//! guarantee always holds; the size stays small, about 115 buckets per decade
//! at 1 %.
//!
//! The sketch aims at the order statistic `x[floor(q (n - 1))]` of the sorted
//! finite values, not at the interpolation of Hyndman and Fan's type 7 that
//! [`quantiles`](super::quantile::quantiles) computes exactly.

use std::collections::BTreeMap;

/// The relative error of the sketches this library builds on its own.
pub const SKETCH_RELATIVE_ERROR: f64 = 0.01;

/// A DDSketch over finite `f64` values.
#[derive(Clone, Debug, PartialEq)]
pub struct LogSketch {
    relative_error: f64,
    gamma: f64,
    ln_gamma: f64,
    positive: BTreeMap<i32, u64>,
    negative: BTreeMap<i32, u64>,
    zero: u64,
    count: u64,
    positive_count: u64,
    not_finite: u64,
    min: f64,
    max: f64,
    min_positive: f64,
}

/// Why a sketch was refused.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SketchRefusal {
    /// The relative error is outside the range that keeps the bucket index in
    /// an `i32`.
    #[error("a relative error of {relative_error} is outside [1e-6, 0.5]")]
    RelativeErrorOutOfRange {
        /// The error asked for.
        relative_error: f64,
    },
    /// Two sketches of different accuracy do not merge.
    #[error("cannot merge a sketch of relative error {theirs} into one of {ours}")]
    DifferentRelativeError {
        /// The error of the sketch merged into.
        ours: f64,
        /// The error of the sketch merged from.
        theirs: f64,
    },
}

impl LogSketch {
    /// An empty sketch of the given relative error, between `1e-6` and `0.5`
    /// (below `1e-6` the bucket index of a value near `1e308` overflows an
    /// `i32`).
    pub fn new(relative_error: f64) -> Result<LogSketch, SketchRefusal> {
        if !relative_error.is_finite() || !(1e-6..=0.5).contains(&relative_error) {
            return Err(SketchRefusal::RelativeErrorOutOfRange { relative_error });
        }
        let gamma = (1.0 + relative_error) / (1.0 - relative_error);
        Ok(LogSketch {
            relative_error,
            gamma,
            ln_gamma: gamma.ln(),
            positive: BTreeMap::new(),
            negative: BTreeMap::new(),
            zero: 0,
            count: 0,
            positive_count: 0,
            not_finite: 0,
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
            min_positive: f64::INFINITY,
        })
    }

    /// Add one value. A value that is not finite is counted apart, never held.
    pub fn push(&mut self, value: f64) {
        if !value.is_finite() {
            self.not_finite += 1;
            return;
        }
        self.count += 1;
        self.min = self.min.min(value);
        self.max = self.max.max(value);
        if value.abs() < f64::MIN_POSITIVE {
            self.zero += 1;
            return;
        }
        // Bucket `i` covers `(gamma^(i-1), gamma^i]`.
        let index = (value.abs().ln() / self.ln_gamma).ceil() as i32;
        if value > 0.0 {
            *self.positive.entry(index).or_default() += 1;
            self.positive_count += 1;
            self.min_positive = self.min_positive.min(value);
        } else {
            *self.negative.entry(index).or_default() += 1;
        }
    }

    /// Fold `other` into this sketch. Refused when the accuracies differ.
    pub fn merge(&mut self, other: &LogSketch) -> Result<(), SketchRefusal> {
        // An exact comparison on purpose: two sketches merge bucket by bucket
        // only when their buckets are the same.
        if self.relative_error != other.relative_error {
            return Err(SketchRefusal::DifferentRelativeError {
                ours: self.relative_error,
                theirs: other.relative_error,
            });
        }
        for (index, n) in &other.positive {
            *self.positive.entry(*index).or_default() += n;
        }
        for (index, n) in &other.negative {
            *self.negative.entry(*index).or_default() += n;
        }
        self.zero += other.zero;
        self.count += other.count;
        self.positive_count += other.positive_count;
        self.not_finite += other.not_finite;
        self.min = self.min.min(other.min);
        self.max = self.max.max(other.max);
        self.min_positive = self.min_positive.min(other.min_positive);
        Ok(())
    }

    /// The relative error this sketch guarantees.
    pub fn relative_error(&self) -> f64 {
        self.relative_error
    }

    /// Finite values held.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Values that were not finite, counted and left out.
    pub fn not_finite(&self) -> u64 {
        self.not_finite
    }

    /// Held values that are `> 0`.
    pub fn positive_count(&self) -> u64 {
        self.positive_count
    }

    /// The representative of bucket `index`: within `relative_error` of both
    /// of its bounds.
    fn representative(&self, index: i32) -> f64 {
        2.0 * self.gamma.powi(index) / (self.gamma + 1.0)
    }

    /// The buckets in ascending order of value: the negative store by
    /// decreasing key, the zero bucket, the positive store by increasing key.
    fn ascending(&self) -> Vec<(f64, u64)> {
        let mut out = Vec::with_capacity(self.negative.len() + self.positive.len() + 1);
        for (index, n) in self.negative.iter().rev() {
            out.push((-self.representative(*index), *n));
        }
        if self.zero > 0 {
            out.push((0.0, self.zero));
        }
        for (index, n) in &self.positive {
            out.push((self.representative(*index), *n));
        }
        out
    }

    /// The value at probability `q`: `None` when nothing is held or `q` is not
    /// in `[0, 1]`. `q == 0` is the minimum and `q == 1` the maximum, exact.
    pub fn quantile(&self, q: f64) -> Option<f64> {
        if self.count == 0 || !q.is_finite() || !(0.0..=1.0).contains(&q) {
            return None;
        }
        if q == 0.0 {
            return Some(self.min);
        }
        if q == 1.0 {
            return Some(self.max);
        }
        // The 0-based rank of the lower order statistic.
        let rank = (q * (self.count - 1) as f64).floor() as u64;
        let mut cumulated = 0u64;
        for (value, n) in self.ascending() {
            cumulated += n;
            if cumulated > rank {
                return Some(value.clamp(self.min, self.max));
            }
        }
        Some(self.max)
    }

    /// The same, over the values `> 0` only: what a logarithmic scale sees.
    /// `None` without a positive value.
    pub fn positive_quantile(&self, q: f64) -> Option<f64> {
        if self.positive_count == 0 || !q.is_finite() || !(0.0..=1.0).contains(&q) {
            return None;
        }
        if q == 0.0 {
            return Some(self.min_positive);
        }
        if q == 1.0 {
            return Some(self.max);
        }
        let rank = (q * (self.positive_count - 1) as f64).floor() as u64;
        let mut cumulated = 0u64;
        for (index, n) in &self.positive {
            cumulated += n;
            if cumulated > rank {
                return Some(self.representative(*index).clamp(self.min_positive, self.max));
            }
        }
        Some(self.max)
    }

    /// Every non-empty bucket as `(representative value, count)`, in ascending
    /// order of value.
    pub fn buckets(&self) -> Vec<(f64, u64)> {
        self.ascending()
    }
}
