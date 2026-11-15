//! Reading an `Activity` and its `ActivityTemplate`, in any of the four releases
//! that have them (RESQML 2.0.1, EML 2.1, 2.2 and 2.3).
//!
//! `provenance` *writes* an activity; this module *reads* one. Reading the four
//! releases is another responsibility, and a reader that knew only what the
//! writer writes would be the writer checking itself.
//!
//! # Read by the field present, never by the class name
//!
//! A parameter's value is decided by the fields it carries (`DataObject`,
//! `TimeIndex`, `Value` with or without `Uom`), not by its `$type`: RESQML 2.0.1
//! calls a double `FloatingPointQuantityParameter`, the three EML releases
//! `DoubleQuantityParameter`, and a reader keyed on names would miss one.
//!
//! The objects are read through the workspace's own JSON encoding
//! ([`energyml_core::json::write_value`]), so this crate depends on no generated
//! crate.
//!
//! # Signals, never refuses
//!
//! A shape this build cannot read is a [`ParameterValue::Unreadable`] line, shown
//! and never blank. What departs from the template is reported by
//! [`conformance`], never refused (rule of 16/10: detect and signal).

use energyml_core::identity::{self, ObjectId};
use energyml_core::naming::strip_obj_prefix;
use energyml_core::value::Reflect;
use serde_json::Value;

mod conformance;
mod roles;

pub use conformance::{ParameterIssue, conformance};
pub use roles::{
    ActivityRole, ENERGYML_BUILD_INPUT_OUTPUT, EnergymlBuildInputOutput, ParameterRole, RoleBasis, RoleRule,
    RoleRules, roles,
};

/// A reference as an activity declares it - not resolved against any store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredReference {
    /// `identity::address_of_parts(type, uuid, version)`: the full uri when the
    /// reference names a type, the bare uuid otherwise (the reference's one
    /// exception, `energyml_core::identity::address_of_parts`).
    pub address: String,
    /// The uuid, as written.
    pub uuid: String,
    /// `QualifiedType`, or the one derived from `ContentType`.
    pub qualified_type: Option<String>,
    /// `Title`.
    pub title: Option<String>,
    /// `ObjectVersion` (eml 2.3) or `VersionString` (eml 2.0 to 2.2), blank as `None`.
    pub version: Option<String>,
}

/// One activity as the file states it, any of the four releases.
#[derive(Debug, Clone, PartialEq)]
pub struct ActivityRecord {
    /// Full uri (`identity::uri_of_reflect`), never a bare uuid.
    pub uri: String,
    /// `Citation.Title`.
    pub title: Option<String>,
    /// `Citation.Creation`, as written.
    pub creation: Option<String>,
    /// `Citation.LastUpdate`, as written.
    pub last_update: Option<String>,
    /// `Citation.Originator`.
    pub originator: Option<String>,
    /// `Citation.Format`: which software wrote it.
    pub format: Option<String>,
    /// `ActivityDescriptor`.
    pub descriptor: Option<DeclaredReference>,
    /// `Parent`.
    pub parent: Option<DeclaredReference>,
    /// `Parameter`, in document order.
    pub parameters: Vec<ActivityParameter>,
}

/// One parameter of an activity.
#[derive(Debug, Clone, PartialEq)]
pub struct ActivityParameter {
    /// Rank in `Activity.Parameter` (0-based).
    pub position: usize,
    /// `Title`; empty when the file has none.
    pub title: String,
    /// `Index`.
    pub index: Option<i64>,
    /// `Selection`.
    pub selection: Option<String>,
    /// `IsUncertain`: eml 2.3 only; `None` in the releases that do not have it.
    pub is_uncertain: Option<bool>,
    /// `Key`.
    pub keys: Vec<ParameterKey>,
    /// The value, read by the field present.
    pub value: ParameterValue,
    /// The `$type` it was read from (`eml23.DoubleQuantityParameter`), for display only.
    pub class: String,
}

