//! `outline` - the contour of a shape, as closed polylines.
//!
//! The edges of the starting surface are not marked: the need is a polyline,
//! with collinear points simplified,
//! that makes the contour of the object. Decided: an operation of its own -
//! this one - **and** an option of the contouring that composes with it.
//!
//! What comes out is what [`energyml_analysis::outline`] computes, and the
//! plan says which of its two answers it got: the **boundary** of a
//! triangulated surface, exact, one loop per rim and per hole; or the
//! **convex hull** in plan view of anything else - a point cloud, a set of
//! polylines - which is not a boundary and is named as what it is.
//!
//! The writing is [`super::polylines`]'s: an outline is a polyline set whose
//! every line is closed, and a second writer of the same object would be a
//! second place for the same mistakes. Typed since step 8 of
//! ADR 0017: the rings go to
//! [`super::polylines::write`] as they are - no bag derived to carry them.
//!
//! # References
//!
//! - Andrew's monotone chain, as named in `energyml-geostat` (`outline`).
//!   TODO(reference): verify - Andrew's monotone chain is named without a full citation; add the original reference once checked.

use energyml_analysis::outline as analysis;
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

use crate::declare::{Inputs, Params, Shape, Suggestions};
use crate::operation::{About, Category, Context, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::Pieces;

use super::polylines::PolylinesInputs;
use super::support;
use super::written::Written;

/// The operation.
pub struct Outline;

/// What `outline` is told.
#[derive(Debug, Params)]
pub struct OutlineParams {
    /// The frame, what is represented, the naming - for the polyline writer.
    #[param(flatten)]
    pub written: Written,
    /// a node within this distance of the chord between its neighbours is dropped; 0 keeps all but the exactly collinear
    #[param(default = 0.0, optional, min = 0.0)]
    pub tolerance: f64,
}

/// What `outline` takes.
#[derive(Debug, Inputs)]
pub struct OutlineInputs {
    /// the shape to outline
    pub shape: Shape,
}

/// The rings of the input, and how they were found.
struct Rings {
    /// Each ring as points, first node not repeated.
    rings: Vec<Vec<[f64; 3]>>,
    /// `boundary` or `hull`.
    how: &'static str,
    /// Nodes dropped by the simplification.
    dropped: usize,
}

fn rings_of(shape: &Shape, tolerance: f64) -> Result<Rings, Refusal> {
    let mut rings = Vec::new();
    let mut dropped = 0usize;
    let mut triangulated = false;
    for patch in &shape.geometry.patches {
        let Topology::Triangles(triangles) = &patch.topology else {
            continue;
        };
        if triangles.is_empty() {
            continue;
        }
        triangulated = true;
        let loops = analysis::boundary_loops(triangles)
            .map_err(|e| Refusal::on("shape", format!("`{}` patch {}: {e}", shape.name, patch.index)))?;
        for ring in loops {
            let kept = analysis::simplify(&patch.points, &ring, tolerance);
            dropped += ring.len() - kept.len();
            rings.push(kept.iter().map(|&i| patch.points[i as usize]).collect());
        }
    }
    if triangulated {
        return Ok(Rings {
            rings,
            how: "boundary",
            dropped,
        });
    }
    // Nothing triangulated: the hull of every point there is, in plan view.
    let points: Vec<[f64; 3]> = shape
        .geometry
        .patches
        .iter()
        .flat_map(|p| p.points.iter().copied())
        .collect();
    let hull = analysis::hull_xy(&points).map_err(|e| Refusal::on("shape", e))?;
    let kept = analysis::simplify(&points, &hull, tolerance);
    Ok(Rings {
        dropped: hull.len() - kept.len(),
        rings: vec![kept.iter().map(|&i| points[i as usize]).collect()],
        how: "hull",
    })
}

impl Operation for Outline {
    type Params = OutlineParams;
    type Inputs = OutlineInputs;

    fn about(&self) -> About {
        About {
            name: "outline",
            describe: "the contour of a shape as closed polylines - the exact boundary of a triangulated surface, or the convex hull in plan view of anything else",
            category: Category::Geometry,
            makes: &["resqml22.PolylineSetRepresentation"],
            recipe: "outline",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
    }

    fn check(&self, inputs: &OutlineInputs) -> Result<(), Refusal> {
        let points = inputs.shape.geometry.point_count();
        if points < 3 {
            return Err(Refusal::on(
                "shape",
                format!("an outline needs at least 3 points; the shape holds {points}"),
            ));
        }
        Ok(())
    }

    fn plan(&self, inputs: OutlineInputs, params: OutlineParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        let found = rings_of(&inputs.shape, params.tolerance)?;
        if found.rings.is_empty() {
            return Err(Refusal::on(
                "shape",
                "the surface is closed: it has no boundary to outline",
            ));
        }
        // The rings as one shape, every line closed - and said so, so that the
        // polyline writer neither detects nor notes anything.
        let name = format!("outline of {}", cx.first());
        let runs: Vec<u32> = found.rings.iter().map(|r| r.len() as u32).collect();
        let rings = Shape {
            name: name.clone(),
            geometry: Geometry {
                kind: "outline",
                patches: vec![Patch {
                    index: 0,
                    node_offset: 0,
                    points: found.rings.iter().flatten().copied().collect(),
                    topology: Topology::Polylines(runs),
                    closed: Vec::new(),
                }],
            },
            closed: Some(vec![true; found.rings.len()]),
            known: None,
        };
        let title = params.written.naming.title();
        let lines = PolylinesInputs {
            shape: vec![rings],
            features: None,
        };
        let mut plan =
            super::polylines::write(&lines, &params.written, None, &format!("{name}:{title}"), cx)?;

        plan.activity.recipe = "outline";
        plan.activity.title = format!("outline → {title}");
        plan.activity
            .parameters
            .retain(|p| !matches!(p, Parameter::Double { title, .. } if title == "closedCount"));
        plan.activity.parameters.push(Parameter::Text {
            title: "how".to_string(),
            value: found.how.to_string(),
        });
        plan.activity.parameters.push(Parameter::Double {
            title: "tolerance".to_string(),
            value: params.tolerance,
            uom: None,
        });
        plan.activity.parameters.push(Parameter::Double {
            title: "nodesDropped".to_string(),
            value: found.dropped as f64,
            uom: None,
        });
        plan.says.push(Note {
            key: found.how,
            detail: match found.how {
                "boundary" => format!("{} boundary loop(s) of the triangulated surface, {} collinear node(s) dropped", found.rings.len(), found.dropped),
                _ => format!("the convex hull in plan view - the input is not a surface, so this is its envelope and not its boundary; {} node(s) dropped", found.dropped),
            },
        });
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
        support::frame_reports(vec![
            count("polylineCount", "rings written"),
            count("pointCount", "nodes written"),
            Slot::required(
                "how",
                Need::Choice(&["boundary", "hull"]),
                "the exact boundary, or the convex hull",
            ),
            count("nodesDropped", "collinear nodes dropped"),
        ])
    }
}
