//! The value of a property at a measured depth.
//!
//! The same arithmetic as everywhere along a well ([`crate::along_md`]): where
//! an MD falls ([`bracket`]), what a value is between two nodes
//! ([`interpolate_row`]), and the rule of the hole ([`GapRule`]) - **a value is
//! never bridged over an absent neighbour or over a gap wider than the rule
//! allows**. This module adds the part that is about a frame's values:
//!
//! | support | an MD on a node | an MD between two nodes |
//! |---|---|---|
//! | nodes, continuous | the node's value | interpolated (`linear` unless the caller names another method), refused over a hole |
//! | nodes, discrete or categorical | the node's value | **refused**: a code is held at its node, never interpolated |
//! | intervals | the interval on the side asked ([`Side`]) | the interval that holds the MD |
//!
//! An MD is compared with the nodes only after the caller has brought it to the
//! frame's unit and datum ([`crate::strata_by_md::comparable`]): this module
//! receives MDs **already brought** there, like [`crate::along_md`].

use crate::along_md::{
    BracketRefusal, GapRule, InterpNote, InterpRefusal, Interpolations, LINEAR, Nature, SNAP_FRACTION,
    StepRefusal, bracket, interpolate_row, median_step,
};

use super::{FrameValues, Support, Value};

/// Which interval answers for an MD that falls exactly on the node between two.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Side {
    /// The interval that **ends** at the node: the one above it.
    Above,
    /// The interval that **opens** at the node: the one below it, which is also
    /// the rule of a node of a frame ([`crate::well_strata`]); the deepest node
    /// belongs to the last interval.
    #[default]
    Below,
}

/// Where a value was taken from.
#[derive(Clone, Debug, PartialEq)]
pub enum Placement {
    /// At this node.
    Node(usize),
    /// Between these two nodes, at this fraction of the way (`0 < t < 1`).
    Between {
        /// The node above.
        lower: usize,
        /// The node below.
        upper: usize,
        /// The fraction of the way from `lower` to `upper`.
        t: f64,
    },
    /// In this interval.
    Interval(usize),
}

/// The value of a property at an MD.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    /// The value. `None` is the file's absence at that node or in that interval.
    pub value: Option<Value>,
    /// Where it was taken from.
    pub placement: Placement,
    /// What the interpolation method says of its result, when it says something.
    pub note: Option<InterpNote>,
}

/// Why a property has no value at an MD.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum AtMdRefusal {
    /// The MD does not fall on the support.
    #[error("{0}")]
    Placement(BracketRefusal),
    /// The MD is the top of the frame and the side asked is above it.
    #[error("the MD {md} is the top of the frame: no interval lies above it")]
    NothingAbove {
        /// The MD asked for.
        md: f64,
    },
    /// A code is held at its node, not between two.
    #[error(
        "the MD falls between nodes {lower} and {upper}, and the values are discrete: a code is held at its node and never interpolated"
    )]
    DiscreteBetweenNodes {
        /// The node above.
        lower: usize,
        /// The node below.
        upper: usize,
    },
    /// A node beside the MD has no value.
    #[error(
        "the MD falls between nodes {lower} and {upper} and node {missing} has no value: the curve is cut there, and is not bridged"
    )]
    MissingNeighbour {
        /// The node above.
        lower: usize,
        /// The node below.
        upper: usize,
        /// The one that has no value.
        missing: usize,
    },
    /// The nodes around the MD are further apart than the gap rule allows.
    #[error(
        "the MD falls between nodes {lower} and {upper}, {gap} apart, wider than the {max} that may be bridged"
    )]
    GapTooWide {
        /// The node above.
        lower: usize,
        /// The node below.
        upper: usize,
        /// The distance between the nodes.
        gap: f64,
        /// The widest interval that may be bridged.
        max: f64,
    },
    /// The spacing of the nodes cannot be judged.
    #[error("the nodes give no spacing to judge a gap by: {0}")]
    Axis(StepRefusal),
    /// No interpolation method of that id.
    #[error("there is no interpolation method `{id}`")]
    UnknownMethod {
        /// The id asked for.
        id: String,
    },
    /// The method refuses these values.
    #[error("{0}")]
    Method(InterpRefusal),
    /// A value beside the MD is not a number.
    #[error("the values are not numbers, so there is nothing to interpolate")]
    NotNumbers,
}

