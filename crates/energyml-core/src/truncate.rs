//! Cutting a JSON document down to a budget, and saying what was cut.
//!
//! An EnergyML object can be megabytes of JSON (a grid, a bundle of
//! representations), and a face that shows it to a person or a language model
//! has a size it can afford. Cutting silently is the failure to avoid: a
//! document that has lost its tail looks exactly like a short document. So
//! [`truncate`] never drops anything without a [`Cut`] naming where, why and
//! how much, by JSON pointer (RFC 6901), and what replaces a cut value is a
//! marker string that reads as a marker, not a plausible value.
//!
//! # The three budgets, in this order
//!
//! 1. **Depth** ([`Budget::max_depth`]): the root is level 0; an object or a
//!    list at a deeper level than `max_depth` is replaced by a marker string
//!    (`<resqml22.Foo: 5 keys cut at depth 3>`). The marker carries the
//!    `$type` of the object it replaces, so the type information the encoding
//!    is built on survives the cut (see [`crate::json`]).
//! 2. **List length** ([`Budget::max_list`]): a longer list keeps its first
//!    `max_list` items; the cut is recorded on the list's own pointer.
//! 3. **Bytes** ([`Budget::max_bytes`]): if the document, compactly
//!    serialized, is still larger, the largest values are replaced by
//!    `<cut: N bytes>` one at a time, always the largest value of the deepest
//!    branch that dominates, until it fits.
//!
//! The `$type` entry of an object is never cut by the byte pass, and an object
//! that has one is never replaced as a whole by it: it is the discriminant of
//! the document.
//!
//! # When the budget cannot be met
//!
//! Some documents cannot be brought under a small `max_bytes`: a root that is
//! a long string, an object of many tiny values. The result is then returned
//! as far as it could be cut and [`Truncation::exceeds`] is set; it is never
//! returned as if it fitted.
//!
//! # Cost
//!
//! The byte pass measures sizes again after every cut, so it is quadratic in
//! the worst case; the depth and list passes run first precisely so that what
//! it sees is already small.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

/// The key that names the type of an object in the EnergyML JSON encoding.
const TYPE_KEY: &str = "$type";

/// How much of a document to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Budget {
    /// Deepest level kept, the root being level 0. Containers deeper than this
    /// are replaced by a marker.
    pub max_depth: usize,
    /// Most items kept in any list.
    pub max_list: usize,
    /// Most bytes of the compact serialization (`serde_json::to_string`).
    pub max_bytes: usize,
}

impl Budget {
    /// A budget of `max_depth` levels, `max_list` items per list and
    /// `max_bytes` bytes.
    pub fn new(max_depth: usize, max_list: usize, max_bytes: usize) -> Self {
        Self {
            max_depth,
            max_list,
            max_bytes,
        }
    }
}

/// What was cut, by JSON pointer (RFC 6901), and why.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Truncation {
    /// Every cut, in the order made: depth and list cuts in document order,
    /// then the byte cuts.
    pub cut: Vec<Cut>,
    /// True when the document is still over `max_bytes` after every cut that
    /// could be made (see the module documentation).
    pub exceeds: bool,
}

impl Truncation {
    /// Whether nothing was cut and the budget was met.
    pub fn is_empty(&self) -> bool {
        self.cut.is_empty() && !self.exceeds
    }
}

/// One cut.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Cut {
    /// Where, as a JSON pointer into the **original** document (which is also
    /// the pointer into the result: a cut never shifts what precedes it).
    pub pointer: String,
    /// Why.
    pub reason: CutReason,
    /// Items removed (for [`CutReason::Depth`] the entries of the replaced
    /// container, for [`CutReason::ListLength`] the items beyond the limit) or
    /// bytes removed (for [`CutReason::Bytes`] the compact size of the value
    /// that was replaced).
    pub removed: usize,
}

/// Why a value was cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CutReason {
    /// Deeper than [`Budget::max_depth`].
    Depth,
    /// Beyond [`Budget::max_list`].
    ListLength,
    /// The document was over [`Budget::max_bytes`] and this was among its
    /// largest values.
    Bytes,
}

/// A copy of `value` within `budget`: deeper levels replaced by a short marker string,
/// lists shortened, then (if still over max_bytes when serialized) the largest values
/// cut until it fits. Every cut is listed. A value already within budget is returned
/// unchanged with an empty Truncation. The `$type` key of an object is never cut.
pub fn truncate(value: &Value, budget: &Budget) -> (Value, Truncation) {
    let mut truncation = Truncation::default();
    let mut markers = BTreeSet::new();
    let mut pointer = String::new();
    let mut out = shrink(value, 0, &mut pointer, budget, &mut truncation, &mut markers);

    let mut size = size_of(&out);
    while size > budget.max_bytes {
        match cut_largest(&mut out, &mut pointer, &mut truncation, &mut markers) {
            Some(saved) => size -= saved,
            None => {
                truncation.exceeds = true;
                break;
            }
        }
    }
    (out, truncation)
}

