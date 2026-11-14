//! Back edges: which edges are drawn against the flow so that what is left is
//! acyclic.

use super::LayerInput;

/// The edges to draw against the flow, ascending.
///
/// Self loops are never back edges (they become `RouteKind::Loop`).
///
/// - When every node has a fixed layer, the columns decide: an edge is a back
///   edge exactly when its source's column is after its target's. Two nodes of
///   the same column make a flat edge, never a back one.
/// - Otherwise a depth-first walk decides (iterative, roots in node order,
///   out edges in edge order): an edge to a node still on the stack is a back
///   edge. Every remaining edge goes from a node that finishes later to one
///   that finishes earlier, and a reversed back edge goes from an ancestor
///   (which finishes later) to its descendant, so the result is acyclic.
///   Partial fixed layers are only minimums for the layering and do not count
///   here.
///
/// An edge naming a node that does not exist is ignored (`layered` refuses it
/// before calling this).
pub fn break_cycles(input: &LayerInput) -> Vec<usize> {
    let nodes = input.nodes.len();
    let edges = &input.edges;

    if nodes > 0
        && input.fixed_layer.len() == nodes
        && input.fixed_layer.iter().all(Option::is_some)
    {
        let mut back = Vec::new();
        for (index, &(from, to)) in edges.iter().enumerate() {
            if from == to || from >= nodes || to >= nodes {
                continue;
            }
            if input.fixed_layer[from] > input.fixed_layer[to] {
                back.push(index);
            }
        }
        return back;
    }

    let mut out: Vec<Vec<usize>> = vec![Vec::new(); nodes];
    for (index, &(from, to)) in edges.iter().enumerate() {
        if from != to && from < nodes && to < nodes {
            out[from].push(index);
        }
    }

    // 0: not seen, 1: on the stack, 2: finished.
    let mut state = vec![0u8; nodes];
    let mut back = Vec::new();
    for root in 0..nodes {
        if state[root] != 0 {
            continue;
        }
        state[root] = 1;
        let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
        while !stack.is_empty() {
            let top = stack.len() - 1;
            let (node, next) = stack[top];
            if next < out[node].len() {
                stack[top].1 += 1;
                let edge = out[node][next];
                let target = edges[edge].1;
                match state[target] {
                    1 => back.push(edge),
                    0 => {
                        state[target] = 1;
                        stack.push((target, 0));
                    }
                    _ => {}
                }
            } else {
                state[node] = 2;
                stack.pop();
            }
        }
    }
    back.sort_unstable();
    back
}
