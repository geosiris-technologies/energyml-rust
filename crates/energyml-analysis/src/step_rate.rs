//! **Which steps of a step-rate injection test have settled, and the
//! injectivity index they give** - the diagnostic of a
//! PRODML/WITSML business view, under the criterion its review sets first.
//!
//! Nothing here reads a package or knows WITSML: the inputs are three slices of
//! one length - instants, rates, pressures - so what can be wrong in the
//! arithmetic is tested without a file, and whatever reads a `ChannelSet` hands
//! its columns over.
//!
//! # Units
//!
//! Instants are **hours** since any origin. A rate is in the caller's unit of
//! a volume per hour (`m3/h` for Lutetia), a pressure in the caller's unit
//! (`bar`). The tolerance on `dp/dt` is then in pressure units per hour, and
//! the injectivity index in rate units per pressure unit - `m3/h/bar`. This
//! module converts nothing: the caller passes one unit per column and names it.
//!
//! # A step is a run of one rate
//!
//! Consecutive samples of **exactly** the same rate make a step; the rank of a
//! step counts from 1. A measured rate that jitters around its set point makes
//! one-sample steps, each refused as too short - grouping by a tolerance on the
//! rate is a decision nobody has taken yet, and taking it here silently would
//! decide which samples a step owns.
//!
//! # A step is settled when its last samples say so
//!
//! A step is **settled** when the pressure of its last `k` samples moves slower
//! than `ε`: every one of the `k − 1` differences between consecutive samples of
//! that window has `|Δp / Δt| < ε`. `k` and `ε` are **declared by the caller**
//! ([`Settle`] has no default): a threshold tuned on Lutetia, whose synthetic
//! pressure relaxes exponentially and is built to settle, would make every real
//! test look like it. A step that has no settled window is refused **by its
//! rank** ([`StepRateError::NotSettled`], [`StepRateError::TooShort`],
//! [`StepRateError::NanInWindow`]) - never read at its last sample as if it
//! had settled.
//!
//! The pressure a settled step reports is its **last** sample: the window
//! guarantees it no longer moves by more than `ε` per hour, and the end-of-step
//! pressure is what step-rate test analyses read (Singh, Agarwal & Krase 1987,
//! *Systematic Design and Analysis of Step-Rate Tests to Determine Formation
//! Parting Pressure*, SPE 16798).
//!
//! TODO(reference): verify - cited from memory; no copy was available (title, authors, SPE number unchecked). The mean of the window would weight its earlier, less settled
//! samples.
//!
//! A pressure `NaN` outside every window is a hole in the record and changes
//! nothing; inside a window it refuses that step - a window with a hole cannot
//! be called settled.
//!
//! # The index is the inverse slope of Δp against q
//!
//! With `Δp = p_settled − p_static`, steady radial flow makes Δp proportional to
//! the rate: `Δp = q / II`. The index is read as `1 / slope` of the
//! least-squares line of Δp against q over the settled steps, and the fit is
//! returned whole - slope, intercept, residuals, R² - so the front end draws the
//! line it was read from. The regression, and not `q / Δp` step by step,
//! because an error on the static pressure shifts every Δp by the same amount:
//! the intercept absorbs it and the slope does not see it, where each ratio
//! would carry it. The ratios are returned beside the fit
//! ([`Injectivity::ratios`]): when they drift from step to step while the fit
//! stays straight, the static pressure is off; when the points bend away from
//! the line, the well is not the linear injector the index assumes (a fracture
//! opening, a turbulent skin).
//!
//! Two settled steps are the least a line needs - and two points always lie on
//! their line, so an R² of 1 from two steps is no evidence;
//! [`Fit::residuals`] says how many points there are. `InjectionFlowTest` has
//! no field for an index: the index is computed and shown, never compared
//! with a value the file would declare.
//!
//! Neither the settled criterion nor the index is in the Energistics guides:
//! they are textbook well-test practice, cited as such.

/// The series of a step-rate test, checked: instants, rates and pressures of
/// one length, instants finite and strictly increasing, rates finite.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Series<'a> {
    hours: &'a [f64],
    rates: &'a [f64],
    pressures: &'a [f64],
}

