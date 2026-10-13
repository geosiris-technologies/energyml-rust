//! `fault-from-sticks` — a fault: its `BoundaryFeature`, its
//! `FaultInterpretation`, the triangulated surface lofted between its sticks,
//! and the sticks themselves.
//!
//! This is the faults step of the reference model's build plan: its faults are
//! synthetic sticks lofted into surfaces, then intersected with the horizons.
//! The registry lists the features, the interpretations and their `FaultThrow`;
//! two faults are lofted from their sticks, two others are planes warped by dip
//! and azimuth, and one fault is written in two patches. The sticks carry
//! `LineRole` `interpretation line`.
//!
//! # Two ways to give the surface — a union
//!
//! - **`sticks`** — stated one by one, as text (`x y z; x y z; …`, top to
//!   bottom, the entries in order along the strike) or as the `lines` inputs,
//!   one polyline each, in the slot's order. A blank entry ends a patch: a fault
//!   in two patches is two bands, and a stick repeated on both sides of
//!   the break is the edge they share — written once among the sticks;
//! - **`plane`** — a trace, a dip and a dip azimuth, and how far down: one
//!   stick per point of the trace, drawn by `energyml_analysis::fault_sticks`.
//!   A curved trace warps the plane. The sticks drawn are **not** written:
//!   nobody digitised them, and the reference model holds the sticks of only some of its faults.
//!
//! # Nothing is written twice
//!
//! An operation that writes the same object as another delegates to it: the loft
//! is `energyml_analysis::loft`, the surface is [`super::triangles::write`],
//! the sticks [`super::polylines::write`], the feature and its interpretation
//! [`super::support::represented_in`] — the one place the pair is made. What
//! this module adds is what that shared writer cannot know: **the fields of a
//! fault** — the throw, `IsListric`, `IsSealed`, `MaximumThrow`, `MeanDip`,
//! `MeanAzimuth`, `Domain`, `HasOccurredDuring`. The interpretation it made is
//! rewritten, typed, with them; its uuid and title are kept. That is the
//! decision the crate's rules ask for (a fault interpretation carries
//! fields the shared writer doesn't write: decide and say): the pair is still
//! made in one place, and the fields of one kind of interpretation live in the
//! one operation that knows them — rather than a third copy of the pair beside
//! `well`'s and `boundary`'s.
//!
//! RESQML 2.2 has no kind on a `BoundaryFeature` (Features.xsd: `IsWellKnown`
//! only — the 2.0.1 `TectonicBoundaryKind` is gone): what makes it a fault is
//! the `FaultInterpretation` of it.
//!
//! # `IsListric`: the business rule, refused by name
//!
//! *« Indicates if the normal fault is listric or not. BUSINESS RULE: Must be
//! present if the fault is normal. Must not be present if the fault is not
//! normal. »* (RESQML 2.2 Interpretations.xsd l. 659–662; TRG 2.2 §6.20
//! p. 188.) The reference model reads *normal* as **the one throw is normal**
//! (a fault with two throws has no `IsListric`) — and so does this: a single
//! `normal` requires `listric`, anything else refuses it, a fault with no throw
//! stated included (its presence would say the fault is normal). The model's
//! first plan broke the first half of that rule on one fault; the deliberate-anomalies
//! model breaks the second on purpose, which this operation will
//! not write.
//!
//! The type's own annotation says a thrust *« has the attributes: is Listric =
//! 0, MaximumThrow = 0 »* — which the element's business rule forbids for a
//! fault that is not normal. The business rule is followed, and the plan says
//! so when a thrust is given.
//!
//! # A synthetic fault says so
//!
//! The reference model's rules on invented objects (ADR-0019): every invented
//! object carries an `ExtensionNameValue` `synthetic = true` and a sentence in
//! `Citation/Description`, and no constant goes without a source. With `synthetic`, a
//! `source` is required, and every object written — feature, interpretation,
//! surface, sticks — carries the mark and the sentence, the form
//! `horizon-from-law` writes. A fault digitised from data states its source
//! too, when given, without the mark.
//!
//! # Two readings of the dip
//!
//! `MeanDip` and `MeanAzimuth` are what the model **states** — *« "mean" means
//! "representative"; it is not a mathematically derived mean »*
//! (Interpretations.xsd). The lofted surface is **measured** as well
//! (`fault_sticks::orientation`), recorded in the activity and said beside what
//! was stated: a trace drawn oblique to its dip, or a stick typed in the wrong
//! place, shows there. Nothing is corrected.
//!
//! # Identity
//!
//! From the title, like `horizon-from-law`'s: correcting a stick rewrites the
//! same fault (correcting means editing the recipe and replaying it).
//! Under uuids given by the caller, the sticks take theirs
//! **after** the surface's — the two writers each count from the start.
//!
//! # What is not suggested
//!
//! Neither the title nor an interpretation of an input: a stick read from the
//! package represents something else, and a fault filed under it — the defect
//! `well-from-plan` once had with the ground — is what a suggestion would
//! make look deliberate. This operation declares no `interpretation` slot at
//! all: it makes its own.
//!
//! # References
//!
//! - Christiansen and Sederberg (1978), the classic tiling between contours, and Fuchs, Kedem and Uselton (1977), the optimum, as named in `energyml-geostat` (`loft`).
//!   TODO(reference): verify — both citations are given without title, venue or DOI; complete and check them.

