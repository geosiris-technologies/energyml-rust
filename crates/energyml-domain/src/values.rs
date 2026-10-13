//! Any EnergyML value array, read the same way.
//!
//! # The two levels, and why both need covering
//!
//! **Below**: a pointer at external data - `Hdf5Dataset` in RESQML 2.0.1
//! (`PathInHdfFile` + a proxy reference), `ExternalDataArrayPart` in 2.2
//! (`PathInExternalFile` + a URI). The storage layer already resolves both and
//! reads them into typed values; nothing here repeats that.
//!
//! **Here**: the level above, where the schema says an array may be external -
//! *or* constant, *or* a lattice, *or* a range, *or* written inline in the XML,
//! *or* derived from another array. Twenty-odd concrete types across two
//! versions, and only about a third of them touch a file at all:
//!
//! | Form | Where the values are |
//! |---|---|
//! | `*ExternalArray` / `*Hdf5Array` | in the external file |
//! | `*ConstantArray` | one value and a count |
//! | `*LatticeArray` | a start and a set of steps - **computed** |
//! | `IntegerRangeArray` (2.0.1 only) | a start and a count |
//! | `*XmlArray` (2.2 only) | inline in the document |
//! | `BooleanArrayFromIndexArray` | from an integer array of indices |
//! | `IntegerArrayFromBooleanMaskArray` | from a boolean mask |
//!
//! A reader that only knows the external forms loses every constant and every
//! lattice, and loses them as **empty arrays** - which is indistinguishable
//! from a property that genuinely has no values. The corpus makes the point:
//! 30 `BooleanConstantArray` and 16 lattices against the file-backed ones.
//!
//! # Reading without widening
//!
//! [`read`] answers "give me the values" and returns them in the family the
//! schema declares. That is what most callers want, and it costs a widening
//! pass - an `int32` dataset comes back as `Vec<i64>`.
//!
//! When the widening is the expensive part - a vertex buffer, a property array
//! about to be uploaded to a GPU - [`read_native`] returns the values exactly
//! as they came out of the file, and [`read_into`] fills a buffer the caller
//! already owns without allocating at all. Both return `None` for a form that
//! has no file behind it, because there is nothing to optimise about a constant
//! array: its values do not exist anywhere until something computes them.
//!
//! # Naming across versions
//!
//! Forms are recognised by the **suffix** of their type name, which is stable:
//! `DoubleConstantArray` and `FloatingPointConstantArray` are the same shape
//! under two names, and so are `IntegerHdf5Array` and `IntegerExternalArray`.
//! The family comes from the prefix. One branch reads both versions.

use energyml_core::value::{FieldValue, Reflect, Scalar};

use crate::error::{Error, Result};
use crate::source::ArraySource;

pub use energyml_storage::array::{ArrayBuffer, ArrayData, ArrayMetadata, ArrayValues};

/// What kind of values an array holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// `Double*` in 2.0.1, `FloatingPoint*` in 2.2.
    Floating,
    /// `Integer*`, in both.
    Integer,
    /// `Boolean*`, in both.
    Boolean,
    /// `String*`, in both.
    Text,
}

/// The values of an array, in the family the schema declares.
#[derive(Clone, PartialEq, Debug)]
pub enum Values {
    /// Floating-point values, widened to `f64`.
    Floating(Vec<f64>),
    /// Integers, widened to `i64`.
    Integer(Vec<i64>),
    /// Booleans.
    Boolean(Vec<bool>),
    /// Strings.
    Text(Vec<String>),
}

