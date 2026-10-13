//! `well-logs` — properties laid on wellbore frames by a **sourced law** of the
//! vertical depth below the ground: a temperature by a geothermal gradient, a
//! hydrostatic pressure below a water table. One continuous property per frame,
//! exactly one value per node.
//!
//! Step 5 of the build plan of Lutetia, the well logs:
//! pressure, temperature, and the logs of one well from the
//! lithology — the part of it a law can give, the two rows of the
//! base of the model, over the 10 BSS frames and the 3 of the doublet:
//!
//! > pressure, `pressure`, bar, nodes — hydrostatic below the top of
//! > the water table (~1 bar per 10 m of water), marked synthetic
//! >
//! > temperature, `temperature`, degC, nodes — ~12–13 °C around 20 m, then
//! > a gradient of a few °C per 100 m — an order of magnitude, to be sourced
//!
//! and the use case of hydrostatic pressure and temperature by gradient, several
//! nodes per unit — the reason the frames are cut finer than their passes.
//!
//! # What is not here, and why
//!
//! - **the logs of GTH-1 from the lithology** (porosity,
//!   resistivity, gamma ray, density, sonic, neutron) — a law of depth alone
//!   would invent them. They follow the stage and the lithology of each pass
//!   — and the table that states them waits for a human review
//!   (the reference model's recipe, step `etages`, `attente`). They are an
//!   operation of their own, once that table is read;
//! - the salinity is a constant on a **local** kind that the
//!   dictionary does not hold; the P10/P50/P90 pressures are facets;
//!   the yearly and monthly pressures are indexed in time —
//!   step 9, `property-time-series`.
//!
//! # The depth is vertical, and below the ground
//!
//! Neither the measured depth — it runs along the hole, and a deviated hole is
//! longer than it is deep — nor the depth below the datum, which the doublet
//! puts on a rig 3 m above the ground. The ground is **given once**,
//! one of two ways and never both:
//!
//! - `ground`, a surface in the frames' own frame, read **under each frame's
//!   head** by `energyml_analysis::heights` — the reading `well-from-plan` made
//!   when it placed the doublet on the ground surface, so the same ground twice and never a
//!   copied number. Under the head and not under each node: the reference model counts
//!   the vertical depth below the ground at the head, and a deviated
//!   hole under a slope would otherwise have its depths bent by the slope;
//! - `datumHeight`, how far above the ground every frame's datum stands — 0 for
//!   the ground-level datums of the BSS import (the BSS ground).
//!
//! The head is the frame's datum as the boundary placed it
//! (`Known::wellhead`), never its first node: since 22/09 a frame starts at its
//! first stated depth.
//!
//! # Which way is down is read, never guessed
//!
//! A node's height is read in its frame's own axis, through `Known::vertical_up`
//! and `Known::vertical_uom`; a frame that states neither is refused by name.
//! Its numbers are read **as placed ones** — right under an identity compound,
//! which every frame this crate writes has — and the one check a pure plan can
//! make of that is made: no node may lie farther from its datum than its
//! measured depth (`energyml_analysis::well_logs::beyond_reach`). A frame whose
//! local numbers are kilometres from its placed datum is refused; one offset by
//! less than its depths is not seen, and the plan says so (`placed`).
//!
//! # A law is sourced, and what it lays says so
//!
//! No constant without a source. A plausible value is declared
//! synthetic: `source` is required and a blank
//! one refused. And the fourth rule — `ExtensionNameValue` `synthetic = true` and a
//! sentence in `Citation/Description` on every invented object — is written on
//! every property, as `horizon-from-law` writes it on its grid: a value laid by
//! a law is not a measurement.
//!
//! # Positional, and nothing extrapolated
//!
//! A property on a frame is a positional array
//! (`crates/energyml-build/ARCHITECTURE.md`): **exactly as many values as the frame
//! has nodes**, checked against its measured depths — the geometry and
//! `NodeMd` are two arrays of the file. Where the law says nothing — above the
//! ground, above its reference depth or the water table — the value is NaN,
//! counted, and said. A frame with no node where the law holds gets no
//! property and is set aside by name (`Plan::dropped`): what arrives by
//! default arrives in number (`Otherwise::Every`), and a BSS hole shallower
//! than the water table is no reason to stop the run.
//!
//! # The writing is `property`'s
//!
//! *An operation that writes the same object as another delegates to it*: each
//! property goes to [`super::property::write`], typed. Its uuid comes from
//! what it is — the frame, the law and its coefficients, the kind, **and the
//! values** (create a second property if the values differ):
//! the same run writes the same objects, a
//! corrected coefficient or a moved ground writes a second property beside the
//! first, never one rewritten under the same uuid. The source is not in it:
//! rewording a citation rewrites the same property.
//!
//! # What is not suggested
//!
//! Nothing. There is no `title` and no `interpretation` slot, so the shared
//! block cannot hand the frame's to the property (the well that took the
//! ground's interpretation, 24/09): each title is the wellbore's name and the
//! measure — `GTH-1 — temperature`, the form `properties-from-table` writes and
//! `contacts-from-frames` reads back.
//!
//! # References
//!
//! - The hydrostatic and geothermal laws of `energyml-analysis` (`well_logs`).
//!   TODO(reference): verify — the geothermal gradient and the hydrostatic law carry no published source (the requirement itself says the source is still to be found).

