//! Faults as **barriers**: a sample on the far side of a fault says nothing
//! about this side.
//!
//! What is asked of a kriging "with fault lines", and what every mapping package means by a fault in plan view: a
//! polyline that the interpolation must not see across. Two samples are
//! neighbours only if the straight segment between them crosses no fault; a
//! query is informed only by the samples it can see. That is a rule on the
//! **neighbourhood**, so it serves IDW and kriging alike, and it is why it
//! lives here rather than inside either.
//!
//! # What a barrier is not
//!
//! It is not a displacement, not a distance around the fault tip, and not a
//! surface: a fault here is a set of 2D segments and nothing else. A sample
//! just past the tip of a fault is visible from just before it, because the
//! segment between them crosses nothing - the geometry says so, and guessing
//! a shadow beyond the tip would be a decision nobody stated.
//!
//! # Touching counts as crossing
//!
//! A sample **on** a fault trace, or a segment that merely grazes a fault's
//! end, is treated as blocked. Geologically a point on the trace belongs to
//! neither block; numerically it is the case where floating point cannot
//! decide, and *refuse rather than draw* says which way to fall.

use crate::Error;

/// How many consecutive segments share one bounding box.
///
/// Consecutive pieces of a trace lie next to each other, so a box around a
/// few of them is small, and a sight line that misses it skips them all.
const CHUNK: usize = 16;

/// A set of fault traces in plan view, kept as segments.
///
/// # Why the boxes, and why they cannot change an answer
///
/// Measured by a speed report: an
/// IDW over 10 000 nodes, 2 000 samples and 500 fault segments took 32 s,
/// every sight line tested against every segment. Two closed segments that
/// share a point have that point in both of their bounding boxes, so a box
/// that the sight line's box does not touch holds no segment it meets - the
/// test is skipped, never decided. The comparisons are on the coordinates
/// themselves, so there is no rounding to argue about.
#[derive(Debug, Clone, PartialEq)]
pub struct Barriers {
    segments: Vec<[[f64; 2]; 2]>,
    /// The box of each run of [`CHUNK`] consecutive segments.
    boxes: Vec<[[f64; 2]; 2]>,
}

impl Barriers {
    /// Build from polylines of plan-view positions.
    ///
    /// Refuses by name a line with fewer than two positions (it blocks
    /// nothing, and was surely meant to) and a non-finite coordinate.
    pub fn from_polylines(lines: &[Vec<[f64; 2]>]) -> Result<Barriers, Error> {
        let mut segments = Vec::new();
        for (at, line) in lines.iter().enumerate() {
            if line.len() < 2 {
                return Err(Error::BadBarrier {
                    what: format!("line {at} has {} position(s), a fault needs two", line.len()),
                });
            }
            for pair in line.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                if !(a.iter().chain(b.iter()).all(|c| c.is_finite())) {
                    return Err(Error::BadBarrier {
                        what: format!("line {at} has a non-finite coordinate"),
                    });
                }
                // A zero-length piece blocks nothing; kept out rather than
                // let it make every orientation test degenerate.
                if a != b {
                    segments.push([a, b]);
                }
            }
        }
        let boxes = segments
            .chunks(CHUNK)
            .map(|run| bounds(run.iter().flatten().copied()))
            .collect();
        Ok(Barriers { segments, boxes })
    }

    /// Whether no fault was given at all.
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// How many segments the traces make.
    pub fn len(&self) -> usize {
        self.segments.len()
    }

    /// Whether the straight segment `a`-`b` crosses (or touches) any fault.
    pub fn blocks(&self, a: [f64; 2], b: [f64; 2]) -> bool {
        let sight = bounds([a, b]);
        self.boxes
            .iter()
            .zip(self.segments.chunks(CHUNK))
            .any(|(around, run)| {
                overlap(&sight, around)
                    && run
                        .iter()
                        .any(|[c, d]| overlap(&sight, &bounds([*c, *d])) && segments_meet(a, b, *c, *d))
            })
    }
}

