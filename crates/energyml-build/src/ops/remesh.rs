//! `remesh` - a triangulated horizon redone cleanly, its nodes kept, its
//! topology thrown away.
//!
//! The need is to resample by the plane:
//! what a 2.5D horizon asks for, and a seam over what exists rather than a
//! new algorithm. The horizon's nodes become a cloud, and
//! [`super::triangulate`] decides a topology for it - Delaunay constrained by the
//! faults, a buffer near them, not regular on their traces.
//!
//! # Why an operation, and not a flag of `triangulate`
//!
//! `triangulate` **refuses** a shape that is already triangulated, and that is a rule
//! worth keeping: triangulating again by mistake would lay a second skin over the
//! first. Redoing the mesh is a gesture that has a name, not an option to
//! tick - hence an operation whose name says it throws away what was there.
//!
//! # What it does not do
//!
//! It does not resample the horizon itself onto a grid - that is
//! `interpolate`, which returns a `Grid2dRepresentation` - and it smooths nothing:
//! the nodes come out where they were, except those that `faultBuffer` sets aside.
//!
//! Typed (ADR 0017):
//! its settings **are** those of `triangulate` (the same type), and its nodes
//! go to [`super::triangulate::run`] with no derived bag.
//!
//! # References
//!
//! - Delaunay triangulation, constrained by the fault traces, delegated to the `spade` crate through `energyml-geostat` (`triangulate`).
//!   TODO(reference): verify - cite the underlying references (as documented by the `spade` crate) once checked.

use energyml_geometry::geometry::{Geometry, Patch, Topology};

use crate::declare::{Inputs, Shape, Suggestions};
use crate::operation::{About, Category, Context, Operation, Plan, Refusal, Slot};
use crate::pieces::Pieces;

use super::triangulate::{TriangulateInputs, TriangulateParams};

/// The operation.
pub struct Remesh;

/// What `remesh` takes.
#[derive(Debug, Inputs)]
pub struct RemeshInputs {
    /// the surface to mesh again - its nodes are kept, its triangles are not
    #[input(topology = "triangles")]
    pub shape: Shape,
    /// fault traces no triangle may straddle; with `faultBuffer` and `faultSpacing`, the mesh near them is made clean rather than faithful
    #[input(topology = "polylines")]
    pub faults: Vec<Shape>,
}

impl Operation for Remesh {
    /// **The same settings as `triangulate`** - the same type, so the two
    /// cannot drift.
    type Params = TriangulateParams;
    type Inputs = RemeshInputs;

    fn about(&self) -> About {
        About {
            name: "remesh",
            describe: "a triangulated surface meshed again from its own nodes - Delaunay on (x, y), faults as constraints, a buffer and a spacing near them",
            category: Category::Geometry,
            makes: &["resqml22.TriangulatedSetRepresentation"],
            recipe: "remesh",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
    }

    fn check(&self, inputs: &RemeshInputs) -> Result<(), Refusal> {
        let triangulated = inputs
            .shape
            .geometry
            .patches
            .iter()
            .any(|p| matches!(p.topology, Topology::Triangles(_)));
        if !triangulated {
            return Err(Refusal::on(
                "shape",
                "it takes a triangulated surface to mesh again - a cloud goes to `triangulate`",
            ));
        }
        let nodes = inputs.shape.geometry.point_count();
        if nodes < 3 {
            return Err(Refusal::on(
                "shape",
                format!("a mesh needs at least 3 nodes; the surface holds {nodes}"),
            ));
        }
        Ok(())
    }

    fn plan(
        &self,
        inputs: RemeshInputs,
        params: TriangulateParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let points: Vec<[f64; 3]> = inputs
            .shape
            .geometry
            .patches
            .iter()
            .flat_map(|p| p.points.iter().copied())
            .collect();
        // The nodes, the topology left out: it is all that `triangulate` needs
        // not to see in order to agree to decide.
        let cloud = Shape {
            name: "shape".to_string(),
            geometry: Geometry {
                kind: "remesh",
                patches: vec![Patch {
                    index: 0,
                    node_offset: 0,
                    points,
                    topology: Topology::Points,
                    closed: Vec::new(),
                }],
            },
            closed: None,
            known: inputs.shape.known.clone(),
        };
        let title = params.written.naming.title();
        let mut plan = super::triangulate::run(
            TriangulateInputs {
                shape: vec![cloud],
                faults: inputs.faults,
            },
            params,
            "shape",
            cx,
        )?;
        plan.activity.recipe = "remesh";
        plan.activity.title = format!("remesh → {title}");
        Ok(plan)
    }

    fn reports(&self) -> Vec<Slot> {
        super::triangulate::Triangulate.reports()
    }
}
