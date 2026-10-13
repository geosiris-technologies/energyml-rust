//! The **filled** cross-section — the panels between two surfaces along a
//! trace.
//!
//! [`section::cut`](crate::section::cut) gives the curves; this module puts
//! material between them. The computation is pure and free of I/O, like the
//! section: it is domain vocabulary, a desktop interface will reuse it as it
//! stands, and a test reaches it with no browser.
//!
//! # The vocabulary, settled
//!
//! A **trace** is a polyline in plan; a **section** is a vertical cut along a
//! trace; a **panel** is the sheet between two consecutive surfaces; a **fence**
//! is several sections crossing each other. The unit of the code is the
//! section; the fence is a composition. Two different things under one name is,
//! six months later, a function nobody knows the argument of.
//!
//! # The three cases that are not confused
//!
//! This is where the difference is decided between an honest section and one
//! that lies with nothing to show it:
//!
//! - **a hole** — a surface does not cover the whole trace, or its nodes are
//!   undefined: the panel **stops**. Never an extrapolation, not even out to
//!   the edge of the drawing; a band extended "to look right" invents a unit
//!   where nobody has data;
//! - **a wedge** — the thickness falls to zero: that is ordinary geology (a
//!   unit pinching out), the panel closes and nothing is reported. Code that
//!   shouted about this would shout about half of all models, and people would
//!   learn to ignore its messages;
//! - **a crossing** — the thickness turns negative: the panel is cut at the
//!   exact crossing point and the inverted part becomes a **marked** patch. It
//!   is neither removed nor straightened: a crossing has at least four causes
//!   (erosion, an extrapolated surface, fault throw, wrong data) and choosing
//!   between them is not the draughtsman's business. Marking it makes it
//!   visible; straightening it makes it invisible.
//!
//! # Why patches rather than one sheet
//!
//! A panel is a list of patches, one per continuous piece. That is already the
//! splitting mechanism (`promote_artefact` writes one `TrianglePatch` per
//! patch, with its own dataset), so multi-patch costs nothing — and it settles
//! holes **and** crossings by the same route, with no special case.

use crate::section::{self, Cut};

/// The relative tolerance of a wedge: beyond it, two surfaces cross; below it,
/// they touch. Relative to the section's vertical extent, never absolute — a
/// model in millimetres and a model in kilometres do not share an idea of
/// "zero".
const PINCH: f64 = 1e-9;

/// One section curve: what a surface gives along the trace, in **continuous**
/// spans increasing in s.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Curve {
    /// The spans, each sorted by increasing s. Two spans do not touch: between
    /// them, the surface does not cut the trace.
    pub spans: Vec<Vec<[f64; 2]>>,
    /// What the curve could not say — a span dropped, and why.
    pub notes: Vec<String>,
}

/// The (s, z) curves of a surface, from its cut segments.
///
/// The chaining is `section`'s — welded on an exactly shared end — because that
/// is what tells two sheets of a faulted surface from two neighbouring
/// triangles, with no threshold.
///
/// A span whose abscissa is **not monotonic** is dropped, saying so: the
/// surface folds back over itself along this trace (an overturned flank), so z
/// is not a function of s and a panel built on it would twist through itself.
/// The refusal names the span; it does not empty the curve, the other spans
/// still answer.
pub fn curve(cuts: &[Cut]) -> Curve {
    let chains = section::chain(cuts)
        .into_iter()
        .map(|chain| (chain, Vec::new()))
        .collect();
    curve_of_chains(chains).curve
}

/// A [`Curve`], and where each point of each span comes from.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct SourcedCurve {
    /// The curve, exactly as [`curve`] gives it from the same cuts.
    pub curve: Curve,
    /// Parallel to `curve.spans`: `sources[i][j]` is the edge `curve.spans[i][j]`
    /// lies on. A span is reversed, and the spans sorted, **with** their
    /// sources. A node where two segments weld keeps the source of the segment
    /// walked first ([`section::SourcedChain::sources`]).
    pub sources: Vec<Vec<section::CutEnd>>,
}

/// [`curve`], keeping the source of every point of every span. The spans, their
/// order, their notes are [`curve`]'s — one function behind both.
pub fn curve_sourced(cuts: &[section::SourcedCut]) -> SourcedCurve {
    let chains = section::chain_sourced(cuts)
        .into_iter()
        .map(|one| (one.chain, one.sources))
        .collect();
    curve_of_chains(chains)
}

