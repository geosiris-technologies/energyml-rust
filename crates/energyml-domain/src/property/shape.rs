//! The shape of a property's values: components, elements, time, realizations.
//!
//! RESQML TRG 2.2 §7 p. 221 gives the order of the dimensions, slowest first:
//! `[RealizationCount] [TimeCount] [IndexableElementCount...]
//! [ValueCountPerIndexableElement...]`, and the Usage Guide 2.2 §4.2.2 p. 39
//! says that the 1D reading of an HDF5 dataset and the RESQML index order are
//! the same: **flattening consecutive dimensions never changes the order of
//! the values**. Everything here rests on those two facts.
//!
//! - [`ElementShape`]: the dimensions of the values of *one* element, and their
//!   labels (the vector or tensor part);
//! - [`ComponentPick`]: which of those components a caller keeps, and how a
//!   dimension is reduced ([`Reduction`]);
//! - [`PropertyShape`]: the whole shape, and its confrontation with the
//!   dimensions a file stores ([`StoredLayout`]);
//! - [`count_fit`]: the one rule "does the count read fit the shape?".

use std::ops::Range;

use energyml_core::value::Reflect;

use super::{IndexableElement, Property, TimeRef};
use crate::error::Result as DomainResult;

// ---------------------------------------------------------------------------
// The count against the shape (the one door)
// ---------------------------------------------------------------------------

/// What the count of values read says against the declared shape - the one
/// rule, for `Property::check`, the scene, and any reader.
///
/// RESQML TRG 2.2 §7.1 p. 222: *"The size of the vector is constant on all
/// elements"*; `ValueCountPerIndexableElement` is the count per element.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CountFit {
    /// Exactly `elements x values per element`.
    Fits,
    /// Declares several values per element and holds exactly one per element:
    /// usable as scalars, and the declaration is what is wrong.
    OnePerElement {
        /// The count the document declares.
        declared: u64,
    },
    /// Anything else, a zero count included: never lined up.
    Mismatch {
        /// `elements x per_element`.
        expected: u64,
        /// What was read.
        actual: u64,
    },
}

/// The one rule: `element_count` elements of `per_element` values, against `actual`.
pub fn count_fit(element_count: u64, per_element: u64, actual: u64) -> CountFit {
    let expected = element_count.saturating_mul(per_element.max(1));
    if element_count == 0 || expected == 0 {
        return CountFit::Mismatch { expected: 0, actual };
    }
    if actual == expected {
        CountFit::Fits
    } else if per_element > 1 && actual == element_count {
        CountFit::OnePerElement {
            declared: per_element,
        }
    } else {
        CountFit::Mismatch { expected, actual }
    }
}

// ---------------------------------------------------------------------------
// The components of one element
// ---------------------------------------------------------------------------

/// The dimensions of one element's values, slowest first, and their labels.
///
/// RESQML TRG 2.2 §7.1 p. 222: "First (left) given dimension is slowest and
/// last (right) given dimension is fastest." A scalar is `[1]`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ElementShape {
    /// Values per element along each dimension, slowest first; never empty,
    /// never a zero.
    pub dims: Vec<usize>,
    /// `LabelPerComponent`, linearised like the values (one per component,
    /// row-major); `None` when the file gives none.
    pub labels: Option<Vec<String>>,
}

/// Why a shape cannot be built, or does not hold.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum ShapeRefusal {
    /// A dimension of the value count per element is 0.
    #[error("dimension {at} of the value count per element is 0: no value can hang on such an element")]
    ZeroDimension {
        /// Which dimension, from 0.
        at: usize,
    },
    /// The labels do not count the components.
    #[error(
        "{labels} label(s) per component for {expected} component(s): `LabelPerComponent` lists one label per component, linearised (RESQML TRG 2.2 §7.1 p. 222)"
    )]
    LabelCount {
        /// Labels the file lists.
        labels: usize,
        /// Components the shape holds.
        expected: usize,
    },
    /// The product of the counts does not fit a memory index.
    #[error("the value count per element does not fit in this platform's memory index")]
    Overflow,
    /// The time is a dimension and the number of instants was not given.
    #[error("the property's time is a series dimension, and its number of instants was not given")]
    TimeNotGiven,
    /// The realizations are a dimension and their number was not given.
    #[error("the property declares `RealizationIndices`, and their number was not given")]
    RealizationsNotGiven,
    /// The stored time dimension contradicts the series.
    #[error(
        "the time dimension holds {array} step(s) where the series has {instants} instant(s) (UseInterval: {use_interval})"
    )]
    TimeCountMismatch {
        /// Steps the stored array holds.
        array: u64,
        /// Instants of the series.
        instants: u64,
        /// `TimeOrIntervalSeries.UseInterval`.
        use_interval: bool,
    },
    /// The stored dimensions are not a regrouping of the declared ones.
    #[error("the stored dimensions {stored:?} are not a regrouping of the declared ones {expected:?}")]
    StoredMismatch {
        /// The dimensions the store gave.
        stored: Vec<u64>,
        /// The dimensions the document declares.
        expected: Vec<u64>,
    },
}

