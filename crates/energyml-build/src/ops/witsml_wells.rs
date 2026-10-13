//! `witsml-wells` — the WITSML 2.1 side of the wells a package holds in RESQML:
//! a `Well` per head, a `Wellbore` per RESQML wellbore, a `Trajectory` per
//! RESQML trajectory, its stations read off the RESQML frames.
//!
//! Step 10 of the build plan of the synthetic model Lutetia:
//! `witsml-wells` (the door learns a third package) — a `Well` (one per head), a `Wellbore`
//! (GTH-3 has two: ST1 with `ParentWellbore` …), a `Trajectory` +
//! stations (a single `Definitive` one per hole, surface to bottom).
//! No operation wrote a WITSML object before this one.
//!
//! # What is read, and from where
//!
//! **Wellbore frames**, as the package states them: each names its trajectory,
//! the trajectory its datum (RESQML 2.2 `Wells.xsd`, `MdInterval.Datum`), and
//! the frame the wellbore it represents — all resolved at the boundary into
//! [`crate::pieces::Known`], so the plan reads no package. The frame's nodes
//! are where its trajectory passes at its measured depths: for a trajectory of
//! this crate, its stations.
//!
//! # What is decided, and how honestly
//!
//! - **a `Well` is a head, and a head is a datum.** WITSML: the `Well` holds
//!   *“all information that is the same for all wellbores (sidetracks)”* and
//!   its surface location *“is shared by all wellbores within the well”*
//!   (`Well.xsd`). The frames that measure from one datum are one well's; the
//!   `Well`'s uuid comes from that datum's, so another run over the same
//!   package names the same well;
//! - **a `Wellbore` is a RESQML wellbore interpretation**, the object a frame
//!   represents: two frames of one interpretation are one hole;
//! - **a sidetrack is found, never assumed.** *“No parent = starts from
//!   top”* (WITSML TRG 2.1 §20.2 p. 502): a wellbore that shares its datum with
//!   another and whose first node lies on the other's path **at the same
//!   measured depth**, to `ON_PARENT`, leaves it there — which is what a
//!   RESQML `ParentIntersection` states, and what `well-from-plan` writes (the
//!   sidetrack's depths are its parent's, from its parent's datum). The bag
//!   does not carry the trajectory's `ParentIntersection` itself; finding the
//!   same fact in the geometry is a check of it, and a wellbore of a shared
//!   datum that meets no other — or several — is refused by name. So are two
//!   wellbores starting from the top of one datum: that is two wells sharing a
//!   datum, which nothing here decides;
//! - **the stations are the frames' nodes, at the package's own measured
//!   depths** — never recomputed (a given MD is never recomputed). A
//!   definitive trajectory *“must define the geometry of the whole wellbore
//!   (surface to bottom)”* (TRG 2.1 §16.6 p. 350), so a sidetrack's runs
//!   through its parent's stations above the kick-off, then its own: the
//!   composite the same paragraph allows (*“may represent a composite of
//!   information in many other trajectories”*), tied on by `ParentTrajectory`
//!   and `MdTieOn`, its kick-off station a `tie in point`;
//! - **inclination and azimuth are derived, not measured**:
//!   `energyml_analysis::survey` inverts the minimum curvature method over the
//!   stations, and checks that every interval **is** one arc from the
//!   direction the hole has. A line that is not — straight segments meeting
//!   at an angle — keeps its stations, positions and depths, and gets **no**
//!   angle rather than a made-up one; the plan says which and why. Where the
//!   hole is vertical, no azimuth is written: there is none;
//! - **vertical depth and displacement are from the datum, where the datum is
//!   a station** — MD 0 is the datum by definition of a measured depth. A
//!   trajectory whose first station is deeper does not say where the hole runs
//!   above it: no TVD, no displacement, and not definitive — said;
//! - **north is grid north where it is checked**: the frame's second axis is
//!   read as north and its first as east — the axes every compound of this
//!   crate has — and they are the projected system's own when the datum,
//!   placed in the projected system at the boundary (`Known::wellhead`), is
//!   where the frame puts its MD 0 station. Otherwise the azimuths are
//!   measured from the frame's own north, and `AziRef` says `plant north`:
//!   *“a direction in a local engineering coordinate reference system”*
//!   (eml 2.3 `NorthReferenceKind`);
//! - **each station is located in the frame the RESQML geometry is in** — an
//!   `EngineeringCompoundPosition` naming the same compound — so the two
//!   standards place one hole with one set of numbers;
//! - **what the wells are is said, not read**: `existence` writes eml's
//!   `Existence` (and `MdPlanned` rather than `Md` for a planned hole). RESQML
//!   says it on the interpretation (`IsDrilled`), which what is known of a
//!   frame does not carry. `synthetic` marks every object the way
//!   an invented object must be marked;
//! - **same content, same uuid**: the `Well` from its datum, the `Wellbore`
//!   from its interpretation, the `Trajectory` from its RESQML trajectory
//!   **and its stations** — a moved node is another trajectory, never the
//!   first one rewritten.
//!
//! # What is not written
//!
//! - **the RESQML side of the link**:
//!   `WellboreFeature.WitsmlWellbore` and
//!   `WellboreTrajectoryRepresentation.WitsmlTrajectory` (RESQML 2.2
//!   `Wells.xsd`) are fields of RESQML objects already in the package. A plan
//!   does not read an object whole, and rewriting one from what the bag knows
//!   would lose what it holds — the BRGM aliases on a BSS feature, to begin
//!   with. What the WITSML objects share with them is written: the datum
//!   (`DefaultMdDatum`, the index's datum), the compound (each station's
//!   location), the names;
//! - the markers, the geology, the logs and channel sets, the geometry and
//!   the completions: nothing in
//!   a frame states them, or the units a marker would name are not on the
//!   frames yet;
//! - a geographic location (the Well asks for one): it needs a `Geographic2dCrs`
//!   of WGS 84 in the package to name, and a reprojection. The projected one is
//!   written when `projected` is given.
//!
//! # References
//!
//! - API Bulletin D20, *Directional Drilling Survey Calculation Methods and Terminology*, 1985; and Sawaryn & Thorogood, *A Compendium of Directional Calculations Based on the Minimum Curvature Method* (SPE 84246, SPE Drilling & Completion 20(1), 2005), as cited in `energyml-analysis` (`well_plan`, `survey`).
//!   TODO(reference): verify — these citations are carried over from `energyml-analysis`, whose own copy of them is pending verification.

use energyml_analysis::survey::{self, Direction, Offset, Point};
use energyml_domain::provenance::Parameter;
use energyml_domain::wellbore_table::stem;
use energyml_eml_2_3 as eml;
use energyml_geometry::geometry::Topology;
use energyml_witsml_2_1 as witsml;