/// The one body of [`curve`] and [`curve_sourced`]. `sources` is empty for a
/// chain whose sources were not asked for, and it is then carried as empty.
fn curve_of_chains(chains: Vec<(section::Chain, Vec<section::CutEnd>)>) -> SourcedCurve {
    let mut out = Curve::default();
    let mut kept: Vec<(Vec<[f64; 2]>, Vec<section::CutEnd>)> = Vec::new();
    for (at, (chain, mut sources)) in chains.into_iter().enumerate() {
        if chain.profile.len() < 2 {
            continue;
        }
        let mut samples = chain.profile.clone();
        // The chaining follows the surface, not the abscissa: a span may be
        // walked right to left, which is the same span.
        if samples.first().map(|p| p[0]) > samples.last().map(|p| p[0]) {
            samples.reverse();
            sources.reverse();
        }
        if !samples.windows(2).all(|pair| pair[1][0] >= pair[0][0]) {
            out.notes.push(format!(
                "span {at}: the abscissa is not monotonic — the surface folds back along this \
                 trace, and z is not a function of s there"
            ));
            continue;
        }
        kept.push((samples, sources));
    }
    // Ordered by starting abscissa: the section reads left to right, and the
    // chaining order is the triangles' order, which means nothing here.
    kept.sort_by(|a, b| a.0[0][0].total_cmp(&b.0[0][0]));
    let mut sourced = Vec::with_capacity(kept.len());
    for (samples, sources) in kept {
        out.spans.push(samples);
        sourced.push(sources);
    }
    SourcedCurve {
        curve: out,
        sources: sourced,
    }
}

/// A curve's z at an abscissa, interpolated **within a span** — `None` outside
/// the spans: between two spans, the surface does not exist.
fn z_at(span: &[[f64; 2]], s: f64) -> Option<f64> {
    let (first, last) = (span.first()?, span.last()?);
    if s < first[0] || s > last[0] {
        return None;
    }
    for pair in span.windows(2) {
        let ([s0, z0], [s1, z1]) = (pair[0], pair[1]);
        if s >= s0 && s <= s1 {
            let span_s = s1 - s0;
            if span_s.abs() < f64::EPSILON {
                return Some(z0);
            }
            return Some(z0 + (z1 - z0) * (s - s0) / span_s);
        }
    }
    None
}

/// One piece of a panel: continuous material between two surfaces.
#[derive(Clone, Debug, PartialEq)]
pub struct Patch {
    /// The vertices, in 3D, placed back onto the trace.
    pub points: Vec<[f64; 3]>,
    /// The triangles, three indices into `points`.
    pub triangles: Vec<[u32; 3]>,
    /// The same vertices in (s, z) — the section laid flat, for free.
    pub profile: Vec<[f64; 2]>,
    /// The expected order is **inverted** here: the surface given as the upper
    /// one passes below the other. A sign, never a correction — this is what
    /// the interpretation workshop will ask a human to settle.
    pub inverted: bool,
}

/// What a panel yields: its pieces, and what it noticed.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Panel {
    /// The material, in continuous pieces — one per hole and per crossing.
    pub patches: Vec<Patch>,
    /// The findings — a crossing measured, a span dropped. Never a silence: a
    /// mute section and a section with no problem look alike.
    pub notes: Vec<String>,
}

impl Panel {
    /// Whether the panel holds any material — an empty panel is an answer (the
    /// two surfaces meet nowhere along this trace), not a failure.
    pub fn is_empty(&self) -> bool {
        self.patches.is_empty()
    }
}

/// The panel between two curves, `upper` being the one expected **above**.
///
/// The z values are those of the frame it is given, and that frame must be the
/// one where z rises (projected). The module does not guess: `to_projected` has
/// either been applied by the caller or not, and a silent inversion here would
/// flip every section of a package in depth.
///
/// `trace` is what places the vertices back in 3D — a panel that did not know
/// where it is would overlay nothing in the scene.
pub fn panel(upper: &Curve, lower: &Curve, trace: &[[f64; 2]]) -> Panel {
    let mut out = Panel::default();
    for (i, up) in upper.spans.iter().enumerate() {
        for (j, low) in lower.spans.iter().enumerate() {
            // Where the two spans overlap: outside it, one of the two surfaces
            // does not exist, and there is nothing to fill.
            let from = up[0][0].max(low[0][0]);
            let to = up[up.len() - 1][0].min(low[low.len() - 1][0]);
            // A null or absent overlap: the two spans follow each other without
            // meeting, and there is nothing to fill. `matches!` rather than a
            // reversed `>` because a NaN must leave here, not further down.
            if !matches!(to.partial_cmp(&from), Some(std::cmp::Ordering::Greater)) {
                continue;
            }
            // **The union of the abscissas**, not a fixed step: it keeps every
            // cut point of both surfaces exactly, where a uniform resampling
            // smooths them and moves the contacts.
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

            let mut samples: Vec<(f64, f64, f64)> = Vec::with_capacity(steps.len());
            for s in steps {
                let (Some(zu), Some(zl)) = (z_at(up, s), z_at(low, s)) else {
                    continue;
                };
                samples.push((s, zu, zl));
            }
            if samples.len() < 2 {
                continue;
            }
            let scale = samples
                .iter()
                .map(|(_, zu, zl)| zu.abs().max(zl.abs()))
                .fold(1.0_f64, f64::max);
            let crossed = build(&mut out, &samples, trace, scale * PINCH);
            if crossed > 0 {
                out.notes.push(format!(
                    "spans {i}/{j}: {crossed} crossing(s) — the upper surface passes below the \
                     lower one, which only a human can explain (erosion, extension, throw, wrong \
                     data)"
                ));
            }
        }
    }
    out.notes.extend(upper.notes.iter().cloned());
    out.notes.extend(lower.notes.iter().cloned());
    out
}