use serde_json::json;

use energyml_analysis::heights::{Height, Heights};
use energyml_analysis::well_logs::{self as analysis, Coefficient, Law, LogError};
use energyml_domain::provenance::Parameter;
use energyml_domain::units;

use crate::declare::{Field, Inputs, ObjectRef, Params, PropertyKindRef, Shape, Title};
use crate::operation::{
    About, ActivityDraft, Category, Context, Dropped, Identity, Need, Note, Operation, Plan, Refusal, Slot,
};
use crate::pieces::{Known, Ref};

use super::property::PropertyParams;
use super::support;

/// The operation.
pub struct WellLogs;

/// What `well-logs` is told.
#[derive(Debug, Params)]
pub struct WellLogsParams {
    /// The law, and its coefficients.
    #[param(flatten)]
    pub law: LawParams,
    /// how far above the ground each frame's datum stands, in metres — 0 for a ground-level datum; left out, the ground is read on `ground` under each frame's head
    ///
    /// One or the other, never both: two answers to where the ground is would
    /// be two grounds.
    #[param(min = 0.0, uom = "m")]
    pub datum_height: Option<f64>,
    /// where the law and its coefficients come from — a document with its page or paragraph; a value stated as plausible says so (synthetic, and why)
    pub source: String,
    /// what is measured; left out, the kind of the standard dictionary the law gives — `temperature` or `pressure`
    ///
    /// Looked up by title, never written as a uuid here: the dictionary is the
    /// authority for its own identifiers.
    pub property_kind: Option<PropertyKindRef>,
    /// the HDF5 file the value arrays go into
    #[param(default = String::from("data.h5"), group = "naming")]
    pub data_file: String,
}

/// the law the values follow with the vertical depth below the ground — a temperature by a geothermal gradient, or a hydrostatic pressure below a water table
///
/// **No default**: which physics is the question, and a default would answer
/// it for whoever did not.
#[derive(Debug, Params)]
#[params(tag = "law")]
pub enum LawParams {
    /// A temperature rising linearly with depth.
    Temperature(TemperatureParams),
    /// The weight of a column of water below a water table.
    Hydrostatic(HydrostaticParams),
}

/// What a geothermal gradient is told.
#[derive(Debug, Params)]
pub struct TemperatureParams {
    /// temperature: the temperature at the reference depth, in degrees Celsius
    #[param(min = -273.15, uom = "degC")]
    pub reference_temperature: f64,
    /// temperature: the depth below the ground where that temperature holds, in metres — nodes above it get no value
    ///
    /// Above it lies the zone of seasonal swings, which a gradient does not
    /// describe; its nodes are NaN, never extrapolated.
    #[param(default = 0.0, optional, min = 0.0, uom = "m")]
    pub reference_depth: f64,
    /// temperature: the geothermal gradient, in degrees Celsius per kilometre of vertical depth — 30 is 3 °C per 100 m
    #[param(uom = "degC/km")]
    pub gradient: f64,
}

