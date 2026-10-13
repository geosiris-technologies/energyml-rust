//! `pointset-from-wellheads` — the DEM of a batch of boreholes, by their wellheads.
//!
//! The need, expressed in the very sentence that removes the node at MD 0:
//! an operation on a set of wells to extract a DEM point cloud
//! from all the MD=0 points of the wells.
//!
//! # The wellhead is read in the datum, never in the geometry
//!
//! This is the point not to undo, and it follows from this rule:
//! a trajectory starts at the **first stated depth**,
//! so its first node is not the ground. On `AUB-PRO-SP06226`, whose log
//! starts at 31.5 m, reading it there would answer thirty metres too low — and a
//! terrain surface thirty metres too low looks exactly like a
//! terrain surface.
//!
//! The reference point is the only thing that states it. It arrives resolved
//! in [`crate::pieces::Known::wellhead`], because following it needs
//! storage and `plan` is pure.
//!
//! # A collar is placed, so it is written in a frame where placed is local
//!
//! Until 24/09 the collars were the datums' coordinates **as they stand**,
//! written under the wells' `LocalCrs`. Right under an identity compound —
//! every package of the corpus — and wrong as soon as the compound has an
//! origin or an azimuth: a `ReferencePointInALocalEngineeringCompoundCrs` is
//! local to its compound (common TRG v2.3 §13.25, p. 371), a
//! `ReferencePointInACrs` projected (§13.24, p. 370), and the numbers of either
//! written under a frame turned 30° — the grid frame of Lutetia —
//! put every collar elsewhere.
//!
//! The boundary now **places** every datum (`read::wellhead_of`, over
//! `energyml_domain::crs::Crs::to_projected`): `Known::wellhead` is a projected
//! x, y and an elevation, whatever form the datum took. Writing those under
//! the wells' compound would need its `from_projected`, and a pure operation
//! does not receive a frame's placement. So the collars are written under
//! **the identity compound of the wells' own systems** — their projected and
//! vertical EPSG codes, `Known::crs_epsg` and `Known::vertical_epsg` — where a
//! placed number *is* a local one, and the plan says so (`collars-placed`). A
//! `ProjectedCrs` named as the frame (decision (c)) is such a frame already
//! and is kept; a caller who gives `sourceEpsg` states the system itself. A
//! frame whose projected code nobody states is refused by name: the collars
//! would have no system to be written in.
//!
//! The cross-check is in `tests/wellheads.rs`: a well written under a turned
//! compound, its collar written here against its datum read by
//! `energyml_geometry::well` — two readers of one place.
//!
//! # It delegates the writing
//!
//! The crate's rule: *an operation that writes the same object as another delegates
//! to it*. What comes out is a `PointSetRepresentation`, so `pointset`
//! writes it and this one only does the scattering — a second writer would be a
//! second place for the same mistakes.

use crate::declare::{Epsg, Inputs, Params, Shape, Suggestions};
use crate::operation::{About, Category, Context, Note, Operation, Output, Plan, Refusal};
use crate::pieces::Pieces;

use super::pointset::{Pointset, PointsetInputs, PointsetParams};
use super::written::{FrameParams, Written};

/// The operation.
pub struct Wellheads;

/// What is set on it: the block common to every writer, and nothing else.
#[derive(Debug, Params)]
pub struct WellheadsParams {
    /// The frame, what is represented, the naming.
    #[param(flatten)]
    pub written: Written,
}

/// What it takes: boreholes of the package.
#[derive(Debug, Inputs)]
pub struct WellheadsInputs {
    /// the wellbores whose collar becomes a point — their trajectories or their frames
    ///
    /// A representation, and not a `WellboreFeature`: this slot receives
    /// shapes, and a feature has none. It is the trajectory or the frame
    /// that is designated, and both name the same datum.
    pub wells: Vec<Shape>,
}

impl Operation for Wellheads {
    type Params = WellheadsParams;
    type Inputs = WellheadsInputs;

    /// The point set, what it represents when asked, and the frame when created.
    ///
    /// `feature` and `interpretation` are written only when `featureType` (or
    /// `interpretation`) asks for them, and **of the type asked for** — any
    /// RESQML 2.2 feature, which a static list cannot name: their `types` are
    /// left empty, meaning *the caller's choice*.
    fn gives(&self) -> Vec<Output> {
        let mut out = vec![
            Output {
                role: "representation",
                types: &["resqml22.PointSetRepresentation"],
                least: 1,
                most: Some(1),
                describe: "the collars: one point per wellbore, at its ground elevation",
            },
            Output {
                role: "feature",
                types: &[],
                least: 0,
                most: Some(1),
                describe: "the feature the points represent, of the type `featureType` names; none unless asked",
            },
            Output {
                role: "interpretation",
                types: &[],
                least: 0,
                most: Some(1),
                describe: "the interpretation over that feature, of the type `interpretationType` names; none unless a feature is asked for",
            },
        ];
        out.extend(super::wells_from_table::frame_outputs(false));
        out
    }

    fn about(&self) -> About {
        About {
            name: "pointset-from-wellheads",
            describe: "a point set of where wellbores are collared — the ground under each, read from its datum and not from its trajectory",
            category: Category::Create,
            makes: &["resqml22.PointSetRepresentation"],
            recipe: "pointset-from-wellheads",
            represents: None,
        }
    }

