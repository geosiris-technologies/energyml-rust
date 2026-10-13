//! `transform` — a shape **moved, turned and scaled**: a translation, a
//! rotation about an axis through a pivot, a scale taken from the same pivot, on
//! the whole shape or on some of its nodes.
//!
//! The front end drives it through an editing mode.
//! The need: a translation (specific to each axis) or
//! a rotation (an XYZ axis entered by the user and an angle in degrees or
//! radians) — a node alone, to correct a badly placed point — and,
//! the scale: the scale is a welcome addition.
//!
//! # What it is, and what it is not
//!
//! An **affine** move, rigid when there is no scale: the topology is copied as
//! it is — same nodes, same order, same triangles or polylines. So the
//! properties of a moved object stay attached to the right nodes (they are
//! indexed by node, and no node is added or lost), and a copy is `transform`
//! with nothing to do — which is how the front end duplicates an object into an
//! artefact. A scale changes distances, which a thickness or an area measured
//! before it no longer matches — the activity says it was scaled.
//!
//! The scale is **not** the vertical exaggeration: that one changes how the
//! scene is looked at and writes nothing; this one writes the geometry.
//!
//! It **creates**: the result is a new shape, written as a new representation
//! of the same kind. Whether a viewer shows it as a
//! copy or in place of the original is the viewer's decision; nothing here
//! modifies an object of the package.
//!
//! # The order, stated once
//!
//! **The scale first, then the rotation, both about the pivot, then the
//! translation** — the classic order. The other orders would turn or scale the
//! translated shape about a pivot it has left, or stretch along axes the
//! rotation has already turned. There is **one** pivot for both: scaled about
//! one point and turned about another is not a gesture anybody makes. It is
//! computed on **what moves** — the centre of the box of the chosen nodes, or of
//! the whole shape — unless it is given; and it is read only when something
//! turns or scales (`Applies::Any`), so a translation records none.
//!
//! # Refused by name
//!
//! - an axis of length zero: a rotation needs a direction;
//! - a scale of zero, which flattens the shape onto a plane — that is a
//!   projection, and it will have that name — or negative, which is a mirror and
//!   turns every triangle inside out without saying so;
//! - a node out of the shape: a correction aimed at a node that does not exist
//!   would move nothing and look as if it had worked;
//! - a rotation about an axis that is not vertical, when the shape's heights are
//!   a **time** (its frame says so): x and y are metres, z milliseconds, and
//!   turning one into the other gives a surface that looks like one and is
//!   nothing — the refusal of `thickness` for the isopach, for the same reason;
//! - a topology this writes nothing for yet — a grid's cells, a volume: moving a
//!   grid would write it as a triangulated surface or as a lattice with turned
//!   vectors, and choosing is a decision, not a default. Mixed patches too
//!   (faces **and** lines): one representation holds one kind.

use energyml_analysis::nodes::flipped;
use energyml_domain::provenance::Parameter;
use energyml_domain::units::Dimension;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