impl Values {
    /// How many values there are.
    pub fn len(&self) -> usize {
        match self {
            Values::Floating(v) => v.len(),
            Values::Integer(v) => v.len(),
            Values::Boolean(v) => v.len(),
            Values::Text(v) => v.len(),
        }
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Which family these belong to.
    pub fn family(&self) -> Family {
        match self {
            Values::Floating(_) => Family::Floating,
            Values::Integer(_) => Family::Integer,
            Values::Boolean(_) => Family::Boolean,
            Values::Text(_) => Family::Text,
        }
    }

    /// The floating-point values, if that is what these are.
    pub fn floating(&self) -> Option<&[f64]> {
        match self {
            Values::Floating(v) => Some(v),
            _ => None,
        }
    }

    /// The integers, if that is what these are.
    pub fn integer(&self) -> Option<&[i64]> {
        match self {
            Values::Integer(v) => Some(v),
            _ => None,
        }
    }

    /// The booleans, if that is what these are.
    pub fn boolean(&self) -> Option<&[bool]> {
        match self {
            Values::Boolean(v) => Some(v),
            _ => None,
        }
    }

    /// The strings, if that is what these are.
    pub fn text(&self) -> Option<&[String]> {
        match self {
            Values::Text(v) => Some(v),
            _ => None,
        }
    }

    /// Everything as `f64`, whatever the family.
    ///
    /// Booleans become 0 and 1. Strings have no numeric reading and give
    /// `None` rather than a vector of zeros.
    pub fn to_f64(&self) -> Option<Vec<f64>> {
        match self {
            Values::Floating(v) => Some(v.clone()),
            Values::Integer(v) => Some(v.iter().map(|x| *x as f64).collect()),
            Values::Boolean(v) => Some(v.iter().map(|x| f64::from(*x)).collect()),
            Values::Text(_) => None,
        }
    }
}

/// The family an array belongs to, without reading anything.
///
/// `None` when the object is not a value array at all.
pub fn family(array: &dyn Reflect) -> Option<Family> {
    let name = array.type_info().name;
    if !name.ends_with("Array") {
        return None;
    }
    if name.starts_with("Boolean") {
        return Some(Family::Boolean);
    }
    if name.starts_with("Integer") {
        return Some(Family::Integer);
    }
    if name.starts_with("Double") || name.starts_with("FloatingPoint") {
        return Some(Family::Floating);
    }
    if name.starts_with("String") {
        return Some(Family::Text);
    }
    // Coordinate arrays. `Point3dHdf5Array` and `Point2dExternalArray` are
    // ordinary floating-point data with a shape convention on top - the
    // grouping into points belongs to whoever asked, not here. The forms that
    // *compute* their points (lattice, parametric, z-value) are geometry's
    // business and are not value arrays at all, so they fall through.
    if name.starts_with("Point") && (name.ends_with("ExternalArray") || name.ends_with("Hdf5Array")) {
        return Some(Family::Floating);
    }
    None
}

/// Whether the values of this array live in an external file.
///
/// What tells a caller whether [`read_native`] and [`read_into`] have anything
/// to offer: for every other form the values are computed, and there is no
/// buffer to avoid copying.
pub fn is_external(array: &dyn Reflect) -> bool {
    let name = array.type_info().name;
    name.ends_with("ExternalArray") || name.ends_with("Hdf5Array")
}

/// One value array found inside an object, and where it sits.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Located {
    /// Dotted path from the object root, in the dialect `path::get` reads:
    /// `PatchOfValues[0].Values`.
    pub path: String,
    /// The concrete form, verbatim: `DoubleConstantArray`, `IntegerHdf5Array`.
    pub form: &'static str,
    /// Which family the values belong to - what a viewer formats by.
    pub family: Family,
    /// Whether its values live in a file - see [`is_external`].
    pub external: bool,
}

