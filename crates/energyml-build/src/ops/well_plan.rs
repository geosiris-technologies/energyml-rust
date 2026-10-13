//! `well-from-plan` - a wellbore from a drilling plan: straight holds and
//! circular arcs from a head, or from a parent's kick-off for a sidetrack; the
//! stations surveyed by the minimum curvature method, the datum, a trajectory
//! through the stations, a frame on it, and the feature and interpretation.
//!
//! Step 6 of the build plan of the synthetic model Lutetia:
//! `well-from-plan` (a trajectory from a drilling plan: kick-off, build rate, hold, resumption, drain;
//! lateral), the heads read on the ground surface - the geothermal doublet GTH-1…GTH-3
//! and the planned lateral GTH-3 ST1.
//!
//! # What is decided here, and how honestly
//!
//! - **the survey is `energyml_analysis::well_plan`**, pure and tested against
//!   the textbook minimum curvature formula; this module only places its
//!   stations in the frame. A calculation lives in the analysis crate, never in
//!   an operation (`crates/energyml-build/ARCHITECTURE.md`);
//! - **the measured depths written are the plan's, never recomputed**: they go
//!   to the writer as `Wellbore::mds`, so a `hold to 120` is written `120`, and
//!   the MDs along an arc are arc lengths - the chord between two stations is
//!   shorter, and deriving the depths from it (`mds: None`) would write
//!   depths the plan never said (a given MD is never recomputed);
//! - **one writer**: the datum is `well::datum_in`, the trajectory and the frame
//!   `well::write` - an operation that writes the same object as another
//!   delegates to it. It writes RESQML's **minimum-curvature line** (`LineKindIndex`
//!   5, which the reference-model description wants for GTH-1): the stations, their measured
//!   depths as parameters, and each station's direction as its tangent
//!   (`Wellbore::tangents`, 24/09 - before it, a linear spline through the
//!   stations, whose chords strayed from the arcs). A vertical well keeps
//!   RESQML's vertical reading, or a linear spline when its depths are not the
//!   system's;
//! - **the feature and interpretation are the ones `well-from-polyline`
//!   writes** - `pair` is the same two objects, word for word, because
//!   `well.rs` keeps them inline and this file may not open it. A copy, named:
//!   extracting `well::pair_in` from `WellFromPolyline::plan` removes it.
//!   `IsDrilled` is false, and here that is the schema's own answer: *“For
//!   planned trajectories, use IsDrilled=FALSE”* (RESQML 2.2 `Wells.xsd`,
//!   `WellboreInterpretation.IsDrilled`);
//! - **the vertical axis is read, never guessed**: a frame created here counts
//!   up (every `VerticalCrs` `crs::build` writes is an elevation), a frame of
//!   the package says it through what the bag knows of it; a frame that does
//!   not say is refused - which way *down* is decides every station's z;
//! - **a well in degrees is refused**, as `well-from-polyline` refuses it: its
//!   depths are metres beside degrees;
//! - **the head's ground is given or read**, never both: `ground`, or a
//!   `surface` - a triangulated surface or a grid in the well's own frame -
//!   read under the head by `energyml_analysis::heights`, the one reading of a
//!   height under a point (`project`, `thickness`). A surface in another frame
//!   is refused by name, the rule `project` states: *a height read in one frame
//!   and written in another is a height from somewhere else*;
//! - **the depth limit is checked, never bent to**: `maxDepth` refuses a plan
//!   that goes deeper below the ground at the head, naming the first station
//!   that does (the reference-model rule: vertical depth ≤ 200 m below the ground at the
//!   head at every point). It is checked at the stations; between two
//!   of them, an arc that passes the horizontal may go deeper by at most the
//!   sag the plan reports.
//!
//! # A sidetrack
//!
//! Given a `parent` - a wellbore frame of the package - and `kickoffMd`, the
//! plan starts on the parent: at the point its nodes place at that depth, in
//! the direction it has there, and its measured depths **continue the
//! parent's**, from the parent's datum (RESQML 2.2 `Wells.xsd`,
//! `WellboreTrajectoryParentIntersection.KickoffMd`: *“the measured depth for
//! the start of the child trajectory, as defined within the child”*; the
//! parent's MD is left out, *“If not specified, then these two MD's are
//! implied to be identical”*). Two things are refused rather than guessed:
//!
//! - **a kick-off on a bend of the parent.** Its direction is read on the
//!   chord between the two nodes around the kick-off, which is its direction
//!   only when the parent is straight there - when the chord is as long as the
//!   hole between the two nodes. A curved stretch is refused by name;
//! - **a parent that says no datum or no trajectory**: the first is what the
//!   depths are counted from, the second what `ParentIntersection` names.
//!
//! `ParentIntersection` is written onto the trajectory `well::write` returns -
//! one field it does not know, set by name. `Wellbore` gaining a `parent` is
//! the proper home, and it is named for whoever opens `well.rs`.
//!
//! # References
//!
//! - API Bulletin D20, *Directional Drilling Survey Calculation Methods and Terminology*, 1985; and Sawaryn & Thorogood, *A Compendium of Directional Calculations Based on the Minimum Curvature Method* (SPE 84246, SPE Drilling & Completion 20(1), 2005), as cited in `energyml-analysis` (`well_plan`, `survey`).
//!   TODO(reference): verify - these citations are carried over from `energyml-analysis`, whose own copy of them is pending verification.

use serde_json::{Value, json};

