//! Moving a property's values between the points and the cells of a shape.
//!
//! A property hangs where the file says it hangs, and that is rarely where
//! somebody wants to look at it: a per-node temperature asked for on the faces,
//! a per-cell facies asked for on the points. VTK calls the two directions
//! `vtkPointDataToCellData` and `vtkCellDataToPointData`; RESQML says nothing
//! about either, because it describes what the data *is*, not what a viewer
//! does with it.
//!
//! So this is **ours**, and it is written where the topology is. Nothing else
//! in the workspace knows which nodes a cell has: [`Drawable`] carries triangles
//! and segments, which are the *drawing* of a shape and not its cells —
//! `cell_count()` counts hexahedra for a grid and runs for a polyline set. The
//! map lives in [`Topology`], so the transfer lives beside it and every consumer
//! — the exporters, the command line, the browser, the desktop — gets the same
//! one.
//!
//! [`Drawable`]: crate::geometry::Drawable
//!
//! # The trap, and why there is more than one method
//!
//! Averaging is right for a measurement and **wrong for a category**. A facies
//! property whose two nodes hold 0 and 3 averages to 1.5, which is not a
//! neighbouring shade of anything: it is a different rock, or no rock at all,
//! and the picture that comes out is confident and false. So a discrete
//! property transfers by [`Method::Majority`] — the value most of the nodes
//! actually hold — and the caller says which it wants rather than getting a
//! default that is right half the time.
//!
//! `NaN` is a value the corpus uses to mean *no data here*. It is skipped
//! rather than propagated: one undefined corner would otherwise blank a whole
//! cell. An element with nothing but `NaN` around it comes back `NaN`, which is
//! the honest answer and is what a colour map's null colour is for.

use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::geometry::{Geometry, Topology};

/// Which elements values hang on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    /// One value per point.
    Points,
    /// One value per cell — a triangle, a polyline, a hexahedron, a grid cell.
    Cells,
}

impl std::fmt::Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Scope::Points => "points",
            Scope::Cells => "cells",
        })
    }
}

/// How several values become one.
///
/// Named rather than inferred: a caller that knows its property is a category
/// asks for [`Method::Majority`], and one that knows it is a measurement asks
/// for [`Method::Average`]. Guessing from the values would work until the day a
/// facies happened to be stored as a float.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Method {
    /// The arithmetic mean of the values that are numbers.
    ///
    /// For a measurement. This is what `vtkPointDataToCellData` does, and what
    /// a smooth picture of a continuous field needs.
    Average,
    /// The value held by most of the contributing elements.
    ///
    /// For a category, where the mean of two indices names a third thing. Ties
    /// go to the **smallest** value, so the same input always gives the same
    /// picture — an arbitrary rule, but a stated and stable one.
    Majority,
    /// The smallest value.
    Minimum,
    /// The largest value.
    Maximum,
}

impl Method {
    /// The name a caller passes and a report prints.
    pub fn name(self) -> &'static str {
        match self {
            Method::Average => "average",
            Method::Majority => "majority",
            Method::Minimum => "minimum",
            Method::Maximum => "maximum",
        }
    }

    /// Every method this build offers, for a chooser to list.
    pub fn all() -> [Method; 4] {
        [
            Method::Average,
            Method::Majority,
            Method::Minimum,
            Method::Maximum,
        ]
    }

    /// Read a name, refusing an unknown one **by listing what there is**.
    pub fn parse(name: &str) -> Result<Method> {
        Method::all()
            .into_iter()
            .find(|method| method.name().eq_ignore_ascii_case(name.trim()))
            .ok_or_else(|| {
                Error::unsupported(
                    "a transfer method",
                    format!(
                        "`{name}` is not one of {}",
                        Method::all().map(|m| m.name()).join_with(", ")
                    ),
                )
            })
    }

    /// Reduce the values gathered for one element.
    ///
    /// `NaN` never reaches here — the caller filters — so an empty slice means
    /// *nothing was defined around this element*, which is `NaN` and not zero.
    fn reduce(self, gathered: &[f64]) -> f64 {
        if gathered.is_empty() {
            return f64::NAN;
        }
        match self {
            Method::Average => gathered.iter().sum::<f64>() / gathered.len() as f64,
            Method::Minimum => gathered.iter().copied().fold(f64::INFINITY, f64::min),
            Method::Maximum => gathered.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            Method::Majority => {
                // Counted on the bit pattern so that two equal values are one
                // key whatever arithmetic produced them, and the winner is
                // resolved by *value* on a tie — the smallest — so the same
                // input always paints the same picture.
                let mut counts: HashMap<u64, (usize, f64)> = HashMap::new();
                for value in gathered {
                    let entry = counts.entry(value.to_bits()).or_insert((0, *value));
                    entry.0 += 1;
                }
                counts
                    .values()
                    .copied()
                    .reduce(|best, next| match next.0.cmp(&best.0) {
                        std::cmp::Ordering::Greater => next,
                        std::cmp::Ordering::Equal if next.1 < best.1 => next,
                        _ => best,
                    })
                    .map(|(_, value)| value)
                    .unwrap_or(f64::NAN)
            }
        }
    }
}

