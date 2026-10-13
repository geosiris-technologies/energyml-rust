//! Dotted-path access into generated objects.
//!
//! Supports `Citation.Title`, `TrianglePatch[0].Count`, and — through
//! [`get_ignore_case`] — the same paths written in any casing, so a user who
//! does not know the model exactly can still address a value.

use crate::error::{Error, Result};
use crate::value::{FieldValue, OwnedValue, Reflect};

/// One step of a path: a field name plus an optional collection index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    /// Field name, as written by the caller.
    pub name: String,
    /// Index into a repeated field, if the segment had one.
    pub index: Option<usize>,
}

/// Split a dotted path into segments.
pub fn parse(path: &str) -> Result<Vec<Segment>> {
    if path.trim().is_empty() {
        return Err(Error::InvalidPath {
            path: path.to_string(),
            reason: "path is empty".into(),
        });
    }
    let mut out = Vec::new();
    for raw in path.split('.') {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(Error::InvalidPath {
                path: path.to_string(),
                reason: "empty segment".into(),
            });
        }
        match raw.find('[') {
            None => out.push(Segment {
                name: raw.to_string(),
                index: None,
            }),
            Some(open) => {
                let close = raw.rfind(']').ok_or_else(|| Error::InvalidPath {
                    path: path.to_string(),
                    reason: format!("unclosed `[` in `{raw}`"),
                })?;
                if close < open {
                    return Err(Error::InvalidPath {
                        path: path.to_string(),
                        reason: format!("malformed index in `{raw}`"),
                    });
                }
                let name = raw[..open].trim();
                if name.is_empty() {
                    return Err(Error::InvalidPath {
                        path: path.to_string(),
                        reason: format!("missing field name in `{raw}`"),
                    });
                }
                let index: usize = raw[open + 1..close]
                    .trim()
                    .parse()
                    .map_err(|_| Error::InvalidPath {
                        path: path.to_string(),
                        reason: format!("`{}` is not a valid index", &raw[open + 1..close]),
                    })?;
                out.push(Segment {
                    name: name.to_string(),
                    index: Some(index),
                });
            }
        }
    }
    Ok(out)
}

fn field_of<'a>(object: &'a dyn Reflect, name: &str, ignore_case: bool) -> Result<FieldValue<'a>> {
    let value = if ignore_case {
        object.field_ignore_case(name)
    } else {
        object.field(name)
    };
    value.ok_or_else(|| Error::UnknownField {
        ty: object.type_info().name,
        field: name.to_string(),
    })
}

fn index_into<'a>(value: FieldValue<'a>, seg: &Segment, path: &str) -> Result<FieldValue<'a>> {
    match seg.index {
        None => Ok(value),
        Some(i) => match value {
            FieldValue::List(mut items) => {
                if i >= items.len() {
                    return Err(Error::InvalidPath {
                        path: path.to_string(),
                        reason: format!("index {i} out of bounds for `{}` (len {})", seg.name, items.len()),
                    });
                }
                Ok(items.swap_remove(i))
            }
            other if i == 0 => Ok(other),
            _ => Err(Error::InvalidPath {
                path: path.to_string(),
                reason: format!("`{}` is not a repeated field", seg.name),
            }),
        },
    }
}

fn get_inner<'a>(root: &'a dyn Reflect, path: &str, ignore_case: bool) -> Result<FieldValue<'a>> {
    let segments = parse(path)?;
    let mut current: &'a dyn Reflect = root;
    for (i, seg) in segments.iter().enumerate() {
        let value = index_into(field_of(current, &seg.name, ignore_case)?, seg, path)?;
        if i + 1 == segments.len() {
            return Ok(value);
        }
        current = value.as_object().ok_or_else(|| Error::InvalidPath {
            path: path.to_string(),
            reason: format!("`{}` is not an object, cannot descend into it", seg.name),
        })?;
    }
    unreachable!("parse() rejects empty paths")
}

/// Read a value by dotted path, matching field names exactly.
pub fn get<'a>(root: &'a dyn Reflect, path: &str) -> Result<FieldValue<'a>> {
    get_inner(root, path, false)
}

/// Read a value by dotted path, ignoring case.
pub fn get_ignore_case<'a>(root: &'a dyn Reflect, path: &str) -> Result<FieldValue<'a>> {
    get_inner(root, path, true)
}

/// Read a value by dotted path and render it as a string, when it is a scalar.
pub fn get_string(root: &dyn Reflect, path: &str) -> Result<Option<String>> {
    match get_ignore_case(root, path)? {
        FieldValue::Null => Ok(None),
        FieldValue::Scalar(s) => Ok(Some(s.to_string())),
        other => Err(Error::TypeMismatch {
            path: path.to_string(),
            expected: "scalar".into(),
            found: other.kind_name().into(),
        }),
    }
}