/// What a hydrostatic law is told.
#[derive(Debug, Params)]
pub struct HydrostaticParams {
    /// hydrostatic: the elevation of the water table, in metres above the vertical datum the frames count from — a horizontal plane; nodes above it get no value
    ///
    /// An elevation and not a depth: the reference model draws the top of the water
    /// table as a horizontal plane, and a piezometric level is
    /// stated as an elevation. Above the ground (artesian water) the ground
    /// still bounds the law: the air above it is no subsurface.
    #[param(uom = "m")]
    pub water_level: f64,
    /// hydrostatic: the density of the water, in kg/m3 — fresh water by default
    #[param(default = 1000.0, optional, min = 0.0, uom = "kg/m3")]
    pub density: f64,
    /// hydrostatic: the acceleration of gravity, in m/s2 — the standard gravity by default
    ///
    /// 9.80665 m/s², the value the 3rd CGPM (1901) defined.
    #[param(default = 9.80665, optional, min = 0.0, uom = "m/s2")]
    pub gravity: f64,
    /// hydrostatic: the pressure at the water table, in bar — 0 writes a gauge pressure, the atmosphere's 1.01325 an absolute one
    #[param(default = 0.0, optional, uom = "bar")]
    pub table_pressure: f64,
}

/// What `well-logs` takes.
#[derive(Debug, Inputs)]
pub struct WellLogsInputs {
    /// the wellbore frames the law is laid on — one property each, a value per node
    ///
    /// **Left empty, every frame of the package** — the declaration of
    /// `properties-from-table` and `contacts-from-frames`. A frame with no node
    /// where the law holds is set aside by name.
    #[input(least = 1, types = "resqml22.WellboreFrameRepresentation", every)]
    pub frames: Vec<Shape>,
    /// the ground — a surface's triangles or a grid's cells, in the frames' own frame; its height under each frame's head is where the depths are counted from
    #[input(topology = "triangles")]
    pub ground: Option<Shape>,
}

/// The law as told, before a frame gives it its ground.
enum Told {
    Temperature { at: f64, from: f64, gradient_km: f64 },
    Hydrostatic { level: f64, density: f64, gravity: f64, at_table_bar: f64 },
}

impl Told {
    fn of(law: &LawParams) -> Told {
        match law {
            LawParams::Temperature(t) => {
                Told::Temperature { at: t.reference_temperature, from: t.reference_depth, gradient_km: t.gradient }
            }
            LawParams::Hydrostatic(h) => Told::Hydrostatic {
                level: h.water_level,
                density: h.density,
                gravity: h.gravity,
                at_table_bar: h.table_pressure,
            },
        }
    }

    /// The law under a frame whose ground stands at `ground` metres of
    /// elevation, in the SI units the analysis counts in — `bar` pascals to
    /// the bar.
    fn law(&self, ground: f64, bar: f64) -> Law {
        match *self {
            Told::Temperature { at, from, gradient_km } => Law::Geothermal { at, from, gradient: gradient_km / 1000.0 },
            Told::Hydrostatic { level, density, gravity, at_table_bar } => {
                Law::Hydrostatic { table: ground - level, density, gravity, at_table: at_table_bar * bar }
            }
        }
    }

    /// The word the union writes.
    fn word(&self) -> &'static str {
        match self {
            Told::Temperature { .. } => "temperature",
            Told::Hydrostatic { .. } => "hydrostatic",
        }
    }

    /// The title of the kind in the standard dictionary: `pressure`
    /// e703be06, `temperature` 08260920.
    fn kind(&self) -> &'static str {
        match self {
            Told::Temperature { .. } => "temperature",
            Told::Hydrostatic { .. } => "pressure",
        }
    }

    /// The unit written: degC, bar.
    fn uom(&self) -> &'static str {
        match self {
            Told::Temperature { .. } => "degC",
            Told::Hydrostatic { .. } => "bar",
        }
    }

    /// From what the analysis gives (°C, Pa) to what is written.
    fn scale(&self, bar: f64) -> f64 {
        match self {
            Told::Temperature { .. } => 1.0,
            Told::Hydrostatic { .. } => 1.0 / bar,
        }
    }

    /// Its coefficients, exactly — what the seed carries.
    fn key(&self) -> String {
        match *self {
            Told::Temperature { at, from, gradient_km } => format!("temperature:{at:?}:{from:?}:{gradient_km:?}"),
            Told::Hydrostatic { level, density, gravity, at_table_bar } => {
                format!("hydrostatic:{level:?}:{density:?}:{gravity:?}:{at_table_bar:?}")
            }
        }
    }

    /// The law, in the words a description and a note use.
    fn words(&self) -> String {
        match *self {
            Told::Temperature { at, from, gradient_km } => format!(
                "a temperature of {at} degC at {from} m below the ground, changing by {gradient_km} degC per km of vertical depth below it"
            ),
            Told::Hydrostatic { level, density, gravity, at_table_bar } => format!(
                "a hydrostatic pressure below a water table at {level} m of elevation: {at_table_bar} bar there, and {density} kg/m3 × {gravity} m/s2 per metre of water below it"
            ),
        }
    }

    /// Where the law starts, in words.
    fn start_words(&self) -> String {
        match *self {
            Told::Temperature { from, .. } => format!("{from} m below the ground, where the temperature is stated"),
            Told::Hydrostatic { level, .. } => format!("the water table, at {level} m of elevation"),
        }
    }
}

