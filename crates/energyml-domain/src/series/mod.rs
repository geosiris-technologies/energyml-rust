//! Values against an index: what WITSML channel sets and PRODML time series
//! hold, read into **one** shape so a front end has one adapter to write.
//!
//! RESQML says *where* things are; WITSML and PRODML mostly say *when* and
//! *how much*. The two readers
//! below are **distinct** - their sources have nothing in common:
//!
//! | reader | source | form |
//! |---|---|---|
//! | [`channel_set`] | WITSML 2.1 `ChannelSet` | rows of a JSON string whose layout is **inferred** |
//! | [`time_series`] | PRODML 2.3 `TimeSeriesData` | typed samples, each with its `dTim` and `status` |
//!
//! and both answer with a [`Series`]: an index, and channels of values. Around
//! them:
//!
//! - [`flow_test`] follows a PRODML flow test's references to the WITSML
//!   channels that hold its measurements - through the part that holds a
//!   **nested** channel set (`energyml_core::identity::nested_objects`);
//! - [`decimate`] reduces a channel to a bounded number of buckets over a
//!   range, spikes and gaps kept - what the front end asks on every zoom;
//! - [`production`] reads a `WellProductionParameters` period by period, the
//!   quantity being the rate times the period's **own** duration;
//! - [`instant`] turns an `xsd:dateTime` into Unix seconds, or refuses it.
//!
//! # What is refused, and how
//!
//! Every refusal is a variant of [`Refused`], never a message to match on: a
//! caller - a test, the front end - asks *which* refusal, and the text is only for
//! a person. None of them is an empty series: a set that holds nothing is
//! [`Refused::NoData`], never zero rows that look like a log with nothing in
//! it (ADR-0005: refuse rather than draw).
//!
//! # A sample is good, absent, or flagged
//!
//! [`Quality`] per sample, because both sources say so and a chart must not
//! draw across what they say:
//!
//! - PRODML: *"If the status attribute is absent and the value is not "NaN",
//!   the data value can be assumed to be good with no restrictions"*
//!   (TimeSeriesData.xsd l. 100; Product Volume, Network Model & Time Series
//!   Usage Guide v2.3 §8.2 p. 31). So a `status` - `frozen` or any other - is
//!   **not** good, and a `NaN` is absent;
//! - WITSML: a `null` in a row (the Energistics JSON Style Guide p. 8: *"the
//!   arrays may have NULLs"*) is absent.
//!
//! A value whose quality is [`Quality::Absent`] is a **placeholder** (`NaN`
//! for a floating channel, the family's zero otherwise) and means nothing:
//! read [`Channel::quality`] before [`Channel::values`].

use serde_json::{Value, json};

use crate::property::PropertyKindRef;
use crate::values::{Family, Values};

pub mod channel_set;
pub mod decimate;
pub mod flow_test;
pub mod instant;
pub mod production;
pub mod rate_pressure;
pub mod time_series;

// ---------------------------------------------------------------------------
// The series
// ---------------------------------------------------------------------------

/// Values against one index - a WITSML channel set, a PRODML time series, the
/// channels a flow test points at.
#[derive(Clone, Debug, PartialEq)]
pub struct Series {
    /// The uuid of the object the values were read from - a channel set, a
    /// time series.
    pub uuid: String,
    /// Its title, as its citation gives it.
    pub title: String,
    /// The primary index: finite, and strictly monotone in its direction.
    pub index: Index,
    /// Secondary indexes, of the primary's family (WITSML TRG v2.1 §7.5
    /// p. 150: *"The first index is the primary index. Any additional indexes
    /// are secondary indexes."*). Their values are neither checked for order
    /// nor required: `NaN` where a row holds `null`.
    pub secondary: Vec<Index>,
    /// The channels, in the order the source lists them.
    pub channels: Vec<Channel>,
}

impl Series {
    /// How many samples each channel holds.
    pub fn len(&self) -> usize {
        self.index.values.len()
    }

    /// Whether it holds none.
    pub fn is_empty(&self) -> bool {
        self.index.values.is_empty()
    }

    /// A channel by mnemonic, exactly as written.
    pub fn channel(&self, mnemonic: &str) -> Option<&Channel> {
        self.channels.iter().find(|channel| channel.mnemonic == mnemonic)
    }

    /// The series as the bridges hand it to a front end.
    ///
    /// One conversion, here, so the CLI, wasm and Python write the same JSON.
    /// `null` stands for every sample that is not a number - JSON has no `NaN`,
    /// and a gap written as `0` is a spike through the middle of a chart.
    pub fn to_json(&self) -> Value {
        json!({
            "uuid": self.uuid,
            "title": self.title,
            "index": self.index.to_json(),
            "secondary": self.secondary.iter().map(Index::to_json).collect::<Vec<_>>(),
            "channels": self.channels.iter().map(Channel::to_json).collect::<Vec<_>>(),
        })
    }
}