use crate::declare::{Choice, Inputs, Params, Shape, Suggestions};
use crate::operation::{About, Category, Context, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::Pieces;

use super::pointset::{Pointset, PointsetInputs, PointsetParams};
use super::polylines::PolylinesInputs;
use super::support;
use super::triangles::TrianglesInputs;
use super::written::Written;

/// The operation.
pub struct Transform;

// ---------------------------------------------------------------- the declaration

/// What `transform` is told.
#[derive(Debug, Params)]
pub struct TransformParams {
    /// the move along x, in the units of the frame
    #[param(default = 0.0, optional)]
    pub dx: f64,
    /// the move along y, in the units of the frame
    #[param(default = 0.0, optional)]
    pub dy: f64,
    /// the move along z, in the unit of the heights — metres, or milliseconds for a surface in time
    #[param(default = 0.0, optional)]
    pub dz: f64,
    /// The rotation, if any, and its settings.
    #[param(flatten)]
    pub rotation: Rotation,
    /// The scale, if any, and its factors.
    #[param(flatten)]
    pub scale: Scale,
    /// The point the rotation and the scale are about — read only when one of
    /// them is asked for. After both, which it depends on.
    #[param(flatten, when_any = "rotate: axis; scale: uniform, axes")]
    pub pivot: Pivot,
    /// only these nodes move — their ranks in the shape; left out, the whole shape moves
    ///
    /// **Advanced**: a form fills it from the nodes chosen in the scene, not by
    /// typing ranks.
    #[param(advanced)]
    pub nodes: Vec<usize>,
    /// The frame, what is represented, the naming — for the writer of the result.
    #[param(flatten)]
    pub written: Written,
}

/// whether the shape turns
#[derive(Debug, Params)]
#[params(tag = "rotate")]
pub enum Rotation {
    /// No rotation: a translation alone.
    #[case(default, name = "none")]
    Still,
    /// A rotation about an axis.
    Axis(AxisParams),
}

/// A rotation: the axis and the angle — about the pivot.
#[derive(Debug, Params)]
pub struct AxisParams {
    /// rotation: the x component of the axis
    #[param(default = 0.0, optional)]
    pub axis_x: f64,
    /// rotation: the y component of the axis
    #[param(default = 0.0, optional)]
    pub axis_y: f64,
    /// rotation: the z component of the axis — (0, 0, 1) turns about the vertical
    #[param(default = 1.0, optional)]
    pub axis_z: f64,
    /// rotation: the angle, anticlockwise looking down the axis
    pub angle: f64,
    /// rotation: the unit of the angle
    #[param(default = AngleUnit::Deg)]
    pub unit: AngleUnit,
}

/// whether the shape is scaled — the same factor on every axis, or one per axis
#[derive(Debug, Params)]
#[params(tag = "scale")]
pub enum Scale {
    /// No scale.
    #[case(default, name = "none")]
    Kept,
    /// The same factor on every axis.
    Uniform(UniformScale),
    /// A factor per axis.
    Axes(AxesScale),
}

/// A scale, the same on every axis.
#[derive(Debug, Params)]
pub struct UniformScale {
    /// scale: the factor on every axis — 2 doubles every distance from the pivot
    pub factor: f64,
}

/// A scale, one factor per axis.
#[derive(Debug, Params)]
pub struct AxesScale {
    /// scale: the factor along x
    #[param(default = 1.0, optional)]
    pub scale_x: f64,
    /// scale: the factor along y
    #[param(default = 1.0, optional)]
    pub scale_y: f64,
    /// scale: the factor along z — of the heights, in their own unit
    #[param(default = 1.0, optional)]
    pub scale_z: f64,
}

/// The unit of an angle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum AngleUnit {
    /// Degrees.
    Deg,
    /// Radians.
    Rad,
}

/// the point the rotation and the scale are about — the centre of what moves, the origin of the frame, or a point given
#[derive(Debug, Params)]
#[params(tag = "pivot")]
pub enum Pivot {
    /// The centre of the box of what moves.
    #[case(default)]
    Centre,
    /// The origin of the frame.
    Origin,
    /// A point given.
    Point(PivotPoint),
}

/// A pivot, given.
#[derive(Debug, Params)]
pub struct PivotPoint {
    /// pivot: x
    pub pivot_x: f64,
    /// pivot: y
    pub pivot_y: f64,
    /// pivot: z
    pub pivot_z: f64,
}

/// What `transform` takes.
#[derive(Debug, Inputs)]
pub struct TransformInputs {
    /// the shape to move — a cloud, polylines or a triangulated surface
    pub shape: Shape,
}

// ---------------------------------------------------------------- the computation

/// The kind of representation a shape is written as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Points,
    Polylines,
    Triangles,
}

/// What the patches hold, or why they cannot be written.
///
/// Shared with `project`, which writes what it moved as `transform` does: the
/// same kinds, the same refusals.
pub(crate) fn kind_of(geometry: &Geometry) -> Result<Kind, String> {
    let mut found: Option<Kind> = None;
    for patch in &geometry.patches {
        let kind = match &patch.topology {
            Topology::Points => Kind::Points,
            Topology::Polylines(_) => Kind::Polylines,
            Topology::Triangles(_) => Kind::Triangles,
            other => {
                return Err(format!(
                    "patch {} is {} — writing it back would make it another kind of representation, and that is a decision, not a default",
                    patch.index,
                    topology_word(other)
                ));
            }
        };
        match found {
            Some(was) if was != kind => {
                return Err(
                    "its patches mix kinds (faces and lines, or points) — one representation holds one kind"
                        .to_string(),
                );
            }
            _ => found = Some(kind),
        }
    }
    found.ok_or_else(|| "it has no patch".to_string())
}

