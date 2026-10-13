//! `Method::Mba` for `interpolate` - the MBA case's own settings and the
//! whole of its planning, kept out of `interpolate.rs` so that file gains
//! only the case itself and a two-line delegation (the MBA integration
//! proposal).
//!
//! # Why this is a whole plan of its own, not another arm of `run`
//!
//! `interpolate`'s three existing methods (`Idw`, `Kriging`, `Rbf`) share one
//! lattice-and-write pipeline because they all answer the same question -
//! *what value at this `(x, y)`* - on samples and queries that stay in world
//! coordinates throughout. MBA adds two things the shared
//! pipeline was never built for: a lattice that can be given
//! directly by two - possibly oblique - direction vectors instead of fitted
//! to the samples' bounding box, and `outer`/`inner` rings that filter the
//! samples before anything is fitted. Both are self-contained enough, and
//! specific enough to this one method, that duplicating a couple of the
//! shared helpers here (by reusing them, `pub(crate)`, not by copying their
//! text - see the integration notes) is a smaller, safer change than folding a
//! fifth kind of lattice determination into `interpolate.rs`'s shared `plan`.
//!
//! # The two extents
//!
//! - [`MbaExtent::Fitted`] is exactly what the other three methods already
//!   do: the samples' own bounding box, widened by `margin`, covered by
//!   cells of `spacing` - or, with `onto`, an existing grid's lattice reused
//!   as it is. Reached through [`lattice_for`]/[`lattice_onto`], made
//!   `pub(crate)` for this file rather than duplicated.
//! - [`MbaExtent::Given`] is the extent as the requester stated it: two
//!   direction vectors, with a scale for each, a count of i and j, and a
//!   starting point. The two directions need not be orthogonal - the
//!   lattice a `Point3dLatticeArray` can already state (RESQML TRG 2.2
//!   §3.17-3.18). MBA is fitted in the lattice's own
//!   `(u, v)` - **Provisional (open question: fitted in the lattice's
//!   frame or in the world frame)**: the integration proposal makes this
//!   reading (the second of two) over fitting in the world
//!   frame with a parallelogram `Ω`, because it needs no change to the
//!   B-spline core and makes `Ω` trivially rectangular whatever the angle
//!   between the two vectors; the alternative is a genuine open question
//!   that is left to the requester. `IDW`/kriging/RBF would
//!   not need this at all - only their **query** positions would be oblique,
//!   which changes nothing about how they are computed - so `Given` only
//!   does the `(u, v)` change of basis for `Mba`; the shared lattice
//!   machinery for the other methods is untouched here.
//!
//! # What is not written here, and why
//!
//! **`frame = meanPlane` is refused for this operation's grid target,
//! named** - a `Grid2dRepresentation`'s height is a
//! *vertical* `ZValueArray` over a lattice, and a value fitted in a tilted
//! local frame is offset **along the mean plane's normal**, which the
//! existing schema path cannot state without an explicit 3D points array -
//! left as an open question in the proposal itself (to be checked against
//! the schema before writing). Refusing rather than writing a grid that
//! quietly means something else is the conservative reading of the house
//! rule *refuse rather than draw* (ADR-0005). `estimate` and `resurface`
//! (`crates/energyml-build/src/ops/estimate.rs`,
//! `crates/energyml-build/src/ops/resurface.rs`) do not have this problem -
//! their targets are points and triangles, not a vertical height array - and
//! carry `meanPlane` in full. **Confirmed in review (open question settled):
//! this is a settled reading, not a question
//! left open for a later increment** - `interpolate`'s grid stays `world`.
//!
//! **`outer`/`inner` filter the samples and cut the grid** - a node outside
//! `outer` or inside an `inner` is written `NaN` (`mask_by_rings`;
//! rings used to filter the samples only, and the grid ran
//! past the ring). **And only for this one method** - the
//! design frames rings as a slot every MBA-serving operation should
//! offer identically, which would mean teaching `interpolate`'s
//! shared pipeline about them for `Idw`/`Kriging`/`Rbf` too. That is a real,
//! separate piece of work (rings are one of the
//! things the first design leaves as future increments) and is
//! scoped out here on purpose - `outer`/`inner` are read only by this file,
//! for `Method::Mba`; a run of `Idw`/`Kriging`/`Rbf` that names them leaves
//! them unread rather than refusing, which is honest about what is missing
//! but not yet the refusal a tidier version would give.
//!
//! TODO(reference): verify - the MBA method is a published algorithm (multilevel
//! B-spline approximation); name its paper here (the core in `energyml-analysis` cites it).
//!
//! # References
//!
//! - Lee, Wolberg, Shin, *"Scattered Data Interpolation with Multilevel B-Splines"*, IEEE Transactions on Visualization and Computer Graphics, vol. 3, n° 3, 1997, p. 228-244 (DOI 10.1109/2945.620490), as cited in `energyml-geostat` (`mba`).
//!   TODO(reference): verify - the citation above (journal, pages, DOI) was not re-checked against the paper (see `energyml-geostat/src/mba.rs`).

