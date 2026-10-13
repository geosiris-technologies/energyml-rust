//! Pulling named values out of an object, across references, by dotted path.
//!
//! # What this is for
//!
//! An exported surface needs to carry more than triangles: a title, a uuid, a
//! content type, and — the part that actually costs something — values that
//! live in *other objects*. The feature name of a horizon is three objects
//! away: the representation names an interpretation, the interpretation names a
//! feature, and the name a person recognises is on the third.
//!
//! Hard-coding that chain works until somebody wants a fourth thing. So the
//! chain is data:
//!
//! ```json
//! {
//!   "uuid":    "uuid",
//!   "title":   "Citation.Title",
//!   "type":    "contentType",
//!   "feature": "RepresentedInterpretation.InterpretedFeature.Citation.Title",
//!   "author":  "Citation.Originator"
//! }
//! ```
//!
//! # References are crossed without saying so
//!
//! `RepresentedInterpretation.InterpretedFeature.Citation.Title` reads as one
//! path, and the two hops through the store are invisible. The rule at each
//! step:
//!
//! 1. if the current object **has** the field, use it;
//! 2. otherwise, if the current object is a reference — it carries a uuid —
//!    resolve it through the store and look for the field on the target.
//!
//! Order matters, and this is the useful way round. A `DataObjectReference`
//! caches `Title`, `UUID` and `QualifiedType`, so `RepresentedInterpretation.Title`
//! answers from the reference **without touching the store**, while
//! `RepresentedInterpretation.Citation.Title` resolves the object and reads its
//! real citation. The first is free and may be stale; the second costs a read
//! and is authoritative. Both are useful, and a caller picks by what it writes.
//!
//! # Names that are not fields
//!
//! `uuid`, `uri`, `contentType`, `qualifiedType` and `title` are computed from
//! the object's identity rather than read from a field, and work at any point
//! in a path — including after crossing a reference. They are what an export
//! usually wants and what the XML does not directly hold.
//!
//! # Nothing found is not an error
//!
//! A path that leads nowhere yields no entry, and the export goes on. That is
//! deliberate: a spec is written once and run over a whole package, where some
//! objects have an interpretation and some do not, and failing the export
//! because one surface lacks a feature would make the feature unusable. What a
//! caller can still see is [`Metadata::misses`] — every key that found nothing,
//! so a spec with a typo in it is visible rather than silently empty.

use std::collections::BTreeMap;

use energyml_core::identity;
use energyml_core::path;
use energyml_core::value::{FieldValue, Reflect};

use crate::error::{Error, Result};
use crate::source::ObjectSource;

/// Which keys to collect, and where each comes from.
///
/// Ordered, so the output of an export is stable between runs — a diff of two
/// exports should show what changed in the data, not a reshuffle of keys.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Spec {
    entries: Vec<(String, String)>,
}

impl Spec {
    /// An empty spec: no metadata at all.
    pub fn new() -> Self {
        Self::default()
    }

    /// The keys an export carries when the caller asks for nothing.
    ///
    /// Identity and provenance: enough to find the object again in the package
    /// it came from, and to say what it represents.
    pub fn standard() -> Self {
        let mut spec = Self::new();
        spec.add("uuid", "uuid")
            .add("title", "Citation.Title")
            .add("contentType", "contentType")
            .add("qualifiedType", "qualifiedType")
            .add("originator", "Citation.Originator")
            .add("interpretation", "RepresentedInterpretation.uuid")
            .add("interpretationTitle", "RepresentedInterpretation.Title")
            .add("feature", "RepresentedInterpretation.InterpretedFeature.uuid")
            .add(
                "featureTitle",
                "RepresentedInterpretation.InterpretedFeature.Title",
            );
        spec
    }

    /// Add one key and the path it reads.
    pub fn add(&mut self, key: impl Into<String>, path: impl Into<String>) -> &mut Self {
        self.entries.push((key.into(), path.into()));
        self
    }

