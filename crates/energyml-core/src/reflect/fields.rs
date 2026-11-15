//! The one door for reading a field of a reflected object as a plain Rust value.
//!
//! Every reader of the model that walks a `&dyn Reflect` needs the same few
//! questions answered: *what text does this field hold*, *which number*, *is
//! the flag set*, *which nested object*, *which objects of a repeated field*.
//! They used to be answered by about forty private functions, one set per
//! module, and the copies disagreed - on a blank string, on a fractional
//! double asked for as an integer, on a string asked for as a number. The same
//! field then read differently depending on which check looked at it.
//! This module is the single answer (see ADR 0046).
//!
//! # The policy, stated once
//!
//! - **Absent is `None`.** A field the object does not have, an optional field
//!   that is unset ([`FieldValue::Null`]) and a field name the type does not
//!   declare all read as `None` (or an empty `Vec` for the plural readers).
//!   Absent is never zero, never `false`, never the empty string.
//! - **Blank is absent, for one field.** [`text`] treats a string that is empty
//!   or only whitespace as absent: no reader of the model wants a blank name.
//!   The text is returned as written, not trimmed. The plural readers keep
//!   every element, blank included, because in a list the position is the
//!   information.
//! - **A field of the wrong shape is `None`, not a guess.** A number asked of a
//!   string, a flag asked of an integer, an object asked of a scalar, a scalar
//!   asked of a list: `None`. The strict readers never parse a string as a
//!   number; a double is an integer only when it is finite, whole and in range
//!   (`2.0` is `2`, `2.5` is not).
//! - **Reading a number out of text is a named variant**, not an accident:
//!   [`number_lexical`], [`integer_lexical`] and [`flag_lexical`] also accept a
//!   string scalar whose trimmed text parses. Ask for them only for a field a
//!   schema version keeps as text.
//! - **A caller that must refuse says so**: the `require_*` readers return a
//!   named error - [`Error::MissingRequired`] when the field is absent,
//!   [`Error::TypeMismatch`] when it is there with another shape - instead of
//!   the caller inventing a default.
//! - **A repeated field and a single one read alike**: [`objects`] and
//!   [`items`] give one element for a field the schema declares singular and a
//!   file repeats, and the elements of a list otherwise. Elements of the wrong
//!   shape are left out.
//!
//! Nothing here follows a reference or looks at a uuid: that is the identity
//! door (`crate::identity`). Nothing here is case-insensitive: that is
//! [`Reflect::field_ignore_case`].

use crate::error::{Error, Result};
use crate::value::{FieldValue, Reflect, Scalar};

// ---------------------------------------------------------------------------
// Objects and items
// ---------------------------------------------------------------------------

/// The nested object a field holds; `None` when the field is absent or is not
/// a single object (a scalar, or a list: use [`objects`] for a repeated field).
pub fn object<'a>(object: &'a dyn Reflect, field: &str) -> Option<&'a dyn Reflect> {
    match object.field(field)? {
        FieldValue::Object(child) => Some(child),
        _ => None,
    }
}