/// What a series cannot be, by name. Ranks count from 1.
#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum SeriesError {
    #[error("{hours} instant(s), {rates} rate(s), {pressures} pressure(s): one of each per sample")]
    MismatchedLengths {
        hours: usize,
        rates: usize,
        pressures: usize,
    },
    #[error("{given} sample(s) where {needed} are needed")]
    TooFewSamples { given: usize, needed: usize },
    #[error("sample {rank}: the {what} is not a finite number ({given})")]
    NotFinite {
        rank: usize,
        what: &'static str,
        given: f64,
    },
    #[error(
        "sample {rank} ({hours} h) does not come after the one before it ({before} h): time runs forward, and two samples at one instant have no rate of change between them"
    )]
    NotIncreasing { rank: usize, hours: f64, before: f64 },
}

impl<'a> Series<'a> {
    /// The series, or why not. A `NaN` pressure is accepted here - a hole in
    /// the record - and judged by whoever reads it.
    pub fn new(hours: &'a [f64], rates: &'a [f64], pressures: &'a [f64]) -> Result<Series<'a>, SeriesError> {
        if hours.len() != rates.len() || hours.len() != pressures.len() {
            return Err(SeriesError::MismatchedLengths {
                hours: hours.len(),
                rates: rates.len(),
                pressures: pressures.len(),
            });
        }
        if hours.is_empty() {
            return Err(SeriesError::TooFewSamples { given: 0, needed: 1 });
        }
        for (at, (&hour, &rate)) in hours.iter().zip(rates).enumerate() {
            if !hour.is_finite() {
                return Err(SeriesError::NotFinite {
                    rank: at + 1,
                    what: "instant",
                    given: hour,
                });
            }
            if !rate.is_finite() {
                return Err(SeriesError::NotFinite {
                    rank: at + 1,
                    what: "rate",
                    given: rate,
                });
            }
            if at > 0 && hour <= hours[at - 1] {
                return Err(SeriesError::NotIncreasing {
                    rank: at + 1,
                    hours: hour,
                    before: hours[at - 1],
                });
            }
        }
        Ok(Series {
            hours,
            rates,
            pressures,
        })
    }

    /// The instants, hours.
    pub fn hours(&self) -> &'a [f64] {
        self.hours
    }

    /// The rates, one per instant.
    pub fn rates(&self) -> &'a [f64] {
        self.rates
    }

    /// The pressures, one per instant - `NaN` where the record has a hole.
    pub fn pressures(&self) -> &'a [f64] {
        self.pressures
    }

    /// How many samples.
    pub fn len(&self) -> usize {
        self.hours.len()
    }

    /// Never: [`Series::new`] refuses an empty series.
    pub fn is_empty(&self) -> bool {
        self.hours.is_empty()
    }
}

/// When a step counts as settled: its last `window` samples move slower than
/// `tolerance`. Both declared by the caller - there is no default.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settle {
    /// `k`, how many of a step's last samples must be still - at least 2, one
    /// sample having no rate of change.
    pub window: usize,
    /// `ε`, the bound on `|dp/dt|`, in pressure units per hour.
    pub tolerance: f64,
}

/// What a step-rate analysis refuses, by name. Ranks count from 1.
#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum StepRateError {
    #[error(transparent)]
    Series(#[from] SeriesError),
    #[error("a settled window is at least 2 samples - one sample has no rate of change; {given} declared")]
    BadWindow { given: usize },
    #[error("the bound on |dp/dt| is a finite number > 0, not {given}")]
    BadTolerance { given: f64 },
    #[error("the static pressure is a finite number, not {given}")]
    BadStaticPressure { given: f64 },

    // -- one step ------------------------------------------------------------
    #[error(
        "step {rank} holds {samples} sample(s) where the window declared asks {window}: too short to say it settled"
    )]
    TooShort {
        rank: usize,
        samples: usize,
        window: usize,
    },
    #[error(
        "step {rank}: sample {sample} of its window has no finite pressure (NaN or infinite) - a window with a hole cannot be called settled"
    )]
    NanInWindow { rank: usize, sample: usize },
    #[error(
        "step {rank} has not settled: |dp/dt| reaches {steepest} in its last {window} samples, where {tolerance} was declared"
    )]
    NotSettled {
        rank: usize,
        steepest: f64,
        window: usize,
        tolerance: f64,
    },

    // -- the index -----------------------------------------------------------
    #[error("{settled} settled step(s) where a line needs 2 - steps set aside: {set_aside:?}")]
    TooFewSettled { settled: usize, set_aside: Vec<usize> },
    #[error("every settled step is at the rate {rate}: points at one rate have no slope")]
    OneRate { rate: f64 },
    #[error("the pressure does not rise with the rate (slope {slope}): no injectivity index is its inverse")]
    NoRise { slope: f64 },
    #[error("step {rank} is at a rate of zero: q / Δp would read 0 / {delta_p}, no index")]
    ZeroRate { rank: usize, delta_p: f64 },
    #[error("step {rank} settled at the static pressure: q / Δp would divide {rate} by zero")]
    NoPressureRise { rank: usize, rate: f64 },
}

