//! Graduations in round numbers: `energyml_sheet::draw::ticks`, the one rule every
//! drawing shares. This module only keeps the section's two call shapes.

use energyml_sheet::draw::ticks;

/// The tick values in `[min, max]` and their step. An empty or reversed window
/// gives no tick, as before. A target below one graduation is taken as one: the
/// former rule fell back to a step of 1 there, which on a frame narrower than
/// 90 px could ask for millions of ticks.
pub fn values(min: f64, max: f64, target: f64) -> (f64, Vec<f64>) {
    let range = max - min;
    if !(range.is_finite() && range > 0.0) {
        return (1.0, Vec::new());
    }
    match ticks::nice_ticks(min, max, target.max(1.0)) {
        Ok(t) => (t.step, t.values),
        Err(_) => (1.0, Vec::new()),
    }
}

/// A tick's text: as many decimals as the step needs, no "-0".
pub fn label(value: f64, step: f64) -> String {
    ticks::label(value, step)
}