/// The objects of a field, one or repeated, in document order.
///
/// A singular field gives one element; a list gives its object elements (a
/// scalar in the list is left out); an absent field, a scalar and anything
/// else give an empty `Vec`.
pub fn objects<'a>(object: &'a dyn Reflect, field: &str) -> Vec<&'a dyn Reflect> {
    match object.field(field) {
        Some(FieldValue::Object(child)) => vec![child],
        Some(FieldValue::List(items)) => items
            .into_iter()
            .filter_map(|item| match item {
                FieldValue::Object(child) => Some(child),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The items of a field, whether the schema spelled it as one element or as
/// many: a list gives its items, an absent or unset field gives none, anything
/// else is one item.
pub fn items<'a>(object: &'a dyn Reflect, field: &str) -> Vec<FieldValue<'a>> {
    match object.field(field) {
        Some(FieldValue::List(items)) => items,
        Some(FieldValue::Null) | None => Vec::new(),
        Some(single) => vec![single],
    }
}

// ---------------------------------------------------------------------------
// One scalar
// ---------------------------------------------------------------------------

/// A scalar field as its wire text: a string as written, an enumeration's
/// value, `true` for a boolean, the decimal text of a number.
///
/// `None` when the field is absent, is not a scalar, or is blank (empty or
/// only whitespace).
pub fn text(object: &dyn Reflect, field: &str) -> Option<String> {
    scalar_text(object.field(field)?.as_scalar()?)
}

/// A numeric scalar field (a double, a signed or an unsigned integer) as `f64`.
///
/// A string is **not** a number here: see [`number_lexical`].
pub fn number(object: &dyn Reflect, field: &str) -> Option<f64> {
    object.field(field)?.as_scalar()?.as_f64()
}

/// A number, or a string scalar whose trimmed text parses as a finite number.
///
/// For a field a schema version keeps as text (a WITSML 1.4 measure value, for
/// instance). A string that does not parse is `None`.
pub fn number_lexical(object: &dyn Reflect, field: &str) -> Option<f64> {
    match object.field(field)?.as_scalar()? {
        Scalar::Str(value) => value.trim().parse::<f64>().ok().filter(|v| v.is_finite()),
        other => other.as_f64(),
    }
}

/// A whole number as `i64`: a signed integer, an unsigned one that fits, or a
/// double that is finite, whole and in range. `None` otherwise (a fractional
/// double is not an integer, a string is not a number: see [`integer_lexical`]).
pub fn integer(object: &dyn Reflect, field: &str) -> Option<i64> {
    match object.field(field)?.as_scalar()? {
        Scalar::Int(v) => Some(v),
        Scalar::UInt(v) => i64::try_from(v).ok(),
        Scalar::Double(v) => whole_f64_to_i64(v),
        _ => None,
    }
}

/// A whole number as `i64`, or a scalar whose trimmed wire text parses as one.
///
/// For a field a schema version keeps as text. A fractional number or a string
/// that does not parse is `None`.
pub fn integer_lexical(object: &dyn Reflect, field: &str) -> Option<i64> {
    match object.field(field)?.as_scalar()? {
        Scalar::Double(v) => whole_f64_to_i64(v),
        other => other.to_string().trim().parse().ok(),
    }
}

/// A non-negative whole number as `u64`: an unsigned integer, a signed one
/// that is not negative, or a double that is finite, whole and not negative.
pub fn unsigned(object: &dyn Reflect, field: &str) -> Option<u64> {
    match object.field(field)?.as_scalar()? {
        Scalar::UInt(v) => Some(v),
        Scalar::Int(v) => u64::try_from(v).ok(),
        Scalar::Double(v) => {
            whole_f64_to_i64(v)
                .and_then(|whole| u64::try_from(whole).ok())
                .or_else(|| {
                    // Above `i64::MAX` but still exactly representable.
                    (v.is_finite() && v.fract() == 0.0 && (0.0..u64::MAX as f64).contains(&v))
                        .then_some(v as u64)
                })
        }
        _ => None,
    }
}

/// A boolean field. `None` when absent or not a boolean: absent is not false,
/// and an integer is not a flag (see [`flag_lexical`]).
pub fn flag(object: &dyn Reflect, field: &str) -> Option<bool> {
    match object.field(field)?.as_scalar()? {
        Scalar::Bool(v) => Some(v),
        _ => None,
    }
}

/// A boolean, or a scalar whose wire text is `true`, `1`, `false` or `0`.
pub fn flag_lexical(object: &dyn Reflect, field: &str) -> Option<bool> {
    match object.field(field)?.as_scalar()? {
        Scalar::Bool(v) => Some(v),
        other => match other.to_string().trim() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        },
    }
}

// ---------------------------------------------------------------------------
// Repeated scalars
// ---------------------------------------------------------------------------

/// The scalars of a field, one or repeated, as wire text.
///
/// Unlike [`text`], a blank element is **kept**: in a list the position is
/// the information (a column of a string table), so an empty string is a value.
/// Elements that are not scalars are left out.
pub fn texts(object: &dyn Reflect, field: &str) -> Vec<String> {
    items(object, field)
        .iter()
        .filter_map(|item| Some(item.as_scalar()?.to_string()))
        .collect()
}

