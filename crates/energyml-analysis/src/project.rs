//! Projecting nodes **vertically** — onto a surface, or onto a level
//!
//!
//! The z replaced, x and y kept: what a geologist calls projecting a point onto
//! a horizon. The projection along the normal — to the nearest point — is
//! another operation, not to be confused with this one, and not now.
//!
//! # A node with nothing under it, and a fold
//!
//! Read by [`Heights`], the same reading `thickness` makes, so that *outside*
//! and *folded* mean the same thing in both:
//!
//! - **outside** — no face above or below the node. It is **left where it was
//!   and counted** (a decision taken as proposed): removed, a line would have a hole
//!   and a surface a missing node its triangles still name; left without a
//!   word, it would look projected;
//! - **folded** — several faces over the node disagree: which one is *the*
//!   surface there is a geological decision. Left, and counted, the same way;
//! - **unplaced** — a node with no finite position. Left, and counted.

use crate::heights::{Height, Heights};

/// What the nodes are projected onto.
pub enum Onto<'a> {
    /// A surface — its height under each node.
    Surface(&'a Heights<'a>),
    /// A level: every node at this z.
    Level(f64),
}

/// Nodes projected, and what could not be.
#[derive(Clone, Debug, PartialEq)]
pub struct Projected {
    /// Every node, in order: projected, or where it was.
    pub points: Vec<[f64; 3]>,
    /// Nodes moved onto the surface or the level.
    pub projected: usize,
    /// Nodes with no face above or below — left.
    pub outside: usize,
    /// Nodes over a fold — left.
    pub folded: usize,
    /// Nodes with no finite position — left.
    pub unplaced: usize,
}

/// The nodes projected — every one, or those `chosen`.
pub fn project(points: &[[f64; 3]], chosen: Option<&[bool]>, onto: &Onto) -> Projected {
    let mut out = Projected { points: points.to_vec(), projected: 0, outside: 0, folded: 0, unplaced: 0 };
    for (at, p) in out.points.iter_mut().enumerate() {
        if chosen.is_some_and(|flags| !flags.get(at).copied().unwrap_or(false)) {
            continue;
        }
        if !p.iter().all(|c| c.is_finite()) {
            out.unplaced += 1;
            continue;
        }
        let z = match onto {
            Onto::Level(z) => *z,
            Onto::Surface(heights) => match heights.height(p[0], p[1]) {
                Height::At(z) => z,
                Height::Outside => {
                    out.outside += 1;
                    continue;
                }
                Height::Folded => {
                    out.folded += 1;
                    continue;
                }
            },
        };
        p[2] = z;
        out.projected += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heights::Surface;

    /// A square of side 10 split in two, its height `z = x + 100`.
    fn tilted() -> (Vec<f64>, Vec<u32>) {
        let corners = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        (corners.iter().flat_map(|&[x, y]| [x, y, x + 100.0]).collect(), vec![0, 1, 2, 0, 2, 3])
    }

    #[test]
    fn the_z_is_replaced_and_x_y_kept() {
        let (points, triangles) = tilted();
        let heights = Heights::of(Surface { points: &points, triangles: &triangles, quads: &[] }, "surface", "under the nodes").unwrap();
        let made = project(&[[2.0, 3.0, -5.0], [7.5, 1.0, 999.0]], None, &Onto::Surface(&heights));
        assert_eq!(made.points, vec![[2.0, 3.0, 102.0], [7.5, 1.0, 107.5]]);
        assert_eq!(made.projected, 2);
    }

    /// **A node with nothing under it is left, and counted**.
    #[test]
    fn outside_is_left_and_counted() {
        let (points, triangles) = tilted();
        let heights = Heights::of(Surface { points: &points, triangles: &triangles, quads: &[] }, "surface", "under the nodes").unwrap();
        let made = project(&[[20.0, 3.0, -5.0], [5.0, 5.0, 0.0]], None, &Onto::Surface(&heights));
        assert_eq!(made.points[0], [20.0, 3.0, -5.0], "where it was");
        assert_eq!((made.projected, made.outside), (1, 1));
    }

    #[test]
    fn a_level_and_the_chosen_nodes_alone() {
        let made = project(&[[0.0, 0.0, 1.0], [1.0, 0.0, 2.0], [f64::NAN, 0.0, 3.0]], Some(&[false, true, true]), &Onto::Level(-50.0));
        assert_eq!(made.points[0], [0.0, 0.0, 1.0], "not chosen, not moved");
        assert_eq!(made.points[1], [1.0, 0.0, -50.0]);
        assert_eq!((made.projected, made.unplaced), (1, 1));
    }

    /// Two faces over the same place at two heights: a fold — left, counted.
    #[test]
    fn a_fold_is_left_and_counted() {
        let points = vec![0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 50.0, 10.0, 0.0, 50.0, 0.0, 10.0, 50.0];
        let triangles = vec![0, 1, 2, 3, 4, 5];
        let heights = Heights::of(Surface { points: &points, triangles: &triangles, quads: &[] }, "surface", "under the nodes").unwrap();
        let made = project(&[[1.0, 1.0, 7.0]], None, &Onto::Surface(&heights));
        assert_eq!((made.points[0][2], made.folded), (7.0, 1));
    }
}
