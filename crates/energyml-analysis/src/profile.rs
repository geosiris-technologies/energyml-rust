//! **Wells projected on a long profile** - which wellheads fall in the band
//! around a drawn track, where along it, how far and on which side - and the
//! band itself, as a polygon.
//!
//! Pure arithmetic: a plan polyline `[x, y]`, two widths, wellheads. Nothing
//! reads a package; whatever reads a trajectory's head
//! (`energyml_geometry::well::head_of`, which returns `[x, y, elevation]`) hands
//! the plan coordinates over. Everything is in the caller's plan unit.
//!
//! # The rules, applied to the letter
//!
//! - *"Plan distance from the wellhead to every section of the polyline; if the
//!   smallest distance is under the width, keep it"* - the distance is
//!   measured **from the head, in plan, to every segment**; a well is kept when
//!   that distance is **strictly under the width of its side**. A head exactly
//!   on the limit is not kept.
//! - **Left** and **right** are read in the direction the track was drawn: for a
//!   segment `A -> B`, `P` is on the left when `(B - A) x (P - A) > 0`. A head
//!   exactly on the axis (`= 0`) counts as left, or as right when the left width
//!   is zero. Where the closest point is a **vertex**, the side is the one of
//!   the vertex's **bisector** - the line along the mean direction of its two
//!   segments (the only answer that does not depend on the order they are
//!   visited in; the outer wedge of a turn is entirely on one side of it).
//! - *"Beyond an end, wells are ignored. Only those that project onto the
//!   segments are taken."* - a head that projects beyond an end of
//!   the track is **ignored**, never drawn, but **counted**
//!   ([`Profile::beyond_an_end`]): a gap that is not said looks like a well that
//!   is absent from the package.
//! - *Outer wedge* - in the **outer wedge of a turn** (the head projects on
//!   neither adjacent segment) the well is projected **on the vertex**: its
//!   abscisse is the vertex's and its distance the one to the vertex. Inside a
//!   turn a head that projects on two segments takes the **nearest**.
//! - The segments near a head are found through a **grid** of cells of side
//!   `max(left, right)`, each segment filed in every cell its widened bounding
//!   box touches - exact, unlike *« the nearest vertex first »*, which can miss
//!   the right segment (a head halfway along a long segment is far from both its
//!   ends, and a neighbouring vertex can be nearer than they are).
//!
//! # The band is the same region
//!
//! [`Corridor::band`] returns the polygon of **exactly the region the criterion
//! keeps**, from the same corridor (a band drawn by the front end and a criterion
//! computed here would end up disagreeing, and a well visibly inside the band
//! would be dropped unexplained): the segments offset by the left width on one
//! side and the right width on the other, **outer turns rounded** (the distance
//! to a vertex is a circle, approximated by a caller-given angle step), **inner
//! turns cut** where the two offsets cross, **straight ends**.
//!
//! The ring is counter-clockwise and **not closed** (the first point is not
//! repeated). Two things to know when drawing it:
//!
//! - arc points lie *on* the circle, so the polygon is the region minus the
//!   sagitta `w (1 - cos(step / 2))`: no dropped well is ever inside, and a
//!   kept one is inside up to that margin;
//! - where a segment is shorter than the width at an inner turn the offsets do
//!   not cross inside their segments; the ring then goes through the vertex,
//!   which makes a small loop. Fill it with the **non-zero winding rule** (the
//!   default `fill-rule` of SVG) and it is exact; a triangulator that wants a
//!   loop-free outline (three.js `ShapeGeometry`) must be given one instead.
//!
//! # What this does not do
//!
//! A well that is *nearest* on one side but too far there, while a farther
//! segment on the other side is within *its* side's width (a track that passes
//! on both sides of a head, a narrow U), is **kept** - by that other segment,
//! and the band contains it. Keeping the criterion and the band the same region
//! matters more than the order the two candidates are tried in.

use std::collections::HashMap;

