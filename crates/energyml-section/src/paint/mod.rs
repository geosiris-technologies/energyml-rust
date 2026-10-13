//! What the front end's scene paints along an object, laid on a section
//! (`View::paints`, `View::joints`) — the resolver the front end supplies; without
//! a paint, `layout` draws as before (the default resolver).
//!
//! Contract of 05/10, filled by the agents of the same day: `well_pieces` here
//! (wells, joints), `surfaces::trace_pieces` (the cut of a painted surface).

pub mod surfaces;

use crate::model::{Blend, Joint, JointKind, Paint};

/// A stretch of one colour, in pixels of the drawing.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    /// The stretch, pixels.
    pub points: Vec<[f64; 2]>,
    /// `#rrggbb`.
    pub colour: String,
    /// 0 to 1.
    pub opacity: f64,
}

/// A smooth stretch is cut into sub-pieces about this long, pixels: two pixels
/// is below what the eye resolves as a step on a gradient, and cheap enough
/// (one `<polyline>` per sub-piece) for a well of a few hundred stretches.
pub const SMOOTH_STEP_PX: f64 = 2.0;

/// The most sub-pieces one smooth stretch is cut into: past it a colour
/// channel changes by at most a few levels per piece, so more pieces would only
/// make the SVG larger.
pub const SMOOTH_MOST: usize = 64;

/// Alpha below this is "no value" — the 3D view's threshold (`fragmentAt`:
/// alpha under one half is discarded), so the section and the scene agree on
/// which stretches are absent. A buffer's alpha is 0 or 255 in practice; the
/// midpoint only decides what a stray byte means.
const ABSENT_BELOW: u8 = 128;

type Rgba = [u8; 4];

/// `#rrggbbaa` (or `#rrggbb`, opaque) as four bytes.
fn parse_colour(text: &str) -> Option<Rgba> {
    let hex = text.strip_prefix('#')?;
    if !(hex.len() == 6 || hex.len() == 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?, if hex.len() == 8 { byte(6)? } else { 255 }])
}

fn hex_of(rgb: [f64; 3]) -> String {
    let c = |v: f64| v.round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", c(rgb[0]), c(rgb[1]), c(rgb[2]))
}

fn mix(a: Rgba, b: Rgba, t: f64) -> [f64; 3] {
    [0, 1, 2].map(|i| a[i] as f64 + (b[i] as f64 - a[i] as f64) * t)
}

/// What a paint and its joints say, read.
struct Read {
    nodes: Vec<Rgba>,
    joint: Rgba,
}

fn read(nodes: usize, paint: &Paint, joints: &Joint) -> Result<Read, String> {
    if paint.nodes.len() != nodes {
        return Err(format!(
            "the paint holds {} node colours but the well has {nodes} nodes — they must match one to one",
            paint.nodes.len()
        ));
    }
    let mut colours = Vec::with_capacity(nodes);
    for (k, text) in paint.nodes.iter().enumerate() {
        colours.push(parse_colour(text).ok_or_else(|| format!("node {k}'s colour {text:?} is not #rrggbbaa"))?);
    }
    let mut joint = [0, 0, 0, 255];
    if joints.kind != JointKind::None {
        joint = parse_colour(&joints.colour).ok_or_else(|| format!("the joint colour {:?} is not #rrggbb", joints.colour))?;
        if !joints.px.is_finite() || joints.px < 0.0 {
            return Err(format!("the joint length {} px is not a length", joints.px));
        }
        if !joints.alpha.is_finite() {
            return Err(format!("the joint alpha {} is not a number", joints.alpha));
        }
    }
    Ok(Read { nodes: colours, joint })
}

/// Whether a paint fits a well of `nodes` nodes and its colours read: the
/// refusal [`well_pieces`] gives for those two reasons, before any pixel is
/// laid — `layout` says it before it reserves the room for its notes.
pub fn check(nodes: usize, paint: &Paint, joints: &Joint) -> Result<(), String> {
    read(nodes, paint, joints).map(|_| ())
}

