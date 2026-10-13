//! `interpolate` - a regular map from scattered samples, written as a 2D grid.
//!
//! A geological operation, and the request for
//! an `energyml compute` command is answered otherwise: `energyml create`
//! already makes every operation scriptable, form and activity template
//! included, so **rendering a computation scriptable is making it an
//! operation**. This is that for [`energyml_analysis`]'s three interpolators,
//! and it gives [`super::grid`] the producer of lattices it had nowhere but
//! the front end.
//!
//! # What is interpolated, and where the values come from
//!
//! The samples are the shape's nodes, read in plan view: `x` and `y` locate a
//! sample, and the value is the **third coordinate** - the ordinary case, a
//! horizon from a point cloud. When the bag also carries a `Source::Field`
//! over that shape, the field is the value instead and the third coordinate is
//! ignored: *interpolate this property* and *interpolate this elevation* are
//! the same computation over different numbers.
//!
//! # The grid is stated by its cell, not by six numbers
//!
//! `spacing` is the only geometry asked for. The extent is the samples' own
//! bounding box, widened by `margin` and covered by whole cells, which is the
//! answer somebody wants nine times out of ten - and the origin and the two
//! counts are **reported** in the activity, so what was written down is not a
//! number nobody stated. Naming an extent by hand is a slot to add when
//! somebody needs one, not a form to fill in every time.
//!
//! # Or it is given: `onto`
//!
//! Several horizons fitted each to its own seeds land on as many lattices, and
//! `ijk-grid-from-horizons` refuses two lattices by name: its pillars stand on
//! one. The optional input `onto` is an existing 2D grid whose lattice -
//! origin, steps, counts - is **reused as it is**: every horizon of a model
//! can be interpolated onto the ground's. `spacing` and `margin` are then the
//! other way of saying the same thing, and they say so (`Without(["onto"])`,
//! [`crate::declare::given_inputs`]) - a run given both is refused naming both.
//!
//! The lattice is read off the grid's nodes by the reader that
//! `ijk-grid-from-horizons` judges it with, `energyml_analysis::ijk::lattice_of`:
//! the node `(0, 0)` is the origin and the counts are the grid's, exactly; the
//! steps are the distance from that node to its two neighbours, which is the
//! written step whenever the sum lands on it (25 m on Lambert-93 does). Two
//! things are refused by name rather than bent:
//!
//! - **a grid in another frame than the samples** - both frames are named. A
//!   grid is never reprojected (a grid keeps the frame it was written in), and a lattice read in one frame and filled with
//!   heights read in another would place every node somewhere else. The frame
//!   written must be the grid's as well: a `crs` naming another, or one created
//!   with `sourceEpsg`, is refused;
//! - **a lattice turned or reversed in its frame's numbers** - the grid writer
//!   writes one along +x then +y; querying a turned lattice and writing it
//!   straight would move every node without a word.
//!
//! What the samples cannot reach on it stays a hole, counted, as on a lattice
//! of their own. The grid's uuid then hangs from the given grid as well: the
//! same samples on another lattice are another grid, and a run without `onto`
//! names its objects as it always has.
//!
//! # Faults as barriers
//!
//! The need: at the BRGM, kriging is used with possible fault lines
//! and other meshing parameters. A fault here is a
//! **barrier**: a sample on the far side of it says nothing about this side,
//! for IDW and kriging alike ([`energyml_analysis::Barriers`]). It is the
//! optional, multiple `faults` input, read by the name of its slot; and it
//! turns kriging from one global system into one small system per query,
//! which is also what gives kriging the search `radius` and `neighbours` it
//! did not have. Radial basis functions are **global** - every sample enters
//! every estimate by construction - so `rbf` with a fault is refused by name
//! rather than quietly ignoring the trace.
//!
//! # What is refused, and what is merely `NaN`
//!
//! A refusal names both ends: too few samples for the kernel, a spacing that
//! is not positive, a grid smaller than two nodes on an axis, a grid so large
//! that it was surely a typo. What is *not* refused is a node no sample could
//! reach - IDW with a radius leaves those `NaN`, and [`super::grid`] writes
//! them as holes and counts them.
//!
//! # Typed, and the method a union
//!
//! Step 6 of the operations architecture: the settings are
//! [`InterpolateParams`], the method a union whose cases carry their own -
//! and the kriging model a union inside it. The flat list a form renders is
//! generated from it, each setting acting only under its case; before, an IDW
//! run recorded `kernel: thin-plate` in its activity.
//!
//! # References
//!
//! - Shepard's method (inverse distance weighting), as named in `energyml-geostat` (`idw`).
//!   TODO(reference): verify - Shepard's method is named without a full citation; add the original reference (authors, year, title, venue, DOI) once checked.
//! - Ordinary kriging and the variogram models of `energyml-geostat` (`kriging`, `variogram`).
//!   TODO(reference): verify - no publication is cited for ordinary kriging or the variogram models; add a reference once checked.
//! - Radial basis function interpolation of `energyml-geostat` (`rbf`).
//!   TODO(reference): verify - no publication is cited for radial basis function interpolation with conditionally positive definite kernels; add a reference once checked.
//! - Lee, Wolberg, Shin, *"Scattered Data Interpolation with Multilevel B-Splines"*, IEEE Transactions on Visualization and Computer Graphics, vol. 3, n° 3, 1997, p. 228-244 (DOI 10.1109/2945.620490), as cited in `energyml-geostat` (`mba`).
//!   TODO(reference): verify - the citation above (journal, pages, DOI) was not re-checked against the paper (see `energyml-geostat/src/mba.rs`).

