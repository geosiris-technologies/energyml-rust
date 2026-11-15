//! The envelope of an IJK grid along a section (ADR-0015).
//!
//! A section does not show every cell wall of a grid: it shows where the grid
//! **is** - its roof and its floor along the trace, the rock between them, and
//! where it is cut by a fault or by nothing at all. The reading is the library's
//! (`energyml_geometry::ijk`, `parent`): hexahedra with the file's own cell
//! numbers, split coordinate lines honoured, K gaps honoured, undefined cells
//! dropped. What is done here is the cut.
//!
//! # How
//!
//! A cell is cut **by the section's vertical plane**, as `section::cut` cuts a
//! surface: its two faces across the layer (the ring of corners with the higher
//! mean height is the roof, the other the floor) are each two triangles, and
//! the cut of each is a short polyline in `(s, z)`. A cell the plane misses
//! gives nothing. The plan boxes of the cells are filed once, when the grid is
//! read, in the grid of buckets of `energyml_geostat::buckets`; a cut asks it,
//! for each segment of the trace, for the cells whose box the segment's box
//! meets, and tests those few against the segment exactly ([`crosses_box`]). A
//! grid of millions of cells so pays, per section, for the few hundred the trace
//! passes near, not for every cell (and a test counts the cells tried).
//!
//! Cells of one column (`i`, `j`) are then stacked, highest first, into **runs** -
//! consecutive layers with no K gap between them (the floor of one and the roof
//! of the next are not at the same height there) - and a run is a polygon:
//! the roof of its first cell, the floor of its last. A column's **roof** is the
//! roof of its highest cell and its **floor** the floor of its lowest, whatever
//! lies between.
//!
//! Neighbouring columns join when they **meet**: the end of one's roof is the
//! start of the next's, to the millimetre. Where the roofs meet in `s` and not in
//! `z`, the two columns do not share their pillars there - a split coordinate
//! line, a fault - and the roof (or the floor) **jumps**: the span is cut, and
//! the vertical step is a segment of `faults`. Where nothing meets in `s`, a
//! column with no defined cell lies between (`CellGeometryIsDefined`, a
//! sub-grid, a missing column): the span is cut, the fill is cut, and no step is
//! drawn - a hole is not a fault.
//!
//! # What is exact and what is not
//!
//! Exact: which cells the plane meets, the roof and floor of each as the plane
//! cuts them, every jump, every hole, the layer order and the K gaps.
//!
//! Approximated, and said where it matters:
//!
//! - a cell face is the **two triangles** `(0, 1, 2)` and `(0, 2, 3)` of its
//!   ring: a warped face is cut along its diagonal, not along a bilinear patch
//!   (the section view's surfaces are triangles all the same);
//! - the sides of a cell, and of a run of cells, are the **straight** line from
//!   the end of its roof to the end of its floor (a tilted pillar is one, a bent
//!   one is not);
//! - a cell the plane meets at **one face only** (the section clips an inclined
//!   cell's corner) is left out, and counted in the cut's notes.

use std::collections::{BTreeMap, HashMap};

use energyml_geometry::geometry::{Patch, Topology};
use energyml_geometry::section::{self, Cut};
use energyml_geostat::buckets;

use crate::model::{CutShape, SectionTrace, SurfaceCut};
use energyml_geostat::polyline::z_at;

type P = [f64; 2];

/// One cell, with what finding it takes.
#[derive(Clone, Copy, Debug)]
struct Cell {
    corners: [u32; 8],
    /// `(i, j)`.
    column: (u32, u32),
    layer: u32,
    /// Plan box, `[x min, y min, x max, y max]`.
    bbox: [f64; 4],
    /// The mean height of the eight corners: what orders a column.
    mean_z: f64,
}

