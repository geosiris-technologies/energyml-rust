//! What a column of numbers is, before anybody draws it.
//!
//! # Why this is here and not in the front end that plots it
//!
//! A histogram is arithmetic with right and wrong answers, and every one of its
//! wrong answers *looks* like a chart. A bin edge off by one puts a hundred
//! thousand values in the neighbouring bar; counting `NaN` as a value drags the
//! mean towards nothing; treating a facies code as a continuous quantity draws
//! ten spikes with empty air between them and calls it a distribution. None of
//! that fails, and none of it is visible in the picture.
//!
//! So the arithmetic lives here, where a test holds it, and the caller draws
//! what it returns. It is also the layer where every reader can reach it: the
//! browser gets it through a WebAssembly front end, the CLI and the desktop viewer get
//! it for free, and none of the three has a copy.
//!
//! # The four decisions
//!
//! - **A value that is not a number is counted, not skipped.** `NaN` is how the
//!   corpus says *no data here* — 785 250 of ALWYN's 987 387 grid nodes — and
//!   the count of them is often the most useful number on the panel. It is kept
//!   apart from the total so that "how much of this array is real" is a
//!   subtraction anybody can check, and neither the mean nor the extremes see
//!   them.
//! - **A discrete array is categorical, not continuous.** A facies property
//!   holding 0…9 in a million cells is ten answers, not a range: binned over 30
//!   intervals it draws ten spikes separated by empty bars that mean nothing.
//!   Below [`CATEGORY_LIMIT`] distinct integers it comes back as counts per
//!   value; above it, as a histogram.
//! - **The extremes are the bin edges, and the last bin is closed.** A value
//!   equal to the maximum belongs to the last bin, not to a bin past the end —
//!   the classic off-by-one, and it silently drops exactly the values a person
//!   is looking for when they open a histogram.
//! - **A series is decimated here, not in the transport.** Plotting a million
//!   points means shipping a million numbers to draw two thousand pixels. The
//!   step is stated with the sample so a chart can label the axis with real
//!   indices instead of pretending the sample is the array.

use std::collections::BTreeMap;

use crate::values::{Family, Values};

/// Distinct integer values below which an array is summarised as categories
/// rather than binned.
///
/// Chosen for what it is used for: facies, region codes, zone indices and the
/// `DiscreteProperty` family generally hold a handful of values, and a legend
/// of more than about two dozen entries stops being readable anyway. Above it,
/// integers are a range like any other.
pub const CATEGORY_LIMIT: usize = 24;

/// Most values a sample carries, whatever the array's size.
pub const SAMPLE_LIMIT: usize = 2_000;

/// How many distinct strings are named before the rest become a remainder.
pub const TEXT_LIMIT: usize = 24;

/// One value of an array that holds a small number of them, with its count.
#[derive(Clone, PartialEq, Debug)]
pub struct Category {
    /// The value, as it should be shown: `3`, `true`, `Top Brent`.
    pub label: String,
    /// Its numeric reading, when it has one — what a chart puts on an axis.
    pub value: Option<f64>,
    /// How many times it occurs.
    pub count: u64,
}

/// The counts of an array's values across equal intervals.
#[derive(Clone, PartialEq, Debug)]
pub struct Histogram {
    /// Lower edge of the first bin — the array's minimum.
    pub from: f64,
    /// Upper edge of the last bin — the array's maximum, **included**.
    pub to: f64,
    /// One count per bin, low to high.
    pub bins: Vec<u64>,
}

impl Histogram {
    /// The width of one bin.
    pub fn width(&self) -> f64 {
        if self.bins.is_empty() {
            return 0.0;
        }
        (self.to - self.from) / self.bins.len() as f64
    }

    /// The half-open interval a bin covers — closed at the top for the last one.
    pub fn edges(&self, bin: usize) -> (f64, f64) {
        let width = self.width();
        (
            self.from + width * bin as f64,
            self.from + width * (bin + 1) as f64,
        )
    }
}

/// A run of values taken at regular intervals, for a chart.
#[derive(Clone, PartialEq, Debug)]
pub struct Sample {
    /// One value out of every `step`.
    pub step: usize,
    /// The values kept, in array order. `NaN` is kept as `NaN`: a gap in a log
    /// is part of what a series says, and dropping it would join the two sides
    /// of the gap with a straight line nobody measured.
    pub values: Vec<f64>,
}

