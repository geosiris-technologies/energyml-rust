//! Counting the cells of a matrix that satisfy a rule, and the runs of rows
//! where their share is high.
//!
//! A value that is not finite is **absent**: neither matched nor not matched.
//! A row of absent values is never part of a run: a hole is not an alert.

use std::ops::Range;

/// A test on one value. The list is closed: every interval test is one of
/// these three.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CellRule {
    /// `value < threshold`.
    Below(f64),
    /// `value > threshold`.
    Above(f64),
    /// `value < lo || value > hi`.
    Outside {
        /// The lower bound of the accepted interval.
        lo: f64,
        /// The upper bound of the accepted interval.
        hi: f64,
    },
}

impl CellRule {
    /// Refuse a threshold that is not finite and a reversed interval.
    pub fn check(&self) -> Result<(), TallyRefusal> {
        match *self {
            CellRule::Below(at) | CellRule::Above(at) => {
                if at.is_finite() {
                    Ok(())
                } else {
                    Err(TallyRefusal::ThresholdNotFinite { at })
                }
            }
            CellRule::Outside { lo, hi } => {
                if !lo.is_finite() {
                    Err(TallyRefusal::ThresholdNotFinite { at: lo })
                } else if !hi.is_finite() {
                    Err(TallyRefusal::ThresholdNotFinite { at: hi })
                } else if lo > hi {
                    Err(TallyRefusal::BoundsReversed { lo, hi })
                } else {
                    Ok(())
                }
            }
        }
    }

    /// Whether `value` satisfies the rule; `None` for a value that is not
    /// finite: absent, neither matched nor not.
    pub fn matches(&self, value: f64) -> Option<bool> {
        if !value.is_finite() {
            return None;
        }
        Some(match *self {
            CellRule::Below(at) => value < at,
            CellRule::Above(at) => value > at,
            CellRule::Outside { lo, hi } => value < lo || value > hi,
        })
    }
}

/// The cells of a matrix that satisfy a rule, in total and row by row.
#[derive(Clone, Debug, PartialEq)]
pub struct RuleTally {
    /// The rule counted.
    pub rule: CellRule,
    /// The number of rows.
    pub rows: usize,
    /// The number of columns.
    pub cols: usize,
    /// Every cell.
    pub cells: u64,
    /// Cells that satisfy the rule.
    pub matched: u64,
    /// Cells that are not finite.
    pub absent: u64,
    /// Per row, the cells that satisfy the rule.
    pub matched_per_row: Vec<u32>,
    /// Per row, the cells that are finite.
    pub present_per_row: Vec<u32>,
}

/// Why a tally was refused.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum TallyRefusal {
    /// A matrix of no column.
    #[error("a matrix of no column")]
    NoColumn,
    /// The values do not make whole rows.
    #[error("{values} value(s) do not make whole rows of {cols}")]
    Ragged {
        /// How many values were given.
        values: usize,
        /// The number of columns.
        cols: usize,
    },
    /// More columns than a per-row count can hold.
    #[error("{cols} columns: more than a row count can hold")]
    TooManyColumns {
        /// The number of columns.
        cols: usize,
    },
    /// A threshold that is `NaN` or infinite.
    #[error("the threshold {at} is not finite")]
    ThresholdNotFinite {
        /// The threshold.
        at: f64,
    },
    /// An interval whose `lo` is above its `hi`.
    #[error("the interval [{lo}, {hi}] is reversed")]
    BoundsReversed {
        /// The lower bound.
        lo: f64,
        /// The upper bound.
        hi: f64,
    },
    /// A share outside `(0, 1]`.
    #[error("a share of {share} is outside (0, 1]")]
    ShareOutOfRange {
        /// The share asked.
        share: f64,
    },
}

/// Count, row by row, the cells of `values` (row-major, `cols` per row) that
/// satisfy `rule`.
pub fn tally(values: &[f64], cols: usize, rule: &CellRule) -> Result<RuleTally, TallyRefusal> {
    if cols == 0 {
        return Err(TallyRefusal::NoColumn);
    }
    if cols > u32::MAX as usize {
        return Err(TallyRefusal::TooManyColumns { cols });
    }
    if values.len() % cols != 0 {
        return Err(TallyRefusal::Ragged {
            values: values.len(),
            cols,
        });
    }
    rule.check()?;
    let rows = values.len() / cols;
    let mut matched_per_row = Vec::with_capacity(rows);
    let mut present_per_row = Vec::with_capacity(rows);
    let (mut matched, mut absent) = (0u64, 0u64);
    for row in values.chunks_exact(cols) {
        let (mut m, mut p) = (0u32, 0u32);
        for value in row {
            match rule.matches(*value) {
                Some(true) => {
                    m += 1;
                    p += 1;
                }
                Some(false) => p += 1,
                None => absent += 1,
            }
        }
        matched += u64::from(m);
        matched_per_row.push(m);
        present_per_row.push(p);
    }
    Ok(RuleTally {
        rule: *rule,
        rows,
        cols,
        cells: values.len() as u64,
        matched,
        absent,
        matched_per_row,
        present_per_row,
    })
}

/// The ranges of consecutive rows where at least `share` of the present cells
/// satisfy the rule. `share` is in `(0, 1]`. A row with no present cell is
/// never in a range.
pub fn runs(tally: &RuleTally, share: f64) -> Result<Vec<Range<usize>>, TallyRefusal> {
    if share.is_nan() || share <= 0.0 || share > 1.0 {
        return Err(TallyRefusal::ShareOutOfRange { share });
    }
    let mut out: Vec<Range<usize>> = Vec::new();
    let mut open: Option<usize> = None;
    for row in 0..tally.rows {
        let present = tally.present_per_row[row];
        let alert = present > 0 && f64::from(tally.matched_per_row[row]) / f64::from(present) >= share;
        match (alert, open) {
            (true, None) => open = Some(row),
            (false, Some(start)) => {
                out.push(start..row);
                open = None;
            }
            _ => {}
        }
    }
    if let Some(start) = open {
        out.push(start..tally.rows);
    }
    Ok(out)
}