use energyml_analysis::heights::{Height, Heights};
use energyml_analysis::well_plan::{self, PlanError, Section, Station, Tie, Turn};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::Topology;

use crate::crs::FrameKind;
use crate::declare::{Inputs, Params, Scalar, Shape as ShapeIn, Suggestions};
use crate::geojson::Position;
use crate::operation::{
    About, ActivityDraft, Category, Context, Error, Identity, Need, NewObject, Note, Operation, Plan,
    Refusal, Slot, When,
};
use crate::pieces::{Known, Pieces, Ref};

use super::support;
use super::well::{self, Datum, DatumKind, TrajectoryShape, Wellbore};
use super::written::WrittenImposed;

/// The operation.
pub struct WellFromPlan;

/// The forms a section is written in, for a refusal.
const FORMS: &str =
    "`hold <length>`, `hold to <md>`, or `build|drop|curve <rate °/30 m> <inclination °> <azimuth °>`";

/// A parent is straight between two nodes when the chord joining them is as
/// long as the hole between them, to this fraction. A 10 m stretch turning
/// 0.3° falls short by about one millionth; the arcs of a plan written here,
/// 5° per 10 m station at 15°/30 m, by three ten-thousandths.
const STRAIGHT: f64 = 1e-6;

/// One section of the plan, as it is written: `hold 40`, `hold to 120`,
/// `build 15 90 270`.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanSection {
    /// The text, its words as given - what the activity records, and what a
    /// second run reads back.
    pub text: String,
    /// What it says.
    pub section: Section,
}

impl PlanSection {
    /// A section from its text - the verb, then its numbers:
    ///
    /// - `hold <length>`: straight on for that many metres of MD;
    /// - `hold to <md>`: straight on down to that MD, kept as written;
    /// - `build`, `drop` or `curve`, then `<rate> <inclination> <azimuth>`: one
    ///   arc at `rate` degrees per 30 m until the hole points at that
    ///   inclination (degrees from vertical down) and azimuth (degrees
    ///   clockwise from the frame's +y axis - grid north in an identity
    ///   compound). A build must raise the inclination and a drop lower it.
    ///
    /// Only what reads is refused here; what reads and cannot be drilled - a
    /// negative length, an inclination of 190° - is refused by the survey, by
    /// the section's number.
    pub fn parse(text: &str) -> Result<PlanSection, String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        let number = |word: &str, what: &str| -> Result<f64, String> {
            word.parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .ok_or_else(|| format!("`{text}`: {what} `{word}` is not a number"))
        };
        let section = match *words.as_slice() {
            [verb, to, md] if verb.eq_ignore_ascii_case("hold") && to.eq_ignore_ascii_case("to") => {
                Section::HoldTo {
                    md: number(md, "the measured depth")?,
                }
            }
            [verb, length] if verb.eq_ignore_ascii_case("hold") => Section::Hold {
                length: number(length, "the length")?,
            },
            [verb, rate, inclination, azimuth] => {
                let turn = match verb.to_ascii_lowercase().as_str() {
                    "build" => Turn::Build,
                    "drop" => Turn::Drop,
                    "curve" => Turn::Any,
                    _ => return Err(format!("`{text}` is none of {FORMS}")),
                };
                Section::Curve {
                    rate: number(rate, "the rate")?,
                    inclination: number(inclination, "the inclination")?,
                    azimuth: number(azimuth, "the azimuth")?,
                    turn,
                }
            }
            _ => return Err(format!("`{text}` is none of {FORMS}")),
        };
        Ok(PlanSection {
            text: words.join(" "),
            section,
        })
    }
}

impl Scalar for PlanSection {
    fn need() -> Need {
        Need::Text
    }
    fn from_json(value: &Value) -> Result<Self, String> {
        match value {
            Value::String(text) => PlanSection::parse(text),
            other => Err(format!("expected a section as text - {FORMS} - got {other}")),
        }
    }
    fn to_json(&self) -> Value {
        Value::from(self.text.as_str())
    }
}

/// What `well-from-plan` is told.
///
/// **Its own pair, not the shared one** - the block is [`WrittenImposed`], as
/// for `well-from-polyline`: an interpretation of a wellbore carries `Domain`
/// and `IsDrilled`, which the shared writer does not write.
#[derive(Debug, Params)]
pub struct WellPlanParams {
    /// The frame, the interpretation when named, the naming.
    #[param(flatten)]
    pub written: WrittenImposed,
    /// the head's first coordinate - in the system of `sourceEpsg`, or in the frame `crs` (easting in an identity compound); left out for a sidetrack, whose head is its parent's
    pub head_x: Option<f64>,
    /// the head's second coordinate - northing in an identity compound; left out for a sidetrack
    pub head_y: Option<f64>,
    /// the vertical coordinate of the ground at the head, in the frame's axis; left out, it is read on `surface` under the head
    pub ground: Option<f64>,
    /// how far above the ground the datum is, in metres - a kelly bushing's height, say; the measured depths start there
    ///
    /// A sidetrack has none: its depths are its parent's, from the parent's
    /// datum.
    #[param(default = 0.0, optional, min = 0.0, uom = "m", group = "frame")]
    pub datum_height: f64,
    /// what the created datum is
    ///
    /// `well-from-polyline`'s list. EML 2.3 also has `rotary table`
    /// (`ReferencePointKind`), which the reference-model description names for the doublet;
    /// it enters here when `well::DatumKind` gains it.
    #[param(default = DatumKind::GroundLevel, optional, group = "frame")]
    pub datum_kind: DatumKind,
    /// the plan, section by section down the hole: `hold 40` (straight on for 40 m), `hold to 120` (straight on to MD 120), `build 15 90 270` (an arc at 15°/30 m to inclination 90° and azimuth 270°), `drop …` (the same, lowering the inclination), `curve …` (any turn); lengths in m, azimuths clockwise from the frame's north
    #[param(least = 1, ordered)]
    pub sections: Vec<PlanSection>,
    /// a station every so many metres of measured depth, on its multiples - each section's end is a station too; the stations are the trajectory's knots and the frame's nodes
    #[param(default = 10.0, optional, min = 0.0, uom = "m")]
    pub step: f64,
    /// the deepest the hole may go, in metres of vertical depth below the ground at its head - checked at every station, never used to bend the plan
    #[param(min = 0.0, uom = "m")]
    pub max_depth: Option<f64>,
    /// with a parent: the measured depth on the parent where this sidetrack leaves it
    #[param(min = 0.0, uom = "m")]
    pub kickoff_md: Option<f64>,
}

