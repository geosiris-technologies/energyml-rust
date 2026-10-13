//! WITSML 2.1 `ChannelSet`: the rows of `ChannelData.Data`, read into a
//! [`Series`].
//!
//! # The row layout is inferred, not cited
//!
//! `ChannelData.Data` is *"The data blob in JSON form"* (WITSML TRG v2.1 §7.2
//! p. 146) and typed a bare string. The one document that states its grammar
//! is named by the Energistics JSON Style Guide (`energistics_json_guide.pdf`
//! p. 8): *"the JSON on-disk representation for channel sets (a format that is
//! specified in Section 3 of the WITSML v2.1 Release Notes), the arrays may
//! have NULLs"* - and those release notes are **not** among this repository's reference documents. So the
//! layout read here is the one the object model leaves no other way to read,
//! and the one `energyml-build`'s `witsml-logs` writes
//! (`crates/energyml-build/src/ops/witsml_logs.rs`, *The row layout*): this
//! reader is that contract **read backwards**, and it is an inference.
//!
//! - `Data` is a JSON array of rows;
//! - a row is **the value of each index the set declares, in order, then one
//!   value per channel, in the order the channels are listed**.
//!
//! Anything else is refused by name rather than read some other way.
//!
//! # What is cited
//!
//! - **Several indexes, one family.** `ChannelSet.Index` is 0..n, *"The first
//!   index is the primary index. Any additional indexes are secondary
//!   indexes"* (§7.5 p. 150), and a set is *"a grouping of channels with a
//!   compatible index […] either in time or in depth"* (§7.7 p. 151). A
//!   secondary of the primary's family is read; time with depth is
//!   [`Refused::IndexFamilyMix`] (Lutetia's *one index, never two* is a writing
//!   choice, not a reading rule).
//! - **Direction.** *"The direction of the index, either increasing or
//!   decreasing"* (§7.5 p. 149): the primary must run that way, strictly -
//!   [`Refused::NotMonotone`]; `unordered` is [`Refused::UnorderedIndex`].
//! - **`FileUri`.** *"If this field is non-null, then the data field is
//!   ignored"* (§7.2 p. 146) - this build does not read the bulk file the
//!   standard means (its format is the release notes'): [`Refused::FileUri`],
//!   never the inline data read in its place. `FileUri` **and** `Data`
//!   together are this library's extension, below.
//! - **Each channel has its own index** (§7.7 p. 151, *"Each channel has its
//!   own index"*): a channel whose primary index differs from the set's in
//!   kind, unit, direction or datum is [`Refused::ChannelIndexContradicts`] -
//!   its values would be read against an index they are not on. The mnemonic
//!   is not compared: two names for one index change no number.
//! - **Nulls.** The style guide's *"the arrays may have NULLs"* - a `null` is
//!   [`Quality::Absent`]; `"NaN"`, `"+Infinity"`, `"-Infinity"` are the
//!   guide's spellings of the non-finite numbers (same page) and read as
//!   such.
//!
//! # Rows in an HDF5 dataset - an extension beyond the standard
//!
//! The model does not provide for this, but the "Data" of a "ChannelData" may
//! carry the path of a dataset in the h5 file. The decision and its reasons are
//! ADR-0013; what this reader holds to:
//!
//! - **the form is told by syntax, never guessed from content.** `FileUri`
//!   names the file - the standard's own field for *"a reference to an
//!   external file"* (the class's note, §7.2 p. 146) - and `Data`, which the
//!   standard ignores once `FileUri` is set, holds the **absolute** path of a
//!   dataset in it: it begins with `/`, which no JSON text can
//!   (`energyml_storage::array::is_channel_data_in_file`, the one definition,
//!   shared with the locator). `FileUri` alone, or beside an inline blob,
//!   stays [`Refused::FileUri`]; `Data` alone stays the inline JSON;
//! - **the dataset is the inline rows, row for row**: two dimensions,
//!   `[rows, indexes + channels]`, each row the index value(s) then one value
//!   per channel in the order they are listed. `NaN` is what inline writes
//!   `null`; an infinity reads as its inline spelling would. So the cells
//!   become the JSON the inline form parses to, and **one loop** reads both -
//!   inline and referenced cannot drift into two readings of a row;
//! - **read through [`ArraySource`], never a store**: the location is the
//!   source's (`energyml_storage::array` locates the form, so a cut, an import
//!   or a prefetch carries the dataset too), and a build without the HDF5
//!   backend is [`Refused::DatasetNeedsFeature`] - a missing capability,
//!   never a series of nothing;
//! - **what a numeric dataset cannot hold is refused before anything is
//!   read**: a `date time` index, a string or a boolean channel -
//!   [`Refused::DatasetColumnKind`]. Encoding a date as a number of seconds
//!   would be a second invention on top of the first.
//!
//! # Data kinds
//!
//! `ChannelDataKind` (§7.3 p. 147): `double`, `float`, `measured depth`,
//! `true vertical depth`, `elapsed time` are numbers; `int`, `long`, `bytes`
//! integers; `boolean`; `string`; `date time` an ISO text, read as Unix
//! seconds. `pass indexed depth` - pass, direction and depth in one value -
//! has no JSON form any document here states, and is refused by name, like
//! any kind outside this list.