/// What an array holds, said in numbers.
#[derive(Clone, PartialEq, Debug)]
pub struct Summary {
    /// Which family the values belong to.
    pub family: Family,
    /// How many values there are, `NaN` included.
    pub count: usize,
    /// How many of them are numbers — the population every statistic below is
    /// computed over.
    pub finite: usize,
    /// How many are not: `NaN`, `+INF`, `-INF`. The corpus's way of saying "no
    /// data here", and worth a line of its own.
    pub non_finite: usize,
    /// The smallest finite value, or `None` when there is none.
    pub min: Option<f64>,
    /// The largest finite value.
    pub max: Option<f64>,
    /// The mean of the finite values.
    pub mean: Option<f64>,
    /// Their population standard deviation.
    pub deviation: Option<f64>,
    /// Equal-interval counts, when the values are a range.
    ///
    /// `None` for text, for a discrete array small enough to be listed value by
    /// value, and for an array with no finite value at all — three cases where
    /// a histogram would be a picture of nothing.
    pub histogram: Option<Histogram>,
    /// Counts per value, when there are few enough of them to name.
    ///
    /// Booleans always; integers below [`CATEGORY_LIMIT`] distinct values;
    /// strings, with the rest folded into a remainder.
    pub categories: Vec<Category>,
    /// How many distinct values the array holds, when that was counted.
    ///
    /// Only for the families where it is cheap and meaningful — booleans,
    /// integers, strings. `None` for floating point, where distinctness is a
    /// question about bit patterns rather than about data.
    pub distinct: Option<usize>,
    /// Values not named in `categories`, when the list was cut short.
    pub other: u64,
    /// A decimated run of the values, for a chart against the index.
    pub sample: Option<Sample>,
}

impl Summary {
    /// Summarise values already in memory.
    ///
    /// `bins` is a request, not a promise: an array whose values are all equal
    /// gets one bin, because a range of zero has one interval and splitting it
    /// into thirty would draw twenty-nine empty bars beside a spike.
    pub fn of(values: &Values, bins: usize) -> Summary {
        match values {
            Values::Floating(v) => numeric(Family::Floating, v.iter().copied(), v.len(), bins, false),
            Values::Integer(v) => numeric(Family::Integer, v.iter().map(|n| *n as f64), v.len(), bins, true),
            Values::Boolean(v) => booleans(v),
            Values::Text(v) => strings(v),
        }
    }
}

/// Floating point and integers: the same arithmetic, one flag apart.
///
/// The flag is whether being *discrete* is worth acting on. An integer array
/// with a handful of distinct values is a set of categories — facies, regions,
/// zone indices — and binning it draws spikes with meaningless gaps between
/// them. A floating-point array is never treated that way, even when it happens
/// to hold three values: equality on doubles is a bit pattern, not a category.
fn numeric(
    family: Family,
    values: impl Iterator<Item = f64> + Clone,
    count: usize,
    bins: usize,
    discrete: bool,
) -> Summary {
    let mut finite = 0usize;
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    let mut sum = 0.0f64;
    let mut sum_of_squares = 0.0f64;
    // Bounded on purpose: an integer array of a million distinct values must
    // not build a map of a million entries to discover it is not categorical.
    let mut seen: BTreeMap<i64, u64> = BTreeMap::new();
    let mut too_many = !discrete;

    for value in values.clone() {
        if !value.is_finite() {
            continue;
        }
        finite += 1;
        min = min.min(value);
        max = max.max(value);
        sum += value;
        sum_of_squares += value * value;
        if !too_many {
            *seen.entry(value as i64).or_default() += 1;
            too_many = seen.len() > CATEGORY_LIMIT;
        }
    }

    let sample = sample_of(values.clone(), count);
    if finite == 0 {
        return Summary {
            family,
            count,
            finite: 0,
            non_finite: count,
            min: None,
            max: None,
            mean: None,
            deviation: None,
            histogram: None,
            categories: Vec::new(),
            distinct: if discrete { Some(0) } else { None },
            other: 0,
            sample,
        };
    }

    let mean = sum / finite as f64;
    // The population deviation, and `max(0.0)` because the two-pass identity is
    // not exact in floating point: a constant array can come out at -1e-16, and
    // the square root of that is `NaN` on a chart that says "0" everywhere else.
    let variance = (sum_of_squares / finite as f64 - mean * mean).max(0.0);

    let categorical = discrete && !too_many;
    let categories = if categorical {
        seen.iter()
            .map(|(value, count)| Category {
                label: value.to_string(),
                value: Some(*value as f64),
                count: *count,
            })
            .collect()
    } else {
        Vec::new()
    };

    Summary {
        family,
        count,
        finite,
        non_finite: count - finite,
        min: Some(min),
        max: Some(max),
        mean: Some(mean),
        deviation: Some(variance.sqrt()),
        histogram: if categorical {
            None
        } else {
            Some(histogram(values, min, max, bins))
        },
        categories,
        distinct: if categorical { Some(seen.len()) } else { None },
        other: 0,
        sample,
    }
}

