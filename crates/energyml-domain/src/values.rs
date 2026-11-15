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

use energyml_core::value::{FieldValue, Reflect};

use crate::error::{Error, Result};
use crate::property::shape::{NarrowedPick, PropertyShape, StoredLayout};
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
        return widen(data.values, family, at);
    }

    // A single value repeated, which the corpus uses more than any other
    // computed form.
    //
    // A missing `Value` or `Count` is a refusal for every family: an empty
    // array, a `false` or an empty string would look like data (ADR 0005).
    if name.ends_with("ConstantArray") {
        let count = declared_count(array, at)?;
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
            Family::Boolean => Values::Boolean(vec![
                flag(array, "Value")
                    .ok_or_else(|| missing(at, "Value"))?;
                count
            ]),
            // `texts` rather than `text`: an empty string is a value of a
            // string constant, where `text` reads a blank as absent.
            Family::Text => Values::Text(vec![
                list_text(array, "Value")
                    .into_iter()
                    .next()
                    .ok_or_else(|| missing(at, "Value"))?;
                count
            ]),
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
        let count = declared_count(array, at)?;
        // `start + count` must stay an `i64`: no index of the range may wrap.
        let end = start.checked_add(count as i64).ok_or_else(|| {
            Error::malformed(
                at,
                format!("a range of {count} from {start} overflows a 64-bit integer"),
            )
        })?;
        return Ok(Values::Integer((start..end).collect()));
    }

    // 2.2 only: the values are in the document. An element of the wrong type is
    // a refusal, never skipped: skipping shortens the array and shifts every
    // value after it.
    if name.ends_with("XmlArray") {
        return Ok(match family {
            Family::Floating => Values::Floating(inline(array, at, "a number", list_f64)?),
            Family::Integer => Values::Integer(inline(array, at, "a whole number", list_i64)?),
            Family::Boolean => Values::Boolean(inline(array, at, "a boolean", list_bool)?),
            Family::Text => Values::Text(inline(array, at, "a string", list_text)?),
        });
    }

    // A boolean array written as the indices that are true (or false).
    if name == "BooleanArrayFromIndexArray" {
        let count = declared_count(array, at)?;
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
        return declared_count(array, at);
    }
    if is_external(array) {
        // The window each part designates, the one `stored_dims` and
        // `read_block` honour: counting the whole dataset of a part that names a
        // slice of it would disagree with what is read.
        let too_many = || Error::malformed(at, "the parts hold more values than a count can express");
        let mut total = 0u64;
        for location in source.locate(array) {
            let metadata = source.metadata(&location)?;
            let (_, counts) = window(&location, &metadata.dimensions, at)?;
            let part = counts
                .iter()
                .try_fold(1u64, |product, c| product.checked_mul(*c))
                .ok_or_else(too_many)?;
            total = total.checked_add(part).ok_or_else(too_many)?;
        }
        return usize::try_from(total).map_err(|_| too_many());
    }
    // Everything else is cheap enough to evaluate, and evaluating it is the
    // only way to be right about a lattice or a mask.
    Ok(read(array, source, at)?.len())
}

// ---------------------------------------------------------------------------
// Reading a block
// ---------------------------------------------------------------------------

/// A block of an array, per **stored** dimension (what `ArraySource::metadata`
/// gives). Empty `start` and `count`: the whole array.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Block {
    /// Where the block starts, per stored dimension, slowest first.
    pub start: Vec<u64>,
    /// How much it holds, per stored dimension, slowest first.
    pub count: Vec<u64>,
}

/// The stored dimensions of an array, reading no value: the dataset's header
/// for a file-backed form, the computed shape otherwise (`[len]`; a lattice's
/// `[count + 1, ...]`, slowest first).
pub fn stored_dims(array: &dyn Reflect, source: &dyn ArraySource, at: &str) -> Result<Vec<u64>> {
    if is_external(array) {
        let (location, dimensions) = single_part(array, source, at)?;
        return Ok(window(&location, &dimensions, at)?.1);
    }
    match lattice_dims(array, at)? {
        Some(dims) => Ok(dims),
        None => Ok(vec![read(array, source, at)?.len() as u64]),
    }
}