use serde_json::json;

use energyml_analysis::fault_sticks::{self, Orientation};
use energyml_analysis::loft::{LoftLine, loft};
use energyml_domain::provenance::Parameter;
use energyml_domain::units::Dimension;
use energyml_eml_2_3 as eml;
use energyml_geometry::geometry::{Geometry, Patch, Topology};
use energyml_resqml_2_2 as resqml;

use crate::crs::FrameKind;
use crate::declare::{
    Choice, CrsRef, Inputs, InterpretationRef, ObjectIn, Params, Shape, Suggestions, TypeName,
};
use crate::operation::{About, Category, Context, Identity, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::Pieces;

use super::polylines::{PolylinesInputs, said_closed};
use super::support;
use super::triangles::TrianglesInputs;
use super::written::{FrameParams, NamingParams, NewFeatureParams, RepresentedParams, Written};

/// The operation.
pub struct FaultFromSticks;

/// How a fault moved — RESQML's `ThrowKind`, a closed list here: an open
/// value would be a kind nobody reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum Throw {
    /// The hanging wall went down.
    Normal,
    /// The hanging wall went up.
    Reverse,
    /// The two sides slid past each other.
    StrikeSlip,
    /// A low-angle reverse fault.
    Thrust,
    /// A throw that changes sense along the fault.
    Scissor,
    /// A throw that changed during the fault's life.
    Variable,
}

impl Throw {
    fn written(self) -> resqml::ThrowKindExt {
        match self {
            Throw::Normal => resqml::ThrowKindExt::Normal,
            Throw::Reverse => resqml::ThrowKindExt::Reverse,
            Throw::StrikeSlip => resqml::ThrowKindExt::StrikeMinusSlip,
            Throw::Thrust => resqml::ThrowKindExt::Thrust,
            Throw::Scissor => resqml::ThrowKindExt::Scissor,
            Throw::Variable => resqml::ThrowKindExt::Variable,
        }
    }
}

/// What `fault-from-sticks` is told.
#[derive(Debug, Params)]
pub struct FaultParams {
    /// How the surface is given.
    #[param(flatten)]
    pub geometry: GeometryParams,
    /// how the fault moved — one kind, or several for a fault with several plays; left out, the interpretation states none
    pub throw: Vec<Throw>,
    /// whether the normal fault is listric — required when its one throw is normal, refused otherwise (the business rule of IsListric)
    pub listric: Option<bool>,
    /// whether the fault is sealed
    pub sealed: Option<bool>,
    /// the largest throw, in metres
    #[param(min = 0.0, uom = "m")]
    pub maximum_throw: Option<f64>,
    /// the representative dip, in degrees from the horizontal — stated, and confronted with the surface's
    #[param(min = 0.0, max = 90.0, uom = "dega")]
    pub mean_dip: Option<f64>,
    /// the representative dip direction, in degrees clockwise from grid north — stated, and confronted with the surface's
    #[param(min = 0.0, max = 360.0, uom = "dega")]
    pub mean_azimuth: Option<f64>,
    /// the fault is invented: every object written carries `synthetic = true` and its source
    #[param(default = false, optional)]
    pub synthetic: bool,
    /// where the fault comes from — a document with its page, or why a synthetic fault is plausible; required for a synthetic one
    pub source: Option<String>,
    /// Where the coordinates are.
    #[param(flatten)]
    pub frame: FrameParams,
    /// The title and the file.
    #[param(flatten)]
    pub naming: NamingParams,
}

/// how the surface is given — sticks stated one by one, or a plane drawn from a trace, a dip and a dip azimuth
#[derive(Debug, Params)]
#[params(tag = "geometry")]
pub enum GeometryParams {
    /// Sticks, as text or as the `lines` inputs.
    #[case(default)]
    Sticks(SticksParams),
    /// A plane, warped along a curved trace.
    Plane(PlaneParams),
}

/// What sticks given one by one add.
#[derive(Debug, Params)]
pub struct SticksParams {
    /// sticks: one per entry, its points `x y z` separated by `;`, top to bottom, the entries in order along the strike — a blank entry ends a patch; or leave it empty and give `lines`
    pub sticks: Vec<String>,
    /// sticks: write the sticks too, as a polyline set whose role is `interpretation line`
    #[param(default = true, optional)]
    pub write_sticks: bool,
}

/// What a plane adds.
#[derive(Debug, Params)]
pub struct PlaneParams {
    /// plane: the fault's top line, its points `x y z` separated by `;` — a curved trace warps the plane
    pub trace: String,
    /// plane: the dip, in degrees from the horizontal
    #[param(min = 0.0, max = 90.0, uom = "dega")]
    pub dip: f64,
    /// plane: the direction the fault goes down, in degrees clockwise from grid north
    #[param(min = 0.0, max = 360.0, uom = "dega")]
    pub dip_azimuth: f64,
    /// plane: how far the fault goes down below its trace, measured vertically, in metres
    #[param(min = 0.0, uom = "m")]
    pub vertical_extent: f64,
}

