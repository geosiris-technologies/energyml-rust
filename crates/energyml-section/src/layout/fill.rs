//! Spans, clipped and filled between — pure functions over `(s, z)` polylines.
//!
//! ADR-0015: a fill goes from a trace to **its neighbour in that
//! direction — chosen at each abscissa** ([`nearest`]: the nearest candidate
//! strictly above or below, the polygon cut where it changes) — and where there
//! is none it fills nothing: no floor is invented (ADR-0015).
//! What may be a neighbour is [`polygons`]' one rule: never a fault, never
//! another version of the same horizon.
//!
//! `energyml_geometry::fence::panel` was the first candidate and does not fit:
//! it answers with *triangulated patches in 3D* (a mesh for the scene), where a
//! drawing needs the **outline** of each piece, in `(s, z)`. The rule it states
//! is the one kept here — the union of both curves' abscissas, so every cut
//! point of both curves stays exact; a hole between two spans leaves a gap; a
//! crossing splits the fill in two, at the crossing, instead of twisting
//! through itself — and the tests below state it again on their own numbers.

use crate::model::{TraceItem, TraceRole};

/// A continuous polyline in `(s, z)`, `s` non-decreasing.
pub type Span = Vec<[f64; 2]>;

/// The part of a span whose abscissa is in `[lo, hi]`, the ends interpolated
/// where the span leaves the range. `None` when nothing of it (two points at
/// least) falls inside. Bounds included: a point exactly at `lo` stays.
pub fn clip_span(span: &[[f64; 2]], lo: f64, hi: f64) -> Option<Span> {
    let mut out: Span = Vec::new();
    for pair in span.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let (min, max) = (a[0].min(b[0]), a[0].max(b[0]));
        if max < lo || min > hi {
            continue;
        }
        let pa = if a[0] < lo { lerp_x(a, b, lo) } else { a };
        let pb = if b[0] > hi { lerp_x(a, b, hi) } else { b };
        if out.last() != Some(&pa) {
            out.push(pa);
        }
        out.push(pb);
    }
    (out.len() >= 2).then_some(out)
}

/// The point of segment `a`–`b` at abscissa `x` (`a` and `b` differ in x
/// whenever this is called: a clip only interpolates across a boundary).
fn lerp_x(a: [f64; 2], b: [f64; 2], x: f64) -> [f64; 2] {
    let dx = b[0] - a[0];
    if dx.abs() < f64::EPSILON {
        return [x, a[1]];
    }
    let t = (x - a[0]) / dx;
    [x, a[1] + (b[1] - a[1]) * t]
}

/// A polygon clipped to the strip `lo <= x <= hi` (Sutherland–Hodgman against
/// two half-planes — the strip is convex, so the result is exact).
pub fn clip_polygon(poly: &[[f64; 2]], lo: f64, hi: f64) -> Vec<[f64; 2]> {
    let against = |input: Vec<[f64; 2]>, keep: &dyn Fn(f64) -> bool, edge: f64| -> Vec<[f64; 2]> {
        let mut out = Vec::new();
        for (i, &cur) in input.iter().enumerate() {
            let prev = input[(i + input.len() - 1) % input.len()];
            let (cur_in, prev_in) = (keep(cur[0]), keep(prev[0]));
            if cur_in != prev_in {
                out.push(lerp_x(prev, cur, edge));
            }
            if cur_in {
                out.push(cur);
            }
        }
        out
    };
    if poly.len() < 3 {
        return Vec::new();
    }
    let step = against(poly.to_vec(), &|x| x >= lo, lo);
    if step.is_empty() {
        return step;
    }
    let out = against(step, &|x| x <= hi, hi);
    if out.len() < 3 { Vec::new() } else { out }
}

/// A span's z at an abscissa — `None` outside it.
fn z_at(span: &[[f64; 2]], s: f64) -> Option<f64> {
    let (first, last) = (span.first()?, span.last()?);
    if s < first[0] || s > last[0] {
        return None;
    }
    for pair in span.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if s >= a[0] && s <= b[0] {
            let ds = b[0] - a[0];
            return Some(if ds.abs() < f64::EPSILON {
                a[1]
            } else {
                a[1] + (b[1] - a[1]) * (s - a[0]) / ds
            });
        }
    }
    None
}