/// A parameter's value, **read by the field present, never by the class name**.
#[derive(Debug, Clone, PartialEq)]
pub enum ParameterValue {
    /// `DataObject`.
    Object(DeclaredReference),
    /// `Value` with `Uom`: `DoubleQuantityParameter` (eml) or
    /// `FloatingPointQuantityParameter` (resqml 2.0.1).
    Double {
        /// The value.
        value: f64,
        /// The unit of measure, as written.
        uom: String,
    },
    /// `Value`, a whole JSON number, without `Uom`: `IntegerQuantityParameter`
    /// (no unit in any release, common TRG 2.3 section 3.8, p. 47).
    Integer {
        /// The value.
        value: i64,
    },
    /// `Value`, text.
    Text(String),
    /// `TimeIndex`: its `Index` and its `TimeSeries`.
    TimeIndex {
        /// The index in the time series.
        index: i64,
        /// The time series.
        series: DeclaredReference,
    },
    /// A shape this build cannot read: shown, never blank.
    Unreadable {
        /// The `$type` the parameter was written with.
        class: String,
    },
}

/// A `Key` of a parameter (common TRG 2.3 section 3.2, p. 41).
#[derive(Debug, Clone, PartialEq)]
pub enum ParameterKey {
    /// `ObjectParameterKey`.
    Object(DeclaredReference),
    /// `TimeIndexParameterKey`.
    TimeIndex {
        /// The index in the time series.
        index: i64,
        /// The time series.
        series: DeclaredReference,
    },
    /// Anything else.
    Unreadable {
        /// The `$type` the key was written with.
        class: String,
    },
}

/// `ActivityParameterKind` (common TRG 2.3 section 3.4, p. 43); resqml 2.0.1's
/// `floatingPoint` is read as `Double`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ParameterKind {
    /// `dataObject`.
    DataObject,
    /// `double` (`floatingPoint` in resqml 2.0.1).
    Double,
    /// `integer`.
    Integer,
    /// `string`.
    String,
    /// `timestamp`.
    Timestamp,
    /// `subActivity`.
    SubActivity,
    /// A word this build does not know, kept as written.
    Other(String),
}

impl ParameterKind {
    /// The kind a template writes: `dataObject`, `double` or `floatingPoint`, ...
    pub fn from_word(word: &str) -> ParameterKind {
        let word = word.trim();
        let is = |name: &str| word.eq_ignore_ascii_case(name);
        if is("dataObject") {
            ParameterKind::DataObject
        } else if is("double") || is("floatingPoint") {
            ParameterKind::Double
        } else if is("integer") {
            ParameterKind::Integer
        } else if is("string") {
            ParameterKind::String
        } else if is("timestamp") {
            ParameterKind::Timestamp
        } else if is("subActivity") {
            ParameterKind::SubActivity
        } else {
            ParameterKind::Other(word.to_string())
        }
    }

    /// The word eml 2.3 writes.
    pub fn word(&self) -> &str {
        match self {
            ParameterKind::DataObject => "dataObject",
            ParameterKind::Double => "double",
            ParameterKind::Integer => "integer",
            ParameterKind::String => "string",
            ParameterKind::Timestamp => "timestamp",
            ParameterKind::SubActivity => "subActivity",
            ParameterKind::Other(word) => word,
        }
    }
}

impl ParameterValue {
    /// The kind this value is written as: `Object` is `DataObject`, `Double` is
    /// `Double`, `Integer` is `Integer`, `Text` is `String`, `TimeIndex` is
    /// `Timestamp`, `Unreadable` is `None`.
    pub fn kind(&self) -> Option<ParameterKind> {
        match self {
            ParameterValue::Object(_) => Some(ParameterKind::DataObject),
            ParameterValue::Double { .. } => Some(ParameterKind::Double),
            ParameterValue::Integer { .. } => Some(ParameterKind::Integer),
            ParameterValue::Text(_) => Some(ParameterKind::String),
            ParameterValue::TimeIndex { .. } => Some(ParameterKind::Timestamp),
            ParameterValue::Unreadable { .. } => None,
        }
    }
}