/// Build the quadrilaterals of a run of samples, cutting at every sign change
/// of the thickness. Returns the number of crossings.
fn build(out: &mut Panel, samples: &[(f64, f64, f64)], trace: &[[f64; 2]], pinch: f64) -> usize {
    let mut crossed = 0usize;
    // The current sign: `None` while the thickness is nil — a panel that starts
    // on a wedge has no side yet.
    let mut current: Vec<(f64, f64, f64)> = Vec::new();
    let mut sign: Option<bool> = None;

    let sign_of = |zu: f64, zl: f64| -> Option<bool> {
        let thickness = zu - zl;
        if thickness.abs() <= pinch {
            None
        } else {
            Some(thickness > 0.0)
        }
    };

    for (at, sample) in samples.iter().enumerate() {
        let here = sign_of(sample.1, sample.2);
        match (sign, here) {
            // The first known side.
            (None, Some(side)) => {
                sign = Some(side);
                current.push(*sample);
            }
            // The wedge: the point is kept, it closes the sheet cleanly.
            (_, None) => current.push(*sample),
            (Some(was), Some(side)) if was == side => current.push(*sample),
            // The crossing: cut at the **exact** intersection of the two
            // segments, not at the last sample — otherwise the patch spills
            // over onto the side that is no longer its own.
            (Some(was), Some(side)) => {
                debug_assert_ne!(was, side);
                crossed += 1;
                let previous = samples[at - 1];
                let meeting = crossing(previous, *sample);
                current.push(meeting);
                flush(out, &mut current, trace, was);
                current.push(meeting);
                current.push(*sample);
                sign = Some(side);
            }
        }
    }
    flush(out, &mut current, trace, sign.unwrap_or(true));
    crossed
}

/// Where two curves cross between two samples — both segments are linear, so
/// the abscissa resolves exactly.
fn crossing(a: (f64, f64, f64), b: (f64, f64, f64)) -> (f64, f64, f64) {
    let (s0, zu0, zl0) = a;
    let (s1, zu1, zl1) = b;
    let d0 = zu0 - zl0;
    let d1 = zu1 - zl1;
    let step = d0 - d1;
    if step.abs() < f64::EPSILON {
        return b;
    }
    let t = (d0 / step).clamp(0.0, 1.0);
    let s = s0 + (s1 - s0) * t;
    let z = zu0 + (zu1 - zu0) * t;
    // The two curves meet: one z only, so both edges of the panel coincide
    // there.
    (s, z, z)
}

/// Close the current piece into a patch, if it carries any material.
fn flush(out: &mut Panel, current: &mut Vec<(f64, f64, f64)>, trace: &[[f64; 2]], upright: bool) {
    let samples = std::mem::take(current);
    if samples.len() < 2 {
        return;
    }
    let mut points = Vec::with_capacity(samples.len() * 2);
    let mut profile = Vec::with_capacity(samples.len() * 2);
    for (s, zu, zl) in &samples {
        let [x, y] = point_at(trace, *s);
        // Two vertices per abscissa: the top then the bottom, so the bottom's
        // index is the top's plus one — which the triangulation assumes.
        points.push([x, y, *zu]);
        points.push([x, y, *zl]);
        profile.push([*s, *zu]);
        profile.push([*s, *zl]);
    }
    let mut triangles = Vec::with_capacity((samples.len() - 1) * 2);
    for at in 0..samples.len() - 1 {
        let (a, b) = ((at * 2) as u32, (at * 2 + 2) as u32);
        // A quadrilateral with a null side (the wedge) gives only one triangle;
        // writing two would lay down a triangle of zero area, which every
        // renderer accepts and nothing reports.
        let left_flat = (samples[at].1 - samples[at].2).abs() <= f64::EPSILON;
        let right_flat = (samples[at + 1].1 - samples[at + 1].2).abs() <= f64::EPSILON;
        if !left_flat {
            triangles.push([a, a + 1, b]);
        }
        if !right_flat {
            triangles.push([a + 1, b + 1, b]);
        }
    }
    if triangles.is_empty() {
        return;
    }
    out.patches.push(Patch {
        points,
        triangles,
        profile,
        inverted: !upright,
    });
}