use energyml_domain::provenance::Parameter;

use crate::declare::{Params, Shape};
use crate::operation::{Context, Note, Plan, Refusal};
use crate::pieces::Lattice;

use energyml_analysis::mba::{Domain, Mba};

use super::interpolate::{
    InterpolateInputs, InterpolateParams, lattice_for, lattice_onto, queries, samples_of,
};

/// A grid nobody meant to ask for - the same ceiling `interpolate.rs`
/// already enforces (`MOST_NODES` there) for the `Fitted` extent, restated
/// here for `Given`, whose count comes from `count1 × count2` rather than a
/// fitted bounding box. Kept as its own constant rather than importing the
/// private one: the two must agree, but agreeing on a number is a smaller
/// coupling than agreeing on a visibility change neither number needs.
const MOST_NODES: usize = 10_000_000;

/// What `Method::Mba` is told, beyond the shared `spacing`/`margin`/`onto`.
#[derive(Debug, Params)]
pub struct MbaParams {
    /// advanced: the coarsest control grid's cell count along the first (fastest) axis - left out, the paper's own rule (§4.1: as coarse as possible, by the domain's aspect ratio), never the sample density
    #[param(advanced, min = 1)]
    pub coarsest_i: Option<usize>,
    /// advanced: the coarsest control grid's cell count along the second (slowest) axis
    #[param(advanced, min = 1)]
    pub coarsest_j: Option<usize>,
    /// the reference frame MBA is fitted in
    #[param(flatten)]
    pub frame: MbaFrame,
    /// the grid's extent
    #[param(flatten)]
    pub extent: MbaExtent,
}

/// The reference-frame parameter requested for the whole MBA family,
/// defaulting to `world`. **Settled (open question closed in review):
/// `interpolate`'s grid target keeps `world` only -
/// `meanPlane` is for `estimate` and `resurface`,** because a node
/// displaced along the mean plane's normal does not fit a
/// `Point3dZValueArray` (module doc). This is no longer a proposal to
/// revisit; `MeanPlane` stays declared here (rather than removed) only so a
/// caller who names it gets this file's own named refusal instead of an
/// unknown-case error from the parameter framework.
///
/// The enumeration is shaped so that a third case - a stated dip and azimuth
/// rather than a fitted mean plane - can be added later without breaking
/// either existing case (an open question, left for a later increment, not
/// implemented here).
#[derive(Debug, Params)]
#[params(tag = "frame")]
pub enum MbaFrame {
    /// Fitted in world (x, y) - the only case this operation writes.
    #[case(default, name = "world")]
    World,
    /// The mean plane's own tilted frame - refused here (module doc); carried
    /// in full by `estimate` and `resurface`.
    #[case(name = "meanPlane")]
    MeanPlane,
}