/// The four states `IsInput` and `IsOutput` make.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParameterDirection {
    /// An input only.
    Input,
    /// An output only.
    Output,
    /// Both.
    Both,
    /// Neither.
    Neither,
}

/// An `ActivityTemplate` as the file states it.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateRecord {
    /// Full uri.
    pub uri: String,
    /// `Citation.Title` (`fabricant:crs`, `workflow:lutetia`).
    pub title: Option<String>,
    /// `Parameter`, in document order.
    pub parameters: Vec<ParameterTemplateRecord>,
}

/// One `ParameterTemplate`.
#[derive(Debug, Clone, PartialEq)]
pub struct ParameterTemplateRecord {
    /// Rank in `ActivityTemplate.Parameter`.
    pub position: usize,
    /// `Title`.
    pub title: String,
    /// `AllowedKind`; empty means any (common TRG 2.3 section 3.10, p. 49).
    pub allowed: Vec<ParameterKind>,
    /// From `IsInput` and `IsOutput`.
    pub direction: ParameterDirection,
    /// `MinOccurs`; `None` when the file lacks it.
    pub min_occurs: Option<i64>,
    /// `MaxOccurs`; -1 is infinite; `None` when the file lacks it.
    pub max_occurs: Option<i64>,
    /// `Constraint`.
    pub constraint: Option<String>,
    /// `DataObjectContentType`.
    pub content_type: Option<String>,
    /// `KeyConstraint`.
    pub key_constraints: Vec<String>,
    /// `DefaultValue`.
    pub defaults: Vec<ActivityParameter>,
}

/// Why an object is not read as an activity or a template.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ActivityRefusal {
    /// Not an `Activity` in any of the four releases.
    #[error("`{qualified_type}` is not an Activity (resqml 2.0.1, eml 2.1, 2.2 or 2.3)")]
    NotAnActivity {
        /// What the object is.
        qualified_type: String,
    },
    /// Not an `ActivityTemplate`.
    #[error("`{qualified_type}` is not an ActivityTemplate (resqml 2.0.1, eml 2.1, 2.2 or 2.3)")]
    NotATemplate {
        /// What the object is.
        qualified_type: String,
    },
    /// No uuid, so no uri: nothing to name it by.
    #[error("this `{qualified_type}` has no uuid, so it has no uri to be named by")]
    NoIdentity {
        /// What the object is.
        qualified_type: String,
    },
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// A key of a JSON object, **case ignored**.
fn field<'v>(value: &'v Value, name: &str) -> Option<&'v Value> {
    value
        .as_object()?
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, found)| found)
}

/// A text field, as written.
fn text(value: &Value, name: &str) -> Option<String> {
    field(value, name).and_then(|found| found.as_str()).map(str::to_string)
}

/// A JSON array field, empty when absent.
fn items<'v>(value: &'v Value, name: &str) -> &'v [Value] {
    field(value, name)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// What the object is, for a refusal.
fn qualified_of(object: &dyn Reflect) -> String {
    let info = object.type_info();
    info.qualified_type.unwrap_or(info.name).to_string()
}

/// A reference, read by the identity the library already knows how to read.
fn declared_reference(value: &Value) -> DeclaredReference {
    let uuid = text(value, "Uuid").unwrap_or_default().trim().to_string();
    let content_type = text(value, "ContentType");
    let qualified_type = text(value, "QualifiedType").or_else(|| {
        let declared = content_type.as_deref()?;
        ObjectId::from_content_type(declared, &uuid)
            .ok()
            .map(|id| id.qualified_type())
    });
    let version = text(value, "ObjectVersion")
        .or_else(|| text(value, "VersionString"))
        .map(|one| one.trim().to_string())
        .filter(|one| !one.is_empty());
    let declared_type = text(value, "QualifiedType").or(content_type);
    DeclaredReference {
        address: identity::address_of_parts(declared_type.as_deref(), &uuid, version.as_deref()),
        uuid,
        qualified_type,
        title: text(value, "Title"),
        version,
    }
}