/// What `fault-from-sticks` takes.
#[derive(Debug, Inputs)]
pub struct FaultInputs {
    /// the sticks, one polyline per input, top to bottom, in order along the strike — or give them as text in `sticks`
    #[input(topology = "polylines", ordered)]
    pub lines: Vec<Shape>,
    /// the older genetic boundary the faulting came after — with `chronoTop`, when the fault occurred
    #[input(types = "resqml22.HorizonInterpretation, resqml22.BoundaryFeatureInterpretation")]
    pub chrono_bottom: Option<ObjectIn>,
    /// the younger genetic boundary the faulting came before — with `chronoBottom`
    #[input(types = "resqml22.HorizonInterpretation, resqml22.BoundaryFeatureInterpretation")]
    pub chrono_top: Option<ObjectIn>,
}

/// One stick: its name for a message, and its points, top to bottom.
#[derive(Debug, Clone)]
struct Stick {
    name: String,
    points: Vec<[f64; 3]>,
}

/// The points of `x y z; x y z; …`, or what is wrong with them.
fn points_of(text: &str) -> Result<Vec<[f64; 3]>, String> {
    let pieces: Vec<&str> = text.split(';').map(str::trim).collect();
    let mut out = Vec::new();
    for (at, piece) in pieces.iter().enumerate() {
        // A closing `;` is a habit, not a point.
        if piece.is_empty() && at + 1 == pieces.len() && at > 0 {
            continue;
        }
        let words: Vec<&str> = piece.split_whitespace().collect();
        if words.len() != 3 {
            return Err(format!(
                "point {} is `{piece}` — a point is `x y z`, three numbers separated by spaces, and points are separated by `;`",
                at + 1
            ));
        }
        let mut point = [0.0f64; 3];
        for (slot, word) in point.iter_mut().zip(&words) {
            *slot = word
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| {
                    format!(
                        "point {}: `{word}` is not a finite number — a decimal separator is `.`",
                        at + 1
                    )
                })?;
        }
        out.push(point);
    }
    Ok(out)
}

/// The one open polyline an input holds — or why it is no stick.
fn one_stick(shape: &Shape) -> Result<Vec<[f64; 3]>, Refusal> {
    let mut found: Option<Vec<[f64; 3]>> = None;
    let mut global = 0usize;
    for patch in &shape.geometry.patches {
        let Topology::Polylines(runs) = &patch.topology else {
            continue;
        };
        let mut start = 0usize;
        for (local, &run) in runs.iter().enumerate() {
            let run = run as usize;
            if found.is_some() {
                return Err(Refusal::on(
                    "lines",
                    format!(
                        "`{}` holds several polylines — a stick is one line: give one per input, in order along the strike",
                        shape.name
                    ),
                ));
            }
            let points = patch.points.get(start..start + run).ok_or_else(|| {
                Refusal::on(
                    "lines",
                    format!(
                        "`{}`: the polylines add up to more nodes than the patch holds",
                        shape.name
                    ),
                )
            })?;
            if said_closed(patch, local, shape.closed.as_deref(), global) == Some(true) {
                return Err(Refusal::on(
                    "lines",
                    format!(
                        "`{}` is a ring — a stick is an open line, top to bottom",
                        shape.name
                    ),
                ));
            }
            found = Some(points.to_vec());
            start += run;
            global += 1;
        }
    }
    found.ok_or_else(|| Refusal::on("lines", format!("`{}` holds no polyline", shape.name)))
}

/// The reference a dating input stands for.
fn dated(slot: &str, input: &ObjectIn) -> Result<eml::DataObjectReference, Refusal> {
    let known = input.known.reference.as_ref().ok_or_else(|| {
        Refusal::on(
            slot,
            format!("`{}` was given without a reference to write", input.name),
        )
    })?;
    Ok(crate::crs::reference_of(known))
}

/// The `synthetic` mark the reference model asks of an invented object
/// (ADR-0019) — the form `horizon-from-law` writes.
fn synthetic_mark() -> eml::ExtensionNameValue {
    eml::ExtensionNameValue {
        name: "synthetic".to_string(),
        value: eml::StringMeasure { value: "true".to_string(), ..Default::default() },
        description: Some("an invented fault, written by `fault-from-sticks` from stated sticks or a stated plane, not measured".to_string()),
        ..Default::default()
    }
}

/// The frame as the plan will write it: what it is, which way is up, in what
/// unit its vertical axis counts.
struct Inspected {
    up: Option<bool>,
    unit: Option<String>,
}

impl Operation for FaultFromSticks {
    type Params = FaultParams;
    type Inputs = FaultInputs;

