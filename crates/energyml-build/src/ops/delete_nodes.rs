//! `delete-nodes` — some nodes of a shape taken out
//! (the scene-editing design, vertex deletion): a cloud loses them, a polyline
//! is cut at them, a surface loses the triangles that name them. Deleted points
//! create holes.
//!
//! The computation is `energyml_analysis::nodes`; this is the seam, and the
//! writing goes to the writer of the shape's own kind, as for `transform`.
//!
//! # Refused by name
//!
//! - no node named, a node out of the shape: a deletion aimed at nothing would
//!   look as if it had worked;
//! - **everything** taken out: nothing would be left to write;
//! - a grid, a volume: what `transform` refuses to write back. A 2D grid's node
//!   would become *undefined* (NaN) rather than go — as the scene-artefacts design says —
//!   once a grid can be written back; a volume's cells are another affair.

use energyml_analysis::nodes::{erase_from_cloud, erase_from_lines, erase_from_surface};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

use crate::declare::{Inputs, Params, Shape, Suggestions};
use crate::operation::{About, Category, Context, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::Pieces;

use super::polylines::said_closed;
use super::support;
use super::transform::{kind_of, write_as};
use super::written::Written;

/// The operation.
pub struct DeleteNodes;

/// What `delete-nodes` is told.
#[derive(Debug, Params)]
pub struct DeleteNodesParams {
    /// the nodes taken out — their ranks in the shape; a form fills it from the nodes pointed in the scene
    #[param(least = 1)]
    pub nodes: Vec<usize>,
    /// The frame, what is represented, the naming — for the writer of the result.
    #[param(flatten)]
    pub written: Written,
}

/// What `delete-nodes` takes.
#[derive(Debug, Inputs)]
pub struct DeleteNodesInputs {
    /// the shape to take nodes out of — a cloud, polylines or a triangulated surface
    pub shape: Shape,
}

fn double(title: &str, value: f64) -> Parameter {
    Parameter::Double { title: title.to_string(), value, uom: None }
}

impl Operation for DeleteNodes {
    type Params = DeleteNodesParams;
    type Inputs = DeleteNodesInputs;

    fn about(&self) -> About {
        About {
            name: "delete-nodes",
            describe: "some nodes of a shape taken out — a cloud loses them, a polyline is cut at them, a surface loses the triangles that name them",
            category: Category::Geometry,
            makes: &[
                "resqml22.PointSetRepresentation",
                "resqml22.PolylineSetRepresentation",
                "resqml22.TriangulatedSetRepresentation",
            ],
            recipe: "delete-nodes",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
    }

    fn check(&self, inputs: &DeleteNodesInputs) -> Result<(), Refusal> {
        kind_of(&inputs.shape.geometry).map(|_| ()).map_err(|why| Refusal::on("shape", why))
    }

    fn plan(&self, inputs: DeleteNodesInputs, params: DeleteNodesParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        let shape = inputs.shape;
        let kind = kind_of(&shape.geometry).map_err(|why| Refusal::on("shape", why))?;
        let total = shape.geometry.point_count();
        if let Some(bad) = params.nodes.iter().find(|&&rank| rank >= total) {
            return Err(Refusal::about(
                "value",
                "nodes",
                format!("`nodes` names node {bad}, and the shape holds {total} (ranks from 0) — a deletion aimed at nothing would look as if it had worked"),
            ));
        }
        let mut gone = vec![false; total];
        for &rank in &params.nodes {
            gone[rank] = true;
        }
        let erased = erase(&shape, &gone, "delete-nodes")
            .map_err(|taken| Refusal::about("value", "nodes", format!("taking out these {taken} node(s) leaves nothing of the shape to write")))?;
        let (result, counts) = erased;
        let says = counts.says();

        let title = params.written.naming.title();
        let seed = format!("delete-nodes:{}:{title}", shape.name);
        let mut plan = write_as(kind, result, params.written, &seed, cx)?;
        plan.activity.recipe = "delete-nodes";
        plan.activity.title = format!("delete-nodes → {title}");
        plan.activity.parameters.extend([
            double("deletedNodes", counts.taken as f64),
            double("droppedPieces", counts.dropped as f64),
            double("removedTriangles", counts.removed as f64),
            double("orphanNodes", counts.orphans as f64),
        ]);
        plan.says.extend(says);
        Ok(plan)
    }

    fn reports(&self) -> Vec<Slot> {
        let count = |name: &'static str, about: &'static str| Slot::required(name, Need::Number { uom: None, least: None, most: None }, about);
        support::frame_reports(vec![
            count("deletedNodes", "nodes taken out"),
            count("droppedPieces", "pieces of line left with a single node, dropped"),
            count("removedTriangles", "triangles that named a node taken out"),
            count("orphanNodes", "nodes no triangle names any more, compacted away"),
        ])
    }
}

/// What [`erase`] took out, counted — said in the plan and in the activity.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Erased {
    /// Nodes taken out.
    pub taken: usize,
    /// Pieces of line left with a single node, dropped.
    pub dropped: usize,
    /// Triangles that named a node taken out.
    pub removed: usize,
    /// Nodes no triangle names any more, compacted away.
    pub orphans: usize,
}

impl Erased {
    /// The notes a plan carries for these counts.
    pub(crate) fn says(&self) -> Vec<Note> {
        let mut says = vec![Note { key: "deleted", detail: format!("{} node(s) taken out; the ranks of the others have changed", self.taken) }];
        if self.dropped > 0 {
            says.push(Note { key: "dropped", detail: format!("{} piece(s) of line left with a single node — dropped", self.dropped) });
        }
        if self.removed > 0 {
            says.push(Note { key: "triangles", detail: format!("{} triangle(s) that named a node taken out — removed", self.removed) });
        }
        if self.orphans > 0 {
            says.push(Note { key: "orphans", detail: format!("{} node(s) no triangle names any more — compacted away", self.orphans) });
        }
        says
    }
}

/// The shape without the nodes `gone` marks (one flag per node, in the
/// shape's ranks): a cloud loses them, a line is cut at them, a surface loses
/// the triangles that name them. `delete-nodes` and `clip` both take nodes out
/// through here — one implementation of what a hole does to each kind.
///
/// Every patch must be points, polylines or triangles (`kind_of`). `Err` holds
/// how many were taken when **nothing** is left to write.
pub(crate) fn erase(shape: &Shape, gone: &[bool], kind_name: &'static str) -> Result<(Shape, Erased), usize> {
    let mut counts = Erased { taken: gone.iter().filter(|g| **g).count(), ..Erased::default() };
    let mut patches = Vec::new();
    let mut first = 0usize;
    let mut line_at = 0usize;
    for patch in &shape.geometry.patches {
        let local = &gone[first..first + patch.points.len()];
        first += patch.points.len();
        let (points, topology, closed) = match &patch.topology {
            Topology::Points => (erase_from_cloud(&patch.points, local), Topology::Points, Vec::new()),
            Topology::Polylines(runs) => {
                let closure: Vec<bool> = (0..runs.len())
                    .map(|k| said_closed(patch, k, shape.closed.as_deref(), line_at + k).unwrap_or(false))
                    .collect();
                line_at += runs.len();
                let made = erase_from_lines(&patch.points, runs, &closure, local);
                counts.dropped += made.dropped;
                (made.points, Topology::Polylines(made.runs), made.closed)
            }
            Topology::Triangles(triangles) => {
                let made = erase_from_surface(&patch.points, triangles, local);
                counts.removed += made.removed;
                counts.orphans += made.orphans;
                (made.points, Topology::Triangles(made.triangles), Vec::new())
            }
            // `kind_of` refused every other topology before this is called.
            _ => continue,
        };
        if !points.is_empty() {
            patches.push(Patch { index: patch.index, node_offset: 0, points, topology, closed });
        }
    }
    if patches.is_empty() {
        return Err(counts.taken);
    }
    let mut offset = 0usize;
    for (index, patch) in patches.iter_mut().enumerate() {
        patch.index = index;
        patch.node_offset = offset;
        offset += patch.points.len();
    }
    // The closures now travel on the patches; the shape-wide list spoke of
    // the lines before the cut.
    let result = Shape { name: shape.name.clone(), geometry: Geometry { kind: kind_name, patches }, closed: None, known: shape.known.clone() };
    Ok((result, counts))
}