/// Every value array inside an object, with its path.
///
/// # Why this is not the same question as "where are the files"
///
/// `EpcStore::arrays_of` answers *where do the bytes live*, and it finds only
/// the file-backed forms - which is right for what it is for. But an array is a
/// thing in its own right whether or not it touches a file: a
/// `ContinuousProperty` whose values are a `DoubleConstantArray` has values, has
/// a count, and has nothing external at all. A viewer built on the file walk
/// alone reports that property as having **no data**, which is
/// indistinguishable from one that genuinely has none. The corpus makes the
/// point on its own: 30 `BooleanConstantArray` and 16 lattices.
///
/// So this walk finds the arrays, and the file walk finds their bytes. The two
/// answer different questions and a caller showing values needs both.
///
/// # It does not descend into one
///
/// An array's insides - its `Count`, its `HdfProxy`, the integer array a mask
/// is built from - are its own business, and listing them beside it would offer
/// a person the parts of an answer as though they were answers.
pub fn locate(object: &dyn Reflect) -> Vec<Located> {
    let mut out = Vec::new();
    locate_in(object, String::new(), &mut out, 0);
    out
}

fn locate_in(object: &dyn Reflect, path: String, out: &mut Vec<Located>, depth: u32) {
    // Objects reference each other only through DORs, never by containment, so
    // the tree is finite. The bound is a seatbelt, not a design assumption.
    if depth > 64 {
        return;
    }
    if let Some(family) = family(object) {
        // Not at the root: an array asked about itself is the answer, not a
        // thing found inside one.
        if !path.is_empty() {
            out.push(Located {
                path,
                form: object.type_info().name,
                family,
                external: is_external(object),
            });
            return;
        }
    }
    for field in object.type_info().fields {
        let Some(value) = object.field(field.name) else {
            continue;
        };
        let child = if path.is_empty() {
            field.xml_name.to_string()
        } else {
            format!("{path}.{}", field.xml_name)
        };
        locate_value(&value, child, out, depth);
    }
}

fn locate_value(value: &FieldValue<'_>, path: String, out: &mut Vec<Located>, depth: u32) {
    match value {
        FieldValue::Object(child) => locate_in(*child, path, out, depth + 1),
        FieldValue::List(items) => {
            for (at, item) in items.iter().enumerate() {
                locate_value(item, format!("{path}[{at}]"), out, depth);
            }
        }
        _ => {}
    }
}

