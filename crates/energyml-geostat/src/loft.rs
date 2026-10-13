//! A surface **between polylines** — the loft:
//! two lines for the idea as it was put, more for a fault surface from
//! successive sticks, which is the same band repeated between neighbours.
//!
//! # The correspondence is the whole subject
//!
//! Each line is measured by its **normalised arc length**, 0 to 1 along it,
//! and the stitch advances on the line whose next vertex comes first in that
//! measure — the classic tiling between contours (Christiansen and Sederberg,
//! 1978). Every triangle has two corners on one line and one on the other, and
//! **every vertex of both lines is kept**: nothing is resampled, the rule of
//! `fence::panel`. The optimum (Fuchs, Kedem and Uselton, 1977) costs a
//! shortest path, and would only be worth it if the simple stitch looked bad.
//!
//! # The refusals are half the value
//!
//! A known weakness of the usual practice: it fails silently on sticks that
//! disagree in direction.
//!
//! - **two open lines running opposite ways** give a twisted band that looks
//!   like a surface. The direction is read at the ends — the start of one
//!   nearer the end of the other than its start — and it is **refused, naming
//!   both**, with the remedy: reversing one is a gesture that exists. Turning
//!   one in silence would decide for somebody;
//! - **a closed line and an open one**: there is no correspondence;
//! - **two rings**: the start must be chosen too — the second ring is turned
//!   so that the two starts are nearest, and the result **says** by how many
//!   vertices. Two rings wound opposite ways in plan are refused like two
//!   opposite lines.
//!
//! TODO(reference): verify — the citations of Christiansen and Sederberg (1978) and Fuchs, Kedem and Uselton (1977) are given without title, venue or DOI; complete and check them.

/// One line of the loft: its vertices, whether it closes, and its name for a refusal.
#[derive(Clone, Copy, Debug)]
pub struct LoftLine<'a> {
    pub name: &'a str,
    pub points: &'a [[f64; 3]],
    pub closed: bool,
}

/// The surface: every vertex of every line, in order, and the triangles between them.
#[derive(Clone, Debug, PartialEq)]
pub struct Lofted {
    pub points: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
    /// For each ring after the first: how many vertices it was turned by to
    /// bring its start next to the previous one's — `0` for open lines.
    pub turned: Vec<usize>,
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt()
}

/// The normalised arc length of each vertex — and of the return to the first,
/// for a ring. `None` for a line of no length.
fn abscissas(points: &[[f64; 3]], closed: bool) -> Option<Vec<f64>> {
    let n = points.len();
    let mut out = vec![0.0];
    let mut total = 0.0;
    let steps = if closed { n } else { n - 1 };
    for i in 0..steps {
        total += distance(points[i], points[(i + 1) % n]);
        out.push(total);
    }
    if total <= 0.0 {
        return None;
    }
    Some(out.into_iter().map(|s| s / total).collect())
}

/// Twice the signed area of a ring in plan.
fn winding(points: &[[f64; 3]]) -> f64 {
    let n = points.len();
    (0..n).map(|i| {
        let (a, b) = (points[i], points[(i + 1) % n]);
        a[0] * b[1] - b[0] * a[1]
    }).sum()
}

/// The band between two lines, as triangles over their vertex numbers (`a`'s
/// from `offset_a`, `b`'s from `offset_b`). For rings, the vertex sequences
/// are walked once around and back to their first.
fn stitch(ua: &[f64], ub: &[f64], na: usize, nb: usize, offset_a: u32, offset_b: u32, out: &mut Vec<[u32; 3]>) {
    // `ua` and `ub` hold one abscissa per step of the walk: n for an open
    // line, n + 1 for a ring (its first vertex again, at 1).
    let (last_a, last_b) = (ua.len() - 1, ub.len() - 1);
    let vertex_a = |i: usize| offset_a + (i % na) as u32;
    let vertex_b = |j: usize| offset_b + (j % nb) as u32;
    let (mut i, mut j) = (0usize, 0usize);
    while i < last_a || j < last_b {
        let advance_a = if i == last_a {
            false
        } else if j == last_b {
            true
        } else {
            ua[i + 1] <= ub[j + 1]
        };
        if advance_a {
            out.push([vertex_a(i), vertex_a(i + 1), vertex_b(j)]);
            i += 1;
        } else {
            out.push([vertex_a(i), vertex_b(j + 1), vertex_b(j)]);
            j += 1;
        }
    }
}