fn topology_word(topology: &Topology) -> &'static str {
    match topology {
        Topology::Points => "a cloud",
        Topology::Polylines(_) => "polylines",
        Topology::Triangles(_) => "triangles",
        Topology::Quads(_) => "a grid's cells",
        Topology::Polygons(_) => "polygons",
        Topology::Segments(_) => "segments",
        Topology::Grid { .. } => "a grid",
        _ => "a volume",
    }
}

/// The rotation of a vector about a unit axis — Rodrigues' formula.
fn rotated(v: [f64; 3], k: [f64; 3], cos: f64, sin: f64) -> [f64; 3] {
    let dot = k[0] * v[0] + k[1] * v[1] + k[2] * v[2];
    let cross = [
        k[1] * v[2] - k[2] * v[1],
        k[2] * v[0] - k[0] * v[2],
        k[0] * v[1] - k[1] * v[0],
    ];
    [
        v[0] * cos + cross[0] * sin + k[0] * dot * (1.0 - cos),
        v[1] * cos + cross[1] * sin + k[1] * dot * (1.0 - cos),
        v[2] * cos + cross[2] * sin + k[2] * dot * (1.0 - cos),
    ]
}

/// The centre of the box of what moves — the chosen nodes, or every node.
fn centre_of(geometry: &Geometry, chosen: Option<&[bool]>) -> [f64; 3] {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    let every = geometry.patches.iter().flat_map(|patch| patch.points.iter());
    for (rank, p) in every.enumerate() {
        if chosen.is_some_and(|flags| !flags[rank]) {
            continue;
        }
        for axis in 0..3 {
            low[axis] = low[axis].min(p[axis]);
            high[axis] = high[axis].max(p[axis]);
        }
    }
    [
        (low[0] + high[0]) / 2.0,
        (low[1] + high[1]) / 2.0,
        (low[2] + high[2]) / 2.0,
    ]
}

/// What happens about the pivot, resolved: the factors, then the turn.
struct Around {
    pivot: [f64; 3],
    /// Per axis; `[1, 1, 1]` when nothing scales.
    factors: [f64; 3],
    /// The unit axis, the cosine and the sine — `None` when nothing turns.
    turn: Option<([f64; 3], f64, f64)>,
}

/// Move the nodes — every one, or those `chosen` — and say how many moved.
fn moved(
    geometry: &Geometry,
    chosen: Option<&[bool]>,
    around: Option<&Around>,
    by: [f64; 3],
) -> (Geometry, usize) {
    let mut count = 0usize;
    let mut rank = 0usize;
    let patches = geometry
        .patches
        .iter()
        .map(|patch| {
            let points = patch
                .points
                .iter()
                .map(|p| {
                    let moves = chosen.is_none_or(|flags| flags[rank]);
                    rank += 1;
                    if !moves {
                        return *p;
                    }
                    count += 1;
                    let q = match around {
                        Some(a) => {
                            // Scaled, then turned — about the pivot.
                            let c = a.pivot;
                            let s = [
                                (p[0] - c[0]) * a.factors[0],
                                (p[1] - c[1]) * a.factors[1],
                                (p[2] - c[2]) * a.factors[2],
                            ];
                            let r = match a.turn {
                                Some((axis, cos, sin)) => rotated(s, axis, cos, sin),
                                None => s,
                            };
                            [r[0] + c[0], r[1] + c[1], r[2] + c[2]]
                        }
                        None => *p,
                    };
                    [q[0] + by[0], q[1] + by[1], q[2] + by[2]]
                })
                .collect();
            Patch {
                index: patch.index,
                node_offset: patch.node_offset,
                points,
                topology: patch.topology.clone(),
                closed: patch.closed.clone(),
            }
        })
        .collect();
    (
        Geometry {
            kind: "transform",
            patches,
        },
        count,
    )
}

/// **Written by the writer of its kind** — the same object the shape came as,
/// never a second copy of a writer. `transform` and `project` both end here.
pub(crate) fn write_as(
    kind: Kind,
    result: Shape,
    written: Written,
    seed: &str,
    cx: &Context<'_>,
) -> Result<Plan, Refusal> {
    match kind {
        Kind::Points => Pointset.plan(
            PointsetInputs {
                shape: vec![result],
                features: None,
            },
            PointsetParams { written },
            cx,
        ),
        Kind::Polylines => super::polylines::write(
            &PolylinesInputs {
                shape: vec![result],
                features: None,
            },
            &written,
            None,
            seed,
            cx,
        ),
        Kind::Triangles => {
            super::triangles::write(&TrianglesInputs { shape: vec![result] }, &written, seed, cx)
        }
    }
}

