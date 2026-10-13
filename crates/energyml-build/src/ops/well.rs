//! `well-from-polyline` - a wellbore from one polyline: its datum, its
//! trajectory, its frame, and the feature and interpretation they hang off.
//!
//! The need (09/09, with the answers of 10/09): the ground
//! position is asked for (the highest Z is displayed to help), the
//! MDs are computed from that elevation, the trajectory takes the points as control
//! points, simpler if it is vertical - the datum created at the
//! first point or an existing one named, a linear spline by default, one frame
//! node per control point with a spacing on option.
//!
//! # What is decided here, and how honestly
//!
//! - **the measured depths** are geometry, not a guess: the drop from the
//!   ground to the first point, then the 3D chord length along the polyline,
//!   in the frame's units. A curve smoother than the points would make the MDs
//!   depend on the curve; the points are what is known;
//! - **the shape is a choice, and the default invents nothing**: a *linear*
//!   spline (kind 1) passes through the points and nothing else. *Natural
//!   cubic* (2) and *tangential cubic* (3) are offered; the third writes its
//!   tangents, Catmull-Rom, unit per unit of MD;
//! - **the parameter written is the measured depth, whatever the kind**. The
//!   reader evaluates a trajectory at the frame's `NodeMd`, so this is not a
//!   convention but the contract between the two objects;
//! - **vertical is measured, not assumed, and it is not enough**: every point
//!   over the first within a millionth of the length *and* the measured depths
//!   equal to the CRS's depths - RESQML's kind 0 reads its parameter as a depth
//!   through the CRS, which is the MD only when the datum is at the system's
//!   zero. Either condition unmet, it stays a linear spline through the same
//!   points, which places them identically, and the plan says why;
//! - **the datum sits in the frame**: under a `LocalEngineeringCompoundCrs` -
//!   every frame created here, and what a geometry should name - it is a
//!   `ReferencePointInALocalEngineeringCompoundCrs` whose `Crs` is that
//!   compound and whose coordinates are local, like the trajectory's (common
//!   TRG v2.3 §13.25, p. 371; a deliberate decision of 24/09). The compound
//!   reaches the vertical system itself, so nothing more is needed. Under a
//!   `ProjectedCrs` named as it stands (decision (c)) it is a
//!   `ReferencePointInACrs` - projected coordinates **in that `ProjectedCrs`**,
//!   and the frame's vertical CRS, which the reader of frames resolves to know
//!   which way is down; a frame of the package brings it in
//!   `Known::vertical_crs`, and without it the datum is refused by name rather
//!   than written pointing at nothing. Until 24/09 the second form was written
//!   under a compound too, its `ProjectedCoordinates.Crs` pointing at the
//!   compound - a local position given for a projected one;
//! - **degrees are refused**, as `sigeol-boreholes` and `wells-from-table`
//!   refuse them: a wellbore is a vertical extent, and under a
//!   `Geographic2dCrs` its depths are metres beside degrees.
//!
//! Everything a reader needs is checked against the reader this workspace
//! ships: `energyml-geometry/src/parametric.rs` and `well.rs` are the
//! triangulation, in `tests/well.rs`.

use serde_json::{Value, json};

use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::Topology;

use crate::crs::FrameKind;
use crate::declare::{Choice, Features, Inputs, ObjectRef, Params, Shape as ShapeIn, Suggestions};
use crate::geojson::{Position, Shape};
use crate::operation::{
    About, ActivityDraft, Category, Context, Dataset, Error, Identity, Need, NewObject, Note, Numbers,
    Operation, Plan, Refusal, Slot,
};
use crate::pieces::{Known, Pieces};

use super::support;
use super::written::WrittenImposed;

/// One polyline of the bag: already in a frame, or foreign and still to
/// be placed.
///
/// `pub(crate)`: `wells-from-polylines` writes one well per line rather than
/// refusing more than one, and reads the same shape (`crates/energyml-build/ARCHITECTURE.md`:
/// an operation that writes the same object as another delegates to it).
pub(crate) enum Found {
    /// A line of a shape - with the shape's name and what is known of where
    /// it came from, since that decides where its numbers are
    /// (`support::shape_placed`).
    Placed {
        points: Vec<[f64; 3]>,
        name: String,
        known: Option<Box<Known>>,
    },
    Foreign(Vec<Position>),
}

impl Found {
    /// The third ordinates, for the ground's suggestion - a foreign line's
    /// elevations are metres whatever its horizontal system.
    pub(crate) fn zs(&self) -> Vec<f64> {
        match self {
            Found::Placed { points, .. } => points.iter().map(|p| p[2]).collect(),
            Found::Foreign(positions) => positions.iter().filter_map(|p| p.z).collect(),
        }
    }

    /// The line, in the frame being written.
    pub(crate) fn placed_in(&self, frame: &support::Frame) -> Result<Vec<[f64; 3]>, Error> {
        match self {
            Found::Placed { points, name, known } => {
                support::shape_placed(name, points, known.as_deref(), frame)
            }
            Found::Foreign(positions) => support::placed(positions, frame),
        }
    }
}

/// The operation.
pub struct WellFromPolyline;

/// The trajectory through the points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum TrajectoryShape {
    /// A linear spline: through the points, and nothing else.
    Linear,
    /// A natural cubic spline.
    NaturalCubic,
    /// A tangential cubic, its tangents written (Catmull-Rom).
    TangentialCubic,
}