/// The polygons between an `upper` curve and a `lower` one — one per run where
/// `upper` really is above `lower`.
///
/// * where either curve has no span, nothing: the two spans' overlap is all
///   there is, and a hole in one is a hole in the fill;
/// * where the order inverts, the fill is cut at the crossing and the inverted
///   run is **not** filled (it would be the area between the curves with the
///   wrong one on top — the neighbour in the other direction owns it).
pub fn between(upper: &[Span], lower: &[Span]) -> Vec<Vec<[f64; 2]>> {
    let mut out = Vec::new();
    for up in upper {
        for low in lower {
            let (Some(u0), Some(ul), Some(l0), Some(ll)) = (up.first(), up.last(), low.first(), low.last())
            else {
                continue;
            };
            let from = u0[0].max(l0[0]);
            let to = ul[0].min(ll[0]);
            if !matches!(to.partial_cmp(&from), Some(std::cmp::Ordering::Greater)) {
                continue;
            }
            let mut steps: Vec<f64> = up
                .iter()
                .chain(low.iter())
                .map(|p| p[0])
                .filter(|s| *s >= from && *s <= to)
                .collect();
            steps.push(from);
            steps.push(to);
            steps.sort_by(f64::total_cmp);
            steps.dedup_by(|a, b| (*a - *b).abs() <= f64::EPSILON * a.abs().max(1.0));
            // (s, z upper, z lower)
            let samples: Vec<(f64, f64, f64)> = steps
                .into_iter()
                .filter_map(|s| Some((s, z_at(up, s)?, z_at(low, s)?)))
                .collect();
            runs(&samples, &mut out);
        }
    }
    out
}

/// Split samples into the runs where the upper curve is above, crossings
/// interpolated, and push each run's outline.
fn runs(samples: &[(f64, f64, f64)], out: &mut Vec<Vec<[f64; 2]>>) {
    let mut run: Vec<(f64, f64, f64)> = Vec::new();
    let flush = |run: &mut Vec<(f64, f64, f64)>, out: &mut Vec<Vec<[f64; 2]>>| {
        if run.len() >= 2 && run.iter().any(|(_, u, l)| u - l > 0.0) {
            let mut poly: Vec<[f64; 2]> = run.iter().map(|&(s, u, _)| [s, u]).collect();
            poly.extend(run.iter().rev().map(|&(s, _, l)| [s, l]));
            out.push(poly);
        }
        run.clear();
    };
    let mut prev: Option<(f64, f64, f64)> = None;
    for &cur in samples {
        let d = cur.1 - cur.2;
        if let Some(p) = prev {
            let dp = p.1 - p.2;
            // A strict sign change: the two curves cross between p and cur.
            if (dp > 0.0 && d < 0.0) || (dp < 0.0 && d > 0.0) {
                let t = dp / (dp - d);
                let s = p.0 + (cur.0 - p.0) * t;
                let z = p.1 + (cur.1 - p.1) * t;
                if dp > 0.0 {
                    run.push((s, z, z));
                    flush(&mut run, out);
                } else {
                    run.clear();
                    run.push((s, z, z));
                }
            }
        }
        if d >= 0.0 {
            run.push(cur);
        } else {
            flush(&mut run, out);
        }
        prev = Some(cur);
    }
    flush(&mut run, out);
}

/// A trace as the fill sees it: its key, what the catalogue says of it, and the
/// spans the drawing keeps of it (clipped, hidden ones already left out).
pub struct Line<'a> {
    /// The trace's key.
    pub key: &'a str,
    /// The trace.
    pub item: &'a TraceItem,
    /// Its spans, `(s, z)`.
    pub spans: &'a [Span],
}