/// An IJK grid's cells in the scene's system, before any section cuts them -
/// what a cache keeps while the trace moves.
#[derive(Clone, Debug)]
pub struct Grid {
    points: Vec<[f64; 3]>,
    cells: Vec<Cell>,
    /// The cells' plan boxes, filed: the id of a box is the cell's place in
    /// `cells`.
    index: buckets::Grid,
    /// How far [`crosses_box`] pads a cell's box, at most: the window of a
    /// query is grown by it, so no cell the exact test keeps is missed.
    pad: f64,
    /// About one cell's side: the length of the pieces a long segment is cut in
    /// to be looked up, so that its box is not the whole rectangle it spans.
    side: f64,
}

impl Grid {
    /// The cells of a patch read by `energyml_geometry` and placed in the
    /// scene. A patch that is not made of hexahedra is refused by name.
    pub fn from_patch(patch: &Patch) -> Result<Grid, String> {
        let Topology::Hexahedra {
            cells, dimensions, ..
        } = &patch.topology
        else {
            return Err("the grid did not read as hexahedra".to_string());
        };
        let [ni, nj, nk] = *dimensions;
        let per_layer = ni * nj;
        if per_layer == 0 {
            return Err(format!("the grid is {ni}x{nj}x{nk}"));
        }
        let mut out = Vec::with_capacity(cells.len());
        for (position, corners) in cells.iter().enumerate() {
            // The cell's number in the file, never its position: a grid with a
            // hole has dropped cells, and a position is then not a cell.
            let Some(number) = patch.topology.cell_at(position) else {
                continue;
            };
            let number = number as usize;
            let (layer, column) = (number / per_layer, number % per_layer);
            let at: Vec<[f64; 3]> = corners.iter().map(|c| patch.points[*c as usize]).collect();
            if at.iter().any(|p| p.iter().any(|v| !v.is_finite())) {
                continue;
            }
            let mut bbox = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
            for p in &at {
                bbox = [
                    bbox[0].min(p[0]),
                    bbox[1].min(p[1]),
                    bbox[2].max(p[0]),
                    bbox[3].max(p[1]),
                ];
            }
            out.push(Cell {
                corners: *corners,
                column: ((column % ni) as u32, (column / ni) as u32),
                layer: layer as u32,
                bbox,
                mean_z: at.iter().map(|p| p[2]).sum::<f64>() / 8.0,
            });
        }
        if out.is_empty() {
            return Err("the grid has no defined cell".to_string());
        }
        Ok(Grid::indexed(patch.points.clone(), out))
    }

    /// The cells filed in the grid of buckets by their plan boxes.
    fn indexed(points: Vec<[f64; 3]>, cells: Vec<Cell>) -> Grid {
        let boxes: Vec<buckets::Bounds> = cells
            .iter()
            .map(|cell| ([cell.bbox[0], cell.bbox[1]], [cell.bbox[2], cell.bbox[3]]))
            .collect();
        // The padding `crosses_box` gives a box grows with its coordinates; the
        // largest is the one of the cell furthest from the origin.
        let far = cells
            .iter()
            .map(|cell| cell.bbox[2].abs().max(cell.bbox[3].abs()))
            .filter(|v| v.is_finite())
            .fold(0.0f64, f64::max);
        let sides: Vec<f64> = cells
            .iter()
            .map(|cell| 0.5 * ((cell.bbox[2] - cell.bbox[0]) + (cell.bbox[3] - cell.bbox[1])))
            .filter(|v| v.is_finite())
            .collect();
        let side = sides.iter().sum::<f64>() / sides.len().max(1) as f64;
        Grid {
            points,
            cells,
            index: buckets::Grid::new(&boxes),
            pad: box_pad(far) * 1.000_001,
            side,
        }
    }

