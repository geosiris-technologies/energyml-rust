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
//!   corpus says *no data here* - 785 250 of ALWYN's 987 387 grid nodes - and
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
//!   equal to the maximum belongs to the last bin, not to a bin past the end -
//!   the classic off-by-one, and it silently drops exactly the values a person
//!   is looking for when they open a histogram.
//! - **A series is decimated here, not in the transport.** Plotting a million
//!   points means shipping a million numbers to draw two thousand pixels. The
//!   step is stated with the sample so a chart can label the axis with real
//!   indices instead of pretending the sample is the array.
//!
//! # Beyond a summary
//!
//! [`Summary`] describes one column of numbers for one chart. The modules below
//! hold the arithmetic every view shares: a one-pass mergeable [`Accumulator`],
//! quantiles ([`quantiles`], with a [`LogSketch`] above ten million values), the
//! [`Scale`] of a value, [`Binned`] histograms, [`modes`] and thresholds chosen
//! from a [`modes::StatMethods`] registry, the variation along [`axes`], the
//! comparison of two arrays in [`pairs`], the counting of cells by a
//! [`CellRule`], and [`describe`], which composes them in steps.

mod summary;
pub use summary::{CATEGORY_LIMIT, Category, Histogram, SAMPLE_LIMIT, Sample, Summary, TEXT_LIMIT};

pub mod accumulate;
pub mod axes;
pub mod binned;
pub mod cells;
pub mod describe;
pub mod modes;
pub mod pairs;
pub mod quantile;
pub mod scale;
pub mod sketch;

pub use accumulate::{Accumulator, Moments};
pub use binned::Binned;
pub use cells::{CellRule, RuleTally, runs, tally};
pub use quantile::{Quantiles, quantiles};
pub use scale::Scale;
pub use sketch::LogSketch;
