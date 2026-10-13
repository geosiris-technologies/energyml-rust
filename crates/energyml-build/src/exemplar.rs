//! *Like that one* — fill a form from an object already written.
//!
//! The need: there is sometimes a lot of information to fill in. Could we offer
//! to copy some information from an object similar to those about to be
//! produced? For example "extract the information from another well" — and the
//! interface would show, under the imported values, "copied from [TITLE]", with
//! the uuid in a tooltip.
//!
//! # What makes it generic
//!
//! Nothing here knows what a well is. Each [`Slot`] declares **where its value
//! lives on an object of the same kind** ([`Slot::reads`], reflective field
//! paths tried in order); this module reads them and returns what it
//! found. An operation therefore gains *copy from a similar object* by
//! naming paths, never by writing code — and a slot that declares
//! none is not copyable, which the form says by not offering it.
//!
//! # It takes JSON, so it stays pure
//!
//! The caller owns the storage and fetches the object; this module sees
//! only a value. This is the crate's rule — `plan()` reads nothing — applied one
//! step earlier, and it is what makes the whole testable without a package.
//!
//! # What it does not copy
//!
//! **A geometry.** *Like that other well* means *its settings*, never
//! *its trajectory*: a path that designated an external array would return a
//! file description, which the form would put in a number field.
//! A value read is retained only if it has the **shape** the slot expects
//! ([`Need::accepts`]) — the same check as `settle`, done here so that nothing
//! unreadable reaches the form.

use serde_json::Value;

use crate::operation::{Need, Slot};
use crate::pieces::Ref;

/// A value taken from a model object.
#[derive(Debug, Clone, PartialEq)]
pub struct Copied {
    /// The slot it fills.
    pub slot: String,
    /// The value, in the shape the slot expects.
    pub value: Value,
    /// The path it was read from, so that the caller can say so.
    pub path: &'static str,
}

/// Walk down a path `A/B/0/C` in a value.
///
/// A wholly numeric segment indexes an array; everything else is an object
/// key. `None` as soon as a link is missing — an object that does not carry the
/// field is not an error, it is an object with nothing to lend.
fn at<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut here = value;
    for step in path.split('/') {
        here = match step.parse::<usize>() {
            Ok(index) => here.get(index)?,
            Err(_) => here.get(step)?,
        };
    }
    Some(here)
}

/// What a model object can give to these slots.
///
/// Each slot is tried on its paths, in order; the first that returns
/// a value **of the right shape** wins. The order of the result is that of the
/// slots, so that a form presents them as it displays them.
///
/// An object reference (`DataObjectReference`) is copied as it is: the
/// slot that receives it is a `Need::Object`, `Need::Crs` or
/// `Need::Interpretation`, and that is exactly what the plan expects.
pub fn prefill(slots: &[Slot], object: &Value) -> Vec<Copied> {
    let mut out = Vec::new();
    for slot in slots {
        for path in slot.reads {
            let Some(found) = at(object, path) else { continue };
            if found.is_null() {
                continue;
            }
            // An external array is not a setting: refuse it here rather
            // than let a description object land in a field.
            if found.is_array()
                || found.get("Values").is_some()
                || found.get("ExternalDataArrayPart").is_some()
            {
                continue;
            }
            let value = coerce(&slot.need, found);
            if slot.need.accepts(&value).is_err() {
                continue;
            }
            out.push(Copied {
                slot: slot.name.to_string(),
                value,
                path,
            });
            break;
        }
    }
    out
}

/// The value read, brought to the slot's shape when the conversion loses nothing.
///
/// Two cases and only two: a number asked for, read as text — eml measures
/// carry their number in `#text` or as a string depending on the
/// encoder — and a **measure** (`{ value, uom }`), of which a number slot
/// wants the value and a text slot the unit. Everything else passes as it is:
/// guessing more would be guessing.
fn coerce(need: &Need, found: &Value) -> Value {
    if let Some(inner) = found.get("value").or_else(|| found.get("#text")) {
        return coerce(need, inner);
    }
    match need {
        // eml 2.3 has no boolean: a flag is written `true`/`false` as
        // text, and that is how it comes back.
        Need::Flag => match found.as_str() {
            Some("true") => Value::Bool(true),
            Some("false") => Value::Bool(false),
            _ => found.clone(),
        },
        Need::Number { .. } => match found {
            Value::String(text) => text
                .trim()
                .parse::<f64>()
                .map(Value::from)
                .unwrap_or_else(|_| found.clone()),
            other => other.clone(),
        },
        // **An integer read back from a measure comes back as a float.** eml 2.3 has no
        // integer parameter for an EPSG code — `parameter_kind` says
        // `double` — so an activity writes `2154.0`, and `Need::Epsg::accepts`
        // asks for an `as_i64()`, which returns `None` on a float. Refusing it
        // would lose a value the file states exactly; rounding it would
        // lose one the file does not state. Hence: a **whole** float
        // becomes an integer, any other passes as it is and is refused by its
        // slot, which is the right answer.
        Need::Epsg | Need::Integer { .. } => {
            let number = match found {
                Value::String(text) => text.trim().parse::<f64>().ok(),
                other => other.as_f64(),
            };
            match number {
                Some(value) if value.fract() == 0.0 && value.abs() < 9e15 => Value::from(value as i64),
                Some(value) => Value::from(value),
                None => found.clone(),
            }
        }
        Need::Text | Need::Title | Need::Choice(_) | Need::Type(_) | Need::Vocabulary(_) => match found {
            Value::String(_) => found.clone(),
            Value::Number(number) => Value::String(number.to_string()),
            other => other.clone(),
        },
        _ => found.clone(),
    }
}

