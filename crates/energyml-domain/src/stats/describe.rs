//! Describing an array of values, in steps.
//!
//! A [`Describer`] reads values slab by slab along the slowest dimension and
//! composes the rest of this module: counts and moments ([`Accumulator`]),
//! quantiles, a proposed [`Scale`], a histogram, its modes and threshold, the
//! variation along each axis, and a list of typed [`Observation`]s that say
//! what the numbers mean. It does no arithmetic of its own: it composes.
//!
//! It works **by steps**, on the model of `IndexBuilder::step`: the caller
//! chains `step` calls and may stop calling, which gives progress and
//! cancellation without a thread on the desktop and in a worker on the web. A
//! description stopped before the end is refused ([`DescribeRefusal::Cancelled`]),
//! never returned as if it were whole.
//!
//! What a step reads is a [`ValueChunks`], not a property: the adapter from a
//! property to a `ValueChunks` (component, time, facets) belongs to the layer
//! that knows properties. Progress counts slabs of `dims[0]`; on wasm32 a
//! `usize` bounds that to four billion slabs.

use super::accumulate::{Accumulator, Moments};
use super::axes::{AxesAccumulator, AxesRefusal, AxisVariation, Jump, STRONGEST_RATIO, strongest};
use super::binned::{BinRefusal, Binned, binned_exact, binned_from_sketch};
use super::modes::{
    BinInput, DEFAULT_BINS, FIXED, HISTOGRAM_PEAKS, MAX_BINS, Mode, StatMethods, Threshold, ThresholdRefusal,
    VALLEY,
};
use super::quantile::{EXACT_QUANTILE_LIMIT, QuantileMethod, QuantileMethodUsed, Quantiles, quantiles};
use super::scale::{AUTO_HIGH, AUTO_LOW, Scale, ScaleKind, ScaleRefusal, propose_log};
use super::sketch::{LogSketch, SKETCH_RELATIVE_ERROR};
use crate::package_index::Progress;

/// Axes are not measured when the dimensions add up to more positions than
/// this: the variation keeps a sum and a count per position of each axis.
pub const MAX_AXES_POSITIONS: u64 = 20_000_000;

/// Values read slab by slab along the slowest dimension.
pub trait ValueChunks {
    /// Row-major, slowest first; their product is the number of values.
    fn dims(&self) -> &[u64];

    /// The declared `NullValue` of an integer array, if any.
    fn null_value(&self) -> Option<i64> {
        None
    }

    /// Slabs `first .. first + count` of `dims[0]`, flattened:
    /// `count * product(dims[1..])` values.
    fn read(&self, first: u64, count: u64) -> crate::Result<Vec<f64>>;
}

/// Values already in memory.
pub struct SliceChunks<'a> {
    values: &'a [f64],
    dims: Vec<u64>,
    slab: usize,
    null_value: Option<i64>,
}

impl<'a> SliceChunks<'a> {
    /// `values` laid out row-major over `dims`, slowest first.
    pub fn new(values: &'a [f64], dims: Vec<u64>) -> Result<SliceChunks<'a>, DescribeRefusal> {
        let (total, slab) = sizes(&dims)?;
        if total != values.len() as u64 {
            return Err(DescribeRefusal::ValuesDoNotFill {
                values: values.len(),
                dims,
                expected: total,
            });
        }
        Ok(SliceChunks {
            values,
            dims,
            slab,
            null_value: None,
        })
    }

    /// Declare the `NullValue` of the values.
    pub fn with_null_value(mut self, null_value: Option<i64>) -> Self {
        self.null_value = null_value;
        self
    }
}

impl ValueChunks for SliceChunks<'_> {
    fn dims(&self) -> &[u64] {
        &self.dims
    }

    fn null_value(&self) -> Option<i64> {
        self.null_value
    }