/// What an index counts. Only the four a row can be read against: the other
/// `DataIndexKind`s (`temperature`, `pressure`, `scalar`) are neither time nor
/// depth, and `pass indexed depth` has no JSON form stated anywhere this
/// repository holds - both refused by name in [`channel_set`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexKind {
    /// `date time` - values are UTC Unix seconds.
    DateTime,
    /// `elapsed time` - values are a duration in the index's unit.
    ElapsedTime,
    /// `measured depth`.
    MeasuredDepth,
    /// `true vertical depth`.
    TrueVerticalDepth,
}

impl IndexKind {
    /// The kind as the schema spells it (eml 2.3 `DataIndexKind`).
    pub fn wire(self) -> &'static str {
        match self {
            IndexKind::DateTime => "date time",
            IndexKind::ElapsedTime => "elapsed time",
            IndexKind::MeasuredDepth => "measured depth",
            IndexKind::TrueVerticalDepth => "true vertical depth",
        }
    }

    /// The kind a wire string names, when it is one of the four.
    pub fn from_wire(text: &str) -> Option<IndexKind> {
        match text {
            "date time" => Some(IndexKind::DateTime),
            "elapsed time" => Some(IndexKind::ElapsedTime),
            "measured depth" => Some(IndexKind::MeasuredDepth),
            "true vertical depth" => Some(IndexKind::TrueVerticalDepth),
            _ => None,
        }
    }

    /// Time or depth - WITSML TRG v2.1 §7.7 p. 151: *"A 'compatible' index
    /// simply means that all of the channels are either in time or in depth
    /// using a common datum."*
    pub fn family(self) -> IndexFamily {
        match self {
            IndexKind::DateTime | IndexKind::ElapsedTime => IndexFamily::Time,
            IndexKind::MeasuredDepth | IndexKind::TrueVerticalDepth => IndexFamily::Depth,
        }
    }
}

/// In time, or in depth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexFamily {
    /// A date-time or an elapsed time.
    Time,
    /// A measured or a vertical depth.
    Depth,
}

impl IndexFamily {
    fn word(self) -> &'static str {
        match self {
            IndexFamily::Time => "time",
            IndexFamily::Depth => "depth",
        }
    }
}

/// Which way an index runs.
///
/// `Unordered`, the third value of eml 2.3 `IndexDirection`, is only ever a
/// **secondary** index's: a primary declared so is refused by name
/// ([`Refused::UnorderedIndex`]) rather than sorted - sorting rows the file
/// did not sort is a decision made in its place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Each value greater than the one before.
    Increasing,
    /// Each value less than the one before.
    Decreasing,
    /// No order stated - a secondary index only.
    Unordered,
}

impl Direction {
    /// As the schema spells it.
    pub fn wire(self) -> &'static str {
        match self {
            Direction::Increasing => "increasing",
            Direction::Decreasing => "decreasing",
            Direction::Unordered => "unordered",
        }
    }
}

/// One index: what it counts, in what, and its values.
#[derive(Clone, Debug, PartialEq)]
pub struct Index {
    /// What it counts.
    pub kind: IndexKind,
    /// Its mnemonic - `dTim` for a PRODML time series, which names none.
    pub mnemonic: String,
    /// The unit of [`Index::values`]. For [`IndexKind::DateTime`] it is always
    /// `s` - the values are Unix seconds - whatever the file declared: the
    /// file's values are ISO text, which has no unit.
    pub uom: Option<String>,
    /// Which way it runs.
    pub direction: Direction,
    /// The uuid of the datum a depth index counts from, when it names one.
    pub datum: Option<String>,
    /// The values, one per sample.
    pub values: Vec<f64>,
}

impl Index {
    fn to_json(&self) -> Value {
        json!({
            "kind": self.kind.wire(),
            "family": self.kind.family().word(),
            "mnemonic": self.mnemonic,
            "uom": self.uom,
            "direction": self.direction.wire(),
            "datum": self.datum,
            "values": self.values.iter().map(|value| number(*value)).collect::<Vec<_>>(),
        })
    }
}

/// How far a sample can be trusted, as its source says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Quality {
    /// Nothing says otherwise.
    Good,
    /// No value: a `null` in a WITSML row, a `NaN` in a PRODML sample with no
    /// status.
    Absent,
    /// A value the source qualifies - a PRODML `status` (`frozen`, `bad`, …),
    /// or an infinity (`"+Infinity"`, JSON Style Guide p. 8), which is a value
    /// no chart can place.
    Flagged(String),
}

impl Quality {
    /// Whether the value can be drawn and computed with.
    pub fn is_good(&self) -> bool {
        matches!(self, Quality::Good)
    }
}