impl ElementShape {
    /// From the counts and labels a file states. Empty counts are a scalar,
    /// `[1]` (what `Property::values_per_element` already assumes).
    pub fn of(counts: &[u64], labels: &[String]) -> Result<ElementShape, ShapeRefusal> {
        let mut dims = Vec::with_capacity(counts.len().max(1));
        let mut product: usize = 1;
        for (at, count) in counts.iter().enumerate() {
            if *count == 0 {
                return Err(ShapeRefusal::ZeroDimension { at });
            }
            let dim = usize::try_from(*count).map_err(|_| ShapeRefusal::Overflow)?;
            product = product.checked_mul(dim).ok_or(ShapeRefusal::Overflow)?;
            dims.push(dim);
        }
        if dims.is_empty() {
            dims.push(1);
        }
        let labels = if labels.is_empty() {
            None
        } else if labels.len() == product {
            Some(labels.to_vec())
        } else {
            return Err(ShapeRefusal::LabelCount {
                labels: labels.len(),
                expected: product,
            });
        };
        Ok(ElementShape { dims, labels })
    }

    /// `ElementShape::of(&p.value_count_per_element, &p.label_per_component)`.
    pub fn of_property(property: &Property<'_>) -> Result<ElementShape, ShapeRefusal> {
        ElementShape::of(&property.value_count_per_element, &property.label_per_component)
    }

    /// Values per element: the product of `dims`.
    #[allow(clippy::len_without_is_empty)] // a shape is never empty: see `dims`
    pub fn len(&self) -> usize {
        self.dims.iter().product()
    }

    /// Number of dimensions.
    pub fn rank(&self) -> usize {
        self.dims.len()
    }

    /// One value per element.
    pub fn is_scalar(&self) -> bool {
        self.len() == 1
    }

    /// The linear position of a multi-index, row-major, slowest first; `None`
    /// when `at` has the wrong rank or an index is past its dimension.
    pub fn ravel(&self, at: &[usize]) -> Option<usize> {
        if at.len() != self.dims.len() {
            return None;
        }
        let mut linear = 0usize;
        for (index, dim) in at.iter().zip(&self.dims) {
            if index >= dim {
                return None;
            }
            linear = linear * dim + index;
        }
        Some(linear)
    }

    /// The multi-index of a linear position; `None` past `len()`.
    pub fn unravel(&self, linear: usize) -> Option<Vec<usize>> {
        if linear >= self.len() {
            return None;
        }
        let mut rest = linear;
        let mut at = vec![0usize; self.dims.len()];
        for (slot, dim) in at.iter_mut().zip(&self.dims).rev() {
            *slot = rest % dim;
            rest /= dim;
        }
        Some(at)
    }

    /// The label of a component, when the file gives labels.
    pub fn label_of(&self, at: &[usize]) -> Option<&str> {
        let position = self.ravel(at)?;
        self.labels.as_ref()?.get(position).map(String::as_str)
    }

    /// `ravel`, refusing by name (what a face sends to the studio and to a
    /// value request).
    pub fn flat_component(&self, at: &[usize]) -> Result<usize, ComponentRefusal> {
        if at.len() != self.dims.len() {
            return Err(ComponentRefusal::Rank {
                axes: self.dims.len(),
                given: at.len(),
            });
        }
        for (axis, (index, dim)) in at.iter().zip(&self.dims).enumerate() {
            if index >= dim {
                return Err(ComponentRefusal::OutOfRange {
                    axis,
                    at: *index,
                    len: *dim,
                });
            }
        }
        self.ravel(at).ok_or(ComponentRefusal::Rank {
            axes: self.dims.len(),
            given: at.len(),
        })
    }

