//! **A grid of buckets over 2D boxes**: the one index of this workspace for
//! "which of these many things lie near here?", so that no caller sweeps every
//! element for every query.
//!
//! The boxes are filed once, each in every bucket of a regular grid it overlaps,
//! the grid sized to about one box per bucket. A query then opens the buckets
//! its window touches and nothing else.
//!
//! # What it answers
//!
//! - [`Grid::query`]: the boxes that meet a window - **exactly** the set a
//!   linear scan with the same closed-box test gives (touching counts), as
//!   ascending ids. What the grid saves is the boxes it never looks at, never
//!   precision: the tests of this module confront it with the scan on
//!   fabricated data.
//! - [`Grid::nearest`]: the box whose item is nearest a position, by the
//!   caller's own distance, the lowest id winning a tie - the answer of a scan
//!   that keeps the first minimum.
//! - [`Grid::candidates_at`]: the ids filed in the bucket of one position, for a
//!   caller whose own test then decides (a face under a point).
//!
//! # What it does not do
//!
//! - A box with a non-finite bound is not indexable: it is **left out**, counted
//!   by [`Grid::left_out`], and never returned - the caller decides what such a
//!   box means rather than the grid losing it silently.
//! - A box that spans most of the grid is filed in most buckets, so a set of
//!   boxes that all span the whole extent costs as much as the scan it replaces,
//!   in memory too. Surface faces, grid cells and the segments of a trace do
//!   not.
//! - It is built once and never updated; `energyml_section::spatial::Index` is
//!   the R-tree with an overflow list for what grows.
//!
//! The module knows nothing of EnergyML: it compiles to wasm and serves any
//! caller of this crate.

/// A 2D box as `(low, high)` corners, `[x, y]` each. The corners may be given in
/// any order: the grid takes the smaller and the larger of each axis.
pub type Bounds = ([f64; 2], [f64; 2]);

/// The most buckets on one axis: a grid is about one box per bucket, but one
/// axis never holds more than this, so a million boxes cost a million buckets
/// and not more.
const MOST_PER_AXIS: usize = 1024;

/// The boxes, filed in a grid of buckets.
#[derive(Debug, Clone)]
pub struct Grid {
    /// Every box, normalised, in the order given; a left-out box is `None`.
    boxes: Vec<Option<Bounds>>,
    /// The ids filed in each bucket, row after row.
    cells: Vec<Vec<u32>>,
    min: [f64; 2],
    size: [f64; 2],
    counts: [usize; 2],
    left_out: usize,
    /// What the rounding of a coordinate can move a box by: a billionth of the
    /// largest coordinate.
    slack: f64,
}

fn normalised((a, b): Bounds) -> Option<Bounds> {
    if !a.iter().chain(b.iter()).all(|v| v.is_finite()) {
        return None;
    }
    Some(([a[0].min(b[0]), a[1].min(b[1])], [a[0].max(b[0]), a[1].max(b[1])]))
}

/// The closed boxes share a point.
fn meets(one: &Bounds, other: &Bounds) -> bool {
    one.0[0] <= other.1[0] && other.0[0] <= one.1[0] && one.0[1] <= other.1[1] && other.0[1] <= one.1[1]
}