    fn read(&self, first: u64, count: u64) -> crate::Result<Vec<f64>> {
        let end = first.checked_add(count).filter(|end| *end <= self.dims[0]);
        let Some(end) = end else {
            return Err(crate::Error::Malformed {
                at: "slab range".to_string(),
                reason: format!(
                    "slabs {first}..{first}+{count} are past the {} the array holds",
                    self.dims[0]
                ),
            });
        };
        Ok(self.values[first as usize * self.slab..end as usize * self.slab].to_vec())
    }
}

/// `(total values, values per slab)` of `dims`, or why they cannot be indexed.
fn sizes(dims: &[u64]) -> Result<(u64, usize), DescribeRefusal> {
    if dims.is_empty() {
        return Err(DescribeRefusal::NoDimension);
    }
    let too_large = || DescribeRefusal::TooLarge { dims: dims.to_vec() };
    let mut total = 1u64;
    for dim in dims {
        total = total.checked_mul(*dim).ok_or_else(too_large)?;
    }
    let mut slab = 1u64;
    for dim in &dims[1..] {
        slab = slab.checked_mul(*dim).ok_or_else(too_large)?;
    }
    usize::try_from(total).map_err(|_| too_large())?;
    let slab = usize::try_from(slab).map_err(|_| too_large())?;
    Ok((total, slab))
}

/// Whether the scale is chosen by the description or asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScaleChoice {
    /// Logarithmic when the positive values span three decades, else linear.
    Auto,
    /// Linear.
    Linear,
    /// Logarithmic; refused, and the linear scale kept, when no value is
    /// positive.
    Log10,
}

/// How the bounds of the scale are chosen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BoundsChoice {
    /// The first and ninety-ninth percentiles.
    Auto,
    /// The extremes.
    MinMax,
    /// Bounds the caller gives.
    Given {
        /// The lower bound.
        min: f64,
        /// The upper bound.
        max: f64,
    },
}

/// Which values of the array are read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sampling {
    /// Every value.
    Whole,
    /// One value in `step`, by flat index. The axes are not measured.
    Regular {
        /// The stride, at least 1.
        step: u64,
    },
    /// Slabs `first .. first + count` of the slowest dimension.
    Slowest {
        /// The first slab.
        first: u64,
        /// How many slabs, at least 1.
        count: u64,
    },
}

/// What to describe and how.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// The scale: `Auto`.
    pub scale: ScaleChoice,
    /// The bounds: `Auto`, the first and ninety-ninth percentiles.
    pub bounds: BoundsChoice,
    /// The id of the bin method: `fixed`.
    pub bin_method: String,
    /// The bars asked for: [`DEFAULT_BINS`].
    pub bins: usize,
    /// The probabilities of the quantiles: 0.01, 0.1, 0.5, 0.9, 0.99.
    pub quantiles: Vec<f64>,
    /// How the quantiles are computed: `Auto`.
    pub quantile_method: QuantileMethod,
    /// Which values are read: `Whole`.
    pub sampling: Sampling,
    /// The id of the mode method: `histogram_peaks`.
    pub mode_method: String,
    /// The id of the threshold method, if one is wanted: `valley`.
    pub threshold_method: Option<String>,
    /// Whether to measure the variation along the axes: `true`.
    pub axes: bool,
    /// What the file says of its own scaling, to be confronted with the values.
    pub file_scaling: Option<crate::color::Scaling>,
}

impl Default for Plan {
    fn default() -> Self {
        Plan {
            scale: ScaleChoice::Auto,
            bounds: BoundsChoice::Auto,
            bin_method: FIXED.to_string(),
            bins: DEFAULT_BINS,
            quantiles: vec![0.01, 0.1, 0.5, 0.9, 0.99],
            quantile_method: QuantileMethod::Auto,
            sampling: Sampling::Whole,
            mode_method: HISTOGRAM_PEAKS.to_string(),
            threshold_method: Some(VALLEY.to_string()),
            axes: true,
            file_scaling: None,
        }
    }
}

