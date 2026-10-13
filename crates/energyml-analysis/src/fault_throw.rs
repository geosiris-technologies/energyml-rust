//! **Throwing a horizon across its faults** — the horizon triangulated with
//! the traces of the faults that cut it as constraints, torn along each trace,
//! and its hanging wall offset by a throw that dies out at both ends.
//!
//! Written for `fault-throw` in `energyml-build`: a step of the build plan of a synthetic model:
//! horizons triangulated with the fault trace as a constraint, then **torn**
//! along it — the trace nodes duplicated, the cut edges declared as a
//! `SplitEdgePatch`, the hanging-wall compartment lowered by the throw, which
//! decreases to zero at the ends of the fault. The
//! verb of the schema says it: `splits`, *« the fault has opened a pair of
//! fault lips in a horizon »* (RESQML TRG 2.2 §6.12 p. 176).
//!
//! Nothing here reads a package or knows a frame: the caller says which way is
//! up, and every position is a plain `[x, y, z]`.
//!
//! # What is done, in order
//!
//! 1. the horizon's nodes are triangulated on (x, y) — a Delaunay, the one
//!    `triangulate` decides ([`crate::triangulate_xy`]);
//! 2. each fault surface is intersected with that surface ([`crate::intersect`]):
//!    **its trace on this horizon**, one open line, or a refusal naming what
//!    was found instead — nothing, several pieces, a ring;
//! 3. the nodes are triangulated again with the traces as constraint edges
//!    ([`crate::triangulate_xy_constrained`]): no triangle straddles a trace,
//!    and each trace becomes a **chain of edges** — found here by walking the
//!    nodes that lie on it in the order of their abscissa, every consecutive
//!    pair required to be an edge;
//! 4. each chain is **torn**: every node of it but its two ends is duplicated,
//!    and the triangles on the hanging-wall side take the copy. The ends are
//!    kept single — the throw is zero there, so the two lips meet;
//! 5. the hanging wall is **offset**: a node on the hanging-wall side, at a
//!    distance `d` in plan from the trace and at the position `s` along it,
//!    moves by `throw(s) · (1 − d / reach)` — the full throw on the lip, none
//!    beyond `reach` — downward for a normal fault, upward for a reverse one;
//!    with `heave`, also horizontally by that amount over `tan(dip)`, so that
//!    the hanging-wall lip slides along a plane of the fault's mean dip rather
//!    than straight down. The footwall does not move.
//!
//! # The throw along the trace — a synthetic law, stated
//!
//! `throw(s) = maximum · profile(s)`, `s` the plan-view arc length along the
//! trace over its whole length, and `profile` zero at both ends and one in the
//! middle: **parabolic** `4 s (1 − s)`, or **elliptic** `√(1 − (2 s − 1)²)`.
//! Neither is a measurement; the requirement is a bell-shaped law,
//! synthetic and declared, and the caller names which.
//!
//! # Which side is the hanging wall
//!
//! The side of the trace toward which the fault **dips** — the block above
//! the fault plane. The dip is **measured** on the fault surface
//! ([`crate::fault_sticks::orientation`], area-weighted), never taken from a
//! stated `MeanAzimuth`: the surface is what the horizon is torn against. A
//! vertical fault has no hanging wall and is refused by name, as is a trace
//! that runs within 30° of the dip direction on average — the side it would
//! read is noise.
//!
//! # What is refused rather than decided
//!
//! Two traces that cross or touch (a crossing of faults is a tear with four
//! lips, not done here); a trace in several pieces or a ring; a chain edge on
//! the border of the horizon, or whose two triangles lie on one side; a chain
//! node whose triangles do not fall in two groups. Each names the fault and,
//! where it helps, the node.
//!
//! # Precision far from the origin
//!
//! Lutetia sits at x ≈ 651 000, y ≈ 6 861 000. Every computation is made
//! **relative to the first node of the horizon**, and the positions are put
//! back at the end; a node the offset does not move comes back bit for bit
//! (`(x − o) + o = x` when `x − o` is exact, which it is for two numbers
//! within a factor two of each other).

use std::collections::{HashMap, HashSet};

use crate::fault_sticks::{self, Orientation};
use crate::intersect::intersect;
use crate::triangulate::{triangulate_xy, triangulate_xy_constrained};

/// Two positions this close in plan are one — a millionth of a metre, a length
/// no survey states and far above the rounding of coordinates taken relative
/// to the horizon's first node.
pub const TOLERANCE: f64 = 1e-6;

/// Above this dip, in degrees, a fault is vertical: which side of it is the
/// hanging wall is not a fact of its surface. The threshold `fault-from-sticks`
/// says a vertical surface with.
pub const VERTICAL: f64 = 89.9;

/// Below this mean sine between the trace and the dip direction — 30° — the
/// side a trace reads as the hanging wall is refused rather than trusted.
pub(crate) const ACROSS: f64 = 0.5;

/// How the throw varies along the trace — zero at both ends, one in the middle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    /// `4 s (1 − s)`.
    Parabolic,
    /// `√(1 − (2 s − 1)²)`.
    Elliptic,
}

impl Profile {
    /// The fraction of the maximum throw at `s`, the position along the trace
    /// from 0 to 1; zero outside the open interval.
    pub fn at(self, s: f64) -> f64 {
        if !(s > 0.0 && s < 1.0) {
            return 0.0;
        }
        match self {
            Profile::Parabolic => 4.0 * s * (1.0 - s),
            Profile::Elliptic => (1.0 - (2.0 * s - 1.0).powi(2)).max(0.0).sqrt(),
        }
    }
}

/// Which way the hanging wall goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sense {
    /// Down — a normal fault.
    Down,
    /// Up — a reverse fault, or a thrust.
    Up,
}

/// A fault, as its surface.
#[derive(Clone, Copy, Debug)]
pub struct Fault<'a> {
    /// Its name, for a message.
    pub name: &'a str,
    /// The surface's nodes, in the horizon's frame.
    pub points: &'a [[f64; 3]],
    /// Its triangles, indices into `points`.
    pub triangles: &'a [[u32; 3]],
    /// The throw where it is largest, in the frame's vertical unit — reached
    /// only where the profile is one.
    pub maximum_throw: f64,
    /// Which way its hanging wall goes.
    pub sense: Sense,
}