impl Grid {
    /// The boxes filed; the id of a box is its place in `boxes`. A box past the
    /// `u32::MAX`th is left out like a non-finite one.
    pub fn new(boxes: &[Bounds]) -> Grid {
        let mut left_out = 0usize;
        let boxes: Vec<Option<Bounds>> = boxes
            .iter()
            .enumerate()
            .map(|(at, one)| {
                let kept = if u32::try_from(at).is_ok() {
                    normalised(*one)
                } else {
                    None
                };
                if kept.is_none() {
                    left_out += 1;
                }
                kept
            })
            .collect();
        let kept = boxes.len() - left_out;
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        for (low, high) in boxes.iter().flatten() {
            for axis in 0..2 {
                min[axis] = min[axis].min(low[axis]);
                max[axis] = max[axis].max(high[axis]);
            }
        }
        if kept == 0 {
            return Grid {
                boxes,
                cells: Vec::new(),
                min: [0.0; 2],
                size: [1.0; 2],
                counts: [0; 2],
                left_out,
                slack: 0.0,
            };
        }
        // About one box per bucket, the buckets as square as the extent allows;
        // a zero span is given a hair of width so a bucket has a size.
        let scale = (max[0] - min[0]).max(max[1] - min[1]).max(1.0);
        let span = [
            (max[0] - min[0]).max(scale * 1e-12),
            (max[1] - min[1]).max(scale * 1e-12),
        ];
        let per_axis = |mine: f64, other: f64| {
            ((kept as f64 * mine / other).sqrt().ceil() as usize).clamp(1, MOST_PER_AXIS)
        };
        let counts = [per_axis(span[0], span[1]), per_axis(span[1], span[0])];
        let mut grid = Grid {
            boxes,
            cells: vec![Vec::new(); counts[0] * counts[1]],
            min,
            size: [span[0] / counts[0] as f64, span[1] / counts[1] as f64],
            counts,
            left_out,
            slack: 1e-9 * (1.0 + min.iter().chain(max.iter()).fold(0.0f64, |m, v| m.max(v.abs()))),
        };
        for at in 0..grid.boxes.len() {
            let Some((low, high)) = grid.boxes[at] else {
                continue;
            };
            let (i0, j0) = grid.cell_of(low);
            let (i1, j1) = grid.cell_of(high);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    grid.cells[j * counts[0] + i].push(at as u32);
                }
            }
        }
        grid
    }

    /// How many boxes were given, left-out ones included.
    pub fn len(&self) -> usize {
        self.boxes.len()
    }

    /// No box was given.
    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty()
    }

    /// How many boxes were not indexable (a non-finite bound).
    pub fn left_out(&self) -> usize {
        self.left_out
    }

    /// The bucket a position falls in, clamped to the grid.
    fn cell_of(&self, at: [f64; 2]) -> (usize, usize) {
        let one = |v: f64, axis: usize| {
            (((v - self.min[axis]) / self.size[axis]).floor().max(0.0) as usize).min(self.counts[axis] - 1)
        };
        (one(at[0], 0), one(at[1], 1))
    }

    /// The ids filed in the bucket of `(x, y)`, a position past the grid landing
    /// in its edge bucket. Every box that holds the position is among them; so
    /// are others, which the caller's own test sets aside.
    pub fn candidates_at(&self, x: f64, y: f64) -> &[u32] {
        if self.cells.is_empty() || !x.is_finite() || !y.is_finite() {
            return &[];
        }
        let (i, j) = self.cell_of([x, y]);
        &self.cells[j * self.counts[0] + i]
    }

    /// The ids of the boxes that meet the window `low..high` (closed: touching
    /// counts), ascending - what a scan with the same test gives. A window with
    /// a non-finite bound meets nothing.
    pub fn query(&self, low: [f64; 2], high: [f64; 2]) -> Vec<usize> {
        let Some(window) = normalised((low, high)) else {
            return Vec::new();
        };
        if self.cells.is_empty() {
            return Vec::new();
        }
        let (i0, j0) = self.cell_of(window.0);
        let (i1, j1) = self.cell_of(window.1);
        let mut ids: Vec<u32> = Vec::new();
        for j in j0..=j1 {
            for i in i0..=i1 {
                ids.extend_from_slice(&self.cells[j * self.counts[0] + i]);
            }
        }
        ids.sort_unstable();
        ids.dedup();
        ids.into_iter()
            .filter(|&id| {
                self.boxes[id as usize]
                    .as_ref()
                    .is_some_and(|one| meets(one, &window))
            })
            .map(|id| id as usize)
            .collect()
    }

    /// The id whose `distance` to `at` is the smallest, with that distance, the
    /// lowest id winning a tie; `None` when no box is indexed, every distance is
    /// `NaN`, or none is within `limit` (pass `f64::INFINITY` for no limit). The
    /// buckets further than the limit are never opened, which is what makes a
    /// "nearest, if within a reach" cheap on a position far from everything.
    ///
    /// `distance(id)` is the caller's own measure and **must not be smaller than
    /// the plan distance from `at` to the box of `id`** - the search stops once
    /// the buckets still to open are further than the best answer, and a measure
    /// that reaches below the box would be cut short. It may be called more than
    /// once for one id (a box is filed in every bucket it overlaps).
    pub fn nearest(
        &self,
        at: [f64; 2],
        limit: f64,
        mut distance: impl FnMut(usize) -> f64,
    ) -> Option<(usize, f64)> {
        if self.cells.is_empty() || !at[0].is_finite() || !at[1].is_finite() {
            return None;
        }
        let (ci, cj) = self.cell_of(at);
        let (ni, nj) = (self.counts[0] as i64, self.counts[1] as i64);
        let (ci, cj) = (ci as i64, cj as i64);
        // How far the position is from the grid's own rectangle, per axis (zero
        // inside): every box lies within it, so no box is nearer than that.
        let out = [0, 1].map(|axis| {
            let (low, high) = (
                self.min[axis],
                self.min[axis] + self.counts[axis] as f64 * self.size[axis],
            );
            (low - at[axis]).max(at[axis] - high).max(0.0)
        });
        let edge = |axis: usize, index: i64| self.min[axis] + index as f64 * self.size[axis];
        let mut best: Option<(usize, f64)> = None;
        let mut consider = |id: u32, best: &mut Option<(usize, f64)>| {
            let id = id as usize;
            let d = distance(id);
            if d.is_nan() || d > limit {
                return;
            }
            if (*best).is_none_or(|(held, kept)| d < kept || (d == kept && id < held)) {
                *best = Some((id, d));
            }
        };
        for r in 0..=ni.max(nj) {
            if r > 0 {
                // The ring `r` is what lies beyond the block of rings `0..r`. Its
                // nearest bucket is past one of the block's four sides - if the
                // grid goes on there - at least as far as that side from the
                // position, and as far as the grid's rectangle is on the other
                // axis. When no side has any bucket left, the grid is done.
                let sides = [
                    (ci + r < ni).then(|| (edge(0, ci + r) - at[0], out[1])),
                    (ci - r >= 0).then(|| (at[0] - edge(0, ci - r + 1), out[1])),
                    (cj + r < nj).then(|| (edge(1, cj + r) - at[1], out[0])),
                    (cj - r >= 0).then(|| (at[1] - edge(1, cj - r + 1), out[0])),
                ];
                let Some(bound) = sides
                    .into_iter()
                    .flatten()
                    .map(|(along, across)| along.max(0.0).hypot(across))
                    .reduce(f64::min)
                else {
                    break;
                };
                let held = best.map_or(limit, |(_, kept)| kept.min(limit));
                if bound - self.slack > held {
                    break;
                }
            }
            for j in (cj - r).max(0)..=(cj + r).min(nj - 1) {
                let on_edge_row = (j - cj).abs() == r;
                let step = if on_edge_row { 1 } else { (2 * r).max(1) };
                let mut i = ci - r;
                while i <= ci + r {
                    if (0..ni).contains(&i) {
                        for &id in &self.cells[(j * ni + i) as usize] {
                            consider(id, &mut best);
                        }
                    }
                    i += step;
                }
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small deterministic generator: a fixed seed, no dependency.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    fn fabricated(count: usize, seed: u64) -> Vec<Bounds> {
        let mut rng = Lcg(seed);
        (0..count)
            .map(|_| {
                let (x, y) = (rng.next() * 1000.0, rng.next() * 400.0);
                let (w, h) = (rng.next() * 12.0, rng.next() * 12.0);
                ([x, y], [x + w, y + h])
            })
            .collect()
    }

    fn scan(boxes: &[Bounds], low: [f64; 2], high: [f64; 2]) -> Vec<usize> {
        let window = normalised((low, high)).unwrap();
        (0..boxes.len())
            .filter(|&at| normalised(boxes[at]).is_some_and(|one| meets(&one, &window)))
            .collect()
    }

    /// The grid is confronted with the scan: same ids, same order, on windows of
    /// every size, a window past the grid, touching boxes and a point.
    #[test]
    fn a_query_gives_the_scans_answer() {
        let boxes = fabricated(3000, 7);
        let grid = Grid::new(&boxes);
        let mut rng = Lcg(99);
        for _ in 0..300 {
            let (x, y) = (rng.next() * 1100.0 - 50.0, rng.next() * 500.0 - 50.0);
            let (w, h) = (rng.next() * 80.0, rng.next() * 80.0);
            assert_eq!(
                grid.query([x, y], [x + w, y + h]),
                scan(&boxes, [x, y], [x + w, y + h])
            );
        }
        assert_eq!(
            grid.query([-1e9, -1e9], [1e9, 1e9]),
            scan(&boxes, [-1e9, -1e9], [1e9, 1e9])
        );
        // Touching counts: a window whose edge is a box's edge.
        let touch = ([10.0, 10.0], [20.0, 20.0]);
        let grid = Grid::new(&[touch]);
        assert_eq!(grid.query([20.0, 20.0], [30.0, 30.0]), vec![0]);
        assert_eq!(grid.query([20.5, 20.0], [30.0, 30.0]), Vec::<usize>::new());
        // Corners in any order.
        assert_eq!(grid.query([30.0, 30.0], [20.0, 20.0]), vec![0]);
        // Break it: file each box only in the bucket of its low corner, or test
        // the window with `<` instead of `<=`, and the two disagree here.
    }

    /// A box with a bound that is not a number is left out, and counted; the
    /// window of a `NaN` meets nothing.
    #[test]
    fn a_box_that_is_not_finite_is_left_out_and_counted() {
        let boxes = [
            ([0.0, 0.0], [1.0, 1.0]),
            ([f64::NAN, 0.0], [1.0, 1.0]),
            ([0.0, 0.0], [f64::INFINITY, 1.0]),
        ];
        let grid = Grid::new(&boxes);
        assert_eq!(grid.left_out(), 2);
        assert_eq!(grid.len(), 3);
        assert_eq!(grid.query([-1.0, -1.0], [2.0, 2.0]), vec![0]);
        assert!(grid.query([f64::NAN, 0.0], [1.0, 1.0]).is_empty());
        assert!(Grid::new(&[]).query([0.0; 2], [1.0; 2]).is_empty());
        // Break it: drop the finiteness check in `normalised` and the second
        // box is filed under a NaN bucket.
    }

    fn segment_distance(p: [f64; 2], (a, b): Bounds) -> f64 {
        // The distance from `p` to the box itself: what a scan of boxes keeps.
        let dx = (a[0] - p[0]).max(0.0).max(p[0] - b[0]);
        let dy = (a[1] - p[1]).max(0.0).max(p[1] - b[1]);
        dx.hypot(dy)
    }

    /// `nearest` is the scan's first minimum, ties included, for positions inside
    /// the grid, past it, and on a bucket border; and it opens few boxes.
    #[test]
    fn the_nearest_is_the_scans_first_minimum_and_few_boxes_are_measured() {
        let boxes = fabricated(20_000, 3);
        let grid = Grid::new(&boxes);
        let mut rng = Lcg(5);
        let mut measured = 0usize;
        for _ in 0..200 {
            let p = [rng.next() * 1300.0 - 150.0, rng.next() * 600.0 - 100.0];
            let mut calls = 0usize;
            let got = grid.nearest(p, f64::INFINITY, |id| {
                calls += 1;
                segment_distance(p, boxes[id])
            });
            let mut want: Option<(usize, f64)> = None;
            for (id, one) in boxes.iter().enumerate() {
                let d = segment_distance(p, *one);
                if want.is_none_or(|(_, kept)| d < kept) {
                    want = Some((id, d));
                }
            }
            assert_eq!(got, want, "at {p:?}");
            measured += calls;
        }
        // The counter of operations, not a clock: a scan measures 20 000 boxes
        // per position, the grid a few dozen.
        assert!(
            measured < 200 * 20_000 / 50,
            "the grid measured {measured} boxes for 200 positions"
        );
        // A limit: the answer of the scan when it is within, nothing when not, and
        // fewer boxes measured still.
        let p = [500.0, 200.0];
        let scanned = (0..boxes.len())
            .map(|id| (id, segment_distance(p, boxes[id])))
            .fold(None::<(usize, f64)>, |best, (id, d)| {
                if best.is_none_or(|(_, kept)| d < kept) {
                    Some((id, d))
                } else {
                    best
                }
            })
            .unwrap();
        assert_eq!(
            grid.nearest(p, scanned.1, |id| segment_distance(p, boxes[id])),
            Some(scanned)
        );
        assert_eq!(
            grid.nearest([1e6, 1e6], 10.0, |id| segment_distance([1e6, 1e6], boxes[id])),
            None
        );
        // Break it: make `nearest` open every bucket (drop the `break`) and the
        // count of measured boxes passes the bound.
    }

    /// Two boxes at the same distance: the lower id wins, as a scan that keeps the
    /// first minimum does.
    #[test]
    fn a_tie_goes_to_the_lower_id() {
        let boxes = [([0.0, 0.0], [1.0, 1.0]), ([2.0, 0.0], [3.0, 1.0])];
        let grid = Grid::new(&boxes);
        let p = [1.5, 0.5];
        let got = grid.nearest(p, f64::INFINITY, |id| segment_distance(p, boxes[id]));
        assert_eq!(got, Some((0, 0.5)));
        // Break it: drop the `id < held` arm and the last box seen wins.
    }

    #[test]
    fn the_candidates_of_a_position_hold_every_box_that_holds_it() {
        let boxes = fabricated(2000, 11);
        let grid = Grid::new(&boxes);
        let mut rng = Lcg(21);
        for _ in 0..500 {
            let p = [rng.next() * 1100.0 - 50.0, rng.next() * 500.0 - 50.0];
            let found = grid.candidates_at(p[0], p[1]);
            for (id, one) in boxes.iter().enumerate() {
                if meets(one, &(p, p)) {
                    assert!(found.contains(&(id as u32)), "box {id} holds {p:?}");
                }
            }
        }
        assert!(grid.candidates_at(f64::NAN, 0.0).is_empty());
        // Break it: clamp the bucket of a position with `min(counts)` instead of
        // `min(counts - 1)` and the lookup indexes past the grid.
    }
}