/// The lattice's extent - the existing `spacing`/`margin`/`onto` of
/// `interpolate.rs`, or a lattice given directly by two direction vectors.
#[derive(Debug, Params)]
#[params(tag = "extent")]
pub enum MbaExtent {
    /// The samples' own bounding box (`spacing`, `margin`), or `onto`'s - the
    /// case every other method already writes.
    #[case(default)]
    Fitted,
    /// Two direction vectors, their own step and count, and a starting point -
    /// as the requester stated them.
    Given(GivenExtent),
}

/// The seven numbers of a lattice given directly. Each
/// vector is two plain fields rather than `[f64; 2]`: the parameter framework
/// reads one [`Scalar`](crate::declare::Scalar) per field, and only the
/// primitives already implement it.
#[derive(Debug, Params)]
pub struct GivenExtent {
    // Named `start` - the requester's "starting point" - and not `origin`:
    // `interpolate` already records the origin it used as the *output*
    // `originX`, and one title cannot be both (tests/operations.rs).
    /// the lattice's starting point, x
    pub start_x: f64,
    /// the lattice's starting point, y
    pub start_y: f64,
    /// the first (fastest) axis's direction - need not be a unit vector, and need not be orthogonal to the second
    pub direction1_x: f64,
    /// the first axis's direction, y
    pub direction1_y: f64,
    /// the first axis's step, in the frame's units
    #[param(min = 0.0)]
    pub step1: f64,
    /// the first axis's node count
    #[param(min = 2)]
    pub count1: usize,
    /// the second (slowest) axis's direction
    pub direction2_x: f64,
    /// the second axis's direction, y
    pub direction2_y: f64,
    /// the second axis's step, in the frame's units
    #[param(min = 0.0)]
    pub step2: f64,
    /// the second axis's node count
    #[param(min = 2)]
    pub count2: usize,
}

/// `energyml_analysis::mba::Error`, turned into the slot it is about.
fn refuse_mba(slot: &str, e: energyml_analysis::mba::Error) -> Refusal {
    Refusal::on(slot, e.to_string())
}

/// The plan-view samples, filtered by `outer`/`inner` -
/// kept when inside `outer` (if given) and outside every `inner`.
/// A caller that names neither gets the samples back untouched, and never
/// pays for `nodes_in_ring`.
fn filter_by_rings(
    at: &[[f64; 2]],
    values: &[f64],
    outer: Option<&Shape>,
    inner: &[Shape],
) -> Result<(Vec<[f64; 2]>, Vec<f64>), Refusal> {
    if outer.is_none() && inner.is_empty() {
        return Ok((at.to_vec(), values.to_vec()));
    }
    let flat: Vec<f64> = at.iter().flat_map(|p| [p[0], p[1], 0.0]).collect();
    let mut keep: Option<Vec<u32>> = None;
    if let Some(shape) = outer {
        let ring = ring_of("outer", shape)?;
        let kept = energyml_geometry::clip::nodes_in_ring(&flat, &ring, false)
            .map_err(|why| Refusal::on("outer", why))?;
        keep = Some(kept);
    }
    for shape in inner {
        let ring = ring_of("inner", shape)?;
        let kept = energyml_geometry::clip::nodes_in_ring(&flat, &ring, true)
            .map_err(|why| Refusal::on("inner", why))?;
        keep = Some(match keep {
            Some(previous) => {
                let allowed: std::collections::HashSet<u32> = kept.into_iter().collect();
                previous.into_iter().filter(|at| allowed.contains(at)).collect()
            }
            None => kept,
        });
    }
    let keep = keep.unwrap_or_default();
    Ok((
        keep.iter().map(|&i| at[i as usize]).collect(),
        keep.iter().map(|&i| values[i as usize]).collect(),
    ))
}