use crate::crs::FrameKind;
use crate::declare::{Choice, Inputs, ObjectIn, Params, Shape};
use crate::operation::{
    About, ActivityDraft, Category, Context, Made, Need, NewObject, Note, Operation, Plan, Refusal, Slot,
};
use crate::pieces::Ref;

use super::support;

/// The operation.
pub struct WitsmlWells;

/// How far, in metres, a sidetrack's first node may be from its parent's path
/// at the same measured depth — a millimetre. The two are one point the
/// package states twice, read back through two frames; a real kick-off off
/// another's path is metres away, not a millimetre.
const ON_PARENT: f64 = 1e-3;

/// How far, in metres, the datum placed in the projected system may be from
/// the frame's MD 0 station for the frame's axes to be the projected system's
/// — a micrometre: a placement through an identity compound is exact.
const SAME_AXES: f64 = 1e-6;

/// How far, in metres, two frames of one trajectory may place one measured
/// depth — a micrometre.
const SAME_NODE: f64 = 1e-6;

/// What the schema allows in `Well/Country` and `Well/Basin` (`String64`).
const SHORT_TEXT: usize = 64;

/// What the wells are — eml 2.3's `ExistenceKind`, a closed list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum Existence {
    /// *“a concrete, real implementation currently setup in the field”* — a
    /// drilled hole.
    Actual,
    /// *“a planned implementation of an object under study and/or analysis,
    /// subject to evolve, prior to being concretely deployed”*.
    Planned,
    /// *“generated as a result of a simulation”*.
    Simulated,
    /// *“a concrete implementation currently setup in the field for test
    /// purpose”*.
    Test,
}

impl Existence {
    fn written(self) -> eml::ExistenceKindExt {
        match self {
            Existence::Actual => eml::ExistenceKindExt::Actual,
            Existence::Planned => eml::ExistenceKindExt::Planned,
            Existence::Simulated => eml::ExistenceKindExt::Simulated,
            Existence::Test => eml::ExistenceKindExt::Test,
        }
    }
}

/// What `witsml-wells` is told.
#[derive(Debug, Params)]
pub struct WitsmlWellsParams {
    /// what the wells are — actual (drilled), planned, simulated or test: eml's Existence, written on every object made; left out, none is written
    ///
    /// **Said, not read**: RESQML states it on the interpretation
    /// (`WellboreInterpretation.IsDrilled`), which what is known of a frame
    /// does not carry. It also decides whether a wellbore's deepest depth is
    /// written `Md` (actual) or `MdPlanned` (planned).
    pub existence: Option<Existence>,
    /// the wells are invented: every object made carries synthetic = true and says so in its description
    ///
    /// `ExtensionNameValue` `synthetic = true` and a sentence
    /// in `Citation/Description` on every invented object.
    #[param(default = false, optional)]
    pub synthetic: bool,
    /// the country the wells are in, written on each Well as given — never guessed from where they are
    pub country: Option<String>,
    /// the basin the wells are in, written on each Well as given
    pub basin: Option<String>,
}

/// What `witsml-wells` takes.
#[derive(Debug, Inputs)]
pub struct WitsmlWellsInputs {
    /// the RESQML wellbore frames — each names its trajectory, its datum and the wellbore it represents
    ///
    /// **Left empty, every frame of the package.** A sidetrack and its parent
    /// must come in the same run: which wellbore leaves which is read between
    /// them.
    #[input(
        least = 1,
        topology = "polylines",
        types = "resqml22.WellboreFrameRepresentation",
        every
    )]
    pub frames: Vec<Shape>,
    /// the ProjectedCrs the frames' compound is built on — given, each Well is located in it too
    #[input(types = "eml23.ProjectedCrs")]
    pub projected: Option<ObjectIn>,
    /// the geologic unit the wellbores are drilled for — written as each Wellbore's TargetGeologicUnitInterpretation
    #[input(
        types = "resqml22.StratigraphicUnitInterpretation, resqml22.GeologicUnitInterpretation, resqml22.RockFluidUnitInterpretation"
    )]
    pub target: Option<ObjectIn>,
}

/// A node of a frame: its measured depth, and where it is in the frame's own
/// axes.
#[derive(Debug, Clone, Copy)]
struct Node {
    md: f64,
    at: [f64; 3],
}

/// One frame, as the bag knows it.
struct Read {
    /// The frame's title — what a refusal names.
    name: String,
    interpretation: Ref,
    trajectory: Ref,
    datum: Ref,
    crs: Ref,
    kind: FrameKind,
    up: bool,
    wellhead: Option<[f64; 3]>,
    epsg: Option<i64>,
    nodes: Vec<Node>,
}

/// One RESQML trajectory: the nodes its frames place, merged.
struct Traj {
    reference: Ref,
    bore: usize,
    nodes: Vec<Node>,
}

/// Where a sidetrack leaves its parent.
#[derive(Debug, Clone, Copy)]
struct Kickoff {
    bore: usize,
    trajectory: usize,
    md: f64,
}

/// One RESQML wellbore — an interpretation.
struct Bore {
    interpretation: Ref,
    name: String,
    head: usize,
    trajectories: Vec<usize>,
    parent: Option<Kickoff>,
}

/// One head: the wellbores measured from one datum, in one frame.
struct Head {
    datum: Ref,
    crs: Ref,
    kind: FrameKind,
    up: bool,
    wellhead: Option<[f64; 3]>,
    epsg: Option<i64>,
    bores: Vec<usize>,
}

/// Which way north is, for one trajectory.
enum North {
    /// The frame's axes are the projected system's.
    Grid,
    /// The frame's own north, and why not grid north.
    Plant(String),
}

/// What the survey read of one trajectory.
struct Surveyed {
    directions: Result<Vec<Direction>, String>,
    /// Where its MD 0 station is, when it has one.
    datum_at: Option<usize>,
    offsets: Option<Vec<Offset>>,
    north: North,
}

fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn dor(one: &Ref) -> eml::DataObjectReference {
    crate::crs::reference_of(one)
}

fn metres(value: f64) -> eml::LengthMeasure {
    eml::LengthMeasure {
        value,
        uom: eml::LengthUom::M,
    }
}

/// A measured depth, in metres, from the datum its object names by default.
fn measured(value: f64) -> eml::MeasuredDepth {
    eml::MeasuredDepth {
        measured_depth: eml::LengthMeasureExt {
            value,
            uom: eml::LengthUomExt::M,
        },
        datum: None,
    }
}