/// One channel of values.
#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    /// Its uuid - a WITSML channel's; `None` for the one channel of a PRODML
    /// time series, which is the series itself.
    pub uuid: Option<String>,
    /// Its mnemonic - a PRODML series' title, which has none.
    pub mnemonic: String,
    /// Its unit, as written.
    pub uom: Option<String>,
    /// The WITSML `ChannelDataKind`, as written (`double`, `date time`, …) -
    /// `date time` values are Unix seconds. `None` for PRODML.
    pub data_kind: Option<String>,
    /// The property kind the channel names, when it names one.
    pub property_kind: Option<PropertyKindRef>,
    /// The PRODML `MeasureClass`, when the series states one.
    pub measure_class: Option<String>,
    /// The values; read [`Channel::quality`] first.
    pub values: Values,
    /// One per value.
    pub quality: Vec<Quality>,
}

/// What a channel holds, counted - the arithmetic a face prints without doing.
#[derive(Clone, Debug, PartialEq)]
pub struct Tally {
    /// Samples whose quality is good.
    pub good: usize,
    /// Samples with no value.
    pub absent: usize,
    /// Samples a status or an infinity qualifies, by label, in the order met.
    pub flagged: Vec<(String, usize)>,
    /// The least good value, for a numeric channel.
    pub min: Option<f64>,
    /// The greatest good value, for a numeric channel.
    pub max: Option<f64>,
}

impl Channel {
    /// The values as numbers, for a floating or integer channel.
    pub fn numbers(&self) -> Option<Vec<f64>> {
        match &self.values {
            Values::Floating(values) => Some(values.clone()),
            Values::Integer(values) => Some(values.iter().map(|value| *value as f64).collect()),
            _ => None,
        }
    }

    /// Good, absent and flagged samples, and the extremes of the good ones.
    pub fn tally(&self) -> Tally {
        let mut tally = Tally {
            good: 0,
            absent: 0,
            flagged: Vec::new(),
            min: None,
            max: None,
        };
        let numbers = self.numbers();
        for (at, quality) in self.quality.iter().enumerate() {
            match quality {
                Quality::Good => {
                    tally.good += 1;
                    if let Some(value) = numbers
                        .as_ref()
                        .map(|all| all[at])
                        .filter(|value| value.is_finite())
                    {
                        tally.min = Some(tally.min.map_or(value, |least: f64| least.min(value)));
                        tally.max = Some(tally.max.map_or(value, |most: f64| most.max(value)));
                    }
                }
                Quality::Absent => tally.absent += 1,
                Quality::Flagged(label) => match tally.flagged.iter_mut().find(|(known, _)| known == label) {
                    Some((_, count)) => *count += 1,
                    None => tally.flagged.push((label.clone(), 1)),
                },
            }
        }
        tally
    }

    fn to_json(&self) -> Value {
        let absent = |at: usize| matches!(self.quality.get(at), Some(Quality::Absent));
        let values: Vec<Value> = match &self.values {
            Values::Floating(values) => values.iter().map(|value| number(*value)).collect(),
            Values::Integer(values) => values
                .iter()
                .enumerate()
                .map(|(at, value)| if absent(at) { Value::Null } else { json!(value) })
                .collect(),
            Values::Boolean(values) => values
                .iter()
                .enumerate()
                .map(|(at, value)| if absent(at) { Value::Null } else { json!(value) })
                .collect(),
            Values::Text(values) => values
                .iter()
                .enumerate()
                .map(|(at, value)| if absent(at) { Value::Null } else { json!(value) })
                .collect(),
        };
        // The flagged samples as runs, [from, to): a series of a million
        // minutes carries a handful of frozen windows, not a million labels.
        let mut flags: Vec<Value> = Vec::new();
        let mut at = 0;
        while at < self.quality.len() {
            if let Quality::Flagged(label) = &self.quality[at] {
                let from = at;
                while at < self.quality.len() && self.quality[at] == Quality::Flagged(label.clone()) {
                    at += 1;
                }
                flags.push(json!({ "from": from, "to": at, "quality": label }));
            } else {
                at += 1;
            }
        }
        json!({
            "uuid": self.uuid,
            "mnemonic": self.mnemonic,
            "uom": self.uom,
            "dataKind": self.data_kind,
            "propertyKind": self.property_kind.as_ref().map(kind_json),
            "measureClass": self.measure_class,
            "family": family_word(self.values.family()),
            "values": values,
            "flags": flags,
        })
    }
}

fn kind_json(kind: &PropertyKindRef) -> Value {
    match kind {
        PropertyKindRef::Object { uuid, title } => json!({ "uuid": uuid, "title": title }),
        PropertyKindRef::Standard { name } => json!({ "name": name }),
    }
}

fn family_word(family: Family) -> &'static str {
    match family {
        Family::Floating => "floating",
        Family::Integer => "integer",
        Family::Boolean => "boolean",
        Family::Text => "text",
    }
}

/// A number JSON can carry, or `null`.
pub(crate) fn number(value: f64) -> Value {
    if value.is_finite() {
        json!(value)
    } else {
        Value::Null
    }
}

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

