//! **An IJK grid cut by its faults** — split coordinate lines where a fault
//! runs between two columns, the hanging wall moved by a stated throw, and the
//! cells nearest a fault removed on request: the computation behind
//! `faulted-ijk-grid`, from a
//! grid [`crate::ijk::stack`] has already built between its horizons.
//!
//! The need: the IJK grids of the synthetic model are too regular. At least one
//! should be "cut" by the faults — perhaps a new one, shifted somewhat downward
//! on one side, with the cells nearest one or two faults removed.
//!
//! # What the file will say, and where RESQML says it
//!
//! - **a split coordinate line is an extra line on the same pillar**, used by
//!   the columns it names instead of the pillar's own line: *« For the first
//!   pillarCount lines, the index of the coordinate line equals the index of
//!   the corresponding pillar. This array provides the pillar indices for the
//!   additional (split) coordinate lines »* (RESQML TRG 2.2 §5.17
//!   `ColumnLayerSplitCoordinateLines` p. 111). The split lines come last —
//!   *« the coordinate lines for a column-layer grid should always be numbered
//!   to first follow the pillars, with any additional split coordinate lines
//!   coming last »* (RESQML Technical Usage Guide 2.2, draft, §9.5.2 p. 136) —
//!   and *« For a faulted IJK grid each pillar can have up to four coordinate
//!   lines per pillar »* (§9.11.1 p. 151): one per group of the (up to four)
//!   columns around it that the faults put on different sides;
//! - **the nodes are `NKL × (pillars + splits)`**, lines fastest: *« Coordinate
//!   line nodes for all of the coordinate lines, with NKL nodes per line […]
//!   Otherwise the nodes will be 2D »* (TRG §5.1 p. 94), the 4 × 3 × 2 example of
//!   the usage guide writing *« NODES = 24 x 3 »* for 20 pillars and 4 split
//!   lines (§9.19.2, figure 9-39, p. 174);
//! - **a removed cell is a cell whose geometry is not defined**:
//!   `CellGeometryIsDefined` *« overrides any other node geometry
//!   specification »* (TRG §5.1 p. 94). Its nodes stay what they are — the
//!   flag, not a `NaN`, is what the reader obeys, and the neighbours share them.
//!
//! # Where a fault runs in a grid of vertical pillars — a staircase, said
//!
//! The pillars stay vertical (`PillarShape` `vertical`, TRG §5.39 p. 135): a
//! column is on **one side** of a fault over its whole height, so a dipping
//! fault is drawn as a vertical staircase along the column faces. Which side
//! is decided at **one level**, stated and reported: the middle of the overlap
//! between the fault's vertical extent and the grid's. There the fault surface
//! is cut by a horizontal plane ([`crate::intersect`]), which gives its
//! **trace**, one open line — nothing, several pieces or a ring is refused by
//! name. A column is in the **hanging wall** when its centre lies on the side of
//! the trace toward which the fault **dips**, the dip measured on the surface
//! ([`crate::fault_sticks::orientation`]) as `fault-throw` measures it; a
//! vertical fault has no hanging wall and is refused, as is a trace that runs
//! within 30° of its dip direction.
//!
//! # The throw — the law `fault-throw` states, so a grid and a torn horizon agree
//!
//! At a pillar, the hanging wall moves by `maximum · profile(s) · (1 − d /
//! reach)`, `s` the pillar's position along the trace (0 to 1, from its plan
//! length), `d` its distance to the trace in plan, nothing beyond `reach` —
//! the law of [`crate::fault_throw`], with its two profiles. Down for a normal
//! fault, up for a reverse one; **vertically only**, since a pillar is
//! vertical (no heave). A node moves only where the fault is: a level node
//! whose height lies outside the fault surface's own vertical extent keeps its
//! place — a fault sealed under the top of the grid (F2 under H6) leaves the
//! top untouched. The footwall does not move.
//!
//! At every pillar, the columns around it are grouped by the displacement the
//! faults give them there; the group that moves least keeps the pillar's own
//! line — the footwall, on a single fault — and each other group gets a split
//! line. Two groups that move alike are one: the throw dies out at the tips of
//! a fault, and so do its split lines.
//!
//! # Refused rather than drawn
//!
//! A fault that splits no pillar (its trace misses the grid, or its throw is
//! zero wherever it does) — a faulted grid with no fault would read as one; a
//! throw that turns a line's levels over (a reverse throw lifting a node above
//! the one over it); a removed layer past the grid; a grid left with no cell.
//!
//! # Precision far from the origin
//!
//! Lutetia sits at x ≈ 651 000, y ≈ 6 861 000: every plan computation is made
//! relative to the first pillar, as `fault_throw` does.