/// A vertical depth below the datum its object names by default.
fn vertical_depth(value: f64) -> eml::AbstractVerticalDepth {
    eml::AbstractVerticalDepth::DatumVerticalDepth(eml::DatumVerticalDepth {
        vertical_depth: eml::LengthMeasureExt {
            value,
            uom: eml::LengthUomExt::M,
        },
        trajectory: None,
        datum: None,
    })
}

fn degrees(value: f64) -> eml::PlaneAngleMeasure {
    eml::PlaneAngleMeasure {
        value,
        uom: eml::PlaneAngleUom::Dega,
    }
}

/// A node, located in the frame the RESQML geometry is in.
fn located(node: &Node, head: &Head) -> eml::AbstractPosition {
    match head.kind {
        FrameKind::Projected => eml::AbstractPosition::Projected2dPosition(eml::Projected2dPosition {
            coordinate1: node.at[0],
            coordinate2: node.at[1],
            projected_crs: dor(&head.crs),
        }),
        _ => eml::AbstractPosition::EngineeringCompoundPosition(eml::EngineeringCompoundPosition {
            coordinate1: node.at[0],
            coordinate2: node.at[1],
            vertical_coordinate: node.at[2],
            local_engineering_compound_crs: dor(&head.crs),
        }),
    }
}

/// A frame, or the refusal that says what it lacks.
fn read_frame(one: &Shape) -> Result<Read, Refusal> {
    let known = one.known.as_ref();
    let name = known
        .and_then(|k| k.reference.as_ref())
        .map(|r| r.title.clone())
        .unwrap_or_else(|| one.name.clone());
    let refuse = |why: String| Refusal::on("frames", why);
    let Some(known) = known else {
        return Err(refuse(format!(
            "`{name}` comes with nothing known of it: a wellbore is written from what the package says of a RESQML frame — its measured depths, its trajectory, its datum, the wellbore it represents"
        )));
    };
    let mds = known.node_mds.as_deref().ok_or_else(|| {
        refuse(format!("`{name}` states no measured depth for its nodes — it is no wellbore frame, or this build cannot read its arrays"))
    })?;
    let points: Vec<[f64; 3]> = one
        .geometry
        .patches
        .iter()
        .flat_map(|patch| patch.points.iter().copied())
        .collect();
    if points.len() != mds.len() {
        return Err(refuse(format!(
            "`{name}` has {} node(s) and {} measured depth(s): each node is where its depth is",
            points.len(),
            mds.len()
        )));
    }
    if mds.len() < 2 {
        return Err(refuse(format!(
            "`{name}` has {} node(s): a trajectory is two stations at least",
            mds.len()
        )));
    }
    if let Some(at) = mds
        .iter()
        .zip(&points)
        .position(|(md, place)| !md.is_finite() || place.iter().any(|v| !v.is_finite()))
    {
        return Err(refuse(format!(
            "node {} of `{name}` has a measured depth or a position that is not a number",
            at + 1
        )));
    }
    if let Some(at) = mds.windows(2).position(|pair| pair[1] <= pair[0]) {
        return Err(refuse(format!(
            "the measured depths of `{name}` do not increase at node {}: {} then {}",
            at + 2,
            mds[at],
            mds[at + 1]
        )));
    }
    let interpretation = known.interpretation.clone().ok_or_else(|| {
        refuse(format!("`{name}` represents no interpretation: a WITSML Wellbore is the RESQML wellbore a frame represents"))
    })?;
    let trajectory = known.trajectory.clone().ok_or_else(|| {
        refuse(format!(
            "`{name}` names no trajectory: the stations of a WITSML trajectory are those of a RESQML one"
        ))
    })?;
    let datum = known.datum.clone().ok_or_else(|| {
        refuse(format!(
            "`{name}` names no datum through its trajectory — a WITSML Well is the head its wellbores are measured from, and the datum is where the package says that head is (RESQML 2.2 Wells.xsd, `MdInterval.Datum`)"
        ))
    })?;
    let crs = known.crs.clone().ok_or_else(|| {
        refuse(format!(
            "`{name}` names no coordinate system: where its stations are cannot be said"
        ))
    })?;
    let kind = match crate::crs::frame_kind(&crs.qualified_type) {
        Ok(FrameKind::Geographic) => {
            return Err(refuse(format!(
                "`{name}` is in `{}`, which measures its horizontal axes in degrees while its depths are metres: no inclination is read across the two — a wellbore in degrees is refused, as `well-from-polyline` refuses it",
                crs.title
            )));
        }
        Ok(FrameKind::Legacy) => {
            return Err(refuse(format!(
                "`{name}` is in `{}`, a RESQML 2.0.1 system, and a WITSML 2.1 station is located in an eml 2.3 one: translate the package first",
                crs.title
            )));
        }
        Ok(kind) => kind,
        Err(why) => return Err(refuse(format!("`{name}`: {why}"))),
    };
    let up = known.vertical_up.ok_or_else(|| {
        refuse(format!(
            "`{name}` is in `{}`, which does not say which way its vertical axis points — which way down is decides every inclination: it cannot be guessed",
            crs.title
        ))
    })?;
    if let Some(uom) = known.vertical_uom.as_deref()
        && uom != "m"
    {
        return Err(refuse(format!(
            "`{name}` counts its vertical axis in `{uom}`, and the stations are written in metres"
        )));
    }
    let nodes = mds
        .iter()
        .zip(&points)
        .map(|(&md, &at)| Node { md, at })
        .collect();
    Ok(Read {
        name,
        interpretation,
        trajectory,
        datum,
        crs,
        kind,
        up,
        wellhead: known.wellhead,
        epsg: known.crs_epsg,
        nodes,
    })
}

/// A second frame of a trajectory: its nodes join the first's, and a depth
/// both place must be one point.
fn merge(traj: &mut Traj, one: &Read) -> Result<(), Refusal> {
    for node in &one.nodes {
        let held = traj
            .nodes
            .iter()
            .find(|held| held.md == node.md)
            .map(|held| held.at);
        match held {
            Some(at) if distance(at, node.at) > SAME_NODE => {
                return Err(Refusal::on(
                    "frames",
                    format!(
                        "two frames of `{}` place MD {} at two points, {at:?} and {:?} — one trajectory is at one place at one depth",
                        traj.reference.title, node.md, node.at
                    ),
                ));
            }
            Some(_) => {}
            None => traj.nodes.push(*node),
        }
    }
    traj.nodes.sort_by(|a, b| a.md.total_cmp(&b.md));
    Ok(())
}

/// Heads, wellbores and trajectories.
type Gathered = (Vec<Head>, Vec<Bore>, Vec<Traj>);