/// What the created datum is - EML's reference point kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum DatumKind {
    /// The ground.
    #[choice(name = "ground level")]
    GroundLevel,
    /// The kelly bushing.
    #[choice(name = "kelly bushing")]
    KellyBushing,
    /// The derrick floor.
    #[choice(name = "derrick floor")]
    DerrickFloor,
    /// The casing flange.
    #[choice(name = "casing flange")]
    CasingFlange,
}

/// What `well-from-polyline` is told.
///
/// **Its own pair, not the shared one**: an interpretation of a wellbore
/// carries `Domain` and `IsDrilled`, which the shared writer does not write -
/// so the block is [`WrittenImposed`], and the feature and interpretation are
/// made here when none is named.
#[derive(Debug, Params)]
pub struct WellParams {
    /// The frame, the interpretation when named, the naming.
    #[param(flatten)]
    pub written: WrittenImposed,
    /// the vertical coordinate of the ground at the wellhead, in the frame's units - the measured depths start there
    #[param(reads = ["MdDatum/VerticalCoordinate"])]
    pub ground: f64,
    /// the unit of the measured depths - the frame's vertical unit
    ///
    /// Where it lives on an existing wellbore frame: what makes *copy the
    /// settings of that other well* work without a line here.
    #[param(default = String::from("m"), reads = ["NodeMd/Uom", "MdUom", "Uom"])]
    pub md_uom: String,
    /// an existing reference point to measure from; left out, one is created at the first point, at the ground, in the frame
    ///
    /// **Two forms, and the frame decides**: in the compound when the frame is
    /// one (`ReferencePointInALocalEngineeringCompoundCrs`, decision (b) of
    /// 24/09), in a `ProjectedCrs` and its vertical system when the frame is a
    /// `ProjectedCrs` named as it stands (`ReferencePointInACrs`). Both are
    /// declared, the first first - it is what a frame created here makes.
    #[param(
        group = "frame",
        creates = ["eml23.ReferencePointInALocalEngineeringCompoundCrs", "eml23.ReferencePointInACrs"],
        reads = ["MdDatum"],
        need = Need::Object { types: &["eml23.ReferencePointInALocalEngineeringCompoundCrs", "eml23.ReferencePointInACrs"] }
    )]
    pub datum: Option<ObjectRef>,
    /// what the created datum is
    ///
    /// It describes the datum this operation makes; naming an existing one
    /// leaves it nothing to describe.
    #[param(default = DatumKind::GroundLevel, optional, without = ["datum"], group = "frame", reads = ["MdDatum/Kind"])]
    pub datum_kind: DatumKind,
    /// the trajectory through the points: a linear spline invents nothing; the cubics smooth; a vertical line is detected and written as such
    #[param(default = TrajectoryShape::Linear, optional)]
    pub shape: TrajectoryShape,
    /// a frame node every so many units of MD; left out, one per control point
    #[param(min = 0.0)]
    pub spacing: Option<f64>,
}

/// What `well-from-polyline` takes: **one** polyline - a shape of the scene or
/// of the package, or a line of a feature collection.
///
/// **`trace` and not `shape`**: this operation already has a setting named
/// `shape`, the *kind* of line, and the house word for a drawn line is
/// **trace** (see the section views). And the GeoJSON line the operation
/// always took is declared now: a slot naming only shapes hid it.
#[derive(Debug, Inputs)]
pub struct WellInputs {
    /// the polyline the trajectory follows
    #[input(topology = "polylines")]
    pub trace: Option<ShapeIn>,
    /// a feature collection holding the line, in its own system
    pub features: Option<Features>,
}

/// Every polyline `trace` or `features` holds - a shape's patches, and the
/// collection's lines. Shared with `wells-from-polylines`, which writes one
/// well per line instead of refusing more than one, and so takes a slice of
/// shapes rather than `WellInputs`' single, optional one.
pub(crate) fn polylines_of(trace: Option<&ShapeIn>, features: Option<&Features>) -> Vec<Found> {
    let mut found: Vec<Found> = Vec::new();
    if let Some(shape) = trace {
        for patch in &shape.geometry.patches {
            match &patch.topology {
                Topology::Polylines(runs) => {
                    let mut start = 0usize;
                    for &run in runs {
                        let run = run as usize;
                        found.push(Found::Placed {
                            points: patch.points.get(start..start + run).unwrap_or(&[]).to_vec(),
                            name: shape.name.clone(),
                            known: shape.known.clone().map(Box::new),
                        });
                        start += run;
                    }
                }
                Topology::Points if patch.points.len() >= 2 => found.push(Found::Placed {
                    points: patch.points.clone(),
                    name: shape.name.clone(),
                    known: shape.known.clone().map(Box::new),
                }),
                _ => {}
            }
        }
    }
    if let Some(collection) = features {
        for feature in &collection.features {
            if matches!(feature.shape, Shape::Lines) {
                for part in &feature.parts {
                    found.push(Found::Foreign(part.clone()));
                }
            }
        }
    }
    found
}