/// How much of the array was visited.
#[derive(Clone, Debug, PartialEq)]
pub struct SamplingUsed {
    /// The sampling that was applied.
    pub sampling: Sampling,
    /// Values kept.
    pub visited: u64,
    /// Values the whole array holds.
    pub total: u64,
}

/// One method or algorithm a description used.
#[derive(Clone, Debug, PartialEq)]
pub struct MethodUsed {
    /// `bins`, `quantiles`, `modes`, `threshold` or `axes`.
    pub family: &'static str,
    /// The id used.
    pub id: String,
    /// The number of bars, or the relative error of a sketch.
    pub value: Option<f64>,
}

/// A fact about the values, typed so that a face can say it in its own words.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Observation {
    /// The positive values span at least three decades: a log scale is proposed.
    SpansDecades {
        /// How many decades.
        decades: f64,
        /// The scale proposed.
        proposed: ScaleKind,
    },
    /// The file asks for a linear mapping of values that span many decades.
    FileSaysLinear {
        /// How many decades.
        decades: f64,
    },
    /// Values outside the `MinMax` the file declares.
    OutsideFileMinMax {
        /// How many.
        count: u64,
        /// Their share of the finite values.
        share: f64,
    },
    /// `NaN` and infinities.
    NotANumber {
        /// How many.
        count: u64,
        /// Their share of the values visited.
        share: f64,
    },
    /// Values equal to the declared `NullValue`.
    NullValues {
        /// How many.
        count: u64,
        /// Their share of the values visited.
        share: f64,
    },
    /// Values at or below zero, which a logarithmic scale cannot place.
    NonPositiveInLog {
        /// How many.
        count: u64,
    },
    /// A logarithmic scale was refused; the linear one is kept.
    LogRefused(ScaleRefusal),
    /// The bin method is undefined for these values; `fixed` was used instead.
    BinMethodUndefined {
        /// The id of the method.
        method: String,
    },
    /// The histogram has several modes.
    Modes {
        /// How many.
        count: usize,
    },
    /// A threshold between two populations.
    Threshold(Threshold),
    /// No threshold, and why.
    NoThreshold(ThresholdRefusal),
    /// The array does not vary along this axis.
    ConstantAlong {
        /// The rank of the axis in `dims`.
        axis: usize,
    },
    /// A seam along an axis.
    Jump(Jump),
    /// One axis varies much more than the others.
    Strongest {
        /// The rank of the axis in `dims`.
        axis: usize,
        /// By how much more than the mean of the others.
        ratio: f64,
    },
    /// Only a sample of the array was visited.
    Sampled {
        /// Values visited.
        visited: u64,
        /// Values the array holds.
        total: u64,
    },
    /// The quantiles and the histogram come from a sketch.
    Sketched {
        /// The relative error it guarantees.
        relative_error: f64,
    },
}

/// What a description found.
#[derive(Clone, Debug, PartialEq)]
pub struct Description {
    /// The plan that was followed.
    pub plan: Plan,
    /// The dimensions of the array.
    pub dims: Vec<u64>,
    /// How much of it was visited.
    pub sampling: SamplingUsed,
    /// The counts of the values visited.
    pub accumulator: Accumulator,
    /// The moments of the finite values.
    pub moments: Option<Moments>,
    /// The scale; `None` when no scale could be made.
    pub scale: Option<Scale>,
    /// The quantiles.
    pub quantiles: Option<Quantiles>,
    /// The histogram over the scale.
    pub binned: Option<Binned>,
    /// The peaks of the histogram.
    pub modes: Vec<Mode>,
    /// The threshold between two populations.
    pub threshold: Option<Threshold>,
    /// The variation along each axis; empty when not measured.
    pub axes: Vec<AxisVariation>,
    /// The axis that varies most, and by how much more than the others.
    pub strongest: Option<(usize, f64)>,
    /// What the values say, typed.
    pub observations: Vec<Observation>,
    /// The methods used.
    pub methods: Vec<MethodUsed>,
}