/// Heads, wellbores and trajectories, in the order the frames came.
fn gather(read: &[Read]) -> Result<Gathered, Refusal> {
    let mut heads: Vec<Head> = Vec::new();
    let mut bores: Vec<Bore> = Vec::new();
    let mut trajs: Vec<Traj> = Vec::new();
    for one in read {
        let h = match heads
            .iter()
            .position(|head| same(&head.datum.uuid, &one.datum.uuid))
        {
            Some(h) => {
                let head = &mut heads[h];
                if !same(&head.crs.uuid, &one.crs.uuid) || head.up != one.up {
                    return Err(Refusal::on(
                        "frames",
                        format!(
                            "`{}` measures from `{}` in `{}`, and another frame of that datum is in `{}`: the wellbores of one head are read in one frame",
                            one.name, one.datum.title, one.crs.title, head.crs.title
                        ),
                    ));
                }
                head.wellhead = head.wellhead.or(one.wellhead);
                head.epsg = head.epsg.or(one.epsg);
                h
            }
            None => {
                heads.push(Head {
                    datum: one.datum.clone(),
                    crs: one.crs.clone(),
                    kind: one.kind,
                    up: one.up,
                    wellhead: one.wellhead,
                    epsg: one.epsg,
                    bores: Vec::new(),
                });
                heads.len() - 1
            }
        };
        let b = match bores
            .iter()
            .position(|bore| same(&bore.interpretation.uuid, &one.interpretation.uuid))
        {
            Some(b) if bores[b].head != h => {
                return Err(Refusal::on(
                    "frames",
                    format!(
                        "`{}` represents `{}`, which another frame measures from another datum — one wellbore is measured from one head",
                        one.name, one.interpretation.title
                    ),
                ));
            }
            Some(b) => b,
            None => {
                let name = match stem(&one.interpretation.title) {
                    "" => stem(&one.name).to_string(),
                    found => found.to_string(),
                };
                bores.push(Bore {
                    interpretation: one.interpretation.clone(),
                    name,
                    head: h,
                    trajectories: Vec::new(),
                    parent: None,
                });
                heads[h].bores.push(bores.len() - 1);
                bores.len() - 1
            }
        };
        match trajs
            .iter()
            .position(|traj| same(&traj.reference.uuid, &one.trajectory.uuid))
        {
            Some(t) if trajs[t].bore != b => {
                return Err(Refusal::on(
                    "frames",
                    format!(
                        "`{}` samples `{}`, which another frame gives to another wellbore",
                        one.name, one.trajectory.title
                    ),
                ));
            }
            Some(t) => merge(&mut trajs[t], one)?,
            None => {
                trajs.push(Traj {
                    reference: one.trajectory.clone(),
                    bore: b,
                    nodes: one.nodes.clone(),
                });
                bores[b].trajectories.push(trajs.len() - 1);
            }
        }
    }
    Ok((heads, bores, trajs))
}

/// Where a wellbore starts: the shallowest first node of its trajectories.
fn start_of(bore: &Bore, trajs: &[Traj]) -> Node {
    bore.trajectories
        .iter()
        .map(|&t| trajs[t].nodes[0])
        .min_by(|a, b| a.md.total_cmp(&b.md))
        .unwrap_or(Node {
            md: f64::INFINITY,
            at: [f64::NAN; 3],
        })
}

/// Where a path of nodes is at `md` — between two of its nodes, on the
/// straight line joining them, which is the RESQML linear spline through them.
/// `None` above its first node (a path does not reach there) or below its
/// last.
fn on_path(nodes: &[Node], md: f64) -> Option<[f64; 3]> {
    let (first, last) = (nodes.first()?, nodes.last()?);
    if md <= first.md || md > last.md {
        return None;
    }
    let at = nodes.windows(2).position(|pair| md <= pair[1].md)?;
    let (a, b) = (nodes[at], nodes[at + 1]);
    let f = (md - a.md) / (b.md - a.md);
    Some([
        a.at[0] + f * (b.at[0] - a.at[0]),
        a.at[1] + f * (b.at[1] - a.at[1]),
        a.at[2] + f * (b.at[2] - a.at[2]),
    ])
}

/// Which wellbore of a head leaves which — see the module.
fn sidetracks(heads: &[Head], bores: &mut [Bore], trajs: &[Traj]) -> Result<(), Refusal> {
    for head in heads {
        if head.bores.len() < 2 {
            continue;
        }
        for &b in &head.bores {
            let start = start_of(&bores[b], trajs);
            let mut found: Vec<Kickoff> = Vec::new();
            for &a in head.bores.iter().filter(|&&a| a != b) {
                for &t in &bores[a].trajectories {
                    if let Some(on) = on_path(&trajs[t].nodes, start.md)
                        && distance(on, start.at) <= ON_PARENT
                    {
                        found.push(Kickoff {
                            bore: a,
                            trajectory: t,
                            md: start.md,
                        });
                    }
                }
            }
            match found.as_slice() {
                [] => {}
                [one] => bores[b].parent = Some(*one),
                many => {
                    let named: Vec<String> = many
                        .iter()
                        .map(|k| format!("`{}`", trajs[k.trajectory].reference.title))
                        .collect();
                    return Err(Refusal::on(
                        "frames",
                        format!(
                            "`{}` starts at MD {} on {} trajectories measured from its datum — {}: which one it leaves cannot be told",
                            bores[b].name,
                            start.md,
                            many.len(),
                            named.join(", ")
                        ),
                    ));
                }
            }
        }
        let roots: Vec<String> = head
            .bores
            .iter()
            .filter(|&&b| bores[b].parent.is_none())
            .map(|&b| format!("`{}`", bores[b].name))
            .collect();
        if roots.len() != 1 {
            return Err(Refusal::on(
                "frames",
                format!(
                    "{} are measured from `{}` and none of them leaves another: a Well is one head, and a sidetrack starts on its parent — “No parent = starts from top” (WITSML TRG 2.1 §20.2 p. 502). Two holes from the top of one datum are two wells sharing a datum, which this operation does not decide",
                    roots.join(" and "),
                    head.datum.title
                ),
            ));
        }
    }
    Ok(())
}

/// What makes a trajectory what it is: its depths and its places.
fn content(nodes: &[Node]) -> String {
    nodes
        .iter()
        .map(|n| format!("{:?}@{:?},{:?},{:?}", n.md, n.at[0], n.at[1], n.at[2]))
        .collect::<Vec<_>>()
        .join(";")
}

