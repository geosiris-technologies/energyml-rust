//! Densifying a polyline: **every vertex kept**, points added between them
//! (the "densify" edit).
//!
//! Not [`resample`](fn@crate::resample): that one walks the line at a spacing and
//! keeps only the two ends, so the inner vertices go and the corners are cut.
//! This one adds and never removes — the need being to cut a polyline up by
//! adding points — and both names are needed, or one will be believed when the
//! other was run.
//!
//! # Where the points go
//!
//! - **by count** — `n` points over the line's **whole length**, at 1/(n+1),
//!   2/(n+1)… of it. The vertices staying, the intervals are not equal; that is
//!   what is asked, and saying it spares the surprise;
//! - **by spacing** — a **station** every `every`, counted from the start like a
//!   chainage, and a station within `tolerance` of a vertex is not added: the
//!   example of the notes, no point at 5 m when there is one at 4.80 m with a
//!   tolerance of 0.25.
//!
//! And **on the segments, or on a curve through the vertices** (`smooth`): a
//! centripetal Catmull-Rom (Yuksel, Schaefer and Keyser, 2011), which neither
//! loops nor overshoots between two close vertices — a natural cubic
//! oscillates near a bend, and a smoothed line that bulges is worse than an
//! angular one. The vertices are on the curve, so they do not move.
//!
//! The length is measured **in 3D** (a trajectory, a trace picked in relief) or
//! **in plan** (a fault trace, where two horizons cut by the same fault must get
//! the same nodes — the reason `resample` measures in plan).
//!
//! A closed line is densified along its closing segment too; a line whose
//! closure nobody stated is **open** here — adding points on an edge the file
//! does not draw would invent a ring.
//!
//! TODO(reference): verify — the centripetal Catmull-Rom citation (Yuksel, Schaefer and Keyser, 2011) is given without title, venue or DOI; complete and check it.

/// Where the added points go along the line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Placement {
    /// This many points over the whole length.
    Count(usize),
    /// A station every `every`, from the start; none within `tolerance` of a vertex.
    Spacing {
        /// The distance between stations.
        every: f64,
        /// How close to a vertex a station may be before it is not added.
        tolerance: f64,
    },
}

/// How the length is measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Along {
    /// In 3D.
    Space,
    /// In plan: x and y alone.
    Plan,
}

/// A line densified: its points, and how many were added.
#[derive(Debug, Clone, PartialEq)]
pub struct Densified {
    /// The vertices, in order, with the added points between them.
    pub points: Vec<[f64; 3]>,
    /// How many points were added.
    pub added: usize,
}

fn length(a: [f64; 3], b: [f64; 3], along: Along) -> f64 {
    let (dx, dy, dz) = (b[0] - a[0], b[1] - a[1], b[2] - a[2]);
    match along {
        Along::Space => (dx * dx + dy * dy + dz * dz).sqrt(),
        Along::Plan => dx.hypot(dy),
    }
}

/// The point at fraction `t` of the way from `p1` to `p2`, on a centripetal
/// Catmull-Rom through `p0, p1, p2, p3` — Barry and Goldman's pyramid.
fn catmull_rom(p0: [f64; 3], p1: [f64; 3], p2: [f64; 3], p3: [f64; 3], t: f64) -> [f64; 3] {
    // Centripetal: knots spaced by the square root of the chord. A chord of
    // zero would make two knots equal and the pyramid divide by zero — kept
    // apart by a hair, which leaves that span straight.
    let knot = |a: [f64; 3], b: [f64; 3]| length(a, b, Along::Space).sqrt().max(1e-12);
    let t0 = 0.0;
    let t1 = t0 + knot(p0, p1);
    let t2 = t1 + knot(p1, p2);
    let t3 = t2 + knot(p2, p3);
    let u = t1 + (t2 - t1) * t;
    let mix = |a: [f64; 3], b: [f64; 3], ta: f64, tb: f64| {
        let w = (u - ta) / (tb - ta);
        [
            a[0] + (b[0] - a[0]) * w,
            a[1] + (b[1] - a[1]) * w,
            a[2] + (b[2] - a[2]) * w,
        ]
    };
    let a1 = mix(p0, p1, t0, t1);
    let a2 = mix(p1, p2, t1, t2);
    let a3 = mix(p2, p3, t2, t3);
    let b1 = mix(a1, a2, t0, t2);
    let b2 = mix(a2, a3, t1, t3);
    mix(b1, b2, t1, t2)
}

