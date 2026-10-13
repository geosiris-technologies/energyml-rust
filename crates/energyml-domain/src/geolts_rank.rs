//! A division of the BRGM chart read **against a unit** - inside it, outside
//! it, or neither - and **at a level**: the containment the register states,
//! and the ages it gives.
//!
//! Asked for the synthetic reference model: grouping point clouds by title would
//! mix internal limits (Upper Lutetian / Lutetian) into H3; what is needed is
//! contacts **at one rank** of the column. The base of the Lutetian is
//! the base of a pass named "Lutétien", "Lutétien moyen" or "Lutétien
//! inférieur" depending on the borehole; the limit between "Lutétien
//! supérieur" and "Lutétien" is no limit of the stage. So a pass is read
//! against the unit through the chart, never by its name.
//!
//! # Three answers, and the one that is not an answer
//!
//! - **inside** - it is the unit, or the register places it under the unit:
//!   its `skos:broader`, followed all the way up. **Every** `broader`, not only
//!   the tree's parent ([`Chart::parent`], the deepest level above): the
//!   Lutetian names both the Eocene and the Middle Eocene, and it is under both;
//! - **outside** - its time and the unit's do not overlap. Decided by the
//!   **ages**, never by the tree: the Ludien sits *beside* the Priabonien in the
//!   tree, under the Upper Eocene, and spans the same 37.8-33.9 Ma - a regional
//!   synonym, not a neighbour, and a tree-only reading would draw a contact
//!   between the two. Two spans that share a bound **touch**, and are outside
//!   each other: the Cuisien ends at 47.8 Ma, where the Lutetian begins;
//! - **neither**, said by its reason - a coarser division that **holds** the
//!   unit (the Middle Eocene for the Lutetian: part of it may be the unit), one
//!   whose time **overlaps** the unit's without the register placing it inside,
//!   one whose ages cannot be read (**undated**).
//!
//! A division without both ages takes the span of the nearest division above
//! it that has them - a division lies within what holds it. The snapshot of
//! 24/09 needs it: the Upper Lutetian ends at a bound named `e1258`, which is
//! no number and gives no age (`geolts_csv::bounds` skips it), so its span is
//! read as the Lutetian's. Wider than its own, never narrower: *outside* stays
//! sound, and only an *overlaps* can come of the widening.
//!
//! # At a level
//!
//! [`at_level`] is the division of a level a division is read as: itself when
//! it is of that level, else the one of that level above it - **the coarsest**
//! when two are: the Cuisien, a regional stage the register places under the
//! Ypresian, is the Ypresian at the stage level. Nothing when no division of
//! that level holds it: the Eocene has no stage.

use crate::geolts::Chart;

/// Where a division of the chart lies with respect to a unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Relation {
    /// It is the unit, or the register places it under the unit.
    Inside,
    /// Its time and the unit's do not overlap - it is older or younger.
    Outside,
    /// A coarser division that holds the unit: part of it may be the unit.
    Holds,
    /// Their times overlap, and the register places neither under the other.
    Overlaps,
    /// Not under the unit, and the ages that would tell cannot be read.
    Undated,
}

impl Relation {
    /// Whether the chart answers - inside or outside. A limit is a contact of
    /// the unit only between an interval inside it and one outside it.
    pub fn is_known(self) -> bool {
        matches!(self, Relation::Inside | Relation::Outside)
    }

    /// Why no answer, in a few words for a note; empty for an answer.
    pub fn reason(self) -> &'static str {
        match self {
            Relation::Inside | Relation::Outside => "",
            Relation::Holds => "a coarser division that holds it",
            Relation::Overlaps => "overlaps it in time, and the chart does not place it inside",
            Relation::Undated => "the chart gives no age to compare",
        }
    }
}

/// Every division above one, by `broader` followed all the way up - nearest
/// generation first, each once, the division itself never. A `broader` the
/// chart does not hold is left out: nothing above it can be read.
pub fn above(chart: &Chart, id: u32) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    let mut next: Vec<u32> = chart.get(id).map(|one| one.broader.clone()).unwrap_or_default();
    while !next.is_empty() {
        let mut after: Vec<u32> = Vec::new();
        for one in next {
            if one == id || out.contains(&one) {
                continue;
            }
            let Some(concept) = chart.get(one) else { continue };
            out.push(one);
            after.extend(concept.broader.iter().copied());
        }
        next = after;
    }
    out
}

/// The span of a division, `(old, young)` in Ma - its own when it states both
/// ends, else the nearest dated division above it (see the module). `None`
/// when nothing up its chain is dated.
pub fn span(chart: &Chart, id: u32) -> Option<(f64, f64)> {
    std::iter::once(id).chain(above(chart, id)).find_map(|one| {
        let concept = chart.get(one)?;
        Some((concept.begins_ma?, concept.ends_ma?))
    })
}

/// Where `division` lies with respect to `unit`. See the module.
pub fn relation(chart: &Chart, division: u32, unit: u32) -> Relation {
    if division == unit || above(chart, division).contains(&unit) {
        return Relation::Inside;
    }
    if above(chart, unit).contains(&division) {
        return Relation::Holds;
    }
    match (span(chart, division), span(chart, unit)) {
        // Older than the unit's old end, or younger than its young end - a
        // shared bound is a touch, not an overlap.
        (Some((old, young)), Some((unit_old, unit_young))) if young >= unit_old || old <= unit_young => {
            Relation::Outside
        }
        (Some(_), Some(_)) => Relation::Overlaps,
        _ => Relation::Undated,
    }
}

/// The divisions of `level` a division is read as at that level: itself when
/// it is of that level and nothing of that level is above it, else the
/// **coarsest** of that level above it. Empty when nothing of that level holds
/// it; more than one when two of that level hold it and neither holds the
/// other - which a caller refuses to choose between.
pub fn at_level(chart: &Chart, id: u32, level: u8) -> Vec<u32> {
    let candidates: Vec<u32> = std::iter::once(id)
        .chain(above(chart, id))
        .filter(|one| chart.get(*one).is_some_and(|concept| concept.level == level))
        .collect();
    candidates
        .iter()
        .copied()
        .filter(|one| {
            !candidates
                .iter()
                .any(|other| other != one && above(chart, *one).contains(other))
        })
        .collect()
}

/// The levels the chart has divisions of, coarsest first - `0`, no level, left
/// out.
pub fn levels(chart: &Chart) -> Vec<u8> {
    let mut out: Vec<u8> = chart
        .concepts()
        .iter()
        .map(|one| one.level)
        .filter(|level| *level > 0)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}
