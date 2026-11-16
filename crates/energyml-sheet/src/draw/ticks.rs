//! Graduations in round numbers: one rule for every drawing.
//!
//! A linear axis gets a step of 1, 2 or 5 times a power of ten, the smallest
//! one that gives no more than `target` intervals: a ruler that crowds is worse
//! than one that is sparse. Its values are `k x step`, never accumulated, each
//! the `f64` nearest to its decimal. A logarithmic axis gets 1, 2 and 5 times
//! each power of ten, then the powers of ten, then one power in every `n`.
//!
//! Nothing is guessed: a bound that is not a number, equal bounds or a target
//! below one graduation are refused by name ([`TickRefusal`]).

/// The tolerance of the first and last multiple of the step, as a fraction of
/// the step.
const SLACK: f64 = 1e-9;

/// The tolerance of a logarithmic bound, relative.
const LOG_SLACK: f64 = 1e-12;

/// The most graduations a linear axis may compute: a guard against a defect,
/// never reached by a target that is a screen's worth of graduations.
const MAX_TICKS: i64 = 10_000;

/// Why no graduation can be made. Typed; the face writes the sentence.
#[derive(Clone, Copy, Debug, PartialEq, thiserror::Error)]
pub enum TickRefusal {
    /// A bound is NaN or infinite, or their difference overflows.
    #[error("no graduation between {a} and {b}: a bound is not a finite number")]
    NotFinite {
        /// The first bound.
        a: f64,
        /// The second bound.
        b: f64,
    },
    /// Both bounds are the same number.
    #[error("no graduation between {at} and itself: the range is empty")]
    Degenerate {
        /// The number both bounds are.
        at: f64,
    },
    /// The number of graduations wanted is below 1, or not finite.
    #[error("the number of graduations wanted must be a finite number from 1, got {target}")]
    BadTarget {
        /// The target given.
        target: f64,
    },
    /// A logarithmic axis needs two positive bounds.
    #[error("a logarithmic axis needs positive bounds, got {a} and {b}")]
    NotPositive {
        /// The first bound.
        a: f64,
        /// The second bound.
        b: f64,
    },
}

/// The graduations of a linear axis.
#[derive(Clone, Debug, PartialEq)]
pub struct LinearTicks {
    /// The step: 1, 2 or 5 times a power of ten, exact as a decimal.
    pub step: f64,
    /// How many decimals a label needs ([`label`]).
    pub decimals: usize,
    /// The values, ascending, each the `f64` nearest to its decimal.
    pub values: Vec<f64>,
}

/// How dense a logarithmic axis came out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogDensity {
    /// 1, 2 and 5 times each power of ten.
    Fine,
    /// The powers of ten.
    Decades,
    /// One power of ten in every `n` (the exponents that are multiples of `n`).
    Every(u32),
}

/// The graduations of a logarithmic axis.
#[derive(Clone, Debug, PartialEq)]
pub struct LogTicks {
    /// How dense the axis came out.
    pub density: LogDensity,
    /// The values, ascending.
    pub values: Vec<f64>,
}

/// The number `m x 10^e`, exact as a decimal (read from its text, so that
/// `5e-2` is the `f64` nearest 0.05 and not `5 x 0.01`).
fn exact(m: u32, e: i32) -> f64 {
    format!("{m}e{e}").parse().unwrap_or(f64::NAN)
}

/// The candidates `1, 2, 5` times a power of ten around `limit`, in the order
/// asked: ascending when `ascending`, descending otherwise.
fn candidates(limit: f64, ascending: bool) -> Vec<f64> {
    let e = limit.log10().floor() as i32;
    let mantissas: [u32; 3] = if ascending { [1, 2, 5] } else { [5, 2, 1] };
    let powers: Vec<i32> = if ascending {
        (e - 1..=e + 1).collect()
    } else {
        (e - 1..=e + 1).rev().collect()
    };
    powers
        .into_iter()
        .flat_map(|power| mantissas.into_iter().map(move |m| exact(m, power)))
        .collect()
}

/// The smallest `1, 2, 5 x 10^k` that is at least `limit`; `None` when `limit`
/// is not a finite, positive number.
pub fn nice_step_at_least(limit: f64) -> Option<f64> {
    if !(limit.is_finite() && limit > 0.0) {
        return None;
    }
    candidates(limit, true)
        .into_iter()
        .find(|c| *c >= limit * (1.0 - SLACK))
}

/// The largest `1, 2, 5 x 10^k` that is at most `limit`; `None` when `limit`
/// is not a finite, positive number.
pub fn nice_step_at_most(limit: f64) -> Option<f64> {
    if !(limit.is_finite() && limit > 0.0) {
        return None;
    }
    candidates(limit, false)
        .into_iter()
        .find(|c| *c <= limit * (1.0 + SLACK))
}

/// How many decimals a step of `step` needs: none from 1, else the decimal
/// place of its first digit.
fn decimals_of(step: f64) -> usize {
    if step >= 1.0 {
        0
    } else {
        (-step.log10().floor()) as usize
    }
}

