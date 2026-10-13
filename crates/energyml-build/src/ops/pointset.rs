//! `pointset` — a `PointSetRepresentation` from anything that has points.
//!
//! The most general operation: a mesh, a polyline, a triangulated surface, a
//! feature collection — whatever the topology, the points are what a point set
//! takes, and extracting them is what `Shape` already knows how to do. This
//! is the second example of the original need: from a 3D model of
//! polylines or of a surface, extract a point cloud.

use serde_json::json;

use energyml_domain::provenance::Parameter;

use crate::declare::{Features, Inputs, Params, Shape, Suggestions};
use crate::operation::{
    About, ActivityDraft, Category, Context, Dataset, Need, NewObject, Numbers, Operation, Plan, Refusal,
    Slot,
};
use crate::pieces::Pieces;
use energyml_geometry::geometry::Topology;

use super::support;
use super::written::Written;

/// The operation.
pub struct Pointset;

/// What `pointset` is told: the shared block of every writer, and nothing else.
#[derive(Debug, Params)]
pub struct PointsetParams {
    /// The frame, what is represented, the naming.
    #[param(flatten)]
    pub written: Written,
}

/// What `pointset` takes.
///
/// A shape **or** a feature collection: two optional slots rather than one
/// required, because it is *one or the other* — a slot naming only one would
/// hide half of what the operation does.
#[derive(Debug, Inputs)]
pub struct PointsetInputs {
    /// shapes whose nodes become the point set
    ///
    /// Several, concatenated in their order: what `plan` always did, which the
    /// declaration (`0..1`) did not say until the typing made it read what it
    /// declares.
    pub shape: Vec<Shape>,
    /// a feature collection to place
    pub features: Option<Features>,
}

impl Operation for Pointset {
    type Params = PointsetParams;
    type Inputs = PointsetInputs;

    fn about(&self) -> About {
        About {
            name: "pointset",
            describe: "a point set from whatever has points — a shape of any topology, or the positions of a feature collection",
            category: Category::Create,
            makes: &["resqml22.PointSetRepresentation"],
            recipe: "pointset",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
    }

    fn check(&self, inputs: &PointsetInputs) -> Result<(), Refusal> {
        if inputs.shape.is_empty() && inputs.features.is_none() {
            return Err(Refusal::new(
                "refused",
                "it takes a shape or a feature collection; the input holds neither",
            ));
        }
        let shape_points: usize = inputs.shape.iter().map(|one| one.geometry.point_count()).sum();
        let feature_points = inputs
            .features
            .as_ref()
            .map(|f| support::positions_of(&f.features).len())
            .unwrap_or(0);
        if shape_points + feature_points == 0 {
            return Err(Refusal::new("refused", "the input has no point at all"));
        }
        Ok(())
    }

    fn plan(
        &self,
        inputs: PointsetInputs,
        params: PointsetParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        // Every point of every shape, then every position of the features —
        // each placed in the frame written, as `write` builds it again below.
        let frame = support::frame_in(&cx.known(), &params.written.frame, cx.identity, cx.when, &mut 0)?;
        let mut points: Vec<[f64; 3]> = Vec::new();
        for one in &inputs.shape {
            let nodes: Vec<[f64; 3]> = one
                .geometry
                .patches
                .iter()
                .flat_map(|p| p.points.iter().copied())
                .collect();
            points.extend(support::shape_placed(
                &one.name,
                &nodes,
                one.known.as_ref(),
                &frame,
            )?);
        }
        if let Some(features) = &inputs.features {
            points.extend(support::placed(
                &support::positions_of(&features.features),
                &frame,
            )?);
        }
        let title = params.written.naming.title();
        let seed = cx.seed(&title);
        write(&title, points, &params.written, &seed, cx)
    }

    fn reports(&self) -> Vec<Slot> {
        support::frame_reports(vec![Slot::required(
            "pointCount",
            Need::Number {
                uom: None,
                least: None,
                most: None,
            },
            "points written",
        )])
    }
}

/// **The point set, written** — what the operation does, and what an
/// operation that computes its own points (`estimate`) calls directly:
/// typed in, no bag to derive and no choices read back by name. `name` is
/// what the input is called in the one refusal this can raise; `seed` is
/// the caller's, so that two routes to the same point set name it the same.
pub fn write(
    name: &str,
    points: Vec<[f64; 3]>,
    written: &Written,
    seed: &str,
    cx: &Context<'_>,
) -> Result<Plan, Refusal> {
    if points.is_empty() {
        return Err(Refusal::on(
            "shape",
            format!("`{name}`: the input has no point at all"),
        ));
    }
    let title = written.naming.title();
    let seed = seed.to_string();
    let mut rank = 0usize;
    let frame = support::frame_in(&cx.known(), &written.frame, cx.identity, cx.when, &mut rank)?;

    let uuid = support::next_uuid(cx.identity, &format!("pointset:{seed}"), &mut rank)?;
    let file = written.naming.data_file.clone();
    let path = format!("/resqml22/{uuid}/points");
    let mut object = json!({
        "$type": "resqml22.PointSetRepresentation",
        "uuid": uuid,
        "schemaVersion": "2.2",
        "Citation": support::citation(cx.when, &title),
        "NodePatchGeometry": [support::point_geometry(&frame.crs, &file, &path, points.len())],
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
        qualified_type: "resqml22.PointSetRepresentation".to_string(),
        role: "representation".to_string(),
        title: title.clone(),
        json: object,
    });
    let count = points.len();
    // The shape is the output; the array is a view of it.
    let shape = support::made_root(
        &title,
        &uuid,
        support::one_patch("pointset", points, Topology::Points),
    );
    let values = support::flat_points(&shape.geometry);
    let mut parameters = frame.parameters;
    parameters.push(Parameter::Double {
        title: "pointCount".to_string(),
        value: count as f64,
        uom: None,
    });
    Ok(Plan {
        media: Vec::new(),
        blocked: frame.blocked.clone(),
        objects,
        datasets: vec![Dataset {
            file,
            path,
            dimensions: vec![count as i64, 3],
            values: Numbers::F64(values),
        }],
        shapes: vec![shape],
        activity: ActivityDraft {
            recipe: "pointset",
            title: format!("pointset → {title}"),
            seed,
            parameters,
        },
        says: frame.says,
        dropped: Vec::new(),
    })
}