/// A found polyline has enough points, all finite - the one check every
/// caller makes, worded the same way either way.
pub(crate) fn valid_polyline(line: &Found) -> Result<(), String> {
    let (count, finite) = match line {
        Found::Placed { points, .. } => (points.len(), points.iter().flatten().all(|v| v.is_finite())),
        Found::Foreign(positions) => (
            positions.len(),
            positions.iter().all(|p| p.z.is_some_and(f64::is_finite)),
        ),
    };
    if count < 2 {
        return Err(format!(
            "a wellbore needs at least 2 points; the polyline has {count}"
        ));
    }
    if !finite {
        return Err("the polyline has a point with no position, or no elevation".to_string());
    }
    Ok(())
}

/// The one polyline of the bag, or why there is not exactly one.
fn polyline_of(inputs: &WellInputs) -> Result<Found, String> {
    let mut found = polylines_of(inputs.trace.as_ref(), inputs.features.as_ref());
    match found.len() {
        0 => Err("it takes one polyline - a shape with one, or a feature collection with one line; the input holds none".to_string()),
        1 => {
            let line = found.remove(0);
            valid_polyline(&line)?;
            Ok(line)
        }
        n => Err(format!("it takes one polyline and the input holds {n} - one wellbore per polyline, run it once each")),
    }
}

/// The topmost Z, for the ground's suggestion: the highest elevation, or the
/// smallest depth, or the maximum when the direction is unknown.
pub(crate) fn topmost(zs: &[f64], vertical_up: Option<bool>) -> f64 {
    match vertical_up {
        Some(false) => zs.iter().copied().fold(f64::INFINITY, f64::min),
        _ => zs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    }
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Measured depth at each point: the drop from the ground to the first, then
/// the chord length along the line.
fn measured_depths(points: &[[f64; 3]], ground: f64) -> Vec<f64> {
    let mut out = Vec::with_capacity(points.len());
    let mut md = (ground - points[0][2]).abs();
    out.push(md);
    for pair in points.windows(2) {
        md += distance(pair[0], pair[1]);
        out.push(md);
    }
    out
}

/// Whether RESQML's vertical line kind would place this line where its points
/// are - which is a question about the *datum*, not about the shape.
///
/// Kind 0 does not interpolate: *"Parameter values are interpreted as depth =>
/// (X,Y,Z), where the depth to Z conversion depends on the vertical CRS
/// direction"* (TRG v2.2 §3.8). A trajectory's parameter is the **measured
/// depth**, counted from the datum. The two are the same number only when the
/// datum sits at the system's zero - and an MD datum is the ground, which is at
/// 39 m on a BRGM borehole and at −25 m offshore.
///
/// Written as kind 0 anyway, a hole whose head is at 39.57 m has its frame node
/// at MD 1.8 read back at z = −1.8: forty metres below where the trajectory
/// passes, on a line the reader still draws correctly. Found on the SIGEOL
/// import of 19/09 - a thousand wellbore frames placed at the wrong depth, the
/// map right and the 3D view empty of anything one could recognise.
fn vertical_agrees(mds: &[f64], points: &[[f64; 3]], up: bool, length: f64) -> bool {
    let slack = 1e-6 * length.max(1.0);
    mds.iter()
        .zip(points)
        .all(|(md, p)| ((if up { -md } else { *md }) - p[2]).abs() <= slack)
}

/// Every point over the first, within a millionth of the length.
fn is_vertical(points: &[[f64; 3]], length: f64) -> bool {
    let slack = 1e-6 * length.max(1.0);
    points
        .iter()
        .all(|p| (p[0] - points[0][0]).abs() <= slack && (p[1] - points[0][1]).abs() <= slack)
}

/// Catmull-Rom tangents, unit, one-sided at the ends.
fn tangents(points: &[[f64; 3]]) -> Vec<[f64; 3]> {
    let n = points.len();
    (0..n)
        .map(|i| {
            let (a, b) = (points[i.saturating_sub(1)], points[(i + 1).min(n - 1)]);
            let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            if len > 0.0 {
                [d[0] / len, d[1] / len, d[2] / len]
            } else {
                [0.0, 0.0, 1.0]
            }
        })
        .collect()
}

/// The frame's nodes: one per control point, or every `spacing` of MD from
/// the first to the last, the last always included.
fn frame_mds(mds: &[f64], spacing: Option<f64>) -> Vec<f64> {
    let Some(step) = spacing.filter(|s| s.is_finite() && *s > 0.0) else {
        return mds.to_vec();
    };
    let (first, last) = (mds[0], mds[mds.len() - 1]);
    let mut out = Vec::new();
    let mut md = first;
    while md < last - step * 1e-9 {
        out.push(md);
        md += step;
    }
    out.push(last);
    out
}

fn floats(file: &str, path: &str, count: usize) -> Value {
    json!({
        "$type": "eml23.FloatingPointExternalArray",
        "ArrayFloatingPointType": "arrayOfDouble64LE",
        "CountPerValue": 1,
        "Values": support::external(file, path, &[count as i64]),
    })
}

fn points_array(file: &str, path: &str, count: usize) -> Value {
    json!({
        "$type": "resqml22.Point3dExternalArray",
        "Coordinates": support::external(file, path, &[count as i64, 3]),
    })
}

/// What [`datum_in`] is told.
pub struct Datum<'a> {
    /// The datum the caller named, when one was named - nothing is made then.
    pub named: Option<&'a Value>,
    /// The head: the first control point's horizontal coordinates.
    pub head: [f64; 2],
    /// The ground there - where the measured depths start.
    pub ground: f64,
    /// What a created datum is.
    pub kind: DatumKind,
    /// The frame it sits in - the `LocalCrs` the trajectory names. Its type
    /// decides the form of a created datum ([`datum_in`]).
    pub crs: &'a Value,
    /// The frame's vertical CRS, which a datum in a `ProjectedCrs` must
    /// reference so that a reader knows which way is down. A datum in a
    /// compound reaches it through the compound, and needs nothing here.
    pub vertical_crs: Option<&'a Value>,
    /// What the created object is titled after.
    pub title: &'a str,
    /// What names it.
    pub seed: &'a str,
    /// Its citation.
    pub citation: &'a dyn Fn(&str) -> Value,
}

