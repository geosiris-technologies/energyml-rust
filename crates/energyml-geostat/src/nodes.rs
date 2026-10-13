//! What an edit of **nodes** does to a shape — erasing them, and the triangles a
//! move turns over.
//!
//! # Erasing, by nature of geometry
//!
//! The rule that deleted points create holes, made precise:
//!
//! - a **cloud** loses the points; nothing else names them;
//! - a **polyline** is **cut** at each node taken out: one line becomes
//!   several. A piece left with a single node is no longer a line — it goes,
//!   **counted**. A ring cut once is an open line, from just after the gap
//!   round to just before it;
//! - a **triangulated surface** loses every triangle that names a node taken
//!   out, and the nodes no triangle names any more are compacted away — the
//!   hole has the shape of its triangles, jagged, as `trim` already makes and
//!   assumes. They are counted too.
//!
//! Ranks change: the caller's selection by ranks means nothing afterwards, and
//! is the caller's to empty.
//!
//! # Turned over
//!
//! Moving or projecting some nodes of a surface can turn a triangle over — one
//! corner passing, in plan, to the other side of its opposite edge. Nothing
//! forbids it, and a folded surface is a legitimate result; [`flipped`] counts
//! them, so that the operation **says** it rather than refuses.

/// Polylines once nodes are taken out.
#[derive(Clone, Debug, PartialEq)]
pub struct ErasedLines {
    /// The nodes that stay, line after line.
    pub points: Vec<[f64; 3]>,
    /// The node count of each line.
    pub runs: Vec<u32>,
    /// Whether each line closes — only an uncut ring still does.
    pub closed: Vec<bool>,
    /// Pieces left with a single node, dropped.
    pub dropped: usize,
}

/// A triangulated surface once nodes are taken out.
#[derive(Clone, Debug, PartialEq)]
pub struct ErasedSurface {
    /// The nodes that stay, in their order.
    pub points: Vec<[f64; 3]>,
    /// The triangles that stay, renumbered.
    pub triangles: Vec<[u32; 3]>,
    /// Triangles that named a node taken out.
    pub removed: usize,
    /// Nodes kept by the choice but named by no triangle any more — compacted away.
    pub orphans: usize,
}

/// A cloud without the nodes `gone` flags.
pub fn erase_from_cloud(points: &[[f64; 3]], gone: &[bool]) -> Vec<[f64; 3]> {
    points.iter().enumerate().filter(|(at, _)| !gone.get(*at).copied().unwrap_or(false)).map(|(_, p)| *p).collect()
}

/// Polylines without the nodes `gone` flags — `runs` their node counts,
/// `closed` their closure (missing entries: open).
pub fn erase_from_lines(points: &[[f64; 3]], runs: &[u32], closed: &[bool], gone: &[bool]) -> ErasedLines {
    let mut out = ErasedLines { points: Vec::new(), runs: Vec::new(), closed: Vec::new(), dropped: 0 };
    let is_gone = |at: usize| gone.get(at).copied().unwrap_or(false);
    let push = |piece: &mut Vec<usize>, out: &mut ErasedLines| {
        match piece.len() {
            0 => {}
            1 => out.dropped += 1,
            n => {
                out.points.extend(piece.iter().map(|&at| points[at]));
                out.runs.push(n as u32);
                out.closed.push(false);
            }
        }
        piece.clear();
    };
    let mut start = 0usize;
    for (line, &run) in runs.iter().enumerate() {
        let run = run as usize;
        let nodes: Vec<usize> = (start..start + run).collect();
        start += run;
        let ring = closed.get(line).copied().unwrap_or(false);
        let first_gone = nodes.iter().position(|&at| is_gone(at));
        match (ring, first_gone) {
            (_, None) => {
                out.points.extend(nodes.iter().map(|&at| points[at]));
                out.runs.push(run as u32);
                out.closed.push(ring);
            }
            (false, Some(_)) => {
                let mut piece = Vec::new();
                for &at in &nodes {
                    if is_gone(at) {
                        push(&mut piece, &mut out);
                    } else {
                        piece.push(at);
                    }
                }
                push(&mut piece, &mut out);
            }
            // A ring is walked once round from just after its first gap: the
            // piece that crosses its start is one line, not two.
            (true, Some(gap)) => {
                let mut piece = Vec::new();
                for step in 1..=run {
                    let at = nodes[(gap + step) % run];
                    if is_gone(at) {
                        push(&mut piece, &mut out);
                    } else {
                        piece.push(at);
                    }
                }
                push(&mut piece, &mut out);
            }
        }
    }
    out
}

/// A triangulated surface without the nodes `gone` flags.
pub fn erase_from_surface(points: &[[f64; 3]], triangles: &[[u32; 3]], gone: &[bool]) -> ErasedSurface {
    let is_gone = |at: u32| gone.get(at as usize).copied().unwrap_or(false);
    let kept: Vec<[u32; 3]> = triangles.iter().filter(|t| !t.iter().any(|&v| is_gone(v))).copied().collect();
    let removed = triangles.len() - kept.len();
    let mut used = vec![false; points.len()];
    for t in &kept {
        for &v in t {
            used[v as usize] = true;
        }
    }
    let mut rank = vec![u32::MAX; points.len()];
    let mut out_points = Vec::new();
    let mut orphans = 0usize;
    for (at, p) in points.iter().enumerate() {
        if used[at] {
            rank[at] = out_points.len() as u32;
            out_points.push(*p);
        } else if !is_gone(at as u32) {
            orphans += 1;
        }
    }
    let triangles = kept.iter().map(|t| t.map(|v| rank[v as usize])).collect();
    ErasedSurface { points: out_points, triangles, removed, orphans }
}

