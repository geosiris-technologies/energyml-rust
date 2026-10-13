//! Min/max decimation: a channel reduced to a bounded number of points over a
//! range of its index, **spikes kept and gaps kept**.
//!
//! What the front end asks on every zoom (the decision being to stay with SVG, which
//! stays tenable because a chart never receives more than a few thousand points per
//! series).
//!
//! # The rule
//!
//! The range `[from, to]` is cut into `buckets` equal slices of the index.
//! Within a slice, each **run** of consecutive good samples gives at most four
//! points - its first, its least, its greatest and its last, in the order the
//! samples come - so a one-sample spike anywhere in a slice survives, and the
//! line drawn through the points enters and leaves the slice where the data
//! does. Nothing is averaged: a mean flattens the very spike a sensor chart is
//! read for.
//!
//! **A gap stays a gap.** A sample that is not [`Quality::Good`] - absent, or
//! flagged (`frozen` and the other PRODML statuses: *"If the status attribute
//! is absent and the value is not "NaN", the data value can be assumed to be
//! good"*, TimeSeriesData.xsd l. 100, and so a flagged one is not) - ends the
//! run it interrupts. The next good sample starts a new one, marked
//! [`Point::starts_run`], which a chart must not join to the point before.
//! Bridging a frozen window with a straight line is exactly the interpolation
//! a front end's log view refuses in depth, carried over to time. The gaps themselves are
//! returned ([`Decimated::gaps`]) so a front end can shade them.
//!
//! # The bound
//!
//! At most `buckets` slices hold points, and each run in a slice gives at most
//! four: **`points ≤ 4 × (buckets + gaps)`**, since each gap splits at most one
//! run in two. A slice with no good sample gives nothing; it is not filled.
//!
//! Only the samples inside the range are read - no sample just outside is
//! added to carry the line to the edge. The index being strictly monotone (the
//! readers refuse it otherwise), the samples in range are contiguous.

use serde_json::{Value, json};

use super::{Quality, Refused, Series};

/// One point kept.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    /// Its index value.
    pub index: f64,
    /// Its value.
    pub value: f64,
    /// The rank of the sample it is, in the series.
    pub sample: usize,
    /// Whether it begins a run: a chart must not join it to the point before.
    pub starts_run: bool,
}

/// A run of samples that are not good, by index extent.
#[derive(Clone, Debug, PartialEq)]
pub struct Gap {
    /// The index of its first sample.
    pub from: f64,
    /// The index of its last sample.
    pub to: f64,
    /// How many samples it holds.
    pub samples: usize,
    /// The quality of its first sample: `absent`, or the status.
    pub quality: String,
}

/// A channel, decimated.
#[derive(Clone, Debug, PartialEq)]
pub struct Decimated {
    /// The channel's mnemonic.
    pub channel: String,
    /// The range asked, low end first.
    pub from: f64,
    /// Its high end.
    pub to: f64,
    /// The buckets asked.
    pub buckets: usize,
    /// How many samples fell in the range.
    pub samples: usize,
    /// The points kept, in sample order.
    pub points: Vec<Point>,
    /// The gaps met in the range.
    pub gaps: Vec<Gap>,
}

impl Decimated {
    /// As the bridges hand it to a front end.
    pub fn to_json(&self) -> Value {
        json!({
            "channel": self.channel,
            "from": self.from,
            "to": self.to,
            "buckets": self.buckets,
            "samples": self.samples,
            "points": self.points.iter().map(|point| json!({
                "index": point.index,
                "value": point.value,
                "sample": point.sample,
                "startsRun": point.starts_run,
            })).collect::<Vec<_>>(),
            "gaps": self.gaps.iter().map(|gap| json!({
                "from": gap.from,
                "to": gap.to,
                "samples": gap.samples,
                "quality": gap.quality,
            })).collect::<Vec<_>>(),
        })
    }
}

/// What one run within one bucket keeps.
struct Run {
    bucket: usize,
    first: Point,
    min: Point,
    max: Point,
    last: Point,
}