/// Why a description was refused.
#[derive(Debug, thiserror::Error)]
pub enum DescribeRefusal {
    /// An array of no dimension.
    #[error("an array of no dimension")]
    NoDimension,
    /// The dimensions hold more values than this build can index.
    #[error("the dimensions {dims:?} hold more values than this build can index")]
    TooLarge {
        /// The dimensions.
        dims: Vec<u64>,
    },
    /// The values do not fill the dimensions.
    #[error("{values} value(s) where the dimensions {dims:?} say {expected}")]
    ValuesDoNotFill {
        /// How many values were given.
        values: usize,
        /// The dimensions.
        dims: Vec<u64>,
        /// How many they hold.
        expected: u64,
    },
    /// A number of bars outside `1..=MAX_BINS`.
    #[error("{bins} bins asked for: between 1 and {limit}")]
    BinsOutOfRange {
        /// The number of bars.
        bins: usize,
        /// The most this build draws.
        limit: usize,
    },
    /// A quantile outside `[0, 1]`.
    #[error("{at} is not a probability between 0 and 1")]
    QuantileOutOfRange {
        /// The probability.
        at: f64,
    },
    /// A method id the registry does not hold.
    #[error("no {family} method `{id}` in this registry")]
    UnknownMethod {
        /// `bins`, `modes` or `threshold`.
        family: &'static str,
        /// The id asked.
        id: String,
    },
    /// A sampling that does not fit the array.
    #[error("sampling {sampling:?} does not fit an array of {slabs} slab(s)")]
    SamplingOutOfRange {
        /// The sampling asked.
        sampling: Sampling,
        /// How many slabs the array holds.
        slabs: u64,
    },
    /// The source changed shape between steps.
    #[error("the source now says {found:?} where it said {expected:?}")]
    SourceChanged {
        /// The dimensions at the start.
        expected: Vec<u64>,
        /// The dimensions now.
        found: Vec<u64>,
    },
    /// The source returned fewer or more values than asked.
    #[error("{count} slab(s) from slab {first} gave {got} value(s), not {expected}")]
    ShortRead {
        /// The first slab asked.
        first: u64,
        /// How many slabs were asked.
        count: u64,
        /// How many values that makes.
        expected: u64,
        /// How many came.
        got: u64,
    },
    /// The source could not be read.
    #[error(transparent)]
    Unreadable(#[from] crate::Error),
    /// `finish` was called before the reading was over.
    #[error("the description was stopped at {done:.1} % of its reading: nothing partial is returned")]
    Cancelled {
        /// How far the reading had got, in percent.
        done: f64,
    },
}

/// A description in progress.
pub struct Describer {
    plan: Plan,
    methods: StatMethods,
    dims: Vec<u64>,
    null_value: Option<i64>,
    total: u64,
    slab: usize,
    start: u64,
    end: u64,
    cursor: u64,
    visited: u64,
    accumulator: Accumulator,
    sketch: LogSketch,
    retained: Option<Vec<f64>>,
    axes: Option<AxesAccumulator>,
    outside_min_max: u64,
}

impl Describer {
    /// Check the plan against the registry and the shape of `source`, without
    /// reading a value.
    ///
    /// Refused: a plan with a bad number of bars, a quantile outside `[0, 1]`,
    /// an unknown method id, or a sampling that does not fit; a source with no
    /// dimension or too many values to index.
    pub fn new(
        source: &dyn ValueChunks,
        plan: Plan,
        methods: StatMethods,
    ) -> Result<Describer, DescribeRefusal> {
        let dims = source.dims().to_vec();
        let (total, slab) = sizes(&dims)?;
        if plan.bins == 0 || plan.bins > MAX_BINS {
            return Err(DescribeRefusal::BinsOutOfRange {
                bins: plan.bins,
                limit: MAX_BINS,
            });
        }
        if let Some(bad) = plan
            .quantiles
            .iter()
            .find(|q| !q.is_finite() || **q < 0.0 || **q > 1.0)
        {
            return Err(DescribeRefusal::QuantileOutOfRange { at: *bad });
        }
        let unknown = |family: &'static str, id: &str| DescribeRefusal::UnknownMethod {
            family,
            id: id.to_string(),
        };
        if methods.bin_method(&plan.bin_method).is_none() {
            return Err(unknown("bins", &plan.bin_method));
        }
        if methods.mode_method(&plan.mode_method).is_none() {
            return Err(unknown("modes", &plan.mode_method));
        }
        if let Some(id) = &plan.threshold_method
            && methods.threshold_method(id).is_none()
        {
            return Err(unknown("threshold", id));
        }
        let slabs = dims[0];
        let out_of_range = DescribeRefusal::SamplingOutOfRange {
            sampling: plan.sampling,
            slabs,
        };
        let (start, end, expected) = match plan.sampling {
            Sampling::Whole => (0, slabs, total),
            Sampling::Regular { step } => {
                if step == 0 {
                    return Err(out_of_range);
                }
                (0, slabs, total.div_ceil(step))
            }
            Sampling::Slowest { first, count } => match first.checked_add(count) {
                Some(end) if count > 0 && end <= slabs => (first, end, count * slab as u64),
                _ => return Err(out_of_range),
            },
        };
        // Keep the values for exact quantiles and an exact histogram when they
        // fit (S8), or when the caller asked for exact quantiles whatever the
        // size; otherwise only the sketch.
        let retain = expected <= EXACT_QUANTILE_LIMIT || plan.quantile_method == QuantileMethod::Exact;
        let kind = if plan.scale == ScaleChoice::Log10 {
            ScaleKind::Log10
        } else {
            ScaleKind::Linear
        };
        let positions: u64 = dims.iter().sum();
        let axes = if plan.axes
            && !matches!(plan.sampling, Sampling::Regular { .. })
            && positions <= MAX_AXES_POSITIONS
        {
            let mut axes_dims = dims.clone();
            axes_dims[0] = end - start;
            match AxesAccumulator::new(&axes_dims, kind) {
                Ok(axes) => Some(axes),
                Err(AxesRefusal::TooLarge { dims }) => return Err(DescribeRefusal::TooLarge { dims }),
                Err(_) => None,
            }
        } else {
            None
        };
        let null_value = source.null_value();
        Ok(Describer {
            plan,
            methods,
            dims,
            null_value,
            total,
            slab,
            start,
            end,
            cursor: start,
            visited: 0,
            accumulator: Accumulator::with_null_value(null_value),
            sketch: LogSketch::new(SKETCH_RELATIVE_ERROR).map_err(|_| DescribeRefusal::NoDimension)?,
            retained: if retain { Some(Vec::new()) } else { None },
            axes,
            outside_min_max: 0,
        })
    }

