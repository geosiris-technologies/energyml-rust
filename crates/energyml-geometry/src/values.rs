//! Numeric arrays, as geometry wants them.
//!
//! The reading itself is [`energyml_domain::values`], which covers every form
//! in every family - floating point, integer, boolean and string, external or
//! constant or lattice or range or inline or derived. This module is the thin
//! layer that gives geometry the two shapes it actually uses: a `Vec<f64>` of
//! coordinates and a `Vec<i64>` of indices.
//!
//! The widening is deliberate *here*: an array reached through this path is
//! about to be mixed with coordinates or handed to a renderer, and both want
//! one type. Code that needs the file's own type - a property array on its way
//! to a GPU - should call [`energyml_domain::values::read_native`] or
//! [`energyml_domain::values::read_into`], which do not widen and do not
//! allocate.

use energyml_core::value::{FieldValue, Reflect, Scalar};
use energyml_domain::values;

use crate::error::{Error, Result};
use crate::source::ValueSource;

/// Every value of a numeric array, as `f64`.
pub fn read_doubles(array: &dyn Reflect, source: &dyn ValueSource, at: &str) -> Result<Vec<f64>> {
    let values = values::read(array, source, at)?;
    values.to_f64().ok_or_else(|| {
        Error::malformed(
            at,
            format!("`{}` holds text, not numbers", array.type_info().name),
        )
    })
}

/// Every value of an integer array, as `i64`.
pub fn read_integers(array: &dyn Reflect, source: &dyn ValueSource, at: &str) -> Result<Vec<i64>> {
    match values::read(array, source, at)? {
        values::Values::Integer(v) => Ok(v),
        // A count written as a double, or a mask read as booleans: both are
        // indices once they get here, and refusing them would help nobody.
        other => other
            .to_f64()
            .map(|v| v.into_iter().map(|x| x as i64).collect())
            .ok_or_else(|| {
                Error::malformed(
                    at,
                    format!("`{}` holds text, not numbers", array.type_info().name),
                )
            }),
    }
}

/// The flags of a boolean array, as the file states them.
///
/// Empty where the array holds something that is not a set of flags: the
/// caller then knows the file said nothing it could use, which is **not** the
/// same as *all false*. Anything numeric is read as zero-or-not, because a
/// producer writing a mask as integers has still said which is which.
pub fn read_flags(array: &dyn Reflect, source: &dyn ValueSource, at: &str) -> Result<Vec<bool>> {
    match values::read(array, source, at)? {
        values::Values::Boolean(flags) => Ok(flags),
        other => Ok(other
            .to_f64()
            .map(|v| v.into_iter().map(|x| x != 0.0).collect())
            .unwrap_or_default()),
    }
}

/// How many values an array holds, reading as little as possible.
pub fn count(array: &dyn Reflect, source: &dyn ValueSource, at: &str) -> Result<usize> {
    Ok(values::count(array, source, at)?)
}

pub(crate) fn scalar_f64(object: &dyn Reflect, field: &str) -> Option<f64> {
    match object.field(field)? {
        FieldValue::Scalar(Scalar::Double(v)) => Some(v),
        FieldValue::Scalar(Scalar::Int(v)) => Some(v as f64),
        FieldValue::Scalar(Scalar::UInt(v)) => Some(v as f64),
        _ => None,
    }
}

pub(crate) fn scalar_i64(object: &dyn Reflect, field: &str) -> Option<i64> {
    match object.field(field)? {
        FieldValue::Scalar(Scalar::Int(v)) => Some(v),
        FieldValue::Scalar(Scalar::UInt(v)) => i64::try_from(v).ok(),
        FieldValue::Scalar(Scalar::Double(v)) => Some(v as i64),
        _ => None,
    }
}