/// A step: a run of samples at one rate, and whether it settled.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    /// From 1, in the order the steps are held.
    pub rank: usize,
    /// Its rate.
    pub rate: f64,
    /// The index of its first sample in the series.
    pub first: usize,
    /// How many samples it holds.
    pub samples: usize,
    /// Its settled pressure, or why it has none.
    pub verdict: Result<Settled, StepRateError>,
}

/// What a settled step reports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settled {
    /// The pressure of its last sample.
    pub pressure: f64,
    /// The instant of that sample, hours.
    pub hours: f64,
    /// The largest `|dp/dt|` of its window, pressure units per hour - below
    /// the tolerance declared.
    pub steepest: f64,
}

impl Settle {
    fn checked(&self) -> Result<(), StepRateError> {
        if self.window < 2 {
            return Err(StepRateError::BadWindow { given: self.window });
        }
        if !(self.tolerance.is_finite() && self.tolerance > 0.0) {
            return Err(StepRateError::BadTolerance {
                given: self.tolerance,
            });
        }
        Ok(())
    }
}

/// The steps of a series, each with its verdict under `settle`.
pub fn steps(series: &Series<'_>, settle: &Settle) -> Result<Vec<Step>, StepRateError> {
    settle.checked()?;
    let (hours, rates, pressures) = (series.hours, series.rates, series.pressures);
    let mut found = Vec::new();
    let mut first = 0;
    while first < rates.len() {
        let rate = rates[first];
        let mut end = first + 1;
        while end < rates.len() && rates[end] == rate {
            end += 1;
        }
        let rank = found.len() + 1;
        let samples = end - first;
        let verdict = if samples < settle.window {
            Err(StepRateError::TooShort {
                rank,
                samples,
                window: settle.window,
            })
        } else {
            let from = end - settle.window;
            match (from..end).find(|&at| !pressures[at].is_finite()) {
                Some(at) => Err(StepRateError::NanInWindow { rank, sample: at + 1 }),
                None => {
                    let steepest = (from + 1..end)
                        .map(|at| ((pressures[at] - pressures[at - 1]) / (hours[at] - hours[at - 1])).abs())
                        .fold(0.0_f64, f64::max);
                    if steepest >= settle.tolerance {
                        Err(StepRateError::NotSettled {
                            rank,
                            steepest,
                            window: settle.window,
                            tolerance: settle.tolerance,
                        })
                    } else {
                        Ok(Settled {
                            pressure: pressures[end - 1],
                            hours: hours[end - 1],
                            steepest,
                        })
                    }
                }
            }
        };
        found.push(Step {
            rank,
            rate,
            first,
            samples,
            verdict,
        });
        first = end;
    }
    Ok(found)
}

/// A least-squares line `y = intercept + slope · x`.
#[derive(Debug, Clone, PartialEq)]
pub struct Fit {
    /// Its slope - pressure units per rate unit for Δp against q.
    pub slope: f64,
    /// Its value at `x = 0`.
    pub intercept: f64,
    /// `y − (intercept + slope · x)` for each point, in their order - as many
    /// as the points the line was fitted to.
    pub residuals: Vec<f64>,
    /// `1 − Σ residual² / Σ (y − ȳ)²` - 1 by construction through two points.
    pub r_squared: f64,
}

/// One settled step as the line sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// The step's rank.
    pub rank: usize,
    /// Its rate.
    pub rate: f64,
    /// Its settled pressure minus the static pressure.
    pub delta_p: f64,
}

/// `q / Δp` of one settled step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ratio {
    /// The step's rank.
    pub rank: usize,
    /// Its rate over its pressure rise - rate units per pressure unit.
    pub index: f64,
}

