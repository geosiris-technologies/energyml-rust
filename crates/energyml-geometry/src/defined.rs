//! Nodes that have no position, and what a drawing may do with them.
//!
//! # What the standard says, and what it does not
//!
//! Energistics fixes the null value of a floating-point array without
//! ambiguity: *"An array of double values provided explicitly by an HDF5
//! dataset. **By convention, the null value is NaN.**"* (Energistics common
//! TRG v2.3 §6.31 `FloatingPointExternalArray`, and the same sentence on
//! RESQML 2.0.1's `DoubleHdf5Array`). Contrast the integer case, where the null
//! *"must be explicitly provided in the `NullValue` attribute"* (§6.41) — for
//! doubles it is NaN, always, and no attribute says otherwise.
//!
//! So a `Point3dZValueArray` whose `ZValues` holds NaN is holding **nulls**,
//! and that much is quotable. What is *not* quotable is the next step: no
//! clause in RESQML 2.0.1 or 2.2 says that a null Z makes a node undefined, and
//! none says that a cell touching an undefined node must not be drawn. For
//! `Grid2dRepresentation` the specification is entirely silent; the mechanism
//! it does give for *"the surface stops here"* is topological —
//! `PatchBoundaries`' `OuterRing` / `InnerRing`, *"inside the ring, the patch
//! is defined, outside it is not"* (RESQML TRG v2.2 §11.12).
//!
//! For column-layer grids the standard **is** explicit, and it is worth quoting
//! because it settles what wins:
//!
//! > `PillarGeometryIsDefined` — *"Indicator that a pillar has at least one
//! > node with a defined cell geometry. **If the indicator does not indicate
//! > that the pillar geometry is defined, then this over-rides any other node
//! > geometry specification.**"* (TRG v2.2 §5.1, mandatory)
//!
//! > `CellGeometryIsDefined` — *"Indicator that a cell has a defined geometry.
//! > **If the indicator shows that the cell geometry is NOT defined, then this
//! > attribute overrides any other node geometry specification.**"* (§5.1,
//! > optional)
//!
//! Note what those do not say: they never promise the coordinates will be NaN.
//! They say the **flag beats the coordinates**. See [`crate::ijk`], which reads
//! them.
//!
//! # What this module decides, and on what ground
//!
//! A node whose position is NaN has **no position**. That is not an
//! interpretation of RESQML, it is arithmetic: there is no point in space to
//! put it at, no edge to draw to it, and no cell it can be a corner of. Every
//! output format this workspace writes demands three numbers per vertex, and
//! two of the three have no syntax for a missing one — GeoJSON is JSON, and
//! **`NaN` is not a JSON value**, so a single null Z made the whole file
//! unparseable by `JSON.parse`.
//!
//! Therefore: **an undefined node is dropped, and so is everything indexed on
//! it.** Recorded here as a reader's decision rather than dressed up as a
//! citation.
//!
//! Two consequences that are deliberate, not oversights:
//!
//! - **A lattice with a hole is not a lattice.** [`Topology::Grid`] promises a
//!   `fastest × slowest` rectangle, row by row; drop one node and every index
//!   after it means something else. So a grid that loses a node becomes
//!   [`Topology::Points`] — a demotion the caller can see, rather than a
//!   rectangle that lies about its own shape.
//! - **Properties move with their elements.** Dropping points without dropping
//!   the matching property values would colour the survivors with their
//!   neighbours' numbers, which is exactly the plausible-picture-of-nothing this
//!   workspace refuses elsewhere. [`Compaction`] therefore carries the index
//!   maps, and [`crate::scene::SceneObject::drawable_only`] uses them.

use crate::geometry::{Geometry, Patch, Topology};
use crate::points::Point;

/// Whether a point can be drawn: three finite numbers.
///
/// Infinity is refused alongside NaN. It is not a null the standard defines,
/// but it is equally undrawable, and a viewer handed one will either clip the
/// scene to nothing or refuse the file.
pub fn is_defined(point: &Point) -> bool {
    point.iter().all(|c| c.is_finite())
}

/// A geometry with its undefined nodes removed, and the maps to follow it.
///
/// The maps are in **merged order** — the order [`Geometry::merged`] and
/// [`Geometry::cell_count`] produce, patch after patch — because that is the
/// order an attribute's values are in.
#[derive(Debug, Clone, PartialEq)]
pub struct Compaction {
    /// The geometry, with undefined nodes and everything touching them gone.
    pub geometry: Geometry,
    /// For each surviving point, its index in the original merged order.
    pub points: Vec<usize>,
    /// For each surviving cell, its index in the original cell order.
    pub cells: Vec<usize>,
    /// How many nodes had no position.
    pub dropped_points: usize,
    /// How many cells touched one.
    pub dropped_cells: usize,
}