/// Everything a series reader refuses to guess - each named, none an empty
/// series.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum Refused {
    /// The object is not what this reader reads.
    #[error("`{found}` is not what this reader reads ({expected})")]
    NotThisReader {
        /// What the reader reads.
        expected: &'static str,
        /// What it was handed.
        found: String,
    },

    /// No reader in this build reads that type.
    #[error("no series reader in this build reads a `{found}`")]
    NoReader {
        /// The type met.
        found: String,
    },

    /// The uuid names no object among those given, at any depth.
    #[error("no object `{uuid}` among those given, as a part or nested in one")]
    NotFound {
        /// The uuid asked for.
        uuid: String,
    },

    /// A PRODML series mixing double and string samples.
    #[error("sample {at} is a {found} where sample 1 is a {first}: one series holds one kind of value")]
    MixedValueKinds {
        /// The first sample's kind.
        first: String,
        /// The 1-based rank of the first sample of another kind.
        at: usize,
        /// Its kind.
        found: String,
    },

    /// A sample with no instant in a series of several - TimeSeriesData.xsd
    /// l. 100: *"If no time is specified then the value is static and only one
    /// sample can be defined."*
    #[error(
        "sample {at} of {samples} has no dTim, and only a series of one sample may be static (TimeSeriesData.xsd l. 100)"
    )]
    UntimedSample {
        /// Its 1-based rank.
        at: usize,
        /// How many samples the series holds.
        samples: usize,
    },

    /// An instant that does not read.
    #[error("{at}: `{text}` is not a date-time this reads - {why}")]
    Instant {
        /// Where it was met.
        at: String,
        /// The text.
        text: String,
        /// What is wrong with it.
        why: String,
    },

    /// An index that does not run the way it says.
    #[error("the index `{index}` is not {direction} at sample {at}: {previous} then {next}")]
    NotMonotone {
        /// The index's mnemonic.
        index: String,
        /// The direction it declares.
        direction: &'static str,
        /// The 1-based rank of the sample that breaks it.
        at: usize,
        /// The value before.
        previous: f64,
        /// The value that breaks it.
        next: f64,
    },

    /// A primary index value that is missing.
    #[error("row {row}: the index `{index}` holds no value, and a row is placed by its index")]
    NullIndex {
        /// The 1-based row.
        row: usize,
        /// The index's mnemonic.
        index: String,
    },

    /// A channel set declaring no index.
    #[error("the channel set declares no index: its rows cannot be split into index and channels")]
    NoIndex,

    /// An index declared `unordered`.
    #[error(
        "the index `{index}` is declared unordered: this reader draws an index in order and does not sort rows the file did not sort"
    )]
    UnorderedIndex {
        /// Its mnemonic.
        index: String,
    },

    /// An index that is neither time nor depth.
    #[error(
        "the index `{index}` is a `{kind}` index: a channel set is in time or in depth (WITSML TRG v2.1 §7.7 p. 151)"
    )]
    IndexNotTimeOrDepth {
        /// Its mnemonic.
        index: String,
        /// Its kind, as written.
        kind: String,
    },

    /// A `pass indexed depth`, whose JSON form no document here states.
    #[error(
        "`{what}` is a pass indexed depth, whose form in a row no document this repository holds states (the WITSML v2.1 release notes are not among this repository's reference documents)"
    )]
    PassIndexedDepth {
        /// The index or channel.
        what: String,
    },

    /// Time and depth indexes in one set - WITSML TRG v2.1 §7.7 p. 151.
    #[error(
        "the index `{other}` is in {other_family} where the primary `{primary}` is in {primary_family}: a channel set is in time or in depth, never both (WITSML TRG v2.1 §7.7 p. 151)"
    )]
    IndexFamilyMix {
        /// The primary index.
        primary: String,
        /// Its family.
        primary_family: &'static str,
        /// The index of the other family.
        other: String,
        /// Its family.
        other_family: &'static str,
    },

    /// The bulk data is in a file this build does not read.
    #[error(
        "the data is in `{uri}`, a file this build does not read (FileUri; WITSML TRG v2.1 §7.2 p. 146: when it is set, Data is ignored)"
    )]
    FileUri {
        /// The file named.
        uri: String,
    },

    /// A channel set carrying no data.
    #[error(
        "the channel set carries no ChannelData: there is nothing to read, which is not a series of zero rows"
    )]
    NoData,

    /// Rows in an HDF5 dataset - the extension of ADR-0013 - that the array
    /// source does not locate: a source that holds no arrays, or one that does
    /// not know the form.
    #[error(
        "the rows are the dataset `{path}` of `{file}` (FileUri, and a dataset path in Data: an extension beyond WITSML 2.1), and the array source given locates no such array"
    )]
    DatasetNotLocated {
        /// The file `FileUri` names.
        file: String,
        /// The dataset `Data` names.
        path: String,
    },

    /// That dataset could not be read because this build has no backend for
    /// its format - a missing **capability**, never missing data.
    #[error(
        "the rows are the dataset `{path}` of `{file}`, and reading it needs the `{feature}` feature, which this build does not have - a missing capability, not missing data"
    )]
    DatasetNeedsFeature {
        /// The file.
        file: String,
        /// The dataset.
        path: String,
        /// The Cargo feature that would read it.
        feature: String,
    },

    /// That dataset could not be read - no such file, no such dataset, not an
    /// HDF5 file.
    #[error("the rows are the dataset `{path}` of `{file}`, and it did not read: {why}")]
    DatasetNotRead {
        /// The file.
        file: String,
        /// The dataset.
        path: String,
        /// What the source said.
        why: String,
    },

    /// A dataset that is not one row per index value and one column per index
    /// then per channel.
    #[error(
        "the dataset `{path}` is {dimensions:?} where a row per sample of {indexes} index(es) and {channels} channel(s) makes [rows, {expected}]"
    )]
    DatasetShape {
        /// The dataset.
        path: String,
        /// Its dimensions, as the source gives them.
        dimensions: Vec<i64>,
        /// The row width expected.
        expected: usize,
        /// Indexes declared.
        indexes: usize,
        /// Channels listed.
        channels: usize,
    },

    /// A column a numeric dataset cannot hold - a date-time, a string, a
    /// boolean.
    #[error(
        "`{column}` holds `{kind}`, and a column of an HDF5 dataset holds numbers: this form carries numeric indexes and channels only"
    )]
    DatasetColumnKind {
        /// The index or channel.
        column: String,
        /// Its kind, as written.
        kind: String,
    },

    /// `Data` is not JSON, or not an array of rows.
    #[error("ChannelData.Data does not read as rows: {why}")]
    NotRows {
        /// What went wrong.
        why: String,
    },

    /// A row whose length is not *indexes + channels*.
    #[error(
        "row {row} holds {found} value(s) where {indexes} index(es) and {channels} channel(s) make {expected}"
    )]
    RowLength {
        /// The 1-based row.
        row: usize,
        /// Its length.
        found: usize,
        /// The length expected.
        expected: usize,
        /// Indexes declared.
        indexes: usize,
        /// Channels listed.
        channels: usize,
    },

    /// A channel whose own index says something else than the set's.
    #[error("the channel `{channel}` is indexed with {what} `{found}` where its set says `{expected}`")]
    ChannelIndexContradicts {
        /// The channel's mnemonic.
        channel: String,
        /// What differs: kind, unit, direction, datum.
        what: &'static str,
        /// The set's.
        expected: String,
        /// The channel's.
        found: String,
    },

    /// A data kind this reader has no reading for.
    #[error("the channel `{channel}` holds `{kind}`, a data kind this reader does not know")]
    UnknownDataKind {
        /// The channel's mnemonic.
        channel: String,
        /// The kind as written.
        kind: String,
    },

    /// A value of the wrong JSON kind for its column.
    #[error("row {row}, `{column}`: {found} where {expected} is expected")]
    ValueKind {
        /// The 1-based row.
        row: usize,
        /// The index or channel.
        column: String,
        /// What its kind wants.
        expected: &'static str,
        /// What the row holds.
        found: String,
    },

    /// Something refused inside one channel set of a log.
    #[error("in the channel set `{set}`: {why}")]
    InSet {
        /// The set's uuid.
        set: String,
        /// The refusal.
        why: Box<Refused>,
    },

    /// A flow test's channel set, nowhere to be found.
    #[error(
        "the channel set `{uuid}` ({title}) is neither a part of its own nor nested in one of the objects given"
    )]
    ChannelSetNotFound {
        /// Its uuid.
        uuid: String,
        /// The title the reference caches.
        title: String,
    },

    /// A flow test naming a channel its set does not list.
    #[error("{role} names the channel `{channel}` ({title}), which the channel set `{set}` does not list")]
    ChannelNotInSet {
        /// The field that names it.
        role: String,
        /// The channel's uuid.
        channel: String,
        /// The title the reference caches.
        title: String,
        /// The set's uuid.
        set: String,
    },

    /// A rate and a pressure indexed on different instants - pairing them
    /// would interpolate one ([`rate_pressure`]).
    #[error(
        "the rate is indexed by `{first}` and the pressure by `{second}`, on different instants: pairing them would interpolate one of the two"
    )]
    DifferentInstants {
        /// The rate's index mnemonic.
        first: String,
        /// The pressure's index mnemonic.
        second: String,
    },

    /// A flow test's `TimeChannel` that holds no time.
    #[error("the time channel `{channel}` holds `{kind}`: a time is a `date time` or an `elapsed time`")]
    TimeChannelKind {
        /// The channel's mnemonic.
        channel: String,
        /// Its data kind.
        kind: String,
    },

    /// A series whose asset identifier cannot say which well it names.
    #[error("`{uri}`: {why}")]
    AssetIdentifier {
        /// The key's value.
        uri: String,
        /// What is wrong with it.
        why: String,
    },

    /// Several asset identifiers naming different wells.
    #[error("the series names several wells by its asset identifiers: {names:?}")]
    SeveralWellsNamed {
        /// The names met.
        names: Vec<String>,
    },

    /// A series naming no well at all.
    #[error("the series names no well: no `asset identifier` key with a `well(…)` part")]
    NoWellNamed,

    /// A well with no name to match by.
    #[error("`{uuid}` has neither a title nor a legal name to be matched by")]
    Unnamed {
        /// Its uuid.
        uuid: String,
    },

    /// Two objects answering to one name - *a name is not an identity*.
    #[error("`{name}` names {} objects ({}): a name is not an identity", .uuids.len(), .uuids.join(", "))]
    NameNotAnIdentity {
        /// The name.
        name: String,
        /// Every object answering to it.
        uuids: Vec<String>,
    },

    /// Nothing matched.
    #[error("no {what} answers to {names:?}")]
    NoMatch {
        /// What was looked for.
        what: &'static str,
        /// The names tried.
        names: Vec<String>,
    },

    /// Several things matched.
    #[error("{} {what}s answer to {names:?} ({}): which one is meant is stated nowhere", .uuids.len(), .uuids.join(", "))]
    SeveralMatch {
        /// What was looked for.
        what: &'static str,
        /// The names tried.
        names: Vec<String>,
        /// Every match.
        uuids: Vec<String>,
    },

    /// Decimating a channel that holds no numbers.
    #[error("the channel `{channel}` holds {family} values, and only numbers are decimated")]
    NotNumeric {
        /// Its mnemonic.
        channel: String,
        /// Its family.
        family: &'static str,
    },

    /// No channel at that rank.
    #[error("the series holds {channels} channel(s); there is no channel {at}")]
    NoSuchChannel {
        /// The 0-based rank asked.
        at: usize,
        /// How many there are.
        channels: usize,
    },

    /// A range or a bucket count that decimates nothing.
    #[error("decimating over [{from}, {to}] in {buckets} bucket(s): {why}")]
    BadWindow {
        /// Start of the range.
        from: f64,
        /// End of the range.
        to: f64,
        /// Buckets asked.
        buckets: usize,
        /// What is wrong.
        why: &'static str,
    },

    /// A unit this reader will not compute with.
    #[error("{what} is in `{uom}`: {why}")]
    Unit {
        /// Where the unit was met.
        what: String,
        /// The unit.
        uom: String,
        /// Why it is refused.
        why: String,
    },

    /// A required field absent.
    #[error("{what} is missing")]
    Missing {
        /// The field.
        what: String,
    },
}

