//! `clip` - a shape cut by rings: what lies outside `outer`, or inside one of
//! the `inner` rings, goes.
//!
//! The need: an operation that applies inner and outer rings to point sets,
//! surfaces and so on, which avoids modifying the other
//! operations - the rings `interpolate` reads for MBA are one operation's
//! own; this one cuts **any** shape, after whichever operation made it.
//!
//! # What a cut does, by kind
//!
//! The same as `delete-nodes`, through the same function (`delete_nodes::erase`):
//! a cloud loses its nodes, a line is cut at them, a surface loses the
//! triangles that name one. **The cut follows the nodes, not the ring**: a
//! triangle straddling the ring goes whole, and no node is added on the ring's
//! edge - the boundary of the result is the mesh's own, said in the plan.
//!
//! A **2D grid keeps its lattice**: a cut node becomes a hole (`NaN`), the
//! grid's way of having no node there (`grid::write` counts and says them).
//! Its lattice is read by `energyml_analysis::ijk::lattice_of`, the reader
//! `interpolate` uses for `onto`, so a lattice turned in its frame's numbers is
//! refused the same way.
//!
//! # Refused by name
//!
//! - no ring at all: there is nothing to cut with;
//! - a ring of fewer than three corners;
//! - a shape and a ring that name two frames: their numbers do not compare;
//! - nothing cut: the result would be the shape itself, under a new uuid;
//! - everything cut: nothing would be left to write;
//! - a volume, or patches of several kinds - what `transform` refuses to write.
//!
//! # References
//!
//! - Containment of a node in a ring (the `outer`/`inner` rule) is `energyml_geometry::clip::inside_ring`.
//!   TODO(reference): verify - the point-in-polygon rule it implements (even-odd or winding) is not cited; add the original reference once checked.

use energyml_analysis::ijk::{self, LatticeError};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::Topology;

