//! Erosions suspected from the surfaces' ages (ADR-0015):
//! detect erosions from the ages of the surfaces, propose local corrections or
//! suggestions of corrections when something is wrong, and colour the portions
//! identified as erosions).
//!
//! # The rule
//!
//! On one section, take two **horizons** that both state an age
//! ([`TraceItem::age`], an interval `[old, young]` in Ma). `A` is older than `B`
//! only when the two intervals are **disjoint** — `A.young > B.old`; an
//! overlap, or two equal ages, orders nothing and is not judged. An older
//! horizon lies **above** a younger one nowhere, so wherever, at the same `s`,
//! the younger one is below the older one (`z` up: `z_young < z_old`), the
//! order of the surfaces contradicts the order of the ages. Two readings fit:
//! the younger surface truncates the older one (an erosion: the older one should
//! stop against it), or one of the two ages is wrong. Which one is the
//! geologist's to say, so the module reports the **portion** `[s0, s1]` and
//! proposes the first reading in words. **A proposal is never applied.**
//!
//! What is not judged: a horizon without an age (the rule — most files date
//! nothing, and nothing is inferred from a name or a depth), a fault (it does
//! not stack), a span where only one of the two lines exists (a surface that
//! stops is not a surface that crosses), and a grid's envelope.
//!
//! Pure: [`detect`] takes traces, [`of_section`] a section, [`notes`] a
//! catalogue; nothing is read.

use crate::model::{Catalogue, Item, Note, ObjectId, Section, SectionKey, TraceItem, TraceRole};

/// The note's code ([`Note::code`]).
pub const CODE: &str = "erosion";

/// How far below, in the section's units, counts as below: float noise is not
/// an erosion.
const Z_TOLERANCE: f64 = 1e-6;
/// A portion shorter than this, along `s`, is a crossing point, not a portion.
const MIN_LENGTH: f64 = 1e-6;

/// A stretch of a section where a younger horizon passes below an older one.
#[derive(Clone, Debug, PartialEq)]
pub struct Portion {
    /// On which section.
    pub section: SectionKey,
    /// The horizon the ages say is older — the one that should stop.
    pub older: ObjectId,
    /// Its age, `[old, young]` in Ma.
    pub older_age: [f64; 2],
    /// The horizon the ages say is younger — the one that truncates.
    pub younger: ObjectId,
    /// Its age, `[old, young]` in Ma.
    pub younger_age: [f64; 2],
    /// Where it starts along the section.
    pub s0: f64,
    /// Where it ends.
    pub s1: f64,
}

impl Portion {
    /// The correction proposed, in words — and only in words.
    pub fn proposal(&self) -> String {
        format!(
            "`{older}` should stop against `{younger}` between s = {s0} and s = {s1} (an erosion: the younger surface \
             truncates the older one), or one of the two ages is inverted; nothing was changed",
            older = self.older.title,
            younger = self.younger.title,
            s0 = round1(self.s0),
            s1 = round1(self.s1),
        )
    }

    /// The sentence of the note.
    pub fn said(&self) -> String {
        format!(
            "erosion suspected on {section}: `{younger}` ({ya}) passes below `{older}` ({oa}) between s = {s0} and s = {s1}, \
             though it is the younger — proposal: {proposal}",
            section = section_name(self.section),
            younger = self.younger.title,
            ya = age_text(self.younger_age),
            older = self.older.title,
            oa = age_text(self.older_age),
            s0 = round1(self.s0),
            s1 = round1(self.s1),
            proposal = self.proposal(),
        )
    }
}

/// The portions of two or more dated traces, by pair of horizons: for each
/// ordered pair (the older first in the item order), each stretch where the
/// younger passes below the older, left to right. Traces with no age, faults,
/// and pairs the ages do not order give nothing.
pub fn detect(section: SectionKey, traces: &[&TraceItem]) -> Vec<Portion> {
    let dated: Vec<(&TraceItem, [f64; 2])> = traces
        .iter()
        .filter(|t| t.role == TraceRole::Horizon)
        .filter_map(|t| t.age.map(|age| (*t, age)))
        .collect();
    let mut out = Vec::new();
    for (i, (a, a_age)) in dated.iter().enumerate() {
        for (b, b_age) in dated.iter().skip(i + 1) {
            // The older first: disjoint intervals only.
            let (older, o_age, younger, y_age) = if a_age[1] > b_age[0] {
                (*a, *a_age, *b, *b_age)
            } else if b_age[1] > a_age[0] {
                (*b, *b_age, *a, *a_age)
            } else {
                continue;
            };
            for (s0, s1) in below(&older.spans, &younger.spans) {
                out.push(Portion {
                    section,
                    older: older.object.clone(),
                    older_age: o_age,
                    younger: younger.object.clone(),
                    younger_age: y_age,
                    s0,
                    s1,
                });
            }
        }
    }
    out
}