/// The values of any value array.
///
/// Works for every form - external, constant, lattice, range, inline, derived.
/// A form this build does not evaluate is an error naming it, never an empty
/// vector.
pub fn read(array: &dyn Reflect, source: &dyn ArraySource, at: &str) -> Result<Values> {
    let name = array.type_info().name;
    let Some(family) = family(array) else {
        return Err(Error::unsupported(
            format!("`{name}` is not a value array"),
            at.to_string(),
        ));
    };

    // The file-backed forms, in both spellings.
    if is_external(array) {
        let data = native(array, source, at)?
            .ok_or_else(|| Error::malformed(at, format!("`{name}` declares no external data at all")))?;
        return Ok(widen(data.values, family));
    }

    // A single value repeated, which the corpus uses more than any other
    // computed form.
    if name.ends_with("ConstantArray") {
        let count = scalar_i64(array, "Count").unwrap_or(0).max(0) as usize;
        return Ok(match family {
            Family::Floating => Values::Floating(vec![
                scalar_f64(array, "Value")
                    .ok_or_else(|| missing(at, "Value"))?;
                count
            ]),
            Family::Integer => Values::Integer(vec![
                scalar_i64(array, "Value")
                    .ok_or_else(|| missing(at, "Value"))?;
                count
            ]),
            Family::Boolean => Values::Boolean(vec![flag(array, "Value").unwrap_or(false); count]),
            Family::Text => Values::Text(vec![text(array, "Value").unwrap_or_default(); count]),
        });
    }

    // Start plus steps. Each dimension multiplies what is already there - that
    // is what makes a lattice a lattice - and the one being added is innermost,
    // matching the slowest-first order the point lattices use. The two describe
    // the same grid from different ends, and disagreeing would put values on
    // the wrong nodes.
    if name.ends_with("LatticeArray") {
        return lattice(array, family, at);
    }

    // 2.0.1 only: consecutive integers from a start.
    if name.ends_with("RangeArray") {
        let start = scalar_i64(array, "Value").ok_or_else(|| missing(at, "Value"))?;
        let count = scalar_i64(array, "Count").unwrap_or(0).max(0);
        return Ok(Values::Integer((0..count).map(|i| start + i).collect()));
    }

    // 2.2 only: the values are in the document.
    if name.ends_with("XmlArray") {
        return Ok(match family {
            Family::Floating => Values::Floating(list_f64(array, "Values")),
            Family::Integer => Values::Integer(list_i64(array, "Values")),
            Family::Boolean => Values::Boolean(list_bool(array, "Values")),
            Family::Text => Values::Text(list_text(array, "Values")),
        });
    }

    // A boolean array written as the indices that are true (or false).
    if name == "BooleanArrayFromIndexArray" {
        let count = scalar_i64(array, "Count").unwrap_or(0).max(0) as usize;
        let listed = flag(array, "IndexIsTrue").unwrap_or(true);
        let mut values = vec![!listed; count];
        if let Some(indices) = field_object(array, "Indices") {
            for index in read(indices, source, &format!("{at}.Indices"))?
                .integer()
                .unwrap_or_default()
            {
                let index = *index;
                if index < 0 || index as usize >= count {
                    return Err(Error::malformed(
                        at,
                        format!("index {index} is outside the {count} value(s) declared"),
                    ));
                }
                values[index as usize] = listed;
            }
        }
        return Ok(Values::Boolean(values));
    }

    // The mirror image: the indices at which a mask is true.
    if name == "IntegerArrayFromBooleanMaskArray" {
        let Some(mask) = field_object(array, "Mask") else {
            return Err(missing(at, "Mask"));
        };
        let mask = read(mask, source, &format!("{at}.Mask"))?;
        let mask = mask
            .boolean()
            .ok_or_else(|| Error::malformed(at, "the mask is not a boolean array"))?;
        return Ok(Values::Integer(
            mask.iter()
                .enumerate()
                .filter(|(_, set)| **set)
                .map(|(index, _)| index as i64)
                .collect(),
        ));
    }

    // Its values come from a whole other data object, whose own array has to be
    // located and read. Named rather than guessed: returning nothing here would
    // look exactly like a property with no values.
    if name == "BooleanArrayFromDiscretePropertyArray" {
        return Err(Error::unsupported(
            "`BooleanArrayFromDiscretePropertyArray`, whose values come from another property object",
            at.to_string(),
        ));
    }

    Err(Error::unsupported(format!("`{name}`"), at.to_string()))
}

/// The values exactly as they came out of the file, without widening.
///
/// `None` for a form whose values are computed rather than stored - there is
/// nothing to read natively about a constant array.
///
/// The point of this over [`read`]: an `int32` property of ten million values
/// is forty megabytes here and eighty through `read`, and a caller uploading it
/// to a GPU wants the former.
pub fn read_native(array: &dyn Reflect, source: &dyn ArraySource, at: &str) -> Result<Option<ArrayData>> {
    if !is_external(array) {
        return Ok(None);
    }
    native(array, source, at)
}

/// Decode straight into a buffer the caller already owns.
///
/// **The zero-allocation path.** `None` when the array is not file-backed, and
/// when the array has more than one part - a concatenation has to be assembled
/// somewhere, and doing it silently into a caller's buffer would hide the copy
/// this method exists to avoid.
pub fn read_into(
    array: &dyn Reflect,
    source: &dyn ArraySource,
    at: &str,
    out: &mut ArrayBuffer<'_>,
) -> Result<Option<ArrayMetadata>> {
    if !is_external(array) {
        return Ok(None);
    }
    let parts = source.locate(array);
    match parts.len() {
        0 => Ok(None),
        1 => Ok(Some(source.read_into(&parts[0], out)?)),
        n => Err(Error::malformed(
            at,
            format!("{n} external parts: read them with `read_native`, which joins them"),
        )),
    }
}