// ---------------------------------------------------------------------------
// One entry point, one reader per kind of object
// ---------------------------------------------------------------------------

/// What reading an object gives.
#[derive(Clone, Debug, PartialEq)]
pub enum Readout {
    /// A channel set, or every channel set of a log.
    Sets(Vec<Series>),
    /// A PRODML time series - boxed, being the largest by far.
    TimeSeries(Box<time_series::TimeSeries>),
    /// The channels a flow test points at.
    FlowTest(Vec<flow_test::Measured>),
    /// Production periods.
    Periods(Vec<production::Period>),
}

impl Readout {
    /// Every series it holds, in order - what a face iterates to draw or to
    /// decimate, whichever reader answered.
    pub fn series(&self) -> Vec<&Series> {
        match self {
            Readout::Sets(sets) => sets.iter().collect(),
            Readout::TimeSeries(series) => match &series.content {
                time_series::Content::Samples(samples) => vec![samples],
                _ => Vec::new(),
            },
            Readout::FlowTest(measured) => measured.iter().map(|one| &one.series).collect(),
            Readout::Periods(_) => Vec::new(),
        }
    }

    /// As the bridges hand it to a front end.
    pub fn to_json(&self) -> Value {
        match self {
            Readout::Sets(sets) => {
                json!({ "kind": "sets", "sets": sets.iter().map(Series::to_json).collect::<Vec<_>>() })
            }
            Readout::TimeSeries(series) => json!({ "kind": "timeSeries", "timeSeries": series.to_json() }),
            Readout::FlowTest(measured) => {
                json!({ "kind": "flowTest", "measured": measured.iter().map(flow_test::Measured::to_json).collect::<Vec<_>>() })
            }
            Readout::Periods(periods) => {
                json!({ "kind": "periods", "periods": periods.iter().map(production::Period::to_json).collect::<Vec<_>>() })
            }
        }
    }
}