    /// How many keys.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The keys and paths, in order.
    pub fn entries(&self) -> &[(String, String)] {
        &self.entries
    }

    /// Read a spec from JSON: a flat object of key → path.
    ///
    /// ```
    /// # use energyml_domain::metadata::Spec;
    /// let spec = Spec::from_json(r#"{"title": "Citation.Title"}"#).unwrap();
    /// assert_eq!(spec.entries(), &[("title".to_string(), "Citation.Title".to_string())]);
    /// ```
    ///
    /// Key order is kept as written, which `serde_json::Map` does not do by
    /// default — so the object is walked as raw text rather than deserialized
    /// into a map. A spec is a human-written file and its order is a choice.
    pub fn from_json(text: &str) -> Result<Self> {
        let value: serde_json::Value = serde_json::from_str(text)
            .map_err(|e| Error::malformed("metadata spec", format!("not valid JSON: {e}")))?;
        let serde_json::Value::Object(map) = value else {
            return Err(Error::malformed(
                "metadata spec",
                "the top level must be an object of \"key\": \"path\"",
            ));
        };
        let mut spec = Self::new();
        for (key, value) in map {
            let serde_json::Value::String(path) = value else {
                return Err(Error::malformed(
                    "metadata spec",
                    format!("`{key}` must map to a string path, not {value}"),
                ));
            };
            spec.add(key, path);
        }
        Ok(spec)
    }

    /// Evaluate every key against an object.
    pub fn read(&self, object: &dyn Reflect, source: &dyn ObjectSource) -> Metadata {
        let mut values = BTreeMap::new();
        let mut order = Vec::new();
        let mut misses = Vec::new();
        for (key, path) in &self.entries {
            // Every key of the spec goes into `order`, found or not. A key that
            // resolved to nothing is still something the caller **asked for**,
            // and a format with a null — GeoJSON — should say so rather than
            // drop the column. See [`Metadata::entries`].
            if !order.iter().any(|existing| existing == key) {
                order.push(key.clone());
            }
            match value_at(object, path, source) {
                Some(found) if !found.is_empty() => {
                    values.insert(key.clone(), found);
                }
                _ => misses.push(key.clone()),
            }
        }
        Metadata {
            values,
            order,
            misses,
        }
    }
}

impl Spec {
    /// The path a key reads, for a caller inspecting a spec it was handed.
    pub fn get_path(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, path)| path.as_str())
    }
}

/// What a [`Spec`] found on one object.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Metadata {
    values: BTreeMap<String, String>,
    order: Vec<String>,
    misses: Vec<String>,
}

impl Metadata {
    /// The pairs, in the order the spec listed them.
    pub fn pairs(&self) -> Vec<(&str, &str)> {
        self.order
            .iter()
            .filter_map(|key| self.values.get(key).map(|v| (key.as_str(), v.as_str())))
            .collect()
    }

    /// Every key the spec asked for, in its order, with what it found.
    ///
    /// The difference from [`pairs`][Self::pairs] is the `None`, and it matters
    /// for one kind of output. A GeoJSON `FeatureCollection` is read as a
    /// **table**: consumers line the properties of every feature up in columns,
    /// and a key that simply vanishes on the features where it found nothing
    /// changes the shape of the table from row to row. `null` keeps the column
    /// and says the value is absent, which is the true statement.
    ///
    /// A comment block in an OBJ file is not a table, and a VTK field array
    /// cannot hold a null, so those two use `pairs` and write what they have.
    pub fn entries(&self) -> Vec<(&str, Option<&str>)> {
        self.order
            .iter()
            .map(|key| (key.as_str(), self.values.get(key).map(String::as_str)))
            .collect()
    }

    /// One value by key.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    /// How many were found.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether none were.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// The keys that found nothing.
    ///
    /// Not an error — see the module documentation — but worth showing, because
    /// a mistyped path and a legitimately absent value look identical in the
    /// output and only one of them is worth fixing.
    pub fn misses(&self) -> &[String] {
        &self.misses
    }