/// The counts, with the maximum inside the last bin rather than past the end.
fn histogram(values: impl Iterator<Item = f64>, min: f64, max: f64, bins: usize) -> Histogram {
    // A range of zero is one interval. Thirty bins over it is a spike and
    // twenty-nine empty bars, which says less than the one bar does.
    let count = if max > min { bins.max(1) } else { 1 };
    let width = (max - min) / count as f64;
    let mut counts = vec![0u64; count];
    for value in values {
        if !value.is_finite() {
            continue;
        }
        let bin = if width > 0.0 {
            // `min` is not multiplied out: `(value - min) / width` keeps the
            // first bin exact, which matters when the range is tiny next to the
            // values themselves — depths around 3 000 m spanning two metres.
            ((value - min) / width) as usize
        } else {
            0
        };
        // The last bin is **closed**: a value equal to the maximum lands one
        // past the end otherwise, and the values a person opens a histogram to
        // find are exactly the extremes.
        counts[bin.min(count - 1)] += 1;
    }
    Histogram {
        from: min,
        to: max,
        bins: counts,
    }
}

fn booleans(values: &[bool]) -> Summary {
    let yes = values.iter().filter(|v| **v).count() as u64;
    let no = values.len() as u64 - yes;
    let mean = if values.is_empty() {
        None
    } else {
        Some(yes as f64 / values.len() as f64)
    };
    // 0 and 1, and only when there is something there: an empty array has no
    // extremes, and answering `0` for both would say every value is false.
    let (min, max) = match (values.is_empty(), no > 0, yes > 0) {
        (true, _, _) => (None, None),
        (_, true, true) => (Some(0.0), Some(1.0)),
        (_, true, false) => (Some(0.0), Some(0.0)),
        (_, false, _) => (Some(1.0), Some(1.0)),
    };
    Summary {
        family: Family::Boolean,
        count: values.len(),
        finite: values.len(),
        non_finite: 0,
        min,
        max,
        mean,
        deviation: mean.map(|p| (p * (1.0 - p)).sqrt()),
        histogram: None,
        // Both, always, and in this order — a run of ten thousand `true` says
        // something different from ten thousand values of which none is false,
        // and a legend that omits the empty half hides which of the two it is.
        categories: vec![
            Category {
                label: "false".into(),
                value: Some(0.0),
                count: no,
            },
            Category {
                label: "true".into(),
                value: Some(1.0),
                count: yes,
            },
        ],
        distinct: Some(usize::from(no > 0) + usize::from(yes > 0)),
        other: 0,
        sample: sample_of(values.iter().map(|v| f64::from(*v)), values.len()),
    }
}

fn strings(values: &[String]) -> Summary {
    let mut seen: BTreeMap<&str, u64> = BTreeMap::new();
    for value in values {
        *seen.entry(value.as_str()).or_default() += 1;
    }
    // Commonest first, and ties broken by the value so the list is the same on
    // every machine: a panel that reorders itself between two runs of the same
    // file reads as a file that changed.
    let mut ranked: Vec<(&str, u64)> = seen.iter().map(|(text, count)| (*text, *count)).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));

    let kept: Vec<Category> = ranked
        .iter()
        .take(TEXT_LIMIT)
        .map(|(text, count)| Category {
            label: (*text).to_string(),
            value: None,
            count: *count,
        })
        .collect();
    let other: u64 = ranked.iter().skip(TEXT_LIMIT).map(|(_, count)| *count).sum();

    Summary {
        family: Family::Text,
        count: values.len(),
        finite: values.len(),
        non_finite: 0,
        min: None,
        max: None,
        mean: None,
        deviation: None,
        histogram: None,
        categories: kept,
        distinct: Some(seen.len()),
        other,
        sample: None,
    }
}

/// One value out of every `step`, capped at [`SAMPLE_LIMIT`].
///
/// Regular rather than random: a chart drawn against the index has to keep the
/// order and the spacing, and a reservoir sample of a well log is a picture of
/// a different well.
fn sample_of(values: impl Iterator<Item = f64>, count: usize) -> Option<Sample> {
    if count == 0 {
        return None;
    }
    let step = count.div_ceil(SAMPLE_LIMIT).max(1);
    Some(Sample {
        step,
        values: values.step_by(step).collect(),
    })
}
