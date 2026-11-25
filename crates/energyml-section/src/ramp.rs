//! The colour ramp of a legend: its steps, its graduations, and the legend a face
//! gives a section.
//!
//! **One arithmetic for every legend of a ramp.** The legend of the curtain along a
//! well ([`crate::along_well`]) and the ramp legend of a section ([`RampLegend`], drawn
//! by [`crate::layout`]) cut the scale in the same number of steps, place a step's value
//! at its middle and graduate the ramp with the same round numbers (linear) or decades
//! (logarithmic): what changes is only what is drawn with them, a display list there, the
//! section's own shapes here. The colours come from the caller - a ramp of
//! `energyml_domain::palettes`, or the map the file gives the property, through the
//! `value_colours` door of `energyml-views` - as a closure from a value to a colour:
//! this crate never decides a colour.

use energyml_domain::color::Rgba;
use energyml_domain::stats::scale::{Fraction, Scale, ScaleKind};
use energyml_sheet::draw::ticks::{label, log_label, log_ticks, nice_ticks};

use crate::model::RampLegend;

/// How many steps a ramp is cut in.
pub const STEPS: usize = 32;

/// The value a step of the ramp stands for: the one at its middle on the scale.
pub fn value_at_step(scale: &Scale, step: usize) -> f64 {
    scale.value_at((step as f64 + 0.5) / STEPS as f64)
}

/// The graduations of a ramp `ramp_px` pixels long: each as the fraction of the ramp it sits
/// at (0 at the lower bound, 1 at the upper) and its text. Round numbers on a linear scale,
/// decades on a logarithmic one; a graduation the scale does not place is left out.
pub fn graduations(scale: &Scale, ramp_px: f64) -> Vec<(f64, String)> {
    let target = (ramp_px / 40.0).max(1.0);
    let (min, max) = (scale.min(), scale.max());
    let named: Vec<(f64, String)> = match scale.kind() {
        ScaleKind::Linear => nice_ticks(min, max, target)
            .map(|ticks| ticks.values.iter().map(|v| (*v, label(*v, ticks.step))).collect())
            .unwrap_or_default(),
        ScaleKind::Log10 => log_ticks(min, max, target)
            .map(|ticks| ticks.values.iter().map(|v| (*v, log_label(*v))).collect())
            .unwrap_or_default(),
    };
    named
        .into_iter()
        .filter_map(|(value, name)| match scale.fraction(value) {
            Fraction::At(fraction) => Some((fraction, name)),
            _ => None,
        })
        .collect()
}

/// The legend of the ramp `colour` paints on `scale`: each of the [`STEPS`] steps sampled once,
/// from the lower bound up. `title` is the property's name and `uom` its unit.
pub fn sampled(
    title: &str,
    uom: Option<&str>,
    scale: &Scale,
    colour: &dyn Fn(f64) -> Option<[u8; 4]>,
) -> RampLegend {
    RampLegend {
        title: title.to_string(),
        uom: uom.map(str::to_string),
        log: scale.kind() == ScaleKind::Log10,
        min: scale.min(),
        max: scale.max(),
        colours: (0..STEPS)
            .map(|step| {
                colour(value_at_step(scale, step)).map(|bytes| Rgba::from_bytes(bytes).to_hex_compact())
            })
            .collect(),
    }
}

/// The scale a ramp legend graduates: its bounds, linear or logarithmic.
///
/// # Errors
///
/// Bounds that do not make a scale (reversed, equal, not finite, or not positive on a log scale),
/// in the scale's own words.
pub fn scale_of(legend: &RampLegend) -> Result<Scale, String> {
    let kind = if legend.log {
        ScaleKind::Log10
    } else {
        ScaleKind::Linear
    };
    Scale::new(kind, legend.min, legend.max, false).map_err(|why| why.to_string())
}