/// The values of one block of an array, in the family the schema declares.
///
/// A file-backed array is read by the window of the block alone: a source that
/// answers with more than the block is refused by name, never trusted. A
/// computed array is evaluated and cut.
pub fn read_block(array: &dyn Reflect, source: &dyn ArraySource, at: &str, block: &Block) -> Result<Values> {
    let name = array.type_info().name;
    let Some(family) = family(array) else {
        return Err(Error::unsupported(
            format!("`{name}` is not a value array"),
            at.to_string(),
        ));
    };
    if block.start.is_empty() && block.count.is_empty() {
        return read(array, source, at);
    }
    if block.start.len() != block.count.len() {
        return Err(Error::unsupported(
            "a block whose start and count differ in rank",
            at.to_string(),
        ));
    }

    if is_external(array) {
        let (location, dimensions) = single_part(array, source, at)?;
        let (base_start, base_count) = window(&location, &dimensions, at)?;
        check_bounds(block, &base_count, at)?;
        let mut part = location.clone();
        part.start = base_start
            .iter()
            .zip(&block.start)
            .map(|(base, start)| (base + start) as i64)
            .collect();
        part.count = block.count.iter().map(|c| *c as i64).collect();
        let data = source.read(&part)?;
        let wanted: u64 = block.count.iter().product();
        if data.values.len() as u64 != wanted {
            return Err(Error::malformed(
                at,
                format!(
                    "the source returned {} value(s) for a block of {wanted}: it does not read by block",
                    data.values.len()
                ),
            ));
        }
        return widen(data.values, family, at);
    }

    // A computed form: evaluate, then cut like a stored one.
    let values = read(array, source, at)?;
    let dims = lattice_dims(array, at)?.unwrap_or_else(|| vec![values.len() as u64]);
    check_bounds(block, &dims, at)?;
    Ok(cut(&values, &dims, block))
}

/// The one external part of an array, and the dimensions its dataset holds.
fn single_part(
    array: &dyn Reflect,
    source: &dyn ArraySource,
    at: &str,
) -> Result<(energyml_storage::array::ArrayLocation, Vec<i64>)> {
    let name = array.type_info().name;
    let mut parts = source.locate(array);
    match parts.len() {
        0 => Err(Error::malformed(
            at,
            format!("`{name}` declares no external data at all"),
        )),
        1 => {
            let location = parts.remove(0);
            let dimensions = source.metadata(&location)?.dimensions;
            Ok((location, dimensions))
        }
        n => Err(Error::unsupported(
            format!("a block of an array in {n} external parts"),
            at.to_string(),
        )),
    }
}

/// The window a part already designates in its dataset, as `(start, count)`
/// per dataset dimension.
///
/// An eml 2.3 `ExternalDataArrayPart` carries a `StartIndex` and a `Count`
/// (EML TRG 2.3, `ExternalDataArrayPart`): a window in the dataset, and a block
/// asked of the array is relative to that window. eml 2.1 and 2.2 never carry
/// one. A one-dimensional count over a dataset of more dimensions is the case
/// the storage layer reads whole (the whole dataset is meant).
fn window(
    location: &energyml_storage::array::ArrayLocation,
    dimensions: &[i64],
    at: &str,
) -> Result<(Vec<u64>, Vec<u64>)> {
    let dims: Vec<u64> = dimensions.iter().map(|d| (*d).max(0) as u64).collect();
    if location.count.is_empty() {
        return Ok((vec![0; dims.len()], dims));
    }
    if location.count.len() == dims.len() {
        let start = if location.start.is_empty() {
            vec![0; dims.len()]
        } else if location.start.len() == dims.len() {
            location.start.iter().map(|s| (*s).max(0) as u64).collect()
        } else {
            return Err(Error::unsupported(
                "a part whose start and count do not match the dataset's rank",
                at.to_string(),
            ));
        };
        let count = location.count.iter().map(|c| (*c).max(0) as u64).collect();
        return Ok((start, count));
    }
    let total: u64 = dims.iter().product();
    let sum: i64 = location.count.iter().sum();
    let whole = location.start.iter().all(|s| *s == 0)
        && (sum.max(0) as u64 >= total
            || (location.count.len() == 1 && Some(&location.count[0]) == dimensions.first()));
    if whole {
        Ok((vec![0; dims.len()], dims))
    } else {
        Err(Error::unsupported(
            "a part whose start and count do not match the dataset's rank",
            at.to_string(),
        ))
    }
}