/// The grid's own nodes cut by the same rings (ring usage 2): a
/// node outside `outer` or inside an `inner` becomes `NaN` - a hole, which
/// `grid::write` and `oblique_grid::write` already count and say. Reported
/// problem: an outer ring was given and the grid still overflowed it -
/// the rings filtered the samples only, so the rectangle still covered
/// everything past the ring. `nodes` are the nodes' world `(x, y)`, in the
/// order of `z`. Answers how many were cut.
fn mask_by_rings(
    nodes: &[[f64; 2]],
    z: &mut [f64],
    outer: Option<&Shape>,
    inner: &[Shape],
) -> Result<usize, Refusal> {
    if outer.is_none() && inner.is_empty() {
        return Ok(0);
    }
    let flat: Vec<f64> = nodes.iter().flat_map(|p| [p[0], p[1], 0.0]).collect();
    let mut cut = vec![false; nodes.len()];
    if let Some(shape) = outer {
        for at in energyml_geometry::clip::nodes_in_ring(&flat, &ring_of("outer", shape)?, true)
            .map_err(|why| Refusal::on("outer", why))?
        {
            cut[at as usize] = true;
        }
    }
    for shape in inner {
        for at in energyml_geometry::clip::nodes_in_ring(&flat, &ring_of("inner", shape)?, false)
            .map_err(|why| Refusal::on("inner", why))?
        {
            cut[at as usize] = true;
        }
    }
    let mut count = 0;
    for (value, cut) in z.iter_mut().zip(cut) {
        if cut && value.is_finite() {
            *value = f64::NAN;
            count += 1;
        }
    }
    Ok(count)
}

/// A ring's corners in plan, flat `[x, y, …]` - refused by `slot`'s name when
/// it cannot enclose anything.
fn ring_of(slot: &str, shape: &Shape) -> Result<Vec<f64>, Refusal> {
    let points: Vec<[f64; 3]> = shape
        .geometry
        .patches
        .iter()
        .flat_map(|p| p.points.iter().copied())
        .collect();
    if points.len() < 3 {
        return Err(Refusal::on(
            slot,
            format!(
                "`{}` holds {} point(s): a ring encloses something with at least 3",
                shape.name,
                points.len()
            ),
        ));
    }
    Ok(points.iter().flat_map(|p| [p[0], p[1]]).collect())
}

/// Drops the samples that fall outside `domain`, counted - never passed to
/// [`Mba::interpolate`], which refuses a call given one (module doc of
/// `energyml_analysis::mba`, *"MBA does not extrapolate…"*). Needed because
/// `lattice_for`'s bounding box, floor-rounded to a whole number of cells,
/// can undershoot the samples' own extent by nearly one cell - a sample the
/// existing methods would simply see at reduced weight near an edge, and
/// which this file instead has to keep out of the rectangle MBA is fitted
/// on, exactly like a ring would (`filter_by_rings`, above).
fn filter_to_domain(at: &[[f64; 2]], values: &[f64], domain: Domain) -> (Vec<[f64; 2]>, Vec<f64>, usize) {
    let mut kept_at = Vec::with_capacity(at.len());
    let mut kept_values = Vec::with_capacity(at.len());
    for (&p, &v) in at.iter().zip(values) {
        if domain.contains(p) {
            kept_at.push(p);
            kept_values.push(v);
        }
    }
    let dropped = at.len() - kept_at.len();
    (kept_at, kept_values, dropped)
}

/// The coarsest control grid - declared (`coarsest_i`/`coarsest_j`), or the
/// paper's own rule (§4.1, p. 233: *"as coarse as possible"*) applied to the
/// **domain's** aspect ratio, never the sample density: 1 on the shorter
/// axis, the rounded ratio on the longer.
fn coarsest_of(mba: &MbaParams, domain_size: [f64; 2]) -> [usize; 2] {
    let ratio = domain_size[0] / domain_size[1];
    let formula = if ratio.is_finite() && ratio >= 1.0 {
        [ratio.round().max(1.0) as usize, 1]
    } else if ratio.is_finite() && ratio > 0.0 {
        [1, (1.0 / ratio).round().max(1.0) as usize]
    } else {
        [1, 1]
    };
    [
        mba.coarsest_i.unwrap_or(formula[0]).max(1),
        mba.coarsest_j.unwrap_or(formula[1]).max(1),
    ]
}