/// The reference point the measured depths are counted from: the one named, or
/// one created at the head, at the ground.
///
/// **Apart from [`write()`], and deliberately.** A datum is a fact about the
/// *frame* - where zero is - not about the curve; keeping it here lets a caller
/// put it in its plan where it belongs, and lets the writer take it as a
/// reference like it takes its interpretation. Folding it into `write` moved it
/// after the feature in `well-from-polyline`'s plan, which a refactor meant to
/// change nothing had no business doing.
pub fn datum_in(
    datum: &Datum<'_>,
    identity: &Identity,
    rank: &mut usize,
) -> Result<(Value, Vec<NewObject>), Error> {
    match datum.named {
        Some(value) if value.is_object() && value["Uuid"].is_string() => Ok((value.clone(), Vec::new())),
        Some(other) => {
            Err(Refusal::on("datum", format!("expected a DataObjectReference, got {other}")).into())
        }
        None => {
            // **The frame decides the form** (decision (b) of 24/09). What the
            // trajectory's `LocalCrs` is, the datum is in: a position in a
            // compound is local, and the compound reaches the vertical system
            // itself; a position in a `ProjectedCrs` is projected, and needs
            // the vertical system beside it. A frame that is no whole system
            // - a half, a 2.0.1 system, none at all - has already blocked the
            // plan (`support::frame_in`), and the datum is then written in it
            // as in a compound, never to be applied.
            let qualified = datum.crs["QualifiedType"].as_str().unwrap_or_default();
            let in_projected = match crate::crs::frame_kind(qualified) {
                Ok(FrameKind::Projected) => true,
                Ok(FrameKind::Geographic) => {
                    return Err(Refusal::on(
                        "datum",
                        format!(
                            "the frame `{}` is a Geographic2dCrs, in degrees, and a datum's vertical coordinate is linear: no reader reconciles the two - write the wellbore under a projected system",
                            datum.crs["Title"].as_str().unwrap_or_default()
                        ),
                    )
                    .into());
                }
                _ => false,
            };
            let uuid = support::next_uuid(identity, &format!("well-datum:{}", datum.seed), rank)?;
            let title = format!("{} - datum", datum.title);
            let (qualified_type, json) = if in_projected {
                let vertical = datum.vertical_crs.cloned().ok_or_else(|| {
                    Error::from(Refusal::new(
                        "datum",
                        "a datum in a ProjectedCrs needs the frame's vertical CRS, which a reader resolves to know which way is down - the input does not carry it: name an existing `datum`, give the object's vertical CRS, or name the LocalEngineeringCompoundCrs that joins the two"
                            .to_string(),
                    ))
                })?;
                (
                    "eml23.ReferencePointInACrs",
                    json!({
                        "$type": "eml23.ReferencePointInACrs",
                        "uuid": uuid,
                        "schemaVersion": "2.3",
                        "Citation": (datum.citation)(&title),
                        "Kind": datum.kind.word(),
                        "VerticalCoordinate": datum.ground,
                        // Projected coordinates **in the `ProjectedCrs`** -
                        // the only frame they can be given for, now that a
                        // compound takes the other form.
                        "HorizontalCoordinates": {
                            "$type": "eml23.ProjectedCoordinates",
                            "Coordinate1": datum.head[0],
                            "Coordinate2": datum.head[1],
                            "Crs": datum.crs,
                        },
                        "VerticalCrs": vertical,
                    }),
                )
            } else {
                (
                    "eml23.ReferencePointInALocalEngineeringCompoundCrs",
                    json!({
                        "$type": "eml23.ReferencePointInALocalEngineeringCompoundCrs",
                        "uuid": uuid,
                        "schemaVersion": "2.3",
                        "Citation": (datum.citation)(&title),
                        "Kind": datum.kind.word(),
                        // Local coordinates, in the compound the trajectory is
                        // in - the head is its first control point, and the
                        // ground a z of the same axis (common TRG v2.3 §13.25,
                        // p. 371: *“a reference point which is defined in the
                        // context of a compound (2D horizontal + 1D vertical)
                        // CRS”*).
                        "HorizontalCoordinates": {
                            "$type": "eml23.HorizontalCoordinates",
                            "Coordinate1": datum.head[0],
                            "Coordinate2": datum.head[1],
                        },
                        "VerticalCoordinate": datum.ground,
                        "Crs": datum.crs,
                    }),
                )
            };
            let made = NewObject {
                uuid: uuid.clone(),
                qualified_type: qualified_type.to_string(),
                role: "datum".to_string(),
                title: title.clone(),
                json,
            };
            Ok((
                json!({
                    "$type": "eml23.DataObjectReference",
                    "Uuid": uuid,
                    "QualifiedType": qualified_type,
                    "Title": title,
                }),
                vec![made],
            ))
        }
    }
}

