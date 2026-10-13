//! `fault-throw` - a horizon torn along the faults that cut it, its hanging
//! wall offset: the triangulated surface with its `SplitEdgePatch`, the new
//! interpretation it represents, and the lips.
//!
//! This is the fault-throw step of the reference model's build plan (lip
//! offset): a `TriangulatedSetRepresentation` with a `SplitEdgePatch`, with
//! nodes duplicated along the faults that split it and lips offset by the throw,
//! and the two cut lines (roof, wall) with `LineRole` `contact`. The gap it
//! closes was that no code wrote a `SplitEdgePatch`; this is the writer.
//!
//! # What the operation decides, and where
//!
//! The computation is `energyml_analysis::fault_throw`: the horizon's nodes
//! triangulated with each fault's trace as a constraint, the chain of edges
//! along each trace torn, the hanging wall offset. So this operation
//! **decides a triangulation**, as `triangulate` does - and takes the horizon
//! as **nodes**, a grid or a cloud: a surface already triangulated is refused,
//! since retriangulating it would lay a second skin over the first (the rule
//! `triangulate` keeps). The surface before the tear is not written: the
//! `Activity` names it (decided in review) - the
//! activity records the horizon it was given.
//!
//! # What it writes
//!
//! - the torn surface, by [`super::triangles::write`] - one `TrianglePatch`,
//!   and in it **one `SplitEdgePatch` per lip**, footwall then hanging wall,
//!   fault after fault: *« Describes edges that are not linked to any other
//!   edge. Because edges do not have indices, a consecutive pair of nodes is
//!   used to identify each edge »*, *« a 2D array of non-negative integers
//!   with dimensions 2 x numSplitEdges »* (RESQML 2.2 Structural.xsd
//!   l. 206-219; TRG 2.2 §11.7 p. 276). The schema says neither how many
//!   `EdgePatch` a tear takes nor what one groups; **one per lip** is this
//!   operation's reading, the one that lets a reader rebuild each lip as a line -
//!   it is said in the plan;
//! - **a new `HorizonInterpretation`** of the horizon's feature, which the
//!   surface represents: *« if a user is changing the number or the
//!   geometrical location of points in the representation of a previously
//!   recorded interpretation, he MUST create a NEW interpretation »* (RESQML
//!   usage guide 2.2, draft, §7.2.2 p. 72). The feature is an
//!   input: what the horizon's previous interpretation interprets is not in
//!   what the bag knows of it;
//! - **the lips**, by [`super::polylines::write`]: one patch per fault, two
//!   open lines each - the footwall's, then the hanging wall's - `LineRole`
//!   `contact`, *« Used to represent nonsealed contact interpretation parts
//!   defined by a horizon/fault intersection »* (Structural.xsd l. 60-64).
//!   Their nodes are nodes of the torn surface, position for position, which
//!   is what `surface-framework` reads to name them there.
//!
//! # One list per fault, and what is refused
//!
//! `maximumThrow` and `throw` follow `faults`, one entry each: a count that
//! differs is refused, naming both. A throw's **sense** is what moves the
//! hanging wall: `normal` lowers it, `reverse` and `thrust` raise it; a
//! strike-slip, scissor or variable play states no vertical sense and is
//! refused by name. These are stated again here - `fault-from-sticks` wrote
//! them on the `FaultInterpretation`, and what the bag knows of an object
//! does not carry an interpretation's fields: reading them at the boundary is
//! what would make the two statements one.
//!
//! The rest is `energyml_analysis::fault_throw`'s: a vertical fault (no
//! hanging wall), a fault that misses the horizon or cuts it in pieces or in a
//! ring, two faults whose traces cross - each refused by its name, on `faults`.
//!
//! # A synthetic throw says so
//!
//! The throw of the reference model is invented (a throw law - maximum at the
//! centre, null at the ends, a bell-shaped law, synthetic and declared):
//! with `synthetic`, a `source` is required, and the surface, its
//! interpretation and the lips carry `synthetic = true` and the sentence
//! (the reference-model rules on invented objects, ADR-0019), with the law in it.
//!
//! # Identity
//!
//! From the horizon and the title, like `horizon-from-law`: correcting a throw
//! rewrites the same objects (correcting means editing the recipe and
//! replaying it). Under uuids given by the caller, the interpretation takes the
//! rank after the surface's objects, and the lips theirs after it.
//!
//! # References
//!
//! - The throw law along a fault trace is a synthetic law stated in `energyml-analysis`, which cites no published source for it.
//!   TODO(reference): verify - no publication is cited; if the law follows one, add it (authors, year, title, venue, DOI) once checked.

