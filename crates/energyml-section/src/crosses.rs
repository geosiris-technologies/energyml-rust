//! The traces of every section: the long one, and its cross sections
//! (ADR-0015; `fence::crosslines` generalised).
//!
//! Contract: [`traces`].
//!
//! # What replaces `fence::crosslines`
//!
//! Two decisions of the old function change, and both are on purpose:
//!
//! - **The direction at a vertex is the bisector**, the line along the mean
//!   direction of the two segments, where `crosslines` took the *tangent of the
//!   incoming segment*. The wells are projected on
//!   the bisector at a vertex (`Corridor::project`),
//!   so a cross section standing on a vertex has to be perpendicular to the
//!   direction a well there is projected along, or the two would disagree about
//!   what a "perpendicular" section is. Away from a vertex both are the
//!   segment's direction.
//! - **Two cross sections that cross are noted, never refused**
//!   (ADR-0015): a `crossesCross` [`Note`] names the two,
//!   and the sections are computed all the same. The old refusal was a closed
//!   door on a drawing the user may well want.
//!
//! What stays: the abscissas of [`Crosses::Every`] are the multiples of the
//! spacing **plus the end of the trace**, the last multiple dropped when it is
//! within half a step of the end; every **pair** is checked for crossing, not
//! only the neighbours (a trace that folds back makes distant cross sections
//! run through each other with tame neighbours).

use crate::inputs::{InputsError, MAX_CROSSES, validate};
use crate::model::{
    CrossOwn, CrossWidths, Crosses, Inputs, Note, SectionKey, SectionTrace,
};

type V = [f64; 2];

/// The traces of the long section and of its cross sections, in that order
/// (the cross sections by increasing abscissa), and what computing them
/// noticed (cross sections running through each other are **noted**, never
/// refused).
///
/// A cross section runs from its `before` end, on the **left** of the trace
/// (read along it), to its `after` end, on the right; `left` and `right` of its
/// [`SectionTrace`] are its corridor, read along its own run.
///
/// [`Crosses::Count`] spreads `count` cross sections evenly, **ends included**
/// (a single one stands in the middle), so that `Count` and `Every` both start
/// the trace and finish it.
pub fn traces(inputs: &Inputs) -> Result<(Vec<SectionTrace>, Vec<Note>), InputsError> {
    validate(inputs)?;
    let long = SectionTrace {
        key: SectionKey::Long,
        trace: inputs.trace.clone(),
        left: inputs.left,
        right: inputs.right,
        crossing: None,
    };
    let mut out = vec![long];
    let mut notes = Vec::new();

    let (abscissas, common, own): (Vec<f64>, &CrossWidths, &[CrossOwn]) = match &inputs.crosses {
        Crosses::None => return Ok((out, notes)),
        Crosses::Every { spacing, common, own } => {
            (every(total_length(&inputs.trace), *spacing)?, common, own)
        }
        Crosses::Count { count, common, own } => {
            (spread(total_length(&inputs.trace), *count), common, own)
        }
    };

    for o in own {
        if o.index as usize >= abscissas.len() {
            notes.push(Note {
                code: "crossOwnUnused".to_string(),
                sections: Vec::new(),
                objects: Vec::new(),
                said: format!(
                    "cross section {} is set apart, but only {} cross sections were computed",
                    o.index,
                    abscissas.len()
                ),
            });
        }
    }

    for (index, &s) in abscissas.iter().enumerate() {
        let widths = own
            .iter()
            .find(|o| o.index as usize == index)
            .map_or(*common, |o| o.widths);
        let (centre, dir) = locate(&inputs.trace, s);
        // The normal to the left of the trace.
        let n = [-dir[1], dir[0]];
        let before = [centre[0] + n[0] * widths.before, centre[1] + n[1] * widths.before];
        let after = [centre[0] - n[0] * widths.after, centre[1] - n[1] * widths.after];
        out.push(SectionTrace {
            key: SectionKey::Cross { index: index as u32 },
            trace: vec![before, after],
            left: widths.left,
            right: widths.right,
            crossing: Some([s, widths.before]),
        });
    }

    // Every pair, the colinear ones that overlap included.
    for i in 1..out.len() {
        for j in i + 1..out.len() {
            if meet(&out[i].trace, &out[j].trace).is_some() {
                notes.push(Note {
                    code: "crossesCross".to_string(),
                    sections: vec![out[i].key, out[j].key],
                    objects: Vec::new(),
                    said: format!(
                        "cross sections {} and {} run through each other; both are computed \
                         as asked",
                        i - 1,
                        j - 1
                    ),
                });
            }
        }
    }
    Ok((out, notes))
}

/// The length of a polyline.
pub fn total_length(trace: &[V]) -> f64 {
    trace.windows(2).map(|p| (p[1][0] - p[0][0]).hypot(p[1][1] - p[0][1])).sum()
}