/// The plan for `Method::Mba` - called from `interpolate.rs`'s `plan()` with
/// a two-line arm; see the module doc for why the rest lives here.
pub fn plan(
    inputs: &InterpolateInputs,
    params: &InterpolateParams,
    mba_settings: &MbaParams,
    cx: &Context<'_>,
) -> Result<Plan, Refusal> {
    if matches!(mba_settings.frame, MbaFrame::MeanPlane) {
        return Err(Refusal::about(
            "value",
            "frame",
            "`meanPlane` is not written for a grid target yet: a Grid2dRepresentation's height is a vertical ZValueArray over its lattice, and a result fitted in a tilted frame is offset along the mean plane's normal, not vertically. `estimate` and `resurface` carry `meanPlane` in full; a grid stays `world`",
        ));
    }
    if matches!(mba_settings.extent, MbaExtent::Given(_)) && inputs.onto.is_some() {
        return Err(Refusal::on(
            "onto",
            "names a grid to reuse, and `extent = given` names one directly: the two say the same thing twice",
        ));
    }

    let samples = samples_of(inputs);
    if samples.at.len() != samples.values.len() {
        return Err(Refusal::on(
            "field",
            format!(
                "`{}` holds {} node(s) and the field {} value(s) - a sample is a place and a number",
                samples.name,
                samples.at.len(),
                samples.values.len()
            ),
        ));
    }
    let (at, values) = filter_by_rings(&samples.at, &samples.values, inputs.outer.as_ref(), &inputs.inner)?;
    let ring_dropped = samples.at.len() - at.len();

    cx.stop_if_cancelled()?;

    let (mut plan, domain_size, levels_said) = match &mba_settings.extent {
        MbaExtent::Fitted => plan_fitted(inputs, params, mba_settings, &at, &values, cx)?,
        MbaExtent::Given(given) => plan_given(inputs, params, mba_settings, given, &at, &values, cx)?,
    };

    plan.activity.recipe = "interpolate";
    plan.activity.parameters.push(Parameter::Text {
        title: "method".to_string(),
        value: "mba".to_string(),
    });
    plan.activity.parameters.push(Parameter::Double {
        title: "sampleCount".to_string(),
        value: at.len() as f64,
        uom: None,
    });
    plan.activity.parameters.push(Parameter::Double {
        title: "coarsestI".to_string(),
        value: levels_said.0[0] as f64,
        uom: None,
    });
    plan.activity.parameters.push(Parameter::Double {
        title: "coarsestJ".to_string(),
        value: levels_said.0[1] as f64,
        uom: None,
    });
    plan.activity.parameters.push(Parameter::Double {
        title: "levels".to_string(),
        value: levels_said.1 as f64,
        uom: None,
    });
    plan.says.push(Note {
        key: "mba-levels",
        detail: format!(
            "MBA fitted on a coarsest control grid of {} by {} cell(s), refined to {} level(s), domain {} by {}",
            levels_said.0[0], levels_said.0[1], levels_said.1, domain_size[0], domain_size[1]
        ),
    });
    if ring_dropped > 0 {
        plan.says.push(Note {
            key: "ring-filtered",
            detail: format!("{ring_dropped} sample(s) removed by `outer`/`inner` before fitting"),
        });
    }
    Ok(plan)
}

/// What either extent plans: the plan, the size of the fitted domain, and the
/// coarsest lattice with the number of levels refined from it.
type Planned = (Plan, [f64; 2], ([usize; 2], usize));

