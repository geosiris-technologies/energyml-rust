//! Reading a measured value without knowing its type.
//!
//! Every Energistics measure has the same shape, in every version:
//!
//! ```xml
//! <ArealRotation uom="rad">0.0</ArealRotation>
//! ```
//!
//! and generates the same Rust shape - a text-bound number plus a `uom`
//! attribute:
//!
//! ```ignore
//! pub struct PlaneAngleMeasure {
//!     #[energyml(text)]           pub value: f64,
//!     #[energyml(attr, xml_name = "uom")] pub uom: PlaneAngleUom,
//! }
//! ```
//!
//! What differs is the *type*: about 190 measure structs per version, each with
//! its own unit enum, none sharing a trait - roughly 730 unrelated types across
//! the four common versions. Anything wanting "the number and its unit" would
//! otherwise match on concrete types, once per version, forever.
//!
//! # Why the unit comes back as a string
//!
//! Because the Rust variant **is not stable across versions** and the wire
//! value is:
//!
//! | | eml 2.0 | eml 2.1 / 2.2 / 2.3 |
//! |---|---|---|
//! | `LengthUom::Mm` | megametre | **millimetre** |
//! | `LengthUom::Mm2` | millimetre | **megametre** |
//!
//! Six unit pairs flipped that way when the generator's case-disambiguation
//! changed (`MA`/`mA`, `MW`/`mW`, `MWb`/`mWb`, `Mcal[th]`/`mcal[th]`,
//! `dB.MW`/`dB.mW`). Code that matches `LengthUom::Mm` reads a length off by
//! **10⁹** on one version and is correct on the other, silently.
//!
//! The wire values - `m`, `ft`, `rad`, `dega` - did not change: 1500 unit
//! strings in eml 2.0, 1540 in eml 2.3, purely additive, nothing removed and
//! nothing respelled. So this module reads the *string* and never the variant,
//! and so must everything downstream.
//!
//! This module knows no unit names and converts nothing - that is schema
//! vocabulary and belongs above the core. It only knows the *shape*.

use crate::reflect::Binding;
use crate::value::{FieldValue, Reflect};

/// A number and the unit it was written in.
#[derive(Clone, Debug, PartialEq)]
pub struct Measure {
    /// The text content of the element.
    pub value: f64,
    /// The `uom` attribute, exactly as written.
    ///
    /// `None` when the element has none - some measures make it optional, and
    /// an absent unit is not the same as an unknown one.
    pub uom: Option<String>,
}

/// Read any measure-shaped object.
///
/// `None` when the object is not a measure: no text-bound field, or one that
/// does not hold a number.
///
/// ```ignore
/// let rotation = measure::read(crs.field_object("ArealRotation")?)?;
/// assert_eq!(rotation.uom.as_deref(), Some("rad"));
/// ```
pub fn read(object: &dyn Reflect) -> Option<Measure> {
    let value = text_value(object)?;
    Some(Measure {
        value,
        uom: unit_of(object),
    })
}

/// The text content of an element, as a number.
///
/// Found through the field whose binding says it is the text, never by
/// guessing at a name: the generator calls it `value`, but that is its
/// convention rather than a rule of the schema.
pub fn text_value(object: &dyn Reflect) -> Option<f64> {
    let text = object
        .type_info()
        .fields
        .iter()
        .find(|field| matches!(field.binding, Binding::Text))?;
    match object.field(text.xml_name).or_else(|| object.field(text.name))? {
        FieldValue::Scalar(scalar) => scalar.as_f64(),
        _ => None,
    }
}

/// The `uom` attribute of an element, as written.
///
/// Every version spells the attribute `uom`; only the *enum* behind it changes,
/// and the enum is exactly what must not be matched on.
pub fn unit_of(object: &dyn Reflect) -> Option<String> {
    let unit = object
        .type_info()
        .fields
        .iter()
        .find(|field| matches!(field.binding, Binding::Attribute) && field.xml_name == "uom")?;
    match object.field(unit.xml_name).or_else(|| object.field(unit.name))? {
        FieldValue::Scalar(scalar) => scalar.as_str().map(str::to_string),
        _ => None,
    }
}

/// The value of an enumeration-typed element, as written.
///
/// The same argument as [`unit_of`]: an element like `<Uom>ft</Uom>` or
/// `<Direction>down</Direction>` carries schema vocabulary whose Rust variant
/// name is a generator artefact. Reading the lexical value keeps the caller
/// honest across versions.
pub fn enum_value(object: &dyn Reflect, field: &str) -> Option<String> {
    match object.field(field)? {
        FieldValue::Scalar(scalar) => scalar.as_str().map(str::to_string),
        FieldValue::Object(child) => text_of(child),
        _ => None,
    }
}

fn text_of(object: &dyn Reflect) -> Option<String> {
    let text = object
        .type_info()
        .fields
        .iter()
        .find(|field| matches!(field.binding, Binding::Text))?;
    match object.field(text.xml_name).or_else(|| object.field(text.name))? {
        FieldValue::Scalar(scalar) => scalar.as_str().map(str::to_string),
        _ => None,
    }
}
