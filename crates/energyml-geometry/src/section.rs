//! The vertical section along a trace — the geologist's profile.
//!
//! A polyline is drawn in plan view; each segment carries a vertical plane, and
//! the intersection of that plane with a surface's triangles gives the curves
//! the profile shows. The computation lives here — pure, no I/O — because it is
//! **domain vocabulary**: a desktop
//! interface will reuse it as it stands, and a test reaches it with no browser.
//!
//! # What comes out
//!
//! [`cut`] yields **segments** — the intersection of a plane and a triangle is
//! one — each of them twice: in 3D and in (s, z), from the same intersections,
//! so the two views cannot contradict each other. [`chain`] then welds them
//! into polylines, and the welding rule is precise ("never re-chain" was
//! too strong a rule): two segments join only on an
//! **exactly** shared end. Two neighbouring triangles compute their
//! intersection point on the **same edge** with the same interpolation — the
//! point is identical to the bit — whereas two sheets of a faulted surface
//! share no edge at all: they never weld, with no threshold deciding it. And a
//! chain whose two ends meet is **closed** — which is what a `PolylineSetPatch`
//! `ClosedPolylines` exists to say, and a ring (outer or inner) is only usable
//! if it is stated closed.
//!
//! # The rules
//!
//! - **A segment is bounded to its slice.** A plane is infinite; the trace is
//!   not. Every intersection is clipped to the `[0, length]` window of its
//!   trace segment — without that, a short trace beside a large surface would
//!   report the whole surface.
//! - **A holed triangle does not lie.** A `NaN` corner drops the triangle: an
//!   intersection computed through a NaN would produce NaN coordinates that
//!   propagate without a sound.
//! - **An empty profile is an answer**, not a failure: a trace that passes
//!   beside everything has nothing to show, and the caller says so with the
//!   counts.

/// One section segment: its two ends in 3D, and in (s, z).
#[derive(Clone, Debug, PartialEq)]
pub struct Cut {
    /// The two ends, in the frame of the input points.
    pub ends: [[f64; 3]; 2],
    /// The same, as curvilinear abscissa along the trace and depth.
    pub profile: [[f64; 2]; 2],
}

/// Where one end of a [`Cut`] comes from: the edge of the surface it lies on,
/// as `point a + t·(point b − point a)` — `a` and `b` are indices into the
/// `points` the cut was given, exactly as the triangles name them.
///
/// The arithmetic is the cut's own (`corners[a] + (corners[b] − corners[a])·t`),
/// so recomputing the end from its source gives the end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CutEnd {
    /// The first node of the edge.
    pub a: u32,
    /// The second node of the edge.
    pub b: u32,
    /// From `a` (0) to `b` (1).
    pub t: f64,
    /// The end was **clipped** to the trace's window: it lies inside the
    /// triangle, between the two edge crossings, not on an edge. The edge and
    /// `t` are then those of the nearer of the two crossings — an approximation
    /// the flag owns up to, never a claim that the point is on that edge.
    pub clipped: bool,
}

/// A [`Cut`] and where each of its ends comes from. The ends and the profile
/// are the very values [`cut`] gives: [`cut`] is this, without the sources.
#[derive(Clone, Debug, PartialEq)]
pub struct SourcedCut {
    /// The segment.
    pub cut: Cut,
    /// The source of `cut.ends[0]` and `cut.ends[1]`.
    pub sources: [CutEnd; 2],
}

/// The intersection of a triangulated surface with the vertical planes of a
/// trace in plan. `trace` is the `[x, y]` polyline of the top-down view.
pub fn cut(points: &[[f64; 3]], triangles: &[[u32; 3]], trace: &[[f64; 2]]) -> Vec<Cut> {
    cut_sourced(points, triangles, trace)
        .into_iter()
        .map(|one| one.cut)
        .collect()
}