/// The numeric scalars of a field, one or repeated, as `f64`.
pub fn numbers(object: &dyn Reflect, field: &str) -> Vec<f64> {
    items(object, field)
        .iter()
        .filter_map(|item| item.as_scalar()?.as_f64())
        .collect()
}

/// The whole-number scalars of a field, one or repeated, as `i64` (the
/// readings of [`integer`]).
pub fn integers(object: &dyn Reflect, field: &str) -> Vec<i64> {
    items(object, field)
        .iter()
        .filter_map(|item| match item.as_scalar()? {
            Scalar::Int(v) => Some(v),
            Scalar::UInt(v) => i64::try_from(v).ok(),
            Scalar::Double(v) => whole_f64_to_i64(v),
            _ => None,
        })
        .collect()
}

/// The non-negative whole-number scalars of a field, one or repeated, as `u64`.
pub fn unsigneds(object: &dyn Reflect, field: &str) -> Vec<u64> {
    items(object, field)
        .iter()
        .filter_map(|item| match item.as_scalar()? {
            Scalar::UInt(v) => Some(v),
            Scalar::Int(v) => u64::try_from(v).ok(),
            _ => None,
        })
        .collect()
}

/// The boolean scalars of a field, one or repeated.
pub fn flags(object: &dyn Reflect, field: &str) -> Vec<bool> {
    items(object, field)
        .iter()
        .filter_map(|item| match item.as_scalar()? {
            Scalar::Bool(v) => Some(v),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// A refusal, when the caller must have the value
// ---------------------------------------------------------------------------

/// [`text`], or a named refusal: [`Error::MissingRequired`] when the field is
/// absent or blank, [`Error::TypeMismatch`] when it is not a scalar.
pub fn require_text(object: &dyn Reflect, field: &str) -> Result<String> {
    required(object, field, "text", text(object, field))
}

/// [`number`], or a named refusal (see [`require_text`]).
pub fn require_number(object: &dyn Reflect, field: &str) -> Result<f64> {
    required(object, field, "number", number(object, field))
}

/// [`integer`], or a named refusal (see [`require_text`]).
pub fn require_integer(object: &dyn Reflect, field: &str) -> Result<i64> {
    required(object, field, "whole number", integer(object, field))
}

/// [`unsigned`], or a named refusal (see [`require_text`]).
pub fn require_unsigned(object: &dyn Reflect, field: &str) -> Result<u64> {
    required(object, field, "count", unsigned(object, field))
}

/// [`flag`], or a named refusal (see [`require_text`]).
pub fn require_flag(object: &dyn Reflect, field: &str) -> Result<bool> {
    required(object, field, "boolean", flag(object, field))
}

fn required<T>(object: &dyn Reflect, field: &str, expected: &'static str, read: Option<T>) -> Result<T> {
    if let Some(value) = read {
        return Ok(value);
    }
    let owner = object.type_info().name;
    match object.field(field) {
        None | Some(FieldValue::Null) => Err(Error::MissingRequired {
            kind: "field",
            name: field.to_string(),
            parent: owner.to_string(),
        }),
        // A blank string is a scalar that reads as absent.
        Some(FieldValue::Scalar(Scalar::Str(blank))) if expected == "text" && blank.trim().is_empty() => {
            Err(Error::MissingRequired {
                kind: "field",
                name: field.to_string(),
                parent: owner.to_string(),
            })
        }
        Some(other) => Err(Error::TypeMismatch {
            path: format!("{owner}.{field}"),
            expected: expected.to_string(),
            found: other.kind_name().to_string(),
        }),
    }
}

// ---------------------------------------------------------------------------
// Shared steps
// ---------------------------------------------------------------------------

fn scalar_text(scalar: Scalar<'_>) -> Option<String> {
    match scalar {
        Scalar::Str(value) if value.trim().is_empty() => None,
        Scalar::Str(value) => Some(value.to_string()),
        other => Some(other.to_string()),
    }
}

fn whole_f64_to_i64(value: f64) -> Option<i64> {
    // `i64::MAX as f64` rounds up to 2^63, so the upper bound is exclusive.
    (value.is_finite() && value.fract() == 0.0 && (i64::MIN as f64..i64::MAX as f64).contains(&value))
        .then_some(value as i64)
}
