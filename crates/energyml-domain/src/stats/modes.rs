//! The peaks of a histogram, the threshold between two populations, and the
//! number of bars, by methods the caller picks.
//!
//! Methods are held in a registry ([`StatMethods`]) on the pattern of the
//! readers registry of `energyml-geometry`: a trait, a `Vec<Box<dyn ..>>`, the
//! caller's additions tried **first**, stable ids that recipes can write. The
//! built-in methods are:
//!
//! - bins: `fixed`, `freedman_diaconis` (Freedman and Diaconis 1981,
//!   *Z. Wahrscheinlichkeitstheorie verw. Gebiete* 57, p. 453-476), `sturges`
//!   (Sturges 1926, *JASA* 21(153), p. 65-66);
//! - modes: `histogram_peaks`;
//! - thresholds: `valley` and `otsu` (Otsu 1979, *IEEE Trans. SMC* 9(1),
//!   p. 62-66).

use super::binned::Binned;

/// Bars of a histogram when the caller does not say.
pub const DEFAULT_BINS: usize = 24;
/// The most bars this build draws.
pub const MAX_BINS: usize = 512;
/// A peak is a mode when its prominence is at least this share of the
/// tallest bar.
pub const MODE_PROMINENCE: f64 = 0.25;
/// A valley is deep enough to split two populations when it is at most this
/// share of the smaller mode.
pub const VALLEY_DEPTH: f64 = 0.5;

/// The id of the `fixed` bin method.
pub const FIXED: &str = "fixed";
/// The id of the `freedman_diaconis` bin method.
pub const FREEDMAN_DIACONIS: &str = "freedman_diaconis";
/// The id of the `sturges` bin method.
pub const STURGES: &str = "sturges";
/// The id of the `histogram_peaks` mode method.
pub const HISTOGRAM_PEAKS: &str = "histogram_peaks";
/// The id of the `valley` threshold method.
pub const VALLEY: &str = "valley";
/// The id of the `otsu` threshold method.
pub const OTSU: &str = "otsu";

/// A peak of a histogram.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mode {
    /// The value at the middle of the peak's bin, in value space.
    pub at: f64,
    /// The bin (the middle one of a plateau of equal bins).
    pub bin: usize,
    /// The count of the bin.
    pub height: u64,
    /// Topographic prominence as a share of the tallest bar.
    pub prominence: f64,
}

/// What a threshold measures.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ThresholdMeasure {
    /// The valley's count as a share of the smaller mode (smaller is deeper).
    ValleyDepth(f64),
    /// Otsu's separability, between-class variance over total variance, in
    /// `[0, 1]` (larger is more separable).
    Separability(f64),
}

/// A threshold between two populations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Threshold {
    /// The value of the threshold.
    pub at: f64,
    /// The two modes it lies between, ascending, when the method uses modes.
    pub between: Option<(f64, f64)>,
    /// What the method measured.
    pub measure: ThresholdMeasure,
    /// The id of the method.
    pub method: &'static str,
}

/// Why no threshold was found.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ThresholdRefusal {
    /// Nothing to split.
    #[error("the histogram holds nothing to split")]
    Empty,
    /// Fewer than two modes.
    #[error("{modes} mode(s): a threshold between two populations needs two")]
    Unimodal {
        /// How many modes there are.
        modes: usize,
    },
    /// The valley is too shallow to be a valley.
    #[error("the valley is {depth} of the smaller mode, above the {limit} that makes it a valley")]
    Shallow {
        /// The valley's count as a share of the smaller mode.
        depth: f64,
        /// The limit it exceeds.
        limit: f64,
    },
}

/// What a bin method sees of the values. `iqr` and `span` are in the space of
/// the scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BinInput {
    /// The number of bars the caller asked for.
    pub requested: usize,
    /// The number of values.
    pub count: u64,
    /// The interquartile range, when known.
    pub iqr: Option<f64>,
    /// The width of the scale.
    pub span: f64,
}

