//! Two sister arrays compared value by value: density, ratio, correlation, and
//! the share of ratios a [`CellRule`] matches.
//!
//! A pair with a member that is not finite is **dropped**: counted and left
//! out, never plotted. A ratio is undefined when its denominator is zero: also
//! counted, never an infinity. The rule that a directional ratio must satisfy
//! (S10) is not here; this module only counts what a rule matches.
//!
//! The co-moments are kept in one pass and merged by the formulas of P. Pebay,
//! *Formulas for robust, one-pass parallel computation of covariances and
//! arbitrary-order statistical moments*, Sandia report SAND2008-6212, 2008.

use super::cells::{CellRule, TallyRefusal};
use super::scale::{Fraction, Scale};
use super::sketch::{LogSketch, SKETCH_RELATIVE_ERROR, SketchRefusal};

/// The default side of a density grid.
pub const DENSITY_SIZE: usize = 64;
/// The largest side of a density grid.
pub const MAX_DENSITY_SIZE: usize = 1024;

/// The probabilities of the quantiles of the ratio.
const RATIO_QUANTILES: [f64; 5] = [0.01, 0.1, 0.5, 0.9, 0.99];

/// Pair counts on a regular grid over two scales.
#[derive(Clone, Debug, PartialEq)]
pub struct DensityGrid {
    /// The scale of the first member.
    pub x: Scale,
    /// The scale of the second member.
    pub y: Scale,
    /// The grid is `n` by `n`.
    pub n: usize,
    /// `counts[row * n + col]`, row 0 at the end of `y` that its fraction 0
    /// names (its `min`, unless the scale is reversed).
    pub counts: Vec<u64>,
    /// Pairs that fell outside either scale.
    pub outside: u64,
}

