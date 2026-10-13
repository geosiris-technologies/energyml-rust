//! **An IJK grid between horizons** — the pillars, the levels, the K gaps, and
//! which cells have a shape: the computation behind `ijk-grid-from-horizons`
//! (a step of the synthetic model build), without a file.
//!
//! # The model, as the reader has it
//!
//! `energyml_geometry::ijk` is the authority on what a written grid means, and
//! this module builds exactly what it reads: `(Ni + 1) × (Nj + 1)` **pillars**,
//! each carrying one node per **level**, the levels numbered from the top, the
//! nodes stored level first, then `j`, then `i` fastest. A layer `k` sits
//! between two consecutive levels; a **K gap** inserts one more level between
//! two layers, so a grid has `Nk + 1 + gaps` levels (RESQML TRG 2.2 §5.36
//! `KGaps` p. 132: *« NKL = NK + gapCount + 1 »*).
//!
//! # Zones, and what a zone of no layer is
//!
//! The horizons are given **top to bottom**, in the declared order of the
//! caller — never sorted by height, which would decide exactly where two picks
//! disagree. Two consecutive horizons bound a **zone**, and each zone is given
//! a number of layers:
//!
//! - `n ≥ 1` layers: `n + 1` levels from the upper horizon to the lower one,
//!   the upper and lower levels **being** the horizons' heights (copied, never
//!   recomputed) and the levels between them spread **linearly** — the node at
//!   `l / n` of the way down is `top + (base − top) · l / n`, proportional
//!   layering (RESQML Technical Usage Guide 2.2, draft, §9.19.7 p. 189: *« a
//!   two zone model with 6 layers in the first zone and 18 layers in the
//!   second zone »*);
//! - `0` layers: the zone is **not meshed** — a K gap. Its upper horizon is the
//!   base of the layer above, its lower one the top of the layer below, and
//!   nothing lies between (TRG §5.36: *« With K gaps, the bottom of one layer
//!   need not be continuous with the top of the next layer »*). the reference model has one:
//!   the Marnes de Meudon between H5 and H6.
//!
//! A gap at the top or the bottom of the stack is no gap — a horizon outside
//! the grid — and two gaps in a row would need two gaps after one layer, which
//! `GapAfterLayer` cannot say: both are refused by name.
//!
//! # A hole undefines the cells it touches, and only those
//!
//! A `NaN` height of a horizon — a hole of its grid — has no node to give its
//! pillar at that level, and the levels interpolated from it have none either.
//! A cell is **defined when its eight corners are**, and only then. That is
//! finer than "the column": a hole in H4 says nothing of the Chalk between H6
//! and H7 under it, and undefining those cells would throw measured rock away.
//! What the file says of it is `CellGeometryIsDefined` — *« If the indicator
//! shows that the cell geometry is NOT defined, then this attribute overrides
//! any other node geometry specification »* (TRG 2.2 §5.1
//! `AbstractColumnLayerGridGeometry` p. 94) — and `PillarGeometryIsDefined`,
//! *« a pillar has at least one node with a defined cell geometry »* (same
//! page), both computed here.
//!
//! # A crossing is refused, or pinched on request
//!
//! A lower horizon **above** the one declared over it, at a pillar, is two
//! picks that disagree. It is compared against the **last defined** horizon
//! above it on that pillar, so a hole in between does not hide it. Two answers,
//! and the caller picks one ([`Crossing`]):
//!
//! - [`Crossing::Refuse`] — the grid is refused, with the count and the first
//!   node, before anything is built;
//! - [`Crossing::Pinch`] — the lower horizon is brought **up to** the upper one
//!   there, and the zone has zero thickness at that pillar: a pinch-out, the
//!   column-layer grid's own notion (TRG §5.1 p. 94: *« Description of the
//!   geometry of a column layer grid, e.g., parity and pinch »*). The upper
//!   one wins because an older horizon **stops** at the younger surface above
//!   it (RESQML Technical Usage Guide 2.2, draft, §7.2.2.2, table 5 p. 84, as
//!   quoted by the synthetic model notes). Every node moved is counted.
//!
//! `horizon_law` makes an eroded node a **hole** rather than clamp it onto the
//! cut, and that is not a contradiction: a node of a horizon is a pick, and a
//! clamped one would be the cut drawn a second time under another name; a node
//! of a grid is where a layer boundary runs, and a layer that has vanished
//! runs **on** the surface that removed it. Even so the pinch is never a
//! default: the default refuses.
//!
//! # What is not done here
//!
//! No split coordinate line (a fault through the grid, the reference model's
//! 21 split pillars), no parametric pillar, no resampling onto another lattice: the
//! pillars stand on the horizons' own nodes (every `step`-th one), so a node of
//! the grid **is** a node of each horizon at its level, and nothing is
//! interpolated sideways.

