//! `change-frame` — a representation read in one frame of the package and
//! written in another: **the same nodes, in the same order, with the same
//! topology**, their numbers changed so that each one lands where it was.
//!
//! The same horizon in time and in depth: one horizon as a 2D grid in the
//! depth compound CRS and another in the time compound CRS, with a declared
//! (sourced) depth-velocity law. Every horizon the
//! recipe writes is an **elevation** grid in the boreholes' compound
//! (EPSG:2154 and EPSG:5720, the identity, z up); the first output is the
//! horizon in the depth compound, and the second is the same grid with Z
//! converted by the velocity law, in the time
//! compound. No operation changed the frame of a surface.
//!
//! # What changes, and through what
//!
//! A node is **placed** through the frame it is read in and **unplaced**
//! through the frame it is written in — [`Crs::to_projected`], then
//! [`Crs::from_projected`], the one placement formula of the workspace, over
//! the frames `energyml_domain::crs::read_with` read:
//!
//! - **the horizontal part** goes through the projected system both frames sit
//!   on — each frame's origin and azimuth (the reference model's local frames are turned 30°);
//! - **elevation ↔ depth** is the vertical half of the same two calls: a sign,
//!   the compound's `VerticalAxis.Direction`, and an origin,
//!   `OriginVerticalCoordinate` — *« Vertical coordinate of the origin of the
//!   local engineering CRS in the base vertical CRS »* (CRS.xsd l. 517–520;
//!   common TRG 2.3 §12.20 p. 322) — both read, never guessed;
//! - **depth ↔ time is not a placement**. A time has no depth, and no frame
//!   carries the velocity that joins them (RESQML leaves the time kind and the
//!   velocity to the acquisition — `Crs::is_time`). So it takes a **stated
//!   law**, `energyml_analysis::t2d::LinearVelocity` — `V(z) = v0 + k·z` — whose
//!   datum, *« where t = 0 »*, is the time frame's own vertical origin: the
//!   place its `OriginVerticalCoordinate` puts in the vertical system. The law
//!   is the person's, with its **source** (the reference-model rule: no constant without a source, ADR-0019), and the
//!   object written carries `synthetic = true` and the law in
//!   `Citation/Description` (rule 4, as `horizon-from-law` does): a time
//!   converted by a law is not a time measured. One-way or two-way is said, not
//!   defaulted — the module doc of `t2d` says why.
//!
//! A node above the time origin gets a negative time: the law is extended above
//! the depth it is stated from, and the plan counts and says it. A node the law
//! does not reach (`k < 0`: the velocity falls to zero) is written with no
//! height, counted — never a plausible number.
//!
//! # Refused by name
//!
//! - **two projected systems**: moving between them is a reprojection — it bends
//!   a lattice and needs a geodesy library (`energyml-proj`); this is not it;
//! - **two vertical datums**, or two horizontal units: an elevation above one
//!   zero is not an elevation above another;
//! - **an unknown vertical sense** — a frame that names no vertical system (a
//!   `ProjectedCrs` named directly), whose third ordinate nobody states;
//! - **a time frame counting in a unit that is no time**, and a length frame
//!   counting in one that is;
//! - **a time without a law**, a law where neither frame counts time (the
//!   activity would record a law nothing used), a law without a source, and two
//!   time frames with two origins (the law is stated below one of them);
//! - **the same frame**, and a frame whose placement the caller did not resolve.
//!
//! # A grid keeps its lattice, or is refused
//!
//! `grid` writes a lattice along the frame's +x then +y (a grid is never
//! reprojected, and its order is the house order;
//! see the `grid` operation). A change of vertical keeps it, and so
//! does a move of the origin: the lattice's axes stay the frame's axes, only
//! its origin moves. A frame **turned** from the grid's is refused for a grid:
//! the lattice would run along turned directions, which a `Grid2dRepresentation`
//! can say (`Point3dLatticeDimension.Direction`) and `grid` does not write —
//! writing one is a decision, and turning the grid into points or triangles is
//! another operation's. A cloud, polylines or triangles take any frame, node by
//! node.
//!
//! # The writing is the writer's of the kind
//!
//! An operation that writes the same object as another delegates to it: a grid
//! goes to [`super::grid::write`], anything else to `transform::write_as` —
//! the kinds and refusals `transform` has.
//!
//! # The same interpretation, and why here it is right
//!
//! `horizon-from-law` refuses to suggest its reference's interpretation: the
//! horizon it lays is **another** horizon. Here it is the **same** one — H6 in
//! depth and H6 in time are two representations of one
//! `HorizonInterpretation`, the idiom RESQML provides (one interpretation,
//! several representations); the reference model registers both
//! under the same horizon. So the shape's interpretation is suggested, and the title too.
//!
//! # Identity
//!
//! From what the new object **is**: the input (its uuid when it has one) and a
//! digest of its nodes, the two frames, the law, and the title. The same input
//! gives the same object; another law gives another one beside it, never a
//! rewrite (the identity comes from the content).
//! `pointset` takes no seed of its own — it hangs its uuid on the first piece
//! of the bag — so the writers are called over a bag whose first piece is named
//! by that content; nothing else of the bag changes.
//!
//! # What the frames must be known as
//!
//! A plan is pure, and a frame's placement — origin, azimuth, vertical axis
//! and origin, units — is what this needs of both: `Known::placement`, read at
//! the boundary by `energyml_domain::crs::read_with`, for the shape's frame
//! and for the one `crs` names (which comes back into the bag as the companion
//! `crs (object)`, as any answered object does). A shape read **placed**, for
//! the scene, is local to no frame any more and has none: the plan is refused
//! naming it.
//!
//! # References
//!
//! - The linear velocity law `V(z) = v0 + k·z` of `energyml-analysis` (`t2d`).
//!   TODO(reference): verify — no publication is cited for the law; add one if it follows a published source.