use serde_json::Value;

use energyml_core::value::Reflect;

use super::{
    Channel, Direction, Index, IndexKind, Quality, Refused, Series, identity_of, instant, monotone, objects,
    reference, text,
};
use crate::property::PropertyKindRef;
use crate::source::{ArraySource, NoObjects};
use crate::values::Values;

/// Whether this reads the object - a channel set of any flavour
/// (`ChannelSet`, `PPFGChannelSet`), by the fields a set carries.
pub fn reads(object: &dyn Reflect) -> bool {
    object.type_info().type_key.ends_with("ChannelSet")
        && object.type_info().field_ignore_case("Channel").is_some()
        && object.type_info().field_ignore_case("Data").is_some()
}

/// Whether this reads the object as a log - a container of channel sets.
pub fn reads_log(object: &dyn Reflect) -> bool {
    object.type_info().type_key.ends_with("Log")
        && object.type_info().field_ignore_case("ChannelSet").is_some()
}

/// Every channel set of a log, each read - the first refusal names its set.
///
/// With no array source: a set whose rows are in a dataset is refused by name
/// ([`Refused::DatasetNotLocated`]); [`read_log_with`] reads it.
pub fn read_log(log: &dyn Reflect) -> Result<Vec<Series>, Refused> {
    read_log_with(log, &NoObjects)
}

/// Every channel set of a log, each read, the datasets through `arrays`.
pub fn read_log_with(log: &dyn Reflect, arrays: &dyn ArraySource) -> Result<Vec<Series>, Refused> {
    if !reads_log(log) {
        return Err(Refused::NotThisReader {
            expected: "a WITSML Log",
            found: log.type_info().type_key.to_string(),
        });
    }
    objects(log, "ChannelSet")
        .into_iter()
        .map(|set| {
            read_with(set, arrays).map_err(|why| Refused::InSet {
                set: identity_of(set).0,
                why: Box::new(why),
            })
        })
        .collect()
}

/// How a column's values are read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Decode {
    Number,
    Integer,
    Boolean,
    Text,
    Instant,
}

impl Decode {
    fn of(kind: &str) -> Option<Decode> {
        match kind {
            "double" | "float" | "measured depth" | "true vertical depth" | "elapsed time" => {
                Some(Decode::Number)
            }
            "int" | "long" | "bytes" => Some(Decode::Integer),
            "boolean" => Some(Decode::Boolean),
            "string" => Some(Decode::Text),
            "date time" => Some(Decode::Instant),
            _ => None,
        }
    }

    fn expects(self) -> &'static str {
        match self {
            Decode::Number => "a number",
            Decode::Integer => "an integer",
            Decode::Boolean => "a boolean",
            Decode::Text => "a string",
            Decode::Instant => "a date-time string",
        }
    }
}

/// An index as the set declares it, before its values are read.
struct Declared {
    kind: IndexKind,
    mnemonic: String,
    uom: Option<String>,
    direction: Direction,
    datum: Option<String>,
}