/// The injectivity index of a test, and what it was read from.
#[derive(Debug, Clone, PartialEq)]
pub struct Injectivity {
    /// `1 / slope` of Δp against q - rate units per pressure unit (`m3/h/bar`).
    pub index: f64,
    /// The line of Δp against q.
    pub fit: Fit,
    /// The settled steps the line was fitted to, in their order.
    pub points: Vec<Point>,
    /// The steps that did not settle, each refused by its rank.
    pub set_aside: Vec<StepRateError>,
    /// `q / Δp` of each settled step, or why it has none (a rate of zero, no
    /// pressure rise) - to compare with the index, never used for it.
    pub ratios: Vec<Result<Ratio, StepRateError>>,
}

/// The injectivity index of a step-rate test: the settled steps under
/// `settle`, then `1 / slope` of their `p − static_pressure` against their rate.
pub fn injectivity(
    series: &Series<'_>,
    settle: &Settle,
    static_pressure: f64,
) -> Result<Injectivity, StepRateError> {
    if !static_pressure.is_finite() {
        return Err(StepRateError::BadStaticPressure {
            given: static_pressure,
        });
    }
    let mut points = Vec::new();
    let mut set_aside = Vec::new();
    for step in steps(series, settle)? {
        match step.verdict {
            Ok(settled) => points.push(Point {
                rank: step.rank,
                rate: step.rate,
                delta_p: settled.pressure - static_pressure,
            }),
            Err(why) => set_aside.push(why),
        }
    }
    if points.len() < 2 {
        let ranks = set_aside.iter().filter_map(rank_of).collect();
        return Err(StepRateError::TooFewSettled {
            settled: points.len(),
            set_aside: ranks,
        });
    }
    let x: Vec<f64> = points.iter().map(|one| one.rate).collect();
    let y: Vec<f64> = points.iter().map(|one| one.delta_p).collect();
    let fit = least_squares(&x, &y).ok_or(StepRateError::OneRate { rate: x[0] })?;
    if fit.slope.is_nan() || fit.slope <= 0.0 {
        return Err(StepRateError::NoRise { slope: fit.slope });
    }
    let ratios = points
        .iter()
        .map(|one| {
            if one.rate == 0.0 {
                Err(StepRateError::ZeroRate {
                    rank: one.rank,
                    delta_p: one.delta_p,
                })
            } else if one.delta_p == 0.0 {
                Err(StepRateError::NoPressureRise {
                    rank: one.rank,
                    rate: one.rate,
                })
            } else {
                Ok(Ratio {
                    rank: one.rank,
                    index: one.rate / one.delta_p,
                })
            }
        })
        .collect();
    Ok(Injectivity {
        index: 1.0 / fit.slope,
        fit,
        points,
        set_aside,
        ratios,
    })
}

/// The rank a per-step refusal names.
fn rank_of(why: &StepRateError) -> Option<usize> {
    match why {
        StepRateError::TooShort { rank, .. }
        | StepRateError::NanInWindow { rank, .. }
        | StepRateError::NotSettled { rank, .. } => Some(*rank),
        _ => None,
    }
}

/// The least-squares line through `(x, y)`, or `None` when every `x` is the
/// same - a vertical cloud has no slope. At least two points, finite.
pub(crate) fn least_squares(x: &[f64], y: &[f64]) -> Option<Fit> {
    let n = x.len() as f64;
    let (mean_x, mean_y) = (x.iter().sum::<f64>() / n, y.iter().sum::<f64>() / n);
    let sxx: f64 = x.iter().map(|one| (one - mean_x).powi(2)).sum();
    if sxx == 0.0 {
        return None;
    }
    let sxy: f64 = x.iter().zip(y).map(|(a, b)| (a - mean_x) * (b - mean_y)).sum();
    let slope = sxy / sxx;
    let intercept = mean_y - slope * mean_x;
    let residuals: Vec<f64> = x
        .iter()
        .zip(y)
        .map(|(a, b)| b - (intercept + slope * a))
        .collect();
    let ss_res: f64 = residuals.iter().map(|one| one * one).sum();
    let ss_tot: f64 = y.iter().map(|one| (one - mean_y).powi(2)).sum();
    // Every y equal: a flat line fits exactly; its slope is 0 and the caller
    // refuses it - R² is then 1 by the same convention as two points.
    let r_squared = if ss_tot == 0.0 { 1.0 } else { 1.0 - ss_res / ss_tot };
    Some(Fit {
        slope,
        intercept,
        residuals,
        r_squared,
    })
}