impl Run {
    fn start(bucket: usize, point: Point) -> Run {
        Run {
            bucket,
            first: point,
            min: point,
            max: point,
            last: point,
        }
    }

    fn add(&mut self, point: Point) {
        if point.value < self.min.value {
            self.min = point;
        }
        if point.value > self.max.value {
            self.max = point;
        }
        self.last = point;
    }

    /// Its points, distinct and in sample order; only the first may start a
    /// run.
    fn flush(self, out: &mut Vec<Point>) {
        let mut kept = [self.first, self.min, self.max, self.last];
        kept.sort_by_key(|point| point.sample);
        let mut previous: Option<usize> = None;
        for mut point in kept {
            if previous == Some(point.sample) {
                continue;
            }
            if previous.is_some() {
                point.starts_run = false;
            }
            previous = Some(point.sample);
            out.push(point);
        }
    }
}

/// Decimate the channel at rank `channel` of `series` over `[from, to]` of its
/// index, into at most `buckets` slices.
///
/// Refused by name: no such channel, a channel that holds no numbers, a range
/// that is not finite or has no width, no bucket.
pub fn decimate(
    series: &Series,
    channel: usize,
    from: f64,
    to: f64,
    buckets: usize,
) -> Result<Decimated, Refused> {
    let Some(chosen) = series.channels.get(channel) else {
        return Err(Refused::NoSuchChannel {
            at: channel,
            channels: series.channels.len(),
        });
    };
    let Some(values) = chosen.numbers() else {
        return Err(Refused::NotNumeric {
            channel: chosen.mnemonic.clone(),
            family: match chosen.values.family() {
                crate::values::Family::Boolean => "boolean",
                _ => "text",
            },
        });
    };
    let bad = |why: &'static str| Refused::BadWindow {
        from,
        to,
        buckets,
        why,
    };
    if buckets == 0 {
        return Err(bad("no bucket to put a point in"));
    }
    if !from.is_finite() || !to.is_finite() {
        return Err(bad("the range is not finite"));
    }
    let (low, high) = if from <= to { (from, to) } else { (to, from) };
    if high == low {
        return Err(bad("the range has no width"));
    }
    let width = (high - low) / buckets as f64;
    let bucket_of = |x: f64| (((x - low) / width).floor() as usize).min(buckets - 1);

    let mut points: Vec<Point> = Vec::new();
    let mut gaps: Vec<Gap> = Vec::new();
    let mut run: Option<Run> = None;
    // Whether the next good sample begins a run, and whether the sample before
    // was part of a gap.
    let mut broken = true;
    let mut in_gap = false;
    let mut samples = 0usize;

    for (at, x) in series.index.values.iter().copied().enumerate() {
        if !(low..=high).contains(&x) {
            continue;
        }
        samples += 1;
        let quality = chosen.quality.get(at).cloned().unwrap_or(Quality::Absent);
        let value = values[at];
        if !quality.is_good() || !value.is_finite() {
            if let Some(done) = run.take() {
                done.flush(&mut points);
            }
            let label = match quality {
                Quality::Flagged(label) => label,
                _ => "absent".to_string(),
            };
            if in_gap && let Some(gap) = gaps.last_mut() {
                gap.to = x;
                gap.samples += 1;
            } else {
                gaps.push(Gap {
                    from: x,
                    to: x,
                    samples: 1,
                    quality: label,
                });
            }
            in_gap = true;
            broken = true;
            continue;
        }
        let point = Point {
            index: x,
            value,
            sample: at,
            starts_run: broken,
        };
        let bucket = bucket_of(x);
        match run.as_mut() {
            Some(current) if current.bucket == bucket => current.add(point),
            _ => {
                if let Some(done) = run.take() {
                    done.flush(&mut points);
                }
                run = Some(Run::start(bucket, point));
            }
        }
        broken = false;
        in_gap = false;
    }
    if let Some(done) = run.take() {
        done.flush(&mut points);
    }
    Ok(Decimated {
        channel: chosen.mnemonic.clone(),
        from: low,
        to: high,
        buckets,
        samples,
        points,
        gaps,
    })
}
