//! **Which cells** - a box of an IJK grid by its index ranges, and the cells a
//! wellbore path crosses with the measured depth of each entry and exit: the
//! computations behind the `subrepresentation` and `blocked-wellbore`
//! operations, without a file.
//!
//! # A cell, in index order
//!
//! A cell is given by its eight corners **in the order of its indices**, not
//! in the order a renderer wants: `[0..4]` on the level of the smaller K, going
//! round the column in the IJ circular node order `(0,0) → (1,0) → (1,1) →
//! (0,1)`, and `[4..8]` the same four on the level of the larger K. That order
//! is the RESQML one - *« the RESQML IJ node ordering within a column of the
//! grid starts at the (0,0) corner and then increases first [in I] then [in
//! J]: (0,0), (1,0), (1,1), (0,1) »* (RESQML Technical Usage Guide 2.2, draft,
//! §9.12, Figure 9-18 p. 152) - and it is what names a cell's faces.
//!
//! # The local faces
//!
//! *« For an IJK grid, there are 6 "faces per cell": 0=top, 1=bottom, 2-5 are
//! side faces following the IJ circular node ordering, described previously
//! […] Faces per cell indices appear in the grid connections representation
//! and the blocked wellbore representation »* (same guide, §9.12.3 p. 159). So:
//!
//! - **0** - the face on the level of the smaller K, **1** - on the larger;
//! - **2** - the side between corners 0 and 1 (`(0,0)-(1,0)`, the J− side),
//!   **3** - between 1 and 2 (I+), **4** - between 2 and 3 (J+), **5** -
//!   between 3 and 0 (I−).
//!
//! *Top* is read as the smaller K. That is what it is for a grid whose
//! `KDirection` is `down` - the one `ijk-grid-from-horizons` writes - and the
//! guide gives no other reading; a caller that means the geometric top of a
//! grid counted upwards says so before calling.
//!
//! # What a path crossing a cell is, and what is refused
//!
//! The path is a polyline whose nodes carry a measured depth. Between two nodes
//! it runs **straight**, and the MD along that chord is interpolated
//! **linearly**: exact at every node, and short of the arc by what a curved
//! hole bends between them - which [`Blocked::chord_excess`] measures, so a
//! caller says it rather than hides it. An MD step **shorter** than its chord
//! is refused: no hole is shorter than the straight line between two of its
//! points.
//!
//! The cells are intersected **exactly for what they are when their pillars
//! stand vertical and each column is a parallelogram in plan** - the grids
//! `ijk-grid-from-horizons` writes on a lattice. A layer boundary is then the
//! bilinear patch through its four corner heights over the column, a
//! quadratic along a straight chord, and where the path crosses it is a root.
//! A leaning pillar, or a column that is no parallelogram, is **refused by
//! name**: the surface between its corners is then no bilinear patch in plan,
//! and cutting it by a diagonal the file never chose would be a guess.
//!
//! A stretch of path **two cells of one grid both claim** - a path running
//! along the face they share - is refused, naming both: which cell holds it is
//! nothing the geometry decides. Several grids are taken **in the order
//! given**, the first that holds a stretch holding it - a local grid listed
//! before its parent, as a real model wants GTH-1 in the refined window. A cell
//! entered where a grid listed earlier stops holding the path has no entry
//! face in the second: it was not entered through one.

use std::fmt;

/// A point, `[x, y, z]`.
pub type Point = [f64; 3];

/// A micrometre, in plan: what a pillar may lean and a parallelogram be off by
/// and still be read as what it was meant to be - the drift of a sum of
/// spacings, nothing anybody means.
const PLAN_TOLERANCE: f64 = 1e-6;

/// A cell of an IJK grid, its corners in index order (module header).
#[derive(Debug, Clone, PartialEq)]
pub struct Hexahedron {
    /// Its RESQML index - `i + ni·(j + nj·k)`.
    pub index: u64,
    /// `[0..4]` on the smaller K, `(0,0) (1,0) (1,1) (0,1)`; `[4..8]` the same
    /// on the larger K.
    pub corners: [Point; 8],
}

/// A grid: the cells that have a shape. A cell whose geometry is not defined
/// is left out, and the path there is in no cell.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Grid {
    /// The cells.
    pub cells: Vec<Hexahedron>,
}

/// One interval of the path inside one cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Held {
    /// The rank of the grid, in the order given.
    pub grid: usize,
    /// The cell's RESQML index in that grid.
    pub cell: u64,
    /// The local faces the path enters and leaves by - `None` where it starts
    /// or ends inside the cell, or enters where an earlier grid lets it go.
    pub faces: [Option<u8>; 2],
}

/// The path, blocked: a node at every entry and exit, and what each interval
/// between two nodes is in.
#[derive(Debug, Clone, PartialEq)]
pub struct Blocked {
    /// The measured depth of each node, increasing.
    pub node_md: Vec<f64>,
    /// One per interval - `node_md.len() − 1` - the cell it lies in, or `None`
    /// for a stretch in no cell of any grid.
    pub intervals: Vec<Option<Held>>,
    /// The largest `(ΔMD − chord) / ΔMD` over the path's segments: how far the
    /// straight chords the MD is spread along fall short of the hole. Zero for
    /// a straight path.
    pub chord_excess: f64,
}

