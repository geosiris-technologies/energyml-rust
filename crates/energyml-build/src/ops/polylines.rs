//! `polylines` — a `PolylineSetRepresentation` from runs of points.
//!
//! From a shape whose topology is polylines — contour lines of the scene, a
//! trajectory read from the package — or from a feature collection of lines
//! and areas. That is what the isovalues need: closed or
//! open polylineSetRepresentations, as the case may be, one for each small
//! line.
//!
//! # Closed, said or seen
//!
//! RESQML states closure in its own array, so it has to come from somewhere:
//!
//! - **said** — the shape carries `closed`, one flag per polyline, because the
//!   producer knew (the contouring closes its rings); an area of a feature
//!   collection is a ring by definition; the `closed` choice forces every line;
//! - **seen** — otherwise the first and last point coinciding is read as a
//!   closed ring, the repeat is dropped (RESQML does not repeat the first
//!   node), and the plan **says so** in a note. Never silently: a ring seen
//!   where the producer meant an open line back to its start is a decision
//!   somebody must be able to read.

use serde_json::json;

use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::Topology;

use crate::declare::{Features, Inputs, Params, Shape as ShapeIn, Suggestions};
use crate::geojson::Shape;
use crate::operation::{About, ActivityDraft, Category, Context, Dataset, Need, NewObject, Note, Numbers, Operation, Plan, Refusal, Slot};
use crate::pieces::Pieces;
use energyml_geometry::geometry::Patch;

use super::support;
use super::written::Written;

/// The operation.
pub struct Polylines;

/// What `polylines` is told: the shared block, and how to close.
#[derive(Debug, Params)]
pub struct PolylinesParams {
    /// The frame, what is represented, the naming.
    #[param(flatten)]
    pub written: Written,
    /// force every polyline closed (true) or open (false); left out, what the input says, or what its ends show
    pub closed: Option<bool>,
}

/// What `polylines` takes.
#[derive(Debug, Inputs)]
pub struct PolylinesInputs {
    /// shapes holding polylines — each one becomes its own patch
    ///
    /// **Several**, each its patch: a `PolylineSetRepresentation` has
    /// `LinePatch 1..*`, and promoting the traces of a fault on five horizons
    /// is one shape per horizon (12/09).
    #[input(topology = "polylines")]
    pub shape: Vec<ShapeIn>,
    /// a feature collection of lines or areas
    pub features: Option<Features>,
}

/// One polyline, ready to write.
struct Line {
    points: Vec<[f64; 3]>,
    closed: bool,
    /// Closed because the ends coincided, not because anybody said so.
    seen: bool,
}

/// The polylines of the input — with what was said and what was seen —
/// **one group per input patch** — or per feature
/// collection.
///
/// # Why groups
///
/// A `PolylineSetRepresentation` has `LinePatch 1..*`, and this writer used to
/// flatten every line into a single one. `intersect` then wrote a fault's
/// traces on five horizons as **one** patch while its plan, its tree and its
/// documentation said *one patch per horizon*. A patch is
/// what a reader addresses — *the trace on this horizon* — so it survives the
/// writing, as it already did in [`super::triangles`].
/// **The closure a shape says of one of its polylines** — patch-local first,
/// then the piece-wide list: a shape built by a caller states it once for all
/// its lines, a shape read from a file states it per patch. `None`: nobody said,
/// which is not *open* — a reader that decided would invent or lose a ring.
///
/// `local` is the polyline's rank in its patch, `global` its rank in the shape.
/// Public to the operations that read polylines before writing them
/// (`densify`), so that there is one reading of it.
pub(crate) fn said_closed(patch: &Patch, local: usize, shape: Option<&[bool]>, global: usize) -> Option<bool> {
    patch.closed.get(local).copied().or_else(|| shape.and_then(|c| c.get(global).copied()))
}

fn lines_of(inputs: &PolylinesInputs, frame: &support::Frame, force_closed: Option<bool>) -> Result<Vec<Vec<Line>>, Refusal> {
    let mut out = Vec::new();
    for shape in &inputs.shape {
        let (name, geometry, closed) = (&shape.name, &shape.geometry, shape.closed.as_deref());
        let mut at = 0usize;
        for patch in &geometry.patches {
            let Topology::Polylines(runs) = &patch.topology else { continue };
            let mut group = Vec::new();
            let mut start = 0usize;
            for &run in runs {
                let run = run as usize;
                let points = patch.points.get(start..start + run).ok_or_else(|| {
                    Refusal::on(
                        "shape",
                        format!(
                            "`{name}`: the polylines add up to more nodes than the patch holds — a count that does not add up would shift every following polyline"
                        ),
                    )
                })?;
                let said = said_closed(patch, group.len(), closed, at);
                group.push(line(points.to_vec(), said, force_closed));
                start += run;
                at += 1;
            }
            if !group.is_empty() {
                out.push(group);
            }
        }
    }
    if let Some(collection) = &inputs.features {
        let mut group = Vec::new();
        for feature in &collection.features {
            let said = match feature.shape {
                Shape::Lines => Some(false),
                Shape::Areas => Some(true),
                Shape::Points => continue,
            };
            for part in &feature.parts {
                let points = support::placed(part, frame)?;
                group.push(line(points, said, force_closed));
            }
        }
        if !group.is_empty() {
            out.push(group);
        }
    }
    Ok(out)
}