use serde_json::json;

use energyml_analysis::fault_throw::{self, Fault, Sense, Settings, TearError};
use energyml_domain::provenance::Parameter;
use energyml_domain::units::Dimension;
use energyml_eml_2_3 as eml;
use energyml_geometry::geometry::{Geometry, Patch, Topology};
use energyml_resqml_2_2 as resqml;

use crate::declare::{Choice, CrsRef, Inputs, ObjectIn, Params, Shape, Suggestions, TypeName};
use crate::operation::{
    About, Category, Context, Dataset, Identity, Need, Note, Numbers, Operation, Plan, Refusal, Slot,
};
use crate::pieces::Pieces;

use super::fault::Throw;
use super::polylines::PolylinesInputs;
use super::support;
use super::triangles::TrianglesInputs;
use super::written::{FrameParams, NamingParams, NewFeatureParams, RepresentedParams, Written};

/// The operation.
pub struct FaultThrow;

/// How the throw varies along a trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum Profile {
    /// `4 s (1 − s)` - zero at both ends, the maximum in the middle.
    Parabolic,
    /// `√(1 − (2 s − 1)²)` - steeper at the ends, flatter in the middle.
    Elliptic,
}

impl Profile {
    pub(super) fn computed(self) -> fault_throw::Profile {
        match self {
            Profile::Parabolic => fault_throw::Profile::Parabolic,
            Profile::Elliptic => fault_throw::Profile::Elliptic,
        }
    }

    pub(super) fn law(self) -> &'static str {
        match self {
            Profile::Parabolic => "4 s (1 − s)",
            Profile::Elliptic => "√(1 − (2 s − 1)²)",
        }
    }
}

/// What `fault-throw` is told.
#[derive(Debug, Params)]
pub struct FaultThrowParams {
    /// the largest throw of each fault, in metres, in the order of `faults` - reached where the profile is one
    #[param(least = 1, min = 0.0, uom = "m")]
    pub maximum_throw: Vec<f64>,
    /// how each fault moved, in the order of `faults` - normal lowers its hanging wall, reverse or thrust raises it
    #[param(least = 1)]
    pub throw: Vec<Throw>,
    /// how the throw varies along each trace - zero at both ends, the maximum in the middle
    ///
    /// No default: a law of throw is a statement about the fault, and a
    /// default would make it for whoever did not.
    pub profile: Profile,
    /// how far from the trace, in plan, the hanging wall is dragged - the offset falls linearly from the throw on the lip to zero at this distance, in metres
    #[param(min = 0.0, uom = "m")]
    pub reach: f64,
    /// move the hanging wall along the fault's measured mean dip rather than straight down - its lip then stays on a plane of that dip
    #[param(default = false, optional)]
    pub heave: bool,
    /// the throw is invented: every object written carries `synthetic = true` and its source
    #[param(default = false, optional)]
    pub synthetic: bool,
    /// where the throw comes from - a document with its page, or why an invented one is plausible; required for a synthetic one
    pub source: Option<String>,
    /// Where the coordinates are.
    #[param(flatten)]
    pub frame: FrameParams,
    /// The title and the file.
    #[param(flatten)]
    pub naming: NamingParams,
}

/// What `fault-throw` takes.
#[derive(Debug, Inputs)]
pub struct FaultThrowInputs {
    /// the horizon to tear - a grid or a cloud of its nodes, which this operation triangulates with the faults' traces as constraints
    #[input(topology = "grid")]
    pub horizon: Shape,
    /// the fault surfaces that cut it, triangulated - in the order `maximumThrow` and `throw` follow
    #[input(topology = "triangles", least = 1, ordered)]
    pub faults: Vec<Shape>,
    /// the horizon's feature - the torn surface represents a new interpretation of it
    #[input(types = "resqml22.BoundaryFeature")]
    pub feature: ObjectIn,
}