use serde_json::{Value, json};

use energyml_analysis::t2d::LinearVelocity;
use energyml_domain::crs::{Crs, CrsKind, VerticalDirection};
use energyml_domain::provenance::Parameter;
use energyml_domain::units::{self, Dimension};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

use crate::declare::{Choice, CrsRef, Inputs, Params, Shape, Suggestions, TypeName, known_of};
use crate::operation::{About, Category, Context, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::{Known, Lattice, Pieces, Ref, Source};

use super::transform::{self, Kind};
use super::written::{FrameParams, NamingParams, NewFeatureParams, RepresentedParams, Written};

/// The operation.
pub struct ChangeFrame;

// ---------------------------------------------------------------- the declaration

/// What `change-frame` is told.
#[derive(Debug, Params)]
pub struct ChangeFrameParams {
    /// the frame to write in — a LocalEngineeringCompoundCrs of the package, on the projected and vertical systems of the frame the shape is in
    ///
    /// **In the `operation` group, not `frame`**: here the target frame is what
    /// the computation needs, not only what the writing names, so a plan asked
    /// for its geometry requires it too. Never suggested: the frame the shape
    /// is in is the one thing it cannot be.
    #[param(reads = ["Geometry/LocalCrs", "Patches/0/Geometry/LocalCrs", "LocalCrs"])]
    pub crs: CrsRef,
    /// The law that joins depth and time, when one of the two frames counts
    /// time.
    #[param(flatten)]
    pub law: VelocityLaw,
    /// What is represented — the shape's own interpretation, suggested.
    #[param(flatten)]
    pub represented: RepresentedParams,
    /// The title and the file.
    #[param(flatten)]
    pub naming: NamingParams,
}

/// the velocity law that joins depth and time — none when neither frame counts time, linear when one does
#[derive(Debug, Params)]
#[params(tag = "law")]
pub enum VelocityLaw {
    /// No law: both frames count lengths — elevation, depth, another origin.
    #[case(default, name = "none")]
    Without,
    /// A velocity growing linearly with depth below the time frame's origin.
    Linear(LinearLaw),
}

/// A linear law in depth, `V(z) = v0 + k·z`, and where it comes from.
#[derive(Debug, Params)]
pub struct LinearLaw {
    /// linear: the velocity at the time frame's vertical origin, where t = 0, in metres per second
    #[param(uom = "m/s")]
    pub v0: f64,
    /// linear: how fast the velocity grows with depth below that origin, per second — 0 is a constant velocity
    #[param(uom = "1/s")]
    pub k: f64,
    /// linear: whether the times are two-way (down and back, the seismic convention) or one-way
    ///
    /// No default: feeding a two-way time to a one-way formula doubles every
    /// depth with no symptom (`energyml_analysis::t2d`).
    pub travel_time: TravelTime,
    /// linear: where the law comes from — a document with its page or paragraph; a law stated as plausible says so (synthetic, and why)
    pub source: String,
}

/// Which travel time the time frame counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum TravelTime {
    /// Down and back: the seismic convention.
    TwoWay,
    /// Down only.
    OneWay,
}

/// What `change-frame` takes.
#[derive(Debug, Inputs)]
pub struct ChangeFrameInputs {
    /// the representation to write in another frame — a cloud, polylines, a triangulated surface or a 2D grid, with what is known of its frame
    pub shape: Shape,
}

// ---------------------------------------------------------------- the computation

/// A turn smaller than this is no turn: what reading an azimuth of 0 twice can
/// differ by, and nothing a frame states.
const TURN_TOLERANCE: f64 = 1e-12;

/// What a shape is written back as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Form {
    /// A 2D grid, `ni` nodes along its fast axis and `nj` along its slow one:
    /// `grid` writes it.
    Lattice { ni: usize, nj: usize },
    /// A cloud, polylines or triangles: the writer of that kind.
    Other(Kind),
}

/// What the shape is written as, or why it cannot be.
fn form_of(geometry: &Geometry) -> Result<Form, String> {
    if let [patch] = geometry.patches.as_slice()
        && let Topology::Grid { fastest, slowest } = &patch.topology
    {
        let (ni, nj) = (*fastest, *slowest);
        if ni < 2 || nj < 2 {
            return Err(format!(
                "is a grid of {ni} × {nj} nodes: a lattice has at least 2 along each axis"
            ));
        }
        if patch.points.len() != ni * nj {
            return Err(format!(
                "declares {ni} × {nj} nodes and holds {}",
                patch.points.len()
            ));
        }
        return Ok(Form::Lattice { ni, nj });
    }
    if geometry
        .patches
        .iter()
        .any(|patch| matches!(patch.topology, Topology::Grid { .. }))
    {
        return Err(format!(
            "holds {} patches, a grid among them: a 2D grid is written from one lattice",
            geometry.patches.len()
        ));
    }
    transform::kind_of(geometry).map(Form::Other)
}

/// A time axis, as a change needs it.
#[derive(Debug, Clone, Copy)]
struct TimeAxis {
    /// Seconds per unit of the axis.
    seconds: f64,
    /// The elevation of `t = 0` above the vertical datum, in metres — the
    /// law's datum.
    origin: f64,
    /// `+1` when times grow downward, `−1` when they grow upward.
    sense: f64,
}

/// What a frame's third ordinate counts.
#[derive(Debug, Clone, Copy)]
enum Counts {
    /// A length, and how many metres one unit of it is.
    Length(f64),
    /// A time.
    Time(TimeAxis),
}