/// A tiny helper so the refusal above reads as a sentence.
trait JoinWith {
    fn join_with(self, separator: &str) -> String;
}

impl<const N: usize> JoinWith for [&'static str; N] {
    fn join_with(self, separator: &str) -> String {
        self.join(separator)
    }
}

/// The nodes of every cell, in `cell_count()` order, numbered into the
/// representation's own points.
///
/// The one place [`crate::geometry::Patch::node_offset`] is applied for cells:
/// a `Topology` indexes its **patch's** points, a property indexes the
/// representation's. Getting that wrong silently colours patch 2 with patch 1's
/// numbers, which on a two-patch surface looks like a plausible picture.
///
/// A grid's cells are derived rather than stored — `(fastest × slowest)` points
/// give `(fastest-1) × (slowest-1)` quadrilaterals, in the same row-major order
/// `cell_count()` uses.
pub fn cell_nodes(geometry: &Geometry) -> Vec<Vec<u32>> {
    let mut out = Vec::with_capacity(geometry.cell_count());
    for patch in &geometry.patches {
        let base = patch.node_offset as u32;
        let shift = |nodes: &[u32]| nodes.iter().map(|n| n + base).collect::<Vec<u32>>();
        match &patch.topology {
            Topology::Points => {}
            Topology::Triangles(triangles) => out.extend(triangles.iter().map(|t| shift(t))),
            Topology::Quads(quads) => out.extend(quads.iter().map(|q| shift(q))),
            Topology::Polygons(rings) => out.extend(rings.iter().map(|r| shift(r))),
            // A wireframe's cell is one edge: two nodes, like a tiny run.
            Topology::Segments(pairs) => out.extend(pairs.iter().map(|p| shift(p))),
            Topology::Hexahedra { cells, .. } => out.extend(cells.iter().map(|c| shift(c))),
            Topology::Polylines(runs) => {
                // A run is one cell: the whole polyline, not each segment —
                // `cell_count()` counts runs, and a per-cell property of a
                // polyline set has one value per line.
                let mut start = 0u32;
                for length in runs {
                    out.push((start..start + length).map(|n| n + base).collect());
                    start += length;
                }
            }
            Topology::Grid { fastest, slowest } => {
                for row in 0..slowest.saturating_sub(1) {
                    for column in 0..fastest.saturating_sub(1) {
                        let corner = (row * fastest + column) as u32;
                        let next = (row * fastest + column + fastest) as u32;
                        out.push(vec![
                            corner + base,
                            corner + 1 + base,
                            next + 1 + base,
                            next + base,
                        ]);
                    }
                }
            }
            Topology::Polyhedra(cells) => {
                for cell in 0..cells.cell_count() {
                    // The nodes of a cell are the nodes of its faces, and a
                    // node on an edge belongs to several of them: deduplicated,
                    // or a corner would weigh three times as much as a centre
                    // in an average.
                    let mut nodes: Vec<u32> = cells
                        .cell(cell)
                        .flat_map(|(face, _)| cells.face(face).iter().map(|n| n + base))
                        .collect();
                    nodes.sort_unstable();
                    nodes.dedup();
                    out.push(nodes);
                }
            }
        }
    }
    out
}