/// One `ChannelIndex`, read - or refused.
fn declared(index: &dyn Reflect, rank: usize) -> Result<Declared, Refused> {
    let mnemonic = text(index, "Mnemonic").unwrap_or_else(|| format!("index {}", rank + 1));
    let written = text(index, "IndexKind").ok_or_else(|| Refused::Missing {
        what: format!("the kind of the index `{mnemonic}`"),
    })?;
    let kind = match IndexKind::from_wire(&written) {
        Some(kind) => kind,
        None if written == "pass indexed depth" => return Err(Refused::PassIndexedDepth { what: mnemonic }),
        None => {
            return Err(Refused::IndexNotTimeOrDepth {
                index: mnemonic,
                kind: written,
            });
        }
    };
    // Absent, the schema's own default reading: the element is 1..1, and a
    // file that omits it is `Validate`'s business, not a reason to refuse rows.
    let direction = match text(index, "Direction").as_deref() {
        Some("increasing") | None => Direction::Increasing,
        Some("decreasing") => Direction::Decreasing,
        _ => Direction::Unordered,
    };
    let uom = if kind == IndexKind::DateTime {
        Some("s".to_string())
    } else {
        text(index, "Uom")
    };
    Ok(Declared {
        kind,
        mnemonic,
        uom,
        direction,
        datum: reference(index, "Datum").map(|(uuid, _)| uuid),
    })
}

/// A channel as listed, before its values are read.
struct Listed {
    uuid: Option<String>,
    mnemonic: String,
    uom: Option<String>,
    data_kind: String,
    decode: Decode,
    property_kind: Option<PropertyKindRef>,
}

/// Where a set's rows are.
enum Rows<'a> {
    /// `Data` alone: the JSON rows.
    Inline(String),
    /// `FileUri` and `Data`: a dataset of an HDF5 file - the extension.
    Dataset {
        /// The `ChannelData`, which the array source locates.
        data: &'a dyn Reflect,
        /// What `FileUri` names.
        file: String,
        /// What `Data` names.
        path: String,
    },
}

/// Read a channel set with no array source: rows kept in a dataset are
/// refused by name ([`Refused::DatasetNotLocated`]) - [`read_with`] reads
/// them.
pub fn read(set: &dyn Reflect) -> Result<Series, Refused> {
    read_with(set, &NoObjects)
}

