//! A PRODML flow test's measurements: the WITSML channels its data points at,
//! read as series against the test's own time channel.
//!
//! # Where the values are
//!
//! PRODML Technical Usage Guide v2.3 §13.6 p. 118: *"Channel is a WITSML
//! object; Channels are collected into Channel Sets and the Channel Set is
//! where the actual values are stored."* A flow test's
//! `IntervalMeasurementSet` lists its data in three groups —
//! `MeasuredFlowRate`, `MeasuredPressure`, `OtherData` — and each item names
//! a `ChannelSet`, a `TimeChannel` (*"The Channel containing the Time data"*,
//! FlowTestActivity.xsd l. 100–103) and its own data channel (`FlowChannel`,
//! `PressureChannel`, `DataChannel`, and `PressureDerivativeChannel` when
//! given). Every such field is found **by shape** — a reference whose element
//! name ends in `Channel` — so a subtype that names its channel otherwise is
//! read the same way.
//!
//! # A reference to a nested set resolves to the part that holds it
//!
//! A WITSML `Log` holds its channel sets inline, and PRODML references the
//! nested set by uuid (`energyml_core::identity::nested_objects`).
//! So the set is looked for among the objects given, as a part or nested in
//! one ([`super::resolve`]), then in the source. Not found is
//! [`Refused::ChannelSetNotFound`]; a channel the set does not list is
//! [`Refused::ChannelNotInSet`] — never a series of nothing.
//!
//! # The time is the test's, not the set's
//!
//! Each [`Measured`] series is indexed by the values of the `TimeChannel`, as
//! the test says, not by the set's index: they may coincide (`injection-test`
//! writes them equal) and nothing requires it. A time channel holds a
//! `date time` (read as Unix seconds) or an `elapsed time` (in its unit); any
//! other kind is [`Refused::TimeChannelKind`], and it must be complete and
//! strictly increasing — a time that goes back or is missing places no value.

use serde_json::{Value, json};

use energyml_core::value::{FieldValue, Reflect};

use super::{
    Channel, Direction, Index, IndexKind, Quality, Refused, Series, channel_set, monotone, objects,
    reference_of, text,
};
use crate::source::ArraySource;

/// The groups of `FlowTestMeasurementSet` that hold channel data, in the
/// order the schema lists them.
const GROUPS: &[&str] = &["MeasuredFlowRate", "MeasuredPressure", "OtherData"];

/// One series a flow test points at.
#[derive(Clone, Debug, PartialEq)]
pub struct Measured {
    /// The group it is listed in: `MeasuredFlowRate`, `MeasuredPressure`,
    /// `OtherData`.
    pub group: String,
    /// The field that names its channel: `FlowChannel`, `PressureChannel`,
    /// `DataChannel`, `PressureDerivativeChannel`…
    pub role: String,
    /// The item's remark, when it has one.
    pub remark: Option<String>,
    /// The channel set's uuid.
    pub channel_set: String,
    /// The values: one channel, against the test's time channel.
    pub series: Series,
}

impl Measured {
    /// As the bridges hand it to a front end.
    pub fn to_json(&self) -> Value {
        json!({
            "group": self.group,
            "role": self.role,
            "remark": self.remark,
            "channelSet": self.channel_set,
            "series": self.series.to_json(),
        })
    }
}

/// Whether this reads the object — any flow test activity, by the field
/// holding its measurements.
pub fn reads(object: &dyn Reflect) -> bool {
    object
        .type_info()
        .field_ignore_case("IntervalMeasurementSet")
        .is_some()
}