/// One `Key` of a parameter, by the field present.
fn read_key(value: &Value) -> ParameterKey {
    if let Some(reference) = field(value, "DataObject") {
        return ParameterKey::Object(declared_reference(reference));
    }
    if let Some(time) = field(value, "TimeIndex")
        && let Some(index) = field(time, "Index").and_then(Value::as_i64)
        && let Some(series) = field(time, "TimeSeries")
    {
        return ParameterKey::TimeIndex {
            index,
            series: declared_reference(series),
        };
    }
    ParameterKey::Unreadable {
        class: text(value, "$type").unwrap_or_default(),
    }
}

/// One parameter, by the field present.
fn read_parameter(position: usize, value: &Value) -> ActivityParameter {
    let class = text(value, "$type").unwrap_or_default();
    let read_value = || -> ParameterValue {
        if let Some(reference) = field(value, "DataObject") {
            return ParameterValue::Object(declared_reference(reference));
        }
        if let Some(time) = field(value, "TimeIndex")
            && let Some(index) = field(time, "Index").and_then(Value::as_i64)
            && let Some(series) = field(time, "TimeSeries")
        {
            return ParameterValue::TimeIndex {
                index,
                series: declared_reference(series),
            };
        }
        if let Some(found) = field(value, "Value") {
            let uom = field(value, "Uom").and_then(Value::as_str);
            if let (Some(number), Some(uom)) = (found.as_f64(), uom) {
                return ParameterValue::Double {
                    value: number,
                    uom: uom.to_string(),
                };
            }
            if let Some(word) = found.as_str() {
                return ParameterValue::Text(word.to_string());
            }
            if uom.is_none()
                && let Some(whole) = found.as_i64()
            {
                return ParameterValue::Integer { value: whole };
            }
        }
        ParameterValue::Unreadable { class: class.clone() }
    };
    ActivityParameter {
        position,
        title: text(value, "Title").unwrap_or_default(),
        index: field(value, "Index").and_then(Value::as_i64),
        selection: text(value, "Selection"),
        is_uncertain: field(value, "IsUncertain").and_then(Value::as_bool),
        keys: items(value, "Key").iter().map(read_key).collect(),
        value: read_value(),
        class,
    }
}

/// Read an activity of any of the four releases.
///
/// Refuses only what it cannot be: an object that is not an `Activity`, or that
/// has no uuid. A parameter it cannot read is a line marked
/// [`ParameterValue::Unreadable`], never a refusal of the whole activity.
pub fn read_activity(object: &dyn Reflect) -> Result<ActivityRecord, ActivityRefusal> {
    let info = object.type_info();
    if strip_obj_prefix(info.name) != "Activity" {
        return Err(ActivityRefusal::NotAnActivity {
            qualified_type: qualified_of(object),
        });
    }
    let uri = identity::uri_of_reflect(object).ok_or_else(|| ActivityRefusal::NoIdentity {
        qualified_type: qualified_of(object),
    })?;
    let json = energyml_core::json::write_value(object);
    let citation = field(&json, "Citation");
    let of_citation = |name: &str| citation.and_then(|one| text(one, name));
    Ok(ActivityRecord {
        uri,
        title: of_citation("Title"),
        creation: of_citation("Creation"),
        last_update: of_citation("LastUpdate"),
        originator: of_citation("Originator"),
        format: of_citation("Format"),
        descriptor: field(&json, "ActivityDescriptor").map(declared_reference),
        parent: field(&json, "Parent").map(declared_reference),
        parameters: items(&json, "Parameter")
            .iter()
            .enumerate()
            .map(|(position, one)| read_parameter(position, one))
            .collect(),
    })
}