/// Read a channel set, its rows inline or in a dataset `arrays` reads - the
/// one reading of a set, both forms through the same row loop.
pub fn read_with(set: &dyn Reflect, arrays: &dyn ArraySource) -> Result<Series, Refused> {
    if !reads(set) {
        return Err(Refused::NotThisReader {
            expected: "a WITSML ChannelSet",
            found: set.type_info().type_key.to_string(),
        });
    }
    let (uuid, title) = identity_of(set);

    // -- the data: inline, in a dataset, or refused ------------------------------
    // By syntax, never guessed from content (module doc).
    let data = objects(set, "Data").into_iter().next().ok_or(Refused::NoData)?;
    let rows_at = match (text(data, "FileUri"), text(data, "Data")) {
        (Some(file), Some(path)) if energyml_storage::array::is_channel_data_in_file(&file, &path) => {
            Rows::Dataset { data, file, path }
        }
        // The standard's own form, with or without a blob beside it (which
        // the standard then ignores).
        (Some(uri), _) => return Err(Refused::FileUri { uri }),
        (None, Some(blob)) => Rows::Inline(blob),
        (None, None) => return Err(Refused::NoData),
    };

    // -- the indexes ---------------------------------------------------------------
    let indexes: Vec<Declared> = objects(set, "Index")
        .into_iter()
        .enumerate()
        .map(|(rank, index)| declared(index, rank))
        .collect::<Result<_, _>>()?;
    let Some(primary) = indexes.first() else {
        return Err(Refused::NoIndex);
    };
    if primary.direction == Direction::Unordered {
        return Err(Refused::UnorderedIndex {
            index: primary.mnemonic.clone(),
        });
    }
    if let Some(other) = indexes
        .iter()
        .find(|one| one.kind.family() != primary.kind.family())
    {
        return Err(Refused::IndexFamilyMix {
            primary: primary.mnemonic.clone(),
            primary_family: primary.kind.family().word(),
            other: other.mnemonic.clone(),
            other_family: other.kind.family().word(),
        });
    }

    // -- the channels, and their own index ----------------------------------------
    let mut listed: Vec<Listed> = Vec::new();
    for (rank, channel) in objects(set, "Channel").into_iter().enumerate() {
        let mnemonic = text(channel, "Mnemonic").unwrap_or_else(|| format!("channel {}", rank + 1));
        let data_kind = text(channel, "DataKind").ok_or_else(|| Refused::Missing {
            what: format!("the data kind of `{mnemonic}`"),
        })?;
        let decode = match Decode::of(&data_kind) {
            Some(decode) => decode,
            None if data_kind == "pass indexed depth" => {
                return Err(Refused::PassIndexedDepth { what: mnemonic });
            }
            None => {
                return Err(Refused::UnknownDataKind {
                    channel: mnemonic,
                    kind: data_kind,
                });
            }
        };
        if let Some(own) = objects(channel, "Index").into_iter().next() {
            let own = declared(own, 0)?;
            let contradiction =
                |what: &'static str, expected: String, found: String| Refused::ChannelIndexContradicts {
                    channel: mnemonic.clone(),
                    what,
                    expected,
                    found,
                };
            if own.kind != primary.kind {
                return Err(contradiction(
                    "the kind",
                    primary.kind.wire().into(),
                    own.kind.wire().into(),
                ));
            }
            if own.uom != primary.uom {
                return Err(contradiction(
                    "the unit",
                    primary.uom.clone().unwrap_or_default(),
                    own.uom.unwrap_or_default(),
                ));
            }
            if own.direction != primary.direction {
                return Err(contradiction(
                    "the direction",
                    primary.direction.wire().into(),
                    own.direction.wire().into(),
                ));
            }
            if let (Some(set_datum), Some(own_datum)) = (&primary.datum, &own.datum)
                && !set_datum.eq_ignore_ascii_case(own_datum)
            {
                return Err(contradiction("the datum", set_datum.clone(), own_datum.clone()));
            }
        }
        listed.push(Listed {
            uuid: energyml_core::identity::uuid_of_reflect(channel)
                .map(str::to_string)
                .filter(|one| !one.is_empty()),
            mnemonic,
            uom: text(channel, "Uom"),
            data_kind,
            decode,
            property_kind: reference(channel, "ChannelPropertyKind").map(|(uuid, title)| {
                PropertyKindRef::Object {
                    uuid,
                    title: Some(title).filter(|one| !one.is_empty()),
                }
            }),
        });
    }

    // -- the rows --------------------------------------------------------------------
    let rows: Vec<Value> = match rows_at {
        Rows::Inline(blob) => {
            let parsed: Value = serde_json::from_str(&blob).map_err(|why| Refused::NotRows {
                why: format!("not JSON: {why}"),
            })?;
            let Value::Array(rows) = parsed else {
                return Err(Refused::NotRows {
                    why: "the JSON is not an array".into(),
                });
            };
            rows
        }
        Rows::Dataset { data, file, path } => dataset_rows(data, file, path, arrays, &indexes, &listed)?,
    };
    let width = indexes.len() + listed.len();
    let mut index_values: Vec<Vec<f64>> = vec![Vec::with_capacity(rows.len()); indexes.len()];
    let mut columns: Vec<Column> = listed
        .iter()
        .map(|one| Column::new(one.decode, rows.len()))
        .collect();

    for (at, row) in rows.iter().enumerate() {
        let number = at + 1;
        let Value::Array(cells) = row else {
            return Err(Refused::NotRows {
                why: format!("row {number} is not an array"),
            });
        };
        if cells.len() != width {
            return Err(Refused::RowLength {
                row: number,
                found: cells.len(),
                expected: width,
                indexes: indexes.len(),
                channels: listed.len(),
            });
        }
        for (rank, index) in indexes.iter().enumerate() {
            let cell = &cells[rank];
            let decode = if index.kind == IndexKind::DateTime {
                Decode::Instant
            } else {
                Decode::Number
            };
            let value = match read_number(cell, decode, number, &index.mnemonic)? {
                Some(value) => value,
                None if rank == 0 => {
                    return Err(Refused::NullIndex {
                        row: number,
                        index: index.mnemonic.clone(),
                    });
                }
                None => f64::NAN,
            };
            if rank == 0 && !value.is_finite() {
                return Err(Refused::NullIndex {
                    row: number,
                    index: index.mnemonic.clone(),
                });
            }
            index_values[rank].push(value);
        }
        for (rank, column) in columns.iter_mut().enumerate() {
            column.push(&cells[indexes.len() + rank], number, &listed[rank].mnemonic)?;
        }
    }

    let mut built: Vec<Index> = indexes
        .into_iter()
        .zip(index_values)
        .map(|(declared, values)| Index {
            kind: declared.kind,
            mnemonic: declared.mnemonic,
            uom: declared.uom,
            direction: declared.direction,
            datum: declared.datum,
            values,
        })
        .collect();
    let index = built.remove(0);
    monotone(&index)?;

    let channels = listed
        .into_iter()
        .zip(columns)
        .map(|(one, column)| Channel {
            uuid: one.uuid,
            mnemonic: one.mnemonic,
            uom: if one.decode == Decode::Instant {
                Some("s".into())
            } else {
                one.uom
            },
            data_kind: Some(one.data_kind),
            property_kind: one.property_kind,
            measure_class: None,
            values: column.values,
            quality: column.quality,
        })
        .collect();
    Ok(Series {
        uuid,
        title,
        index,
        secondary: built,
        channels,
    })
}