/// `MbaExtent::Fitted`: the shared bounding-box/`onto` lattice, reused as it
/// is - `lattice_for`/`lattice_onto`, `pub(crate)` in `interpolate.rs`.
fn plan_fitted(
    inputs: &InterpolateInputs,
    params: &InterpolateParams,
    mba_settings: &MbaParams,
    at: &[[f64; 2]],
    values: &[f64],
    cx: &Context<'_>,
) -> Result<Planned, Refusal> {
    let (origin, steps, counts) = match &inputs.onto {
        Some(onto) => {
            let lattice = lattice_onto(onto, &inputs.shape, &params.written.frame)?;
            (lattice.origin, lattice.spacing, lattice.counts)
        }
        None => {
            let spacing = params.spacing.ok_or_else(|| {
                Refusal::about(
                    "required",
                    "spacing",
                    "`spacing` is required: the cell size of the grid - or give `onto`, or `extent = given`",
                )
            })?;
            let (origin, counts) = lattice_for(at, spacing, params.margin)?;
            (origin, [spacing, spacing], counts)
        }
    };
    let domain = Domain {
        origin,
        size: [
            steps[0] * (counts[0] - 1) as f64,
            steps[1] * (counts[1] - 1) as f64,
        ],
    };
    let (at, values, domain_dropped) = filter_to_domain(at, values, domain);
    let coarsest = coarsest_of(mba_settings, domain.size);
    let mba = Mba::new(coarsest).map_err(|e| refuse_mba("coarsest", e))?;
    let finest_spacing = steps[0].min(steps[1]);
    let nodes = queries(origin, steps, counts);
    let answer = mba
        .interpolate(&at, &values, domain, finest_spacing, &nodes)
        .map_err(|e| refuse_mba("shape", e))?;
    let mut z = answer.values;
    let ring_cut = mask_by_rings(&nodes, &mut z, inputs.outer.as_ref(), &inputs.inner)?;

    let name = format!("mba of {}", samples_name(inputs));
    let title = params.written.naming.title();
    let seed = match &inputs.onto {
        None => format!("{name}:{title}"),
        Some(onto) => format!("{name}:{title}:onto {}", onto_key(onto)),
    };
    let lattice = Lattice {
        origin,
        spacing: steps,
        counts,
        z,
    };
    let mut plan = super::grid::write(&name, &lattice, &params.written, &seed, cx)?;
    if let Some(onto) = &inputs.onto
        && let Some(reference) = onto.known.as_ref().and_then(|known| known.reference.as_ref())
    {
        plan.activity.parameters.push(Parameter::Object {
            title: "input".to_string(),
            reference: reference.dor(),
        });
    } else {
        plan.activity.parameters.push(Parameter::Double {
            title: "spacing".to_string(),
            value: steps[0],
            uom: None,
        });
        plan.activity.parameters.push(Parameter::Double {
            title: "margin".to_string(),
            value: params.margin,
            uom: None,
        });
    }
    if answer.set_aside > 0 {
        plan.says.push(Note {
            key: "set-aside",
            detail: format!(
                "{} sample(s) set aside: a value or a coordinate that is not a finite number",
                answer.set_aside
            ),
        });
    }
    if domain_dropped > 0 {
        plan.says.push(Note {
            key: "outside-fitted-domain",
            detail: format!("{domain_dropped} sample(s) fell outside the fitted lattice's own extent (rounded down to a whole number of cells) and were not fitted"),
        });
    }
    said_ring_cut(&mut plan, ring_cut);
    Ok((plan, domain.size, (coarsest, answer.levels)))
}

