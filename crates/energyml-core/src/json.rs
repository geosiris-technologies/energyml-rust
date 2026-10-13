//! The EnergyML JSON encoding.
//!
//! JSON is a second wire format for the same model, and Energistics fixes its
//! conventions in the *JSON Style Guide* rather than leaving them to whatever a
//! serialization library happens to produce. The rules that matter here, as the
//! reference implementations apply them (`energyml-python`'s `OSDU_OFFICIAL`
//! mode, and the JSON OSDU ingests):
//!
//! | Rule | Why it is not optional |
//! |---|---|
//! | **every** non-primitive object carries `$type`, its qualified type | it is the only type information in the document; without it a reader must guess from the field name, and cannot read a substituted subtype at all |
//! | enumerations are written as their bare lexical value | `"m"`, not `{"value": "m"}` — the same string XML writes |
//! | absent values are omitted | a null carries no more meaning than an absent key, and doubles the size of a large document |
//! | the text of a `simpleContent` type is the key `_` | it has no element name of its own to use |
//! | `_data` may carry the object a reference points at | makes a document self-contained without a package around it |
//!
//! ```json
//! {
//!   "$type": "resqml22.IjkGridRepresentation",
//!   "Citation": { "$type": "eml23.Citation", "Title": "Two faulted sugar cubes" },
//!   "Geometry": { "$type": "resqml22.IjkGridGeometry", "Points": {
//!       "$type": "resqml22.Point3dExternalArray", "Coordinates": { "…": "…" } } }
//! }
//! ```
//!
//! # Why this is written over reflection, not over serde
//!
//! `$type` is not decoration: it is the **discriminant**. A field declared with
//! an abstract type holds one of many concrete subtypes, and JSON has no
//! `xsi:type` to fall back on. Serde's derive cannot supply it — it knows the
//! Rust enum, not the EnergyML type, and its own answer (externally tagged,
//! `{"Point3dExternalArray": {…}}`) is a shape no other implementation reads.
//! Reflection sees the concrete subtype and its [`TypeInfo`], so the encoding is
//! written from that instead: free functions over an object in memory, on the
//! same principle as everything else dynamic in this crate.
//!
//! # What is ours rather than Energistics'
//!
//! Two cases the guide does not cover, both marked here so nobody mistakes them
//! for the standard:
//!
//! - **[`AnyXml`]** — a foreign subtree (a GML CRS definition) has no JSON form.
//!   It is written as `{"$type": "eml.AnyXml", "content": "<xml…>", …}` so that
//!   the round trip keeps it, per the rule that what is not modelled is still
//!   not dropped. `eml.AnyXml` is not a schema type and no other implementation
//!   will know it.
//! - **A duplicated element name** — WITSML objects declare their own
//!   `witsml:Citation` beside the inherited `eml:Citation`, which would be one
//!   JSON key twice. The second occurrence is qualified with its prefix,
//!   `"witsml:Citation"`. XML tells the two apart by namespace and needs no
//!   such trick; JSON has no namespaces, so somebody has to choose, and this is
//!   the choice.
//!
//! # What each direction needs
//!
//! Writing needs **nothing**: reflection carries the type of every object it
//! walks. Reading needs a [`Registry`] holding every type the document names —
//! nested ones included, since `$type` names them all and the reader builds
//! each one by name. A generated crate's `register` covers that by
//! construction.

use std::collections::BTreeSet;

use serde_json::{Map, Number, Value};

use crate::any_xml::AnyXml;
use crate::error::{Error, Result};
use crate::identity;
use crate::naming;
use crate::reflect::{Binding, FieldInfo, ScalarKind, TypeInfo};
use crate::registry::{DynObject, Registry};
use crate::value::{FieldValue, OwnedValue, Reflect, Scalar};

/// The key carrying an object's qualified type.
pub const TYPE_KEY: &str = "$type";
/// The key carrying the text content of a `simpleContent` type.
pub const TEXT_KEY: &str = "_";
/// The key carrying the object a reference points at.
pub const DATA_KEY: &str = "_data";