/// A block against the dimensions it is read from: same rank, inside.
fn check_bounds(block: &Block, dims: &[u64], at: &str) -> Result<()> {
    if block.start.len() != dims.len() {
        return Err(Error::unsupported(
            format!(
                "a block of rank {} on an array stored with {} dimension(s)",
                block.start.len(),
                dims.len()
            ),
            at.to_string(),
        ));
    }
    for (k, ((start, count), dim)) in block.start.iter().zip(&block.count).zip(dims).enumerate() {
        if start.saturating_add(*count) > *dim {
            return Err(Error::malformed(
                at,
                format!("dimension {k}: a block of {count} from {start} where the array holds {dim}"),
            ));
        }
    }
    Ok(())
}

/// The dimensions of a lattice, slowest first: `[count + 1, ...]` in the order
/// of its `Offset`s. `None` for any other form.
/// An `Offset` without a `Count` is refused, as in [`read`].
fn lattice_dims(array: &dyn Reflect, at: &str) -> Result<Option<Vec<u64>>> {
    if !array.type_info().name.ends_with("LatticeArray") {
        return Ok(None);
    }
    let mut dims = Vec::new();
    for offset in objects(array, "Offset") {
        dims.push(declared_count(offset, &format!("{at}.Offset"))? as u64 + 1);
    }
    if dims.is_empty() {
        return Ok(Some(vec![1]));
    }
    Ok(Some(dims))
}

/// The block of a flat array of the given dimensions, row-major.
fn cut(values: &Values, dims: &[u64], block: &Block) -> Values {
    let rank = dims.len();
    let mut strides = vec![1usize; rank];
    for k in (0..rank.saturating_sub(1)).rev() {
        strides[k] = strides[k + 1] * dims[k + 1] as usize;
    }
    let total: u64 = block.count.iter().product();
    let mut picked: Vec<usize> = Vec::with_capacity(total as usize);
    if total > 0 {
        let mut at = vec![0u64; rank];
        'cells: loop {
            let linear: usize = (0..rank)
                .map(|k| (block.start[k] + at[k]) as usize * strides[k])
                .sum();
            picked.push(linear);
            for k in (0..rank).rev() {
                at[k] += 1;
                if at[k] < block.count[k] {
                    continue 'cells;
                }
                at[k] = 0;
            }
            break;
        }
    }
    gather(values, &picked)
}

fn gather(values: &Values, picked: &[usize]) -> Values {
    match values {
        Values::Floating(v) => Values::Floating(picked.iter().map(|i| v[*i]).collect()),
        Values::Integer(v) => Values::Integer(picked.iter().map(|i| v[*i]).collect()),
        Values::Boolean(v) => Values::Boolean(picked.iter().map(|i| v[*i]).collect()),
        Values::Text(v) => Values::Text(picked.iter().map(|i| v[*i].clone()).collect()),
    }
}

/// The block holding elements `first .. first + count`, all their components,
/// for a property of this shape stored as `layout` says.
///
/// Only a property with neither time nor realizations (read the others by
/// [`slab_block`] and [`read_block`]). When the elements sit on one declared
/// dimension the block is [`slab_block`]'s; the two other cases are the whole
/// store flattened, and a first stored dimension that holds all the element
/// dimensions (with, possibly, their components).
pub fn element_block(shape: &PropertyShape, layout: &StoredLayout, first: u64, count: u64) -> Result<Block> {
    if shape.realizations.is_some() || shape.time.is_some() {
        return Err(Error::unsupported(
            "an element block of a property with a time or realization dimension: read it with read_block",
            "",
        ));
    }
    let elements = shape.elements.count();
    if count == 0 || first.saturating_add(count) > elements {
        return Err(Error::malformed(
            "element block",
            format!("{count} element(s) from {first} where the property holds {elements}"),
        ));
    }
    let element_dims = match &shape.elements {
        crate::property::shape::ElementAxes::Ordered(dims) => dims.len(),
        crate::property::shape::ElementAxes::Unordered(_) => 1,
    };
    if element_dims == 1 {
        return Ok(slab_block(shape, layout, &[], first, count, None)?.block);
    }
    let per = shape.components.len() as u64;
    if layout.groups.len() == 1 {
        return Ok(Block {
            start: vec![first * per],
            count: vec![count * per],
        });
    }
    let declared = shape.declared_dims();
    let head = &layout.groups[0];
    if element_dims > 1 && head.start == 0 && head.end >= element_dims {
        let p0: u64 = declared[element_dims..head.end].iter().product();
        let mut start = vec![0u64; layout.stored.len()];
        start[0] = first * p0;
        let mut counts = layout.stored.clone();
        counts[0] = count * p0;
        return Ok(Block { start, count: counts });
    }
    Err(Error::unsupported(
        "a run of elements over several stored dimensions",
        "",
    ))
}

