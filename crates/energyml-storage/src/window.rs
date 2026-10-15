//! Reading a bounded window of an array, with the statistics of what was read.
//!
//! [`EnergymlStorage::read_array`] reads the slice a location names, however
//! large; nothing in the trait bounds it, says how much would fit, or looks at
//! the numbers. A face that hands the values to a person or a model (an MCP
//! server, a notebook cell) needs all three, and needs them the same on every
//! store, so they are here, over the trait, once.
//!
//! # What "bounded" means
//!
//! The bound is on the **number of elements read**, never on the bytes of an
//! answer, and the window is cut along the **first axis only**. A window is a
//! block of whole rows: clipping the other axes too would return a rectangle
//! nobody asked for, and clipping an inner axis alone would mean one read per
//! row. When one row along the first axis already exceeds the bound, nothing
//! is read and the request is refused, with the size of the row in the
//! message, so that the caller narrows the trailing axes itself.
//!
//! The metadata is read first ([`EnergymlStorage::array_metadata`]), the data
//! second, and only the window: the store never reads more than the bound.

use crate::array::{ArrayData, ArrayLocation, ArrayMetadata, ArrayValues};
use crate::error::{Error, ExternalDataReason, Result};
use crate::storage::EnergymlStorage;

/// Bounds of a window read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct WindowLimits {
    /// The most elements one read may return, counted over all axes.
    pub max_elements: usize,
}

impl WindowLimits {
    /// A bound of `max_elements` elements.
    pub fn new(max_elements: usize) -> Self {
        Self { max_elements }
    }
}

/// Statistics of the numeric values read (NaN are counted, not folded in).
///
/// Of the window, not of the whole array: the whole array is never read.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Stats {
    /// How many values were looked at, NaN included.
    pub count: usize,
    /// How many of them are NaN.
    pub nan: usize,
    /// The smallest value that is not NaN; `None` when there is none.
    pub min: Option<f64>,
    /// The largest value that is not NaN; `None` when there is none.
    pub max: Option<f64>,
    /// The mean of the values that are not NaN, summed in `f64`; `None` when
    /// there is none. An infinity among them makes it infinite, which is what
    /// the data says.
    pub mean: Option<f64>,
}

/// A window of one array: what was asked, what was read, and why they differ.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Window {
    /// The whole array's shape (metadata read first).
    pub whole: ArrayMetadata,
    /// The location actually read (start/count after clipping), with every
    /// axis spelled out.
    pub read: ArrayLocation,
    /// The values of the window, row-major.
    pub data: ArrayData,
    /// `None` when the values are not numeric: the array's logical type is a
    /// boolean, a string or a custom type (the bytes of a boolean array are
    /// not a quantity). Every other element type is numeric.
    pub stats: Option<Stats>,
    /// True when `count` was reduced to fit `max_elements`.
    pub clipped: bool,
}

/// Statistics of numeric data; `None` for non-numeric values.
///
/// Numeric is decided by the element type (every [`ArrayValues`] variant is
/// numeric) and by the logical type the metadata carries: an array of booleans
/// travels as bytes and has no meaningful mean. Folds the borrowed slice; the
/// only conversion is per element, nothing is copied.
///
/// `TODO(verify)`: the spellings of ETP 1.2 `LogicalArrayType` (`arrayOfBoolean`,
/// `arrayOfString`, `arrayOfCustom`); the test is a case-insensitive search for
/// `bool`, `string` and `custom`.
pub fn stats_of(data: &ArrayData) -> Option<Stats> {
    if non_numeric(data.metadata.logical_type.as_deref()) {
        return None;
    }
    Some(match &data.values {
        ArrayValues::F64(v) => fold(v.iter().copied()),
        ArrayValues::F32(v) => fold(v.iter().map(|x| f64::from(*x))),
        ArrayValues::I64(v) => fold(v.iter().map(|x| *x as f64)),
        ArrayValues::I32(v) => fold(v.iter().map(|x| f64::from(*x))),
        ArrayValues::I16(v) => fold(v.iter().map(|x| f64::from(*x))),
        ArrayValues::I8(v) => fold(v.iter().map(|x| f64::from(*x))),
        ArrayValues::U64(v) => fold(v.iter().map(|x| *x as f64)),
        ArrayValues::U32(v) => fold(v.iter().map(|x| f64::from(*x))),
        ArrayValues::U16(v) => fold(v.iter().map(|x| f64::from(*x))),
        // Exhaustive on purpose: a variant added to `ArrayValues` stops this
        // from compiling, rather than being taken as numeric or dropped.
        ArrayValues::U8(v) => fold(v.iter().map(|x| f64::from(*x))),
    })
}

fn non_numeric(logical_type: Option<&str>) -> bool {
    logical_type.is_some_and(|logical| {
        let logical = logical.to_ascii_lowercase();
        ["bool", "string", "custom"]
            .iter()
            .any(|word| logical.contains(word))
    })
}

