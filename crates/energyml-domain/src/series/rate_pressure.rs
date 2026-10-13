//! A flow test's rate and pressure, **one column each on one time axis**, in
//! the units `energyml_analysis::step_rate` and `energyml_analysis::hall`
//! compute in: instants in hours since the first sample, a rate in a volume
//! per hour, a pressure in the channel's own unit.
//!
//! The analyses take three slices of one length and read no package
//! (`step_rate.rs`, *Nothing here reads a package*). What a flow test holds is
//! [`super::flow_test::Measured`] series - one channel each, against the
//! test's time channel. This is the one place that turns the second into the
//! first, so the front end, the CLI and Python hand the analyses the same columns.
//!
//! # Which channels
//!
//! PRODML Technical Usage Guide v2.3 §13.6 p. 118 groups a test's data as
//! `MeasuredFlowRate`, `MeasuredPressure` and `OtherData`. The rate is the
//! `FlowChannel` of the **one** `MeasuredFlowRate` item, the pressure the
//! `PressureChannel` of the **one** `MeasuredPressure` item (a
//! `PressureDerivativeChannel` beside it is not a pressure). None is
//! [`Refused::Missing`]; several is [`Refused::SeveralMatch`] - which of two
//! pressures a diagnostic reads is stated nowhere, and the first is a guess.
//!
//! # One time axis, never two made into one
//!
//! Both series are indexed by their item's `TimeChannel`. When the two indexes
//! do not hold the same instants, pairing a rate with a pressure means
//! interpolating one of them - [`Refused::DifferentInstants`]. `injection-test`
//! writes one `TIME` channel for both, which is the case this reads.
//!
//! # Units
//!
//! - **Time.** A `date time` index is Unix seconds; an `elapsed time` one is in
//!   its unit, converted through [`crate::units::seconds`] or refused. Hours
//!   count from the first sample.
//! - **Rate.** Its unit must read `<volume>/<time>` with the time a unit
//!   [`crate::units`] knows - `m3/h`, `m3/d`, `bbl/d` - the rule
//!   [`super::production`] already applies. It is brought to **per hour**,
//!   and its volume kept as written: this build has no table of volumes and
//!   converts nothing it cannot check.
//! - **Pressure.** Kept in its unit, which must be a pressure
//!   [`crate::units`] knows: a channel in `degC` named as the pressure is a
//!   test pointing at the wrong channel, and a diagnostic of it would be a
//!   line through temperatures.
//!
//! A sample that is not [`Quality::Good`] becomes `NaN` in its column: the
//! analyses refuse a `NaN` rate by its rank and treat a `NaN` pressure as a
//! hole - never a value.

use super::flow_test::Measured;
use super::{Channel, IndexKind, Quality, Refused};

/// The two columns a step-rate analysis reads, and what they are in.
#[derive(Clone, Debug, PartialEq)]
pub struct RateAndPressure {
    /// Hours since the first sample, one per sample.
    pub hours: Vec<f64>,
    /// The rate, in [`RateAndPressure::rate_uom`]; `NaN` where not good.
    pub rates: Vec<f64>,
    /// The pressure, in [`RateAndPressure::pressure_uom`]; `NaN` where not good.
    pub pressures: Vec<f64>,
    /// `<volume>/h` - the rate's volume as written, per hour.
    pub rate_uom: String,
    /// The rate's volume as written - `m3`.
    pub volume_uom: String,
    /// The pressure's unit as written - `bar`.
    pub pressure_uom: String,
    /// The first sample's index value, in the index's own terms (Unix seconds
    /// for a `date time` index) - what "hour 0" is.
    pub origin: f64,
    /// The rate channel's mnemonic.
    pub rate_channel: String,
    /// The pressure channel's mnemonic.
    pub pressure_channel: String,
}

