//! The model's documentation, as JSON — the reflection, serialized once.
//!
//! # Why this exists
//!
//! Everything here is already answerable at runtime — `TypeInfo`, `FieldInfo`
//! and `VariantInfo` carry the `xsd:documentation`, the cardinalities, the
//! accepted types and the declared values, which is what drives the UI's
//! documentation view. What was missing is the same answer **as a document**:
//! a static JSON a tool — or an AI — can fetch without running the
//! application, and without the Energistics login the schema downloads sit
//! behind. The CLI writes these files; a front end build ships them beside the
//! front end.
//!
//! # The shape
//!
//! One JSON object per (package, version) — `resqml 2.2`, `eml 2.3` — holding
//! every registered type of that release. Filtering "only resqml" or "only
//! resqml 2.2" is therefore *choosing files*, not a query language: the index
//! lists what exists, and each file is self-contained.
//!
//! The spellings are the model's, not inventions: `xmlName` is the element
//! name, `objectType` the complexType name (the one identifiers use),
//! `typeKey` the cross-version grouping key. The three-names rule of the
//! repository applies to documentation too — a reader who only ever sees a
//! merged name cannot tell an element from its type, and that distinction is
//! an interoperability fact, not cosmetics.

use serde_json::{Value, json};

use crate::reflect::{Binding, FieldInfo, MaxOccurs, TypeInfo, TypeKind, VariantInfo};
use crate::registry::Registry;

/// Every type reachable from the registered ones — the registry's entries plus
/// the closure of their fields and variants.
///
/// The walk is not optional: enumerations and choices are **deliberately
/// absent from the registry** — they are written as lexical values or selected
/// by `xsi:type`, never constructed by name — so iterating the registry alone
/// documents 3 345 structs and not one of the vocabularies people actually
/// look up. Every enumeration is reachable through some field, every choice
/// through a field or a variant, and each carries its own package and version,
/// so the grouping stays right.
fn reachable(registry: &Registry) -> Vec<&'static TypeInfo> {
    let mut seen: Vec<&'static TypeInfo> = Vec::new();
    let mut queue: Vec<&'static TypeInfo> = registry.types().collect();
    while let Some(info) = queue.pop() {
        if seen.iter().any(|held| std::ptr::eq(*held, info)) {
            continue;
        }
        seen.push(info);
        for field in info.fields {
            if let Some(inner) = field.ty.type_info() {
                queue.push(inner);
            }
        }
        for variant in info.variants {
            if let Some(inner) = variant.ty {
                queue.push(inner());
            }
        }
    }
    seen
}

/// Every registered release, one entry per (package, version), types included.
///
/// Sorted by package then version so the output is stable — a regenerated
/// file that only reorders is a diff nobody can review.
pub fn registry_docs(registry: &Registry) -> Value {
    let mut releases: Vec<(&'static str, &'static str, Vec<&'static TypeInfo>)> = Vec::new();
    for info in reachable(registry) {
        match releases
            .iter_mut()
            .find(|(package, version, _)| *package == info.package && *version == info.version)
        {
            Some((_, _, types)) => types.push(info),
            None => releases.push((info.package, info.version, vec![info])),
        }
    }
    // A type with no version is runtime support, not a release: `AnyXml` — the
    // kept-verbatim wrapper for foreign vocabularies — reaches here through
    // the fields that hold it, and grouped naively it minted a one-type
    // release called `eml-` in every index. The schema releases all state
    // their version; what does not is not schema.
    releases.retain(|(package, version, _)| !package.is_empty() && !version.is_empty());
    releases.sort_by_key(|(package, version, _)| (*package, *version));
    Value::Array(
        releases
            .into_iter()
            .map(|(package, version, mut types)| {
                types.sort_by_key(|info| info.name);
                json!({
                    "package": package,
                    "version": version,
                    "schemaVersion": types.first().map(|t| t.schema_version),
                    "types": types.into_iter().map(type_doc).collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}

/// One type, everything the reflection knows about it.
pub fn type_doc(info: &'static TypeInfo) -> Value {
    // The JSON key comes from `json::json_keys`, the writer's own mapping —
    // `json_name` says `Value` where the JSON writes `_`.
    let keys = crate::json::json_keys(info);
    let fields: Vec<Value> = info
        .fields
        .iter()
        .zip(&keys)
        .map(|(field, key)| field_doc(field, key))
        .collect();
    let variants: Vec<Value> = info.variants.iter().map(variant_doc).collect();
    json!({
        "name": info.name,
        "xmlName": info.xml_name,
        "objectType": info.object_type,
        "typeKey": info.type_key,
        "namespace": info.xml_ns.uri,
        "qualifiedType": info.qualified_type,
        "contentType": info.content_type,
        "kind": match info.kind {
            TypeKind::Struct => "struct",
            TypeKind::Enumeration => "enumeration",
            TypeKind::Choice => "choice",
        },
        "abstract": info.is_abstract,
        "topLevel": info.is_top_level,
        "nestable": info.is_nestable,
        // An `…Ext`-style open enumeration accepts any string beside its list;
        // a chooser that reads this as closed silently blocks legitimate data.
        "open": info.is_open,
        "doc": info.doc,
        "fields": fields,
        "variants": variants,
    })
}

/// One field: names, documentation, shape, and what may go in it.
fn field_doc(field: &FieldInfo, json_key: &str) -> Value {
    // What may go in the field — the question `--describe` answers, asked of
    // every field at once. An enumeration offers its values, an abstract type
    // its concrete subtypes with the `xsi:type` that selects each; a reference
    // is where reflection stops, and saying so beats letting a reader assume
    // the model knows.
    let accepted: Vec<Value> = field
        .accepted_types()
        .into_iter()
        .map(|info| {
            json!({
                "name": info.name,
                "qualifiedType": info.qualified_type,
                "isObjectReference": info.is_object_ref,
            })
        })
        .collect();
    let values: Vec<Value> = field
        .ty
        .allowed_values()
        .iter()
        .filter(|variant| !variant.is_other)
        .map(|variant| json!({ "value": variant.value, "doc": variant.doc }))
        .collect();
    json!({
        "name": field.xml_name,
        "jsonName": json_key,
        "namespace": field.xml_ns.uri,
        "doc": field.doc,
        "binding": match field.binding {
            Binding::Attribute => "attribute",
            Binding::Element => "element",
            Binding::Text => "text",
            _ => "any",
        },
        "minOccurs": field.occurs.min,
        "maxOccurs": match field.occurs.max {
            MaxOccurs::Bounded(max) => json!(max),
            MaxOccurs::Unbounded => Value::Null,
        },
        "required": field.is_required(),
        // One element holding every value, not one element per value — the
        // `xsd:list` distinction the repository documents at length.
        "xsdList": field.is_list,
        "scalar": field.ty.type_info().is_none(),
        "type": field.ty.type_info().map(|info| info.name),
        "openEnumeration": field.ty.type_info().is_some_and(|info| info.is_open),
        "allowedValues": values,
        "acceptedTypes": accepted,
    })
}

/// One variant: a declared value for an enumeration, a selectable subtype for
/// a choice. The `Other` hole of an open enumeration is not a value and is
/// left out — `type_doc` says `open` instead, once.
fn variant_doc(variant: &VariantInfo) -> Value {
    json!({
        "name": variant.name,
        "value": if variant.is_other { Value::Null } else { json!(variant.value) },
        "xsiType": variant.xsi_type,
        "doc": variant.doc,
        "isOther": variant.is_other,
    })
}