    /// The cells the trace may cross - every one [`crosses_box`] keeps, and a few
    /// more, in the order of `cells`: those whose box meets the box of a segment
    /// of the trace, grown by the padding.
    fn near(&self, trace: &[P]) -> Vec<usize> {
        let mut found: Vec<usize> = Vec::new();
        for pair in trace.windows(2) {
            let (p, q) = (pair[0], pair[1]);
            // A segment is looked up in pieces about a cell long: the box of a
            // long diagonal segment is the whole rectangle it spans, the box of
            // a piece only the cells it runs through.
            let length = (q[0] - p[0]).hypot(q[1] - p[1]);
            let pieces = if length.is_finite() && self.side > 0.0 {
                (length / self.side).ceil().clamp(1.0, 65_536.0) as usize
            } else {
                1
            };
            let along = |k: usize| {
                if k == 0 {
                    p
                } else if k == pieces {
                    q
                } else {
                    let t = k as f64 / pieces as f64;
                    [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]
                }
            };
            for k in 0..pieces {
                let (a, b) = (along(k), along(k + 1));
                found.extend(self.index.query(
                    [a[0].min(b[0]) - self.pad, a[1].min(b[1]) - self.pad],
                    [a[0].max(b[0]) + self.pad, a[1].max(b[1]) + self.pad],
                ));
            }
        }
        found.sort_unstable();
        found.dedup();
        found
    }

    /// The grid cut along every section; a section it does not meet has no entry.
    pub fn cut(&self, sections: &[SectionTrace]) -> Vec<SurfaceCut> {
        sections.iter().filter_map(|one| self.cut_along(one)).collect()
    }

    fn cut_along(&self, section: &SectionTrace) -> Option<SurfaceCut> {
        let trace = &section.trace;
        let length: f64 = trace
            .windows(2)
            .map(|p| (p[1][0] - p[0][0]).hypot(p[1][1] - p[0][1]))
            .sum();
        // What "the same place" is: a millimetre in a kilometre, as a pair of
        // neighbouring cells computes the same point on their shared edge in
        // two orders.
        let tol_s = 1e-6 * length.max(1.0);

        let mut cut_cells: Vec<CellCut> = Vec::new();
        let mut clipped = 0usize;
        for at in self.near(trace) {
            let cell = &self.cells[at];
            if !crosses_box(trace, cell.bbox) {
                continue;
            }
            match self.cut_cell(cell, trace, tol_s) {
                Tried::Cut(done) => cut_cells.push(done),
                Tried::Clipped => clipped += 1,
                Tried::Missed => {}
            }
        }
        if cut_cells.is_empty() {
            return None;
        }
        let z_max = cut_cells
            .iter()
            .flat_map(|c| c.top.iter().chain(&c.bottom))
            .fold(1.0f64, |m, p| m.max(p[1].abs()));
        let tol_z = 1e-6 * z_max;

        // Columns, as the section meets them: a column the trace leaves and
        // enters again is two stretches.
        let mut columns: BTreeMap<(u32, u32), Vec<CellCut>> = BTreeMap::new();
        for done in cut_cells {
            columns.entry(done.column).or_default().push(done);
        }
        let mut pieces: Vec<Piece> = Vec::new();
        for (_, mut cells) in columns {
            cells.sort_by(|a, b| a.s_range().0.total_cmp(&b.s_range().0));
            let mut stretches: Vec<Vec<CellCut>> = Vec::new();
            let mut reach = f64::NEG_INFINITY;
            for done in cells {
                let (from, to) = done.s_range();
                if stretches.is_empty() || from > reach + tol_s {
                    stretches.push(Vec::new());
                    reach = to;
                } else {
                    reach = reach.max(to);
                }
                stretches
                    .last_mut()
                    .expect("a stretch was just pushed")
                    .push(done);
            }
            for stretch in stretches {
                pieces.push(Piece::of(stretch, tol_z));
            }
        }
        pieces.sort_by(|a, b| a.start().total_cmp(&b.start()));

        let (top, mut faults) = join(pieces.iter().map(|p| &p.top), tol_s, tol_z);
        let (bottom, floor_steps) = join(pieces.iter().map(|p| &p.bottom), tol_s, tol_z);
        faults.extend(floor_steps);

        let mut fills: Vec<Vec<P>> = Vec::new();
        let mut open: HashMap<(u32, u32), usize> = HashMap::new();
        let mut runs: Vec<Run> = Vec::new();
        for piece in &pieces {
            for run in &piece.runs {
                let joined = open
                    .get(&run.key)
                    .copied()
                    .filter(|at| runs[*at].meets(run, tol_s, tol_z));
                match joined {
                    Some(at) => runs[at].extend(run),
                    None => {
                        open.insert(run.key, runs.len());
                        runs.push(run.clone());
                    }
                }
            }
        }
        for run in runs {
            let mut ring = run.upper;
            ring.extend(run.lower.into_iter().rev());
            if ring.len() >= 3 {
                fills.push(ring);
            }
        }

        let worth =
            |spans: Vec<Vec<P>>| -> Vec<Vec<P>> { spans.into_iter().filter(|s| s.len() >= 2).collect() };
        let mut notes = Vec::new();
        if clipped > 0 {
            notes.push(format!(
                "{clipped} cell(s) of the grid are met by this section at one face only (a corner of an inclined \
                 cell) and are left out of its envelope"
            ));
        }
        Some(SurfaceCut {
            section: section.key,
            shape: CutShape::Envelope {
                top: worth(top),
                bottom: worth(bottom),
                fills,
                faults,
            },
            notes,
            sources: Vec::new(),
        })
    }

