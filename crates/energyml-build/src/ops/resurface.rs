//! `resurface` — MBA's representation target: recompute a shape's own z
//! (or, in the mean-plane frame, its own height off the mean plane), keeping
//! its topology bit for bit. The need: give as input a grid […] or a
//! triangulatedSetRepresentation to apply MBA on it and have it create a new
//! representation […] reusing the same face indices for the triangulation,
//! with just a Z-value array to recreate for the Grid2D.
//!
//! # What makes this different from `estimate`
//!
//! In `estimate` the samples and the queries are two different things — a
//! shape and a cloud somebody hands in separately. Here they are **the same
//! set of points**: the shape's own nodes are fitted, and evaluated again at
//! their own `(x, y)` — a smoothing/re-approximation in place, never a
//! second calculation (a small algorithm extracts the points of the given
//! object, to reuse the code asked for above, then the same indices are
//! reused — exactly one branch, not a new computation).
//!
//! **Scoped to MBA only**, for the same reason `estimate` is
//! (`crates/energyml-build/src/ops/estimate.rs`'s own module doc) — a
//! deliberate reduction for this task, not a reading of the original need.
//!
//! # A new object, never an edit
//!
//! The house rule (`crates/energyml-build/ARCHITECTURE.md`): `apply` only ever
//! inserts new objects. This operation writes a fresh
//! `Grid2dRepresentation`/`TriangulatedSetRepresentation`, delegating to
//! [`super::oblique_grid::write`] or [`super::triangles::write`] exactly as
//! the input's own topology asks; the input stays untouched in the package.
//!
//! # `outer`/`inner` filter samples, never the topology
//!
//! The tension is plain: this operation
//! promises to keep the same indices, and a ring that removed nodes or
//! triangles would break that promise. The rule kept here is the proposed
//! one: a ring only decides which of the shape's own points
//! **inform the fit** — every point, ring-excluded or not, is still
//! evaluated and still appears in the output, at the same rank. A ring on
//! `resurface` filters only the samples, never the copied topology —
//! proposed, not a settled decision,
//! and this file follows it as the conservative reading (nothing disappears
//! silently from an object whose whole point is to keep its shape).
//!
//! # `frame = meanPlane` — a grid keeps `world`, a triangulated surface does not
//!
//! A `Grid2dRepresentation`'s height is a vertical `ZValueArray` (the same
//! reasoning as `interpolate_mba`'s module doc) — `meanPlane`
//! is refused, named, for a grid target. A `TriangulatedSetRepresentation`
//! writes explicit 3D points per node already
//! (`crates/energyml-build/src/ops/triangles.rs::write`, `point_geometry`) —
//! there is no vertical-only constraint to run into, and `meanPlane` is
//! written in full there: every node is projected to `(u, v)`, refitted, and
//! rotated back, generally moving it off its own former `(x, y)` too — a
//! point of the surface is returned, not an elevation.
//!
//! **Settled: this split is not a question left open** — a
//! grid target keeps `world`; a triangulated one carries `meanPlane`.
//! **Also settled: one mean plane for the whole shape**
//! — piecewise mean planes for a listric, strongly-curved fault are a later
//! increment, not written here; [`check_not_folded`] is what says when one
//! plane no longer suffices for a given input, rather than a silent guess.
//!
//! # References
//!
//! - Lee, Wolberg, Shin, *"Scattered Data Interpolation with Multilevel B-Splines"*, IEEE Transactions on Visualization and Computer Graphics, vol. 3, n° 3, 1997, p. 228-244 (DOI 10.1109/2945.620490), as cited in `energyml-geostat` (`mba`).
//!   TODO(reference): verify — the citation above (journal, pages, DOI) was not re-checked against the paper (see `energyml-geostat/src/mba.rs`).
//! - Principal components (Hoppe, 1992) reweighted with Huber's weights, as named in `energyml-geostat` (`extend`, reused by `mba_frame`).
//!   TODO(reference): verify — Hoppe (1992) and Huber's weights are named without a full citation, and the 1.4826 scale factor of the median absolute residual is unreferenced; add the references (authors, year, title, venue, DOI) once checked.

use energyml_analysis::mba::{Domain, Mba};
use energyml_analysis::mba_frame::{Frame as MeanFrame, check_not_folded};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