    fn about(&self) -> About {
        About {
            name: "fault-from-sticks",
            describe: "a fault — its feature, its fault interpretation with the throw it is given, the surface lofted between its sticks (or drawn from a plane), and the sticks",
            category: Category::Faults,
            makes: &[
                "resqml22.BoundaryFeature",
                "resqml22.FaultInterpretation",
                "resqml22.TriangulatedSetRepresentation",
                "resqml22.PolylineSetRepresentation",
            ],
            recipe: "fault-from-sticks",
            represents: Some(("resqml22.BoundaryFeature", "resqml22.FaultInterpretation")),
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
        // **Not an input's title**: a stick read from the package is another
        // object, and the fault is named for itself — see the module.
        if let Some(slot) = slots.slot_mut("title") {
            slot.suggested = None;
            slot.from = None;
        }
    }

    fn check(&self, inputs: &FaultInputs) -> Result<(), Refusal> {
        for shape in &inputs.lines {
            one_stick(shape)?;
        }
        match (&inputs.chrono_bottom, &inputs.chrono_top) {
            (Some(_), None) => Err(Refusal::on(
                "chronoTop",
                "`chronoBottom` is given alone: a time interval between two genetic boundaries needs `chronoTop` too — the one the faulting came before",
            )),
            (None, Some(_)) => Err(Refusal::on(
                "chronoBottom",
                "`chronoTop` is given alone: a time interval between two genetic boundaries needs `chronoBottom` too — the one the faulting came after",
            )),
            _ => Ok(()),
        }
    }

    fn plan(&self, inputs: FaultInputs, params: FaultParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        let title = params.naming.title();

        // -- what the fault is, before any geometry ------------------------------
        let mut throws: Vec<Throw> = Vec::new();
        for one in &params.throw {
            if !throws.contains(one) {
                throws.push(*one);
            }
        }
        let normal = throws == [Throw::Normal];
        match (normal, params.listric) {
            (true, None) => {
                return Err(Refusal::on(
                    "listric",
                    "the fault's one throw is normal, and `IsListric` must then be given — « BUSINESS RULE: Must be present if the fault is normal » (RESQML 2.2 Interpretations.xsd l. 659–662; TRG 2.2 §6.20 p. 188): say whether it is listric",
                ));
            }
            (false, Some(_)) => {
                let what = match throws.as_slice() {
                    [] => "not stated".to_string(),
                    list => list
                        .iter()
                        .map(|one| one.word())
                        .collect::<Vec<_>>()
                        .join(" and "),
                };
                return Err(Refusal::on(
                    "listric",
                    format!(
                        "`IsListric` is for a normal fault — « Must not be present if the fault is not normal » (RESQML 2.2 Interpretations.xsd l. 659–662; TRG 2.2 §6.20 p. 188) — and this one's throw is {what}: leave it out (a fault with several plays is not *the* normal fault the rule names)"
                    ),
                ));
            }
            _ => {}
        }
        let source = params
            .source
            .as_deref()
            .map(str::trim)
            .filter(|one| !one.is_empty())
            .map(str::to_string);
        if params.synthetic && source.is_none() {
            return Err(Refusal::on(
                "source",
                "a synthetic fault says where its numbers come from — a document with its page, or why they are plausible (reference-model rule: no constant without a source)",
            ));
        }
        let chrono = match (&inputs.chrono_bottom, &inputs.chrono_top) {
            (Some(bottom), Some(top)) => Some((dated("chronoBottom", bottom)?, dated("chronoTop", top)?)),
            _ => None,
        };

        // -- the frame: what it is, which way is up ------------------------------
        let frame = inspect(&params.frame, cx)?;

        // -- the sticks, in patches ----------------------------------------------
        let mut says: Vec<Note> = Vec::new();
        // The sticks as the patches use them, and the sticks to write — a stick
        // repeated across a break is the edge two patches share, one stick.
        let mut patches: Vec<Vec<Stick>> = Vec::new();
        let mut written_sticks: Vec<Stick> = Vec::new();
        let mut in_package = 0usize;
        let (slot, how, write_sticks) = match &params.geometry {
            GeometryParams::Sticks(given) => {
                match (given.sticks.is_empty(), inputs.lines.is_empty()) {
                    (false, false) => {
                        return Err(Refusal::on(
                            "sticks",
                            "the sticks are given twice — as text in `sticks` and as `lines`: give them one way",
                        ));
                    }
                    (true, true) => {
                        return Err(Refusal::on(
                            "sticks",
                            "no stick is given: state them as text in `sticks` (`x y z; x y z`, top to bottom), or give them as polylines in `lines`",
                        ));
                    }
                    (false, true) => {
                        let mut current: Vec<Stick> = Vec::new();
                        let mut count = 0usize;
                        let mut repeated = 0usize;
                        for (at, entry) in given.sticks.iter().enumerate() {
                            if entry.trim().is_empty() {
                                if current.is_empty() {
                                    return Err(Refusal::on(
                                        "sticks",
                                        format!(
                                            "`sticks[{at}]` is blank where no patch has begun — a blank entry ends a patch that holds sticks"
                                        ),
                                    ));
                                }
                                patches.push(std::mem::take(&mut current));
                                continue;
                            }
                            count += 1;
                            let points = points_of(entry)
                                .map_err(|why| Refusal::on("sticks", format!("`sticks[{at}]`: {why}")))?;
                            let stick = Stick {
                                name: format!("stick {count}"),
                                points,
                            };
                            // The first stick after a break, bit for bit the last
                            // one before it: the edge the two patches share.
                            let shared = current.is_empty()
                                && patches
                                    .last()
                                    .and_then(|patch| patch.last())
                                    .is_some_and(|last| same(&last.points, &stick.points));
                            if shared {
                                repeated += 1;
                            } else {
                                written_sticks.push(stick.clone());
                            }
                            current.push(stick);
                        }
                        if current.is_empty() {
                            return Err(Refusal::on(
                                "sticks",
                                "the last entry is blank — a blank entry ends a patch, and the patch after it would hold no stick",
                            ));
                        }
                        patches.push(current);
                        if repeated > 0 {
                            says.push(Note {
                                key: "shared-stick",
                                detail: format!(
                                    "{repeated} stick(s) given on both sides of a break: the edge two patches share, written once among the sticks"
                                ),
                            });
                        }
                    }
                    (true, false) => {
                        let mut patch = Vec::new();
                        for shape in &inputs.lines {
                            let stick = Stick {
                                name: shape.name.clone(),
                                points: one_stick(shape)?,
                            };
                            if shape
                                .known
                                .as_ref()
                                .is_some_and(|known| known.reference.is_some())
                            {
                                in_package += 1;
                            } else {
                                written_sticks.push(stick.clone());
                            }
                            patch.push(stick);
                        }
                        patches.push(patch);
                        // A set of sticks holds all of them or none: half of
                        // them written beside objects already in the package
                        // would read as the whole.
                        if in_package > 0 {
                            written_sticks.clear();
                        }
                    }
                }
                let sticks: usize = patches.iter().map(Vec::len).sum();
                let slot = if inputs.lines.is_empty() {
                    "sticks"
                } else {
                    "lines"
                };
                (
                    slot,
                    format!("lofted between {sticks} stick(s) given in order along its strike"),
                    given.write_sticks,
                )
            }
            GeometryParams::Plane(plane) => {
                if !inputs.lines.is_empty() {
                    return Err(Refusal::on(
                        "lines",
                        "a plane draws its own sticks from `trace`: give polylines with `geometry: sticks`",
                    ));
                }
                let Some(up) = frame.up else {
                    return Err(Refusal::on(
                        "crs",
                        "the frame does not say which way its vertical axis points, and a plane goes **down** its dip: name a LocalEngineeringCompoundCrs as `crs`, or give `sourceEpsg`",
                    ));
                };
                if let Some(unit) = &frame.unit
                    && unit != "m"
                {
                    return Err(Refusal::on(
                        "crs",
                        format!(
                            "the frame counts its vertical axis in `{unit}`, and a plane is drawn in metres — a dip is an angle between two lengths of one unit"
                        ),
                    ));
                }
                let trace = points_of(&plane.trace).map_err(|why| Refusal::on("trace", why))?;
                let drawn = fault_sticks::sticks_from_plane(
                    &trace,
                    plane.dip,
                    plane.dip_azimuth,
                    plane.vertical_extent,
                    up,
                )
                .map_err(|why| {
                    let slot = match why {
                        fault_sticks::PlaneError::Dip { .. } => "dip",
                        fault_sticks::PlaneError::Azimuth { .. } => "dipAzimuth",
                        fault_sticks::PlaneError::Extent { .. } => "verticalExtent",
                        _ => "trace",
                    };
                    Refusal::on(slot, why)
                })?;
                patches.push(
                    drawn
                        .into_iter()
                        .enumerate()
                        .map(|(at, points)| Stick {
                            name: format!("stick {}", at + 1),
                            points,
                        })
                        .collect(),
                );
                (
                    "trace",
                    format!(
                        "drawn from its trace at {}° toward {}°, {} m down",
                        plane.dip, plane.dip_azimuth, plane.vertical_extent
                    ),
                    false,
                )
            }
        };
        for (at, patch) in patches.iter().enumerate() {
            if patch.len() < 2 {
                return Err(Refusal::on(
                    slot,
                    format!(
                        "patch {} holds {} stick(s) — a band is lofted between two at least",
                        at + 1,
                        patch.len()
                    ),
                ));
            }
            if let Some(short) = patch.iter().find(|stick| stick.points.len() < 2) {
                return Err(Refusal::on(
                    slot,
                    format!(
                        "`{}` has {} point(s) — a stick is a line, two points at least",
                        short.name,
                        short.points.len()
                    ),
                ));
            }
        }

        // -- the loft, a band per patch -------------------------------------------
        cx.stop_if_cancelled()?;
        let mut made: Vec<Patch> = Vec::new();
        let mut all_points: Vec<[f64; 3]> = Vec::new();
        let mut all_triangles: Vec<[u32; 3]> = Vec::new();
        for (at, patch) in patches.iter().enumerate() {
            let lines: Vec<LoftLine> = patch
                .iter()
                .map(|stick| LoftLine {
                    name: &stick.name,
                    points: &stick.points,
                    closed: false,
                })
                .collect();
            let band = loft(&lines).map_err(|why| Refusal::on(slot, why))?;
            let base = all_points.len() as u32;
            all_points.extend(band.points.iter().copied());
            all_triangles.extend(
                band.triangles
                    .iter()
                    .map(|t| [t[0] + base, t[1] + base, t[2] + base]),
            );
            let node_offset = made.iter().map(|one| one.points.len()).sum();
            made.push(Patch {
                index: at,
                node_offset,
                points: band.points,
                topology: Topology::Triangles(band.triangles),
                closed: Vec::new(),
            });
            let order: Vec<Vec<[f64; 3]>> = patch.iter().map(|stick| stick.points.clone()).collect();
            for back in fault_sticks::backtracking(&order) {
                says.push(Note {
                    key: "stick-order",
                    detail: format!(
                        "`{}` does not advance along the strike from `{}`: the surface folds back there — the order given is kept, check it",
                        patch[back].name,
                        patch[back - 1].name
                    ),
                });
            }
        }
        if made.len() > 1 {
            says.push(Note {
                key: "patches",
                detail: format!("{} patches, one band each, in one triangulated set", made.len()),
            });
        }

        // -- the surface, and the pair — written by their writers -----------------
        let seed = format!("fault-from-sticks:{title}");
        let written = Written {
            frame: params.frame.clone(),
            represented: RepresentedParams { interpretation: None },
            naming: params.naming.clone(),
            new_feature: NewFeatureParams {
                feature_type: Some(TypeName("resqml22.BoundaryFeature".to_string())),
                interpretation_type: TypeName("resqml22.FaultInterpretation".to_string()),
                feature_kind: None,
            },
        };
        let surface = Shape {
            name: title.clone(),
            geometry: Geometry {
                kind: "fault",
                patches: made,
            },
            closed: None,
            known: None,
        };
        let mut plan =
            super::triangles::write(&TrianglesInputs { shape: vec![surface] }, &written, &seed, cx)?;
        let (feature_uuid, feature_title) = plan
            .objects
            .iter()
            .find(|one| one.role == "feature")
            .map(|one| (one.uuid.clone(), one.title.clone()))
            .ok_or_else(|| Refusal::new("refused", "the surface's writer made no feature"))?;
        let at = plan
            .objects
            .iter()
            .position(|one| one.role == "interpretation")
            .ok_or_else(|| Refusal::new("refused", "the surface's writer made no interpretation"))?;
        let (interpretation_uuid, interpretation_title) =
            (plan.objects[at].uuid.clone(), plan.objects[at].title.clone());

        // -- what is said of every object ------------------------------------------
        let description = source.as_ref().map(|source| {
            if params.synthetic {
                format!("Synthetic fault, not measured: {how}. Source: {source}")
            } else {
                format!("Fault {how}. Source: {source}")
            }
        });
        if let Some(text) = &description
            && text.chars().count() > 2000
        {
            // A source cut short cites nothing.
            return Err(Refusal::on(
                "source",
                format!(
                    "makes a description of {} characters, and a Citation/Description holds 2000 — cite the document shorter",
                    text.chars().count()
                ),
            ));
        }

        // -- the interpretation, with what a fault carries --------------------------
        let domain = match frame.unit.as_deref().and_then(Dimension::of) {
            Some(Dimension::Time) => Some(resqml::Domain::Time),
            Some(Dimension::Length) => Some(resqml::Domain::Depth),
            _ => None,
        };
        let interpretation = resqml::FaultInterpretation {
            uuid: interpretation_uuid.clone(),
            schema_version: "2.2".to_string(),
            citation: eml::Citation {
                title: interpretation_title.clone(),
                originator: cx.when.originator.clone(),
                creation: cx.when.creation.clone(),
                format: energyml_core::CITATION_FORMAT.to_string(),
                description: description.clone(),
                ..Default::default()
            },
            extension_name_value: if params.synthetic {
                vec![synthetic_mark()]
            } else {
                Vec::new()
            },
            domain,
            has_occurred_during: chrono.clone().map(|(chrono_bottom, chrono_top)| {
                resqml::AbstractTimeInterval::GeneticBoundaryBasedTimeInterval(
                    resqml::GeneticBoundaryBasedTimeInterval {
                        chrono_bottom,
                        chrono_top,
                    },
                )
            }),
            interpreted_feature: crate::crs::reference_to(
                &feature_uuid,
                "resqml22.BoundaryFeature",
                &feature_title,
            ),
            is_listric: params.listric,
            is_sealed: params.sealed,
            maximum_throw: params.maximum_throw.map(|value| eml::LengthMeasure {
                value,
                uom: eml::LengthUom::M,
            }),
            mean_azimuth: params.mean_azimuth.map(|value| eml::PlaneAngleMeasure {
                value,
                uom: eml::PlaneAngleUom::Dega,
            }),
            mean_dip: params.mean_dip.map(|value| eml::PlaneAngleMeasure {
                value,
                uom: eml::PlaneAngleUom::Dega,
            }),
            throw_interpretation: if throws.is_empty() {
                Vec::new()
            } else {
                // **One** `FaultThrow` naming every play: separate ones would
                // each want a `HasOccurredDuring` to say which came first, and
                // the model gives none (a fault that is both normal and strike-slip).
                vec![resqml::FaultThrow {
                    throw: throws.iter().map(|one| one.written()).collect(),
                    has_occurred_during: None,
                }]
            },
            ..Default::default()
        };
        plan.objects[at].qualified_type = "resqml22.FaultInterpretation".to_string();
        plan.objects[at].json = energyml_core::json::write_value(&interpretation);

        // -- the sticks, written by the polyline writer ------------------------------
        let mut sticks_written = 0usize;
        if write_sticks && !written_sticks.is_empty() {
            // The frame the surface named: the compound created with it, or the
            // one named — so that the sticks create no second frame.
            let frame_params = match plan.objects.iter().find(|one| one.role == crate::crs::FRAME_ROLE) {
                Some(created) => FrameParams {
                    crs: Some(CrsRef(json!({
                        "$type": "eml23.DataObjectReference",
                        "Uuid": created.uuid,
                        "QualifiedType": created.qualified_type,
                        "Title": created.title,
                    }))),
                    source_epsg: None,
                    project: None,
                    vertical: None,
                },
                None => params.frame.clone(),
            };
            let sticks_written_as = Written {
                frame: frame_params,
                represented: RepresentedParams {
                    interpretation: Some(InterpretationRef(json!({
                        "$type": "eml23.DataObjectReference",
                        "Uuid": interpretation_uuid,
                        "QualifiedType": "resqml22.FaultInterpretation",
                        "Title": interpretation_title,
                    }))),
                },
                naming: NamingParams {
                    title: Some(crate::declare::Title(format!("{title} — sticks"))),
                    data_file: params.naming.data_file.clone(),
                },
                new_feature: NewFeatureParams {
                    feature_type: None,
                    interpretation_type: TypeName("resqml22.GenericFeatureInterpretation".to_string()),
                    feature_kind: None,
                },
            };
            let runs: Vec<u32> = written_sticks
                .iter()
                .map(|stick| stick.points.len() as u32)
                .collect();
            let lines = Shape {
                name: format!("{title} — sticks"),
                geometry: Geometry {
                    kind: "sticks",
                    patches: vec![Patch {
                        index: 0,
                        node_offset: 0,
                        points: written_sticks
                            .iter()
                            .flat_map(|stick| stick.points.iter().copied())
                            .collect(),
                        topology: Topology::Polylines(runs.clone()),
                        closed: vec![false; runs.len()],
                    }],
                },
                closed: None,
                known: None,
            };
            // **The uuids the surface took are not the sticks'**: each writer
            // counts its own from zero, and a list given by the caller would
            // otherwise name two objects with one uuid.
            let shifted = match cx.identity {
                Identity::Given(list) => {
                    Identity::Given(list.iter().skip(plan.objects.len()).cloned().collect())
                }
                other => other.clone(),
            };
            let sticks_cx = Context {
                identity: &shifted,
                when: cx.when,
                purpose: cx.purpose,
                feedback: cx.feedback,
                bag: cx.bag,
            };
            let sticks = super::polylines::write(
                &PolylinesInputs {
                    shape: vec![lines],
                    features: None,
                },
                &sticks_written_as,
                Some(false),
                &format!("{seed}:sticks"),
                &sticks_cx,
            )?;
            for mut object in sticks.objects {
                if object.role == "representation" {
                    // The sticks' `LineRole` `interpretation line` — *« Line
                    // corresponding to a digitalization along an earth model
                    // section »* (RESQML 2.2 Structural.xsd l. 55–58).
                    object.json["LineRole"] = json!("interpretation line");
                    object.role = "sticks".to_string();
                }
                plan.objects.push(object);
            }
            plan.datasets.extend(sticks.datasets);
            for mut shape in sticks.shapes {
                // Each writer calls its root `0`: the sticks are the plan's
                // second root, and an id is unique in a plan.
                shape.id = plan.shapes.len().to_string();
                plan.shapes.push(shape);
            }
            sticks_written = written_sticks.len();
        } else if write_sticks && in_package > 0 {
            says.push(Note {
                key: "sticks-in-package",
                detail: format!("the {in_package} stick(s) are objects of the package already: they are not written a second time"),
            });
        }

        // -- the mark and the sentence on every object written ----------------------
        for object in plan.objects.iter_mut() {
            if !matches!(object.role.as_str(), "feature" | "representation" | "sticks") {
                continue;
            }
            if let Some(text) = &description {
                object.json["Citation"]["Description"] = json!(text);
            }
            if params.synthetic {
                object.json["ExtensionNameValue"] =
                    json!([energyml_core::json::write_value(&synthetic_mark())]);
            }
        }

        // -- the surface, measured -----------------------------------------------------
        let measured: Option<Orientation> = match (frame.up, frame.unit.as_deref().and_then(Dimension::of)) {
            (_, Some(Dimension::Time)) => {
                says.push(Note {
                    key: "orientation-not-measured",
                    detail: "the frame counts its vertical axis in time: a dip is an angle between two lengths, and none is measured".to_string(),
                });
                None
            }
            (None, _) => {
                says.push(Note {
                    key: "orientation-not-measured",
                    detail: "the frame does not say which way its vertical axis points: which way the surface dips is not measured".to_string(),
                });
                None
            }
            (Some(up), _) => fault_sticks::orientation(&all_points, &all_triangles, up),
        };
        if let Some(found) = measured {
            let mut detail = format!(
                "measured on the surface: dip {:.1}° toward {:.1}°",
                found.dip, found.dip_azimuth
            );
            match (params.mean_dip, params.mean_azimuth) {
                (None, None) => detail.push_str("; nothing stated"),
                (dip, azimuth) => {
                    detail.push_str("; stated:");
                    if let Some(dip) = dip {
                        detail.push_str(&format!(" dip {dip}°"));
                    }
                    if let Some(azimuth) = azimuth {
                        detail.push_str(&format!(" toward {azimuth}°"));
                    }
                }
            }
            if found.dip > 89.9 {
                detail.push_str(" — a vertical surface dips either way, and its measured azimuth is one of two opposite directions");
            }
            says.push(Note {
                key: "orientation",
                detail,
            });
        }

        // -- what the plan says -----------------------------------------------------------
        if throws.is_empty() {
            says.push(Note {
                key: "no-throw",
                detail: "no throw is stated: the interpretation carries none".to_string(),
            });
        }
        if throws.contains(&Throw::Thrust) {
            says.push(Note {
                key: "thrust",
                detail: "a thrust: the type's annotation gives it « is Listric = 0, MaximumThrow = 0 », which the business rule of IsListric forbids for a fault that is not normal — the business rule is followed (Interpretations.xsd l. 659–662)".to_string(),
            });
        }
        if params.synthetic {
            says.push(Note {
                key: "synthetic",
                detail: "the fault is invented: its feature, its interpretation and what represents it carry `synthetic = true` and the source in their description".to_string(),
            });
        }
        if chrono.is_none() {
            says.push(Note {
                key: "not-dated",
                detail: "no `chronoBottom` and `chronoTop`: when the fault occurred is not written"
                    .to_string(),
            });
        }

        // -- the activity -------------------------------------------------------------------
        plan.activity.recipe = "fault-from-sticks";
        plan.activity.title = format!("fault-from-sticks → {title}");
        let number = |title: &str, value: f64, uom: Option<&str>| Parameter::Double {
            title: title.to_string(),
            value,
            uom: uom.map(str::to_string),
        };
        let sticks_count: usize = patches.iter().map(Vec::len).sum();
        plan.activity
            .parameters
            .push(number("stickCount", sticks_count as f64, None));
        plan.activity
            .parameters
            .push(number("patchCount", patches.len() as f64, None));
        plan.activity
            .parameters
            .push(number("writtenStickCount", sticks_written as f64, None));
        if let Some(found) = measured {
            plan.activity
                .parameters
                .push(number("measuredDip", found.dip, Some("dega")));
            plan.activity
                .parameters
                .push(number("measuredDipAzimuth", found.dip_azimuth, Some("dega")));
        }
        // What the registry cannot record — a list is no single value — and a
        // second run needs, in the order given.
        for one in &throws {
            plan.activity.parameters.push(Parameter::Text {
                title: "throw".to_string(),
                value: one.word().to_string(),
            });
        }
        if let GeometryParams::Sticks(given) = &params.geometry {
            for entry in &given.sticks {
                plan.activity.parameters.push(Parameter::Text {
                    title: "sticks".to_string(),
                    value: entry.clone(),
                });
            }
        }
        // What the frame said, after what the fault says.
        says.extend(std::mem::take(&mut plan.says));
        plan.says = says;
        Ok(plan)
    }

    /// The counts the writers record, and what this measures. `throw`,
    /// `sticks` and the stated numbers are inputs, recorded under the
    /// parameters the projection already declares — a title cannot be both.
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
        let angle = |name: &'static str, about: &'static str| {
            Slot::optional(
                name,
                Need::Number {
                    uom: Some("dega"),
                    least: None,
                    most: None,
                },
                about,
            )
        };
        support::frame_reports(vec![
            count("pointCount", "nodes of the surface"),
            count("triangleCount", "triangles of the surface"),
            count("stickCount", "sticks the surface is lofted between"),
            count("patchCount", "bands of the surface, one per patch"),
            count("writtenStickCount", "sticks written as a polyline set"),
            angle(
                "measuredDip",
                "the dip of the surface as lofted, area-weighted — when the frame says which way is up",
            ),
            angle(
                "measuredDipAzimuth",
                "the direction the lofted surface goes down, clockwise from grid north",
            ),
        ])
    }
}