/// The `$type` of any type, nested ones included: `eml20.Citation`,
/// `resqml20.obj_Activity`.
///
/// Built from the same three pieces as every other Energistics identifier —
/// package, schema version compacted, and the complexType name **verbatim**, so
/// RESQML 2.0.1 keeps its `obj_` prefix here as it does in a content type.
pub fn qualified_type_of(info: &TypeInfo) -> String {
    format!(
        "{}{}.{}",
        info.package,
        naming::compact_version(info.schema_version),
        info.object_type
    )
}

// ---------------------------------------------------------------------------
// Supplying the objects a reference points at
// ---------------------------------------------------------------------------

/// Where `_data` comes from: the object a `DataObjectReference` names.
///
/// A borrow rather than an owned object, because the caller writing a
/// self-contained document has usually already read the whole package.
pub trait ObjectSource {
    /// The object with this uuid, if the caller can supply it.
    fn find(&self, uuid: &str) -> Option<&dyn Reflect>;
}

/// No `_data` at all: references are written as they stand.
pub struct NoObjects;

impl ObjectSource for NoObjects {
    fn find(&self, _uuid: &str) -> Option<&dyn Reflect> {
        None
    }
}

/// An [`ObjectSource`] over objects already in memory.
pub struct ObjectsInMemory<'a> {
    by_uuid: Vec<(String, &'a dyn Reflect)>,
}

impl<'a> ObjectsInMemory<'a> {
    /// Index the objects that carry a uuid; the others are ignored.
    pub fn new(objects: impl IntoIterator<Item = &'a dyn Reflect>) -> Self {
        let by_uuid = objects
            .into_iter()
            .filter_map(|object| {
                identity::uuid_of_reflect(object).map(|uuid| (uuid.to_ascii_lowercase(), object))
            })
            .collect();
        Self { by_uuid }
    }
}

impl ObjectSource for ObjectsInMemory<'_> {
    fn find(&self, uuid: &str) -> Option<&dyn Reflect> {
        let key = uuid.to_ascii_lowercase();
        self.by_uuid
            .iter()
            .find(|(u, _)| *u == key)
            .map(|(_, object)| *object)
    }
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// Encode an object.
pub fn write_value(object: &dyn Reflect) -> Value {
    write_value_with(object, &NoObjects)
}

/// Encode an object, expanding references through `source` into `_data`.
///
/// Each uuid is expanded **once** per document. Without that, a document with a
/// reference cycle — an interpretation naming a representation that names the
/// interpretation back — would never terminate.
pub fn write_value_with(object: &dyn Reflect, source: &dyn ObjectSource) -> Value {
    let mut expanded = BTreeSet::new();
    if let Some(uuid) = identity::uuid_of_reflect(object) {
        expanded.insert(uuid.to_ascii_lowercase());
    }
    write_object(object, source, &mut expanded)
}

/// Encode an object as JSON text.
pub fn write_string(object: &dyn Reflect, pretty: bool) -> Result<String> {
    write_string_with(object, &NoObjects, pretty)
}

/// Encode an object as JSON text, expanding references into `_data`.
pub fn write_string_with(object: &dyn Reflect, source: &dyn ObjectSource, pretty: bool) -> Result<String> {
    let value = write_value_with(object, source);
    let out = if pretty {
        serde_json::to_string_pretty(&value)
    } else {
        serde_json::to_string(&value)
    };
    out.map_err(|e| Error::other(format!("JSON serialization failed: {e}")))
}

fn write_object(object: &dyn Reflect, source: &dyn ObjectSource, expanded: &mut BTreeSet<String>) -> Value {
    let info = object.type_info();
    if let Some(any) = object.as_any().downcast_ref::<AnyXml>() {
        return write_any_xml(any, info);
    }

    let mut map = Map::new();
    map.insert(TYPE_KEY.to_string(), Value::String(qualified_type_of(info)));

    // The keys are computed for every field, present or not: qualification of a
    // duplicate name depends on field *order*, never on which fields happen to
    // be filled in.
    for (field, key) in info.fields.iter().zip(json_keys(info)) {
        let Some(value) = object.field(field.name) else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        map.insert(key, write_field(&value, source, expanded));
    }

    if info.is_object_ref {
        write_data(&mut map, object, source, expanded);
    }
    Value::Object(map)
}

