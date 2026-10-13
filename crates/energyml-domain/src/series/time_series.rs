//! PRODML 2.3 `TimeSeriesData`: typed samples, each with its instant and its
//! status, read into a [`Series`] - and matched to a well by its keys.
//!
//! # The shape
//!
//! `DataValue` 0..n, each a `DoubleValue` or a `StringValue` (TimeSeriesData.xsd
//! l. 87), holding a value, an optional `dTim` and - for a double only - an
//! optional `status`. What the schema says of them, on `dTim` (l. 100, and
//! l. 119 for strings): *"If no time is specified then the value is static and
//! only one sample can be defined. […] If the status attribute is absent and
//! the value is not "NaN", the data value can be assumed to be good with no
//! restrictions."* So:
//!
//! - one sample and no instant is a **static** value ([`Content::Static`]),
//!   several samples of which one has no instant are refused
//!   ([`Refused::UntimedSample`]);
//! - a status makes a sample [`Quality::Flagged`] - `frozen` *"indicates the
//!   sensor has been reading the same value for a specific period of time"*
//!   (Product Volume, Network Model & Time Series Usage Guide v2.3 §8.2 p. 31),
//!   which is not a measurement of that minute; a `NaN` with no status is
//!   [`Quality::Absent`];
//! - doubles and strings in one series are refused ([`Refused::MixedValueKinds`]):
//!   the unit and measure class of the series cannot mean both;
//! - instants must **strictly increase**. The schema does not order them; a
//!   reader that draws a series against its instants does, and sorting what
//!   the file did not sort is a decision about the data made in its place.
//!
//! An empty series is [`Content::Empty`] - said, never a series of nothing.
//!
//! # A series names its well by text
//!
//! `Key` is a `KeywordValueStruct` (TimeSeriesData.xsd l. 67) - text, **no
//! reference**. The
//! convention that ties it to a well is the house rule
//! `house/time-series-asset-resolves` of [`crate::integrity`]: the `well(…)`
//! part of an `asset identifier` URI names a well by its title or its
//! `NameLegal`. [`well_of`] and [`of_well`] read it with **the same two
//! functions** that rule uses, and refuse by name when zero or several match -
//! a name is not an identity.

use serde_json::{Value, json};

use energyml_core::value::Reflect;

use super::{
    Channel, Direction, Index, IndexKind, Quality, Refused, Series, identity_of, instant, monotone, objects,
    text,
};
use crate::values::Values;

/// A PRODML time series, read.
#[derive(Clone, Debug, PartialEq)]
pub struct TimeSeries {
    /// Its uuid.
    pub uuid: String,
    /// Its title.
    pub title: String,
    /// Its keys, `(keyword, value)`, in the order written.
    pub keys: Vec<(String, String)>,
    /// Its unit.
    pub uom: Option<String>,
    /// Its measure class.
    pub measure_class: Option<String>,
    /// Its comment.
    pub comment: Option<String>,
    /// What it holds.
    pub content: Content,
}

/// What a time series holds.
#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    /// Samples against their instants.
    Samples(Series),
    /// One sample with no instant: *"the value is static"*.
    Static {
        /// The value, one of it.
        value: Values,
        /// Its quality.
        quality: Quality,
    },
    /// No sample at all - the schema allows it (`DataValue` 0..n).
    Empty,
}

impl TimeSeries {
    /// The well its `asset identifier` names, if it names one.
    pub fn well_named(&self) -> Result<Option<String>, Refused> {
        well_in_keys(
            self.keys
                .iter()
                .map(|(keyword, value)| (keyword.as_str(), value.as_str())),
        )
    }

    /// As the bridges hand it to a front end.
    pub fn to_json(&self) -> Value {
        let content = match &self.content {
            Content::Samples(series) => json!({ "kind": "samples", "series": series.to_json() }),
            Content::Static { value, quality } => {
                let value = match value {
                    Values::Floating(v) => v.first().map(|one| super::number(*one)).unwrap_or(Value::Null),
                    Values::Integer(v) => json!(v.first()),
                    Values::Boolean(v) => json!(v.first()),
                    Values::Text(v) => json!(v.first()),
                };
                let quality = match quality {
                    Quality::Good => json!("good"),
                    Quality::Absent => json!("absent"),
                    Quality::Flagged(label) => json!(label),
                };
                json!({ "kind": "static", "value": value, "quality": quality })
            }
            Content::Empty => json!({ "kind": "empty" }),
        };
        json!({
            "uuid": self.uuid,
            "title": self.title,
            "keys": self.keys.iter().map(|(keyword, value)| json!({ "keyword": keyword, "value": value })).collect::<Vec<_>>(),
            "uom": self.uom,
            "measureClass": self.measure_class,
            "comment": self.comment,
            "content": content,
        })
    }
}

