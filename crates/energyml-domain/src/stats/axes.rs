//! Along which axis an array varies most, and where it **jumps**.
//!
//! For each axis of an array, the mean absolute step between neighbours, and
//! the positions where the step is more than [`JUMP_RATIO`] times the median
//! step of that axis (a seam, decision S7). A step is measured **in indices**:
//! turning it into "per metre" is the caller's, who has the coordinates of the
//! axis.
//!
//! A pair of neighbours of which one member is not valid makes no step: a hole
//! is not bridged. A value is valid when it is finite, and on a logarithmic
//! scale when it is also positive (the step is then in decades).

use super::scale::ScaleKind;

pub use crate::along_md::JUMP_RATIO;

/// An axis dominates the others when its mean step is at least this many times
/// the mean of theirs.
pub const STRONGEST_RATIO: f64 = 1.5;

/// A position along an axis where the step is many times the median step of
/// the axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Jump {
    /// The axis, as a rank in `dims`, slowest first.
    pub axis: usize,
    /// The position along the axis: the step is between `at - 1` and `at`.
    pub at: u64,
    /// The mean absolute step at that position.
    pub step: f64,
    /// `step` over the median step; infinite when the median is zero.
    pub ratio_to_median: f64,
}

/// How an array varies along one axis.
#[derive(Clone, Debug, PartialEq)]
pub struct AxisVariation {
    /// The rank in `dims`, slowest first.
    pub axis: usize,
    /// The length of the axis.
    pub len: u64,
    /// Pairs of neighbours that were both valid, counted.
    pub steps: u64,
    /// `None` for an axis of length 1 or with no valid pair: an absence, never
    /// zero.
    pub mean_abs_step: Option<f64>,
    /// The median, over the positions, of the mean absolute step at each.
    pub median_step: Option<f64>,
    /// The seams, ascending in `at`.
    pub jumps: Vec<Jump>,
    /// Valid pairs exist and every step is zero.
    pub constant: bool,
}

/// Why a variation was refused.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum AxesRefusal {
    /// An array of no dimension.
    #[error("an array of no dimension has no axis")]
    NoDimension,
    /// The values do not fill whole slabs.
    #[error("{values} value(s) do not fill whole slabs of {slab} along the slowest axis")]
    NotWholeSlabs {
        /// How many values were given.
        values: usize,
        /// The size of a slab.
        slab: u64,
    },
    /// Slabs pushed out of order.
    #[error(
        "slab {first} was pushed where slab {expected} was due: steps along the slowest axis need slabs in order"
    )]
    NotContiguous {
        /// The slab that was due.
        expected: u64,
        /// The slab that came.
        first: u64,
    },
    /// More slabs than the slowest axis holds.
    #[error("{slabs} slab(s) past the {len} the slowest axis holds")]
    PastTheEnd {
        /// How many slabs lie beyond the end.
        slabs: u64,
        /// The length of the slowest axis.
        len: u64,
    },
    /// The dimensions hold more values than this build can index.
    #[error("the dimensions {dims:?} hold more values than this build can index")]
    TooLarge {
        /// The dimensions.
        dims: Vec<u64>,
    },
}

/// The sums of absolute steps and their counts, per position, per axis.
#[derive(Clone, Debug)]
pub struct AxesAccumulator {
    dims: Vec<u64>,
    kind: ScaleKind,
    slab: usize,
    total: u64,
    next: u64,
    carry: Vec<f64>,
    sums: Vec<Vec<f64>>,
    counts: Vec<Vec<u64>>,
}

impl AxesAccumulator {
    /// An accumulator for an array of dimensions `dims` (slowest first), taking
    /// steps in the space of `kind`.
    pub fn new(dims: &[u64], kind: ScaleKind) -> Result<AxesAccumulator, AxesRefusal> {
        if dims.is_empty() {
            return Err(AxesRefusal::NoDimension);
        }
        let too_large = || AxesRefusal::TooLarge { dims: dims.to_vec() };
        let mut total = 1u64;
        for dim in dims {
            total = total.checked_mul(*dim).ok_or_else(too_large)?;
        }
        let mut slab = 1usize;
        for dim in &dims[1..] {
            slab = slab
                .checked_mul(usize::try_from(*dim).map_err(|_| too_large())?)
                .ok_or_else(too_large)?;
        }
        let mut sums = Vec::with_capacity(dims.len());
        let mut counts = Vec::with_capacity(dims.len());
        for dim in dims {
            let len = usize::try_from(*dim).map_err(|_| too_large())?;
            sums.push(vec![0.0; len]);
            counts.push(vec![0u64; len]);
        }
        Ok(AxesAccumulator {
            dims: dims.to_vec(),
            kind,
            slab,
            total,
            next: 0,
            carry: Vec::new(),
            sums,
            counts,
        })
    }

    /// A value in the space of the steps; `NaN` when it is not valid.
    fn valid(&self, value: f64) -> f64 {
        if !value.is_finite() {
            return f64::NAN;
        }
        match self.kind {
            ScaleKind::Linear => value,
            ScaleKind::Log10 => {
                if value > 0.0 {
                    value.log10()
                } else {
                    f64::NAN
                }
            }
        }
    }

