//! A histogram whose bins are equal **in the space of a scale**: equal steps of
//! the value on a linear scale, equal decades on a log10 scale.
//!
//! The two rules of a histogram hold here as everywhere in this module: the
//! last bin is **closed** (a value equal to the maximum belongs to it, not to
//! a bin past the end), and `min` is not multiplied out (`(t - t_min) / width`
//! keeps the first bin exact when the range is tiny next to the values).
//!
//! A value that is outside the scale or has no place on it is counted apart
//! (`below`, `above`, `not_positive`, `not_finite`), never put in the nearest
//! bin: an absence is not a value.

use super::modes::MAX_BINS;
use super::scale::{Scale, ScaleKind};
use super::sketch::LogSketch;

/// What the counts were made from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BinnedFrom {
    /// Every value, one by one: exact.
    Values,
    /// A sketch: approximate, within this relative error.
    Sketch {
        /// The relative error of the sketch.
        relative_error: f64,
    },
}

/// A histogram over a resolved scale.
#[derive(Clone, Debug, PartialEq)]
pub struct Binned {
    /// `min` and `max` are the outer edges; `reverse` is ignored here.
    pub scale: Scale,
    /// Counts from the lowest bin to the highest; the last bin is closed.
    pub counts: Vec<u64>,
    /// Values under the scale's `min`.
    pub below: u64,
    /// Values over the scale's `max`.
    pub above: u64,
    /// Log10 only: values at or below zero, never in a bin.
    pub not_positive: u64,
    /// Values that are `NaN` or infinite.
    pub not_finite: u64,
    /// Exact or approximate.
    pub from: BinnedFrom,
}

/// Why a histogram was refused.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum BinRefusal {
    /// Zero bins asked for.
    #[error("no bin asked for")]
    NoBin,
    /// More bins than this build draws.
    #[error("{bins} bins asked for, more than the {limit} this build draws")]
    TooManyBins {
        /// The number asked for.
        bins: usize,
        /// The most this build draws.
        limit: usize,
    },
}

/// The value of a point of the scale's own space.
fn inverse(kind: ScaleKind, t: f64) -> f64 {
    match kind {
        ScaleKind::Linear => t,
        ScaleKind::Log10 => 10f64.powf(t),
    }
}

/// A bound of the scale in its own space (bounds are positive on a log scale).
fn space(scale: &Scale, value: f64) -> f64 {
    scale.transform(value).unwrap_or(f64::NAN)
}

impl Binned {
    /// How many bins.
    pub fn bins(&self) -> usize {
        self.counts.len()
    }

    fn width(&self) -> f64 {
        (space(&self.scale, self.scale.max()) - space(&self.scale, self.scale.min())) / self.bins() as f64
    }

    fn edge(&self, k: usize) -> f64 {
        let t = space(&self.scale, self.scale.min()) + k as f64 * self.width();
        inverse(self.scale.kind(), t)
    }

    /// The two edges of bin `bin`, in value space (`10^` of the edge on a
    /// log10 scale).
    pub fn edges(&self, bin: usize) -> (f64, f64) {
        (self.edge(bin), self.edge(bin + 1))
    }

    /// The middle of bin `bin` in the space of the scale, back in value space.
    pub fn center(&self, bin: usize) -> f64 {
        let t = space(&self.scale, self.scale.min()) + (bin as f64 + 0.5) * self.width();
        inverse(self.scale.kind(), t)
    }

    /// The sum of the counts of the bins.
    pub fn total(&self) -> u64 {
        self.counts.iter().sum()
    }

    /// Count `n` values equal to `value`.
    fn add(&mut self, value: f64, n: u64) {
        if !value.is_finite() {
            self.not_finite += n;
            return;
        }
        if self.scale.kind() == ScaleKind::Log10 && value <= 0.0 {
            self.not_positive += n;
            return;
        }
        if value < self.scale.min() {
            self.below += n;
            return;
        }
        if value > self.scale.max() {
            self.above += n;
            return;
        }
        let bins = self.bins();
        let t_min = space(&self.scale, self.scale.min());
        let t = space(&self.scale, value);
        // `min` is not multiplied out, and the last bin is closed: the maximum
        // lands in the last bin, not one past it.
        let bin = ((t - t_min) / self.width()) as usize;
        self.counts[bin.min(bins - 1)] += n;
    }
}

fn empty(scale: &Scale, bins: usize, from: BinnedFrom) -> Result<Binned, BinRefusal> {
    if bins == 0 {
        return Err(BinRefusal::NoBin);
    }
    if bins > MAX_BINS {
        return Err(BinRefusal::TooManyBins {
            bins,
            limit: MAX_BINS,
        });
    }
    Ok(Binned {
        scale: *scale,
        counts: vec![0; bins],
        below: 0,
        above: 0,
        not_positive: 0,
        not_finite: 0,
        from,
    })
}

/// Count `values` in `bins` equal bins of `scale`.
pub fn binned_exact(values: &[f64], scale: &Scale, bins: usize) -> Result<Binned, BinRefusal> {
    let mut binned = empty(scale, bins, BinnedFrom::Values)?;
    for value in values {
        binned.add(*value, 1);
    }
    Ok(binned)
}

/// Count the buckets of a sketch, each as one value at its representative.
/// Approximate: the result says so in `from`.
pub fn binned_from_sketch(sketch: &LogSketch, scale: &Scale, bins: usize) -> Result<Binned, BinRefusal> {
    let mut binned = empty(
        scale,
        bins,
        BinnedFrom::Sketch {
            relative_error: sketch.relative_error(),
        },
    )?;
    for (value, n) in sketch.buckets() {
        binned.add(value, n);
    }
    binned.not_finite += sketch.not_finite();
    Ok(binned)
}
