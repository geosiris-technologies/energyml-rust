//! The axes of a 2D section carried by each node of the well (a matrix `[m, n]` per element): which
//! dimension follows the high side and which the right, with what sign and which offsets in metres.
//!
//! Everything is **declared** by the caller (S2): the slow dimension (the first count of
//! `ValueCountPerIndexableElement`, RESQML TRG 2.2 section 7.1, "the top XML element is slower than the
//! bottom") and the lateral sign. The offsets come from the axes of the property (`component_axes`); this
//! module only places them.

use crate::around_well::hole_frame::{Frames, HoleFrame};
use crate::points::Point;
use crate::vector::{add, dot, scale, sub, unit};

/// Which axis of the plane across the hole a dimension runs along.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlaneAxis {
    /// Along the high side of the hole.
    HighSide,
    /// Along the right of an observer looking down the hole.
    Right,
}

/// The sign of a dimension along its axis. A lateral computed as "high side x tangent" in a direct frame is
/// `Right` with `Negative`; "tangent x high side" is `Positive`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AxisSign {
    /// Offsets count along the axis.
    Positive,
    /// Offsets count against the axis.
    Negative,
}

impl AxisSign {
    fn factor(self) -> f64 {
        match self {
            AxisSign::Positive => 1.0,
            AxisSign::Negative => -1.0,
        }
    }
}

/// One dimension of the matrix, declared.
#[derive(Clone, Debug, PartialEq)]
pub struct GridAxis {
    /// The axis of the plane this dimension runs along.
    pub along: PlaneAxis,
    /// The sign of its offsets along that axis.
    pub sign: AxisSign,
    /// Metres from the hole's centre, one per index of this dimension, strictly monotonic.
    pub offsets: Vec<f64>,
}

/// The axes of the per-node matrix: `slow` is the first count of `ValueCountPerIndexableElement`.
#[derive(Clone, Debug, PartialEq)]
pub struct TransverseGrid {
    slow: GridAxis,
    fast: GridAxis,
}

/// Which plane of the cube a ribbon shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RibbonPlane {
    /// The slow dimension's offsets, at this index of the fast one.
    FixedFast(usize),
    /// The fast dimension's offsets, at this index of the slow one.
    FixedSlow(usize),
}

/// Two consecutive slices that cross: the volume folds between these two depths.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Fold {
    /// The depth of the first slice.
    pub md_before: f64,
    /// The depth of the second slice.
    pub md_after: f64,
}

/// Why a grid cannot be built, by name.
#[derive(Clone, Debug, PartialEq)]
pub enum GridRefusal {
    /// Both dimensions run along the same axis.
    SameAxisTwice,
    /// A dimension with no offset.
    Empty {
        /// True for the slow dimension, false for the fast one.
        slow: bool,
    },
    /// An offset that is not a number.
    NotFinite {
        /// True for the slow dimension, false for the fast one.
        slow: bool,
        /// The index of the offset.
        at: usize,
    },
    /// Offsets that are not strictly monotonic.
    OffsetsNotMonotonic {
        /// True for the slow dimension, false for the fast one.
        slow: bool,
    },
    /// The property's per-node shape is not the grid's.
    ShapeMismatch {
        /// The per-node counts the property declares.
        declared: Vec<u64>,
        /// The grid's counts, slowest first.
        grid: [usize; 2],
    },
}

fn which(slow: bool) -> &'static str {
    if slow { "slow" } else { "fast" }
}

impl std::fmt::Display for GridRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GridRefusal::SameAxisTwice => {
                write!(
                    f,
                    "both dimensions run along the same axis: the plane is not spanned"
                )
            }
            GridRefusal::Empty { slow } => write!(f, "the {} dimension has no offset", which(*slow)),
            GridRefusal::NotFinite { slow, at } => {
                write!(f, "offset {at} of the {} dimension is not a number", which(*slow))
            }
            GridRefusal::OffsetsNotMonotonic { slow } => write!(
                f,
                "the offsets of the {} dimension are not strictly monotonic",
                which(*slow)
            ),
            GridRefusal::ShapeMismatch { declared, grid } => write!(
                f,
                "the property holds {declared:?} values per node, the grid is {grid:?} (slowest first, TRG 2.2 section 7.1)"
            ),
        }
    }
}

impl std::error::Error for GridRefusal {}

fn checked_axis(axis: &GridAxis, slow: bool) -> Result<(), GridRefusal> {
    if axis.offsets.is_empty() {
        return Err(GridRefusal::Empty { slow });
    }
    if let Some(at) = axis.offsets.iter().position(|o| !o.is_finite()) {
        return Err(GridRefusal::NotFinite { slow, at });
    }
    if axis.offsets.len() >= 2 {
        let up = axis.offsets[1] > axis.offsets[0];
        let ok = axis.offsets.windows(2).all(|w| {
            let step = w[1] - w[0];
            if up { step > 0.0 } else { step < 0.0 }
        });
        if !ok {
            return Err(GridRefusal::OffsetsNotMonotonic { slow });
        }
    }
    Ok(())
}

