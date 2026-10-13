//! The cut of a painted surface: each point's colour from the two nodes of the
//! edge it lies on (`SurfaceCut::sources`).
//!
//! # What a point's colour is
//!
//! A point of a surface's cut lies on an edge of one of its triangles
//! (`node a + t·(node b − node a)`), and the front end's paint gives one colour per
//! **node**. So the colour of the point is read from the two ends of its edge,
//! by the paint's [`Blend`]:
//!
//! - **`Smooth`** (a continuous property): the two nodes' colours, mixed at
//!   `t` — component by component, alpha included — and, between two
//!   consecutive points of the cut, the colour runs from one to the other: the
//!   segment is split into sub-pieces, as a front end does along a well.
//! - **`Net`** (a discrete property — strata, facies): a colour is a category,
//!   and mixing two categories makes a third that nobody measured. The point
//!   takes the colour of the **nearer node of its edge** (`t < ½` is `a`, `t ≥
//!   ½` is `b`: a point halfway belongs to the node the edge ends on). On a
//!   segment whose two ends read the same colour, that colour holds; where they
//!   differ, the nearest-node regions of the triangle meet somewhere between
//!   the two points, which the cut does not say — so the segment is cut at its
//!   **middle**, each half in its end's colour. Symmetric (a span read from
//!   either end paints the same), and the one place the colour changes lies
//!   between the two points that disagree, never beyond them.
//!
//! # Gaps
//!
//! Alpha 0 on a node is *no value there* — a gap, never a colour. A point whose
//! edge carries a node of alpha 0 **with a weight** (the node `t = 0` leaves out
//! `b`, and `t = 1` leaves out `a`) has no colour; a segment with such a point
//! at either end is not drawn. The gap is the segment, never a line drawn
//! through it in some other colour.
//!
//! # Refusals
//!
//! By name, and then the span is drawn in the object's flat colour: a source
//! naming a node the paint does not hold (the paint and the cut do not number
//! the same nodes — `read::surfaces::Prepared::nodes`), sources that are not
//! one per point, a colour that does not read, a `t` that is not on its edge.

use crate::model::{Blend, CutSource, Paint};

use super::Piece;

/// The most pieces a smooth segment is cut into: a ramp's steps are invisible
/// beyond a few tens, and a long section holds many segments.
const MAX_STEPS: usize = 16;

/// A colour change under this per channel (out of 255) is not seen: one step.
const STEP: f64 = 12.0;

/// A colour as four channels in 0..=255 — a float, so a mix is not rounded
/// twice.
type Rgba = [f64; 4];

/// One span of a cut, already laid in pixels, and the source of each of its
/// points: the pieces to draw, or why the paint does not fit — said by name,
/// and the span is then drawn in the object's flat colour.
pub fn trace_pieces(span_px: &[[f64; 2]], sources: &[CutSource], paint: &Paint) -> Result<Vec<Piece>, String> {
    if sources.len() != span_px.len() {
        return Err(format!(
            "the cut has {} point(s) and {} source(s): the paint cannot be laid on it",
            span_px.len(),
            sources.len()
        ));
    }
    // Every point's colour first: a refusal is for the whole span, never a
    // part of it drawn and the rest flat.
    let mut colours: Vec<Option<Rgba>> = Vec::with_capacity(sources.len());
    for (at, source) in sources.iter().enumerate() {
        colours.push(point_colour(at, source, paint)?);
    }

    let mut pieces: Vec<Piece> = Vec::new();
    for at in 0..span_px.len().saturating_sub(1) {
        let (Some(from), Some(to)) = (colours[at], colours[at + 1]) else {
            continue;
        };
        let (p, q) = (span_px[at], span_px[at + 1]);
        match paint.blend {
            Blend::Net => {
                if hex_of(from) == hex_of(to) && from[3] == to[3] {
                    push(&mut pieces, vec![p, q], from);
                } else {
                    let middle = [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0];
                    push(&mut pieces, vec![p, middle], from);
                    push(&mut pieces, vec![middle, q], to);
                }
            }
            Blend::Smooth => {
                let spread = (0..4).map(|k| (from[k] - to[k]).abs()).fold(0.0_f64, f64::max);
                let steps = ((spread / STEP).ceil() as usize).clamp(1, MAX_STEPS);
                for step in 0..steps {
                    let (t0, t1) = (step as f64 / steps as f64, (step + 1) as f64 / steps as f64);
                    let at_t = |t: f64| [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t];
                    // A sub-piece wears the colour at its middle.
                    let colour = mix(from, to, (t0 + t1) / 2.0);
                    push(&mut pieces, vec![at_t(t0), at_t(t1)], colour);
                }
            }
        }
    }
    Ok(pieces)
}