/// A frame, as a change reads it.
struct Side<'a> {
    reference: &'a Ref,
    crs: &'a Crs,
    counts: Counts,
}

/// What the third ordinate of `crs` counts, or why it cannot be read — the
/// refusal under `slot`, the side it is about.
fn side<'a>(reference: &'a Ref, crs: &'a Crs, slot: &str) -> Result<Side<'a>, Refusal> {
    let title = &reference.title;
    if crs.vertical == CrsKind::Absent {
        return Err(Refusal::on(
            slot,
            format!(
                "`{title}` names no vertical system, so which way its third ordinate counts, and from where, is stated nowhere — a change of frame reads both: name a LocalEngineeringCompoundCrs"
            ),
        ));
    }
    let unit = crs.vertical_axis_uom();
    let counts = if crs.is_time() {
        let Some(unit) = unit else {
            return Err(Refusal::on(
                slot,
                format!("`{title}` counts time and states no unit for it"),
            ));
        };
        if Dimension::of(unit) != Some(Dimension::Time) {
            return Err(Refusal::on(
                slot,
                format!(
                    "`{title}` says its third ordinate is a time and counts it in `{unit}`, which is no time unit this build knows"
                ),
            ));
        }
        // `t = 0` is the frame's vertical origin: `to_projected` of a zero time
        // is the origin's elevation, in the datum's length — no time enters.
        let origin = crs.to_projected([0.0, 0.0, 0.0])[2];
        let origin = match crs.vertical_uom.as_deref() {
            Some(datum) if origin != 0.0 => units::metres(origin, datum).map_err(|why| {
                Refusal::on(
                    slot,
                    format!("`{title}` places its time origin at {origin} `{datum}`: {why}"),
                )
            })?,
            _ => origin,
        };
        Counts::Time(TimeAxis {
            seconds: units::factor(unit).unwrap_or(1.0),
            origin: plain(origin),
            sense: if crs.vertical_direction == VerticalDirection::Down {
                1.0
            } else {
                -1.0
            },
        })
    } else {
        match unit {
            // Taken at its word, as `Crs::to_metres` takes it.
            None => Counts::Length(1.0),
            Some(unit) => match Dimension::of(unit) {
                Some(Dimension::Length) => Counts::Length(units::factor(unit).unwrap_or(1.0)),
                Some(Dimension::Time) => {
                    return Err(Refusal::on(
                        slot,
                        format!(
                            "`{title}` counts its third ordinate in `{unit}`, a time, and does not say it is a time frame (`IsTime`)"
                        ),
                    ));
                }
                _ => {
                    return Err(Refusal::on(
                        slot,
                        format!(
                            "`{title}` counts its third ordinate in `{unit}`, which is no length unit this build knows"
                        ),
                    ));
                }
            },
        }
    };
    Ok(Side {
        reference,
        crs,
        counts,
    })
}

/// A system, in words.
fn system_words(kind: &CrsKind) -> String {
    match kind {
        CrsKind::Epsg(code) => format!("EPSG:{code}"),
        CrsKind::Gml => "a GML definition".to_string(),
        CrsKind::Wkt(_) => "a WKT definition".to_string(),
        CrsKind::LocalAuthority(name) => format!("`{name}`"),
        CrsKind::Unknown => "a system it says is unknown".to_string(),
        CrsKind::Absent => "no system".to_string(),
    }
}

/// Whether two systems are said to be one.
fn same_system(a: &CrsKind, b: &CrsKind) -> bool {
    match (a, b) {
        (CrsKind::Epsg(x), CrsKind::Epsg(y)) => x == y,
        (CrsKind::Wkt(x), CrsKind::Wkt(y)) | (CrsKind::LocalAuthority(x), CrsKind::LocalAuthority(y)) => {
            x == y
        }
        _ => false,
    }
}

/// The two frames sit on one projected system, in one horizontal unit, over
/// one vertical datum — or the refusal that names the two.
fn same_ground(
    from: &Side<'_>,
    to: &Side<'_>,
    from_datum: Option<&Ref>,
    to_datum: Option<&Ref>,
) -> Result<(), Refusal> {
    let (a, b) = (&from.reference.title, &to.reference.title);
    let (p, q) = (&from.crs.projected, &to.crs.projected);
    if !same_system(p, q) {
        return Err(Refusal::new(
            "frames",
            match (p, q) {
                (CrsKind::Epsg(x), CrsKind::Epsg(y)) => format!(
                    "`{a}` sits on EPSG:{x} and `{b}` on EPSG:{y}: moving between two projected systems is a reprojection, not a change of frame — it bends a lattice and needs a geodesy library (`energyml-proj`); write in a frame on EPSG:{x}"
                ),
                _ => format!(
                    "`{a}` sits on {} and `{b}` on {}: whether they are one projected system cannot be told, and a change of frame keeps the system",
                    system_words(p),
                    system_words(q)
                ),
            },
        ));
    }
    if let (Some(x), Some(y)) = (&from.crs.horizontal_uom, &to.crs.horizontal_uom)
        && x != y
    {
        return Err(Refusal::new(
            "frames",
            format!(
                "`{a}` counts x and y in `{x}` and `{b}` in `{y}` over one projected system: one of the two contradicts it"
            ),
        ));
    }
    let same_datum = match (&from.crs.vertical, &to.crs.vertical) {
        (CrsKind::Epsg(x), CrsKind::Epsg(y)) => x == y,
        // A datum without a code — `VerticalUnknownCrs` — is the same datum
        // only as the same object.
        _ => matches!((from_datum, to_datum), (Some(x), Some(y)) if x.same_object(y)),
    };
    if !same_datum {
        return Err(Refusal::new(
            "frames",
            format!(
                "`{a}` counts heights from {} and `{b}` from {}: an elevation above one zero is not an elevation above the other — a change of vertical datum is a reprojection, not a change of frame",
                system_words(&from.crs.vertical),
                system_words(&to.crs.vertical)
            ),
        ));
    }
    Ok(())
}

