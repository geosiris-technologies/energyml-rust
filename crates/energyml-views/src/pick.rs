//! The components of a property a setting asks for, written as JSON: one reading for every view.

use energyml_domain::property::shape::{ComponentPick, Reduction};
use serde_json::Value;

use crate::refusal::ViewRefusal;

/// The pick a setting's JSON asks for: `"all"`, `{"along": 0, "at": [0, 40]}` (with `"reduced": [1]` and
/// `"by": "mean"` to project dimensions), `{"at": [3, 1]}`, or `{"reduce": "max"}`. An `along` without `at` takes 0 in
/// each of the `rank` dimensions.
///
/// # Errors
///
/// [`ViewRefusal::BadSetting`] on `setting`, saying what was expected.
pub fn component_pick(setting: &str, value: &Value, rank: usize) -> Result<ComponentPick, ViewRefusal> {
    let bad = |why: String| ViewRefusal::BadSetting {
        name: setting.to_string(),
        why,
    };
    if value.as_str() == Some("all") {
        return Ok(ComponentPick::All);
    }
    let Some(map) = value.as_object() else {
        return Err(bad("expected \"all\" or an object".to_string()));
    };
    let indices = |key: &str| -> Result<Option<Vec<usize>>, ViewRefusal> {
        match map.get(key) {
            None => Ok(None),
            Some(list) => list
                .as_array()
                .and_then(|items| {
                    items
                        .iter()
                        .map(|item| item.as_u64().map(|n| n as usize))
                        .collect::<Option<Vec<usize>>>()
                })
                .map(Some)
                .ok_or_else(|| bad(format!("`{key}` is a list of whole numbers"))),
        }
    };
    if let Some(axis) = map.get("along") {
        let axis =
            axis.as_u64()
                .ok_or_else(|| bad("`along` is the number of a dimension".to_string()))? as usize;
        let at = indices("at")?.unwrap_or_else(|| vec![0; rank]);
        let reduced = indices("reduced")?.unwrap_or_default();
        if reduced.is_empty() {
            return Ok(ComponentPick::Along { axis, at });
        }
        let id = map
            .get("by")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("`reduced` needs `by`: the id of a reduction".to_string()))?;
        let how = Reduction::from_id(id).ok_or_else(|| bad(format!("no reduction `{id}`")))?;
        return Ok(ComponentPick::along_reduced(axis, at, reduced, how));
    }
    if let Some(at) = indices("at")? {
        return Ok(ComponentPick::At(at));
    }
    if let Some(id) = map.get("reduce").and_then(Value::as_str) {
        let how = Reduction::from_id(id).ok_or_else(|| bad(format!("no reduction `{id}`")))?;
        return Ok(ComponentPick::Reduce(how));
    }
    Err(bad("expected `along`, `at` or `reduce`".to_string()))
}