/// Whether this reads the object - a `TimeSeriesData`, by its type name.
pub fn reads(object: &dyn Reflect) -> bool {
    object.type_info().type_key == "TimeSeriesData"
}

/// Read a `TimeSeriesData`.
pub fn read(object: &dyn Reflect) -> Result<TimeSeries, Refused> {
    if !reads(object) {
        return Err(Refused::NotThisReader {
            expected: "a PRODML TimeSeriesData",
            found: object.type_info().type_key.to_string(),
        });
    }
    let (uuid, title) = identity_of(object);
    let keys: Vec<(String, String)> = objects(object, "Key")
        .into_iter()
        .map(|key| {
            (
                text(key, "keyword").unwrap_or_default(),
                text(key, "value").unwrap_or_default(),
            )
        })
        .collect();
    let uom = text(object, "Uom");
    let measure_class = text(object, "MeasureClass");
    let comment = text(object, "Comment");

    // -- the samples, as the schema types them ---------------------------------
    struct Sample {
        kind: &'static str,
        number: f64,
        text: String,
        at: Option<String>,
        status: Option<String>,
    }
    let mut samples: Vec<Sample> = Vec::new();
    for (rank, value) in objects(object, "DataValue").into_iter().enumerate() {
        let (kind, inner_name) = match value.type_info().type_key {
            "DoubleValue" => ("double", "DoubleValue"),
            "StringValue" => ("string", "StringValue"),
            other => {
                return Err(Refused::MixedValueKinds {
                    first: "double or string".into(),
                    at: rank + 1,
                    found: other.to_string(),
                });
            }
        };
        let inner = objects(value, inner_name)
            .into_iter()
            .next()
            .ok_or_else(|| Refused::Missing {
                what: format!("sample {}: its {inner_name}", rank + 1),
            })?;
        samples.push(Sample {
            kind,
            number: super::number_of(inner, "value").unwrap_or(f64::NAN),
            text: text(inner, "value").unwrap_or_default(),
            at: text(inner, "dTim"),
            status: text(inner, "status"),
        });
    }
    if let Some(first) = samples.first()
        && let Some(other) = samples.iter().position(|one| one.kind != first.kind)
    {
        return Err(Refused::MixedValueKinds {
            first: first.kind.into(),
            at: other + 1,
            found: samples[other].kind.into(),
        });
    }

    let quality_of = |sample: &Sample| -> Quality {
        if let Some(status) = &sample.status {
            Quality::Flagged(status.clone())
        } else if sample.kind == "double" && sample.number.is_nan() {
            Quality::Absent
        } else if sample.kind == "double" && sample.number.is_infinite() {
            Quality::Flagged(if sample.number > 0.0 {
                "+Infinity".into()
            } else {
                "-Infinity".into()
            })
        } else {
            Quality::Good
        }
    };
    let values_of = |samples: &[Sample]| -> Values {
        match samples.first().map(|one| one.kind) {
            Some("string") => Values::Text(samples.iter().map(|one| one.text.clone()).collect()),
            _ => Values::Floating(samples.iter().map(|one| one.number).collect()),
        }
    };

    let content = match samples.as_slice() {
        [] => Content::Empty,
        [only] if only.at.is_none() => Content::Static {
            value: values_of(&samples),
            quality: quality_of(only),
        },
        _ => {
            if let Some(at) = samples.iter().position(|one| one.at.is_none()) {
                return Err(Refused::UntimedSample {
                    at: at + 1,
                    samples: samples.len(),
                });
            }
            let mut instants = Vec::with_capacity(samples.len());
            for (rank, sample) in samples.iter().enumerate() {
                let written = sample.at.as_deref().unwrap_or_default();
                let seconds = instant::unix_seconds(written).map_err(|why| Refused::Instant {
                    at: format!("sample {}", rank + 1),
                    text: written.to_string(),
                    why,
                })?;
                instants.push(seconds);
            }
            let index = Index {
                kind: IndexKind::DateTime,
                mnemonic: "dTim".into(),
                uom: Some("s".into()),
                direction: Direction::Increasing,
                datum: None,
                values: instants,
            };
            monotone(&index)?;
            let channel = Channel {
                uuid: None,
                mnemonic: title.clone(),
                uom: uom.clone(),
                data_kind: None,
                property_kind: None,
                measure_class: measure_class.clone(),
                values: values_of(&samples),
                quality: samples.iter().map(quality_of).collect(),
            };
            Content::Samples(Series {
                uuid: uuid.clone(),
                title: title.clone(),
                index,
                secondary: Vec::new(),
                channels: vec![channel],
            })
        }
    };
    Ok(TimeSeries {
        uuid,
        title,
        keys,
        uom,
        measure_class,
        comment,
        content,
    })
}

// ---------------------------------------------------------------------------
// A series and its well
// ---------------------------------------------------------------------------