/// The depth and list passes, in one walk.
fn shrink(
    value: &Value,
    depth: usize,
    pointer: &mut String,
    budget: &Budget,
    truncation: &mut Truncation,
    markers: &mut BTreeSet<String>,
) -> Value {
    match value {
        Value::Object(map) => {
            if depth > budget.max_depth {
                let marker = match map.get(TYPE_KEY).and_then(Value::as_str) {
                    Some(qualified) => format!("<{qualified}: {} keys cut at depth {depth}>", map.len()),
                    None => format!("<object: {} keys cut at depth {depth}>", map.len()),
                };
                return replaced(marker, pointer, CutReason::Depth, map.len(), truncation, markers);
            }
            let mut out = Map::new();
            for (key, child) in map {
                let at = pointer.len();
                push_segment(pointer, key);
                out.insert(
                    key.clone(),
                    shrink(child, depth + 1, pointer, budget, truncation, markers),
                );
                pointer.truncate(at);
            }
            Value::Object(out)
        }
        Value::Array(items) => {
            if depth > budget.max_depth {
                let marker = format!("<list: {} items cut at depth {depth}>", items.len());
                return replaced(
                    marker,
                    pointer,
                    CutReason::Depth,
                    items.len(),
                    truncation,
                    markers,
                );
            }
            let kept = items.len().min(budget.max_list);
            if items.len() > kept {
                truncation.cut.push(Cut {
                    pointer: pointer.clone(),
                    reason: CutReason::ListLength,
                    removed: items.len() - kept,
                });
            }
            let mut out = Vec::with_capacity(kept);
            for (index, child) in items[..kept].iter().enumerate() {
                let at = pointer.len();
                push_segment(pointer, &index.to_string());
                out.push(shrink(child, depth + 1, pointer, budget, truncation, markers));
                pointer.truncate(at);
            }
            Value::Array(out)
        }
        other => other.clone(),
    }
}

/// A marker standing for a value, with its cut recorded.
fn replaced(
    marker: String,
    pointer: &str,
    reason: CutReason,
    removed: usize,
    truncation: &mut Truncation,
    markers: &mut BTreeSet<String>,
) -> Value {
    truncation.cut.push(Cut {
        pointer: pointer.to_string(),
        reason,
        removed,
    });
    markers.insert(pointer.to_string());
    Value::String(marker)
}

/// Which child of a container.
enum Slot {
    Key(String),
    Index(usize),
}

impl Slot {
    fn segment(&self) -> String {
        match self {
            Slot::Key(key) => escape(key),
            Slot::Index(index) => index.to_string(),
        }
    }
}

fn child_mut<'a>(node: &'a mut Value, slot: &Slot) -> &'a mut Value {
    match (node, slot) {
        (Value::Object(map), Slot::Key(key)) => map.get_mut(key).expect("a key just listed"),
        (Value::Array(items), Slot::Index(index)) => &mut items[*index],
        _ => unreachable!("a slot is only built from the container it indexes"),
    }
}

/// The byte pass, one cut: find the largest value that may be cut, going down
/// into the largest child while it is a container with something to cut in
/// it, and replace it by `<cut: N bytes>`. Returns the bytes saved, `None`
/// when nothing below `node` can be cut to a gain.
fn cut_largest(
    node: &mut Value,
    pointer: &mut String,
    truncation: &mut Truncation,
    markers: &mut BTreeSet<String>,
) -> Option<usize> {
    let is_marker = |pointer: &str, segment: &str| markers.contains(&format!("{pointer}/{segment}"));
    let (slot, size) = match &*node {
        Value::Object(map) => map
            .iter()
            // The discriminant is never a candidate, however long its value.
            .filter(|(key, _)| key.as_str() != TYPE_KEY)
            .filter(|(key, _)| !is_marker(pointer, &escape(key)))
            .map(|(key, child)| (Slot::Key(key.clone()), size_of(child)))
            .max_by_key(|(_, size)| *size)?,
        Value::Array(items) => items
            .iter()
            .enumerate()
            .filter(|(index, _)| !is_marker(pointer, &index.to_string()))
            .map(|(index, child)| (Slot::Index(index), size_of(child)))
            .max_by_key(|(_, size)| *size)?,
        _ => return None,
    };

    let at = pointer.len();
    pointer.push('/');
    pointer.push_str(&slot.segment());
    let child = child_mut(node, &slot);

    let has_something_inside = match &*child {
        Value::Object(_) => true,
        Value::Array(items) => items.iter().any(|item| item.is_object() || item.is_array()),
        _ => false,
    };
    let inner = if has_something_inside {
        cut_largest(child, pointer, truncation, markers)
    } else {
        None
    };
    if inner.is_some() {
        pointer.truncate(at);
        return inner;
    }

    // Nothing smaller to cut inside: the value itself, unless it is an object
    // that carries a `$type`, which is never replaced as a whole.
    let typed = matches!(&*child, Value::Object(map) if map.contains_key(TYPE_KEY));
    let marker = Value::String(format!("<cut: {size} bytes>"));
    let marker_size = size_of(&marker);
    let saved = if !typed && size > marker_size {
        truncation.cut.push(Cut {
            pointer: pointer.clone(),
            reason: CutReason::Bytes,
            removed: size,
        });
        markers.insert(pointer.clone());
        *child = marker;
        Some(size - marker_size)
    } else {
        None
    };
    pointer.truncate(at);
    saved
}