/// A rule for the number of bars.
pub trait BinMethod: Send + Sync {
    /// The stable id.
    fn id(&self) -> &'static str;
    /// The number of bars, or `None` when the rule is undefined for these
    /// values (a zero spread): the caller says so and falls back.
    fn bins(&self, input: &BinInput) -> Option<usize>;
}

/// A rule that finds the peaks of a histogram.
pub trait ModeMethod: Send + Sync {
    /// The stable id.
    fn id(&self) -> &'static str;
    /// The peaks, ascending in value.
    fn modes(&self, binned: &Binned) -> Vec<Mode>;
}

/// A rule that finds the threshold between two populations.
pub trait ThresholdMethod: Send + Sync {
    /// The stable id.
    fn id(&self) -> &'static str;
    /// The threshold, or the reason there is none.
    fn threshold(&self, binned: &Binned, modes: &[Mode]) -> Result<Threshold, ThresholdRefusal>;
}

/// The registry of methods. The caller's additions are tried first.
pub struct StatMethods {
    bins: Vec<Box<dyn BinMethod>>,
    modes: Vec<Box<dyn ModeMethod>>,
    thresholds: Vec<Box<dyn ThresholdMethod>>,
}

impl Default for StatMethods {
    fn default() -> Self {
        Self::builtin()
    }
}

impl StatMethods {
    /// A registry with no method.
    pub fn none() -> Self {
        Self {
            bins: Vec::new(),
            modes: Vec::new(),
            thresholds: Vec::new(),
        }
    }

    /// The built-in methods: bins `fixed`, `freedman_diaconis`, `sturges`;
    /// modes `histogram_peaks`; thresholds `valley`, `otsu`.
    pub fn builtin() -> Self {
        Self {
            bins: vec![Box::new(Fixed), Box::new(FreedmanDiaconis), Box::new(Sturges)],
            modes: vec![Box::new(HistogramPeaks)],
            thresholds: vec![Box::new(Valley), Box::new(Otsu)],
        }
    }

    /// Add a bin method; it is tried before the others.
    pub fn push_bin_method(&mut self, method: Box<dyn BinMethod>) -> &mut Self {
        self.bins.insert(0, method);
        self
    }

    /// Add a mode method; it is tried before the others.
    pub fn push_mode_method(&mut self, method: Box<dyn ModeMethod>) -> &mut Self {
        self.modes.insert(0, method);
        self
    }

    /// Add a threshold method; it is tried before the others.
    pub fn push_threshold_method(&mut self, method: Box<dyn ThresholdMethod>) -> &mut Self {
        self.thresholds.insert(0, method);
        self
    }

    /// The first bin method of this id.
    pub fn bin_method(&self, id: &str) -> Option<&dyn BinMethod> {
        self.bins.iter().find(|m| m.id() == id).map(|m| m.as_ref())
    }

    /// The first mode method of this id.
    pub fn mode_method(&self, id: &str) -> Option<&dyn ModeMethod> {
        self.modes.iter().find(|m| m.id() == id).map(|m| m.as_ref())
    }

    /// The first threshold method of this id.
    pub fn threshold_method(&self, id: &str) -> Option<&dyn ThresholdMethod> {
        self.thresholds.iter().find(|m| m.id() == id).map(|m| m.as_ref())
    }

    /// The ids of the bin methods, in the order they are tried.
    pub fn bin_ids(&self) -> Vec<&'static str> {
        self.bins.iter().map(|m| m.id()).collect()
    }

    /// The ids of the mode methods, in the order they are tried.
    pub fn mode_ids(&self) -> Vec<&'static str> {
        self.modes.iter().map(|m| m.id()).collect()
    }

    /// The ids of the threshold methods, in the order they are tried.
    pub fn threshold_ids(&self) -> Vec<&'static str> {
        self.thresholds.iter().map(|m| m.id()).collect()
    }
}

struct Fixed;
struct FreedmanDiaconis;
struct Sturges;
struct HistogramPeaks;
struct Valley;
struct Otsu;

