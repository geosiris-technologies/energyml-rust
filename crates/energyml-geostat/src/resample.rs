//! Resampling a polyline, and the distance from a point to one - the two
//! small tools a clean remesh near a fault needs.
//!
//! The need: near faults the measurements are always noisy and uncertain, so it
//! is better to remesh cleanly, ideally with a mesh that has the same number of
//! edges on each side at the fault lips. Both come down to this:
//! drop the samples within a buffer of the trace, and give the trace vertices
//! at a regular spacing. A constrained triangulation then meets the trace with
//! exactly those vertices, on both sides - the same count by construction.

/// The polyline resampled at `spacing`, by arc length **in plan view**, ends
/// kept; z follows the line.
///
/// Plan view and not 3D, and it is the point: two horizons cut by the same
/// fault carry traces with the same (x, y) and different z. Measured in plan,
/// the same spacing gives them the **same nodes** along the fault - which is
/// what a volume mesh between them will need. Measured in 3D, the steeper
/// trace would come out sparser, and nothing would line up.
///
/// The last interval is whatever is left, never stretched: a trace of 10 at
/// a spacing of 3 gives nodes at 0, 3, 6, 9, 10 - the geometry is the trace's,
/// not the spacing's. Refused by name for a non-positive spacing; a line of
/// fewer than two positions comes back as it is.
pub fn resample(line: &[[f64; 3]], spacing: f64) -> Result<Vec<[f64; 3]>, String> {
    if !(spacing.is_finite() && spacing > 0.0) {
        return Err(format!("the spacing must be a finite number > 0, not {spacing}"));
    }
    if line.len() < 2 {
        return Ok(line.to_vec());
    }
    let mut out = vec![line[0]];
    // Distance walked since the last emitted node.
    let mut since = 0.0;
    for pair in line.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let length = (b[0] - a[0]).hypot(b[1] - a[1]);
        if length <= 0.0 {
            continue;
        }
        let mut at = spacing - since;
        while at < length {
            let t = at / length;
            out.push([
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t,
                a[2] + (b[2] - a[2]) * t,
            ]);
            at += spacing;
        }
        since = length - (at - spacing);
    }
    // The last vertex is the trace's own end - kept unless the walk landed on
    // it exactly, in which case it is already there.
    let last = *line.last().unwrap();
    if out.last() != Some(&last) {
        out.push(last);
    }
    Ok(out)
}

/// The plan-view distance from `p` to the nearest piece of any polyline.
pub fn plan_distance(p: [f64; 2], lines: &[Vec<[f64; 3]>]) -> f64 {
    let mut best = f64::INFINITY;
    for line in lines {
        if line.len() == 1 {
            best = best.min((p[0] - line[0][0]).hypot(p[1] - line[0][1]));
        }
        for pair in line.windows(2) {
            let (a, b) = ([pair[0][0], pair[0][1]], [pair[1][0], pair[1][1]]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len2 = dx * dx + dy * dy;
            let t = if len2 > 0.0 {
                (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let q = [a[0] + dx * t, a[1] + dy * t];
            best = best.min((p[0] - q[0]).hypot(p[1] - q[1]));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_resampled_by_arc_length_and_keeps_its_ends() {
        let line = vec![[0.0, 0.0, 0.0], [10.0, 0.0, 5.0]];
        let out = resample(&line, 3.0).unwrap();
        let xs: Vec<f64> = out.iter().map(|p| p[0]).collect();
        assert_eq!(xs, vec![0.0, 3.0, 6.0, 9.0, 10.0]);
        // z follows the line - and the spacing was measured in plan: in 3D the
        // line is 11.18 long, and the nodes would not sit on 3, 6, 9.
        assert!((out[1][2] - 1.5).abs() < 1e-12);
    }

    /// The spacing carries across a bend: the walk does not restart at every
    /// vertex, or a bent trace would come out denser than a straight one.
    #[test]
    fn the_spacing_carries_across_a_bend() {
        let bent = vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 4.0, 0.0]];
        let out = resample(&bent, 3.0).unwrap();
        // Total length 6: nodes at 0, 3 (one past the bend), 6.
        assert_eq!(out.len(), 3);
        assert!(
            (out[1][0] - 2.0).abs() < 1e-12 && (out[1][1] - 1.0).abs() < 1e-12,
            "{:?}",
            out[1]
        );
    }

    #[test]
    fn a_bad_spacing_is_refused_and_a_short_line_comes_back_as_it_is() {
        assert!(resample(&[[0.0; 3], [1.0; 3]], 0.0).is_err());
        assert_eq!(resample(&[[1.0; 3]], 2.0).unwrap(), vec![[1.0; 3]]);
    }

    #[test]
    fn the_distance_is_to_the_nearest_piece_not_to_a_vertex() {
        let lines = vec![vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]]];
        assert!((plan_distance([5.0, 2.0], &lines) - 2.0).abs() < 1e-12);
        assert!((plan_distance([12.0, 0.0], &lines) - 2.0).abs() < 1e-12);
        assert_eq!(plan_distance([0.0, 0.0], &[]), f64::INFINITY);
    }
}