/// The slot a coefficient of the analysis is told in.
fn slot_of(which: Coefficient) -> &'static str {
    match which {
        Coefficient::Temperature => "referenceTemperature",
        Coefficient::From => "referenceDepth",
        Coefficient::Gradient => "gradient",
        Coefficient::Table => "waterLevel",
        Coefficient::Density => "density",
        Coefficient::Gravity => "gravity",
        Coefficient::AtTable => "tablePressure",
    }
}

/// A frame, as a law is laid on it.
struct Hole<'a> {
    /// What a refusal, a note and the property's title name it by.
    title: String,
    reference: &'a Ref,
    known: &'a Known,
    /// Its nodes, their third coordinate **an elevation** in the unit of its
    /// vertical axis — `z` in a frame that counts up, `−z` in one that counts
    /// down.
    nodes: Vec<[f64; 3]>,
    mds: &'a [f64],
    /// Its datum, placed: easting, northing, elevation.
    head: [f64; 3],
    up: bool,
    uom: &'a str,
    /// Metres per unit of its vertical axis.
    metres: f64,
}

/// What a frame must state for a law to be laid on it — or why it does not.
fn hole_of(shape: &Shape) -> Result<Hole<'_>, String> {
    let name = shape.name.as_str();
    let known = shape.known.as_ref().ok_or_else(|| {
        format!("`{name}` comes with nothing known of it: a property hangs on a wellbore frame of the package, and names it")
    })?;
    let reference = known
        .reference
        .as_ref()
        .ok_or_else(|| format!("`{name}` names no object of the package for a property to hang on"))?;
    let title = if reference.title.is_empty() { name.to_string() } else { reference.title.clone() };
    let points: Vec<[f64; 3]> =
        shape.geometry.patches.iter().flat_map(|patch| patch.points.iter().copied()).collect();
    if points.is_empty() {
        return Err(format!("`{title}` places no node"));
    }
    let up = known.vertical_up.ok_or_else(|| {
        format!(
            "`{title}` does not say which way its vertical axis points — a depth below the ground is read on it, and up and down cannot be guessed"
        )
    })?;
    let uom = known.vertical_uom.as_deref().ok_or_else(|| {
        format!("`{title}` does not say in which unit its vertical axis counts — a depth in feet read as metres is 3.28 times too deep")
    })?;
    let metres = units::metres(1.0, uom).map_err(|why| {
        format!("`{title}` counts its vertical axis in `{uom}`, and a depth below the ground is a length: {why}")
    })?;
    let mds = known.node_mds.as_deref().ok_or_else(|| {
        format!("`{title}` states no measured depth for its nodes — it is no wellbore frame, or this build cannot read its arrays")
    })?;
    if mds.len() != points.len() {
        return Err(format!(
            "`{title}` has {} node(s) and {} measured depth(s): a value is laid on each node, and each node is where its depth is",
            points.len(),
            mds.len()
        ));
    }
    let head = known.wellhead.ok_or_else(|| {
        format!("`{title}` names no datum that places it — the depths are counted from the ground at its head, and its head is its datum")
    })?;
    let nodes = points.iter().map(|p| [p[0], p[1], if up { p[2] } else { -p[2] }]).collect();
    Ok(Hole { title, reference, known, nodes, mds, head, up, uom, metres })
}

