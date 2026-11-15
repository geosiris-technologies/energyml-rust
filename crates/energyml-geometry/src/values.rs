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

use energyml_core::value::Reflect;
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
        other => {
            let numbers = other.to_f64().ok_or_else(|| {
                Error::malformed(
                    at,
                    format!("`{}` holds text, not numbers", array.type_info().name),
                )
            })?;
            // `NaN as i64` is 0: an index that is no number would silently
            // become the first one (`[geometry G15]`).
            if let Some(bad) = numbers.iter().find(|x| !x.is_finite()) {
                return Err(Error::malformed(
                    at,
                    format!("`{}` holds {bad}: not an integer", array.type_info().name),
                ));
            }
            Ok(numbers.into_iter().map(|x| x as i64).collect())
        }
    }
}

/// The flags of a boolean array, as the file states them.
///
/// An array that holds text is refused by name: it is no set of flags, and an
/// empty answer would read as *the file said nothing*, which it did not.
/// Anything numeric is read as zero-or-not, because a producer writing a mask
/// as integers has still said which is which.
pub fn read_flags(array: &dyn Reflect, source: &dyn ValueSource, at: &str) -> Result<Vec<bool>> {
    match values::read(array, source, at)? {
        values::Values::Boolean(flags) => Ok(flags),
        // A text array is no set of flags: refused by name rather than read as
        // an empty list that states nothing (`[geometry G15]`).
        other => other
            .to_f64()
            .map(|v| v.into_iter().map(|x| x != 0.0).collect())
            .ok_or_else(|| {
                Error::malformed(at, format!("`{}` holds text, not flags", array.type_info().name))
            }),
    }
}

/// How many values an array holds, reading as little as possible.
pub fn count(array: &dyn Reflect, source: &dyn ValueSource, at: &str) -> Result<usize> {
    Ok(values::count(array, source, at)?)
}

/// The readers of a numeric field are the door's (ADR 0046): a double is an
/// integer only when it is whole, and a string is never a number.
pub(crate) use energyml_core::reflect::fields::{integer as scalar_i64, number as scalar_f64};