use energyml_analysis::ijk::{self, LatticeError};
use energyml_analysis::{
    Barriers, Idw as IdwAnalysis, Kernel as KernelAnalysis, Model as ModelAnalysis, ModelKind, Ordinary,
    Rbf as RbfAnalysis, empirical,
};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::Topology;

use crate::declare::{Field, Inputs, Params, Shape, Suggestions, known_named};
use crate::operation::{About, Category, Context, Error, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::{Lattice, Pieces};

use super::support;

/// The operation.
pub struct Interpolate;

/// A grid nobody meant to ask for. Ten million nodes is eighty megabytes of
/// doubles, and every method here is at least linear in the node count.
const MOST_NODES: usize = 10_000_000;

// ---------------------------------------------------------------- the declaration

/// What `interpolate` is told.
///
/// **The method is a union** (operations architecture):
/// each interpolator carries its own settings, and the kriging model a
/// second union inside it. On the wire it is the flat list a form already
/// renders, each setting acting only under its case; in `plan` it is a Rust
/// `enum`, so the IDW case has no `kernel` to read - and nothing to record.
#[derive(Debug, Params)]
pub struct InterpolateParams {
    /// the cell size of the grid, in the units of the frame - unless `onto` gives the lattice
    ///
    /// **Required when it acts** (`least = 1`) and acting only without `onto`:
    /// an `Option` because a run given `onto` has none, and `plan` refuses its
    /// absence by name when neither is there.
    #[param(min = 0.0, least = 1, without = ["onto"])]
    pub spacing: Option<f64>,
    /// how far past the samples the grid reaches, in the same units - unless `onto` gives the lattice
    #[param(default = 0.0, optional, min = 0.0, without = ["onto"])]
    pub margin: f64,
    /// The interpolator, and its own settings.
    #[param(flatten)]
    pub method: Method,
    /// The frame, what is represented, the naming - for the grid writer.
    #[param(flatten)]
    pub written: support::Written,
}

/// which interpolator
#[derive(Debug, Params)]
#[params(tag = "method")]
pub enum Method {
    /// Inverse distance.
    #[case(default)]
    Idw(IdwParams),
    /// Ordinary kriging.
    Kriging(KrigingParams),
    /// Radial basis functions.
    Rbf(RbfParams),
    /// Multilevel B-spline Approximation - see `super::interpolate_mba`.
    Mba(super::interpolate_mba::MbaParams),
}

/// Inverse distance.
#[derive(Debug, Params)]
pub struct IdwParams {
    /// inverse distance: the exponent - larger is more local
    #[param(default = 2.0, optional, min = 0.0)]
    pub power: f64,
    /// inverse distance and kriging: ignore samples farther than this; a node out of reach of every sample is left empty
    ///
    /// **Advanced**, as the neighbourhood cap below: a first map is drawn
    /// without either, and a form that shows them first reads as a form that
    /// needs them.
    #[param(min = 0.0, advanced)]
    pub radius: Option<f64>,
    /// inverse distance and kriging: use only this many nearest samples
    #[param(min = 1, advanced)]
    pub neighbours: Option<usize>,
}

/// Ordinary kriging.
#[derive(Debug, Params)]
pub struct KrigingParams {
    /// inverse distance and kriging: ignore samples farther than this; a node out of reach of every sample is left empty
    #[param(min = 0.0, advanced)]
    pub radius: Option<f64>,
    /// inverse distance and kriging: use only this many nearest samples
    #[param(min = 1, advanced)]
    pub neighbours: Option<usize>,
    /// The variogram model, fitted or stated.
    #[param(flatten)]
    pub model: VariogramModel,
}

/// kriging: the variogram model; `fitted` fits one to the samples
#[derive(Debug, Params)]
#[params(tag = "model")]
pub enum VariogramModel {
    /// Fitted to the samples' own variogram, and said.
    #[case(default)]
    Fitted,
    /// A stated spherical model.
    Spherical(StatedModel),
    /// A stated exponential model.
    Exponential(StatedModel),
}

/// The three numbers of a stated variogram model.
#[derive(Debug, Params)]
pub struct StatedModel {
    /// kriging: with a stated model
    #[param(min = 0.0)]
    pub nugget: f64,
    /// kriging: with a stated model
    #[param(min = 0.0)]
    pub sill: f64,
    /// kriging: with a stated model
    #[param(min = 0.0)]
    pub range: f64,
}

/// Radial basis functions.
#[derive(Debug, Params)]
pub struct RbfParams {
    /// The kernel, and its shape when it has one.
    #[param(flatten)]
    pub kernel: RadialKernel,
}

/// radial basis: which kernel
#[derive(Debug, Params)]
#[params(tag = "kernel")]
pub enum RadialKernel {
    /// Thin plate.
    #[case(default, name = "thin-plate")]
    ThinPlate,
    /// Multiquadric, with its shape.
    Multiquadric(Epsilon),
    /// Linear.
    Linear,
    /// Gaussian, with its shape.
    Gaussian(Epsilon),
}

/// The shape parameter of a kernel.
#[derive(Debug, Params)]
pub struct Epsilon {
    /// radial basis: the shape parameter of the multiquadric and gaussian kernels
    #[param(min = 0.0)]
    pub epsilon: f64,
}

/// What `interpolate` takes.
#[derive(Debug, Inputs)]
pub struct InterpolateInputs {
    /// the samples: a shape whose nodes carry the values
    pub shape: Shape,
    /// the values, instead of the nodes' elevation
    pub field: Option<Field>,
    /// fault traces the interpolation must not see across - a sample behind one takes no part
    #[input(topology = "polylines")]
    pub faults: Vec<Shape>,
    /// an existing 2D grid whose lattice - origin, steps, counts - the map is computed on, instead of one fitted to the samples; in the samples' frame
    #[input(topology = "grid")]
    pub onto: Option<Shape>,
    /// `method = mba` only: keeps only the samples inside this ring
    #[input(topology = "polylines")]
    pub outer: Option<Shape>,
    /// `method = mba` only: removes the samples inside each of these rings
    #[input(topology = "polylines")]
    pub inner: Vec<Shape>,
}

/// The title of the object behind a shape, or the shape's name.
fn title_of(shape: &Shape) -> String {
    shape
        .known
        .as_ref()
        .and_then(|known| known.reference.as_ref())
        .map(|r| r.title.clone())
        .unwrap_or_else(|| shape.name.clone())
}

/// The counts of the grid `onto` names - `[ni, nj]` - or why it is none.
fn grid_counts(onto: &Shape) -> Result<[usize; 2], Refusal> {
    let [patch] = onto.geometry.patches.as_slice() else {
        return Err(Refusal::on(
            "onto",
            format!(
                "`{}` holds {} patch(es): the lattice is taken from one 2D grid",
                title_of(onto),
                onto.geometry.patches.len()
            ),
        ));
    };
    let Topology::Grid { fastest, slowest } = &patch.topology else {
        return Err(Refusal::on(
            "onto",
            format!(
                "`{}` is no 2D grid: only a Grid2dRepresentation's lattice can be reused",
                title_of(onto)
            ),
        ));
    };
    Ok([*fastest, *slowest])
}

/// The lattice of the grid `onto` names, **as it is** - or the refusal that
/// names what makes it one this operation cannot fill.
///
/// Its frame is checked against the samples' and against the frame written:
/// see the module, *Or it is given*.
pub(crate) fn lattice_onto(
    onto: &Shape,
    samples: &Shape,
    frame: &super::written::FrameParams,
) -> Result<ijk::Lattice, Refusal> {
    let counts = grid_counts(onto)?;
    let lattice = ijk::lattice_of(&onto.geometry.patches[0].points, counts).map_err(|why| {
        let title = title_of(onto);
        Refusal::on(
            "onto",
            match why {
                LatticeError::Turned { along_i, along_j } => format!(
                    "`{title}` runs along ({}, {}) then ({}, {}) in its frame's numbers: a turned or reversed lattice is not filled here - the grid is written along +x then +y, and querying a turned one to write it straight would move every node",
                    along_i[0], along_i[1], along_j[0], along_j[1]
                ),
                other => format!("`{title}` {other}"),
            },
        )
    })?;
    let frame_of = |shape: &Shape| shape.known.as_ref().and_then(|known| known.crs.clone());
    let Some(grid_frame) = frame_of(onto) else {
        return Ok(lattice);
    };
    if let Some(seeds_frame) = frame_of(samples)
        && !seeds_frame.uuid.eq_ignore_ascii_case(&grid_frame.uuid)
    {
        return Err(Refusal::on(
            "onto",
            format!(
                "`{}` is in `{}` and the samples `{}` in `{}`: a grid is never reprojected - interpolate onto a grid of the samples' frame, or bring the samples into `{}` first",
                title_of(onto),
                grid_frame.title,
                title_of(samples),
                seeds_frame.title,
                grid_frame.title
            ),
        ));
    }
    match (&frame.crs, frame.source_epsg) {
        (Some(named), _) => {
            let uuid = named
                .0
                .get("Uuid")
                .and_then(|v| v.as_str())
                .or_else(|| named.0.as_str())
                .unwrap_or_default();
            if !uuid.eq_ignore_ascii_case(&grid_frame.uuid) {
                return Err(Refusal::on(
                    "crs",
                    format!(
                        "names another frame than `{}`'s `{}`: the lattice is written as it is, so in its own frame - name that one, or leave `crs` out to take it",
                        title_of(onto),
                        grid_frame.title
                    ),
                ));
            }
        }
        (None, Some(code)) => {
            return Err(Refusal::on(
                "sourceEpsg",
                format!(
                    "would create a frame from EPSG:{}, and the lattice of `{}` is in `{}`: name that one as `crs`, or leave both out to take it",
                    code.0,
                    title_of(onto),
                    grid_frame.title
                ),
            ));
        }
        (None, None) => {}
    }
    Ok(lattice)
}

/// The bag without the grid `onto` and what is known of it - what the frame
/// and the naming are suggested from. The given grid lends its lattice and
/// nothing else: its interpretation or its title on the new map is the error a
/// suggestion would make look deliberate.
fn without_onto(bag: &Pieces) -> Pieces {
    bag.iter()
        .filter(|(name, _)| *name != "onto" && *name != "onto (object)")
        .fold(Pieces::empty(), |kept, (name, source)| {
            kept.with(name, source.clone())
        })
}

// ---------------------------------------------------------------- the computation

/// The samples: where they are in plan view, what they are worth, and what
/// the caller called them.
pub(crate) struct Samples {
    pub(crate) name: String,
    pub(crate) at: Vec<[f64; 2]>,
    pub(crate) values: Vec<f64>,
    /// Whether the values came from a field rather than the third coordinate.
    pub(crate) from_field: bool,
}

/// The samples of the bound inputs: the shape's nodes, valued by the field
/// laid over it when there is one, by their elevation otherwise.
pub(crate) fn samples_of(inputs: &InterpolateInputs) -> Samples {
    let points: Vec<[f64; 3]> = inputs
        .shape
        .geometry
        .patches
        .iter()
        .flat_map(|patch| patch.points.iter().copied())
        .collect();
    let field = inputs.field.as_ref().map(|field| field.values.clone());
    let from_field = field.is_some();
    let values = field.unwrap_or_else(|| points.iter().map(|p| p[2]).collect());
    Samples {
        name: inputs.shape.name.clone(),
        at: points.iter().map(|p| [p[0], p[1]]).collect(),
        values,
        from_field,
    }
}

/// The lattice the samples ask for: their bounding box, widened and covered
/// by whole cells of `spacing`.
pub(crate) fn lattice_for(
    at: &[[f64; 2]],
    spacing: f64,
    margin: f64,
) -> Result<([f64; 2], [usize; 2]), Error> {
    if !(spacing.is_finite() && spacing > 0.0) {
        return Err(Refusal::about(
            "value",
            "spacing",
            format!("`spacing` must be a positive length - {spacing} given"),
        )
        .into());
    }
    let finite: Vec<&[f64; 2]> = at.iter().filter(|p| p.iter().all(|c| c.is_finite())).collect();
    if finite.is_empty() {
        return Err(Error::new(
            "no sample has a finite position: there is no extent to grid".to_string(),
        ));
    }
    let fold = |pick: fn(f64, f64) -> f64, axis: usize| finite.iter().map(|p| p[axis]).fold(f64::NAN, pick);
    let (x0, x1) = (fold(f64::min, 0) - margin, fold(f64::max, 0) + margin);
    let (y0, y1) = (fold(f64::min, 1) - margin, fold(f64::max, 1) + margin);
    let count = |low: f64, high: f64| ((high - low) / spacing).floor() as i64 + 1;
    let (ni, nj) = (count(x0, x1), count(y0, y1));
    if ni < 2 || nj < 2 {
        return Err(Refusal::about(
            "value",
            "spacing",
            format!(
                "a spacing of {spacing} over an extent of {:.6} by {:.6} makes a grid of {ni} by {nj}: a grid needs at least 2 nodes on each axis",
                x1 - x0,
                y1 - y0
            ),
        )
        .into());
    }
    let nodes = (ni as usize).saturating_mul(nj as usize);
    if nodes > MOST_NODES {
        return Err(Refusal::about(
            "value",
            "spacing",
            format!("a spacing of {spacing} makes {ni} by {nj} - {nodes} nodes, past the {MOST_NODES} this writes: widen the spacing"),
        )
        .into());
    }
    Ok(([x0, y0], [ni as usize, nj as usize]))
}

/// The queries, in the lattice's own order: `i` along x varies fastest.
pub(crate) fn queries(origin: [f64; 2], steps: [f64; 2], counts: [usize; 2]) -> Vec<[f64; 2]> {
    let [ni, nj] = counts;
    (0..nj)
        .flat_map(|j| {
            (0..ni).map(move |i| [origin[0] + i as f64 * steps[0], origin[1] + j as f64 * steps[1]])
        })
        .collect()
}

/// What a run answered, beside its numbers.
struct Ran {
    values: Vec<f64>,
    set_aside: usize,
    unreached: usize,
    /// What the activity records of the method, beyond its name.
    parameters: Vec<Parameter>,
    says: Vec<Note>,
}

fn double(title: &str, value: f64) -> Parameter {
    Parameter::Double {
        title: title.to_string(),
        value,
        uom: None,
    }
}

/// The run - **one arm per case of the union**, each reading only its own
/// settings, because those are the only ones its case has.
fn run(
    method: &Method,
    samples: &Samples,
    queries: &[[f64; 2]],
    faults: Option<Barriers>,
) -> Result<Ran, Error> {
    let refuse = |e: energyml_analysis::Error| Error::new(e.to_string());
    let mut parameters = Vec::new();
    if let Some(faults) = &faults {
        parameters.push(double("faultSegments", faults.len() as f64));
    }
    match method {
        Method::Idw(settings) => {
            let mut idw = IdwAnalysis::new(settings.power);
            if let Some(faults) = faults {
                idw = idw.with_barriers(faults);
            }
            if let Some(radius) = settings.radius {
                idw = idw.with_radius(radius);
                parameters.push(double("radius", radius));
            }
            if let Some(k) = settings.neighbours {
                idw = idw.with_neighbours(k);
                parameters.push(double("neighbours", k as f64));
            }
            parameters.push(double("power", settings.power));
            let answer = idw
                .interpolate(&samples.at, &samples.values, queries)
                .map_err(refuse)?;
            Ok(Ran {
                values: answer.values,
                set_aside: answer.set_aside,
                unreached: answer.unreached,
                parameters,
                says: Vec::new(),
            })
        }
        Method::Kriging(settings) => {
            let mut says = Vec::new();
            let (named, model) = match &settings.model {
                VariogramModel::Fitted => {
                    // Fitted to the samples' own variogram, and said: a model
                    // nobody stated is a decision, not a default.
                    let observed = empirical(&samples.at, &samples.values, None, 12).map_err(refuse)?;
                    let fitted = ModelAnalysis::fit(&observed, ModelKind::Spherical).map_err(refuse)?;
                    says.push(Note {
                        key: "fitted",
                        detail: format!(
                            "no model was stated, so a spherical one was fitted to the samples' own variogram: nugget {:.6}, sill {:.6}, range {:.6}",
                            fitted.nugget, fitted.sill, fitted.range
                        ),
                    });
                    ("fitted", fitted)
                }
                VariogramModel::Spherical(stated) => (
                    "spherical",
                    ModelAnalysis::new(ModelKind::Spherical, stated.nugget, stated.sill, stated.range)
                        .map_err(refuse)?,
                ),
                VariogramModel::Exponential(stated) => (
                    "exponential",
                    ModelAnalysis::new(ModelKind::Exponential, stated.nugget, stated.sill, stated.range)
                        .map_err(refuse)?,
                ),
            };
            parameters.push(Parameter::Text {
                title: "model".to_string(),
                value: named.to_string(),
            });
            for (title, value) in [
                ("nugget", model.nugget),
                ("sill", model.sill),
                ("range", model.range),
            ] {
                parameters.push(double(title, value));
            }
            // Only the estimates: the grid writes no variance, and asking for
            // one costs a solve per node (`Ordinary::without_variance`).
            let mut kriging = Ordinary::new(model).without_variance();
            // The same neighbourhood words as IDW's, and they mean the same:
            // a radius, a cap - which is what turns the one global system into
            // one per query, the BRGM's kind.
            if let Some(radius) = settings.radius {
                kriging = kriging.with_radius(radius);
                parameters.push(double("radius", radius));
            }
            if let Some(k) = settings.neighbours {
                kriging = kriging.with_neighbours(k);
                parameters.push(double("neighbours", k as f64));
            }
            if let Some(faults) = faults {
                kriging = kriging.with_barriers(faults);
            }
            let answer = kriging
                .interpolate(&samples.at, &samples.values, queries)
                .map_err(refuse)?;
            Ok(Ran {
                values: answer.values,
                set_aside: answer.set_aside,
                unreached: answer.unreached,
                parameters,
                says,
            })
        }
        Method::Rbf(settings) => {
            if faults.is_some() {
                // Global by construction: every sample enters every estimate
                // through the kernel matrix, so there is no neighbourhood to
                // cut. Refused by name rather than ignoring the trace, which
                // would draw a map that looks faulted and is not.
                return Err(Refusal::about(
                    "value",
                    "method",
                    "`rbf` is a global interpolator and cannot honour a fault as a barrier: choose `idw` or `kriging`, or leave `faults` out",
                )
                .into());
            }
            let (named, kernel) = match &settings.kernel {
                RadialKernel::ThinPlate => ("thin-plate", KernelAnalysis::ThinPlate),
                RadialKernel::Linear => ("linear", KernelAnalysis::Linear),
                RadialKernel::Multiquadric(shape) => {
                    parameters.push(double("epsilon", shape.epsilon));
                    (
                        "multiquadric",
                        KernelAnalysis::Multiquadric {
                            epsilon: shape.epsilon,
                        },
                    )
                }
                RadialKernel::Gaussian(shape) => {
                    parameters.push(double("epsilon", shape.epsilon));
                    (
                        "gaussian",
                        KernelAnalysis::Gaussian {
                            epsilon: shape.epsilon,
                        },
                    )
                }
            };
            parameters.push(Parameter::Text {
                title: "kernel".to_string(),
                value: named.to_string(),
            });
            let answer = RbfAnalysis::new(kernel)
                .interpolate(&samples.at, &samples.values, queries)
                .map_err(refuse)?;
            let unreached = answer.values.iter().filter(|v| !v.is_finite()).count();
            Ok(Ran {
                values: answer.values,
                set_aside: answer.set_aside,
                unreached,
                parameters,
                says: Vec::new(),
            })
        }
        // `Method::Mba` is planned by `super::interpolate_mba::plan` before
        // `run` is ever called - see the arm added to `Operation::plan`
        // below. This arm only keeps the match exhaustive.
        Method::Mba(_) => Err(Error::new(
            "`mba` is planned by `interpolate_mba::plan`, never `run`".to_string(),
        )),
    }
}

/// The name of the method, as the activity records it.
fn method_word(method: &Method) -> &'static str {
    match method {
        Method::Idw(_) => "idw",
        Method::Kriging(_) => "kriging",
        Method::Rbf(_) => "rbf",
        Method::Mba(_) => "mba",
    }
}

