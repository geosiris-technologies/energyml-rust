//! `boundary` - a cultural feature and its outline: a model frontier, a
//! licence block, a pipeline, as a closed polyline the package can point at.
//!
//! What the old artefact promoter alone still knew how to make, and the last
//! thing it did that the one door did not; the new door could not replace the
//! old one while this was missing.
//!
//! Three objects, linked before any exists: a `CulturalFeature` of the chosen
//! kind, a `GenericFeatureInterpretation` of it, and the polyline set - closed
//! by default, because a boundary is a ring, and said so through `closed` so
//! that the writer neither detects nor notes anything. The writing is
//! [`super::polylines`]'s - [`super::polylines::write`], typed in since step 8
//! of the operations architecture: a second writer of the same
//! object would be a second place for the same mistakes.

use serde_json::json;

use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::Topology;

use crate::declare::{Choice, Inputs, Params, Shape, Suggestions, TypeName};
use crate::operation::{About, Category, Context, Operation, Plan, Refusal, Slot};
use crate::pieces::Pieces;

use super::polylines::PolylinesInputs;
use super::support;
use super::written::{FrameParams, NamingParams, NewFeatureParams, RepresentedParams, Written};

/// The operation.
pub struct Boundary;

/// What the feature is - RESQML's `CulturalFeatureKind`, a closed list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum Kind {
    /// A model frontier.
    #[choice(name = "model frontier")]
    ModelFrontier,
    /// Project boundaries.
    #[choice(name = "project boundaries")]
    ProjectBoundaries,
    /// A field block.
    Fieldblock,
    /// Licences.
    Licenses,
    /// A pipeline.
    Pipeline,
}

/// What `boundary` is told: the frame and the naming - **not** the
/// interpretation, which it makes; naming one would be naming two.
#[derive(Debug, Params)]
pub struct BoundaryParams {
    /// Where the coordinates are.
    #[param(flatten)]
    pub frame: FrameParams,
    /// The title and the file.
    #[param(flatten)]
    pub naming: NamingParams,
    /// what the feature is
    #[param(default = Kind::ModelFrontier, optional)]
    pub feature_kind: Kind,
    /// a boundary is a ring; false writes the lines open, as a pipeline is
    #[param(default = true, optional)]
    pub closed: bool,
}

/// What `boundary` takes.
#[derive(Debug, Inputs)]
pub struct BoundaryInputs {
    /// the lines that bound the model
    #[input(topology = "polylines")]
    pub shape: Shape,
}

impl Operation for Boundary {
    type Params = BoundaryParams;
    type Inputs = BoundaryInputs;

    fn about(&self) -> About {
        About {
            name: "boundary",
            describe: "a cultural feature - a model frontier, a licence block, a pipeline - with its interpretation and its outline as closed polylines",
            category: Category::Create,
            makes: &[
                "resqml22.CulturalFeature",
                "resqml22.GenericFeatureInterpretation",
                "resqml22.PolylineSetRepresentation",
            ],
            recipe: "boundary",
            // It writes the pair itself - declared so that the form can offer
            // *create one* beside *choose one*, before anything is planned.
            represents: Some((
                "resqml22.CulturalFeature",
                "resqml22.GenericFeatureInterpretation",
            )),
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, self.about().represents, slots);
    }

    fn check(&self, inputs: &BoundaryInputs) -> Result<(), Refusal> {
        let runs: usize = inputs
            .shape
            .geometry
            .patches
            .iter()
            .map(|p| match &p.topology {
                Topology::Polylines(runs) => runs.len(),
                _ => 0,
            })
            .sum();
        if runs == 0 {
            return Err(Refusal::on(
                "shape",
                "the shape has no polyline - its topology is points, triangles or cells",
            ));
        }
        Ok(())
    }

    fn plan(
        &self,
        inputs: BoundaryInputs,
        params: BoundaryParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let title = params.naming.title();
        let seed = cx.seed(&title);
        let kind = params.feature_kind.word();
        // The polylines, with no interpretation: this operation sets it.
        let written = Written {
            frame: params.frame.clone(),
            represented: RepresentedParams { interpretation: None },
            naming: params.naming.clone(),
            new_feature: NewFeatureParams {
                feature_type: None,
                interpretation_type: TypeName(String::from("resqml22.GenericFeatureInterpretation")),
                feature_kind: None,
            },
        };
        let lines = PolylinesInputs {
            shape: vec![inputs.shape],
            features: None,
        };
        let mut plan = super::polylines::write(&lines, &written, Some(params.closed), &seed, cx)?;

        // The feature and its interpretation, numbered after what the polylines
        // took, so that given uuids land in a predictable order.
        let mut rank = plan.objects.len();
        let feature_uuid = support::next_uuid(cx.identity, &format!("boundary-feature:{seed}"), &mut rank)?;
        let interpretation_uuid =
            support::next_uuid(cx.identity, &format!("boundary-interpretation:{seed}"), &mut rank)?;
        let feature_title = format!("{title} - feature");
        let interpretation_title = format!("{title} - interpretation");
        let feature = crate::operation::NewObject {
            uuid: feature_uuid.clone(),
            qualified_type: "resqml22.CulturalFeature".to_string(),
            role: "feature".to_string(),
            title: feature_title.clone(),
            json: json!({
                "$type": "resqml22.CulturalFeature",
                "uuid": feature_uuid,
                "schemaVersion": "2.2",
                "Citation": support::citation(cx.when, &feature_title),
                "IsWellKnown": false,
                "CulturalFeatureKind": kind,
            }),
        };
        let interpretation = crate::operation::NewObject {
            uuid: interpretation_uuid.clone(),
            qualified_type: "resqml22.GenericFeatureInterpretation".to_string(),
            role: "interpretation".to_string(),
            title: interpretation_title.clone(),
            json: json!({
                "$type": "resqml22.GenericFeatureInterpretation",
                "uuid": interpretation_uuid,
                "schemaVersion": "2.2",
                "Citation": support::citation(cx.when, &interpretation_title),
                "InterpretedFeature": {
                    "$type": "eml23.DataObjectReference",
                    "Uuid": feature_uuid,
                    "QualifiedType": "resqml22.CulturalFeature",
                    "Title": feature_title,
                },
            }),
        };
        let dor = json!({
            "$type": "eml23.DataObjectReference",
            "Uuid": interpretation_uuid,
            "QualifiedType": "resqml22.GenericFeatureInterpretation",
            "Title": interpretation_title,
        });
        // The representation points at the interpretation; it is put after it.
        let at = plan
            .objects
            .iter()
            .position(|o| o.role == "representation")
            .ok_or_else(|| Refusal::new("refused", "no representation was planned"))?;
        plan.objects[at].json["RepresentedObject"] = dor;
        plan.objects.insert(at, interpretation);
        plan.objects.insert(at, feature);

        plan.activity.recipe = "boundary";
        plan.activity.title = format!("boundary → {title}");
        plan.activity.parameters.push(Parameter::Text {
            title: "featureKind".to_string(),
            value: kind.to_string(),
        });
        Ok(plan)
    }

    fn reports(&self) -> Vec<Slot> {
        crate::operation::AnyOperation::reports(&crate::operation::Typed(super::polylines::Polylines))
    }
}