    /// `unravel`, refusing by name (`ValueVectorIndex` to a multi-index). A
    /// position past the end is reported on the **linear** space: `axis` is 0
    /// and `len` is `self.len()`.
    pub fn component_at(&self, linear: usize) -> Result<Vec<usize>, ComponentRefusal> {
        self.unravel(linear).ok_or(ComponentRefusal::OutOfRange {
            axis: 0,
            at: linear,
            len: self.len(),
        })
    }
}

// ---------------------------------------------------------------------------
// Picking components, reducing a dimension
// ---------------------------------------------------------------------------

/// Which values of one element to keep.
#[derive(Clone, PartialEq, Debug)]
pub enum ComponentPick {
    /// Every component, in linear order.
    All,
    /// One component, by its multi-index.
    At(Vec<usize>),
    /// Every component along `axis`, the other axes fixed at `at`
    /// (`at[axis]` is ignored).
    Along {
        /// The dimension shown.
        axis: usize,
        /// The index of every dimension (`at[axis]` is ignored).
        at: Vec<usize>,
    },
    /// One value per element, reduced over all its components.
    Reduce(Reduction),
    /// Every component along `axis`; the dimensions listed in `reduced` reduced
    /// together by `by` over their whole length; every other dimension fixed at
    /// `at` (`at[axis]` and `at[d]` for `d` in `reduced` are ignored). The
    /// "projection" of a dimension. `reduced` is never empty: with nothing
    /// reduced, the pick is `Along` (see [`ComponentPick::along_reduced`]).
    AlongReduced {
        /// The dimension shown.
        axis: usize,
        /// The index of every dimension; ignored for `axis` and `reduced`.
        at: Vec<usize>,
        /// The dimensions reduced together.
        reduced: Vec<usize>,
        /// How they are reduced.
        by: Reduction,
    },
}

/// How components are reduced to one value. Only finite values take part (a
/// norm is the exception: see [`Reduction::apply`]).
///
/// The one arithmetic of a reduction in the domain: `Reduce` and
/// `AlongReduced` both call [`Reduction::apply`]. The sheet crate's drawing
/// reduces *rows of pixels* by the same rule ("finite values only, `NaN`
/// otherwise"), written there a second time on purpose: the drawing cannot
/// call the domain.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub enum Reduction {
    /// The mean of the finite values.
    Mean,
    /// The smallest finite value.
    Min,
    /// The largest finite value.
    Max,
    /// The length of a vector.
    Norm,
}

impl Reduction {
    /// Every reduction, in a stable order (what a face offers).
    pub const ALL: [Reduction; 4] = [Reduction::Mean, Reduction::Min, Reduction::Max, Reduction::Norm];

    /// The one arithmetic of a reduction. `Mean`, `Min` and `Max` take the
    /// finite values only and give `NaN` when none is finite; `Norm` is the
    /// square root of the sum of squares and gives `NaN` as soon as one value
    /// is not finite (a norm with a missing component is unknown). An empty
    /// slice gives `NaN`.
    pub fn apply(self, values: &[f64]) -> f64 {
        match self {
            Reduction::Mean => {
                let mut sum = 0.0;
                let mut n = 0usize;
                for v in values.iter().filter(|v| v.is_finite()) {
                    sum += v;
                    n += 1;
                }
                if n == 0 { f64::NAN } else { sum / n as f64 }
            }
            Reduction::Min => values
                .iter()
                .copied()
                .filter(|v| v.is_finite())
                .fold(None, |acc: Option<f64>, v| Some(acc.map_or(v, |a| a.min(v))))
                .unwrap_or(f64::NAN),
            Reduction::Max => values
                .iter()
                .copied()
                .filter(|v| v.is_finite())
                .fold(None, |acc: Option<f64>, v| Some(acc.map_or(v, |a| a.max(v))))
                .unwrap_or(f64::NAN),
            Reduction::Norm => {
                if values.is_empty() || values.iter().any(|v| !v.is_finite()) {
                    f64::NAN
                } else {
                    values.iter().map(|v| v * v).sum::<f64>().sqrt()
                }
            }
        }
    }