/// The point of the trace at a curvilinear abscissa — the same abscissa
/// `section::cut` accumulates, so the two place themselves at the same spot.
///
/// Outside the trace, the point is **clamped** to its ends rather than
/// extrapolated: a section does not leave its trace.
pub fn point_at(trace: &[[f64; 2]], s: f64) -> [f64; 2] {
    let Some(first) = trace.first() else {
        return [0.0, 0.0];
    };
    if s <= 0.0 {
        return *first;
    }
    let mut travelled = 0.0;
    for pair in trace.windows(2) {
        let ([x0, y0], [x1, y1]) = (pair[0], pair[1]);
        let length = (x1 - x0).hypot(y1 - y0);
        if length < 1e-12 {
            continue;
        }
        if s <= travelled + length {
            let t = (s - travelled) / length;
            return [x0 + (x1 - x0) * t, y0 + (y1 - y0) * t];
        }
        travelled += length;
    }
    *trace.last().unwrap_or(first)
}

/// A **crossline**: the plan trace of a section perpendicular to the leading
/// trace, at a given abscissa.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crossline {
    /// The curvilinear abscissa on the leading trace — the same one
    /// `section::cut` accumulates, so the two place themselves at the same spot.
    pub s: f64,
    /// The two ends in plan: `[x0, y0, x1, y1]`. A two-point polyline, ready for
    /// `section::cut` like any other trace.
    pub trace: [f64; 4],
}

/// A trace's crosslines, and what computing them noticed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Crosslines {
    /// The crosslines, by increasing abscissa on the leading trace.
    pub lines: Vec<Crossline>,
    /// What the computation noticed without it being a refusal.
    pub notes: Vec<String>,
}

/// The crosslines of a leading trace — the *fence* proper
/// (crosslines that cut each other are refused).
///
/// # What it is
///
/// The leading trace gives one section; these crosslines give others,
/// perpendicular, spread along it. Together they are the grid of panels a
/// geologist calls a fence.
///
/// # Sampled in **arc length**, not per vertex
///
/// A hand-drawn trace has its vertices where the hand clicked: crowded in a
/// bend, spread out on a straight run. One crossline per vertex would therefore
/// give a grid whose spacing means nothing — dense where someone hesitated.
/// Curvilinear abscissa is the only measure that makes a step regular.
///
/// # `half_width` is a **half**-width
///
/// The crossline extends on both sides of the trace, so its length is
/// `2 × half_width`. Named that way rather than "width" for the same reason the
/// tube takes a radius: converting silently on one side of a pair of interfaces
/// is how a section comes out twice too wide.
///
/// # A tight bend is **refused by name**
///
/// In a bend tighter than the half-width, two neighbouring crosslines run
/// through each other. Drawing them would produce an intersection that does not
/// exist, and the eye reads it as a structure. The refusal says **where** and
/// **which half-width** would pass — a refusal that does not say how to get out
/// of it is a closed door.
///
/// Refused rather than corrected: dropping one of the two crosslines settles
/// nothing (its neighbours then run through each other in turn), and smoothing
/// the trace would draw a section along a path nobody pointed at.
pub fn crosslines(trace: &[[f64; 2]], spacing: f64, half_width: f64) -> Result<Crosslines, String> {
    if !spacing.is_finite() || spacing <= 0.0 {
        return Err(format!("a spacing of {spacing} places no crossline"));
    }
    if !half_width.is_finite() || half_width <= 0.0 {
        return Err(format!("a half-width of {half_width} cuts nothing"));
    }
    // The useful segments: the ones that have a length. A trace clicked twice
    // in the same place holds some, and a tangent is undefined there.
    let mut segments: Vec<([f64; 2], [f64; 2], f64)> = Vec::new();
    let mut total = 0.0;
    for pair in trace.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if !a.iter().chain(b.iter()).all(|v| v.is_finite()) {
            return Err(
                "the trace carries a point with no position — nothing is perpendicular \
                        to it"
                    .into(),
            );
        }
        let length = (b[0] - a[0]).hypot(b[1] - a[1]);
        if length < 1e-12 {
            continue;
        }
        segments.push((a, b, length));
        total += length;
    }
    if segments.is_empty() {
        return Err("the trace has no two distinct points".into());
    }

    // The abscissas: the multiples of the spacing, plus **the end**. The end is
    // kept because a fence that stops before the trace does suggests the trace
    // stopped there; the last multiple is dropped if it is within half a step
    // of the end, where the two crosslines would overlap without showing
    // anything more.
    let mut abscissas: Vec<f64> = Vec::new();
    let mut at = 0.0;
    while at < total {
        abscissas.push(at);
        at += spacing;
    }
    match abscissas.last() {
        Some(&last) if total - last < spacing / 2.0 && abscissas.len() > 1 => {
            abscissas.pop();
        }
        _ => {}
    }
    abscissas.push(total);

    let lines: Vec<Crossline> = abscissas
        .iter()
        .map(|&s| {
            let centre = point_at(trace, s);
            let [tx, ty] = tangent_at(&segments, s);
            // The normal in plan: the tangent turned a quarter turn. Its
            // direction does not matter — the crossline is symmetric — but it
            // is the **same** everywhere, which keeps the crosslines consistent
            // with each other and makes a crossing detectable.
            let (nx, ny) = (-ty, tx);
            Crossline {
                s,
                trace: [
                    centre[0] - nx * half_width,
                    centre[1] - ny * half_width,
                    centre[0] + nx * half_width,
                    centre[1] + ny * half_width,
                ],
            }
        })
        .collect();

    // The check: two crosslines that cut each other are the case the design refuses.
    // Measured by segment intersection rather than deduced from an angle,
    // because an angle needs a radius of curvature and a clicked trace has none.
    //
    // **Every pair, not only the neighbours.** The document spoke only of the
    // bend, but a trace that folds back on itself — a hairpin, a trace that
    // recrosses — makes crosslines far apart in s run through each other, with
    // perfectly tame neighbours. Checking only the neighbours would read as
    // "verified", and the square over a few dozen crosslines costs nothing.
    for (at, one) in lines.iter().enumerate() {
        for next in &lines[at + 1..] {
            let Some(reach) = crossing_reach(&one.trace, &next.trace) else {
                continue;
            };
            // **The cause is not named.** A first attempt told "the bend is too
            // tight" from "the trace folds back on itself" by the gap in s, and
            // the test contradicted it on a plain L: the first pair that runs
            // through itself there is two steps apart while the cause is
            // certainly the bend. What is *measured* is that the trace passes
            // within `reach` of itself; the rest was a guess, and a guess in an
            // error message sends people looking in the wrong place.
            return Err(format!(
                "the crosslines at s = {:.3} and s = {:.3} run through each other: the trace \
                 passes within {:.3} of itself there. A half-width under {:.3} would pass.",
                one.s, next.s, reach, reach
            ));
        }
    }

    let mut notes = Vec::new();
    if lines.len() < 2 {
        notes.push(format!(
            "only one crossline: the trace measures {total:.3} for a spacing of {spacing}"
        ));
    }
    Ok(Crosslines { lines, notes })
}

