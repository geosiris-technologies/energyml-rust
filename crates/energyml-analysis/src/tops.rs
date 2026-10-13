//! **Where a well crosses a surface** — the computed tops.
//!
//! # What is computed, and with what
//!
//! A path — a function from a measured depth to a position, which the caller
//! evaluates (`energyml_geometry::parametric::Path` for a trajectory of a
//! package: the reader that draws the well, minimum curvature included) — and
//! a surface read by [`crate::heights`], the one reading of *the height of
//! this surface under this (x, y)*. Along the path,
//!
//! `g(md) = z(md) − h(x(md), y(md))`
//!
//! and a crossing is a root of `g`: bracketed where two samples of the path
//! lie on either side of the surface, then bisected to the last representable
//! measured depth. Neither the path nor the surface is re-implemented here —
//! a second evaluator of either would be a second answer to where the well
//! is, or where the horizon is.
//!
//! # Why no ray caster
//!
//! A ray caster (a BVH, for instance) could do it. It is not needed **for a surface read as heights**:
//! a vertical line meets such a surface once, so the crossing is a root in one
//! variable, and [`crate::heights`] already buckets the faces to answer it.
//! `intersect` was done the same way, without the ray caster. What a ray
//! caster would add is exactly what heights refuse — an overturned surface, a
//! reverse fault, where several faces lie under one position: there the
//! crossing is **not decided** here, and every such span is named
//! ([`Tops::folded`]). That case is what still waits for a ray caster.
//!
//! # What is counted, and why none of it is silent
//!
//! - **crossings**, in the order of the path: a well that goes down through a
//!   horizon and back up through it crosses it twice, and both are answers;
//! - **touches**: the path meets the surface at a sample and leaves on the
//!   side it came from — no crossing, said;
//! - **gaps**: the path is on one side, leaves the surface's footprint (or
//!   passes over a hole), and comes back on the other. It crossed *where the
//!   surface is not*, which is no top — counted, with its span;
//! - **folded**: the same, across a region where the surface has several
//!   heights. The crossing is somewhere in the span, and which face it is on
//!   is not a numerical choice;
//! - **unresolved**: the bisection met a position with no single height — a
//!   face set aside, the edge of a hole — before it converged;
//! - and when there is **no crossing at all**, which side the path stayed on
//!   and how close it came, or that it never passed over the surface.
//!
//! # The one limit, named
//!
//! A sign change is only seen between two samples. Two crossings closer than
//! one sampling step along the path — a hole that dips into a surface and out
//! again within a step — cancel, and are missed. The step is the caller's
//! ([`plan_spacing`] proposes half the surface's median edge in plan: a path
//! moves at most one unit in plan per unit of measured depth, so every face it
//! passes over is sampled at least twice), and it is reported with the answer.

use crate::heights::{Height, Heights, Surface};

/// Which side of the surface a point lies on, in the frame's own numbers:
/// `Greater` is a z larger than the surface's height there. Whether that is
/// *above* is the frame's vertical sense, which the caller knows and this
/// module does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// z larger than the surface's height.
    Greater,
    /// z smaller than the surface's height.
    Less,
}

impl Side {
    fn of(g: f64) -> Side {
        if g > 0.0 { Side::Greater } else { Side::Less }
    }
}

/// One crossing of the surface by the path.
#[derive(Clone, Debug, PartialEq)]
pub struct Crossing {
    /// The measured depth where `z` equals the surface's height.
    pub md: f64,
    /// The path's position there — the path evaluated at [`Crossing::md`].
    pub at: [f64; 3],
    /// The side the path comes from, in the order of increasing depth.
    pub from: Side,
    /// How far the position lies from the surface vertically, `z − h` — the
    /// residual of the bisection, a few units in the last place.
    pub residual: f64,
}