/// What `well-from-plan` takes - both optional: a new well from a head needs
/// neither when `ground` is given.
#[derive(Debug, Inputs)]
pub struct WellPlanInputs {
    /// the ground under the head - a surface's triangles or a grid's cells, in the well's frame; its height under the head is the ground
    #[input(topology = "triangles")]
    pub surface: Option<ShapeIn>,
    /// the wellbore frame a sidetrack leaves - its nodes, their measured depths, its trajectory and its datum
    #[input(topology = "polylines", types = "resqml22.WellboreFrameRepresentation")]
    pub parent: Option<ShapeIn>,
}

/// The frame a sidetrack leaves, as its piece carries it.
struct Parent<'a> {
    name: &'a str,
    /// Its nodes, in the order of its measured depths.
    points: Vec<[f64; 3]>,
    /// The measured depth of each node, as the package states them.
    mds: &'a [f64],
    trajectory: &'a Ref,
    datum: &'a Ref,
    crs: Option<&'a Ref>,
}

/// The parent a piece stands for, or why it is none.
fn parent_of(shape: &ShapeIn) -> Result<Parent<'_>, String> {
    let name = shape.name.as_str();
    let known = shape.known.as_ref().ok_or_else(|| {
        format!("`{name}` comes with nothing known of it: a sidetrack leaves a wellbore frame of the package, whose measured depths, trajectory and datum it continues")
    })?;
    let points: Vec<[f64; 3]> = shape
        .geometry
        .patches
        .iter()
        .flat_map(|patch| patch.points.iter().copied())
        .collect();
    let mds = known.node_mds.as_deref().ok_or_else(|| {
        format!("`{name}` states no measured depth for its nodes - it is no wellbore frame, or this build cannot read its arrays")
    })?;
    if mds.len() != points.len() {
        return Err(format!(
            "`{name}` has {} node(s) and {} measured depth(s): each node is where its depth is",
            points.len(),
            mds.len()
        ));
    }
    if mds.len() < 2 {
        return Err(format!(
            "`{name}` has {} node(s): a direction is read between two",
            mds.len()
        ));
    }
    if let Some(at) = mds.windows(2).position(|pair| pair[1] <= pair[0]) {
        return Err(format!(
            "the measured depths of `{name}` do not increase at node {}: {} then {}",
            at + 1,
            mds[at],
            mds[at + 1]
        ));
    }
    let trajectory = known.trajectory.as_ref().ok_or_else(|| {
        format!("`{name}` names no trajectory: a sidetrack's `ParentIntersection` names the parent trajectory (RESQML 2.2 Wells.xsd, 1..1)")
    })?;
    let datum = known.datum.as_ref().ok_or_else(|| {
        format!("`{name}` names no datum through its trajectory: a sidetrack's measured depths are counted from its parent's")
    })?;
    Ok(Parent {
        name,
        points,
        mds,
        trajectory,
        datum,
        crs: known.crs.as_ref(),
    })
}

/// Where a sidetrack leaves its parent.
struct Kickoff {
    /// The point, in the frame.
    at: [f64; 3],
    /// The measured depth and the direction there.
    tie: Tie,
    /// The parent's nodes around it.
    between: (f64, f64),
}

/// Where a sidetrack leaves its parent, and which way the parent points there -
/// read on the parent's nodes, and **only on a straight stretch**.
fn kickoff(parent: &Parent<'_>, md: f64, up: bool) -> Result<Kickoff, String> {
    let (first, last) = (parent.mds[0], parent.mds[parent.mds.len() - 1]);
    if !(first..=last).contains(&md) {
        return Err(format!(
            "MD {md} is not on `{}`, whose nodes run from MD {first} to MD {last}",
            parent.name
        ));
    }
    // The stretch below the kick-off - the last one when it is the last node.
    let i = parent
        .mds
        .windows(2)
        .position(|pair| md < pair[1])
        .unwrap_or(parent.mds.len() - 2);
    let (m0, m1) = (parent.mds[i], parent.mds[i + 1]);
    let (p0, p1) = (parent.points[i], parent.points[i + 1]);
    let d = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
    let chord = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let hole = m1 - m0;
    if !chord.is_finite() || (hole - chord).abs() > STRAIGHT * hole {
        return Err(format!(
            "`{}` is not straight between its nodes at MD {m0} and MD {m1} - {chord} apart for {hole} of hole - so its direction at MD {md} is not known from its nodes: kick off on a straight stretch",
            parent.name
        ));
    }
    let f = (md - m0) / hole;
    let at = [p0[0] + f * d[0], p0[1] + f * d[1], p0[2] + f * d[2]];
    // North is +y and east +x of the frame; down is −z in a frame that counts
    // up, +z in one that counts down.
    let down = if up { -d[2] / chord } else { d[2] / chord };
    let (inclination, azimuth) = well_plan::angles([d[1] / chord, d[0] / chord, down], 0.0);
    Ok(Kickoff {
        at,
        tie: Tie {
            md,
            inclination,
            azimuth,
        },
        between: (m0, m1),
    })
}

