//! What an activity writes against what its template declares: **signalled,
//! never refused** (rule of 16/10: detect and signal).

use super::{ActivityParameter, ActivityRecord, ParameterKind, ParameterValue, TemplateRecord};

/// What departs from the template.
#[derive(Debug, Clone, PartialEq)]
pub enum ParameterIssue {
    /// The title has no equivalent in the template (common TRG 2.3 section 3.1, p. 40).
    NotInTemplate {
        /// Rank of the parameter.
        position: usize,
        /// Its title.
        title: String,
    },
    /// The kind written is not among the kinds the template allows.
    KindDiffers {
        /// Rank of the parameter.
        position: usize,
        /// Its title.
        title: String,
        /// The kinds the template allows.
        declared: Vec<ParameterKind>,
        /// The kind written.
        written: ParameterKind,
    },
    /// Fewer than `MinOccurs`.
    TooFew {
        /// The title.
        title: String,
        /// `MinOccurs`.
        min: i64,
        /// How many parameters of this title the activity has.
        found: usize,
    },
    /// More than `MaxOccurs`.
    TooMany {
        /// The title.
        title: String,
        /// `MaxOccurs`.
        max: i64,
        /// How many parameters of this title the activity has.
        found: usize,
    },
    /// The activity names no template, or one the caller could not find.
    TemplateAbsent,
}

/// Whether a parameter naming an activity stands for a sub-activity: a
/// reference whose type, without prefix and without `obj_`, is `Activity`.
fn names_an_activity(parameter: &ActivityParameter) -> bool {
    let ParameterValue::Object(reference) = &parameter.value else {
        return false;
    };
    reference.qualified_type.as_deref().is_some_and(|qualified| {
        let local = qualified.rsplit('.').next().unwrap_or(qualified);
        energyml_core::naming::strip_obj_prefix(local) == "Activity"
    })
}

/// What departs from `template` in `activity`. Signals; never refuses.
///
/// 1. No template: `[TemplateAbsent]`.
/// 2. Each parameter, in document order: no `ParameterTemplate` of its title is
///    `NotInTemplate`; an empty `AllowedKind` allows every kind (common TRG 2.3
///    section 3.10, p. 49); a value that cannot be read is not judged on its
///    kind; a reference to an activity under `subActivity` conforms.
/// 3. Each `ParameterTemplate`: `MinOccurs` above zero not held is `TooFew`;
///    `MaxOccurs` from zero up exceeded is `TooMany` (-1 is infinite).
pub fn conformance(activity: &ActivityRecord, template: Option<&TemplateRecord>) -> Vec<ParameterIssue> {
    let Some(template) = template else {
        return vec![ParameterIssue::TemplateAbsent];
    };
    let mut issues = Vec::new();
    for parameter in &activity.parameters {
        let Some(entry) = template.parameters.iter().find(|one| one.title == parameter.title) else {
            issues.push(ParameterIssue::NotInTemplate {
                position: parameter.position,
                title: parameter.title.clone(),
            });
            continue;
        };
        if entry.allowed.is_empty() {
            continue;
        }
        let Some(written) = parameter.value.kind() else {
            continue;
        };
        if entry.allowed.contains(&written) {
            continue;
        }
        if written == ParameterKind::DataObject
            && entry.allowed.contains(&ParameterKind::SubActivity)
            && names_an_activity(parameter)
        {
            continue;
        }
        issues.push(ParameterIssue::KindDiffers {
            position: parameter.position,
            title: parameter.title.clone(),
            declared: entry.allowed.clone(),
            written,
        });
    }
    for entry in &template.parameters {
        let found = activity
            .parameters
            .iter()
            .filter(|parameter| parameter.title == entry.title)
            .count();
        if let Some(min) = entry.min_occurs
            && min > 0
            && (found as i64) < min
        {
            issues.push(ParameterIssue::TooFew {
                title: entry.title.clone(),
                min,
                found,
            });
        }
        if let Some(max) = entry.max_occurs
            && max >= 0
            && (found as i64) > max
        {
            issues.push(ParameterIssue::TooMany {
                title: entry.title.clone(),
                max,
                found,
            });
        }
    }
    issues
}