/// The `synthetic` mark the reference model asks of an invented object (ADR-0019) -
/// the form `horizon-from-law` and `fault-from-sticks` write.
fn synthetic_mark() -> eml::ExtensionNameValue {
    eml::ExtensionNameValue {
        name: "synthetic".to_string(),
        value: eml::StringMeasure {
            value: "true".to_string(),
            ..Default::default()
        },
        description: Some(
            "an invented throw, written by `fault-throw` from a stated law, not measured".to_string(),
        ),
        ..Default::default()
    }
}

/// A triangulated surface: its nodes and its triangles.
type Surface = (Vec<[f64; 3]>, Vec<[u32; 3]>);

/// A shape's triangles, every patch's indices shifted to the whole shape's
/// nodes - or `None` when it holds none.
fn surface_of(shape: &Shape) -> Option<Surface> {
    let mut points = Vec::new();
    let mut triangles = Vec::new();
    for patch in &shape.geometry.patches {
        let Topology::Triangles(faces) = &patch.topology else {
            continue;
        };
        let base = points.len() as u32;
        points.extend(patch.points.iter().copied());
        triangles.extend(faces.iter().map(|f| [f[0] + base, f[1] + base, f[2] + base]));
    }
    (!triangles.is_empty()).then_some((points, triangles))
}

/// What names a shape in a message: its object's title, or its name in the bag.
fn name_of(shape: &Shape) -> String {
    shape
        .known
        .as_ref()
        .and_then(|known| known.reference.as_ref())
        .map(|one| one.title.clone())
        .unwrap_or_else(|| shape.name.clone())
}

/// The slot a refusal of the computation belongs to.
fn refusal(why: TearError) -> Refusal {
    let slot = match why {
        TearError::BadThrow { .. } => "maximumThrow",
        TearError::BadReach { .. } => "reach",
        TearError::Horizon { .. } => "horizon",
        _ => "faults",
    };
    Refusal::on(slot, why)
}

impl Operation for FaultThrow {
    type Params = FaultThrowParams;
    type Inputs = FaultThrowInputs;