impl FrameValues {
    /// The value at a measured depth `md`, in the unit and datum of the frame.
    ///
    /// `side` says which interval answers on a node between two intervals (it is
    /// not read for nodes); `method` is the id of an interpolation of
    /// [`Interpolations::builtin`] (`"linear"`, `"log_linear"`, ...) for the
    /// continuous values between two nodes.
    pub fn at_md(&self, md: f64, side: Side, method: &str) -> Result<Sample, AtMdRefusal> {
        match &self.support {
            Support::Nodes { md: nodes } => self.at_md_on_nodes(nodes, md, method),
            Support::Intervals { .. } => self.at_md_in_intervals(md, side),
        }
    }

    /// The value held by node or interval `at`, `None` when absent or past the end.
    fn held(&self, at: usize) -> Option<&Value> {
        self.values.get(at).and_then(Option::as_ref)
    }

    fn at_md_on_nodes(&self, nodes: &[f64], md: f64, method: &str) -> Result<Sample, AtMdRefusal> {
        let found = bracket(nodes, md, snap(nodes)).map_err(AtMdRefusal::Placement)?;
        if let Some(node) = found.on_node() {
            return Ok(Sample {
                value: self.held(node).cloned(),
                placement: Placement::Node(node),
                note: None,
            });
        }
        let (lower, upper) = (found.lower, found.upper);
        if !self.continuous {
            return Err(AtMdRefusal::DiscreteBetweenNodes { lower, upper });
        }
        let number = |at: usize| match self.held(at) {
            None => Ok(None),
            Some(Value::Number(number)) => Ok(Some(*number)),
            Some(_) => Err(AtMdRefusal::NotNumbers),
        };
        let (a, b) = (number(lower)?, number(upper)?);
        let (Some(a), Some(b)) = (a, b) else {
            return Err(AtMdRefusal::MissingNeighbour {
                lower,
                upper,
                missing: if a.is_none() { lower } else { upper },
            });
        };
        let rule = GapRule::of_axis(nodes).map_err(AtMdRefusal::Axis)?;
        let methods = Interpolations::builtin();
        let method = if method.is_empty() { LINEAR } else { method };
        let Some(interpolator) = methods.get(method) else {
            return Err(AtMdRefusal::UnknownMethod {
                id: method.to_string(),
            });
        };
        let gap = nodes[upper] - nodes[lower];
        let row = interpolate_row(&[a], &[b], found.t, gap, &rule, interpolator, &Nature::default())
            .map_err(AtMdRefusal::Method)?;
        if let Some(reason) = row.first_refusal {
            return Err(match reason {
                InterpRefusal::GapTooWide { gap, max } => AtMdRefusal::GapTooWide {
                    lower,
                    upper,
                    gap,
                    max,
                },
                other => AtMdRefusal::Method(other),
            });
        }
        match row.values.first().copied().filter(|value| value.is_finite()) {
            Some(value) => Ok(Sample {
                value: Some(Value::Number(value)),
                placement: Placement::Between {
                    lower,
                    upper,
                    t: found.t,
                },
                note: row.note,
            }),
            None => Err(AtMdRefusal::MissingNeighbour {
                lower,
                upper,
                missing: lower,
            }),
        }
    }

    fn at_md_in_intervals(&self, md: f64, side: Side) -> Result<Sample, AtMdRefusal> {
        // The nodes of the frame the intervals are cut from: each interval ends
        // where the next one begins.
        let nodes = self.support.node_md();
        let found = bracket(&nodes, md, snap(&nodes)).map_err(AtMdRefusal::Placement)?;
        let count = self.support.len();
        let interval = match found.on_node() {
            Some(node) => match side {
                // The deepest node closes the last interval.
                Side::Below => node.min(count.saturating_sub(1)),
                Side::Above => node.checked_sub(1).ok_or(AtMdRefusal::NothingAbove { md })?,
            },
            None => found.lower,
        };
        Ok(Sample {
            value: self.held(interval).cloned(),
            placement: Placement::Interval(interval),
            note: None,
        })
    }
}

/// An MD within a millionth of a step of a node is that node
/// ([`SNAP_FRACTION`]); a support of one node has no step, and no tolerance.
fn snap(nodes: &[f64]) -> f64 {
    median_step(nodes).map(|step| step * SNAP_FRACTION).unwrap_or(0.0)
}