/// The ground under `at`, read on `surface` - refused when the surface is in
/// another frame, or counts another unit, or has nothing under `at`.
fn ground_under(
    surface: &ShapeIn,
    at: [f64; 2],
    frame_uuid: &str,
    frame_title: &str,
) -> Result<f64, Refusal> {
    // One frame, one unit - the rule of `project`.
    if let Some(crs) = surface.known.as_ref().and_then(|known| known.crs.as_ref())
        && !crs.uuid.eq_ignore_ascii_case(frame_uuid)
    {
        return Err(Refusal::on(
            "surface",
            format!(
                "`{}` is in `{}` and the well in `{frame_title}` - a height read in one frame and written in another is a height from somewhere else: give a surface in the well's frame, or `ground`",
                surface.name, crs.title
            ),
        ));
    }
    if let Some(uom) = surface
        .known
        .as_ref()
        .and_then(|known| known.vertical_uom.as_deref())
        && uom != "m"
    {
        return Err(Refusal::on(
            "surface",
            format!(
                "`{}` counts its heights in `{uom}`, and a plan is in metres",
                surface.name
            ),
        ));
    }
    let faces = support::Faces::of(&surface.geometry);
    let heights = Heights::of(faces.surface(), "surface", "under the wellhead")
        .map_err(|why| Refusal::on("surface", why))?;
    match heights.height(at[0], at[1]) {
        Height::At(z) => Ok(z),
        Height::Outside => Err(Refusal::on(
            "surface",
            format!(
                "the head ({}, {}) is outside `{}`: no face lies under it - give `ground`, or a surface that covers the head",
                at[0], at[1], surface.name
            ),
        )),
        Height::Folded => Err(Refusal::on(
            "surface",
            format!(
                "`{}` folds under the head ({}, {}): several faces disagree on its height there - give `ground`",
                surface.name, at[0], at[1]
            ),
        )),
    }
}

/// The wellbore feature and its interpretation - **the two objects
/// `well-from-polyline` writes, word for word** (`ops/well.rs`, in
/// `WellFromPolyline::plan`), under the same seeds' shape.
///
/// A copy, and named as one: `well.rs` keeps them inline, and extracting a
/// `well::pair_in` that both call is the change that removes this. A third
/// writer would not be justified (`crates/energyml-build/ARCHITECTURE.md`).
fn pair(
    title: &str,
    seed: &str,
    when: &When,
    identity: &Identity,
    rank: &mut usize,
) -> Result<(Value, Vec<NewObject>), Error> {
    let feature_uuid = support::next_uuid(identity, &format!("well-feature:{seed}"), rank)?;
    let feature_title = format!("{title} - feature");
    let feature = NewObject {
        uuid: feature_uuid.clone(),
        qualified_type: "resqml22.WellboreFeature".to_string(),
        role: "feature".to_string(),
        title: feature_title.clone(),
        json: json!({
            "$type": "resqml22.WellboreFeature",
            "uuid": feature_uuid,
            "schemaVersion": "2.2",
            "Citation": support::citation(when, &feature_title),
            "IsWellKnown": false,
        }),
    };
    let uuid = support::next_uuid(identity, &format!("well-interpretation:{seed}"), rank)?;
    let interpretation_title = format!("{title} - interpretation");
    let interpretation = NewObject {
        uuid: uuid.clone(),
        qualified_type: "resqml22.WellboreInterpretation".to_string(),
        role: "interpretation".to_string(),
        title: interpretation_title.clone(),
        json: json!({
            "$type": "resqml22.WellboreInterpretation",
            "uuid": uuid,
            "schemaVersion": "2.2",
            "Citation": support::citation(when, &interpretation_title),
            "Domain": "depth",
            // “For planned trajectories, use IsDrilled=FALSE” (Wells.xsd).
            "IsDrilled": false,
            "InterpretedFeature": {
                "$type": "eml23.DataObjectReference",
                "Uuid": feature_uuid,
                "QualifiedType": "resqml22.WellboreFeature",
                "Title": feature_title,
            },
        }),
    };
    Ok((
        json!({
            "$type": "eml23.DataObjectReference",
            "Uuid": uuid,
            "QualifiedType": "resqml22.WellboreInterpretation",
            "Title": interpretation_title,
        }),
        vec![feature, interpretation],
    ))
}

impl Operation for WellFromPlan {
    type Params = WellPlanParams;
    type Inputs = WellPlanInputs;