    fn about(&self) -> About {
        About {
            name: "fault-throw",
            describe: "a horizon torn along the faults that cut it - the triangulated surface with its split edges, its hanging walls offset by a stated throw, a new interpretation of its feature, and the lips",
            category: Category::Faults,
            makes: &[
                "resqml22.TriangulatedSetRepresentation",
                "resqml22.HorizonInterpretation",
                "resqml22.PolylineSetRepresentation",
            ],
            recipe: "fault-throw",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
        // **Not an input's title**: the torn surface is a new object, and a
        // title borrowed from the horizon would make two objects one name.
        if let Some(slot) = slots.slot_mut("title") {
            slot.suggested = None;
            slot.from = None;
        }
    }

    /// The horizon is nodes, the faults are surfaces.
    fn check(&self, inputs: &FaultThrowInputs) -> Result<(), Refusal> {
        for patch in &inputs.horizon.geometry.patches {
            match &patch.topology {
                Topology::Triangles(_) => {
                    return Err(Refusal::on(
                        "horizon",
                        format!(
                            "`{}` is already triangulated, and this operation decides the triangles itself - with the faults' traces as constraints: give the grid or the cloud of its nodes (retriangulating would lay a second skin over the first)",
                            name_of(&inputs.horizon)
                        ),
                    ));
                }
                Topology::Points | Topology::Grid { .. } => {}
                _ => {
                    return Err(Refusal::on(
                        "horizon",
                        format!(
                            "`{}` is not a surface's nodes - give a grid or a cloud",
                            name_of(&inputs.horizon)
                        ),
                    ));
                }
            }
        }
        for fault in &inputs.faults {
            if surface_of(fault).is_none() {
                return Err(Refusal::on(
                    "faults",
                    format!(
                        "`{}` is not a triangulated surface - a fault's trace on the horizon is computed from its surface",
                        name_of(fault)
                    ),
                ));
            }
        }
        Ok(())
    }

    fn plan(
        &self,
        inputs: FaultThrowInputs,
        params: FaultThrowParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let title = params.naming.title();
        let count = inputs.faults.len();

        // -- one entry per fault ---------------------------------------------------
        if params.maximum_throw.len() != count {
            return Err(Refusal::on(
                "maximumThrow",
                format!(
                    "{} throw(s) for {count} fault(s) - one per fault, in the order of `faults`",
                    params.maximum_throw.len()
                ),
            ));
        }
        if params.throw.len() != count {
            return Err(Refusal::on(
                "throw",
                format!(
                    "{} kind(s) of throw for {count} fault(s) - one per fault, in the order of `faults`",
                    params.throw.len()
                ),
            ));
        }
        let names: Vec<String> = inputs.faults.iter().map(name_of).collect();
        let mut senses: Vec<Sense> = Vec::with_capacity(count);
        for (one, name) in params.throw.iter().zip(&names) {
            senses.push(match one {
                Throw::Normal => Sense::Down,
                Throw::Reverse | Throw::Thrust => Sense::Up,
                other => {
                    return Err(Refusal::on(
                        "throw",
                        format!(
                            "`{name}` is given as {}: that play states no vertical sense, and which way its hanging wall goes is what is thrown - give normal, reverse or thrust",
                            other.word()
                        ),
                    ));
                }
            });
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
                "a synthetic throw says where its numbers come from - a document with its page, or why they are plausible (no constant without a source, ADR-0019)",
            ));
        }

        // -- the frame: the horizon's, and which way is up -------------------------
        let horizon_name = name_of(&inputs.horizon);
        let horizon_frame = inputs.horizon.known.as_ref().and_then(|known| known.crs.clone());
        if let Some(own) = &horizon_frame {
            if params.frame.source_epsg.is_some() {
                return Err(Refusal::on(
                    "sourceEpsg",
                    format!(
                        "`{horizon_name}` is in `{}`, and its torn copy is written there - name it as `crs` rather than creating a frame",
                        own.title
                    ),
                ));
            }
            if let Some(named) = params.frame.crs.as_ref().and_then(|one| one.0["Uuid"].as_str())
                && !named.eq_ignore_ascii_case(&own.uuid)
            {
                return Err(Refusal::on(
                    "crs",
                    format!(
                        "`{horizon_name}` is in `{}`, and the frame named is another - its torn copy is written in the horizon's own frame",
                        own.title
                    ),
                ));
            }
        }
        for (fault, name) in inputs.faults.iter().zip(&names) {
            if let (Some(own), Some(theirs)) = (
                &horizon_frame,
                fault.known.as_ref().and_then(|known| known.crs.as_ref()),
            ) && !own.uuid.eq_ignore_ascii_case(&theirs.uuid)
            {
                return Err(Refusal::on(
                    "faults",
                    format!(
                        "`{name}` is in `{}` and `{horizon_name}` in `{}` - a trace read across two frames is read across two origins",
                        theirs.title, own.title
                    ),
                ));
            }
        }
        let (up, unit) = vertical_of(&inputs, &params.frame, cx)?;
        let Some(up) = up else {
            return Err(Refusal::on(
                "crs",
                "the frame does not say which way its vertical axis points, and a hanging wall goes **down**: name a LocalEngineeringCompoundCrs as `crs`",
            ));
        };
        match unit.as_deref() {
            Some("m") => {}
            Some(other) => {
                return Err(Refusal::on(
                    "crs",
                    format!(
                        "the frame counts its vertical axis in `{other}`, and a throw and a reach are given in metres - no conversion is made"
                    ),
                ));
            }
            None => {
                return Err(Refusal::on(
                    "crs",
                    "the frame does not say in what unit its vertical axis counts, and a throw is given in metres: name a LocalEngineeringCompoundCrs as `crs`",
                ));
            }
        }

