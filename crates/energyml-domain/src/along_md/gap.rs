//! The median step of an axis and the rule of the gap (S7).

use super::GAP_RATIO;
use super::stepper::StepRefusal;

/// The median of the differences between neighbouring MDs, which must be
/// finite and strictly increasing. For an even count of differences, the mean
/// of the two central ones.
pub fn median_step(mds: &[f64]) -> Result<f64, StepRefusal> {
    if mds.len() < 2 {
        return Err(StepRefusal::TooFewNodes { nodes: mds.len() });
    }
    for (at, md) in mds.iter().enumerate() {
        if !md.is_finite() {
            return Err(StepRefusal::NodeNotFinite { at });
        }
    }
    let mut steps = Vec::with_capacity(mds.len() - 1);
    for at in 1..mds.len() {
        if mds[at] <= mds[at - 1] {
            return Err(StepRefusal::NotIncreasing { at });
        }
        steps.push(mds[at] - mds[at - 1]);
    }
    steps.sort_by(f64::total_cmp);
    let n = steps.len();
    Ok(if n % 2 == 1 {
        steps[n / 2]
    } else {
        (steps[n / 2 - 1] + steps[n / 2]) / 2.0
    })
}

/// The widest interval that may be bridged: [`GAP_RATIO`] times the median
/// step, or less, never more (S7). The limit is private so that nobody can
/// loosen it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GapRule {
    max_gap: f64,
}

impl GapRule {
    /// The rule of an axis: [`GAP_RATIO`] times its median step.
    pub fn of_axis(mds: &[f64]) -> Result<GapRule, StepRefusal> {
        Ok(GapRule {
            max_gap: GAP_RATIO * median_step(mds)?,
        })
    }

    /// A caller may be stricter, never looser (S7). A `max_gap` that is not
    /// finite and positive never loosens the rule: it is ignored.
    pub fn stricter(self, max_gap: f64) -> GapRule {
        if max_gap.is_finite() && max_gap > 0.0 {
            GapRule {
                max_gap: self.max_gap.min(max_gap),
            }
        } else {
            self
        }
    }

    /// The widest interval that may be bridged.
    pub fn max_gap(&self) -> f64 {
        self.max_gap
    }

    /// Whether an interval of this length may be bridged. A length that is not
    /// finite or is negative is never bridged.
    pub fn bridges(&self, gap: f64) -> bool {
        gap.is_finite() && gap >= 0.0 && gap <= self.max_gap
    }
}