/// Move values from one scope to the other.
///
/// `components` is the width of one element's value; every component is
/// transferred independently, so a vector property stays a vector property.
///
/// Same scope in and out is not an error and not a copy of convenience: it is
/// the case a caller hits whenever the user's choice happens to match what the
/// file says, and refusing it would push that test into every call site.
pub fn transfer(
    values: &[f64],
    components: usize,
    from: Scope,
    to: Scope,
    method: Method,
    geometry: &Geometry,
) -> Result<Vec<f64>> {
    let components = components.max(1);
    let point_count = geometry.point_count();
    let cell_count = geometry.cell_count();

    let expected = match from {
        Scope::Points => point_count,
        Scope::Cells => cell_count,
    };
    if values.len() != expected * components {
        return Err(Error::malformed(
            "transfer",
            format!(
                "{} value(s) for {expected} {from} × {components} component(s); \
                 a transfer cannot invent the difference",
                values.len()
            ),
        ));
    }
    if from == to {
        return Ok(values.to_vec());
    }
    if cell_count == 0 {
        return Err(Error::unsupported(
            "transferring between points and cells",
            "this shape has no cells: loose points have nothing to average over".to_string(),
        ));
    }

    let map = cell_nodes(geometry);
    let mut gathered: Vec<f64> = Vec::new();

    match to {
        // One value per cell, from the nodes it has.
        Scope::Cells => {
            let mut out = Vec::with_capacity(cell_count * components);
            for nodes in &map {
                for component in 0..components {
                    gathered.clear();
                    gathered.extend(
                        nodes
                            .iter()
                            .filter_map(|node| values.get(*node as usize * components + component))
                            .copied()
                            .filter(|v| !v.is_nan()),
                    );
                    out.push(method.reduce(&gathered));
                }
            }
            Ok(out)
        }
        // One value per point, from the cells that touch it. A point no cell
        // uses — a loose node of a patch that also has cells — comes back
        // `NaN`, not zero.
        Scope::Points => {
            let mut touching: Vec<Vec<usize>> = vec![Vec::new(); point_count];
            for (cell, nodes) in map.iter().enumerate() {
                for node in nodes {
                    if let Some(slot) = touching.get_mut(*node as usize) {
                        slot.push(cell);
                    }
                }
            }
            let mut out = Vec::with_capacity(point_count * components);
            for cells in &touching {
                for component in 0..components {
                    gathered.clear();
                    gathered.extend(
                        cells
                            .iter()
                            .filter_map(|cell| values.get(cell * components + component))
                            .copied()
                            .filter(|v| !v.is_nan()),
                    );
                    out.push(method.reduce(&gathered));
                }
            }
            Ok(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Patch;
    use crate::points::Point;

    fn grid(fastest: usize, slowest: usize) -> Geometry {
        Geometry {
            kind: "test",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points: (0..fastest * slowest)
                    .map(|i| -> Point { [(i % fastest) as f64, (i / fastest) as f64, 0.0] })
                    .collect(),
                topology: Topology::Grid { fastest, slowest },
                closed: Vec::new(),
            }],
        }
    }

    #[test]
    fn a_grid_cell_has_its_four_corners() {
        // 3×2 points make two cells, and cell 0 is the quadrilateral
        // (0, 1, 4, 3) — the row below is `+ fastest`. Getting this ring wrong
        // averages the wrong corners and draws a plausible picture.
        let map = cell_nodes(&grid(3, 2));
        assert_eq!(map.len(), 2);
        assert_eq!(map[0], vec![0, 1, 4, 3]);
        assert_eq!(map[1], vec![1, 2, 5, 4]);
    }

    #[test]
    fn a_patch_offset_is_applied_to_cells() {
        // Two patches: a `Topology` numbers its own points, a property numbers
        // the representation's. Without the offset, patch 1's cells would
        // point at patch 0's nodes — the same picture, the wrong values.
        let mut geometry = grid(2, 2);
        geometry.patches.push(Patch {
            index: 1,
            node_offset: 4,
            points: (0..4).map(|_| -> Point { [0.0, 0.0, 0.0] }).collect(),
            topology: Topology::Triangles(vec![[0, 1, 2]]),
            closed: Vec::new(),
        });
        let map = cell_nodes(&geometry);
        assert_eq!(map.len(), 2);
        assert_eq!(map[1], vec![4, 5, 6], "the second patch counts from its offset");
    }

    #[test]
    fn a_measurement_averages_onto_the_cells() {
        // 3×2 points holding 0..5; cell 0 has corners 0, 1, 4, 3 → mean 2.
        let geometry = grid(3, 2);
        let values: Vec<f64> = (0..6).map(|i| i as f64).collect();
        let cells = transfer(
            &values,
            1,
            Scope::Points,
            Scope::Cells,
            Method::Average,
            &geometry,
        )
        .unwrap();
        assert_eq!(cells, vec![2.0, 3.0]);
    }

    #[test]
    fn a_category_takes_the_majority_and_never_the_mean() {
        // **The trap this module exists for.** Three corners of facies 0 and
        // one of facies 3: the mean is 0.75, which is not a facies at all. The
        // majority is 0, which is a rock.
        let geometry = grid(3, 2);
        let values = vec![0.0, 0.0, 9.0, 0.0, 3.0, 9.0];
        let mean = transfer(
            &values,
            1,
            Scope::Points,
            Scope::Cells,
            Method::Average,
            &geometry,
        )
        .unwrap();
        let vote = transfer(
            &values,
            1,
            Scope::Points,
            Scope::Cells,
            Method::Majority,
            &geometry,
        )
        .unwrap();
        assert_eq!(mean[0], 0.75, "the mean of 0, 0, 3, 0 — not one of the values");
        assert_eq!(vote[0], 0.0, "the value three of the four corners hold");
        // And the two really do differ, so this is not passing on a constant.
        assert_ne!(mean[0], vote[0]);
    }

    #[test]
    fn a_tie_goes_to_the_smallest_value() {
        // Arbitrary but stated: the same input must always paint the same
        // picture, and a HashMap's order is not a rule.
        let geometry = grid(2, 2);
        let values = vec![7.0, 7.0, 2.0, 2.0];
        let vote = transfer(
            &values,
            1,
            Scope::Points,
            Scope::Cells,
            Method::Majority,
            &geometry,
        )
        .unwrap();
        assert_eq!(vote, vec![2.0]);
    }

    #[test]
    fn an_undefined_corner_does_not_blank_the_cell() {
        // `NaN` is the corpus's way of saying *no data here*. Propagating it
        // through the mean would lose a cell for one missing corner.
        let geometry = grid(3, 2);
        let values = vec![0.0, f64::NAN, 4.0, 2.0, 6.0, 8.0];
        let cells = transfer(
            &values,
            1,
            Scope::Points,
            Scope::Cells,
            Method::Average,
            &geometry,
        )
        .unwrap();
        assert_eq!(cells[0], (0.0 + 2.0 + 6.0) / 3.0);
    }

    #[test]
    fn an_element_with_nothing_defined_around_it_is_not_zero() {
        // The distinction that matters: no data and a reading of zero are not
        // the same thing, and a colour map has a null colour for the first.
        let geometry = grid(2, 2);
        let values = vec![f64::NAN; 4];
        let cells = transfer(
            &values,
            1,
            Scope::Points,
            Scope::Cells,
            Method::Average,
            &geometry,
        )
        .unwrap();
        assert!(cells[0].is_nan(), "got {}", cells[0]);
    }

    #[test]
    fn the_trip_back_lands_on_the_points() {
        // Cells → points: each point takes the cells that touch it. Corner 0
        // touches only cell 0; the middle of the top row touches both.
        let geometry = grid(3, 2);
        let cells = vec![10.0, 20.0];
        let points = transfer(&cells, 1, Scope::Cells, Scope::Points, Method::Average, &geometry).unwrap();
        assert_eq!(points.len(), 6);
        assert_eq!(points[0], 10.0, "a corner of one cell");
        assert_eq!(points[1], 15.0, "a node between the two cells");
        assert_eq!(points[2], 20.0, "a corner of the other cell");
    }

    #[test]
    fn every_component_travels_on_its_own() {
        // A vector property stays a vector property: mixing the components
        // would average a velocity's x into its y and look like a picture.
        let geometry = grid(3, 2);
        let values: Vec<f64> = (0..6).flat_map(|i| [i as f64, 100.0 + i as f64]).collect();
        let cells = transfer(
            &values,
            2,
            Scope::Points,
            Scope::Cells,
            Method::Average,
            &geometry,
        )
        .unwrap();
        assert_eq!(cells, vec![2.0, 102.0, 3.0, 103.0]);
    }

    #[test]
    fn a_count_that_does_not_line_up_is_refused_with_both_numbers() {
        let geometry = grid(3, 2);
        let refused = transfer(
            &[1.0, 2.0],
            1,
            Scope::Points,
            Scope::Cells,
            Method::Average,
            &geometry,
        )
        .expect_err("2 values for 6 points");
        let message = refused.to_string();
        assert!(message.contains('2') && message.contains('6'), "{message}");
    }

    #[test]
    fn the_same_scope_is_a_pass_through() {
        let geometry = grid(3, 2);
        let values: Vec<f64> = (0..6).map(|i| i as f64).collect();
        let same = transfer(
            &values,
            1,
            Scope::Points,
            Scope::Points,
            Method::Average,
            &geometry,
        )
        .unwrap();
        assert_eq!(same, values);
    }

    #[test]
    fn an_unknown_method_is_refused_by_listing_the_known_ones() {
        let refused = Method::parse("smooth").expect_err("no such method");
        let message = refused.to_string();
        assert!(message.contains("average"), "{message}");
        assert!(message.contains("majority"), "{message}");
    }

    #[test]
    fn a_polyline_run_is_one_cell() {
        // `cell_count()` counts runs, so a per-cell property of a polyline set
        // has one value per **line** and not per segment.
        let geometry = Geometry {
            kind: "test",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points: (0..5).map(|_| -> Point { [0.0, 0.0, 0.0] }).collect(),
                topology: Topology::Polylines(vec![3, 2]),
                closed: Vec::new(),
            }],
        };
        let map = cell_nodes(&geometry);
        assert_eq!(map, vec![vec![0, 1, 2], vec![3, 4]]);
        let points = transfer(
            &[1.0, 5.0],
            1,
            Scope::Cells,
            Scope::Points,
            Method::Average,
            &geometry,
        )
        .unwrap();
        assert_eq!(points, vec![1.0, 1.0, 1.0, 5.0, 5.0]);
    }
}
