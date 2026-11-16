//! The steps a workflow template declares, and the two conventions a writer
//! may have used that this crate does not know.
//!
//! `energyml-domain` knows no writer's convention: the free-text `Constraint` of
//! a sub-activity (`follows fabricant:crs; serves ...`) and the value of an
//! `inputFile` parameter are read by a reader the caller hands in, through the
//! two traits below. `energyml-build` provides the readers of its own writer.

use crate::activity::{ActivityParameter, ActivityRecord, ParameterKind, TemplateRecord};

/// What a workflow template's `Constraint` says of a step, read by a reader
/// that knows the writer's convention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepConstraint {
    /// The canonical recipe name (`crs` from `follows fabricant:crs`).
    pub operation: String,
    /// The template uuid, when written.
    pub template_uuid: Option<String>,
    /// What the step serves.
    pub serves: Vec<String>,
    /// Why it was held, when it was.
    pub held: Option<String>,
}

/// A reader of the free-text constraint of a sub-activity. `energyml-build`
/// provides one (`recipe::RecipeConstraints`); domain knows no convention.
pub trait StepConstraintReader: Send + Sync {
    /// The constraint read, or `None` when its form is not the reader's.
    fn read(&self, constraint: &str) -> Option<StepConstraint>;
}

/// No convention known: every constraint is shown as written.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoStepConstraints;

impl StepConstraintReader for NoStepConstraints {
    fn read(&self, _constraint: &str) -> Option<StepConstraint> {
        None
    }
}

/// A reader of the file a text parameter names, by a writer's convention.
pub trait InputFileReader: Send + Sync {
    /// Stable id of the convention, shown beside the role of the edge it gives.
    fn id(&self) -> &'static str;
    /// The path the parameter names, or `None` when it is not a file parameter
    /// of this convention.
    fn file(&self, activity: &ActivityRecord, parameter: &ActivityParameter) -> Option<String>;
}

/// No convention known: a text parameter is never a file.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoInputFiles;

impl InputFileReader for NoInputFiles {
    fn id(&self) -> &'static str {
        "none"
    }

    fn file(&self, _activity: &ActivityRecord, _parameter: &ActivityParameter) -> Option<String> {
        None
    }
}

/// One declared step of a workflow template.
#[derive(Debug, Clone, PartialEq)]
pub struct DeclaredStep {
    /// The workflow template's uri.
    pub workflow: String,
    /// The `ParameterTemplate` title: the step's name.
    pub title: String,
    /// Its rank in the template.
    pub position: usize,
    /// `MinOccurs`, when the file states it.
    pub min_occurs: Option<i64>,
    /// The constraint as written.
    pub constraint: Option<String>,
    /// The constraint read, when the reader knows its form.
    pub parsed: Option<StepConstraint>,
}

impl DeclaredStep {
    /// The reason the step was held: `MinOccurs` is 0 **and** the constraint
    /// says why. A step with `MinOccurs` 0 that says nothing is optional, not
    /// held.
    pub fn held(&self) -> Option<&str> {
        if self.min_occurs != Some(0) {
            return None;
        }
        self.parsed
            .as_ref()
            .and_then(|parsed| parsed.held.as_deref())
            .filter(|reason| !reason.trim().is_empty())
    }
}

/// The steps a workflow template declares: one per parameter whose allowed
/// kinds include `subActivity`, in the template's order. A template with no
/// such parameter declares none.
pub fn workflow_steps(template: &TemplateRecord, reader: &dyn StepConstraintReader) -> Vec<DeclaredStep> {
    template
        .parameters
        .iter()
        .filter(|parameter| parameter.allowed.contains(&ParameterKind::SubActivity))
        .map(|parameter| DeclaredStep {
            workflow: template.uri.clone(),
            title: parameter.title.clone(),
            position: parameter.position,
            min_occurs: parameter.min_occurs,
            constraint: parameter.constraint.clone(),
            parsed: parameter
                .constraint
                .as_deref()
                .and_then(|constraint| reader.read(constraint)),
        })
        .collect()
}
