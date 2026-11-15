//! `isosurface` - an iso-surface computed in a property, promoted to a
//! `TriangulatedSetRepresentation` with the recipe it was computed by.
//!
//! The surface is an **artefact** first: `energyml_geostat::isosurface`
//! (marching tetrahedra) computes it and a scene draws it. This operation
//! promotes it **on demand**, the way `isocontours` writes its lines: the
//! computation is the geostat's, the writing is another operation's - here
//! that of [`super::triangles`].
//!
//! # Why the operation does not compute again
//!
//! A plan is pure and reads no array: the cube (millions of values) and the
//! lattice's positions (trajectory, CRS, axes, frame of the hole) are not in
//! the bag. The operation therefore takes **the mesh as it was computed** and
//! records the recipe **as the caller states it** - the level, the spacing,
//! the inputs. It says so (the note `promoted`), and ADR-0037 writes the
//! decision. The caller refused a folded lattice **before** computing; this
//! operation cannot check it and does not pretend to.

use energyml_domain::provenance::Parameter;

use crate::declare::{Choice, Inputs, ObjectIn, Params, Shape};
use crate::operation::{About, Category, Context, Need, Note, Operation, Plan, Refusal, Slot};

use super::triangles::{Triangles, TrianglesInputs, TrianglesParams};
use super::written::Written;

/// The operation.
pub struct Isosurface;

/// How the value was taken to vary along an edge -
/// `energyml_geostat::isosurface::ValueSpacing`, as a form says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum SpacingChoice {
    /// linearly in the value
    Linear,
    /// linearly in the base-10 logarithm of the value
    #[choice(name = "log10")]
    Log10,
}

/// What `isosurface` is told.
#[derive(Debug, Params)]
pub struct IsosurfaceParams {
    /// The frame, what is represented, the naming.
    #[param(flatten)]
    pub written: Written,
    /// the value the surface was traced at, in the unit of the property
    pub level: f64,
    /// the unit of `level` - an Energistics uom code, the property's
    pub level_uom: Option<String>,
    /// how the value was taken to vary along an edge of the lattice
    pub spacing: SpacingChoice,
    /// how many cells of the lattice were set aside (a corner with no value, or <= 0 under log10), as the computation counted them
    #[param(default = 0usize, optional)]
    pub set_aside: usize,
}

/// What `isosurface` takes.
#[derive(Debug, Inputs)]
pub struct IsosurfaceInputs {
    /// the iso-surface as it was computed - a shape of triangles
    #[input(topology = "triangles")]
    pub shape: Shape,
    /// the property the surface was traced in
    #[input(types = "resqml22.ContinuousProperty")]
    pub cube: ObjectIn,
    /// the representation the property lies on - a wellbore frame, a grid
    pub support: ObjectIn,
    /// the table of the property's component axes, when the lattice was placed by one
    #[input(types = "eml23.ColumnBasedTable")]
    pub axes: Option<ObjectIn>,
}

impl Operation for Isosurface {
    type Params = IsosurfaceParams;
    type Inputs = IsosurfaceInputs;

    fn about(&self) -> About {
        About {
            name: "isosurface",
            describe: "an iso-surface computed in a property, kept as a triangulated surface with the recipe it was computed by",
            category: Category::Geometry,
            makes: &["resqml22.TriangulatedSetRepresentation"],
            recipe: "isosurface",
            represents: None,
        }
    }

    fn suggest(&self, bag: &crate::pieces::Pieces, slots: &mut crate::declare::Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
    }

    fn check(&self, inputs: &IsosurfaceInputs) -> Result<(), Refusal> {
        // The refusal of `triangles`, not a copy of it.
        Triangles.check(&TrianglesInputs {
            shape: vec![inputs.shape.clone()],
        })
    }

    fn plan(
        &self,
        inputs: IsosurfaceInputs,
        params: IsosurfaceParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        if !params.level.is_finite() {
            return Err(Refusal::on("level", "is not a finite number"));
        }
        if params.spacing == SpacingChoice::Log10 && params.level <= 0.0 {
            return Err(Refusal::on(
                "level",
                "a log10 spacing traces a level > 0 - this one has no logarithm",
            ));
        }
        let title = params.written.naming.title();
        // The writing is the writer of `triangles`, the one door.
        let mut plan = Triangles.plan(
            TrianglesInputs {
                shape: vec![inputs.shape.clone()],
            },
            TrianglesParams {
                written: params.written.clone(),
            },
            cx,
        )?;
        plan.activity.recipe = "isosurface";
        plan.activity.title = format!("isosurface → {title}");
        let mut named: Vec<&ObjectIn> = vec![&inputs.cube, &inputs.support];
        named.extend(inputs.axes.iter());
        for one in named {
            if let Some(reference) = &one.known.reference {
                plan.activity.parameters.push(Parameter::Object {
                    title: "input".to_string(),
                    reference: reference.dor(),
                });
            }
        }
        plan.activity.parameters.push(Parameter::Double {
            title: "level".to_string(),
            value: params.level,
            uom: params.level_uom.clone(),
        });
        plan.activity.parameters.push(Parameter::Text {
            title: "spacing".to_string(),
            value: params.spacing.word().to_string(),
        });
        plan.activity.parameters.push(Parameter::Text {
            title: "method".to_string(),
            value: "marching-tetrahedra".to_string(),
        });
        plan.activity.parameters.push(Parameter::Integer {
            title: "setAside".to_string(),
            value: params.set_aside as i64,
        });
        plan.says.push(Note {
            key: "promoted",
            detail: "the surface is written as it was computed; the level, the spacing and the inputs are recorded as the caller states them - this operation does not compute it again".to_string(),
        });
        Ok(plan)
    }

    fn reports(&self) -> Vec<Slot> {
        let mut slots = crate::operation::AnyOperation::reports(&crate::operation::Typed(Triangles));
        slots.push(Slot::required(
            "method",
            Need::Choice(&["marching-tetrahedra"]),
            "how the surface was computed",
        ));
        slots
    }
}