        // -- the horizon's nodes ----------------------------------------------------
        let nodes: Vec<[f64; 3]> = inputs
            .horizon
            .geometry
            .patches
            .iter()
            .flat_map(|patch| patch.points.iter().copied())
            .collect();
        let holes = nodes.iter().filter(|p| !p.iter().all(|c| c.is_finite())).count();
        if holes > 0 {
            return Err(Refusal::on(
                "horizon",
                format!(
                    "{holes} of the {} node(s) of `{horizon_name}` have no position - a triangulation of the others would span the holes, drawing a surface the horizon does not state",
                    nodes.len()
                ),
            ));
        }

        // -- the computation ----------------------------------------------------------
        cx.stop_if_cancelled()?;
        let surfaces: Vec<Surface> = inputs
            .faults
            .iter()
            .map(|fault| surface_of(fault).unwrap_or_default())
            .collect();
        let faults: Vec<Fault<'_>> = surfaces
            .iter()
            .zip(&names)
            .zip(params.maximum_throw.iter().zip(&senses))
            .map(|(((points, triangles), name), (throw, sense))| Fault {
                name,
                points,
                triangles,
                maximum_throw: *throw,
                sense: *sense,
            })
            .collect();
        let settings = Settings {
            profile: params.profile.computed(),
            reach: params.reach,
            heave: params.heave,
            up,
        };
        let torn = fault_throw::throw(&nodes, &faults, &settings).map_err(refusal)?;
        cx.stop_if_cancelled()?;

        // -- the torn surface, by its writer -------------------------------------------
        let seed = format!(
            "fault-throw:{}:{title}",
            inputs
                .horizon
                .known
                .as_ref()
                .and_then(|known| known.reference.as_ref())
                .map(|one| one.uuid.to_ascii_lowercase())
                .unwrap_or_else(|| inputs.horizon.name.clone())
        );
        let written = Written {
            frame: params.frame.clone(),
            represented: RepresentedParams { interpretation: None },
            naming: params.naming.clone(),
            new_feature: NewFeatureParams {
                feature_type: None,
                interpretation_type: TypeName("resqml22.GenericFeatureInterpretation".to_string()),
                feature_kind: None,
            },
        };
        let surface = Shape {
            name: title.clone(),
            geometry: Geometry {
                kind: "fault-throw",
                patches: vec![Patch {
                    index: 0,
                    node_offset: 0,
                    points: torn.points.clone(),
                    topology: Topology::Triangles(torn.triangles.clone()),
                    closed: Vec::new(),
                }],
            },
            closed: None,
            known: None,
        };
        let mut plan =
            super::triangles::write(&TrianglesInputs { shape: vec![surface] }, &written, &seed, cx)?;
        let at = plan
            .objects
            .iter()
            .position(|one| one.role == "representation")
            .ok_or_else(|| Refusal::new("refused", "the surface's writer made no representation"))?;
        let surface_uuid = plan.objects[at].uuid.clone();
        let file = params.naming.data_file.clone();

        // -- the split edges: one EdgePatch per lip ---------------------------------------
        let mut edge_patches = Vec::new();
        let mut split_edges = 0usize;
        for fault in &torn.faults {
            for lip in [&fault.lip.footwall, &fault.lip.hanging_wall] {
                let rank = edge_patches.len();
                let path = format!("/resqml22/{surface_uuid}/patch0/splitEdges{rank}");
                let pairs: Vec<i64> = lip
                    .windows(2)
                    .flat_map(|pair| [pair[0] as i64, pair[1] as i64])
                    .collect();
                let edges = lip.len().saturating_sub(1);
                split_edges += edges;
                edge_patches.push(json!({
                    "$type": "resqml22.EdgePatch",
                    "SplitEdges": {
                        "$type": "eml23.IntegerExternalArray",
                        "ArrayIntegerType": "arrayOfInt64LE",
                        "NullValue": -1,
                        "CountPerValue": 1,
                        "Values": support::external(&file, &path, &[edges as i64, 2]),
                    },
                }));
                plan.datasets.push(Dataset {
                    file: file.clone(),
                    path,
                    dimensions: vec![edges as i64, 2],
                    values: Numbers::I64(pairs),
                });
            }
        }
        plan.objects[at].json["TrianglePatch"][0]["SplitEdgePatch"] = json!(edge_patches);

