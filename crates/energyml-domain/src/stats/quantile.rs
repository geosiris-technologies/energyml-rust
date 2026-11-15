//! Quantiles of an array: exact up to a limit, a sketch beyond it.
//!
//! The exact definition is type 7 of R. J. Hyndman and Y. Fan, *Sample
//! Quantiles in Statistical Packages*, The American Statistician 50(4), 1996,
//! p. 361-365: with `h = (n - 1) q` over the sorted finite values,
//! `Q(q) = x[floor(h)] + (h - floor(h)) (x[floor(h) + 1] - x[floor(h)])` (the
//! default of R and of NumPy).
//!
//! Above [`EXACT_QUANTILE_LIMIT`] values the answer comes from a
//! [`LogSketch`](super::sketch::LogSketch) and says so in
//! [`Quantiles::method`], with its relative error.

use super::sketch::{LogSketch, SKETCH_RELATIVE_ERROR, SketchRefusal};

/// Most finite values whose quantiles are computed exactly; beyond it a
/// sketch of declared relative error answers instead.
pub const EXACT_QUANTILE_LIMIT: u64 = 10_000_000;

/// How the caller wants the quantiles computed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QuantileMethod {
    /// Exact up to [`EXACT_QUANTILE_LIMIT`] finite values, a sketch of
    /// [`SKETCH_RELATIVE_ERROR`] above.
    Auto,
    /// Exact whatever the size (the caller asked for it, so it is done).
    Exact,
    /// A sketch of this relative error.
    Sketch {
        /// The relative error the sketch guarantees.
        relative_error: f64,
    },
}

/// How the quantiles were in fact computed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QuantileMethodUsed {
    /// Hyndman and Fan's type 7, over every finite value.
    Exact,
    /// The order statistic `x[floor(q (n - 1))]`, within `relative_error`.
    Sketch {
        /// The relative error the sketch guarantees.
        relative_error: f64,
    },
}

/// The quantiles of an array of values.
#[derive(Clone, Debug, PartialEq)]
pub struct Quantiles {
    /// The probabilities asked, in the order asked.
    pub at: Vec<f64>,
    /// One value per probability of `at`.
    pub values: Vec<f64>,
    /// The population they are taken over: the finite values.
    pub finite: u64,
    /// `NaN` and infinities: counted and left out.
    pub set_aside: u64,
    /// How they were computed.
    pub method: QuantileMethodUsed,
}

/// Why quantiles were refused.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum QuantileRefusal {
    /// No probability was given.
    #[error("no quantile was asked for")]
    NothingAsked,
    /// A probability outside `[0, 1]`, or not a number.
    #[error("{at} is not a probability between 0 and 1")]
    OutOfRange {
        /// The first offending probability.
        at: f64,
    },
    /// Nothing finite to take a quantile of.
    #[error("no finite value to take a quantile of ({set_aside} value(s) set aside)")]
    NoFiniteValue {
        /// How many values were not finite.
        set_aside: u64,
    },
    /// The sketch refused its accuracy.
    #[error(transparent)]
    Sketch(#[from] SketchRefusal),
}

/// The method that will be used for `finite` values.
pub(crate) fn choose(finite: u64, method: QuantileMethod) -> QuantileMethodUsed {
    match method {
        QuantileMethod::Exact => QuantileMethodUsed::Exact,
        QuantileMethod::Sketch { relative_error } => QuantileMethodUsed::Sketch { relative_error },
        QuantileMethod::Auto => {
            if finite <= EXACT_QUANTILE_LIMIT {
                QuantileMethodUsed::Exact
            } else {
                QuantileMethodUsed::Sketch {
                    relative_error: SKETCH_RELATIVE_ERROR,
                }
            }
        }
    }
}

/// The type 7 value at probability `q` of sorted-or-selectable values.
fn type7_by_selection(work: &mut [f64], q: f64) -> f64 {
    let h = (work.len() - 1) as f64 * q;
    let lo = h.floor() as usize;
    work.select_nth_unstable_by(lo, f64::total_cmp);
    let low = work[lo];
    if h > lo as f64 {
        // Everything right of `lo` is at least `low`: the next order
        // statistic is the smallest of them.
        let high = work[lo + 1..]
            .iter()
            .copied()
            .min_by(f64::total_cmp)
            .unwrap_or(low);
        low + (h - lo as f64) * (high - low)
    } else {
        low
    }
}