use std::collections::{HashMap, VecDeque};

use crate::fault_sticks::{self, Orientation};
use crate::fault_throw::{self, Profile, Sense, VERTICAL};
use crate::ijk::Stacked;
use crate::intersect::intersect;
use crate::surface::Up;

type Point = [f64; 3];

/// A fault, as its surface.
#[derive(Clone, Copy, Debug)]
pub struct Fault<'a> {
    /// Its name, for a message.
    pub name: &'a str,
    /// The surface's nodes, in the grid's frame.
    pub points: &'a [Point],
    /// Its triangles, indices into `points`.
    pub triangles: &'a [[u32; 3]],
    /// The throw where it is largest, in the frame's vertical unit.
    pub maximum_throw: f64,
    /// Which way its hanging wall goes.
    pub sense: Sense,
    /// How many columns of its hanging wall, counted from the fault, lose
    /// their cells — 0 keeps them all.
    pub dead_columns: usize,
}

/// What every fault is cut with.
#[derive(Clone, Copy, Debug)]
pub struct Settings<'a> {
    /// How the throw varies along each trace.
    pub profile: Profile,
    /// How far from the trace, in plan, the hanging wall is dragged.
    pub reach: f64,
    /// The layers, from 0 at the top, in which the columns next to a fault
    /// lose their cells — empty for every layer.
    pub dead_layers: &'a [usize],
}

/// What one fault did to the grid.
#[derive(Clone, Debug, PartialEq)]
pub struct FaultCut {
    /// Its surface's orientation, as measured.
    pub orientation: Orientation,
    /// The height its trace is read at — the middle of the overlap between
    /// its vertical extent and the grid's.
    pub level: f64,
    /// Its trace at that level, in the frame.
    pub trace: Vec<Point>,
    /// The trace's plan length.
    pub length: f64,
    /// Per column, `i` fastest: in its hanging wall.
    pub hanging_wall: Vec<bool>,
    /// The largest displacement it gives a split pillar.
    pub largest: f64,
    /// The columns whose cells it removed, `i` fastest.
    pub dead_columns: Vec<usize>,
    /// The cells it removed that had a geometry.
    pub dead_cells: usize,
}

/// The grid, cut.
#[derive(Clone, Debug, PartialEq)]
pub struct Faulted {
    /// `[Ni, Nj, Nk]`.
    pub cells: [usize; 3],
    /// `[Ni + 1, Nj + 1]`.
    pub pillars: [usize; 2],
    /// `Nk + 1 + gaps`.
    pub levels: usize,
    /// Pillars, then split lines.
    pub lines: usize,
    /// The pillar each split line is on, in split order.
    pub split_pillars: Vec<usize>,
    /// The columns each split line serves, ascending, in split order.
    pub split_columns: Vec<Vec<usize>>,
    /// `levels × lines` heights, lines fastest.
    pub z: Vec<f64>,
    /// `Nk × Nj × Ni` flags, `i` fastest.
    pub cell_defined: Vec<bool>,
    /// `(Nj + 1) × (Ni + 1)` flags: a pillar with a node of a defined cell.
    pub pillar_defined: Vec<bool>,
    /// The cells removed next to a fault that had a geometry, ascending.
    pub dead: Vec<usize>,
    /// One per fault, in the order given.
    pub faults: Vec<FaultCut>,
    /// The level each layer's top sits at.
    layer_tops: Vec<usize>,
    /// Which way the third coordinate points.
    up: Up,
    /// `(pillar, column)` → split line, for the columns that use one.
    lookup: HashMap<(usize, usize), usize>,
}

impl Faulted {
    /// The pillar a line stands on.
    pub fn pillar_of(&self, line: usize) -> usize {
        let per_level = self.pillars[0] * self.pillars[1];
        if line < per_level {
            line
        } else {
            self.split_pillars[line - per_level]
        }
    }

    /// The line a column takes its corner from on a pillar.
    pub fn line_for(&self, pillar: usize, column: usize) -> usize {
        self.lookup.get(&(pillar, column)).copied().unwrap_or(pillar)
    }