    /// Add a value the spec did not produce.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let key = key.into();
        // `order` now holds keys that found nothing too, so membership is what
        // decides — not whether a value was already there.
        if !self.order.contains(&key) {
            self.order.push(key.clone());
        }
        self.values.insert(key, value.into());
    }
}

/// Follow one path from an object, crossing references as needed.
///
/// `None` when any step leads nowhere — see the module documentation for why
/// that is a result rather than an error.
pub fn value_at(object: &dyn Reflect, path: &str, source: &dyn ObjectSource) -> Option<String> {
    let segments = path::parse(path).ok()?;
    walk(object, &segments, source, 0, Goal::Text)
}

/// The `type_key` of the **reference** a path ends on — same walk as
/// [`value_at`] (crossings and version respellings included), different last
/// step: instead of rendering the final object as its uuid, read which
/// concept its reference names.
///
/// This is what lets a business rule say *the interpreted feature of the
/// represented interpretation is a `SeismicLatticeFeature`* — a path that
/// crosses one reference and ends on another (`energyml_domain::rules`,
/// BRT4/BRT5's condition). `None` when the path does not end on a readable
/// reference.
pub fn target_key_at(object: &dyn Reflect, path: &str, source: &dyn ObjectSource) -> Option<String> {
    let segments = path::parse(path).ok()?;
    walk(object, &segments, source, 0, Goal::TargetKey)
}

/// What the last step of a walk should yield.
#[derive(Clone, Copy)]
enum Goal {
    /// The lexical value — a scalar as text, a final object as its uuid.
    Text,
    /// The `type_key` the final reference names. A path ending on a scalar
    /// yields nothing: a number is not a reference.
    TargetKey,
}

/// Recursive rather than iterative, and not for elegance: crossing a reference
/// produces an **owned** object that the rest of the walk borrows from, and a
/// loop would have to keep those alive in a side vector and borrow out of it.
/// A stack frame per hop does the same thing with no lifetime gymnastics.
fn walk(
    object: &dyn Reflect,
    segments: &[path::Segment],
    source: &dyn ObjectSource,
    hops: usize,
    goal: Goal,
) -> Option<String> {
    // A reference whose target references it back would otherwise recurse
    // forever. Real packages do contain cycles.
    if hops > 16 {
        return None;
    }
    let (segment, rest) = segments.split_first()?;
    let last = rest.is_empty();

    // Identity first: `uuid` and its friends are computed, not fields.
    if last
        && matches!(goal, Goal::Text)
        && let Some(value) = identity_value(object, &segment.name)
    {
        return Some(value);
    }

    match step(object, &segment.name, segment.index) {
        Some(Step::Value(text)) if last => match goal {
            Goal::Text => Some(text),
            Goal::TargetKey => None,
        },
        // A path that stops on an object: its identity is the only sensible
        // rendering — or, for a rule, the concept its reference names.
        Some(Step::Object(child)) if last => match goal {
            Goal::Text => identity_value(child, "uuid"),
            Goal::TargetKey => identity::key_of_reflect(child).map(|key| key.type_key().to_owned()),
        },
        Some(Step::Object(child)) => walk(child, rest, source, hops, goal),
        // A scalar with path left over: the caller asked to descend into a
        // number.
        Some(Step::Value(_)) => None,
        // Not a field here. If this object is a reference, the field may be on
        // what it points at — the whole reason this function exists.
        None => {
            identity::uuid_of_reflect(object).filter(|u| !u.is_empty())?;
            // By the full identity the reference carries, version included.
            let resolved = source.object_of(object)?;
            walk(resolved.as_reflect(), segments, source, hops + 1, goal)
        }
    }
}

enum Step<'a> {
    Value(String),
    Object(&'a dyn Reflect),
}