/// Whether `other` may be what `mine` fills **to** — the one rule, for the
/// drawing and for the 3D panels alike:
///
/// * never `mine` itself;
/// * never a fault: a fault is a sub-vertical surface that bounds no unit, and
///   an oblique one crossing the interval would steal the fill from the horizon
///   that really lies there;
/// * never another version of the same horizon: two surfaces whose
///   interpretations interpret **the same feature** are one horizon drawn twice
///   (a lattice and a mesh, a depth and a time version, a faulted and a smooth
///   one — the latter have each their own interpretation, and share only the
///   feature). With no readable feature nothing is grouped: the title and the
///   geometry are never asked.
fn is_candidate(mine: &Line<'_>, other: &Line<'_>) -> bool {
    if other.key == mine.key || other.item.role == TraceRole::Fault {
        return false;
    }
    match (&mine.item.feature, &other.item.feature) {
        (Some(a), Some(b)) => a != b,
        _ => true,
    }
}

/// The polygons of the fill of `mine` up (`up`) or down, to the nearest line in
/// each abscissa among `all`'s candidates ([`is_candidate`]). Empty for a fault:
/// it is never filled, whatever a view carries.
pub fn polygons(mine: &Line<'_>, all: &[Line<'_>], up: bool) -> Vec<Vec<[f64; 2]>> {
    if mine.item.role == TraceRole::Fault {
        return Vec::new();
    }
    let others: Vec<(String, Vec<Span>)> = all
        .iter()
        .filter(|o| is_candidate(mine, o))
        .map(|o| (o.key.to_string(), o.spans.to_vec()))
        .collect();
    nearest(mine.spans, &others, up)
}

/// The fill of `mine` towards the nearest of `others` **in each abscissa**.
///
/// At every `s`, the neighbour is the nearest candidate **strictly** above (up)
/// or below (down); where the nearest changes, or none exists, the polygon is
/// cut. The breaks are the vertices of every curve and the crossings of any two
/// of them, so between two breaks every curve is a straight line and the choice
/// is exact — no sampling. Where no candidate lies on that side, nothing is
/// filled: no floor is invented. A candidate in several spans (a faulted
/// horizon) is followed span by span, so the fill follows the throw.
pub fn nearest(mine: &[Span], others: &[(String, Vec<Span>)], up: bool) -> Vec<Vec<[f64; 2]>> {
    let mut out = Vec::new();
    for m in mine {
        for env in envelope(m, others, up) {
            if up {
                out.extend(between(std::slice::from_ref(&env), std::slice::from_ref(m)));
            } else {
                out.extend(between(std::slice::from_ref(m), std::slice::from_ref(&env)));
            }
        }
    }
    out
}

/// A span's z at each of the ascending `steps` — `None` outside it. One pass.
fn sample(span: &[[f64; 2]], steps: &[f64]) -> Vec<Option<f64>> {
    let (Some(first), Some(last)) = (span.first(), span.last()) else {
        return vec![None; steps.len()];
    };
    let mut j = 0;
    steps
        .iter()
        .map(|&s| {
            if s < first[0] || s > last[0] || span.len() < 2 {
                return None;
            }
            while j + 2 < span.len() && span[j + 1][0] < s {
                j += 1;
            }
            let (a, b) = (span[j], span[j + 1]);
            let ds = b[0] - a[0];
            Some(if ds.abs() < f64::EPSILON {
                a[1]
            } else {
                a[1] + (b[1] - a[1]) * (s - a[0]) / ds
            })
        })
        .collect()
}

