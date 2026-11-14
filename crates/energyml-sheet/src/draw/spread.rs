//! Labels along one axis, pushed apart without changing their order.
//!
//! One rule, the desktop's and the studio's: a label is moved only towards
//! larger values (down the page), because a label pushed back would sit before
//! one that comes before it, which reads as the wrong order.

use crate::draw::svg::DrawError;

/// Labels at these positions along one axis, pushed apart by at least `gap`
/// and only towards larger values: the position each label sits at, in the
/// order given.
///
/// Positions are visited in ascending order (equal positions keep the order
/// given); each sits at its own position or just past the previous label plus
/// `gap`, whichever is larger. A position that is not a finite number is not
/// placed: it comes back as `NaN` and moves nobody. Nothing is clamped: a label
/// pushed beyond the drawing area is returned as it is, and the caller compares
/// it to its own height and says so (truncation, a note).
///
/// # Errors
///
/// [`DrawError::BadGap`] when `gap` is not a finite number from 0.
pub fn spread(rows: &[f64], gap: f64) -> Result<Vec<f64>, DrawError> {
    if !(gap.is_finite() && gap >= 0.0) {
        return Err(DrawError::BadGap { gap });
    }
    let mut order: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].is_finite()).collect();
    // `sort_by` is stable: two equal positions keep the order they were given.
    order.sort_by(|&a, &b| rows[a].total_cmp(&rows[b]));
    let mut out = vec![f64::NAN; rows.len()];
    let mut floor = f64::NEG_INFINITY;
    for i in order {
        let y = rows[i].max(floor);
        out[i] = y;
        floor = y + gap;
    }
    Ok(out)
}