/// The rows of a dataset, as the cells the inline JSON would parse to - so
/// the row loop of [`read_with`] reads them as it reads inline rows.
fn dataset_rows(
    data: &dyn Reflect,
    file: String,
    path: String,
    arrays: &dyn ArraySource,
    indexes: &[Declared],
    listed: &[Listed],
) -> Result<Vec<Value>, Refused> {
    // Kinds first: nothing is read for a column no number can hold.
    if let Some(index) = indexes.iter().find(|index| index.kind == IndexKind::DateTime) {
        return Err(Refused::DatasetColumnKind {
            column: index.mnemonic.clone(),
            kind: IndexKind::DateTime.wire().to_string(),
        });
    }
    if let Some(channel) = listed
        .iter()
        .find(|one| !matches!(one.decode, Decode::Number | Decode::Integer))
    {
        return Err(Refused::DatasetColumnKind {
            column: channel.mnemonic.clone(),
            kind: channel.data_kind.clone(),
        });
    }

    let location = arrays
        .locate(data)
        .into_iter()
        .find(|one| one.path_in_file == path)
        .ok_or_else(|| Refused::DatasetNotLocated {
            file: file.clone(),
            path: path.clone(),
        })?;
    let read = arrays.read(&location).map_err(|why| match &why {
        crate::Error::Storage(energyml_storage::Error::ExternalData {
            reason: energyml_storage::ExternalDataReason::FeatureNotEnabled { feature, .. },
            ..
        }) => Refused::DatasetNeedsFeature {
            file: file.clone(),
            path: path.clone(),
            feature: feature.to_string(),
        },
        _ => Refused::DatasetNotRead {
            file: file.clone(),
            path: path.clone(),
            why: why.to_string(),
        },
    })?;

    let width = indexes.len() + listed.len();
    let dimensions = read.metadata.dimensions.clone();
    let cells = cells_of(&read.values);
    let fits = dimensions.len() == 2
        && dimensions[1] == width as i64
        && dimensions[0] >= 0
        && cells.len() as i64 == dimensions[0] * dimensions[1];
    if !fits {
        return Err(Refused::DatasetShape {
            path,
            dimensions,
            expected: width,
            indexes: indexes.len(),
            channels: listed.len(),
        });
    }
    Ok(cells
        .chunks(width)
        .map(|row| Value::Array(row.to_vec()))
        .collect())
}

/// A dataset's values as the JSON cells of an inline row: `NaN` is `null`,
/// an infinity its style-guide spelling, an integer stays exact.
fn cells_of(values: &energyml_storage::ArrayValues) -> Vec<Value> {
    use energyml_storage::ArrayValues;
    let real = |value: f64| {
        if value.is_nan() {
            Value::Null
        } else if value == f64::INFINITY {
            Value::String("+Infinity".into())
        } else if value == f64::NEG_INFINITY {
            Value::String("-Infinity".into())
        } else {
            serde_json::json!(value)
        }
    };
    match values {
        ArrayValues::F64(all) => all.iter().map(|value| real(*value)).collect(),
        ArrayValues::F32(all) => all.iter().map(|value| real(f64::from(*value))).collect(),
        ArrayValues::I64(all) => all.iter().map(|value| Value::from(*value)).collect(),
        ArrayValues::I32(all) => all.iter().map(|value| Value::from(*value)).collect(),
        ArrayValues::I16(all) => all.iter().map(|value| Value::from(*value)).collect(),
        ArrayValues::I8(all) => all.iter().map(|value| Value::from(*value)).collect(),
        ArrayValues::U64(all) => all.iter().map(|value| Value::from(*value)).collect(),
        ArrayValues::U32(all) => all.iter().map(|value| Value::from(*value)).collect(),
        ArrayValues::U16(all) => all.iter().map(|value| Value::from(*value)).collect(),
        ArrayValues::U8(all) => all.iter().map(|value| Value::from(*value)).collect(),
        other => other.to_f64().into_iter().map(real).collect(),
    }
}