impl Geometry {
    /// How many of this geometry's nodes have no position.
    pub fn undefined_count(&self) -> usize {
        self.patches
            .iter()
            .flat_map(|patch| patch.points.iter())
            .filter(|point| !is_defined(point))
            .count()
    }

    /// This geometry with every undefined node removed — or `None` when there
    /// is none, which is the ordinary case and costs one pass and no copy.
    ///
    /// `None` rather than a clone of `self` on purpose: the caller can then
    /// keep borrowing the original, and the overwhelmingly common package pays
    /// nothing for a rule that exists for the others.
    pub fn defined_only(&self) -> Option<Compaction> {
        if self.undefined_count() == 0 {
            return None;
        }

        let mut patches = Vec::with_capacity(self.patches.len());
        let mut kept_points = Vec::new();
        let mut kept_cells = Vec::new();
        let mut point_base = 0usize;
        let mut cell_base = 0usize;
        let mut node_offset = 0usize;
        let mut dropped_points = 0usize;
        let mut dropped_cells = 0usize;

        for patch in &self.patches {
            // An unstructured grid is left **whole**, points included. Its
            // topology is a face/node index structure with shared faces, and
            // renumbering it correctly means rebuilding the face table and the
            // cell-to-face map. Dropping its points without that would leave
            // every face pointing at the wrong node — far worse than leaving
            // one undrawable cell in place. Refused loudly rather than
            // half-done; see the module documentation.
            let compactable = !matches!(patch.topology, Topology::Polyhedra(_));

            // `keep[i]` is the new index of old point `i`, or `None`.
            let mut keep = Vec::with_capacity(patch.points.len());
            let mut points = Vec::with_capacity(patch.points.len());
            for (index, point) in patch.points.iter().enumerate() {
                if !compactable || is_defined(point) {
                    keep.push(Some(points.len() as u32));
                    points.push(*point);
                    kept_points.push(point_base + index);
                } else {
                    keep.push(None);
                    dropped_points += 1;
                }
            }

            let (topology, kept_here, dropped_here) = compact_topology(&patch.topology, &keep, cell_base);
            kept_cells.extend(kept_here);
            dropped_cells += dropped_here;

            let points_len = points.len();
            patches.push(Patch {
                index: patch.index,
                // Renumbered, because the patches before this one shrank too.
                // Leaving the original here would send anything that indexes
                // *nodes of the representation* to the wrong patch entirely.
                node_offset,
                points,
                topology,
                closed: Vec::new(),
            });
            point_base += patch.points.len();
            cell_base += cells_in(&patch.topology);
            node_offset += points_len;
        }

        Some(Compaction {
            geometry: Geometry {
                kind: self.kind,
                patches,
            },
            points: kept_points,
            cells: kept_cells,
            dropped_points,
            dropped_cells,
        })
    }
}

/// How many cells a topology has — [`Topology::cell_count`], which became the
/// one definition when the drawable's cell table needed it too; this wrapper
/// stays for the callers' sake.
fn cells_in(topology: &Topology) -> usize {
    topology.cell_count()
}