use std::fmt;

use crate::surface::Up;

/// A millionth of a cell: what a reader's running sum of spacings can drift
/// by, and nothing a producer means — the tolerance `horizon_law` reads a
/// lattice with.
const TOLERANCE: f64 = 1e-6;

/// The most nodes a grid may have: `interpolate` refuses a lattice past ten
/// million nodes for the same reason — eighty megabytes of doubles per axis
/// that nobody meant to ask for.
pub const MAX_NODES: usize = 10_000_000;

/// A regular lattice along +x then +y — the house lattice
/// (a grid is not reprojected, and its order is the house order): node `(i, j)` at `origin + (i·dx, j·dy)`,
/// `i` varying fastest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lattice {
    /// `[x0, y0]`, the node `(0, 0)`.
    pub origin: [f64; 2],
    /// `[dx, dy]`, both positive.
    pub spacing: [f64; 2],
    /// `[ni, nj]`, nodes along x and along y.
    pub counts: [usize; 2],
}

impl Lattice {
    /// Whether two lattices put their nodes at the same places — the same
    /// counts, and the first and the last node within a millionth of a cell.
    pub fn same_as(&self, other: &Lattice) -> bool {
        if self.counts != other.counts {
            return false;
        }
        let tolerance = TOLERANCE * self.spacing[0].min(self.spacing[1]);
        (0..2).all(|axis| {
            let far = |lattice: &Lattice| {
                lattice.origin[axis] + (lattice.counts[axis] - 1) as f64 * lattice.spacing[axis]
            };
            (self.origin[axis] - other.origin[axis]).abs() <= tolerance
                && (far(self) - far(other)).abs() <= tolerance
        })
    }
}

/// Why nodes are not a lattice the grid can stand on.
#[derive(Debug, Clone, PartialEq)]
pub enum LatticeError {
    /// Fewer than two nodes along an axis.
    TooSmall {
        /// `[ni, nj]`.
        counts: [usize; 2],
    },
    /// The node count does not match the declared counts.
    Count {
        /// `[ni, nj]`.
        counts: [usize; 2],
        /// The nodes held.
        held: usize,
    },
    /// The axes do not run along +x then +y.
    Turned {
        /// From node `(0, 0)` to node `(1, 0)`.
        along_i: [f64; 2],
        /// From node `(0, 0)` to node `(0, 1)`.
        along_j: [f64; 2],
    },
    /// A node is not where a regular lattice puts it.
    Irregular {
        /// `[i, j]`.
        node: [usize; 2],
        /// Where it is.
        at: [f64; 2],
        /// Where the lattice puts it.
        expected: [f64; 2],
    },
}

impl fmt::Display for LatticeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LatticeError::TooSmall { counts: [ni, nj] } => {
                write!(
                    f,
                    "is a grid of {ni} × {nj} nodes: a lattice has at least 2 along each axis"
                )
            }
            LatticeError::Count {
                counts: [ni, nj],
                held,
            } => write!(f, "declares {ni} × {nj} nodes and holds {held}"),
            LatticeError::Turned { along_i, along_j } => write!(
                f,
                "runs along ({}, {}) then ({}, {}): the pillars stand on a lattice along +x then +y of its frame, and a turned or reversed one is not re-indexed here — grid the horizons in the frame the lattice is straight in",
                along_i[0], along_i[1], along_j[0], along_j[1]
            ),
            LatticeError::Irregular {
                node: [i, j],
                at,
                expected,
            } => write!(
                f,
                "node ({i}, {j}) is at ({}, {}) where a regular lattice puts it at ({}, {})",
                at[0], at[1], expected[0], expected[1]
            ),
        }
    }
}

impl std::error::Error for LatticeError {}

