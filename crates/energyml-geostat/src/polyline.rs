//! The elevation of a polyline in `(s, z)` at an abscissa: the one place the
//! "find the segment holding `s`, interpolate `z`" scan is written.
//!
//! A *span* is a polyline whose abscissas `s` ascend. It has a `z` only where it
//! exists: **outside `[first s, last s]` the answer is `None`**, never the
//! nearest end's value. Rules shared by every caller:
//!
//! - a segment of no width (its two abscissas within [`f64::EPSILON`]) gives
//!   its first end's `z`, not a division by zero;
//! - at a vertex shared by two segments the **first** one answers (the two
//!   agree unless the span steps there, and a step is the caller's to state);
//! - a span of one point has a `z` only at its own abscissa.

/// A span's `z` at the abscissa `s`: `None` outside it.
pub fn z_at(span: &[[f64; 2]], s: f64) -> Option<f64> {
    let (first, last) = (span.first()?, span.last()?);
    if s < first[0] || s > last[0] {
        return None;
    }
    if span.len() == 1 {
        return Some(first[1]);
    }
    span.windows(2)
        .find(|w| s >= w[0][0] && s <= w[1][0])
        .map(|w| lerp_z(w[0], w[1], s))
}

/// The `z` at `s` of the first of `spans` that covers it.
pub fn z_in_any(spans: &[Vec<[f64; 2]>], s: f64) -> Option<f64> {
    spans.iter().find_map(|span| z_at(span, s))
}

/// A span's `z` at each of the ascending `steps`, `None` outside it: the same
/// answers as [`z_at`], in one pass over the span.
pub fn sample(span: &[[f64; 2]], steps: &[f64]) -> Vec<Option<f64>> {
    let (Some(first), Some(last)) = (span.first(), span.last()) else {
        return vec![None; steps.len()];
    };
    let mut j = 0;
    steps
        .iter()
        .map(|&s| {
            if s < first[0] || s > last[0] {
                return None;
            }
            if span.len() == 1 {
                return Some(first[1]);
            }
            while j + 2 < span.len() && span[j + 1][0] < s {
                j += 1;
            }
            Some(lerp_z(span[j], span[j + 1], s))
        })
        .collect()
}

/// [`z_in_any`] at each of the ascending `steps`: one pass over each span, not a
/// rescan of the spans from their start for every step.
pub fn sample_in_any(spans: &[Vec<[f64; 2]>], steps: &[f64]) -> Vec<Option<f64>> {
    let mut out: Vec<Option<f64>> = vec![None; steps.len()];
    for span in spans {
        let here = sample(span, steps);
        for (slot, z) in out.iter_mut().zip(here) {
            if slot.is_none() {
                *slot = z;
            }
        }
    }
    out
}

/// `z` at `s` on the segment `a b`; `a`'s `z` for a segment of no width.
fn lerp_z(a: [f64; 2], b: [f64; 2], s: f64) -> f64 {
    let ds = b[0] - a[0];
    if ds.abs() <= f64::EPSILON {
        a[1]
    } else {
        a[1] + (b[1] - a[1]) * (s - a[0]) / ds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPAN: [[f64; 2]; 3] = [[0.0, 10.0], [10.0, 20.0], [20.0, 20.0]];

    #[test]
    fn z_is_interpolated_inside_and_absent_outside() {
        assert_eq!(z_at(&SPAN, 5.0), Some(15.0));
        assert_eq!(z_at(&SPAN, 15.0), Some(20.0));
        assert_eq!(z_at(&SPAN, -0.1), None);
        assert_eq!(z_at(&SPAN, 20.1), None);
        assert_eq!(z_at(&[], 1.0), None);
    }

    #[test]
    fn a_step_answers_with_the_first_segment_and_a_point_only_at_itself() {
        let stepped = [[0.0, 1.0], [5.0, 1.0], [5.0, 9.0], [10.0, 9.0]];
        assert_eq!(z_at(&stepped, 5.0), Some(1.0));
        assert_eq!(z_at(&[[3.0, 7.0]], 3.0), Some(7.0));
        assert_eq!(z_at(&[[3.0, 7.0]], 4.0), None);
    }

    #[test]
    fn the_one_pass_sample_agrees_with_the_scan_everywhere() {
        let steps = [-1.0, 0.0, 2.5, 10.0, 12.0, 20.0, 21.0];
        let one_pass = sample(&SPAN, &steps);
        for (s, got) in steps.iter().zip(one_pass) {
            assert_eq!(got, z_at(&SPAN, *s), "at {s}");
        }
        // Break it: let `sample` step `j` past a vertex that is exactly `s` and it answers with the next segment, not the first.
    }

    #[test]
    fn the_one_pass_sample_in_any_agrees_with_the_first_span_that_covers() {
        // Two spans that overlap on 0.5..1.0: the first answers there.
        let spans = vec![vec![[0.0, 1.0], [1.0, 1.0]], vec![[0.5, 5.0], [3.0, 5.0]], vec![]];
        let steps = [-1.0, 0.0, 0.5, 0.7, 1.0, 1.5, 3.0, 3.5];
        for (s, got) in steps.iter().zip(sample_in_any(&spans, &steps)) {
            assert_eq!(got, z_in_any(&spans, *s), "at {s}");
        }
        // Break it: let a later span overwrite the answer of an earlier one
        // (drop the `is_none` test) and the overlap answers 5, not 1.
    }

    #[test]
    fn the_first_span_that_covers_the_abscissa_answers() {
        let spans = vec![vec![[0.0, 1.0], [1.0, 1.0]], vec![[0.5, 5.0], [3.0, 5.0]]];
        assert_eq!(z_in_any(&spans, 0.7), Some(1.0));
        assert_eq!(z_in_any(&spans, 2.0), Some(5.0));
        assert_eq!(z_in_any(&spans, 4.0), None);
    }
}