    /// How many slabs of `dims[0]` have been read, out of how many.
    pub fn progress(&self) -> Progress {
        Progress {
            done: (self.cursor - self.start) as usize,
            total: (self.end - self.start) as usize,
        }
    }

    /// Read up to `slabs` more slabs of the slowest dimension. `slabs == 0`
    /// reads nothing.
    pub fn step(&mut self, source: &dyn ValueChunks, slabs: usize) -> Result<Progress, DescribeRefusal> {
        if source.dims() != self.dims.as_slice() {
            return Err(DescribeRefusal::SourceChanged {
                expected: self.dims.clone(),
                found: source.dims().to_vec(),
            });
        }
        let remaining = self.end - self.cursor;
        let count = (slabs as u64).min(remaining);
        if count == 0 {
            return Ok(self.progress());
        }
        let values = source.read(self.cursor, count)?;
        let expected = count * self.slab as u64;
        if values.len() as u64 != expected {
            return Err(DescribeRefusal::ShortRead {
                first: self.cursor,
                count,
                expected,
                got: values.len() as u64,
            });
        }
        let file_range = self.plan.file_scaling.and_then(|s| s.min_max);
        let base = self.cursor * self.slab as u64;
        for (offset, value) in values.iter().enumerate() {
            if let Sampling::Regular { step } = self.plan.sampling
                && !(base + offset as u64).is_multiple_of(step)
            {
                continue;
            }
            self.visited += 1;
            self.accumulator.push(*value);
            let is_null = self.null_value.is_some_and(|n| *value == n as f64);
            if is_null || !value.is_finite() {
                continue;
            }
            self.sketch.push(*value);
            if let Some(retained) = &mut self.retained {
                retained.push(*value);
            }
            if let Some((lo, hi)) = file_range
                && (*value < lo || *value > hi)
            {
                self.outside_min_max += 1;
            }
        }
        if let Some(axes) = &mut self.axes {
            // The axes see whole slabs, the null value as a hole.
            let seen: Vec<f64> = values
                .iter()
                .map(|v| {
                    if self.null_value.is_some_and(|n| *v == n as f64) {
                        f64::NAN
                    } else {
                        *v
                    }
                })
                .collect();
            axes.push_slabs(self.cursor - self.start, &seen)
                .map_err(|_| DescribeRefusal::ShortRead {
                    first: self.cursor,
                    count,
                    expected,
                    got: values.len() as u64,
                })?;
        }
        self.cursor += count;
        Ok(self.progress())
    }