impl Operation for Interpolate {
    type Params = InterpolateParams;
    type Inputs = InterpolateInputs;

    fn about(&self) -> About {
        About {
            name: "interpolate",
            describe: "a regular map from scattered samples - inverse distance, kriging, radial basis or multilevel B-spline (MBA) - written as a 2D grid",
            category: Category::Geometry,
            makes: &["resqml22.Grid2dRepresentation"],
            recipe: "interpolate",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(&without_onto(bag), None, slots);
        // The frame the lattice is in, when the samples name none: the map is
        // written in it or refused (`lattice_onto`), so it is the one answer.
        if let Some(frame) = known_named(bag, "onto").and_then(|known| known.crs.as_ref())
            && slots.slot_mut("crs").is_some_and(|slot| slot.suggested.is_none())
        {
            slots.inferred("crs", frame.dor(), frame.clone());
        }
    }

    fn check(&self, inputs: &InterpolateInputs) -> Result<(), Refusal> {
        let nodes: usize = inputs
            .shape
            .geometry
            .patches
            .iter()
            .map(|patch| patch.points.len())
            .sum();
        if nodes < 3 {
            return Err(Refusal::on(
                "shape",
                format!("an interpolation needs at least 3 samples; the shape holds {nodes}"),
            ));
        }
        if let Some(onto) = &inputs.onto {
            grid_counts(onto)?;
        }
        // A field over another piece is not the values of these samples: said,
        // rather than quietly interpolating the elevation instead.
        if let Some(field) = &inputs.field
            && field.over != inputs.shape.name
            && !field.over.starts_with(&format!("{} ", inputs.shape.name))
        {
            return Err(Refusal::on(
                "field",
                format!(
                    "lies over `{}`, and the samples are `{}`",
                    field.over, inputs.shape.name
                ),
            ));
        }
        Ok(())
    }

    fn plan(
        &self,
        inputs: InterpolateInputs,
        params: InterpolateParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        if let Method::Mba(settings) = &params.method {
            return super::interpolate_mba::plan(&inputs, &params, settings, cx);
        }
        let samples = samples_of(&inputs);
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
        // The lattice: the given grid's as it is, or one fitted to the samples.
        let (origin, steps, counts) = match &inputs.onto {
            Some(onto) => {
                let lattice = lattice_onto(onto, &inputs.shape, &params.written.frame)?;
                (lattice.origin, lattice.spacing, lattice.counts)
            }
            None => {
                let spacing = params.spacing.ok_or_else(|| {
                    Refusal::about("required", "spacing", "`spacing` is required: the cell size of the grid - or give `onto`, a grid whose lattice to reuse")
                })?;
                let (origin, counts) = lattice_for(&samples.at, spacing, params.margin)?;
                (origin, [spacing, spacing], counts)
            }
        };
        // The traces in plan view: a barrier is a rule on (x, y), and a
        // trace's z, if it has one, is not what cuts a neighbourhood.
        // Only the traces **at this horizon's depth**: an object holding one
        // trace per horizon must not wall this horizon off with the others'.
        let horizon_z = inputs
            .shape
            .geometry
            .patches
            .iter()
            .flat_map(|p| p.points.iter().map(|q| q[2]));
        let traces: super::triangulate::Traces = inputs
            .faults
            .iter()
            .flat_map(|fault| super::triangulate::traces_of(&fault.geometry))
            .collect();
        let (kept, other_horizons) =
            super::triangulate::faults_on(traces, horizon_z).map_err(|why| Refusal::on("faults", why))?;
        let plan_view: Vec<Vec<[f64; 2]>> = kept
            .into_iter()
            .map(|line| line.into_iter().map(|p| [p[0], p[1]]).collect())
            .collect();
        let faults = if plan_view.is_empty() {
            None
        } else {
            Some(Barriers::from_polylines(&plan_view).map_err(|e| Refusal::on("faults", e))?)
        };
        cx.stop_if_cancelled()?;
        let mut ran = run(&params.method, &samples, &queries(origin, steps, counts), faults)?;
        cx.stop_if_cancelled()?;
        if other_horizons > 0 {
            ran.says.push(Note {
                key: "faults-elsewhere",
                detail: format!("{other_horizons} fault trace(s) lie at another horizon's depth and were set aside - only the traces at this horizon's depth act as barriers"),
            });
        }

        // The lattice goes to the grid writer as it is, in the frame the
        // samples were read in - which is why `grid` refuses to place one.
        // **Typed in**: no bag to derive, no choices to read back by name. The
        // seed is the one the derived bag's first piece gave, so the objects
        // are named as they always were.
        let method = method_word(&params.method);
        let name = format!("{method} of {}", samples.name);
        let lattice = Lattice {
            origin,
            spacing: steps,
            counts,
            z: ran.values,
        };
        let title = params.written.naming.title();
        // The given grid enters the seed - the same samples on another lattice
        // are another grid - and a run without it keeps the seed it had.
        let onto_key = inputs.onto.as_ref().map(|onto| {
            onto.known
                .as_ref()
                .and_then(|known| known.reference.as_ref())
                .map(|r| r.uuid.clone())
                .unwrap_or_else(|| onto.name.clone())
        });
        let seed = match &onto_key {
            None => format!("{name}:{title}"),
            Some(key) => format!("{name}:{title}:onto {key}"),
        };
        let mut plan = super::grid::write(&name, &lattice, &params.written, &seed, cx)?;

        plan.activity.recipe = "interpolate";
        plan.activity.title = format!("interpolate → {title}");
        plan.activity.parameters.push(Parameter::Text {
            title: "method".to_string(),
            value: method.to_string(),
        });
        match &inputs.onto {
            // What the run was told: the grid whose lattice it reused, as an
            // input - `spacing` and `margin` were not asked, so not recorded.
            Some(onto) => {
                if let Some(reference) = onto.known.as_ref().and_then(|known| known.reference.as_ref()) {
                    plan.activity.parameters.push(Parameter::Object {
                        title: "input".to_string(),
                        reference: reference.dor(),
                    });
                }
                plan.says.push(Note {
                    key: "onto",
                    detail: format!(
                        "on the lattice of `{}`, as it is: {} by {} nodes from ({}, {}) every ({}, {})",
                        title_of(onto),
                        counts[0],
                        counts[1],
                        origin[0],
                        origin[1],
                        steps[0],
                        steps[1]
                    ),
                });
            }
            None => {
                plan.activity.parameters.push(double("spacing", steps[0]));
                plan.activity.parameters.push(double("margin", params.margin));
            }
        }
        plan.activity.parameters.push(double("originX", origin[0]));
        plan.activity.parameters.push(double("originY", origin[1]));
        plan.activity
            .parameters
            .push(double("sampleCount", samples.at.len() as f64));
        plan.activity.parameters.extend(ran.parameters);
        plan.says.extend(ran.says);
        plan.says.push(Note {
            key: "samples",
            detail: format!(
                "{} sample(s) {}, on a grid of {} by {} at a spacing of {}",
                samples.at.len(),
                if samples.from_field {
                    "valued by the field"
                } else {
                    "valued by their elevation"
                },
                counts[0],
                counts[1],
                if steps[0] == steps[1] {
                    steps[0].to_string()
                } else {
                    format!("{} by {}", steps[0], steps[1])
                }
            ),
        });
        if ran.set_aside > 0 {
            plan.says.push(Note {
                key: "set-aside",
                detail: format!(
                    "{} sample(s) set aside: a value or a coordinate that is not a finite number",
                    ran.set_aside
                ),
            });
        }
        if ran.unreached > 0 {
            plan.says.push(Note {
                key: "unreached",
                detail: format!(
                    "{} node(s) out of reach of every sample: written as holes rather than extrapolated",
                    ran.unreached
                ),
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
            count("nodeCount", "nodes written"),
            count("fastestAxisCount", "nodes along x"),
            count("slowestAxisCount", "nodes along y"),
            count("originX", "the x of the node (0, 0)"),
            count("originY", "the y of the node (0, 0)"),
            count("sampleCount", "samples read"),
        ])
    }
}