/// Borrow the **object** a path names, mutably.
///
/// [`set`] walks to a path's parent and writes one field. This walks the whole
/// path and hands back the object itself, which is what a caller needs when the
/// thing to change is not one value but a small cluster of them that must stay
/// consistent — a `DataObjectReference`'s uuid, type and title being the case
/// this was written for. Doing that through [`set`] would mean three walks and
/// three chances to write half of a change.
pub fn object_mut<'a>(root: &'a mut dyn Reflect, path: &str) -> Result<&'a mut dyn Reflect> {
    let segments = parse(path)?;
    let mut current: &'a mut dyn Reflect = root;
    for seg in &segments {
        let ty = current.type_info().name;
        current = current
            .field_object_mut(&seg.name, seg.index)
            .ok_or_else(|| Error::InvalidPath {
                path: path.to_string(),
                reason: format!("`{}` is absent or not an object on `{ty}`", seg.name),
            })?;
    }
    Ok(current)
}

/// Write a value by dotted path.
///
/// Intermediate objects must already exist; this completes an instance rather
/// than building missing branches, so a UI stays in control of what it creates.
pub fn set(root: &mut dyn Reflect, path: &str, value: OwnedValue) -> Result<()> {
    let segments = parse(path)?;
    let (last, parents) = segments.split_last().expect("parse() rejects empty paths");

    let mut current: &mut dyn Reflect = root;
    for seg in parents {
        let ty = current.type_info().name;
        current = current
            .field_object_mut(&seg.name, seg.index)
            .ok_or_else(|| Error::InvalidPath {
                path: path.to_string(),
                reason: format!("`{}` is absent or not an object on `{}`", seg.name, ty),
            })?;
    }

    match last.index {
        None => current.set_field(&last.name, value),
        Some(_) => Err(Error::InvalidPath {
            path: path.to_string(),
            reason: "setting an individual element of a repeated field is not supported; \
                     set the whole field instead"
                .into(),
        }),
    }
}

/// Set one value from JSON — a scalar, or a whole object.
///
/// # Why this exists next to [`set`]
///
/// [`set`] takes an [`OwnedValue`], which a caller across a language boundary
/// cannot build: putting a `Citation` in a field means constructing the right
/// concrete type, and only the registry knows how. So an editor written in
/// JavaScript could set a title and nothing else.
///
/// Here the value arrives as JSON and the **field decides how to read it**. The
/// field knows its type, whether it repeats, and — for an abstract slot — which
/// concrete classes are allowed; `$type` in the JSON picks among them, exactly
/// as it does in a document.
///
/// ```text
/// set_json(o, "Citation.Title",  "\"Top Brent\"")
/// set_json(o, "Citation",        "{\"Title\":\"Top Brent\",\"Originator\":\"moi\"}")
/// set_json(o, "Geometry.Points", "{\"$type\":\"resqml22.Point3dLatticeArray\", …}")
/// set_json(o, "Citation.Editor", "null")
/// ```
///
/// Objects a `_data` member carried are **returned, not dropped**: a reference
/// may ship its target alongside it, and a caller filling a store has somewhere
/// to put them.
#[cfg(feature = "json")]
pub fn set_json(
    root: &mut dyn Reflect,
    path: &str,
    json: &str,
    registry: &crate::registry::Registry,
) -> Result<Vec<Box<dyn crate::registry::DynObject>>> {
    let value: serde_json::Value = serde_json::from_str(json).map_err(|e| Error::InvalidPath {
        path: path.to_string(),
        reason: format!("the value is not JSON: {e}"),
    })?;
    set_json_value(root, path, &value, registry)
}

/// [`set_json`], from an already-parsed value.
#[cfg(feature = "json")]
pub fn set_json_value(
    root: &mut dyn Reflect,
    path: &str,
    value: &serde_json::Value,
    registry: &crate::registry::Registry,
) -> Result<Vec<Box<dyn crate::registry::DynObject>>> {
    let segments = parse(path)?;
    let (last, parents) = segments.split_last().expect("parse() rejects empty paths");

    let mut current: &mut dyn Reflect = root;
    for seg in parents {
        let ty = current.type_info().name;
        current = current
            .field_object_mut(&seg.name, seg.index)
            .ok_or_else(|| Error::InvalidPath {
                path: path.to_string(),
                reason: format!("`{}` is absent or not an object on `{}`", seg.name, ty),
            })?;
    }

    if last.index.is_some() {
        return Err(Error::InvalidPath {
            path: path.to_string(),
            reason: "setting an individual element of a repeated field is not supported; \
                     set the whole field instead"
                .into(),
        });
    }

    // The field, found the same way `set_field` finds it — by name, then
    // ignoring case. A path typed by a person says `citation.title` as often as
    // `Citation.Title`, and refusing the first while accepting it one segment
    // earlier would be the sort of inconsistency nobody can remember.
    let info = current.type_info();
    let field = info
        .field(&last.name)
        .or_else(|| info.field_ignore_case(&last.name))
        .ok_or_else(|| Error::InvalidPath {
            path: path.to_string(),
            reason: format!("`{}` has no field `{}`", info.name, last.name),
        })?;

    let (owned, referenced) = crate::json::read_field_value(value, field, registry)?;
    current.set_field(field.name, owned)?;
    Ok(referenced)
}
