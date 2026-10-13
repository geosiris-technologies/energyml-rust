//! `densify` — a polyline with **every vertex kept** and points added between
//! them (the scene-editing design, densification).
//!
//! The computation is `energyml_analysis::densify`; this is the seam: the
//! shape's polylines read, one by one, and the result written by the writer of
//! polyline sets — the same object the input came as.
//!
//! Not `resample`, which keeps only the ends and cuts the corners. The two
//! names are the difference, and the activity records which one ran.
//!
//! # What it is not
//!
//! A shape that holds no polyline is **unfit**, by name (`check`):
//! densifying a surface's nodes is remeshing, a different operation. A closure
//! nobody stated is **open** — adding points on an edge the file does not draw
//! would invent a ring.
//!
//! # References
//!
//! - Centripetal Catmull-Rom (Yuksel, Schaefer and Keyser, 2011), as named in `energyml-geostat` (`densify`).
//!   TODO(reference): verify — the citation is given without title, venue or DOI; complete and check it.

use energyml_analysis::{Along, Placement, densify};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

use crate::declare::{Choice, Inputs, Params, Shape, Suggestions};
use crate::operation::{About, Category, Context, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::Pieces;

use super::polylines::{PolylinesInputs, said_closed};
use super::support;
use super::written::Written;

/// The operation.
pub struct Densify;

/// What `densify` is told.
#[derive(Debug, Params)]
pub struct DensifyParams {
    /// Where the points go along each polyline.
    #[param(flatten)]
    pub method: Method,
    /// the added points lie on a curve through the vertices (a centripetal Catmull-Rom) rather than on the segments
    #[param(default = false, optional)]
    pub smooth: bool,
    /// how lengths are measured — in 3D, or in plan (x and y alone, as for a fault trace two horizons share)
    #[param(default = Measure::Space)]
    pub measure: Measure,
    /// The frame, what is represented, the naming — for the writer of the result.
    #[param(flatten)]
    pub written: Written,
}

/// how the added points are placed — a number of them over the whole length, or a station every so often
#[derive(Debug, Params)]
#[params(tag = "method")]
pub enum Method {
    /// A number of points over the whole length.
    #[case(default)]
    Count(CountParams),
    /// A station every so often.
    Spacing(SpacingParams),
}

/// By count.
#[derive(Debug, Params)]
pub struct CountParams {
    /// count: how many points each polyline gets, spread over its whole length — its vertices stay, so the intervals are not equal
    #[param(default = 1_i64, min = 1)]
    pub count: i64,
}

/// By spacing.
#[derive(Debug, Params)]
pub struct SpacingParams {
    /// spacing: a station every this much, counted from the start of the line, in the units of the frame
    #[param(min = 0)]
    pub every: f64,
    /// spacing: a station this close to a vertex is not added — no point at 5 m when there is one at 4.80 m with 0.25
    #[param(default = 0.0, optional, min = 0)]
    pub tolerance: f64,
}

/// How a length is measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum Measure {
    /// In 3D.
    Space,
    /// In plan.
    Plan,
}

/// What `densify` takes.
#[derive(Debug, Inputs)]
pub struct DensifyInputs {
    /// the polylines to densify
    #[input(topology = "polylines")]
    pub shape: Shape,
}

impl Operation for Densify {
    type Params = DensifyParams;
    type Inputs = DensifyInputs;

    fn about(&self) -> About {
        About {
            name: "densify",
            describe: "polylines with every vertex kept and points added between them — by count or every so often, on the segments or on a curve through the vertices",
            category: Category::Geometry,
            makes: &["resqml22.PolylineSetRepresentation"],
            recipe: "densify",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
    }

    fn check(&self, inputs: &DensifyInputs) -> Result<(), Refusal> {
        let lines = inputs
            .shape
            .geometry
            .patches
            .iter()
            .any(|patch| matches!(patch.topology, Topology::Polylines(_)));
        if lines {
            Ok(())
        } else {
            Err(Refusal::on(
                "shape",
                "it holds no polyline — a surface's nodes are remeshed, not densified",
            ))
        }
    }

    fn plan(&self, inputs: DensifyInputs, params: DensifyParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        let shape = inputs.shape;
        let placement = match &params.method {
            Method::Count(count) => {
                let n = usize::try_from(count.count)
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or_else(|| {
                        Refusal::about(
                            "value",
                            "count",
                            format!(
                                "`count` is {} — at least one point is added, or it is a copy",
                                count.count
                            ),
                        )
                    })?;
                Placement::Count(n)
            }
            Method::Spacing(spacing) => Placement::Spacing {
                every: spacing.every,
                tolerance: spacing.tolerance,
            },
        };
        let along = match params.measure {
            Measure::Space => Along::Space,
            Measure::Plan => Along::Plan,
        };

        let mut added = 0usize;
        let mut lines = 0usize;
        let mut at = 0usize;
        let mut patches = Vec::new();
        for patch in &shape.geometry.patches {
            let Topology::Polylines(runs) = &patch.topology else {
                continue;
            };
            let mut points = Vec::new();
            let mut counts = Vec::new();
            let mut start = 0usize;
            for (local, &run) in runs.iter().enumerate() {
                let run = run as usize;
                let line = patch.points.get(start..start + run).ok_or_else(|| {
                    Refusal::on("shape", "the polylines add up to more nodes than the patch holds")
                })?;
                let closed = said_closed(patch, local, shape.closed.as_deref(), at).unwrap_or(false);
                let made = densify(line, closed, placement, along, params.smooth).map_err(|why| {
                    let slot = match placement {
                        Placement::Count(_) => "count",
                        Placement::Spacing { .. } => "every",
                    };
                    Refusal::about("value", slot, why)
                })?;
                added += made.added;
                lines += 1;
                counts.push(made.points.len() as u32);
                points.extend(made.points);
                start += run;
                at += 1;
            }
            patches.push(Patch {
                index: patch.index,
                node_offset: 0,
                points,
                topology: Topology::Polylines(counts),
                closed: patch.closed.clone(),
            });
        }
        // The running sum, the contract of `Patch::node_offset`.
        let mut offset = 0usize;
        for patch in &mut patches {
            patch.node_offset = offset;
            offset += patch.points.len();
        }
        let result = Shape {
            name: shape.name.clone(),
            geometry: Geometry {
                kind: "densify",
                patches,
            },
            closed: shape.closed.clone(),
            known: shape.known.clone(),
        };

        let title = params.written.naming.title();
        let seed = format!("densify:{}:{title}", shape.name);
        let mut plan = super::polylines::write(
            &PolylinesInputs {
                shape: vec![result],
                features: None,
            },
            &params.written,
            None,
            &seed,
            cx,
        )?;
        plan.activity.recipe = "densify";
        plan.activity.title = format!("densify → {title}");
        plan.activity.parameters.push(Parameter::Double {
            title: "addedNodes".to_string(),
            value: added as f64,
            uom: None,
        });
        plan.says.push(Note {
            key: "added",
            detail: format!("{added} node(s) added on {lines} polyline(s); every vertex kept"),
        });
        Ok(plan)
    }

    fn reports(&self) -> Vec<Slot> {
        support::frame_reports(vec![Slot::required(
            "addedNodes",
            Need::Number {
                uom: None,
                least: None,
                most: None,
            },
            "nodes added",
        )])
    }
}