fn type7_sorted(sorted: &[f64], q: f64) -> f64 {
    let h = (sorted.len() - 1) as f64 * q;
    let lo = h.floor() as usize;
    let low = sorted[lo];
    if h > lo as f64 {
        low + (h - lo as f64) * (sorted[lo + 1] - low)
    } else {
        low
    }
}

/// The exact median of `values`, reordered in place: the middle value, or the
/// mean of the two middle ones for an even count. `None` for no value.
///
/// The one median of the workspace's exact statistics (the along-MD step, the
/// median step of an axis). The values are ordered by `f64::total_cmp`, so a
/// `NaN` sorts to an end and is the caller's to set aside first; it is by
/// selection, not by a full sort. It is the mean of the two middle values
/// that is taken, not the type 7 interpolation of [`quantiles`] at 0.5: the two
/// are the same number up to the last bit of the sum.
pub fn median(values: &mut [f64]) -> Option<f64> {
    let n = values.len();
    if n == 0 {
        return None;
    }
    let (below, middle, _) = values.select_nth_unstable_by(n / 2, f64::total_cmp);
    let middle = *middle;
    if n % 2 == 1 {
        return Some(middle);
    }
    // The lower middle value is the largest of what lies below the upper one.
    let lower = below.iter().copied().max_by(f64::total_cmp).unwrap_or(middle);
    Some((lower + middle) / 2.0)
}

/// The quantiles `at` of `values`; `NaN` and infinities are set aside and
/// counted, never sorted in.
///
/// With [`QuantileMethodUsed::Exact`] the result is Hyndman and Fan's type 7.
/// With a sketch it is the order statistic `x[floor(q (n - 1))]` to within the
/// declared relative error, and the result says so.
pub fn quantiles(values: &[f64], at: &[f64], method: QuantileMethod) -> Result<Quantiles, QuantileRefusal> {
    if at.is_empty() {
        return Err(QuantileRefusal::NothingAsked);
    }
    if let Some(bad) = at.iter().find(|q| !q.is_finite() || **q < 0.0 || **q > 1.0) {
        return Err(QuantileRefusal::OutOfRange { at: *bad });
    }
    let finite = values.iter().filter(|v| v.is_finite()).count() as u64;
    let set_aside = values.len() as u64 - finite;
    if finite == 0 {
        return Err(QuantileRefusal::NoFiniteValue { set_aside });
    }
    let used = choose(finite, method);
    let result = match used {
        QuantileMethodUsed::Exact => {
            let mut work: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
            if at.len() <= 8 {
                at.iter().map(|q| type7_by_selection(&mut work, *q)).collect()
            } else {
                work.sort_unstable_by(f64::total_cmp);
                at.iter().map(|q| type7_sorted(&work, *q)).collect()
            }
        }
        QuantileMethodUsed::Sketch { relative_error } => {
            let mut sketch = LogSketch::new(relative_error)?;
            for value in values {
                sketch.push(*value);
            }
            at.iter()
                .map(|q| sketch.quantile(*q).unwrap_or(f64::NAN))
                .collect()
        }
    };
    Ok(Quantiles {
        at: at.to_vec(),
        values: result,
        finite,
        set_aside,
        method: used,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_median_is_the_middle_or_the_mean_of_the_two_middle_values() {
        assert_eq!(median(&mut []), None);
        assert_eq!(median(&mut [7.0]), Some(7.0));
        assert_eq!(median(&mut [9.0, 1.0, 5.0]), Some(5.0));
        assert_eq!(median(&mut [4.0, 1.0, 3.0, 2.0]), Some(2.5));
        assert_eq!(median(&mut [1.0, 1.0, 1.0, 100.0]), Some(1.0));
        // Break it: take `below.iter().min_by(..)` as the lower middle value and the even case gives 1.5.
    }

    #[test]
    fn auto_switches_to_a_sketch_above_the_limit_and_says_so() {
        // S8: exact up to 10 million, a sketch above. No array of 10 million in
        // a test: the choice is a function of the count.
        assert_eq!(
            choose(EXACT_QUANTILE_LIMIT + 1, QuantileMethod::Auto),
            QuantileMethodUsed::Sketch {
                relative_error: SKETCH_RELATIVE_ERROR
            }
        );
        assert_eq!(
            choose(EXACT_QUANTILE_LIMIT, QuantileMethod::Auto),
            QuantileMethodUsed::Exact
        );
        // An exact request above the limit is honoured.
        assert_eq!(
            choose(EXACT_QUANTILE_LIMIT + 1, QuantileMethod::Exact),
            QuantileMethodUsed::Exact
        );
    }
}