impl BinMethod for Fixed {
    fn id(&self) -> &'static str {
        FIXED
    }

    fn bins(&self, input: &BinInput) -> Option<usize> {
        Some(input.requested.clamp(1, MAX_BINS))
    }
}

impl BinMethod for FreedmanDiaconis {
    fn id(&self) -> &'static str {
        FREEDMAN_DIACONIS
    }

    fn bins(&self, input: &BinInput) -> Option<usize> {
        // A zero or missing spread, a `NaN`, or no value: undefined.
        let iqr = input.iqr.filter(|v| *v > 0.0)?;
        if input.count == 0 || input.span.is_nan() || input.span <= 0.0 {
            return None;
        }
        // The bar width is `h = 2 iqr / cbrt(n)`; `span / h` is written
        // without the division by `h` so a perfect cube gives an exact count.
        let bins = (input.span * (input.count as f64).cbrt() / (2.0 * iqr)).ceil();
        Some((bins as usize).clamp(1, MAX_BINS))
    }
}

impl BinMethod for Sturges {
    fn id(&self) -> &'static str {
        STURGES
    }

    fn bins(&self, input: &BinInput) -> Option<usize> {
        if input.count == 0 {
            return None;
        }
        let bins = (input.count as f64).log2().ceil() + 1.0;
        Some((bins as usize).clamp(1, MAX_BINS))
    }
}

impl ModeMethod for HistogramPeaks {
    fn id(&self) -> &'static str {
        HISTOGRAM_PEAKS
    }

    /// A bar (or a plateau of equal bars, taken whole, its middle bar standing
    /// for it, the lower one on a tie) is a peak when both its neighbours are
    /// strictly lower; the edge counts as lower. Its prominence is the height
    /// above the higher of the two lowest points met going left and right up
    /// to a strictly taller bar (or the edge, where a bar of zero is
    /// supposed), as a share of the tallest bar. Peaks under
    /// [`MODE_PROMINENCE`] are dropped.
    fn modes(&self, binned: &Binned) -> Vec<Mode> {
        let counts = &binned.counts;
        let n = counts.len();
        let max_count = counts.iter().copied().max().unwrap_or(0);
        if binned.total() == 0 || max_count == 0 {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut start = 0;
        while start < n {
            let height = counts[start];
            let mut end = start;
            while end + 1 < n && counts[end + 1] == height {
                end += 1;
            }
            let left_lower = start == 0 || counts[start - 1] < height;
            let right_lower = end + 1 == n || counts[end + 1] < height;
            if height > 0 && left_lower && right_lower {
                let left = base_going_left(counts, start, height);
                let right = base_going_right(counts, end, height);
                let prominence = (height - left.max(right)) as f64 / max_count as f64;
                if prominence >= MODE_PROMINENCE {
                    let bin = start + (end - start) / 2;
                    out.push(Mode {
                        at: binned.center(bin),
                        bin,
                        height,
                        prominence,
                    });
                }
            }
            start = end + 1;
        }
        out
    }
}

/// The lowest count between the bar before `start` and the first strictly
/// taller bar to the left; zero when the edge is reached first.
fn base_going_left(counts: &[u64], start: usize, height: u64) -> u64 {
    let mut lowest = u64::MAX;
    let mut i = start;
    while i > 0 {
        i -= 1;
        if counts[i] > height {
            return lowest;
        }
        lowest = lowest.min(counts[i]);
    }
    0
}

/// The same going right from the bar after `end`.
fn base_going_right(counts: &[u64], end: usize, height: u64) -> u64 {
    let mut lowest = u64::MAX;
    for count in &counts[end + 1..] {
        if *count > height {
            return lowest;
        }
        lowest = lowest.min(*count);
    }
    0
}