impl Blocked {
    /// The intervals that lie in a cell - `CellCount` (RESQML TRG 2.2 §5.32
    /// `IntervalGridCells` p. 128: *« The number of non-null entries in the
    /// grid indices array »*).
    pub fn cell_count(&self) -> usize {
        self.intervals.iter().filter(|one| one.is_some()).count()
    }
}

/// Why no cell is selected, no path blocked.
#[derive(Debug, Clone, PartialEq)]
pub enum CellsError {
    /// A range whose first index is past its last.
    Reversed {
        /// `i`, `j` or `k`.
        axis: char,
        /// The range.
        range: [usize; 2],
    },
    /// A range past the grid.
    Beyond {
        /// `i`, `j` or `k`.
        axis: char,
        /// Its last index.
        last: usize,
        /// The cells along that axis.
        count: usize,
    },
    /// Not one measured depth per node of the path.
    Mismatched {
        /// The nodes.
        nodes: usize,
        /// The depths.
        mds: usize,
    },
    /// A path of fewer than two nodes.
    TooShort(usize),
    /// A node with no position, or no depth.
    NotFinite {
        /// Its rank.
        node: usize,
    },
    /// A measured depth that does not increase.
    NotIncreasing {
        /// The rank of the node where it stops.
        node: usize,
        /// Its depth.
        md: f64,
        /// The one before.
        before: f64,
    },
    /// An MD step shorter than the straight line between its nodes.
    ShorterThanChord {
        /// The rank of the segment's first node.
        node: usize,
        /// The MD step.
        step: f64,
        /// The chord.
        chord: f64,
    },
    /// A pillar that is not vertical.
    Leaning {
        /// The grid's rank.
        grid: usize,
        /// The cell.
        cell: u64,
    },
    /// A column that is no parallelogram in plan.
    NotParallelogram {
        /// The grid's rank.
        grid: usize,
        /// The cell.
        cell: u64,
    },
    /// A stretch two cells of one grid both hold.
    Shared {
        /// The grid's rank.
        grid: usize,
        /// The two cells.
        cells: [u64; 2],
        /// Where, in MD.
        md: [f64; 2],
    },
    /// The path is in no cell of any grid.
    NoCell,
}

impl fmt::Display for CellsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CellsError::Reversed { axis, range } => {
                write!(
                    f,
                    "the {axis} range starts at {} and ends at {}: first, then last",
                    range[0], range[1]
                )
            }
            CellsError::Beyond { axis, last, count } => write!(
                f,
                "the {axis} range ends at {last}, and the grid has {count} cell(s) along {axis} - indices from 0 to {}",
                count.saturating_sub(1)
            ),
            CellsError::Mismatched { nodes, mds } => {
                write!(
                    f,
                    "{nodes} node(s) and {mds} measured depth(s): one depth per node"
                )
            }
            CellsError::TooShort(nodes) => {
                write!(f, "a path of {nodes} node(s) crosses nothing: two at least")
            }
            CellsError::NotFinite { node } => {
                write!(f, "node {node} of the path has no position or no depth")
            }
            CellsError::NotIncreasing { node, md, before } => write!(
                f,
                "the measured depth of node {node} is {md}, after {before}: depths increase along a hole"
            ),
            CellsError::ShorterThanChord { node, step, chord } => write!(
                f,
                "between nodes {node} and {} the MD grows by {step} and the straight line is {chord} long: no hole is shorter than the chord between two of its points",
                node + 1
            ),
            CellsError::Leaning { grid, cell } => write!(
                f,
                "cell {cell} of grid {grid} stands on a pillar that is not vertical: only vertical pillars are intersected here, where a layer boundary is the bilinear patch through its corners"
            ),
            CellsError::NotParallelogram { grid, cell } => write!(
                f,
                "the column of cell {cell} of grid {grid} is no parallelogram in plan: its layer boundaries are no bilinear patch over it, and cutting it by a diagonal would be a guess"
            ),
            CellsError::Shared { grid, cells, md } => write!(
                f,
                "cells {} and {} of grid {grid} both hold the path from MD {} to {}: it runs along a face they share, and which one holds it is nothing the geometry decides",
                cells[0], cells[1], md[0], md[1]
            ),
            CellsError::NoCell => write!(f, "the path crosses no cell of any grid"),
        }
    }
}

impl std::error::Error for CellsError {}

