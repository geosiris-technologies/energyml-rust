//! **A value that drifts with time** — the law that lays a
//! property through a time series by (the `property-time-series` operation, both forms).
//!
//! Nothing here reads a package or knows RESQML: the inputs are a snapshot of
//! values, an anchor instant, a change per year and the instants of a series,
//! all in plain numbers, so that what can be wrong in the arithmetic — which
//! step gets which offset, how many steps there are, which dimension runs
//! slowest — is tested without a file. The operation that writes the objects
//! only sews.
//!
//! # The law
//!
//! A snapshot `v₀(e)`, one value per element, holds at the anchor `t₀`; every
//! value changes by `r` per year from there: `v(e, t) = v₀(e) + r · (t − t₀) / Y`,
//! `Y` the Julian year of 365.25 days — the year `flow_law` counts a decline
//! per, and for the same reason: a change *per year* read over months of 28 to
//! 31 days is only exact against one stated length. A linear drift is the
//! least a law in time can say; it is a law, stated and sourced by its caller,
//! never a measurement.
//!
//! # An instant, or an interval
//!
//! A time series is a list of instants (`eml23.TimeSeries.Time`, *« The list
//! ordering is used by the time index »*, Energistics common Technical
//! Reference Guide v2.3 §11.14 p. 297). A property through it holds either one
//! value per element **at** each instant, or one **over** each interval from
//! an instant to the next — `TimeOrIntervalSeries.UseInterval`
//! (CommonTypes.xsd l. 423–428), which the schema leaves undocumented. `n`
//! instants make `n − 1` intervals: Do 13 dates under `UseInterval`
//! give 12 values? Answered yes. The value over an
//! interval is the law's **mean** over it, which for a linear law is its value
//! at the middle — exactly, not as an approximation.
//!
//! # Time is the slowest dimension
//!
//! RESQML Technical Reference Guide v2.2 §7, the note of the diagram p. 221:
//! *« Values Per patch are structured that way in HDF (top is slowest
//! dimension, bottom is fastest dimension) : \[RealizationCount] … \[TimeCount]
//! … \[IndexableElementCount] … \[ValueCountPerIndexableElement] »*. [`lay`]
//! writes `values[t · n + e]`: every element at the first step, then every
//! element at the second. Laid the other way, a series reads as a property
//! whose elements each hold the whole history of one node — the same count,
//! and every value on the wrong element.

use crate::flow_law::HOURS_PER_YEAR;

/// Seconds in the Julian year, 365.25 days — the year a change is counted per.
pub const SECONDS_PER_YEAR: f64 = HOURS_PER_YEAR * 3600.0;

/// What a law in time or a list of instants cannot be, by name. Ranks count
/// from 1.
#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum TimeLawError {
    #[error("the change per year is a finite number, not {given}")]
    BadChange { given: f64 },
    #[error("{given} instant(s), where {needed} are needed at least: {why}")]
    TooFewInstants {
        given: usize,
        needed: usize,
        why: &'static str,
    },
    #[error(
        "instant {rank} ({seconds} s) does not come after the one before it ({before} s): a time index names an instant by its place in the list, so the list increases and no two instants are equal"
    )]
    NotIncreasing { rank: usize, seconds: i64, before: i64 },
}

/// A linear drift: `per_year` more every year after `anchor`, less before it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drift {
    /// The instant the snapshot holds at, in seconds since 1970-01-01T00:00:00Z.
    pub anchor: i64,
    /// The change per Julian year, in the unit of the values.
    pub per_year: f64,
}

impl Drift {
    /// A drift, or why not.
    pub fn new(anchor: i64, per_year: f64) -> Result<Drift, TimeLawError> {
        if !per_year.is_finite() {
            return Err(TimeLawError::BadChange { given: per_year });
        }
        Ok(Drift { anchor, per_year })
    }

    /// What the drift adds at `seconds` — negative before the anchor for a
    /// positive change.
    pub fn at(&self, seconds: i64) -> f64 {
        self.per_year * (seconds - self.anchor) as f64 / SECONDS_PER_YEAR
    }

    /// What the drift adds **on average** from `from` to `to` — its value at
    /// the middle, which for a linear law is its mean exactly. Computed in
    /// floating point: halving an odd number of seconds in integers would lose
    /// half a second.
    pub fn mean(&self, from: i64, to: i64) -> f64 {
        let middle = ((from - self.anchor) as f64 + (to - self.anchor) as f64) / 2.0;
        self.per_year * middle / SECONDS_PER_YEAR
    }
}

/// At each instant, or over each interval between two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Steps {
    /// One step per instant.
    Instants,
    /// One step per interval from an instant to the next: `n` instants make
    /// `n − 1` steps.
    Intervals,
}

/// What the drift adds at each step of a series whose instants are
/// `instants`, in seconds since the epoch — one offset per instant, or per
/// interval.
///
/// The instants are refused by their rank when they do not increase: a
/// series out of order would give every step an instant nobody meant.
pub fn offsets(drift: &Drift, instants: &[i64], steps: Steps) -> Result<Vec<f64>, TimeLawError> {
    let needed = match steps {
        Steps::Instants => 1,
        Steps::Intervals => 2,
    };
    if instants.len() < needed {
        return Err(TimeLawError::TooFewInstants {
            given: instants.len(),
            needed,
            why: match steps {
                Steps::Instants => "a series holds one instant at least",
                Steps::Intervals => "an interval runs from one instant to the next",
            },
        });
    }
    for rank in 1..instants.len() {
        if instants[rank] <= instants[rank - 1] {
            return Err(TimeLawError::NotIncreasing {
                rank: rank + 1,
                seconds: instants[rank],
                before: instants[rank - 1],
            });
        }
    }
    Ok(match steps {
        Steps::Instants => instants.iter().map(|&at| drift.at(at)).collect(),
        Steps::Intervals => instants
            .windows(2)
            .map(|pair| drift.mean(pair[0], pair[1]))
            .collect(),
    })
}

