//! `intersect` - the trace of a fault on each horizon it cuts.
//!
//! The need: an operation that takes 2 surfaces and outputs their
//! intersection polyline. It could be done between one fault and several other
//! surfaces, creating a single object that holds several polylines, one per
//! horizon. Those lines could then be used for kriging. This is
//! It is one of the geological operations.
//!
//! # Why this is the mandatory door of a fault surface
//!
//! A barrier and a meshing constraint are rules on a **line in plan view**; a
//! dipping fault surface projects to a band, and the right line depends on the
//! horizon one stands on. The intersection is that line, horizon by horizon -
//! exactly what the other RESQML convention writes by hand: a horizontal
//! polyline at the height of the horizon.
//!
//! # Two roles, one object, one tree
//!
//! `fault` is single, `surfaces` are several and **unordered** - a trace does
//! not depend on the order of the horizons, and declaring an order that does
//! not matter would put meaningless arrows in the form. What comes out is **one**
//! `PolylineSetRepresentation`, a patch per horizon, written by
//! [`super::polylines`]; and an artefact tree whose every child is the
//! trace of **one** horizon. That child is the one later designated
//! as `faults` when kriging that horizon's cloud: picking the right
//! piece is then a choice one can see, not a guess about a z.
//!
//! # What is said
//!
//! A horizon the fault does not cut gives an empty trace - that is an
//! answer, counted by its name. A fault that cuts **no** horizon is
//! a refusal: an empty plan would read as a successful computation that saw nothing.
//! A horizon torn by the fault gives two traces, one per lip; they
//! stay two, matching the lips being a separate operation.
//!
//! # References
//!
//! - The trace of a horizon on a fault surface is computed by `energyml_analysis::intersect`, from the triangle-triangle intersection of the two surfaces.
//!   TODO(reference): verify - no source is cited for the triangle-triangle intersection; add one once checked.

use energyml_analysis::intersect;
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

use crate::declare::{Inputs, Params, Shape, Suggestions};
use crate::operation::{About, Category, Context, Made, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::Pieces;

use super::polylines::PolylinesInputs;
use super::support;
use super::written::Written;

/// The operation.
pub struct Intersect;

/// What `intersect` is told.
///
/// **Not `closed`**: it belongs to the computation - a trace closes where the
/// fault pierces a horizon in a loop.
#[derive(Debug, Params)]
pub struct IntersectParams {
    /// The frame, what is represented, the naming - for the polyline writer.
    #[param(flatten)]
    pub written: Written,
}

/// What `intersect` takes - **two roles, read by the slot's name**.
#[derive(Debug, Inputs)]
pub struct IntersectInputs {
    /// the fault, as a triangulated surface
    #[input(topology = "triangles")]
    pub fault: Shape,
    /// the surfaces the fault cuts - one trace per surface, in no particular order
    #[input(topology = "triangles", least = 1)]
    pub surfaces: Vec<Shape>,
}

/// What a horizon returned: its name, its traces, and which of them close.
type Traced = (String, Vec<Vec<[f64; 3]>>, Vec<bool>);

/// A surface as the operation reads it.
struct Surface<'a> {
    name: &'a str,
    points: Vec<[f64; 3]>,
    triangles: Vec<[u32; 3]>,
}

fn surface_of(shape: &Shape) -> Option<Surface<'_>> {
    let mut points = Vec::new();
    let mut triangles = Vec::new();
    let mut base = 0u32;
    for patch in &shape.geometry.patches {
        let Topology::Triangles(faces) = &patch.topology else {
            continue;
        };
        points.extend(patch.points.iter().copied());
        triangles.extend(faces.iter().map(|f| [f[0] + base, f[1] + base, f[2] + base]));
        base += patch.points.len() as u32;
    }
    if triangles.is_empty() {
        None
    } else {
        Some(Surface {
            name: &shape.name,
            points,
            triangles,
        })
    }
}

/// The two roles, read as surfaces - or the refusal that names what is not one.
fn surfaces_of(inputs: &IntersectInputs) -> Result<(Surface<'_>, Vec<Surface<'_>>), Refusal> {
    let fault = surface_of(&inputs.fault).ok_or_else(|| {
        Refusal::on(
            "fault",
            "must be a triangulated surface - a fault given as lines is already a trace",
        )
    })?;
    let mut surfaces = Vec::new();
    for one in &inputs.surfaces {
        match surface_of(one) {
            Some(surface) => surfaces.push(surface),
            None => {
                return Err(Refusal::on(
                    "surfaces",
                    format!(
                        "`{}` is not triangulated - `triangulate` makes a surface from a cloud",
                        one.name
                    ),
                ));
            }
        }
    }
    Ok((fault, surfaces))
}

impl Operation for Intersect {
    type Params = IntersectParams;
    type Inputs = IntersectInputs;