/// [`cut`], each segment carrying the edge its ends lie on.
pub fn cut_sourced(points: &[[f64; 3]], triangles: &[[u32; 3]], trace: &[[f64; 2]]) -> Vec<SourcedCut> {
    let mut out = Vec::new();
    // The abscissa each trace segment starts at: the profile is continuous
    // along the whole polyline, not reset to zero at every bend.
    let mut offset = 0.0;
    for pair in trace.windows(2) {
        let [p, q] = [pair[0], pair[1]];
        let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
        let length = dx.hypot(dy);
        if length < 1e-12 {
            continue;
        }
        let (ux, uy) = (dx / length, dy / length);
        // The vertical plane's normal: perpendicular to the segment, in plan.
        let distance = |point: &[f64; 3]| -> f64 { (point[0] - p[0]) * -uy + (point[1] - p[1]) * ux };
        // A point's local abscissa, along the segment.
        let along = |point: &[f64; 3]| -> f64 { (point[0] - p[0]) * ux + (point[1] - p[1]) * uy };

        for triangle in triangles {
            let corners: Vec<&[f64; 3]> = triangle
                .iter()
                .filter_map(|&index| points.get(index as usize))
                .collect();
            if corners.len() != 3 || corners.iter().any(|c| c.iter().any(|v| !v.is_finite())) {
                continue;
            }
            let sides: Vec<f64> = corners.iter().map(|c| distance(c)).collect();
            // The edges that cross the plane: opposite signs at the two ends.
            let mut hits: Vec<[f64; 3]> = Vec::with_capacity(2);
            // The edge (node a, node b, t) each hit was computed on, in step.
            let mut edges: Vec<(u32, u32, f64)> = Vec::with_capacity(2);
            for (a, b) in [(0, 1), (1, 2), (2, 0)] {
                let (da, db) = (sides[a], sides[b]);
                if (da > 0.0 && db > 0.0) || (da < 0.0 && db < 0.0) {
                    continue;
                }
                let span = da - db;
                if span.abs() < 1e-12 {
                    continue;
                }
                let t = da / span;
                let hit = [
                    corners[a][0] + (corners[b][0] - corners[a][0]) * t,
                    corners[a][1] + (corners[b][1] - corners[a][1]) * t,
                    corners[a][2] + (corners[b][2] - corners[a][2]) * t,
                ];
                if !hits.iter().any(|seen: &[f64; 3]| {
                    (seen[0] - hit[0]).hypot(seen[1] - hit[1]).hypot(seen[2] - hit[2]) < 1e-9
                }) {
                    hits.push(hit);
                    edges.push((triangle[a], triangle[b], t));
                }
            }
            let [mut first, mut second] = match hits.as_slice() {
                [a, b] => [*a, *b],
                _ => continue,
            };
            let (mut edge_first, mut edge_second) = (edges[0], edges[1]);
            // Bounded to the slice: sorted by abscissa, clipped to [0, length].
            let (mut s0, mut s1) = (along(&first), along(&second));
            if s0 > s1 {
                std::mem::swap(&mut s0, &mut s1);
                std::mem::swap(&mut first, &mut second);
                std::mem::swap(&mut edge_first, &mut edge_second);
            }
            if s1 < 0.0 || s0 > length {
                continue;
            }
            let clip = |from: &[f64; 3], to: &[f64; 3], at: f64| -> [f64; 3] {
                if s1 - s0 < 1e-12 {
                    return *from;
                }
                let t = (at - s0) / (s1 - s0);
                [
                    from[0] + (to[0] - from[0]) * t,
                    from[1] + (to[1] - from[1]) * t,
                    from[2] + (to[2] - from[2]) * t,
                ]
            };
            let (c0, c1) = (s0.max(0.0), s1.min(length));
            let start = clip(&first, &second, c0);
            let end = clip(&first, &second, c1);
            // An end that was not clipped is its crossing; a clipped one lies
            // between the two, and takes the nearer's edge (see `CutEnd`).
            let source = |at: f64, clipped: bool| -> CutEnd {
                let (a, b, t) = if !clipped {
                    if at == s0 { edge_first } else { edge_second }
                } else if s1 - s0 < 1e-12 || (at - s0) / (s1 - s0) < 0.5 {
                    edge_first
                } else {
                    edge_second
                };
                CutEnd { a, b, t, clipped }
            };
            out.push(SourcedCut {
                cut: Cut {
                    ends: [start, end],
                    profile: [[offset + c0, start[2]], [offset + c1, end[2]]],
                },
                sources: [source(c0, c0 != s0), source(c1, c1 != s1)],
            });
        }
        offset += length;
    }
    out
}