/// The cells of an IJK box, in RESQML order - `i` fastest, then `j`, then `k`.
///
/// `dimensions` is `[ni, nj, nk]`; each range is `[first, last]`, **both
/// included**, or `None` for the whole axis. A range reversed or past the grid
/// is refused by its axis.
pub fn ijk_box(dimensions: [usize; 3], ranges: [Option<[usize; 2]>; 3]) -> Result<Vec<u64>, CellsError> {
    let mut bounds = [[0usize; 2]; 3];
    for (axis, (range, count)) in ranges.iter().zip(dimensions).enumerate() {
        let name = ['i', 'j', 'k'][axis];
        bounds[axis] = match range {
            None => [0, count.saturating_sub(1)],
            Some([first, last]) => {
                if first > last {
                    return Err(CellsError::Reversed {
                        axis: name,
                        range: [*first, *last],
                    });
                }
                if *last >= count {
                    return Err(CellsError::Beyond {
                        axis: name,
                        last: *last,
                        count,
                    });
                }
                [*first, *last]
            }
        };
    }
    let [ni, nj, _] = dimensions;
    let mut out = Vec::new();
    for k in bounds[2][0]..=bounds[2][1] {
        for j in bounds[1][0]..=bounds[1][1] {
            for i in bounds[0][0]..=bounds[0][1] {
                out.push((i + ni * (j + nj * k)) as u64);
            }
        }
    }
    Ok(out)
}

/// What a piece of path starts or ends on.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Edge {
    /// A local face of the cell.
    Face(u8),
    /// A node of the path, not a face: the path goes on in the next segment,
    /// or starts or ends here.
    Node,
}

/// A piece of path inside one cell of one grid.
#[derive(Debug, Clone, Copy)]
struct Piece {
    cell: u64,
    md: [f64; 2],
    ends: [Edge; 2],
}

/// A cell, read once: its column in plan and its two level patches.
struct Column {
    index: u64,
    /// The corner `(0,0)` in plan.
    origin: [f64; 2],
    /// Along I, `(1,0) − (0,0)`, and along J, `(0,1) − (0,0)`.
    axes: [[f64; 2]; 2],
    /// The four corners in plan, in circular order.
    ring: [[f64; 2]; 4],
    /// The corner heights of each level, `[smaller K, larger K]`.
    heights: [[f64; 4]; 2],
    /// The column's box in plan, and its range of heights.
    reach: [[f64; 2]; 3],
}

fn column(grid: usize, cell: &Hexahedron) -> Result<Column, CellsError> {
    let c = &cell.corners;
    for n in 0..4 {
        if (c[n][0] - c[n + 4][0]).abs() > PLAN_TOLERANCE || (c[n][1] - c[n + 4][1]).abs() > PLAN_TOLERANCE {
            return Err(CellsError::Leaning {
                grid,
                cell: cell.index,
            });
        }
    }
    let ring = [
        [c[0][0], c[0][1]],
        [c[1][0], c[1][1]],
        [c[2][0], c[2][1]],
        [c[3][0], c[3][1]],
    ];
    // A parallelogram: its diagonals share their middle.
    if (ring[0][0] + ring[2][0] - ring[1][0] - ring[3][0]).abs() > PLAN_TOLERANCE
        || (ring[0][1] + ring[2][1] - ring[1][1] - ring[3][1]).abs() > PLAN_TOLERANCE
    {
        return Err(CellsError::NotParallelogram {
            grid,
            cell: cell.index,
        });
    }
    let axes = [
        [ring[1][0] - ring[0][0], ring[1][1] - ring[0][1]],
        [ring[3][0] - ring[0][0], ring[3][1] - ring[0][1]],
    ];
    let mut reach = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
    for corner in c {
        for axis in 0..3 {
            reach[axis][0] = reach[axis][0].min(corner[axis]);
            reach[axis][1] = reach[axis][1].max(corner[axis]);
        }
    }
    Ok(Column {
        index: cell.index,
        origin: ring[0],
        axes,
        ring,
        heights: [
            [c[0][2], c[1][2], c[2][2], c[3][2]],
            [c[4][2], c[5][2], c[6][2], c[7][2]],
        ],
        reach,
    })
}

fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

/// The roots of `a·t² + b·t + c` in `[low, high]`.
fn roots(a: f64, b: f64, c: f64, low: f64, high: f64) -> Vec<f64> {
    let scale = a.abs().max(b.abs()).max(c.abs());
    let mut out = Vec::new();
    if scale == 0.0 {
        // Identically zero: the path runs in the level itself. No crossing -
        // the stretch is then held by both cells it separates, and refused
        // as such.
        return out;
    }
    if a.abs() <= 1e-12 * scale {
        if b != 0.0 {
            out.push(-c / b);
        }
    } else {
        let discriminant = b * b - 4.0 * a * c;
        if discriminant >= 0.0 {
            // The stable form: no difference of two nearly equal numbers.
            let q = -0.5 * (b + b.signum() * discriminant.sqrt());
            if q != 0.0 {
                out.push(q / a);
                out.push(c / q);
            } else {
                out.push(0.0);
            }
        }
    }
    out.retain(|t| t.is_finite() && *t >= low && *t <= high);
    out
}