/// A number, an instant or nothing - or a refusal naming the cell.
fn read_number(cell: &Value, decode: Decode, row: usize, column: &str) -> Result<Option<f64>, Refused> {
    let wrong = |found: String| Refused::ValueKind {
        row,
        column: column.to_string(),
        expected: decode.expects(),
        found,
    };
    match (cell, decode) {
        (Value::Null, _) => Ok(None),
        (Value::Number(number), Decode::Number) => Ok(number.as_f64()),
        (Value::String(spelled), Decode::Number) => match spelled.as_str() {
            "NaN" => Ok(None),
            "+Infinity" => Ok(Some(f64::INFINITY)),
            "-Infinity" => Ok(Some(f64::NEG_INFINITY)),
            _ => Err(wrong(format!("the string `{spelled}`"))),
        },
        (Value::String(written), Decode::Instant) => {
            instant::unix_seconds(written)
                .map(Some)
                .map_err(|why| Refused::Instant {
                    at: format!("row {row}, `{column}`"),
                    text: written.clone(),
                    why,
                })
        }
        (other, _) => Err(wrong(kind_of(other))),
    }
}

fn kind_of(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(value) => format!("the boolean {value}"),
        Value::Number(value) => format!("the number {value}"),
        Value::String(value) => format!("the string `{value}`"),
        Value::Array(_) => "an array".into(),
        Value::Object(_) => "an object".into(),
    }
}

/// One channel's values as they are read.
struct Column {
    decode: Decode,
    values: Values,
    quality: Vec<Quality>,
}

impl Column {
    fn new(decode: Decode, rows: usize) -> Column {
        let values = match decode {
            Decode::Number | Decode::Instant => Values::Floating(Vec::with_capacity(rows)),
            Decode::Integer => Values::Integer(Vec::with_capacity(rows)),
            Decode::Boolean => Values::Boolean(Vec::with_capacity(rows)),
            Decode::Text => Values::Text(Vec::with_capacity(rows)),
        };
        Column {
            decode,
            values,
            quality: Vec::with_capacity(rows),
        }
    }

    fn push(&mut self, cell: &Value, row: usize, column: &str) -> Result<(), Refused> {
        let decode = self.decode;
        let wrong = |found: String| Refused::ValueKind {
            row,
            column: column.to_string(),
            expected: decode.expects(),
            found,
        };
        match (&mut self.values, cell) {
            (Values::Floating(values), _) => {
                let value = read_number(cell, decode, row, column)?;
                let (value, quality) = match value {
                    None => (f64::NAN, Quality::Absent),
                    Some(value) if value.is_infinite() => (
                        value,
                        Quality::Flagged(if value > 0.0 {
                            "+Infinity".into()
                        } else {
                            "-Infinity".into()
                        }),
                    ),
                    Some(value) => (value, Quality::Good),
                };
                values.push(value);
                self.quality.push(quality);
            }
            (Values::Integer(values), Value::Null) => {
                values.push(0);
                self.quality.push(Quality::Absent);
            }
            (Values::Integer(values), Value::Number(number)) => {
                let whole = number.as_i64().or_else(|| {
                    number
                        .as_f64()
                        .filter(|value| value.fract() == 0.0 && value.abs() < 9.0e15)
                        .map(|value| value as i64)
                });
                let Some(whole) = whole else {
                    return Err(wrong(format!("the number {number}")));
                };
                values.push(whole);
                self.quality.push(Quality::Good);
            }
            (Values::Boolean(values), Value::Null) => {
                values.push(false);
                self.quality.push(Quality::Absent);
            }
            (Values::Boolean(values), Value::Bool(value)) => {
                values.push(*value);
                self.quality.push(Quality::Good);
            }
            (Values::Text(values), Value::Null) => {
                values.push(String::new());
                self.quality.push(Quality::Absent);
            }
            (Values::Text(values), Value::String(value)) => {
                values.push(value.clone());
                self.quality.push(Quality::Good);
            }
            (_, other) => return Err(wrong(kind_of(other))),
        }
        Ok(())
    }
}
