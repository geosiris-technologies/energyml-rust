//! The section fills, put back on the trace — the coloured panels of the 3D
//! view (ADR-0015).
//!
//! The 3D surfaces are created from the 2D view being drawn. Their colouring
//! is chosen by the user, for each surface line projected in the 2D section
//! view, as to whether the colour propagates up to the line above, the line
//! below, or both.
//!
//! The panels are the **polygons of the drawing** — the same rule
//! (`crate::layout::fill::polygons`: a trace to its neighbour in each
//! abscissa, nothing where there is none), the same choices (`View::fills`), the same hidden traces,
//! the same colour — each vertex `(s, z)` put back at `point_at(trace, s)`, `z`
//! unchanged. Nothing is recomputed from a surface: the 3D shows what the 2D
//! drew.
//!
//! Every section of the catalogue gets its panels, whichever the view's target
//! is: the view carries the choices, the 3D shows every section. A view's
//! `portion` is a crop of one drawing and does not apply here.

use energyml_geometry::fence::point_at;
use serde::{Deserialize, Serialize};

use crate::layout::fill::{self, Span};
use crate::layout::trace_colour;
use crate::model::{Catalogue, FillChoice, Item, Section, SectionKey, TraceItem, View};

/// The filled polygons of one trace on one section, in the scene's `(x, y, z)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Panel {
    /// The section the polygons stand on.
    pub section: SectionKey,
    /// The trace (surface) they fill from, by key — `FillChoice::key`.
    pub key: String,
    /// Their colour: the drawing's.
    pub colour: String,
    /// Each polygon's vertices, not closed.
    pub polygons: Vec<Vec<[f64; 3]>>,
    /// The same polygons in the section's own `(s, z)`, vertex for vertex —
    /// where a polygon is flat, so where it is triangulated: unrolled from its
    /// 3D vertices instead, a trace folding back on itself would fold it too.
    pub sz: Vec<Vec<[f64; 2]>>,
}

/// The panels of every section of `catalogue` for the fills `view` chooses.
pub fn panels(catalogue: &Catalogue, view: &View) -> Vec<Panel> {
    let mut out = Vec::new();
    if view.fills.is_empty() {
        return out;
    }
    for section in std::iter::once(&catalogue.long).chain(catalogue.crosses.iter()) {
        of_section(section, view, &mut out);
    }
    out
}

fn of_section(section: &Section, view: &View, out: &mut Vec<Panel>) {
    // What the drawing keeps: a hidden trace is neither filled nor a neighbour.
    let mut kept: Vec<(String, &TraceItem, String, Vec<Span>)> = Vec::new();
    for item in &section.items {
        let Item::Trace(t) = item else { continue };
        let key = t.object.key();
        if view.hidden.contains(&key) {
            continue;
        }
        let spans: Vec<Span> = t.spans.iter().filter(|s| s.len() >= 2).cloned().collect();
        if spans.is_empty() {
            continue;
        }
        // The drawing's cascade: the scene, the file, then the derived hue.
        let colour = trace_colour(view, t);
        kept.push((key, t, colour, spans));
    }
    let lines: Vec<fill::Line<'_>> = kept
        .iter()
        .map(|(k, item, _, s)| fill::Line {
            key: k,
            item,
            spans: s,
        })
        .collect();
    for ((key, _, colour, _), mine) in kept.iter().zip(&lines) {
        let Some(FillChoice { up, down, .. }) = view.fills.iter().find(|f| f.key == *key) else {
            continue;
        };
        let mut polygons = Vec::new();
        let mut sz = Vec::new();
        for (wanted, is_up) in [(*up, true), (*down, false)] {
            if !wanted {
                continue;
            }
            for poly in fill::polygons(mine, &lines, is_up) {
                polygons.push(
                    poly.iter()
                        .map(|&[s, z]| {
                            let [x, y] = point_at(&section.trace, s);
                            [x, y, z]
                        })
                        .collect(),
                );
                sz.push(poly);
            }
        }
        if !polygons.is_empty() {
            out.push(Panel {
                section: section.key,
                key: key.clone(),
                colour: colour.clone(),
                polygons,
                sz,
            });
        }
    }
}