/// What a path does with respect to one surface — see the module.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tops {
    /// The crossings, by increasing measured depth.
    pub crossings: Vec<Crossing>,
    /// Measured depths where the path meets the surface and leaves on the side
    /// it came from.
    pub touches: Vec<f64>,
    /// `[from, to]` spans where the path changes side where the surface is
    /// not — outside its footprint, over a hole.
    pub gaps: Vec<[f64; 2]>,
    /// `[from, to]` spans where the path changes side over a fold of the
    /// surface: a crossing is in the span, undecided.
    pub folded: Vec<[f64; 2]>,
    /// `[from, to]` spans where a crossing was bracketed and the bisection met
    /// a position with no single height before it converged.
    pub unresolved: Vec<[f64; 2]>,
    /// When no sample changed side: the one side every sample with a height
    /// lies on. `None` when some did, or when no sample has a height.
    pub side: Option<Side>,
    /// The smallest `|z − h|` over the samples with a height — how close the
    /// path came, when it did not cross.
    pub closest: Option<f64>,
    /// How many samples of the path have a height under them.
    pub over: usize,
    /// How many samples lie over a fold.
    pub over_fold: usize,
    /// How many samples were evaluated.
    pub samples: usize,
    /// The sampling step used, in units of measured depth.
    pub step: f64,
}

/// What [`tops`] refuses, by name.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum TopsError {
    /// Fewer than two finite, increasing knots: no stretch of hole to follow.
    #[error(
        "the path states {given} usable knot(s) — a stretch of hole needs two finite measured depths, increasing"
    )]
    NoStretch {
        /// How many were usable.
        given: usize,
    },
    /// A step that is not a positive number.
    #[error("the sampling step must be a finite number > 0, not {given}")]
    BadStep {
        /// What was given.
        given: f64,
    },
    /// More samples than a path is worth.
    #[error(
        "sampling {length} of hole every {step} asks for {wanted} evaluations where {most} is the most — give a larger step"
    )]
    TooManySamples {
        /// The length of hole.
        length: f64,
        /// The step.
        step: f64,
        /// How many samples that makes.
        wanted: usize,
        /// The ceiling.
        most: usize,
    },
}

/// The most samples along one path.
pub const MOST_SAMPLES: usize = 5_000_000;

/// The median length, in plan, of the edges of a surface's faces — what a
/// sampling step is proposed from. `None` when no edge has a finite, non-zero
/// length.
pub fn plan_spacing(surface: &Surface) -> Option<f64> {
    let point = |i: u32| {
        let i = i as usize * 3;
        surface.points.get(i..i + 2).map(|p| [p[0], p[1]])
    };
    let mut lengths = Vec::new();
    let mut ring = |corners: &[u32]| {
        for (k, &a) in corners.iter().enumerate() {
            let b = corners[(k + 1) % corners.len()];
            if let (Some(p), Some(q)) = (point(a), point(b)) {
                let length = ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2)).sqrt();
                if length.is_finite() && length > 0.0 {
                    lengths.push(length);
                }
            }
        }
    };
    surface.triangles.chunks_exact(3).for_each(&mut ring);
    surface.quads.chunks_exact(4).for_each(&mut ring);
    if lengths.is_empty() {
        return None;
    }
    lengths.sort_by(f64::total_cmp);
    Some(lengths[lengths.len() / 2])
}

/// The measured depths the path is evaluated at: every knot, and between two
/// knots as many equal steps as it takes to stay under `step`.
fn samples(knots: &[f64], step: f64) -> Result<Vec<f64>, TopsError> {
    let length = knots[knots.len() - 1] - knots[0];
    let mut wanted = 1usize;
    for pair in knots.windows(2) {
        wanted = wanted.saturating_add(((pair[1] - pair[0]) / step).ceil().max(1.0) as usize);
    }
    if wanted > MOST_SAMPLES {
        return Err(TopsError::TooManySamples {
            length,
            step,
            wanted,
            most: MOST_SAMPLES,
        });
    }
    let mut out = Vec::with_capacity(wanted);
    for pair in knots.windows(2) {
        let pieces = ((pair[1] - pair[0]) / step).ceil().max(1.0) as usize;
        let width = (pair[1] - pair[0]) / pieces as f64;
        out.extend((0..pieces).map(|k| pair[0] + k as f64 * width));
    }
    out.push(knots[knots.len() - 1]);
    Ok(out)
}