/// The pieces of the segment `a → b` inside one cell: `(t0, t1, ends)`.
fn inside(cell: &Column, a: Point, b: Point) -> Vec<(f64, f64, [Edge; 2])> {
    // -- in plan: the column clips the segment ---------------------------------
    let d = [b[0] - a[0], b[1] - a[1]];
    let turn = {
        let area = cross(
            [
                cell.ring[1][0] - cell.ring[0][0],
                cell.ring[1][1] - cell.ring[0][1],
            ],
            [
                cell.ring[3][0] - cell.ring[0][0],
                cell.ring[3][1] - cell.ring[0][1],
            ],
        );
        if area >= 0.0 { 1.0 } else { -1.0 }
    };
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let (mut enter, mut leave) = (Edge::Node, Edge::Node);
    for e in 0..4 {
        let p = cell.ring[e];
        let q = cell.ring[(e + 1) % 4];
        let edge = [q[0] - p[0], q[1] - p[1]];
        // Inside the column is the left of each edge (right when it turns the
        // other way): f(t) ≥ 0.
        let f0 = turn * cross(edge, [a[0] - p[0], a[1] - p[1]]);
        let slope = turn * cross(edge, d);
        let face = Edge::Face(2 + e as u8);
        if slope == 0.0 {
            if f0 < 0.0 {
                return Vec::new();
            }
            continue;
        }
        let at = -f0 / slope;
        if slope > 0.0 {
            // Entering across this edge.
            if at >= t0 {
                t0 = at;
                enter = face;
            }
        } else if at <= t1 {
            t1 = at;
            leave = face;
        }
    }
    if t0 >= t1 {
        return Vec::new();
    }

    // -- across the levels: where the path meets each bilinear patch -----------
    // (u, v) in the column's own axes, affine in t.
    let solve = |x: f64, y: f64| -> [f64; 2] {
        let r = [x - cell.origin[0], y - cell.origin[1]];
        let det = cross(cell.axes[0], cell.axes[1]);
        [cross(r, cell.axes[1]) / det, cross(cell.axes[0], r) / det]
    };
    let [u0, v0] = solve(a[0], a[1]);
    let [u1, v1] = solve(b[0], b[1]);
    let (du, dv) = (u1 - u0, v1 - v0);
    // z(t) − h(t) as a quadratic, for each level.
    let level = |h: [f64; 4]| -> [f64; 3] {
        // h(u, v) = h0 + (h1 − h0)·u + (h3 − h0)·v + (h0 − h1 + h2 − h3)·u·v
        let (h0, hu, hv, huv) = (h[0], h[1] - h[0], h[3] - h[0], h[0] - h[1] + h[2] - h[3]);
        let constant = h0 + hu * u0 + hv * v0 + huv * u0 * v0;
        let linear = hu * du + hv * dv + huv * (u0 * dv + v0 * du);
        let square = huv * du * dv;
        // g(t) = z(t) − h(t)
        [-square, (b[2] - a[2]) - linear, a[2] - constant]
    };
    let g = [level(cell.heights[0]), level(cell.heights[1])];
    let value = |q: [f64; 3], t: f64| (q[0] * t + q[1]) * t + q[2];

    let mut cuts: Vec<(f64, Edge)> = vec![(t0, enter), (t1, leave)];
    for (face, q) in g.iter().enumerate() {
        for t in roots(q[0], q[1], q[2], t0, t1) {
            cuts.push((t, Edge::Face(face as u8)));
        }
    }
    // By position; at one position a level face before a side, a side before
    // a node - through an edge the path touches two faces, and the K face is
    // the one named.
    let rank = |edge: &Edge| match edge {
        Edge::Face(0) | Edge::Face(1) => 0,
        Edge::Face(_) => 1,
        Edge::Node => 2,
    };
    cuts.sort_by(|x, y| x.0.total_cmp(&y.0).then(rank(&x.1).cmp(&rank(&y.1))));
    let mut out = Vec::new();
    for pair in cuts.windows(2) {
        let (s, e) = (pair[0].0, pair[1].0);
        if e <= s {
            continue;
        }
        let middle = 0.5 * (s + e);
        if value(g[0], middle) * value(g[1], middle) < 0.0 {
            // The edge each end sits on: the best-ranked cut at that position.
            let at = |t: f64| {
                cuts.iter()
                    .find(|cut| cut.0 == t)
                    .map(|cut| cut.1)
                    .unwrap_or(Edge::Node)
            };
            out.push((s, e, [at(s), at(e)]));
        }
    }
    out
}

/// Where along the path each of these measured depths falls - on the chords,
/// as [`block`] spreads the MD. A depth outside the path's is `None`.
pub fn positions(points: &[Point], mds: &[f64], at: &[f64]) -> Vec<Option<Point>> {
    at.iter()
        .map(|md| {
            let segment = mds.windows(2).position(|w| w[0] <= *md && *md <= w[1])?;
            let t = (md - mds[segment]) / (mds[segment + 1] - mds[segment]);
            let (a, b) = (points[segment], points[segment + 1]);
            Some([
                a[0] + t * (b[0] - a[0]),
                a[1] + t * (b[1] - a[1]),
                a[2] + t * (b[2] - a[2]),
            ])
        })
        .collect()
}