/// The triangles a move turned over in plan, every patch — counted, never
/// refused: a folded surface is a legitimate result.
/// Shared with `project`.
pub(crate) fn flipped_in(before: &Geometry, after: &Geometry) -> usize {
    before
        .patches
        .iter()
        .zip(&after.patches)
        .map(|(b, a)| match &b.topology {
            Topology::Triangles(triangles) => flipped(&b.points, &a.points, triangles),
            _ => 0,
        })
        .sum()
}

fn double(title: &str, value: f64) -> Parameter {
    Parameter::Double {
        title: title.to_string(),
        value,
        uom: None,
    }
}

impl Operation for Transform {
    type Params = TransformParams;
    type Inputs = TransformInputs;

    fn about(&self) -> About {
        About {
            name: "transform",
            describe: "a shape moved, turned and scaled — a translation, a rotation about an axis, a scale, both about one pivot — whole, or only some of its nodes",
            category: Category::Geometry,
            makes: &[
                "resqml22.PointSetRepresentation",
                "resqml22.PolylineSetRepresentation",
                "resqml22.TriangulatedSetRepresentation",
            ],
            recipe: "transform",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
    }

    fn check(&self, inputs: &TransformInputs) -> Result<(), Refusal> {
        kind_of(&inputs.shape.geometry)
            .map(|_| ())
            .map_err(|why| Refusal::on("shape", why))?;
        if inputs.shape.geometry.point_count() == 0 {
            return Err(Refusal::on("shape", "it has no node to move"));
        }
        Ok(())
    }

    fn plan(
        &self,
        inputs: TransformInputs,
        params: TransformParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let shape = inputs.shape;
        let kind = kind_of(&shape.geometry).map_err(|why| Refusal::on("shape", why))?;
        let total = shape.geometry.point_count();

        // The nodes that move — checked, all of them, before anything moves.
        let chosen: Option<Vec<bool>> = if params.nodes.is_empty() {
            None
        } else {
            if let Some(bad) = params.nodes.iter().find(|&&rank| rank >= total) {
                return Err(Refusal::about(
                    "value",
                    "nodes",
                    format!(
                        "`nodes` names node {bad}, and the shape holds {total} (ranks from 0) — a correction aimed at a node that does not exist would move nothing"
                    ),
                ));
            }
            let mut flags = vec![false; total];
            for &rank in &params.nodes {
                flags[rank] = true;
            }
            Some(flags)
        };
        let mut says: Vec<Note> = Vec::new();
        let mut parameters: Vec<Parameter> = Vec::new();
        let factors = match &params.scale {
            Scale::Kept => None,
            Scale::Uniform(uniform) => Some([("factor", uniform.factor); 3]),
            Scale::Axes(axes) => Some([
                ("scaleX", axes.scale_x),
                ("scaleY", axes.scale_y),
                ("scaleZ", axes.scale_z),
            ]),
        };
        if let Some(factors) = &factors
            && let Some((name, value)) = factors
                .iter()
                .find(|(_, value)| !(value.is_finite() && *value > 0.0))
        {
            let why = if *value < 0.0 {
                "is negative: that is a mirror, which turns every triangle inside out without saying so"
            } else if *value == 0.0 {
                "is zero: that flattens the shape onto a plane, which is a projection and not a scale"
            } else {
                "is not a number"
            };
            return Err(Refusal::about(
                "value",
                name,
                format!("the scale `{name}` = {value} {why}"),
            ));
        }
        let factors = factors.map(|f| [f[0].1, f[1].1, f[2].1]);
        let turn = match &params.rotation {
            Rotation::Still => None,
            Rotation::Axis(axis) => {
                let length = (axis.axis_x.powi(2) + axis.axis_y.powi(2) + axis.axis_z.powi(2)).sqrt();
                if !(length.is_finite() && length > 0.0) {
                    return Err(Refusal::about(
                        "value",
                        "axisZ",
                        "the axis (`axisX`, `axisY`, `axisZ`) has no length — a rotation needs a direction",
                    ));
                }
                let k = [axis.axis_x / length, axis.axis_y / length, axis.axis_z / length];
                let vertical = k[0].abs() < 1e-12 && k[1].abs() < 1e-12;
                let in_time = shape
                    .known
                    .as_ref()
                    .and_then(|known| known.vertical_uom.as_deref())
                    .and_then(Dimension::of)
                    == Some(Dimension::Time);
                if in_time && !vertical {
                    return Err(Refusal::about(
                        "value",
                        "axisZ",
                        "the heights are a time and the axis is not vertical: turning would mix metres and milliseconds, which is not a rotation — turn about the vertical (0, 0, 1), or convert to depth first",
                    ));
                }
                let radians = match axis.unit {
                    AngleUnit::Deg => axis.angle.to_radians(),
                    AngleUnit::Rad => axis.angle,
                };
                says.push(Note {
                    key: "rotation",
                    detail: format!(
                        "turned by {} {} about the axis ({:.6}, {:.6}, {:.6})",
                        axis.angle,
                        axis.unit.word(),
                        k[0],
                        k[1],
                        k[2]
                    ),
                });
                Some((k, radians.cos(), radians.sin()))
            }
        };

        // The pivot, only when something turns or scales — the condition its
        // slot carries, so a translation has none to record.
        let around = if turn.is_some() || factors.is_some() {
            let pivot = match &params.pivot {
                Pivot::Centre => centre_of(&shape.geometry, chosen.as_deref()),
                Pivot::Origin => [0.0, 0.0, 0.0],
                Pivot::Point(point) => [point.pivot_x, point.pivot_y, point.pivot_z],
            };
            // The pivot is **reported**: the centre of a box is a number
            // nobody stated, and an activity that did not record it could not
            // be read back into the same move. Under names of their own —
            // `pivotX` is an input already, and one title cannot be both
            // (`crates/energyml-build/ARCHITECTURE.md`).
            for (title, value) in [
                ("pivotUsedX", pivot[0]),
                ("pivotUsedY", pivot[1]),
                ("pivotUsedZ", pivot[2]),
            ] {
                parameters.push(double(title, value));
            }
            if let Some(f) = factors {
                says.push(Note {
                    key: "scale",
                    detail: format!("scaled by ({}, {}, {})", f[0], f[1], f[2]),
                });
            }
            says.push(Note {
                key: "pivot",
                detail: format!(
                    "about ({:.3}, {:.3}, {:.3}), then moved by ({}, {}, {})",
                    pivot[0], pivot[1], pivot[2], params.dx, params.dy, params.dz
                ),
            });
            Some(Around {
                pivot,
                factors: factors.unwrap_or([1.0; 3]),
                turn,
            })
        } else {
            None
        };

        let (geometry, count) = moved(
            &shape.geometry,
            chosen.as_deref(),
            around.as_ref(),
            [params.dx, params.dy, params.dz],
        );
        parameters.push(double("movedNodes", count as f64));
        if kind == Kind::Triangles {
            let turned = flipped_in(&shape.geometry, &geometry);
            parameters.push(double("flippedTriangles", turned as f64));
            if turned > 0 {
                says.push(Note {
                    key: "flipped",
                    detail: format!("{turned} triangle(s) turned over in plan — a fold, kept as it is"),
                });
            }
        }
        if chosen.is_some() {
            says.push(Note {
                key: "nodes",
                detail: format!("{count} of {total} node(s) moved; the others stayed where they were"),
            });
        }
        let result = Shape {
            name: shape.name.clone(),
            geometry,
            closed: shape.closed.clone(),
            known: shape.known.clone(),
        };

        let title = params.written.naming.title();
        let seed = format!("transform:{}:{title}", shape.name);
        let mut plan = write_as(kind, result, params.written, &seed, cx)?;
        plan.activity.recipe = "transform";
        plan.activity.title = format!("transform → {title}");
        plan.activity.parameters.extend(parameters);
        plan.says.extend(says);
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
        support::frame_reports(vec![
            count("movedNodes", "nodes moved"),
            optional(
                "flippedTriangles",
                "triangles turned over in plan by the move — a surface only",
            ),
            optional(
                "pivotUsedX",
                "the pivot the rotation and the scale were about, x — as used",
            ),
            optional("pivotUsedY", "the pivot as used, y"),
            optional("pivotUsedZ", "the pivot as used, z"),
        ])
    }
}