/// One reader: which objects it takes, and how it reads them.
pub struct Reader {
    /// A name for listings.
    pub name: &'static str,
    /// Whether it reads this object - by shape, never by version.
    pub reads: fn(&dyn energyml_core::value::Reflect) -> bool,
    /// Read it; `objects` and `source` lend what it references.
    pub read: ReadFn,
}

/// How a [`Reader`] reads: the object, every object of the package with its
/// uuid, and the source that lends the rest - objects, and the arrays a
/// channel set's rows may be (`channel_set`, *Rows in an HDF5 dataset*).
pub type ReadFn = fn(
    &dyn energyml_core::value::Reflect,
    &[(&str, &dyn energyml_core::value::Reflect)],
    &dyn crate::source::ArraySource,
) -> Result<Readout, Refused>;

/// The shipped readers - a slice rather than a `match`, like every list here
/// that will grow: a new reader is an entry and a function.
pub fn readers() -> &'static [Reader] {
    READERS
}

const READERS: &[Reader] = &[
    Reader {
        name: "channel set",
        reads: channel_set::reads,
        read: |object, _, source| Ok(Readout::Sets(vec![channel_set::read_with(object, source)?])),
    },
    Reader {
        name: "log",
        reads: channel_set::reads_log,
        read: |object, _, source| Ok(Readout::Sets(channel_set::read_log_with(object, source)?)),
    },
    Reader {
        name: "time series",
        reads: time_series::reads,
        read: |object, _, _| Ok(Readout::TimeSeries(Box::new(time_series::read(object)?))),
    },
    Reader {
        name: "flow test",
        reads: flow_test::reads,
        read: |object, objects, source| Ok(Readout::FlowTest(flow_test::read(object, objects, source)?)),
    },
    Reader {
        name: "production periods",
        reads: production::reads,
        read: |object, _, _| Ok(Readout::Periods(production::read(object)?)),
    },
];