/// The lattice of a grid's nodes — `points` in the house order, `counts` its
/// `[ni, nj]` — or the first thing that makes it none.
pub fn lattice_of(points: &[[f64; 3]], counts: [usize; 2]) -> Result<Lattice, LatticeError> {
    let [ni, nj] = counts;
    if ni < 2 || nj < 2 {
        return Err(LatticeError::TooSmall { counts });
    }
    if points.len() != ni * nj {
        return Err(LatticeError::Count {
            counts,
            held: points.len(),
        });
    }
    let at = |i: usize, j: usize| points[j * ni + i];
    let origin = at(0, 0);
    let along_i = [at(1, 0)[0] - origin[0], at(1, 0)[1] - origin[1]];
    let along_j = [at(0, 1)[0] - origin[0], at(0, 1)[1] - origin[1]];
    let (dx, dy) = (along_i[0], along_j[1]);
    if !(dx.is_finite() && dx > 0.0 && dy.is_finite() && dy > 0.0) {
        return Err(LatticeError::Turned { along_i, along_j });
    }
    let tolerance = TOLERANCE * dx.min(dy);
    if along_i[1].abs() > tolerance || along_j[0].abs() > tolerance {
        return Err(LatticeError::Turned { along_i, along_j });
    }
    for j in 0..nj {
        for i in 0..ni {
            let node = at(i, j);
            let expected = [origin[0] + i as f64 * dx, origin[1] + j as f64 * dy];
            if !((node[0] - expected[0]).abs() <= tolerance && (node[1] - expected[1]).abs() <= tolerance) {
                return Err(LatticeError::Irregular {
                    node: [i, j],
                    at: [node[0], node[1]],
                    expected,
                });
            }
        }
    }
    Ok(Lattice {
        origin: [origin[0], origin[1]],
        spacing: [dx, dy],
        counts,
    })
}

/// What to do where a horizon lies above the one declared over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Crossing {
    /// Refuse the grid.
    Refuse,
    /// Bring the lower horizon up to the upper one: a zero-thickness node.
    Pinch,
}

/// What a grid is built from.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    /// The horizons' heights, **top to bottom**, each `ni × nj` in the house
    /// order (`z[j·ni + i]`); a hole is a `NaN`.
    pub heights: &'a [Vec<f64>],
    /// `[ni, nj]` of the horizons' common lattice.
    pub counts: [usize; 2],
    /// One count per zone — per pair of consecutive horizons — top to bottom;
    /// `0` leaves the zone unmeshed, a K gap.
    pub layers: &'a [usize],
    /// A pillar on every `step`-th node of the lattice, along both axes.
    pub step: usize,
    /// Which way the third coordinate points.
    pub up: Up,
    /// What a crossing does.
    pub crossing: Crossing,
}

/// The first crossing met, pillar by pillar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crossed {
    /// The horizon above, by its rank in the request.
    pub upper: usize,
    /// The horizon that crosses it.
    pub lower: usize,
    /// The lattice node, `[i, j]`.
    pub node: [usize; 2],
    /// The upper horizon's height there.
    pub upper_z: f64,
    /// The lower horizon's height there.
    pub lower_z: f64,
}

/// Why no grid is built.
#[derive(Debug, Clone, PartialEq)]
pub enum IjkError {
    /// Fewer than two horizons: no zone.
    TooFewHorizons(usize),
    /// A horizon does not hold one height per lattice node.
    Heights {
        /// Its rank.
        horizon: usize,
        /// `ni × nj`.
        expected: usize,
        /// What it holds.
        given: usize,
    },
    /// Not one layer count per zone.
    LayerCount {
        /// The zones, horizons − 1.
        zones: usize,
        /// The counts given.
        given: usize,
    },
    /// A K gap as the first or the last zone.
    GapAtEdge {
        /// The zone, from 0 at the top.
        zone: usize,
    },
    /// Two K gaps in a row.
    GapsTogether {
        /// The second of the two zones.
        zone: usize,
    },
    /// A step that leaves fewer than two pillars along an axis.
    Step {
        /// The step.
        step: usize,
        /// `[ni, nj]` of the lattice.
        counts: [usize; 2],
    },
    /// More nodes than [`MAX_NODES`].
    TooMany {
        /// The nodes the grid would have.
        nodes: usize,
    },
    /// Horizons that cross, and [`Crossing::Refuse`].
    Crossed {
        /// Pillar nodes where a horizon lies above the one over it.
        count: usize,
        /// The first one.
        first: Crossed,
    },
    /// No cell has its eight corners.
    NothingDefined {
        /// The cells.
        cells: usize,
    },
}