/// Of `near` - `(distance, sample)` pairs already in reach - keep the ones no
/// fault hides, and of those the `cap` nearest when there is a cap.
///
/// With a cap the candidates are walked **nearest first**, and the walk stops
/// at the `cap`-th visible one: the same neighbourhood as testing every
/// candidate and then taking the nearest, for the fault tests of the few that
/// are kept. The cap applies *after* the fault, never before - a wall of near
/// samples behind a trace must not crowd out the only visible one. Ties are
/// broken by the sample's rank, so the answer does not depend on a sort's
/// whims.
pub(crate) fn keep_visible(near: &mut Vec<(f64, usize)>, cap: Option<usize>, hidden: impl Fn(usize) -> bool) {
    match cap {
        None => near.retain(|&(_, at)| !hidden(at)),
        Some(cap) => {
            near.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            let mut kept = 0;
            for read in 0..near.len() {
                if kept == cap {
                    break;
                }
                if !hidden(near[read].1) {
                    near[kept] = near[read];
                    kept += 1;
                }
            }
            near.truncate(kept);
        }
    }
}

/// The closed box around some positions: `[low, high]`.
fn bounds(points: impl IntoIterator<Item = [f64; 2]>) -> [[f64; 2]; 2] {
    points
        .into_iter()
        .fold([[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]], |[low, high], p| {
            [
                [low[0].min(p[0]), low[1].min(p[1])],
                [high[0].max(p[0]), high[1].max(p[1])],
            ]
        })
}

/// Whether two closed boxes share a point - touching counts, as it does for
/// the segments.
fn overlap(a: &[[f64; 2]; 2], b: &[[f64; 2]; 2]) -> bool {
    a[0][0] <= b[1][0] && b[0][0] <= a[1][0] && a[0][1] <= b[1][1] && b[0][1] <= a[1][1]
}