/// A well, by its head in plan.
#[derive(Debug, Clone, PartialEq)]
pub struct Head {
    /// What the caller calls the well (a uuid, a URI, a name): returned as is.
    pub id: String,
    /// `[x, y]` of the head, in the unit of the track.
    pub xy: [f64; 2],
}

impl Head {
    pub fn new(id: impl Into<String>, xy: [f64; 2]) -> Self {
        Self { id: id.into(), xy }
    }
}

/// Which side of the track, read in the direction it was drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
}

/// A well kept: where it falls on the profile.
#[derive(Debug, Clone, PartialEq)]
pub struct Kept {
    pub id: String,
    /// Curvilinear abscisse along the track, from its first vertex.
    pub s: f64,
    /// Plan distance from the head to the track (to the vertex when the head is
    /// in the outer wedge of a turn).
    pub d: f64,
    pub side: Side,
    /// The foot of the projection, `[x, y]`: where the dotted line of the 3D
    /// view ends.
    pub foot: [f64; 2],
    /// The unit direction `[dx, dy]` of the track **where this well was kept**:
    /// the direction of the segment the projection retained, and at a vertex
    /// (the outer wedge of a turn) the **mean** of the two segments'
    /// directions, normalised - the bisector the side is read against.
    /// What places each node of the
    /// well along the profile: `s_node = s + (node - head) . direction`.
    pub direction: [f64; 2],
}

/// Why a well that is close to the track was not drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// Close to the track, but not under the width of its side.
    TooFarOnItsSide,
    /// Projects beyond an end of the track, within reach of its end.
    BeyondAnEnd,
    /// The head has no usable position (a coordinate is not a finite number):
    /// set aside and counted, never placed at the origin.
    NoPosition,
}

/// A well set aside **near the track**. A well far from everything is not
/// listed: a profile of 2 700 wells would otherwise answer with 2 690 lines.
#[derive(Debug, Clone, PartialEq)]
pub struct Rejected {
    pub id: String,
    pub reason: Reason,
}

/// What [`Corridor::project`] returns.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    /// In the order the heads were given.
    pub kept: Vec<Kept>,
    pub rejected: Vec<Rejected>,
}

impl Profile {
    /// How many wells were within reach beyond an end and so ignored.
    pub fn beyond_an_end(&self) -> usize {
        self.count(Reason::BeyondAnEnd)
    }

    pub fn count(&self, reason: Reason) -> usize {
        self.rejected.iter().filter(|r| r.reason == reason).count()
    }
}

/// The refusals, named.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ProfileError {
    #[error("a track needs at least two vertices, got {found}")]
    TooFewVertices { found: usize },
    #[error("track vertex {index} has a coordinate that is not a finite number")]
    NonFiniteVertex { index: usize },
    #[error(
        "track segment {index} has zero length (vertices {index} and {} coincide): \
         a degenerate track has no direction, so no side",
        index + 1
    )]
    ZeroLengthSegment { index: usize },
    #[error("the {side:?} width must be a finite number, not negative ({value})")]
    BadWidth { side: Side, value: f64 },
    #[error("both widths are zero: the band would keep nothing")]
    NoWidth,
    #[error("the arc step must be in (0, pi] radians, got {0}")]
    BadArcStep(f64),
    #[error(
        "segment {index} touches {cells} grid cells: the widths are far too small for the \
         length of the track"
    )]
    GridTooLarge { index: usize, cells: u64 },
}

/// The most cells one segment may be filed in - a guard against a width of a
/// millimetre on a track of kilometres, not a limit anyone meets.
const MAX_CELLS_PER_SEGMENT: u64 = 4_000_000;

type V = [f64; 2];

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1]]
}
fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1]]
}
fn scale(a: V, k: f64) -> V {
    [a[0] * k, a[1] * k]
}
fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn cross(a: V, b: V) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn norm(a: V) -> f64 {
    a[0].hypot(a[1])
}