        // -- what is said of every object ----------------------------------------------------
        let law = format!(
            "throw(s) = maximum · {}, s along the trace from 0 to 1; the hanging wall dragged over {} m, the offset falling linearly to zero there{}",
            params.profile.law(),
            params.reach,
            if params.heave {
                ", along the fault's measured mean dip"
            } else {
                ", straight down"
            }
        );
        let description = source.as_ref().map(|source| {
            if params.synthetic {
                format!("Synthetic throw, not measured: {law}. Source: {source}")
            } else {
                format!("Throw: {law}. Source: {source}")
            }
        });
        if let Some(text) = &description
            && text.chars().count() > 2000
        {
            return Err(Refusal::on(
                "source",
                format!(
                    "makes a description of {} characters, and a Citation/Description holds 2000 - cite the document shorter",
                    text.chars().count()
                ),
            ));
        }

        // -- the new interpretation ------------------------------------------------------------
        let feature = inputs.feature.known.reference.clone().ok_or_else(|| {
            Refusal::on(
                "feature",
                format!("`{}` was given without a reference to write", inputs.feature.name),
            )
        })?;
        let interpretation_uuid = cx
            .identity
            .uuid(&format!("interpretation:{seed}"), plan.objects.len())?;
        let interpretation_title = format!("{title} - interpretation");
        let interpretation = resqml::HorizonInterpretation {
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
            domain: match unit.as_deref().and_then(Dimension::of) {
                Some(Dimension::Length) => Some(resqml::Domain::Depth),
                _ => None,
            },
            interpreted_feature: crate::crs::reference_of(&feature),
            ..Default::default()
        };
        plan.objects[at].json["RepresentedObject"] = json!({
            "$type": "eml23.DataObjectReference",
            "Uuid": interpretation_uuid,
            "QualifiedType": "resqml22.HorizonInterpretation",
            "Title": interpretation_title,
        });
        plan.objects.push(crate::operation::NewObject {
            uuid: interpretation_uuid.clone(),
            qualified_type: "resqml22.HorizonInterpretation".to_string(),
            role: "interpretation".to_string(),
            title: interpretation_title.clone(),
            json: energyml_core::json::write_value(&interpretation),
        });

