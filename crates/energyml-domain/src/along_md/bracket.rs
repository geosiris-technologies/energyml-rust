//! Where an MD falls between the nodes of a support.

use super::stepper::StepRefusal;

/// Where an MD falls: `lower == upper` and `t == 0` on a node.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bracket {
    /// The index of the node above (shallower than) the MD.
    pub lower: usize,
    /// The index of the node below (deeper than) the MD.
    pub upper: usize,
    /// The fraction of the way from `lower` to `upper`, in `[0, 1]`.
    pub t: f64,
}

impl Bracket {
    /// The node, when the MD falls on one (`lower == upper`).
    pub fn on_node(&self) -> Option<usize> {
        (self.lower == self.upper).then_some(self.lower)
    }
}

/// Why an MD cannot be placed on a support.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum BracketRefusal {
    /// The support has no node.
    #[error("a support of no node")]
    Empty,
    /// The MD asked for is not a number.
    #[error("the MD {md} is not a number")]
    NotFinite {
        /// The MD asked for.
        md: f64,
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
    /// The MD is beyond the ends of the support, more than the tolerance.
    #[error("the MD {md} is outside the support, which runs from {from} to {to}")]
    Outside {
        /// The MD asked for.
        md: f64,
        /// The first MD of the support.
        from: f64,
        /// The last MD of the support.
        to: f64,
    },
    /// The tolerance is negative or not a number.
    #[error("a tolerance of {tolerance} is not a finite non-negative length")]
    BadTolerance {
        /// The tolerance given.
        tolerance: f64,
    },
}

/// Where `md` falls among the `mds` of a support (strictly increasing, in the
/// unit and datum of `md`). Within `tolerance` of a node, it is that node. The
/// validation of the support is O(n) at each call: a choice of simplicity.
pub fn bracket(mds: &[f64], md: f64, tolerance: f64) -> Result<Bracket, BracketRefusal> {
    if !tolerance.is_finite() || tolerance < 0.0 {
        return Err(BracketRefusal::BadTolerance { tolerance });
    }
    if mds.is_empty() {
        return Err(BracketRefusal::Empty);
    }
    if !md.is_finite() {
        return Err(BracketRefusal::NotFinite { md });
    }
    for (at, node) in mds.iter().enumerate() {
        if !node.is_finite() {
            return Err(BracketRefusal::NodeNotFinite { at });
        }
        if at > 0 && *node <= mds[at - 1] {
            return Err(BracketRefusal::NotIncreasing { at });
        }
    }
    let from = mds[0];
    let to = mds[mds.len() - 1];
    if md < from - tolerance || md > to + tolerance {
        return Err(BracketRefusal::Outside { md, from, to });
    }
    let i = mds.partition_point(|x| *x < md);
    // The candidates for "on a node": the one before and the one at `i`; the
    // closer wins, the shallower on a tie (the one before is tried first).
    let mut best: Option<(usize, f64)> = None;
    for j in [i.checked_sub(1), (i < mds.len()).then_some(i)]
        .into_iter()
        .flatten()
    {
        let d = (mds[j] - md).abs();
        if d <= tolerance && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((j, d));
        }
    }
    if let Some((j, _)) = best {
        return Ok(Bracket {
            lower: j,
            upper: j,
            t: 0.0,
        });
    }
    if i == 0 || i >= mds.len() {
        // Beyond an end and not within the tolerance of its node (a single
        // node lands here too): only "on the node" was possible.
        return Err(BracketRefusal::Outside { md, from, to });
    }
    Ok(Bracket {
        lower: i - 1,
        upper: i,
        t: (md - mds[i - 1]) / (mds[i] - mds[i - 1]),
    })
}

/// The index of the node closest to `md`; on a tie, the shallower (the
/// smaller MD), then the smaller index. The order of the nodes does not
/// matter. `None` when `md` is not finite or no node is.
pub fn nearest_node(mds: &[f64], md: f64) -> Option<usize> {
    if !md.is_finite() {
        return None;
    }
    let mut best: Option<(usize, f64)> = None;
    for (i, node) in mds.iter().enumerate() {
        if !node.is_finite() {
            continue;
        }
        let d = (node - md).abs();
        let better = match best {
            None => true,
            Some((j, bd)) => d < bd || (d == bd && *node < mds[j]),
        };
        if better {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

/// The common axis of several supports, already in one unit and datum: the
/// finite MDs of all of them, sorted, the nodes within `tolerance` of the
/// first of a cluster merged into it (the shallowest is kept). The distance is
/// measured from the first of the cluster, so a chain of close nodes does not
/// drift. Values that are not finite are left out.
pub fn union_axis(axes: &[&[f64]], tolerance: f64) -> Result<Vec<f64>, StepRefusal> {
    if !tolerance.is_finite() || tolerance < 0.0 {
        return Err(StepRefusal::BadTolerance { tolerance });
    }
    let mut all: Vec<f64> = axes
        .iter()
        .flat_map(|a| a.iter().copied())
        .filter(|x| x.is_finite())
        .collect();
    all.sort_by(f64::total_cmp);
    let mut out: Vec<f64> = Vec::with_capacity(all.len());
    for x in all {
        match out.last() {
            Some(first) if x - first <= tolerance => {}
            _ => out.push(x),
        }
    }
    Ok(out)
}