/// Block a path through grids - see the module header.
///
/// `points` and `mds` are the path's nodes and their measured depths; `grids`
/// are taken in the order given.
pub fn block(points: &[Point], mds: &[f64], grids: &[Grid]) -> Result<Blocked, CellsError> {
    if points.len() != mds.len() {
        return Err(CellsError::Mismatched {
            nodes: points.len(),
            mds: mds.len(),
        });
    }
    if points.len() < 2 {
        return Err(CellsError::TooShort(points.len()));
    }
    for (node, (point, md)) in points.iter().zip(mds).enumerate() {
        if !point.iter().all(|c| c.is_finite()) || !md.is_finite() {
            return Err(CellsError::NotFinite { node });
        }
        if node > 0 && *md <= mds[node - 1] {
            return Err(CellsError::NotIncreasing {
                node,
                md: *md,
                before: mds[node - 1],
            });
        }
    }
    let span = mds[mds.len() - 1] - mds[0];
    let md_tolerance = 1e-9 * span.max(1.0);
    let mut chord_excess = 0.0f64;
    for node in 0..points.len() - 1 {
        let (a, b) = (points[node], points[node + 1]);
        let chord = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt();
        let step = mds[node + 1] - mds[node];
        if step < chord - md_tolerance.max(1e-9 * chord) {
            return Err(CellsError::ShorterThanChord { node, step, chord });
        }
        chord_excess = chord_excess.max(((step - chord) / step).max(0.0));
    }

    // -- each grid on its own ----------------------------------------------------
    let mut held: Vec<Vec<Piece>> = Vec::with_capacity(grids.len());
    for (rank, grid) in grids.iter().enumerate() {
        let columns: Vec<Column> = grid
            .cells
            .iter()
            .map(|cell| column(rank, cell))
            .collect::<Result<_, _>>()?;
        let mut pieces: Vec<Piece> = Vec::new();
        for node in 0..points.len() - 1 {
            let (a, b) = (points[node], points[node + 1]);
            let (ma, mb) = (mds[node], mds[node + 1]);
            let low = [a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])];
            let high = [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])];
            for cell in &columns {
                if (0..3).any(|axis| high[axis] < cell.reach[axis][0] || low[axis] > cell.reach[axis][1]) {
                    continue;
                }
                for (s, e, ends) in inside(cell, a, b) {
                    pieces.push(Piece {
                        cell: cell.index,
                        md: [ma + s * (mb - ma), ma + e * (mb - ma)],
                        ends,
                    });
                }
            }
        }
        pieces.retain(|piece| piece.md[1] - piece.md[0] > md_tolerance);
        pieces.sort_by(|x, y| x.md[0].total_cmp(&y.md[0]));
        // One cell across a node of the path is one piece.
        let mut merged: Vec<Piece> = Vec::with_capacity(pieces.len());
        for piece in pieces {
            if let Some(last) = merged.last_mut() {
                if last.cell == piece.cell
                    && matches!(last.ends[1], Edge::Node)
                    && matches!(piece.ends[0], Edge::Node)
                    && (piece.md[0] - last.md[1]).abs() <= md_tolerance
                {
                    last.md[1] = piece.md[1];
                    last.ends[1] = piece.ends[1];
                    continue;
                }
                if piece.md[0] < last.md[1] - md_tolerance {
                    return Err(CellsError::Shared {
                        grid: rank,
                        cells: [last.cell, piece.cell],
                        md: [piece.md[0], last.md[1].min(piece.md[1])],
                    });
                }
            }
            merged.push(piece);
        }
        held.push(merged);
    }

    // -- the grids together, the first given first --------------------------------
    let mut cuts: Vec<f64> = held.iter().flatten().flat_map(|piece| piece.md).collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|b, a| (*b - *a).abs() <= md_tolerance);
    let mut spans: Vec<([f64; 2], Option<Held>)> = Vec::new();
    for pair in cuts.windows(2) {
        let (s, e) = (pair[0], pair[1]);
        let middle = 0.5 * (s + e);
        let owner = held.iter().enumerate().find_map(|(rank, pieces)| {
            pieces
                .iter()
                .find(|piece| piece.md[0] <= middle && middle <= piece.md[1])
                .map(|piece| (rank, piece))
        });
        let one = owner.map(|(rank, piece)| {
            let face = |edge: Edge, at: f64, here: f64| match edge {
                Edge::Face(face) if (at - here).abs() <= md_tolerance => Some(face),
                _ => None,
            };
            Held {
                grid: rank,
                cell: piece.cell,
                faces: [
                    face(piece.ends[0], piece.md[0], s),
                    face(piece.ends[1], piece.md[1], e),
                ],
            }
        });
        match (spans.last_mut(), one) {
            (Some((range, Some(last))), Some(now)) if last.grid == now.grid && last.cell == now.cell => {
                range[1] = e;
                last.faces[1] = now.faces[1];
            }
            (Some((range, None)), None) => range[1] = e,
            (_, one) => spans.push(([s, e], one)),
        }
    }
    while matches!(spans.first(), Some((_, None))) {
        spans.remove(0);
    }
    while matches!(spans.last(), Some((_, None))) {
        spans.pop();
    }
    if spans.is_empty() {
        return Err(CellsError::NoCell);
    }
    let mut node_md = vec![spans[0].0[0]];
    node_md.extend(spans.iter().map(|(range, _)| range[1]));
    Ok(Blocked {
        node_md,
        intervals: spans.into_iter().map(|(_, one)| one).collect(),
        chord_excess,
    })
}