/// What every fault is thrown with.
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    /// How the throw varies along each trace.
    pub profile: Profile,
    /// How far from the trace, in plan, the hanging wall is dragged: the
    /// offset falls linearly from the throw on the lip to zero at this distance.
    pub reach: f64,
    /// Move the hanging wall horizontally too, along the fault's mean dip.
    pub heave: bool,
    /// Whether the third coordinate grows upward (an elevation) or downward (a
    /// depth).
    pub up: bool,
}

/// The two lips of a fault on the torn horizon — node indices into
/// [`Torn::points`], both running the trace in the same direction, and sharing
/// their two ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lip {
    /// The footwall's lip: the trace's own nodes.
    pub footwall: Vec<u32>,
    /// The hanging wall's lip: the ends, and the copies between them.
    pub hanging_wall: Vec<u32>,
}

/// What one fault did.
#[derive(Clone, Debug, PartialEq)]
pub struct Thrown {
    /// Its surface's orientation, as measured.
    pub orientation: Orientation,
    /// Its lips.
    pub lip: Lip,
    /// The largest offset its lip received — below the maximum when no node
    /// falls where the profile is one.
    pub largest: f64,
    /// How many nodes it moved, its lip's copies included.
    pub moved: usize,
}

/// The torn horizon.
#[derive(Clone, Debug, PartialEq)]
pub struct Torn {
    /// Every node: the horizon's own (duplicates of an (x, y) dropped), then
    /// the traces' vertices, then the copies of the torn nodes.
    pub points: Vec<[f64; 3]>,
    /// Index triples, wound anticlockwise in plan as the triangulation made them.
    pub triangles: Vec<[u32; 3]>,
    /// One per fault, in the order given.
    pub faults: Vec<Thrown>,
    /// How many nodes the traces added to the horizon's.
    pub added: usize,
    /// How many of the horizon's nodes were dropped as duplicates of an (x, y).
    pub duplicates: usize,
    /// How many nodes were duplicated along the traces.
    pub split: usize,
    /// How many nodes moved.
    pub moved: usize,
    /// How many triangles the offset turned over in plan — a heave larger than
    /// a triangle does that, and it is said, not corrected.
    pub flipped: usize,
}

/// Why a horizon could not be thrown.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum TearError {
    /// A maximum throw that is not a positive length.
    #[error("the maximum throw of `{fault}` must be a finite number > 0, not {given}")]
    BadThrow {
        /// The fault.
        fault: String,
        /// What was given.
        given: f64,
    },
    /// A reach that is not a positive length.
    #[error("the reach must be a finite length > 0, not {given}")]
    BadReach {
        /// What was given.
        given: f64,
    },
    /// The horizon's nodes do not make a surface.
    #[error("the horizon: {why}")]
    Horizon {
        /// What the triangulation said.
        why: String,
    },
    /// A fault with no area.
    #[error("`{fault}` has no area — no triangle to measure its dip on")]
    NoSurface {
        /// The fault.
        fault: String,
    },
    /// A vertical fault.
    #[error(
        "`{fault}` is vertical (dip {dip:.2}°): a vertical fault has no hanging wall, and which of its sides goes down is not stated"
    )]
    Vertical {
        /// The fault.
        fault: String,
        /// Its measured dip.
        dip: f64,
    },
    /// A fault that misses the horizon.
    #[error("`{fault}` does not cut the horizon")]
    NoTrace {
        /// The fault.
        fault: String,
    },
    /// A fault that cuts it in several pieces.
    #[error(
        "`{fault}` cuts the horizon in {count} separate traces — which of them to tear, and how their throws meet, is not decided here"
    )]
    Pieces {
        /// The fault.
        fault: String,
        /// How many.
        count: usize,
    },
    /// A fault whose trace closes on itself.
    #[error("`{fault}` cuts the horizon along a closed ring — a ring has no end for its throw to die out at")]
    Ring {
        /// The fault.
        fault: String,
    },
    /// Two traces that meet.
    #[error("the traces of `{first}` and `{second}` cross or touch — tearing a horizon across a crossing of faults is not done here")]
    Crossing {
        /// One fault.
        first: String,
        /// The other.
        second: String,
    },
    /// A trace too close to the dip direction to tell the sides apart.
    #[error(
        "the trace of `{fault}` runs {angle:.0}° from its dip direction on average — within 30°, which side is the hanging wall cannot be read from it"
    )]
    AlongDip {
        /// The fault.
        fault: String,
        /// The mean angle, in degrees.
        angle: f64,
    },
    /// A chain that is not a path of edges.
    #[error("the trace of `{fault}` does not follow the triangulation's edges between nodes {from} and {to}")]
    NotAlong {
        /// The fault.
        fault: String,
        /// One node.
        from: u32,
        /// The next.
        to: u32,
    },
    /// A chain edge without a triangle on each side.
    #[error(
        "the edge {a}–{b} of the trace of `{fault}` does not have one triangle on each side — the trace runs along the horizon's border, or the surface folds there"
    )]
    Sides {
        /// The fault.
        fault: String,
        /// One end.
        a: u32,
        /// The other.
        b: u32,
    },
    /// A chain node whose triangles do not split in two.
    #[error("the triangles around node {node} of the trace of `{fault}` fall in {groups} group(s) where the trace makes two sides")]
    Fan {
        /// The fault.
        fault: String,
        /// The node.
        node: u32,
        /// How many groups.
        groups: usize,
    },
}

type Point = [f64; 3];

pub(crate) fn cross2(u: [f64; 2], v: [f64; 2]) -> f64 {
    u[0] * v[1] - u[1] * v[0]
}

pub(crate) fn plan(a: &Point, b: &Point) -> [f64; 2] {
    [b[0] - a[0], b[1] - a[1]]
}