    /// Cells whose geometry is not defined.
    pub fn undefined_cells(&self) -> usize {
        self.cell_defined.iter().filter(|defined| !**defined).count()
    }

    /// The nodes, level first, lines fastest — `x` and `y` the pillar's.
    pub fn points(&self, pillars: &[[f64; 2]]) -> Vec<Point> {
        let mut out = Vec::with_capacity(self.levels * self.lines);
        for level in 0..self.levels {
            for line in 0..self.lines {
                let [x, y] = pillars
                    .get(self.pillar_of(line))
                    .copied()
                    .unwrap_or([f64::NAN; 2]);
                out.push([x, y, self.z[level * self.lines + line]]);
            }
        }
        out
    }

    /// The defined cells in RESQML's order, as eight indices into
    /// [`Faulted::points`] in VTK's order, and the RESQML index of each —
    /// empty when every cell is defined. [`Stacked::hexahedra`] with each
    /// corner looked up.
    pub fn hexahedra(&self) -> (Vec<[u32; 8]>, Vec<u32>) {
        let [ni, nj, nk] = self.cells;
        let pi = self.pillars[0];
        let mut cells = Vec::new();
        let mut index = Vec::new();
        for (k, &top) in self.layer_tops.iter().enumerate() {
            for j in 0..nj {
                for i in 0..ni {
                    let cell = (k * nj + j) * ni + i;
                    if !self.cell_defined[cell] {
                        continue;
                    }
                    let column = j * ni + i;
                    let around = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
                    let (first, second) = match self.up {
                        Up::MinusZ => (top, top + 1),
                        Up::PlusZ => (top + 1, top),
                    };
                    let mut node = [0u32; 8];
                    for (corner, (a, b)) in around.iter().enumerate() {
                        let line = self.line_for(b * pi + a, column);
                        node[corner] = (first * self.lines + line) as u32;
                        node[corner + 4] = (second * self.lines + line) as u32;
                    }
                    cells.push(node);
                    index.push(cell as u32);
                }
            }
        }
        if cells.len() == ni * nj * nk {
            index.clear();
        }
        (cells, index)
    }
}

/// Why a grid could not be cut.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CutError {
    /// No fault given.
    #[error(
        "no fault given: a grid cut by nothing is the grid between its horizons — `ijk-grid-from-horizons` writes it"
    )]
    NoFault,
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
    /// Not one position per pillar.
    #[error("{given} pillar position(s) for {expected} pillar(s)")]
    Pillars {
        /// The pillars.
        expected: usize,
        /// What was given.
        given: usize,
    },
    /// A removed layer past the grid.
    #[error("layer {layer} is asked to lose its cells, and the grid has {layers} layer(s), numbered from 0")]
    DeadLayer {
        /// The layer asked.
        layer: usize,
        /// `Nk`.
        layers: usize,
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
    /// A fault whose vertical extent misses the grid's.
    #[error(
        "`{fault}` runs from {top} to {bottom} and the grid from {grid_top} to {grid_bottom}: it does not reach the grid's heights"
    )]
    Misses {
        /// The fault.
        fault: String,
        /// Its highest node's third coordinate.
        top: f64,
        /// Its lowest.
        bottom: f64,
        /// The grid's highest.
        grid_top: f64,
        /// The grid's lowest.
        grid_bottom: f64,
    },
    /// No trace at the level.
    #[error("`{fault}` has no trace at {level}: the plane at that height misses its surface")]
    NoTrace {
        /// The fault.
        fault: String,
        /// The level.
        level: f64,
    },
    /// A trace in several pieces.
    #[error(
        "`{fault}` is cut at {level} in {count} separate traces — which of them splits the grid, and how their throws meet, is not decided here"
    )]
    Pieces {
        /// The fault.
        fault: String,
        /// The level.
        level: f64,
        /// How many.
        count: usize,
    },
    /// A trace that closes on itself.
    #[error(
        "`{fault}` is cut at {level} along a closed ring — a ring has no end for its throw to die out at"
    )]
    Ring {
        /// The fault.
        fault: String,
        /// The level.
        level: f64,
    },
    /// A trace too close to the dip direction.
    #[error(
        "the trace of `{fault}` runs {angle:.0}° from its dip direction on average — within 30°, which side is the hanging wall cannot be read from it"
    )]
    AlongDip {
        /// The fault.
        fault: String,
        /// The mean angle, in degrees.
        angle: f64,
    },
    /// A fault that splits no pillar.
    #[error(
        "`{fault}` splits no pillar: its trace at {level} runs outside the grid, or its throw is zero wherever it passes between two columns"
    )]
    Uncut {
        /// The fault.
        fault: String,
        /// The level.
        level: f64,
    },
    /// A throw that turns a line over.
    #[error(
        "the throw turns the levels over on the line of pillar ({i}, {j}): level {level} ends up above level {above} — a throw larger than the layers it crosses, against their order"
    )]
    Overturned {
        /// The pillar's `i`.
        i: usize,
        /// Its `j`.
        j: usize,
        /// The lower level.
        level: usize,
        /// The level it passed.
        above: usize,
    },
    /// Nothing left.
    #[error(
        "none of the {cells} cell(s) keeps a geometry once the cells next to the faults are removed — nothing would be drawn"
    )]
    NothingLeft {
        /// The cells.
        cells: usize,
    },
}