/// How much work [`cut_many`] did — counted, so a test can prove the box
/// prefilter without a stopwatch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CutWork {
    /// The (trace segment, triangle) pairs a plain loop over every segment would
    /// have tested: triangles × segments of non-degenerate length.
    pub pairs: usize,
    /// The pairs that survived the box prefilter and were really cut.
    pub tested: usize,
}

/// [`cut`] along **several** traces in one pass — the section view cuts a surface
/// along its long section and every cross section (ADR-0015).
///
/// Each answer is **the same `Vec<Cut>` that [`cut`] gives for that trace, to the
/// bit**: the arithmetic is `cut`'s own, called segment by segment, and only the
/// triangles it is given differ. Those are the ones whose (x, y) box touches the
/// segment's box, widened by a margin that swallows rounding: a triangle whose
/// box is clear of the segment cannot give a point on it — its intersection
/// with the plane lies inside the triangle's box, and what is kept of that is
/// on the segment, so inside the segment's — which is what makes skipping it
/// invisible. The boxes of the triangles are computed once for all the traces.
pub fn cut_many<T: AsRef<[[f64; 2]]>>(
    points: &[[f64; 3]],
    triangles: &[[u32; 3]],
    traces: &[T],
) -> Vec<Vec<Cut>> {
    cut_many_counted(points, triangles, traces).0
}

/// [`cut_many`], and the count of work it did.
pub fn cut_many_counted<T: AsRef<[[f64; 2]]>>(
    points: &[[f64; 3]],
    triangles: &[[u32; 3]],
    traces: &[T],
) -> (Vec<Vec<Cut>>, CutWork) {
    let (sourced, work) = cut_many_sourced_counted(points, triangles, traces);
    let plain = sourced
        .into_iter()
        .map(|cuts| cuts.into_iter().map(|one| one.cut).collect())
        .collect();
    (plain, work)
}

/// [`cut_many`], each segment carrying the edge its ends lie on.
pub fn cut_many_sourced<T: AsRef<[[f64; 2]]>>(
    points: &[[f64; 3]],
    triangles: &[[u32; 3]],
    traces: &[T],
) -> Vec<Vec<SourcedCut>> {
    cut_many_sourced_counted(points, triangles, traces).0
}