fn line(mut points: Vec<[f64; 3]>, said: Option<bool>, forced: Option<bool>) -> Line {
    // Bit for bit and not `==`: a position without a third ordinate carries a
    // NaN, and `NaN != NaN` would read every flat ring as open — the same
    // reason `assemble` compares its places on the bits.
    let same = |a: &[f64; 3], b: &[f64; 3]| a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits());
    let coincide = points.len() >= 2 && same(&points[0], &points[points.len() - 1]);
    let (closed, seen) = match forced.or(said) {
        Some(closed) => (closed, false),
        None => (coincide, coincide),
    };
    // A closed ring does not repeat its first node in RESQML; a repeat that
    // came with the input is dropped, whether the closure was said or seen.
    if closed && coincide {
        points.pop();
    }
    Line { points, closed, seen }
}

impl Operation for Polylines {
    type Params = PolylinesParams;
    type Inputs = PolylinesInputs;

    fn about(&self) -> About {
        About {
            name: "polylines",
            describe: "a polyline set from runs of points — a shape with polylines, or the lines and areas of a feature collection; closed where said, or where the ends coincide",
            category: Category::Create,
            makes: &["resqml22.PolylineSetRepresentation"],
            recipe: "polylines",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
    }

    fn check(&self, inputs: &PolylinesInputs) -> Result<(), Refusal> {
        let runs: usize = inputs
            .shape
            .iter()
            .flat_map(|one| one.geometry.patches.iter())
            .map(|p| match &p.topology {
                Topology::Polylines(runs) => runs.len(),
                _ => 0,
            })
            .sum();
        let parts: usize = inputs
            .features
            .iter()
            .flat_map(|collection| collection.features.iter())
            .filter(|f| !matches!(f.shape, Shape::Points))
            .map(|f| f.parts.len())
            .sum();
        if runs + parts > 0 {
            Ok(())
        } else if !inputs.shape.is_empty() {
            Err(Refusal::on("shape", "the shape has no polyline — its topology is points, triangles or cells"))
        } else if inputs.features.is_some() {
            Err(Refusal::on("features", "the feature collection holds only points — a polyline needs lines or areas"))
        } else {
            Err(Refusal::new("refused", "it takes a shape or a feature collection; the input holds neither"))
        }
    }

    fn plan(&self, mut inputs: PolylinesInputs, params: PolylinesParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        // The shapes given are placed here, not in `write`: `write` is also
        // called with lines an operation computed (`outline`, `isocontours`,
        // `intersect`), already in the frame, which nothing may move again.
        let frame = support::frame_in(&cx.known(), &params.written.frame, cx.identity, cx.when, &mut 0)?;
        for shape in &mut inputs.shape {
            shape.geometry = support::geometry_placed(&shape.name, &shape.geometry, shape.known.as_ref(), &frame)?;
        }
        let seed = cx.seed(&params.written.naming.title());
        write(&inputs, &params.written, params.closed, &seed, cx)
    }

    fn reports(&self) -> Vec<Slot> {
        let count = |name: &'static str, about: &'static str| {
            Slot::required(name, Need::Number { uom: None, least: None, most: None }, about)
        };
        support::frame_reports(vec![
            count("polylineCount", "polylines written"),
            count("pointCount", "nodes written"),
            count("closedCount", "of which closed"),
        ])
    }
}

/// **The polyline set, written** — what the operation does, and what an
/// operation that computes lines (`outline`, `isocontours`, `intersect`)
/// calls directly: typed in, no bag to derive and no choices to read back by
/// name. `seed` is the caller's, so that two routes to the same set name it
/// the same; `closed` forces every line, as the operation's own setting does.
pub fn write(
    inputs: &PolylinesInputs,
    written: &Written,
    closed: Option<bool>,
    seed: &str,
    cx: &Context<'_>,
) -> Result<Plan, Refusal> {
    let title = written.naming.title();
    let seed = seed.to_string();
    let mut rank = 0usize;
    let frame = support::frame_in(&cx.known(), &written.frame, cx.identity, cx.when, &mut rank)?;
    let groups = lines_of(inputs, &frame, closed)?;
    if groups.is_empty() {
        return Err(Refusal::new("refused", "the input has no polyline"));
    }
    for (at, line) in groups.iter().flatten().enumerate() {
        if line.points.len() < 2 {
            return Err(Refusal::new("refused", format!("polyline {at} has {} node(s); a polyline needs at least 2", line.points.len())));
        }
        if line.closed && line.points.len() < 3 {
            return Err(Refusal::new(
                "refused",
                format!("polyline {at} is closed with {} nodes — two would fold the ring onto its own edge", line.points.len()),
            ));
        }
    }

    let uuid = support::next_uuid(cx.identity, &format!("polylines:{seed}"), &mut rank)?;
    let file = written.naming.data_file.clone();
    let mut line_patches = Vec::new();
    let mut made: Vec<Patch> = Vec::new();
    let mut datasets = Vec::new();
    let mut node_offset = 0usize;
    for (at, group) in groups.iter().enumerate() {
        let counts: Vec<i64> = group.iter().map(|l| l.points.len() as i64).collect();
        let nodes: i64 = counts.iter().sum();
        let intervals: i64 = group.iter().map(|l| if l.closed { l.points.len() } else { l.points.len() - 1 } as i64).sum();
        let flags: Vec<bool> = group.iter().map(|l| l.closed).collect();
        let closed_polylines = if flags.windows(2).any(|w| w[0] != w[1]) {
            json!({ "$type": "eml23.BooleanXmlArray", "CountPerValue": 1, "Values": flags })
        } else {
            json!({ "$type": "eml23.BooleanConstantArray", "Value": flags[0], "Count": flags.len() })
        };
        // One patch keeps the path it always had, so a single-patch set —
        // the ordinary case — writes exactly what it wrote before.
        let path = if groups.len() == 1 {
            format!("/resqml22/{uuid}/points")
        } else {
            format!("/resqml22/{uuid}/patch{at}/points")
        };
        line_patches.push(json!({
            "$type": "resqml22.PolylineSetPatch",
            "NodeCount": nodes,
            "IntervalCount": intervals,
            "NodeCountPerPolyline": {
                "$type": "eml23.IntegerXmlArray",
                "CountPerValue": 1,
                "NullValue": -1,
                "Values": counts,
            },
            "ClosedPolylines": closed_polylines,
            "Geometry": support::point_geometry(&frame.crs, &file, &path, nodes as usize),
        }));
        let points: Vec<[f64; 3]> = group.iter().flat_map(|l| l.points.iter().copied()).collect();
        datasets.push(Dataset {
            file: file.clone(),
            path,
            dimensions: vec![nodes, 3],
            values: Numbers::F64(points.iter().flatten().copied().collect()),
        });
        // The shape is the output, one patch per patch written, and
        // `node_offset` is **the running sum** — the contract of `Patch`.
        // It carries the closure, which the points array does not say.
        made.push(Patch {
            index: at,
            node_offset,
            points,
            topology: Topology::Polylines(counts.iter().map(|c| *c as u32).collect()),
            closed: flags,
        });
        node_offset += nodes as usize;
    }
    let mut object = json!({
        "$type": "resqml22.PolylineSetRepresentation",
        "uuid": uuid,
        "schemaVersion": "2.2",
        "Citation": support::citation(cx.when, &title),
        "LinePatch": line_patches,
    });
    let represented = support::represented_in(
        &written.represented,
        Some(&written.new_feature),
        None,
        cx.identity,
        cx.when,
        &seed,
        &title,
        &mut rank,
    )?;
    if let Some(reference) = represented.reference {
        object["RepresentedObject"] = reference;
    }

    let mut says = Vec::new();
    let lines: Vec<&Line> = groups.iter().flatten().collect();
    let seen = lines.iter().filter(|l| l.seen).count();
    if seen > 0 {
        says.push(Note {
            key: "closed-seen",
            detail: format!("{seen} polyline(s) closed because their ends coincide — nobody said so; `--set closed=false` keeps them open"),
        });
    }
    let mut objects = frame.objects;
    objects.extend(represented.objects);
    objects.push(NewObject {
        uuid: uuid.clone(),
        qualified_type: "resqml22.PolylineSetRepresentation".to_string(),
        role: "representation".to_string(),
        title: title.clone(),
        json: object,
    });
    let shape = support::made_root(
        &title,
        &uuid,
        energyml_geometry::geometry::Geometry { kind: "polylines", patches: made },
    );
    let nodes: usize = lines.iter().map(|l| l.points.len()).sum();
    // What the frame must say of itself, after what the lines said.
    says.extend(frame.says);
    let mut parameters = frame.parameters;
    parameters.push(Parameter::Double { title: "polylineCount".to_string(), value: lines.len() as f64, uom: None });
    parameters.push(Parameter::Double { title: "pointCount".to_string(), value: nodes as f64, uom: None });
    parameters.push(Parameter::Double { title: "closedCount".to_string(), value: lines.iter().filter(|l| l.closed).count() as f64, uom: None });
    Ok(Plan {
        media: Vec::new(),
        // What the frame signalled travels up to here: the geometry is
        // computed, the writing is what is missing. `apply` refuses.
        blocked: frame.blocked.clone(),
        objects,
        datasets,
        shapes: vec![shape],
        activity: ActivityDraft { recipe: "polylines", title: format!("polylines → {title}"), seed, parameters },
        says,
        dropped: Vec::new(),
    })
}
