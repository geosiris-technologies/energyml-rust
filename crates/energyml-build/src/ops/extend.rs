//! `extend` — a polyline prolonged at its ends, a triangulated surface grown
//! past its border.
//!
//! The need: extend a point cloud, a surface or polylines to widen a model
//! synthetically. For example, a fault surface that stops too low: widen that
//! surface by 10 m upwards, 5 downwards and 8 on the sides.
//!
//! The computation is `energyml_analysis::extend` — the direction read on a
//! robust fit to a **band** of the border, the rings, the dip frame, the
//! refusals; this is the seam, and the writing is delegated to the writer of
//! the kind the input came as (`polylines::write`, `triangles::write`).
//!
//! # Two axes
//!
//! The method is a parameter of the operation, and each method has its
//! own sub-parameters: one operation `extend`, and two unions —
//!
//! - **`method`**, how the direction is read and followed. `linear` only: the
//!   band, and straight on. `natural` (the curvature, damped to zero) is the
//!   second method and is not written — a case a form would offer and the
//!   operation refuse would be a promise nobody keeps;
//! - **`toward`**, how far: `around` (one distance), `ends` (a polyline's
//!   start and end), `dip` (up, down and sides, along the surface or as a
//!   height, in the frame of the surface's mean plane — or of the vertical, for
//!   a polyline). `until` a surface waits for a ray caster.
//!
//! # The invented is never mixed silently with the measured
//!
//! - **a new object**, as for every operation (so that the earlier
//!   version is always there to compare with): the input stays as it was;
//! - **inside it, the original nodes come first and bit for bit** — for a
//!   surface, nodes `0..n` and the triangles that were there, in their order;
//!   for a polyline, its nodes in the middle, the invented ones before and
//!   after. The plan says where the invented ones are (`invented`), and the
//!   activity records how many (`originalNodes`, `addedNodes`);
//! - **the artefact tree has a child**: the invented nodes as a
//!   cloud under the result, so the scene can show them apart. It has no object
//!   of its own — it is the part of the representation the root names.
//!
//! The property *distance to the original border* that the first design leaves open is not
//! written: the count and the range say the same, and the property can come
//! when somebody asks for it.
//!
//! # What is refused
//!
//! At the offer, by name: a **point cloud** (it has no border of its own —
//! `triangulate` it, then extend the surface: the second reading, which
//! needs no code of its own), a **closed** polyline (a ring has no end; widening
//! it is an offset, another operation), a 2D grid, a surface of several
//! patches (two patches meeting along a seam would be extended along it), mixed
//! kinds. At the computation: what `energyml_analysis::extend` refuses — a flat
//! surface in the dip frame, fronts that cross, a closed surface, several
//! pieces.
//!
//! # References
//!
//! - Principal components (Hoppe, 1992) reweighted with Huber's weights, as named in `energyml-geostat` (`extend`, reused by `mba_frame`).
//!   TODO(reference): verify — Hoppe (1992) and Huber's weights are named without a full citation, and the 1.4826 scale factor of the median absolute residual is unreferenced; add the references (authors, year, title, venue, DOI) once checked.

use energyml_analysis::extend::{Dip, ExtendError, Reach, Setting, extend_line, extend_surface};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};
use serde_json::json;