/// [`cut_many_counted`], each segment carrying the edge its ends lie on. The
/// nodes of a source are indices into `points`, not into the triangles that
/// survived the prefilter.
pub fn cut_many_sourced_counted<T: AsRef<[[f64; 2]]>>(
    points: &[[f64; 3]],
    triangles: &[[u32; 3]],
    traces: &[T],
) -> (Vec<Vec<SourcedCut>>, CutWork) {
    // The (x, y) box of every triangle that `cut` could use: finite corners, all
    // three indices in range. The others give nothing in `cut` either.
    let boxes: Vec<Option<[f64; 4]>> = triangles
        .iter()
        .map(|triangle| {
            let mut bounds = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
            for &index in triangle {
                let corner = points.get(index as usize)?;
                if corner.iter().any(|v| !v.is_finite()) {
                    return None;
                }
                bounds[0] = bounds[0].min(corner[0]);
                bounds[1] = bounds[1].min(corner[1]);
                bounds[2] = bounds[2].max(corner[0]);
                bounds[3] = bounds[3].max(corner[1]);
            }
            Some(bounds)
        })
        .collect();

    let mut work = CutWork::default();
    let mut out = Vec::with_capacity(traces.len());
    let mut kept: Vec<[u32; 3]> = Vec::new();
    for trace in traces {
        let mut cuts = Vec::new();
        // The abscissa each segment starts at, accumulated exactly as `cut` does
        // (and a degenerate segment advances nothing, as there).
        let mut offset = 0.0;
        for pair in trace.as_ref().windows(2) {
            let [p, q] = [pair[0], pair[1]];
            let length = (q[0] - p[0]).hypot(q[1] - p[1]);
            if length < 1e-12 {
                continue;
            }
            // The margin: relative to the coordinates, so a Lambert-93 model
            // (six-figure numbers) is not asked for a tolerance below its own
            // rounding, and absolute where they are small.
            let scale = p.iter().chain(q.iter()).fold(1.0_f64, |m, v| m.max(v.abs()));
            let margin = 1e-9 * scale;
            let low = [p[0].min(q[0]) - margin, p[1].min(q[1]) - margin];
            let high = [p[0].max(q[0]) + margin, p[1].max(q[1]) + margin];
            kept.clear();
            for (triangle, bounds) in triangles.iter().zip(&boxes) {
                work.pairs += 1;
                let Some(b) = bounds else { continue };
                if b[2] < low[0] || b[0] > high[0] || b[3] < low[1] || b[1] > high[1] {
                    continue;
                }
                kept.push(*triangle);
            }
            work.tested += kept.len();
            for mut found in cut_sourced(points, &kept, &[p, q]) {
                // `cut` started this segment at 0: `0.0 + c` is `c`, so adding
                // the offset here is the very sum `cut` makes over a whole trace.
                found.cut.profile[0][0] += offset;
                found.cut.profile[1][0] += offset;
                cuts.push(found);
            }
            offset += length;
        }
        out.push(cuts);
    }
    (out, work)
}

/// A chained polyline: its nodes in order, and whether it closes.
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    /// The nodes, in the order they are walked.
    pub points: Vec<[f64; 3]>,
    /// The **same** nodes in (s, z), in the same order.
    ///
    /// Same rule as [`Cut`], and for the same reason: both views come out of
    /// the same intersection, so they cannot contradict each other. A second
    /// function re-chaining the profiles separately would end up diverging from
    /// the 3D chaining — and it is the chaining that carries the subtlety (the
    /// weld on an exactly shared end). `points[i]` and `profile[i]` are the
    /// same node.
    pub profile: Vec<[f64; 2]>,
    /// The two ends meet: a ring, to be stated as `ClosedPolylines`.
    pub closed: bool,
}

/// Weld a cut's segments into polylines.
///
/// Joined on an **exactly** shared end (the same point computed from the same
/// edge — see the module); a node where three or more segments meet stops the
/// chain, because choosing a branch would draw a path the cut does not state.
pub fn chain(cuts: &[Cut]) -> Vec<Chain> {
    chain_walk(cuts, None)
        .into_iter()
        .map(|(chain, _)| chain)
        .collect()
}

/// A [`Chain`] and the source of each of its nodes.
#[derive(Clone, Debug, PartialEq)]
pub struct SourcedChain {
    /// The chain, as [`chain`] gives it.
    pub chain: Chain,
    /// One source per node: `sources[i]` is the source of `chain.points[i]`.
    ///
    /// **A welded node keeps the source of the segment that was walked first**
    /// — the one the chain already held when the next segment met it. Two
    /// triangles compute a shared edge's point alike, to the bit, but may name
    /// the edge's two nodes in opposite order (`a, b, t` against `b, a, 1 − t`):
    /// both describe the same point.
    pub sources: Vec<CutEnd>,
}