    /// Stable id, written in a recipe or a view document: "mean", "min", "max",
    /// "norm".
    pub fn id(self) -> &'static str {
        match self {
            Reduction::Mean => "mean",
            Reduction::Min => "min",
            Reduction::Max => "max",
            Reduction::Norm => "norm",
        }
    }

    /// The reduction an id names; `None` for an id this build does not know.
    pub fn from_id(id: &str) -> Option<Reduction> {
        Reduction::ALL.into_iter().find(|reduction| reduction.id() == id)
    }
}

/// Why a pick does not fit a shape.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum ComponentRefusal {
    /// The wrong number of indices.
    #[error("{given} index(es) for a shape of {axes} dimension(s)")]
    Rank {
        /// Dimensions the shape has.
        axes: usize,
        /// Indices given.
        given: usize,
    },
    /// An index (or an axis) past its dimension.
    #[error("index {at} on dimension {axis}, which holds {len}")]
    OutOfRange {
        /// The dimension.
        axis: usize,
        /// The index given.
        at: usize,
        /// What the dimension holds.
        len: usize,
    },
    /// A norm of something that is not a vector.
    #[error("a norm is the length of a vector: this shape has {rank} dimension(s)")]
    NotAVector {
        /// Dimensions the shape has.
        rank: usize,
    },
    /// An element row of the wrong length.
    #[error("an element row of {found} value(s) where the shape holds {expected}")]
    RowLength {
        /// Values the shape holds.
        expected: usize,
        /// Values given.
        found: usize,
    },
    /// An `AlongReduced` that reduces nothing.
    #[error("no dimension is reduced along dimension {axis}: that pick is `Along`")]
    NothingReduced {
        /// The dimension shown.
        axis: usize,
    },
    /// The shown dimension is also listed as reduced.
    #[error("dimension {dim} is the one shown: it cannot also be reduced")]
    ReducedAxis {
        /// The dimension.
        dim: usize,
    },
    /// A dimension listed twice among the reduced ones.
    #[error("dimension {dim} is listed twice among the reduced ones")]
    RepeatedDimension {
        /// The dimension.
        dim: usize,
    },
}

/// The components a pick reads, as a block of the component dimensions, and the
/// same pick restated on that block.
#[derive(Clone, PartialEq, Debug)]
pub struct NarrowedPick {
    /// Per component dimension, slowest first: where the block starts.
    pub start: Vec<u64>,
    /// Per component dimension, slowest first: how much the block holds.
    pub count: Vec<u64>,
    /// The shape of the block (`count`, as an `ElementShape`; no labels).
    pub shape: ElementShape,
    /// The pick restated on `shape`: `pick.select(&shape, block_row)` equals
    /// the original pick on the whole row.
    pub pick: ComponentPick,
}

impl ComponentPick {
    /// `AlongReduced`, or `Along` when `reduced` is empty: the one canonical
    /// form.
    pub fn along_reduced(axis: usize, at: Vec<usize>, reduced: Vec<usize>, by: Reduction) -> ComponentPick {
        if reduced.is_empty() {
            ComponentPick::Along { axis, at }
        } else {
            ComponentPick::AlongReduced {
                axis,
                at,
                reduced,
                by,
            }
        }
    }

