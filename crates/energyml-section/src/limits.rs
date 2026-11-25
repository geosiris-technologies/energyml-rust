//! The limits of a well, from `energyml_domain::limits` to what a section draws.
//!
//! The classes, the runs, the rank of each limit and its place in MD are decided in the domain
//! (ADR-0051); this crate draws. [`well_limits`] turns the limits of one end of the runs (their tops,
//! or their bases) into the [`WellLimit`]s a [`crate::model::View`] carries: where (an MD), which one
//! of its class (its rank), between what and what (the names of the class above and below). A limit
//! the domain cannot place - an interpolation over a hole - is **left out and said**, never drawn
//! where it is not.

use energyml_domain::limits::{Anchor, LimitKind, Transitions};

use crate::model::{LimitAt, WellItem, WellLimit};

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

/// The lines between the strata units a well shows, as limits: the places where one unit gives way to the
/// next along `well`, which a view lays over the well's colours **whatever colours it** (a property
/// painted along it as much as its units).
///
/// The units are the well's own intervals ([`WellItem::intervals`]): one class per interval, the unit's
/// full uri when it has one and its title else, so two units that share a title are two classes. The
/// runs and the limits are [`energyml_domain::limits`]'s (ADR-0051): a unit that comes back is two runs, and
/// no limit crosses a hole between two intervals. The limits are the **tops** (the base of the unit
/// above is the same place), each named by the titles of the unit above and below. The second value is
/// what could not be placed, one sentence each.
pub fn strata_limits(well: &WellItem) -> (Vec<WellLimit>, Vec<String>) {
    use energyml_domain::frame_values::Support;
    use energyml_domain::limits::{Class, Classification, transitions};

    if well.intervals.len() < 2 {
        return (Vec::new(), Vec::new());
    }
    let identity = |at: usize| {
        let interval = &well.intervals[at];
        interval.unit_uri.clone().unwrap_or_else(|| interval.unit.clone())
    };
    // A stretch of the well that no interval covers is a **hole**: an interval of no class between the two, so that no
    // limit crosses it (the domain places a limit between two classes that follow each other, and the
    // numbers alone do not say that two intervals do not touch).
    let mut top = Vec::new();
    let mut base = Vec::new();
    let mut classes = Vec::new();
    for (at, interval) in well.intervals.iter().enumerate() {
        if let Some(&before) = base.last()
            && interval.from > before
        {
            top.push(before);
            base.push(interval.from);
            classes.push(None);
        }
        top.push(interval.from);
        base.push(interval.to);
        classes.push(Some(Class::Unit(identity(at))));
    }
    let support = Support::Intervals { top, base };
    let classification = match Classification::new(support, classes) {
        Ok(classification) => classification,
        Err(why) => return (Vec::new(), vec![why.to_string()]),
    };
    let found = transitions(&classification);
    let (mut placed, refused) = well_limits(&found, LimitKind::Top, Anchor::Inside);
    // A unit is named by its title, not by its identity.
    let title_of = |class: &Class| match class {
        Class::Unit(identity_text) => well
            .intervals
            .iter()
            .enumerate()
            .find(|(at, _)| &identity(*at) == identity_text)
            .map(|(_, interval)| interval.unit.clone()),
        _ => None,
    };
    let tops: Vec<_> = found
        .limits
        .iter()
        .filter(|limit| limit.kind == LimitKind::Top)
        .collect();
    let mut named = tops
        .iter()
        .filter(|limit| found.place(limit, Anchor::Inside).is_ok())
        .map(|limit| {
            let boundary = &found.boundaries[limit.boundary];
            (
                title_of(&found.runs[boundary.above].class),
                title_of(&found.runs[boundary.below].class),
            )
        });
    for limit in &mut placed {
        if let Some((above, below)) = named.next() {
            limit.above = above;
            limit.below = below;
        }
    }
    (placed, refused)
}