use crate::declare::{Inputs, Params, Shape, Suggestions};
use crate::operation::{About, Category, Context, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::Pieces;

use super::interpolate_mba::MbaFrame;
use super::support;
use super::triangles::TrianglesInputs;
use super::written::Written;

/// The operation.
pub struct Resurface;

/// What `resurface` is told.
#[derive(Debug, Params)]
pub struct ResurfaceParams {
    /// the finest cell size MBA refines its control grid to, in the units of the frame
    #[param(min = 0.0)]
    pub spacing: f64,
    /// advanced: the coarsest control grid's cell count along the first axis
    #[param(advanced, min = 1)]
    pub coarsest_i: Option<usize>,
    /// advanced: the coarsest control grid's cell count along the second axis
    #[param(advanced, min = 1)]
    pub coarsest_j: Option<usize>,
    /// the frame MBA is fitted in — a grid target keeps `world` (see the module doc); a triangulated surface carries `meanPlane` in full (settled)
    #[param(flatten)]
    pub frame: MbaFrame,
    /// The frame, what is represented, the naming.
    #[param(flatten)]
    pub written: Written,
}

/// What `resurface` takes. `topology = "grid"` is the form's hint only — a
/// triangulated surface is accepted too, and `check` is what actually judges
/// the topology, exactly as `triangles::check` already does for its own
/// input rather than trusting a single-word hint.
#[derive(Debug, Inputs)]
pub struct ResurfaceInputs {
    /// the grid or triangulated surface to recompute
    #[input(topology = "grid")]
    pub shape: Shape,
    /// keeps only the samples inside this ring for the fit — every node is still recomputed and kept (module doc)
    #[input(topology = "polylines")]
    pub outer: Option<Shape>,
    /// removes the samples inside each of these rings from the fit — every node is still recomputed and kept
    #[input(topology = "polylines")]
    pub inner: Vec<Shape>,
}

/// Which topology, owned rather than borrowed from the input `Shape` — this
/// file matches on it more than once, and an owned value sidesteps every
/// question of how long a borrow of `inputs.shape` would need to live.
#[derive(Clone)]
enum Kind {
    Grid { fastest: usize, slowest: usize },
    Triangles(Vec<[u32; 3]>),
}

/// The word a refusal names a topology by — the same vocabulary
/// `triangles.rs::check` already uses for its own refusal.
fn topology_word(topology: &Topology) -> &'static str {
    match topology {
        Topology::Grid { .. } => "a grid",
        Topology::Triangles(list) if list.is_empty() => "an empty triangle list",
        Topology::Triangles(_) => "triangles",
        Topology::Points => "points",
        Topology::Quads(_) => "quadrilaterals",
        Topology::Polygons(_) => "polygons",
        Topology::Polylines(_) => "polylines",
        _ => "cells",
    }
}

fn kind_of(shape: &Shape) -> Result<(Kind, Vec<[f64; 3]>), Refusal> {
    let [patch] = shape.geometry.patches.as_slice() else {
        return Err(Refusal::on(
            "shape",
            format!("`{}` holds {} patch(es): resurfacing one ajustement at a time needs exactly one", shape.name, shape.geometry.patches.len()),
        ));
    };
    match &patch.topology {
        Topology::Grid { fastest, slowest } => {
            if *fastest < 2 || *slowest < 2 || patch.points.len() != fastest * slowest {
                return Err(Refusal::on(
                    "shape",
                    format!("`{}`'s grid is {} by {} for {} point(s): its own two axis steps cannot be read off a malformed lattice", shape.name, fastest, slowest, patch.points.len()),
                ));
            }
            Ok((Kind::Grid { fastest: *fastest, slowest: *slowest }, patch.points.clone()))
        }
        Topology::Triangles(list) if !list.is_empty() => Ok((Kind::Triangles(list.clone()), patch.points.clone())),
        other => Err(Refusal::on(
            "shape",
            format!(
                "`{}`'s topology is {}: resurface recopies a lattice or a triangulation, it does not decide one — no topology to recopy here",
                shape.name,
                topology_word(other)
            ),
        )),
    }
}