/// A validated track with its two widths and the grid over its segments: build
/// it once, ask it for the band and for as many projections as needed.
#[derive(Debug, Clone)]
pub struct Corridor {
    points: Vec<V>,
    /// Unit direction of each segment.
    dirs: Vec<V>,
    lens: Vec<f64>,
    /// Curvilinear abscisse of each vertex.
    cum: Vec<f64>,
    left: f64,
    right: f64,
    /// `max(left, right)`: the side of a cell, and the reach of the band.
    reach: f64,
    origin: V,
    grid: HashMap<(i64, i64), Vec<usize>>,
}

impl Corridor {
    /// Validates the track and widths, and files the segments in the grid.
    pub fn new(track: &[[f64; 2]], left: f64, right: f64) -> Result<Self, ProfileError> {
        if track.len() < 2 {
            return Err(ProfileError::TooFewVertices { found: track.len() });
        }
        if let Some(index) = track.iter().position(|p| !p[0].is_finite() || !p[1].is_finite()) {
            return Err(ProfileError::NonFiniteVertex { index });
        }
        for (side, value) in [(Side::Left, left), (Side::Right, right)] {
            if !value.is_finite() || value < 0.0 {
                return Err(ProfileError::BadWidth { side, value });
            }
        }
        if left == 0.0 && right == 0.0 {
            return Err(ProfileError::NoWidth);
        }
        let n = track.len() - 1;
        let (mut dirs, mut lens, mut cum) = (Vec::with_capacity(n), Vec::with_capacity(n), vec![0.0]);
        for i in 0..n {
            let ab = sub(track[i + 1], track[i]);
            let len = norm(ab);
            if len == 0.0 || !len.is_finite() {
                return Err(ProfileError::ZeroLengthSegment { index: i });
            }
            dirs.push(scale(ab, 1.0 / len));
            lens.push(len);
            cum.push(cum[i] + len);
        }
        let reach = left.max(right);
        let origin = [
            track.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min),
            track.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min),
        ];
        let mut me = Self {
            points: track.to_vec(),
            dirs,
            lens,
            cum,
            left,
            right,
            reach,
            origin,
            grid: HashMap::new(),
        };
        for i in 0..n {
            let (a, b) = (me.points[i], me.points[i + 1]);
            let (x0, x1) = (
                me.cell_of(a[0].min(b[0]) - reach, 0),
                me.cell_of(a[0].max(b[0]) + reach, 0),
            );
            let (y0, y1) = (
                me.cell_of(a[1].min(b[1]) - reach, 1),
                me.cell_of(a[1].max(b[1]) + reach, 1),
            );
            let cells = (x1 - x0 + 1) as u64 * (y1 - y0 + 1) as u64;
            if cells > MAX_CELLS_PER_SEGMENT {
                return Err(ProfileError::GridTooLarge { index: i, cells });
            }
            for cx in x0..=x1 {
                for cy in y0..=y1 {
                    me.grid.entry((cx, cy)).or_default().push(i);
                }
            }
        }
        Ok(me)
    }

    fn cell_of(&self, v: f64, axis: usize) -> i64 {
        ((v - self.origin[axis]) / self.reach).floor() as i64
    }

    fn width(&self, side: Side) -> f64 {
        match side {
            Side::Left => self.left,
            Side::Right => self.right,
        }
    }

    /// The side of a signed cross product; on the axis, the side that has a
    /// width.
    fn side_of(&self, cross: f64) -> Side {
        if cross > 0.0 || (cross == 0.0 && self.left > 0.0) {
            Side::Left
        } else {
            Side::Right
        }
    }

    /// Total length of the track.
    pub fn length(&self) -> f64 {
        *self.cum.last().unwrap()
    }

    /// Projects the heads: kept ones with `s`, `d`, side and foot; the ones set
    /// aside near the track, with the reason.
    pub fn project(&self, heads: &[Head]) -> Profile {
        let mut profile = Profile {
            kept: Vec::new(),
            rejected: Vec::new(),
        };
        for head in heads {
            if !head.xy[0].is_finite() || !head.xy[1].is_finite() {
                profile.rejected.push(Rejected {
                    id: head.id.clone(),
                    reason: Reason::NoPosition,
                });
                continue;
            }
            match self.classify(head.xy) {
                Verdict::Kept {
                    s,
                    d,
                    side,
                    foot,
                    direction,
                } => profile.kept.push(Kept {
                    id: head.id.clone(),
                    s,
                    d,
                    side,
                    foot,
                    direction,
                }),
                Verdict::Rejected(reason) => profile.rejected.push(Rejected {
                    id: head.id.clone(),
                    reason,
                }),
                Verdict::Far => {}
            }
        }
        profile
    }

    fn classify(&self, p: V) -> Verdict {
        let key = (self.cell_of(p[0], 0), self.cell_of(p[1], 1));
        let Some(segments) = self.grid.get(&key) else {
            return Verdict::Far;
        };
        let n = self.lens.len();
        // (segment, parameter of the projection along it), ascending.
        let ts: Vec<(usize, f64)> = segments
            .iter()
            .map(|&i| (i, dot(sub(p, self.points[i]), self.dirs[i]) / self.lens[i]))
            .collect();
        // Every place the head can be said to be at: the interior of a segment
        // (foot included) or the vertex of an outer wedge.
        let mut candidates: Vec<Candidate> = Vec::new();
        for &(i, t) in &ts {
            if (0.0..=1.0).contains(&t) {
                let a = self.points[i];
                let foot = add(a, scale(self.dirs[i], t * self.lens[i]));
                let side = self.side_of(cross(self.dirs[i], sub(p, a)));
                candidates.push(Candidate {
                    d: norm(sub(p, foot)),
                    s: self.cum[i] + t * self.lens[i],
                    side,
                    foot,
                    direction: self.dirs[i],
                });
            }
        }
        for w in ts.windows(2) {
            let ((i, ti), (j, tj)) = (w[0], w[1]);
            // The outer wedge of vertex j: past the end of i, before the start
            // of j (strictly: on the perpendicular, the segment answers).
            if j == i + 1 && ti > 1.0 && tj < 0.0 {
                let v = self.points[j];
                let tangent = add(self.dirs[i], self.dirs[j]);
                let reference = if norm(tangent) > 1e-12 {
                    tangent
                } else {
                    self.dirs[i]
                };
                candidates.push(Candidate {
                    d: norm(sub(p, v)),
                    s: self.cum[j],
                    side: self.side_of(cross(reference, sub(p, v))),
                    foot: v,
                    direction: scale(reference, 1.0 / norm(reference)),
                });
            }
        }
        let mut best: Option<&Candidate> = None;
        for c in candidates.iter().filter(|c| c.d < self.width(c.side)) {
            if best.is_none_or(|b| (c.d, c.s) < (b.d, b.s)) {
                best = Some(c);
            }
        }
        if let Some(c) = best {
            return Verdict::Kept {
                s: c.s,
                d: c.d,
                side: c.side,
                foot: c.foot,
                direction: c.direction,
            };
        }
        if candidates.iter().any(|c| c.d <= self.reach) {
            return Verdict::Rejected(Reason::TooFarOnItsSide);
        }
        // Beyond an end, and within reach of it.
        for &(i, t) in &ts {
            let end = if i == 0 && t < 0.0 {
                Some((self.points[0], self.dirs[0]))
            } else if i == n - 1 && t > 1.0 {
                Some((self.points[n], self.dirs[n - 1]))
            } else {
                None
            };
            if let Some((v, dir)) = end {
                let side = self.side_of(cross(dir, sub(p, v)));
                if norm(sub(p, v)) < self.width(side) {
                    return Verdict::Rejected(Reason::BeyondAnEnd);
                }
            }
        }
        Verdict::Far
    }

    /// The polygon of the region [`project`](Self::project) keeps: counter-clockwise,
    /// not closed. `arc_step` is the angle, in radians, between two points of
    /// a rounded outer turn.
    pub fn band(&self, arc_step: f64) -> Result<Vec<[f64; 2]>, ProfileError> {
        if !(arc_step > 0.0 && arc_step <= std::f64::consts::PI) {
            return Err(ProfileError::BadArcStep(arc_step));
        }
        let left = self.offset(1.0, self.left, arc_step);
        let mut right = self.offset(-1.0, self.right, arc_step);
        right.reverse();
        let mut ring = left;
        ring.extend(right);
        // Left forward then right backward runs clockwise; the sign of the
        // area says so instead of assuming it.
        let area: f64 = (0..ring.len())
            .map(|i| cross(ring[i], ring[(i + 1) % ring.len()]))
            .sum();
        if area < 0.0 {
            ring.reverse();
        }
        Ok(ring)
    }

    /// One edge of the band, from the start of the track to its end. `sign` is
    /// `+1` for the left, `-1` for the right.
    fn offset(&self, sign: f64, w: f64, arc_step: f64) -> Vec<V> {
        let n = self.lens.len();
        let normal = |i: usize| scale([-self.dirs[i][1], self.dirs[i][0]], sign);
        let mut out = vec![add(self.points[0], scale(normal(0), w))];
        for k in 1..n {
            let (d0, d1) = (self.dirs[k - 1], self.dirs[k]);
            let (cr, dt) = (cross(d0, d1), dot(d0, d1));
            let reversal = cr == 0.0 && dt < 0.0;
            let v = self.points[k];
            let (end0, start1) = (add(v, scale(normal(k - 1), w)), add(v, scale(normal(k), w)));
            if reversal || sign * cr < 0.0 {
                // Outer turn: the arc around the vertex, at radius w.
                let phi = if reversal {
                    -sign * std::f64::consts::PI
                } else {
                    cr.atan2(dt)
                };
                let n0 = normal(k - 1);
                let a0 = n0[1].atan2(n0[0]);
                let steps = (phi.abs() / arc_step).ceil().max(1.0) as usize;
                out.push(end0);
                for j in 1..steps {
                    let a = a0 + phi * j as f64 / steps as f64;
                    out.push(add(v, [w * a.cos(), w * a.sin()]));
                }
                out.push(start1);
            } else if cr == 0.0 {
                // Straight on: the two offsets are the same point.
                out.push(end0);
            } else {
                // Inner turn: cut where the two offsets cross, if they do
                // inside their segments; through the vertex otherwise.
                let o1 = add(self.points[k - 1], scale(normal(k - 1), w));
                let delta = sub(start1, o1);
                let a = cross(delta, d1) / cr;
                let b = cross(delta, d0) / cr;
                if (0.0..=self.lens[k - 1]).contains(&a) && (0.0..=self.lens[k]).contains(&b) {
                    out.push(add(o1, scale(d0, a)));
                } else {
                    out.push(end0);
                    out.push(v);
                    out.push(start1);
                }
            }
        }
        out.push(add(self.points[n], scale(normal(n - 1), w)));
        out
    }
}

struct Candidate {
    d: f64,
    s: f64,
    side: Side,
    foot: V,
    direction: V,
}

enum Verdict {
    Kept {
        s: f64,
        d: f64,
        side: Side,
        foot: V,
        direction: V,
    },
    Rejected(Reason),
    /// Not within reach of the track: not listed.
    Far,
}

/// The whole call: the projections and the band.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub profile: Profile,
    /// See [`Corridor::band`].
    pub band: Vec<[f64; 2]>,
    /// Length of the track.
    pub length: f64,
}

/// Validates, projects and draws the band in one call - what a width slider
/// re-asks on release.
pub fn project(
    track: &[[f64; 2]],
    left: f64,
    right: f64,
    heads: &[Head],
    arc_step: f64,
) -> Result<Section, ProfileError> {
    let corridor = Corridor::new(track, left, right)?;
    let band = corridor.band(arc_step)?;
    Ok(Section {
        profile: corridor.project(heads),
        band,
        length: corridor.length(),
    })
}
