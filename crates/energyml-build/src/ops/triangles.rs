//! `triangles` — a `TriangulatedSetRepresentation` from a triangulated shape.
//!
//! One `TrianglePatch` per patch of the shape, each with its own points and
//! its indices **local to the patch** — the usage that keeps every patch
//! self-contained, and the one the house reader accepts alongside the other.
//! A shape whose topology is not triangles is refused by name: quads and
//! polygons would need a triangulation nobody asked for, and points and
//! polylines have no faces at all.

use serde_json::json;

use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Patch, Topology};

use crate::declare::{Inputs, Params, Shape, Suggestions};
use crate::operation::{About, ActivityDraft, Category, Context, Dataset, Need, NewObject, Numbers, Operation, Plan, Refusal, Slot};
use crate::pieces::Pieces;

use super::support;
use super::written::Written;

/// The operation.
pub struct Triangles;

/// What `triangles` is told: the shared block of every writer.
#[derive(Debug, Params)]
pub struct TrianglesParams {
    /// The frame, what is represented, the naming.
    #[param(flatten)]
    pub written: Written,
}

/// What `triangles` takes.
#[derive(Debug, Inputs)]
pub struct TrianglesInputs {
    /// triangulated shapes, copied as they are — each patch its own
    ///
    /// **Several**, each with its patches: what `plan` has done from the first
    /// day — one RESQML patch per input patch. Promoting the panels of a
    /// section is one shape per panel (12/09).
    #[input(topology = "triangles", least = 1)]
    pub shape: Vec<Shape>,
}

impl Operation for Triangles {
    type Params = TrianglesParams;
    type Inputs = TrianglesInputs;