fn fold(values: impl Iterator<Item = f64>) -> Stats {
    let (mut count, mut nan, mut valid) = (0usize, 0usize, 0usize);
    let (mut min, mut max): (Option<f64>, Option<f64>) = (None, None);
    let mut sum = 0.0f64;
    for x in values {
        count += 1;
        if x.is_nan() {
            nan += 1;
            continue;
        }
        valid += 1;
        sum += x;
        min = Some(min.map_or(x, |m| m.min(x)));
        max = Some(max.map_or(x, |m| m.max(x)));
    }
    Stats {
        count,
        nan,
        min,
        max,
        mean: (valid > 0).then(|| sum / valid as f64),
    }
}

/// Read the metadata, then at most `limits.max_elements` elements of the window
/// `location` asks for (its `start`/`count`; empty = from the start / the whole):
/// the count is clipped along the FIRST axis only, keeping the other axes whole, and
/// `clipped` says so.
///
/// Refused by name, before any data is read: [`Error::OutOfBounds`] for a
/// `start` or `count` whose length is not the array's rank, a negative value,
/// a `start` outside its axis, or a `count` reaching past the end of its axis
/// (on every axis, the first included: only the bound clips, never the
/// array's end); [`Error::TooLarge`] for a bound of zero or a single row along
/// the first axis larger than the bound. Both carry the array's address and
/// the exact figures.
///
/// A store that answers with a different number of values than the window
/// holds is refused too, as [`ExternalDataReason::ShapeDisagrees`]: plausible
/// but short data is the failure that looks like success.
pub async fn read_window<S: EnergymlStorage + Sync + ?Sized>(
    store: &S,
    location: &ArrayLocation,
    limits: &WindowLimits,
) -> Result<Window> {
    let whole = store.array_metadata(location).await?;
    let dims = whole.dimensions.clone();
    let rank = dims.len();
    let refuse = |reason: String| Error::OutOfBounds {
        location: address(location),
        reason,
    };
    let too_large = |reason: String| Error::TooLarge {
        location: address(location),
        reason,
    };

    if limits.max_elements == 0 {
        return Err(too_large(
            "the window bound is zero elements: nothing could be read".into(),
        ));
    }
    if let Some(axis) = dims.iter().position(|d| *d < 0) {
        return Err(refuse(format!(
            "the store reports a negative length on axis {axis}"
        )));
    }
    for (name, given) in [("start", &location.start), ("count", &location.count)] {
        if !given.is_empty() && given.len() != rank {
            return Err(refuse(format!(
                "`{name}` has {} value(s), the array has {rank} axis(es) {dims:?}",
                given.len()
            )));
        }
    }

    let mut starts = vec![0i64; rank];
    let mut counts = vec![0i64; rank];
    for (axis, &length) in dims.iter().enumerate() {
        let start = location.start.get(axis).copied().unwrap_or(0);
        if start < 0 || (start >= length && !(length == 0 && start == 0)) {
            return Err(refuse(format!(
                "start {start} on axis {axis} is outside the array (length {length})"
            )));
        }
        let count = location.count.get(axis).copied().unwrap_or(length - start);
        if count < 0 || (count == 0 && length != 0) || count > length - start {
            return Err(refuse(format!(
                "count {count} from start {start} on axis {axis} does not fit the array (length {length})"
            )));
        }
        starts[axis] = start;
        counts[axis] = count;
    }

    let mut clipped = false;
    if rank > 0 {
        let mut row: usize = 1;
        for count in &counts[1..] {
            row = row.checked_mul(*count as usize).ok_or_else(|| {
                too_large("the trailing axes hold more elements than a usize can count".into())
            })?;
        }
        // A row of zero elements (an empty trailing axis) holds nothing to bound.
        if let Some(allowed) = limits.max_elements.checked_div(row) {
            if allowed == 0 {
                return Err(too_large(format!(
                    "one row along the first axis holds {row} elements, more than the bound of {}: \
                     narrow `count` on the trailing axes",
                    limits.max_elements
                )));
            }
            if counts[0] as usize > allowed {
                counts[0] = allowed as i64;
                clipped = true;
            }
        }
    }

    let mut read = location.clone();
    read.start = starts;
    read.count = counts;
    let data = store.read_array(&read).await?;

    let promised = read.count.iter().map(|c| *c as usize).product::<usize>();
    if data.values.len() != promised {
        return Err(Error::ExternalData {
            location: address(location),
            reason: ExternalDataReason::ShapeDisagrees {
                promised: promised as i64,
                held: data.values.len(),
            },
        });
    }

    let stats = if non_numeric(whole.logical_type.as_deref()) {
        None
    } else {
        stats_of(&data)
    };
    Ok(Window {
        whole,
        read,
        data,
        stats,
        clipped,
    })
}

/// How an error names an array: the ETP address when it has one, the file and
/// the path inside it otherwise.
fn address(location: &ArrayLocation) -> String {
    match location.etp_address() {
        Some((uri, path)) => format!("{uri}#{path}"),
        None => format!("{}#{}", location.file, location.path_in_file),
    }
}