impl fmt::Display for IjkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IjkError::TooFewHorizons(n) => {
                write!(f, "{n} horizon(s) given: a grid lies between two at least")
            }
            IjkError::Heights {
                horizon,
                expected,
                given,
            } => {
                write!(
                    f,
                    "horizon {} holds {given} height(s) where its lattice has {expected} node(s)",
                    horizon + 1
                )
            }
            IjkError::LayerCount { zones, given } => write!(
                f,
                "{given} count(s) given for {zones} zone(s): one per pair of consecutive horizons, top to bottom — 0 for a zone left unmeshed"
            ),
            IjkError::GapAtEdge { zone } => write!(
                f,
                "zone {} has no layer and is at the {} of the stack: a K gap lies between two layers — leave that horizon out instead",
                zone + 1,
                if *zone == 0 { "top" } else { "bottom" }
            ),
            IjkError::GapsTogether { zone } => write!(
                f,
                "zones {} and {} both have no layer: `GapAfterLayer` holds one gap after a layer — give the gap's top and base only",
                zone,
                zone + 1
            ),
            IjkError::Step {
                step,
                counts: [ni, nj],
            } => write!(
                f,
                "a pillar every {step} node(s) of a {ni} × {nj} lattice leaves fewer than 2 pillars along an axis — at least 1, and at most {}",
                (*ni).min(*nj).saturating_sub(1)
            ),
            IjkError::TooMany { nodes } => {
                write!(
                    f,
                    "the grid would have {nodes} nodes, past the {MAX_NODES} a grid may have — coarsen the pillars or use fewer layers"
                )
            }
            IjkError::Crossed { count, first } => write!(
                f,
                "{count} pillar node(s) have a horizon above the one declared over it — the first at lattice node ({}, {}), horizon {} at {} over horizon {} at {}; give the horizons top to bottom, correct the picks, or ask for `pinch`",
                first.node[0],
                first.node[1],
                first.lower + 1,
                first.lower_z,
                first.upper + 1,
                first.upper_z
            ),
            IjkError::NothingDefined { cells } => write!(
                f,
                "none of the {cells} cell(s) has its eight corners — every one touches a hole of a horizon; nothing would be drawn"
            ),
        }
    }
}

impl std::error::Error for IjkError {}

/// The grid, built.
#[derive(Debug, Clone, PartialEq)]
pub struct Stacked {
    /// `[Ni, Nj, Nk]`, cells.
    pub cells: [usize; 3],
    /// `[Ni + 1, Nj + 1]`, pillars.
    pub pillars: [usize; 2],
    /// `[ni, nj]` of the lattice the pillars stand on.
    pub lattice: [usize; 2],
    /// A pillar every this many lattice nodes.
    pub step: usize,
    /// `Nk + 1 + gaps`.
    pub levels: usize,
    /// The level each horizon sits at, top to bottom.
    pub horizon_levels: Vec<usize>,
    /// The level each layer's top sits at.
    pub layer_tops: Vec<usize>,
    /// The zone each layer belongs to.
    pub layer_zone: Vec<usize>,
    /// `levels × (Nj + 1) × (Ni + 1)` heights, `i` fastest.
    pub z: Vec<f64>,
    /// `Nk − 1` flags: a gap after that layer.
    pub gap_after_layer: Vec<bool>,
    /// `Nk × Nj × Ni` flags, `i` fastest: the cell's eight corners are known.
    pub cell_defined: Vec<bool>,
    /// `(Nj + 1) × (Ni + 1)` flags: the pillar has a node of a defined cell.
    pub pillar_defined: Vec<bool>,
    /// Horizon nodes on the pillars with no height.
    pub holes: usize,
    /// Pillar nodes where a horizon lay above the one over it — before pinching.
    pub crossed: usize,
    /// Pillar nodes where a meshed or unmeshed zone has zero thickness —
    /// after pinching.
    pub pinched: usize,
    /// Lattice nodes past the last pillar, along i and along j.
    pub trimmed: [usize; 2],
    /// Which way the third coordinate points.
    pub up: Up,
}

impl Stacked {
    /// K gaps.
    pub fn gaps(&self) -> usize {
        self.gap_after_layer.iter().filter(|gap| **gap).count()
    }

    /// Cells whose geometry is not defined.
    pub fn undefined_cells(&self) -> usize {
        self.cell_defined.iter().filter(|defined| !**defined).count()
    }

    /// The nodes, level first, then `j`, then `i` fastest — `x` and `y` taken
    /// from the lattice's own nodes (`nodes`, `ni × nj`, the house order), so
    /// that a pillar stands **exactly** where the horizons' node is.
    pub fn points(&self, nodes: &[[f64; 3]]) -> Vec<[f64; 3]> {
        let [pi, pj] = self.pillars;
        let li = self.lattice[0];
        let per_level = pi * pj;
        let mut out = Vec::with_capacity(self.levels * per_level);
        for level in 0..self.levels {
            for j in 0..pj {
                for i in 0..pi {
                    let node = nodes
                        .get(j * self.step * li + i * self.step)
                        .copied()
                        .unwrap_or([f64::NAN; 3]);
                    out.push([node[0], node[1], self.z[level * per_level + j * pi + i]]);
                }
            }
        }
        out
    }