/// The multiples of `spacing` up to the end of a trace of length `total`, and
/// the end — the last multiple dropped when it is within half a step of it.
fn every(total: f64, spacing: f64) -> Result<Vec<f64>, InputsError> {
    let n = (total / spacing).ceil();
    if n > MAX_CROSSES as f64 {
        return Err(InputsError::Setting(format!(
            "a spacing of {spacing} on a trace of {total} makes {n} cross sections, over the \
             limit of {MAX_CROSSES}"
        )));
    }
    let mut at: Vec<f64> = Vec::new();
    let mut k = 0u64;
    while (k as f64) * spacing < total {
        at.push(k as f64 * spacing);
        k += 1;
    }
    if at.len() > 1 && total - at[at.len() - 1] < spacing / 2.0 {
        at.pop();
    }
    at.push(total);
    Ok(at)
}

/// `count` abscissas, evenly spread, ends included; the middle for one.
fn spread(total: f64, count: u32) -> Vec<f64> {
    if count == 1 {
        return vec![total / 2.0];
    }
    (0..count).map(|i| total * f64::from(i) / f64::from(count - 1)).collect()
}

/// The point at abscissa `s`, and the unit direction there: the segment's, or
/// at a vertex the **bisector** — the normalised sum of the two segments'
/// directions (the incoming one when the trace turns back on itself and the
/// sum is nothing).
pub fn locate(trace: &[V], s: f64) -> (V, V) {
    let total = total_length(trace);
    let eps = 1e-9 * total.max(1.0);
    let mut dirs: Vec<(V, V, f64, f64)> = Vec::new(); // (a, unit dir, from, to)
    let mut at = 0.0;
    for p in trace.windows(2) {
        let d = [p[1][0] - p[0][0], p[1][1] - p[0][1]];
        let len = d[0].hypot(d[1]);
        if len == 0.0 {
            continue;
        }
        dirs.push((p[0], [d[0] / len, d[1] / len], at, at + len));
        at += len;
    }
    let last = dirs.len() - 1;
    for (k, &(a, d, from, to)) in dirs.iter().enumerate() {
        if s <= to + eps || k == last {
            let t = (s.clamp(from, to)) - from;
            let point = [a[0] + d[0] * t, a[1] + d[1] * t];
            if k < last && (to - s).abs() <= eps {
                let next = dirs[k + 1].1;
                return (point, bisector(d, next));
            }
            if k > 0 && (s - from).abs() <= eps {
                return (point, bisector(dirs[k - 1].1, d));
            }
            return (point, d);
        }
    }
    unreachable!("a trace has a segment: `validate` checked it")
}

fn bisector(incoming: V, outgoing: V) -> V {
    let sum = [incoming[0] + outgoing[0], incoming[1] + outgoing[1]];
    let n = sum[0].hypot(sum[1]);
    if n > 1e-12 { [sum[0] / n, sum[1] / n] } else { incoming }
}

/// Where two polylines meet: the abscissa on each, of the first meeting found
/// (segments touching at an end count; two colinear segments that overlap meet
/// where the overlap starts on the first). `None` when they do not meet.
pub fn meet(a: &[V], b: &[V]) -> Option<[f64; 2]> {
    let scale = a.iter().chain(b.iter()).flat_map(|p| p.iter()).fold(1.0_f64, |m, v| m.max(v.abs()));
    let eps = 1e-9 * scale;
    let mut sa = 0.0;
    for pa in a.windows(2) {
        let (p, r) = (pa[0], [pa[1][0] - pa[0][0], pa[1][1] - pa[0][1]]);
        let la = r[0].hypot(r[1]);
        let mut sb = 0.0;
        for pb in b.windows(2) {
            let (q, u) = (pb[0], [pb[1][0] - pb[0][0], pb[1][1] - pb[0][1]]);
            let lb = u[0].hypot(u[1]);
            if la > 0.0 && lb > 0.0 {
                let qp = [q[0] - p[0], q[1] - p[1]];
                let den = r[0] * u[1] - r[1] * u[0];
                // `den / (la lb)` is the sine of the angle between the two.
                if den.abs() > 1e-12 * la * lb {
                    let t = (qp[0] * u[1] - qp[1] * u[0]) / den;
                    let w = (qp[0] * r[1] - qp[1] * r[0]) / den;
                    let (te, we) = (eps / la, eps / lb);
                    if (-te..=1.0 + te).contains(&t) && (-we..=1.0 + we).contains(&w) {
                        return Some([sa + t.clamp(0.0, 1.0) * la, sb + w.clamp(0.0, 1.0) * lb]);
                    }
                } else if (qp[0] * r[1] - qp[1] * r[0]).abs() / la <= eps {
                    // Colinear: the overlap, in the parameter of the first.
                    let t0 = (qp[0] * r[0] + qp[1] * r[1]) / (la * la);
                    let t1 = t0 + (u[0] * r[0] + u[1] * r[1]) / (la * la);
                    let (lo, hi) = (t0.min(t1).max(0.0), t0.max(t1).min(1.0));
                    let te = eps / la;
                    if lo <= hi + te {
                        let point = [p[0] + r[0] * lo, p[1] + r[1] * lo];
                        let w = ((point[0] - q[0]) * u[0] + (point[1] - q[1]) * u[1]) / lb;
                        return Some([sa + lo * la, sb + w.clamp(0.0, lb)]);
                    }
                }
            }
            sb += lb;
        }
        sa += la;
    }
    None
}