/// [`detect`] over what a section draws.
pub fn of_section(section: &Section) -> Vec<Portion> {
    let traces: Vec<&TraceItem> = section
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Trace(t) => Some(t),
            _ => None,
        })
        .collect();
    detect(section.key, &traces)
}

/// One note per portion, over the sections given (the long one first).
pub fn notes_of<'a>(sections: impl IntoIterator<Item = &'a Section>) -> Vec<Note> {
    sections
        .into_iter()
        .flat_map(of_section)
        .map(|portion| Note {
            code: CODE.to_string(),
            sections: vec![portion.section],
            objects: vec![portion.older.clone(), portion.younger.clone()],
            said: portion.said(),
        })
        .collect()
}

/// [`notes_of`] over a finished catalogue.
pub fn notes(catalogue: &Catalogue) -> Vec<Note> {
    notes_of(std::iter::once(&catalogue.long).chain(&catalogue.crosses))
}

/// The `z` of a polyline set at `s`, when one of its spans covers `s`.
fn z_at(spans: &[Vec<[f64; 2]>], s: f64) -> Option<f64> {
    spans.iter().find_map(|span| {
        let (first, last) = (span.first()?, span.last()?);
        if s < first[0] || s > last[0] {
            return None;
        }
        span.windows(2).find(|w| s >= w[0][0] && s <= w[1][0]).map(|w| {
            let run = w[1][0] - w[0][0];
            if run <= 0.0 {
                w[0][1]
            } else {
                w[0][1] + (w[1][1] - w[0][1]) * (s - w[0][0]) / run
            }
        })
    })
}

/// The stretches of `s` where `lower` is below `upper` — both exist there, and
/// `z_lower < z_upper` by more than the tolerance. Between two breakpoints (a
/// vertex of either) the difference is linear, so its sign change is exact.
fn below(upper: &[Vec<[f64; 2]>], lower: &[Vec<[f64; 2]>]) -> Vec<(f64, f64)> {
    let mut at: Vec<f64> = upper
        .iter()
        .chain(lower)
        .flatten()
        .map(|p| p[0])
        .filter(|s| s.is_finite())
        .collect();
    at.sort_by(f64::total_cmp);
    at.dedup();
    let mut found: Vec<(f64, f64)> = Vec::new();
    for pair in at.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let mid = 0.5 * (a + b);
        // Both lines must run across the whole segment: probe the middle too,
        // so a gap between two spans is not bridged.
        let (Some(ua), Some(ub), Some(la), Some(lb), Some(_), Some(_)) = (
            z_at(upper, a),
            z_at(upper, b),
            z_at(lower, a),
            z_at(lower, b),
            z_at(upper, mid),
            z_at(lower, mid),
        ) else {
            continue;
        };
        let (fa, fb) = (la - ua, lb - ub);
        let neg = |f: f64| f < -Z_TOLERANCE;
        let part = match (neg(fa), neg(fb)) {
            (true, true) => Some((a, b)),
            (true, false) => Some((a, a + (b - a) * fa / (fa - fb))),
            (false, true) => Some((a + (b - a) * fa / (fa - fb), b)),
            (false, false) => None,
        };
        if let Some((s0, s1)) = part.filter(|(s0, s1)| s1 - s0 > MIN_LENGTH) {
            match found.last_mut() {
                Some(last) if (s0 - last.1).abs() <= MIN_LENGTH => last.1 = s1,
                _ => found.push((s0, s1)),
            }
        }
    }
    found
}

fn section_name(key: SectionKey) -> String {
    match key {
        SectionKey::Long => "the long section".to_string(),
        SectionKey::Cross { index } => format!("cross section {index}"),
    }
}

/// `10 Ma`, or `10–5 Ma` for a span.
fn age_text(age: [f64; 2]) -> String {
    if (age[0] - age[1]).abs() < 1e-9 {
        format!("{} Ma", trim(age[0]))
    } else {
        format!("{}–{} Ma", trim(age[0]), trim(age[1]))
    }
}

fn trim(v: f64) -> String {
    let text = format!("{v:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn round1(v: f64) -> String {
    format!("{v:.1}")
}