/// How the third ordinate goes from one frame to the other.
#[derive(Debug, Clone, Copy)]
enum Vertical {
    /// Two lengths — elevation, depth, another origin: through the elevation
    /// above the shared datum. Metres per unit, each side.
    Lengths { from: f64, to: f64 },
    /// Two times from one origin: a unit and a sense.
    Times { from: TimeAxis, to: TimeAxis },
    /// A length to a time, through the law.
    ToTime {
        law: LinearVelocity,
        travel: TravelTime,
        from: f64,
        to: TimeAxis,
    },
    /// A time to a length, through the law.
    ToLength {
        law: LinearVelocity,
        travel: TravelTime,
        from: TimeAxis,
        to: f64,
    },
}

/// The law as `t2d` takes it, or why it is none — its source first: a law
/// nobody can trace is not stated.
fn law_of(linear: &LinearLaw) -> Result<LinearVelocity, Refusal> {
    if linear.source.trim().is_empty() {
        return Err(Refusal::on(
            "source",
            "is empty: a velocity law is sourced — a document with its page or paragraph, or a plausible law said to be one (reference-model rule: no constant without a source, ADR-0019)",
        ));
    }
    LinearVelocity::new(linear.v0, linear.k).map_err(|_| {
        if linear.v0.is_finite() && linear.v0 > 0.0 {
            Refusal::on("k", format!("is {}: a gradient is a finite number", linear.k))
        } else {
            Refusal::on(
                "v0",
                format!("is {}: a velocity is a finite number > 0", linear.v0),
            )
        }
    })
}

/// How the third ordinate changes, or the refusal that says what is missing
/// or superfluous.
fn vertical_of(from: &Side<'_>, to: &Side<'_>, law: Option<&LinearLaw>) -> Result<Vertical, Refusal> {
    let (a, b) = (&from.reference.title, &to.reference.title);
    match (from.counts, to.counts, law) {
        (Counts::Length(x), Counts::Length(y), None) => Ok(Vertical::Lengths { from: x, to: y }),
        (Counts::Length(_), Counts::Length(_), Some(_)) => Err(Refusal::on(
            "law",
            format!(
                "neither `{a}` nor `{b}` counts time, so no law is used — and an activity recording one would say it was: leave `law` at none"
            ),
        )),
        (Counts::Time(x), Counts::Time(y), None) if (x.origin - y.origin).abs() <= 1e-9 => {
            Ok(Vertical::Times { from: x, to: y })
        }
        (Counts::Time(x), Counts::Time(y), None) => Err(Refusal::new(
            "frames",
            format!(
                "`{a}` counts time from {} m and `{b}` from {} m above the vertical datum: moving a time origin needs the velocities between the two — go through a depth frame, with a law each way",
                x.origin, y.origin
            ),
        )),
        (Counts::Time(_), Counts::Time(_), Some(_)) => Err(Refusal::on(
            "law",
            format!(
                "`{a}` and `{b}` both count time: a law is stated below one time origin, and none is needed between two times from one — leave `law` at none, or go through a depth frame"
            ),
        )),
        (Counts::Length(_), Counts::Time(_), None) | (Counts::Time(_), Counts::Length(_), None) => {
            Err(Refusal::on(
                "law",
                format!(
                    "`{a}` and `{b}` are a length and a time: a time has no depth without a velocity, and no frame carries one — state the law (`law = linear`, with `v0`, `k`, `travelTime` and its `source`)"
                ),
            ))
        }
        (Counts::Length(x), Counts::Time(y), Some(linear)) => Ok(Vertical::ToTime {
            law: law_of(linear)?,
            travel: linear.travel_time,
            from: x,
            to: y,
        }),
        (Counts::Time(x), Counts::Length(y), Some(linear)) => Ok(Vertical::ToLength {
            law: law_of(linear)?,
            travel: linear.travel_time,
            from: x,
            to: y,
        }),
    }
}

/// The time the law gives at a depth below its datum, in seconds.
fn time_at(law: &LinearVelocity, travel: TravelTime, depth: f64) -> f64 {
    match travel {
        TravelTime::TwoWay => law.twt_of_depth(depth),
        TravelTime::OneWay => law.owt_of_depth(depth),
    }
}

/// The depth below its datum the law gives at a time, in metres.
fn depth_at(law: &LinearVelocity, travel: TravelTime, seconds: f64) -> f64 {
    match travel {
        TravelTime::TwoWay => law.depth_of_twt(seconds),
        TravelTime::OneWay => law.depth_of_owt(seconds),
    }
}

/// What became of the heights, counted.
#[derive(Debug, Default, Clone, Copy)]
struct Tally {
    /// Nodes above the time origin: a negative time, or a negative time read.
    above: usize,
    /// Nodes with a height the law does not reach — written with none.
    unreached: usize,
}

/// The change itself: two frames and what happens to the third ordinate.
struct Change<'a> {
    from: &'a Crs,
    to: &'a Crs,
    vertical: Vertical,
}