use crate::declare::{Inputs, Params, Shape, Suggestions};
use crate::operation::{About, Category, Context, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::{Lattice, Pieces};

use super::delete_nodes::erase;
use super::support;
use super::transform::{kind_of, write_as};
use super::written::Written;

/// The operation.
pub struct Clip;

/// What `clip` is told.
#[derive(Debug, Params)]
pub struct ClipParams {
    /// The frame, what is represented, the naming - for the writer of the result.
    #[param(flatten)]
    pub written: Written,
}

/// What `clip` takes.
#[derive(Debug, Inputs)]
pub struct ClipInputs {
    /// the shape to cut - a cloud, polylines, a triangulated surface or a 2D grid
    pub shape: Shape,
    /// keeps only what lies inside this ring
    #[input(topology = "polylines")]
    pub outer: Option<Shape>,
    /// cut a hole each - what lies inside one of them goes
    #[input(topology = "polylines")]
    pub inner: Vec<Shape>,
}

fn double(title: &str, value: f64) -> Parameter {
    Parameter::Double {
        title: title.to_string(),
        value,
        uom: None,
    }
}

/// The title of the object behind a shape, or the shape's name.
fn title_of(shape: &Shape) -> String {
    shape
        .known
        .as_ref()
        .and_then(|known| known.reference.as_ref())
        .map(|r| r.title.clone())
        .unwrap_or_else(|| shape.name.clone())
}

/// The single 2D grid a shape is, if it is one.
fn grid_counts(shape: &Shape) -> Option<[usize; 2]> {
    match shape.geometry.patches.as_slice() {
        [patch] => match patch.topology {
            Topology::Grid { fastest, slowest } => Some([fastest, slowest]),
            _ => None,
        },
        _ => None,
    }
}

/// A ring's corners in plan, flat `[x, y, …]`.
fn corners(slot: &str, ring: &Shape) -> Result<Vec<f64>, Refusal> {
    let points: Vec<[f64; 3]> = ring
        .geometry
        .patches
        .iter()
        .flat_map(|p| p.points.iter().copied())
        .collect();
    if points.len() < 3 {
        return Err(Refusal::on(
            slot,
            format!(
                "`{}` holds {} point(s): a ring encloses something with at least 3",
                title_of(ring),
                points.len()
            ),
        ));
    }
    Ok(points.iter().flat_map(|p| [p[0], p[1]]).collect())
}

/// A ring in another frame than the shape is refused: the comparison is made
/// in the numbers, and two frames' numbers do not compare. A shape or a ring
/// that names no frame (a drawing of the scene) is taken as given.
fn same_frame(slot: &str, shape: &Shape, ring: &Shape) -> Result<(), Refusal> {
    let frame_of = |one: &Shape| one.known.as_ref().and_then(|known| known.crs.clone());
    if let (Some(of_shape), Some(of_ring)) = (frame_of(shape), frame_of(ring))
        && !of_shape.uuid.eq_ignore_ascii_case(&of_ring.uuid)
    {
        return Err(Refusal::on(
            slot,
            format!(
                "`{}` is in `{}` and `{}` in `{}`: a ring is compared to the nodes in their numbers, and two frames' numbers do not compare",
                title_of(ring),
                of_ring.title,
                title_of(shape),
                of_shape.title
            ),
        ));
    }
    Ok(())
}

/// One flag per node of the shape: outside `outer`, or inside an `inner`.
fn cut_nodes(inputs: &ClipInputs) -> Result<Vec<bool>, Refusal> {
    let flat: Vec<f64> = inputs
        .shape
        .geometry
        .patches
        .iter()
        .flat_map(|p| p.points.iter().flat_map(|q| [q[0], q[1], 0.0]))
        .collect();
    let mut gone = vec![false; flat.len() / 3];
    if let Some(ring) = &inputs.outer {
        same_frame("outer", &inputs.shape, ring)?;
        let outside = energyml_geometry::clip::nodes_in_ring(&flat, &corners("outer", ring)?, true)
            .map_err(|why| Refusal::on("outer", why))?;
        for at in outside {
            gone[at as usize] = true;
        }
    }
    for ring in &inputs.inner {
        same_frame("inner", &inputs.shape, ring)?;
        let inside = energyml_geometry::clip::nodes_in_ring(&flat, &corners("inner", ring)?, false)
            .map_err(|why| Refusal::on("inner", why))?;
        for at in inside {
            gone[at as usize] = true;
        }
    }
    Ok(gone)
}

impl Operation for Clip {
    type Params = ClipParams;
    type Inputs = ClipInputs;

    fn about(&self) -> About {
        About {
            name: "clip",
            describe: "a shape cut by rings - what lies outside the outer ring or inside an inner one goes: a cloud loses those nodes, a line is cut, a surface loses the triangles that name one, a 2D grid gets holes",
            category: Category::Geometry,
            makes: &[
                "resqml22.PointSetRepresentation",
                "resqml22.PolylineSetRepresentation",
                "resqml22.TriangulatedSetRepresentation",
                "resqml22.Grid2dRepresentation",
            ],
            recipe: "clip",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
    }

    fn check(&self, inputs: &ClipInputs) -> Result<(), Refusal> {
        if grid_counts(&inputs.shape).is_some() {
            return Ok(());
        }
        kind_of(&inputs.shape.geometry)
            .map(|_| ())
            .map_err(|why| Refusal::on("shape", why))
    }

    fn plan(&self, inputs: ClipInputs, params: ClipParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        // Refused here and not in `check`: each ring is optional on its own,
        // and only the pair says « at least one » - which no slot can declare.
        if inputs.outer.is_none() && inputs.inner.is_empty() {
            return Err(Refusal::on(
                "outer",
                "give `outer`, `inner` or both: without a ring there is nothing to cut with",
            ));
        }
        let gone = cut_nodes(&inputs)?;
        let cut = gone.iter().filter(|g| **g).count();
        let total = gone.len();
        let shape = &inputs.shape;
        if cut == 0 {
            return Err(Refusal::on(
                "shape",
                format!(
                    "every node of `{}` lies inside `outer` and outside every `inner`: nothing to cut - the result would be the shape itself",
                    title_of(shape)
                ),
            ));
        }
        if cut == total {
            return Err(Refusal::on(
                "shape",
                format!(
                    "the rings cut all {total} node(s) of `{}`: nothing would be left to write",
                    title_of(shape)
                ),
            ));
        }
        cx.stop_if_cancelled()?;

        let title = params.written.naming.title();
        let seed = format!("clip:{}:{title}", shape.name);
        let mut plan = if let Some(counts) = grid_counts(shape) {
            let points = &shape.geometry.patches[0].points;
            let lattice = ijk::lattice_of(points, counts).map_err(|why| {
                Refusal::on(
                    "shape",
                    match why {
                        LatticeError::Turned { along_i, along_j } => format!(
                            "`{}` runs along ({}, {}) then ({}, {}) in its frame's numbers: a turned or reversed lattice is not written back here",
                            title_of(shape),
                            along_i[0],
                            along_i[1],
                            along_j[0],
                            along_j[1]
                        ),
                        other => format!("`{}` {other}", title_of(shape)),
                    },
                )
            })?;
            let z = points
                .iter()
                .zip(&gone)
                .map(|(p, cut)| if *cut { f64::NAN } else { p[2] })
                .collect();
            let lattice = Lattice {
                origin: lattice.origin,
                spacing: lattice.spacing,
                counts: lattice.counts,
                z,
            };
            let mut plan = super::grid::write(&shape.name, &lattice, &params.written, &seed, cx)?;
            plan.activity.parameters.push(double("cutNodes", cut as f64));
            plan
        } else {
            let kind = kind_of(&shape.geometry).map_err(|why| Refusal::on("shape", why))?;
            let (result, counts) = erase(shape, &gone, "clip").map_err(|taken| {
                Refusal::on(
                    "shape",
                    format!("the rings cut {taken} node(s) and leave nothing to write"),
                )
            })?;
            let mut plan = write_as(kind, result, params.written, &seed, cx)?;
            plan.activity.parameters.extend([
                double("cutNodes", counts.taken as f64),
                double("droppedPieces", counts.dropped as f64),
                double("removedTriangles", counts.removed as f64),
                double("orphanNodes", counts.orphans as f64),
            ]);
            plan.says
                .extend(counts.says().into_iter().filter(|note| note.key != "deleted"));
            plan.says.push(Note { key: "follows-the-mesh", detail: "the cut follows the nodes: a triangle or a segment across a ring goes whole, and no node is added on the ring".to_string() });
            plan
        };
        plan.activity.recipe = "clip";
        plan.activity.title = format!("clip → {title}");
        plan.says.insert(
            0,
            Note {
                key: "cut",
                detail: format!("{cut} of {total} node(s) outside `outer` or inside an `inner`"),
            },
        );
        Ok(plan)
    }

    fn reports(&self) -> Vec<Slot> {
        let count = |name: &'static str, about: &'static str| {
            Slot::required(
                name,
                Need::Number {
                    uom: None,
                    least: None,
                    most: None,
                },
                about,
            )
        };
        let optional = |name: &'static str, about: &'static str| {
            Slot::optional(
                name,
                Need::Number {
                    uom: None,
                    least: None,
                    most: None,
                },
                about,
            )
        };
        support::frame_reports(vec![
            count("cutNodes", "nodes outside the outer ring or inside an inner one"),
            optional("droppedPieces", "pieces of line left with a single node, dropped"),
            optional("removedTriangles", "triangles that named a cut node"),
            optional("orphanNodes", "nodes no triangle names any more, compacted away"),
        ])
    }
}
