//! Stepping the current MD by notches, on the nodes or on a fine grid.

use super::gap::median_step;
use super::bracket::nearest_node;
use super::{MAX_FINE_STEPS, SNAP_FRACTION};

/// How a notch moves the MD.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StepMode {
    /// From node to node.
    Nodes,
    /// On a grid `anchor + k * step`, snapped onto a node that is within a
    /// millionth of a step of a grid point.
    Fine {
        /// The length of a notch, strictly positive.
        step: f64,
        /// A grid point (typically the first node).
        anchor: f64,
    },
}

/// Why an axis, a step or a tolerance is refused.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum StepRefusal {
    /// The axis has no node.
    #[error("an axis of no node")]
    EmptyAxis,
    /// A median step needs two nodes.
    #[error("{nodes} node(s): a step between nodes needs two")]
    TooFewNodes {
        /// How many nodes there were.
        nodes: usize,
    },
    /// A node has no finite MD.
    #[error("node {at} has no finite MD")]
    NodeNotFinite {
        /// Index of the node.
        at: usize,
    },
    /// The MDs do not strictly increase.
    #[error("node {at} is not deeper than the node before it: MDs must strictly increase")]
    NotIncreasing {
        /// Index of the node that is not deeper than its predecessor.
        at: usize,
    },
    /// The fine step is not a positive length.
    #[error("a step of {step} is not a positive length")]
    StepNotPositive {
        /// The step given.
        step: f64,
    },
    /// The anchor of the fine grid is not a number.
    #[error("the anchor {anchor} is not a number")]
    AnchorNotFinite {
        /// The anchor given.
        anchor: f64,
    },
    /// The fine grid has more steps over the axis than allowed.
    #[error("{count} fine steps over the axis, more than the {limit} allowed")]
    TooManySteps {
        /// How many fine steps the axis would need.
        count: u64,
        /// The most allowed ([`MAX_FINE_STEPS`]).
        limit: u64,
    },
    /// The tolerance is negative or not a number.
    #[error("a tolerance of {tolerance} is not a finite non-negative length")]
    BadTolerance {
        /// The tolerance given.
        tolerance: f64,
    },
}

/// The result of a step or a snap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stepped {
    /// The new MD.
    pub md: f64,
    /// The node it is, when it is one: the reading says "node", not "interpolated".
    pub on_node: Option<usize>,
    /// The request went past an end of the axis and was bounded.
    pub clamped: bool,
}

/// Advances an MD by notches over an axis. The axis is already in one unit and
/// datum (see the module doc of `along_md`).
#[derive(Clone, Debug, PartialEq)]
pub struct MdStepper {
    axis: Vec<f64>,
    mode: StepMode,
    epsilon: f64,
}

impl MdStepper {
    /// A stepper over `axis` (finite, strictly increasing, not empty).
    pub fn new(axis: Vec<f64>, mode: StepMode) -> Result<MdStepper, StepRefusal> {
        if axis.is_empty() {
            return Err(StepRefusal::EmptyAxis);
        }
        for (at, md) in axis.iter().enumerate() {
            if !md.is_finite() {
                return Err(StepRefusal::NodeNotFinite { at });
            }
            if at > 0 && *md <= axis[at - 1] {
                return Err(StepRefusal::NotIncreasing { at });
            }
        }
        let epsilon = match mode {
            StepMode::Nodes => {
                if axis.len() >= 2 {
                    SNAP_FRACTION * median_step(&axis)?
                } else {
                    0.0
                }
            }
            StepMode::Fine { step, anchor } => {
                if !step.is_finite() || step <= 0.0 {
                    return Err(StepRefusal::StepNotPositive { step });
                }
                if !anchor.is_finite() {
                    return Err(StepRefusal::AnchorNotFinite { anchor });
                }
                let span = axis[axis.len() - 1] - axis[0];
                let count = (span / step).ceil() as u64;
                if count > MAX_FINE_STEPS {
                    return Err(StepRefusal::TooManySteps {
                        count,
                        limit: MAX_FINE_STEPS,
                    });
                }
                SNAP_FRACTION * step
            }
        };
        Ok(MdStepper { axis, mode, epsilon })
    }