/// Read the object `uuid` - a part among `objects`, one nested in them, or one
/// `source` lends - with the first reader that takes it.
///
/// `objects` is every object of the package the caller has read, with its
/// uuid, as [`crate::integrity::run`] takes them: a nested channel set is
/// reached through the part that holds it, and a flow test's references
/// resolve among them.
///
/// By uuid, so a package holding the object in two versions answers the first
/// it meets; [`read_at`] is the one that names the version.
pub fn read(
    uuid: &str,
    objects: &[(&str, &dyn energyml_core::value::Reflect)],
    source: &dyn crate::source::ArraySource,
) -> Result<Readout, Refused> {
    read_found(
        resolve(uuid, objects),
        || source.object(uuid),
        uuid,
        objects,
        source,
    )
}

/// [`read`] by **uri**: the version it names is the version read, among
/// `objects` and in `source` alike.
pub fn read_at(
    uri: &str,
    objects: &[(&str, &dyn energyml_core::value::Reflect)],
    source: &dyn crate::source::ArraySource,
) -> Result<Readout, Refused> {
    let uuid = energyml_core::identifiers::EtpUri::parse(uri)
        .ok()
        .and_then(|parsed| parsed.uuid)
        .unwrap_or_else(|| uri.to_string());
    read_found(
        resolve_at(uri, objects),
        || source.object_at(uri),
        &uuid,
        objects,
        source,
    )
}

fn read_found(
    found: Option<&dyn energyml_core::value::Reflect>,
    lent: impl FnOnce() -> Option<Box<dyn energyml_core::registry::DynObject>>,
    uuid: &str,
    objects: &[(&str, &dyn energyml_core::value::Reflect)],
    source: &dyn crate::source::ArraySource,
) -> Result<Readout, Refused> {
    let with = |object: &dyn energyml_core::value::Reflect| -> Result<Readout, Refused> {
        let reader = READERS
            .iter()
            .find(|reader| (reader.reads)(object))
            .ok_or_else(|| Refused::NoReader {
                found: object.type_info().type_key.to_string(),
            })?;
        (reader.read)(object, objects, source)
    };
    if let Some(object) = found {
        return with(object);
    }
    match lent() {
        Some(object) => with(object.as_reflect()),
        None => Err(Refused::NotFound {
            uuid: uuid.to_string(),
        }),
    }
}

/// The object `uuid` among `objects`: a part of its own first, then one
/// nested in a part (`energyml_core::identity::nested_objects`): a reference
/// to a nested data object resolves to the part that holds it.
pub fn resolve<'a>(
    uuid: &str,
    objects: &[(&'a str, &'a dyn energyml_core::value::Reflect)],
) -> Option<&'a dyn energyml_core::value::Reflect> {
    resolve_named(uuid, None, objects)
}

/// [`resolve`] by **uri**: when the uri names a version, only that version
/// answers; a uri that names none takes the uuid.
pub fn resolve_at<'a>(
    uri: &str,
    objects: &[(&'a str, &'a dyn energyml_core::value::Reflect)],
) -> Option<&'a dyn energyml_core::value::Reflect> {
    match energyml_core::identifiers::EtpUri::parse(uri) {
        Ok(parsed) => resolve_named(parsed.uuid.as_deref()?, parsed.object_version.as_deref(), objects),
        Err(_) => resolve_named(uri, None, objects),
    }
}