/// Attach the referenced object, when the caller supplied one and this uuid has
/// not already been expanded higher up the document.
fn write_data(
    map: &mut Map<String, Value>,
    reference: &dyn Reflect,
    source: &dyn ObjectSource,
    expanded: &mut BTreeSet<String>,
) {
    let Some(uuid) = identity::uuid_of_reflect(reference).filter(|u| !u.is_empty()) else {
        return;
    };
    if !expanded.insert(uuid.to_ascii_lowercase()) {
        return;
    }
    if let Some(target) = source.find(uuid) {
        let value = write_object(target, source, expanded);
        map.insert(DATA_KEY.to_string(), value);
    }
}

fn write_field(value: &FieldValue<'_>, source: &dyn ObjectSource, expanded: &mut BTreeSet<String>) -> Value {
    match value {
        FieldValue::Null => Value::Null,
        FieldValue::Scalar(scalar) => write_scalar(*scalar),
        FieldValue::Object(child) => write_object(*child, source, expanded),
        FieldValue::List(items) => Value::Array(
            items
                .iter()
                .map(|item| write_field(item, source, expanded))
                .collect(),
        ),
    }
}

fn write_scalar(scalar: Scalar<'_>) -> Value {
    match scalar {
        Scalar::Bool(v) => Value::Bool(v),
        Scalar::Int(v) => Value::Number(Number::from(v)),
        Scalar::UInt(v) => Value::Number(Number::from(v)),
        Scalar::Double(v) => match Number::from_f64(v) {
            Some(number) => Value::Number(number),
            // JSON has no `NaN` and no infinity; `xsd:double` has all three,
            // and the corpus uses them — `prodml22.FluidSystem` carries a NaN.
            // Writing the XSD lexical form as a string keeps the value where
            // `null` would have dropped it and failed to read back. Ours, not
            // the style guide's: it has nothing to say about a value JSON
            // cannot spell.
            None => Value::String(xsd_double(v)),
        },
        Scalar::Str(v) => Value::String(v.to_string()),
    }
}

/// The `xsd:double` lexical form of a value JSON cannot hold.
fn xsd_double(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_string()
    } else if value.is_sign_positive() {
        "INF".to_string()
    } else {
        "-INF".to_string()
    }
}

/// The three lexical doubles JSON has no number for, read back.
fn parse_xsd_double(text: &str) -> Option<f64> {
    match text.trim() {
        "NaN" => Some(f64::NAN),
        "INF" | "+INF" => Some(f64::INFINITY),
        "-INF" => Some(f64::NEG_INFINITY),
        other => other.parse::<f64>().ok(),
    }
}

fn write_any_xml(any: &AnyXml, info: &TypeInfo) -> Value {
    let pairs = |items: &[(String, String)]| {
        Value::Object(
            items
                .iter()
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect::<Map<_, _>>(),
        )
    };
    let mut map = Map::new();
    map.insert(TYPE_KEY.to_string(), Value::String(qualified_type_of(info)));
    if !any.attributes.is_empty() {
        map.insert("attributes".to_string(), pairs(&any.attributes));
    }
    if !any.content.is_empty() {
        map.insert("content".to_string(), Value::String(any.content.clone()));
    }
    if !any.namespaces.is_empty() {
        map.insert("namespaces".to_string(), pairs(&any.namespaces));
    }
    Value::Object(map)
}