#[cfg(test)]
mod tests {
    //! The form of the assertions is the guide's - an interval per stretch
    //! between two faces, 0/1 the K faces and 2-5 the sides in the IJ circular
    //! order (RESQML Technical Usage Guide 2.2, draft, §9.12.3 p. 159; §11.5
    //! p. 214: *« For each actual grid cell, the blocked well contains the
    //! cell index and the cell face indices where the trajectory enters and
    //! exits »*). The cell a point is in is confronted with **arithmetic** on
    //! a regular grid - `floor` of the offsets - which shares nothing with the
    //! clipping and the roots above.

    use super::*;

    /// A regular grid, `ni × nj × nk` boxes of `size`, its top at `z = 0`,
    /// K going **down** (elevation decreasing) - `[0..4]` on the upper level.
    fn boxes(origin: [f64; 2], size: [f64; 3], counts: [usize; 3]) -> Grid {
        let [ni, nj, nk] = counts;
        let mut cells = Vec::new();
        for k in 0..nk {
            for j in 0..nj {
                for i in 0..ni {
                    let x = |a: usize| origin[0] + size[0] * (i + a) as f64;
                    let y = |b: usize| origin[1] + size[1] * (j + b) as f64;
                    let z = |c: usize| -size[2] * (k + c) as f64;
                    let around = [(0, 0), (1, 0), (1, 1), (0, 1)];
                    let mut corners = [[0.0; 3]; 8];
                    for (n, (a, b)) in around.iter().enumerate() {
                        corners[n] = [x(*a), y(*b), z(0)];
                        corners[n + 4] = [x(*a), y(*b), z(1)];
                    }
                    cells.push(Hexahedron {
                        index: (i + ni * (j + nj * k)) as u64,
                        corners,
                    });
                }
            }
        }
        Grid { cells }
    }

    /// The cell of `boxes` a point is in, by arithmetic.
    fn by_arithmetic(origin: [f64; 2], size: [f64; 3], counts: [usize; 3], p: Point) -> Option<u64> {
        let i = ((p[0] - origin[0]) / size[0]).floor();
        let j = ((p[1] - origin[1]) / size[1]).floor();
        let k = (-p[2] / size[2]).floor();
        let inside = |v: f64, n: usize| v >= 0.0 && v < n as f64;
        (inside(i, counts[0]) && inside(j, counts[1]) && inside(k, counts[2]))
            .then(|| (i as usize + counts[0] * (j as usize + counts[1] * k as usize)) as u64)
    }

    /// Where along the path an MD falls.
    fn at_md(points: &[Point], mds: &[f64], md: f64) -> Point {
        let s = mds.windows(2).position(|w| w[0] <= md && md <= w[1]).unwrap();
        let t = (md - mds[s]) / (mds[s + 1] - mds[s]);
        let (a, b) = (points[s], points[s + 1]);
        [
            a[0] + t * (b[0] - a[0]),
            a[1] + t * (b[1] - a[1]),
            a[2] + t * (b[2] - a[2]),
        ]
    }