/// Two sticks the same, bit for bit.
fn same(a: &[[f64; 3]], b: &[[f64; 3]]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(p, q)| p.iter().zip(q).all(|(x, y)| x.to_bits() == y.to_bits()))
}

/// What the frame the plan will write is — read by the frame's own reader
/// ([`support::frame_in`]), whose objects are **not** kept: the surface's
/// writer makes them. A frame in degrees is refused: a fault is a vertical
/// extent, and degrees against metres is what no reader reconciles (the
/// refusal `well-from-plan` makes). A frame created here does not reproject
/// what is given: the sticks are written as they are.
fn inspect(params: &FrameParams, cx: &Context<'_>) -> Result<Inspected, Refusal> {
    let mut scratch = 0usize;
    let frame = support::frame_in(&cx.known(), params, cx.identity, cx.when, &mut scratch)?;
    if frame.kind == Some(FrameKind::Geographic) {
        let (slot, what) = match params.crs.as_ref() {
            Some(_) => ("crs", "the frame named"),
            None => ("project", "the frame created without `project`"),
        };
        return Err(Refusal::on(
            slot,
            format!(
                "a fault is a vertical extent, and {what} measures its horizontal axes in degrees while its depths are metres — name a projected system in `project`, or a LocalEngineeringCompoundCrs as `crs`"
            ),
        ));
    }
    let created = !frame.objects.is_empty();
    if created
        && let (Some(source), Some(project)) = (params.source_epsg, params.project)
        && source != project
    {
        return Err(Refusal::on(
            "project",
            format!(
                "the sticks are written as they are given, never reprojected: with `sourceEpsg` {}, give `project` {} or leave it out",
                source.0, source.0
            ),
        ));
    }
    if created {
        // `crs::build` writes an identity compound over metres, counting up.
        return Ok(Inspected {
            up: Some(true),
            unit: Some("m".to_string()),
        });
    }
    let uuid = frame.crs["Uuid"].as_str().unwrap_or_default().to_string();
    let known = cx.known();
    let in_frame: Vec<&crate::pieces::Known> = known
        .iter()
        .copied()
        .filter(|one| {
            one.crs
                .as_ref()
                .is_some_and(|crs| crs.uuid.eq_ignore_ascii_case(&uuid))
        })
        .collect();
    Ok(Inspected {
        up: in_frame.iter().find_map(|one| one.vertical_up),
        unit: in_frame.iter().find_map(|one| one.vertical_uom.clone()),
    })
}