    /// The first (shallowest) MD of the axis.
    pub fn first(&self) -> f64 {
        self.axis[0]
    }

    /// The last (deepest) MD of the axis.
    pub fn last(&self) -> f64 {
        self.axis[self.axis.len() - 1]
    }

    /// The mode of the stepper.
    pub fn mode(&self) -> StepMode {
        self.mode
    }

    /// The nodes.
    pub fn axis(&self) -> &[f64] {
        &self.axis
    }

    /// The node within `epsilon` of `md`, if any.
    fn node_within_epsilon(&self, md: f64) -> Option<usize> {
        let k = nearest_node(&self.axis, md)?;
        ((self.axis[k] - md).abs() <= self.epsilon).then_some(k)
    }

    /// Bounds `md` to the axis, then snaps it on a node within `epsilon`.
    fn settle(&self, md: f64, already_clamped: bool) -> Stepped {
        let (first, last) = (self.first(), self.last());
        let bounded = md.clamp(first, last);
        let clamped = already_clamped || bounded != md;
        match self.node_within_epsilon(bounded) {
            Some(k) => Stepped {
                md: self.axis[k],
                on_node: Some(k),
                clamped,
            },
            None => Stepped {
                md: bounded,
                on_node: None,
                clamped,
            },
        }
    }

    /// The MD brought onto a node (`Nodes`) or a grid point (`Fine`), bounded
    /// to the axis. An MD that is not a number gives the first node, clamped.
    pub fn snap(&self, md: f64) -> Stepped {
        if !md.is_finite() {
            return Stepped {
                md: self.first(),
                on_node: Some(0),
                clamped: true,
            };
        }
        let out_of_range = md < self.first() || md > self.last();
        match self.mode {
            StepMode::Nodes => {
                let bounded = md.clamp(self.first(), self.last());
                // The axis is not empty and `bounded` is finite.
                let k = nearest_node(&self.axis, bounded).unwrap_or(0);
                Stepped {
                    md: self.axis[k],
                    on_node: Some(k),
                    clamped: out_of_range,
                }
            }
            StepMode::Fine { step, anchor } => {
                let k = ((md - anchor) / step).round();
                self.settle(anchor + k * step, out_of_range)
            }
        }
    }

    /// The MD `notches` further (positive: deeper). In `Nodes` mode the first
    /// notch from between two nodes goes to the node strictly in that
    /// direction, not to the nearest one. Past an end it is bounded and says
    /// so (`clamped`).
    pub fn step(&self, md: f64, notches: i64) -> Stepped {
        if notches == 0 || !md.is_finite() {
            return self.snap(md);
        }
        let n = self.axis.len() as i64;
        match self.mode {
            StepMode::Nodes => {
                let target = match self.node_within_epsilon(md) {
                    Some(k) => (k as i64).saturating_add(notches),
                    None => {
                        let i = self.axis.partition_point(|x| *x < md) as i64;
                        if notches > 0 {
                            i.saturating_add(notches).saturating_sub(1)
                        } else {
                            i.saturating_add(notches)
                        }
                    }
                };
                let bounded = target.clamp(0, n - 1);
                Stepped {
                    md: self.axis[bounded as usize],
                    on_node: Some(bounded as usize),
                    clamped: bounded != target,
                }
            }
            StepMode::Fine { step, anchor } => {
                let x = (md - anchor) / step;
                let rounded = x.round();
                let k = if (x - rounded).abs() * step <= self.epsilon {
                    (rounded as i64).saturating_add(notches)
                } else {
                    let below = x.floor() as i64;
                    if notches > 0 {
                        below.saturating_add(notches)
                    } else {
                        below.saturating_add(1).saturating_add(notches)
                    }
                };
                self.settle(anchor + (k as f64) * step, false)
            }
        }
    }
}