/// Twice the signed area of a triangle in plan.
fn signed_area(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// How many triangles a move turned over in plan: their signed area changed
/// sign. A triangle with no area in plan, before or after — a vertical face —
/// is not counted: it had no side to turn over from.
pub fn flipped(before: &[[f64; 3]], after: &[[f64; 3]], triangles: &[[u32; 3]]) -> usize {
    triangles
        .iter()
        .filter(|t| {
            let [a, b, c] = t.map(|v| v as usize);
            let (Some(&a0), Some(&b0), Some(&c0), Some(&a1), Some(&b1), Some(&c1)) =
                (before.get(a), before.get(b), before.get(c), after.get(a), after.get(b), after.get(c))
            else {
                return false;
            };
            let (s0, s1) = (signed_area(a0, b0, c0), signed_area(a1, b1, c1));
            s0 != 0.0 && s1 != 0.0 && s0.signum() != s1.signum()
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(n: usize, gone: &[usize]) -> Vec<bool> {
        (0..n).map(|at| gone.contains(&at)).collect()
    }

    #[test]
    fn a_cloud_loses_the_points() {
        let points = [[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        assert_eq!(erase_from_cloud(&points, &flags(3, &[1])), vec![[0.0; 3], [2.0, 0.0, 0.0]]);
    }

    #[test]
    fn a_line_is_cut_and_a_lone_node_goes_counted() {
        // 0 1 2 3 4 5, taking out 2 and 4: [0 1] [3] [5] → one line, two lone nodes.
        let points: Vec<[f64; 3]> = (0..6).map(|i| [i as f64, 0.0, 0.0]).collect();
        let made = erase_from_lines(&points, &[6], &[], &flags(6, &[2, 4]));
        assert_eq!(made.runs, vec![2]);
        assert_eq!(made.points, vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
        assert_eq!(made.dropped, 2);
    }

    #[test]
    fn a_ring_cut_once_is_one_open_line_round_the_gap() {
        // A ring 0 1 2 3 4 without 1: one open line 2 3 4 0 — crossing its start.
        let points: Vec<[f64; 3]> = (0..5).map(|i| [i as f64, 0.0, 0.0]).collect();
        let made = erase_from_lines(&points, &[5], &[true], &flags(5, &[1]));
        assert_eq!(made.runs, vec![4]);
        assert_eq!(made.closed, vec![false]);
        assert_eq!(made.points.iter().map(|p| p[0] as usize).collect::<Vec<_>>(), vec![2, 3, 4, 0]);
    }

    #[test]
    fn an_untouched_ring_stays_closed_and_lines_stay_apart() {
        let points: Vec<[f64; 3]> = (0..7).map(|i| [i as f64, 0.0, 0.0]).collect();
        // A ring of 4 untouched, then a line of 3 losing its middle.
        let made = erase_from_lines(&points, &[4, 3], &[true, false], &flags(7, &[5]));
        assert_eq!(made.runs, vec![4]);
        assert_eq!(made.closed, vec![true]);
        assert_eq!(made.dropped, 2);
    }

    #[test]
    fn a_surface_loses_the_triangles_and_compacts_its_orphans() {
        // A fan of four triangles round a centre 4; taking out corner 0 takes
        // the two triangles that name it — and no node is left unnamed.
        let points = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 2.0, 0.0], [0.0, 2.0, 0.0], [1.0, 1.0, 0.0]];
        let triangles = [[0, 1, 4], [1, 2, 4], [2, 3, 4], [3, 0, 4]];
        let made = erase_from_surface(&points, &triangles, &flags(5, &[0]));
        assert_eq!((made.removed, made.orphans), (2, 0));
        assert_eq!(made.points.len(), 4);
        // Taking out the centre takes every triangle: the corners are orphans.
        let hole = erase_from_surface(&points, &triangles, &flags(5, &[4]));
        assert_eq!((hole.removed, hole.orphans, hole.points.len()), (4, 4, 0));
    }

    #[test]
    fn the_renumbering_keeps_the_triangles_on_their_nodes() {
        let points = [[0.0, 0.0, 0.0], [9.0, 9.0, 9.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let made = erase_from_surface(&points, &[[0, 2, 3], [1, 2, 3]], &flags(4, &[1]));
        assert_eq!(made.triangles, vec![[0, 1, 2]]);
        assert_eq!(made.points, vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
    }

    #[test]
    fn a_triangle_turned_over_in_plan_is_counted() {
        let before = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let after = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]];
        assert_eq!(flipped(&before, &after, &[[0, 1, 2]]), 1);
        assert_eq!(flipped(&before, &before, &[[0, 1, 2]]), 0);
        // A vertical face has no side to turn from.
        let wall = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0]];
        assert_eq!(flipped(&wall, &after, &[[0, 1, 2]]), 0);
    }
}