/// The line densified — `closed` draws its closing segment, `smooth` puts the
/// added points on a curve through the vertices.
///
/// Refused by name: a count of zero (nothing to add is a copy, not a
/// densification), a spacing that is not a positive number, a negative
/// tolerance. A line of fewer than two vertices comes back as it is.
pub fn densify(
    line: &[[f64; 3]],
    closed: bool,
    placement: Placement,
    along: Along,
    smooth: bool,
) -> Result<Densified, String> {
    match placement {
        Placement::Count(0) => return Err("the count must be at least 1 — adding none is a copy".to_string()),
        Placement::Spacing { every, tolerance } => {
            if !(every.is_finite() && every > 0.0) {
                return Err(format!("the spacing must be a finite number > 0, not {every}"));
            }
            if !(tolerance.is_finite() && tolerance >= 0.0) {
                return Err(format!(
                    "the tolerance must be a finite number ≥ 0, not {tolerance}"
                ));
            }
        }
        Placement::Count(_) => {}
    }
    let n = line.len();
    if n < 2 {
        return Ok(Densified {
            points: line.to_vec(),
            added: 0,
        });
    }
    // The segments, the closing one included for a ring.
    let segments = if closed { n } else { n - 1 };
    let at = |i: usize| line[i % n];
    // Where each vertex stands along the line.
    let mut starts = Vec::with_capacity(segments + 1);
    let mut total = 0.0;
    for i in 0..segments {
        starts.push(total);
        total += length(at(i), at(i + 1), along);
    }
    starts.push(total);
    if total <= 0.0 {
        return Ok(Densified {
            points: line.to_vec(),
            added: 0,
        });
    }
    // The abscissas of the points to add, in order.
    let mut wanted: Vec<f64> = Vec::new();
    match placement {
        Placement::Count(count) => {
            for k in 1..=count {
                wanted.push(total * k as f64 / (count + 1) as f64);
            }
        }
        Placement::Spacing { every, .. } => {
            let mut k = 1.0;
            while k * every < total {
                wanted.push(k * every);
                k += 1.0;
            }
        }
    }
    let tolerance = match placement {
        Placement::Spacing { tolerance, .. } => tolerance,
        // A point landing on a vertex is not a point added.
        Placement::Count(_) => total * 1e-12,
    };
    let neighbour = |i: isize| -> [f64; 3] {
        if closed {
            return at(i.rem_euclid(n as isize) as usize);
        }
        // Past an open end, the end mirrored: the curve leaves it straight.
        if i < 0 {
            let (a, b) = (line[0], line[1]);
            return [2.0 * a[0] - b[0], 2.0 * a[1] - b[1], 2.0 * a[2] - b[2]];
        }
        if i as usize >= n {
            let (a, b) = (line[n - 1], line[n - 2]);
            return [2.0 * a[0] - b[0], 2.0 * a[1] - b[1], 2.0 * a[2] - b[2]];
        }
        line[i as usize]
    };
    let mut points = Vec::with_capacity(n + wanted.len());
    let mut added = 0;
    let mut next = 0;
    for i in 0..segments {
        points.push(at(i));
        let (s0, s1) = (starts[i], starts[i + 1]);
        let span = s1 - s0;
        while next < wanted.len() && wanted[next] < s1 {
            let s = wanted[next];
            next += 1;
            if span <= 0.0 || s - s0 <= tolerance || s1 - s <= tolerance {
                continue;
            }
            let t = (s - s0) / span;
            let (a, b) = (at(i), at(i + 1));
            let p = if smooth {
                catmull_rom(neighbour(i as isize - 1), a, b, neighbour(i as isize + 2), t)
            } else {
                [
                    a[0] + (b[0] - a[0]) * t,
                    a[1] + (b[1] - a[1]) * t,
                    a[2] + (b[2] - a[2]) * t,
                ]
            };
            points.push(p);
            added += 1;
        }
    }
    // An open line ends on its last vertex; a ring's closing segment ends on
    // its first, which is already there.
    if !closed {
        points.push(line[n - 1]);
    }
    Ok(Densified { points, added })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() < 1e-9)
    }

    #[test]
    fn every_vertex_is_kept() {
        let line = [[0.0, 0.0, 0.0], [3.0, 0.0, 0.0], [3.0, 7.0, 0.0]];
        let out = densify(&line, false, Placement::Count(4), Along::Space, false).unwrap();
        for vertex in line {
            assert!(out.points.iter().any(|p| close(*p, vertex)), "{vertex:?} kept");
        }
        assert_eq!(out.added, 4);
        assert_eq!(out.points.len(), 7);
    }

    #[test]
    fn by_count_over_the_whole_length() {
        // Length 10, three points: at 2.5, 5, 7.5 — whatever the vertices.
        let line = [[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [10.0, 0.0, 0.0]];
        let out = densify(&line, false, Placement::Count(3), Along::Space, false).unwrap();
        let xs: Vec<f64> = out.points.iter().map(|p| p[0]).collect();
        assert_eq!(xs, vec![0.0, 2.5, 4.0, 5.0, 7.5, 10.0]);
    }

    #[test]
    fn by_spacing_stations_from_the_start_with_a_tolerance() {
        // The example of the notes: a vertex at 4.80, stations every 5 with a
        // tolerance of 0.25 — no station at 5, one at 10.
        let line = [[0.0, 0.0, 0.0], [4.8, 0.0, 0.0], [12.0, 0.0, 0.0]];
        let out = densify(
            &line,
            false,
            Placement::Spacing {
                every: 5.0,
                tolerance: 0.25,
            },
            Along::Space,
            false,
        )
        .unwrap();
        let xs: Vec<f64> = out.points.iter().map(|p| p[0]).collect();
        assert_eq!(xs, vec![0.0, 4.8, 10.0, 12.0]);
        // Without the tolerance, the station at 5 is added.
        let loose = densify(
            &line,
            false,
            Placement::Spacing {
                every: 5.0,
                tolerance: 0.0,
            },
            Along::Space,
            false,
        )
        .unwrap();
        assert_eq!(loose.added, 2);
    }

    #[test]
    fn in_plan_a_steep_line_gets_the_plan_stations() {
        // Plan length 10, 3D length much more: every 5 in plan gives one station.
        let line = [[0.0, 0.0, 0.0], [10.0, 0.0, 100.0]];
        let plan = densify(
            &line,
            false,
            Placement::Spacing {
                every: 5.0,
                tolerance: 0.0,
            },
            Along::Plan,
            false,
        )
        .unwrap();
        assert_eq!(plan.added, 1);
        assert!(close(plan.points[1], [5.0, 0.0, 50.0]));
        let space = densify(
            &line,
            false,
            Placement::Spacing {
                every: 5.0,
                tolerance: 0.0,
            },
            Along::Space,
            false,
        )
        .unwrap();
        assert!(space.added > 10);
    }

    #[test]
    fn a_ring_is_densified_along_its_closing_segment_and_not_closed_twice() {
        let square = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]];
        let out = densify(&square, true, Placement::Count(3), Along::Space, false).unwrap();
        // Perimeter 4, points at 1, 2, 3 land on vertices: nothing added…
        assert_eq!(out.added, 0);
        let out = densify(&square, true, Placement::Count(4), Along::Space, false).unwrap();
        // …at 0.8, 1.6, 2.4, 3.2: the last one on the closing edge.
        assert_eq!(out.added, 4);
        assert!(close(*out.points.last().unwrap(), [0.0, 0.8, 0.0]));
        assert!(
            !close(*out.points.last().unwrap(), square[0]),
            "the first vertex is not repeated"
        );
    }

    #[test]
    fn smooth_points_lie_off_the_segments_and_the_vertices_do_not_move() {
        let line = [[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 0.0, 0.0], [3.0, 1.0, 0.0]];
        let straight = densify(&line, false, Placement::Count(5), Along::Space, false).unwrap();
        let curved = densify(&line, false, Placement::Count(5), Along::Space, true).unwrap();
        assert_eq!(straight.points.len(), curved.points.len());
        let moved = straight
            .points
            .iter()
            .zip(&curved.points)
            .filter(|(a, b)| !close(**a, **b))
            .count();
        assert!(moved > 0, "a curve, not the segments");
        for vertex in line {
            assert!(
                curved.points.iter().any(|p| close(*p, vertex)),
                "{vertex:?} stays on the curve"
            );
        }
        // Centripetal: no overshoot past the vertices' band in y.
        assert!(
            curved.points.iter().all(|p| p[1] > -0.2 && p[1] < 1.2),
            "{:?}",
            curved.points
        );
    }

    #[test]
    fn refused_by_name() {
        let line = [[0.0; 3], [1.0, 0.0, 0.0]];
        assert!(
            densify(&line, false, Placement::Count(0), Along::Space, false)
                .unwrap_err()
                .contains("at least 1")
        );
        assert!(
            densify(
                &line,
                false,
                Placement::Spacing {
                    every: 0.0,
                    tolerance: 0.0
                },
                Along::Space,
                false
            )
            .is_err()
        );
        assert!(
            densify(
                &line,
                false,
                Placement::Spacing {
                    every: 1.0,
                    tolerance: -1.0
                },
                Along::Space,
                false
            )
            .is_err()
        );
    }
}