/// Read an activity template of any of the four releases.
pub fn read_template(object: &dyn Reflect) -> Result<TemplateRecord, ActivityRefusal> {
    let info = object.type_info();
    if strip_obj_prefix(info.name) != "ActivityTemplate" {
        return Err(ActivityRefusal::NotATemplate {
            qualified_type: qualified_of(object),
        });
    }
    let uri = identity::uri_of_reflect(object).ok_or_else(|| ActivityRefusal::NoIdentity {
        qualified_type: qualified_of(object),
    })?;
    let json = energyml_core::json::write_value(object);
    let parameters = items(&json, "Parameter")
        .iter()
        .enumerate()
        .map(|(position, one)| {
            let flag = |name: &str| field(one, name).and_then(Value::as_bool).unwrap_or(false);
            let direction = match (flag("IsInput"), flag("IsOutput")) {
                (true, false) => ParameterDirection::Input,
                (false, true) => ParameterDirection::Output,
                (true, true) => ParameterDirection::Both,
                (false, false) => ParameterDirection::Neither,
            };
            ParameterTemplateRecord {
                position,
                title: text(one, "Title").unwrap_or_default(),
                allowed: items(one, "AllowedKind")
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ParameterKind::from_word)
                    .collect(),
                direction,
                min_occurs: field(one, "MinOccurs").and_then(Value::as_i64),
                max_occurs: field(one, "MaxOccurs").and_then(Value::as_i64),
                constraint: text(one, "Constraint"),
                content_type: text(one, "DataObjectContentType"),
                key_constraints: items(one, "KeyConstraint")
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect(),
                defaults: items(one, "DefaultValue")
                    .iter()
                    .enumerate()
                    .map(|(rank, default)| read_parameter(rank, default))
                    .collect(),
            }
        })
        .collect();
    Ok(TemplateRecord {
        uri,
        title: field(&json, "Citation").and_then(|one| text(one, "Title")),
        parameters,
    })
}

/// The template an activity's `ActivityDescriptor` names, among `templates`:
/// same uuid (case ignored) and versions that agree
/// ([`identity::versions_agree`]); of several, the first whose version is an
/// exact fit, else the first that is compatible.
pub fn descriptor_of<'t>(activity: &ActivityRecord, templates: &'t [TemplateRecord]) -> Option<&'t TemplateRecord> {
    let wanted = activity.descriptor.as_ref()?;
    let (wanted_uuid, wanted_version) = identity::parse_address(&wanted.address);
    let mut compatible: Option<&TemplateRecord> = None;
    for template in templates {
        let (uuid, version) = identity::parse_address(&template.uri);
        if uuid != wanted_uuid {
            continue;
        }
        match identity::version_fit(wanted_version.as_deref(), version.as_deref()) {
            identity::VersionFit::Exact => return Some(template),
            identity::VersionFit::Compatible => {
                compatible.get_or_insert(template);
            }
            identity::VersionFit::Contradicts => {}
        }
    }
    compatible
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // 08 section 2.3: a parameter that cannot be read is shown, never blank.
    // Break it: skip the parameter that has no readable value, and `len()` is 3.
    #[test]
    fn an_unknown_parameter_shape_is_unreadable_never_blank() {
        let no_value = json!({ "$type": "eml23.StringParameter", "Title": "mode" });
        let float_without_unit = json!({ "$type": "eml23.DoubleQuantityParameter", "Title": "step", "Value": 2.5 });
        let read: Vec<ActivityParameter> = [no_value, float_without_unit]
            .iter()
            .enumerate()
            .map(|(position, one)| read_parameter(position, one))
            .collect();
        assert_eq!(read.len(), 2);
        for parameter in &read {
            assert_eq!(
                parameter.value,
                ParameterValue::Unreadable { class: parameter.class.clone() },
                "{}",
                parameter.title
            );
            assert!(!parameter.class.is_empty());
        }
    }
}