/// A block of stored values, and whether it holds only the components a
/// narrowed pick reads.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SlabBlock {
    /// The block, per stored dimension.
    pub block: Block,
    /// `true`: each element row holds `NarrowedPick::shape.len()` values;
    /// `false`: every component.
    pub narrowed: bool,
}

/// The stored block of slabs `first .. first + count` of the first declared
/// dimension not in `fixed` (`fixed` gives the indices of the slowest declared
/// dimensions, in order: a realization, then a time step), every faster
/// dimension whole, or only the components of `components` when each component
/// dimension has its own stored dimension.
///
/// The values read arrive in the order `[slab][faster elements][components
/// read]` (Usage Guide 2.2 §4.2.2 p. 39: the 1D order is the same whatever the
/// grouping of the stored dimensions).
pub fn slab_block(
    shape: &PropertyShape,
    layout: &StoredLayout,
    fixed: &[u64],
    first: u64,
    count: u64,
    components: Option<&NarrowedPick>,
) -> Result<SlabBlock> {
    let declared = shape.declared_dims();
    let p = fixed.len();
    let component_dims = if shape.components.is_scalar() {
        0
    } else {
        shape.components.rank()
    };
    let c0 = declared.len().saturating_sub(component_dims);
    if p >= c0 {
        return Err(Error::unsupported(
            "a slab of components: fix the slower dimensions only",
            "",
        ));
    }
    for (i, index) in fixed.iter().enumerate() {
        if *index >= declared[i] {
            return Err(Error::malformed(
                "slab block",
                format!("dimension {i}: index {index} where it holds {}", declared[i]),
            ));
        }
    }
    if count == 0 {
        return Err(Error::unsupported("a block of no slab", ""));
    }
    if first.saturating_add(count) > declared[p] {
        return Err(Error::malformed(
            "slab block",
            format!(
                "dimension {p}: {count} slab(s) from {first} where it holds {}",
                declared[p]
            ),
        ));
    }

    let narrow = components.is_some() && (c0..declared.len()).all(|c| layout.groups.contains(&(c..c + 1)));
    // The product of the declared dimensions after `i`, inside a group ending at `b`.
    let stride = |b: usize, i: usize| -> u64 { declared[i + 1..b].iter().product() };

    let mut start: Vec<u64> = Vec::with_capacity(layout.groups.len());
    let mut counts: Vec<u64> = Vec::with_capacity(layout.groups.len());
    for (k, group) in layout.groups.iter().enumerate() {
        let (a, b) = (group.start, group.end);
        if a == b {
            start.push(0);
            counts.push(1);
        } else if b <= p {
            start.push((a..b).map(|i| fixed[i] * stride(b, i)).sum());
            counts.push(1);
        } else if a <= p {
            let before: u64 = (a..p).map(|i| fixed[i] * stride(b, i)).sum();
            start.push(before + first * stride(b, p));
            counts.push(count * stride(b, p));
        } else if narrow && b == a + 1 && a >= c0 {
            let n = components.expect("narrow implies components");
            start.push(n.start[a - c0]);
            counts.push(n.count[a - c0]);
        } else {
            start.push(0);
            counts.push(layout.stored[k]);
        }
    }
    Ok(SlabBlock {
        block: Block { start, count: counts },
        narrowed: narrow,
    })
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
        (ArrayValues::Text(a), ArrayValues::Text(b)) => a.extend(b),
        // Unreachable: the element types were compared before this was called
        // - `Text` included, which says `string` on both sides.
        _ => {}
    }
}