/// How many values the array holds, reading as little as possible.
///
/// A constant, a range or a lattice says its own size; a file-backed array
/// needs the dataset's metadata, which is a header read rather than a data
/// read.
pub fn count(array: &dyn Reflect, source: &dyn ArraySource, at: &str) -> Result<usize> {
    let name = array.type_info().name;
    if name.ends_with("ConstantArray") || name.ends_with("RangeArray") {
        return Ok(scalar_i64(array, "Count").unwrap_or(0).max(0) as usize);
    }
    if is_external(array) {
        let mut total = 0usize;
        for location in source.locate(array) {
            let metadata = source.metadata(&location)?;
            total += metadata.dimensions.iter().product::<i64>().max(0) as usize;
        }
        return Ok(total);
    }
    // Everything else is cheap enough to evaluate, and evaluating it is the
    // only way to be right about a lattice or a mask.
    Ok(read(array, source, at)?.len())
}

/// A jagged array: rows of differing length, flattened with a cumulative index.
///
/// `ResqmlJaggedArray` in 2.0.1, `JaggedArray` in eml 2.3 - the same two fields
/// under two names. The corpus holds 17 of them.
pub fn jagged(array: &dyn Reflect, source: &dyn ArraySource, at: &str) -> Result<Vec<Values>> {
    let Some(elements) = field_object(array, "Elements") else {
        return Err(missing(at, "Elements"));
    };
    let Some(lengths) = field_object(array, "CumulativeLength") else {
        return Err(missing(at, "CumulativeLength"));
    };
    let flat = read(elements, source, &format!("{at}.Elements"))?;
    let bounds = read(lengths, source, &format!("{at}.CumulativeLength"))?;
    let bounds = bounds
        .integer()
        .ok_or_else(|| Error::malformed(at, "CumulativeLength is not an integer array"))?;

    let mut rows = Vec::with_capacity(bounds.len());
    let mut start = 0usize;
    for bound in bounds {
        let end = (*bound).max(0) as usize;
        if end < start || end > flat.len() {
            return Err(Error::malformed(
                at,
                format!(
                    "the cumulative length {end} does not fit the {} element(s) it indexes",
                    flat.len()
                ),
            ));
        }
        rows.push(slice(&flat, start, end));
        start = end;
    }
    Ok(rows)
}

// ---------------------------------------------------------------------------

/// Read every part of a file-backed array and join them.
///
/// eml 2.3 changed the shape here: 2.0.1 had one dataset path per array, while
/// an `ExternalDataArray` holds **a list of** `ExternalDataArrayPart`, each a
/// hyperslab of some file, together forming one logical array. Every array in
/// the reference corpus has exactly one part, so the joining path is
/// exercised by construction rather than by the corpus - worth knowing when
/// reading a file that uses the feature the schema was changed to allow.
fn native(array: &dyn Reflect, source: &dyn ArraySource, at: &str) -> Result<Option<ArrayData>> {
    let parts = source.locate(array);
    let mut found: Option<ArrayData> = None;
    for location in &parts {
        let piece = source.read(location)?;
        found = Some(match found {
            None => piece,
            Some(mut joined) => {
                if joined.values.element_type() != piece.values.element_type() {
                    return Err(Error::malformed(
                        at,
                        format!(
                            "the parts of this array disagree on their element type: \
                             `{}` then `{}`",
                            joined.values.element_type(),
                            piece.values.element_type()
                        ),
                    ));
                }
                append(&mut joined.values, piece.values);
                // A joined array is one long run: the shapes of the parts
                // describe hyperslabs, not the whole, and adding them up would
                // be inventing a shape nothing declared.
                joined.metadata.dimensions = vec![joined.values.len() as i64];
                joined
            }
        });
    }
    Ok(found)
}