    fn cut_cell(&self, cell: &Cell, trace: &[P], tol: f64) -> Tried {
        let at = |n: usize| self.points[cell.corners[n] as usize];
        let ring_z = |from: usize| (from..from + 4).map(|n| at(n)[2]).sum::<f64>() / 4.0;
        let (roof, floor) = if ring_z(0) >= ring_z(4) { (0, 4) } else { (4, 0) };
        let chain = |from: usize| -> Vec<P> {
            let local: Vec<[f64; 3]> = (from..from + 4).map(at).collect();
            let found = section::cut(&local, &[[0, 1, 2], [0, 2, 3]], trace);
            chain_of(&found, tol)
        };
        let (top, bottom) = (chain(roof), chain(floor));
        match (top.is_empty(), bottom.is_empty()) {
            (true, true) => Tried::Missed,
            (false, false) => Tried::Cut(CellCut {
                column: cell.column,
                layer: cell.layer,
                mean_z: cell.mean_z,
                top,
                bottom,
            }),
            _ => Tried::Clipped,
        }
    }
}

enum Tried {
    Cut(CellCut),
    Clipped,
    Missed,
}

/// A cell, cut: its roof and its floor along the section, `(s, z)` by `s`.
#[derive(Clone, Debug)]
struct CellCut {
    column: (u32, u32),
    layer: u32,
    mean_z: f64,
    top: Vec<P>,
    bottom: Vec<P>,
}

impl CellCut {
    fn s_range(&self) -> (f64, f64) {
        let ends = [
            self.top.first(),
            self.top.last(),
            self.bottom.first(),
            self.bottom.last(),
        ];
        let s = ends.iter().flatten().map(|p| p[0]);
        (
            s.clone().fold(f64::INFINITY, f64::min),
            s.fold(f64::NEG_INFINITY, f64::max),
        )
    }
}

/// A column's cells across one stretch of the section: its roof, its floor, and
/// the polygons of its runs.
#[derive(Clone, Debug)]
struct Piece {
    top: Vec<P>,
    bottom: Vec<P>,
    runs: Vec<Run>,
}

impl Piece {
    fn start(&self) -> f64 {
        self.top[0][0].min(self.bottom[0][0])
    }