    fn chord_mds(points: &[Point], start: f64) -> Vec<f64> {
        let mut out = vec![start];
        for w in points.windows(2) {
            let d = ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2) + (w[1][2] - w[0][2]).powi(2))
                .sqrt();
            out.push(out.last().unwrap() + d);
        }
        out
    }

    #[test]
    fn a_vertical_hole_enters_by_the_top_and_leaves_by_the_bottom() {
        let (origin, size, counts) = ([1000.0, 2000.0], [10.0, 20.0, 5.0], [3, 2, 4]);
        let grid = boxes(origin, size, counts);
        // Above the grid, through all four layers, below it; in column (1, 1).
        let points = [[1015.0, 2030.0, 7.0], [1015.0, 2030.0, -27.0]];
        let mds = [100.0, 134.0];
        let blocked = block(&points, &mds, &[grid]).unwrap();
        // The grid spans z 0 … −20: MD 107 to 127, a node at every level.
        assert_eq!(blocked.node_md.len(), 5, "{:?}", blocked.node_md);
        for (n, md) in blocked.node_md.iter().enumerate() {
            assert!((md - (107.0 + 5.0 * n as f64)).abs() < 1e-9, "node {n} at {md}");
        }
        for (k, one) in blocked.intervals.iter().enumerate() {
            let held = one.expect("every interval is in a cell");
            assert_eq!(held.cell, (1 + 3 * (1 + 2 * k)) as u64);
            assert_eq!(held.faces, [Some(0), Some(1)], "in by the top, out by the bottom");
        }
        assert_eq!(blocked.cell_count(), 4);
        assert_eq!(blocked.chord_excess, 0.0);
    }

    /// **The cells crossed are the cells the path is in**: at the middle of
    /// every interval, the arithmetic cell of that point; and between two
    /// intervals of the path no cell is missed - the arithmetic at a hundred
    /// points along the path finds only cells the blocking names.
    #[test]
    fn a_slanted_hole_is_held_by_the_cells_arithmetic_finds() {
        let (origin, size, counts) = ([0.0, 0.0], [10.0, 10.0, 4.0], [4, 3, 5]);
        let grid = boxes(origin, size, counts);
        let points = [
            [3.0, 2.5, 6.0],
            [7.0, 4.0, -3.0],
            [24.0, 17.0, -12.0],
            [38.5, 26.0, -25.0],
        ];
        let mds = chord_mds(&points, 50.0);
        let blocked = block(&points, &mds, &[grid]).unwrap();
        assert!(blocked.cell_count() >= 6, "{blocked:?}");
        for (n, one) in blocked.intervals.iter().enumerate() {
            let middle = 0.5 * (blocked.node_md[n] + blocked.node_md[n + 1]);
            let expected = by_arithmetic(origin, size, counts, at_md(&points, &mds, middle));
            assert_eq!(one.map(|held| held.cell), expected, "interval {n}, MD {middle}");
        }
        let named: Vec<u64> = blocked.intervals.iter().flatten().map(|held| held.cell).collect();
        for step in 0..=100 {
            let md = mds[0] + (mds[3] - mds[0]) * step as f64 / 100.0;
            if let Some(cell) = by_arithmetic(origin, size, counts, at_md(&points, &mds, md)) {
                assert!(
                    named.contains(&cell),
                    "cell {cell} at MD {md} is crossed and not named"
                );
            }
        }
        // Each exit face is the entry face of the next cell, seen from it:
        // bottom to top, I+ to I−, J+ to J−.
        for pair in blocked.intervals.windows(2) {
            if let (Some(a), Some(b)) = (pair[0], pair[1]) {
                let opposite = match a.faces[1] {
                    Some(0) => 1,
                    Some(1) => 0,
                    Some(2) => 4,
                    Some(3) => 5,
                    Some(4) => 2,
                    Some(5) => 3,
                    other => panic!("an exit between two cells is a face, not {other:?}"),
                };
                assert_eq!(b.faces[0], Some(opposite), "{a:?} then {b:?}");
            }
        }
    }

    #[test]
    fn a_hole_that_starts_inside_a_cell_has_no_entry_face() {
        let grid = boxes([0.0, 0.0], [10.0, 10.0, 10.0], [2, 1, 1]);
        let points = [[5.0, 5.0, -5.0], [15.0, 5.0, -5.0]];
        let blocked = block(&points, &[0.0, 10.0], &[grid]).unwrap();
        assert_eq!(blocked.node_md, vec![0.0, 5.0, 10.0]);
        assert_eq!(
            blocked.intervals[0].unwrap().faces,
            [None, Some(3)],
            "out through I+"
        );
        assert_eq!(
            blocked.intervals[1].unwrap().faces,
            [Some(5), None],
            "in through I−, ends inside"
        );
    }

    #[test]
    fn a_gap_between_grids_is_an_interval_in_no_cell() {
        // Two layers with a void between them, as a K gap leaves it.
        let upper = boxes([0.0, 0.0], [10.0, 10.0, 10.0], [1, 1, 1]);
        let mut lower = boxes([0.0, 0.0], [10.0, 10.0, 10.0], [1, 1, 1]);
        for corner in lower.cells[0].corners.iter_mut() {
            corner[2] -= 15.0;
        }
        let points = [[5.0, 5.0, 1.0], [5.0, 5.0, -30.0]];
        let blocked = block(&points, &[0.0, 31.0], &[upper, lower]).unwrap();
        assert_eq!(blocked.node_md, vec![1.0, 11.0, 16.0, 26.0]);
        assert_eq!(blocked.intervals[1], None);
        assert_eq!(blocked.intervals[0].unwrap().grid, 0);
        assert_eq!(blocked.intervals[2].unwrap().grid, 1);
        assert_eq!(blocked.cell_count(), 2);
    }

    /// The grid listed first holds what it holds; the second is entered where
    /// the first lets the path go, through no face of its own.
    #[test]
    fn a_grid_listed_first_holds_its_stretch() {
        let parent = boxes([0.0, 0.0], [10.0, 10.0, 10.0], [1, 1, 1]);
        let child = boxes([0.0, 0.0], [5.0, 10.0, 10.0], [1, 1, 1]);
        let points = [[2.5, 5.0, -5.0], [9.0, 5.0, -5.0]];
        let blocked = block(&points, &[0.0, 6.5], &[child, parent]).unwrap();
        assert_eq!(blocked.node_md, vec![0.0, 2.5, 6.5]);
        assert_eq!(blocked.intervals[0].unwrap().grid, 0);
        assert_eq!(blocked.intervals[0].unwrap().faces, [None, Some(3)]);
        assert_eq!(blocked.intervals[1].unwrap().grid, 1);
        assert_eq!(
            blocked.intervals[1].unwrap().faces,
            [None, None],
            "not entered through a face of the parent"
        );
    }

    #[test]
    fn a_curved_top_is_crossed_where_its_bilinear_patch_is() {
        // One cell whose top is the saddle z = x·y/100 over a 10 × 10 column.
        let mut grid = boxes([0.0, 0.0], [10.0, 10.0, 10.0], [1, 1, 1]);
        grid.cells[0].corners[2][2] = 1.0; // (1,1) raised: h = u·v
        let points = [[5.0, 5.0, 3.0], [5.0, 5.0, -3.0]];
        let blocked = block(&points, &[0.0, 6.0], std::slice::from_ref(&grid)).unwrap();
        // At (0.5, 0.5) the bilinear top is 0.25: entered at MD 2.75.
        assert!((blocked.node_md[0] - 2.75).abs() < 1e-12, "{:?}", blocked.node_md);
        // Along the diagonal, u = v = t and z = 1 − t: the top h = t² is met
        // where 1 − t = t², at t = (√5 − 1) / 2 - the product term of the
        // patch is what puts it there, not at t = 1.
        let points = [[0.0, 0.0, 1.0], [10.0, 10.0, 0.0]];
        let chord = 201.0f64.sqrt();
        let blocked = block(&points, &[0.0, chord], &[grid]).unwrap();
        let t = (5.0f64.sqrt() - 1.0) / 2.0;
        assert!(
            (blocked.node_md[0] - t * chord).abs() < 1e-9,
            "{:?}",
            blocked.node_md
        );
        assert_eq!(
            blocked.intervals[0].unwrap().faces[0],
            Some(0),
            "in through the top"
        );
    }

    #[test]
    fn what_is_refused_is_named() {
        let grid = boxes([0.0, 0.0], [10.0, 10.0, 10.0], [2, 1, 1]);
        let flat = [[5.0, 5.0, -5.0], [15.0, 5.0, -5.0]];
        assert_eq!(
            block(&flat, &[0.0], std::slice::from_ref(&grid)),
            Err(CellsError::Mismatched { nodes: 2, mds: 1 })
        );
        assert_eq!(
            block(&flat, &[3.0, 3.0], std::slice::from_ref(&grid)),
            Err(CellsError::NotIncreasing {
                node: 1,
                md: 3.0,
                before: 3.0
            })
        );
        assert!(matches!(
            block(&flat, &[0.0, 9.0], std::slice::from_ref(&grid)),
            Err(CellsError::ShorterThanChord { node: 0, .. })
        ));
        // Along the face the two cells share: x = 10.
        let along = [[10.0, 5.0, -2.0], [10.0, 5.0, -8.0]];
        assert!(matches!(
            block(&along, &[0.0, 6.0], std::slice::from_ref(&grid)),
            Err(CellsError::Shared { grid: 0, .. })
        ));
        let away = [[50.0, 5.0, -2.0], [60.0, 5.0, -8.0]];
        assert_eq!(
            block(&away, &[0.0, 20.0], std::slice::from_ref(&grid)),
            Err(CellsError::NoCell)
        );
        let mut leaning = grid.clone();
        leaning.cells[1].corners[5][0] += 1.0;
        assert_eq!(
            block(&flat, &[0.0, 10.0], &[leaning]),
            Err(CellsError::Leaning { grid: 0, cell: 1 })
        );
        let mut kite = grid;
        for n in [2, 6] {
            kite.cells[0].corners[n][0] += 2.0;
        }
        assert_eq!(
            block(&flat, &[0.0, 10.0], &[kite]),
            Err(CellsError::NotParallelogram { grid: 0, cell: 0 })
        );
    }

    #[test]
    fn a_box_is_its_cells_in_resqml_order() {
        assert_eq!(
            ijk_box([3, 2, 2], [Some([1, 2]), None, Some([1, 1])]).unwrap(),
            vec![7, 8, 10, 11]
        );
        assert_eq!(ijk_box([3, 2, 2], [None, None, None]).unwrap().len(), 12);
        assert_eq!(
            ijk_box([3, 2, 2], [Some([2, 1]), None, None]),
            Err(CellsError::Reversed {
                axis: 'i',
                range: [2, 1]
            })
        );
        assert_eq!(
            ijk_box([3, 2, 2], [None, None, Some([0, 2])]),
            Err(CellsError::Beyond {
                axis: 'k',
                last: 2,
                count: 2
            })
        );
    }
}