    /// Whether the pick fits the shape, before anything is read.
    ///
    /// `Reduce(Norm)` needs a shape of rank 1: that is all that can be checked
    /// here, the facet `direction` (X, Y, Z) of the components being the
    /// caller's to read.
    pub fn check(&self, shape: &ElementShape) -> Result<(), ComponentRefusal> {
        let rank = shape.rank();
        match self {
            ComponentPick::All => Ok(()),
            ComponentPick::Reduce(by) => {
                if *by == Reduction::Norm && rank != 1 {
                    Err(ComponentRefusal::NotAVector { rank })
                } else {
                    Ok(())
                }
            }
            ComponentPick::At(at) => shape.flat_component(at).map(|_| ()),
            ComponentPick::Along { axis, at } => {
                // An axis past the rank is reported as an index past the axes.
                if *axis >= rank {
                    return Err(ComponentRefusal::OutOfRange {
                        axis: *axis,
                        at: *axis,
                        len: rank,
                    });
                }
                if at.len() != rank {
                    return Err(ComponentRefusal::Rank {
                        axes: rank,
                        given: at.len(),
                    });
                }
                for (k, (index, dim)) in at.iter().zip(&shape.dims).enumerate() {
                    if k != *axis && index >= dim {
                        return Err(ComponentRefusal::OutOfRange {
                            axis: k,
                            at: *index,
                            len: *dim,
                        });
                    }
                }
                Ok(())
            }
            ComponentPick::AlongReduced {
                axis,
                at,
                reduced,
                by,
            } => {
                if reduced.is_empty() {
                    return Err(ComponentRefusal::NothingReduced { axis: *axis });
                }
                if *axis >= rank {
                    return Err(ComponentRefusal::OutOfRange {
                        axis: *axis,
                        at: *axis,
                        len: rank,
                    });
                }
                if at.len() != rank {
                    return Err(ComponentRefusal::Rank {
                        axes: rank,
                        given: at.len(),
                    });
                }
                let mut seen: Vec<usize> = Vec::with_capacity(reduced.len());
                for dim in reduced {
                    if *dim >= rank {
                        return Err(ComponentRefusal::OutOfRange {
                            axis: *dim,
                            at: *dim,
                            len: rank,
                        });
                    }
                    if dim == axis {
                        return Err(ComponentRefusal::ReducedAxis { dim: *dim });
                    }
                    if seen.contains(dim) {
                        return Err(ComponentRefusal::RepeatedDimension { dim: *dim });
                    }
                    seen.push(*dim);
                }
                for (k, (index, dim)) in at.iter().zip(&shape.dims).enumerate() {
                    if k != *axis && !reduced.contains(&k) && index >= dim {
                        return Err(ComponentRefusal::OutOfRange {
                            axis: k,
                            at: *index,
                            len: *dim,
                        });
                    }
                }
                // A norm reduces a whole vector, never one dimension of a matrix.
                if *by == Reduction::Norm {
                    return Err(ComponentRefusal::NotAVector { rank });
                }
                Ok(())
            }
        }
    }

    /// How many values the pick keeps per element.
    pub fn width(&self, shape: &ElementShape) -> Result<usize, ComponentRefusal> {
        self.check(shape)?;
        Ok(match self {
            ComponentPick::All => shape.len(),
            ComponentPick::At(_) | ComponentPick::Reduce(_) => 1,
            ComponentPick::Along { axis, .. } | ComponentPick::AlongReduced { axis, .. } => shape.dims[*axis],
        })
    }