/// The JSON key of every field, in declaration order.
///
/// The text of a `simpleContent` type is `_`; a name already used by an earlier
/// field is qualified with its prefix. This is the only place the mapping is
/// defined — the generated code carries no JSON names of its own any more.
///
/// **Public, because a description must name the key the JSON uses.** The
/// front end's editor, the type documentation and the Python bridge read
/// `FieldInfo::json_name`, which the generator fills with the XML name — so the
/// text of every `*Measure`, `StringMeasure`… was described as `Value` and
/// written as `_`, and the editor showed an empty field (the
/// `Value` part looked unfilled while only `name` was). A
/// describer takes its keys from here, never from `json_name`.
pub fn json_keys(info: &TypeInfo) -> Vec<String> {
    let mut seen = BTreeSet::new();
    info.fields
        .iter()
        .map(|field| {
            if field.binding == Binding::Text {
                TEXT_KEY.to_string()
            } else if seen.insert(field.xml_name) {
                field.xml_name.to_string()
            } else {
                format!("{}:{}", field.xml_ns.prefix, field.xml_name)
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// What one JSON document held.
pub struct JsonDocument {
    /// The object the document is about.
    pub root: Box<dyn DynObject>,
    /// Objects found under `_data`, in the order they were met.
    ///
    /// Separate from the root because they are *other* objects — a document
    /// carrying them is a small package, and flattening them into the root
    /// would lose that.
    pub referenced: Vec<Box<dyn DynObject>>,
}

/// Decode a document.
pub fn read_string(text: &str, registry: &Registry) -> Result<JsonDocument> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| Error::other(format!("JSON deserialization failed: {e}")))?;
    read_value(&value, registry)
}

/// Decode a document already parsed.
pub fn read_value(value: &Value, registry: &Registry) -> Result<JsonDocument> {
    let mut referenced = Vec::new();
    let root = read_object(value, None, registry, &mut referenced)?;
    Ok(JsonDocument { root, referenced })
}

/// Build one object. `declared` is the type the field expects, used only when
/// the document omits `$type` — tolerated because hand-written JSON does, and
/// unambiguous whenever the declared type is concrete.
fn read_object(
    value: &Value,
    declared: Option<&'static TypeInfo>,
    registry: &Registry,
    referenced: &mut Vec<Box<dyn DynObject>>,
) -> Result<Box<dyn DynObject>> {
    let map = value
        .as_object()
        .ok_or_else(|| Error::other(format!("expected a JSON object, found {}", kind_of(value))))?;

    let qualified = match map.get(TYPE_KEY).and_then(Value::as_str) {
        Some(qualified) => qualified.to_string(),
        None => {
            let info = declared.ok_or_else(|| {
                Error::other(format!("a JSON object without `{TYPE_KEY}` and no declared type"))
            })?;
            if info.is_abstract {
                return Err(Error::other(format!(
                    "`{}` is abstract, so `{TYPE_KEY}` is required to say which subtype this is",
                    info.name
                )));
            }
            qualified_type_of(info)
        }
    };

    let mut object = registry.create(&qualified)?;
    fill(object.as_reflect_mut(), map, registry, referenced)?;
    Ok(object)
}

fn fill(
    object: &mut dyn Reflect,
    map: &Map<String, Value>,
    registry: &Registry,
    referenced: &mut Vec<Box<dyn DynObject>>,
) -> Result<()> {
    let info = object.type_info();
    let keys = json_keys(info);

    for (key, value) in map {
        if key == TYPE_KEY {
            continue;
        }
        if key == DATA_KEY {
            // Another object, not a field of this one.
            let extra = read_object(value, None, registry, referenced)?;
            referenced.push(extra);
            continue;
        }
        let Some(index) = find_field(info, &keys, key) else {
            // Forward compatibility, same rule as the XML reader: a document
            // from a newer minor version stays readable.
            continue;
        };
        let field = &info.fields[index];
        let owned = read_field(value, field, registry, referenced)?;
        // Name the field: `expected double, found null` says nothing on its own
        // in a document with two hundred keys.
        object
            .set_field(field.name, owned)
            .map_err(|e| Error::other(format!("`{}.{}`: {e}", qualified_type_of(info), field.xml_name)))?;
    }
    Ok(())
}

/// Match a JSON key to a field: by its canonical key first, then by the Rust
/// name and the bare element name, so a document written by a looser tool still
/// lands.
fn find_field(info: &TypeInfo, keys: &[String], key: &str) -> Option<usize> {
    if let Some(index) = keys.iter().position(|k| k == key) {
        return Some(index);
    }
    let bare = key.rsplit(':').next().unwrap_or(key);
    info.fields
        .iter()
        .position(|f| f.name.eq_ignore_ascii_case(key) || f.xml_name.eq_ignore_ascii_case(bare))
}

/// One field's value, read from JSON, ready for [`crate::value::Reflect::set_field`].
///
/// The public door onto what the document reader already does per field, opened
/// for one caller and one need: an editor that has to put a **whole object** in
/// a field — a `Citation`, a `Point3dLatticeArray`, a `DataObjectReference` —
/// and until now could only put a string there.
///
/// It takes the [`FieldInfo`] rather than a type name, and that is the point:
/// the field already says what it accepts, including which concrete class an
/// abstract slot may hold, so a caller cannot pass a `Citation` where a
/// `Point3d` belongs and have it quietly land.
///
/// The returned objects are the ones a `_data` member carried — the Energistics
/// convention for shipping the target of a reference alongside it. A caller with
/// nowhere to put them may drop them; a caller filling a store should not.
#[cfg(feature = "json")]
pub fn read_field_value(
    value: &Value,
    field: &FieldInfo,
    registry: &Registry,
) -> Result<(OwnedValue, Vec<Box<dyn DynObject>>)> {
    let mut referenced = Vec::new();
    let owned = read_field(value, field, registry, &mut referenced)?;
    Ok((owned, referenced))
}

fn read_field(
    value: &Value,
    field: &FieldInfo,
    registry: &Registry,
    referenced: &mut Vec<Box<dyn DynObject>>,
) -> Result<OwnedValue> {
    let declared = field.ty.type_info();
    match value {
        Value::Null => Ok(OwnedValue::Null),
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(read_field(item, field, registry, referenced)?);
            }
            Ok(OwnedValue::List(out))
        }
        Value::Object(map) => {
            // A foreign subtree is not a registered type; rebuild it directly.
            if declared.is_some_and(|info| info.name == "AnyXml") {
                // Unless what arrived is a **typed** object. A slot declared as
                // a subtree can be handed one whenever the class is really
                // known — `AbstractGraphicalInformation` is declared in EML and
                // specialised in RESQML, so the field is a subtree for reasons
                // of crate layering, not of data. Without this branch such an
                // object became an empty subtree, and silently: `read_any_xml`
                // looks for `content` and finds none.
                let stated = map.get(TYPE_KEY).and_then(Value::as_str).unwrap_or_default();
                if !stated.is_empty() && stated != "eml.AnyXml" {
                    let typed = read_object(value, None, registry, referenced)?;
                    let subtree = AnyXml::of(typed.as_ref()).map_err(|e| {
                        Error::other(format!("`{stated}` cannot be written as a subtree: {e}"))
                    })?;
                    return Ok(OwnedValue::Object(Box::new(subtree)));
                }
                return Ok(OwnedValue::Object(Box::new(read_any_xml(map))));
            }
            // An enumeration slot given an object. Refused *here*, by name,
            // because the next step asks the registry for the enumeration's
            // qualified type — which is deliberately not registered — and
            // reports "unknown type `resqml22.ThrowKindExt` (not registered)":
            // a true sentence about a type that is not the problem, and one
            // that sends the reader looking for a missing crate. What is
            // actually wrong is the shape of the value.
            if let Some(info) = declared
                && info.kind == crate::reflect::TypeKind::Enumeration
            {
                return Err(Error::other(format!(
                    "`{}` is an enumeration ({}): it takes one of its values, not an object",
                    field.xml_name, info.name
                )));
            }
            let object = read_object(value, declared, registry, referenced)?;
            Ok(OwnedValue::Object(object.into_reflect()))
        }
        scalar => read_scalar(scalar, field),
    }
}

