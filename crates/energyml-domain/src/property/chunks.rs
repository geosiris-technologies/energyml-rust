//! A property read slab by slab, as `stats::describe` reads values.
//!
//! The question this module answers: how does one describe the values of a
//! property (its bounds, its scale, its variation along each axis) without
//! loading the whole array, for one component, one time step and one
//! realization of it? [`PropertyChunks`] is the adapter from a property to the
//! `ValueChunks` that `stats::describe::Describer` reads; [`PairWalk`] reads a
//! property and its sister by the same slab into a `PairAccumulator`.
//!
//! The slowest free dimension is time when every step is kept, the first
//! dimension of the elements otherwise (RESQML TRG 2.2 §7 p. 221; Usage Guide
//! 2.2 §6.6.3 p. 65). It refuses rather than guesses: a categorical, boolean
//! or text property, several patches, a realization not chosen, a time step
//! past the series, a pick that does not fit the shape. Nothing here does
//! statistics: the adapter reads, `stats` describes.

use std::cell::Cell;

use energyml_core::value::Reflect;

use super::Property;
use super::shape::{
    ComponentPick, ComponentRefusal, CountFit, ElementAxes, ElementShape, NarrowedPick, PropertyShape,
    ShapeRefusal, StoredLayout,
};
use crate::error::Error;
use crate::package_index::Progress;
use crate::source::ArraySource;
use crate::stats::axes::{STRONGEST_RATIO, strongest};
use crate::stats::cells::CellRule;
use crate::stats::describe::{Description, Observation, ValueChunks};
use crate::stats::pairs::{PairAccumulator, PairRefusal, PairSummary};
use crate::stats::scale::Scale;
use crate::values::{self, Family};

/// Which time steps to keep.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TimeChoice {
    /// Every step: time stays the slowest dimension.
    #[default]
    All,
    /// One step, by its index in the array's time dimension.
    Step(u64),
}

/// What to read of a property.
#[derive(Clone, PartialEq, Debug)]
pub struct ChunkSelection {
    /// Which components to keep (`ComponentPick::All` by default).
    pub pick: ComponentPick,
    /// Which time steps to keep (all by default).
    pub time: TimeChoice,
    /// The realization to read, when the values have a realization dimension.
    pub realization: Option<u64>,
}

impl Default for ChunkSelection {
    fn default() -> Self {
        ChunkSelection {
            pick: ComponentPick::All,
            time: TimeChoice::All,
            realization: None,
        }
    }
}

/// What one dimension of the values read stands for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AxisId {
    /// The time dimension.
    Time,
    /// Dimension `dim` of the elements; `ordered: false` for elements with no
    /// "along" (`ElementAxes::Unordered`): no variation is measured across them.
    Element {
        /// The rank among the dimensions of the elements.
        dim: usize,
        /// Whether the guide states an order for these elements.
        ordered: bool,
    },
    /// Dimension `dim` of the components (`ElementShape`).
    Component {
        /// The rank among the dimensions of the components.
        dim: usize,
    },
}