/// `value` written with `decimals` decimals and read back: the `f64` nearest to
/// that decimal (`0.30000000000000004` becomes `0.3`), and never `-0.0`.
fn tidy(value: f64, decimals: usize) -> f64 {
    let rounded = format!("{value:.decimals$}").parse::<f64>().unwrap_or(value);
    if rounded == 0.0 { 0.0 } else { rounded }
}

/// The graduations of a linear axis between `a` and `b` (in whichever order),
/// at most `target + 1` of them.
///
/// # Errors
///
/// [`TickRefusal::NotFinite`], [`TickRefusal::Degenerate`] or
/// [`TickRefusal::BadTarget`], in that order of test.
pub fn nice_ticks(a: f64, b: f64, target: f64) -> Result<LinearTicks, TickRefusal> {
    if !(a.is_finite() && b.is_finite()) {
        return Err(TickRefusal::NotFinite { a, b });
    }
    if a == b {
        return Err(TickRefusal::Degenerate { at: a });
    }
    if !(target.is_finite() && target >= 1.0) {
        return Err(TickRefusal::BadTarget { target });
    }
    let low = a.min(b);
    let high = a.max(b);
    let range = high - low;
    if !range.is_finite() {
        return Err(TickRefusal::NotFinite { a, b });
    }
    let step = nice_step_at_least(range / target).ok_or(TickRefusal::NotFinite { a, b })?;
    let decimals = decimals_of(step);
    let first = (low / step - SLACK).ceil() as i64;
    let last = (high / step + SLACK).floor() as i64;
    if last - first > MAX_TICKS {
        return Err(TickRefusal::NotFinite { a, b });
    }
    let values = (first..=last).map(|k| tidy(k as f64 * step, decimals)).collect();
    Ok(LinearTicks {
        step,
        decimals,
        values,
    })
}

/// The graduations of a logarithmic axis between `a` and `b` (in whichever
/// order): 1, 2 and 5 per decade if they fit `target`, else the decades, else
/// one decade in every few.
///
/// # Errors
///
/// [`TickRefusal::NotPositive`] first (a bound that is not positive, or not
/// finite), then [`TickRefusal::Degenerate`] and [`TickRefusal::BadTarget`].
pub fn log_ticks(a: f64, b: f64, target: f64) -> Result<LogTicks, TickRefusal> {
    if !(a.is_finite() && b.is_finite() && a > 0.0 && b > 0.0) {
        return Err(TickRefusal::NotPositive { a, b });
    }
    if a == b {
        return Err(TickRefusal::Degenerate { at: a });
    }
    if !(target.is_finite() && target >= 1.0) {
        return Err(TickRefusal::BadTarget { target });
    }
    let low = a.min(b);
    let high = a.max(b);
    let first = low.log10().floor() as i32 - 1;
    let last = high.log10().ceil() as i32 + 1;
    let within = |mantissas: &[u32], every: i32| -> Vec<f64> {
        let mut out = Vec::new();
        for power in first..=last {
            if power.rem_euclid(every) != 0 {
                continue;
            }
            for &m in mantissas {
                let v = exact(m, power);
                if v >= low * (1.0 - LOG_SLACK) && v <= high * (1.0 + LOG_SLACK) {
                    out.push(v);
                }
            }
        }
        out
    };
    let fine = within(&[1, 2, 5], 1);
    if fine.len() as f64 <= target {
        return Ok(LogTicks {
            density: LogDensity::Fine,
            values: fine,
        });
    }
    let decades = within(&[1], 1);
    if decades.len() as f64 <= target {
        return Ok(LogTicks {
            density: LogDensity::Decades,
            values: decades,
        });
    }
    for every in 2..400 {
        let values = within(&[1], every);
        if values.len() as f64 <= target {
            return Ok(LogTicks {
                density: LogDensity::Every(every as u32),
                values,
            });
        }
    }
    Ok(LogTicks {
        density: LogDensity::Every(400),
        values: within(&[1], 400),
    })
}

/// `text` without a minus sign when everything after it is zero or the point:
/// Rust writes `-0.0` for a negative zero, which no axis should show.
fn without_negative_zero(text: String) -> String {
    if text.starts_with('-') && text[1..].chars().all(|c| c == '0' || c == '.') {
        text[1..].to_string()
    } else {
        text
    }
}

/// A linear graduation's text: as many decimals as the step needs, never "-0".
pub fn label(value: f64, step: f64) -> String {
    let decimals = decimals_of(step);
    without_negative_zero(format!("{value:.decimals$}"))
}

/// A logarithmic graduation's text: `0.01`, `2`, `5000`; `1e6` and `2e-5`
/// beyond.
pub fn log_label(value: f64) -> String {
    if value >= 1e6 || value < 1e-4 {
        return format!("{value:e}");
    }
    let decimals = (-value.log10().floor()).max(0.0) as usize;
    without_negative_zero(format!("{value:.decimals$}"))
}