/// The stations of one trajectory, read as a survey.
fn survey_of(nodes: &[Node], head: &Head) -> Surveyed {
    // North is the frame's second axis, east its first; down is its vertical
    // axis, or the opposite of it when it counts up.
    let points: Vec<Point> = nodes
        .iter()
        .map(|n| Point {
            md: n.md,
            at: [n.at[1], n.at[0], if head.up { -n.at[2] } else { n.at[2] }],
        })
        .collect();
    let directions = survey::directions(&points).map_err(|why| why.to_string());
    let datum_at = nodes.iter().position(|n| n.md == 0.0);
    let offsets = datum_at.map(|at| survey::offsets(&points, points[at].at));
    let north = match (head.kind, datum_at, head.wellhead) {
        (FrameKind::Projected, _, _) => North::Grid,
        (_, Some(at), Some(w)) if (w[0] - nodes[at].at[0]).abs() <= SAME_AXES && (w[1] - nodes[at].at[1]).abs() <= SAME_AXES => North::Grid,
        (_, Some(at), Some(w)) => North::Plant(format!(
            "its datum is at ({}, {}) in the projected system and at ({}, {}) in the frame's own axes — the frame is offset or turned from it",
            w[0], w[1], nodes[at].at[0], nodes[at].at[1]
        )),
        (_, Some(_), None) => North::Plant("the package does not place its datum in the projected system".to_string()),
        (_, None, _) => North::Plant("its first station is below its datum, so its axes cannot be checked against the projected system".to_string()),
    };
    Surveyed {
        directions,
        datum_at,
        offsets,
        north,
    }
}