/// Convert a JSON scalar to what the field declares.
///
/// The declared kind decides, because JSON's number is not Rust's: `3` in a
/// `double` field arrives as an integer, and a strict setter would refuse it.
fn read_scalar(value: &Value, field: &FieldInfo) -> Result<OwnedValue> {
    let kind = match field.ty {
        crate::reflect::FieldType::Scalar(kind) => Some(kind),
        // An enumeration takes its lexical value, like XML.
        _ => None,
    };
    let bad = |wanted: &str| {
        Error::other(format!(
            "`{}` expects {wanted}, found {}",
            field.xml_name,
            kind_of(value)
        ))
    };

    match (value, kind) {
        (Value::Bool(v), Some(ScalarKind::Bool) | None) => Ok(OwnedValue::Bool(*v)),
        (Value::Bool(v), Some(ScalarKind::String)) => Ok(OwnedValue::Str(v.to_string())),
        (Value::Bool(_), _) => Err(bad("a number")),

        (Value::String(s), Some(ScalarKind::Bool)) => match s.trim() {
            "true" | "1" => Ok(OwnedValue::Bool(true)),
            "false" | "0" => Ok(OwnedValue::Bool(false)),
            _ => Err(bad("a boolean")),
        },
        // Numbers written as strings: not conformant, and cheap to accept.
        (Value::String(s), Some(ScalarKind::Int)) => s
            .trim()
            .parse::<i64>()
            .map(OwnedValue::Int)
            .map_err(|_| bad("an integer")),
        (Value::String(s), Some(ScalarKind::UInt)) => s
            .trim()
            .parse::<u64>()
            .map(OwnedValue::UInt)
            .map_err(|_| bad("an unsigned integer")),
        (Value::String(s), Some(ScalarKind::Double)) => parse_xsd_double(s)
            .map(OwnedValue::Double)
            .ok_or_else(|| bad("a double")),
        (Value::String(s), _) => Ok(OwnedValue::Str(s.clone())),

        (Value::Number(n), Some(ScalarKind::Double)) => {
            n.as_f64().map(OwnedValue::Double).ok_or_else(|| bad("a double"))
        }
        (Value::Number(n), Some(ScalarKind::UInt)) => n
            .as_u64()
            .or_else(|| u64::try_from(n.as_i64()?).ok())
            .map(OwnedValue::UInt)
            .ok_or_else(|| bad("an unsigned integer")),
        (Value::Number(n), Some(ScalarKind::Int)) => n
            .as_i64()
            .or_else(|| i64::try_from(n.as_u64()?).ok())
            .map(OwnedValue::Int)
            .ok_or_else(|| bad("an integer")),
        (Value::Number(n), Some(ScalarKind::String | ScalarKind::DateTime | ScalarKind::Binary)) => {
            Ok(OwnedValue::Str(n.to_string()))
        }
        (Value::Number(n), _) => {
            if let Some(v) = n.as_i64() {
                Ok(OwnedValue::Int(v))
            } else if let Some(v) = n.as_u64() {
                Ok(OwnedValue::UInt(v))
            } else {
                n.as_f64().map(OwnedValue::Double).ok_or_else(|| bad("a number"))
            }
        }
        _ => Err(bad("a scalar")),
    }
}

fn read_any_xml(map: &Map<String, Value>) -> AnyXml {
    let pairs = |key: &str| -> Vec<(String, String)> {
        map.get(key)
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                    .collect()
            })
            .unwrap_or_default()
    };
    AnyXml {
        attributes: pairs("attributes"),
        content: map
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        namespaces: pairs("namespaces"),
    }
}

fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reflect::EnergymlType;

    #[test]
    fn a_qualified_type_covers_nested_types_too() {
        let info = <AnyXml as EnergymlType>::type_info();
        assert_eq!(qualified_type_of(info), "eml.AnyXml");
    }

    #[test]
    fn a_foreign_subtree_survives_the_round_trip() {
        let any = AnyXml {
            attributes: vec![("srsName".to_string(), "EPSG::4326".to_string())],
            content: "<gml:origin>0 0</gml:origin>".to_string(),
            namespaces: vec![("gml".to_string(), "http://www.opengis.net/gml/3.2".to_string())],
        };
        let value = write_any_xml(&any, <AnyXml as EnergymlType>::type_info());
        let back = read_any_xml(value.as_object().unwrap());
        assert_eq!(back, any);
        assert_eq!(value[TYPE_KEY], "eml.AnyXml");
    }
}