impl Change<'_> {
    /// One node, local to `from`, as `to` stores it.
    fn node(&self, p: [f64; 3], tally: &mut Tally) -> [f64; 3] {
        let placed = self.from.to_projected(p);
        // The target's x and y, whatever its third ordinate: `from_projected`
        // turns and moves the plane apart from the vertical.
        let unplaced = |z: f64| self.to.from_projected([placed[0], placed[1], z]);
        match self.vertical {
            // The elevation above the shared datum, in metres, then in the
            // target's unit — the sign and the origin are `from_projected`'s.
            Vertical::Lengths { from, to } => unplaced(placed[2] * from / to),
            Vertical::Times { from, to } => {
                let seconds = from.sense * p[2] * from.seconds;
                let [x, y, _] = unplaced(0.0);
                [x, y, to.sense * seconds / to.seconds]
            }
            Vertical::ToTime {
                law,
                travel,
                from,
                to,
            } => {
                let elevation = placed[2] * from;
                let depth = to.origin - elevation;
                let seconds = time_at(&law, travel, depth);
                if depth < 0.0 {
                    tally.above += 1;
                }
                if elevation.is_finite() && !seconds.is_finite() {
                    tally.unreached += 1;
                }
                let [x, y, _] = unplaced(0.0);
                [x, y, to.sense * seconds / to.seconds]
            }
            Vertical::ToLength {
                law,
                travel,
                from,
                to,
            } => {
                let seconds = from.sense * p[2] * from.seconds;
                let elevation = from.origin - depth_at(&law, travel, seconds);
                if seconds < 0.0 {
                    tally.above += 1;
                }
                if seconds.is_finite() && !elevation.is_finite() {
                    tally.unreached += 1;
                }
                unplaced(elevation / to)
            }
        }
    }
}

/// The turn from one frame's axes to the other's, in this view's radians
/// (anticlockwise), in (−π, π].
fn turn_between(from: &Crs, to: &Crs) -> f64 {
    let turn = to.areal_rotation - from.areal_rotation;
    turn.sin().atan2(turn.cos())
}

/// The lattice of the nodes as the target stores them — origin and spacing —
/// or the first node that is not where a lattice along +x then +y puts it.
///
/// The reading `horizon-from-law` makes of its reference, on the nodes once
/// moved: a lattice that was one in the shape's frame is one in a frame of the
/// same azimuth, and a rotated, reversed or irregular one is refused rather
/// than re-indexed.
fn lattice_of(points: &[[f64; 3]], ni: usize, nj: usize) -> Result<([f64; 2], [f64; 2]), String> {
    let at = |i: usize, j: usize| points[j * ni + i];
    let origin = at(0, 0);
    let (dx, dy) = (at(1, 0)[0] - origin[0], at(0, 1)[1] - origin[1]);
    if !(dx.is_finite() && dx > 0.0 && dy.is_finite() && dy > 0.0) {
        return Err(format!(
            "runs along ({}, {}) then ({}, {}) in the target frame: `grid` writes a lattice along +x then +y, and a rotated or reversed one is not re-indexed here",
            at(1, 0)[0] - origin[0],
            at(1, 0)[1] - origin[1],
            at(0, 1)[0] - origin[0],
            at(0, 1)[1] - origin[1]
        ));
    }
    // A millionth of a cell: what the reader's running sum of spacings, and
    // the placement's arithmetic at map coordinates, can drift by.
    let tolerance = 1e-6 * dx.min(dy);
    for j in 0..nj {
        for i in 0..ni {
            let node = at(i, j);
            let (x, y) = (origin[0] + i as f64 * dx, origin[1] + j as f64 * dy);
            if !((node[0] - x).abs() <= tolerance && (node[1] - y).abs() <= tolerance) {
                return Err(format!(
                    "node ({i}, {j}) is at ({}, {}) in the target frame where a regular lattice along +x then +y puts it at ({x}, {y}): a rotated or irregular grid is not what `grid` writes",
                    node[0], node[1]
                ));
            }
        }
    }
    Ok(([origin[0], origin[1]], [dx, dy]))
}

/// The object a `crs` answer names — a `DataObjectReference` (its uuid, type and
/// `ObjectVersion`), a uri (read by `EtpUri`, the version it names included), or
/// a bare uuid, which names no version.
fn reference_of(value: &Value) -> Option<Ref> {
    let text = |name: &str| {
        value
            .get(name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|one| !one.is_empty())
            .map(str::to_string)
    };
    if let Some(uuid) = text("Uuid") {
        let qualified_type = text("QualifiedType").unwrap_or_default();
        return Some(
            Ref::new(uuid, qualified_type, text("Title").unwrap_or_default())
                .with_version(text("ObjectVersion")),
        );
    }
    let said = value.as_str()?.trim();
    Ref::of_uri(said, "").or_else(|| (!said.is_empty()).then(|| Ref::new(said, "", "")))
}

/// Whether the answer `target` names this frame: the same uuid, and the same
/// version when the answer states one.
fn names(frame: &Ref, target: &Ref) -> bool {
    frame.uuid.eq_ignore_ascii_case(&target.uuid)
        && (target.object_version.is_none() || target.object_version == frame.object_version)
}