        // -- the lips, by the polyline writer ---------------------------------------------------
        let lips_frame = match plan.objects.iter().find(|one| one.role == crate::crs::FRAME_ROLE) {
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
        let lips_written = Written {
            frame: lips_frame,
            represented: RepresentedParams { interpretation: None },
            naming: NamingParams {
                title: Some(crate::declare::Title(format!("{title} - lips"))),
                data_file: file.clone(),
            },
            new_feature: NewFeatureParams {
                feature_type: None,
                interpretation_type: TypeName("resqml22.GenericFeatureInterpretation".to_string()),
                feature_kind: None,
            },
        };
        let mut patches = Vec::new();
        let mut node_offset = 0usize;
        for (index, fault) in torn.faults.iter().enumerate() {
            let points: Vec<[f64; 3]> = fault
                .lip
                .footwall
                .iter()
                .chain(&fault.lip.hanging_wall)
                .map(|&v| torn.points[v as usize])
                .collect();
            let runs = vec![
                fault.lip.footwall.len() as u32,
                fault.lip.hanging_wall.len() as u32,
            ];
            let length = points.len();
            patches.push(Patch {
                index,
                node_offset,
                points,
                topology: Topology::Polylines(runs),
                closed: vec![false, false],
            });
            node_offset += length;
        }
        let lips = Shape {
            name: format!("{title} - lips"),
            geometry: Geometry {
                kind: "lips",
                patches,
            },
            closed: None,
            known: None,
        };
        // **The ranks the surface and the interpretation took are not the
        // lips'**: each writer counts its own from zero.
        let shifted = match cx.identity {
            Identity::Given(list) => Identity::Given(list.iter().skip(plan.objects.len()).cloned().collect()),
            other => other.clone(),
        };
        let lips_cx = Context {
            identity: &shifted,
            when: cx.when,
            purpose: cx.purpose,
            feedback: cx.feedback,
            bag: cx.bag,
        };
        let lips_plan = super::polylines::write(
            &PolylinesInputs {
                shape: vec![lips],
                features: None,
            },
            &lips_written,
            Some(false),
            &format!("{seed}:lips"),
            &lips_cx,
        )?;
        for mut object in lips_plan.objects {
            if object.role == "representation" {
                // The lips' `LineRole` `contact` - *« Used to represent
                // nonsealed contact interpretation parts defined by a
                // horizon/fault intersection »* (Structural.xsd l. 60-64).
                object.json["LineRole"] = json!("contact");
                // **And they represent the torn horizon's interpretation** -
                // the lines where it meets the faults are of that horizon.
                // Without it the lips of the reference model would stand for
                // nothing: a resqml object without an interpretation - so the
                // horizon's interpretation is proposed for them.
                object.json["RepresentedObject"] = json!({
                    "$type": "eml23.DataObjectReference",
                    "Uuid": interpretation_uuid,
                    "QualifiedType": "resqml22.HorizonInterpretation",
                    "Title": interpretation_title,
                });
                object.role = "lips".to_string();
            }
            plan.objects.push(object);
        }
        plan.datasets.extend(lips_plan.datasets);
        for mut shape in lips_plan.shapes {
            shape.id = plan.shapes.len().to_string();
            plan.shapes.push(shape);
        }

        // -- the mark and the sentence ------------------------------------------------------------
        for object in plan.objects.iter_mut() {
            if !matches!(object.role.as_str(), "representation" | "lips") {
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

        // -- what the plan says ----------------------------------------------------------------------
        let mut says: Vec<Note> = Vec::new();
        says.push(Note {
            key: "new-interpretation",
            detail: format!(
                "the torn surface represents a new HorizonInterpretation of `{}` - its points changed (RESQML usage guide 2.2, draft, §7.2.2 p. 72); what the horizon's previous interpretation states beyond its feature is not read here, and not copied",
                feature.title
            ),
        });
        for ((fault, name), throw) in torn.faults.iter().zip(&names).zip(&params.maximum_throw) {
            let mut detail = format!(
                "`{name}`: dips {:.1}° toward {:.1}° as measured on its surface, its hanging wall on that side; {} split edge(s) per lip; the largest offset on the lip is {:.3} m of {throw} m; {} node(s) moved",
                fault.orientation.dip,
                fault.orientation.dip_azimuth,
                fault.lip.footwall.len().saturating_sub(1),
                fault.largest,
                fault.moved
            );
            if !params.heave {
                let off = fault.largest / fault.orientation.dip.to_radians().tan();
                detail.push_str(&format!(
                    " - moved straight down, the hanging wall's lip lies off a plane of that dip by up to {off:.3} m (`heave` moves it along)"
                ));
            }
            says.push(Note {
                key: "thrown",
                detail,
            });
        }
        says.push(Note {
            key: "split-edges",
            detail: format!(
                "{split_edges} split edge(s) in {} SplitEdgePatch - one per lip, the footwall's then the hanging wall's, fault after fault; the schema does not say what an EdgePatch groups (Structural.xsd l. 206-219), and one per lip is this operation's reading",
                torn.faults.len() * 2
            ),
        });
        if torn.added > 0 {
            says.push(Note {
                key: "nodes-added",
                detail: format!(
                    "{} node(s) come from the faults' traces, not from the horizon's nodes",
                    torn.added
                ),
            });
        }
        if torn.duplicates > 0 {
            says.push(Note {
                key: "duplicates",
                detail: format!("{} node(s) of the horizon share an (x, y) with an earlier one and are not in the torn surface", torn.duplicates),
            });
        }
        if torn.flipped > 0 {
            says.push(Note {
                key: "flipped",
                detail: format!("{} triangle(s) turned over in plan under the offset - a heave larger than the triangles near the lip; said, not corrected", torn.flipped),
            });
        }
        if params.synthetic {
            says.push(Note {
                key: "synthetic",
                detail: "the throw is invented: the surface, its interpretation and the lips carry `synthetic = true` and the law and its source in their description".to_string(),
            });
        }

        // -- the activity -------------------------------------------------------------------------------
        plan.activity.recipe = "fault-throw";
        plan.activity.title = format!("fault-throw → {title}");
        let number = |title: &str, value: usize| Parameter::Double {
            title: title.to_string(),
            value: value as f64,
            uom: None,
        };
        plan.activity.parameters.push(number("faultCount", count));
        plan.activity.parameters.push(number("nodesAdded", torn.added));
        plan.activity
            .parameters
            .push(number("splitNodeCount", torn.split));
        plan.activity
            .parameters
            .push(number("splitEdgeCount", split_edges));
        plan.activity
            .parameters
            .push(number("movedNodeCount", torn.moved));
        plan.activity
            .parameters
            .push(number("flippedTriangleCount", torn.flipped));
        // What the registry cannot record - a list is no single value - and a
        // second run needs, in the order given.
        for (one, throw) in params.throw.iter().zip(&params.maximum_throw) {
            plan.activity.parameters.push(Parameter::Text {
                title: "throw".to_string(),
                value: one.word().to_string(),
            });
            plan.activity.parameters.push(Parameter::Double {
                title: "maximumThrow".to_string(),
                value: *throw,
                uom: Some("m".to_string()),
            });
        }
        says.extend(std::mem::take(&mut plan.says));
        plan.says = says;
        Ok(plan)
    }

    /// The counts the writers record, and what the tear counted.
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
            count("pointCount", "nodes of the torn surface"),
            count("triangleCount", "triangles of the torn surface"),
            count("faultCount", "faults the horizon is torn along"),
            count("nodesAdded", "nodes the faults' traces added to the horizon's"),
            count("splitNodeCount", "nodes duplicated along the traces"),
            count("splitEdgeCount", "split edges declared, both lips of every fault"),
            count("movedNodeCount", "nodes the offset moved"),
            count("flippedTriangleCount", "triangles the offset turned over in plan"),
        ])
    }
}