    /// The defined cells, in RESQML's order (`i` fastest, then `j`, then `k`),
    /// as eight indices into [`Stacked::points`] in VTK's hexahedron order —
    /// the face `(0, 1, 2, 3)` wound so that its right-hand normal points at
    /// `(4, 5, 6, 7)` — and the RESQML index of each, empty when every cell is
    /// defined (the reader's convention, `energyml_geometry::ijk`).
    pub fn hexahedra(&self) -> (Vec<[u32; 8]>, Vec<u32>) {
        let [ni, nj, nk] = self.cells;
        let pi = self.pillars[0];
        let per_level = pi * self.pillars[1];
        let mut cells = Vec::new();
        let mut index = Vec::new();
        for (k, &top) in self.layer_tops.iter().enumerate() {
            for j in 0..nj {
                for i in 0..ni {
                    let cell = (k * nj + j) * ni + i;
                    if !self.cell_defined[cell] {
                        continue;
                    }
                    // Anticlockwise seen from +z: its right-hand normal is +z.
                    let around = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
                    // The face with the smaller z first: the top level in a
                    // depth frame, the bottom one in an elevation frame.
                    let (first, second) = match self.up {
                        Up::MinusZ => (top, top + 1),
                        Up::PlusZ => (top + 1, top),
                    };
                    let mut node = [0u32; 8];
                    for (corner, (a, b)) in around.iter().enumerate() {
                        let pillar = b * pi + a;
                        node[corner] = (first * per_level + pillar) as u32;
                        node[corner + 4] = (second * per_level + pillar) as u32;
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

    /// `GridIsRighthanded`: the sign of the triple product of the I, J and K
    /// tangents in the frame's numbers (TRG 2.2 §5.29 `IjkGridGeometry` p. 125,
    /// *« as determined by the triple product of tangent vectors in the I, J,
    /// and K directions »*). I runs along +x, J along +y, K down: −z in an
    /// elevation frame (left-handed), +z in a depth frame (right-handed).
    pub fn right_handed(&self) -> bool {
        matches!(self.up, Up::MinusZ)
    }
}

/// The grid between the horizons — or the first reason there is none.
pub fn stack(request: &Request<'_>) -> Result<Stacked, IjkError> {
    let Request {
        heights,
        counts,
        layers,
        step,
        up,
        crossing,
    } = *request;
    let [li, lj] = counts;
    let horizons = heights.len();
    if horizons < 2 {
        return Err(IjkError::TooFewHorizons(horizons));
    }
    for (horizon, one) in heights.iter().enumerate() {
        if one.len() != li * lj {
            return Err(IjkError::Heights {
                horizon,
                expected: li * lj,
                given: one.len(),
            });
        }
    }
    let zones = horizons - 1;
    if layers.len() != zones {
        return Err(IjkError::LayerCount {
            zones,
            given: layers.len(),
        });
    }
    for zone in [0, zones - 1] {
        if layers[zone] == 0 {
            return Err(IjkError::GapAtEdge { zone });
        }
    }
    if let Some(at) = layers.windows(2).position(|pair| pair[0] == 0 && pair[1] == 0) {
        return Err(IjkError::GapsTogether { zone: at + 1 });
    }
    if step == 0 || li < 2 || lj < 2 || li - 1 < step || lj - 1 < step {
        return Err(IjkError::Step { step, counts });
    }
    let (pi, pj) = ((li - 1) / step + 1, (lj - 1) / step + 1);
    let trimmed = [(li - 1) % step, (lj - 1) % step];
    let (ni, nj) = (pi - 1, pj - 1);
    let nk: usize = layers.iter().sum();
    let gaps = layers.iter().filter(|n| **n == 0).count();
    let levels = nk + 1 + gaps;
    let per_level = pi * pj;
    let nodes = levels.saturating_mul(per_level);
    if nodes > MAX_NODES {
        return Err(IjkError::TooMany { nodes });
    }

    // The horizons on the pillars, top to bottom.
    let mut on: Vec<Vec<f64>> = heights
        .iter()
        .map(|one| {
            (0..pj)
                .flat_map(|j| (0..pi).map(move |i| one[j * step * li + i * step]))
                .collect()
        })
        .collect();
    let holes = on.iter().flatten().filter(|z| !z.is_finite()).count();

    // Strictly above, in the frame's sense.
    let above = |a: f64, b: f64| match up {
        Up::PlusZ => a > b,
        Up::MinusZ => a < b,
    };
    let mut crossed = 0usize;
    let mut first: Option<Crossed> = None;
    for pillar in 0..per_level {
        // The last defined horizon above, on this pillar: a hole between two
        // horizons does not hide their crossing.
        let mut last: Option<(usize, f64)> = None;
        for (horizon, row) in on.iter_mut().enumerate() {
            let z = row[pillar];
            if !z.is_finite() {
                continue;
            }
            if let Some((upper, top)) = last
                && above(z, top)
            {
                crossed += 1;
                if first.is_none() {
                    first = Some(Crossed {
                        upper,
                        lower: horizon,
                        node: [pillar % pi * step, pillar / pi * step],
                        upper_z: top,
                        lower_z: z,
                    });
                }
                if crossing == Crossing::Pinch {
                    row[pillar] = top;
                }
            }
            last = Some((horizon, row[pillar]));
        }
    }
    if let (Crossing::Refuse, Some(first)) = (crossing, first) {
        return Err(IjkError::Crossed {
            count: crossed,
            first,
        });
    }

    // The levels: the top horizon, then each zone down to its base.
    let mut z: Vec<f64> = Vec::with_capacity(nodes);
    z.extend_from_slice(&on[0]);
    let mut horizon_levels = vec![0usize];
    let mut layer_tops = Vec::with_capacity(nk);
    let mut layer_zone = Vec::with_capacity(nk);
    let mut gap_after_layer: Vec<bool> = Vec::with_capacity(nk);
    let mut pinched = 0usize;
    let mut level = 0usize;
    for (zone, &n) in layers.iter().enumerate() {
        let (top, base) = (&on[zone], &on[zone + 1]);
        pinched += (0..per_level)
            .filter(|&p| top[p].is_finite() && top[p] == base[p])
            .count();
        if n == 0 {
            // A gap after the layer just written: the first zone is never one.
            if let Some(flag) = gap_after_layer.last_mut() {
                *flag = true;
            }
            z.extend_from_slice(base);
            level += 1;
        } else {
            for l in 1..=n {
                layer_tops.push(level);
                layer_zone.push(zone);
                gap_after_layer.push(false);
                if l == n {
                    // The horizon's own heights, copied: a node of the grid at
                    // a horizon's level **is** that horizon's node.
                    z.extend_from_slice(base);
                } else {
                    let t = l as f64 / n as f64;
                    z.extend((0..per_level).map(|p| top[p] + (base[p] - top[p]) * t));
                }
                level += 1;
            }
        }
        horizon_levels.push(level);
    }
    // `Nk − 1` interfaces: the flag after the last layer is no interface.
    gap_after_layer.pop();

    // A cell is defined when its eight corners are.
    let mut cell_defined = vec![false; nk * nj * ni];
    let mut pillar_defined = vec![false; per_level];
    for (k, &top) in layer_tops.iter().enumerate() {
        for j in 0..nj {
            for i in 0..ni {
                let around = [j * pi + i, j * pi + i + 1, (j + 1) * pi + i + 1, (j + 1) * pi + i];
                let known = around
                    .iter()
                    .all(|&p| z[top * per_level + p].is_finite() && z[(top + 1) * per_level + p].is_finite());
                if known {
                    cell_defined[(k * nj + j) * ni + i] = true;
                    for p in around {
                        pillar_defined[p] = true;
                    }
                }
            }
        }
    }
    if !cell_defined.iter().any(|defined| *defined) {
        return Err(IjkError::NothingDefined {
            cells: cell_defined.len(),
        });
    }

    Ok(Stacked {
        cells: [ni, nj, nk],
        pillars: [pi, pj],
        lattice: counts,
        step,
        levels,
        horizon_levels,
        layer_tops,
        layer_zone,
        z,
        gap_after_layer,
        cell_defined,
        pillar_defined,
        holes,
        crossed,
        pinched,
        trimmed,
        up,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 3 × 2 lattice at map coordinates, `i` fastest.
    fn nodes() -> Vec<[f64; 3]> {
        let mut out = Vec::new();
        for j in 0..2 {
            for i in 0..3 {
                out.push([651_000.0 + 25.0 * i as f64, 6_861_000.0 + 40.0 * j as f64, 0.0]);
            }
        }
        out
    }

    fn request<'a>(heights: &'a [Vec<f64>], layers: &'a [usize]) -> Request<'a> {
        Request {
            heights,
            counts: [3, 2],
            layers,
            step: 1,
            up: Up::PlusZ,
            crossing: Crossing::Refuse,
        }
    }

    /// The lattice read off its nodes, with both spacings — a transposed
    /// reading would swap 25 and 40.
    #[test]
    fn a_lattice_is_read_off_its_nodes() {
        let lattice = lattice_of(&nodes(), [3, 2]).unwrap();
        assert_eq!(lattice.origin, [651_000.0, 6_861_000.0]);
        assert_eq!(lattice.spacing, [25.0, 40.0]);
        assert!(lattice.same_as(&lattice));
        let shifted = Lattice {
            origin: [651_010.0, 6_861_000.0],
            ..lattice
        };
        assert!(
            !lattice.same_as(&shifted),
            "an origin 10 m off is another lattice"
        );
        let finer = Lattice {
            counts: [3, 3],
            ..lattice
        };
        assert!(!lattice.same_as(&finer));
    }

    /// A lattice turned by 30° is refused as turned, not re-indexed.
    #[test]
    fn a_turned_lattice_is_refused() {
        let (c, s) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
        let turned: Vec<[f64; 3]> = nodes()
            .into_iter()
            .map(|[x, y, z]| {
                let (u, v) = (x - 651_000.0, y - 6_861_000.0);
                [651_000.0 + c * u - s * v, 6_861_000.0 + s * u + c * v, z]
            })
            .collect();
        assert!(matches!(
            lattice_of(&turned, [3, 2]),
            Err(LatticeError::Turned { .. })
        ));
        let mut irregular = nodes();
        irregular[4][0] += 3.0;
        assert!(matches!(
            lattice_of(&irregular, [3, 2]),
            Err(LatticeError::Irregular { node: [1, 1], .. })
        ));
    }

    /// Two layers between two flat horizons: the middle level halfway, the
    /// outer ones the horizons' own heights, and every cell defined.
    #[test]
    fn two_layers_split_a_zone_linearly() {
        let heights = vec![vec![10.0; 6], vec![-20.0; 6]];
        let built = stack(&request(&heights, &[2])).unwrap();
        assert_eq!(built.cells, [2, 1, 2]);
        assert_eq!(built.levels, 3);
        assert_eq!(built.horizon_levels, vec![0, 2]);
        assert_eq!(&built.z[0..6], &[10.0; 6]);
        assert_eq!(&built.z[6..12], &[-5.0; 6]);
        assert_eq!(&built.z[12..18], &[-20.0; 6]);
        assert_eq!(built.undefined_cells(), 0);
        let (cells, index) = built.hexahedra();
        assert_eq!(cells.len(), 4);
        assert!(index.is_empty(), "every cell defined: positions are the indices");
        // Elevation, K down: the lower face first.
        assert_eq!(
            cells[0][0], 6,
            "the first cell's face (0,1,2,3) is its bottom, level 1"
        );
        assert!(!built.right_handed());
    }

    /// A gap zone adds a level and a flag, no layer.
    #[test]
    fn a_zone_of_no_layer_is_a_k_gap() {
        let heights = vec![vec![0.0; 6], vec![-10.0; 6], vec![-30.0; 6], vec![-60.0; 6]];
        let built = stack(&request(&heights, &[1, 0, 3])).unwrap();
        assert_eq!(built.cells[2], 4);
        assert_eq!(built.gap_after_layer, vec![true, false, false]);
        assert_eq!(built.levels, 4 + 1 + 1);
        assert_eq!(built.horizon_levels, vec![0, 1, 2, 5]);
        assert_eq!(
            built.layer_tops,
            vec![0, 2, 3, 4],
            "the layer under the gap starts one level further"
        );
        assert_eq!(built.z[2 * 6], -30.0);
        assert!(
            (built.z[3 * 6] + 40.0).abs() < 1e-12,
            "a third of the way from −30 to −60"
        );
    }

    /// A hole in the middle horizon undefines the cells of the two zones it
    /// bounds, and not those of the third.
    #[test]
    fn a_hole_undefines_only_the_cells_it_touches() {
        let mut middle = vec![-10.0; 6];
        middle[0] = f64::NAN;
        let heights = vec![vec![0.0; 6], middle, vec![-30.0; 6], vec![-60.0; 6]];
        let built = stack(&request(&heights, &[1, 1, 1])).unwrap();
        assert_eq!(built.holes, 1);
        // Layer 0 and 1, column (0, 0): undefined; layer 2 whole.
        assert_eq!(built.cell_defined, vec![false, true, false, true, true, true]);
        assert!(
            built.pillar_defined.iter().all(|defined| *defined),
            "every pillar has a defined cell in layer 2"
        );
        let (cells, index) = built.hexahedra();
        assert_eq!(cells.len(), 4);
        assert_eq!(index, vec![1, 3, 4, 5]);
    }

    /// A crossing is refused by default, pinched on request — the lower one
    /// brought to the upper one, and counted.
    #[test]
    fn a_crossing_is_refused_or_pinched() {
        let mut lower = vec![-10.0; 6];
        lower[5] = 5.0;
        let heights = vec![vec![0.0; 6], lower];
        let refused = stack(&request(&heights, &[2])).unwrap_err();
        assert!(
            matches!(
                refused,
                IjkError::Crossed {
                    count: 1,
                    first: Crossed {
                        node: [2, 1],
                        upper: 0,
                        lower: 1,
                        ..
                    }
                }
            ),
            "{refused:?}"
        );
        let pinched = stack(&Request {
            crossing: Crossing::Pinch,
            ..request(&heights, &[2])
        })
        .unwrap();
        assert_eq!(pinched.crossed, 1);
        assert_eq!(pinched.pinched, 1);
        assert_eq!(pinched.z[2 * 6 + 5], 0.0, "brought up to the upper horizon");
        assert_eq!(pinched.z[6 + 5], 0.0, "and the level between with it");
        // The same numbers read as depths are in order.
        let depth = stack(&Request {
            up: Up::MinusZ,
            ..request(&heights, &[2])
        });
        assert!(
            matches!(depth, Err(IjkError::Crossed { .. })),
            "in depth, −10 is above 0 everywhere else"
        );
    }

    /// A pillar every second node of a 5-node axis keeps 3; of a 4-node one,
    /// 2 and a node trimmed.
    #[test]
    fn a_coarser_lattice_takes_every_step_th_node() {
        let heights = vec![vec![0.0; 20], vec![-10.0; 20]];
        let built = stack(&Request {
            heights: &heights,
            counts: [5, 4],
            layers: &[1],
            step: 2,
            up: Up::PlusZ,
            crossing: Crossing::Refuse,
        })
        .unwrap();
        assert_eq!(built.pillars, [3, 2]);
        assert_eq!(built.cells, [2, 1, 1]);
        assert_eq!(built.trimmed, [0, 1]);
        let too_far = stack(&Request {
            heights: &heights,
            counts: [5, 4],
            layers: &[1],
            step: 4,
            up: Up::PlusZ,
            crossing: Crossing::Refuse,
        });
        assert!(matches!(too_far, Err(IjkError::Step { .. })));
    }

    /// What is refused before anything is built.
    #[test]
    fn a_stack_that_cannot_be_is_refused_by_name() {
        let two = vec![vec![0.0; 6], vec![-10.0; 6]];
        let four = vec![vec![0.0; 6], vec![-10.0; 6], vec![-20.0; 6], vec![-30.0; 6]];
        assert!(matches!(
            stack(&request(&two[..1], &[])),
            Err(IjkError::TooFewHorizons(1))
        ));
        assert!(matches!(
            stack(&request(&two, &[1, 1])),
            Err(IjkError::LayerCount { zones: 1, given: 2 })
        ));
        assert!(matches!(
            stack(&request(&two, &[0])),
            Err(IjkError::GapAtEdge { zone: 0 })
        ));
        assert!(matches!(
            stack(&request(&four, &[1, 1, 0])),
            Err(IjkError::GapAtEdge { zone: 2 })
        ));
        let five = vec![
            vec![0.0; 6],
            vec![-10.0; 6],
            vec![-20.0; 6],
            vec![-30.0; 6],
            vec![-40.0; 6],
        ];
        assert!(matches!(
            stack(&request(&five, &[1, 0, 0, 1])),
            Err(IjkError::GapsTogether { zone: 2 })
        ));
        let holes = vec![vec![f64::NAN; 6], vec![-10.0; 6]];
        assert!(matches!(
            stack(&request(&holes, &[1])),
            Err(IjkError::NothingDefined { cells: 2 })
        ));
    }

    /// The points stand on the lattice's own nodes.
    #[test]
    fn the_points_stand_on_the_lattice_nodes() {
        let heights = vec![vec![0.0; 6], vec![-10.0; 6]];
        let built = stack(&request(&heights, &[1])).unwrap();
        let points = built.points(&nodes());
        assert_eq!(points.len(), 12);
        assert_eq!(points[4], [651_025.0, 6_861_040.0, 0.0]);
        assert_eq!(points[6 + 2], [651_050.0, 6_861_000.0, -10.0]);
    }
}