/// The well the `asset identifier` keys name, read as
/// `house/time-series-asset-resolves` reads it; `None` when no key names one.
fn well_in_keys<'a>(keys: impl Iterator<Item = (&'a str, &'a str)>) -> Result<Option<String>, Refused> {
    let mut names: Vec<String> = Vec::new();
    for (keyword, value) in keys {
        if keyword != "asset identifier" {
            continue;
        }
        match crate::integrity::well_named_by(value) {
            Ok(Some(name)) => {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
            Ok(None) => {}
            Err(why) => {
                return Err(Refused::AssetIdentifier {
                    uri: value.to_string(),
                    why,
                });
            }
        }
    }
    match names.len() {
        0 => Ok(None),
        1 => Ok(names.pop()),
        _ => Err(Refused::SeveralWellsNamed { names }),
    }
}

/// The well a series object names, read off its keys without reading its
/// samples.
fn well_named_by_object(series: &dyn Reflect) -> Result<Option<String>, Refused> {
    let keys: Vec<(String, String)> = objects(series, "Key")
        .into_iter()
        .map(|key| {
            (
                text(key, "keyword").unwrap_or_default(),
                text(key, "value").unwrap_or_default(),
            )
        })
        .collect();
    well_in_keys(
        keys.iter()
            .map(|(keyword, value)| (keyword.as_str(), value.as_str())),
    )
}

/// The well a series names, among `objects` (every object of the package,
/// with its uuid) - a WITSML `Well` answering to the name by its title or its
/// `NameLegal`, exactly as `house/time-series-asset-resolves` looks.
///
/// Refused when the series names no well, when no well or several answer.
pub fn well_of<'a>(
    series: &dyn Reflect,
    objects: &[(&'a str, &'a dyn Reflect)],
) -> Result<(&'a str, &'a dyn Reflect), Refused> {
    let name = well_named_by_object(series)?.ok_or(Refused::NoWellNamed)?;
    let found: Vec<(&'a str, &'a dyn Reflect)> = objects
        .iter()
        .filter(|(_, object)| object.type_info().type_key == "Well")
        .filter(|(_, object)| crate::integrity::names_of(*object).contains(&name))
        .copied()
        .collect();
    match found.as_slice() {
        [one] => Ok(*one),
        [] => Err(Refused::NoMatch {
            what: "well",
            names: vec![name],
        }),
        several => Err(Refused::NameNotAnIdentity {
            name,
            uuids: several.iter().map(|(uuid, _)| uuid.to_string()).collect(),
        }),
    }
}

/// The one time series of `objects` that names `well` - a `Well` or a
/// `Wellbore`, matched by its title and `NameLegal` - and, when given, has
/// that measure class.
///
/// Refused by name:
///
/// - the object has no name to match by;
/// - another object of the same type answers to one of its names - the series
///   could mean either (*a name is not an identity*);
/// - no series, or several, answer. A well commonly has several series - a
///   head pressure and a head temperature - so a caller that wants one says
///   which by `measure_class`; with none, several is a refusal and never the
///   first.
///
/// A series whose asset identifier does not read is skipped here: it names no
/// well this function could match, and the house rule of `check` is where it
/// is reported.
pub fn of_well<'a>(
    objects: &[(&'a str, &'a dyn Reflect)],
    well: &dyn Reflect,
    measure_class: Option<&str>,
) -> Result<(&'a str, &'a dyn Reflect), Refused> {
    let (uuid, _) = identity_of(well);
    let names = crate::integrity::names_of(well);
    if names.is_empty() {
        return Err(Refused::Unnamed { uuid });
    }
    let kind = well.type_info().type_key;
    for name in &names {
        let rivals: Vec<String> = objects
            .iter()
            .filter(|(_, object)| object.type_info().type_key == kind)
            .filter(|(held, _)| !held.eq_ignore_ascii_case(&uuid))
            .filter(|(_, object)| crate::integrity::names_of(*object).contains(name))
            .map(|(held, _)| held.to_string())
            .collect();
        if !rivals.is_empty() {
            let mut uuids = vec![uuid.clone()];
            uuids.extend(rivals);
            return Err(Refused::NameNotAnIdentity {
                name: name.clone(),
                uuids,
            });
        }
    }
    let found: Vec<(&'a str, &'a dyn Reflect)> = objects
        .iter()
        .filter(|(_, object)| reads(*object))
        .filter(
            |(_, object)| matches!(well_named_by_object(*object), Ok(Some(named)) if names.contains(&named)),
        )
        .filter(|(_, object)| {
            measure_class.is_none_or(|wanted| text(*object, "MeasureClass").as_deref() == Some(wanted))
        })
        .copied()
        .collect();
    match found.as_slice() {
        [one] => Ok(*one),
        [] => Err(Refused::NoMatch {
            what: "time series",
            names,
        }),
        several => Err(Refused::SeveralMatch {
            what: "time series",
            names,
            uuids: several.iter().map(|(uuid, _)| uuid.to_string()).collect(),
        }),
    }
}