/// Which way the vertical axis points, and in what unit - the horizon's own
/// frame when the bag knows it, else the frame named, else the one created
/// here (`crs::build`: an identity compound over metres, counting up).
fn vertical_of(
    inputs: &FaultThrowInputs,
    params: &FrameParams,
    cx: &Context<'_>,
) -> Result<(Option<bool>, Option<String>), Refusal> {
    if let Some(known) = inputs.horizon.known.as_ref()
        && known.crs.is_some()
        && (known.vertical_up.is_some() || known.vertical_uom.is_some())
    {
        return Ok((known.vertical_up, known.vertical_uom.clone()));
    }
    let mut scratch = 0usize;
    let frame = support::frame_in(&cx.known(), params, cx.identity, cx.when, &mut scratch)?;
    if frame.kind == Some(crate::crs::FrameKind::Geographic) {
        return Err(Refusal::on(
            if params.crs.is_some() { "crs" } else { "project" },
            "the frame measures its horizontal axes in degrees while a throw and a reach are metres - name a projected system in `project`, or a LocalEngineeringCompoundCrs as `crs`",
        ));
    }
    if !frame.objects.is_empty() {
        if let (Some(source), Some(project)) = (params.source_epsg, params.project)
            && source != project
        {
            return Err(Refusal::on(
                "project",
                format!(
                    "the horizon's nodes are torn as they are given, never reprojected: with `sourceEpsg` {}, give `project` {} or leave it out",
                    source.0, source.0
                ),
            ));
        }
        // `crs::build` writes an identity compound over metres, counting up.
        return Ok((Some(true), Some("m".to_string())));
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
    Ok((
        in_frame.iter().find_map(|one| one.vertical_up),
        in_frame.iter().find_map(|one| one.vertical_uom.clone()),
    ))
}
