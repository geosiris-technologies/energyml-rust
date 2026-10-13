//! `estimate` — MBA's cloud target: the z of a cloud the caller supplies,
//! without producing a regular grid. This is the cloud-target half of the MBA
//! design, answering the need to supply a set of
//! points and ask for an estimate of each one's Z, so as not to
//! necessarily produce regular grids.
//!
//! # Why this is its own operation, and not a fourth thing `interpolate` writes
//!
//! `interpolate` only ever writes a `resqml22.Grid2dRepresentation`
//! (`about().makes`) — teaching it a second output type under the same name
//! would make the registry's `makes()` contract, checked before a run
//! starts, lie about what a given run will produce. Nothing
//! in the core changes (`energyml_analysis::mba::Mba::interpolate` already
//! takes an arbitrary `queries: &[[f64; 2]]`, regular or not) — only the
//! writer at the end differs, `pointset`'s rather than `grid`'s.
//!
//! **Scoped to MBA only, unlike the original proposal, which
//! reuses `interpolate`'s full `Idw`/`Kriging`/`Rbf`/`Mba` union.** This
//! file's `method` is [`super::interpolate_mba::MbaParams`]'s own settings
//! (minus the extent, meaningless for a cloud target), not a choice of
//! method — a deliberate reduction of scope for this task, not a reading of
//! the specification: extending this operation to the other three methods is
//! mechanical (they already answer `(at, values, queries) -> values` with
//! no notion of a domain) but was left out to keep this file's own
//! correctness — the frame handling, the domain-union rule, the ring
//! semantics — checkable on its own.
//!
//! # The domain — the one rule specific to this target
//!
//! Unlike `interpolate`'s grid (where `Ω` is always the very lattice being
//! written), nothing here fixes the output's extent: a query in `at` can sit
//! anywhere, including past every sample. `Ω` is the bounding box of
//! **samples ∪ queries**, widened by `margin` — never the samples alone,
//! which would leave a distant query outside a domain MBA is not defined on.
//! **Provisional (open question: rings on the query cloud):** `outer`/`inner`, when given, also filter `at`
//! — a query outside the kept region is left `NaN` and counted, exactly the
//! contract `interpolate` already keeps for a node no sample can reach —
//! rather than only ever filtering the samples. The design poses this as
//! an open question; this is the conservative reading (counted, never a
//! silently-dropped point that a caller counting rows would not notice is
//! missing).
//!
//! # `frame = meanPlane`
//!
//! Unlike `interpolate`'s grid, a point set has no schema tying its height
//! to a vertical array over a lattice: `meanPlane` is written in full here.
//! A query is read off in the mean plane's own `(u, v)`, evaluated, and
//! rotated back — what is returned is a point of the surface, not a
//! height: the point this operation writes is generally **not**
//! directly above or below the query's own `(x, y)`, because the inverse
//! rotation moves it along the plane's own tilt. Refused, named, when
//! combined with a `field` override: the mean plane is fitted to the
//! samples' own geometry, and a field replacing the values leaves no surface
//! to tilt into.
//!
//! # References
//!
//! - Lee, Wolberg, Shin, *"Scattered Data Interpolation with Multilevel B-Splines"*, IEEE Transactions on Visualization and Computer Graphics, vol. 3, n° 3, 1997, p. 228-244 (DOI 10.1109/2945.620490), as cited in `energyml-geostat` (`mba`).
//!   TODO(reference): verify — the citation above (journal, pages, DOI) was not re-checked against the paper (see `energyml-geostat/src/mba.rs`).
//! - Principal components (Hoppe, 1992) reweighted with Huber's weights, as named in `energyml-geostat` (`extend`, reused by `mba_frame`).
//!   TODO(reference): verify — Hoppe (1992) and Huber's weights are named without a full citation, and the 1.4826 scale factor of the median absolute residual is unreferenced; add the references (authors, year, title, venue, DOI) once checked.

use energyml_analysis::mba::{Domain, Mba};
use energyml_analysis::mba_frame::Frame as MeanFrame;
use energyml_domain::provenance::Parameter;