pub(crate) fn length2(v: [f64; 2]) -> f64 {
    (v[0] * v[0] + v[1] * v[1]).sqrt()
}

/// The nearest point of a polyline to `p`, in plan: its abscissa along the
/// line, its distance, and the side of `p` — the sign of the cross product
/// with the segment it falls on.
pub(crate) fn project(line: &[Point], p: &Point) -> Option<(f64, f64, f64)> {
    let mut best: Option<(f64, f64, f64)> = None;
    let mut run = 0.0f64;
    for pair in line.windows(2) {
        let v = plan(&pair[0], &pair[1]);
        let w = plan(&pair[0], p);
        let long = v[0] * v[0] + v[1] * v[1];
        let len = long.sqrt();
        let t = if long > 0.0 { ((w[0] * v[0] + w[1] * v[1]) / long).clamp(0.0, 1.0) } else { 0.0 };
        let q = [pair[0][0] + t * v[0], pair[0][1] + t * v[1]];
        let d = ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt();
        if best.is_none_or(|(_, bd, _)| d < bd) {
            best = Some((run + t * len, d, cross2(v, w)));
        }
        run += len;
    }
    best
}

/// Plan-view length of a polyline.
pub(crate) fn plan_length(line: &[Point]) -> f64 {
    line.windows(2).map(|pair| length2(plan(&pair[0], &pair[1]))).sum()
}

/// Whether two segments meet in plan, ends included, within [`TOLERANCE`].
fn segments_meet(a: &Point, b: &Point, c: &Point, d: &Point) -> bool {
    let distance = |p: &Point, s: &Point, e: &Point| project(&[*s, *e], p).map_or(f64::INFINITY, |(_, d, _)| d);
    if distance(a, c, d) <= TOLERANCE || distance(b, c, d) <= TOLERANCE || distance(c, a, b) <= TOLERANCE || distance(d, a, b) <= TOLERANCE {
        return true;
    }
    let (ab, ac, ad) = (plan(a, b), plan(a, c), plan(a, d));
    let (cd, ca, cb) = (plan(c, d), plan(c, a), plan(c, b));
    let (s1, s2) = (cross2(ab, ac), cross2(ab, ad));
    let (s3, s4) = (cross2(cd, ca), cross2(cd, cb));
    s1 * s2 < 0.0 && s3 * s4 < 0.0
}

fn lines_meet(first: &[Point], second: &[Point]) -> bool {
    first.windows(2).any(|p| second.windows(2).any(|q| segments_meet(&p[0], &p[1], &q[0], &q[1])))
}

fn close3(a: &Point, b: &Point) -> bool {
    (a[0] - b[0]).abs() <= TOLERANCE && (a[1] - b[1]).abs() <= TOLERANCE && (a[2] - b[2]).abs() <= TOLERANCE
}

/// The pieces of an intersection, joined where their ends meet — a fault in
/// two patches cuts a horizon in two pieces that share an end to the
/// rounding, not to the bit, since each patch numbers its corners its own way.
pub(crate) fn weld(mut lines: Vec<Vec<Point>>) -> Vec<(Vec<Point>, bool)> {
    let mut out: Vec<(Vec<Point>, bool)> = Vec::new();
    while let Some(mut line) = lines.pop() {
        loop {
            let mut joined = false;
            for at in 0..lines.len() {
                let other = &lines[at];
                let (first, last) = (line[0], line[line.len() - 1]);
                let (o_first, o_last) = (other[0], other[other.len() - 1]);
                let merged = if close3(&last, &o_first) {
                    Some(line.iter().copied().chain(other.iter().skip(1).copied()).collect::<Vec<_>>())
                } else if close3(&last, &o_last) {
                    Some(line.iter().copied().chain(other.iter().rev().skip(1).copied()).collect())
                } else if close3(&first, &o_last) {
                    Some(other.iter().copied().chain(line.iter().skip(1).copied()).collect())
                } else if close3(&first, &o_first) {
                    Some(other.iter().rev().copied().chain(line.iter().skip(1).copied()).collect())
                } else {
                    None
                };
                if let Some(merged) = merged {
                    line = merged;
                    lines.remove(at);
                    joined = true;
                    break;
                }
            }
            if !joined {
                break;
            }
        }
        let closed = line.len() > 2 && close3(&line[0], &line[line.len() - 1]);
        out.push((line, closed));
    }
    out
}

/// A trace vertex that falls on a node of the horizon, to [`TOLERANCE`] in
/// plan, takes the node's position: the constrained triangulation then finds
/// the node rather than inserting a second vertex beside it.
fn snap(line: Vec<Point>, cloud: &[Point]) -> Vec<Point> {
    line.into_iter()
        .map(|p| {
            cloud
                .iter()
                .find(|q| (q[0] - p[0]).abs() <= TOLERANCE && (q[1] - p[1]).abs() <= TOLERANCE)
                .copied()
                .unwrap_or(p)
        })
        .collect()
}

fn key(a: u32, b: u32) -> (u32, u32) {
    if a < b { (a, b) } else { (b, a) }
}

/// Every edge of a triangulation, and the triangles on it.
fn edges_of(triangles: &[[u32; 3]]) -> HashMap<(u32, u32), Vec<usize>> {
    let mut out: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (at, t) in triangles.iter().enumerate() {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            out.entry(key(a, b)).or_default().push(at);
        }
    }
    out
}

/// The nodes of the triangulation that lie on the trace, in the order of their
/// abscissa along it — and every consecutive pair an edge, or the refusal.
fn chain_along(points: &[Point], trace: &[Point], edges: &HashMap<(u32, u32), Vec<usize>>, fault: &str) -> Result<Vec<u32>, TearError> {
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for p in trace {
        for axis in 0..2 {
            lo[axis] = lo[axis].min(p[axis]);
            hi[axis] = hi[axis].max(p[axis]);
        }
    }
    let mut on: Vec<(f64, u32)> = Vec::new();
    for (at, p) in points.iter().enumerate() {
        if p[0] < lo[0] - TOLERANCE || p[0] > hi[0] + TOLERANCE || p[1] < lo[1] - TOLERANCE || p[1] > hi[1] + TOLERANCE {
            continue;
        }
        if let Some((abscissa, d, _)) = project(trace, p)
            && d <= TOLERANCE
        {
            on.push((abscissa, at as u32));
        }
    }
    on.sort_by(|a, b| a.0.total_cmp(&b.0));
    if on.len() < 2 {
        return Err(TearError::NoTrace { fault: fault.to_string() });
    }
    for pair in on.windows(2) {
        if !edges.contains_key(&key(pair[0].1, pair[1].1)) {
            return Err(TearError::NotAlong { fault: fault.to_string(), from: pair[0].1, to: pair[1].1 });
        }
    }
    Ok(on.into_iter().map(|(_, node)| node).collect())
}