    /// The linear positions, inside one element, the pick reads (all of them
    /// for `Reduce`). For `AlongReduced`: for each index along `axis`, the
    /// positions of the reduced dimensions in row-major order (the slowest
    /// reduced dimension first), so `dims[axis]` groups of equal length.
    pub fn positions(&self, shape: &ElementShape) -> Result<Vec<usize>, ComponentRefusal> {
        self.check(shape)?;
        match self {
            ComponentPick::All | ComponentPick::Reduce(_) => Ok((0..shape.len()).collect()),
            ComponentPick::At(at) => Ok(vec![shape.flat_component(at)?]),
            ComponentPick::Along { axis, at } => {
                let mut out = Vec::with_capacity(shape.dims[*axis]);
                let mut index = at.clone();
                for i in 0..shape.dims[*axis] {
                    index[*axis] = i;
                    out.push(shape.flat_component(&index)?);
                }
                Ok(out)
            }
            ComponentPick::AlongReduced {
                axis, at, reduced, ..
            } => {
                let mut sorted = reduced.clone();
                sorted.sort_unstable();
                let mut out = Vec::new();
                let mut index = at.clone();
                for i in 0..shape.dims[*axis] {
                    index[*axis] = i;
                    for d in &sorted {
                        index[*d] = 0;
                    }
                    'combinations: loop {
                        out.push(shape.flat_component(&index)?);
                        for d in sorted.iter().rev() {
                            index[*d] += 1;
                            if index[*d] < shape.dims[*d] {
                                continue 'combinations;
                            }
                            index[*d] = 0;
                        }
                        break;
                    }
                }
                Ok(out)
            }
        }
    }

    /// Apply the pick to one element's values (`row.len() == shape.len()`).
    ///
    /// A `NaN` chosen by `All`, `At` or `Along` stays `NaN`, never replaced.
    /// `AlongReduced` reduces the values of **all** its reduced dimensions
    /// together: a mean over two reduced dimensions is the mean of the finite
    /// values of the sub-block, not a mean of means (with a `NaN` the two
    /// differ).
    pub fn select(&self, shape: &ElementShape, row: &[f64]) -> Result<Vec<f64>, ComponentRefusal> {
        if row.len() != shape.len() {
            return Err(ComponentRefusal::RowLength {
                expected: shape.len(),
                found: row.len(),
            });
        }
        self.check(shape)?;
        match self {
            ComponentPick::Reduce(by) => Ok(vec![by.apply(row)]),
            ComponentPick::AlongReduced { axis, by, .. } => {
                let positions = self.positions(shape)?;
                let group = positions.len() / shape.dims[*axis];
                Ok(positions
                    .chunks(group)
                    .map(|chunk| {
                        let values: Vec<f64> = chunk.iter().map(|p| row[*p]).collect();
                        by.apply(&values)
                    })
                    .collect())
            }
            _ => Ok(self.positions(shape)?.into_iter().map(|p| row[p]).collect()),
        }
    }

    /// The block of components this pick reads; `None` when it reads every
    /// component (`All`, `Reduce`).
    pub fn narrowed(&self, shape: &ElementShape) -> Result<Option<NarrowedPick>, ComponentRefusal> {
        self.check(shape)?;
        let rank = shape.rank();
        let dim = |k: usize| shape.dims[k] as u64;
        let (start, count, pick) = match self {
            ComponentPick::All | ComponentPick::Reduce(_) => return Ok(None),
            ComponentPick::At(at) => (
                at.iter().map(|i| *i as u64).collect::<Vec<u64>>(),
                vec![1u64; rank],
                ComponentPick::At(vec![0; rank]),
            ),
            ComponentPick::Along { axis, at } => {
                let mut start: Vec<u64> = at.iter().map(|i| *i as u64).collect();
                let mut count = vec![1u64; rank];
                start[*axis] = 0;
                count[*axis] = dim(*axis);
                (
                    start,
                    count,
                    ComponentPick::Along {
                        axis: *axis,
                        at: vec![0; rank],
                    },
                )
            }
            ComponentPick::AlongReduced {
                axis,
                at,
                reduced,
                by,
            } => {
                let mut start: Vec<u64> = at.iter().map(|i| *i as u64).collect();
                let mut count = vec![1u64; rank];
                start[*axis] = 0;
                count[*axis] = dim(*axis);
                for d in reduced {
                    start[*d] = 0;
                    count[*d] = dim(*d);
                }
                (
                    start,
                    count,
                    ComponentPick::AlongReduced {
                        axis: *axis,
                        at: vec![0; rank],
                        reduced: reduced.clone(),
                        by: *by,
                    },
                )
            }
        };
        let narrowed = ElementShape::of(&count, &[])
            .expect("a narrowed count is never 0, `check` ran and every dimension holds at least 1");
        Ok(Some(NarrowedPick {
            start,
            count,
            shape: narrowed,
            pick,
        }))
    }
}

// ---------------------------------------------------------------------------
// The whole shape
// ---------------------------------------------------------------------------

/// The elements of a representation, as an array axis or several.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ElementAxes {
    /// Elements with an order the guide states, slowest first: a frame's nodes
    /// `[n]`, an IJK grid's cells `[nk, nj, ni]` (Usage Guide 2.2 §6.6.3.1
    /// p. 65).
    Ordered(Vec<u64>),
    /// Elements with no "along": triangles, scattered points. Their count.
    Unordered(u64),
}

impl ElementAxes {
    /// How many elements, all axes together.
    pub fn count(&self) -> u64 {
        match self {
            ElementAxes::Ordered(dims) => dims.iter().fold(1u64, |acc, d| acc.saturating_mul(*d)),
            ElementAxes::Unordered(count) => *count,
        }
    }
}

/// A time dimension and the series it comes from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimeAxis {
    /// Steps in the array: `instants`, or `instants - 1` with `UseInterval`.
    pub steps: u64,
    /// Instants of the `TimeSeries`.
    pub instants: u64,
    /// `TimeOrIntervalSeries.UseInterval`.
    pub use_interval: bool,
}