/// The unit tangent at abscissa `s`, taken on the segment that contains it.
///
/// At a vertex the tangent jumps — there are two, one per side. The *incoming*
/// segment's is returned, and that choice beats an average: a bisector would
/// give a crossline perpendicular to neither segment, hence oblique to the
/// section it crosses.
fn tangent_at(segments: &[([f64; 2], [f64; 2], f64)], s: f64) -> [f64; 2] {
    let mut travelled = 0.0;
    let mut last = [1.0, 0.0];
    for &(a, b, length) in segments {
        last = [(b[0] - a[0]) / length, (b[1] - a[1]) / length];
        if s <= travelled + length {
            return last;
        }
        travelled += length;
    }
    last
}

/// Do two crosslines run through each other, and how far from their trace?
///
/// `None` if they do not cut. Otherwise the distance from the crossing to the
/// first one's centre: that is the half-width at which the problem appears,
/// hence the one to go under for it to disappear.
fn crossing_reach(one: &[f64; 4], two: &[f64; 4]) -> Option<f64> {
    let (p, r) = ([one[0], one[1]], [one[2] - one[0], one[3] - one[1]]);
    let (q, s) = ([two[0], two[1]], [two[2] - two[0], two[3] - two[1]]);
    let centre = [(one[0] + one[2]) / 2.0, (one[1] + one[3]) / 2.0];
    let other = [(two[0] + two[2]) / 2.0, (two[1] + two[3]) / 2.0];
    let denominator = r[0] * s[1] - r[1] * s[0];
    let (dx, dy) = (q[0] - p[0], q[1] - p[1]);

    if denominator.abs() < 1e-15 {
        // Parallel. Two **distinct** parallel crosslines never run through each
        // other — the ordinary case of a straight trace, where all of them are.
        // But parallel and **carried by the same line**, they overlap, and two
        // panels drawn on top of each other state the same section twice. That
        // is the case of a trace that comes back exactly along its own path,
        // and missing it was the second defect the tests found: a segment does
        // not "cut" another one when it lies on it.
        let off_line = dx * r[1] - dy * r[0];
        if off_line.abs() > 1e-12 * r[0].hypot(r[1]).max(1.0) {
            return None;
        }
        // On the same line: they overlap as long as their centres are closer
        // than the sum of the two half-lengths — which are the same half-width.
        let apart = (other[0] - centre[0]).hypot(other[1] - centre[1]);
        let half = r[0].hypot(r[1]) / 2.0;
        return (apart < half * 2.0).then_some(apart / 2.0);
    }

    let t = (dx * s[1] - dy * s[0]) / denominator;
    let u = (dx * r[1] - dy * r[0]) / denominator;
    // Bounds **inclusive**: two crosslines touching end to end are the exact
    // limit of the refused case, and on the right side of the refusal — "refuse
    // rather than draw". That is consistent with the message, which offers a
    // half-width *under* the measure rather than equal to it.
    if !(0.0..=1.0).contains(&t) || !(0.0..=1.0).contains(&u) {
        return None;
    }
    let hit = [p[0] + r[0] * t, p[1] + r[1] * t];
    Some((hit[0] - centre[0]).hypot(hit[1] - centre[1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A horizontal square at the given height, from (0,0) to (10,10).
    fn flat(z: f64) -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
        (
            vec![[0.0, 0.0, z], [10.0, 0.0, z], [10.0, 10.0, z], [0.0, 10.0, z]],
            vec![[0, 1, 2], [0, 2, 3]],
        )
    }

    /// A sloped square: z runs from `at_zero` at x = 0 to `at_ten` at x = 10.
    fn sloped(at_zero: f64, at_ten: f64) -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
        (
            vec![
                [0.0, 0.0, at_zero],
                [10.0, 0.0, at_ten],
                [10.0, 10.0, at_ten],
                [0.0, 10.0, at_zero],
            ],
            vec![[0, 1, 2], [0, 2, 3]],
        )
    }

    /// The reference trace: a west-to-east crossing at y = 5.
    fn trace() -> Vec<[f64; 2]> {
        vec![[0.0, 5.0], [10.0, 5.0]]
    }

    fn curve_of(surface: (Vec<[f64; 3]>, Vec<[u32; 3]>)) -> Curve {
        curve(&section::cut(&surface.0, &surface.1, &trace()))
    }

    #[test]
    fn two_flat_surfaces_make_one_slab_of_the_right_thickness() {
        let panel = panel(&curve_of(flat(100.0)), &curve_of(flat(50.0)), &trace());
        assert_eq!(panel.patches.len(), 1, "{:?}", panel.notes);
        assert!(panel.notes.is_empty(), "{:?}", panel.notes);
        let patch = &panel.patches[0];
        assert!(!patch.inverted);
        // All the material is between the two surfaces, and nowhere else.
        for point in &patch.points {
            assert!((50.0..=100.0).contains(&point[2]), "{point:?}");
            // And on the trace: y = 5 throughout, x within the crossing.
            assert!((point[1] - 5.0).abs() < 1e-9, "{point:?}");
            assert!((0.0..=10.0).contains(&point[0]), "{point:?}");
        }
        assert!(
            patch
                .triangles
                .iter()
                .flatten()
                .all(|i| (*i as usize) < patch.points.len())
        );
    }

    #[test]
    fn a_wedge_pinches_out_without_a_word() {
        // The roof comes down until it touches the base: a unit pinching out,
        // ordinary geology — no message, and one continuous sheet.
        let panel = panel(&curve_of(sloped(100.0, 50.0)), &curve_of(flat(50.0)), &trace());
        assert_eq!(panel.patches.len(), 1, "{:?}", panel.notes);
        assert!(
            panel.notes.is_empty(),
            "a wedge is not an anomaly: {:?}",
            panel.notes
        );
        assert!(!panel.patches[0].inverted);
        // The thickness never turns negative.
        let patch = &panel.patches[0];
        for pair in patch.profile.chunks(2) {
            assert!(pair[0][1] - pair[1][1] >= -1e-9, "{pair:?}");
        }
    }

    #[test]
    fn a_crossing_splits_the_panel_and_marks_the_inverted_half() {
        // Two surfaces crossing in the middle: the "upper" one ends up below
        // the "lower" one. The panel is cut at the crossing and the second half
        // is marked — neither straightened nor removed.
        let panel = panel(
            &curve_of(sloped(100.0, 0.0)),
            &curve_of(sloped(0.0, 100.0)),
            &trace(),
        );
        assert_eq!(panel.patches.len(), 2, "{:?}", panel.notes);
        assert!(!panel.patches[0].inverted);
        assert!(panel.patches[1].inverted, "the inverted half must say so");
        assert!(
            panel.notes.iter().any(|note| note.contains("crossing")),
            "{:?}",
            panel.notes
        );
        // The cut falls at the exact crossing (x = 5, z = 50), not at the last
        // sample before it: a patch that spilled over would cover the half that
        // is no longer its own.
        let end = panel.patches[0].profile.last().expect("a last point");
        assert!((end[0] - 5.0).abs() < 1e-9, "{end:?}");
        assert!((end[1] - 50.0).abs() < 1e-9, "{end:?}");
    }

    #[test]
    fn a_hole_in_a_surface_ends_the_patch_rather_than_bridging_it() {
        // Two squares separated by a void: the upper surface does not exist in
        // the middle of the trace. The panel must stop there and resume after —
        // never bridge, which would invent material.
        let points = vec![
            [0.0, 0.0, 100.0],
            [3.0, 0.0, 100.0],
            [3.0, 10.0, 100.0],
            [0.0, 10.0, 100.0],
            [7.0, 0.0, 100.0],
            [10.0, 0.0, 100.0],
            [10.0, 10.0, 100.0],
            [7.0, 10.0, 100.0],
        ];
        let triangles = vec![[0, 1, 2], [0, 2, 3], [4, 5, 6], [4, 6, 7]];
        let upper = curve(&section::cut(&points, &triangles, &trace()));
        assert_eq!(upper.spans.len(), 2, "two spans, not one");

        let panel = panel(&upper, &curve_of(flat(50.0)), &trace());
        assert_eq!(panel.patches.len(), 2);
        // Nothing between x = 3 and x = 7: the hole stays a hole.
        for patch in &panel.patches {
            for point in &patch.points {
                assert!(point[0] <= 3.0 + 1e-9 || point[0] >= 7.0 - 1e-9, "{point:?}");
            }
        }
    }

    #[test]
    fn a_surface_with_no_cut_gives_an_empty_panel_not_a_failure() {
        // A trace beside everything: the two surfaces meet nowhere, and that is
        // an answer.
        let away = vec![[100.0, 100.0], [110.0, 100.0]];
        let upper = curve(&section::cut(&flat(100.0).0, &flat(100.0).1, &away));
        let panel = panel(&upper, &curve_of(flat(50.0)), &away);
        assert!(panel.is_empty());
    }

    #[test]
    fn a_bent_trace_places_its_points_on_the_bend() {
        // The curvilinear abscissa is continuous through the bend — that is
        // what `section::cut` accumulates — so placing back in 3D must follow
        // the bend and not the chord.
        let bent = vec![[0.0, 0.0], [5.0, 0.0], [5.0, 5.0]];
        assert_eq!(point_at(&bent, 0.0), [0.0, 0.0]);
        assert_eq!(point_at(&bent, 2.5), [2.5, 0.0]);
        assert_eq!(point_at(&bent, 5.0), [5.0, 0.0]);
        assert_eq!(point_at(&bent, 7.5), [5.0, 2.5]);
        // Outside the trace: clamped, never extrapolated.
        assert_eq!(point_at(&bent, 99.0), [5.0, 5.0]);
        assert_eq!(point_at(&bent, -3.0), [0.0, 0.0]);
    }

    #[test]
    fn a_fold_is_refused_by_name_rather_than_twisted_into_a_panel() {
        // A surface folded along the trace: z is not a function of s there. A
        // panel built on it would twist through itself.
        let cuts = vec![
            Cut {
                ends: [[0.0, 5.0, 10.0], [5.0, 5.0, 20.0]],
                profile: [[0.0, 10.0], [5.0, 20.0]],
            },
            Cut {
                ends: [[5.0, 5.0, 20.0], [2.0, 5.0, 30.0]],
                profile: [[5.0, 20.0], [2.0, 30.0]],
            },
        ];
        let made = curve(&cuts);
        assert!(made.spans.is_empty());
        assert_eq!(made.notes.len(), 1);
        assert!(made.notes[0].contains("monotonic"), "{:?}", made.notes);
    }

    // --- The crosslines ---------------------------------------------

    /// The ten-by-ten L trace: the bend is a right angle, so the crossline
    /// crossing threshold is exactly 5 and works out by hand. A case whose
    /// answer is known without the code is the only one that can contradict it.
    fn ell() -> Vec<[f64; 2]> {
        vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]]
    }

    #[test]
    fn the_profiles_are_spaced_in_arc_length_and_reach_the_end() {
        let made = crosslines(&ell(), 5.0, 2.0).expect("an L at half-width 2 passes");
        // 0, 5, 10, 15 then the end: the trace measures 20, and a fence that
        // stops before the end suggests the trace stopped there.
        let at: Vec<f64> = made.lines.iter().map(|one| one.s).collect();
        assert_eq!(at, vec![0.0, 5.0, 10.0, 15.0, 20.0]);
    }

    #[test]
    fn the_last_profile_does_not_double_the_end() {
        // A 10.4-long trace at a spacing of 5: the multiple 10 is 0.4 from the
        // end, less than half a step. Keeping both would draw two superimposed
        // sections, which adds nothing and doubles the computation.
        let almost = vec![[0.0, 0.0], [10.4, 0.0]];
        let made = crosslines(&almost, 5.0, 1.0).expect("straight, so nothing crosses");
        let at: Vec<f64> = made.lines.iter().map(|one| one.s).collect();
        assert_eq!(at, vec![0.0, 5.0, 10.4]);
    }

    #[test]
    fn half_width_is_a_half_and_the_profile_is_perpendicular() {
        // Two classic mistakes at once: taking the half-width for a width (a
        // section twice too wide, which looks normal) and laying the crossline
        // along the trace instead of across it.
        let straight = vec![[0.0, 0.0], [10.0, 0.0]];
        let made = crosslines(&straight, 5.0, 3.0).expect("straight");
        let first = made.lines[0];
        let [x0, y0, x1, y1] = first.trace;
        assert!((x1 - x0).hypot(y1 - y0) - 6.0 < 1e-12, "the length is 2 × 3");
        // Perpendicular: the crossline is vertical where the trace is
        // horizontal, and it is **centred** on the trace.
        assert!((x0 - 0.0).abs() < 1e-12 && (x1 - 0.0).abs() < 1e-12);
        assert!((y0 + y1).abs() < 1e-12, "centred on the trace");
    }

    #[test]
    fn a_bend_tighter_than_the_half_width_is_refused_naming_a_half_width_that_would_pass() {
        // The threshold is 5: the bend's crosslines are at s = 10 and s = 15,
        // and the centre of the turn is 5 from each. Without this check, two
        // panels would run through each other and the eye would read the
        // intersection as a structure.
        let why = crosslines(&ell(), 5.0, 6.0).expect_err("6 > 5: it runs through itself");
        // The first pair that runs through itself is **s = 5 and s = 15**, not
        // the bend's pair: the crossline at s = 5 is vertical at x = 5 and the
        // one at s = 15 is horizontal at y = 5, and they cut at (5, 5) as soon
        // as the half-width reaches 5. That is what the first draft of this test
        // had wrong, and why the message no longer names a cause.
        assert!(why.contains("5.000") && why.contains("15.000"), "{why}");
        // The refusal says **how to get out of it**, and the figure is right.
        assert!(why.contains("A half-width under 5.000"), "{why}");
        // And just below the threshold it passes: the check is not a disguised
        // refusal of every bend.
        assert!(crosslines(&ell(), 5.0, 4.9).is_ok());
    }

    #[test]
    fn a_trace_folding_back_on_itself_is_caught_even_where_neighbours_are_tame() {
        // The hairpin: two parallel passes 4 apart. The neighbouring crosslines
        // are tame everywhere, the turn included; the ones running through each
        // other are far apart in s. Checking only the neighbours would pass here
        // while reading as "verified".
        let hairpin = vec![[0.0, 0.0], [20.0, 0.0], [20.0, 4.0], [0.0, 4.0]];
        let why = crosslines(&hairpin, 5.0, 3.0).expect_err("the two branches are 4 apart");
        // Both branches are horizontal, so **every** crossline is vertical: the
        // ones that overlap are carried by the same line, and a segment does not
        // "cut" another when it lies on it. This test found that hole in the
        // check.
        assert!(why.contains("A half-width under 2.000"), "{why}");
        // A half-width that fits inside the gap passes.
        assert!(crosslines(&hairpin, 5.0, 1.5).is_ok());
    }

    #[test]
    fn what_places_no_profile_is_refused_by_name_rather_than_returning_none() {
        // An empty list of crosslines draws a fence with nothing in it, which
        // looks like a fence whose surfaces do not cut.
        let straight = vec![[0.0, 0.0], [10.0, 0.0]];
        assert!(crosslines(&straight, 0.0, 1.0).is_err());
        assert!(crosslines(&straight, f64::NAN, 1.0).is_err());
        assert!(crosslines(&straight, 5.0, -1.0).is_err());
        assert!(crosslines(&[[0.0, 0.0]], 5.0, 1.0).is_err());
        // The same point twice: no tangent, hence no normal.
        assert!(crosslines(&[[3.0, 3.0], [3.0, 3.0]], 5.0, 1.0).is_err());
        // A point with no position cannot be perpendiculared either.
        assert!(crosslines(&[[0.0, 0.0], [f64::NAN, 1.0]], 5.0, 1.0).is_err());
    }
}