/// Every channel a flow test's measurements point at, each a series against
/// its time channel.
///
/// `objects` is every object of the package with its uuid — a channel set
/// nested in a `Log` is found through it; `source` lends a set that is a part
/// of its own and not among them, and the dataset a set's rows are in when
/// they are in one ([`channel_set::read_with`]).
pub fn read(
    test: &dyn Reflect,
    objects_given: &[(&str, &dyn Reflect)],
    source: &dyn ArraySource,
) -> Result<Vec<Measured>, Refused> {
    if !reads(test) {
        return Err(Refused::NotThisReader {
            expected: "a PRODML flow test activity",
            found: test.type_info().type_key.to_string(),
        });
    }
    // Each set is read once, whichever items name it.
    let mut read_sets: Vec<(String, Series)> = Vec::new();
    let mut out = Vec::new();

    for measurement_set in objects(test, "IntervalMeasurementSet") {
        for group in GROUPS {
            for item in objects(measurement_set, group) {
                let set_reflect = objects(item, "ChannelSet").into_iter().next();
                let set_ref = set_reflect
                    .and_then(reference_of)
                    .ok_or_else(|| Refused::Missing {
                        what: format!("the ChannelSet of a {group} item"),
                    })?;
                let time_ref = objects(item, "TimeChannel")
                    .into_iter()
                    .next()
                    .and_then(reference_of)
                    .ok_or_else(|| Refused::Missing {
                        what: format!("the TimeChannel of a {group} item"),
                    })?;
                // Each set is remembered by the uri its reference names, so two
                // versions of one channel set are two sets.
                let set_key = set_reflect
                    .and_then(energyml_core::identity::uri_of_reflect)
                    .unwrap_or_else(|| set_ref.0.clone());
                let set = match read_sets
                    .iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case(&set_key))
                {
                    Some((_, series)) => series.clone(),
                    None => {
                        let series = set_of(&set_ref, set_reflect, objects_given, source)?;
                        read_sets.push((set_key, series.clone()));
                        series
                    }
                };
                let time = channel_in(&set, &time_ref, "TimeChannel")?;
                let index = time_index(time)?;
                for (role, channel_ref) in channel_fields(item) {
                    let channel = channel_in(&set, &channel_ref, &role)?;
                    out.push(Measured {
                        group: group.to_string(),
                        role,
                        remark: text(item, "Remark"),
                        channel_set: set.uuid.clone(),
                        series: Series {
                            uuid: set.uuid.clone(),
                            title: set.title.clone(),
                            index: index.clone(),
                            secondary: Vec::new(),
                            channels: vec![channel.clone()],
                        },
                    });
                }
            }
        }
    }
    Ok(out)
}

/// The references of an item whose field name ends in `Channel`, other than
/// its `TimeChannel` and its `ChannelSet` — in schema order.
fn channel_fields(item: &dyn Reflect) -> Vec<(String, (String, String))> {
    let mut out = Vec::new();
    for field in item.type_info().fields {
        let name = field.xml_name;
        if !name.ends_with("Channel") || name == "TimeChannel" {
            continue;
        }
        if let Some(FieldValue::Object(child)) = item.field(field.name)
            && child.type_info().is_object_ref
            && let Some(found) = reference_of(child)
        {
            out.push((name.to_string(), found));
        }
    }
    out
}

/// The channel set a reference names, read.
fn set_of(
    set_ref: &(String, String),
    reference: Option<&dyn Reflect>,
    objects_given: &[(&str, &dyn Reflect)],
    source: &dyn ArraySource,
) -> Result<Series, Refused> {
    let not_found = || Refused::ChannelSetNotFound {
        uuid: set_ref.0.clone(),
        title: set_ref.1.clone(),
    };
    let among = match reference {
        Some(reference) => super::resolve_reference(reference, objects_given),
        None => super::resolve(&set_ref.0, objects_given),
    };
    if let Some(found) = among {
        if !channel_set::reads(found) {
            return Err(not_found());
        }
        return channel_set::read_with(found, source);
    }
    let lent = match reference {
        Some(reference) => source.object_of(reference),
        None => source.object(&set_ref.0),
    };
    match lent {
        Some(found) if channel_set::reads(found.as_reflect()) => {
            channel_set::read_with(found.as_reflect(), source)
        }
        _ => Err(not_found()),
    }
}

/// The channel a reference names, among the set's.
fn channel_in<'a>(set: &'a Series, reference: &(String, String), role: &str) -> Result<&'a Channel, Refused> {
    set.channels
        .iter()
        .find(|channel| {
            channel
                .uuid
                .as_deref()
                .is_some_and(|uuid| uuid.eq_ignore_ascii_case(&reference.0))
        })
        .ok_or_else(|| Refused::ChannelNotInSet {
            role: role.to_string(),
            channel: reference.0.clone(),
            title: reference.1.clone(),
            set: set.uuid.clone(),
        })
}

/// A time channel as an index — complete, strictly increasing, and a time.
fn time_index(channel: &Channel) -> Result<Index, Refused> {
    let kind = channel.data_kind.clone().unwrap_or_default();
    let kind = match kind.as_str() {
        "date time" => IndexKind::DateTime,
        "elapsed time" => IndexKind::ElapsedTime,
        _ => {
            return Err(Refused::TimeChannelKind {
                channel: channel.mnemonic.clone(),
                kind,
            });
        }
    };
    if let Some(at) = channel
        .quality
        .iter()
        .position(|quality| *quality != Quality::Good)
    {
        return Err(Refused::NullIndex {
            row: at + 1,
            index: channel.mnemonic.clone(),
        });
    }
    let values = channel.numbers().unwrap_or_default();
    let index = Index {
        kind,
        mnemonic: channel.mnemonic.clone(),
        uom: channel.uom.clone(),
        direction: Direction::Increasing,
        datum: None,
        values,
    };
    monotone(&index)?;
    Ok(index)
}