/// Bring a file's values into the family the schema declares.
///
/// HDF5 has no boolean type - RESQML writes them as bytes - so a boolean array
/// read from a file arrives as integers and is compared against zero here.
///
/// A string array is never read from numbers and a numeric array is never read
/// from strings: either is a refusal that names it, not an empty answer that
/// looks like a column with no values (ADR 0005).
fn widen(values: ArrayValues, family: Family, at: &str) -> Result<Values> {
    match (family, values) {
        (Family::Text, ArrayValues::Text(strings)) => Ok(Values::Text(strings)),
        (Family::Text, other) => Err(Error::unsupported(
            format!(
                "a string array whose external data came back as `{}` values",
                other.element_type()
            ),
            format!(
                "{at}: the backend that read it does not decode strings, and no text is invented from numbers"
            ),
        )),
        (family, ArrayValues::Text(_)) => Err(Error::malformed(
            at,
            format!(
                "a {} array whose external data holds strings",
                family_word(family)
            ),
        )),
        // Nonzero stays nonzero through `f64`, so the boolean reading is exact.
        (Family::Boolean, v) => Ok(Values::Boolean(
            numbers(v, at)?.into_iter().map(|x| x != 0.0).collect(),
        )),
        // Integers never go through `f64`: above 2^53 it rounds (a `NullValue`
        // or an id near the 64-bit range would come back changed).
        (Family::Integer, v) => Ok(Values::Integer(integers(v, at)?)),
        (Family::Floating, v) => Ok(Values::Floating(numbers(v, at)?)),
    }
}

/// The integers of a file's values, widened to `i64` without a detour through
/// `f64`. An `u64` above `i64::MAX` is a named refusal, not a saturated value.
/// A float is truncated, as before: that reading is the file's declaring a
/// floating dataset for an integer array, not a loss this function introduces.
fn integers(values: ArrayValues, at: &str) -> Result<Vec<i64>> {
    Ok(match values {
        ArrayValues::I64(v) => v,
        ArrayValues::I32(v) => v.into_iter().map(i64::from).collect(),
        ArrayValues::I16(v) => v.into_iter().map(i64::from).collect(),
        ArrayValues::I8(v) => v.into_iter().map(i64::from).collect(),
        ArrayValues::U32(v) => v.into_iter().map(i64::from).collect(),
        ArrayValues::U16(v) => v.into_iter().map(i64::from).collect(),
        ArrayValues::U8(v) => v.into_iter().map(i64::from).collect(),
        ArrayValues::U64(v) => v
            .into_iter()
            .map(|x| {
                i64::try_from(x).map_err(|_| {
                    Error::malformed(at, format!("the value {x} does not fit a signed 64-bit integer"))
                })
            })
            .collect::<Result<Vec<i64>>>()?,
        ArrayValues::F64(v) => v.into_iter().map(|x| x as i64).collect(),
        ArrayValues::F32(v) => v.into_iter().map(|x| x as i64).collect(),
        other => {
            return Err(Error::unsupported(
                format!("`{}` values in a numeric array", other.element_type()),
                at.to_string(),
            ));
        }
    })
}

/// The family named in a sentence.
fn family_word(family: Family) -> &'static str {
    match family {
        Family::Floating => "floating-point",
        Family::Integer => "integer",
        Family::Boolean => "boolean",
        Family::Text => "string",
    }
}

/// The values as `f64`, taking the buffer: a floating-point buffer is moved,
/// not cloned.
fn numbers(values: ArrayValues, at: &str) -> Result<Vec<f64>> {
    Ok(match values {
        ArrayValues::F64(v) => v,
        ArrayValues::F32(v) => v.into_iter().map(f64::from).collect(),
        ArrayValues::I64(v) => v.into_iter().map(|x| x as f64).collect(),
        ArrayValues::I32(v) => v.into_iter().map(f64::from).collect(),
        ArrayValues::I16(v) => v.into_iter().map(f64::from).collect(),
        ArrayValues::I8(v) => v.into_iter().map(f64::from).collect(),
        ArrayValues::U64(v) => v.into_iter().map(|x| x as f64).collect(),
        ArrayValues::U32(v) => v.into_iter().map(f64::from).collect(),
        ArrayValues::U16(v) => v.into_iter().map(f64::from).collect(),
        ArrayValues::U8(v) => v.into_iter().map(f64::from).collect(),
        other => {
            return Err(Error::unsupported(
                format!("`{}` values in a numeric array", other.element_type()),
                at.to_string(),
            ));
        }
    })
}