/// The sign of the hanging-wall side of a chain walked forward: the side the
/// dip direction points to, on average over its edges.
fn hanging_wall_side(points: &[Point], chain: &[u32], dip_azimuth: f64, fault: &str) -> Result<f64, TearError> {
    let direction = [dip_azimuth.to_radians().sin(), dip_azimuth.to_radians().cos()];
    let (mut sum, mut total) = (0.0f64, 0.0f64);
    for pair in chain.windows(2) {
        let v = plan(&points[pair[0] as usize], &points[pair[1] as usize]);
        sum += cross2(v, direction);
        total += length2(v);
    }
    let mean = if total > 0.0 { sum / total } else { 0.0 };
    if mean.abs() < ACROSS {
        return Err(TearError::AlongDip { fault: fault.to_string(), angle: mean.abs().clamp(0.0, 1.0).asin().to_degrees() });
    }
    Ok(mean.signum())
}

/// The side of `c` from the directed edge `a → b`, in plan.
fn side(points: &[Point], a: u32, b: u32, c: u32) -> f64 {
    let (a, b, c) = (&points[a as usize], &points[b as usize], &points[c as usize]);
    cross2(plan(a, b), plan(a, c))
}

/// The hanging-wall and footwall triangles of a chain edge.
fn sides_of(points: &[Point], triangles: &[[u32; 3]], edges: &HashMap<(u32, u32), Vec<usize>>, a: u32, b: u32, sigma: f64, fault: &str) -> Result<(usize, usize), TearError> {
    let refuse = || TearError::Sides { fault: fault.to_string(), a, b };
    let Some(on) = edges.get(&key(a, b)) else { return Err(refuse()) };
    let [one, two] = on.as_slice() else { return Err(refuse()) };
    let third = |t: usize| triangles[t].iter().copied().find(|&v| v != a && v != b).unwrap_or(a);
    let (s1, s2) = (side(points, a, b, third(*one)), side(points, a, b, third(*two)));
    if s1 * sigma > 0.0 && s2 * sigma < 0.0 {
        Ok((*one, *two))
    } else if s2 * sigma > 0.0 && s1 * sigma < 0.0 {
        Ok((*two, *one))
    } else {
        Err(refuse())
    }
}

fn find(parent: &mut [usize], mut at: usize) -> usize {
    while parent[at] != at {
        parent[at] = parent[parent[at]];
        at = parent[at];
    }
    at
}

/// Tear the triangulation along one chain: each node but the ends gets a copy,
/// which the triangles of the hanging-wall side take. Returns the copies, in
/// the chain's order.
fn tear(points: &mut Vec<Point>, triangles: &mut [[u32; 3]], chain: &[u32], sigma: f64, fault: &str) -> Result<Vec<u32>, TearError> {
    let edges = edges_of(triangles);
    let mut sides: Vec<(usize, usize)> = Vec::with_capacity(chain.len().saturating_sub(1));
    for pair in chain.windows(2) {
        sides.push(sides_of(points, triangles, &edges, pair[0], pair[1], sigma, fault)?);
    }
    let mut incident: HashMap<u32, Vec<usize>> = HashMap::new();
    let interior: HashSet<u32> = chain[1..chain.len() - 1].iter().copied().collect();
    for (at, t) in triangles.iter().enumerate() {
        for v in t {
            if interior.contains(v) {
                incident.entry(*v).or_default().push(at);
            }
        }
    }
    // Decide every group first, then write: a triangle may touch two nodes of
    // the chain, and the second decision must see the triangles as they were.
    let mut moves: Vec<(u32, Vec<usize>)> = Vec::new();
    for at in 1..chain.len() - 1 {
        let (before, node, after) = (chain[at - 1], chain[at], chain[at + 1]);
        let around = incident.get(&node).cloned().unwrap_or_default();
        let mut parent: Vec<usize> = (0..around.len()).collect();
        let mut by_edge: HashMap<u32, Vec<usize>> = HashMap::new();
        for (rank, t) in around.iter().enumerate() {
            for w in triangles[*t] {
                if w != node {
                    by_edge.entry(w).or_default().push(rank);
                }
            }
        }
        for (w, ranks) in &by_edge {
            if *w == before || *w == after {
                continue;
            }
            if let [one, two] = ranks.as_slice() {
                let (x, y) = (find(&mut parent, *one), find(&mut parent, *two));
                parent[x] = y;
            }
        }
        let roots: HashSet<usize> = (0..around.len()).map(|rank| find(&mut parent, rank)).collect();
        if roots.len() != 2 {
            return Err(TearError::Fan { fault: fault.to_string(), node, groups: roots.len() });
        }
        let group_of = |t: usize, parent: &mut Vec<usize>| around.iter().position(|one| *one == t).map(|rank| find(parent, rank));
        let (hw_before, fw_before) = sides[at - 1];
        let (hw_after, fw_after) = sides[at];
        let hw = group_of(hw_after, &mut parent);
        let consistent = hw.is_some()
            && group_of(hw_before, &mut parent) == hw
            && group_of(fw_before, &mut parent) != hw
            && group_of(fw_after, &mut parent) != hw;
        if !consistent {
            return Err(TearError::Fan { fault: fault.to_string(), node, groups: 2 });
        }
        let hw = hw.unwrap_or_default();
        let taken: Vec<usize> = around.iter().enumerate().filter(|(rank, _)| find(&mut parent, *rank) == hw).map(|(_, t)| *t).collect();
        moves.push((node, taken));
    }
    let mut copies = Vec::with_capacity(moves.len());
    for (node, taken) in moves {
        let copy = points.len() as u32;
        points.push(points[node as usize]);
        for t in taken {
            for v in triangles[t].iter_mut() {
                if *v == node {
                    *v = copy;
                }
            }
        }
        copies.push(copy);
    }
    Ok(copies)
}