/// The ranks of `points` kept for the fit: inside `outer` (if given),
/// outside every `inner` — the same rule `interpolate_mba`/`estimate` apply,
/// restated on ranks so every point can still be evaluated afterwards
/// (module doc).
fn fit_ranks(points: &[[f64; 3]], outer: Option<&Shape>, inner: &[Shape]) -> Result<Vec<usize>, Refusal> {
    if outer.is_none() && inner.is_empty() {
        return Ok((0..points.len()).collect());
    }
    let flat: Vec<f64> = points.iter().flat_map(|p| [p[0], p[1], 0.0]).collect();
    let ring_of = |slot: &str, shape: &Shape| -> Result<Vec<f64>, Refusal> {
        let ring_points: Vec<[f64; 3]> = shape.geometry.patches.iter().flat_map(|p| p.points.iter().copied()).collect();
        if ring_points.len() < 3 {
            return Err(Refusal::on(slot, format!("`{}` holds {} point(s): a ring encloses something with at least 3", shape.name, ring_points.len())));
        }
        Ok(ring_points.iter().flat_map(|p| [p[0], p[1]]).collect())
    };
    let mut keep: Option<std::collections::HashSet<usize>> = None;
    if let Some(shape) = outer {
        let ring = ring_of("outer", shape)?;
        let kept: std::collections::HashSet<usize> =
            energyml_geometry::clip::nodes_in_ring(&flat, &ring, false).map_err(|why| Refusal::on("outer", why))?.into_iter().map(|at| at as usize).collect();
        keep = Some(kept);
    }
    for shape in inner {
        let ring = ring_of("inner", shape)?;
        let kept: std::collections::HashSet<usize> =
            energyml_geometry::clip::nodes_in_ring(&flat, &ring, true).map_err(|why| Refusal::on("inner", why))?.into_iter().map(|at| at as usize).collect();
        keep = Some(match keep {
            Some(previous) => previous.into_iter().filter(|at| kept.contains(at)).collect(),
            None => kept,
        });
    }
    let mut ranks: Vec<usize> = keep.unwrap_or_default().into_iter().collect();
    ranks.sort_unstable();
    Ok(ranks)
}

impl Operation for Resurface {
    type Params = ResurfaceParams;
    type Inputs = ResurfaceInputs;

