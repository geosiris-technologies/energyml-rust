//! `project` — nodes projected **vertically** onto a surface, or onto a level
//!. The whole shape, or some of
//! its nodes.
//!
//! The computation is `energyml_analysis::project`, which reads a surface's
//! heights the way `thickness` does ([`Heights`]); this is the seam, and the
//! writing goes to the writer of the shape's own kind, as for `transform`.
//!
//! # What is left, and said
//!
//! A node with no face above or below it, or over a fold, is **left where it
//! was and counted** — in a note and in the activity. The other choices
//! would each lie: removed, a line would have a hole and a surface a node its
//! triangles still name; left without a word, it would look projected.
//!
//! # Refused by name
//!
//! - a surface and a level together, or neither: they are alternatives;
//! - the shape and the surface in two frames, or two vertical units: a height
//!   read in one frame and written in another is a height from somewhere else;
//! - what `transform` refuses to write, for the same reason: a grid's cells, a
//!   volume, mixed patches.

use energyml_analysis::project::{Onto, project};
use energyml_analysis::Heights;
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch};

use crate::declare::{Inputs, Params, Shape, Suggestions};
use crate::operation::{About, Category, Context, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::Pieces;

use super::support;
use super::transform::{Kind, flipped_in, kind_of, write_as};
use super::written::Written;

/// The operation.
pub struct Project;

/// What `project` is told.
#[derive(Debug, Params)]
pub struct ProjectParams {
    /// What the nodes go onto.
    #[param(flatten)]
    pub onto: OntoParams,
    /// only these nodes are projected — their ranks in the shape; left out, every node
    ///
    /// **Advanced**: a form fills it from the nodes pointed in the scene.
    #[param(advanced)]
    pub nodes: Vec<usize>,
    /// The frame, what is represented, the naming — for the writer of the result.
    #[param(flatten)]
    pub written: Written,
}

/// what the nodes are projected onto — a surface, given as an input, or a level
#[derive(Debug, Params)]
#[params(tag = "onto")]
pub enum OntoParams {
    /// A surface.
    #[case(default)]
    Surface,
    /// A level.
    Level(LevelParams),
}

/// A level.
#[derive(Debug, Params)]
pub struct LevelParams {
    /// level: the z every projected node is moved to, in the unit of the heights
    pub z: f64,
}

/// What `project` takes.
#[derive(Debug, Inputs)]
pub struct ProjectInputs {
    /// the nodes to project — a cloud, polylines or a triangulated surface
    pub shape: Shape,
    /// the surface to project onto — its triangles, or a grid's cells; left out when projecting onto a level
    #[input(topology = "triangles")]
    pub surface: Option<Shape>,
}

fn double(title: &str, value: f64) -> Parameter {
    Parameter::Double { title: title.to_string(), value, uom: None }
}

impl Operation for Project {
    type Params = ProjectParams;
    type Inputs = ProjectInputs;

    fn about(&self) -> About {
        About {
            name: "project",
            describe: "nodes projected vertically onto a surface or a level — the z replaced, x and y kept — whole, or only some of its nodes",
            category: Category::Geometry,
            makes: &[
                "resqml22.PointSetRepresentation",
                "resqml22.PolylineSetRepresentation",
                "resqml22.TriangulatedSetRepresentation",
            ],
            recipe: "project",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
    }

    fn check(&self, inputs: &ProjectInputs) -> Result<(), Refusal> {
        kind_of(&inputs.shape.geometry).map(|_| ()).map_err(|why| Refusal::on("shape", why))
    }

    fn plan(&self, inputs: ProjectInputs, params: ProjectParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        let shape = inputs.shape;
        let kind = kind_of(&shape.geometry).map_err(|why| Refusal::on("shape", why))?;
        let points: Vec<[f64; 3]> = shape.geometry.patches.iter().flat_map(|patch| patch.points.iter().copied()).collect();
        let total = points.len();
        let chosen: Option<Vec<bool>> = if params.nodes.is_empty() {
            None
        } else {
            if let Some(bad) = params.nodes.iter().find(|&&rank| rank >= total) {
                return Err(Refusal::about(
                    "value",
                    "nodes",
                    format!("`nodes` names node {bad}, and the shape holds {total} (ranks from 0)"),
                ));
            }
            let mut flags = vec![false; total];
            for &rank in &params.nodes {
                flags[rank] = true;
            }
            Some(flags)
        };

        let faces = match (&params.onto, &inputs.surface) {
            (OntoParams::Surface, None) => {
                return Err(Refusal::on("surface", "projecting onto a surface needs one — or `onto: level`, with a z"));
            }
            (OntoParams::Level(_), Some(_)) => {
                return Err(Refusal::on("surface", "a level and a surface are alternatives: leave the surface out, or project onto it"));
            }
            (OntoParams::Surface, Some(surface)) => {
                // One frame, one unit: a height read in one and written in the
                // other is a height from somewhere else.
                if let (Some(a), Some(b)) = (
                    shape.known.as_ref().and_then(|k| k.crs.as_ref()),
                    surface.known.as_ref().and_then(|k| k.crs.as_ref()),
                ) && a.uuid != b.uuid
                {
                    return Err(Refusal::new(
                        "frames",
                        format!("`shape` is in `{}` and `surface` in `{}` — bring them into one frame first", a.title, b.title),
                    ));
                }
                if let (Some(a), Some(b)) = (
                    shape.known.as_ref().and_then(|k| k.vertical_uom.as_deref()),
                    surface.known.as_ref().and_then(|k| k.vertical_uom.as_deref()),
                ) && a != b
                {
                    return Err(Refusal::new("units", format!("`shape` counts its heights in `{a}` and `surface` in `{b}`")));
                }
                Some(support::Faces::of(&surface.geometry))
            }
            (OntoParams::Level(_), None) => None,
        };
        let heights = match &faces {
            Some(faces) => Some(
                Heights::of(faces.surface(), "surface", "under the nodes to project")
                    .map_err(|why| Refusal::on("surface", why.to_string()))?,
            ),
            None => None,
        };
        let onto = match (&params.onto, &heights) {
            (OntoParams::Level(level), _) => Onto::Level(level.z),
            (OntoParams::Surface, Some(heights)) => Onto::Surface(heights),
            (OntoParams::Surface, None) => unreachable!("a surface was required above"),
        };
        let made = project(&points, chosen.as_deref(), &onto);

        // The patches again, each with its nodes.
        let mut next = made.points.into_iter();
        let patches = shape
            .geometry
            .patches
            .iter()
            .map(|patch| Patch {
                index: patch.index,
                node_offset: patch.node_offset,
                points: next.by_ref().take(patch.points.len()).collect(),
                topology: patch.topology.clone(),
                closed: patch.closed.clone(),
            })
            .collect();
        let result = Shape { name: shape.name.clone(), geometry: Geometry { kind: "project", patches }, closed: shape.closed.clone(), known: shape.known.clone() };

        let mut says = Vec::new();
        let turned = if kind == Kind::Triangles { flipped_in(&shape.geometry, &result.geometry) } else { 0 };
        if turned > 0 {
            says.push(Note { key: "flipped", detail: format!("{turned} triangle(s) turned over in plan — a fold, kept as it is") });
        }
        if made.outside > 0 {
            says.push(Note { key: "outside", detail: format!("{} node(s) with no face above or below — left where they were", made.outside) });
        }
        if made.folded > 0 {
            says.push(Note { key: "folded", detail: format!("{} node(s) over a fold of the surface — left where they were", made.folded) });
        }
        if made.unplaced > 0 {
            says.push(Note { key: "unplaced", detail: format!("{} node(s) with no finite position — left", made.unplaced) });
        }

        let title = params.written.naming.title();
        let seed = format!("project:{}:{title}", shape.name);
        let mut plan = write_as(kind, result, params.written, &seed, cx)?;
        plan.activity.recipe = "project";
        plan.activity.title = format!("project → {title}");
        plan.activity.parameters.extend([
            double("projectedNodes", made.projected as f64),
            double("outsideNodes", made.outside as f64),
            double("foldedNodes", made.folded as f64),
        ]);
        if kind == Kind::Triangles {
            plan.activity.parameters.push(double("flippedTriangles", turned as f64));
        }
        plan.says.extend(says);
        Ok(plan)
    }

    fn reports(&self) -> Vec<Slot> {
        let count = |name: &'static str, about: &'static str| Slot::required(name, Need::Number { uom: None, least: None, most: None }, about);
        support::frame_reports(vec![
            count("projectedNodes", "nodes projected"),
            count("outsideNodes", "nodes with no face above or below — left where they were"),
            count("foldedNodes", "nodes over a fold — left where they were"),
            Slot::optional("flippedTriangles", Need::Number { uom: None, least: None, most: None }, "triangles turned over in plan by the projection — a surface only"),
        ])
    }
}