/// What an **activity** of the package can give back to these slots.
///
/// # Why an activity is worth more than an object
///
/// An object lends what it *is*; an activity lends what it was **told**.
/// The simplification tolerance that was chosen is nowhere in the
/// polyline it produced — only the run knows. It is also the provenance loop
/// closed: the package explains how it was made, and that
/// explanation can be replayed.
///
/// The parameter titles **are** the slot names — `Registry::plan`
/// writes each settled choice under its own, and the template declares them all. What
/// matches no slot of *this* operation is silently ignored:
/// replaying the activity of another recipe is a legitimate gesture of which only
/// what makes sense here is kept.
///
/// An **output** parameter is not an answer: `output` names what the
/// run wrote, and taking it back as an input would make a form point to its
/// own result.
pub fn from_activity(slots: &[Slot], activity: &Value) -> Vec<Copied> {
    let empty = Vec::new();
    let parameters = activity["Parameter"].as_array().unwrap_or(&empty);
    let mut out = Vec::new();
    for slot in slots {
        if slot.name == "output" {
            continue;
        }
        let Some(found) = parameters.iter().find(|p| p["Title"].as_str() == Some(slot.name)) else {
            continue;
        };
        // The three forms `provenance::Parameter` writes, and nothing else:
        // a fourth that would be read by guesswork would be a guess.
        let raw = if !found["DataObject"].is_null() {
            found["DataObject"].clone()
        } else if !found["Value"].is_null() {
            found["Value"].clone()
        } else {
            continue;
        };
        let value = coerce(&slot.need, &raw);
        if slot.need.accepts(&value).is_err() {
            continue;
        }
        out.push(Copied {
            slot: slot.name.to_string(),
            value,
            path: "Parameter",
        });
    }
    out
}