/// The title the object behind a shape carries, or the shape's name.
fn title_of(shape: &Shape) -> String {
    shape
        .known
        .as_ref()
        .and_then(|known| known.reference.as_ref())
        .map(|reference| reference.title.clone())
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| shape.name.clone())
}

/// Values as the seed carries them — exactly, NaN included.
fn digest(values: &[f64]) -> String {
    values.iter().map(|value| format!("{value:?}")).collect::<Vec<_>>().join(",")
}

/// A count, as the activity records it.
fn count(title: &str, value: usize) -> Parameter {
    Parameter::Double { title: title.to_string(), value: value as f64, uom: None }
}

impl Operation for WellLogs {
    type Params = WellLogsParams;
    type Inputs = WellLogsInputs;

    fn about(&self) -> About {
        About {
            name: "well-logs",
            describe: "a temperature by a geothermal gradient, or a hydrostatic pressure, laid on wellbore frames by a sourced law of the vertical depth below the ground — one property per frame, one value per node, marked synthetic",
            category: Category::Synthetic,
            makes: &["resqml22.ContinuousProperty"],
            recipe: "well-logs",
            represents: None,
        }
    }


    fn check(&self, inputs: &WellLogsInputs) -> Result<(), Refusal> {
        if let Some(ground) = &inputs.ground
            && !support::Faces::of(&ground.geometry).any()
        {
            return Err(Refusal::on(
                "ground",
                format!("`{}` has no face — the ground is read on triangles or a grid's cells, and points or lines have none", ground.name),
            ));
        }
        Ok(())
    }