/// The hanging-wall side of a trace walked forward: the sign of the side its
/// dip direction points to, on average — the rule of `fault_throw`.
fn hanging_wall_side(trace: &[Point], dip_azimuth: f64, fault: &str) -> Result<f64, CutError> {
    let direction = [dip_azimuth.to_radians().sin(), dip_azimuth.to_radians().cos()];
    let (mut sum, mut total) = (0.0f64, 0.0f64);
    for pair in trace.windows(2) {
        let v = fault_throw::plan(&pair[0], &pair[1]);
        sum += fault_throw::cross2(v, direction);
        total += fault_throw::length2(v);
    }
    let mean = if total > 0.0 { sum / total } else { 0.0 };
    if mean.abs() < fault_throw::ACROSS {
        return Err(CutError::AlongDip {
            fault: fault.to_string(),
            angle: mean.abs().clamp(0.0, 1.0).asin().to_degrees(),
        });
    }
    Ok(mean.signum())
}

/// One fault, read against the grid: its trace, its sides, its throw at each
/// pillar, and where it acts vertically.
struct Read {
    orientation: Orientation,
    level: f64,
    trace: Vec<Point>,
    length: f64,
    hanging_wall: Vec<bool>,
    /// Per pillar, the displacement's size (0 beyond reach).
    amount: Vec<f64>,
    /// `+1` or `−1`: which way the third coordinate goes when the hanging
    /// wall moves.
    sign: f64,
    /// The fault's own vertical extent.
    span: [f64; 2],
}