/// Elements the releases spell differently, grouped by what they mean.
///
/// The same rule the rest of the workspace follows — `energyml-geometry` has
/// `INTERPRETS`, `energyml-domain::color` has its own list — brought here
/// because a path is written by a person, once, and then run against packages
/// of every release. Without this, `RepresentedInterpretation.…` finds nothing
/// on a resqml 2.2 file and `RepresentedObject.…` finds nothing on a 2.0.1 one,
/// so a spec has to be forked per version to say one thing.
///
/// It bit the shipped spec: [`Spec::standard`] asks for
/// `RepresentedInterpretation.uuid`, which on every 2.2 package in the corpus
/// resolved to nothing at all — quietly, since a path that finds nothing is a
/// result and not an error.
const SYNONYMS: &[&[&str]] = &[
    &["RepresentedInterpretation", "RepresentedObject"],
    // A discrete property's category table: `Lookup` in RESQML 2.0.1,
    // `CategoryLookup` in 2.2. A business rule spelling only one of them would
    // say nothing about half the packages - and silently, as always here.
    &["Lookup", "CategoryLookup"],
];

/// The other spellings of a name, if it has any.
fn synonyms_of(name: &str) -> &'static [&'static str] {
    SYNONYMS
        .iter()
        .find(|group| group.iter().any(|spelling| spelling.eq_ignore_ascii_case(name)))
        .copied()
        .unwrap_or(&[])
}

/// The field under that name **or any spelling the releases use for it**.
///
/// Shared by the walk and by [`absent_at`], deliberately: a caller asking
/// *is this association empty?* and one asking *what does it hold?* must
/// agree on which field they are talking about, or the first says "missing"
/// about a field the second reads.
fn field_any_spelling<'a>(object: &'a dyn Reflect, name: &str) -> Option<FieldValue<'a>> {
    if let Some(field) = field_ignore_case(object, name) {
        return Some(field);
    }
    // Not under that name. The releases may spell it another way, and the
    // path was written against one of them.
    synonyms_of(name)
        .iter()
        .filter(|spelling| !spelling.eq_ignore_ascii_case(name))
        .find_map(|spelling| field_ignore_case(object, spelling))
}

/// Whether the path names something this object **declares** and that holds
/// nothing — an association the model allows and this document left out.
///
/// The distinction it exists for: a field that is absent and a path that
/// names no field at all are different news. The first is a document being
/// terse (and whether the schema required it is `Validate`'s business); the
/// second is a mistake in whoever wrote the path. Anything reasoning over
/// paths — a business rule, a metadata spec — needs to tell them apart, and
/// it cannot do that from a `None`.
///
/// Version spellings are followed here too, which is the whole reason this
/// is not two lines at the call site: `RepresentedInterpretation` names
/// nothing on a 2.2 grid, and reading that as "the rule is wrong" would
/// have made every 2.2 package unverifiable.
pub fn absent_at(object: &dyn Reflect, path: &str) -> bool {
    let Ok(segments) = path::parse(path) else {
        return false;
    };
    let mut current = object;
    for (at, segment) in segments.iter().enumerate() {
        let last = at + 1 == segments.len();
        match field_any_spelling(current, &segment.name) {
            // Declared, and holding nothing.
            Some(FieldValue::Null) => return true,
            Some(FieldValue::List(items)) if items.is_empty() => return true,
            Some(FieldValue::Object(child)) if !last => current = child,
            // Declared and holding something — nothing absent here.
            Some(_) => return false,
            // Not a field of this type under any spelling: not absence.
            None => return false,
        }
    }
    false
}

/// Whether the object has a field of this path's **first name**, under that
/// spelling and no other.
///
/// The escape hatch from [`SYNONYMS`], and it exists because the synonyms are
/// right nearly always and wrong in one case: when two releases disagree about
/// what an element may *hold*, one statement per release is two rules, and each
/// has to be able to say which spelling it is about. Without this,
/// `RepresentedInterpretation` — RESQML 2.0.1's stricter typing — would judge
/// every 2.2 package through the synonym and report as a violation exactly what
/// 2.2 added.
///
/// Only the first segment is looked at: a path's later segments are read on
/// another object, whose release is its own business.
pub fn declares_exactly(object: &dyn Reflect, path: &str) -> bool {
    let Ok(segments) = path::parse(path) else {
        return false;
    };
    let Some(first) = segments.first() else {
        return false;
    };
    field_ignore_case(object, &first.name).is_some()
}