/// Why a pair computation was refused.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum PairRefusal {
    /// The two members have different lengths.
    #[error("{x} value(s) against {y}: a pair needs one of each")]
    LengthsDiffer {
        /// How many values of the first member.
        x: usize,
        /// How many values of the second member.
        y: usize,
    },
    /// A grid side outside `1..=MAX_DENSITY_SIZE`.
    #[error("a density grid of {n} x {n} is outside 1 to {limit}")]
    GridSize {
        /// The side asked.
        n: usize,
        /// The largest side.
        limit: usize,
    },
    /// Two accumulators over different scales, sizes or rules.
    #[error("two density grids over different scales or sizes do not merge")]
    DifferentGrids,
    /// The ratio rule was refused.
    #[error(transparent)]
    Rule(#[from] TallyRefusal),
    /// The ratio sketch was refused.
    #[error(transparent)]
    Sketch(#[from] SketchRefusal),
}

impl DensityGrid {
    /// An empty `n` by `n` grid over two scales.
    pub fn new(x: Scale, y: Scale, n: usize) -> Result<DensityGrid, PairRefusal> {
        if n == 0 || n > MAX_DENSITY_SIZE {
            return Err(PairRefusal::GridSize {
                n,
                limit: MAX_DENSITY_SIZE,
            });
        }
        Ok(DensityGrid {
            x,
            y,
            n,
            counts: vec![0; n * n],
            outside: 0,
        })
    }

    /// Count one pair; `false` when it falls outside either scale (counted in
    /// `outside`). The last cell is closed: a value at the maximum belongs to
    /// the last column, not to one past it.
    pub fn push(&mut self, x: f64, y: f64) -> bool {
        match (self.x.fraction(x), self.y.fraction(y)) {
            (Fraction::At(fx), Fraction::At(fy)) => {
                let col = ((fx * self.n as f64) as usize).min(self.n - 1);
                let row = ((fy * self.n as f64) as usize).min(self.n - 1);
                self.counts[row * self.n + col] += 1;
                true
            }
            _ => {
                self.outside += 1;
                false
            }
        }
    }

    /// The count of one cell.
    pub fn count(&self, col: usize, row: usize) -> u64 {
        self.counts[row * self.n + col]
    }

    /// Add the counts of `other`; refused when the scales or the size differ.
    pub fn merge(&mut self, other: &DensityGrid) -> Result<(), PairRefusal> {
        if self.n != other.n || self.x != other.x || self.y != other.y {
            return Err(PairRefusal::DifferentGrids);
        }
        for (a, b) in self.counts.iter_mut().zip(&other.counts) {
            *a += *b;
        }
        self.outside += other.outside;
        Ok(())
    }
}

/// The pairs of two arrays, counted in one pass and mergeable.
#[derive(Clone, Debug, PartialEq)]
pub struct PairAccumulator {
    pairs: u64,
    dropped: u64,
    undefined_ratio: u64,
    rule: Option<CellRule>,
    matched: u64,
    density: DensityGrid,
    ratio: LogSketch,
    // Co-moments of the pairs both scales can place.
    n: u64,
    mean_x: f64,
    mean_y: f64,
    m2_x: f64,
    m2_y: f64,
    c_xy: f64,
}

/// What a pair accumulator found.
#[derive(Clone, Debug, PartialEq)]
pub struct PairSummary {
    /// Pairs whose two members are finite.
    pub pairs: u64,
    /// Pairs with a member that is not finite: counted and left out.
    pub dropped: u64,
    /// Pairs whose first member is zero: the ratio does not exist.
    pub undefined_ratio: u64,
    /// The rule the ratios were tried against, if any.
    pub ratio_rule: Option<CellRule>,
    /// Ratios `y / x` that the rule matched.
    pub matched: u64,
    /// `matched / (pairs - undefined_ratio)`; `None` when that is zero.
    pub share_matched: Option<f64>,
    /// `(q, y / x)` for q in 0.01, 0.1, 0.5, 0.9, 0.99, by a sketch (within its
    /// relative error). Empty when no ratio is defined.
    pub ratio_quantiles: Vec<(f64, f64)>,
    /// The density of the pairs.
    pub density: DensityGrid,
    /// Pearson's coefficient, in the space of each scale; `None` under two
    /// pairs or when a member does not vary.
    pub correlation: Option<f64>,
}

impl PairAccumulator {
    /// An accumulator over two scales, a density grid of side `n`, and an
    /// optional rule for the ratio `y / x`.
    pub fn new(x: Scale, y: Scale, n: usize, ratio_rule: Option<CellRule>) -> Result<PairAccumulator, PairRefusal> {
        let density = DensityGrid::new(x, y, n)?;
        if let Some(rule) = &ratio_rule {
            rule.check()?;
        }
        Ok(PairAccumulator {
            pairs: 0,
            dropped: 0,
            undefined_ratio: 0,
            rule: ratio_rule,
            matched: 0,
            density,
            ratio: LogSketch::new(SKETCH_RELATIVE_ERROR)?,
            n: 0,
            mean_x: 0.0,
            mean_y: 0.0,
            m2_x: 0.0,
            m2_y: 0.0,
            c_xy: 0.0,
        })
    }

    /// Add the pairs `(x[i], y[i])`.
    pub fn push(&mut self, x: &[f64], y: &[f64]) -> Result<(), PairRefusal> {
        if x.len() != y.len() {
            return Err(PairRefusal::LengthsDiffer {
                x: x.len(),
                y: y.len(),
            });
        }
        for (x, y) in x.iter().zip(y) {
            let (x, y) = (*x, *y);
            if !x.is_finite() || !y.is_finite() {
                // The crossplot rule: a pair with a hole is not plotted.
                self.dropped += 1;
                continue;
            }
            self.pairs += 1;
            self.density.push(x, y);
            if x == 0.0 {
                self.undefined_ratio += 1;
            } else {
                let ratio = y / x;
                self.ratio.push(ratio);
                if let Some(rule) = &self.rule
                    && rule.matches(ratio) == Some(true)
                {
                    self.matched += 1;
                }
            }
            if let (Ok(tx), Ok(ty)) = (self.density.x.transform(x), self.density.y.transform(y)) {
                self.comoment(tx, ty);
            }
        }
        Ok(())
    }

    /// Welford's update, extended to two variables.
    fn comoment(&mut self, x: f64, y: f64) {
        self.n += 1;
        let n = self.n as f64;
        let dx = x - self.mean_x;
        self.mean_x += dx / n;
        let dy = y - self.mean_y;
        self.mean_y += dy / n;
        self.m2_x += dx * (x - self.mean_x);
        self.m2_y += dy * (y - self.mean_y);
        self.c_xy += dx * (y - self.mean_y);
    }

    /// Fold `other` in. Refused when the grids, the scales or the rules differ.
    pub fn merge(&mut self, other: &PairAccumulator) -> Result<(), PairRefusal> {
        if self.rule != other.rule {
            return Err(PairRefusal::DifferentGrids);
        }
        self.density.merge(&other.density)?;
        self.ratio.merge(&other.ratio)?;
        self.pairs += other.pairs;
        self.dropped += other.dropped;
        self.undefined_ratio += other.undefined_ratio;
        self.matched += other.matched;
        if other.n > 0 {
            if self.n == 0 {
                self.n = other.n;
                self.mean_x = other.mean_x;
                self.mean_y = other.mean_y;
                self.m2_x = other.m2_x;
                self.m2_y = other.m2_y;
                self.c_xy = other.c_xy;
            } else {
                let (na, nb) = (self.n as f64, other.n as f64);
                let n = na + nb;
                let dx = other.mean_x - self.mean_x;
                let dy = other.mean_y - self.mean_y;
                self.c_xy += other.c_xy + dx * dy * na * nb / n;
                self.m2_x += other.m2_x + dx * dx * na * nb / n;
                self.m2_y += other.m2_y + dy * dy * na * nb / n;
                self.mean_x += dx * nb / n;
                self.mean_y += dy * nb / n;
                self.n += other.n;
            }
        }
        Ok(())
    }

    /// The summary of what was pushed.
    pub fn finish(self) -> PairSummary {
        let defined = self.pairs - self.undefined_ratio;
        let correlation = if self.n >= 2 && self.m2_x > 0.0 && self.m2_y > 0.0 {
            Some(self.c_xy / (self.m2_x * self.m2_y).sqrt())
        } else {
            None
        };
        PairSummary {
            pairs: self.pairs,
            dropped: self.dropped,
            undefined_ratio: self.undefined_ratio,
            ratio_rule: self.rule,
            matched: self.matched,
            share_matched: if defined > 0 {
                Some(self.matched as f64 / defined as f64)
            } else {
                None
            },
            ratio_quantiles: RATIO_QUANTILES
                .iter()
                .filter_map(|q| self.ratio.quantile(*q).map(|v| (*q, v)))
                .collect(),
            density: self.density,
            correlation,
        }
    }
}