/// One wellbore's **trajectory and frame** - the writer both operations go
/// through.
///
/// Extracted from [`WellFromPolyline::plan`] on 18/09, when `sigeol-boreholes`
/// needed the same two objects from a different vocabulary. The house rule
/// (`crates/energyml-build/ARCHITECTURE.md`: an operation that writes the same object
/// as another delegates to it): a second writer would be a second place for the
/// same mistakes - the measured depths, the vertical detection, the parametric
/// line's kind.
///
/// What is **not** here is the feature and its interpretation, nor the datum:
/// this takes a reference to each and makes none of them. `well-from-polyline` makes a
/// bare pair when none is named; `sigeol-boreholes` makes one carrying the
/// producer's own identifiers. Folding those together would mean deciding
/// where a BSS alias lives on a generic wellbore feature.
pub struct Wellbore<'a> {
    /// The name every object of this hole is titled after.
    pub title: &'a str,
    /// What names them: the seed of every derived uuid.
    pub seed: &'a str,
    /// The uuids of the trajectory and of the frame to **rewrite**, when
    /// a borehole that the package already holds is continued.
    ///
    /// When given, they replace the derived uuids: it is *those* objects that are
    /// rewritten, and a derived uuid would name a copy that nothing else in the
    /// package references. The derivation is done all the same, so that the
    /// rank advances - `Identity::Given` hands out its uuids in the order of the
    /// plan, and skipping one would shift all the following ones.
    pub keep: Option<(&'a str, &'a str)>,
    /// The trajectory's control points, in the frame.
    pub line: &'a [[f64; 3]],
    /// The measured depth of each control point, when the **caller states
    /// them** rather than the line implying them.
    ///
    /// `None` is `well-from-polyline`'s case: a line was drawn, and its
    /// measured depths are its own geometry - the drop from the ground, then
    /// the chord lengths. `Some` is `sigeol-boreholes`', where the depths are
    /// what the file says and the line was built *from* them.
    ///
    /// **The distinction is not cosmetic, and it was found by a test.**
    /// Rebuilding the depths from a line that was built from the depths does
    /// not return them: a hole whose file says `6.4` came back `6.399999999999999`,
    /// because the round trip is a subtraction followed by an accumulation of
    /// chord lengths. Writing that number would be this crate inventing a value
    /// by a detour and putting it where a producer's own is expected.
    pub mds: Option<&'a [f64]>,
    /// The vertical coordinate of the ground at the head - where the measured
    /// depths start.
    pub ground: f64,
    /// The unit of the measured depths.
    pub uom: &'a str,
    /// The curve through the control points.
    pub shape: TrajectoryShape,
    /// The line's tangent at each control point, in the frame's axes - given
    /// by a caller who knows them exactly (`well-from-plan`, whose stations
    /// lie on minimum-curvature arcs). The trajectory is then RESQML's
    /// minimum-curvature line (`LineKindIndex` 5 with its `TangentVectors`,
    /// read as circular arcs by `energyml-geometry`'s `parametric.rs`) and
    /// `shape` is not read; a vertical line keeps its vertical reading. One
    /// per control point, or refused.
    pub tangents: Option<&'a [[f64; 3]]>,
    /// A frame node every so many units of MD; `None` is one per control point.
    pub spacing: Option<f64>,
    /// The datum the measured depths are counted from - made by [`datum_in`],
    /// or named by the caller. **Given, never made here**: the writer points
    /// at two objects, its datum and its interpretation, and creates neither.
    pub datum: &'a Value,
    /// What the two representations represent.
    pub interpretation: &'a Value,
    /// The external file the arrays go into.
    pub file: &'a str,
    /// Which way the frame's vertical axis points - `None` when unknown, and
    /// a vertical hole then stays a linear spline rather than being written as
    /// RESQML's vertical line with parameters nobody can orient.
    pub vertical_up: Option<bool>,
    /// The `LocalCrs` every geometry carries.
    pub crs: &'a Value,
    /// The citation every object gets - passed rather than built, because a
    /// producer's own dates belong on the objects made from its file.
    pub citation: &'a dyn Fn(&str) -> Value,
}

/// What [`write()`] wrote, and what it measured doing so.
pub struct Written {
    /// The trajectory and the frame.
    pub objects: Vec<NewObject>,
    /// Their arrays.
    pub datasets: Vec<Dataset>,
    /// What the writing wants said.
    pub says: Vec<Note>,
    /// The measured depth of each control point.
    pub mds: Vec<f64>,
    /// The RESQML line kind written: 0 vertical, 1 linear, 2 natural cubic,
    /// 3 tangential cubic.
    pub kind_index: i64,
    /// How many nodes the frame got.
    pub nodes: usize,
}

