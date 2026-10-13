//! `polyline-through` - one polyline through the nodes of its inputs, in their
//! order.
//!
//! The need: after selecting several points, create a polyline
//! from the selection - and
//! the answer chosen the same day: the nodes **in the order of the list**, one
//! line, open unless asked. A selection of nodes reaches an operation as a
//! cloud of them ([`Source::only_nodes`](crate::pieces::Source::only_nodes)),
//! so this takes any shape and reads its nodes in their numbering: the nodes
//! pointed, in the order they were pointed; a whole shape, in its own order.
//! Several shapes are joined one after the other, in the order of the slot -
//! which is why the slot is `ordered`.
//!
//! The writing is `polylines::write`'s: a line of one node, a ring of two, are
//! refused there, by name, as for any other polyline set.

use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

use crate::declare::{Inputs, Params, Shape, Suggestions};
use crate::operation::{About, Category, Context, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::Pieces;

use super::polylines::{self, PolylinesInputs};
use super::support;
use super::written::Written;

/// The operation.
pub struct PolylineThrough;

/// What `polyline-through` is told.
#[derive(Debug, Params)]
pub struct PolylineThroughParams {
    /// close the line from its last node back to its first
    #[param(default = false, optional)]
    pub closed: bool,
    /// The frame, what is represented, the naming.
    #[param(flatten)]
    pub written: Written,
}

/// What `polyline-through` takes.
#[derive(Debug, Inputs)]
pub struct PolylineThroughInputs {
    /// the shapes whose nodes the line goes through - in this order, each in its own
    #[input(least = 1, ordered)]
    pub shape: Vec<Shape>,
}

impl Operation for PolylineThrough {
    type Params = PolylineThroughParams;
    type Inputs = PolylineThroughInputs;

    fn about(&self) -> About {
        About {
            name: "polyline-through",
            describe: "one polyline through the nodes of the input, in their order - the nodes pointed in the scene, or every node of a shape; open, or closed",
            category: Category::Create,
            makes: &["resqml22.PolylineSetRepresentation"],
            recipe: "polyline-through",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
    }

    fn check(&self, inputs: &PolylineThroughInputs) -> Result<(), Refusal> {
        let nodes: usize = inputs.shape.iter().map(|one| one.geometry.point_count()).sum();
        if nodes < 2 {
            return Err(Refusal::on(
                "shape",
                format!("{nodes} node(s) in the input - a line goes through two at least"),
            ));
        }
        Ok(())
    }

    fn plan(
        &self,
        inputs: PolylineThroughInputs,
        params: PolylineThroughParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let points: Vec<[f64; 3]> = inputs
            .shape
            .iter()
            .flat_map(|one| {
                one.geometry
                    .patches
                    .iter()
                    .flat_map(|patch| patch.points.iter().copied())
            })
            .collect();
        let count = points.len();
        let names: Vec<&str> = inputs.shape.iter().map(|one| one.name.as_str()).collect();
        let known = inputs.shape.first().and_then(|one| one.known.clone());
        let line = Shape {
            name: names.join(" + "),
            geometry: Geometry {
                kind: "polyline-through",
                patches: vec![Patch {
                    index: 0,
                    node_offset: 0,
                    points,
                    topology: Topology::Polylines(vec![count as u32]),
                    closed: Vec::new(),
                }],
            },
            closed: Some(vec![params.closed]),
            known,
        };
        let title = params.written.naming.title();
        let seed = format!("polyline-through:{}:{title}", names.join("+"));
        let mut plan = polylines::write(
            &PolylinesInputs {
                shape: vec![line],
                features: None,
            },
            &params.written,
            Some(params.closed),
            &seed,
            cx,
        )?;
        plan.activity.recipe = "polyline-through";
        plan.activity.title = format!("polyline-through → {title}");
        plan.activity.parameters.push(Parameter::Double {
            title: "throughNodes".to_string(),
            value: count as f64,
            uom: None,
        });
        plan.says.push(Note {
            key: "through",
            detail: format!(
                "one {} line through {count} node(s), in the order given",
                if params.closed { "closed" } else { "open" }
            ),
        });
        Ok(plan)
    }

    fn reports(&self) -> Vec<Slot> {
        support::frame_reports(vec![Slot::required(
            "throughNodes",
            Need::Number {
                uom: None,
                least: None,
                most: None,
            },
            "nodes the line goes through",
        )])
    }
}
