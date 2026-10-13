//! Graduations in round numbers.

/// The step of a graduation: 1, 2 or 5 times a power of ten, the smallest one
/// that gives no more than about `target` ticks over `range`.
pub fn step(range: f64, target: f64) -> f64 {
    if !(range.is_finite() && range > 0.0 && target >= 1.0) {
        return 1.0;
    }
    let raw = range / target;
    let magnitude = 10f64.powf(raw.log10().floor());
    let norm = raw / magnitude;
    let nice = if norm <= 1.0 {
        1.0
    } else if norm <= 2.0 {
        2.0
    } else if norm <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * magnitude
}

/// The tick values in `[min, max]`, multiples of the step — computed as
/// `k × step`, never accumulated, so a long axis does not drift.
pub fn values(min: f64, max: f64, target: f64) -> (f64, Vec<f64>) {
    let range = max - min;
    let step = step(range, target);
    if !(range.is_finite() && range > 0.0) {
        return (step, Vec::new());
    }
    let first = (min / step - 1e-9).ceil() as i64;
    let last = (max / step + 1e-9).floor() as i64;
    (step, (first..=last).map(|k| k as f64 * step).collect())
}

/// A tick's text: as many decimals as the step needs, no "-0".
pub fn label(value: f64, step: f64) -> String {
    let decimals = if step >= 1.0 { 0 } else { (-step.log10().floor()) as usize };
    let text = format!("{value:.decimals$}");
    if text.starts_with('-') && text[1..].chars().all(|c| c == '0' || c == '.') { text[1..].to_string() } else { text }
}