    fn about(&self) -> About {
        About {
            name: "well-from-plan",
            describe: "a wellbore from a drilling plan - holds and minimum-curvature arcs from a head, or from a parent's kick-off: the stations, the datum, a trajectory through them, a frame on it - and the feature and interpretation when none is named",
            category: Category::Wells,
            makes: &[
                "eml23.ReferencePointInALocalEngineeringCompoundCrs",
                "eml23.ReferencePointInACrs",
                "resqml22.WellboreFeature",
                "resqml22.WellboreInterpretation",
                "resqml22.WellboreTrajectoryRepresentation",
                "resqml22.WellboreFrameRepresentation",
            ],
            recipe: "well-from-plan",
            represents: Some(("resqml22.WellboreFeature", "resqml22.WellboreInterpretation")),
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, self.about().represents, slots);
        // **A well is named by whoever plans it.** The shared suggestion takes
        // the first named piece - here the ground surface, the parent's frame
        // or the CRS just chosen - and a well titled `surface` or
        // `EPSG:2154 + EPSG:5720` is a title nobody gave.
        if let Some(slot) = slots.slot_mut("title") {
            slot.suggested = None;
            slot.from = None;
        }
        // **Nor does a well take its inputs' interpretation.** The one the
        // bag names is the ground surface's - a horizon's - or the parent's,
        // and a lateral is a wellbore of its own
        // (see the reference model). Left empty, the slot creates the pair, as it says
        // (`a_well_makes_its_own_interpretation_never_the_grounds`).
        if let Some(slot) = slots.slot_mut("interpretation") {
            slot.suggested = None;
            slot.from = None;
        }
    }

    fn check(&self, inputs: &WellPlanInputs) -> Result<(), Refusal> {
        if let Some(surface) = &inputs.surface
            && !support::Faces::of(&surface.geometry).any()
        {
            return Err(Refusal::on(
                "surface",
                format!(
                    "`{}` has no face - the ground is read on triangles or a grid's cells, and points or lines have none",
                    surface.name
                ),
            ));
        }
        if let Some(parent) = &inputs.parent {
            parent_of(parent).map_err(|why| Refusal::on("parent", why))?;
        }
        Ok(())
    }