/// `MbaExtent::Given`: the two direction vectors, fitted in the lattice's own
/// `(u, v)` - **Provisional (open question: lattice frame)**, see the module doc.
fn plan_given(
    inputs: &InterpolateInputs,
    params: &InterpolateParams,
    mba_settings: &MbaParams,
    given: &GivenExtent,
    at: &[[f64; 2]],
    values: &[f64],
    cx: &Context<'_>,
) -> Result<Planned, Refusal> {
    let origin = [given.start_x, given.start_y];
    let dir1 = [given.direction1_x, given.direction1_y];
    let dir2 = [given.direction2_x, given.direction2_y];
    let n1 = (dir1[0] * dir1[0] + dir1[1] * dir1[1]).sqrt();
    let n2 = (dir2[0] * dir2[0] + dir2[1] * dir2[1]).sqrt();
    if !(n1.is_finite() && n1 > 1e-9) {
        return Err(Refusal::about(
            "value",
            "direction1",
            format!(
                "must be a non-zero, finite vector, not [{}, {}]",
                dir1[0], dir1[1]
            ),
        ));
    }
    if !(n2.is_finite() && n2 > 1e-9) {
        return Err(Refusal::about(
            "value",
            "direction2",
            format!(
                "must be a non-zero, finite vector, not [{}, {}]",
                dir2[0], dir2[1]
            ),
        ));
    }
    let dir1_hat = [dir1[0] / n1, dir1[1] / n1];
    let dir2_hat = [dir2[0] / n2, dir2[1] / n2];
    let det = dir1_hat[0] * dir2_hat[1] - dir1_hat[1] * dir2_hat[0];
    if det.abs() < 1e-9 {
        return Err(Refusal::about(
            "value",
            "direction2",
            "is collinear with `direction1`: two directions that do not span a plane cannot make a lattice",
        ));
    }
    if !(given.step1.is_finite() && given.step1 > 0.0) {
        return Err(Refusal::about(
            "value",
            "step1",
            format!("must be a positive length, not {}", given.step1),
        ));
    }
    if !(given.step2.is_finite() && given.step2 > 0.0) {
        return Err(Refusal::about(
            "value",
            "step2",
            format!("must be a positive length, not {}", given.step2),
        ));
    }
    let nodes = given.count1.saturating_mul(given.count2);
    if nodes > MOST_NODES {
        return Err(Refusal::about(
            "value",
            "count1",
            format!(
                "{} by {} - {nodes} nodes, past the {MOST_NODES} this writes: reduce `count1`/`count2`",
                given.count1, given.count2
            ),
        ));
    }
    cx.stop_if_cancelled()?;

    // The change of basis to the lattice's own (u, v) - MBA is
    // fitted where the grid says it is, never in a world-frame
    // parallelogram, which the core (`energyml_analysis::mba::Mba`) has no
    // notion of. `M⁻¹ (p - origin)`, `M = [dir1_hat | dir2_hat]`.
    let inv_det = 1.0 / det;
    let to_uv = |p: [f64; 2]| -> [f64; 2] {
        let d = [p[0] - origin[0], p[1] - origin[1]];
        [
            inv_det * (dir2_hat[1] * d[0] - dir2_hat[0] * d[1]),
            inv_det * (-dir1_hat[1] * d[0] + dir1_hat[0] * d[1]),
        ]
    };
    let samples_uv: Vec<[f64; 2]> = at.iter().map(|&p| to_uv(p)).collect();
    let domain_size = [
        given.step1 * (given.count1 - 1) as f64,
        given.step2 * (given.count2 - 1) as f64,
    ];
    let domain = Domain {
        origin: [0.0, 0.0],
        size: domain_size,
    };
    let queries_uv: Vec<[f64; 2]> = (0..given.count2)
        .flat_map(|j| (0..given.count1).map(move |i| [i as f64 * given.step1, j as f64 * given.step2]))
        .collect();

    let (samples_uv, values, domain_dropped) = filter_to_domain(&samples_uv, values, domain);
    let coarsest = coarsest_of(mba_settings, domain_size);
    let mba = Mba::new(coarsest).map_err(|e| refuse_mba("coarsest", e))?;
    let finest_spacing = given.step1.min(given.step2);
    let answer = mba
        .interpolate(&samples_uv, &values, domain, finest_spacing, &queries_uv)
        .map_err(|e| refuse_mba("shape", e))?;
    // The rings are in the world, the queries in (u, v): back to the world.
    let nodes: Vec<[f64; 2]> = queries_uv
        .iter()
        .map(|q| {
            [
                origin[0] + q[0] * dir1_hat[0] + q[1] * dir2_hat[0],
                origin[1] + q[0] * dir1_hat[1] + q[1] * dir2_hat[1],
            ]
        })
        .collect();
    let mut z = answer.values;
    let ring_cut = mask_by_rings(&nodes, &mut z, inputs.outer.as_ref(), &inputs.inner)?;

    // Whether the two world directions are orthogonal - computed, never
    // affirmed. **Provisional (open question)**: the business rule
    // (RESQML TRG 2.2 §3.17 p. 40) says to
    // omit `AllDimensionsAreOrthogonal` "if you don't know it"; a computed
    // dot product is never exactly zero for a caller's own numbers, so a
    // confident tolerance band decides `true`/`false` and a narrow band in
    // between omits the field - the two thresholds (1e-6, 1e-3) are this
    // file's own choice, not read off the business rule, which states the
    // *when to omit* but not *how close counts as unknown*.
    let dot = dir1_hat[0] * dir2_hat[0] + dir1_hat[1] * dir2_hat[1];
    let orthogonal = if dot.abs() < 1e-6 {
        Some(true)
    } else if dot.abs() > 1e-3 {
        Some(false)
    } else {
        None
    };

    let name = "mba (given lattice)".to_string();
    let title = params.written.naming.title();
    let seed = format!("{name}:{title}:{:?}:{:?}", dir1_hat, dir2_hat);
    let mut plan = super::oblique_grid::write(
        &name,
        origin,
        dir1_hat,
        given.step1,
        given.count1,
        dir2_hat,
        given.step2,
        given.count2,
        orthogonal,
        &z,
        &params.written,
        &seed,
        cx,
    )?;
    plan.activity.parameters.push(Parameter::Double {
        title: "step1".to_string(),
        value: given.step1,
        uom: None,
    });
    plan.activity.parameters.push(Parameter::Double {
        title: "step2".to_string(),
        value: given.step2,
        uom: None,
    });
    plan.activity.parameters.push(Parameter::Double {
        title: "count1".to_string(),
        value: given.count1 as f64,
        uom: None,
    });
    plan.activity.parameters.push(Parameter::Double {
        title: "count2".to_string(),
        value: given.count2 as f64,
        uom: None,
    });
    if answer.set_aside > 0 {
        plan.says.push(Note {
            key: "set-aside",
            detail: format!(
                "{} sample(s) set aside: a value or a coordinate that is not a finite number",
                answer.set_aside
            ),
        });
    }
    if domain_dropped > 0 {
        plan.says.push(Note {
            key: "outside-given-lattice",
            detail: format!(
                "{domain_dropped} sample(s) fell outside the given lattice's own window and did not take part"
            ),
        });
    }
    said_ring_cut(&mut plan, ring_cut);
    Ok((plan, domain_size, (coarsest, answer.levels)))
}

/// The nodes the rings cut, said - a hole nobody named reads as a fit that failed.
fn said_ring_cut(plan: &mut Plan, cut: usize) {
    if cut > 0 {
        plan.says.push(Note {
            key: "ring-cut",
            detail: format!("{cut} node(s) outside `outer` or inside an `inner` written as NaN"),
        });
    }
}

/// The title of the samples' shape, or its name - mirrors `interpolate.rs`'s
/// own `title_of`, kept local rather than reused: it is three lines, and the
/// existing one takes a `&Shape` this file does not always have handy.
fn samples_name(inputs: &InterpolateInputs) -> String {
    inputs
        .shape
        .known
        .as_ref()
        .and_then(|known| known.reference.as_ref())
        .map(|r| r.title.clone())
        .unwrap_or_else(|| inputs.shape.name.clone())
}

/// The given grid's identity key, for the seed - mirrors `interpolate.rs`'s
/// inline use at the same spot.
fn onto_key(onto: &Shape) -> String {
    onto.known
        .as_ref()
        .and_then(|known| known.reference.as_ref())
        .map(|r| r.uuid.clone())
        .unwrap_or_else(|| onto.name.clone())
}