/// The snapshot `base` moved by each offset, **time slowest**:
/// `values[t · base.len() + e] = base[e] + offsets[t]`. A `NaN` of the
/// snapshot — an element the law it came from said nothing about — stays
/// `NaN` at every step: a drift of nothing is nothing.
pub fn lay(base: &[f64], offsets: &[f64]) -> Vec<f64> {
    offsets
        .iter()
        .flat_map(|offset| base.iter().map(move |value| value + offset))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2016-01-01T00:00:00Z and 2025-01-01T00:00:00Z, from the calendar: 46
    /// years of 365 days and 11 leap days (1972…2012) after 1970, then 9 years
    /// with 2016, 2020 and 2024 leap.
    const Y2016: i64 = (46 * 365 + 11) * 86_400;
    const Y2025: i64 = Y2016 + (9 * 365 + 3) * 86_400;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-12 * (1.0 + b.abs())
    }

    #[test]
    fn the_calendar_constants_are_the_dates_they_name() {
        // 2016-01-01 is day 16 801 of the Unix epoch, 2025-01-01 day 20 089.
        assert_eq!(Y2016, 16_801 * 86_400);
        assert_eq!(Y2025, 20_089 * 86_400);
    }

    #[test]
    fn a_drift_adds_its_change_per_julian_year_from_the_anchor() {
        let drift = Drift::new(Y2016, -0.5).unwrap();
        assert_eq!(drift.at(Y2016), 0.0);
        // 3 288 days are 9.002… Julian years.
        assert!(
            close(drift.at(Y2025), -0.5 * 3288.0 / 365.25),
            "{}",
            drift.at(Y2025)
        );
        // Before the anchor, the other way.
        assert!(close(drift.at(Y2016 - 86_400 * 365), 0.5 * 365.0 / 365.25));
    }

    #[test]
    fn the_mean_over_an_interval_is_the_law_integrated_over_it() {
        // Checked against a Riemann sum of the law, not against the middle.
        let drift = Drift::new(Y2016, 1.25).unwrap();
        let (from, to) = (Y2025, Y2025 + 31 * 86_400);
        let n = 100_000;
        let sum: f64 = (0..n)
            .map(|k| {
                let t = from as f64 + (k as f64 + 0.5) * (to - from) as f64 / n as f64;
                1.25 * (t - Y2016 as f64) / SECONDS_PER_YEAR
            })
            .sum();
        assert!((drift.mean(from, to) - sum / n as f64).abs() < 1e-9);
    }

    #[test]
    fn n_instants_make_n_steps_or_n_minus_one_intervals() {
        let drift = Drift::new(Y2016, 2.0).unwrap();
        let instants = [Y2016, Y2016 + 86_400, Y2016 + 3 * 86_400];
        let at = offsets(&drift, &instants, Steps::Instants).unwrap();
        assert_eq!(at.len(), 3);
        assert!(close(at[2], 2.0 * 3.0 / 365.25));
        let over = offsets(&drift, &instants, Steps::Intervals).unwrap();
        assert_eq!(over.len(), 2, "open question 9: three dates give two values");
        assert!(close(over[0], 2.0 * 0.5 / 365.25), "the middle of the first day");
        assert!(close(over[1], 2.0 * 2.0 / 365.25), "the middle of days 2 and 3");
    }

    #[test]
    fn a_series_that_does_not_increase_is_refused_by_its_rank() {
        let drift = Drift::new(0, 1.0).unwrap();
        assert_eq!(
            offsets(&drift, &[0, 10, 10], Steps::Instants),
            Err(TimeLawError::NotIncreasing {
                rank: 3,
                seconds: 10,
                before: 10
            })
        );
        assert!(matches!(
            offsets(&drift, &[5, 1], Steps::Intervals),
            Err(TimeLawError::NotIncreasing { rank: 2, .. })
        ));
        assert!(matches!(
            offsets(&drift, &[5], Steps::Intervals),
            Err(TimeLawError::TooFewInstants {
                given: 1,
                needed: 2,
                ..
            })
        ));
        assert!(matches!(
            offsets(&drift, &[], Steps::Instants),
            Err(TimeLawError::TooFewInstants {
                given: 0,
                needed: 1,
                ..
            })
        ));
        assert!(matches!(
            Drift::new(0, f64::NAN),
            Err(TimeLawError::BadChange { .. })
        ));
    }

    #[test]
    fn time_is_the_slowest_dimension_and_a_nan_stays_one() {
        let laid = lay(&[10.0, f64::NAN, 30.0], &[0.0, 1.0]);
        assert_eq!(laid.len(), 6);
        // Every element at the first step, then every element at the second.
        assert_eq!([laid[0], laid[2], laid[3], laid[5]], [10.0, 30.0, 11.0, 31.0]);
        assert!(laid[1].is_nan() && laid[4].is_nan());
    }
}