    /// Cells stacked, highest first; a run ends where the layers are not
    /// consecutive or the floor of one is not the roof of the next.
    fn of(mut cells: Vec<CellCut>, tol_z: f64) -> Piece {
        cells.sort_by(|a, b| b.mean_z.total_cmp(&a.mean_z));
        let mut runs: Vec<Run> = Vec::new();
        let mut last: Option<&CellCut> = None;
        for cell in &cells {
            let continues = last.is_some_and(|prev| {
                prev.layer.abs_diff(cell.layer) == 1 && touches(&prev.bottom, &cell.top, tol_z)
            });
            if continues {
                let run = runs.last_mut().expect("a run is open");
                run.lower = cell.bottom.clone();
                run.key = (run.key.0.min(cell.layer), run.key.1.max(cell.layer));
            } else {
                runs.push(Run {
                    key: (cell.layer, cell.layer),
                    upper: cell.top.clone(),
                    lower: cell.bottom.clone(),
                });
            }
            last = Some(cell);
        }
        let highest = cells.first().expect("a stretch has a cell");
        let lowest = cells.last().expect("a stretch has a cell");
        Piece {
            top: highest.top.clone(),
            bottom: lowest.bottom.clone(),
            runs,
        }
    }
}

/// Consecutive layers of a column: the polygon between the roof of the first
/// and the floor of the last.
#[derive(Clone, Debug)]
struct Run {
    /// The layers it spans, lowest and highest number.
    key: (u32, u32),
    upper: Vec<P>,
    lower: Vec<P>,
}

impl Run {
    fn meets(&self, next: &Run, tol_s: f64, tol_z: f64) -> bool {
        let same = |a: Option<&P>, b: Option<&P>| match (a, b) {
            (Some(a), Some(b)) => (a[0] - b[0]).abs() <= tol_s && (a[1] - b[1]).abs() <= tol_z,
            _ => false,
        };
        same(self.upper.last(), next.upper.first()) && same(self.lower.last(), next.lower.first())
    }

    fn extend(&mut self, next: &Run) {
        self.upper.extend(next.upper.iter().skip(1));
        self.lower.extend(next.lower.iter().skip(1));
    }
}

/// Whether the floor `above` rests on the roof `below` where they overlap in
/// `s` - and `false` where they do not overlap at all, or where they are apart
/// (a K gap).
fn touches(above: &[P], below: &[P], tol_z: f64) -> bool {
    let (Some(a0), Some(a1), Some(b0), Some(b1)) = (above.first(), above.last(), below.first(), below.last())
    else {
        return false;
    };
    let (from, to) = (a0[0].max(b0[0]), a1[0].min(b1[0]));
    if from > to {
        return false;
    }
    let mid = 0.5 * (from + to);
    match (z_at(above, mid), z_at(below, mid)) {
        (Some(za), Some(zb)) => (za - zb).abs() <= tol_z,
        _ => false,
    }
}

/// The cut of one face as a chain of `(s, z)` by `s`, welded points merged.
fn chain_of(cuts: &[Cut], tol: f64) -> Vec<P> {
    let mut points: Vec<P> = cuts.iter().flat_map(|c| c.profile).collect();
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    points.dedup_by(|b, a| (a[0] - b[0]).abs() <= tol * 1e-3 && (a[1] - b[1]).abs() <= tol * 1e-3);
    points
}

/// Chains joined where they meet. Where one starts at the `s` another ends and
/// at another height, the line jumps: a new span and a **step**; where it starts
/// elsewhere, the new span has nothing to say of what lies between.
fn join<'a>(chains: impl Iterator<Item = &'a Vec<P>>, tol_s: f64, tol_z: f64) -> (Vec<Vec<P>>, Vec<Vec<P>>) {
    let mut spans: Vec<Vec<P>> = Vec::new();
    let mut steps: Vec<Vec<P>> = Vec::new();
    for chain in chains {
        let Some(first) = chain.first() else { continue };
        match spans.last_mut() {
            Some(span) => {
                let last = *span.last().expect("a span has a point");
                if (first[0] - last[0]).abs() <= tol_s {
                    if (first[1] - last[1]).abs() <= tol_z {
                        span.extend(chain.iter().skip(1));
                    } else {
                        steps.push(vec![last, *first]);
                        spans.push(chain.clone());
                    }
                } else {
                    spans.push(chain.clone());
                }
            }
            None => spans.push(chain.clone()),
        }
    }
    (spans, steps)
}