/// The lower and upper bounds of sample `i`, halfway to its neighbours (half a step past the ends); a single
/// value is a cell of no width.
fn bounds(offsets: &[f64], i: usize) -> Option<(f64, f64)> {
    let c = offsets.len();
    if i >= c {
        return None;
    }
    if c == 1 {
        return Some((offsets[0], offsets[0]));
    }
    let lower = if i == 0 {
        offsets[0] - (offsets[1] - offsets[0]) / 2.0
    } else {
        (offsets[i - 1] + offsets[i]) / 2.0
    };
    let upper = if i == c - 1 {
        offsets[c - 1] + (offsets[c - 1] - offsets[c - 2]) / 2.0
    } else {
        (offsets[i] + offsets[i + 1]) / 2.0
    };
    Some((lower, upper))
}

impl TransverseGrid {
    /// A grid from its two declared dimensions.
    pub fn new(slow: GridAxis, fast: GridAxis) -> Result<TransverseGrid, GridRefusal> {
        if slow.along == fast.along {
            return Err(GridRefusal::SameAxisTwice);
        }
        checked_axis(&slow, true)?;
        checked_axis(&fast, false)?;
        Ok(TransverseGrid { slow, fast })
    }

    /// The slow dimension.
    pub fn slow(&self) -> &GridAxis {
        &self.slow
    }

    /// The fast dimension.
    pub fn fast(&self) -> &GridAxis {
        &self.fast
    }

    /// `[m, n]`: the slow count, then the fast count.
    pub fn counts(&self) -> [usize; 2] {
        [self.slow.offsets.len(), self.fast.offsets.len()]
    }

    /// Refuses a per-node shape that is not `[m, n]`, slowest first.
    pub fn checked(&self, shape: &[u64]) -> Result<(), GridRefusal> {
        let [m, n] = self.counts();
        if shape == [m as u64, n as u64] {
            Ok(())
        } else {
            Err(GridRefusal::ShapeMismatch {
                declared: shape.to_vec(),
                grid: [m, n],
            })
        }
    }

    /// The signed offset of sample `i` of one dimension, as `(right, high)` contribution on its axis.
    fn on_plane(axis: &GridAxis, value: f64) -> [f64; 2] {
        match axis.along {
            PlaneAxis::Right => [value, 0.0],
            PlaneAxis::HighSide => [0.0, value],
        }
    }

    /// The rectangle of cell (j, k) in the canonical plane: `[[right_min, high_min], [right_max, high_max]]`.
    pub fn cell(&self, j: usize, k: usize) -> Option<[[f64; 2]; 2]> {
        let mut min = [0.0; 2];
        let mut max = [0.0; 2];
        for (axis, i) in [(&self.slow, j), (&self.fast, k)] {
            let (lower, upper) = bounds(&axis.offsets, i)?;
            let (a, b) = (axis.sign.factor() * lower, axis.sign.factor() * upper);
            let slot = match axis.along {
                PlaneAxis::Right => 0,
                PlaneAxis::HighSide => 1,
            };
            min[slot] = a.min(b);
            max[slot] = a.max(b);
        }
        Some([min, max])
    }

    /// Sample (j, k) in the canonical plane, (right, high side).
    pub fn plane_point(&self, j: usize, k: usize) -> Option<[f64; 2]> {
        let a = Self::on_plane(&self.slow, self.signed(&self.slow, j)?);
        let b = Self::on_plane(&self.fast, self.signed(&self.fast, k)?);
        Some([a[0] + b[0], a[1] + b[1]])
    }

    fn signed(&self, axis: &GridAxis, i: usize) -> Option<f64> {
        axis.offsets.get(i).map(|o| axis.sign.factor() * o)
    }

    /// Sample (j, k) in space: `frame.at` plus each dimension's signed offset along its axis.
    pub fn lattice_point(&self, frame: &HoleFrame, j: usize, k: usize) -> Option<Point> {
        let plane = self.plane_point(j, k)?;
        Some(add(
            add(frame.at, scale(frame.right, plane[0])),
            scale(frame.high_side, plane[1]),
        ))
    }

    /// The m x n points of one node, index `j * n + k`.
    pub fn slice(&self, frame: &HoleFrame) -> Vec<Point> {
        let [m, n] = self.counts();
        let mut points = Vec::with_capacity(m * n);
        for j in 0..m {
            for k in 0..n {
                if let Some(point) = self.lattice_point(frame, j, k) {
                    points.push(point);
                }
            }
        }
        points
    }

    /// Every pair of consecutive frames whose slices cross.
    ///
    /// A pair folds when a corner of the slice goes backwards along the mean tangent of the two frames: on a
    /// bend of radius R, a point offset by `d` towards the centre advances by `(1 - d/R)` times the step, and
    /// goes back when `d > R`. A refused frame cuts the check: nothing is bridged over it.
    pub fn check_folds(&self, frames: &Frames) -> Vec<Fold> {
        let [m, n] = self.counts();
        let corners = [(0, 0), (0, n - 1), (m - 1, 0), (m - 1, n - 1)];
        let mut folds = Vec::new();
        for pair in frames.frames.windows(2) {
            let (Ok(a), Ok(b)) = (&pair[0], &pair[1]) else {
                continue;
            };
            let Some(mean) = unit(add(a.tangent, b.tangent)) else {
                continue;
            };
            let folded = corners.iter().any(|&(j, k)| {
                match (self.lattice_point(a, j, k), self.lattice_point(b, j, k)) {
                    (Some(pa), Some(pb)) => dot(sub(pb, pa), mean) <= 0.0,
                    _ => false,
                }
            });
            if folded {
                folds.push(Fold {
                    md_before: a.md,
                    md_after: b.md,
                });
            }
        }
        folds
    }
}