impl TimeAxis {
    /// The time axis of a series of `instants` instants, read as instants or
    /// intervals. One value per interval between two instants is the reading
    /// the library writes (EML TRG 2.3 p. 296 states `UseInterval` without a
    /// text), hence `n - 1` steps for `n` instants.
    pub fn of(instants: u64, use_interval: bool) -> TimeAxis {
        TimeAxis {
            steps: if use_interval {
                instants.saturating_sub(1)
            } else {
                instants
            },
            instants,
            use_interval,
        }
    }
}

/// The whole shape of a property's values, slowest first:
/// `[realizations][time][elements...][components...]` (RESQML TRG 2.2 §7
/// p. 221; Usage Guide 2.2 §6.6.3 p. 65).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PropertyShape {
    /// Realizations, when `RealizationIndices` makes them a dimension.
    pub realizations: Option<u64>,
    /// Time, when a series makes it a dimension.
    pub time: Option<TimeAxis>,
    /// The elements.
    pub elements: ElementAxes,
    /// The components of one element.
    pub components: ElementShape,
}

/// How the stored dimensions regroup the declared ones.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StoredLayout {
    /// The stored dimensions, as `ArraySource::metadata` gave them.
    pub stored: Vec<u64>,
    /// For each stored dimension, the range of declared dimensions it holds
    /// (`PropertyShape::declared_dims` positions); an empty range for a stored
    /// 1 the declaration does not have.
    pub groups: Vec<Range<usize>>,
}

/// Whether a property's time is a dimension of its values (the 2.2
/// `TimeOrIntervalSeries` form: a series and no single index). An inherited
/// time index (2.0.1) is not a dimension (Usage Guide 2.2 §6.6.3 p. 65: "It is
/// not related to the inherited time index, which should be ignored if also
/// present.").
pub fn time_is_a_dimension(time: Option<&TimeRef>) -> bool {
    time.is_some_and(|t| t.index.is_none() && t.series.is_some())
}

/// The elements a property hangs on, ordered when the guide states their
/// order.
///
/// `Ordered` only for the pairs whose order is written; everything else the
/// library can count is `Unordered`. A truncated grid stays `Unordered`: its
/// truncation cells join the index space, which is no longer a lattice.
pub fn element_axes(representation: &dyn Reflect, element: IndexableElement) -> DomainResult<ElementAxes> {
    use IndexableElement::*;
    let name = representation.type_info().name;
    let kind = name.trim_start_matches("obj_");
    match kind {
        "IjkGridRepresentation" if super::truncation_cells(representation) == 0 => match element {
            Cells | Columns | Layers => {
                let ni = super::required(representation, "Ni")?;
                let nj = super::required(representation, "Nj")?;
                let nk = super::required(representation, "Nk")?;
                Ok(ElementAxes::Ordered(match element {
                    Cells => vec![nk, nj, ni],
                    Columns => vec![nj, ni],
                    _ => vec![nk],
                }))
            }
            _ => unordered(representation, element),
        },
        "Grid2dRepresentation" if element == Nodes => {
            let (fastest, slowest) = super::grid2d_axes(representation)?;
            Ok(ElementAxes::Ordered(vec![slowest, fastest]))
        }
        "WellboreFrameRepresentation"
        | "SeismicWellboreFrameRepresentation"
        | "WellboreMarkerFrameRepresentation"
            if matches!(
                element,
                Nodes | Intervals | Interval | IntervalEdges | IntervalsFromDatum
            ) =>
        {
            Ok(ElementAxes::Ordered(vec![super::element_count(
                representation,
                element,
            )?]))
        }
        "PolylineRepresentation" if matches!(element, Nodes | Intervals | Interval) => Ok(
            ElementAxes::Ordered(vec![super::element_count(representation, element)?]),
        ),
        _ => unordered(representation, element),
    }
}

fn unordered(representation: &dyn Reflect, element: IndexableElement) -> DomainResult<ElementAxes> {
    Ok(ElementAxes::Unordered(super::element_count(
        representation,
        element,
    )?))
}