use crate::declare::{Choice, Inputs, Params, Shape, Suggestions, known_of};
use crate::operation::{About, Category, Context, Made, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::Pieces;

use super::polylines::{PolylinesInputs, said_closed};
use super::support;
use super::thickness::Vertical;
use super::triangles::TrianglesInputs;
use super::written::Written;

/// The operation.
pub struct Extend;

/// Past this many times the band, the extension reaches farther than the
/// direction was read over — said, not refused.
const FAR: f64 = 3.0;

/// What `extend` is told.
#[derive(Debug, Params)]
pub struct ExtendParams {
    /// How the direction is read and followed.
    #[param(flatten)]
    pub method: Method,
    /// How far.
    #[param(flatten)]
    pub toward: Toward,
    /// The frame, what is represented, the naming — for the writer of the result.
    #[param(flatten)]
    pub written: Written,
}

/// how the direction is read and followed — linear: straight on, along a line or a plane fitted to a band of the border
#[derive(Debug, Params)]
#[params(tag = "method")]
pub enum Method {
    /// Straight on, along the fitted direction.
    #[case(default)]
    Linear(LinearParams),
}

/// The linear extension.
#[derive(Debug, Params)]
pub struct LinearParams {
    /// linear: how much of the border the direction is read over — a polyline's last so much of length, the nodes within so much of a surface's border node — in the units of the frame; too narrow reads the noise, too wide smooths a real bend
    #[param(min = 0)]
    pub band: f64,
}

/// how far — one distance all around, a polyline's two ends, or up, down and sides in the dip frame
#[derive(Debug, Params)]
#[params(tag = "toward")]
pub enum Toward {
    /// The same distance in every outward direction.
    #[case(default)]
    Around(AroundParams),
    /// A polyline's start and end.
    Ends(EndsParams),
    /// Up, down and sides.
    Dip(DipParams),
}

/// Around.
#[derive(Debug, Params)]
pub struct AroundParams {
    /// around: how far past the border, in every outward direction, in the units of the frame
    #[param(min = 0)]
    pub distance: f64,
}

/// By the ends.
#[derive(Debug, Params)]
pub struct EndsParams {
    /// ends: how far before a polyline's first node, in the units of the frame
    #[param(default = 0.0, optional, min = 0)]
    pub start: f64,
    /// ends: how far after a polyline's last node, in the units of the frame
    #[param(default = 0.0, optional, min = 0)]
    pub end: f64,
}

/// In the dip frame.
#[derive(Debug, Params)]
pub struct DipParams {
    /// dip: how far up — up the steepest line of the surface's mean plane, or for a polyline's end pointing up
    #[param(default = 0.0, optional, min = 0)]
    pub up: f64,
    /// dip: how far down
    #[param(default = 0.0, optional, min = 0)]
    pub down: f64,
    /// dip: how far along the horizontal of the plane, or for a level end; in between, the answers blend by the square of the direction's components
    #[param(default = 0.0, optional, min = 0)]
    pub sides: f64,
    /// dip: whether `up` and `down` are lengths along the surface or heights — on a fault dipping 60°, 10 m along the dip is 8.7 m of height
    #[param(default = Measure::Along)]
    pub measure: Measure,
    /// dip: what the third coordinate measures — an elevation (larger is higher) or a depth (larger is lower)
    ///
    /// Suggested from the CRS the reader read, never from the shape: up is a
    /// fact of the frame.
    pub axis: Vertical,
    /// dip: below this dip of the mean plane, in degrees, a surface has no up and no down, and the dip frame is refused
    #[param(default = 5.0, optional, min = 0, max = 90, advanced)]
    pub flat_below: f64,
}

/// How `up` and `down` are measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum Measure {
    /// Along the surface.
    Along,
    /// As a height.
    Height,
}

/// What `extend` takes.
#[derive(Debug, Inputs)]
pub struct ExtendInputs {
    /// the polylines or the triangulated surface to extend
    #[input(topology = "triangles")]
    pub shape: Shape,
}

/// What the shape is, once checked.
enum Kind {
    Lines,
    Surface,
}

fn kind_of(shape: &Shape) -> Result<Kind, Refusal> {
    let refuse = |why: String| Err(Refusal::on("shape", why));
    let patches = &shape.geometry.patches;
    if patches.is_empty() {
        return refuse("it has no patch".to_string());
    }
    let lines = patches
        .iter()
        .filter(|p| matches!(p.topology, Topology::Polylines(_)))
        .count();
    let surfaces = patches
        .iter()
        .filter(|p| matches!(p.topology, Topology::Triangles(_)))
        .count();
    if lines == patches.len() {
        let mut at = 0usize;
        for patch in patches {
            let Topology::Polylines(runs) = &patch.topology else {
                continue;
            };
            for local in 0..runs.len() {
                if said_closed(patch, local, shape.closed.as_deref(), at).unwrap_or(false) {
                    return refuse(format!(
                        "polyline {at} is closed — a ring has no end to prolong; widening it is an offset, another operation"
                    ));
                }
                at += 1;
            }
        }
        return Ok(Kind::Lines);
    }
    if surfaces == patches.len() {
        if surfaces > 1 {
            return refuse(format!(
                "it holds {surfaces} triangulated patches — two patches meeting along a seam would be extended along it; extend one patch at a time"
            ));
        }
        return Ok(Kind::Surface);
    }
    if lines + surfaces > 0 {
        return refuse("its patches mix lines and faces — one extension extends one kind".to_string());
    }
    match &patches[0].topology {
        Topology::Points => refuse(
            "it is a point cloud, which has no border of its own — `triangulate` it, then extend the surface"
                .to_string(),
        ),
        Topology::Grid { .. } => refuse(
            "it is a 2D grid — a lattice grows by rows and columns, not by rings, and that is not written"
                .to_string(),
        ),
        _ => refuse(
            "it holds neither polylines nor triangles — only those have a border to extend".to_string(),
        ),
    }
}