/// Signed area of the triangle `a b c`: > 0 anticlockwise, < 0 clockwise,
/// 0 collinear.
fn orient(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// Whether `c` lies on the closed segment `a`-`b`, `c` being collinear with it.
fn on_segment(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> bool {
    c[0] >= a[0].min(b[0]) && c[0] <= a[0].max(b[0]) && c[1] >= a[1].min(b[1]) && c[1] <= a[1].max(b[1])
}

/// Whether the closed segments `a`-`b` and `c`-`d` share at least one point.
///
/// The textbook orientation test, endpoints included: a proper crossing has
/// `c` and `d` on opposite sides of `ab` **and** `a`, `b` on opposite sides of
/// `cd`; the collinear cases are settled by a bounding check.
fn segments_meet(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let o1 = orient(a, b, c);
    let o2 = orient(a, b, d);
    let o3 = orient(c, d, a);
    let o4 = orient(c, d, b);
    if ((o1 > 0.0 && o2 < 0.0) || (o1 < 0.0 && o2 > 0.0))
        && ((o3 > 0.0 && o4 < 0.0) || (o3 < 0.0 && o4 > 0.0))
    {
        return true;
    }
    (o1 == 0.0 && on_segment(a, b, c))
        || (o2 == 0.0 && on_segment(a, b, d))
        || (o3 == 0.0 && on_segment(c, d, a))
        || (o4 == 0.0 && on_segment(c, d, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall() -> Barriers {
        // A vertical fault at x = 5, from y = 0 to y = 10.
        Barriers::from_polylines(&[vec![[5.0, 0.0], [5.0, 10.0]]]).unwrap()
    }

    #[test]
    fn a_segment_through_the_fault_is_blocked_and_one_beside_it_is_not() {
        let fault = wall();
        assert!(fault.blocks([0.0, 5.0], [10.0, 5.0]));
        // Same side: free.
        assert!(!fault.blocks([0.0, 2.0], [4.0, 8.0]));
        // Past the tip of the fault: free - the geometry says so, and a
        // shadow beyond the tip would be a decision nobody stated.
        assert!(!fault.blocks([0.0, 12.0], [10.0, 12.0]));
    }

    #[test]
    fn touching_counts_as_crossing() {
        let fault = wall();
        // A sample sitting exactly on the trace.
        assert!(fault.blocks([5.0, 5.0], [9.0, 5.0]));
        // A segment that ends on the fault's tip.
        assert!(fault.blocks([0.0, 10.0], [5.0, 10.0]));
        // Collinear overlap along the trace.
        assert!(fault.blocks([5.0, 2.0], [5.0, 4.0]));
    }

    #[test]
    fn a_polyline_blocks_along_every_piece_and_a_bent_one_is_not_a_chord() {
        // An L: what the two arms block, the chord between the ends does not.
        let bent = Barriers::from_polylines(&[vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]]]).unwrap();
        assert_eq!(bent.len(), 2);
        assert!(bent.blocks([5.0, -1.0], [5.0, 1.0]));
        assert!(bent.blocks([9.0, 5.0], [11.0, 5.0]));
        // Crossing the chord (0,0)-(10,10) but neither arm: free.
        assert!(!bent.blocks([2.0, 6.0], [6.0, 2.0]));
    }

    #[test]
    fn a_one_point_line_and_a_nan_are_refused_by_name() {
        let why = Barriers::from_polylines(&[vec![[1.0, 1.0]]]).unwrap_err();
        assert!(why.to_string().contains("line 0"), "{why}");
        let why = Barriers::from_polylines(&[vec![[0.0, 0.0], [f64::NAN, 1.0]]]).unwrap_err();
        assert!(why.to_string().contains("non-finite"), "{why}");
    }

    /// **The boxes decide nothing** - confronted with the plain test against
    /// every segment, over sight lines and traces drawn at random, touching
    /// cases included (positions on a coarse lattice, so that endpoints and
    /// collinear pieces are frequent rather than never).
    #[test]
    fn the_boxes_skip_tests_and_never_change_an_answer() {
        let mut state = 7u64;
        let mut draw = || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((state >> 33) % 41) as f64
        };
        // Short walks: traces that wander locally, as faults do, so that a
        // sight line is blocked often but not always.
        let step = |at: f64, by: f64| (at + by % 5.0 - 2.0).clamp(0.0, 40.0);
        let lines: Vec<Vec<[f64; 2]>> = (0..12)
            .map(|_| {
                let mut at = [draw(), draw()];
                (0..20)
                    .map(|_| {
                        at = [step(at[0], draw()), step(at[1], draw())];
                        at
                    })
                    .collect()
            })
            .collect();
        let faults = Barriers::from_polylines(&lines).unwrap();
        assert!(
            faults.boxes.len() > 1,
            "several boxes, or the prefilter is not exercised"
        );
        let (mut blocked, mut free) = (0, 0);
        for _ in 0..20_000 {
            let a = [draw(), draw()];
            let b = [
                (a[0] + draw() % 17.0 - 8.0).clamp(0.0, 40.0),
                (a[1] + draw() % 17.0 - 8.0).clamp(0.0, 40.0),
            ];
            let plain = faults.segments.iter().any(|[c, d]| segments_meet(a, b, *c, *d));
            assert_eq!(faults.blocks(a, b), plain, "{a:?}-{b:?}");
            if plain { blocked += 1 } else { free += 1 }
        }
        assert!(
            blocked > 1000 && free > 1000,
            "both answers exercised: {blocked} blocked, {free} free"
        );
    }

    /// With a cap, the nearest-first walk keeps **exactly** the neighbourhood
    /// that testing every candidate and then taking the nearest keeps.
    #[test]
    fn the_nearest_visible_are_those_of_the_plain_rule() {
        let mut state = 11u64;
        let mut draw = || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) % 1000
        };
        for round in 0..200 {
            let near: Vec<(f64, usize)> = (0..60).map(|at| ((draw() % 50) as f64, at)).collect();
            let hidden: Vec<bool> = (0..60).map(|_| draw() % 3 == 0).collect();
            for cap in [1, 5, 20, 59, 100] {
                let mut plain: Vec<(f64, usize)> =
                    near.iter().copied().filter(|&(_, at)| !hidden[at]).collect();
                plain.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                plain.truncate(cap);
                let mut walked = near.clone();
                keep_visible(&mut walked, Some(cap), |at| hidden[at]);
                assert_eq!(walked, plain, "round {round}, cap {cap}");
            }
            let mut all = near.clone();
            keep_visible(&mut all, None, |at| hidden[at]);
            assert_eq!(all.len(), hidden.iter().filter(|h| !**h).count());
        }
    }

    #[test]
    fn no_fault_blocks_nothing() {
        let none = Barriers::from_polylines(&[]).unwrap();
        assert!(none.is_empty());
        assert!(!none.blocks([0.0, 0.0], [100.0, 100.0]));
    }
}