/// Why a property cannot be read slab by slab.
#[derive(Debug, thiserror::Error)]
pub enum ChunksRefusal {
    /// More than one patch (or none).
    #[error("{count} patch(es): a property is described one patch at a time, and several are not read yet")]
    Patches {
        /// How many patches the property has.
        count: usize,
    },
    /// Booleans and text are not described as numbers.
    #[error("a property of {family:?} values: only numbers are described")]
    NotNumeric {
        /// The family of the array.
        family: Family,
    },
    /// The property names a lookup table: its values are codes.
    #[error(
        "the property names the lookup table `{lookup}`: its values are codes, described by their categories (`stats::Summary`)"
    )]
    Categorical {
        /// The lookup table, by uri when known, by uuid otherwise.
        lookup: String,
    },
    /// Several values per element declared, one held, and a pick other than `All`.
    #[error("declares {declared} values per element and holds one: only `ComponentPick::All` reads it, as scalars")]
    OnePerElement {
        /// The count the document declares.
        declared: u64,
    },
    /// The values have a realization dimension and none was chosen.
    #[error("the property has {count} realizations: choose one")]
    RealizationNotChosen {
        /// How many realizations the property has.
        count: u64,
    },
    /// A realization past the last one.
    #[error("realization {at} where the property has {count}")]
    RealizationOutOfRange {
        /// The realization asked.
        at: u64,
        /// How many the property has.
        count: u64,
    },
    /// A realization was chosen on values with no realization dimension.
    #[error("a realization was chosen, and the property's values have no realization dimension")]
    NoRealizationDimension,
    /// A time step past the last one.
    #[error("time step {step} where the array holds {steps}")]
    StepOutOfRange {
        /// The step asked.
        step: u64,
        /// How many steps the array holds.
        steps: u64,
    },
    /// A time step was chosen on values with no time dimension.
    #[error("a time step was chosen, and time is not a dimension of the property's values")]
    NoTimeDimension,
    /// The declared shape and the stored one disagree, or the shape cannot be built.
    #[error(transparent)]
    Shape(#[from] ShapeRefusal),
    /// The component pick does not fit the shape.
    #[error(transparent)]
    Pick(#[from] ComponentRefusal),
    /// The array could not be read.
    #[error(transparent)]
    Read(#[from] Error),
}

/// A property's values, slab by slab: what `stats::describe::Describer` reads.
///
/// Built by [`PropertyChunks::new`], which reads at most the header of the
/// array; each [`ValueChunks::read`] then reads only the block it needs
/// (`values::slab_block`, `values::read_block`).
pub struct PropertyChunks<'a> {
    at: String,
    array: &'a dyn Reflect,
    source: &'a dyn ArraySource,
    shape: PropertyShape,
    /// What the values are read as: `shape`, or the same with scalar components
    /// for a property that declares several values per element and holds one.
    read_shape: PropertyShape,
    layout: StoredLayout,
    stored: Vec<u64>,
    pick: ComponentPick,
    narrowed: Option<NarrowedPick>,
    reads_narrowed: bool,
    fixed: Vec<u64>,
    dims: Vec<u64>,
    axis_ids: Vec<AxisId>,
    read_as_scalars: Option<u64>,
    null_value: Option<i64>,
    nulls: Cell<u64>,
}

impl<'a> PropertyChunks<'a> {
    /// `uri` is the property's full uri (a `Property` carries its uuid only);
    /// `elements`, `instants`, `realizations` as [`PropertyShape::of`] takes
    /// them.
    ///
    /// Refused, by name and before any value is read: several patches, a
    /// lookup table (the values are codes), a boolean or text array, a
    /// realization not chosen or out of range, a time step out of range, a
    /// pick that does not fit the shape, a stored shape that is not a
    /// regrouping of the declared one.
    pub fn new(
        uri: &str,
        property: &Property<'a>,
        elements: ElementAxes,
        instants: Option<u64>,
        realizations: Option<u64>,
        selection: &ChunkSelection,
        source: &'a dyn ArraySource,
    ) -> Result<PropertyChunks<'a>, ChunksRefusal> {
        // 1. One patch.
        if property.patches.len() != 1 {
            return Err(ChunksRefusal::Patches {
                count: property.patches.len(),
            });
        }
        let patch = &property.patches[0];
        let array = patch.array;
        let at = format!("{uri} patch 0");

        // 2. Numbers, and not codes.
        if property.lookup.is_some() {
            return Err(ChunksRefusal::Categorical {
                lookup: property
                    .lookup_uri
                    .clone()
                    .or_else(|| property.lookup.clone())
                    .unwrap_or_default(),
            });
        }
        let family = values::family(array).ok_or_else(|| {
            Error::unsupported(format!("`{}` is not a value array", array.type_info().name), &at)
        })?;
        if matches!(family, Family::Boolean | Family::Text) {
            return Err(ChunksRefusal::NotNumeric { family });
        }

        // 3. The declared shape.
        let shape = PropertyShape::of(property, elements, instants, realizations)?;

        // 4. The selection, nothing read.
        match (shape.realizations, selection.realization) {
            (Some(count), None) => return Err(ChunksRefusal::RealizationNotChosen { count }),
            (Some(count), Some(index)) if index >= count => {
                return Err(ChunksRefusal::RealizationOutOfRange { at: index, count });
            }
            (None, Some(_)) => return Err(ChunksRefusal::NoRealizationDimension),
            _ => {}
        }
        match (shape.time, selection.time) {
            (Some(time), TimeChoice::Step(step)) if step >= time.steps => {
                return Err(ChunksRefusal::StepOutOfRange {
                    step,
                    steps: time.steps,
                });
            }
            (None, TimeChoice::Step(_)) => return Err(ChunksRefusal::NoTimeDimension),
            _ => {}
        }
        selection.pick.check(&shape.components)?;

        // 5. The stored shape, one header.
        let stored = values::stored_dims(array, source, &at)?;
        let mut read_shape = shape.clone();
        let mut read_as_scalars = None;
        let layout = match shape.layout(&stored) {
            Ok(layout) => layout,
            Err(mismatch @ ShapeRefusal::StoredMismatch { .. }) => {
                let held = stored.iter().fold(1u64, |total, dim| total.saturating_mul(*dim));
                match shape.count_fit(held) {
                    CountFit::OnePerElement { declared } => {
                        if !matches!(selection.pick, ComponentPick::All) {
                            return Err(ChunksRefusal::OnePerElement { declared });
                        }
                        read_shape.components = ElementShape::of(&[], &[])?;
                        read_as_scalars = Some(declared);
                        read_shape.layout(&stored)?
                    }
                    _ => return Err(ChunksRefusal::Shape(mismatch)),
                }
            }
            Err(other) => return Err(ChunksRefusal::Shape(other)),
        };

        // 6. The slowest dimensions fixed: a realization, then a time step.
        let mut fixed = Vec::new();
        if shape.realizations.is_some() {
            fixed.push(selection.realization.unwrap_or(0));
        }
        if let TimeChoice::Step(step) = selection.time {
            fixed.push(step);
        }

        // 7. The dimensions exposed, slowest first, and what each stands for.
        let mut dims: Vec<u64> = Vec::new();
        let mut axis_ids: Vec<AxisId> = Vec::new();
        if let Some(time) = shape.time
            && selection.time == TimeChoice::All
        {
            dims.push(time.steps);
            axis_ids.push(AxisId::Time);
        }
        match &shape.elements {
            ElementAxes::Ordered(counts) => {
                for (dim, count) in counts.iter().enumerate() {
                    dims.push(*count);
                    axis_ids.push(AxisId::Element { dim, ordered: true });
                }
            }
            ElementAxes::Unordered(count) => {
                dims.push(*count);
                axis_ids.push(AxisId::Element {
                    dim: 0,
                    ordered: false,
                });
            }
        }
        let component_dims = &read_shape.components.dims;
        match &selection.pick {
            ComponentPick::All => {
                if !read_shape.components.is_scalar() {
                    for (dim, count) in component_dims.iter().enumerate() {
                        dims.push(*count as u64);
                        axis_ids.push(AxisId::Component { dim });
                    }
                }
            }
            ComponentPick::Along { axis, .. } | ComponentPick::AlongReduced { axis, .. } => {
                dims.push(component_dims[*axis] as u64);
                axis_ids.push(AxisId::Component { dim: *axis });
            }
            _ => {}
        }

        // 8. The components a slab reads, and the NullValue.
        let narrowed = selection.pick.narrowed(&read_shape.components)?;
        let null_value = values::null_value(array);
        // Whether a slab reads only the narrowed block is `slab_block`'s to say
        // (the one block arithmetic); asking it for the first slab reads no value
        // and checks the fixed indices.
        let reads_narrowed = values::slab_block(&read_shape, &layout, &fixed, 0, 1, narrowed.as_ref())?.narrowed;

        Ok(PropertyChunks {
            at,
            array,
            source,
            shape,
            read_shape,
            layout,
            stored,
            pick: selection.pick.clone(),
            narrowed,
            reads_narrowed,
            fixed,
            dims,
            axis_ids,
            read_as_scalars,
            null_value,
            nulls: Cell::new(0),
        })
    }

    /// The declared shape (never the scalar reading of `OnePerElement`).
    pub fn shape(&self) -> &PropertyShape {
        &self.shape
    }

    /// The stored dimensions, as `values::stored_dims` read them.
    pub fn stored(&self) -> &[u64] {
        &self.stored
    }

    /// What each dimension of `dims()` stands for, slowest first.
    pub fn axis_ids(&self) -> &[AxisId] {
        &self.axis_ids
    }

    /// `Some(declared)`: several values per element declared, one held, read as scalars.
    pub fn read_as_scalars(&self) -> Option<u64> {
        self.read_as_scalars
    }

    /// Whether a slab reads only the components the pick keeps.
    ///
    /// `false` for a pick that keeps every component, and for a store that
    /// regroups the component dimensions: it is then read whole and the pick
    /// applied after.
    pub fn reads_narrowed(&self) -> bool {
        self.reads_narrowed
    }

    /// Values equal to the NullValue met by the reads so far, before a
    /// reduction replaced them (a slab read twice counts twice). 0 without a
    /// reduction.
    pub fn nulls_before_reduction(&self) -> u64 {
        self.nulls.get()
    }

    /// Whether the pick reduces components to one value: the values rendered
    /// are then reductions, and none of them is the NullValue.
    fn reduces(&self) -> bool {
        matches!(self.pick, ComponentPick::Reduce(_) | ComponentPick::AlongReduced { .. })
    }
}