/// Cut a grid by its faults — see the module.
///
/// `pillars` is each pillar's `(x, y)`, `i` fastest, in the grid's frame.
pub fn cut(
    stacked: &Stacked,
    pillars: &[[f64; 2]],
    faults: &[Fault<'_>],
    settings: &Settings<'_>,
) -> Result<Faulted, CutError> {
    let [ni, nj, nk] = stacked.cells;
    let [pi, pj] = stacked.pillars;
    let per_level = pi * pj;
    let levels = stacked.levels;
    if faults.is_empty() {
        return Err(CutError::NoFault);
    }
    if pillars.len() != per_level {
        return Err(CutError::Pillars {
            expected: per_level,
            given: pillars.len(),
        });
    }
    if !(settings.reach.is_finite() && settings.reach > 0.0) {
        return Err(CutError::BadReach {
            given: settings.reach,
        });
    }
    for fault in faults {
        if !(fault.maximum_throw.is_finite() && fault.maximum_throw > 0.0) {
            return Err(CutError::BadThrow {
                fault: fault.name.to_string(),
                given: fault.maximum_throw,
            });
        }
    }
    if let Some(&layer) = settings.dead_layers.iter().find(|&&layer| layer >= nk) {
        return Err(CutError::DeadLayer { layer, layers: nk });
    }

    // -- plan positions, relative to the first pillar ------------------------------
    let origin = pillars[0];
    let local = |p: &Point| -> Point { [p[0] - origin[0], p[1] - origin[1], p[2]] };
    let at_pillar: Vec<Point> = pillars
        .iter()
        .map(|[x, y]| [x - origin[0], y - origin[1], 0.0])
        .collect();
    let centre = |i: usize, j: usize| -> Point {
        let corners = [j * pi + i, j * pi + i + 1, (j + 1) * pi + i + 1, (j + 1) * pi + i];
        let mut sum = [0.0f64; 3];
        for c in corners {
            sum[0] += at_pillar[c][0] / 4.0;
            sum[1] += at_pillar[c][1] / 4.0;
        }
        sum
    };
    let (mut grid_low, mut grid_high) = (f64::INFINITY, f64::NEG_INFINITY);
    for z in stacked.z.iter().filter(|z| z.is_finite()) {
        grid_low = grid_low.min(*z);
        grid_high = grid_high.max(*z);
    }
    let up = matches!(stacked.up, Up::PlusZ);

    // -- each fault, read ------------------------------------------------------------
    let mut read: Vec<Read> = Vec::with_capacity(faults.len());
    for fault in faults {
        let surface: Vec<Point> = fault.points.iter().map(local).collect();
        let orientation =
            fault_sticks::orientation(&surface, fault.triangles, up).ok_or_else(|| CutError::NoSurface {
                fault: fault.name.to_string(),
            })?;
        if orientation.dip > VERTICAL {
            return Err(CutError::Vertical {
                fault: fault.name.to_string(),
                dip: orientation.dip,
            });
        }
        let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for p in surface
            .iter()
            .chain(&at_pillar)
            .filter(|p| p.iter().all(|c| c.is_finite()))
        {
            for axis in 0..2 {
                lo[axis] = lo[axis].min(p[axis]);
                hi[axis] = hi[axis].max(p[axis]);
            }
        }
        for p in surface.iter().filter(|p| p[2].is_finite()) {
            low = low.min(p[2]);
            high = high.max(p[2]);
        }
        let (bottom, top) = (low.max(grid_low), high.min(grid_high));
        if !matches!(
            bottom.partial_cmp(&top),
            Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
        ) {
            let (top_of, bottom_of) = if up { (high, low) } else { (low, high) };
            let (grid_top, grid_bottom) = if up {
                (grid_high, grid_low)
            } else {
                (grid_low, grid_high)
            };
            return Err(CutError::Misses {
                fault: fault.name.to_string(),
                top: top_of,
                bottom: bottom_of,
                grid_top,
                grid_bottom,
            });
        }
        let level = (bottom + top) / 2.0;
        // A horizontal plate over everything, at that level.
        let plate: Vec<Point> = vec![
            [lo[0] - 1.0, lo[1] - 1.0, level],
            [hi[0] + 1.0, lo[1] - 1.0, level],
            [hi[0] + 1.0, hi[1] + 1.0, level],
            [lo[0] - 1.0, hi[1] + 1.0, level],
        ];
        let found = intersect(&surface, fault.triangles, &plate, &[[0, 1, 2], [0, 2, 3]]);
        let mut pieces = fault_throw::weld(
            found
                .traces
                .into_iter()
                .map(|trace| trace.points)
                .filter(|line| line.len() >= 2)
                .collect(),
        );
        let (trace, closed) = match pieces.len() {
            0 => {
                return Err(CutError::NoTrace {
                    fault: fault.name.to_string(),
                    level,
                });
            }
            1 => pieces.remove(0),
            count => {
                return Err(CutError::Pieces {
                    fault: fault.name.to_string(),
                    level,
                    count,
                });
            }
        };
        if closed {
            return Err(CutError::Ring {
                fault: fault.name.to_string(),
                level,
            });
        }
        let sigma = hanging_wall_side(&trace, orientation.dip_azimuth, fault.name)?;
        let length = fault_throw::plan_length(&trace);
        let amount: Vec<f64> = at_pillar
            .iter()
            .map(|p| match fault_throw::project(&trace, p) {
                Some((s, d, _)) if d < settings.reach && length > 0.0 => {
                    fault.maximum_throw * settings.profile.at(s / length) * (1.0 - d / settings.reach)
                }
                _ => 0.0,
            })
            .collect();
        let mut hanging_wall = vec![false; ni * nj];
        for j in 0..nj {
            for i in 0..ni {
                if let Some((_, _, side)) = fault_throw::project(&trace, &centre(i, j)) {
                    hanging_wall[j * ni + i] = side * sigma > 0.0;
                }
            }
        }
        let sense = match fault.sense {
            Sense::Down => -1.0,
            Sense::Up => 1.0,
        };
        let sign = if up { sense } else { -sense };
        let world: Vec<Point> = trace
            .iter()
            .map(|p| [p[0] + origin[0], p[1] + origin[1], p[2]])
            .collect();
        read.push(Read {
            orientation,
            level,
            trace: world,
            length,
            hanging_wall,
            amount,
            sign,
            span: [low, high],
        });
    }

    // -- the displacement a column takes at a pillar, level by level -----------------
    let displacement = |column: usize, pillar: usize| -> Vec<f64> {
        (0..levels)
            .map(|level| {
                let z = stacked.z[level * per_level + pillar];
                let mut shift = 0.0f64;
                for one in &read {
                    if one.hanging_wall[column]
                        && one.amount[pillar] > 0.0
                        && z >= one.span[0]
                        && z <= one.span[1]
                    {
                        shift += one.sign * one.amount[pillar];
                    }
                }
                shift
            })
            .collect()
    };

    // -- the lines: each pillar's own, then the splits --------------------------------
    let mut own: Vec<Vec<f64>> = Vec::with_capacity(per_level);
    let mut split_pillars: Vec<usize> = Vec::new();
    let mut split_columns: Vec<Vec<usize>> = Vec::new();
    let mut split_z: Vec<Vec<f64>> = Vec::new();
    let mut cuts = vec![false; faults.len()];
    let mut largest = vec![0.0f64; faults.len()];
    for b in 0..pj {
        for a in 0..pi {
            let pillar = b * pi + a;
            let mut around: Vec<usize> = Vec::with_capacity(4);
            for (ci, cj) in [
                (a.wrapping_sub(1), b.wrapping_sub(1)),
                (a, b.wrapping_sub(1)),
                (a.wrapping_sub(1), b),
                (a, b),
            ] {
                if ci < ni && cj < nj {
                    around.push(cj * ni + ci);
                }
            }
            around.sort_unstable();
            for (f, one) in read.iter().enumerate() {
                let sides = around.iter().filter(|c| one.hanging_wall[**c]).count();
                let acts = (0..levels).any(|level| {
                    let z = stacked.z[level * per_level + pillar];
                    z >= one.span[0] && z <= one.span[1]
                });
                if sides > 0 && sides < around.len() && one.amount[pillar] > 0.0 && acts {
                    cuts[f] = true;
                    largest[f] = largest[f].max(one.amount[pillar]);
                }
            }
            // Group the columns by what they do here, in column order.
            let mut groups: Vec<(Vec<f64>, Vec<usize>)> = Vec::new();
            for &column in &around {
                let shift = displacement(column, pillar);
                match groups
                    .iter_mut()
                    .find(|(s, _)| s.iter().zip(&shift).all(|(a, b)| a.to_bits() == b.to_bits()))
                {
                    Some((_, members)) => members.push(column),
                    None => groups.push((shift, vec![column])),
                }
            }
            // The one that moves least keeps the pillar's own line.
            let size = |shift: &[f64]| shift.iter().map(|s| s.abs()).sum::<f64>();
            let keeper = groups
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| size(&a.0).total_cmp(&size(&b.0)))
                .map(|(at, _)| at)
                .unwrap_or(0);
            let line_z = |shift: &[f64]| -> Vec<f64> {
                (0..levels)
                    .map(|level| stacked.z[level * per_level + pillar] + shift[level])
                    .collect()
            };
            own.push(
                groups
                    .get(keeper)
                    .map(|(shift, _)| line_z(shift))
                    .unwrap_or_else(|| line_z(&vec![0.0; levels])),
            );
            for (at, (shift, members)) in groups.iter().enumerate() {
                if at == keeper {
                    continue;
                }
                split_pillars.push(pillar);
                split_columns.push(members.clone());
                split_z.push(line_z(shift));
            }
        }
    }
    for (f, fault) in faults.iter().enumerate() {
        if !cuts[f] {
            return Err(CutError::Uncut {
                fault: fault.name.to_string(),
                level: read[f].level,
            });
        }
    }

    // -- a throw never turns a line over ---------------------------------------------
    let above = |a: f64, b: f64| if up { a > b } else { a < b };
    for (line, heights) in own.iter().chain(&split_z).enumerate() {
        let mut last: Option<(usize, f64)> = None;
        for (level, &z) in heights.iter().enumerate() {
            if !z.is_finite() {
                continue;
            }
            if let Some((upper, top)) = last
                && above(z, top)
            {
                let pillar = if line < per_level {
                    line
                } else {
                    split_pillars[line - per_level]
                };
                return Err(CutError::Overturned {
                    i: pillar % pi,
                    j: pillar / pi,
                    level,
                    above: upper,
                });
            }
            last = Some((level, z));
        }
    }

    let lines = per_level + split_pillars.len();
    let mut z = Vec::with_capacity(levels * lines);
    for level in 0..levels {
        z.extend(own.iter().map(|heights| heights[level]));
        z.extend(split_z.iter().map(|heights| heights[level]));
    }
    let mut lookup = HashMap::new();
    for (split, members) in split_columns.iter().enumerate() {
        for &column in members {
            lookup.insert((split_pillars[split], column), per_level + split);
        }
    }

    // -- the cells removed next to a fault ----------------------------------------------
    let mut cell_defined = stacked.cell_defined.clone();
    let mut dead: Vec<usize> = Vec::new();
    let layers: Vec<usize> = if settings.dead_layers.is_empty() {
        (0..nk).collect()
    } else {
        settings.dead_layers.to_vec()
    };
    let mut cut_faults: Vec<FaultCut> = Vec::with_capacity(faults.len());
    for (f, (fault, one)) in faults.iter().zip(read).enumerate() {
        let mut marked: Vec<usize> = Vec::new();
        if fault.dead_columns > 0 {
            // A hanging-wall column **on** the fault: across one of its edges
            // lies the footwall, and the fault throws on that edge.
            let mut distance: Vec<Option<usize>> = vec![None; ni * nj];
            let mut queue: VecDeque<usize> = VecDeque::new();
            for j in 0..nj {
                for i in 0..ni {
                    let column = j * ni + i;
                    if !one.hanging_wall[column] {
                        continue;
                    }
                    let edges = [
                        (i + 1 < ni, column + 1, [j * pi + i + 1, (j + 1) * pi + i + 1]),
                        (i > 0, column.wrapping_sub(1), [j * pi + i, (j + 1) * pi + i]),
                        (j + 1 < nj, column + ni, [(j + 1) * pi + i, (j + 1) * pi + i + 1]),
                        (j > 0, column.wrapping_sub(ni), [j * pi + i, j * pi + i + 1]),
                    ];
                    let on = edges.iter().any(|(inside, other, shared)| {
                        *inside && !one.hanging_wall[*other] && shared.iter().any(|p| one.amount[*p] > 0.0)
                    });
                    if on {
                        distance[column] = Some(1);
                        queue.push_back(column);
                    }
                }
            }
            while let Some(column) = queue.pop_front() {
                let reached = distance[column].unwrap_or(1);
                marked.push(column);
                if reached >= fault.dead_columns {
                    continue;
                }
                let (i, j) = (column % ni, column / ni);
                let next = [
                    (i + 1 < ni).then(|| column + 1),
                    (i > 0).then(|| column - 1),
                    (j + 1 < nj).then(|| column + ni),
                    (j > 0).then(|| column - ni),
                ];
                for other in next.into_iter().flatten() {
                    if one.hanging_wall[other] && distance[other].is_none() {
                        distance[other] = Some(reached + 1);
                        queue.push_back(other);
                    }
                }
            }
            marked.sort_unstable();
        }
        let mut removed = 0usize;
        for &column in &marked {
            for &k in &layers {
                let cell = k * ni * nj + column;
                if cell_defined[cell] {
                    cell_defined[cell] = false;
                    dead.push(cell);
                    removed += 1;
                }
            }
        }
        cut_faults.push(FaultCut {
            orientation: one.orientation,
            level: one.level,
            trace: one.trace,
            length: one.length,
            hanging_wall: one.hanging_wall,
            largest: largest[f],
            dead_columns: marked,
            dead_cells: removed,
        });
    }
    dead.sort_unstable();
    if !cell_defined.iter().any(|defined| *defined) {
        return Err(CutError::NothingLeft {
            cells: cell_defined.len(),
        });
    }
    let mut pillar_defined = vec![false; per_level];
    for k in 0..nk {
        for j in 0..nj {
            for i in 0..ni {
                if cell_defined[(k * nj + j) * ni + i] {
                    for p in [j * pi + i, j * pi + i + 1, (j + 1) * pi + i + 1, (j + 1) * pi + i] {
                        pillar_defined[p] = true;
                    }
                }
            }
        }
    }

    Ok(Faulted {
        cells: stacked.cells,
        pillars: stacked.pillars,
        levels,
        lines,
        split_pillars,
        split_columns,
        z,
        cell_defined,
        pillar_defined,
        dead,
        faults: cut_faults,
        layer_tops: stacked.layer_tops.clone(),
        up: stacked.up,
        lookup,
    })
}