    fn plan(
        &self,
        inputs: WellPlanInputs,
        params: WellPlanParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let written = &params.written;
        let title = written.naming.title();
        let identity = cx.identity;
        let when = cx.when;
        let known: Vec<&Known> = cx.known();
        let mut rank = 0usize;

        // -- a new well at a head, or a sidetrack on a parent ---------------
        let parent = match &inputs.parent {
            Some(shape) => Some(parent_of(shape).map_err(|why| Refusal::on("parent", why))?),
            None => None,
        };
        match (&parent, params.kickoff_md) {
            (Some(one), None) => {
                return Err(Refusal::on(
                    "kickoffMd",
                    format!("a sidetrack leaves `{}` at a measured depth: give it", one.name),
                ));
            }
            (None, Some(_)) => {
                return Err(Refusal::on(
                    "kickoffMd",
                    "a kick-off, and no parent to kick off from: give the wellbore frame this sidetrack leaves as `parent`",
                ));
            }
            _ => {}
        }
        if parent.is_some() {
            if params.head_x.is_some() || params.head_y.is_some() {
                let slot = if params.head_x.is_some() { "headX" } else { "headY" };
                return Err(Refusal::on(
                    slot,
                    "a sidetrack starts on its parent, and its head is its parent's: leave the head out",
                ));
            }
            if params.datum_height != 0.0 {
                return Err(Refusal::on(
                    "datumHeight",
                    "a sidetrack measures from its parent's datum, and creates none: leave its height out",
                ));
            }
        } else if params.datum_height > 0.0 && params.datum_kind == DatumKind::GroundLevel {
            return Err(Refusal::on(
                "datumKind",
                format!(
                    "a ground-level datum is at the ground, and this one is {} m above it - say what it is: a kelly bushing, a derrick floor",
                    params.datum_height
                ),
            ));
        }
        if params.ground.is_some() && inputs.surface.is_some() {
            return Err(Refusal::on(
                "ground",
                "`ground` and `surface` are two answers to the same question: give one",
            ));
        }
        if parent.is_some()
            && params.max_depth.is_none()
            && (params.ground.is_some() || inputs.surface.is_some())
        {
            let slot = if params.ground.is_some() {
                "ground"
            } else {
                "surface"
            };
            return Err(Refusal::on(
                slot,
                "a sidetrack's datum is its parent's, so the ground serves only `maxDepth`, which is not given - an answer that changes nothing is a misreading somewhere",
            ));
        }

        // -- the frame, and which way is down --------------------------------
        let frame = support::frame_in(&known, &written.frame, identity, when, &mut rank)?;
        let frame_title = frame.crs["Title"].as_str().unwrap_or_default().to_string();
        if frame.kind == Some(FrameKind::Geographic) {
            // The refusal `well-from-polyline` makes, for the same reason:
            // a wellbore is a vertical extent.
            let (slot, what) = match written.frame.crs.as_ref() {
                Some(_) => ("crs", format!("the frame `{frame_title}` named")),
                None => ("project", "the frame created without `project`".to_string()),
            };
            return Err(Refusal::on(
                slot,
                format!(
                    "a wellbore is a vertical extent, and {what} measures its horizontal axes in degrees while its depths are metres, which no reader can reconcile - name a projected system in `project` (EPSG:2154 for mainland France), or a LocalEngineeringCompoundCrs as `crs`"
                ),
            ));
        }
        let frame_uuid = frame.crs["Uuid"].as_str().unwrap_or_default().to_string();
        let in_frame = |one: &&Known| {
            one.crs
                .as_ref()
                .is_some_and(|crs| crs.uuid.eq_ignore_ascii_case(&frame_uuid))
        };
        let created = !frame.objects.is_empty();
        // A frame created here counts up: `crs::build` writes every vertical
        // system as an elevation, and the identity compound follows it.
        let up = if created {
            Some(true)
        } else {
            known
                .iter()
                .copied()
                .filter(in_frame)
                .find_map(|one| one.vertical_up)
        };
        let Some(up) = up else {
            return Err(match written.frame.crs {
                None => Refusal::on(
                    "crs",
                    "a planned well needs its frame before any station is placed - which way its vertical axis points decides every z: name a LocalEngineeringCompoundCrs as `crs`, or give `sourceEpsg` and `project`",
                ),
                Some(_) => Refusal::on(
                    "crs",
                    format!(
                        "the frame `{frame_title}` does not say which way its vertical axis points, and which way down is decides every station's z: it cannot be guessed"
                    ),
                ),
            });
        };
        if !created
            && let Some(uom) = known
                .iter()
                .copied()
                .filter(in_frame)
                .find_map(|one| one.vertical_uom.as_deref())
            && uom != "m"
        {
            return Err(Refusal::on(
                "crs",
                format!(
                    "the frame `{frame_title}` counts its vertical axis in `{uom}`, and a plan is in metres - lengths in m, rates in °/30 m"
                ),
            ));
        }
        if let Some(one) = &parent
            && let Some(crs) = one.crs
            && !crs.uuid.eq_ignore_ascii_case(&frame_uuid)
        {
            return Err(Refusal::on(
                "parent",
                format!(
                    "`{}` is in `{}` and the sidetrack in `{frame_title}`: a sidetrack starts where its parent is, in its parent's frame",
                    one.name, crs.title
                ),
            ));
        }

        // -- the head, and the ground under it -------------------------------
        let head: Option<[f64; 2]> = match (&parent, params.head_x, params.head_y) {
            (Some(_), _, _) => None,
            (None, Some(x), Some(y)) => Some(match &frame.place {
                // Given in the system of `sourceEpsg`, placed into the frame.
                Some(_) => {
                    let placed = support::placed(
                        &[Position {
                            lon: x,
                            lat: y,
                            z: Some(0.0),
                        }],
                        &frame,
                    )?;
                    [placed[0][0], placed[0][1]]
                }
                None => [x, y],
            }),
            (None, x, _) => {
                let slot = if x.is_none() { "headX" } else { "headY" };
                return Err(Refusal::on(
                    slot,
                    "a new well stands somewhere: give its head, `headX` and `headY`",
                ));
            }
        };
        let mut ground_read: Option<(f64, String)> = None;
        let ground: Option<f64> = match (params.ground, &inputs.surface) {
            (Some(z), _) => Some(z),
            (None, Some(surface)) => {
                let at = match (&parent, head) {
                    (None, Some(xy)) => xy,
                    (Some(one), _) => match one.mds.iter().position(|md| *md == 0.0) {
                        Some(i) => [one.points[i][0], one.points[i][1]],
                        None => {
                            return Err(Refusal::on(
                                "surface",
                                format!(
                                    "the head of `{}` is not among its nodes - its frame starts at MD {}, not at its datum - so the ground cannot be read under it: give `ground`",
                                    one.name, one.mds[0]
                                ),
                            ));
                        }
                    },
                    (None, None) => {
                        return Err(Refusal::on("headX", "a new well stands somewhere: give its head"));
                    }
                };
                let z = ground_under(surface, at, &frame_uuid, &frame_title)?;
                ground_read = Some((z, surface.name.clone()));
                Some(z)
            }
            (None, None) => None,
        };

        // -- where the plan starts ------------------------------------------
        let (tie, origin, kicked) = match (&parent, head) {
            (None, Some(xy)) => {
                let Some(ground) = ground else {
                    return Err(Refusal::on(
                        "ground",
                        "a new well's measured depths start at its datum, above its ground: give `ground`, or a `surface` to read it on under the head",
                    ));
                };
                let datum_z = if up {
                    ground + params.datum_height
                } else {
                    ground - params.datum_height
                };
                (Tie::vertical(), [xy[0], xy[1], datum_z], None)
            }
            (Some(one), _) => {
                let md = params.kickoff_md.unwrap_or_default();
                let found = kickoff(one, md, up).map_err(|why| Refusal::on("kickoffMd", why))?;
                (found.tie, found.at, Some(found))
            }
            (None, None) => return Err(Refusal::on("headX", "a new well stands somewhere: give its head")),
        };

        // -- the survey -------------------------------------------------------
        let sections: Vec<Section> = params.sections.iter().map(|one| one.section).collect();
        let planned = well_plan::plan(tie, &sections, params.step).map_err(|why| match why.section() {
            Some(number) => {
                let text = params
                    .sections
                    .get(number - 1)
                    .map(|one| one.text.as_str())
                    .unwrap_or_default();
                Refusal::on("sections", format!("{why} - `{text}`"))
            }
            None if matches!(why, PlanError::BadStep { .. } | PlanError::TooManyStations { .. }) => {
                Refusal::on("step", why)
            }
            None => Refusal::on("sections", why),
        })?;
        let place = |station: &Station| -> [f64; 3] {
            let z = if up {
                origin[2] - station.tvd
            } else {
                origin[2] + station.tvd
            };
            [origin[0] + station.east, origin[1] + station.north, z]
        };
        let line: Vec<[f64; 3]> = planned.stations.iter().map(place).collect();
        // The direction at each station, in the frame's axes - the exact
        // tangents of the minimum-curvature arcs the stations lie on, which
        // the writer turns into RESQML's `LineKindIndex` 5.
        let tangents: Vec<[f64; 3]> = planned
            .stations
            .iter()
            .map(|station| {
                let d = well_plan::direction(station.inclination, station.azimuth);
                [d[1], d[0], if up { -d[2] } else { d[2] }]
            })
            .collect();
        let mds: Vec<f64> = planned.stations.iter().map(|station| station.md).collect();
        let last = planned.stations[planned.stations.len() - 1];

        // -- the depth limit: checked, never bent to ------------------------
        let below = |z: f64, ground: f64| if up { ground - z } else { z - ground };
        let depth_max = ground.map(|g| {
            line.iter()
                .map(|p| below(p[2], g))
                .fold(f64::NEG_INFINITY, f64::max)
        });
        let mut says = Vec::new();
        if let Some(most) = params.max_depth {
            let Some(g) = ground else {
                return Err(Refusal::on(
                    "maxDepth",
                    "is measured from the ground at the head: give `ground`, or a `surface` to read it on",
                ));
            };
            let slack = 1e-9 * most.max(1.0);
            if let Some((station, point)) = planned
                .stations
                .iter()
                .zip(&line)
                .find(|(_, p)| below(p[2], g) > most + slack)
            {
                return Err(Refusal::on(
                    "maxDepth",
                    format!(
                        "the plan reaches {} m below the ground at MD {} - deeper than the {most} m allowed; a plan is checked against the limit, never bent to it",
                        below(point[2], g),
                        station.md
                    ),
                ));
            }
            says.push(Note {
                key: "max-depth",
                detail: format!(
                    "{} m below the ground at its deepest station, within the {most} m allowed - checked at the stations",
                    depth_max.unwrap_or_default()
                ),
            });
        }

        // -- identity: the well by its name, the datum by its place, the
        // trajectory and the frame by their depths --------------------------
        let plan_text = params
            .sections
            .iter()
            .map(|one| one.text.as_str())
            .collect::<Vec<_>>()
            .join("|");
        let parent_seed = match (&parent, &kicked) {
            (Some(one), Some(found)) => format!("{}@{:?}", one.trajectory.uuid, found.tie.md),
            _ => String::new(),
        };
        // Every number to the millimetre (`support::seed_mm`): the ground under
        // the head is read on a DTM, so its last bit is the platform's.
        let placed = origin.map(super::support::seed_mm);
        let pair_seed = format!("well-from-plan:{title}");
        let datum_seed = format!(
            "well-from-plan:{title}:{frame_uuid}:{placed:?}:{}",
            params.datum_kind.word()
        );
        let line_seed = format!(
            "well-from-plan:{title}:{frame_uuid}:{placed:?}:{plan_text}:{:?}:{parent_seed}",
            params.step
        );

        // -- the datum: made at the head, or the parent's -------------------
        let cite = |name: &str| support::citation(when, name);
        let parent_datum: Option<Value> = parent.as_ref().map(|one| one.datum.dor());
        let (datum, datum_objects) = well::datum_in(
            &Datum {
                named: parent_datum.as_ref(),
                head: [origin[0], origin[1]],
                ground: origin[2],
                kind: params.datum_kind,
                crs: &frame.crs,
                vertical_crs: frame.vertical.as_ref(),
                title: &title,
                seed: &datum_seed,
                citation: &cite,
            },
            identity,
            &mut rank,
        )?;

        // -- the feature and its interpretation, when none is named ---------
        let (interpretation, pair_objects) =
            match written.represented.interpretation.as_ref().map(|one| &one.0) {
                Some(value) if value.is_object() && value["Uuid"].is_string() => (value.clone(), Vec::new()),
                Some(other) => {
                    return Err(Refusal::on(
                        "interpretation",
                        format!("expected a DataObjectReference, got {other}"),
                    ));
                }
                None => pair(&title, &pair_seed, when, identity, &mut rank)?,
            };
        let pair_made = !pair_objects.is_empty();

        // -- the trajectory and the frame, by the shared writer -------------
        let file = written.naming.data_file.clone();
        let mut drawn = well::write(
            &Wellbore {
                title: &title,
                seed: &line_seed,
                keep: None,
                line: &line,
                // **The plan's depths, never recomputed** from the chords.
                mds: Some(&mds),
                // Where the measured depths start: the datum of a new well.
                // A sidetrack's start at its kick-off - the writer reads it
                // only to say whether the ground lies below the first point,
                // which is no question for a line that starts down the hole.
                ground: origin[2],
                uom: "m",
                shape: TrajectoryShape::Linear,
                tangents: Some(&tangents),
                spacing: None,
                datum: &datum,
                interpretation: &interpretation,
                file: &file,
                vertical_up: Some(up),
                crs: &frame.crs,
                citation: &cite,
            },
            identity,
            &mut rank,
        )?;
        if let (Some(one), Some(found)) = (&parent, &kicked) {
            // The one field the shared writer does not know (RESQML 2.2
            // Wells.xsd, `WellboreTrajectoryRepresentation.ParentIntersection`,
            // TRG 2.2 §12.16). `ParentMd` is left out: the child's depths are
            // the parent's, and "If not specified, then these two MD's are
            // implied to be identical".
            for object in drawn
                .objects
                .iter_mut()
                .filter(|object| object.role == "trajectory")
            {
                object.json["ParentIntersection"] = json!({
                    "$type": "resqml22.WellboreTrajectoryParentIntersection",
                    "KickoffMd": found.tie.md,
                    "ParentTrajectory": one.trajectory.dor(),
                });
            }
        }
        let trajectory_uuid = drawn
            .objects
            .iter()
            .find(|object| object.role == "trajectory")
            .map(|object| object.uuid.clone());

        // -- what is said ------------------------------------------------------
        says.insert(
            0,
            Note {
                key: "stations",
                detail: format!(
                    "{} station(s) from MD {} to MD {}: every {} m of MD and each section's end",
                    line.len(),
                    mds[0],
                    last.md,
                    params.step
                ),
            },
        );
        if drawn.kind_index == 1 {
            let strays = match planned.tightest {
                Some(radius) => format!(
                    " - on the tightest arc (radius {radius:.3} m) a chord between two stations {} m apart strays at most {:.4} m from it",
                    params.step,
                    well_plan::sag(radius, params.step)
                ),
                None => String::new(),
            };
            says.push(Note {
                key: "linear-through-stations",
                detail: format!(
                    "the trajectory is a linear spline through the stations, its parameter the measured depth at each - the vertical reading the writer keeps for a straight hole{strays}"
                ),
            });
        }
        if let Some((z, surface)) = &ground_read {
            says.push(Note {
                key: "ground-read",
                detail: format!("the ground under the head, {z}, read on `{surface}`"),
            });
        }
        if parent.is_none() && params.datum_height > 0.0 {
            says.push(Note {
                key: "datum-above-ground",
                detail: format!(
                    "the datum, a {}, is {} m above the ground: the measured depths start there",
                    params.datum_kind.word(),
                    params.datum_height
                ),
            });
        }
        if let (Some(one), Some(found)) = (&parent, &kicked) {
            says.push(Note {
                key: "sidetrack",
                detail: format!(
                    "leaves `{}` at MD {}, on its straight stretch from MD {} to MD {}, at inclination {}° and azimuth {}°: its measured depths are the parent's, counted from the parent's datum `{}`",
                    one.name, found.tie.md, found.between.0, found.between.1, found.tie.inclination, found.tie.azimuth, one.datum.title
                ),
            });
        }
        if pair_made {
            says.push(Note {
                key: "planned",
                detail: "the interpretation says IsDrilled false: a trajectory computed from a plan is a planned one (RESQML 2.2 Wells.xsd, WellboreInterpretation.IsDrilled)".to_string(),
            });
        }
        says.append(&mut drawn.says);

        // -- the activity ---------------------------------------------------
        let departure = last.north.hypot(last.east);
        let support::Frame {
            objects: frame_objects,
            parameters: mut frame_parameters,
            says: frame_says,
            blocked,
            ..
        } = frame;
        frame_parameters.extend([
            Parameter::Double {
                title: "lineKind".to_string(),
                value: drawn.kind_index as f64,
                uom: None,
            },
            Parameter::Double {
                title: "stationCount".to_string(),
                value: line.len() as f64,
                uom: None,
            },
            Parameter::Double {
                title: "mdMax".to_string(),
                value: last.md,
                uom: Some("m".to_string()),
            },
            Parameter::Double {
                title: "departure".to_string(),
                value: departure,
                uom: Some("m".to_string()),
            },
        ]);
        if let Some(depth) = depth_max {
            frame_parameters.push(Parameter::Double {
                title: "depthMax".to_string(),
                value: depth,
                uom: Some("m".to_string()),
            });
        }
        if let Some((z, _)) = &ground_read {
            frame_parameters.push(Parameter::Double {
                title: "groundRead".to_string(),
                value: *z,
                uom: Some("m".to_string()),
            });
        }
        // What the frame must say of itself, after what the well said.
        says.extend(frame_says);

        let mut objects = frame_objects;
        objects.extend(datum_objects);
        objects.extend(pair_objects);
        objects.extend(drawn.objects);

        // **The shape is the stations**: the minimum-curvature line passes
        // through them (and bows between them by at most the sag of the
        // tightest arc, which the plan computes); the polyline of the stations
        // is what a reader of the artefact draws.
        let shapes = match &trajectory_uuid {
            Some(uuid) => {
                let count = line.len() as u32;
                vec![support::made_root(
                    &title,
                    uuid,
                    support::one_patch("polyline", line, Topology::Polylines(vec![count])),
                )]
            }
            None => Vec::new(),
        };

        Ok(Plan {
            media: Vec::new(),
            // What the frame blocked, the plan carries - a 2.0.1 system, a half.
            blocked,
            objects,
            datasets: drawn.datasets,
            shapes,
            activity: ActivityDraft {
                recipe: "well-from-plan",
                title: format!("well-from-plan → {title}"),
                seed: line_seed,
                parameters: frame_parameters,
            },
            says,
            dropped: Vec::new(),
        })
    }

    fn reports(&self) -> Vec<Slot> {
        let number = || Need::Number {
            uom: None,
            least: None,
            most: None,
        };
        support::frame_reports(vec![
            Slot::required(
                "lineKind",
                number(),
                "the RESQML line kind written: 0 vertical, 1 linear through the stations",
            ),
            Slot::required(
                "stationCount",
                number(),
                "stations - the trajectory's knots and the frame's nodes",
            ),
            Slot::required("mdMax", number(), "the deepest measured depth"),
            Slot::required(
                "departure",
                number(),
                "the horizontal distance from where the plan starts to its last station",
            ),
            Slot::optional(
                "depthMax",
                number(),
                "the greatest vertical depth below the ground at the head, when the ground is known",
            ),
            Slot::optional(
                "groundRead",
                number(),
                "the ground under the head, when it was read on `surface`",
            ),
        ])
    }
}