impl ValueChunks for PropertyChunks<'_> {
    fn dims(&self) -> &[u64] {
        &self.dims
    }

    fn null_value(&self) -> Option<i64> {
        if self.reduces() { None } else { self.null_value }
    }

    fn read(&self, first: u64, count: u64) -> crate::Result<Vec<f64>> {
        let slab = values::slab_block(
            &self.read_shape,
            &self.layout,
            &self.fixed,
            first,
            count,
            self.narrowed.as_ref(),
        )?;
        let read = values::read_block(self.array, self.source, &self.at, &slab.block)?;
        let values = read.to_f64().ok_or_else(|| {
            Error::unsupported(format!("values of the family {:?} as numbers", read.family()), &self.at)
        })?;
        if matches!(self.pick, ComponentPick::All) {
            return Ok(values);
        }
        let (row_shape, row_pick) = match (&self.narrowed, slab.narrowed) {
            (Some(narrowed), true) => (&narrowed.shape, &narrowed.pick),
            _ => (&self.read_shape.components, &self.pick),
        };
        let width = row_shape.len();
        if width == 0 || values.len() % width != 0 {
            return Err(Error::malformed(
                &self.at,
                format!("{} value(s) do not make whole rows of {width}", values.len()),
            ));
        }
        let null = if self.reduces() {
            self.null_value.map(|v| v as f64)
        } else {
            None
        };
        let mut out = Vec::new();
        let mut row = Vec::with_capacity(width);
        for chunk in values.chunks(width) {
            row.clear();
            row.extend_from_slice(chunk);
            if let Some(null) = null {
                for value in row.iter_mut() {
                    if *value == null {
                        *value = f64::NAN;
                        self.nulls.set(self.nulls.get() + 1);
                    }
                }
            }
            let picked = row_pick
                .select(row_shape, &row)
                .map_err(|refusal| Error::malformed(&self.at, refusal.to_string()))?;
            out.extend(picked);
        }
        Ok(out)
    }
}

