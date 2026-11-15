//! The role of each parameter of an activity: input, output, both, neither, a
//! sub-activity, or not declared - and where that role comes from.
//!
//! The role of a parameter is written in the template and nowhere else (common
//! TRG 2.3 section 3.10, p. 49). Without a template, only a **named rule** gives
//! one; with neither, the role is [`ActivityRole::Undeclared`]. A role is never
//! guessed from a title.

use super::{
    ActivityParameter, ActivityRecord, ParameterDirection, ParameterKind, ParameterValue, TemplateRecord,
};

/// What a parameter is to its activity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActivityRole {
    /// An input.
    Input,
    /// An output.
    Output,
    /// Both an input and an output.
    Both,
    /// Neither: a setting.
    Neither,
    /// A step of a workflow, named as a sub-activity.
    SubActivity,
    /// Nothing says: no template, no rule.
    Undeclared,
}

/// Where a role comes from: the template, or a named rule. Never guessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RoleBasis {
    /// The template declares it.
    Template,
    /// A named rule of the registry gives it.
    Rule(&'static str),
    /// Nothing: the role is `Undeclared`.
    None,
}

/// The role of one parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParameterRole {
    /// Rank of the parameter in the activity.
    pub position: usize,
    /// Its role.
    pub role: ActivityRole,
    /// Where the role comes from.
    pub basis: RoleBasis,
}

/// A rule that gives a role the template does not.
pub trait RoleRule: Send + Sync {
    /// Stable id, shown beside the role (`energyml-build:input-output`).
    fn id(&self) -> &'static str;
    /// The role, or `None` when the rule does not apply.
    fn role(&self, activity: &ActivityRecord, parameter: &ActivityParameter) -> Option<ActivityRole>;
}

/// The rules, the caller's tried first (ADR-0006).
pub struct RoleRules {
    rules: Vec<Box<dyn RoleRule>>,
    /// How many of `rules` the caller added, at the front.
    added: usize,
}

impl RoleRules {
    /// The built-in rules: [`EnergymlBuildInputOutput`].
    pub fn new() -> Self {
        RoleRules {
            rules: vec![Box::new(EnergymlBuildInputOutput)],
            added: 0,
        }
    }

    /// No rule at all.
    pub fn empty() -> Self {
        RoleRules {
            rules: Vec::new(),
            added: 0,
        }
    }

    /// A caller's rule, tried before the built-ins and after the caller's earlier ones.
    pub fn add(&mut self, rule: Box<dyn RoleRule>) {
        self.rules.insert(self.added, rule);
        self.added += 1;
    }

    /// The ids, in the order they are tried.
    pub fn ids(&self) -> Vec<&'static str> {
        self.rules.iter().map(|rule| rule.id()).collect()
    }
}

impl Default for RoleRules {
    fn default() -> Self {
        Self::new()
    }
}

/// The built-in rule: `input` and `output` written by `energyml-rust`.
///
/// It exists for the packages written before the templates declared `input`;
/// an activity of another software does not get it.
pub struct EnergymlBuildInputOutput;

/// The id of [`EnergymlBuildInputOutput`].
pub const ENERGYML_BUILD_INPUT_OUTPUT: &str = "energyml-build:input-output";

impl RoleRule for EnergymlBuildInputOutput {
    fn id(&self) -> &'static str {
        ENERGYML_BUILD_INPUT_OUTPUT
    }

    fn role(&self, activity: &ActivityRecord, parameter: &ActivityParameter) -> Option<ActivityRole> {
        if activity.originator.as_deref() != Some("energyml-rust") {
            return None;
        }
        if !matches!(parameter.value, ParameterValue::Object(_)) {
            return None;
        }
        match parameter.title.as_str() {
            "input" => Some(ActivityRole::Input),
            "output" => Some(ActivityRole::Output),
            _ => None,
        }
    }
}

/// The role of every parameter, in parameter order.
///
/// 1. The template: a `ParameterTemplate` of the same title (exact, case
///    sensitive). A `subActivity` kind on a reference is a `SubActivity`; else
///    the two flags give `Input`, `Output`, `Both` or `Neither`.
/// 2. Else the first rule of `rules` that applies.
/// 3. Else `Undeclared`. Never an entry guessed from the title.
pub fn roles(
    activity: &ActivityRecord,
    template: Option<&TemplateRecord>,
    rules: &RoleRules,
) -> Vec<ParameterRole> {
    activity
        .parameters
        .iter()
        .map(|parameter| {
            let declared =
                template.and_then(|one| one.parameters.iter().find(|entry| entry.title == parameter.title));
            if let Some(entry) = declared {
                let role = if entry.allowed.contains(&ParameterKind::SubActivity)
                    && matches!(parameter.value, ParameterValue::Object(_))
                {
                    ActivityRole::SubActivity
                } else {
                    match entry.direction {
                        ParameterDirection::Input => ActivityRole::Input,
                        ParameterDirection::Output => ActivityRole::Output,
                        ParameterDirection::Both => ActivityRole::Both,
                        ParameterDirection::Neither => ActivityRole::Neither,
                    }
                };
                return ParameterRole {
                    position: parameter.position,
                    role,
                    basis: RoleBasis::Template,
                };
            }
            for rule in &rules.rules {
                if let Some(role) = rule.role(activity, parameter) {
                    return ParameterRole {
                        position: parameter.position,
                        role,
                        basis: RoleBasis::Rule(rule.id()),
                    };
                }
            }
            ParameterRole {
                position: parameter.position,
                role: ActivityRole::Undeclared,
                basis: RoleBasis::None,
            }
        })
        .collect()
}