    fn about(&self) -> About {
        About {
            name: "triangles",
            describe: "a triangulated surface from a shape whose topology is triangles — one patch per patch, indices local",
            category: Category::Create,
            makes: &["resqml22.TriangulatedSetRepresentation"],
            recipe: "triangles",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
    }

    /// A shape whose topology is not triangles is refused **naming what it
    /// is**: quads and polygons would need a triangulation nobody asked for —
    /// that is `triangulate` — and points and polylines have no faces at all.
    fn check(&self, inputs: &TrianglesInputs) -> Result<(), Refusal> {
        let mut triangulated = 0usize;
        let mut other: Vec<&'static str> = Vec::new();
        for shape in &inputs.shape {
            for patch in &shape.geometry.patches {
                match &patch.topology {
                    Topology::Triangles(list) if !list.is_empty() => triangulated += 1,
                    Topology::Triangles(_) => other.push("an empty triangle list"),
                    Topology::Points => other.push("points"),
                    Topology::Quads(_) => other.push("quadrilaterals"),
                    Topology::Polygons(_) => other.push("polygons"),
                    Topology::Polylines(_) => other.push("polylines"),
                    _ => other.push("cells"),
                }
            }
        }
        if triangulated > 0 {
            return Ok(());
        }
        other.sort_unstable();
        other.dedup();
        Err(Refusal::on(
            "shape",
            format!(
                "the shape's topology is {} — a triangulated surface takes triangles, and triangulating is not decided here",
                if other.is_empty() { "empty".to_string() } else { other.join(", ") }
            ),
        ))
    }

    fn plan(&self, mut inputs: TrianglesInputs, params: TrianglesParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        // The shapes given are placed here, not in `write`: `write` is also
        // called with surfaces an operation computed (`triangulate`,
        // `cross-section`), already in the frame, which nothing may move again.
        let frame = support::frame_in(&cx.known(), &params.written.frame, cx.identity, cx.when, &mut 0)?;
        for shape in &mut inputs.shape {
            shape.geometry = support::geometry_placed(&shape.name, &shape.geometry, shape.known.as_ref(), &frame)?;
        }
        let seed = cx.seed(&params.written.naming.title());
        write(&inputs, &params.written, &seed, cx)
    }

    fn reports(&self) -> Vec<Slot> {
        let count = |name: &'static str, about: &'static str| {
            Slot::required(name, Need::Number { uom: None, least: None, most: None }, about)
        };
        support::frame_reports(vec![count("pointCount", "nodes written"), count("triangleCount", "triangles written")])
    }
}

/// **The triangulated set, written** — what the operation does, and what an
/// operation that decides triangles (`triangulate`, `cross-section`) calls
/// directly: typed in, no bag to derive and no choices read back by name.
/// `seed` is the caller's, so that two routes to the same set name it the same.
pub fn write(inputs: &TrianglesInputs, written: &Written, seed: &str, cx: &Context<'_>) -> Result<Plan, Refusal> {
    let title = written.naming.title();
    let seed = seed.to_string();
    let mut rank = 0usize;
    let frame = support::frame_in(&cx.known(), &written.frame, cx.identity, cx.when, &mut rank)?;
    let uuid = support::next_uuid(cx.identity, &format!("triangles:{seed}"), &mut rank)?;
    let file = written.naming.data_file.clone();

    let mut patches = Vec::new();
    // The computed shape, one RESQML patch per input patch.
    // `node_offset` is the rank of the first node in the whole shape: the
    // indices of a patch stay **local** to the patch, which the refusal
    // further down checks, and it is this offset that says where it starts.
    let mut made: Vec<Patch> = Vec::new();
    let mut datasets = Vec::new();
    let mut points_total = 0usize;
    let mut triangles_total = 0usize;
    for shape in &inputs.shape {
        for patch in &shape.geometry.patches {
            let Topology::Triangles(triangles) = &patch.topology else { continue };
            if triangles.is_empty() {
                continue;
            }
            let n = patch.points.len();
            if let Some(bad) = triangles.iter().flatten().find(|&&i| i as usize >= n) {
                return Err(Refusal::on(
                    "shape",
                    format!(
                        "`{}` patch {}: a triangle names node {bad} and the patch holds {n} — indices are local to the patch",
                        shape.name, patch.index
                    ),
                ));
            }
            let at = patches.len();
            let points_path = format!("/resqml22/{uuid}/patch{at}/points");
            let triangles_path = format!("/resqml22/{uuid}/patch{at}/triangles");
            patches.push(json!({
                "$type": "resqml22.TrianglePatch",
                "NodeCount": n,
                "Geometry": support::point_geometry(&frame.crs, &file, &points_path, n),
                "Triangles": {
                    "$type": "eml23.IntegerExternalArray",
                    "ArrayIntegerType": "arrayOfInt64LE",
                    "NullValue": -1,
                    "CountPerValue": 1,
                    "Values": support::external(&file, &triangles_path, &[triangles.len() as i64, 3]),
                },
            }));
            made.push(Patch {
                index: at,
                node_offset: points_total,
                points: patch.points.clone(),
                topology: Topology::Triangles(triangles.clone()),
                closed: Vec::new(),
            });
            datasets.push(Dataset {
                file: file.clone(),
                path: points_path,
                dimensions: vec![n as i64, 3],
                values: Numbers::F64(patch.points.iter().flatten().copied().collect()),
            });
            datasets.push(Dataset {
                file: file.clone(),
                path: triangles_path,
                dimensions: vec![triangles.len() as i64, 3],
                values: Numbers::I64(triangles.iter().flatten().map(|&i| i as i64).collect()),
            });
            points_total += n;
            triangles_total += triangles.len();
        }
    }
    if patches.is_empty() {
        return Err(Refusal::new("refused", "the input has no triangle"));
    }

    let mut object = json!({
        "$type": "resqml22.TriangulatedSetRepresentation",
        "uuid": uuid,
        "schemaVersion": "2.2",
        "Citation": support::citation(cx.when, &title),
        "TrianglePatch": patches,
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

    let mut objects = frame.objects;
    objects.extend(represented.objects);
    objects.push(NewObject {
        uuid: uuid.clone(),
        qualified_type: "resqml22.TriangulatedSetRepresentation".to_string(),
        role: "representation".to_string(),
        title: title.clone(),
        json: object,
    });
    let mut parameters = frame.parameters;
    parameters.push(Parameter::Double { title: "pointCount".to_string(), value: points_total as f64, uom: None });
    parameters.push(Parameter::Double { title: "triangleCount".to_string(), value: triangles_total as f64, uom: None });
    Ok(Plan {
        media: Vec::new(),
        // What the frame signalled travels up to here: the geometry is
        // computed, the writing is what is missing. `apply` refuses.
        blocked: frame.blocked.clone(),
        objects,
        datasets,
        shapes: vec![support::made_root(
            &title,
            &uuid,
            energyml_geometry::geometry::Geometry { kind: "triangles", patches: made },
        )],
        activity: ActivityDraft { recipe: "triangles", title: format!("triangles → {title}"), seed, parameters },
        // What the frame must say of itself — degrees, a `ProjectedCrs` taken
        // as it stands — and nothing when it is a compound.
        says: frame.says,
        dropped: Vec::new(),
    })
}
