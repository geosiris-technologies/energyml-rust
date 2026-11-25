//! The limits of a well, from `energyml_domain::limits` to what a section draws.
//!
//! The classes, the runs, the rank of each limit and its place in MD are decided in the domain
//! (ADR-0051); this crate draws. [`well_limits`] turns the limits of one end of the runs (their tops,
//! or their bases) into the [`WellLimit`]s a [`crate::model::View`] carries: where (an MD), which one
//! of its class (its rank), between what and what (the names of the class above and below). A limit
//! the domain cannot place - an interpolation over a hole - is **left out and said**, never drawn
//! where it is not.

use energyml_domain::limits::{Anchor, LimitKind, Transitions};

use crate::model::{LimitAt, WellLimit};

/// The limits of `transitions` that are `kind` (every top, or every base), placed with `anchor`, in
/// depth order.
///
/// `above` and `below` are the names of the class of the run above and of the run below the
/// boundary (`Class`'s own words: `code 2`, `at or above the threshold`...): a face that knows
/// better names - a unit's title - replaces them. The second value is what could not be placed, one
/// sentence each.
pub fn well_limits(
    transitions: &Transitions,
    kind: LimitKind,
    anchor: Anchor,
) -> (Vec<WellLimit>, Vec<String>) {
    let mut placed = Vec::new();
    let mut refused = Vec::new();
    for limit in transitions.limits.iter().filter(|limit| limit.kind == kind) {
        let boundary = &transitions.boundaries[limit.boundary];
        let name = |run: usize| Some(transitions.runs[run].class.to_string());
        match transitions.place(limit, anchor) {
            Ok(at) => placed.push(WellLimit {
                at: LimitAt::Md(at.md),
                rank: limit.rank,
                above: name(boundary.above),
                below: name(boundary.below),
            }),
            Err(why) => refused.push(format!(
                "the limit between {} and {} was not placed: {why}",
                transitions.runs[boundary.above].class, transitions.runs[boundary.below].class
            )),
        }
    }
    (placed, refused)
}