/// A point of segment `a`–`b` at `t`, exact at both ends: two stretches that
/// meet at a node meet on the very same point, which is what merging compares.
fn at(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    if t <= 0.0 {
        a
    } else if t >= 1.0 {
        b
    } else {
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
    }
}

/// A well's path, already laid in pixels (one point per node, in node order),
/// painted: the pieces to draw, joints included, or why the paint does not fit
/// this well — said by name, and the well is then drawn as before.
///
/// The reading is a web 3D view's (`fragmentAt`), transcribed on the path in pixels:
///
/// * **Net**: node `k`'s colour over the stretch from node `k` to node `k+1`;
///   **Smooth**: from node `k`'s colour to node `k+1`'s — except that an absent
///   neighbour stops the blend (the stretch is then flat in node `k`'s colour),
///   because an interpolated absent colour is a dark fringe, not a value. Each
///   smooth stretch is cut into sub-pieces about [`SMOOTH_STEP_PX`] long (at
///   most [`SMOOTH_MOST`]), each in the colour at its middle. The last node's
///   colour is never seen in the net reading: no stretch follows it.
/// * a node whose alpha is under one half is absent: its stretch is not drawn.
///   A gap, never a colour.
/// * joints (`jointFlags`): never at the two ends; `Nodes` at every inner
///   node, `Changes` where the node's bytes differ from the previous node's
///   (alpha included — absent next to a colour is a change). A joint is
///   `joints.px` long, centred on the node: each half is **drawn only if the
///   run on its side is at least two joints long** (`jointsFit` — below that a
///   run would be all joint, a gap that looks like missing data), the run being
///   from the flagged node (or the end of the well) before to the one after:
///   a stretch in `Nodes`, a stretch of one colour in `Changes` (05/10), so a
///   finely sampled well keeps its changes. A half lies on whatever stretches it
///   covers, and only where they are drawn. Alpha 0 leaves the joint's span empty;
///   otherwise the joint colour is composited over the stretch's colour **at
///   the node** (`rgb + (joint − rgb)·alpha`, the 3D rule — on a smooth stretch
///   the underlying colour moves a little across half a joint, and the node's
///   is used), precomputed: the pieces always carry opacity 1.
///
/// Consecutive pieces of one colour that touch are one piece.
pub fn well_pieces(path_px: &[[f64; 2]], paint: &Paint, joints: &Joint) -> Result<Vec<Piece>, String> {
    let Read { nodes: colours, joint } = read(path_px.len(), paint, joints)?;
    let count = path_px.len();
    if count < 2 {
        return Err(format!("a well of {count} node(s) has no stretch to paint"));
    }
    if let Some(k) = path_px.iter().position(|p| !(p[0].is_finite() && p[1].is_finite())) {
        return Err(format!("node {k} of the well has no position on the drawing"));
    }
    let absent = |k: usize| colours[k][3] < ABSENT_BELOW;
    // The 3D `jointFlags`, one per node.
    let flagged = |k: usize| -> bool {
        if k == 0 || k + 1 >= count {
            return false;
        }
        match joints.kind {
            JointKind::None => false,
            JointKind::Nodes => true,
            JointKind::Changes => colours[k] != colours[k - 1],
        }
    };
    let half = joints.px / 2.0;
    let alpha = joints.alpha.clamp(0.0, 1.0);

    // The abscissa of each node along the path, in pixels.
    let mut arc = vec![0.0; count];
    for k in 1..count {
        let (a, b) = (path_px[k - 1], path_px[k]);
        arc[k] = arc[k - 1] + ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
    }
    // Where the joints are, as spans of abscissa. A joint is judged on the
    // **run** its half lies in — from the flagged node (or the end of the
    // well) before it to the flagged node (or the end) after it — never on the
    // stretch between two nodes: in `Changes` a run is a stretch of one colour,
    // which on a finely sampled well (a node every metre) holds many
    // nodes; in `Nodes` every inner node is flagged and a run is a stretch, the
    // old rule exactly (in "at value change" mode the computation is done between
    // the changes, not between every available MD). a front end's `fragmentAt` says the same.
    let mut spans: Vec<(f64, f64)> = Vec::new();
    if joints.px > 0.0 {
        let flags: Vec<usize> = (0..count).filter(|&k| flagged(k)).collect();
        for (at, &c) in flags.iter().enumerate() {
            let before = if at == 0 { 0 } else { flags[at - 1] };
            let after = flags.get(at + 1).copied().unwrap_or(count - 1);
            if arc[c] - arc[before] >= 2.0 * joints.px {
                spans.push((arc[c] - half, arc[c]));
            }
            if arc[after] - arc[c] >= 2.0 * joints.px {
                spans.push((arc[c], arc[c] + half));
            }
        }
    }
    let in_joint = |s: f64| spans.iter().any(|&(lo, hi)| s > lo && s < hi);

    let mut out: Vec<Piece> = Vec::new();
    let mut push = |points: Vec<[f64; 2]>, colour: String| {
        if let Some(last) = out.last_mut()
            && last.colour == colour
            && last.points.last() == points.first()
        {
            last.points.extend(points.into_iter().skip(1));
        } else {
            out.push(Piece { points, colour, opacity: 1.0 });
        }
    };

    for k in 0..count - 1 {
        if absent(k) {
            continue;
        }
        let (a, b) = (path_px[k], path_px[k + 1]);
        let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        if len <= 1e-9 {
            continue;
        }
        let from = colours[k];
        let to = if paint.blend == Blend::Smooth && !absent(k + 1) { colours[k + 1] } else { from };
        // The stretch cut where a joint span begins or ends inside it, each
        // part either joint or paint.
        let (s0, s1) = (arc[k], arc[k + 1]);
        let mut cuts = vec![s0, s1];
        for &(lo, hi) in &spans {
            for edge in [lo, hi] {
                if edge > s0 && edge < s1 {
                    cuts.push(edge);
                }
            }
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
        for pair in cuts.windows(2) {
            let (u0, u1) = ((pair[0] - s0) / len, (pair[1] - s0) / len);
            if in_joint((pair[0] + pair[1]) / 2.0) {
                // Alpha 0 is a gap. Otherwise the joint over the paint at the
                // node the joint stands on — the end of the stretch it is near.
                if alpha > 0.0 {
                    let under = if (u0 + u1) / 2.0 < 0.5 { from } else { to };
                    push(vec![at(a, b, u0), at(a, b, u1)], hex_of(mix(under, joint, alpha)));
                }
            } else if to == from {
                push(vec![at(a, b, u0), at(a, b, u1)], hex_of(mix(from, from, 0.0)));
            } else {
                let n = (((u1 - u0) * len / SMOOTH_STEP_PX).ceil() as usize).clamp(1, SMOOTH_MOST);
                for j in 0..n {
                    let (v0, v1) = (u0 + (u1 - u0) * j as f64 / n as f64, u0 + (u1 - u0) * (j + 1) as f64 / n as f64);
                    // The colour at the middle of the sub-piece, along the whole stretch.
                    push(vec![at(a, b, v0), at(a, b, v1)], hex_of(mix(from, to, (v0 + v1) / 2.0)));
                }
            }
        }
    }
    Ok(out)
}

/// `colour` at `opacity` composited over `under` (both `#rrggbb`): the one
/// colour the SVG writer can say, which has no opacity on a line. A colour
/// that does not read is returned as it came.
pub fn over(colour: &str, opacity: f64, under: &str) -> String {
    if opacity >= 1.0 {
        return colour.to_string();
    }
    match (parse_colour(colour), parse_colour(under)) {
        (Some(top), Some(bottom)) => hex_of(mix(bottom, top, opacity.clamp(0.0, 1.0))),
        _ => colour.to_string(),
    }
}