/// Write one wellbore's trajectory and frame.
pub fn write(well: &Wellbore<'_>, identity: &Identity, rank: &mut usize) -> Result<Written, Error> {
    let (title, line, ground, uom) = (well.title, well.line, well.ground, well.uom);
    let file = well.file;
    let cite = well.citation;
    let mut objects = Vec::new();
    let mut datasets = Vec::new();
    let mut says = Vec::new();

    let datum = well.datum;

    // -- the measured depths and the line's kind -------------------------
    let mds = match well.mds {
        Some(given) if given.len() != line.len() => {
            return Err(Error::new(format!(
                "{} measured depth(s) were given for {} control point(s): a frame node has to be on a knot",
                given.len(),
                line.len()
            )));
        }
        Some(given) => given.to_vec(),
        None => measured_depths(line, ground),
    };
    let length = mds[mds.len() - 1] - mds[0];
    if (ground - line[0][2]).abs() > 0.0 {
        match well.vertical_up {
            Some(true) if ground < line[0][2] => says.push(Note {
                key: "ground-below",
                detail: format!("the ground {ground} is below the first point {} - the measured depths start with that drop anyway", line[0][2]),
            }),
            Some(false) if ground > line[0][2] => says.push(Note {
                key: "ground-below",
                detail: format!("the ground {ground} is deeper than the first point {} - the measured depths start with that drop anyway", line[0][2]),
            }),
            _ => {}
        }
    }
    if let Some(given) = well.tangents
        && given.len() != line.len()
    {
        return Err(Error::new(format!(
            "{} tangent(s) were given for {} control point(s): a minimum-curvature line has one at each knot",
            given.len(),
            line.len()
        )));
    }
    let vertical = is_vertical(line, length);
    // **The parameter written is the measured depth, whatever the kind.** A
    // trajectory's parametric line is read at the frame's `NodeMd` - `at(md)`
    // in `energyml-geometry/src/parametric.rs`, and the corpus agrees:
    // `acd2cdcf`'s parameters run 0…1000 and its frames sample 0, 200 … 1000
    // of the same well. Writing anything else there puts every frame node
    // somewhere the trajectory does not pass.
    let (kind_index, tangents_written): (i64, Option<Vec<[f64; 3]>>) = match (
        vertical,
        well.vertical_up,
        well.shape.word(),
    ) {
        (false, _, _) if well.tangents.is_some() => {
            let given = well.tangents.expect("the arm's guard holds tangents");
            says.push(Note {
                    key: "minimum-curvature",
                    detail: "written as RESQML's minimum-curvature line (LineKindIndex 5): the circular arcs through the given stations, their tangents written".to_string(),
                });
            (5, Some(given.to_vec()))
        }
        (true, Some(up), _) if vertical_agrees(&mds, line, up, length) => {
            says.push(Note { key: "vertical", detail: "the polyline is vertical and its measured depths are the CRS's depths: written as RESQML's vertical line".to_string() });
            (0, None)
        }
        (true, Some(_), _) => {
            says.push(Note {
                    key: "vertical-datum-offset",
                    detail: "the polyline is vertical but its measured depths are not the vertical CRS's depths - the ground is not at the system's zero - so it is written as a linear spline through its points, whose parameter is the measured depth".to_string(),
                });
            (1, None)
        }
        (true, None, _) => {
            says.push(Note { key: "vertical-unknown-direction", detail: "the polyline is vertical but the frame's direction is unknown: written as a linear spline through its points".to_string() });
            (1, None)
        }
        (false, _, "natural-cubic") => (2, None),
        (false, _, "tangential-cubic") => (3, Some(tangents(line))),
        (false, _, _) => (1, None),
    };
    let parameters_written = mds.clone();

    // -- the trajectory ---------------------------------------------------
    let derived_trajectory = support::next_uuid(identity, &format!("well-trajectory:{}", well.seed), rank)?;
    let trajectory_uuid = well
        .keep
        .map(|(one, _)| one.to_string())
        .unwrap_or(derived_trajectory);
    let trajectory_title = format!("{title} - trajectory");
    let control_path = format!("/resqml22/{trajectory_uuid}/controlPoints");
    let parameters_path = format!("/resqml22/{trajectory_uuid}/controlPointParameters");
    let mut geometry = json!({
        "$type": "resqml22.ParametricLineGeometry",
        "LocalCrs": well.crs,
        "KnotCount": line.len(),
        "LineKindIndex": kind_index,
        "ControlPoints": points_array(file, &control_path, line.len()),
        "ControlPointParameters": floats(file, &parameters_path, line.len()),
    });
    datasets.push(Dataset {
        file: file.to_string(),
        path: control_path,
        dimensions: vec![line.len() as i64, 3],
        values: Numbers::F64(line.iter().flatten().copied().collect()),
    });
    datasets.push(Dataset {
        file: file.to_string(),
        path: parameters_path,
        dimensions: vec![line.len() as i64],
        values: Numbers::F64(parameters_written),
    });
    if let Some(tangents) = tangents_written {
        let path = format!("/resqml22/{trajectory_uuid}/tangentVectors");
        geometry["TangentVectors"] = points_array(file, &path, tangents.len());
        datasets.push(Dataset {
            file: file.to_string(),
            path,
            dimensions: vec![tangents.len() as i64, 3],
            values: Numbers::F64(tangents.into_iter().flatten().collect()),
        });
    }
    objects.push(NewObject {
        uuid: trajectory_uuid.clone(),
        qualified_type: "resqml22.WellboreTrajectoryRepresentation".to_string(),
        role: "trajectory".to_string(),
        title: trajectory_title.clone(),
        json: json!({
            "$type": "resqml22.WellboreTrajectoryRepresentation",
            "uuid": trajectory_uuid,
            "schemaVersion": "2.2",
            "Citation": cite(&trajectory_title),
            "RepresentedObject": well.interpretation,
            "MdInterval": {
                "$type": "eml23.MdInterval",
                "MdMin": mds[0],
                "MdMax": mds[mds.len() - 1],
                "Uom": uom,
                "Datum": datum,
            },
            "MdDomain": "driller",
            "Geometry": geometry,
        }),
    });

    // -- the frame ---------------------------------------------------------
    let nodes = frame_mds(&mds, well.spacing);
    let derived_frame = support::next_uuid(identity, &format!("well-frame:{}", well.seed), rank)?;
    let frame_uuid = well.keep.map(|(_, one)| one.to_string()).unwrap_or(derived_frame);
    let frame_title = format!("{title} - frame");
    let node_path = format!("/resqml22/{frame_uuid}/nodeMd");
    objects.push(NewObject {
        uuid: frame_uuid.clone(),
        qualified_type: "resqml22.WellboreFrameRepresentation".to_string(),
        role: "frame".to_string(),
        title: frame_title.clone(),
        json: json!({
            "$type": "resqml22.WellboreFrameRepresentation",
            "uuid": frame_uuid,
            "schemaVersion": "2.2",
            "Citation": cite(&frame_title),
            "RepresentedObject": well.interpretation,
            "Trajectory": {
                "$type": "eml23.DataObjectReference",
                "Uuid": trajectory_uuid,
                "QualifiedType": "resqml22.WellboreTrajectoryRepresentation",
                "Title": trajectory_title,
            },
            "NodeCount": nodes.len(),
            "NodeMd": floats(file, &node_path, nodes.len()),
        }),
    });
    datasets.push(Dataset {
        file: file.to_string(),
        path: node_path,
        dimensions: vec![nodes.len() as i64],
        values: Numbers::F64(nodes.clone()),
    });

    Ok(Written {
        objects,
        datasets,
        says,
        mds,
        kind_index,
        nodes: nodes.len(),
    })
}