    fn about(&self) -> About {
        About {
            name: "intersect",
            describe: "the trace of a fault surface on every surface it cuts - one polyline set, a patch per surface",
            category: Category::Faults,
            makes: &["resqml22.PolylineSetRepresentation"],
            recipe: "intersect",
            // A fault trace on a horizon is a contact; RESQML has no
            // generic interpretation for that and inventing one would lie
            // about what the object is. The activity carries the provenance.
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
    }

    fn check(&self, inputs: &IntersectInputs) -> Result<(), Refusal> {
        surfaces_of(inputs).map(|_| ())
    }

    fn plan(
        &self,
        inputs: IntersectInputs,
        params: IntersectParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let (fault, surfaces) = surfaces_of(&inputs)?;
        let mut says = Vec::new();
        let mut per_surface: Vec<Traced> = Vec::new();
        let mut dropped = 0usize;
        let mut empty = 0usize;
        for surface in &surfaces {
            let made = intersect(
                &fault.points,
                &fault.triangles,
                &surface.points,
                &surface.triangles,
            );
            dropped += made.dropped;
            if made.traces.is_empty() {
                empty += 1;
                says.push(Note {
                    key: "no-trace",
                    detail: format!("{}: the fault does not cut it", surface.name),
                });
                continue;
            }
            let lines: Vec<Vec<[f64; 3]>> = made.traces.iter().map(|t| t.points.clone()).collect();
            let closed: Vec<bool> = made.traces.iter().map(|t| t.closed).collect();
            if lines.len() > 1 {
                says.push(Note {
                    key: "torn",
                    detail: format!(
                        "{}: {} separate traces - a surface torn by the fault gives one per lip",
                        surface.name,
                        lines.len()
                    ),
                });
            }
            per_surface.push((surface.name.to_string(), lines, closed));
        }
        if per_surface.is_empty() {
            return Err(Refusal::on(
                "surfaces",
                format!("the fault cuts none of the {} surface(s)", surfaces.len()),
            ));
        }
        if dropped > 0 {
            says.push(Note {
                key: "dropped",
                detail: format!("{dropped} triangle(s) with a non-finite corner took no part"),
            });
        }

        // **One patch per horizon**, in a single shape: that is what makes one
        // single object, and it is by the patch's rank that the front end finds the
        // trace of a horizon.
        let mut patches = Vec::new();
        let mut closed_all = Vec::new();
        let mut node_offset = 0usize;
        for (index, (_, lines, closed)) in per_surface.iter().enumerate() {
            let points: Vec<[f64; 3]> = lines.iter().flatten().copied().collect();
            let runs: Vec<u32> = lines.iter().map(|l| l.len() as u32).collect();
            patches.push(Patch {
                index,
                node_offset,
                points,
                topology: Topology::Polylines(runs),
                closed: Vec::new(),
            });
            node_offset += lines.iter().map(Vec::len).sum::<usize>();
            closed_all.extend(closed.iter().copied());
        }
        let name = format!("{} on {} surface(s)", fault.name, per_surface.len());
        let shape = Shape {
            name: name.clone(),
            geometry: Geometry {
                kind: "intersect",
                patches: patches.clone(),
            },
            closed: Some(closed_all),
            known: None,
        };
        let title = params.written.naming.title();
        let lines = PolylinesInputs {
            shape: vec![shape],
            features: None,
        };
        let mut plan =
            super::polylines::write(&lines, &params.written, None, &format!("{name}:{title}"), cx)?;

        plan.activity.recipe = "intersect";
        plan.activity.title = format!("intersect → {title}");
        plan.activity
            .parameters
            .retain(|p| !matches!(p, Parameter::Double { title, .. } if title == "closedCount"));
        plan.activity.parameters.push(Parameter::Double {
            title: "surfaceCount".to_string(),
            value: surfaces.len() as f64,
            uom: None,
        });
        plan.activity.parameters.push(Parameter::Double {
            title: "tracedSurfaces".to_string(),
            value: per_surface.len() as f64,
            uom: None,
        });
        plan.activity.parameters.push(Parameter::Double {
            title: "uncutSurfaces".to_string(),
            value: empty as f64,
            uom: None,
        });
        plan.says.append(&mut says);
        plan.shapes = tree_of(&plan.shapes, &per_surface, &patches);
        Ok(plan)
    }

    fn reports(&self) -> Vec<Slot> {
        support::frame_reports(vec![
            Slot::optional(
                "surfaceCount",
                Need::Integer {
                    least: Some(0),
                    most: None,
                },
                "how many surfaces were given",
            ),
            Slot::optional(
                "tracedSurfaces",
                Need::Integer {
                    least: Some(0),
                    most: None,
                },
                "how many the fault cuts - one patch each",
            ),
            Slot::optional(
                "uncutSurfaces",
                Need::Integer {
                    least: Some(0),
                    most: None,
                },
                "how many the fault does not reach",
            ),
        ])
    }
}

/// The tree: a grouping root, one child per horizon - the trace that will
/// later be designated as `faults` for that horizon.
fn tree_of(written: &[Made], per_surface: &[Traced], patches: &[Patch]) -> Vec<Made> {
    let Some(first) = written.first() else {
        return Vec::new();
    };
    let root = Made {
        geometry: Geometry {
            kind: "intersect",
            patches: Vec::new(),
        },
        ..first.clone()
    };
    let mut out = vec![root.clone()];
    for (at, ((name, _, _), patch)) in per_surface.iter().zip(patches).enumerate() {
        let mut own = patch.clone();
        own.index = 0;
        own.node_offset = 0;
        out.push(Made {
            id: format!("{at}-trace"),
            parent: Some(root.id.clone()),
            title: format!("trace on {name}"),
            geometry: Geometry {
                kind: "intersect",
                patches: vec![own],
            },
            object: None,
        });
    }
    out
}