fn append(into: &mut ArrayValues, from: ArrayValues) {
    match (into, from) {
        (ArrayValues::F64(a), ArrayValues::F64(b)) => a.extend(b),
        (ArrayValues::F32(a), ArrayValues::F32(b)) => a.extend(b),
        (ArrayValues::I64(a), ArrayValues::I64(b)) => a.extend(b),
        (ArrayValues::I32(a), ArrayValues::I32(b)) => a.extend(b),
        (ArrayValues::I16(a), ArrayValues::I16(b)) => a.extend(b),
        (ArrayValues::I8(a), ArrayValues::I8(b)) => a.extend(b),
        (ArrayValues::U64(a), ArrayValues::U64(b)) => a.extend(b),
        (ArrayValues::U32(a), ArrayValues::U32(b)) => a.extend(b),
        (ArrayValues::U16(a), ArrayValues::U16(b)) => a.extend(b),
        (ArrayValues::U8(a), ArrayValues::U8(b)) => a.extend(b),
        // Unreachable: the element types were compared before this was called.
        _ => {}
    }
}

/// Bring a file's numbers into the family the schema declares.
///
/// HDF5 has no boolean type - RESQML writes them as bytes - so a boolean array
/// read from a file arrives as integers and is compared against zero here.
fn widen(values: ArrayValues, family: Family) -> Values {
    match family {
        Family::Boolean => Values::Boolean(numbers(&values).into_iter().map(|v| v != 0.0).collect()),
        Family::Integer => Values::Integer(numbers(&values).into_iter().map(|v| v as i64).collect()),
        Family::Floating => Values::Floating(numbers(&values)),
        // Nothing in the corpus writes strings to HDF5, and the backend does
        // not decode them; an empty text array is the honest answer rather
        // than numbers pretending to be words.
        Family::Text => Values::Text(Vec::new()),
    }
}

fn numbers(values: &ArrayValues) -> Vec<f64> {
    match values {
        ArrayValues::F64(v) => v.clone(),
        ArrayValues::F32(v) => v.iter().map(|x| *x as f64).collect(),
        ArrayValues::I64(v) => v.iter().map(|x| *x as f64).collect(),
        ArrayValues::I32(v) => v.iter().map(|x| *x as f64).collect(),
        ArrayValues::I16(v) => v.iter().map(|x| *x as f64).collect(),
        ArrayValues::I8(v) => v.iter().map(|x| *x as f64).collect(),
        ArrayValues::U64(v) => v.iter().map(|x| *x as f64).collect(),
        ArrayValues::U32(v) => v.iter().map(|x| *x as f64).collect(),
        ArrayValues::U16(v) => v.iter().map(|x| *x as f64).collect(),
        ArrayValues::U8(v) => v.iter().map(|x| *x as f64).collect(),
        _ => Vec::new(),
    }
}

fn lattice(array: &dyn Reflect, family: Family, at: &str) -> Result<Values> {
    let steps: Vec<(f64, i64)> = match array.field("Offset") {
        Some(FieldValue::List(offsets)) => offsets
            .iter()
            .filter_map(|offset| offset.as_object())
            .map(|offset| {
                (
                    scalar_f64(offset, "Value").unwrap_or(0.0),
                    scalar_i64(offset, "Count").unwrap_or(0).max(0),
                )
            })
            .collect(),
        Some(FieldValue::Object(offset)) => vec![(
            scalar_f64(offset, "Value").unwrap_or(0.0),
            scalar_i64(offset, "Count").unwrap_or(0).max(0),
        )],
        _ => Vec::new(),
    };
    let start = scalar_f64(array, "StartValue").ok_or_else(|| missing(at, "StartValue"))?;

    let mut values = vec![start];
    for (step, count) in steps {
        let outer = std::mem::take(&mut values);
        values.reserve(outer.len() * (count as usize + 1));
        for value in &outer {
            for i in 0..=count {
                values.push(value + step * i as f64);
            }
        }
    }
    Ok(match family {
        Family::Integer => Values::Integer(values.into_iter().map(|v| v as i64).collect()),
        _ => Values::Floating(values),
    })
}