/// The line the fill of one span goes to: the nearest candidate in each
/// abscissa, as spans — one per run of the same candidate, so a change of
/// neighbour (or a throw of its own) starts a new span.
fn envelope(m: &[[f64; 2]], others: &[(String, Vec<Span>)], up: bool) -> Vec<Span> {
    let (Some(first), Some(last)) = (m.first(), m.last()) else {
        return Vec::new();
    };
    let (lo, hi) = (first[0], last[0]);
    if !matches!(hi.partial_cmp(&lo), Some(std::cmp::Ordering::Greater)) {
        return Vec::new();
    }
    let mut steps: Vec<f64> = m.iter().map(|p| p[0]).collect();
    for (_, spans) in others {
        for span in spans {
            steps.extend(span.iter().map(|p| p[0]));
        }
    }
    steps.retain(|s| *s >= lo && *s <= hi);
    steps.push(lo);
    steps.push(hi);
    steps.sort_by(f64::total_cmp);
    steps.dedup_by(|a, b| (*a - *b).abs() <= f64::EPSILON * a.abs().max(1.0));

    let zm = sample(m, &steps);
    let samples: Vec<Vec<Vec<Option<f64>>>> = others
        .iter()
        .map(|(_, spans)| spans.iter().map(|sp| sample(sp, &steps)).collect())
        .collect();

    // (candidate, start, end) of each stretch where one candidate is the nearest.
    let mut pieces: Vec<(usize, [f64; 2], [f64; 2])> = Vec::new();
    for i in 0..steps.len() - 1 {
        let (a, b) = (steps[i], steps[i + 1]);
        let (Some(ma), Some(mb)) = (zm[i], zm[i + 1]) else {
            continue;
        };
        // The candidates on the wanted side at one end at least, whole over [a, b].
        let mut alive: Vec<(usize, f64, f64)> = Vec::new();
        for (ci, per_span) in samples.iter().enumerate() {
            for sv in per_span {
                if let (Some(za), Some(zb)) = (sv[i], sv[i + 1]) {
                    let (da, db) = (za - ma, zb - mb);
                    if (up && (da > 0.0 || db > 0.0)) || (!up && (da < 0.0 || db < 0.0)) {
                        alive.push((ci, za, zb));
                    }
                    break;
                }
            }
        }
        if alive.is_empty() {
            continue;
        }
        // Where two lines of the interval cross, in t in (0, 1).
        let mut lines: Vec<(f64, f64)> = vec![(ma, mb)];
        lines.extend(alive.iter().map(|&(_, za, zb)| (za, zb)));
        let mut ts: Vec<f64> = vec![0.0, 1.0];
        for x in 0..lines.len() {
            for y in x + 1..lines.len() {
                let (d0, d1) = (lines[x].0 - lines[y].0, lines[x].1 - lines[y].1);
                if (d0 > 0.0 && d1 < 0.0) || (d0 < 0.0 && d1 > 0.0) {
                    ts.push(d0 / (d0 - d1));
                }
            }
        }
        ts.sort_by(f64::total_cmp);
        let at = |t: f64| {
            if t <= 0.0 {
                a
            } else if t >= 1.0 {
                b
            } else {
                a + (b - a) * t
            }
        };
        for w in ts.windows(2) {
            let (t0, t1) = (w[0], w[1]);
            if t1 - t0 <= 1e-12 {
                continue;
            }
            let tm = (t0 + t1) / 2.0;
            let zmm = ma + (mb - ma) * tm;
            let mut best: Option<(usize, f64)> = None;
            for &(ci, za, zb) in &alive {
                let zc = za + (zb - za) * tm;
                let beyond = if up { zc > zmm } else { zc < zmm };
                let nearer = best.is_none_or(|(_, bz)| if up { zc < bz } else { zc > bz });
                if beyond && nearer {
                    best = Some((ci, zc));
                }
            }
            let Some((ci, _)) = best else { continue };
            let (_, za, zb) = *alive
                .iter()
                .find(|(c, _, _)| *c == ci)
                .expect("chosen from alive");
            let z = |t: f64| za + (zb - za) * t;
            pieces.push((ci, [at(t0), z(t0)], [at(t1), z(t1)]));
        }
    }

    // One span per run of the same candidate, continuous end to start.
    let mut runs: Vec<(usize, Span)> = Vec::new();
    for (ci, p, q) in pieces {
        match runs.last_mut() {
            Some((c, span))
                if *c == ci
                    && span.last().is_some_and(|e| {
                        e[0] == p[0] && (e[1] - p[1]).abs() <= 1e-9 * (1.0 + p[1].abs())
                    }) =>
            {
                span.push(q);
            }
            _ => runs.push((ci, vec![p, q])),
        }
    }
    runs.into_iter().map(|(_, span)| span).collect()
}