fn step<'a>(object: &'a dyn Reflect, name: &str, index: Option<usize>) -> Option<Step<'a>> {
    let field = field_any_spelling(object, name)?;
    let field = match (field, index) {
        (FieldValue::List(items), Some(i)) => copy_of(items.get(i)?),
        (FieldValue::List(items), None) => copy_of(items.first()?),
        (other, _) => other,
    };
    match field {
        FieldValue::Scalar(scalar) => Some(Step::Value(scalar.to_string())),
        FieldValue::Object(child) => Some(Step::Object(child)),
        _ => None,
    }
}

/// `FieldValue` is not `Clone` — it borrows — but the two variants that matter
/// here are a scalar and an object reference, both of which are copyable.
fn copy_of<'a>(item: &FieldValue<'a>) -> FieldValue<'a> {
    match item {
        FieldValue::Scalar(scalar) => FieldValue::Scalar(*scalar),
        FieldValue::Object(object) => FieldValue::Object(*object),
        _ => FieldValue::Null,
    }
}

fn field_ignore_case<'a>(object: &'a dyn Reflect, name: &str) -> Option<FieldValue<'a>> {
    if let Some(value) = object.field(name) {
        return Some(value);
    }
    let info = object.type_info();
    let wanted = name.to_ascii_lowercase();
    for field in info.fields {
        if field.xml_name.to_ascii_lowercase() == wanted || field.name.to_ascii_lowercase() == wanted {
            if let Some(value) = object.field(field.xml_name).or_else(|| object.field(field.name)) {
                return Some(value);
            }
        }
    }
    None
}

/// The names that are computed from identity rather than read from a field.
fn identity_value(object: &dyn Reflect, name: &str) -> Option<String> {
    let key = identity::key_of_reflect(object);
    match name.to_ascii_lowercase().as_str() {
        "uuid" => identity::uuid_of_reflect(object)
            .map(str::to_string)
            .filter(|u| !u.is_empty()),
        "uri" | "etpuri" => key.as_ref().map(|k| k.etp_uri(None)),
        "contenttype" => key.as_ref().map(|k| k.content_type()),
        "qualifiedtype" => key.as_ref().map(|k| k.qualified_type()),
        "partname" => key.as_ref().map(|k| k.part_name()),
        "typename" => Some(object.type_info().name.to_string()),
        "typekey" => Some(object.type_info().type_key.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spec_keeps_what_it_was_given() {
        let mut spec = Spec::new();
        spec.add("a", "Citation.Title").add("b", "uuid");
        assert_eq!(spec.len(), 2);
        assert_eq!(spec.entries()[0].0, "a");
    }

    #[test]
    fn json_that_is_not_a_flat_object_is_refused() {
        // Silently ignoring a nested value would mean a spec that looks
        // configured and collects nothing.
        assert!(Spec::from_json("[]").is_err());
        assert!(Spec::from_json(r#"{"a": {"b": "c"}}"#).is_err());
        assert!(Spec::from_json(r#"{"a": 3}"#).is_err());
        assert!(Spec::from_json("not json at all").is_err());
    }

    #[test]
    fn json_round_trips_through_a_spec() {
        let spec = Spec::from_json(r#"{"title":"Citation.Title","who":"Citation.Originator"}"#).unwrap();
        assert_eq!(spec.len(), 2);
        assert_eq!(spec.get_path("title"), Some("Citation.Title"));
    }

    #[test]
    fn metadata_reports_what_it_could_not_find() {
        let mut found = Metadata::default();
        found.insert("a", "1");
        assert_eq!(found.get("a"), Some("1"));
        assert_eq!(found.pairs(), vec![("a", "1")]);
        assert!(found.misses().is_empty());
    }
}