    fn plan(&self, inputs: WellLogsInputs, params: WellLogsParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        let WellLogsInputs { frames, ground } = inputs;
        // **Said here and not in `check`**: an empty `frames` is what a list of
        // offers sees before the edge fills it (`read::every`).
        if frames.is_empty() {
            return Err(Refusal::on(
                "frames",
                "it takes wellbore frames; the input holds none, and the package gives none either",
            ));
        }

        // -- the source, before anything is laid -----------------------------
        let source = params.source.trim().to_string();
        if source.is_empty() {
            return Err(Refusal::on(
                "source",
                "is empty: a law is sourced — a document with its page or paragraph, or a plausible value said to be one",
            ));
        }

        // -- the ground: given once --------------------------------------------
        match (&ground, params.datum_height) {
            (Some(surface), Some(_)) => {
                return Err(Refusal::on(
                    "datumHeight",
                    format!("`datumHeight` and `ground` (`{}`) are two answers to where the ground is: give one", title_of(surface)),
                ));
            }
            (None, None) => {
                return Err(Refusal::on(
                    "ground",
                    "the depths are counted from the ground, and nothing says where it is: give `ground`, a surface to read it on under each head, or `datumHeight`, how far above it each frame's datum stands",
                ));
            }
            _ => {}
        }

        // -- the law, checked before any frame is read --------------------------
        let bar = units::pascals(1.0, "bar").map_err(|why| Refusal::new("units", why.to_string()))?;
        let told = Told::of(&params.law);
        told.law(0.0, bar).check().map_err(|why| refusal_of(&why))?;

        // -- the kind: the dictionary's, by title --------------------------------
        let kind = match params.property_kind {
            Some(given) => given.0,
            None => {
                let found = energyml_domain::property_kind::with_default(|dictionary| {
                    dictionary.by_title(told.kind()).first().map(|kind| (kind.uuid.clone(), kind.title.clone()))
                });
                let (uuid, title) = found.ok_or_else(|| {
                    Refusal::on(
                        "propertyKind",
                        format!("the dictionary in use holds no kind called `{}`: name a `propertyKind` of the package", told.kind()),
                    )
                })?;
                json!({ "$type": "eml23.DataObjectReference", "Uuid": uuid, "QualifiedType": "eml23.PropertyKind", "Title": title })
            }
        };
        let kind_uuid = kind.get("Uuid").and_then(|v| v.as_str()).or_else(|| kind.as_str()).unwrap_or_default().to_string();

        // -- the ground surface, read once ---------------------------------------
        let faces = ground.as_ref().map(|surface| support::Faces::of(&surface.geometry));
        let heights = match &faces {
            Some(faces) => {
                Some(Heights::of(faces.surface(), "ground", "under the wellheads").map_err(|why| Refusal::on("ground", why))?)
            }
            None => None,
        };
        let ground_title = ground.as_ref().map(title_of).unwrap_or_default();

        // -- each frame ------------------------------------------------------------
        let mut objects = Vec::new();
        let mut datasets = Vec::new();
        let mut says: Vec<Note> = Vec::new();
        let mut dropped: Vec<Dropped> = Vec::new();
        let mut inputs: Vec<Parameter> = Vec::new();
        let mut reads: Vec<Parameter> = Vec::new();
        let mut grounds: Vec<Note> = Vec::new();
        let (mut above_ground, mut shallow) = (0usize, 0usize);
        let mut range: Option<[f64; 2]> = None;
        let mut written = 0usize;
        for shape in &frames {
            cx.stop_if_cancelled()?;
            let hole = hole_of(shape).map_err(|why| Refusal::on("frames", why))?;
            inputs.push(Parameter::Object { title: "input".to_string(), reference: hole.reference.dor() });

            // Where the datum and the depths put the nodes — the one check a
            // pure plan can make that the numbers are placed ones.
            if let Some(far) = analysis::beyond_reach(hole.head, &hole.nodes, hole.mds) {
                let datum = hole.known.datum.as_ref().map(|d| format!("`{}`", d.title)).unwrap_or_else(|| "its datum".to_string());
                return Err(Refusal::on(
                    "frames",
                    format!(
                        "`{}`: node {} (MD {}) lies {} from {datum} — no hole of that depth reaches so far, so its numbers are not where its datum puts them: a frame with an origin, an azimuth or a vertical origin of its own gives local numbers, and the depths here are read in placed ones",
                        hole.title, far.node, far.md, far.distance
                    ),
                ));
            }

            // The ground, in metres of elevation.
            let ground_m = match (&ground, &heights, params.datum_height) {
                (Some(surface), Some(heights), _) => {
                    let theirs = surface.known.as_ref().and_then(|known| known.crs.as_ref());
                    match (hole.known.crs.as_ref(), theirs) {
                        (Some(mine), Some(theirs)) if mine.uuid.eq_ignore_ascii_case(&theirs.uuid) => {}
                        (Some(mine), Some(theirs)) => {
                            return Err(Refusal::on(
                                "ground",
                                format!(
                                    "`{ground_title}` is in `{}` and `{}` in `{}` — a height read in one frame and compared in another is a height from somewhere else: give a ground in the frames' frame, or `datumHeight`",
                                    theirs.title, hole.title, mine.title
                                ),
                            ));
                        }
                        _ => {
                            return Err(Refusal::on(
                                "ground",
                                format!(
                                    "`{ground_title}` or `{}` states no frame, so the ground cannot be compared with the frame's nodes: give both from the package, or `datumHeight`",
                                    hole.title
                                ),
                            ));
                        }
                    }
                    let [x, y, _] = hole.head;
                    let z = match heights.height(x, y) {
                        Height::At(z) => z,
                        Height::Outside => {
                            return Err(Refusal::on(
                                "ground",
                                format!("the head of `{}` ({x}, {y}) is outside `{ground_title}`: no face lies under it — give a ground that covers it, or `datumHeight`", hole.title),
                            ));
                        }
                        Height::Folded => {
                            return Err(Refusal::on(
                                "ground",
                                format!("`{ground_title}` folds under the head of `{}` ({x}, {y}): several faces disagree on its height there", hole.title),
                            ));
                        }
                    };
                    reads.push(Parameter::Double { title: "groundRead".to_string(), value: z, uom: Some(hole.uom.to_string()) });
                    grounds.push(Note {
                        key: "ground",
                        detail: format!("`{}`: the ground under its head ({x}, {y}), {z} {}, read on `{ground_title}`", hole.title, hole.uom),
                    });
                    (if hole.up { z } else { -z }) * hole.metres
                }
                (_, _, Some(height)) => {
                    grounds.push(Note {
                        key: "ground",
                        detail: format!("`{}`: the ground {height} m below its datum, at {} {} of elevation", hole.title, hole.head[2], hole.uom),
                    });
                    hole.head[2] * hole.metres - height
                }
                _ => return Err(Refusal::on("ground", "the ground is neither read nor stated")),
            };

            // The law, at each node's vertical depth below that ground.
            let depths: Vec<f64> = hole.nodes.iter().map(|node| ground_m - node[2] * hole.metres).collect();
            let law = told.law(ground_m, bar);
            let logged = analysis::lay(&law, &depths).map_err(|why| match &why {
                LogError::BadDepth { .. } => Refusal::on("frames", format!("`{}`: {why}", hole.title)),
                other => refusal_of(other),
            })?;
            if logged.laid() == 0 {
                dropped.push(Dropped {
                    count: 1,
                    why: format!(
                        "`{}`: no node lies where the law holds — {} above the ground, {} above {}: no property written",
                        hole.title,
                        logged.above_ground,
                        logged.shallow,
                        told.start_words()
                    ),
                });
                continue;
            }
            above_ground += logged.above_ground;
            shallow += logged.shallow;
            let scale = told.scale(bar);
            let values: Vec<f64> = logged.values.iter().map(|value| value * scale).collect();
            if let Some([low, high]) = logged.range {
                let (low, high) = (low * scale, high * scale);
                range = Some(match range {
                    None => [low, high],
                    Some([a, b]) => [a.min(low), b.max(high)],
                });
            }

            // -- the writing is `property`'s ---------------------------------
            let well = energyml_domain::wellbore_table::stem(&hole.title).to_string();
            let title = format!("{well} — {}", told.kind());
            let seed = format!("well-logs:{}:{}:{kind_uuid}:{}", hole.reference.uuid, told.key(), digest(&values));
            let field = Field {
                name: format!("{} of {}", told.kind(), shape.name),
                over: shape.name.clone(),
                on: "nodes".to_string(),
                values,
                uom: Some(told.uom().to_string()),
            };
            let property = PropertyParams {
                representation: ObjectRef(hole.reference.dor()),
                property_kind: PropertyKindRef(kind.clone()),
                uom: told.uom().to_string(),
                title: Title(title.clone()),
                data_file: params.data_file.clone(),
            };
            // The writer checks the values against the nodes it is told of; the
            // count it is told is the frame's, or its measured depths' — the
            // geometry and `NodeMd` were found to agree above.
            let mut counted = hole.known.clone();
            if counted.node_count.is_none() {
                counted.node_count = Some(hole.mds.len());
            }
            // **One uuid each, in the plan's order.** The writer takes the first
            // of the identity it is given; a list given by the caller is handed
            // out one uuid per property.
            let identity = match cx.identity {
                Identity::Given(_) => Identity::Given(vec![cx.identity.uuid("", written)?]),
                derived => derived.clone(),
            };
            let one = Context { identity: &identity, when: cx.when, purpose: cx.purpose, feedback: cx.feedback, bag: cx.bag };
            let mut drawn = super::property::write(&field, Some(&counted), &property, &seed, &one)?;

            // What the property says of itself: laid, not measured, and from what.
            let description = format!(
                "Laid by a law, not measured: {}. The depth is vertical, below the ground at the head of `{}`. Source: {source}",
                told.words(),
                hole.title
            );
            // `Citation/Description` holds 2000 characters: refused rather than
            // cut short, since a source cut short cites nothing.
            if description.chars().count() > 2000 {
                return Err(Refusal::on(
                    "source",
                    format!(
                        "makes a description of {} characters, and a Citation/Description holds 2000 — cite the document shorter",
                        description.chars().count()
                    ),
                ));
            }
            for object in drawn.objects.iter_mut() {
                object.json["Citation"]["Description"] = json!(description);
                // The mark of an invented value.
                object.json["ExtensionNameValue"] = json!([{
                    "$type": "eml23.ExtensionNameValue",
                    "Name": "synthetic",
                    "Value": { "$type": "eml23.StringMeasure", "_": "true" },
                    "Description": format!("laid by `well-logs` by a law of depth ({}), not measured", told.word()),
                }]);
            }
            objects.append(&mut drawn.objects);
            datasets.append(&mut drawn.datasets);
            says.append(&mut drawn.says);
            written += 1;
        }

        if written == 0 {
            // A plan of nothing but set-aside frames would read as a run that
            // had nothing to do — the case that looks most like a success.
            return Err(Refusal::on(
                "frames",
                format!(
                    "no frame has a node where the law holds: {}",
                    dropped.iter().map(|one| one.why.as_str()).collect::<Vec<_>>().join("; ")
                ),
            ));
        }

        // -- what is said ------------------------------------------------------------
        let mut notes = vec![
            Note { key: "law", detail: format!("{} — source: {source}", told.words()) },
            Note {
                key: "vertical-depth",
                detail: "the law is read at the vertical depth below the ground at each frame's head — not the measured depth, which runs along the hole, and not the depth below the datum, which may stand above the ground".to_string(),
            },
        ];
        notes.append(&mut grounds);
        notes.push(Note {
            key: "synthetic",
            detail: "each property is laid, not measured: it carries `synthetic = true` and a description of its law and its source".to_string(),
        });
        notes.push(Note {
            key: "placed",
            detail: "the nodes are read in their frame's own numbers as placed ones — right under an identity compound; a frame whose numbers lie farther from its datum than its depths is refused, one offset by less is not seen".to_string(),
        });
        if above_ground > 0 {
            notes.push(Note {
                key: "above-ground",
                detail: format!(
                    "{above_ground} node(s) above the ground — a datum on a rig, the first metres of a hole measured from it: no law of the subsurface holds there, written as NaN"
                ),
            });
        }
        if shallow > 0 {
            notes.push(Note {
                key: "shallow",
                detail: format!(
                    "{shallow} node(s) below the ground but above where the law starts — {}: written as NaN, not extrapolated",
                    told.start_words()
                ),
            });
        }
        if let Some([low, high]) = range {
            notes.push(Note { key: "range", detail: format!("the values laid run from {low} to {high} {}", told.uom()) });
        }
        notes.append(&mut says);

        // -- the activity -------------------------------------------------------------
        if let Some(reference) = ground.as_ref().and_then(|surface| surface.known.as_ref()).and_then(|known| known.reference.as_ref()) {
            inputs.push(Parameter::Object { title: "input".to_string(), reference: reference.dor() });
        }
        let mut parameters = inputs;
        parameters.push(count("propertyCount", written));
        parameters.push(count("aboveGroundCount", above_ground));
        parameters.push(count("shallowCount", shallow));
        parameters.append(&mut reads);
        let frame_keys: Vec<&str> = frames
            .iter()
            .filter_map(|shape| shape.known.as_ref().and_then(|known| known.reference.as_ref()).map(|r| r.uuid.as_str()))
            .collect();

        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            objects,
            datasets,
            // **No shape**: a property is laid on a geometry, it
            // makes none.
            shapes: Vec::new(),
            activity: ActivityDraft {
                recipe: "well-logs",
                title: format!("well-logs → {} on {written} frame(s)", told.kind()),
                seed: format!("well-logs:{}:{}", told.key(), frame_keys.join(",")),
                parameters,
            },
            says: notes,
            dropped,
        })
    }

    /// The law's settings, `datumHeight` and `source` are inputs, recorded
    /// under the parameters the projection already declares — a title cannot be
    /// both.
    fn reports(&self) -> Vec<Slot> {
        let number = || Need::Number { uom: None, least: None, most: None };
        vec![
            Slot::required("propertyCount", number(), "properties written, one per frame that has a node where the law holds"),
            Slot::required(
                "aboveGroundCount",
                number(),
                "nodes above the ground — a datum on a rig, the air: no value",
            ),
            Slot::required(
                "shallowCount",
                number(),
                "nodes below the ground and above where the law starts — its reference depth, or the water table: no value",
            ),
            Slot::many(
                "groundRead",
                number(),
                "the ground under each frame's head, read on `ground`, in the frame's vertical axis — one per frame, in their order",
                0,
            ),
        ]
    }
}

/// A refusal of the analysis, under the slot its coefficient is told in.
fn refusal_of(why: &LogError) -> Refusal {
    match why {
        LogError::NotFinite { which, .. } | LogError::NotPositive { which, .. } | LogError::Negative { which, .. } => {
            Refusal::on(slot_of(*which), why)
        }
        LogError::BadDepth { .. } => Refusal::on("frames", why),
    }
}