/// `z − h` at a measured depth, or the height's answer when it has none.
fn residual(path: &dyn Fn(f64) -> [f64; 3], surface: &Heights, md: f64) -> ([f64; 3], Height, f64) {
    let at = path(md);
    if !at.iter().all(|c| c.is_finite()) {
        return (at, Height::Outside, f64::NAN);
    }
    match surface.height(at[0], at[1]) {
        Height::At(h) => (at, Height::At(h), at[2] - h),
        other => (at, other, f64::NAN),
    }
}

/// The root of `g` in `[a, b]`, where `g(a)` lies on `from`'s side and `g(b)`
/// on the other: halved until the two ends are adjacent numbers. `None` when a
/// midpoint has no single height.
fn bisect(
    path: &dyn Fn(f64) -> [f64; 3],
    surface: &Heights,
    mut a: f64,
    mut b: f64,
    from: Side,
) -> Option<Crossing> {
    let mut best: Option<Crossing> = None;
    for _ in 0..2100 {
        let m = a + 0.5 * (b - a);
        if m <= a || m >= b {
            break;
        }
        let (at, height, g) = residual(path, surface, m);
        if !matches!(height, Height::At(_)) {
            return None;
        }
        if g == 0.0 {
            return Some(Crossing {
                md: m,
                at,
                from,
                residual: 0.0,
            });
        }
        if Side::of(g) == from {
            a = m;
        } else {
            b = m;
        }
        best = Some(Crossing {
            md: m,
            at,
            from,
            residual: g,
        });
    }
    // The two ends are adjacent: the one nearer the surface.
    let ends = [a, b].map(|md| residual(path, surface, md));
    let [(pa, ha, ga), (pb, hb, gb)] = ends;
    let pick = match (ha, hb) {
        (Height::At(_), Height::At(_)) if ga.abs() <= gb.abs() => Some(Crossing {
            md: a,
            at: pa,
            from,
            residual: ga,
        }),
        (Height::At(_), Height::At(_)) => Some(Crossing {
            md: b,
            at: pb,
            from,
            residual: gb,
        }),
        (Height::At(_), _) => Some(Crossing {
            md: a,
            at: pa,
            from,
            residual: ga,
        }),
        (_, Height::At(_)) => Some(Crossing {
            md: b,
            at: pb,
            from,
            residual: gb,
        }),
        _ => None,
    };
    pick.or(best)
}

