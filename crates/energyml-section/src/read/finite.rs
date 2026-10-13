//! **A number that is not a number never leaves the reader.**
//!
//! Building a section on a large real-world model failed with: *"the
//! contributions do not read: invalid type: null, expected f64 at line 1 column
//! 2496480"*. The package holds SIGEOL boreholes with no
//! elevation - their trajectories' control points are `NaN` - and intervals
//! whose top or base MD is `NaN`. `serde_json` writes a `NaN` as `null`, and the
//! catalogue, reading the contribution back, refused the whole of it: one well
//! without a datum took every section of every package down.
//!
//! The rule, at the one place a contribution is put together
//! (`contribution_cached`): nothing that is not finite is kept, and nothing is
//! dropped without saying so.
//!
//! - **a well** keeps the nodes it can place; a node with a coordinate or an MD
//!   that is not finite is dropped and counted. A well left with no finite head
//!   or fewer than two nodes cannot be drawn: it is **refused**, by name, with
//!   the reason - never drawn at a guessed depth;
//! - **an interval** whose top or base is not finite is not drawn (where would
//!   it start?); the well stays, and the count is said - in a note of code
//!   `notANumber`, never a refusal: the well is drawn, and a refused row beside
//!   its drawn row would say two things of one object;
//! - **a surface's cut** is broken where a point is not finite - a span on each
//!   side, never a segment through the hole - and a closed fill that holds one
//!   is dropped; each cut says how many points went;
//! - **an age** that is not finite is no age.

use crate::model::{CutShape, CutSource, Note, Refused, SurfaceRead, WellRead};

fn finite2(point: &[f64; 2]) -> bool {
    point.iter().all(|value| value.is_finite())
}

fn finite3(point: &[f64; 3]) -> bool {
    point.iter().all(|value| value.is_finite())
}

/// The code of the note that says what was dropped.
pub const NOT_A_NUMBER: &str = "notANumber";

/// A well with only what is finite, and a note saying what was dropped - or
/// why it cannot be drawn at all.
#[allow(clippy::result_large_err)]
pub fn finite_well(mut well: WellRead) -> Result<(WellRead, Option<Note>), Refused> {
    let name = if well.object.title.is_empty() {
        well.object.uuid.clone()
    } else {
        well.object.title.clone()
    };
    let refuse = |well: &WellRead, reason: String| Refused {
        object: well.object.clone(),
        reason,
    };
    if !finite3(&well.head) {
        return Err(refuse(
            &well,
            format!(
                "`{name}`: its head has a coordinate that is not a number (no elevation?), so it cannot be placed"
            ),
        ));
    }
    let total = well.path.len();
    let keep: Vec<bool> = (0..total)
        .map(|at| {
            finite3(&well.path[at])
                && well
                    .md
                    .as_ref()
                    .is_none_or(|md| md.get(at).is_none_or(|value| value.is_finite()))
        })
        .collect();
    let dropped_nodes = keep.iter().filter(|kept| !**kept).count();
    if dropped_nodes > 0 {
        let mut at = 0;
        well.path.retain(|_| {
            at += 1;
            keep[at - 1]
        });
        if let Some(md) = well.md.as_mut() {
            let mut at = 0;
            md.retain(|_| {
                at += 1;
                keep.get(at - 1).copied().unwrap_or(true)
            });
        }
    }
    if well.path.len() < 2 {
        return Err(refuse(
            &well,
            format!(
                "`{name}`: {dropped_nodes} of its {total} nodes have a coordinate or an MD that is not a number, too few are left to draw it"
            ),
        ));
    }
    let intervals = well.intervals.len();
    well.intervals
        .retain(|interval| interval.from.is_finite() && interval.to.is_finite());
    let dropped_intervals = intervals - well.intervals.len();

    let mut said = Vec::new();
    if dropped_nodes > 0 {
        said.push(format!(
            "{dropped_nodes} of its {total} nodes dropped: a coordinate or an MD is not a number"
        ));
    }
    if dropped_intervals > 0 {
        said.push(format!(
            "{dropped_intervals} of its {intervals} intervals not drawn: their top or base MD is not a number"
        ));
    }
    let note = (!said.is_empty()).then(|| Note {
        code: NOT_A_NUMBER.to_string(),
        sections: Vec::new(),
        objects: vec![well.object.clone()],
        said: format!("`{name}`: {}", said.join("; ")),
    });
    Ok((well, note))
}