    /// The description. Refused if the reading is not over.
    pub fn finish(self) -> Result<Description, DescribeRefusal> {
        if self.cursor < self.end {
            let done = 100.0 * (self.cursor - self.start) as f64 / (self.end - self.start) as f64;
            return Err(DescribeRefusal::Cancelled { done });
        }
        let Describer {
            plan,
            methods,
            dims,
            total,
            visited,
            accumulator,
            sketch,
            retained,
            axes,
            outside_min_max,
            ..
        } = self;
        let mut observations: Vec<Observation> = Vec::new();
        let mut used: Vec<MethodUsed> = Vec::new();

        let moments = accumulator.moments();
        let decades = accumulator.decades();

        // The scale.
        let mut kind = match plan.scale {
            ScaleChoice::Linear => ScaleKind::Linear,
            ScaleChoice::Log10 => ScaleKind::Log10,
            ScaleChoice::Auto => {
                if propose_log(decades) {
                    ScaleKind::Log10
                } else {
                    ScaleKind::Linear
                }
            }
        };
        if kind == ScaleKind::Log10 && accumulator.positive_moments().is_none() {
            observations.push(Observation::LogRefused(ScaleRefusal::NoValues {
                kind: ScaleKind::Log10,
            }));
            kind = ScaleKind::Linear;
        }
        let bounds: Result<(f64, f64), ScaleRefusal> = match plan.bounds {
            BoundsChoice::Given { min, max } => Ok((min, max)),
            BoundsChoice::MinMax => {
                let extremes = match kind {
                    ScaleKind::Linear => moments.map(|m| (m.min, m.max)),
                    ScaleKind::Log10 => accumulator.positive_moments().map(|m| (m.min, m.max)),
                };
                extremes.ok_or(ScaleRefusal::NoValues { kind })
            }
            BoundsChoice::Auto => match &retained {
                Some(values) => super::scale::auto_bounds(values, kind, QuantileMethod::Exact),
                None => {
                    let (low, high) = match kind {
                        ScaleKind::Linear => (sketch.quantile(AUTO_LOW), sketch.quantile(AUTO_HIGH)),
                        ScaleKind::Log10 => (
                            sketch.positive_quantile(AUTO_LOW),
                            sketch.positive_quantile(AUTO_HIGH),
                        ),
                    };
                    match (low, high) {
                        (Some(low), Some(high)) => Ok((low, high)),
                        _ => Err(ScaleRefusal::NoValues { kind }),
                    }
                }
            },
        };
        let scale = match bounds.and_then(|(min, max)| Scale::new(kind, min, max, false)) {
            Ok(scale) => Some(scale),
            Err(refusal) => {
                // A log refusal is said; a constant has no range to divide and
                // simply has no histogram.
                if kind == ScaleKind::Log10 {
                    observations.push(Observation::LogRefused(refusal));
                }
                None
            }
        };

        // The quantiles.
        let at = plan.quantiles.clone();
        let (quantiles_found, quantile_used) = if accumulator.finite() == 0 || at.is_empty() {
            (None, None)
        } else {
            let found = match &retained {
                Some(values) => quantiles(values, &at, plan.quantile_method).ok(),
                None => {
                    let values: Vec<f64> = at
                        .iter()
                        .map(|q| sketch.quantile(*q).unwrap_or(f64::NAN))
                        .collect();
                    Some(Quantiles {
                        at: at.clone(),
                        values,
                        finite: accumulator.finite(),
                        set_aside: 0,
                        method: QuantileMethodUsed::Sketch {
                            relative_error: sketch.relative_error(),
                        },
                    })
                }
            };
            let found = found.map(|mut q| {
                // What was left out is what the accumulator counted apart.
                q.set_aside = accumulator.non_finite();
                q
            });
            let used = found.as_ref().map(|q| q.method);
            (found, used)
        };
        if let Some(method) = quantile_used {
            match method {
                QuantileMethodUsed::Exact => used.push(MethodUsed {
                    family: "quantiles",
                    id: "exact".to_string(),
                    value: None,
                }),
                QuantileMethodUsed::Sketch { relative_error } => {
                    observations.push(Observation::Sketched { relative_error });
                    used.push(MethodUsed {
                        family: "quantiles",
                        id: "sketch".to_string(),
                        value: Some(relative_error),
                    });
                }
            }
        }

        // The histogram, its modes and its threshold.
        let mut binned: Option<Binned> = None;
        let mut mode_list: Vec<Mode> = Vec::new();
        let mut threshold: Option<Threshold> = None;
        if let Some(scale) = &scale {
            let t = |v: f64| scale.transform(v).ok();
            let (q25, q75) = match &retained {
                Some(values) => match quantiles(values, &[0.25, 0.75], QuantileMethod::Exact) {
                    Ok(q) => (Some(q.values[0]), Some(q.values[1])),
                    Err(_) => (None, None),
                },
                None => match kind {
                    ScaleKind::Linear => (sketch.quantile(0.25), sketch.quantile(0.75)),
                    ScaleKind::Log10 => (sketch.positive_quantile(0.25), sketch.positive_quantile(0.75)),
                },
            };
            let iqr = match (q25.and_then(t), q75.and_then(t)) {
                (Some(a), Some(b)) => Some(b - a),
                _ => None,
            };
            let span = match (t(scale.max()), t(scale.min())) {
                (Some(hi), Some(lo)) => hi - lo,
                _ => 0.0,
            };
            let input = BinInput {
                requested: plan.bins,
                count: accumulator.finite(),
                iqr,
                span,
            };
            // Validated in `new`, so the method is there.
            let method = methods.bin_method(&plan.bin_method);
            let (mut bins, mut method_id) = match method {
                Some(m) => (m.bins(&input), m.id().to_string()),
                None => (None, plan.bin_method.clone()),
            };
            if bins.is_none() {
                observations.push(Observation::BinMethodUndefined {
                    method: plan.bin_method.clone(),
                });
                bins = Some(plan.bins.clamp(1, MAX_BINS));
                method_id = FIXED.to_string();
            }
            let bins = bins.unwrap_or(plan.bins);
            let counted = match &retained {
                Some(values) => binned_exact(values, scale, bins),
                None => binned_from_sketch(&sketch, scale, bins),
            };
            let counted = counted.map_err(|refusal| match refusal {
                BinRefusal::NoBin => DescribeRefusal::BinsOutOfRange {
                    bins: 0,
                    limit: MAX_BINS,
                },
                BinRefusal::TooManyBins { bins, limit } => DescribeRefusal::BinsOutOfRange { bins, limit },
            })?;
            used.push(MethodUsed {
                family: "bins",
                id: method_id,
                value: Some(bins as f64),
            });

            if let Some(mode_method) = methods.mode_method(&plan.mode_method) {
                mode_list = mode_method.modes(&counted);
                used.push(MethodUsed {
                    family: "modes",
                    id: mode_method.id().to_string(),
                    value: None,
                });
            }
            if mode_list.len() >= 2 {
                observations.push(Observation::Modes {
                    count: mode_list.len(),
                });
            }
            if let Some(id) = &plan.threshold_method
                && let Some(threshold_method) = methods.threshold_method(id)
            {
                match threshold_method.threshold(&counted, &mode_list) {
                    Ok(found) => {
                        observations.push(Observation::Threshold(found));
                        threshold = Some(found);
                    }
                    Err(refusal) => observations.push(Observation::NoThreshold(refusal)),
                }
                used.push(MethodUsed {
                    family: "threshold",
                    id: threshold_method.id().to_string(),
                    value: None,
                });
            }
            binned = Some(counted);
        }

        // The axes.
        let mut axis_list: Vec<AxisVariation> = Vec::new();
        let mut strongest_axis = None;
        match axes {
            Some(axes) => {
                axis_list = axes.finish();
                for axis in &axis_list {
                    if axis.constant {
                        observations.push(Observation::ConstantAlong { axis: axis.axis });
                    }
                    for jump in &axis.jumps {
                        observations.push(Observation::Jump(*jump));
                    }
                }
                strongest_axis = strongest(&axis_list, STRONGEST_RATIO);
                if let Some((axis, ratio)) = strongest_axis {
                    observations.push(Observation::Strongest { axis, ratio });
                }
                used.push(MethodUsed {
                    family: "axes",
                    id: "along".to_string(),
                    value: None,
                });
            }
            None => used.push(MethodUsed {
                family: "axes",
                id: "skipped".to_string(),
                value: None,
            }),
        }

        // The counts.
        let share = |count: u64| {
            if visited > 0 {
                count as f64 / visited as f64
            } else {
                0.0
            }
        };
        if accumulator.non_finite() > 0 {
            observations.push(Observation::NotANumber {
                count: accumulator.non_finite(),
                share: share(accumulator.non_finite()),
            });
        }
        if accumulator.null() > 0 {
            observations.push(Observation::NullValues {
                count: accumulator.null(),
                share: share(accumulator.null()),
            });
        }
        if propose_log(decades)
            && let Some(decades) = decades
        {
            observations.push(Observation::SpansDecades {
                decades,
                proposed: ScaleKind::Log10,
            });
            if plan.file_scaling.is_some_and(|s| !s.logarithmic) {
                observations.push(Observation::FileSaysLinear { decades });
            }
        }
        if outside_min_max > 0 {
            observations.push(Observation::OutsideFileMinMax {
                count: outside_min_max,
                share: outside_min_max as f64 / accumulator.finite().max(1) as f64,
            });
        }
        if kind == ScaleKind::Log10 && accumulator.non_positive() > 0 {
            observations.push(Observation::NonPositiveInLog {
                count: accumulator.non_positive(),
            });
        }
        if plan.sampling != Sampling::Whole {
            observations.push(Observation::Sampled { visited, total });
        }

        Ok(Description {
            sampling: SamplingUsed {
                sampling: plan.sampling,
                visited,
                total,
            },
            plan,
            dims,
            accumulator,
            moments,
            scale,
            quantiles: quantiles_found,
            binned,
            modes: mode_list,
            threshold,
            axes: axis_list,
            strongest: strongest_axis,
            observations,
            methods: used,
        })
    }
}

/// The whole description in one call, for values in memory.
pub fn describe(values: &[f64], dims: &[u64], plan: Plan) -> Result<Description, DescribeRefusal> {
    let source = SliceChunks::new(values, dims.to_vec())?;
    let mut describer = Describer::new(&source, plan, StatMethods::builtin())?;
    describer.step(&source, usize::MAX)?;
    describer.finish()
}