/// How far [`crosses_box`] pads a box whose farthest coordinate (in absolute
/// value) is `far`: a billionth of the coordinates and of one.
fn box_pad(far: f64) -> f64 {
    1e-9 * (1.0 + far)
}

/// Whether the trace passes through a plan box (the slab test, a hair wider than
/// the box so a cell the trace only grazes is still cut, and says so itself).
fn crosses_box(trace: &[P], bbox: [f64; 4]) -> bool {
    let pad = box_pad(bbox[2].abs().max(bbox[3].abs()));
    let (lo, hi) = ([bbox[0] - pad, bbox[1] - pad], [bbox[2] + pad, bbox[3] + pad]);
    trace.windows(2).any(|pair| {
        let (p, q) = (pair[0], pair[1]);
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        for axis in 0..2 {
            let d = q[axis] - p[axis];
            if d.abs() < 1e-15 {
                if p[axis] < lo[axis] || p[axis] > hi[axis] {
                    return false;
                }
            } else {
                let (a, b) = ((lo[axis] - p[axis]) / d, (hi[axis] - p[axis]) / d);
                t0 = t0.max(a.min(b));
                t1 = t1.min(a.max(b));
                if t0 > t1 {
                    return false;
                }
            }
        }
        true
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lattice of `n x n` square cells `side` apart, at map coordinates; only the
    /// plan boxes matter to the lookup, so the corners are left at zero.
    fn lattice(n: usize, side: f64) -> Grid {
        let (x0, y0) = (651_000.0, 6_861_000.0);
        let mut cells = Vec::new();
        for j in 0..n {
            for i in 0..n {
                let (x, y) = (x0 + i as f64 * side, y0 + j as f64 * side);
                cells.push(Cell {
                    corners: [0; 8],
                    column: (i as u32, j as u32),
                    layer: 0,
                    bbox: [x, y, x + side, y + side],
                    mean_z: 0.0,
                });
            }
        }
        Grid::indexed(Vec::new(), cells)
    }

    /// [section S5] The cells the cut tries are those the scan keeps, in the same
    /// order, and they are few: a trace across a grid of 90 000 cells tries a few
    /// hundred (the count is the evidence, no clock is involved).
    #[test]
    fn a_trace_tries_the_cells_it_crosses_and_not_every_cell() {
        let grid = lattice(300, 10.0);
        let (x0, y0) = (651_000.0, 6_861_000.0);
        let traces: Vec<Vec<P>> = vec![
            vec![
                [x0 + 5.0, y0 + 5.0],
                [x0 + 1500.0, y0 + 900.0],
                [x0 + 2990.0, y0 + 2990.0],
            ],
            // along cell edges, so the padding matters
            vec![[x0 + 100.0, y0 - 50.0], [x0 + 100.0, y0 + 3050.0]],
            // outside the grid altogether, and a point
            vec![[x0 - 500.0, y0 - 500.0], [x0 - 100.0, y0 - 100.0]],
            vec![[x0 + 50.0, y0 + 50.0]],
        ];
        for trace in &traces {
            let by_scan: Vec<usize> = (0..grid.cells.len())
                .filter(|&at| crosses_box(trace, grid.cells[at].bbox))
                .collect();
            let tried = grid.near(trace);
            let by_buckets: Vec<usize> = tried
                .iter()
                .copied()
                .filter(|&at| crosses_box(trace, grid.cells[at].bbox))
                .collect();
            assert_eq!(by_buckets, by_scan, "trace {trace:?}");
            assert!(
                tried.len() < 5000,
                "{} cells tried of {}",
                tried.len(),
                grid.cells.len()
            );
        }
        // Break it: have `near` return every cell index and the count passes the bound.
    }
}
