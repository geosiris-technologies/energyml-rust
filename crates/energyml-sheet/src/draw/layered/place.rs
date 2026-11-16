//! Positions: where each box and each bend point sits.
//!
//! Everything is worked out in `(along the flow, across the flow)` and
//! converted to `(x, y)` at the end: for `LeftToRight` along is `x`, for
//! `TopToBottom` it is `y`.

use super::ordering::{Layers, Slot};
use super::{Flow, LayerInput, PlacedBox, Spacing, along_cross, point_of};

/// The placed boxes, the point of every slot, and the drawing's size.
#[derive(Clone, Debug, PartialEq)]
pub struct Positions {
    /// One per node, in node order.
    pub boxes: Vec<PlacedBox>,
    /// One per slot: the centre of a box, the point of a bend.
    pub slot_at: Vec<[f64; 2]>,
    /// Width and height of the drawing, margins included, user units.
    pub size: [f64; 2],
}

/// Place the slots of `layers`.
///
/// 1. A layer is as long along the flow as its longest box (0 with no box);
///    layers follow each other `layer_gap` apart from `margin`; a box is
///    centred in its layer along the flow.
/// 2. Across the flow the slots of a layer are stacked from `margin` (a box
///    takes its size and a `node_gap`, a bend nothing and a `bend_gap`), then
///    each layer is centred on the tallest.
/// 3. A pinned box is put exactly at its pin. It blocks, in every layer whose
///    band along the flow it cuts, the interval it takes across the flow (and
///    `node_gap` on each side); the slots that are not pinned are pushed past
///    those intervals, keeping their order.
/// 4. The size is the furthest edge of any box or bend, plus `margin`.
pub fn place(layers: &Layers, input: &LayerInput, flow: Flow, spacing: &Spacing) -> Positions {
    let layer_count = layers.order.len();
    let slot_count = layers.slots.len();

    // (along, across) size of each slot.
    let sizes: Vec<(f64, f64)> = layers
        .slots
        .iter()
        .map(|slot| match *slot {
            Slot::Node(node) => {
                let spec = input.nodes[node];
                match flow {
                    Flow::LeftToRight => (spec.width, spec.height),
                    Flow::TopToBottom => (spec.height, spec.width),
                }
            }
            Slot::Bend { .. } => (0.0, 0.0),
        })
        .collect();
    let gaps: Vec<f64> = layers
        .slots
        .iter()
        .map(|slot| match slot {
            Slot::Node(_) => spacing.node_gap,
            Slot::Bend { .. } => spacing.bend_gap,
        })
        .collect();

    // 1. The layers along the flow.
    let extent: Vec<f64> = layers
        .order
        .iter()
        .map(|slots| {
            slots
                .iter()
                .filter(|&&slot| matches!(layers.slots[slot], Slot::Node(_)))
                .map(|&slot| sizes[slot].0)
                .fold(0.0, f64::max)
        })
        .collect();
    let mut layer_start = Vec::with_capacity(layer_count);
    let mut cursor = spacing.margin;
    for length in &extent {
        layer_start.push(cursor);
        cursor += length + spacing.layer_gap;
    }
    let mut along = vec![0.0f64; slot_count];
    for (layer, slots) in layers.order.iter().enumerate() {
        for &slot in slots {
            along[slot] = layer_start[layer] + (extent[layer] - sizes[slot].0) / 2.0;
        }
    }

    // 2. Across the flow.
    let mut cross = vec![0.0f64; slot_count];
    let mut heights = vec![0.0f64; layer_count];
    for (layer, slots) in layers.order.iter().enumerate() {
        let mut at = spacing.margin;
        let mut end = spacing.margin;
        for &slot in slots {
            cross[slot] = at;
            end = at + sizes[slot].1;
            at = end + gaps[slot];
        }
        heights[layer] = if slots.is_empty() {
            0.0
        } else {
            end - spacing.margin
        };
    }
    let tallest = heights.iter().copied().fold(0.0, f64::max);
    for (layer, slots) in layers.order.iter().enumerate() {
        let shift = (tallest - heights[layer]) / 2.0;
        for &slot in slots {
            cross[slot] += shift;
        }
    }

    // 3. Pins.
    let nodes = input.nodes.len();
    let pin_of = |node: usize| -> Option<[f64; 2]> { input.pins.get(node).copied().flatten() };
    let mut pinned = vec![false; slot_count];
    let mut blocked: Vec<Vec<(f64, f64)>> = vec![Vec::new(); layer_count];
    for node in 0..nodes.min(slot_count) {
        let Some(pin) = pin_of(node) else { continue };
        let (a, t) = along_cross(flow, pin);
        pinned[node] = true;
        along[node] = a;
        cross[node] = t;
        let (size_along, size_across) = sizes[node];
        for layer in 0..layer_count {
            let band_start = layer_start[layer];
            let band_end = band_start + extent[layer];
            if a < band_end && band_start < a + size_along {
                blocked[layer].push((t - spacing.node_gap, t + size_across + spacing.node_gap));
            }
        }
    }
    for (layer, slots) in layers.order.iter().enumerate() {
        if blocked[layer].is_empty() {
            continue;
        }
        let mut cursor = spacing.margin;
        for &slot in slots {
            if pinned[slot] {
                continue;
            }
            let size = sizes[slot].1;
            let mut t = cursor.max(cross[slot]);
            while let Some(&(_, end)) = blocked[layer]
                .iter()
                .find(|&&(start, end)| t < end && t + size > start)
            {
                t = end;
            }
            cross[slot] = t;
            cursor = t + size + gaps[slot];
        }
    }

    // 4. The boxes, the slot points and the size.
    let order_in_layer = layers.positions();
    let mut boxes = Vec::with_capacity(nodes);
    let mut slot_at = vec![[0.0f64; 2]; slot_count];
    let mut far = [spacing.margin, spacing.margin];
    for slot in 0..slot_count {
        let point = point_of(flow, along[slot], cross[slot]);
        match layers.slots[slot] {
            Slot::Node(node) => {
                let spec = input.nodes[node];
                let placed = PlacedBox {
                    x: point[0],
                    y: point[1],
                    width: spec.width,
                    height: spec.height,
                    layer: layers.layer_of[slot],
                    order: order_in_layer[slot],
                    pinned: pinned[slot],
                };
                slot_at[slot] = [point[0] + spec.width / 2.0, point[1] + spec.height / 2.0];
                far[0] = far[0].max(placed.x + placed.width);
                far[1] = far[1].max(placed.y + placed.height);
                boxes.push(placed);
            }
            Slot::Bend { .. } => {
                slot_at[slot] = point;
                far[0] = far[0].max(point[0]);
                far[1] = far[1].max(point[1]);
            }
        }
    }
    Positions {
        boxes,
        slot_at,
        size: [far[0] + spacing.margin, far[1] + spacing.margin],
    }
}
