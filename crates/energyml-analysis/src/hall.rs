//! **The Hall plot of an injection** — the cumulative pressure rise over time
//! against the cumulative volume injected, whose slope changing says the well
//! is plugging (steeper) or fracturing (flatter)
//! (a PRODML/WITSML business view).
//!
//! # Source — a textbook method, not an Energistics one
//!
//! Hall, H.N. (1963), *How to Analyze Waterflood Injection Well Performance*,
//! World Oil. No page is given.
//!
//! TODO(reference): verify — cited from memory; no copy was available (publication details and page unchecked). The PRODML Technical Usage Guide v2.3 does not name the
//! method (a full-text search finds no occurrence), and nothing in an `InjectionFlowTest` declares a Hall slope to
//! compare with.
//!
//! The argument, in the form this module relies on: under steady radial flow
//! the pressure rise is proportional to the rate, `Δp = q / II`, so
//! `∫ Δp dt = (1 / II) · ∫ q dt` — plotted one against the other, a line whose
//! slope is the inverse of the injectivity index. The same `1 / II` that
//! [`crate::step_rate::injectivity`] reads from the settled steps, which is
//! what a test can confront the two with.
//!
//! # What is integrated, and how
//!
//! `Δp = p − p_static` at each sample, `p_static` declared by the caller —
//! Hall's reservoir pressure. Both integrals are cumulated from the first
//! sample by the **trapezoid rule**: between samples `i − 1` and `i`,
//! `(f(i − 1) + f(i)) / 2 · (t(i) − t(i − 1))`. For a rate held in steps and
//! sampled at instants, an interval that straddles a change of rate is
//! credited half of each: the error is at most half the jump times that
//! interval, and it shrinks as the sampling tightens. Said, not corrected: a
//! rule that assumed where the jump falls would invent it.
//!
//! # Units
//!
//! Instants in **hours**, as in [`crate::step_rate`]. The volume is in the
//! rate's unit times hours — `m3` for a rate in `m3/h` — and the pressure-time
//! in the pressure's unit times hours — `bar·h`. Hall's own axes were in field
//! units and days; the unit changes the scale of the line, never its shape.
//!
//! # Refusals
//!
//! Every sample enters an integral, so a `NaN` pressure is refused by its rank
//! — integrating across a hole would draw a line the record does not hold.
//! Fewer than two samples make no interval.

use crate::step_rate::{Series, SeriesError};

/// What a Hall plot refuses, by name. Ranks count from 1.
#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum HallError {
    #[error(transparent)]
    Series(#[from] SeriesError),
    #[error("the static pressure is a finite number, not {given}")]
    BadStaticPressure { given: f64 },
    #[error("sample {rank} has no finite pressure ({given}): every sample enters the integral, and a hole would be drawn as a value")]
    NoPressure { rank: usize, given: f64 },
}

/// The two cumulative series of a Hall plot, one value per sample, both 0 at
/// the first.
#[derive(Debug, Clone, PartialEq)]
pub struct Hall {
    /// `∫ q dt` — the x axis, rate units × hours (`m3`).
    pub volume: Vec<f64>,
    /// `∫ (p − p_static) dt` — the y axis, pressure units × hours (`bar·h`).
    pub pressure_time: Vec<f64>,
}

/// The Hall plot of a series, against `static_pressure`.
pub fn hall(series: &Series<'_>, static_pressure: f64) -> Result<Hall, HallError> {
    if series.len() < 2 {
        return Err(SeriesError::TooFewSamples { given: series.len(), needed: 2 }.into());
    }
    if !static_pressure.is_finite() {
        return Err(HallError::BadStaticPressure { given: static_pressure });
    }
    let (hours, rates, pressures) = (series.hours(), series.rates(), series.pressures());
    if let Some(at) = pressures.iter().position(|one| !one.is_finite()) {
        return Err(HallError::NoPressure { rank: at + 1, given: pressures[at] });
    }
    let mut volume = Vec::with_capacity(hours.len());
    let mut pressure_time = Vec::with_capacity(hours.len());
    let (mut v, mut w) = (0.0, 0.0);
    volume.push(v);
    pressure_time.push(w);
    for at in 1..hours.len() {
        let dt = hours[at] - hours[at - 1];
        v += (rates[at - 1] + rates[at]) / 2.0 * dt;
        w += ((pressures[at - 1] - static_pressure) + (pressures[at] - static_pressure)) / 2.0 * dt;
        volume.push(v);
        pressure_time.push(w);
    }
    Ok(Hall { volume, pressure_time })
}