fn lattice(array: &dyn Reflect, family: Family, at: &str) -> Result<Values> {
    // An `Offset` with no `Value` or no `Count` is refused: a step of 0.0 or a
    // count of 0 would invent a lattice the file does not describe.
    let offset_at = format!("{at}.Offset");
    let mut steps: Vec<(f64, usize)> = Vec::new();
    for offset in objects(array, "Offset") {
        let step = scalar_f64(offset, "Value").ok_or_else(|| missing(&offset_at, "Value"))?;
        steps.push((step, declared_count(offset, &offset_at)?));
    }
    let start = scalar_f64(array, "StartValue").ok_or_else(|| missing(at, "StartValue"))?;

    // The product of the dimensions, checked before anything is allocated.
    let mut total: u64 = 1;
    for (_, count) in &steps {
        total = total
            .checked_mul(*count as u64 + 1)
            .filter(|total| *total <= MAX_COMPUTED_VALUES)
            .ok_or_else(|| too_many(at, "the lattice"))?;
    }

    let mut values = vec![start];
    for (step, count) in steps {
        let outer = std::mem::take(&mut values);
        values.reserve(outer.len() * (count + 1));
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

// The readers of a field are the door's (ADR 0046), under the names this file
// already uses.
use energyml_core::reflect::fields::{
    flag, flags as list_bool, integer as scalar_i64, integers as list_i64, items as field_items,
    number as scalar_f64, numbers as list_f64, object as field_object, objects, texts as list_text, unsigned,
};

/// The `NullValue` an integer array declares, when it declares one.
///
/// An integer `long` in every release that has it: `IntegerExternalArray`
/// (EML TRG 2.3 section 6.41 p. 127: "The null value must be explicitly provided
/// in the NullValue attribute of this class"), `IntegerXmlArray` (section 6.44
/// p. 130), `IntegerHdf5Array` (RESQML TRG 2.0.1 section 6.28 p. 73-74). A
/// floating-point array has none: its null is `NaN`. `None` when the field is
/// absent or holds no integer.
///
/// The one reading of it: the copies of `integrity`, `well_strata` and
/// `energyml-build` are replaced by this.
pub fn null_value(array: &dyn Reflect) -> Option<i64> {
    energyml_core::reflect::fields::integer_lexical(array, "NullValue")
}

fn missing(at: &str, field: &str) -> Error {
    Error::malformed(at, format!("no `{field}`"))
}

/// The most values a computed array (constant, range, lattice, index list) may
/// declare. The `Count` comes straight from the file: without a ceiling a
/// hostile or corrupt package asks for an allocation that aborts the host
/// (and a wasm face with it) instead of being refused by name.
const MAX_COMPUTED_VALUES: u64 = 1 << 30;

fn too_many(at: &str, what: impl std::fmt::Display) -> Error {
    Error::malformed(
        at,
        format!("{what} is above the ceiling of {MAX_COMPUTED_VALUES} values for a computed array"),
    )
}

/// The `Count` a computed array declares: present, not negative, and not
/// above [`MAX_COMPUTED_VALUES`]. A missing `Count` is a refusal, not an empty
/// array.
fn declared_count(array: &dyn Reflect, at: &str) -> Result<usize> {
    let Some(count) = scalar_i64(array, "Count") else {
        // An unsigned `Count` above `i64::MAX` is not absent, it is absurd.
        return Err(match unsigned(array, "Count") {
            Some(huge) => too_many(at, format!("`Count` {huge}")),
            None => missing(at, "Count"),
        });
    };
    if count < 0 {
        return Err(Error::malformed(at, format!("`Count` is negative ({count})")));
    }
    if count as u64 > MAX_COMPUTED_VALUES {
        return Err(too_many(at, format!("`Count` {count}")));
    }
    Ok(count as usize)
}

/// The elements of an inline `Values` list, all of them or a refusal: an
/// element that is not of the array's type is named, never dropped.
///
/// `read` is one of the door's plural readers, which leave out an element of
/// the wrong shape; comparing what it kept with what the list holds finds the
/// ones it left out.
fn inline<T>(
    array: &dyn Reflect,
    at: &str,
    expected: &str,
    read: fn(&dyn Reflect, &str) -> Vec<T>,
) -> Result<Vec<T>> {
    let declared = field_items(array, "Values").len();
    let kept = read(array, "Values");
    if kept.len() != declared {
        return Err(Error::malformed(
            at,
            format!(
                "`Values` holds {declared} element(s) and {} of them are not {expected}; none is dropped",
                declared - kept.len()
            ),
        ));
    }
    Ok(kept)
}