fn slice(values: &Values, start: usize, end: usize) -> Values {
    match values {
        Values::Floating(v) => Values::Floating(v[start..end].to_vec()),
        Values::Integer(v) => Values::Integer(v[start..end].to_vec()),
        Values::Boolean(v) => Values::Boolean(v[start..end].to_vec()),
        Values::Text(v) => Values::Text(v[start..end].to_vec()),
    }
}

fn field_object<'a>(object: &'a dyn Reflect, field: &str) -> Option<&'a dyn Reflect> {
    match object.field(field)? {
        FieldValue::Object(child) => Some(child),
        _ => None,
    }
}

fn scalar_f64(object: &dyn Reflect, field: &str) -> Option<f64> {
    match object.field(field)? {
        FieldValue::Scalar(Scalar::Double(v)) => Some(v),
        FieldValue::Scalar(Scalar::Int(v)) => Some(v as f64),
        FieldValue::Scalar(Scalar::UInt(v)) => Some(v as f64),
        _ => None,
    }
}

fn scalar_i64(object: &dyn Reflect, field: &str) -> Option<i64> {
    match object.field(field)? {
        FieldValue::Scalar(Scalar::Int(v)) => Some(v),
        FieldValue::Scalar(Scalar::UInt(v)) => i64::try_from(v).ok(),
        FieldValue::Scalar(Scalar::Double(v)) => Some(v as i64),
        _ => None,
    }
}

fn flag(object: &dyn Reflect, field: &str) -> Option<bool> {
    match object.field(field)? {
        FieldValue::Scalar(Scalar::Bool(v)) => Some(v),
        _ => None,
    }
}

fn text(object: &dyn Reflect, field: &str) -> Option<String> {
    match object.field(field)? {
        FieldValue::Scalar(Scalar::Str(v)) => Some(v.to_string()),
        _ => None,
    }
}

/// The items of a list-bound field, whether the schema spelled it as one
/// element holding many values or as many elements.
fn items<'a>(object: &'a dyn Reflect, field: &str) -> Vec<FieldValue<'a>> {
    match object.field(field) {
        Some(FieldValue::List(list)) => list,
        Some(single) => vec![single],
        None => Vec::new(),
    }
}

fn list_f64(object: &dyn Reflect, field: &str) -> Vec<f64> {
    items(object, field)
        .iter()
        .filter_map(|item| match item {
            FieldValue::Scalar(Scalar::Double(v)) => Some(*v),
            FieldValue::Scalar(Scalar::Int(v)) => Some(*v as f64),
            FieldValue::Scalar(Scalar::UInt(v)) => Some(*v as f64),
            _ => None,
        })
        .collect()
}

fn list_i64(object: &dyn Reflect, field: &str) -> Vec<i64> {
    items(object, field)
        .iter()
        .filter_map(|item| match item {
            FieldValue::Scalar(Scalar::Int(v)) => Some(*v),
            FieldValue::Scalar(Scalar::UInt(v)) => i64::try_from(*v).ok(),
            FieldValue::Scalar(Scalar::Double(v)) => Some(*v as i64),
            _ => None,
        })
        .collect()
}

fn list_bool(object: &dyn Reflect, field: &str) -> Vec<bool> {
    items(object, field)
        .iter()
        .filter_map(|item| match item {
            FieldValue::Scalar(Scalar::Bool(v)) => Some(*v),
            _ => None,
        })
        .collect()
}

fn list_text(object: &dyn Reflect, field: &str) -> Vec<String> {
    items(object, field)
        .iter()
        .filter_map(|item| match item {
            FieldValue::Scalar(Scalar::Str(v)) => Some(v.to_string()),
            _ => None,
        })
        .collect()
}

fn missing(at: &str, field: &str) -> Error {
    Error::malformed(at, format!("no `{field}`"))
}
