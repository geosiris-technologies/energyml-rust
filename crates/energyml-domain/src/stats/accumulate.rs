//! A one-pass, mergeable count of a stream of numbers.
//!
//! The [`Accumulator`] counts everything it is given, sets apart what is not a
//! number (`NaN`, the infinities) and the declared `NullValue` of an integer
//! array, and keeps the moments of the finite values by Welford's update
//! (B. P. Welford, 1962, *Technometrics* 4(3), p. 419-420). Two accumulators
//! over two halves of a stream merge into the accumulator of the whole by the
//! formula of Chan, Golub and LeVeque (*The American Statistician* 37(3), 1983,
//! p. 242-247), so a reader that visits a large array slab by slab, or on
//! several threads, gets the same numbers as one that holds it in memory.
//!
//! An absence is never a zero: a stream with no finite value has no moments
//! (`None`), not moments of zero.

/// Count, extremes, mean and population deviation of a stream of values, in
/// one pass, mergeable.
#[derive(Clone, Debug, PartialEq)]
pub struct Accumulator {
    null_value: Option<i64>,
    count: u64,
    finite: u64,
    not_a_number: u64,
    infinite: u64,
    null: u64,
    // Welford, finite values only.
    min: f64,
    max: f64,
    mean: f64,
    m2: f64,
    // Values > 0.
    positive: u64,
    min_positive: f64,
    max_positive: f64,
    // Welford of log10 of the positive values.
    log_mean: f64,
    log_m2: f64,
}

/// The moments of the finite values: present only when there is at least one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moments {
    /// How many values the moments are taken over.
    pub count: u64,
    /// The smallest value.
    pub min: f64,
    /// The largest value.
    pub max: f64,
    /// The arithmetic mean.
    pub mean: f64,
    /// The population standard deviation (divided by `count`, not `count - 1`).
    pub deviation: f64,
}

/// Why two accumulators cannot be merged.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum MergeRefusal {
    /// The two streams do not declare the same `NullValue`.
    #[error("cannot merge a stream whose NullValue is {theirs:?} into one whose NullValue is {ours:?}")]
    DifferentNullValue {
        /// The `NullValue` of the accumulator merged into.
        ours: Option<i64>,
        /// The `NullValue` of the accumulator merged from.
        theirs: Option<i64>,
    },
}

impl Default for Accumulator {
    fn default() -> Self {
        Self::new()
    }
}

/// The count, mean and sum of squared deviations of two groups, merged
/// (Chan, Golub and LeVeque 1983).
fn chan(na: u64, ma: f64, m2a: f64, nb: u64, mb: f64, m2b: f64) -> (u64, f64, f64) {
    if na == 0 {
        return (nb, mb, m2b);
    }
    if nb == 0 {
        return (na, ma, m2a);
    }
    let n = na + nb;
    let (naf, nbf, nf) = (na as f64, nb as f64, n as f64);
    let d = mb - ma;
    let mean = ma + d * nbf / nf;
    let m2 = m2a + m2b + d * d * naf * nbf / nf;
    (n, mean, m2)
}

impl Accumulator {
    /// An empty accumulator with no declared `NullValue`.
    pub fn new() -> Self {
        Self::with_null_value(None)
    }

    /// An empty accumulator whose stream declares `null_value`: the value of an
    /// integer array that means "no data here" (RESQML
    /// `IntegerExternalArray.NullValue`).
    pub fn with_null_value(null_value: Option<i64>) -> Self {
        Self {
            null_value,
            count: 0,
            finite: 0,
            not_a_number: 0,
            infinite: 0,
            null: 0,
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
            mean: 0.0,
            m2: 0.0,
            positive: 0,
            min_positive: f64::INFINITY,
            max_positive: 0.0,
            log_mean: 0.0,
            log_m2: 0.0,
        }
    }

    /// Count one value.
    pub fn push(&mut self, value: f64) {
        if let Some(null) = self.null_value
            && value == null as f64
        {
            self.count += 1;
            self.null += 1;
            return;
        }
        self.record(value);
    }

    /// Count one integer. The comparison with the declared `NullValue` is made
    /// on the integer, before it is widened: an integer past 2^53 loses its
    /// precision in the `f64` the moments are kept in.
    pub fn push_integer(&mut self, value: i64) {
        if self.null_value == Some(value) {
            self.count += 1;
            self.null += 1;
            return;
        }
        self.record(value as f64);
    }