/// [`chain`], keeping where each node comes from. The chains, their order and
/// their nodes are exactly [`chain`]'s: it is the same walk, one function.
pub fn chain_sourced(cuts: &[SourcedCut]) -> Vec<SourcedChain> {
    let plain: Vec<Cut> = cuts.iter().map(|one| one.cut.clone()).collect();
    let ends: Vec<[CutEnd; 2]> = cuts.iter().map(|one| one.sources).collect();
    chain_walk(&plain, Some(&ends))
        .into_iter()
        .map(|(chain, sources)| SourcedChain { chain, sources })
        .collect()
}

fn chain_walk(cuts: &[Cut], sources_of: Option<&[[CutEnd; 2]]>) -> Vec<(Chain, Vec<CutEnd>)> {
    // The ends, indexed by their bits: exact equality is the criterion, so the
    // key can be the binary representation.
    let key = |p: &[f64; 3]| -> [u64; 3] { [p[0].to_bits(), p[1].to_bits(), p[2].to_bits()] };
    let mut by_end: std::collections::HashMap<[u64; 3], Vec<usize>> = std::collections::HashMap::new();
    for (index, one) in cuts.iter().enumerate() {
        for end in &one.ends {
            by_end.entry(key(end)).or_default().push(index);
        }
    }
    let mut used = vec![false; cuts.len()];
    let mut out = Vec::new();
    for start in 0..cuts.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        // The two views advance **together**, node by node: `profile` follows
        // exactly the same walking decisions as `points`, which is the only way
        // to guarantee they describe the same polyline (see `Chain::profile`).
        let mut points = vec![cuts[start].ends[0], cuts[start].ends[1]];
        let mut profile = vec![cuts[start].profile[0], cuts[start].profile[1]];
        let mut sources: Vec<CutEnd> = sources_of.map(|all| all[start].to_vec()).unwrap_or_default();
        // Extend from both ends, as long as the node has **degree two**: the
        // segment one came from, and one single continuation. The degree is
        // measured over *all* segments, consumed ones included — a criterion
        // over the free ones alone would depend on the walking order, and a
        // three-way junction would be crossed as soon as two were consumed.
        for forward in [true, false] {
            loop {
                let tip = if forward {
                    *points.last().unwrap()
                } else {
                    points[0]
                };
                let at_tip = by_end.get(&key(&tip)).map(Vec::as_slice).unwrap_or(&[]);
                // Two ends in all, or nothing: a junction (≥ 3) is a stop —
                // choosing a branch would draw a path nobody stated.
                if at_tip.len() != 2 {
                    break;
                }
                let Some(&next) = at_tip.iter().find(|&&i| !used[i]) else {
                    break;
                };
                used[next] = true;
                let [a, b] = cuts[next].ends;
                // The end taken and **its** profile: the index that decides in
                // 3D decides in (s, z) too, so the two lists stay one path.
                let far = usize::from(key(&a) == key(&tip));
                let extend = if far == 1 { b } else { a };
                let extend_profile = cuts[next].profile[far];
                if forward {
                    points.push(extend);
                    profile.push(extend_profile);
                } else {
                    points.insert(0, extend);
                    profile.insert(0, extend_profile);
                }
                if let Some(all) = sources_of {
                    if forward {
                        sources.push(all[next][far]);
                    } else {
                        sources.insert(0, all[next][far]);
                    }
                }
            }
        }
        // Closed: the two ends are the same point. The node is not repeated —
        // closure is stated, not stored (the same rule as promoting rings).
        let closed = points.len() > 2 && key(points.first().unwrap()) == key(points.last().unwrap());
        if closed {
            points.pop();
            profile.pop();
            sources.pop();
        }
        out.push((
            Chain {
                points,
                profile,
                closed,
            },
            sources,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A horizontal square at z = 5: two triangles, from (0,0) to (10,10).
    fn a_flat_square() -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
        (
            vec![
                [0.0, 0.0, 5.0],
                [10.0, 0.0, 5.0],
                [10.0, 10.0, 5.0],
                [0.0, 10.0, 5.0],
            ],
            vec![[0, 1, 2], [0, 2, 3]],
        )
    }

    #[test]
    fn a_trace_across_a_flat_surface_cuts_at_its_height() {
        let (points, triangles) = a_flat_square();
        // Crosses the square through the middle, left to right.
        let cuts = cut(&points, &triangles, &[[-5.0, 5.0], [15.0, 5.0]]);
        assert!(!cuts.is_empty());
        for one in &cuts {
            // Everything is at z = 5, on the line y = 5.
            for end in &one.ends {
                assert!((end[2] - 5.0).abs() < 1e-9, "{end:?}");
                assert!((end[1] - 5.0).abs() < 1e-9, "{end:?}");
            }
            // The profile carries the same height.
            for profile in &one.profile {
                assert!((profile[1] - 5.0).abs() < 1e-9);
            }
        }
        // The extent in s covers the crossing: from s=5 (x=0) to s=15 (x=10).
        let s_min = cuts
            .iter()
            .flat_map(|c| c.profile)
            .map(|p| p[0])
            .fold(f64::MAX, f64::min);
        let s_max = cuts
            .iter()
            .flat_map(|c| c.profile)
            .map(|p| p[0])
            .fold(f64::MIN, f64::max);
        assert!((s_min - 5.0).abs() < 1e-9, "{s_min}");
        assert!((s_max - 15.0).abs() < 1e-9, "{s_max}");
    }

    #[test]
    fn the_cut_is_clipped_to_the_trace_not_the_whole_plane() {
        let (points, triangles) = a_flat_square();
        // The trace stops in the middle of the square: so does the cut.
        let cuts = cut(&points, &triangles, &[[-5.0, 5.0], [5.0, 5.0]]);
        let s_max = cuts
            .iter()
            .flat_map(|c| c.profile)
            .map(|p| p[0])
            .fold(f64::MIN, f64::max);
        assert!(
            (s_max - 10.0).abs() < 1e-9,
            "clipped at the trace's end, s = {s_max}"
        );
        let x_max = cuts
            .iter()
            .flat_map(|c| c.ends)
            .map(|e| e[0])
            .fold(f64::MIN, f64::max);
        assert!((x_max - 5.0).abs() < 1e-9, "bounded at x = 5, not 10: {x_max}");
    }

    #[test]
    fn a_sloped_surface_profiles_its_slope() {
        // z = x: the profile must rise linearly.
        let points = vec![
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 10.0],
            [10.0, 10.0, 10.0],
            [0.0, 10.0, 0.0],
        ];
        let triangles = vec![[0, 1, 2], [0, 2, 3]];
        let cuts = cut(&points, &triangles, &[[0.0, 5.0], [10.0, 5.0]]);
        for one in &cuts {
            for (end, profile) in one.ends.iter().zip(one.profile.iter()) {
                assert!((end[2] - end[0]).abs() < 1e-9, "z = x on this sheet");
                assert!((profile[1] - profile[0]).abs() < 1e-9, "and the profile says so");
            }
        }
    }

    #[test]
    fn a_trace_beside_everything_is_an_empty_answer_and_a_bent_trace_accumulates_s() {
        let (points, triangles) = a_flat_square();
        assert!(cut(&points, &triangles, &[[20.0, 20.0], [30.0, 20.0]]).is_empty());
        // A bend: the second segment starts at the accumulated abscissa, not zero.
        let cuts = cut(&points, &triangles, &[[5.0, -5.0], [5.0, 5.0], [15.0, 5.0]]);
        let s_min = cuts
            .iter()
            .flat_map(|c| c.profile)
            .map(|p| p[0])
            .fold(f64::MAX, f64::min);
        // The first segment (10 long) enters the square at y=0, s=5.
        assert!((s_min - 5.0).abs() < 1e-9, "{s_min}");
    }

    #[test]
    fn chaining_welds_shared_ends_and_keeps_gaps_apart() {
        let seg = |a: [f64; 3], b: [f64; 3]| Cut {
            ends: [a, b],
            profile: [[0.0, 0.0]; 2],
        };
        // Three segments following each other, then one isolated across a gap:
        // a chain of four nodes, and one of two — never welded together.
        let cuts = vec![
            seg([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            seg([1.0, 0.0, 0.0], [2.0, 0.0, 1.0]),
            seg([2.0, 0.0, 1.0], [3.0, 0.0, 1.0]),
            seg([10.0, 0.0, 5.0], [11.0, 0.0, 5.0]),
        ];
        let mut chains = chain(&cuts);
        chains.sort_by_key(|c| std::cmp::Reverse(c.points.len()));
        assert_eq!(chains.len(), 2);
        assert_eq!(chains[0].points.len(), 4);
        assert!(!chains[0].closed);
        assert_eq!(chains[1].points.len(), 2);
    }

    #[test]
    fn a_loop_closes_and_does_not_repeat_its_node() {
        let seg = |a: [f64; 3], b: [f64; 3]| Cut {
            ends: [a, b],
            profile: [[0.0, 0.0]; 2],
        };
        let cuts = vec![
            seg([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            seg([1.0, 0.0, 0.0], [1.0, 1.0, 0.0]),
            seg([1.0, 1.0, 0.0], [0.0, 0.0, 0.0]),
        ];
        let chains = chain(&cuts);
        assert_eq!(chains.len(), 1);
        assert!(chains[0].closed, "the two ends meet: a ring");
        // Three nodes, not four: closure is stated, not stored.
        assert_eq!(chains[0].points.len(), 3);
    }

    #[test]
    fn a_junction_of_three_stops_the_chain_rather_than_choosing() {
        let seg = |a: [f64; 3], b: [f64; 3]| Cut {
            ends: [a, b],
            profile: [[0.0, 0.0]; 2],
        };
        // Three segments share the same node: no chain crosses it — choosing a
        // branch would draw a path nobody stated.
        let hub = [0.0, 0.0, 0.0];
        let cuts = vec![
            seg([-1.0, 0.0, 0.0], hub),
            seg(hub, [1.0, 0.0, 0.0]),
            seg(hub, [0.0, 1.0, 0.0]),
        ];
        let chains = chain(&cuts);
        assert_eq!(chains.len(), 3, "three strands, none welded through the junction");
    }

    #[test]
    fn a_real_cut_chains_into_one_line_across_the_shared_edge() {
        // The square's two triangles share the edge (0,2): the cut computes the
        // same point on both sides, to the bit — which is the welding
        // criterion, checked on a real cut rather than on fabricated segments.
        let (points, triangles) = a_flat_square();
        let cuts = cut(&points, &triangles, &[[-5.0, 5.0], [15.0, 5.0]]);
        assert_eq!(cuts.len(), 2, "one segment per triangle");
        let chains = chain(&cuts);
        assert_eq!(chains.len(), 1, "welded on the shared edge");
        assert_eq!(chains[0].points.len(), 3);
        assert!(!chains[0].closed);
    }

    #[test]
    fn a_nan_corner_drops_its_triangle_not_the_answer() {
        let (mut points, triangles) = a_flat_square();
        points[3][2] = f64::NAN;
        let cuts = cut(&points, &triangles, &[[-5.0, 5.0], [15.0, 5.0]]);
        // The holed triangle is dropped; the other still answers.
        assert!(!cuts.is_empty());
        assert!(
            cuts.iter()
                .all(|c| c.ends.iter().all(|e| e.iter().all(|v| v.is_finite())))
        );
    }
}