/// Break a polyline where a point is not finite; keep the pieces of two points
/// or more. Returns how many points went.
fn split_spans(spans: &mut Vec<Vec<[f64; 2]>>) -> usize {
    split_spans_sourced(spans, &mut Vec::new())
}

/// [`split_spans`], and the sources of a trace cut kept **parallel** to the
/// spans it returns: a point that goes takes its source with it, and a piece
/// that is dropped (fewer than two points) takes its sources.
///
/// `sources` empty is "no sources", and stays so. Sources that are not parallel
/// to the spans (a list per span, an entry per point) cannot be carried: they
/// are cleared - the cut is then drawn flat - rather than left to name the
/// wrong points.
fn split_spans_sourced(spans: &mut Vec<Vec<[f64; 2]>>, sources: &mut Vec<Vec<CutSource>>) -> usize {
    let parallel = !sources.is_empty()
        && sources.len() == spans.len()
        && spans
            .iter()
            .zip(sources.iter())
            .all(|(span, list)| span.len() == list.len());
    let mut dropped = 0;
    let mut out = Vec::with_capacity(spans.len());
    let mut out_sources = Vec::new();
    let mut lists = std::mem::take(sources).into_iter();
    for span in spans.drain(..) {
        let list = if parallel {
            lists.next().unwrap_or_default()
        } else {
            Vec::new()
        };
        if span.iter().all(finite2) {
            out.push(span);
            if parallel {
                out_sources.push(list);
            }
            continue;
        }
        let mut piece = Vec::new();
        let mut piece_sources = Vec::new();
        let mut list = list.into_iter();
        for point in span {
            let source = if parallel { list.next() } else { None };
            if finite2(&point) {
                piece.push(point);
                piece_sources.extend(source);
            } else {
                dropped += 1;
                if piece.len() >= 2 {
                    out.push(std::mem::take(&mut piece));
                    out_sources.push(std::mem::take(&mut piece_sources));
                } else {
                    piece.clear();
                    piece_sources.clear();
                }
            }
        }
        if piece.len() >= 2 {
            out.push(piece);
            out_sources.push(piece_sources);
        }
    }
    *spans = out;
    if parallel {
        *sources = out_sources;
    }
    dropped
}

/// Drop a closed polygon that holds a point that is not finite: its outline
/// would go through the hole. Returns how many points went.
fn drop_rings(rings: &mut Vec<Vec<[f64; 2]>>) -> usize {
    let mut dropped = 0;
    rings.retain(|ring| {
        let bad = ring.iter().filter(|point| !finite2(point)).count();
        dropped += bad;
        bad == 0
    });
    dropped
}

/// A surface with only what is finite in its cuts, each cut saying what went.
pub fn finite_surface(surface: &mut SurfaceRead) {
    if surface.age.is_some_and(|age| !finite2(&age)) {
        surface.age = None;
    }
    for cut in &mut surface.cuts {
        let dropped = match &mut cut.shape {
            CutShape::Trace { spans } => split_spans_sourced(spans, &mut cut.sources),
            CutShape::Envelope {
                top,
                bottom,
                fills,
                faults,
            } => split_spans(top) + split_spans(bottom) + split_spans(faults) + drop_rings(fills),
        };
        if dropped > 0 {
            cut.notes.push(format!(
                "{dropped} point(s) of the cut are not numbers: the cut is broken there"
            ));
        }
    }
}
