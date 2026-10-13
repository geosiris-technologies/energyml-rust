//! `triangulate` — a sheet of triangles from a point cloud.
//!
//! Reported as a need: selecting a PointSetRepresentation,
//! one could not apply […] the creation of a triangulation
//! (whereas that is exactly what the triangulation does with the button at
//! the top). The module knew how to write triangles it was given; it did not
//! know how to **make** them, and the front end did it by itself through a button
//! of its own. Operations are pure functions (ADR 0017).
//!
//! # Why an operation distinct from [`super::triangles`]
//!
//! They are two gestures, and confusing them would be the worst of both worlds.
//! `triangles` **copies** a topology that the input already carries; this one
//! **decides** one — a 2D Delaunay on (x, y), hence a surface with a single
//! value per position. Letting `triangles` accept a cloud would amount to making it
//! invent in silence the thing the input does not say, and an invented mesh
//! looks exactly like a read one.
//!
//! Hence also the symmetric refusal: a shape that is **already triangulated** is sent
//! back to `triangles`. Triangulating it again would lay a second skin over the first
//! — it is the rule the front end already held for its button.
//!
//! # Faults as meshing constraints
//!
//! The need: the BRGM uses kriging with possible fault lines
//! and other meshing parameters. For a
//! triangulation, a fault is a **constraint**: no triangle
//! straddles it, its trace becomes a chain of edges, and the two compartments
//! are each meshed on their own side — `spade`'s constrained Delaunay. This is
//! the `faults` slot, optional and multiple; without it, nothing changes.
//!
//! What this adds to the object is **said**: the vertices of the traces and the
//! points where two traces cross enter the surface, and an object that
//! has more nodes than samples is an object one must be warned about.
//! A trace carries its z — its own is what is written, and at a crossing it
//! is read along the inserted trace.
//!
//! # Remeshing cleanly near the fault
//!
//! Near faults the measurements are always noisy and uncertain.
//! There is no point in keeping at all costs a point close to the
//! fault […] it is better to remesh cleanly — and the same number
//! of edges on each side. Two settings, optional and without effect without a
//! fault:
//!
//! - `faultBuffer`: the points of the cloud closer than this distance in plan
//!   to a trace are **set aside** — counted in the plan, never kept quiet;
//! - `faultSpacing`: each trace is **resampled** at this step in
//!   arc length, its ends kept. The vertices of the trace are then
//!   regular, and as the two sides of the constraint share exactly
//!   these vertices, the number of edges on either side is the same **by
//!   construction** — not through one more setting.
//!
//! For **open** lips — two traces, one per compartment — the operation that
//! matches lips decides the correspondence; the same step on both traces is
//! what makes it possible, and that is all that is done here.
//!
//! # What it does not do itself
//!
//! The computation is [`energyml_analysis::triangulate_xy`] — or its constrained
//! variant — and the writing is that of [`super::triangles`]: a second
//! writer of the same object would be a second place for the same mistakes.
//! This operation is the seam between the two, and nothing else — typed
//! (ADR 0017): the triangles decided
//! go to [`super::triangles::write`] as they are.
//!
//! # References
//!
//! - Delaunay triangulation, constrained by the fault traces, delegated to the `spade` crate through `energyml-geostat` (`triangulate`).
//!   TODO(reference): verify — cite the underlying references (as documented by the `spade` crate) once checked.

use energyml_analysis::{plan_distance, resample, triangulate_xy, triangulate_xy_constrained};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