    /// Add whole slabs of the slowest axis, `first` being the index of the
    /// first one. Slabs must come in order and without a gap: the first step
    /// along the slowest axis uses the last slab of the previous call.
    pub fn push_slabs(&mut self, first: u64, values: &[f64]) -> Result<(), AxesRefusal> {
        if self.slab == 0 {
            // A dimension of zero: no value at all.
            return if values.is_empty() {
                Ok(())
            } else {
                Err(AxesRefusal::NotWholeSlabs {
                    values: values.len(),
                    slab: 0,
                })
            };
        }
        if !values.len().is_multiple_of(self.slab) {
            return Err(AxesRefusal::NotWholeSlabs {
                values: values.len(),
                slab: self.slab as u64,
            });
        }
        if first != self.next {
            return Err(AxesRefusal::NotContiguous {
                expected: self.next,
                first,
            });
        }
        let count = (values.len() / self.slab) as u64;
        if first + count > self.dims[0] {
            return Err(AxesRefusal::PastTheEnd {
                slabs: first + count - self.dims[0],
                len: self.dims[0],
            });
        }
        let strides: Vec<usize> = (0..self.dims.len())
            .map(|a| self.dims[a + 1..].iter().map(|d| *d as usize).product())
            .collect();
        for s in 0..count as usize {
            let i0 = first as usize + s;
            let slab: Vec<f64> = values[s * self.slab..(s + 1) * self.slab]
                .iter()
                .map(|v| self.valid(*v))
                .collect();
            // Axis 0: each cell against the same cell of the slab before.
            if i0 >= 1 && self.carry.len() == self.slab {
                for (now, before) in slab.iter().zip(&self.carry) {
                    let step = (now - before).abs();
                    if !step.is_nan() {
                        self.sums[0][i0] += step;
                        self.counts[0][i0] += 1;
                    }
                }
            }
            // Axes 1 and up: neighbours inside the slab.
            for (a, (stride, dim)) in strides.iter().zip(&self.dims).enumerate().skip(1) {
                let (stride, len) = (*stride, *dim as usize);
                for j in 0..self.slab {
                    let c = (j / stride) % len;
                    if c >= 1 {
                        let step = (slab[j] - slab[j - stride]).abs();
                        if !step.is_nan() {
                            self.sums[a][c] += step;
                            self.counts[a][c] += 1;
                        }
                    }
                }
            }
            self.carry = slab;
        }
        self.next += count;
        Ok(())
    }

    /// The variation along every axis, slowest first.
    pub fn finish(self) -> Vec<AxisVariation> {
        let mut out = Vec::with_capacity(self.dims.len());
        for (a, (counts, sums)) in self.counts.iter().zip(&self.sums).enumerate() {
            let steps: u64 = counts.iter().sum();
            let total: f64 = sums.iter().sum();
            let mut per_position: Vec<(u64, f64)> = Vec::new();
            for (p, (n, sum)) in counts.iter().zip(sums).enumerate() {
                if *n > 0 {
                    per_position.push((p as u64, sum / *n as f64));
                }
            }
            let mut steps_here: Vec<f64> = per_position.iter().map(|(_, s)| *s).collect();
            let median = super::quantile::median(&mut steps_here);
            let mut jumps = Vec::new();
            if let Some(median) = median {
                for (at, step) in &per_position {
                    let jump = if median > 0.0 {
                        *step > JUMP_RATIO * median
                    } else {
                        *step > 0.0
                    };
                    if jump {
                        jumps.push(Jump {
                            axis: a,
                            at: *at,
                            step: *step,
                            ratio_to_median: if median > 0.0 {
                                *step / median
                            } else {
                                f64::INFINITY
                            },
                        });
                    }
                }
            }
            out.push(AxisVariation {
                axis: a,
                len: self.dims[a],
                steps,
                mean_abs_step: if steps > 0 {
                    Some(total / steps as f64)
                } else {
                    None
                },
                median_step: median,
                jumps,
                constant: steps > 0 && total == 0.0,
            });
        }
        out
    }
}

/// The variation along every axis of `values` held in memory.
pub fn variation(values: &[f64], dims: &[u64], kind: ScaleKind) -> Result<Vec<AxisVariation>, AxesRefusal> {
    let mut accumulator = AxesAccumulator::new(dims, kind)?;
    if values.len() as u64 != accumulator.total {
        return Err(AxesRefusal::NotWholeSlabs {
            values: values.len(),
            slab: accumulator.slab as u64,
        });
    }
    accumulator.push_slabs(0, values)?;
    Ok(accumulator.finish())
}

/// The axis that varies most, and by how much more than the mean of the
/// others; `None` when none dominates.
///
/// Among the axes that have a mean step; fewer than two give `None`. When the
/// others do not vary at all the ratio is infinite. A dominating axis needs a
/// mean step above `ratio` times the mean of the others, and above zero.
pub fn strongest(axes: &[AxisVariation], ratio: f64) -> Option<(usize, f64)> {
    let with_step: Vec<(usize, f64)> = axes
        .iter()
        .filter_map(|a| a.mean_abs_step.map(|m| (a.axis, m)))
        .collect();
    if with_step.len() < 2 {
        return None;
    }
    let (axis, m) = with_step
        .iter()
        .copied()
        .reduce(|best, next| if next.1 > best.1 { next } else { best })?;
    if m <= 0.0 {
        return None;
    }
    let others: f64 = with_step
        .iter()
        .filter(|(a, _)| *a != axis)
        .map(|(_, v)| *v)
        .sum::<f64>()
        / (with_step.len() - 1) as f64;
    if others == 0.0 {
        return Some((axis, f64::INFINITY));
    }
    if m >= ratio * others {
        Some((axis, m / others))
    } else {
        None
    }
}
