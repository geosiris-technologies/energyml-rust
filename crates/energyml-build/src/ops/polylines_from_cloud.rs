//! `polylines-from-cloud` — fake, dated test data: a vertical polyline
//! descending from each point of a point cloud, one wellhead-like line per
//! point.
//!
//! From the original need:
//! from a point cloud, create many vertical polylines,
//! with either a fixed number of cuts or a random number
//! between 2 bounds, either a precise z or a z between 2 bounds (to
//! start with, begin from each point as a wellhead), and cuts
//! either regular or with fluctuations.
//!
//! # What is decided here
//!
//! - **a length, not an absolute z** — the original request reads *"the z of the
//!   end"*, but a fixed z would put the free end above or below the head
//!   depending on where in the frame the cloud sits; a length is what
//!   descends from every head the same way;
//! - **the sense of z is read, never assumed** — the CRS says which way is
//!   down (`ZIncreasingDownward`), not this operation, and a frame that does
//!   not say is refused rather than guessed;
//! - **the seed is a setting, and the activity records it** — the same seed
//!   over the same cloud remakes the same polylines, bit for bit
//!   ([`energyml_analysis::fake`]);
//! - **fabricated data says so** — the recipe's own name, and a note on the
//!   plan, so that nothing downstream mistakes a synthetic wellhead for a
//!   measurement.
//!
//! # What it does not do itself
//!
//! The draw is [`energyml_analysis::fake::vertical_polylines`]; the writing
//! is [`super::polylines::write`] — a second writer of the same
//! `PolylineSetRepresentation` would be a second place for the same mistakes.

use energyml_analysis::fake::{self, vertical_polylines};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