/// A list of names, in words.
fn listed(names: &[String]) -> String {
    names
        .iter()
        .map(|one| format!("`{one}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The seed a WITSML `Wellbore`'s uuid is derived from: the RESQML wellbore
/// interpretation it stands for. The one formula — `witsml-logs` references
/// the same Wellbore without writing it, and recomputes its uuid from here.
pub(crate) fn wellbore_seed(interpretation_uuid: &str) -> String {
    format!(
        "witsml-wells:wellbore:{}",
        interpretation_uuid.to_ascii_lowercase()
    )
}

impl Operation for WitsmlWells {
    type Params = WitsmlWellsParams;
    type Inputs = WitsmlWellsInputs;

    fn about(&self) -> About {
        About {
            name: "witsml-wells",
            describe: "the WITSML side of the RESQML wells: a Well per head, a Wellbore per RESQML wellbore — a sidetrack naming its parent — and a Trajectory per RESQML trajectory, its stations the frames' nodes with the inclination and azimuth they make",
            category: Category::Wells,
            makes: &["witsml21.Well", "witsml21.Wellbore", "witsml21.Trajectory"],
            recipe: "witsml-wells",
            represents: None,
        }
    }

    fn reports(&self) -> Vec<Slot> {
        let number = || Need::Number {
            uom: None,
            least: None,
            most: None,
        };
        vec![
            Slot::required(
                "wellCount",
                number(),
                "Well objects written — one per datum the wellbores are measured from",
            ),
            Slot::required(
                "wellboreCount",
                number(),
                "Wellbore objects written — one per RESQML wellbore interpretation",
            ),
            Slot::required(
                "trajectoryCount",
                number(),
                "Trajectory objects written — one per RESQML trajectory",
            ),
            Slot::required(
                "stationCount",
                number(),
                "trajectory stations written, over every trajectory",
            ),
            Slot::required(
                "sidetrackCount",
                number(),
                "wellbores written as the sidetrack of another",
            ),
        ]
    }

    fn plan(
        &self,
        inputs: WitsmlWellsInputs,
        params: WitsmlWellsParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        if inputs.frames.is_empty() {
            return Err(Refusal::on(
                "frames",
                "it takes wellbore frames; the input holds none, and the package gives none either",
            ));
        }
        for (slot, text) in [("country", &params.country), ("basin", &params.basin)] {
            if let Some(text) = text
                && text.chars().count() > SHORT_TEXT
            {
                return Err(Refusal::on(
                    slot,
                    format!(
                        "holds {} characters, and the schema caps it at {SHORT_TEXT} (Well.xsd, String64)",
                        text.chars().count()
                    ),
                ));
            }
        }

        // -- what is known of the inputs -------------------------------------
        let read: Vec<Read> = inputs.frames.iter().map(read_frame).collect::<Result<_, _>>()?;
        let projected: Option<(Ref, i64)> = match &inputs.projected {
            None => None,
            Some(one) => {
                let Some(reference) = one.known.reference.clone() else {
                    return Err(Refusal::on(
                        "projected",
                        format!("`{}` was given without a reference to write", one.name),
                    ));
                };
                if crate::crs::frame_kind(&reference.qualified_type) != Ok(FrameKind::Projected) {
                    return Err(Refusal::on(
                        "projected",
                        format!(
                            "`{}` is a `{}`, not a ProjectedCrs",
                            reference.title, reference.qualified_type
                        ),
                    ));
                }
                let Some(code) = one.known.crs_epsg else {
                    return Err(Refusal::on(
                        "projected",
                        format!(
                            "`{}` names no EPSG code, so whether the wells' heads are in it cannot be told",
                            reference.title
                        ),
                    ));
                };
                Some((reference, code))
            }
        };
        let target: Option<Ref> = match &inputs.target {
            None => None,
            Some(one) => {
                let Some(reference) = one.known.reference.clone() else {
                    return Err(Refusal::on(
                        "target",
                        format!("`{}` was given without a reference to write", one.name),
                    ));
                };
                // “Pointer to a RESQML GeologicUnitInterpretation” (WITSML
                // 2.1 Wellbore.xsd) — which a stratigraphic and a rock-fluid
                // unit interpretation both are.
                if !reference.qualified_type.ends_with("UnitInterpretation") {
                    return Err(Refusal::on(
                        "target",
                        format!(
                            "`{}` is a `{}`, and a wellbore's target is a RESQML geologic unit interpretation",
                            reference.title, reference.qualified_type
                        ),
                    ));
                }
                Some(reference)
            }
        };

        // -- heads, wellbores, trajectories, and who leaves whom -------------
        let (mut heads, mut bores, trajs) = gather(&read)?;
        sidetracks(&heads, &mut bores, &trajs)?;
        // Parents before their sidetracks: a parent starts higher.
        for head in &mut heads {
            head.bores.sort_by(|&a, &b| {
                start_of(&bores[a], &trajs)
                    .md
                    .total_cmp(&start_of(&bores[b], &trajs).md)
            });
        }
        if let Some((reference, code)) = &projected {
            for head in &heads {
                if let Some(epsg) = head.epsg
                    && epsg != *code
                {
                    return Err(Refusal::on(
                        "projected",
                        format!(
                            "`{}` is EPSG:{code}, and the wells measured from `{}` are in `{}`, over EPSG:{epsg} — a head placed in one system is not a position in another",
                            reference.title, head.datum.title, head.crs.title
                        ),
                    ));
                }
            }
        }

        // -- each trajectory's stations: its own, after its parent's --------
        let mut composite: Vec<Vec<Node>> = vec![Vec::new(); trajs.len()];
        let mut tie: Vec<Option<Kickoff>> = vec![None; trajs.len()];
        for head in &heads {
            for &b in &head.bores {
                for &t in &bores[b].trajectories {
                    let own = &trajs[t].nodes;
                    let nodes = match bores[b].parent {
                        Some(k) if own[0].md == k.md => {
                            tie[t] = Some(k);
                            let mut all: Vec<Node> = composite[k.trajectory]
                                .iter()
                                .copied()
                                .filter(|n| n.md < k.md)
                                .collect();
                            all.extend(own.iter().copied());
                            all
                        }
                        _ => own.clone(),
                    };
                    composite[t] = nodes;
                }
            }
        }
        let surveyed: Vec<Surveyed> = trajs
            .iter()
            .enumerate()
            .map(|(t, traj)| survey_of(&composite[t], &heads[bores[traj.bore].head]))
            .collect();

        // -- identity: the head, the interpretation, the stations ------------
        let mut rank = 0usize;
        let mut well_uuid = vec![String::new(); heads.len()];
        let mut bore_uuid = vec![String::new(); bores.len()];
        let mut traj_uuid = vec![String::new(); trajs.len()];
        for (h, head) in heads.iter().enumerate() {
            well_uuid[h] = support::next_uuid(
                cx.identity,
                &format!("witsml-wells:well:{}", head.datum.uuid.to_ascii_lowercase()),
                &mut rank,
            )?;
            for &b in &head.bores {
                bore_uuid[b] = support::next_uuid(
                    cx.identity,
                    &wellbore_seed(&bores[b].interpretation.uuid),
                    &mut rank,
                )?;
                for &t in &bores[b].trajectories {
                    traj_uuid[t] = support::next_uuid(
                        cx.identity,
                        &format!(
                            "witsml-wells:trajectory:{}:{}",
                            trajs[t].reference.uuid.to_ascii_lowercase(),
                            content(&composite[t])
                        ),
                        &mut rank,
                    )?;
                }
            }
        }

        // -- what every object carries ---------------------------------------
        let existence = params.existence.map(Existence::written);
        let citation = |title: &str, of: &str| {
            eml::Citation {
            title: title.to_string(),
            originator: cx.when.originator.clone(),
            creation: cx.when.creation.clone(),
            format: energyml_core::CITATION_FORMAT.to_string(),
            description: params.synthetic.then(|| {
                format!("Synthetic: the WITSML side of the RESQML wellbore `{of}`, which is invented — not a measured well.")
            }),
            ..Default::default()
        }
        };
        let mark = || -> Vec<eml::ExtensionNameValue> {
            if !params.synthetic {
                return Vec::new();
            }
            vec![eml::ExtensionNameValue {
                name: "synthetic".to_string(),
                value: eml::StringMeasure {
                    value: "true".to_string(),
                    uom: None,
                },
                description: Some(
                    "written by `witsml-wells` from a RESQML wellbore declared invented, not measured"
                        .to_string(),
                ),
                ..Default::default()
            }]
        };

        // -- the objects, parents first --------------------------------------
        let mut objects: Vec<NewObject> = Vec::new();
        let mut shapes: Vec<Made> = Vec::new();
        let mut station_count = 0usize;
        let mut well_titles: Vec<String> = Vec::new();
        for (h, head) in heads.iter().enumerate() {
            let root = &bores[head.bores[0]];
            let well_title = root.name.clone();
            well_titles.push(well_title.clone());

            // Where the head is: its datum, as a station, in the frame — and
            // in the projected system when one is given.
            let datum_node: Option<Node> = head
                .bores
                .iter()
                .flat_map(|&b| bores[b].trajectories.iter())
                .find_map(|&t| surveyed[t].datum_at.map(|at| composite[t][at]));
            let mut surface: Vec<eml::AbstractPosition> =
                datum_node.iter().map(|node| located(node, head)).collect();
            let informational = match (&projected, head.wellhead, head.epsg) {
                (Some((reference, _)), Some(w), Some(_)) => {
                    let position = eml::Projected2dPosition {
                        coordinate1: w[0],
                        coordinate2: w[1],
                        projected_crs: dor(reference),
                    };
                    // A frame that is itself that system has said it already.
                    if !(head.kind == FrameKind::Projected
                        && same(&head.crs.uuid, &reference.uuid)
                        && datum_node.is_some())
                    {
                        surface.push(eml::AbstractPosition::Projected2dPosition(position.clone()));
                    }
                    Some(position)
                }
                _ => None,
            };
            let well = witsml::Well {
                citation: citation(&well_title, &root.name),
                existence: existence.clone(),
                extension_name_value: mark(),
                uuid: well_uuid[h].clone(),
                schema_version: "2.1".to_string(),
                // STORE MANAGED — “populated by a store on read” — and
                // still required. Nothing is being drilled or logged into a
                // package sitting in a file: inactive, the alternative a
                // version translation chooses for the same case.
                active_status: eml::ActiveStatusKind::Inactive,
                country: params.country.clone(),
                basin: params.basin.clone(),
                informational_projected_location: informational,
                well_surface_location: surface,
                ..Default::default()
            };
            objects.push(NewObject {
                uuid: well_uuid[h].clone(),
                qualified_type: "witsml21.Well".to_string(),
                role: "well".to_string(),
                title: well_title.clone(),
                json: energyml_core::json::write_value(&well),
            });

            for &b in &head.bores {
                let bore = &bores[b];
                // The deepest station of the wellbore, and its vertical depth
                // when the datum is a station.
                let deepest = bore
                    .trajectories
                    .iter()
                    .map(|&t| {
                        let last = composite[t].len() - 1;
                        (
                            composite[t][last].md,
                            surveyed[t].offsets.as_ref().map(|all| all[last].tvd),
                        )
                    })
                    .max_by(|a, b| a.0.total_cmp(&b.0));
                let kickoff_tvd = bore.parent.and_then(|k| {
                    bore.trajectories.iter().find_map(|&t| {
                        let at = composite[t].iter().position(|n| n.md == k.md)?;
                        surveyed[t].offsets.as_ref().map(|all| all[at].tvd)
                    })
                });
                let mut wellbore = witsml::Wellbore {
                    citation: citation(&bore.name, &bore.name),
                    existence: existence.clone(),
                    extension_name_value: mark(),
                    uuid: bore_uuid[b].clone(),
                    schema_version: "2.1".to_string(),
                    active_status: eml::ActiveStatusKind::Inactive,
                    well: crate::crs::reference_to(&well_uuid[h], "witsml21.Well", &well_title),
                    parent_wellbore: bore.parent.map(|k| {
                        crate::crs::reference_to(&bore_uuid[k.bore], "witsml21.Wellbore", &bores[k.bore].name)
                    }),
                    type_wellbore: bore.parent.map(|_| witsml::WellboreType::Sidetrack),
                    md_kickoff: bore.parent.map(|k| measured(k.md)),
                    tvd_kickoff: kickoff_tvd.map(vertical_depth),
                    default_md_datum: Some(dor(&head.datum)),
                    default_tvd_datum: Some(dor(&head.datum)),
                    target_geologic_unit_interpretation: target.as_ref().map(dor),
                    ..Default::default()
                };
                if let Some((md, tvd)) = deepest {
                    match params.existence {
                        Some(Existence::Planned) => {
                            wellbore.md_planned = Some(measured(md));
                            wellbore.tvd_planned = tvd.map(vertical_depth);
                        }
                        Some(Existence::Actual) => {
                            wellbore.md = Some(measured(md));
                            wellbore.tvd = tvd.map(vertical_depth);
                        }
                        _ => {}
                    }
                }
                objects.push(NewObject {
                    uuid: bore_uuid[b].clone(),
                    qualified_type: "witsml21.Wellbore".to_string(),
                    role: "wellbore".to_string(),
                    title: bore.name.clone(),
                    json: energyml_core::json::write_value(&wellbore),
                });

                for &t in &bore.trajectories {
                    let nodes = &composite[t];
                    let read_back = &surveyed[t];
                    let angles: Option<&Vec<Direction>> = read_back.directions.as_ref().ok();
                    let kickoff = tie[t].map(|k| k.md);
                    let stations: Vec<witsml::TrajectoryStation> = nodes
                        .iter()
                        .enumerate()
                        .map(|(at, node)| {
                            let direction: Option<Direction> = angles.map(|all| all[at]);
                            let offset: Option<Offset> = read_back.offsets.as_ref().map(|all| all[at]);
                            witsml::TrajectoryStation {
                                creation: cx.when.creation.clone(),
                                last_update: cx.when.creation.clone(),
                                uid: format!("station-{at}"),
                                md: measured(node.md),
                                type_traj_station: if Some(node.md) == kickoff {
                                    witsml::TrajStationType::TieInPoint
                                } else {
                                    witsml::TrajStationType::NEAndTVD
                                },
                                calc_algorithm: direction
                                    .map(|_| witsml::TrajStnCalcAlgorithm::MinimumCurvature),
                                tvd: offset.map(|o| vertical_depth(o.tvd)),
                                incl: direction.map(|d| degrees(d.inclination)),
                                azi: direction.and_then(|d| d.azimuth).map(degrees),
                                disp_ns: offset.map(|o| metres(o.north)),
                                disp_ew: offset.map(|o| metres(o.east)),
                                dls: direction.and_then(|d| d.dogleg_severity).map(|value| {
                                    eml::AnglePerLengthMeasure {
                                        value,
                                        uom: eml::AnglePerLengthUom::V1Per30DegaPerM,
                                    }
                                }),
                                location: vec![located(node, head)],
                                ..Default::default()
                            }
                        })
                        .collect();
                    station_count += stations.len();
                    let title = trajs[t].reference.title.clone();
                    let mut remark = format!(
                        "The stations are the nodes of the RESQML trajectory `{title}` ({}), at their own measured depths",
                        trajs[t].reference.uuid
                    );
                    if let Some(k) = tie[t] {
                        remark.push_str(&format!(
                            ", after those of `{}` above the kick-off at MD {}",
                            trajs[k.trajectory].reference.title, k.md
                        ));
                    }
                    remark.push_str(match &read_back.directions {
                        Ok(_) => ". Inclination and azimuth are derived, not measured: the minimum curvature method inverted over the stations.",
                        Err(_) => ". No inclination or azimuth: the stations are not on minimum-curvature arcs.",
                    });
                    let trajectory = witsml::Trajectory {
                        citation: citation(&title, &bore.name),
                        existence: existence.clone(),
                        extension_name_value: mark(),
                        uuid: traj_uuid[t].clone(),
                        schema_version: "2.1".to_string(),
                        active_status: eml::ActiveStatusKind::Inactive,
                        index: eml::GrowingObjectIndex {
                            index_kind: eml::DataIndexKind::MeasuredDepth,
                            index_property_kind: None,
                            uom: eml::UnitOfMeasureExt::M,
                            direction: eml::IndexDirection::Increasing,
                            datum: Some(dor(&head.datum)),
                        },
                        md_interval: Some(eml::MdInterval {
                            comment: None,
                            md_min: nodes[0].md,
                            md_max: nodes[nodes.len() - 1].md,
                            uom: eml::LengthUomExt::M,
                            datum: Some(dor(&head.datum)),
                        }),
                        // Surface to bottom, and the only one of its hole.
                        definitive: (read_back.datum_at == Some(0) && bore.trajectories.len() == 1)
                            .then_some(true),
                        default_md_datum: Some(dor(&head.datum)),
                        default_tvd_datum: Some(dor(&head.datum)),
                        parent_trajectory: tie[t].map(|k| {
                            crate::crs::reference_to(
                                &traj_uuid[k.trajectory],
                                "witsml21.Trajectory",
                                &trajs[k.trajectory].reference.title,
                            )
                        }),
                        md_tie_on: kickoff.map(measured),
                        trajectory_station: stations,
                        wellbore: crate::crs::reference_to(&bore_uuid[b], "witsml21.Wellbore", &bore.name),
                        acquisition_remark: Some(remark),
                        nominal_calc_algorithm: angles
                            .map(|_| witsml::TrajStnCalcAlgorithmExt::MinimumCurvature),
                        nominal_type_traj_station: Some(witsml::TrajStationTypeExt::NEAndTVD),
                        azi_ref: Some(match read_back.north {
                            North::Grid => eml::NorthReferenceKind::GridNorth,
                            North::Plant(_) => eml::NorthReferenceKind::PlantNorth,
                        }),
                        ..Default::default()
                    };
                    objects.push(NewObject {
                        uuid: traj_uuid[t].clone(),
                        qualified_type: "witsml21.Trajectory".to_string(),
                        role: "trajectory".to_string(),
                        title: title.clone(),
                        json: energyml_core::json::write_value(&trajectory),
                    });
                    // The stations as a line, where the RESQML geometry is.
                    let line: Vec<[f64; 3]> = nodes.iter().map(|n| n.at).collect();
                    let count = line.len() as u32;
                    // One root per trajectory, each its own id in the plan.
                    let root = support::made_root(
                        &title,
                        &traj_uuid[t],
                        support::one_patch("polyline", line, Topology::Polylines(vec![count])),
                    );
                    shapes.push(Made {
                        id: shapes.len().to_string(),
                        ..root
                    });
                }
            }
        }

        // -- what is said ------------------------------------------------------
        let name_of = |t: usize| trajs[t].reference.title.clone();
        let sidetrack_count = bores.iter().filter(|bore| bore.parent.is_some()).count();
        let mut says = vec![Note {
            key: "written",
            detail: format!(
                "{} Well(s), {} Wellbore(s), {} Trajectory(ies), {station_count} station(s)",
                heads.len(),
                bores.len(),
                trajs.len()
            ),
        }];
        for bore in &bores {
            let Some(k) = bore.parent else { continue };
            says.push(Note {
                key: "sidetrack",
                detail: format!(
                    "`{}` leaves `{}` at MD {}, where its first node lies on the parent's path: its Wellbore names its parent (ParentWellbore), and its Trajectory ties on at MD {} (ParentTrajectory, MdTieOn) and runs from the surface through the parent's stations above — a definitive trajectory is surface to bottom (WITSML TRG 2.1 §16.6 p. 350)",
                    bore.name, bores[k.bore].name, k.md, k.md
                ),
            });
        }
        let derived: Vec<String> = (0..trajs.len())
            .filter(|&t| surveyed[t].directions.is_ok())
            .map(name_of)
            .collect();
        if !derived.is_empty() {
            says.push(Note {
                key: "angles",
                detail: format!(
                    "inclination and azimuth are derived, not measured — the minimum curvature method inverted over the stations, every interval checked to be one arc: {}",
                    listed(&derived)
                ),
            });
        }
        for (t, one) in surveyed.iter().enumerate() {
            if let Err(why) = &one.directions {
                says.push(Note {
                    key: "angles-not-derived",
                    detail: format!(
                        "`{}` keeps its stations and gets no inclination or azimuth: {why}",
                        name_of(t)
                    ),
                });
            }
        }
        let vertical: usize = surveyed
            .iter()
            .filter_map(|one| one.directions.as_ref().ok())
            .map(|all| all.iter().filter(|d| d.azimuth.is_none()).count())
            .sum();
        if vertical > 0 {
            says.push(Note {
                key: "vertical-no-azimuth",
                detail: format!("{vertical} station(s) where the hole is vertical are written with no azimuth: a vertical hole has none"),
            });
        }
        let from_below: Vec<String> = (0..trajs.len())
            .filter(|&t| surveyed[t].datum_at.is_none())
            .map(name_of)
            .collect();
        if !from_below.is_empty() {
            says.push(Note {
                key: "no-datum-station",
                detail: format!(
                    "{} start below their datum, and nothing states the hole above their first station: no vertical depth, no displacement, and not definitive",
                    listed(&from_below)
                ),
            });
        }
        let several: Vec<String> = bores
            .iter()
            .filter(|bore| bore.trajectories.len() > 1)
            .map(|bore| bore.name.clone())
            .collect();
        if !several.is_empty() {
            says.push(Note {
                key: "several-trajectories",
                detail: format!(
                    "{} have several RESQML trajectories, and which one is definitive is nobody's statement here: none is written definitive",
                    listed(&several)
                ),
            });
        }
        let grid: Vec<String> = (0..trajs.len())
            .filter(|&t| matches!(surveyed[t].north, North::Grid))
            .map(name_of)
            .collect();
        if !grid.is_empty() {
            says.push(Note {
                key: "grid-north",
                detail: format!("azimuths from grid north: {}", listed(&grid)),
            });
        }
        for (t, one) in surveyed.iter().enumerate() {
            if let North::Plant(why) = &one.north {
                says.push(Note {
                    key: "plant-north",
                    detail: format!(
                        "`{}` has its azimuths from its frame's own north (plant north): {why}",
                        name_of(t)
                    ),
                });
            }
        }
        if let Some((reference, _)) = &projected {
            let unplaced: Vec<String> = heads
                .iter()
                .filter(|head| head.wellhead.is_none() || head.epsg.is_none())
                .map(|head| bores[head.bores[0]].name.clone())
                .collect();
            says.push(Note {
                key: "projected",
                detail: format!(
                    "each Well located in `{}` where the package places its datum",
                    reference.title
                ),
            });
            if !unplaced.is_empty() {
                says.push(Note {
                    key: "projected-unplaced",
                    detail: format!(
                        "{} are not located in `{}`: the package does not place their datum in a projected system with a code",
                        listed(&unplaced),
                        reference.title
                    ),
                });
            }
        }
        if params.existence.is_none() {
            says.push(Note {
                key: "existence-unsaid",
                detail: "no `existence` was given, so nothing says whether these wells are drilled or planned: no Existence is written, nor the deepest depth (Md or MdPlanned)".to_string(),
            });
        }
        if params.synthetic {
            says.push(Note {
                key: "synthetic",
                detail:
                    "every object says it is invented: `synthetic = true` and a sentence in its description"
                        .to_string(),
            });
        }
        says.push(Note {
            key: "resqml-links",
            detail: "the RESQML side of the link — WellboreFeature.WitsmlWellbore and WellboreTrajectoryRepresentation.WitsmlTrajectory (RESQML 2.2 Wells.xsd) — is on objects of the package this plan does not rewrite: a plan reads no object whole, and rewriting one from what is known of it would lose what it holds. The WITSML objects share the datum and the frame with them".to_string(),
        });
        says.push(Note {
            key: "left-out",
            detail: "not written: markers, geology, logs and channel sets, wellbore geometry and completions, and a geographic location — nothing in a frame states them".to_string(),
        });

        let seed = format!(
            "witsml-wells:{}",
            objects
                .iter()
                .map(|o| o.uuid.as_str())
                .collect::<Vec<_>>()
                .join(",")
        );
        let count = |title: &str, value: usize| Parameter::Double {
            title: title.to_string(),
            value: value as f64,
            uom: None,
        };
        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            objects,
            datasets: Vec::new(),
            activity: ActivityDraft {
                recipe: "witsml-wells",
                title: format!("witsml-wells → {}", well_titles.join(", ")),
                seed,
                parameters: vec![
                    count("wellCount", heads.len()),
                    count("wellboreCount", bores.len()),
                    count("trajectoryCount", trajs.len()),
                    count("stationCount", station_count),
                    count("sidetrackCount", sidetrack_count),
                ],
            },
            says,
            dropped: Vec::new(),
            shapes,
        })
    }
}