/// One step of FNV-1a over eight bytes.
fn fnv(hash: &mut u64, bytes: [u8; 8]) {
    for byte in bytes {
        *hash ^= u64::from(byte);
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// A digest of a shape's nodes — FNV-1a over each patch's count and every
/// coordinate's bits — so that the identity follows the content, and stays the
/// same from one build to the next (the standard library's hasher does not
/// promise that).
fn digest(geometry: &Geometry) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for patch in &geometry.patches {
        fnv(&mut hash, (patch.points.len() as u64).to_le_bytes());
        for point in &patch.points {
            for value in point {
                fnv(&mut hash, value.to_bits().to_le_bytes());
            }
        }
    }
    hash
}

/// The bag the writers are called over: its first piece named by `key`, so
/// that a writer hanging its uuid on the first piece (`pointset`) hangs it on
/// the content; every object the caller resolved kept, so that the frame
/// named is found as it was.
fn seeded(bag: &Pieces, key: &str) -> Pieces {
    let mut out = Pieces::empty().with(key, Source::Object(Known::default()));
    for (name, known) in bag.objects() {
        out = out.with(name, Source::Object(known.clone()));
    }
    out
}

/// A frame's third ordinate, in words — for a note.
fn axis_words(side: &Side<'_>) -> String {
    let unit = side.crs.vertical_axis_uom().unwrap_or("an unstated unit");
    match side.counts {
        Counts::Time(axis) => format!(
            "time in {unit}, t = 0 at {} m above the vertical datum",
            axis.origin
        ),
        Counts::Length(_) => {
            let zero = plain(side.crs.to_projected([0.0, 0.0, 0.0])[2]);
            let word = if side.crs.vertical_direction == VerticalDirection::Down {
                "depth"
            } else {
                "elevation"
            };
            format!("{word} in {unit}, its zero {zero} {unit} above the vertical datum")
        }
    }
}

/// Zero as `0`, never `-0`: `to_projected` of a zero origin on a downward axis
/// is `−0.0`, which reads as a sign in a note and an activity.
fn plain(value: f64) -> f64 {
    if value == 0.0 { 0.0 } else { value }
}

/// A number rounded to a millionth, for a note.
fn rounded(value: f64) -> f64 {
    (value * 1e6).round() / 1e6
}

fn double(title: &str, value: f64, uom: Option<&str>) -> Parameter {
    Parameter::Double {
        title: title.to_string(),
        value,
        uom: uom.map(str::to_string),
    }
}

impl Operation for ChangeFrame {
    type Params = ChangeFrameParams;
    type Inputs = ChangeFrameInputs;

    fn about(&self) -> About {
        About {
            name: "change-frame",
            describe: "a representation written in another frame of the package — elevation and depth through the frames' vertical axes and origins, depth and time through a stated velocity law, the plane through each frame's origin and azimuth — same nodes, same order, same interpretation",
            category: Category::Frame,
            makes: &[
                "resqml22.Grid2dRepresentation",
                "resqml22.PointSetRepresentation",
                "resqml22.PolylineSetRepresentation",
                "resqml22.TriangulatedSetRepresentation",
            ],
            recipe: "change-frame",
            represents: None,
        }
    }

    /// The shape's interpretation and its title — the same horizon, see the
    /// module. **Not** the frame: the one it is in is the one it cannot go to.
    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        let Some(known) = known_of(bag, "shape") else {
            return;
        };
        if let Some(interpretation) = &known.interpretation {
            slots.inferred("interpretation", interpretation.dor(), interpretation.clone());
        }
        if let Some(reference) = &known.reference {
            slots.inferred("title", json!(reference.title), reference.clone());
        }
    }

    fn check(&self, inputs: &ChangeFrameInputs) -> Result<(), Refusal> {
        form_of(&inputs.shape.geometry).map_err(|why| Refusal::on("shape", why))?;
        if inputs.shape.geometry.point_count() == 0 {
            return Err(Refusal::on("shape", "it has no node to write"));
        }
        Ok(())
    }

    fn plan(
        &self,
        inputs: ChangeFrameInputs,
        params: ChangeFrameParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let shape = inputs.shape;
        let form = form_of(&shape.geometry).map_err(|why| Refusal::on("shape", why))?;

        // -- the frame the numbers are in -----------------------------------------
        let known = shape.known.as_ref();
        let from_ref = known.and_then(|k| k.crs.as_ref()).ok_or_else(|| {
            Refusal::on(
                "shape",
                "names no frame: a change of frame starts from the frame the numbers are in — give the shape with what is known of its object",
            )
        })?;
        let from_crs = known.and_then(|k| k.placement.as_ref()).ok_or_else(|| {
            Refusal::on(
                "shape",
                format!(
                    "is in `{}`, whose placement is not known — its origin, azimuth and vertical axis, which the caller resolves from the package; a shape read placed, for the scene, is local to no frame any more",
                    from_ref.title
                ),
            )
        })?;

        // -- the frame they go to ------------------------------------------------------
        let target = reference_of(&params.crs.0).ok_or_else(|| {
            Refusal::about(
                "value",
                "crs",
                format!(
                    "`crs`: expected a DataObjectReference, a uri or a uuid, got {}",
                    params.crs.0
                ),
            )
        })?;
        if names(from_ref, &target) {
            return Err(Refusal::on(
                "crs",
                format!(
                    "names `{}`, the frame the shape is already in — nothing would change",
                    from_ref.title
                ),
            ));
        }
        // The frame the answer names — by its version when the answer states
        // one; an answer that states none (a bare uuid, a short uri) takes the
        // frame that declares none first, then any.
        let framed = |exact: bool| {
            cx.known().into_iter().find_map(|k| match (&k.crs, &k.placement) {
                (Some(frame), Some(placement))
                    if names(frame, &target) && (!exact || frame.object_version == target.object_version) =>
                {
                    Some((frame, placement, k.vertical_crs.as_ref()))
                }
                _ => None,
            })
        };
        let (to_ref, to_crs, to_datum) = framed(true)
            .or_else(|| framed(false))
            .ok_or_else(|| {
                let title = params.crs.0.get("Title").and_then(Value::as_str).unwrap_or(target.uuid.as_str()).to_string();
                Refusal::on(
                    "crs",
                    format!(
                        "names `{title}`, whose placement is not known — its origin, azimuth and vertical axis, which the caller resolves from the package for the frame it names"
                    ),
                )
            })?;
        if let Err(why) = crate::crs::frame_kind(&to_ref.qualified_type) {
            return Err(Refusal::on("crs", format!("names no whole system: {why}")));
        }

        // -- what may change, what may not -------------------------------------------
        let from = side(from_ref, from_crs, "shape")?;
        let to = side(to_ref, to_crs, "crs")?;
        same_ground(&from, &to, known.and_then(|k| k.vertical_crs.as_ref()), to_datum)?;
        let stated = match &params.law {
            VelocityLaw::Without => None,
            VelocityLaw::Linear(linear) => Some(linear),
        };
        let vertical = vertical_of(&from, &to, stated)?;
        let turn = turn_between(from_crs, to_crs);
        if let Form::Lattice { .. } = form
            && turn.abs() > TURN_TOLERANCE
        {
            return Err(Refusal::on(
                "shape",
                format!(
                    "is a 2D grid, and `{}` is turned {}° from `{}`: the lattice would run along turned axes, which `grid` does not write (it writes along the frame's +x then +y) — write the grid in a frame of the same azimuth, or make points or triangles of it first",
                    to_ref.title,
                    rounded(-turn.to_degrees()),
                    from_ref.title
                ),
            ));
        }

        // -- every node, in its order ----------------------------------------------------
        let change = Change {
            from: from_crs,
            to: to_crs,
            vertical,
        };
        let mut tally = Tally::default();
        let mut patches: Vec<Patch> = Vec::with_capacity(shape.geometry.patches.len());
        for patch in &shape.geometry.patches {
            cx.stop_if_cancelled()?;
            let mut points = Vec::with_capacity(patch.points.len());
            for &p in &patch.points {
                points.push(change.node(p, &mut tally));
            }
            patches.push(Patch {
                index: patch.index,
                node_offset: patch.node_offset,
                points,
                topology: patch.topology.clone(),
                closed: patch.closed.clone(),
            });
        }

        // -- identity: what the new object is ------------------------------------------
        let title = params.naming.title();
        let law_key = match stated {
            None => "none".to_string(),
            Some(linear) => format!(
                "linear v0={:?} k={:?} {}",
                linear.v0,
                linear.k,
                linear.travel_time.word()
            ),
        };
        // The version joins a seed only when the object declares one, so what
        // was derived for an unversioned object keeps its uuid.
        let seed_of = |one: &Ref| match &one.object_version {
            Some(version) => format!("{}@{version}", one.uuid.to_ascii_lowercase()),
            None => one.uuid.to_ascii_lowercase(),
        };
        let input_key = known
            .and_then(|k| k.reference.as_ref())
            .map(seed_of)
            .unwrap_or_else(|| shape.name.clone());
        let key = format!(
            "change-frame:{input_key}#{:016x}:{}>{}:{law_key}",
            digest(&shape.geometry),
            seed_of(from_ref),
            seed_of(to_ref)
        );
        let seed = format!("{key}:{title}");
        let bag = seeded(cx.bag, &key);
        let inner = Context {
            identity: cx.identity,
            when: cx.when,
            purpose: cx.purpose,
            feedback: cx.feedback,
            bag: &bag,
        };

        // -- the writing is the writer's of the kind ---------------------------------------
        let written = Written {
            frame: FrameParams {
                crs: Some(params.crs.clone()),
                source_epsg: None,
                project: None,
                vertical: None,
            },
            represented: params.represented.clone(),
            naming: params.naming.clone(),
            // Nothing to create: the shape's interpretation, or none — what it
            // represents does not change with its frame.
            new_feature: NewFeatureParams {
                feature_type: None,
                interpretation_type: TypeName("resqml22.GenericFeatureInterpretation".to_string()),
                feature_kind: None,
            },
        };
        let mut plan = match form {
            Form::Lattice { ni, nj } => {
                let points = &patches[0].points;
                let (origin, spacing) =
                    lattice_of(points, ni, nj).map_err(|why| Refusal::on("shape", why))?;
                let lattice = Lattice {
                    origin,
                    spacing,
                    counts: [ni, nj],
                    z: points.iter().map(|p| p[2]).collect(),
                };
                super::grid::write(
                    &format!("change-frame of {}", shape.name),
                    &lattice,
                    &written,
                    &seed,
                    &inner,
                )?
            }
            Form::Other(kind) => {
                let result = Shape {
                    name: shape.name.clone(),
                    geometry: Geometry {
                        kind: "change-frame",
                        patches,
                    },
                    closed: shape.closed.clone(),
                    known: shape.known.clone(),
                };
                transform::write_as(kind, result, written, &seed, &inner)?
            }
        };

        // -- what the object says of itself, and what the plan says ---------------------
        let mut says: Vec<Note> = Vec::new();
        let horizontal = if from_crs.x_offset == to_crs.x_offset
            && from_crs.y_offset == to_crs.y_offset
            && turn.abs() <= TURN_TOLERANCE
        {
            "the same horizontal placement, x and y unchanged".to_string()
        } else {
            format!(
                "the local origin moves from ({}, {}) to ({}, {}) and the axes turn {}° clockwise — every node stays where it is, its numbers change",
                from_crs.x_offset,
                from_crs.y_offset,
                to_crs.x_offset,
                to_crs.y_offset,
                rounded(-turn.to_degrees())
            )
        };
        says.push(Note {
            key: "frames",
            detail: format!(
                "from `{}` to `{}`: {horizontal}; {} → {}",
                from_ref.title,
                to_ref.title,
                axis_words(&from),
                axis_words(&to)
            ),
        });
        let mut parameters: Vec<Parameter> = Vec::new();
        let timed = match vertical {
            Vertical::ToTime {
                law,
                travel,
                to: axis,
                ..
            } => Some((law, travel, axis, to_ref)),
            Vertical::ToLength {
                law,
                travel,
                from: axis,
                ..
            } => Some((law, travel, axis, from_ref)),
            Vertical::Lengths { .. } | Vertical::Times { .. } => None,
        };
        if let (Some((law, travel, axis, frame)), Some(linear)) = (timed, stated) {
            let source = linear.source.trim();
            let stated_law = format!(
                "V(z) = {} m/s + {} /s × z, z the depth below the origin of `{}` ({} m above the vertical datum), {} times",
                law.v0,
                law.k,
                frame.title,
                axis.origin,
                travel.word()
            );
            let description = format!(
                "Converted from `{}` to `{}` through a stated velocity law, not measured: {stated_law}. Source of the law: {source}",
                from_ref.title, to_ref.title
            );
            // `Citation/Description` holds 2000 characters (eml 2.3
            // `Citation`, `DescriptionString`): refused rather than cut short,
            // since a source cut short cites nothing.
            if description.chars().count() > 2000 {
                return Err(Refusal::on(
                    "source",
                    format!(
                        "makes a description of {} characters, and a Citation/Description holds 2000 — cite the document shorter",
                        description.chars().count()
                    ),
                ));
            }
            if let Some(object) = plan
                .objects
                .iter_mut()
                .find(|object| object.role == "representation")
            {
                object.json["Citation"]["Description"] = json!(description);
                // The reference-model rule on invented objects (ADR-0019), as `horizon-from-law`
                // writes it: a time converted by a law is not a time measured.
                object.json["ExtensionNameValue"] = json!([{
                    "$type": "eml23.ExtensionNameValue",
                    "Name": "synthetic",
                    "Value": { "$type": "eml23.StringMeasure", "_": "true" },
                    "Description": "converted by `change-frame` through a stated velocity law, not measured",
                }]);
            }
            says.push(Note {
                key: "law",
                detail: format!("{stated_law} — source: {source}"),
            });
            says.push(Note {
                key: "synthetic",
                detail: "the heights come through a law, not a measurement: the object carries `synthetic = true` and a description of its law and its source".to_string(),
            });
            if tally.above > 0 {
                says.push(Note {
                    key: "above-time-origin",
                    detail: format!(
                        "{} node(s) lie above the time origin: their times are negative, the law extended above the depth it is stated from",
                        tally.above
                    ),
                });
            }
            if tally.unreached > 0 {
                says.push(Note {
                    key: "unreached",
                    detail: format!(
                        "{} node(s) lie deeper than the law reaches — its velocity falls to zero at {} m below the time origin: written with no height",
                        tally.unreached,
                        -law.v0 / law.k
                    ),
                });
            }
            parameters.push(double("timeOrigin", axis.origin, Some("m")));
            parameters.push(double("aboveTimeOrigin", tally.above as f64, None));
            parameters.push(double("unreachedNodes", tally.unreached as f64, None));
        }

        plan.activity.recipe = "change-frame";
        plan.activity.title = format!("change-frame → {title}");
        plan.activity.parameters.extend(parameters);
        plan.says.extend(says);
        Ok(plan)
    }

    /// What was measured, and the counts the writer of each kind records —
    /// each optional, since which appear depends on the kind and on the law.
    /// The inputs (`law`, `v0`, …) are recorded under their own titles.
    fn reports(&self) -> Vec<Slot> {
        let optional = |name: &'static str, about: &'static str| {
            Slot::optional(
                name,
                Need::Number {
                    uom: None,
                    least: None,
                    most: None,
                },
                about,
            )
        };
        vec![
            Slot::optional(
                "timeOrigin",
                Need::Number {
                    uom: Some("m"),
                    least: None,
                    most: None,
                },
                "with a law: the elevation of the time frame's origin, where t = 0, above the vertical datum",
            ),
            optional(
                "aboveTimeOrigin",
                "with a law: nodes above the time origin — negative times, the law extended above its datum",
            ),
            optional(
                "unreachedNodes",
                "with a law: nodes deeper than the law reaches — written with no height",
            ),
            optional("nodeCount", "a grid: nodes written"),
            optional("fastestAxisCount", "a grid: nodes along x"),
            optional("slowestAxisCount", "a grid: nodes along y"),
            optional("pointCount", "a cloud, polylines or triangles: nodes written"),
            optional("polylineCount", "polylines: lines written"),
            optional("closedCount", "polylines: closed lines written"),
            optional("triangleCount", "triangles: triangles written"),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: &str = "7e300000-0000-4000-8000-000000000001";

    #[test]
    fn an_answer_names_a_frame_by_its_uuid_and_the_version_it_states() {
        let type_ = "eml23.LocalEngineeringCompoundCrs";
        let first = Ref::new(FRAME, type_, "frame").with_version(Some("1".to_string()));
        let second = Ref::new(FRAME, type_, "frame").with_version(Some("2".to_string()));

        // A `DataObjectReference` states its version, and so does a uri.
        let dor = reference_of(&json!({ "Uuid": FRAME, "QualifiedType": type_, "ObjectVersion": "2" }))
            .expect("a reference");
        assert!(
            names(&second, &dor) && !names(&first, &dor),
            "the version stated is the version taken"
        );
        let uri = reference_of(&json!(second.uri())).expect("a uri");
        assert_eq!(uri.object_version.as_deref(), Some("2"), "read by `EtpUri`");
        assert!(names(&second, &uri) && !names(&first, &uri));

        // A bare uuid states none, and names the frame whatever its version.
        let bare = reference_of(&json!(FRAME.to_ascii_uppercase())).expect("a bare uuid");
        assert_eq!(bare.object_version, None);
        assert!(names(&first, &bare) && names(&second, &bare));

        assert!(reference_of(&json!(7)).is_none() && reference_of(&json!("")).is_none());
    }
}