use crate::crs::FrameKind;
use crate::declare::{Inputs, Params, Shape, Suggestions};
use crate::operation::{About, Category, Context, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::Pieces;

use super::polylines::PolylinesInputs;
use super::support;
use super::written::Written;

/// The operation.
pub struct PolylinesFromCloud;

/// What `polylines-from-cloud` is told.
#[derive(Debug, Params)]
pub struct FakeParams {
    /// how many cuts each polyline gets — `cuts + 1` nodes, the head included
    #[param(flatten)]
    pub cuts: Cuts,
    /// how long each polyline is, from its head down — the frame's vertical unit, and its own sense of down
    #[param(flatten)]
    pub length: Length,
    /// how the cuts are placed along the length
    #[param(flatten)]
    pub spacing: Spacing,
    /// the random seed — the same seed over the same cloud remakes the same polylines
    #[param(default = 0_i64, optional, min = 0)]
    pub seed: i64,
    /// The frame, what is represented, the naming.
    #[param(flatten)]
    pub written: Written,
}

/// how many cuts
#[derive(Debug, Params)]
#[params(tag = "cuts")]
pub enum Cuts {
    /// exactly this many, every polyline
    #[case(default)]
    Fixed(FixedCuts),
    /// a random count, uniform between two bounds — the same seed draws the same count
    Random(RandomCuts),
}

/// A fixed number of cuts.
#[derive(Debug, Params)]
pub struct FixedCuts {
    /// cuts: exactly this many
    #[param(min = 1)]
    pub count: i64,
}

/// A random number of cuts.
#[derive(Debug, Params)]
pub struct RandomCuts {
    /// cuts: the fewest
    #[param(min = 1)]
    pub cuts_min: i64,
    /// cuts: the most
    #[param(min = 1)]
    pub cuts_max: i64,
}

/// how long, from the head down
#[derive(Debug, Params)]
#[params(tag = "length")]
pub enum Length {
    /// exactly this long, every polyline
    #[case(default)]
    Fixed(FixedLength),
    /// a random length, uniform between two bounds — the same seed draws the same length
    Random(RandomLength),
}

/// A fixed length.
#[derive(Debug, Params)]
pub struct FixedLength {
    /// length: exactly this long
    ///
    /// **Not named `length`**: the tag above already is, and `template.rs`
    /// compares by title — the same name on both would read as one
    /// parameter declared twice, as two different things.
    #[param(min = 0.0)]
    pub value: f64,
}

/// A random length.
#[derive(Debug, Params)]
pub struct RandomLength {
    /// length: the shortest
    #[param(min = 0.0)]
    pub length_min: f64,
    /// length: the longest
    #[param(min = 0.0)]
    pub length_max: f64,
}

/// how the cuts are spaced
#[derive(Debug, Params)]
#[params(tag = "spacing")]
pub enum Spacing {
    /// evenly, every segment the same length
    #[case(default)]
    Regular,
    /// evenly, jittered by up to a fraction of the even segment length
    Fluctuating(Fluctuation),
}

/// The fluctuation's own setting.
#[derive(Debug, Params)]
pub struct Fluctuation {
    /// spacing: how far a cut may drift from its even position, as a fraction of the even segment length
    #[param(min = 0.0, max = 1.0)]
    pub fraction: f64,
}

/// What `polylines-from-cloud` takes: a point cloud, each point a head.
#[derive(Debug, Inputs)]
pub struct FakeInputs {
    /// the point cloud — each point becomes the head of a polyline
    #[input(topology = "points", least = 1)]
    pub shape: Vec<Shape>,
}

fn cuts_of(cuts: &Cuts) -> fake::Cuts {
    match cuts {
        Cuts::Fixed(one) => fake::Cuts::Fixed(one.count.max(0) as u32),
        Cuts::Random(range) => fake::Cuts::Random {
            min: range.cuts_min.max(0) as u32,
            max: range.cuts_max.max(0) as u32,
        },
    }
}

fn length_of(length: &Length) -> fake::Length {
    match length {
        Length::Fixed(one) => fake::Length::Fixed(one.value),
        Length::Random(range) => fake::Length::Random {
            min: range.length_min,
            max: range.length_max,
        },
    }
}

fn spacing_of(spacing: &Spacing) -> fake::Spacing {
    match spacing {
        Spacing::Regular => fake::Spacing::Regular,
        Spacing::Fluctuating(one) => fake::Spacing::Fluctuating {
            fraction: one.fraction,
        },
    }
}

impl Operation for PolylinesFromCloud {
    type Params = FakeParams;
    type Inputs = FakeInputs;

    fn about(&self) -> About {
        About {
            name: "polylines-from-cloud",
            describe: "fake, dated test data: a vertical polyline descending from each point of a cloud, its cuts fixed or random, regularly or unevenly spaced",
            category: Category::Geometry,
            makes: &["resqml22.PolylineSetRepresentation"],
            recipe: "polylines-from-cloud",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
        if let Some(name) = support::suggested_title(bag)
            && slots.declared("title")
        {
            slots.set("title", serde_json::json!(format!("{name} — fake polylines")));
        }
    }

    fn check(&self, inputs: &FakeInputs) -> Result<(), Refusal> {
        let points: usize = inputs.shape.iter().map(|s| s.geometry.point_count()).sum();
        if points == 0 {
            Err(Refusal::on("shape", "the input has no point at all"))
        } else {
            Ok(())
        }
    }

    fn plan(&self, inputs: FakeInputs, params: FakeParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        // **The sense of z, read from the frame — never assumed** (the
        // original need's own note). A frame named in the package speaks for itself; one created
        // here from EPSG codes is `crs::build`'s own convention, elevation up
        // — the same reading `well-from-polyline` gives a fresh frame.
        let known = cx.known();
        let mut discarded_rank = 0usize;
        let frame = support::frame_in(
            &known,
            &params.written.frame,
            cx.identity,
            cx.when,
            &mut discarded_rank,
        )?;

        // The heads, placed in that frame before anything is computed from
        // them (`support::shape_placed`).
        let mut heads: Vec<[f64; 3]> = Vec::new();
        for one in &inputs.shape {
            let nodes: Vec<[f64; 3]> = one
                .geometry
                .patches
                .iter()
                .flat_map(|p| p.points.iter().copied())
                .collect();
            heads.extend(support::shape_placed(
                &one.name,
                &nodes,
                one.known.as_ref(),
                &frame,
            )?);
        }
        if heads.is_empty() {
            return Err(Refusal::on("shape", "the input has no point at all"));
        }
        if frame.kind == Some(FrameKind::Geographic) {
            let (slot, what) = match params.written.frame.crs.as_ref() {
                Some(_) => (
                    "crs",
                    format!(
                        "the frame `{}` named",
                        frame.crs["Title"].as_str().unwrap_or_default()
                    ),
                ),
                None => ("project", "the frame created without `project`".to_string()),
            };
            return Err(Refusal::on(
                slot,
                format!(
                    "a vertical polyline is a vertical extent, and {what} measures its horizontal axes in degrees while its length is linear: every polyline would be written between a hundred and ten thousand times longer than the model is wide — name a projected system in `project` (EPSG:2154 for mainland France), or a LocalEngineeringCompoundCrs as `crs`"
                ),
            ));
        }
        let vertical_up = if frame.objects.is_empty() {
            known.first().and_then(|k| k.vertical_up)
        } else {
            Some(true)
        };
        let Some(up) = vertical_up else {
            return Err(Refusal::on(
                "crs",
                "the frame does not say which way is up (`ZIncreasingDownward`), and a length cannot be turned into an end point without it",
            ));
        };

        let seed_value = params.seed.max(0) as u64;
        let made = vertical_polylines(
            &heads,
            &cuts_of(&params.cuts),
            &length_of(&params.length),
            &spacing_of(&params.spacing),
            up,
            seed_value,
        )
        .map_err(|why| Refusal::new("refused", why.to_string()))?;

        let mut points = Vec::new();
        let mut runs = Vec::new();
        for line in &made {
            runs.push(line.points.len() as u32);
            points.extend(line.points.iter().copied());
        }
        let title = params.written.naming.title();
        let shape = Shape {
            name: format!("{title} — cloud"),
            known: None,
            geometry: Geometry {
                kind: "polylines-from-cloud",
                patches: vec![Patch {
                    index: 0,
                    node_offset: 0,
                    points,
                    topology: Topology::Polylines(runs),
                    closed: Vec::new(),
                }],
            },
            closed: None,
        };
        let seed = cx.seed(&title);
        let lines = PolylinesInputs {
            shape: vec![shape],
            features: None,
        };
        // **Never closed**: a line that descends from a head never rings back
        // to it, forced rather than left to `closed`'s own "seen" detection.
        let mut plan = super::polylines::write(&lines, &params.written, Some(false), &seed, cx)?;

        plan.activity.recipe = "polylines-from-cloud";
        plan.activity.title = format!("polylines-from-cloud → {title}");
        plan.activity.parameters.push(Parameter::Double {
            title: "headCount".to_string(),
            value: heads.len() as f64,
            uom: None,
        });
        // **Not `seed`**: it is already a parameter, and `Registry::plan`
        // records every settled one under its own title automatically — a
        // report of the same title would read as a disagreement of direction
        // (`crates/energyml-build/ARCHITECTURE.md`: a parameter title cannot
        // be both an input and an output).
        //
        // **Fabricated data says so** — the rule on invented objects (ADR-0019), taken by
        // the other end: a note nobody can miss, beside the recipe's name.
        plan.says.push(Note {
            key: "fake",
            detail: format!(
                "synthetic test data: {} vertical polyline(s) generated from a point cloud, seed {} — not a measurement",
                heads.len(),
                params.seed
            ),
        });
        Ok(plan)
    }

    fn reports(&self) -> Vec<Slot> {
        let mut slots =
            crate::operation::AnyOperation::reports(&crate::operation::Typed(super::polylines::Polylines));
        slots.push(Slot::required(
            "headCount",
            Need::Number {
                uom: None,
                least: None,
                most: None,
            },
            "cloud points turned into polylines",
        ));
        slots
    }
}