    /// Count every value of a slice.
    pub fn extend(&mut self, values: &[f64]) {
        for value in values {
            self.push(*value);
        }
    }

    /// Count a value that is not the declared `NullValue`.
    fn record(&mut self, value: f64) {
        self.count += 1;
        if value.is_nan() {
            self.not_a_number += 1;
            return;
        }
        if value.is_infinite() {
            self.infinite += 1;
            return;
        }
        self.finite += 1;
        self.min = self.min.min(value);
        self.max = self.max.max(value);
        // Welford 1962.
        let delta = value - self.mean;
        self.mean += delta / self.finite as f64;
        self.m2 += delta * (value - self.mean);
        if value > 0.0 {
            self.positive += 1;
            self.min_positive = self.min_positive.min(value);
            self.max_positive = self.max_positive.max(value);
            let log = value.log10();
            let delta = log - self.log_mean;
            self.log_mean += delta / self.positive as f64;
            self.log_m2 += delta * (log - self.log_mean);
        }
    }

    /// Fold `other` into this accumulator, as if its values had been pushed
    /// here. Refused when the two streams declare different `NullValue`s.
    pub fn merge(&mut self, other: &Accumulator) -> Result<(), MergeRefusal> {
        if self.null_value != other.null_value {
            return Err(MergeRefusal::DifferentNullValue {
                ours: self.null_value,
                theirs: other.null_value,
            });
        }
        let (finite, mean, m2) = chan(self.finite, self.mean, self.m2, other.finite, other.mean, other.m2);
        let (positive, log_mean, log_m2) = chan(
            self.positive,
            self.log_mean,
            self.log_m2,
            other.positive,
            other.log_mean,
            other.log_m2,
        );
        self.count += other.count;
        self.not_a_number += other.not_a_number;
        self.infinite += other.infinite;
        self.null += other.null;
        self.min = self.min.min(other.min);
        self.max = self.max.max(other.max);
        self.min_positive = self.min_positive.min(other.min_positive);
        self.max_positive = self.max_positive.max(other.max_positive);
        self.finite = finite;
        self.mean = mean;
        self.m2 = m2;
        self.positive = positive;
        self.log_mean = log_mean;
        self.log_m2 = log_m2;
        Ok(())
    }

    /// The declared `NullValue`, if any.
    pub fn null_value(&self) -> Option<i64> {
        self.null_value
    }

    /// Everything pushed.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Finite values.
    pub fn finite(&self) -> u64 {
        self.finite
    }

    /// Values that are `NaN`.
    pub fn not_a_number(&self) -> u64 {
        self.not_a_number
    }

    /// Values that are infinite.
    pub fn infinite(&self) -> u64 {
        self.infinite
    }

    /// `not_a_number` plus `infinite`: the meaning of `Summary::non_finite`.
    pub fn non_finite(&self) -> u64 {
        self.not_a_number + self.infinite
    }

    /// Values equal to the declared `NullValue`.
    pub fn null(&self) -> u64 {
        self.null
    }

    /// Finite values that are `<= 0`: they have no logarithm.
    pub fn non_positive(&self) -> u64 {
        self.finite - self.positive
    }

    /// The moments of the finite values; `None` when there is none, an absence
    /// and never a zero.
    pub fn moments(&self) -> Option<Moments> {
        if self.finite == 0 {
            return None;
        }
        Some(Moments {
            count: self.finite,
            min: self.min,
            max: self.max,
            mean: self.mean,
            deviation: (self.m2 / self.finite as f64).max(0.0).sqrt(),
        })
    }

    /// The moments of `log10` of the positive values (`mean` and `deviation`
    /// are in decades); `min` and `max` are the values themselves.
    pub fn positive_moments(&self) -> Option<Moments> {
        if self.positive == 0 {
            return None;
        }
        Some(Moments {
            count: self.positive,
            min: self.min_positive,
            max: self.max_positive,
            mean: self.log_mean,
            deviation: (self.log_m2 / self.positive as f64).max(0.0).sqrt(),
        })
    }

    /// `log10(max_positive / min_positive)`: how many decades the positive
    /// values span. `None` without a positive value, `0.0` for a single one.
    pub fn decades(&self) -> Option<f64> {
        if self.positive == 0 {
            return None;
        }
        Some((self.max_positive / self.min_positive).log10())
    }
}