/// Throw a horizon across its faults — see the module.
///
/// `cloud` is the horizon's nodes, every one a finite position; a hole in a
/// lattice is the caller's to refuse or to set aside, since a triangulation
/// over the rest would span it.
pub fn throw(cloud: &[Point], faults: &[Fault<'_>], settings: &Settings) -> Result<Torn, TearError> {
    if !(settings.reach.is_finite() && settings.reach > 0.0) {
        return Err(TearError::BadReach { given: settings.reach });
    }
    for fault in faults {
        if !(fault.maximum_throw.is_finite() && fault.maximum_throw > 0.0) {
            return Err(TearError::BadThrow { fault: fault.name.to_string(), given: fault.maximum_throw });
        }
    }
    let Some(origin) = cloud.first().copied() else {
        return Err(TearError::Horizon { why: "it has no node".to_string() });
    };
    if let Some(at) = cloud.iter().position(|p| !p.iter().all(|c| c.is_finite())) {
        return Err(TearError::Horizon { why: format!("node {at} is not a finite position") });
    }
    let local = |p: &Point| [p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]];
    let nodes: Vec<Point> = cloud.iter().map(local).collect();
    let flat: Vec<f64> = nodes.iter().flatten().copied().collect();
    let first: Vec<[u32; 3]> =
        triangulate_xy(&flat).map_err(|why| TearError::Horizon { why })?.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect();

    // -- each fault: its dip, and its trace on this horizon ----------------------
    let mut orientations: Vec<Orientation> = Vec::with_capacity(faults.len());
    let mut traces: Vec<Vec<Point>> = Vec::with_capacity(faults.len());
    for fault in faults {
        let surface: Vec<Point> = fault.points.iter().map(local).collect();
        let orientation =
            fault_sticks::orientation(&surface, fault.triangles, settings.up).ok_or_else(|| TearError::NoSurface { fault: fault.name.to_string() })?;
        if orientation.dip > VERTICAL {
            return Err(TearError::Vertical { fault: fault.name.to_string(), dip: orientation.dip });
        }
        let cut = intersect(&surface, fault.triangles, &nodes, &first);
        let mut pieces = weld(cut.traces.into_iter().map(|trace| trace.points).filter(|line| line.len() >= 2).collect());
        let (line, closed) = match pieces.len() {
            0 => return Err(TearError::NoTrace { fault: fault.name.to_string() }),
            1 => pieces.remove(0),
            count => return Err(TearError::Pieces { fault: fault.name.to_string(), count }),
        };
        if closed {
            return Err(TearError::Ring { fault: fault.name.to_string() });
        }
        traces.push(snap(line, &nodes));
        orientations.push(orientation);
    }
    for i in 0..traces.len() {
        for j in i + 1..traces.len() {
            if lines_meet(&traces[i], &traces[j]) {
                return Err(TearError::Crossing { first: faults[i].name.to_string(), second: faults[j].name.to_string() });
            }
        }
    }

    // -- the triangulation with the traces as constraints -------------------------
    let made = triangulate_xy_constrained(&flat, &traces).map_err(|why| TearError::Horizon { why })?;
    let mut points = made.points;
    let mut triangles: Vec<[u32; 3]> = made.triangles.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect();
    let edges = edges_of(&triangles);
    let mut chains: Vec<Vec<u32>> = Vec::with_capacity(faults.len());
    for (trace, fault) in traces.iter().zip(faults) {
        chains.push(chain_along(&points, trace, &edges, fault.name)?);
    }

    // -- the tears --------------------------------------------------------------------
    let mut sigmas: Vec<f64> = Vec::with_capacity(faults.len());
    let mut copies: Vec<Vec<u32>> = Vec::with_capacity(faults.len());
    for ((chain, fault), orientation) in chains.iter().zip(faults).zip(&orientations) {
        let sigma = hanging_wall_side(&points, chain, orientation.dip_azimuth, fault.name)?;
        copies.push(tear(&mut points, &mut triangles, chain, sigma, fault.name)?);
        sigmas.push(sigma);
    }
    let split: usize = copies.iter().map(Vec::len).sum();

    // -- the offset ----------------------------------------------------------------------
    let before = points.clone();
    let mut shift = vec![[0.0f64; 3]; points.len()];
    let mut touched = vec![false; points.len()];
    let mut thrown: Vec<Thrown> = Vec::with_capacity(faults.len());
    for (k, fault) in faults.iter().enumerate() {
        let chain = &chains[k];
        let line: Vec<Point> = chain.iter().map(|&v| before[v as usize]).collect();
        let total = plan_length(&line);
        let mut abscissa = vec![0.0f64; chain.len()];
        for at in 1..chain.len() {
            abscissa[at] = abscissa[at - 1] + length2(plan(&line[at - 1], &line[at]));
        }
        let orientation = orientations[k];
        let sense = match fault.sense {
            Sense::Down => 1.0,
            Sense::Up => -1.0,
        };
        // Per unit of throw: down (or up) the vertical, and along the dip.
        let vertical = if settings.up { -sense } else { sense };
        let horizontal = if settings.heave {
            let spread = sense / orientation.dip.to_radians().tan();
            [spread * orientation.dip_azimuth.to_radians().sin(), spread * orientation.dip_azimuth.to_radians().cos()]
        } else {
            [0.0, 0.0]
        };
        let own: HashSet<u32> = chain.iter().copied().collect();
        let copy_at: HashMap<u32, usize> = copies[k].iter().enumerate().map(|(rank, copy)| (*copy, rank + 1)).collect();
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for p in &line {
            for axis in 0..2 {
                lo[axis] = lo[axis].min(p[axis] - settings.reach);
                hi[axis] = hi[axis].max(p[axis] + settings.reach);
            }
        }
        let mut largest = 0.0f64;
        let mut moved = 0usize;
        for (node, p) in before.iter().enumerate() {
            let node32 = node as u32;
            let amount = if let Some(&rank) = copy_at.get(&node32) {
                let amount = fault.maximum_throw * settings.profile.at(abscissa[rank] / total);
                largest = largest.max(amount);
                amount
            } else if own.contains(&node32) {
                0.0
            } else {
                if p[0] < lo[0] || p[0] > hi[0] || p[1] < lo[1] || p[1] > hi[1] {
                    continue;
                }
                match project(&line, p) {
                    Some((at, d, s)) if s * sigmas[k] > 0.0 && d < settings.reach => {
                        fault.maximum_throw * settings.profile.at(at / total) * (1.0 - d / settings.reach)
                    }
                    _ => 0.0,
                }
            };
            if amount > 0.0 {
                shift[node][0] += amount * horizontal[0];
                shift[node][1] += amount * horizontal[1];
                shift[node][2] += amount * vertical;
                touched[node] = true;
                moved += 1;
            }
        }
        let hanging_wall: Vec<u32> =
            std::iter::once(chain[0]).chain(copies[k].iter().copied()).chain(std::iter::once(chain[chain.len() - 1])).collect();
        thrown.push(Thrown { orientation, lip: Lip { footwall: chain.clone(), hanging_wall }, largest, moved });
    }
    for (p, d) in points.iter_mut().zip(&shift) {
        p[0] += d[0];
        p[1] += d[1];
        p[2] += d[2];
    }
    let flipped = crate::nodes::flipped(&before, &points, &triangles);
    let moved = touched.iter().filter(|one| **one).count();
    let world: Vec<Point> = points.iter().map(|p| [p[0] + origin[0], p[1] + origin[1], p[2] + origin[2]]).collect();
    Ok(Torn { points: world, triangles, faults: thrown, added: made.added, duplicates: made.duplicates, split, moved, flipped })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where a real model sits — the tests run at map coordinates, since a defect once
    /// only showed far from the origin.
    const X0: f64 = 651_000.0;
    const Y0: f64 = 6_861_000.0;

    /// A flat horizon at elevation `z`, a lattice of `n × n` nodes `step`
    /// apart, its corner at the origin.
    fn horizon(n: usize, step: f64, z: f64) -> Vec<Point> {
        let mut out = Vec::new();
        for j in 0..n {
            for i in 0..n {
                out.push([X0 + i as f64 * step, Y0 + j as f64 * step, z]);
            }
        }
        out
    }

    /// A plane fault striking north (along y), through x = `x` at elevation
    /// `z0`, dipping `dip`° toward the east (azimuth 90°), from `top` down to
    /// `bottom`, from `y_from` to `y_to`.
    fn fault(x: f64, z0: f64, dip: f64, top: f64, bottom: f64, y_from: f64, y_to: f64) -> (Vec<Point>, Vec<[u32; 3]>) {
        let at = |z: f64| X0 + x + (z0 - z) / dip.to_radians().tan();
        let points = vec![
            [at(top), Y0 + y_from, top],
            [at(top), Y0 + y_to, top],
            [at(bottom), Y0 + y_from, bottom],
            [at(bottom), Y0 + y_to, bottom],
        ];
        (points, vec![[0, 1, 2], [1, 3, 2]])
    }

    fn settings(reach: f64, heave: bool) -> Settings {
        Settings { profile: Profile::Parabolic, reach, heave, up: true }
    }

    /// A fault dying out inside a 21 × 21 horizon 10 m apart: from y = 45 to
    /// y = 155, through x = 105 at the horizon's level — between two columns of
    /// nodes, so that no edge of the horizon lies in the fault plane.
    fn thrown(throw: f64, reach: f64, heave: bool) -> Torn {
        let cloud = horizon(21, 10.0, -50.0);
        let (points, triangles) = fault(105.0, -50.0, 60.0, 0.0, -100.0, 45.0, 155.0);
        let one = Fault { name: "F1", points: &points, triangles: &triangles, maximum_throw: throw, sense: Sense::Down };
        super::throw(&cloud, &[one], &settings(reach, heave)).expect("thrown")
    }

    #[test]
    fn the_profiles_are_zero_at_the_ends_and_one_in_the_middle() {
        for profile in [Profile::Parabolic, Profile::Elliptic] {
            assert_eq!(profile.at(0.0), 0.0);
            assert_eq!(profile.at(1.0), 0.0);
            assert_eq!(profile.at(-0.2), 0.0);
            assert!((profile.at(0.5) - 1.0).abs() < 1e-15, "{profile:?}");
        }
        assert!((Profile::Parabolic.at(0.25) - 0.75).abs() < 1e-15);
        assert!((Profile::Elliptic.at(0.25) - 0.75f64.sqrt()).abs() < 1e-15);
    }

    /// The footwall's lip is the trace: every node of it lies on the fault
    /// plane, at the horizon's level, and stays there; the hanging wall's lip
    /// shares its two ends and nothing else.
    #[test]
    fn the_lips_share_their_ends_and_nothing_else() {
        let torn = thrown(10.0, 50.0, false);
        let lip = &torn.faults[0].lip;
        assert_eq!(lip.footwall.len(), lip.hanging_wall.len());
        assert_eq!(lip.footwall.first(), lip.hanging_wall.first());
        assert_eq!(lip.footwall.last(), lip.hanging_wall.last());
        let inner_fw: HashSet<u32> = lip.footwall[1..lip.footwall.len() - 1].iter().copied().collect();
        assert!(lip.hanging_wall[1..lip.hanging_wall.len() - 1].iter().all(|v| !inner_fw.contains(v)));
        assert_eq!(torn.split, lip.footwall.len() - 2);
        for &v in &lip.footwall {
            let p = torn.points[v as usize];
            assert!((p[0] - (X0 + 105.0)).abs() < 1e-6, "on the fault at the horizon's level: {p:?}");
            assert!((p[2] + 50.0).abs() < 1e-9, "the footwall does not move: {p:?}");
        }
        // The trace runs where the fault meets the horizon, 45 → 155 in y.
        let ys: Vec<f64> = lip.footwall.iter().map(|v| torn.points[*v as usize][1] - Y0).collect();
        assert!((ys[0] - 45.0).abs() < 1e-6 && (ys[ys.len() - 1] - 155.0).abs() < 1e-6, "{ys:?}");
    }

    /// The offset on the hanging wall's lip is the profile, confronted with an
    /// independent reading: the drop of each copy against its footwall twin,
    /// read off the positions, and the parabola evaluated at the twin's `y`.
    #[test]
    fn the_hanging_wall_lip_drops_by_the_profile() {
        let torn = thrown(10.0, 50.0, false);
        let lip = &torn.faults[0].lip;
        let mut largest = 0.0f64;
        for (fw, hw) in lip.footwall.iter().zip(&lip.hanging_wall) {
            let (f, h) = (torn.points[*fw as usize], torn.points[*hw as usize]);
            assert_eq!([f[0], f[1]], [h[0], h[1]], "no heave: straight down");
            let s = (f[1] - (Y0 + 45.0)) / 110.0;
            let expected = 10.0 * 4.0 * s * (1.0 - s);
            assert!(((f[2] - h[2]) - expected).abs() < 1e-9, "at s = {s}: dropped {} for {expected}", f[2] - h[2]);
            largest = largest.max(f[2] - h[2]);
        }
        assert!((torn.faults[0].largest - largest).abs() < 1e-12);
        assert!(largest > 9.9, "a node sits near the middle: {largest}");
    }

    /// Every triangle keeps its side: the ones east of the trace (the hanging
    /// wall, since the fault dips east) name no footwall node of the lip but
    /// its ends, the ones west name no copy.
    #[test]
    fn each_triangle_takes_the_lip_of_its_side() {
        let torn = thrown(10.0, 50.0, false);
        let lip = &torn.faults[0].lip;
        let fw: HashSet<u32> = lip.footwall[1..lip.footwall.len() - 1].iter().copied().collect();
        let hw: HashSet<u32> = lip.hanging_wall[1..lip.hanging_wall.len() - 1].iter().copied().collect();
        let mut east = 0usize;
        let mut west = 0usize;
        for t in &torn.triangles {
            let centre = t.iter().map(|v| torn.points[*v as usize][0]).sum::<f64>() / 3.0 - (X0 + 105.0);
            let names_fw = t.iter().any(|v| fw.contains(v));
            let names_hw = t.iter().any(|v| hw.contains(v));
            if centre > 0.0 {
                assert!(!names_fw, "a hanging-wall triangle names a footwall node: {t:?}");
                east += names_hw as usize;
            } else {
                assert!(!names_hw, "a footwall triangle names a copy: {t:?}");
                west += names_fw as usize;
            }
        }
        assert!(east > 0 && west > 0, "the lips are used: {east} east, {west} west");
    }

    /// The drag: the offset falls with the distance to the trace and is gone
    /// at `reach`; the footwall never moves.
    #[test]
    fn the_offset_falls_to_zero_at_the_reach_and_the_footwall_stays() {
        let cloud = horizon(21, 10.0, -50.0);
        let torn = thrown(10.0, 50.0, false);
        // The horizon's own nodes come first, in order: a lattice has no
        // duplicate.
        assert_eq!(torn.duplicates, 0);
        for (at, p) in cloud.iter().enumerate() {
            let q = torn.points[at];
            let (dx, y) = (p[0] - (X0 + 105.0), p[1] - Y0);
            if dx <= 0.0 || dx >= 50.0 || !(45.0..=155.0).contains(&y) {
                assert_eq!(p, &q, "node {at} is out of the drag and moved");
            } else {
                let s = (y - 45.0) / 110.0;
                let expected = 10.0 * 4.0 * s * (1.0 - s) * (1.0 - dx / 50.0);
                assert!(((p[2] - q[2]) - expected).abs() < 1e-9, "node {at}: {} for {expected}", p[2] - q[2]);
            }
        }
    }

    /// With the heave, the hanging-wall lip slides along the fault plane: each
    /// copy lands on it again, `throw / tan(60°)` east of its twin.
    #[test]
    fn with_the_heave_the_hanging_wall_lip_stays_on_the_fault() {
        let torn = thrown(10.0, 50.0, true);
        let lip = &torn.faults[0].lip;
        let plane_x = |z: f64| X0 + 105.0 + (-50.0 - z) / 60f64.to_radians().tan();
        for hw in &lip.hanging_wall {
            let h = torn.points[*hw as usize];
            assert!((h[0] - plane_x(h[2])).abs() < 1e-6, "off the fault: {h:?}");
        }
        assert_eq!(torn.flipped, 0, "a heave of 5.8 m over a drag of 50 m folds nothing");
    }

    /// A reverse fault lifts its hanging wall; a depth frame counts the other
    /// way — both flip the sign of the vertical offset, not its size.
    #[test]
    fn the_sense_and_the_frame_set_the_sign() {
        let cloud = horizon(21, 10.0, -50.0);
        let (points, triangles) = fault(105.0, -50.0, 60.0, 0.0, -100.0, 45.0, 155.0);
        let mut drops = Vec::new();
        for (sense, up) in [(Sense::Down, true), (Sense::Up, true)] {
            let one = Fault { name: "F", points: &points, triangles: &triangles, maximum_throw: 10.0, sense };
            let torn = super::throw(&cloud, &[one], &Settings { up, ..settings(50.0, false) }).expect("thrown");
            let lip = &torn.faults[0].lip;
            let middle = lip.footwall.len() / 2;
            drops.push(torn.points[lip.footwall[middle] as usize][2] - torn.points[lip.hanging_wall[middle] as usize][2]);
        }
        assert!(drops[0] > 0.0 && (drops[0] + drops[1]).abs() < 1e-9, "{drops:?}");
        // In a depth frame the same fault, drawn in depths, goes down by a
        // larger depth.
        let deep: Vec<Point> = cloud.iter().map(|p| [p[0], p[1], 50.0]).collect();
        let points_deep: Vec<Point> = points.iter().map(|p| [p[0], p[1], -p[2]]).collect();
        let one = Fault { name: "F", points: &points_deep, triangles: &triangles, maximum_throw: 10.0, sense: Sense::Down };
        let torn = super::throw(&deep, &[one], &Settings { up: false, ..settings(50.0, false) }).expect("thrown");
        let lip = &torn.faults[0].lip;
        let middle = lip.footwall.len() / 2;
        let deeper = torn.points[lip.hanging_wall[middle] as usize][2] - torn.points[lip.footwall[middle] as usize][2];
        assert!((deeper - drops[0]).abs() < 1e-9, "{deeper} for {}", drops[0]);
    }

    #[test]
    fn what_cannot_be_torn_is_refused_by_name() {
        let cloud = horizon(21, 10.0, -50.0);
        // Vertical.
        let (points, triangles) = fault(105.0, -50.0, 90.0, 0.0, -100.0, 45.0, 155.0);
        let one = Fault { name: "F3", points: &points, triangles: &triangles, maximum_throw: 5.0, sense: Sense::Down };
        assert!(matches!(super::throw(&cloud, &[one], &settings(50.0, false)), Err(TearError::Vertical { .. })));
        // Above the horizon.
        let (points, triangles) = fault(105.0, -50.0, 60.0, 0.0, -40.0, 45.0, 155.0);
        let one = Fault { name: "F2", points: &points, triangles: &triangles, maximum_throw: 5.0, sense: Sense::Down };
        assert_eq!(super::throw(&cloud, &[one], &settings(50.0, false)), Err(TearError::NoTrace { fault: "F2".into() }));
        // Two faults crossing.
        let (a, ta) = fault(105.0, -50.0, 60.0, 0.0, -100.0, 45.0, 155.0);
        let turned: Vec<Point> = a.iter().map(|p| [X0 + (p[1] - Y0), Y0 + (p[0] - X0), p[2]]).collect();
        let one = Fault { name: "F1", points: &a, triangles: &ta, maximum_throw: 5.0, sense: Sense::Down };
        let two = Fault { name: "F4", points: &turned, triangles: &ta, maximum_throw: 5.0, sense: Sense::Down };
        assert!(matches!(super::throw(&cloud, &[one, two], &settings(50.0, false)), Err(TearError::Crossing { .. })));
        // A bad throw, a bad reach.
        let one = Fault { name: "F1", points: &a, triangles: &ta, maximum_throw: 0.0, sense: Sense::Down };
        assert!(matches!(super::throw(&cloud, &[one], &settings(50.0, false)), Err(TearError::BadThrow { .. })));
        let one = Fault { name: "F1", points: &a, triangles: &ta, maximum_throw: 5.0, sense: Sense::Down };
        assert!(matches!(super::throw(&cloud, &[one], &settings(0.0, false)), Err(TearError::BadReach { .. })));
    }

    /// A fault in two patches cuts the horizon in two pieces that share an
    /// end: they are one trace, welded — F1 of Lutetia is made of two
    /// patches.
    #[test]
    fn a_fault_in_two_patches_is_one_trace() {
        let cloud = horizon(21, 10.0, -50.0);
        let (south, _) = fault(105.0, -50.0, 60.0, 0.0, -100.0, 45.0, 100.0);
        let (north, _) = fault(105.0, -50.0, 60.0, 0.0, -100.0, 100.0, 155.0);
        let points: Vec<Point> = south.iter().chain(&north).copied().collect();
        let triangles = vec![[0, 1, 2], [1, 3, 2], [4, 5, 6], [5, 7, 6]];
        let one = Fault { name: "F1", points: &points, triangles: &triangles, maximum_throw: 10.0, sense: Sense::Down };
        let torn = super::throw(&cloud, &[one], &settings(50.0, false)).expect("one trace");
        let lip = &torn.faults[0].lip;
        let ys: Vec<f64> = lip.footwall.iter().map(|v| torn.points[*v as usize][1] - Y0).collect();
        assert!((ys[0] - 45.0).abs() < 1e-6 && (ys[ys.len() - 1] - 155.0).abs() < 1e-6, "{ys:?}");
    }

    /// Two faults apart: each its lips, each its drop, the offsets added where
    /// both drag.
    #[test]
    fn two_faults_apart_each_tear_their_own_lip() {
        let cloud = horizon(21, 10.0, -50.0);
        let (a, ta) = fault(65.0, -50.0, 60.0, 0.0, -100.0, 45.0, 155.0);
        let (b, tb) = fault(145.0, -50.0, 60.0, 0.0, -100.0, 45.0, 155.0);
        let faults = [
            Fault { name: "F1", points: &a, triangles: &ta, maximum_throw: 10.0, sense: Sense::Down },
            Fault { name: "F2", points: &b, triangles: &tb, maximum_throw: 4.0, sense: Sense::Down },
        ];
        let torn = super::throw(&cloud, &faults, &settings(100.0, false)).expect("thrown");
        assert_eq!(torn.faults.len(), 2);
        assert_eq!(torn.split, torn.faults.iter().map(|f| f.lip.footwall.len() - 2).sum::<usize>());
        // The node at x = 150, y = 100: 85 m east of F1's trace, 5 m east of
        // F2's, both at their middle — the two drags add.
        let at = cloud.iter().position(|p| p[0] == X0 + 150.0 && p[1] == Y0 + 100.0).expect("a node");
        let s = (100.0 - 45.0) / 110.0;
        let expected = 10.0 * 4.0 * s * (1.0 - s) * (1.0 - 85.0 / 100.0) + 4.0 * 4.0 * s * (1.0 - s) * (1.0 - 5.0 / 100.0);
        assert!(((cloud[at][2] - torn.points[at][2]) - expected).abs() < 1e-9);
    }
}