/// Drops from a description what it says of the axes `axis_ids` marks unordered
/// (`AxisId::Element { ordered: false, .. }`): their `AxisVariation`, the
/// `ConstantAlong`, `Jump` and `Strongest` observations about them, and the
/// strongest axis, recomputed by `stats::axes::strongest` on what is left.
///
/// Returns the axes dropped. When `axis_ids` does not have one entry per
/// dimension of the description, they are not the axes of this description:
/// nothing is changed and nothing is returned.
pub fn keep_ordered_axes(description: &mut Description, axis_ids: &[AxisId]) -> Vec<usize> {
    if axis_ids.len() != description.dims.len() {
        return Vec::new();
    }
    let drop: Vec<usize> = axis_ids
        .iter()
        .enumerate()
        .filter(|(_, id)| matches!(id, AxisId::Element { ordered: false, .. }))
        .map(|(rank, _)| rank)
        .collect();
    description.axes.retain(|axis| !drop.contains(&axis.axis));
    description.observations.retain(|observation| match observation {
        Observation::ConstantAlong { axis } => !drop.contains(axis),
        Observation::Jump(jump) => !drop.contains(&jump.axis),
        Observation::Strongest { .. } => false,
        _ => true,
    });
    description.strongest = strongest(&description.axes, STRONGEST_RATIO);
    if let Some((axis, ratio)) = description.strongest {
        description.observations.push(Observation::Strongest { axis, ratio });
    }
    drop
}