/// [`resolve`] for a **reference**: the object it names, at the version it
/// names - the way a reader follows a `DataObjectReference` among the objects
/// it has in hand.
pub fn resolve_reference<'a>(
    reference: &dyn energyml_core::value::Reflect,
    objects: &[(&'a str, &'a dyn energyml_core::value::Reflect)],
) -> Option<&'a dyn energyml_core::value::Reflect> {
    let uuid = energyml_core::identity::uuid_of_reflect(reference)?;
    let version = energyml_core::identity::object_version_of_reflect(reference)
        .map(str::trim)
        .filter(|version| !version.is_empty());
    resolve_named(uuid, version, objects)
}

/// The object of `uuid` the wanted version names, among the objects in hand -
/// `energyml_core::identity::version_fit`, the one rule: the exact version
/// first, then one that contradicts nothing.
fn resolve_named<'a>(
    uuid: &str,
    version: Option<&str>,
    objects: &[(&'a str, &'a dyn energyml_core::value::Reflect)],
) -> Option<&'a dyn energyml_core::value::Reflect> {
    use energyml_core::identity::{VersionFit, object_version_of_reflect, version_fit};

    let mut compatible: Option<&'a dyn energyml_core::value::Reflect> = None;
    for (held, object) in objects {
        if !held.eq_ignore_ascii_case(uuid) {
            continue;
        }
        match version_fit(version, object_version_of_reflect(*object)) {
            VersionFit::Exact => return Some(*object),
            VersionFit::Compatible => {
                compatible.get_or_insert(*object);
            }
            VersionFit::Contradicts => {}
        }
    }
    if compatible.is_some() {
        return compatible;
    }
    for (_, object) in objects {
        for nested in energyml_core::identity::nested_objects(*object) {
            if !nested.key.uuid.eq_ignore_ascii_case(uuid)
                || !energyml_core::identity::versions_agree(version, nested.key.object_version)
            {
                continue;
            }
            if let Ok(energyml_core::value::FieldValue::Object(found)) =
                energyml_core::path::get(*object, &nested.path)
            {
                return Some(found);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Reading by shape
// ---------------------------------------------------------------------------

use energyml_core::value::{FieldValue, Reflect, Scalar};

/// A scalar field as its wire text; `None` when absent or blank.
pub(crate) fn text(object: &dyn Reflect, name: &str) -> Option<String> {
    match object.field(name)? {
        FieldValue::Scalar(Scalar::Str(value)) => {
            Some(value.to_string()).filter(|value| !value.trim().is_empty())
        }
        FieldValue::Scalar(scalar) => Some(scalar.to_string()),
        _ => None,
    }
}

/// A numeric scalar field.
pub(crate) fn number_of(object: &dyn Reflect, name: &str) -> Option<f64> {
    match object.field(name)? {
        FieldValue::Scalar(Scalar::Int(value)) => Some(value as f64),
        FieldValue::Scalar(Scalar::UInt(value)) => Some(value as f64),
        FieldValue::Scalar(Scalar::Double(value)) => Some(value),
        FieldValue::Scalar(Scalar::Str(value)) => value.trim().parse().ok(),
        _ => None,
    }
}

/// The objects of a field, one or repeated.
pub(crate) fn objects<'a>(object: &'a dyn Reflect, name: &str) -> Vec<&'a dyn Reflect> {
    match object.field(name) {
        Some(FieldValue::Object(child)) => vec![child],
        Some(FieldValue::List(items)) => items
            .into_iter()
            .filter_map(|item| match item {
                FieldValue::Object(child) => Some(child),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// A reference held by a field: its uuid and the title it caches.
pub(crate) fn reference(object: &dyn Reflect, name: &str) -> Option<(String, String)> {
    let child = objects(object, name).into_iter().next()?;
    reference_of(child)
}

/// What a reference names: its uuid and cached title.
pub(crate) fn reference_of(child: &dyn Reflect) -> Option<(String, String)> {
    let key = energyml_core::identity::key_of_reflect(child)?;
    if key.uuid.trim().is_empty() {
        return None;
    }
    Some((key.uuid.to_string(), key.title.unwrap_or_default().to_string()))
}

/// The uuid and title of an object.
pub(crate) fn identity_of(object: &dyn Reflect) -> (String, String) {
    (
        energyml_core::identity::uuid_of_reflect(object)
            .unwrap_or_default()
            .to_string(),
        energyml_core::identity::title_of_reflect(object)
            .unwrap_or_default()
            .to_string(),
    )
}

/// Refuse an index that does not run the way it says - strictly: two rows at
/// one index are two values at one place, and which one holds is stated
/// nowhere.
pub(crate) fn monotone(index: &Index) -> Result<(), Refused> {
    for (at, pair) in index.values.windows(2).enumerate() {
        let ordered = match index.direction {
            Direction::Increasing => pair[1] > pair[0],
            Direction::Decreasing => pair[1] < pair[0],
            Direction::Unordered => true,
        };
        if !ordered {
            return Err(Refused::NotMonotone {
                index: index.mnemonic.clone(),
                direction: index.direction.wire(),
                at: at + 2,
                previous: pair[0],
                next: pair[1],
            });
        }
    }
    Ok(())
}