/// Renumber one patch's topology onto the surviving points.
///
/// Returns the new topology, the original indices of the cells that survived,
/// and how many were dropped.
fn compact_topology(
    topology: &Topology,
    keep: &[Option<u32>],
    cell_base: usize,
) -> (Topology, Vec<usize>, usize) {
    let at = |index: u32| keep.get(index as usize).copied().flatten();

    match topology {
        Topology::Points => (Topology::Points, Vec::new(), 0),

        // A quadrilateral touching an undefined node goes with it, exactly
        // like a triangle. This is where "the absent cell is not
        // represented" finally means something visible: a grid drawn as cells
        // loses the four around a null node, not one dot among 300 000.
        Topology::Quads(quads) => {
            let mut out = Vec::with_capacity(quads.len());
            let mut kept = Vec::with_capacity(quads.len());
            let mut dropped = 0;
            for (index, q) in quads.iter().enumerate() {
                let mut ring = [0u32; 4];
                let mut whole = true;
                for (slot, corner) in ring.iter_mut().zip(q.iter()) {
                    match at(*corner) {
                        Some(new) => *slot = new,
                        None => {
                            whole = false;
                            break;
                        }
                    }
                }
                if whole {
                    out.push(ring);
                    kept.push(cell_base + index);
                } else {
                    dropped += 1;
                }
            }
            (Topology::Quads(out), kept, dropped)
        }

        // Same rule as a quadrilateral, one length up.
        Topology::Polygons(rings) => {
            let mut out = Vec::with_capacity(rings.len());
            let mut kept = Vec::with_capacity(rings.len());
            let mut dropped = 0;
            for (index, ring) in rings.iter().enumerate() {
                let renumbered: Option<Vec<u32>> = ring.iter().map(|corner| at(*corner)).collect();
                match renumbered {
                    Some(ring) => {
                        out.push(ring);
                        kept.push(cell_base + index);
                    }
                    None => dropped += 1,
                }
            }
            (Topology::Polygons(out), kept, dropped)
        }

        Topology::Triangles(triangles) => {
            let mut out = Vec::with_capacity(triangles.len());
            let mut kept = Vec::with_capacity(triangles.len());
            let mut dropped = 0;
            for (index, t) in triangles.iter().enumerate() {
                match (at(t[0]), at(t[1]), at(t[2])) {
                    (Some(a), Some(b), Some(c)) => {
                        out.push([a, b, c]);
                        kept.push(cell_base + index);
                    }
                    _ => dropped += 1,
                }
            }
            (Topology::Triangles(out), kept, dropped)
        }

        // The same rule as a triangle, one dimension down: an edge touching an
        // undefined node goes with it, and its cell entry goes too.
        Topology::Segments(pairs) => {
            let mut out = Vec::with_capacity(pairs.len());
            let mut kept = Vec::with_capacity(pairs.len());
            let mut dropped = 0;
            for (index, pair) in pairs.iter().enumerate() {
                match (at(pair[0]), at(pair[1])) {
                    (Some(a), Some(b)) => {
                        out.push([a, b]);
                        kept.push(cell_base + index);
                    }
                    _ => dropped += 1,
                }
            }
            (Topology::Segments(out), kept, dropped)
        }

        // A run through an undefined node is **cut**, not bridged. Joining the
        // two survivors would draw a segment across the hole — a line the file
        // never described, and one that looks entirely plausible on screen.
        //
        // A cut run becomes several, so the one-value-per-run correspondence a
        // per-cell property relies on is gone. Rather than guess which piece
        // inherits the value, every run of an affected patch is reported as
        // dropped: an attribute on it will be refused by name, which is the
        // behaviour this workspace prefers to a coloured picture of nothing.
        Topology::Polylines(runs) => {
            let mut out = Vec::new();
            let mut start = 0u32;
            let mut cut = false;
            for run in runs {
                let mut length = 0u32;
                for offset in 0..*run {
                    match at(start + offset) {
                        Some(_) => length += 1,
                        None => {
                            if length > 0 {
                                out.push(length);
                            }
                            length = 0;
                            cut = true;
                        }
                    }
                }
                if length > 0 {
                    out.push(length);
                }
                start += run;
            }
            let dropped = if cut { runs.len() } else { 0 };
            let kept = if cut {
                Vec::new()
            } else {
                (0..runs.len()).map(|i| cell_base + i).collect()
            };
            (Topology::Polylines(out), kept, dropped)
        }

        Topology::Hexahedra {
            cells,
            dimensions,
            index,
        } => {
            let mut out = Vec::with_capacity(cells.len());
            let mut kept = Vec::with_capacity(cells.len());
            let mut dropped = 0;
            for (index, cell) in cells.iter().enumerate() {
                let mut corners = [0u32; 8];
                let mut whole = true;
                for (slot, corner) in corners.iter_mut().zip(cell.iter()) {
                    match at(*corner) {
                        Some(new) => *slot = new,
                        None => {
                            whole = false;
                            break;
                        }
                    }
                }
                if whole {
                    out.push(corners);
                    kept.push(cell_base + index);
                } else {
                    dropped += 1;
                }
            }
            // `dimensions` is kept as it was: it says what shape the cells were
            // *built from*, and that stays true. It is no longer the shape of
            // the cells that remain, and no consumer should read it as such —
            // the count is `cells.len()`.
            (
                Topology::Hexahedra {
                    cells: out,
                    dimensions: *dimensions,
                    // **Filtered with its cell**, never re-derived: this pass
                    // drops cells too, and a numbering rebuilt from the survivors
                    // would renumber them onto themselves — which is exactly the
                    // mistake the numbering exists to prevent. `kept` holds the
                    // positions that survived, in order.
                    index: kept
                        .iter()
                        .map(|at| index.get(*at).copied().unwrap_or(*at as u32))
                        .collect(),
                },
                kept,
                dropped,
            )
        }

        // A rectangle with a hole in it is not a rectangle. Demoted to loose
        // points, which is what the survivors honestly are; every cell of the
        // lattice is reported dropped because none of them can be addressed by
        // `(i, j)` any more.
        Topology::Grid { fastest, slowest } => (
            Topology::Points,
            Vec::new(),
            fastest.saturating_sub(1) * slowest.saturating_sub(1),
        ),

        // Left whole — see the `compactable` guard above, which also keeps this
        // patch's points, so these indices still address what they always did.
        Topology::Polyhedra(cells) => (
            Topology::Polyhedra(cells.clone()),
            (0..cells.cell_count()).map(|i| cell_base + i).collect(),
            0,
        ),
    }
}