impl Operation for WellFromPolyline {
    type Params = WellParams;
    type Inputs = WellInputs;

    fn about(&self) -> About {
        About {
            name: "well-from-polyline",
            describe: "a wellbore from one polyline: the datum at the ground, the measured depths, a trajectory through the points, a frame on it - and the feature and interpretation when none is named",
            category: Category::Wells,
            makes: &[
                "eml23.ReferencePointInALocalEngineeringCompoundCrs",
                "eml23.ReferencePointInACrs",
                "resqml22.WellboreFeature",
                "resqml22.WellboreInterpretation",
                "resqml22.WellboreTrajectoryRepresentation",
                "resqml22.WellboreFrameRepresentation",
            ],
            recipe: "well-from-polyline",
            // It writes the pair itself when no interpretation is named -
            // declared so that the form can offer *create one* beside *choose
            // one*, before anything is planned.
            represents: Some(("resqml22.WellboreFeature", "resqml22.WellboreInterpretation")),
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, self.about().represents, slots);
        let known = bag.objects().map(|(_, k)| k).next();
        if let Some(unit) = known.and_then(|k| k.vertical_uom.clone()) {
            slots.set("mdUom", serde_json::json!(unit));
        }
        // The topmost Z, to help: the highest elevation, or the smallest depth.
        if let Ok(line) = WellInputs::bind(bag)
            .map_err(|_| ())
            .and_then(|inputs| polyline_of(&inputs).map_err(|_| ()))
        {
            slots.set(
                "ground",
                serde_json::json!(topmost(&line.zs(), known.and_then(|k| k.vertical_up))),
            );
        }
    }

    fn check(&self, inputs: &WellInputs) -> Result<(), Refusal> {
        polyline_of(inputs)
            .map(|_| ())
            .map_err(|why| Refusal::on("trace", why))
    }