/// The loft of `lines`, in their order — see the module documentation for what
/// is refused, and why.
pub fn loft(lines: &[LoftLine]) -> Result<Lofted, String> {
    if lines.len() < 2 {
        return Err(format!("a loft needs at least two lines, and {} was given", lines.len()));
    }
    for line in lines {
        let least = if line.closed { 3 } else { 2 };
        if line.points.len() < least {
            return Err(format!("`{}` has {} node(s) — a{} line needs at least {least}", line.name, line.points.len(), if line.closed { " closed" } else { "n open" }));
        }
    }
    let first = lines[0];
    if let Some(other) = lines.iter().find(|line| line.closed != first.closed) {
        let (closed, open) = if first.closed { (first, *other) } else { (*other, first) };
        return Err(format!("`{}` is closed and `{}` is open — there is no correspondence between a ring and a line", closed.name, open.name));
    }

    let mut points: Vec<[f64; 3]> = Vec::new();
    let mut triangles = Vec::new();
    let mut turned = Vec::new();
    // Each line as it is used — a ring possibly turned.
    let mut previous: Vec<[f64; 3]> = first.points.to_vec();
    points.extend(&previous);
    let mut previous_offset = 0u32;
    for pair in lines.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let mut next: Vec<[f64; 3]> = b.points.to_vec();
        if a.closed {
            let (wa, wb) = (winding(&previous), winding(&next));
            if wa * wb < 0.0 {
                return Err(format!(
                    "`{}` and `{}` turn opposite ways — the band between them would be twisted; reverse one of them",
                    a.name, b.name
                ));
            }
            // Turn the ring so that its start is the vertex nearest the
            // previous ring's start.
            let start = previous[0];
            let (shift, _) = next
                .iter()
                .enumerate()
                .map(|(k, p)| (k, distance(start, *p)))
                .fold((0, f64::INFINITY), |best, one| if one.1 < best.1 { one } else { best });
            next.rotate_left(shift);
            turned.push(shift);
        } else {
            let (sa, ea) = (previous[0], previous[previous.len() - 1]);
            let (sb, eb) = (next[0], next[next.len() - 1]);
            if distance(sa, eb) + distance(ea, sb) < distance(sa, sb) + distance(ea, eb) {
                return Err(format!(
                    "`{}` and `{}` run opposite ways — the band between them would be twisted; reverse one of them",
                    a.name, b.name
                ));
            }
            turned.push(0);
        }
        let ua = abscissas(&previous, a.closed).ok_or_else(|| format!("`{}` has no length", a.name))?;
        let ub = abscissas(&next, b.closed).ok_or_else(|| format!("`{}` has no length", b.name))?;
        let offset = points.len() as u32;
        stitch(&ua, &ub, previous.len(), next.len(), previous_offset, offset, &mut triangles);
        points.extend(&next);
        previous = next;
        previous_offset = offset;
    }
    Ok(Lofted { points, triangles, turned })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line<'a>(name: &'a str, points: &'a [[f64; 3]]) -> LoftLine<'a> {
        LoftLine { name, points, closed: false }
    }

    #[test]
    fn every_vertex_kept_and_every_triangle_spans_both_lines() {
        let a = [[0.0, 0.0, 0.0], [5.0, 0.0, 0.0], [10.0, 0.0, 0.0]];
        let b = [[0.0, 10.0, 0.0], [2.0, 10.0, 0.0], [4.0, 10.0, 0.0], [10.0, 10.0, 0.0]];
        let made = loft(&[line("a", &a), line("b", &b)]).unwrap();
        assert_eq!(made.points.len(), 7, "all vertices, none resampled");
        // A band between n and m vertices has n + m − 2 triangles.
        assert_eq!(made.triangles.len(), 5);
        for t in &made.triangles {
            let on_a = t.iter().filter(|&&v| v < 3).count();
            assert!(on_a == 1 || on_a == 2, "two corners on one line, one on the other: {t:?}");
        }
    }

    #[test]
    fn the_stitch_follows_the_normalised_abscissa() {
        // The second vertex of `b` is at 0.2 of its length, the one of `a` at
        // 0.5: `b` advances first.
        let a = [[0.0, 0.0, 0.0], [5.0, 0.0, 0.0], [10.0, 0.0, 0.0]];
        let b = [[0.0, 10.0, 0.0], [2.0, 10.0, 0.0], [10.0, 10.0, 0.0]];
        let made = loft(&[line("a", &a), line("b", &b)]).unwrap();
        assert_eq!(made.triangles[0], [0, 4, 3], "advance on b first: a0, b1, b0");
    }

    #[test]
    fn lines_running_opposite_ways_are_refused_by_name() {
        let a = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]];
        let b = [[10.0, 10.0, 0.0], [0.0, 10.0, 0.0]];
        let why = loft(&[line("stick 1", &a), line("stick 2", &b)]).unwrap_err();
        assert!(why.contains("`stick 1`") && why.contains("`stick 2`") && why.contains("reverse"), "{why}");
    }

    #[test]
    fn a_ring_and_a_line_are_refused() {
        let ring = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0]];
        let open = [[0.0, 0.0, 1.0], [1.0, 0.0, 1.0]];
        let why = loft(&[LoftLine { name: "ring", points: &ring, closed: true }, line("open", &open)]).unwrap_err();
        assert!(why.contains("`ring` is closed") && why.contains("`open` is open"), "{why}");
    }

    #[test]
    fn two_rings_close_the_band_and_the_second_is_turned_to_meet_the_first() {
        let low = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]];
        // The same square higher, its start one vertex along.
        let high = [[1.0, 0.0, 5.0], [1.0, 1.0, 5.0], [0.0, 1.0, 5.0], [0.0, 0.0, 5.0]];
        let made = loft(&[LoftLine { name: "low", points: &low, closed: true }, LoftLine { name: "high", points: &high, closed: true }]).unwrap();
        assert_eq!(made.turned, vec![3], "turned so that it starts above the first ring's start");
        assert_eq!(made.points[4], [0.0, 0.0, 5.0]);
        // A closed band between two rings of four: eight triangles, every
        // vertex used, around and back.
        assert_eq!(made.triangles.len(), 8);
        for v in 0..8u32 {
            assert!(made.triangles.iter().any(|t| t.contains(&v)), "vertex {v} used");
        }
    }

    #[test]
    fn rings_wound_opposite_ways_are_refused() {
        let ccw = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]];
        let cw = [[0.0, 0.0, 5.0], [0.0, 1.0, 5.0], [1.0, 1.0, 5.0], [1.0, 0.0, 5.0]];
        let why = loft(&[LoftLine { name: "a", points: &ccw, closed: true }, LoftLine { name: "b", points: &cw, closed: true }]).unwrap_err();
        assert!(why.contains("opposite ways"), "{why}");
    }

    #[test]
    fn three_sticks_make_two_bands() {
        let s1 = [[0.0, 0.0, 0.0], [0.0, 0.0, -10.0]];
        let s2 = [[5.0, 0.0, 0.0], [5.0, 0.0, -5.0], [5.0, 0.0, -10.0]];
        let s3 = [[10.0, 0.0, 0.0], [10.0, 0.0, -10.0]];
        let made = loft(&[line("1", &s1), line("2", &s2), line("3", &s3)]).unwrap();
        assert_eq!(made.points.len(), 7);
        assert_eq!(made.triangles.len(), 3 + 3);
    }
}