use crate::declare::{Inputs, Params, Shape, Suggestions};
use crate::operation::{About, Category, Context, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::Pieces;

use super::triangles::TrianglesInputs;
use super::written::Written;

/// The operation.
pub struct Triangulate;

/// What `triangulate` is told.
#[derive(Debug, Params)]
pub struct TriangulateParams {
    /// with faults: drop the cloud's points closer than this to a trace, in plan view — measurements near a fault are noise, and a clean mesh beats a faithful one there
    #[param(min = 0.0)]
    pub fault_buffer: Option<f64>,
    /// with faults: resample every trace at this spacing along it, so both sides of the fault meet it on the same regular nodes
    #[param(min = 0.0)]
    pub fault_spacing: Option<f64>,
    /// The frame, what is represented, the naming — for the triangle writer.
    #[param(flatten)]
    pub written: Written,
}

/// What `triangulate` takes.
#[derive(Debug, Inputs)]
pub struct TriangulateInputs {
    /// point clouds to triangulate
    ///
    /// **Points, not triangles**: this operation *decides* a topology where
    /// `triangles` copies the one it is given. Several, one Delaunay over all
    /// of them — triangulating two clouds apart and writing them in one object
    /// would leave a gap between them nobody names.
    #[input(topology = "points", least = 1)]
    pub shape: Vec<Shape>,
    /// fault traces no triangle may straddle — each becomes a chain of edges; a trace carries its own z
    ///
    /// Optional and multiple: an empty list is an absence, and a
    /// triangulation with no fault is the ordinary case.
    #[input(topology = "polylines")]
    pub faults: Vec<Shape>,
}

/// Fault traces, each as a polyline of `[x, y, z]`.
pub(crate) type Traces = Vec<Vec<[f64; 3]>>;

/// The lines of one geometry, each as a polyline of `[x, y, z]` — a run of a
/// polyline and a segment are two ways of writing a line, and both are read.
/// What an operation calls on the shapes bound to `faults` — **by the slot**,
/// never by topology: the binding decided which shapes are faults, so a cloud
/// is never read as one, nor a shape filed under `faults` as a cloud.
pub(crate) fn traces_of(geometry: &energyml_geometry::geometry::Geometry) -> Traces {
    let mut out = Vec::new();
    for patch in &geometry.patches {
        match &patch.topology {
            Topology::Polylines(runs) => {
                let mut from = 0usize;
                for run in runs {
                    let to = (from + *run as usize).min(patch.points.len());
                    out.push(patch.points[from..to].to_vec());
                    from = to;
                }
            }
            Topology::Segments(segments) => {
                for [a, b] in segments {
                    if let (Some(p), Some(q)) = (patch.points.get(*a as usize), patch.points.get(*b as usize))
                    {
                        out.push(vec![*p, *q]);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The traces **of this horizon**: those with at least one node at its
/// depth, and the count of those that are not.
///
/// RESQML often writes a fault as *one horizontal polyline per
/// horizon*, and several at several heights in one object draw the
/// fault across a set of units. Passing that object as `faults` while kriging
/// **one** horizon would make the traces of the others a barrier — lines
/// whose (x, y) is not the fault's at that depth, since it
/// dips. The sorting is geometric and not guessed: a trace of which **no** node
/// is in the horizon's z range (widened by a tenth) is the trace
/// of another horizon. What is set aside is counted, and setting everything aside is
/// a refusal — never a silence that would look like *no fault here*.
pub(crate) fn faults_on(faults: Traces, zs: impl Iterator<Item = f64>) -> Result<(Traces, usize), String> {
    let (lo, hi) = zs
        .filter(|z| z.is_finite())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), z| {
            (lo.min(z), hi.max(z))
        });
    if !(lo.is_finite() && hi.is_finite()) || faults.is_empty() {
        return Ok((faults, 0));
    }
    let margin = ((hi - lo) * 0.1).max(1e-9);
    let (lo, hi) = (lo - margin, hi + margin);
    let total = faults.len();
    let kept: Vec<Vec<[f64; 3]>> = faults
        .into_iter()
        .filter(|line| line.iter().any(|p| p[2] >= lo && p[2] <= hi))
        .collect();
    if kept.is_empty() {
        return Err(format!(
            "none of the {total} fault trace(s) lies at this horizon's depth ({lo:.3} to {hi:.3}): they are other horizons' traces — run `intersect` for this one, or give its own trace"
        ));
    }
    let dropped = total - kept.len();
    Ok((kept, dropped))
}

/// The cloud alone: what is not a fault — the binding has already separated them.
fn cloud_of(inputs: &TriangulateInputs) -> Vec<[f64; 3]> {
    inputs
        .shape
        .iter()
        .flat_map(|one| one.geometry.patches.iter().flat_map(|p| p.points.iter().copied()))
        .collect()
}

impl Operation for Triangulate {
    type Params = TriangulateParams;
    type Inputs = TriangulateInputs;

    fn about(&self) -> About {
        About {
            name: "triangulate",
            describe: "triangulate a point cloud into a surface — Delaunay on (x, y), each point keeping its own z",
            category: Category::Geometry,
            makes: &["resqml22.TriangulatedSetRepresentation"],
            recipe: "triangulate",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
    }

    fn check(&self, inputs: &TriangulateInputs) -> Result<(), Refusal> {
        let points: usize = inputs.shape.iter().map(|one| one.geometry.point_count()).sum();
        let triangulated = inputs.shape.iter().any(|one| {
            one.geometry
                .patches
                .iter()
                .any(|p| matches!(p.topology, Topology::Triangles(_)))
        });
        if triangulated {
            // Name the other operation, because a refusal that does not say where
            // to go reads as *this cannot be done*.
            return Err(Refusal::on(
                "shape",
                "the shape is already triangulated: `triangles` writes it as it is, and triangulating again would lay a second skin over the first",
            ));
        }
        if points < 3 {
            return Err(Refusal::on(
                "shape",
                format!("a triangulation needs at least 3 points; the shape holds {points}"),
            ));
        }
        Ok(())
    }

    fn plan(
        &self,
        inputs: TriangulateInputs,
        params: TriangulateParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        run(inputs, params, &cx.first(), cx)
    }

    fn reports(&self) -> Vec<Slot> {
        let mut slots =
            crate::operation::AnyOperation::reports(&crate::operation::Typed(super::triangles::Triangles));
        slots.push(Slot::required(
            "how",
            Need::Choice(&["delaunay-xy", "constrained-delaunay-xy"]),
            "how the triangles were decided",
        ));
        slots.push(Slot::optional(
            "faultCount",
            Need::Integer {
                least: Some(0),
                most: None,
            },
            "how many fault traces constrained the triangulation",
        ));
        slots.push(Slot::optional(
            "nodesAdded",
            Need::Integer {
                least: Some(0),
                most: None,
            },
            "how many nodes the traces and their crossings added to the cloud",
        ));
        slots.push(Slot::optional(
            "nodesBuffered",
            Need::Integer {
                least: Some(0),
                most: None,
            },
            "how many cloud points were set aside for lying within `faultBuffer` of a trace",
        ));
        slots
    }
}

/// **The triangulation, decided and written** — what the operation does, and
/// what `remesh` calls with the nodes of a surface. `source` names the cloud in
/// the derived name (`triangulation of …`) the identities hang from, so each
/// caller keeps the one it always had.
pub fn run(
    inputs: TriangulateInputs,
    params: TriangulateParams,
    source: &str,
    cx: &Context<'_>,
) -> Result<Plan, Refusal> {
    // All the points of all the shapes, in the order they arrive.
    // One Delaunay rather than one per shape: triangulating two clouds
    // separately and then writing them in one object would leave between them a
    // hole that nothing names.
    let mut cloud = cloud_of(&inputs);
    let traces: Traces = inputs
        .faults
        .iter()
        .flat_map(|fault| traces_of(&fault.geometry))
        .collect();
    let (mut faults, other_horizons) =
        faults_on(traces, cloud.iter().map(|p| p[2])).map_err(|why| Refusal::on("faults", why))?;
    let mut buffered = 0usize;
    if !faults.is_empty() {
        if let Some(spacing) = params.fault_spacing {
            faults = faults
                .iter()
                .map(|line| resample(line, spacing))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|why| Refusal::on("faultSpacing", why))?;
        }
        if let Some(buffer) = params.fault_buffer {
            if !(buffer.is_finite() && buffer >= 0.0) {
                return Err(Refusal::on(
                    "faultBuffer",
                    format!("must be a finite length >= 0, not {buffer}"),
                ));
            }
            let before = cloud.len();
            cloud.retain(|p| plan_distance([p[0], p[1]], &faults) > buffer);
            buffered = before - cloud.len();
        }
    }
    let flat: Vec<f64> = cloud.iter().flatten().copied().collect();
    // **Two paths, one algorithm.** Without faults, the triangles index
    // the cloud as it is and nothing is added; with them, the triangulation returns
    // its own points — the vertices of the traces are part of them — and
    // that is what is written.
    let (points, indices, added, how) = if faults.is_empty() {
        let indices = triangulate_xy(&flat).map_err(|why| Refusal::on("shape", why))?;
        (cloud, indices, 0usize, "delaunay-xy")
    } else {
        let made = triangulate_xy_constrained(&flat, &faults).map_err(|why| Refusal::on("faults", why))?;
        (made.points, made.triangles, made.added, "constrained-delaunay-xy")
    };
    let triangles: Vec<[u32; 3]> = indices.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect();

    let name = format!("triangulation of {source}");
    let nodes = points.len();
    let surface = Shape {
        name: name.clone(),
        geometry: Geometry {
            kind: "triangulate",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points,
                topology: Topology::Triangles(triangles.clone()),
                closed: Vec::new(),
            }],
        },
        closed: None,
        known: None,
    };
    // What the input knows of itself travels through the context: its CRS,
    // its interpretation, its release. Without that the writing would ask again for a
    // frame the user had just named by choosing the object.
    let title = params.written.naming.title();
    let written = TrianglesInputs { shape: vec![surface] };
    let mut plan = super::triangles::write(&written, &params.written, &format!("{name}:{title}"), cx)?;

    plan.activity.recipe = "triangulate";
    plan.activity.title = format!("triangulate → {title}");
    plan.activity.parameters.push(Parameter::Text {
        title: "how".to_string(),
        value: how.to_string(),
    });
    plan.activity.parameters.push(Parameter::Double {
        title: "faultCount".to_string(),
        value: faults.len() as f64,
        uom: None,
    });
    plan.activity.parameters.push(Parameter::Double {
        title: "nodesAdded".to_string(),
        value: added as f64,
        uom: None,
    });
    plan.activity.parameters.push(Parameter::Double {
        title: "nodesBuffered".to_string(),
        value: buffered as f64,
        uom: None,
    });
    if other_horizons > 0 {
        plan.says.push(Note {
            key: "faults-elsewhere",
            detail: format!("{other_horizons} fault trace(s) lie at another horizon's depth and were set aside — only the traces at this horizon's depth constrain it"),
        });
    }
    if buffered > 0 {
        plan.says.push(Note {
            key: "nodes-buffered",
            detail: format!("{buffered} cloud point(s) within `faultBuffer` of a trace were set aside — near a fault the cloud is noise, and the trace decides"),
        });
    }
    if added > 0 {
        // An object that has more nodes than samples is an object
        // one must be warned about: these are the traces, and their
        // crossings, entered into the surface.
        plan.says.push(Note {
            key: "nodes-added",
            detail: format!(
                "{added} node(s) come from the {} fault trace(s) — their vertices and crossings — not from the cloud",
                faults.len()
            ),
        });
    }
    // **Duplicates are not an error, and they are counted.** Two
    // points at the same (x, y) are a single vertex for the Delaunay; the second
    // stays in the object written and is named by no triangle. Keeping quiet about it
    // would make a cloud with duplicates a surface that looks complete.
    let used = triangles
        .iter()
        .flatten()
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    if used < nodes {
        plan.says.push(Note {
            key: "nodes-unused",
            detail: format!(
                "{} of {nodes} node(s) are in no triangle — points sharing an (x, y) are one vertex for the triangulation, and the duplicates stay in the object",
                nodes - used
            ),
        });
    }
    Ok(plan)
}