    fn also(&self) -> &'static [Category] {
        &[Category::Wells]
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
    }

    fn check(&self, inputs: &WellheadsInputs) -> Result<(), Refusal> {
        if inputs.wells.is_empty() {
            return Err(Refusal::on("wells", "it takes wellbores; the input holds none"));
        }
        Ok(())
    }

    fn plan(&self, inputs: WellheadsInputs, params: WellheadsParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        // **What does not say where it is collared is named, never counted.**
        // A silent borehole removed in silence would make a DEM with holes a DEM
        // that looks complete — and that is the result that most looks like a
        // success.
        let gather = tracing::info_span!("wellheads.gather", wells = inputs.wells.len()).entered();
        let mut points: Vec<[f64; 3]> = Vec::new();
        let mut mute: Vec<&str> = Vec::new();
        for well in &inputs.wells {
            match well.known.as_ref().and_then(|known| known.wellhead) {
                Some(head) => points.push(head),
                None => mute.push(well.name.as_str()),
            }
        }
        if !mute.is_empty() {
            return Err(Refusal::on(
                "wells",
                format!(
                    "{} of the {} given state no datum, so where they are collared is not in the package: {}. \
                     A wellbore's collar is its reference point — a `ReferencePointInALocalEngineeringCompoundCrs` \
                     or a `ReferencePointInACrs` — never the first node of its trajectory, \
                     which since 22/09 is its first stated depth",
                    mute.len(),
                    inputs.wells.len(),
                    mute.join(", ")
                ),
            ));
        }

        // **One frame, or nothing.** Two boreholes written in two systems
        // have no common frame, and putting their collars in the same cloud
        // would place them wrong — with no number looking abnormal.
        let mut frames = inputs.wells.iter().filter_map(|well| {
            well.known.as_ref().and_then(|known| known.crs.as_ref()).map(|crs| (crs.uuid.as_str(), crs.title.as_str()))
        });
        if let Some((first, first_title)) = frames.next()
            && let Some((_, other_title)) = frames.find(|(uuid, _)| *uuid != first)
        {
            return Err(Refusal::on(
                "wells",
                format!(
                    "these wellbores are not written in one frame — `{first_title}` and `{other_title}` — so their \
                     collars have no common ground to sit on"
                ),
            ));
        }

        // **The frame the collars are written in** — see the module doc: they
        // are placed, so a frame where placed is local. Kept as asked when the
        // caller creates one (`sourceEpsg`) or names a `ProjectedCrs`, whose
        // local numbers are projected ones; otherwise the identity compound of
        // the wells' own codes.
        let mut written = params.written;
        let mut says = Vec::new();
        let named_projected = written
            .frame
            .crs
            .as_ref()
            .and_then(|crs| crs.0["QualifiedType"].as_str().map(str::to_string))
            .is_some_and(|kind| crate::crs::frame_kind(&kind) == Ok(crate::crs::FrameKind::Projected));
        if written.frame.source_epsg.is_none() && !named_projected {
            let first = inputs.wells.first().and_then(|well| well.known.as_ref());
            let Some(code) = first.and_then(|known| known.crs_epsg) else {
                return Err(Refusal::on(
                    "crs",
                    format!(
                        "the collars are read placed — projected x and y, an elevation — and the wells' frame `{}` states no projected EPSG code, so they have no system to be written in: give `sourceEpsg` (the projected system they are in)",
                        first.and_then(|known| known.crs.as_ref()).map(|crs| crs.title.as_str()).unwrap_or("?")
                    ),
                ));
            };
            let vertical = first.and_then(|known| known.vertical_epsg);
            says.push(Note {
                key: "collars-placed",
                detail: format!(
                    "the collars are placed through the wells' frame and written under the identity compound of EPSG:{code}{} — where a placed number is a local one — rather than under `{}`, whose origin and azimuth this operation does not receive",
                    vertical.map(|v| format!(" + EPSG:{v}")).unwrap_or_default(),
                    first.and_then(|known| known.crs.as_ref()).map(|crs| crs.title.as_str()).unwrap_or("?")
                ),
            });
            written.frame = FrameParams {
                crs: None,
                source_epsg: Some(Epsg(code)),
                project: Some(Epsg(code)),
                vertical: vertical.map(Epsg),
            };
        }

        drop(gather);
        let _write = tracing::info_span!("wellheads.write", points = points.len()).entered();
        // The scattering, and the writing delegated to `pointset` by its typed door.
        let semis = Shape {
            name: "wellheads".to_string(),
            geometry: energyml_geometry::geometry::Geometry {
                kind: "wellheads",
                patches: vec![energyml_geometry::geometry::Patch {
                    index: 0,
                    node_offset: 0,
                    points,
                    topology: energyml_geometry::geometry::Topology::Points,
                    closed: Vec::new(),
                }],
            },
            closed: None,
            // What `pointset` reads of an input object — the frame, what is
            // represented — is that of the boreholes, and it is the same for all:
            // that is what the refusal above has just established.
            known: inputs.wells.first().and_then(|well| well.known.clone()),
        };
        let mut plan = Pointset.plan(PointsetInputs { shape: vec![semis], features: None }, PointsetParams { written }, cx)?;
        plan.says.extend(says);
        Ok(plan)
    }
}