impl Operation for Extend {
    type Params = ExtendParams;
    type Inputs = ExtendInputs;

    fn about(&self) -> About {
        About {
            name: "extend",
            describe: "polylines prolonged at their ends, or a triangulated surface grown past its border — along a direction read on a band of the border; the original nodes kept bit for bit, the invented ones marked apart",
            category: Category::Geometry,
            makes: &[
                "resqml22.PolylineSetRepresentation",
                "resqml22.TriangulatedSetRepresentation",
            ],
            recipe: "extend",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
        if let Some(up) = known_of(bag, "shape").and_then(|known| known.vertical_up) {
            slots.set("axis", json!(if up { "elevation" } else { "depth" }));
        }
    }

    fn check(&self, inputs: &ExtendInputs) -> Result<(), Refusal> {
        kind_of(&inputs.shape).map(|_| ())
    }

    fn plan(&self, inputs: ExtendInputs, params: ExtendParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        let shape = inputs.shape;
        let kind = kind_of(&shape)?;
        let Method::Linear(linear) = &params.method;
        let band = linear.band;
        let (reach, distance_slot) = match &params.toward {
            Toward::Around(around) => (Reach::Around(around.distance), "distance"),
            Toward::Ends(ends) => (
                Reach::Ends {
                    start: ends.start,
                    end: ends.end,
                },
                "end",
            ),
            Toward::Dip(dip) => (
                Reach::Dip(Dip {
                    up: dip.up,
                    down: dip.down,
                    sides: dip.sides,
                    height: dip.measure == Measure::Height,
                    plus_z_is_up: dip.axis == Vertical::Elevation,
                    flat_below: dip.flat_below,
                }),
                "up",
            ),
        };
        let refuse = |why: ExtendError| match &why {
            ExtendError::Setting {
                which: Setting::Band, ..
            } => Refusal::about("value", "band", why.to_string()),
            ExtendError::Setting {
                which: Setting::Distance,
                ..
            } => Refusal::about("value", distance_slot, why.to_string()),
            ExtendError::Setting {
                which: Setting::Toward,
                ..
            } => Refusal::about("value", "toward", why.to_string()),
            ExtendError::Shape(_) => Refusal::on("shape", why),
        };

        let mut says = Vec::new();
        let (result, invented, original, added, outliers, longest) = match kind {
            Kind::Lines => {
                let mut patches = Vec::new();
                let mut invented = Vec::new();
                let (mut original, mut added, mut outliers, mut longest) = (0usize, 0usize, 0usize, 0.0f64);
                let mut ranges = Vec::new();
                let mut at = 0usize;
                for patch in &shape.geometry.patches {
                    let Topology::Polylines(runs) = &patch.topology else {
                        continue;
                    };
                    let mut points = Vec::new();
                    let mut counts = Vec::new();
                    let mut start = 0usize;
                    for &run in runs {
                        let run = run as usize;
                        let line = patch.points.get(start..start + run).ok_or_else(|| {
                            Refusal::on("shape", "the polylines add up to more nodes than the patch holds")
                        })?;
                        let made = extend_line(line, &reach, band).map_err(|why| match why {
                            ExtendError::Shape(text) => {
                                Refusal::on("shape", format!("polyline {at}: {text}"))
                            }
                            other => refuse(other),
                        })?;
                        invented.extend_from_slice(&made.points[..made.before]);
                        invented.extend_from_slice(&made.points[made.before + run..]);
                        ranges.push(format!("line {at}: {} before, {} after", made.before, made.after));
                        original += run;
                        added += made.before + made.after;
                        outliers += made.outliers;
                        longest = [&made.start, &made.end]
                            .into_iter()
                            .flatten()
                            .map(|e| e.distance)
                            .fold(longest, f64::max);
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
                if added == 0 {
                    return Err(Refusal::about(
                        "value",
                        distance_slot,
                        "every distance is zero — there is nothing to extend",
                    ));
                }
                says.push(Note {
                    key: "invented",
                    detail: format!(
                        "{added} node(s) invented past the ends, each line keeping its own nodes bit for bit between them — {}",
                        ranges.join("; ")
                    ),
                });
                let mut offset = 0usize;
                for patch in &mut patches {
                    patch.node_offset = offset;
                    offset += patch.points.len();
                }
                (
                    Geometry {
                        kind: "extend",
                        patches,
                    },
                    invented,
                    original,
                    added,
                    outliers,
                    longest,
                )
            }
            Kind::Surface => {
                let patch = &shape.geometry.patches[0];
                let Topology::Triangles(triangles) = &patch.topology else {
                    unreachable!("checked")
                };
                let made = extend_surface(&patch.points, triangles, &reach, band).map_err(refuse)?;
                let original = made.original_nodes;
                let added = made.points.len() - original;
                let longest = made.outward.iter().map(|o| o.distance).fold(0.0, f64::max);
                says.push(Note {
                    key: "invented",
                    detail: format!(
                        "{added} node(s) and {} triangle(s) invented in {} ring(s) past the border — nodes 0..{original} and triangles 0..{} are the original ones bit for bit, nodes {original}..{} are invented",
                        made.triangles.len() - made.original_triangles,
                        made.rings,
                        made.original_triangles,
                        made.points.len()
                    ),
                });
                if let Reach::Dip(_) = reach {
                    says.push(Note {
                        key: "dip",
                        detail: format!(
                            "up, down and sides read in the mean plane, which dips {:.2}°",
                            made.dip
                        ),
                    });
                }
                if made.holes > 0 {
                    says.push(Note {
                        key: "holes",
                        detail: format!(
                            "{} hole(s) left as they are — only the outer border is extended",
                            made.holes
                        ),
                    });
                }
                let invented = made.points[original..].to_vec();
                let geometry = Geometry {
                    kind: "extend",
                    patches: vec![Patch {
                        index: patch.index,
                        node_offset: 0,
                        points: made.points,
                        topology: Topology::Triangles(made.triangles),
                        closed: patch.closed.clone(),
                    }],
                };
                (geometry, invented, original, added, made.outliers, longest)
            }
        };
        if outliers > 0 {
            says.push(Note {
                key: "outliers",
                detail: format!("{outliers} node(s) near the border stand off the direction read there and were weighted down — look at them"),
            });
        }
        if longest > FAR * band {
            says.push(Note {
                key: "far",
                detail: format!(
                    "extended up to {longest}, {:.1} times the band of {band} the direction was read over",
                    longest / band
                ),
            });
        }

        let written_kind = matches!(kind, Kind::Lines);
        let result = Shape {
            name: shape.name.clone(),
            geometry: result,
            closed: shape.closed.clone(),
            known: shape.known.clone(),
        };
        let title = params.written.naming.title();
        let seed = format!("extend:{}:{title}", shape.name);
        let mut plan = if written_kind {
            super::polylines::write(
                &PolylinesInputs {
                    shape: vec![result],
                    features: None,
                },
                &params.written,
                None,
                &seed,
                cx,
            )?
        } else {
            super::triangles::write(
                &TrianglesInputs { shape: vec![result] },
                &params.written,
                &seed,
                cx,
            )?
        };
        plan.activity.recipe = "extend";
        plan.activity.title = format!("extend → {title}");
        plan.activity.parameters.extend([
            Parameter::Double {
                title: "originalNodes".to_string(),
                value: original as f64,
                uom: None,
            },
            Parameter::Double {
                title: "addedNodes".to_string(),
                value: added as f64,
                uom: None,
            },
            Parameter::Double {
                title: "outlierNodes".to_string(),
                value: outliers as f64,
                uom: None,
            },
        ]);
        plan.says.extend(says);
        // The invented nodes, a child of the result: the scene shows them
        // apart, and the root keeps the whole shape for what comes next.
        if let Some(root) = plan.shapes.first().cloned() {
            plan.shapes.push(Made {
                id: format!("{}-invented", root.id),
                parent: Some(root.id.clone()),
                title: format!("{} — invented nodes", root.title),
                geometry: Geometry {
                    kind: "extend",
                    patches: vec![Patch {
                        index: 0,
                        node_offset: 0,
                        points: invented,
                        topology: Topology::Points,
                        closed: Vec::new(),
                    }],
                },
                object: None,
            });
        }
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
            count("originalNodes", "nodes of the input, kept bit for bit"),
            count("addedNodes", "nodes invented past the border"),
            count("outlierNodes", "border nodes the robust fit weighted down"),
        ])
    }
}
