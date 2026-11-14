//! Routes: one chain of cubic curves per edge, from its own source to its own
//! target.
//!
//! Worked out in `(along the flow, across the flow)` and converted to `(x, y)`
//! at the end. "After" is the side of a box towards the later layers, "before"
//! the side towards the earlier ones.

use super::ordering::{Layers, Slot};
use super::place::Positions;
use super::{
    Flow, LayerInput, Route, RouteKind, Spacing, along_cross, along_span, cross_span, oriented,
    point_of,
};

type Pt = (f64, f64);

/// Route every edge of `input`, in edge order (duplicates get the same route).
///
/// - `Forward`: from the middle of the source's "after" side, through each bend
///   point, to the middle of the target's "before" side; between two points
///   `p -> q`, the controls are `p + (dl / 2, 0)` and `q - (dl / 2, 0)` with
///   `dl` the distance along the flow: tangents with the flow, an S curve.
/// - `Back`: the same chain of bends, but out of the way: from the middle of
///   the "below" side (across the flow) of each box, each piece with controls
///   `p + (0, back_bulge)` and `q + (0, back_bulge)`. Walked from the edge's
///   own source to its own target, so the arrow stays at the target.
/// - `Flat`: from the "after" side of the source to the "after" side of the
///   target, controls `p + (back_bulge, 0)` and `q + (back_bulge, 0)`.
/// - `Loop`: from the middle of the "after" side to the middle of the "top"
///   side, controls `p + (loop_size, 0)` and `q - (0, loop_size)`.
///
/// The edges are in range (checked by `layered`).
pub fn route(
    layers: &Layers,
    positions: &Positions,
    input: &LayerInput,
    back: &[usize],
    flow: Flow,
    spacing: &Spacing,
) -> Vec<Route> {
    // The bends of each edge, by step.
    let mut bends: Vec<Vec<(usize, Pt)>> = vec![Vec::new(); input.edges.len()];
    for (slot, kind) in layers.slots.iter().enumerate() {
        if let Slot::Bend { edge, step } = *kind
            && edge < bends.len()
        {
            bends[edge].push((step, along_cross(flow, positions.slot_at[slot])));
        }
    }
    for list in &mut bends {
        list.sort_by_key(|&(step, _)| step);
    }

    let after = |node: usize| -> Pt {
        let b = &positions.boxes[node];
        let (a0, sa) = along_span(flow, b);
        let (c0, sc) = cross_span(flow, b);
        (a0 + sa, c0 + sc / 2.0)
    };
    let before = |node: usize| -> Pt {
        let b = &positions.boxes[node];
        let (a0, _) = along_span(flow, b);
        let (c0, sc) = cross_span(flow, b);
        (a0, c0 + sc / 2.0)
    };
    let below = |node: usize| -> Pt {
        let b = &positions.boxes[node];
        let (a0, sa) = along_span(flow, b);
        let (c0, sc) = cross_span(flow, b);
        (a0 + sa / 2.0, c0 + sc)
    };
    let top = |node: usize| -> Pt {
        let b = &positions.boxes[node];
        let (a0, sa) = along_span(flow, b);
        let (c0, _) = cross_span(flow, b);
        (a0 + sa / 2.0, c0)
    };

    let mut routes = Vec::with_capacity(input.edges.len());
    for (index, &(from, to)) in input.edges.iter().enumerate() {
        let (kind, points, controls): (RouteKind, Vec<Pt>, Controls) = if from == to {
            (
                RouteKind::Loop,
                vec![after(from), top(from)],
                Controls::Loop(spacing.loop_size),
            )
        } else if back.binary_search(&index).is_ok() {
            // Oriented (a, b), a earlier than b; walked from `from` = b to `to` = a.
            let (a, b) = oriented(index, from, to, back);
            let mut points = vec![below(b)];
            points.extend(bends[index].iter().rev().map(|&(_, p)| p));
            points.push(below(a));
            (RouteKind::Back, points, Controls::Back(spacing.back_bulge))
        } else if layers.layer_of[from] == layers.layer_of[to] {
            (
                RouteKind::Flat,
                vec![after(from), after(to)],
                Controls::Flat(spacing.back_bulge),
            )
        } else {
            let mut points = vec![after(from)];
            points.extend(bends[index].iter().map(|&(_, p)| p));
            points.push(before(to));
            (RouteKind::Forward, points, Controls::Forward)
        };

        let segments = points
            .windows(2)
            .map(|pair| {
                let (p, q) = (pair[0], pair[1]);
                let (c1, c2) = controls.of(p, q);
                [
                    point_of(flow, p.0, p.1),
                    point_of(flow, c1.0, c1.1),
                    point_of(flow, c2.0, c2.1),
                    point_of(flow, q.0, q.1),
                ]
            })
            .collect();
        routes.push(Route {
            edge: index,
            kind,
            segments,
        });
    }
    routes
}

/// How the control points of a piece `p -> q` are chosen.
enum Controls {
    Forward,
    Back(f64),
    Flat(f64),
    Loop(f64),
}

impl Controls {
    fn of(&self, p: Pt, q: Pt) -> (Pt, Pt) {
        match *self {
            Controls::Forward => {
                let half = (q.0 - p.0) / 2.0;
                ((p.0 + half, p.1), (q.0 - half, q.1))
            }
            Controls::Back(bulge) => ((p.0, p.1 + bulge), (q.0, q.1 + bulge)),
            Controls::Flat(bulge) => ((p.0 + bulge, p.1), (q.0 + bulge, q.1)),
            Controls::Loop(size) => ((p.0 + size, p.1), (q.0, q.1 - size)),
        }
    }
}