impl ThresholdMethod for Valley {
    fn id(&self) -> &'static str {
        VALLEY
    }

    /// The lowest bar strictly between the two tallest modes (the one nearest
    /// their middle on a tie, then the lowest), as a share of the smaller
    /// mode. Refused when there are fewer than two modes, or when the valley is
    /// above [`VALLEY_DEPTH`].
    fn threshold(&self, binned: &Binned, modes: &[Mode]) -> Result<Threshold, ThresholdRefusal> {
        if binned.total() == 0 {
            return Err(ThresholdRefusal::Empty);
        }
        if modes.len() < 2 {
            return Err(ThresholdRefusal::Unimodal { modes: modes.len() });
        }
        let mut by_height: Vec<&Mode> = modes.iter().collect();
        by_height.sort_by(|a, b| b.height.cmp(&a.height).then(a.at.total_cmp(&b.at)));
        let (a, b) = if by_height[0].bin < by_height[1].bin {
            (by_height[0], by_height[1])
        } else {
            (by_height[1], by_height[0])
        };
        let middle = (a.bin + b.bin) as f64 / 2.0;
        let mut valley = a.bin + 1;
        for bin in a.bin + 1..b.bin {
            let better = binned.counts[bin] < binned.counts[valley]
                || (binned.counts[bin] == binned.counts[valley]
                    && ((bin as f64 - middle).abs() < (valley as f64 - middle).abs()));
            if better {
                valley = bin;
            }
        }
        let depth = binned.counts[valley] as f64 / a.height.min(b.height) as f64;
        if depth > VALLEY_DEPTH {
            return Err(ThresholdRefusal::Shallow {
                depth,
                limit: VALLEY_DEPTH,
            });
        }
        Ok(Threshold {
            at: binned.center(valley),
            between: Some((a.at, b.at)),
            measure: ThresholdMeasure::ValleyDepth(depth),
            method: VALLEY,
        })
    }
}

impl ThresholdMethod for Otsu {
    fn id(&self) -> &'static str {
        OTSU
    }

    /// Otsu's cut, on the centres of the bars in the space of the scale: the
    /// cut after bar `k` that maximises the between-class variance
    /// `w0 w1 (mu0 - mu1)^2`, the smallest `k` on a tie. The modes are not
    /// used.
    fn threshold(&self, binned: &Binned, _modes: &[Mode]) -> Result<Threshold, ThresholdRefusal> {
        let counts = &binned.counts;
        if counts.iter().filter(|c| **c > 0).count() < 2 {
            return Err(ThresholdRefusal::Empty);
        }
        let total = binned.total() as f64;
        let centres: Vec<f64> = (0..counts.len())
            .map(|k| binned.scale.transform(binned.center(k)).unwrap_or(f64::NAN))
            .collect();
        let total_sum: f64 = counts.iter().zip(&centres).map(|(n, t)| *n as f64 * t).sum();
        let mean = total_sum / total;
        let total_variance: f64 = counts
            .iter()
            .zip(&centres)
            .map(|(n, t)| *n as f64 * (t - mean) * (t - mean))
            .sum::<f64>()
            / total;

        let mut best: Option<(usize, f64)> = None;
        let (mut n0, mut sum0) = (0.0f64, 0.0f64);
        for k in 0..counts.len() - 1 {
            n0 += counts[k] as f64;
            sum0 += counts[k] as f64 * centres[k];
            let n1 = total - n0;
            if n0 == 0.0 || n1 == 0.0 {
                continue;
            }
            let (w0, w1) = (n0 / total, n1 / total);
            let (mu0, mu1) = (sum0 / n0, (total_sum - sum0) / n1);
            let between = w0 * w1 * (mu0 - mu1) * (mu0 - mu1);
            if best.is_none_or(|(_, b)| between > b) {
                best = Some((k, between));
            }
        }
        let Some((k, between)) = best else {
            return Err(ThresholdRefusal::Empty);
        };
        let eta = if total_variance > 0.0 {
            (between / total_variance).clamp(0.0, 1.0)
        } else {
            0.0
        };
        Ok(Threshold {
            at: binned.edges(k).1,
            between: None,
            measure: ThresholdMeasure::Separability(eta),
            method: OTSU,
        })
    }
}