/// The rate and the pressure a flow test measured, on one axis of hours.
pub fn rate_and_pressure(measured: &[Measured]) -> Result<RateAndPressure, Refused> {
    let rate = one_of(measured, "MeasuredFlowRate", "FlowChannel")?;
    let pressure = one_of(measured, "MeasuredPressure", "PressureChannel")?;

    let (rate_index, pressure_index) = (&rate.series.index, &pressure.series.index);
    if rate_index.values != pressure_index.values {
        return Err(Refused::DifferentInstants {
            first: rate_index.mnemonic.clone(),
            second: pressure_index.mnemonic.clone(),
        });
    }
    let hours = hours_of(&rate.series.index)?;

    let rate_channel = only_channel(rate)?;
    let pressure_channel = only_channel(pressure)?;

    let rate_uom = rate_channel.uom.clone().ok_or_else(|| Refused::Missing {
        what: format!("the unit of the flow rate `{}`", rate_channel.mnemonic),
    })?;
    let (volume, per) = rate_uom.rsplit_once('/').ok_or_else(|| Refused::Unit {
        what: format!("the flow rate `{}`", rate_channel.mnemonic),
        uom: rate_uom.clone(),
        why: "a rate is read as <volume>/<time>".to_string(),
    })?;
    let seconds_per = crate::units::seconds(1.0, per).map_err(|why| Refused::Unit {
        what: format!("the flow rate `{}`", rate_channel.mnemonic),
        uom: rate_uom.clone(),
        why: format!("its time is not one this build knows: {why}"),
    })?;
    if volume.trim().is_empty() {
        return Err(Refused::Unit {
            what: format!("the flow rate `{}`", rate_channel.mnemonic),
            uom: rate_uom.clone(),
            why: "a rate is read as <volume>/<time>, and this one names no volume".to_string(),
        });
    }
    // One `per` is `seconds_per` seconds; an hour holds 3600 / seconds_per of them.
    let to_hourly = 3600.0 / seconds_per;

    let pressure_uom = pressure_channel.uom.clone().ok_or_else(|| Refused::Missing {
        what: format!("the unit of the pressure `{}`", pressure_channel.mnemonic),
    })?;
    if crate::units::Dimension::of(&pressure_uom) != Some(crate::units::Dimension::Pressure) {
        return Err(Refused::Unit {
            what: format!("the pressure `{}`", pressure_channel.mnemonic),
            uom: pressure_uom,
            why: "not a pressure unit this build knows - a test pointing at the wrong channel draws a line through the wrong quantity".to_string(),
        });
    }

    let rates = good_numbers(rate_channel)?
        .into_iter()
        .map(|value| value * to_hourly)
        .collect();
    let pressures = good_numbers(pressure_channel)?;

    Ok(RateAndPressure {
        hours,
        rates,
        pressures,
        rate_uom: format!("{volume}/h"),
        volume_uom: volume.to_string(),
        pressure_uom,
        origin: rate_index.values.first().copied().unwrap_or(f64::NAN),
        rate_channel: rate_channel.mnemonic.clone(),
        pressure_channel: pressure_channel.mnemonic.clone(),
    })
}

/// The one item of `group` naming its channel by `role`.
fn one_of<'a>(measured: &'a [Measured], group: &str, role: &str) -> Result<&'a Measured, Refused> {
    let found: Vec<&Measured> = measured
        .iter()
        .filter(|one| one.group == group && one.role == role)
        .collect();
    match found.as_slice() {
        [one] => Ok(one),
        [] => Err(Refused::Missing {
            what: format!("a {group} item with a {role}"),
        }),
        several => Err(Refused::SeveralMatch {
            what: "channel",
            names: vec![format!("{group}/{role}")],
            uuids: several
                .iter()
                .map(|one| {
                    one.series
                        .channels
                        .first()
                        .and_then(|c| c.uuid.clone())
                        .unwrap_or_default()
                })
                .collect(),
        }),
    }
}

/// The one channel of a measured series.
fn only_channel(measured: &Measured) -> Result<&Channel, Refused> {
    match measured.series.channels.as_slice() {
        [one] => Ok(one),
        _ => Err(Refused::Missing {
            what: format!("the one channel of {}/{}", measured.group, measured.role),
        }),
    }
}

/// The values of a numeric channel, `NaN` where the sample is not good.
fn good_numbers(channel: &Channel) -> Result<Vec<f64>, Refused> {
    let numbers = channel.numbers().ok_or(Refused::NotNumeric {
        channel: channel.mnemonic.clone(),
        family: match channel.values.family() {
            crate::values::Family::Boolean => "boolean",
            _ => "text",
        },
    })?;
    Ok(numbers
        .into_iter()
        .zip(&channel.quality)
        .map(|(value, quality)| {
            if *quality == Quality::Good {
                value
            } else {
                f64::NAN
            }
        })
        .collect())
}

/// Hours since the first sample of a time index.
fn hours_of(index: &super::Index) -> Result<Vec<f64>, Refused> {
    let seconds_each = match index.kind {
        IndexKind::DateTime => 1.0,
        IndexKind::ElapsedTime => {
            let uom = index.uom.clone().ok_or_else(|| Refused::Missing {
                what: format!("the unit of the time index `{}`", index.mnemonic),
            })?;
            crate::units::seconds(1.0, &uom).map_err(|why| Refused::Unit {
                what: format!("the time index `{}`", index.mnemonic),
                uom,
                why: format!("{why}"),
            })?
        }
        // `flow_test::read` builds only the two above; said all the same.
        other => {
            return Err(Refused::TimeChannelKind {
                channel: index.mnemonic.clone(),
                kind: other.wire().to_string(),
            });
        }
    };
    let origin = index.values.first().copied().unwrap_or(0.0);
    Ok(index
        .values
        .iter()
        .map(|value| (value - origin) * seconds_each / 3600.0)
        .collect())
}