/// The length of the compact serialization of a value, without building it.
fn size_of(value: &Value) -> usize {
    match value {
        Value::Null => 4,
        Value::Bool(true) => 4,
        Value::Bool(false) => 5,
        Value::Number(number) => number.to_string().len(),
        Value::String(text) => string_size(text),
        Value::Array(items) => 2 + items.iter().map(size_of).sum::<usize>() + items.len().saturating_sub(1),
        Value::Object(map) => {
            2 + map
                .iter()
                .map(|(key, child)| string_size(key) + 1 + size_of(child))
                .sum::<usize>()
                + map.len().saturating_sub(1)
        }
    }
}

fn string_size(text: &str) -> usize {
    serde_json::to_string(text).map_or(text.len() + 2, |quoted| quoted.len())
}

/// RFC 6901 §3: `~` is `~0` and `/` is `~1`, in that order.
fn escape(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

fn push_segment(pointer: &mut String, segment: &str) {
    pointer.push('/');
    pointer.push_str(&escape(segment));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pointers(truncation: &Truncation, reason: CutReason) -> Vec<&str> {
        let mut out: Vec<&str> = truncation
            .cut
            .iter()
            .filter(|cut| cut.reason == reason)
            .map(|cut| cut.pointer.as_str())
            .collect();
        out.sort_unstable();
        out
    }

    fn compact(value: &Value) -> usize {
        serde_json::to_string(value).unwrap().len()
    }

    #[test]
    fn a_value_within_budget_is_returned_unchanged_with_nothing_said() {
        let value = json!({"$type": "a.B", "xs": [1, 2, 3], "c": {"$type": "a.C", "n": 1}});
        let (out, truncation) = truncate(&value, &Budget::new(5, 10, 10_000));
        assert_eq!(out, value);
        assert!(truncation.is_empty());

        // Negative: the same value under a budget it breaks is not "empty".
        let (_, truncation) = truncate(&value, &Budget::new(5, 2, 10_000));
        assert!(!truncation.is_empty());
    }

    #[test]
    fn size_of_is_the_length_of_the_compact_serialization() {
        // The byte pass trusts it, so it is checked against the serializer.
        let value = json!({
            "a": [1, -2.5, true, false, null, "tab\there \"quoted\" é"],
            "b/c": {"$type": "x.Y", "n": 1.0e10, "s": ""},
            "empty": [], "none": {}
        });
        assert_eq!(size_of(&value), compact(&value));
    }

    #[test]
    fn deeper_levels_become_a_marker_that_keeps_the_type() {
        let value = json!({"a": {"$type": "x.Y", "b": {"$type": "x.Z", "c": {"d": 1}, "e": 2}}});
        let (out, truncation) = truncate(&value, &Budget::new(1, 10, 10_000));
        assert_eq!(out["a"]["b"], json!("<x.Z: 3 keys cut at depth 2>"));
        assert_eq!(
            out["a"]["$type"],
            json!("x.Y"),
            "what is within depth is untouched"
        );
        assert_eq!(
            truncation.cut,
            vec![Cut {
                pointer: "/a/b".into(),
                reason: CutReason::Depth,
                removed: 3
            }]
        );

        // Negative: one level deeper, the cut moves down and `b` is kept.
        let (out, truncation) = truncate(&value, &Budget::new(2, 10, 10_000));
        assert_eq!(out["a"]["b"]["e"], json!(2));
        assert_eq!(out["a"]["b"]["c"], json!("<object: 1 keys cut at depth 3>"));
        assert_eq!(pointers(&truncation, CutReason::Depth), vec!["/a/b/c"]);
    }

    #[test]
    fn a_list_beyond_the_limit_is_shortened_and_the_cut_is_on_the_list() {
        let value = json!({"xs": [1, 2, 3, 4, 5], "ys": [[1, 2, 3], [4]]});
        let (out, truncation) = truncate(&value, &Budget::new(5, 2, 10_000));
        assert_eq!(out["xs"], json!([1, 2]));
        assert_eq!(out["ys"], json!([[1, 2], [4]]));
        assert_eq!(pointers(&truncation, CutReason::ListLength), vec!["/xs", "/ys/0"]);
        let removed = |pointer: &str| {
            truncation
                .cut
                .iter()
                .find(|c| c.pointer == pointer)
                .unwrap()
                .removed
        };
        assert_eq!((removed("/xs"), removed("/ys/0")), (3, 1));
        assert_eq!(truncation.cut.len(), 2);

        // Negative: a list of exactly the limit loses nothing.
        let (out, truncation) = truncate(&json!({"xs": [1, 2]}), &Budget::new(5, 2, 10_000));
        assert_eq!(out["xs"], json!([1, 2]));
        assert!(truncation.is_empty());
    }

    #[test]
    fn the_largest_values_are_cut_until_it_fits_and_each_cut_is_listed() {
        let value = json!({
            "$type": "a.B",
            "small": 1,
            "big": "x".repeat(500),
            "medium": "y".repeat(300),
        });
        let (out, truncation) = truncate(&value, &Budget::new(5, 10, 200));
        assert!(compact(&out) <= 200, "{}", compact(&out));
        assert!(!truncation.exceeds);
        assert_eq!(out["$type"], json!("a.B"));
        assert_eq!(out["small"], json!(1), "the small value is not touched");
        assert_eq!(pointers(&truncation, CutReason::Bytes), vec!["/big", "/medium"]);
        let big = truncation.cut.iter().find(|c| c.pointer == "/big").unwrap();
        assert_eq!(big.removed, 502, "the compact size of the string it replaced");
        assert!(out["big"].as_str().unwrap().starts_with("<cut: "));

        // Negative: with room for the medium one, only the big one goes.
        let (out, truncation) = truncate(&value, &Budget::new(5, 10, 400));
        assert_eq!(pointers(&truncation, CutReason::Bytes), vec!["/big"]);
        assert_eq!(out["medium"], value["medium"]);
    }

    #[test]
    fn the_byte_pass_goes_into_a_list_of_typed_objects_and_keeps_every_type() {
        let value = json!({"items": [
            {"$type": "a.B", "blob": "z".repeat(400)},
            {"$type": "a.B", "blob": "z".repeat(10)},
        ]});
        let (out, truncation) = truncate(&value, &Budget::new(5, 10, 150));
        assert_eq!(pointers(&truncation, CutReason::Bytes), vec!["/items/0/blob"]);
        assert_eq!(out["items"][0]["$type"], json!("a.B"));
        assert_eq!(out["items"][1], value["items"][1]);
        assert!(compact(&out) <= 150);
    }

    #[test]
    fn the_type_key_is_never_cut_even_when_it_is_the_largest_value() {
        let long = format!("{}.Foo", "very".repeat(30));
        let value = json!({"$type": long, "n": "x".repeat(30)});
        let (out, truncation) = truncate(&value, &Budget::new(5, 10, 160));
        assert_eq!(out["$type"], json!(long));
        assert!(!truncation.exceeds);
        assert_eq!(pointers(&truncation, CutReason::Bytes), vec!["/n"]);

        // Even with a budget nothing can meet, the typed object is not
        // replaced as a whole: the result says it is still too big.
        let (out, truncation) = truncate(&json!({"$type": long}), &Budget::new(5, 10, 10));
        assert_eq!(out["$type"], json!(long));
        assert!(truncation.exceeds);
        assert!(!truncation.is_empty());
    }

    #[test]
    fn a_budget_that_cannot_be_met_is_said_not_hidden() {
        let (out, truncation) = truncate(&json!("x".repeat(100)), &Budget::new(5, 10, 10));
        assert_eq!(out, json!("x".repeat(100)), "the root is not replaceable");
        assert!(truncation.exceeds);
        assert!(truncation.cut.is_empty());
    }

    #[test]
    fn pointers_escape_the_tilde_and_the_slash() {
        let value = json!({"a/b": {"m~n": {"deep": 1}}, "list/1": [1, 2, 3]});
        let (_, truncation) = truncate(&value, &Budget::new(1, 2, 10_000));
        assert_eq!(pointers(&truncation, CutReason::Depth), vec!["/a~1b/m~0n"]);
        assert_eq!(pointers(&truncation, CutReason::ListLength), vec!["/list~11"]);

        // Negative: a naive join would say `/a/b/m~n`, a pointer to another place.
        assert_ne!(pointers(&truncation, CutReason::Depth), vec!["/a/b/m~n"]);

        // And RFC 6901 §5's own example resolves back to the cut value.
        let pointer = &pointers(&truncation, CutReason::Depth)[0].to_string();
        assert_eq!(value.pointer(pointer), Some(&json!({"deep": 1})));
    }
}