/// Why two properties could not be walked as a pair.
#[derive(Debug, thiserror::Error)]
pub enum PairWalkRefusal {
    /// The two properties do not read as the same shape.
    #[error(
        "the sibling reads as {other:?} where this property reads as {this:?}: a pair is never aligned silently"
    )]
    SiblingShape {
        /// The dimensions of this property.
        this: Vec<u64>,
        /// The dimensions of its sibling.
        other: Vec<u64>,
    },
    /// The two reads gave a different number of values.
    #[error("{count} slab(s) from slab {first} gave {x} and {y} value(s)")]
    ShortRead {
        /// The first slab asked.
        first: u64,
        /// How many slabs were asked.
        count: u64,
        /// Values of this property.
        x: usize,
        /// Values of its sibling.
        y: usize,
    },
    /// `finish` was called before the reading was over.
    #[error("the pair was stopped at {done:.1} % of its reading: nothing partial is returned")]
    Cancelled {
        /// How far the reading had got, in percent.
        done: f64,
    },
    /// The accumulator refused.
    #[error(transparent)]
    Pair(#[from] PairRefusal),
    /// A property could not be read.
    #[error(transparent)]
    Read(#[from] Error),
}

/// Two properties read by the same slab into a `PairAccumulator`: `x` this,
/// `y` the sibling.
///
/// The kinship of the two is **not** checked here: the caller chose them (by
/// `PackageIndex::siblings`, or by hand). What it counts is a figure, not a
/// judgement: that a share of ratios is a warning, and which sister is the
/// vertical one, is the rule `house/property/directional-ratio`.
pub struct PairWalk {
    accumulator: PairAccumulator,
    dims: Vec<u64>,
    cursor: u64,
    end: u64,
    x_null: Option<i64>,
    y_null: Option<i64>,
}

impl PairWalk {
    /// `x` and `y` the scales of each (`Description::scale` of each); `n` the
    /// density grid size; `rule` the caller's.
    ///
    /// Refused when the two properties do not read as the same dimensions.
    pub fn new(
        this: &PropertyChunks<'_>,
        sibling: &PropertyChunks<'_>,
        x: Scale,
        y: Scale,
        n: usize,
        rule: Option<CellRule>,
    ) -> Result<PairWalk, PairWalkRefusal> {
        if this.dims() != sibling.dims() {
            return Err(PairWalkRefusal::SiblingShape {
                this: this.dims().to_vec(),
                other: sibling.dims().to_vec(),
            });
        }
        let accumulator = PairAccumulator::new(x, y, n, rule)?;
        let dims = this.dims().to_vec();
        let end = dims[0];
        Ok(PairWalk {
            accumulator,
            dims,
            cursor: 0,
            end,
            x_null: ValueChunks::null_value(this),
            y_null: ValueChunks::null_value(sibling),
        })
    }

    /// How many slabs of `dims[0]` have been read, out of how many.
    pub fn progress(&self) -> Progress {
        Progress {
            done: self.cursor as usize,
            total: self.end as usize,
        }
    }

    /// Read up to `slabs` more slabs of each property, by the same slab.
    ///
    /// A value equal to the NullValue of its own property is not a value: the
    /// pair is dropped and counted.
    pub fn step(
        &mut self,
        this: &PropertyChunks<'_>,
        sibling: &PropertyChunks<'_>,
        slabs: usize,
    ) -> Result<Progress, PairWalkRefusal> {
        if this.dims() != self.dims.as_slice() || sibling.dims() != self.dims.as_slice() {
            return Err(PairWalkRefusal::SiblingShape {
                this: this.dims().to_vec(),
                other: sibling.dims().to_vec(),
            });
        }
        let count = (slabs as u64).min(self.end - self.cursor);
        if count == 0 {
            return Ok(self.progress());
        }
        let mut a = this.read(self.cursor, count)?;
        let mut b = sibling.read(self.cursor, count)?;
        if a.len() != b.len() {
            return Err(PairWalkRefusal::ShortRead {
                first: self.cursor,
                count,
                x: a.len(),
                y: b.len(),
            });
        }
        blank(&mut a, self.x_null);
        blank(&mut b, self.y_null);
        self.accumulator.push(&a, &b)?;
        self.cursor += count;
        Ok(self.progress())
    }

    /// The summary of the pairs. Refused if the reading is not over.
    pub fn finish(self) -> Result<PairSummary, PairWalkRefusal> {
        if self.cursor < self.end {
            let done = 100.0 * self.cursor as f64 / self.end as f64;
            return Err(PairWalkRefusal::Cancelled { done });
        }
        Ok(self.accumulator.finish())
    }
}

/// Turn the values equal to `null` into `NaN`: a hole, which a pair drops.
fn blank(values: &mut [f64], null: Option<i64>) {
    if let Some(null) = null {
        let null = null as f64;
        for value in values.iter_mut() {
            if *value == null {
                *value = f64::NAN;
            }
        }
    }
}