/// Add a piece, joining it to the previous one when it continues it in the
/// very same colour: a run of one colour is one polyline, not a heap of
/// segments.
fn push(pieces: &mut Vec<Piece>, points: Vec<[f64; 2]>, colour: Rgba) {
    let (hex, opacity) = (hex_of(colour), colour[3] / 255.0);
    if let Some(last) = pieces.last_mut()
        && last.colour == hex
        && last.opacity == opacity
        && last.points.last() == points.first()
    {
        last.points.extend(points.into_iter().skip(1));
        return;
    }
    pieces.push(Piece { points, colour: hex, opacity });
}

/// The colour of one point of the cut — `None` where the paint has no value
/// (a node of alpha 0 the point leans on).
fn point_colour(at: usize, source: &CutSource, paint: &Paint) -> Result<Option<Rgba>, String> {
    if !source.t.is_finite() || !(-1e-9..=1.0 + 1e-9).contains(&source.t) {
        return Err(format!("point {at}: t = {} is not a place on its edge (0 to 1)", source.t));
    }
    let t = source.t.clamp(0.0, 1.0);
    let node = |number: u32| -> Result<Rgba, String> {
        let text = paint.nodes.get(number as usize).ok_or_else(|| {
            format!(
                "point {at}: its edge names node {number}, and the paint holds {} node(s) — the paint and the cut do not number the same nodes",
                paint.nodes.len()
            )
        })?;
        parse(text).map_err(|why| format!("point {at}: the colour of node {number}: {why}"))
    };
    // Both ends are looked up even at `t = 0` or `1`: a node past the paint is
    // a numbering that is wrong, whatever the weight.
    let (a, b) = (node(source.a)?, node(source.b)?);
    let leans_on_a = t < 1.0;
    let leans_on_b = t > 0.0;
    if (leans_on_a && a[3] == 0.0) || (leans_on_b && b[3] == 0.0) {
        return Ok(None);
    }
    Ok(Some(match paint.blend {
        Blend::Smooth => mix(a, b, t),
        Blend::Net => {
            if t < 0.5 {
                a
            } else {
                b
            }
        }
    }))
}

fn mix(a: Rgba, b: Rgba, t: f64) -> Rgba {
    [0, 1, 2, 3].map(|k| a[k] + (b[k] - a[k]) * t)
}

/// `#rrggbbaa` (or `#rrggbb`, opaque).
fn parse(text: &str) -> Result<Rgba, String> {
    let digits = text.strip_prefix('#').ok_or_else(|| format!("`{text}` is not `#rrggbbaa`"))?;
    if !(digits.len() == 8 || digits.len() == 6) || !digits.is_ascii() {
        return Err(format!("`{text}` is not `#rrggbbaa`"));
    }
    let channel = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).map(f64::from);
    let read = |at: usize| channel(at).map_err(|_| format!("`{text}` is not `#rrggbbaa`"));
    let alpha = if digits.len() == 8 { read(6)? } else { 255.0 };
    Ok([read(0)?, read(2)?, read(4)?, alpha])
}

/// `#rrggbb`, lower case, rounded.
fn hex_of(colour: Rgba) -> String {
    let byte = |value: f64| value.round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", byte(colour[0]), byte(colour[1]), byte(colour[2]))
}