/// The values taken, as a form reads them: the slot, the value, the
/// path, and **from where** — the model object, for the line *"copied from X"* and
/// its uuid in a tooltip.
pub fn json(copied: &[Copied], from: &Ref) -> Value {
    serde_json::json!({
        "from": from.dor(),
        "values": copied
            .iter()
            .map(|one| serde_json::json!({ "slot": one.slot, "value": one.value, "path": one.path }))
            .collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::Need;

    fn slots() -> Vec<Slot> {
        vec![
            Slot::required("mdUom", Need::Text, "").reads(&["NodeMd/Uom", "MdUom"]),
            Slot::optional("datum", Need::Object { types: &[] }, "").reads(&["MdDatum"]),
            Slot::optional(
                "ground",
                Need::Number {
                    uom: None,
                    least: None,
                    most: None,
                },
                "",
            )
            .reads(&["MdDatum/VerticalCoordinate"]),
            Slot::optional("shape", Need::Choice(&["linear", "cubic"]), "").reads(&["Shape"]),
            // Declares a path that targets an array: nothing must come out of it.
            Slot::optional("points", Need::Text, "").reads(&["Geometry/Points"]),
            // Declares none: not copyable.
            Slot::optional("title", Need::Title, ""),
        ]
    }

    fn a_wellbore() -> Value {
        serde_json::json!({
            "NodeMd": { "Uom": "ft", "Values": { "ExternalDataArrayPart": [] } },
            "MdDatum": { "$type": "eml23.DataObjectReference", "Uuid": "d-1", "QualifiedType": "eml23.ReferencePointInACrs", "Title": "KB" },
            "Shape": "cubic",
            "Geometry": { "Points": [1.0, 2.0, 3.0] },
        })
    }

    #[test]
    fn what_an_object_lends_is_what_the_slots_declare() {
        let copied = prefill(&slots(), &a_wellbore());
        let names: Vec<&str> = copied.iter().map(|one| one.slot.as_str()).collect();
        // `mdUom` by its first path, `datum` by the reference, `shape` by
        // the closed choice. `ground` is not there: the datum is a reference,
        // not the object, so its elevation is not in this file — it is *not
        // found*, and not zero.
        assert_eq!(names, vec!["mdUom", "datum", "shape"]);
        assert_eq!(copied[0].value, serde_json::json!("ft"));
        assert_eq!(copied[0].path, "NodeMd/Uom");
    }

    #[test]
    fn an_array_is_not_a_setting() {
        // The check that stops *like that other well* from meaning *its
        // trajectory*. Without it, `points` would come out with an array in it.
        let copied = prefill(&slots(), &a_wellbore());
        assert!(copied.iter().all(|one| one.slot != "points"));
    }

    #[test]
    fn a_slot_without_a_path_is_not_copyable() {
        let copied = prefill(&slots(), &a_wellbore());
        assert!(copied.iter().all(|one| one.slot != "title"));
    }

    #[test]
    fn a_value_of_the_wrong_shape_does_not_pass() {
        // `shape` is a closed choice: a value outside the list does not fill
        // the field — the same check as `settle`, done earlier.
        let mut object = a_wellbore();
        object["Shape"] = serde_json::json!("hyperbolic");
        let copied = prefill(&slots(), &object);
        assert!(copied.iter().all(|one| one.slot != "shape"));
    }

    #[test]
    fn a_measure_gives_its_number_to_a_number_slot() {
        let slots = vec![
            Slot::optional(
                "ground",
                Need::Number {
                    uom: None,
                    least: None,
                    most: None,
                },
                "",
            )
            .reads(&["Elevation"]),
        ];
        let object = serde_json::json!({ "Elevation": { "value": "23.5", "uom": "m" } });
        let copied = prefill(&slots, &object);
        assert_eq!(copied.len(), 1);
        assert_eq!(copied[0].value, serde_json::json!(23.5));
    }

    /// An activity as `Registry::plan` and `provenance::activity` write
    /// one.
    fn an_activity() -> Value {
        serde_json::json!({
            "$type": "eml23.Activity",
            "Parameter": [
                { "$type": "eml23.StringParameter", "Title": "mdUom", "Value": "ft" },
                { "$type": "eml23.StringParameter", "Title": "shape", "Value": "cubic" },
                { "$type": "eml23.DoubleQuantityParameter", "Title": "ground", "Value": 80.0, "Uom": "m" },
                { "$type": "eml23.DataObjectParameter", "Title": "datum", "DataObject": {
                    "$type": "eml23.DataObjectReference", "Uuid": "d-1",
                    "QualifiedType": "eml23.ReferencePointInACrs", "Title": "KB" } },
                { "$type": "eml23.DataObjectParameter", "Title": "output", "DataObject": {
                    "$type": "eml23.DataObjectReference", "Uuid": "o-1",
                    "QualifiedType": "resqml22.WellboreFrameRepresentation", "Title": "F12" } },
                { "$type": "eml23.StringParameter", "Title": "unknown", "Value": "x" },
            ],
        })
    }

    #[test]
    fn an_activity_gives_back_what_it_was_told() {
        // This is what an object cannot give back: `ground` and `shape` are
        // decisions, and the well they produced carries them nowhere.
        // To make it fail: read the parameters other than by the slot's
        // title, or forget one of the three forms that `Parameter` writes.
        let copied = from_activity(&slots(), &an_activity());
        let by = |name: &str| {
            copied
                .iter()
                .find(|one| one.slot == name)
                .map(|one| one.value.clone())
        };
        assert_eq!(by("mdUom"), Some(serde_json::json!("ft")));
        assert_eq!(by("shape"), Some(serde_json::json!("cubic")));
        assert_eq!(by("ground"), Some(serde_json::json!(80.0)));
        assert_eq!(
            by("datum").and_then(|v| v["Uuid"].as_str().map(str::to_string)),
            Some("d-1".to_string())
        );
    }

    #[test]
    fn what_a_run_wrote_is_not_an_answer() {
        // `output` names the result; taking it back as an input would make the
        // form point to its own product. And a parameter that no slot
        // names is ignored, so that an activity of another recipe is replayed
        // partially rather than not at all.
        let mut slots = slots();
        slots.push(Slot::optional("output", Need::Object { types: &[] }, "").reads(&["x"]));
        slots.push(Slot::optional("unknown", Need::Text, ""));
        let copied = from_activity(&slots, &an_activity());
        assert!(copied.iter().all(|one| one.slot != "output"));
        // `unknown` **is** taken: it matches a slot. What is ignored
        // is what no slot names — the check is here, the other way round.
        assert!(copied.iter().any(|one| one.slot == "unknown"));
    }

    #[test]
    fn an_activity_without_parameters_gives_nothing() {
        assert!(from_activity(&slots(), &serde_json::json!({ "$type": "eml23.Activity" })).is_empty());
    }

    #[test]
    fn an_epsg_code_read_back_from_a_measure_stays_a_code() {
        // The defect found while writing the round trip: eml 2.3 writes an EPSG
        // code as a `double`, so it comes back as `2154.0`, and `Need::Epsg` asks for an
        // integer. Without the conversion, *redo like this activity* silently lost
        // the one parameter that matters.
        let slots = vec![Slot::optional("epsg", Need::Epsg, "").reads(&["Code"])];
        assert_eq!(
            prefill(&slots, &serde_json::json!({ "Code": 2154.0 }))[0].value,
            serde_json::json!(2154)
        );
        // And a float that is not whole is not rounded: it is refused
        // by its slot, which is the right answer.
        assert!(prefill(&slots, &serde_json::json!({ "Code": 2154.5 })).is_empty());
    }

    #[test]
    fn an_object_that_carries_nothing_lends_nothing() {
        // And that is the ordinary case: an object of another kind is pointed at. An
        // empty result, never an error — nothing failed.
        assert!(prefill(&slots(), &serde_json::json!({ "Citation": { "Title": "x" } })).is_empty());
    }
}