    fn about(&self) -> About {
        About {
            name: "resurface",
            describe: "MBA's own re-approximation of a grid's or a triangulated surface's z, keeping its topology bit for bit",
            category: Category::Geometry,
            makes: &["resqml22.Grid2dRepresentation", "resqml22.TriangulatedSetRepresentation"],
            recipe: "resurface",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
    }

    fn check(&self, inputs: &ResurfaceInputs) -> Result<(), Refusal> {
        kind_of(&inputs.shape).map(|_| ())
    }

    fn plan(&self, inputs: ResurfaceInputs, params: ResurfaceParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        let (kind, points) = kind_of(&inputs.shape)?;
        if matches!(&kind, Kind::Grid { .. }) && matches!(&params.frame, MbaFrame::MeanPlane) {
            return Err(Refusal::about(
                "value",
                "frame",
                "`meanPlane` is not written for a grid target (a grid height is not a mean-plane offset): a Grid2dRepresentation's height is a vertical ZValueArray, and a tilted result is offset along the mean plane's normal, not vertically. A triangulated surface carries `meanPlane` in full; a grid stays `world`",
            ));
        }

        cx.stop_if_cancelled()?;
        let fit_ranks = fit_ranks(&points, inputs.outer.as_ref(), &inputs.inner)?;
        if fit_ranks.is_empty() {
            return Err(Refusal::on("outer", "no node remains inside `outer`/outside every `inner`: nothing left to fit"));
        }
        let ring_excluded = points.len() - fit_ranks.len();

        let frame = match &params.frame {
            MbaFrame::World => None,
            MbaFrame::MeanPlane => {
                let fitted = MeanFrame::fit(&points).map_err(|e| Refusal::on("shape", e.to_string()))?;
                if let Kind::Triangles(triangles) = &kind {
                    check_not_folded(&points, triangles, &fitted).map_err(|e| Refusal::on("shape", e.to_string()))?;
                }
                Some(fitted)
            }
        };
        let to_local = |p: [f64; 3]| -> [f64; 3] { frame.map(|f| f.to_local(p)).unwrap_or(p) };
        let to_world = |local: [f64; 3]| -> [f64; 3] { frame.map(|f| f.to_world(local)).unwrap_or(local) };

        let local: Vec<[f64; 3]> = points.iter().map(|&p| to_local(p)).collect();
        let fit_uv: Vec<[f64; 2]> = fit_ranks.iter().map(|&at| [local[at][0], local[at][1]]).collect();
        let fit_w: Vec<f64> = fit_ranks.iter().map(|&at| local[at][2]).collect();
        let all_uv: Vec<[f64; 2]> = local.iter().map(|p| [p[0], p[1]]).collect();

        // Ω is every point's own bounding box — no margin: samples and
        // queries are the same set here (module doc), so coverage is never
        // a question the way it is for `estimate`.
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        for p in &all_uv {
            if p[0].is_finite() && p[1].is_finite() {
                lo[0] = lo[0].min(p[0]);
                lo[1] = lo[1].min(p[1]);
                hi[0] = hi[0].max(p[0]);
                hi[1] = hi[1].max(p[1]);
            }
        }
        if !(lo[0].is_finite() && lo[1].is_finite()) {
            return Err(Refusal::on("shape", "no node has a finite plan-view position: there is no domain to fit on"));
        }
        // A hair wider than the exact bounding box: the box is built from
        // *every* node while only `fit_uv` (its `outer`/`inner`-kept subset)
        // is actually fitted, and a kept node can sit exactly on the box's
        // own edge without ever being the one that defined it.
        let pad = 1e-6 * (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1.0);
        let domain = Domain { origin: [lo[0] - pad, lo[1] - pad], size: [(hi[0] - lo[0]) + 2.0 * pad, (hi[1] - lo[1]) + 2.0 * pad] };
        let ratio = domain.size[0] / domain.size[1];
        let formula = if ratio.is_finite() && ratio >= 1.0 { [ratio.round().max(1.0) as usize, 1] } else if ratio.is_finite() && ratio > 0.0 { [1, (1.0 / ratio).round().max(1.0) as usize] } else { [1, 1] };
        let coarsest = [params.coarsest_i.unwrap_or(formula[0]).max(1), params.coarsest_j.unwrap_or(formula[1]).max(1)];
        let mba = Mba::new(coarsest).map_err(|e| Refusal::on("coarsest", e.to_string()))?;
        let answer = mba.interpolate(&fit_uv, &fit_w, domain, params.spacing, &all_uv).map_err(|e| Refusal::on("spacing", e.to_string()))?;

        let new_points: Vec<[f64; 3]> = local.iter().zip(&answer.values).map(|(p, &w)| to_world([p[0], p[1], w])).collect();

        let seed = cx.seed(&params.written.naming.title());
        let mut plan = match kind {
            Kind::Grid { fastest, slowest } => {
                let origin = [points[0][0], points[0][1]];
                let (dir1_raw, dir2_raw) = ([points[1][0] - points[0][0], points[1][1] - points[0][1]], [points[fastest][0] - points[0][0], points[fastest][1] - points[0][1]]);
                let step1 = (dir1_raw[0] * dir1_raw[0] + dir1_raw[1] * dir1_raw[1]).sqrt();
                let step2 = (dir2_raw[0] * dir2_raw[0] + dir2_raw[1] * dir2_raw[1]).sqrt();
                if !(step1.is_finite() && step1 > 0.0 && step2.is_finite() && step2 > 0.0) {
                    return Err(Refusal::on("shape", "the grid's own two axis steps could not be read off its first row and column"));
                }
                let dir1 = [dir1_raw[0] / step1, dir1_raw[1] / step1];
                let dir2 = [dir2_raw[0] / step2, dir2_raw[1] / step2];
                // Computed, never affirmed — `Provisional`, same
                // reading and the same two thresholds as
                // `interpolate_mba::plan_given` (its module doc).
                let dot = dir1[0] * dir2[0] + dir1[1] * dir2[1];
                let orthogonal = if dot.abs() < 1e-6 {
                    Some(true)
                } else if dot.abs() > 1e-3 {
                    Some(false)
                } else {
                    None
                };
                let z: Vec<f64> = new_points.iter().map(|p| p[2]).collect();
                super::oblique_grid::write(&format!("resurface of {}", inputs.shape.name), origin, dir1, step1, fastest, dir2, step2, slowest, orthogonal, &z, &params.written, &seed, cx)?
            }
            Kind::Triangles(triangles) => {
                let new_shape = Shape {
                    name: inputs.shape.name.clone(),
                    geometry: Geometry {
                        kind: "triangles",
                        patches: vec![Patch { index: 0, node_offset: 0, points: new_points, topology: Topology::Triangles(triangles), closed: Vec::new() }],
                    },
                    closed: None,
                    known: None,
                };
                super::triangles::write(&TrianglesInputs { shape: vec![new_shape] }, &params.written, &seed, cx)?
            }
        };

        plan.activity.recipe = "resurface";
        plan.activity.title = format!("resurface → {}", params.written.naming.title());
        plan.activity.parameters.push(Parameter::Text { title: "method".to_string(), value: "mba".to_string() });
        plan.activity.parameters.push(Parameter::Double { title: "spacing".to_string(), value: params.spacing, uom: None });
        plan.activity.parameters.push(Parameter::Double { title: "levels".to_string(), value: answer.levels as f64, uom: None });
        plan.activity.parameters.push(Parameter::Double { title: "fitSampleCount".to_string(), value: fit_ranks.len() as f64, uom: None });
        if ring_excluded > 0 {
            plan.says.push(Note {
                key: "ring-filtered",
                detail: format!("{ring_excluded} node(s) outside `outer`/`inner` were not fitted, but are still recomputed and kept — the topology is not touched"),
            });
        }
        if answer.set_aside > 0 {
            plan.says.push(Note { key: "set-aside", detail: format!("{} sample(s) set aside: a value or a coordinate that is not a finite number", answer.set_aside) });
        }
        Ok(plan)
    }

    fn reports(&self) -> Vec<Slot> {
        support::frame_reports(vec![])
    }
}