impl PropertyShape {
    /// Build and check. `instants` is the number of `Time` entries of the
    /// series the property names; `realizations` the number of
    /// `RealizationIndices`. Each is read only when the property makes it a
    /// dimension, and is ignored otherwise.
    pub fn of(
        property: &Property<'_>,
        elements: ElementAxes,
        instants: Option<u64>,
        realizations: Option<u64>,
    ) -> Result<PropertyShape, ShapeRefusal> {
        let components = ElementShape::of_property(property)?;
        let time = if time_is_a_dimension(property.time.as_ref()) {
            let instants = instants.ok_or(ShapeRefusal::TimeNotGiven)?;
            let use_interval = property.time.as_ref().is_some_and(|t| t.use_interval);
            Some(TimeAxis::of(instants, use_interval))
        } else {
            None
        };
        // TRG 2.2 §7.1 p. 222: without `RealizationIndices` the realization
        // count "does not introduce a dimension".
        let realizations = if property.has_realization_indices {
            Some(realizations.ok_or(ShapeRefusal::RealizationsNotGiven)?)
        } else {
            None
        };
        Ok(PropertyShape {
            realizations,
            time,
            elements,
            components,
        })
    }

    /// The declared dimensions, slowest first; the component dimension is left
    /// out for a scalar (TRG 2.2 p. 221: "don't provide this dimension if not
    /// needed").
    pub fn declared_dims(&self) -> Vec<u64> {
        let mut dims = Vec::new();
        if let Some(realizations) = self.realizations {
            dims.push(realizations);
        }
        if let Some(time) = &self.time {
            dims.push(time.steps);
        }
        match &self.elements {
            ElementAxes::Ordered(axes) => dims.extend(axes.iter().copied()),
            ElementAxes::Unordered(count) => dims.push(*count),
        }
        if !self.components.is_scalar() {
            dims.extend(self.components.dims.iter().map(|d| *d as u64));
        }
        dims
    }

    /// Values the document implies, all dimensions together.
    pub fn value_count(&self) -> u64 {
        self.declared_dims()
            .iter()
            .fold(1u64, |acc, d| acc.saturating_mul(*d))
    }

    /// `count_fit` on the whole shape: elements x time x realizations, against
    /// the components of one element.
    pub fn count_fit(&self, actual: u64) -> CountFit {
        let slabs = self
            .realizations
            .unwrap_or(1)
            .saturating_mul(self.time.map_or(1, |t| t.steps));
        let element_count = slabs.saturating_mul(self.elements.count());
        count_fit(element_count, self.components.len() as u64, actual)
    }

    /// Confront the stored dimensions with the declared ones.
    ///
    /// Aggregating consecutive dimensions leaves the 1D order unchanged (Usage
    /// Guide 2.2 §4.2.2 p. 39), so a store may regroup the declared dimensions
    /// (`[n, 81, 81]` or `[n x 6561]`) and nothing else: each stored dimension
    /// must be the product of a run of consecutive declared ones.
    pub fn layout(&self, stored: &[u64]) -> Result<StoredLayout, ShapeRefusal> {
        let declared = self.declared_dims();
        let mismatch = || ShapeRefusal::StoredMismatch {
            stored: stored.to_vec(),
            expected: declared.clone(),
        };
        if stored.is_empty() {
            return Err(mismatch());
        }
        // The time dimension is named before the general comparison, so the
        // cause is the one said.
        if let Some(time) = &self.time
            && stored.len() == declared.len()
        {
            let at = usize::from(self.realizations.is_some());
            if stored[at] != time.steps {
                return Err(ShapeRefusal::TimeCountMismatch {
                    array: stored[at],
                    instants: time.instants,
                    use_interval: time.use_interval,
                });
            }
        }

        let mut groups: Vec<Range<usize>> = Vec::with_capacity(stored.len());
        let mut d = 0usize;
        for size in stored {
            let begin = d;
            if *size == 1 {
                // A stored 1 takes the declared 1s that follow, or none.
                while d < declared.len() && declared[d] == 1 {
                    d += 1;
                }
            } else {
                let mut product = 1u64;
                while d < declared.len() && product < *size {
                    product = product.saturating_mul(declared[d]);
                    d += 1;
                }
                if product != *size {
                    return Err(mismatch());
                }
            }
            groups.push(begin..d);
        }
        if d < declared.len() {
            // Declared 1s left after the last stored dimension join the last group.
            if declared[d..].iter().all(|x| *x == 1) {
                if let Some(last) = groups.last_mut() {
                    last.end = declared.len();
                }
            } else {
                return Err(mismatch());
            }
        }
        Ok(StoredLayout {
            stored: stored.to_vec(),
            groups,
        })
    }
}