use crate::declare::{Field, Inputs, Params, Shape, Suggestions};
use crate::operation::{About, Category, Context, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::Pieces;

use super::interpolate_mba::MbaFrame;
use super::support;
use super::written::Written;

/// The operation.
pub struct Estimate;

/// What `estimate` is told.
#[derive(Debug, Params)]
pub struct EstimateParams {
    /// the finest cell size MBA refines its control grid to, in the units of the frame — the smaller, the closer the fit to the samples, at the cost of more levels
    #[param(min = 0.0)]
    pub spacing: f64,
    /// how far past the samples **and** the query cloud the fitted domain reaches, in the same units
    #[param(default = 0.0, optional, min = 0.0)]
    pub margin: f64,
    /// advanced: the coarsest control grid's cell count along the first axis — left out, the paper's own rule, never the sample density
    #[param(advanced, min = 1)]
    pub coarsest_i: Option<usize>,
    /// advanced: the coarsest control grid's cell count along the second axis
    #[param(advanced, min = 1)]
    pub coarsest_j: Option<usize>,
    /// the reference frame MBA is fitted in — `world` or `meanPlane`; this operation carries `meanPlane` in full
    #[param(flatten)]
    pub frame: MbaFrame,
    /// The frame, what is represented, the naming — for the point set writer.
    #[param(flatten)]
    pub written: Written,
}

/// What `estimate` takes.
#[derive(Debug, Inputs)]
pub struct EstimateInputs {
    /// the samples: a shape whose nodes carry the values MBA is fitted to
    pub samples: Shape,
    /// the values, instead of the samples' elevation
    pub field: Option<Field>,
    /// the cloud whose z is estimated — its own z, if it has one, is ignored and replaced
    #[input(topology = "points")]
    pub at: Shape,
    /// keeps only the samples inside this ring
    #[input(topology = "polylines")]
    pub outer: Option<Shape>,
    /// removes the samples inside each of these rings; also removes the query points inside them (provisional: open question on the query cloud)
    #[input(topology = "polylines")]
    pub inner: Vec<Shape>,
}

/// The plan-view `(x, y)` of a 3D point — what a ring filters on.
fn plan(p: [f64; 3]) -> [f64; 2] {
    [p[0], p[1]]
}

/// Keeps the ranks of `points` inside `outer` (when given) and outside every
/// `inner` — the same rule `interpolate_mba::filter_by_rings` applies to
/// samples, restated on plain ranks so it can be reused for the query cloud
/// too (provisional: open question on the query cloud).
fn ring_keep(points: &[[f64; 3]], outer: Option<&Shape>, inner: &[Shape]) -> Result<Option<Vec<usize>>, Refusal> {
    if outer.is_none() && inner.is_empty() {
        return Ok(None);
    }
    let flat: Vec<f64> = points.iter().flat_map(|&p| { let [x, y] = plan(p); [x, y, 0.0] }).collect();
    let ring_of = |slot: &str, shape: &Shape| -> Result<Vec<f64>, Refusal> {
        let ring_points: Vec<[f64; 3]> = shape.geometry.patches.iter().flat_map(|p| p.points.iter().copied()).collect();
        if ring_points.len() < 3 {
            return Err(Refusal::on(slot, format!("`{}` holds {} point(s): a ring encloses something with at least 3", shape.name, ring_points.len())));
        }
        Ok(ring_points.iter().flat_map(|p| [p[0], p[1]]).collect())
    };
    let mut keep: Option<Vec<usize>> = None;
    if let Some(shape) = outer {
        let ring = ring_of("outer", shape)?;
        let kept = energyml_geometry::clip::nodes_in_ring(&flat, &ring, false).map_err(|why| Refusal::on("outer", why))?;
        keep = Some(kept.into_iter().map(|at| at as usize).collect());
    }
    for shape in inner {
        let ring = ring_of("inner", shape)?;
        let kept: std::collections::HashSet<usize> =
            energyml_geometry::clip::nodes_in_ring(&flat, &ring, true).map_err(|why| Refusal::on("inner", why))?.into_iter().map(|at| at as usize).collect();
        keep = Some(match keep {
            Some(previous) => previous.into_iter().filter(|at| kept.contains(at)).collect(),
            None => kept.into_iter().collect(),
        });
    }
    Ok(keep)
}

impl Operation for Estimate {
    type Params = EstimateParams;
    type Inputs = EstimateInputs;

    fn about(&self) -> About {
        About {
            name: "estimate",
            describe: "MBA's z at a cloud of points you supply — no regular grid, unlike `interpolate`",
            category: Category::Geometry,
            makes: &["resqml22.PointSetRepresentation"],
            recipe: "estimate",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
    }

    fn check(&self, inputs: &EstimateInputs) -> Result<(), Refusal> {
        let samples: usize = inputs.samples.geometry.patches.iter().map(|p| p.points.len()).sum();
        if samples == 0 {
            return Err(Refusal::on("samples", "no samples to fit MBA to"));
        }
        let at: usize = inputs.at.geometry.patches.iter().map(|p| p.points.len()).sum();
        if at == 0 {
            return Err(Refusal::on("at", "no point to estimate"));
        }
        if let Some(field) = &inputs.field
            && field.over != inputs.samples.name
            && !field.over.starts_with(&format!("{} ", inputs.samples.name))
        {
            return Err(Refusal::on("field", format!("lies over `{}`, and the samples are `{}`", field.over, inputs.samples.name)));
        }
        Ok(())
    }

    fn plan(&self, inputs: EstimateInputs, params: EstimateParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        let samples_points: Vec<[f64; 3]> = inputs.samples.geometry.patches.iter().flat_map(|p| p.points.iter().copied()).collect();
        let field_values = inputs.field.as_ref().map(|f| f.values.clone());
        let from_field = field_values.is_some();
        if from_field && matches!(&params.frame, MbaFrame::MeanPlane) {
            return Err(Refusal::about(
                "value",
                "frame",
                "`meanPlane` fits a mean plane to the samples' own geometry; a `field` replacing their values leaves no surface to tilt into — leave `field` out, or use `world`",
            ));
        }
        let values: Vec<f64> = field_values.unwrap_or_else(|| samples_points.iter().map(|p| p[2]).collect());
        if samples_points.len() != values.len() {
            return Err(Refusal::on(
                "field",
                format!("`{}` holds {} node(s) and the field {} value(s)", inputs.samples.name, samples_points.len(), values.len()),
            ));
        }
        let at_points: Vec<[f64; 3]> = inputs.at.geometry.patches.iter().flat_map(|p| p.points.iter().copied()).collect();

        cx.stop_if_cancelled()?;

        // Rings, on samples and on the query cloud alike (module doc,
        // provisional open question on the query cloud) — before anything is fitted or evaluated.
        let sample_keep = ring_keep(&samples_points, inputs.outer.as_ref(), &inputs.inner)?;
        let (fit_points, fit_values): (Vec<[f64; 3]>, Vec<f64>) = match &sample_keep {
            Some(keep) => keep.iter().map(|&at| (samples_points[at], values[at])).unzip(),
            None => (samples_points.clone(), values.clone()),
        };
        let ring_dropped_samples = samples_points.len() - fit_points.len();
        if fit_points.is_empty() {
            return Err(Refusal::on("outer", "no sample remains inside `outer`/outside every `inner`"));
        }
        let at_keep = ring_keep(&at_points, inputs.outer.as_ref(), &inputs.inner)?;
        let at_excluded: std::collections::HashSet<usize> = match &at_keep {
            Some(keep) => (0..at_points.len()).filter(|i| !keep.contains(i)).collect(),
            None => std::collections::HashSet::new(),
        };

        // The frame — world, or the mean plane fitted to the *kept* samples.
        let frame = match &params.frame {
            MbaFrame::World => None,
            MbaFrame::MeanPlane => Some(MeanFrame::fit(&fit_points).map_err(|e| Refusal::on("samples", e.to_string()))?),
        };
        let to_local = |p: [f64; 3]| -> [f64; 3] { frame.map(|f| f.to_local(p)).unwrap_or(p) };
        let to_world = |local: [f64; 3]| -> [f64; 3] { frame.map(|f| f.to_world(local)).unwrap_or(local) };

        let samples_uv: Vec<[f64; 2]> = fit_points.iter().map(|&p| { let l = to_local(p); [l[0], l[1]] }).collect();
        // What is fitted: in the world frame, **the values** — the elevation,
        // or the field when one is given; in the mean-plane frame, the local
        // height above the plane (a field is refused there, above). The first
        // version fitted the local height in both cases, i.e. the elevation
        // even when a field was given: a porosity was estimated from the
        // samples' z. Caught by the compiler's unused-`fit_values` warning
        // held by `tests/mba.rs`.
        let fitted_w: Vec<f64> = match frame {
            None => fit_values,
            Some(_) => fit_points.iter().map(|&p| to_local(p)[2]).collect(),
        };
        let evaluate_at: Vec<usize> = (0..at_points.len()).filter(|i| !at_excluded.contains(i)).collect();
        let queries_uv: Vec<[f64; 2]> = evaluate_at.iter().map(|&i| { let l = to_local(at_points[i]); [l[0], l[1]] }).collect();

        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        for p in samples_uv.iter().chain(&queries_uv) {
            if p[0].is_finite() && p[1].is_finite() {
                lo[0] = lo[0].min(p[0]);
                lo[1] = lo[1].min(p[1]);
                hi[0] = hi[0].max(p[0]);
                hi[1] = hi[1].max(p[1]);
            }
        }
        if !(lo[0].is_finite() && lo[1].is_finite()) {
            return Err(Refusal::on("samples", "no sample or query has a finite plan-view position: there is no domain to fit on"));
        }
        let domain = Domain { origin: [lo[0] - params.margin, lo[1] - params.margin], size: [(hi[0] - lo[0]) + 2.0 * params.margin, (hi[1] - lo[1]) + 2.0 * params.margin] };
        let ratio = domain.size[0] / domain.size[1];
        let formula = if ratio.is_finite() && ratio >= 1.0 { [ratio.round().max(1.0) as usize, 1] } else if ratio.is_finite() && ratio > 0.0 { [1, (1.0 / ratio).round().max(1.0) as usize] } else { [1, 1] };
        let coarsest = [params.coarsest_i.unwrap_or(formula[0]).max(1), params.coarsest_j.unwrap_or(formula[1]).max(1)];
        let mba = Mba::new(coarsest).map_err(|e| Refusal::on("coarsest", e.to_string()))?;
        let answer = mba.interpolate(&samples_uv, &fitted_w, domain, params.spacing, &queries_uv).map_err(|e| Refusal::on("spacing", e.to_string()))?;

        // Scatter back onto every query, in `at`'s own order: an excluded
        // one keeps its own (x, y), height `NaN` — counted, never silently
        // absent. An evaluated one is rebuilt in world coordinates: in the
        // mean-plane case this generally moves its (x, y) too — what is returned is a point
        // of the surface, not a height (module doc).
        let mut written_points = vec![[f64::NAN; 3]; at_points.len()];
        for (rank, &original) in evaluate_at.iter().enumerate() {
            let uv = queries_uv[rank];
            written_points[original] = to_world([uv[0], uv[1], answer.values[rank]]);
        }
        for &excluded in &at_excluded {
            let [x, y] = plan(at_points[excluded]);
            written_points[excluded] = [x, y, f64::NAN];
        }

        let seed = cx.seed(&params.written.naming.title());
        let mut plan = super::pointset::write(&format!("estimate of {}", inputs.samples.name), written_points, &params.written, &seed, cx)?;
        plan.activity.recipe = "estimate";
        plan.activity.title = format!("estimate → {}", params.written.naming.title());
        plan.activity.parameters.push(Parameter::Text { title: "method".to_string(), value: "mba".to_string() });
        plan.activity.parameters.push(Parameter::Double { title: "spacing".to_string(), value: params.spacing, uom: None });
        plan.activity.parameters.push(Parameter::Double { title: "margin".to_string(), value: params.margin, uom: None });
        plan.activity.parameters.push(Parameter::Double { title: "sampleCount".to_string(), value: fit_points.len() as f64, uom: None });
        plan.activity.parameters.push(Parameter::Double { title: "levels".to_string(), value: answer.levels as f64, uom: None });
        if ring_dropped_samples > 0 {
            plan.says.push(Note { key: "ring-filtered", detail: format!("{ring_dropped_samples} sample(s) removed by `outer`/`inner` before fitting") });
        }
        if !at_excluded.is_empty() {
            plan.says.push(Note { key: "ring-excluded-queries", detail: format!("{} query point(s) outside `outer`/`inner`: left NaN, not evaluated", at_excluded.len()) });
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