    fn plan(&self, inputs: WellInputs, params: WellParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        let found = polyline_of(&inputs).map_err(|why| Refusal::on("trace", why))?;
        let written = &params.written;
        let title = written.naming.title();
        let seed = cx.seed(&title);
        let mut rank = 0usize;
        let identity = cx.identity;
        let when = cx.when;
        let frame = support::frame_in(&cx.known(), &written.frame, identity, when, &mut rank)?;
        // **A wellbore is a vertical extent, and degrees cannot hold one** -
        // the refusal `sigeol-boreholes` and `wells-from-table` already make
        // (the argument is written out in `sigeol_boreholes`), which this
        // operation lacked: its depths are metres, a
        // `Geographic2dCrs`'s horizontal axes are degrees, and every hole comes
        // out between a hundred and ten thousand times longer than the model
        // is wide, in a package `check` validates.
        if frame.kind == Some(FrameKind::Geographic) {
            let (slot, what) = match written.frame.crs.as_ref() {
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
                    "a wellbore is a vertical extent, and {what} measures its horizontal axes in degrees while its depths are linear: every hole would be written between a hundred and ten thousand times longer than the model is wide, which no reader can reconcile - name a projected system in `project` (EPSG:2154 for mainland France), or a LocalEngineeringCompoundCrs as `crs`"
                ),
            ));
        }
        let line: Vec<[f64; 3]> = found.placed_in(&frame)?;
        let known: Option<&Known> = cx.known().into_iter().next();
        let vertical_up = if frame.objects.is_empty() {
            known.and_then(|k| k.vertical_up)
        } else {
            Some(true)
        };
        let ground = params.ground;
        let uom = params.md_uom.clone();
        let shape = params.shape;
        let file = written.naming.data_file.clone();
        let mut says = Vec::new();
        let mut objects = frame.objects;
        let mut datasets = Vec::new();
        let mut parameters = frame.parameters;

        // -- the datum -------------------------------------------------------
        // Made by the shared maker, which `sigeol-boreholes` goes through too:
        // a second copy would be a second place for the same mistakes.
        let cite = |title: &str| support::citation(when, title);
        let (datum, datum_objects) = datum_in(
            &Datum {
                named: params.datum.as_ref().map(|datum| &datum.0),
                head: [line[0][0], line[0][1]],
                ground,
                kind: params.datum_kind,
                crs: &frame.crs,
                vertical_crs: frame.vertical.as_ref(),
                title: &title,
                seed: &seed,
                citation: &cite,
            },
            identity,
            &mut rank,
        )?;
        objects.extend(datum_objects);

        // -- the feature and its interpretation, when none is named ---------
        let interpretation: Value = match written.represented.interpretation.as_ref().map(|one| &one.0) {
            Some(value) if value.is_object() && value["Uuid"].is_string() => value.clone(),
            Some(other) => {
                return Err(Refusal::on(
                    "interpretation",
                    format!("expected a DataObjectReference, got {other}"),
                ));
            }
            None => {
                let feature_uuid = support::next_uuid(identity, &format!("well-feature:{seed}"), &mut rank)?;
                let feature_title = format!("{title} - feature");
                objects.push(NewObject {
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
                });
                let uuid = support::next_uuid(identity, &format!("well-interpretation:{seed}"), &mut rank)?;
                let interpretation_title = format!("{title} - interpretation");
                objects.push(NewObject {
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
                        "IsDrilled": false,
                        "InterpretedFeature": {
                            "$type": "eml23.DataObjectReference",
                            "Uuid": feature_uuid,
                            "QualifiedType": "resqml22.WellboreFeature",
                            "Title": feature_title,
                        },
                    }),
                });
                json!({
                    "$type": "eml23.DataObjectReference",
                    "Uuid": uuid,
                    "QualifiedType": "resqml22.WellboreInterpretation",
                    "Title": interpretation_title,
                })
            }
        };

        // -- the trajectory and the frame ------------------------------------
        let written_well = write(
            &Wellbore {
                title: &title,
                seed: &seed,
                line: &line,
                mds: None,
                ground,
                uom: &uom,
                shape,
                tangents: None,
                spacing: params.spacing,
                datum: &datum,
                interpretation: &interpretation,
                file: &file,
                vertical_up,
                crs: &frame.crs,
                citation: &cite,
                // Nothing to resume: this operation creates, it does not take up again.
                keep: None,
            },
            identity,
            &mut rank,
        )?;
        let mds = written_well.mds;
        let kind_index = written_well.kind_index;
        let nodes_count = written_well.nodes;
        objects.extend(written_well.objects);
        datasets.extend(written_well.datasets);
        says.extend(written_well.says);

        parameters.extend([
            Parameter::Double {
                title: "ground".to_string(),
                value: ground,
                uom: Some(uom.clone()),
            },
            Parameter::Text {
                title: "shape".to_string(),
                value: TrajectoryShape::WORDS
                    .get(kind_index.max(1) as usize - 1)
                    .unwrap_or(&"linear")
                    .to_string(),
            },
            Parameter::Double {
                title: "lineKind".to_string(),
                value: kind_index as f64,
                uom: None,
            },
            Parameter::Double {
                title: "controlPointCount".to_string(),
                value: line.len() as f64,
                uom: None,
            },
            Parameter::Double {
                title: "frameNodeCount".to_string(),
                value: nodes_count as f64,
                uom: None,
            },
            Parameter::Double {
                title: "mdMax".to_string(),
                value: mds[mds.len() - 1],
                uom: Some(uom),
            },
        ]);
        // What the frame must say of itself - a `ProjectedCrs` taken as it
        // stands - after what the well said.
        says.extend(frame.says);
        Ok(Plan {
            media: Vec::new(),
            // **What the frame blocked, the plan carries.** This was `None`
            // whatever `frame_in` said, so a well under a RESQML 2.0.1 system
            // was applied half of one release; and a half named as the frame -
            // a `VerticalCrs` - would have been written as the trajectory's
            // `LocalCrs`.
            blocked: frame.blocked,
            objects,
            datasets,
            // **No shape, and for a reason of correctness.** What is written
            // is a *parametric line*: its control points are its
            // trajectory only when it is linear. Returning the control
            // polyline would give, for a cubic, a shape that the re-reading
            // of the object will contradict - two readers in agreement on nodes and
            // in disagreement on the curve that joins them.
            shapes: Vec::new(),
            activity: ActivityDraft {
                recipe: "well-from-polyline",
                title: format!("well-from-polyline → {title}"),
                seed,
                parameters,
            },
            says,
            dropped: Vec::new(),
        })
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
            count(
                "lineKind",
                "the RESQML line kind written: 0 vertical, 1 linear, 2 natural cubic, 3 tangential cubic",
            ),
            count("controlPointCount", "control points"),
            count("frameNodeCount", "frame nodes"),
            count("mdMax", "the deepest measured depth"),
        ])
    }
}