/// Where a path crosses a surface — see the module.
///
/// `path` gives the position at a measured depth; `knots` are the measured
/// depths it states (its stations) — the path is followed from the first to
/// the last, never beyond: a well does not exist past its last station. A
/// non-finite position counts as one with no height under it.
pub fn tops(
    path: &dyn Fn(f64) -> [f64; 3],
    knots: &[f64],
    step: f64,
    surface: &Heights,
) -> Result<Tops, TopsError> {
    if !(step.is_finite() && step > 0.0) {
        return Err(TopsError::BadStep { given: step });
    }
    // The leading run of finite, strictly increasing knots.
    let mut usable: Vec<f64> = Vec::with_capacity(knots.len());
    for &knot in knots {
        if !knot.is_finite() || usable.last().is_some_and(|&last| knot <= last) {
            break;
        }
        usable.push(knot);
    }
    if usable.len() < 2 {
        return Err(TopsError::NoStretch { given: usable.len() });
    }
    let mds = samples(&usable, step)?;

    /// What lies between the last sample with a height and this one.
    #[derive(Clone, Copy, PartialEq)]
    enum Between {
        Nothing,
        Gap,
        Fold,
    }

    let mut out = Tops {
        samples: mds.len(),
        step,
        ..Tops::default()
    };
    // The last sample with a height, off the surface: its depth and side.
    let mut last: Option<(f64, Side)> = None;
    // Where the path started lying exactly on the surface, since `last`.
    let mut on_since: Option<f64> = None;
    let mut between = Between::Nothing;
    let mut sides = [false; 2];

    for &md in &mds {
        let (_, height, g) = residual(path, surface, md);
        match height {
            Height::Outside => {
                if between == Between::Nothing {
                    between = Between::Gap;
                }
                on_since = None;
            }
            Height::Folded => {
                out.over_fold += 1;
                between = Between::Fold;
                on_since = None;
            }
            Height::At(_) => {
                out.over += 1;
                out.closest = Some(out.closest.map_or(g.abs(), |c| c.min(g.abs())));
                if g == 0.0 {
                    on_since.get_or_insert(md);
                    continue;
                }
                let side = Side::of(g);
                sides[usize::from(side == Side::Less)] = true;
                if let Some((from_md, from)) = last {
                    match (between, on_since) {
                        (Between::Gap, _) if from != side => out.gaps.push([from_md, md]),
                        (Between::Fold, _) if from != side => out.folded.push([from_md, md]),
                        (Between::Nothing, Some(on)) => {
                            if from != side {
                                let (at, _, _) = residual(path, surface, on);
                                out.crossings.push(Crossing {
                                    md: on,
                                    at,
                                    from,
                                    residual: 0.0,
                                });
                            } else {
                                out.touches.push(on);
                            }
                        }
                        (Between::Nothing, None) if from != side => {
                            match bisect(path, surface, from_md, md, from) {
                                Some(crossing) => out.crossings.push(crossing),
                                None => out.unresolved.push([from_md, md]),
                            }
                        }
                        _ => {}
                    }
                } else if let Some(on) = on_since {
                    // It starts on the surface: nothing says which side it
                    // came from, so it is no crossing.
                    out.touches.push(on);
                }
                last = Some((md, side));
                on_since = None;
                between = Between::Nothing;
            }
        }
    }
    // It ends on the surface.
    if let Some(on) = on_since {
        out.touches.push(on);
    }
    if out.crossings.is_empty() && out.gaps.is_empty() && out.folded.is_empty() && out.unresolved.is_empty() {
        out.side = match sides {
            [true, false] => Some(Side::Greater),
            [false, true] => Some(Side::Less),
            _ => None,
        };
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A square of `2 × half` on a side, centred on the origin, as a plane
    /// `z = c + a·x + b·y` — two triangles, exact for a plane.
    fn plane(half: f64, c: f64, a: f64, b: f64) -> (Vec<f64>, Vec<u32>) {
        let z = |x: f64, y: f64| c + a * x + b * y;
        let corners = [[-half, -half], [half, -half], [half, half], [-half, half]];
        let points = corners.iter().flat_map(|p| [p[0], p[1], z(p[0], p[1])]).collect();
        (points, vec![0, 1, 2, 0, 2, 3])
    }

    fn heights<'a>(points: &'a [f64], triangles: &'a [u32]) -> Heights<'a> {
        Heights::of(
            Surface {
                points,
                triangles,
                quads: &[],
            },
            "horizon",
            "in a test",
        )
        .expect("faces")
    }

    /// **A straight hole through a tilted plane lands where the algebra says.**
    ///
    /// The path `p(s) = (s·sin θ, 0, −s·cos θ)` — a hole at θ from the
    /// vertical, collared at the origin — meets `z = c + a·x` where
    /// `−s cos θ = c + a s sin θ`, so `s = −c / (cos θ + a sin θ)`. To break:
    /// a bisection that stops early, a root taken at a sample, `x` and `y`
    /// swapped in the height.
    #[test]
    fn a_straight_hole_meets_a_tilted_plane_at_the_algebras_depth() {
        let (points, triangles) = plane(1000.0, -300.0, 0.2, 0.0);
        let surface = heights(&points, &triangles);
        let theta: f64 = 30f64.to_radians();
        let path = |s: f64| [s * theta.sin(), 0.0, -s * theta.cos()];
        let found = tops(&path, &[0.0, 800.0], 25.0, &surface).expect("a path");
        assert_eq!(found.crossings.len(), 1, "{found:?}");
        let expected = 300.0 / (theta.cos() + 0.2 * theta.sin());
        let crossing = &found.crossings[0];
        assert!(
            (crossing.md - expected).abs() < 1e-9,
            "{} against {expected}",
            crossing.md
        );
        assert_eq!(
            crossing.from,
            Side::Greater,
            "it comes from above in an elevation frame"
        );
        assert!((crossing.at[2] - (-300.0 + 0.2 * crossing.at[0])).abs() < 1e-9);
    }

    /// **An arc is followed, not its chord.** A quarter circle of radius `R`
    /// from the vertical to the horizontal — `x = R (1 − cos(s/R))`,
    /// `z = −R sin(s/R)` — meets the plane `z = −d` at `s = R asin(d/R)`. The
    /// chord between the two stations meets it elsewhere: sampling the chord
    /// instead of the path makes this red.
    #[test]
    fn a_curved_hole_is_followed_between_its_stations() {
        let r = 500.0;
        let (points, triangles) = plane(2000.0, -300.0, 0.0, 0.0);
        let surface = heights(&points, &triangles);
        let path = |s: f64| [r * (1.0 - (s / r).cos()), 0.0, -r * (s / r).sin()];
        let end = r * std::f64::consts::FRAC_PI_2;
        let found = tops(&path, &[0.0, end], 10.0, &surface).expect("a path");
        assert_eq!(found.crossings.len(), 1);
        let expected = r * (300.0f64 / r).asin();
        assert!(
            (found.crossings[0].md - expected).abs() < 1e-9,
            "{} against {expected}",
            found.crossings[0].md
        );
    }

    /// **Down and back up is two crossings, in the order of the hole.**
    #[test]
    fn a_hole_that_turns_back_crosses_twice() {
        let (points, triangles) = plane(2000.0, -100.0, 0.0, 0.0);
        let surface = heights(&points, &triangles);
        // A parabola in depth: z = −s (400 − s) / 400 — its deepest point is
        // −100 at s = 200… just touching. Deeper: −s (400 − s) / 300.
        let path = |s: f64| [s, 0.0, -s * (400.0 - s) / 300.0];
        let found = tops(&path, &[0.0, 400.0], 7.0, &surface).expect("a path");
        assert_eq!(found.crossings.len(), 2, "{found:?}");
        // −s (400 − s) / 300 = −100  ⇔  s² − 400 s + 30 000 = 0  ⇔  s = 100, 300.
        assert!((found.crossings[0].md - 100.0).abs() < 1e-9);
        assert!((found.crossings[1].md - 300.0).abs() < 1e-9);
        assert_eq!(found.crossings[0].from, Side::Greater);
        assert_eq!(found.crossings[1].from, Side::Less);
    }

    /// **A hole that stops short says which side it stayed on, and how close
    /// it came.**
    #[test]
    fn a_hole_that_stops_short_says_so() {
        let (points, triangles) = plane(1000.0, -300.0, 0.0, 0.0);
        let surface = heights(&points, &triangles);
        let path = |s: f64| [0.0, 0.0, -s];
        let found = tops(&path, &[0.0, 250.0], 10.0, &surface).expect("a path");
        assert!(found.crossings.is_empty());
        assert_eq!(found.side, Some(Side::Greater));
        assert!((found.closest.unwrap() - 50.0).abs() < 1e-9);
        assert_eq!(found.over, found.samples);
    }

    /// **Beside the surface is not above it.**
    #[test]
    fn a_hole_beside_the_surface_is_over_nothing() {
        let (points, triangles) = plane(100.0, -300.0, 0.0, 0.0);
        let surface = heights(&points, &triangles);
        let path = |s: f64| [5000.0, 0.0, -s];
        let found = tops(&path, &[0.0, 600.0], 10.0, &surface).expect("a path");
        assert!(found.crossings.is_empty() && found.gaps.is_empty());
        assert_eq!((found.over, found.side, found.closest), (0, None, None));
    }

    /// **Passing where the surface is not is no top.** A hole goes down
    /// outside the square and comes back under it: the side changed, and no
    /// face was crossed.
    #[test]
    fn changing_side_outside_the_surface_is_a_gap_not_a_crossing() {
        let (points, triangles) = plane(100.0, -300.0, 0.0, 0.0);
        let surface = heights(&points, &triangles);
        // Over the square at the top, off it in the middle, under it after.
        let path = |s: f64| {
            let x = if (100.0..500.0).contains(&s) { 1000.0 } else { 0.0 };
            [x, 0.0, -s]
        };
        let found = tops(&path, &[0.0, 600.0], 10.0, &surface).expect("a path");
        assert!(found.crossings.is_empty(), "{found:?}");
        assert_eq!(found.gaps.len(), 1);
        assert!(
            found.gaps[0][0] < 100.0 && found.gaps[0][1] >= 500.0,
            "{:?}",
            found.gaps
        );
    }

    /// **Over a fold, the crossing is named and not decided.** Two
    /// overlapping triangles at −200 and −400 under the hole.
    #[test]
    fn a_fold_is_named_not_decided() {
        let points = [
            -10.0, -10.0, -200.0, 10.0, -10.0, -200.0, 0.0, 10.0, -200.0, //
            -10.0, -10.0, -400.0, 10.0, -10.0, -400.0, 0.0, 10.0, -400.0,
        ];
        let triangles = [0, 1, 2, 3, 4, 5];
        // The folded triangles, and a plain one beside at −300 that the
        // hole's top and bottom lie over.
        let mut all = points.to_vec();
        all.extend([20.0, -10.0, -300.0, 40.0, -10.0, -300.0, 30.0, 10.0, -300.0]);
        let mut faces = triangles.to_vec();
        faces.extend([6, 7, 8]);
        let surface = heights(&all, &faces);
        let path = |s: f64| {
            let x = if (100.0..500.0).contains(&s) { 0.0 } else { 30.0 };
            [x, 0.0, -s]
        };
        let found = tops(&path, &[0.0, 600.0], 10.0, &surface).expect("a path");
        assert!(found.crossings.is_empty(), "{found:?}");
        assert_eq!(found.folded.len(), 1, "{found:?}");
        assert!(found.over_fold > 0);
    }

    /// **Meeting the surface at a sample and leaving on the same side is a
    /// touch.** The parabola that reaches exactly −100 at s = 200, which is a
    /// sample.
    #[test]
    fn grazing_the_surface_is_a_touch() {
        let (points, triangles) = plane(2000.0, -100.0, 0.0, 0.0);
        let surface = heights(&points, &triangles);
        let path = |s: f64| [s, 0.0, -s * (400.0 - s) / 400.0];
        let found = tops(&path, &[0.0, 400.0], 10.0, &surface).expect("a path");
        assert!(found.crossings.is_empty(), "{found:?}");
        assert_eq!(found.touches, vec![200.0]);
    }

    /// **A crossing at a sample is still a crossing** — the root is the
    /// sample itself, found without bisecting.
    #[test]
    fn a_crossing_on_a_sample_is_found_there() {
        let (points, triangles) = plane(1000.0, -300.0, 0.0, 0.0);
        let surface = heights(&points, &triangles);
        let path = |s: f64| [0.0, 0.0, -s];
        let found = tops(&path, &[0.0, 600.0], 10.0, &surface).expect("a path");
        assert_eq!(found.crossings.len(), 1);
        assert_eq!(found.crossings[0].md, 300.0);
        assert!(found.touches.is_empty());
    }

    #[test]
    fn what_cannot_be_followed_is_refused_by_name() {
        let (points, triangles) = plane(10.0, 0.0, 0.0, 0.0);
        let surface = heights(&points, &triangles);
        let path = |s: f64| [0.0, 0.0, -s];
        assert_eq!(
            tops(&path, &[0.0], 1.0, &surface),
            Err(TopsError::NoStretch { given: 1 })
        );
        assert_eq!(
            tops(&path, &[0.0, f64::NAN], 1.0, &surface),
            Err(TopsError::NoStretch { given: 1 })
        );
        assert_eq!(
            tops(&path, &[0.0, 10.0], 0.0, &surface),
            Err(TopsError::BadStep { given: 0.0 })
        );
        assert!(matches!(
            tops(&path, &[0.0, 1e9], 1e-3, &surface),
            Err(TopsError::TooManySamples { .. })
        ));
    }

    #[test]
    fn the_spacing_is_the_median_edge_in_plan() {
        let (points, triangles) = plane(50.0, 0.0, 0.3, 0.0);
        // Four sides of 100 twice, and the diagonal 141 twice: the median of
        // [100 × 4 (+ shared twice), 141 × 2] is 100 whatever the slope.
        let spacing = plan_spacing(&Surface {
            points: &points,
            triangles: &triangles,
            quads: &[],
        })
        .unwrap();
        assert!((spacing - 100.0).abs() < 1e-12, "{spacing}");
    }
}
