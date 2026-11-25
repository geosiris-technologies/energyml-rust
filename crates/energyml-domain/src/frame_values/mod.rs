//! The values of a property along a wellbore frame, read **once**.
//!
//! The question *what is this property worth along this well?* was answered
//! three times, each its own way: [`crate::wellbore_table`] (a table, one row per
//! node or interval), [`crate::well_strata`] (the units a category property
//! names) and the sheet readers. They did not index alike (one compared the
//! element with `Intervals` only and lost `intervals from datum`, issue #34), did
//! not read a category table alike (the last column that is not a key, against
//! every column joined), and did not refuse alike. This module is the one door;
//! the others ask it. ADR-0049.
//!
//! # What [`of`] gives
//!
//! | | |
//! |---|---|
//! | the **support** | [`Support::Nodes`] (one value per node) or [`Support::Intervals`] (one per span from a node to the next) - by [`IndexableElement::is_frame_interval`], so `intervals`, `interval` and `intervals from datum` are one thing |
//! | the **values** | `Vec<Option<Value>>`: a number, or a **code** (an integer of a discrete or categorical property), or a truth value or a text; `None` is the file's absence (`NullValue`, `NaN`), never a zero |
//! | the **unit**, the **kind**, the **range** | the property's own, and the minimum and maximum of the values that are not absent |
//! | the **table**, for a property that names a `CategoryLookup` | [`Categories`]: the raw codes stay codes, and a code gives its **row** of the table with each column's title, kind, unit and value ([`FrameValues::row_at`]); a label is *a column among others* ([`Categories::label_column`]) |
//!
//! # What is refused, by name
//!
//! A number of values that is not the number of nodes or of intervals (reading it
//! would shift every value by a rank); several patches; a property that holds
//! several values per element (a vector: which component?) or declares
//! `RealizationIndices` (which realization?); a property that does not hang on
//! this frame, or on neither its nodes nor its intervals; a table that is not
//! there or cannot be read. See [`Refusal`]. **Time is not refused**: a
//! property with a time index is one instant of a series, and [`FrameValues::time`]
//! says which; a property that wrote every instant in one array is caught by the
//! count.
//!
//! # At an MD, and across representations
//!
//! [`FrameValues::at_md`] gives the value at any measured depth, through
//! [`crate::along_md`]: nodes of a continuous property are interpolated and a
//! discrete one is held at its node and refused between two; **never across a
//! hole** (an absent neighbour, a gap wider than [`crate::along_md::GapRule`]
//! allows); on intervals, the interval that holds the MD, and at a node the side
//! asked. [`carry()`] puts the values of the frames of a wellbore on **another
//! representation of the same wellbore** by its MDs, after
//! [`crate::strata_by_md::comparable`] has said the MDs count the same way.
//!
//! # A category as a number
//!
//! [`FrameValues::derive`] turns a numeric column of the category table into a
//! continuous property of the same support (a table that gives each facies its
//! porosity): nothing is written, and the refusals are named (a column that is
//! not numeric, several values per row, a code with no row, a table that repeats
//! a key, a table that is a function and not a list of categories).
//!
//! # What is not here
//!
//! The colour of a category (that is [`crate::color`] and
//! [`crate::well_strata`]), the limits between two runs of values, and the
//! interpretation of a code as a stratigraphic unit.

mod at_md;
mod carry;
mod derived;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

use energyml_core::identity;
use energyml_core::text::plain_number;
use energyml_core::value::{FieldValue, Reflect};

use crate::error::Error;
use crate::property::{self, IndexableElement, PropertyKindRef, TimeRef};
use crate::source::ArraySource;
use crate::table::{CODE, Cell, ColumnMeta, KeyIndex, Lookup, RowMatch, row_named_by};
use crate::values::{self, Values};

pub use at_md::{AtMdRefusal, Placement, Sample, Side};
pub use carry::{Carried, Carry, Stated, carry};
pub use derived::{Derivation, DeriveRefusal};

/// The measured depth of each node of a frame, in the order the file gives them.
const NODE_MD: &str = "NodeMd";

// ---------------------------------------------------------------------------
// What comes back
// ---------------------------------------------------------------------------

/// One value of a property.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A number: a continuous property, or a discrete one the file stored as
    /// floating-point.
    Number(f64),
    /// A code: an integer of a discrete, categorical or comment property. It
    /// names a row of the property's table when there is one; alone it means
    /// nothing, and `3` is not three.
    Code(i64),
    /// A truth value.
    Boolean(bool),
    /// A text.
    Text(String),
}

impl Value {
    /// The value as a number: a number, or a code. `None` for a truth value and
    /// a text.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(number) => Some(*number),
            Value::Code(code) => Some(*code as f64),
            Value::Boolean(_) | Value::Text(_) => None,
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Number(number) => f.write_str(&plain_number(*number)),
            Value::Code(code) => write!(f, "{code}"),
            Value::Boolean(flag) => write!(f, "{flag}"),
            Value::Text(text) => f.write_str(text),
        }
    }
}

/// What the values of a property are placed on.
#[derive(Clone, Debug, PartialEq)]
pub enum Support {
    /// One value per node of the frame, at its measured depth.
    Nodes {
        /// The measured depth of each node.
        md: Vec<f64>,
    },
    /// One value per interval, from a node to the next: interval `i` runs from
    /// `top[i]` to `base[i]`, and `top[i + 1] == base[i]`.
    Intervals {
        /// The measured depth at the top of each interval.
        top: Vec<f64>,
        /// The measured depth at the base of each interval.
        base: Vec<f64>,
    },
}

impl Support {
    /// How many values the support holds: nodes, or intervals.
    pub fn len(&self) -> usize {
        match self {
            Support::Nodes { md } => md.len(),
            Support::Intervals { top, .. } => top.len(),
        }
    }

    /// Whether the support holds none.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether the values hang on the intervals.
    pub fn is_intervals(&self) -> bool {
        matches!(self, Support::Intervals { .. })
    }

    /// The measured depth of each node of the frame the support is cut from:
    /// its own nodes, or the tops of the intervals and the base of the last.
    pub fn node_md(&self) -> Vec<f64> {
        match self {
            Support::Nodes { md } => md.clone(),
            Support::Intervals { top, base } => {
                let mut axis = top.clone();
                if let Some(last) = base.last() {
                    axis.push(*last);
                }
                axis
            }
        }
    }
}

/// The smallest and the largest of the values that are not absent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    /// The smallest.
    pub min: f64,
    /// The largest.
    pub max: f64,
}

/// A cell of the row of a category table, with what its column says of itself.
#[derive(Clone, Debug, PartialEq)]
pub struct RowCell {
    /// The column's position in the table, keys first.
    pub column: usize,
    /// The column's title.
    pub title: Option<String>,
    /// What the column measures (`code`, `name`, a depth...): a `ColumnBasedTable`
    /// states it, a 2.0.1 lookup does not.
    pub kind: Option<PropertyKindRef>,
    /// The column's own unit.
    pub uom: Option<String>,
    /// Whether the column is a key column.
    pub key: bool,
    /// The value. `None` is an absence (`NullValue`, `NaN`) and the cell of a
    /// column that holds several values per row or that could not be read.
    pub value: Option<Cell>,
}

/// The table a categorical property names, read, and the rule that says which
/// row a code names.
pub struct Categories {
    /// The uuid of the table object.
    pub table: String,
    /// The uri the property names the table by, version included - what a colour
    /// on the table is looked up by. `None` when the property names no uri.
    pub uri: Option<String>,
    /// The type of the table object, without the `obj_` of release 2.0.1:
    /// `ColumnBasedTable`, `StringTableLookup`, `DoubleTableLookup`.
    pub table_type: String,
    /// The table, read.
    pub lookup: Lookup,
    /// What each column says of itself, in the order of [`Lookup::titles`].
    pub columns: Vec<ColumnMeta>,
    index: KeyIndex,
    /// A key column of text that reads as numbers: the first row of each number.
    text_numbers: HashMap<u64, usize>,
}

impl Categories {
    pub(crate) fn new(
        table: String,
        table_type: String,
        lookup: Lookup,
        columns: Vec<ColumnMeta>,
    ) -> Categories {
        let index = lookup.key_index();
        let mut text_numbers = HashMap::new();
        if let [only] = lookup.key_columns.as_slice()
            && matches!(lookup.rows.columns.get(*only), Some(Some(Values::Text(_))))
        {
            for row in 0..lookup.rows.rows {
                if let Some(Cell::Text(text)) = lookup.rows.cell(row, *only)
                    && let Ok(number) = text.trim().parse::<f64>()
                    && !number.is_nan()
                {
                    text_numbers.entry(number_bits(number)).or_insert(row);
                }
            }
        }
        Categories {
            table,
            uri: None,
            table_type,
            lookup,
            columns,
            index,
            text_numbers,
        }
    }

    /// The row a value names: **by the key**, or by its rank from 0 when the table
    /// has no key ([`row_named_by`], the one rule); the first row when a key is
    /// repeated. A key column of text whose cells read as numbers is read the way
    /// the front ends read it (`3` names the row keyed `" 3"`), after the exact
    /// match. `None` for a code no row holds - and for a table keyed on several
    /// columns, which a single value cannot name.
    pub fn row_of(&self, value: &Value) -> Option<usize> {
        let cell = match value {
            Value::Number(number) => Cell::Number(*number),
            Value::Code(code) => Cell::Number(*code as f64),
            Value::Boolean(flag) => Cell::Boolean(*flag),
            Value::Text(text) => Cell::Text(text.clone()),
        };
        match row_named_by(&self.index, &cell) {
            Ok(RowMatch::One(row)) => Some(row),
            Ok(RowMatch::Several(rows)) => rows.first().copied(),
            Ok(RowMatch::None) => match cell {
                Cell::Number(number) => self.text_numbers.get(&number_bits(number)).copied(),
                _ => None,
            },
            Err(_) => None,
        }
    }

    /// The whole of a row, one [`RowCell`] per column, keys first.
    pub fn row(&self, row: usize) -> Vec<RowCell> {
        self.columns
            .iter()
            .enumerate()
            .map(|(column, meta)| RowCell {
                column,
                title: meta.title.clone(),
                kind: meta.kind.clone(),
                uom: meta.uom.clone(),
                key: meta.key,
                value: self.lookup.rows.cell(row, column),
            })
            .collect()
    }

    /// The columns that are not keys, in the table's order.
    pub fn value_columns(&self) -> Vec<usize> {
        (0..self.columns.len())
            .filter(|column| !self.lookup.key_columns.contains(column))
            .collect()
    }

    /// The column a person reads as the **label** of a category - one column
    /// among others: the value column whose kind is `code`
    /// ([`crate::table::CODE`]), else the only value column that holds text.
    /// `None` when neither is single: a table with two text columns and no kind
    /// does not say which is the name, and none is guessed.
    pub fn label_column(&self) -> Option<usize> {
        let value_columns = self.value_columns();
        let coded: Vec<usize> = value_columns
            .iter()
            .copied()
            .filter(|column| {
                self.columns.get(*column).and_then(|meta| meta.kind.as_ref()).is_some_and(|kind| {
                    matches!(kind, PropertyKindRef::Object { uuid, .. } if uuid.eq_ignore_ascii_case(CODE))
                })
            })
            .collect();
        match coded.as_slice() {
            [only] => return Some(*only),
            [] => {}
            _ => return None,
        }
        let texts: Vec<usize> = value_columns
            .iter()
            .copied()
            .filter(|column| matches!(self.lookup.rows.columns.get(*column), Some(Some(Values::Text(_)))))
            .collect();
        match texts.as_slice() {
            [only] => Some(*only),
            _ => None,
        }
    }

    /// The label of a row ([`Categories::label_column`]) as text.
    pub fn label(&self, row: usize) -> Option<String> {
        let cell = self.lookup.rows.cell(row, self.label_column()?)?;
        Some(match cell {
            Cell::Text(text) => text,
            Cell::Number(number) => plain_number(number),
            Cell::Boolean(flag) => flag.to_string(),
        })
    }

    /// The key columns, in the table's order.
    pub fn key_columns(&self) -> &[usize] {
        &self.lookup.key_columns
    }

    /// Every key that more than one row holds, with its rows: the first is the one
    /// a code names ([`Categories::row_of`]).
    pub fn duplicate_keys(&self) -> Vec<(Vec<Cell>, Vec<usize>)> {
        self.index.duplicates()
    }

    /// The index of the key columns, for a caller that asks something this does
    /// not.
    pub fn key_index(&self) -> &KeyIndex {
        &self.index
    }
}

/// A number as a hash key: its bits, `-0.0` being `0.0`.
fn number_bits(number: f64) -> u64 {
    if number == 0.0 {
        0.0f64.to_bits()
    } else {
        number.to_bits()
    }
}

/// What a property says along a frame.
pub struct FrameValues {
    /// The frame's uuid, lowercased.
    pub frame: String,
    /// The property's uuid.
    pub property: String,
    /// The property's title, or its uuid when it has none.
    pub title: String,
    /// What the property hangs on, as the file says it.
    pub element: IndexableElement,
    /// What the values are placed on.
    pub support: Support,
    /// One per node or interval. `None` is the file's absence - the integer equal
    /// to the array's `NullValue`, a floating value that is not a finite number -,
    /// never a zero.
    pub values: Vec<Option<Value>>,
    /// Whether the property is a `ContinuousProperty`: the only kind of property
    /// whose value between two nodes is not a guess.
    pub continuous: bool,
    /// The unit, for a continuous property.
    pub uom: Option<String>,
    /// What kind of quantity it is, unresolved.
    pub kind: Option<PropertyKindRef>,
    /// The uri of the property kind, when the property references one.
    pub kind_uri: Option<String>,
    /// The smallest and the largest of the values that are not absent, codes and
    /// numbers alike. `None` when none is a number, or none is present.
    pub range: Option<Range>,
    /// The table of a property that names one, read.
    pub categories: Option<Categories>,
    /// When it applies, as the file says it - not a refusal: one property is one
    /// instant of a series.
    pub time: Option<TimeRef>,
    /// The 2.0.1 `RealizationIndex`: which realization this property is.
    pub realization: Option<u64>,
    /// Set when this is not stored: the property and the column it was derived from
    /// ([`FrameValues::derive`]).
    pub derivation: Option<Derivation>,
    /// What was met on the way and is worth saying.
    pub notes: Vec<String>,
}

impl FrameValues {
    /// The whole row of the table that the value at `at` names, one cell per
    /// column. `None` when the property names no table, when there is no value at
    /// `at`, or when its code names no row.
    pub fn row_at(&self, at: usize) -> Option<Vec<RowCell>> {
        let categories = self.categories.as_ref()?;
        let value = self.values.get(at)?.as_ref()?;
        Some(categories.row(categories.row_of(value)?))
    }

    /// The row each value names, `None` for an absent value and for a code no row
    /// holds. `None` altogether for a property that names no table.
    pub fn rows(&self) -> Option<Vec<Option<usize>>> {
        let categories = self.categories.as_ref()?;
        Some(
            self.values
                .iter()
                .map(|value| value.as_ref().and_then(|value| categories.row_of(value)))
                .collect(),
        )
    }

    fn range_of(values: &[Option<Value>]) -> Option<Range> {
        let mut found: Option<Range> = None;
        for number in values.iter().flatten().filter_map(Value::as_f64) {
            if number.is_nan() {
                continue;
            }
            found = Some(match found {
                None => Range {
                    min: number,
                    max: number,
                },
                Some(range) => Range {
                    min: range.min.min(number),
                    max: range.max.max(number),
                },
            });
        }
        found
    }
}

/// Why the values of a property cannot be read along a frame.
#[derive(Debug, thiserror::Error)]
pub enum Refusal {
    /// The object is not a wellbore frame.
    #[error(
        "this object states no `NodeMd`, so it is not a wellbore frame and its values have no depth to sit at"
    )]
    NotAFrame,
    /// Reading the frame's depths, or the property object, failed - in the
    /// reader's own words.
    #[error(transparent)]
    Read(#[from] Error),
    /// The property hangs on another representation.
    #[error(
        "`{property}` hangs on {}, not on this frame, so it says nothing about its nodes or its intervals",
        shown(.supporting)
    )]
    NotOnThisFrame {
        /// The property's title.
        property: String,
        /// The uuid of the representation it names.
        supporting: Option<String>,
    },
    /// The property hangs on neither the nodes nor the intervals of a frame.
    #[error("`{property}` hangs on {element}, which is neither the nodes nor the intervals of a frame")]
    Element {
        /// The property's title.
        property: String,
        /// What it hangs on.
        element: IndexableElement,
    },
    /// The property holds several values per element.
    #[error(
        "`{property}` holds {per_element} values per element: a vector or a matrix has no single value to give along a frame - which component?"
    )]
    Vector {
        /// The property's title.
        property: String,
        /// Values per element.
        per_element: u64,
    },
    /// The property declares `RealizationIndices`, which add a dimension.
    #[error(
        "`{property}` declares `RealizationIndices`, which add a dimension to its values - which realization?"
    )]
    Realizations {
        /// The property's title.
        property: String,
    },
    /// The property states no value array.
    #[error("`{property}` states no values")]
    NoValues {
        /// The property's title.
        property: String,
    },
    /// The property has several patches.
    #[error("`{property}` has {count} patches; a frame has one, and none is chosen for it")]
    Patches {
        /// The property's title.
        property: String,
        /// How many patches.
        count: usize,
    },
    /// The values could not be read.
    #[error("`{property}`: {source}")]
    Values {
        /// The property's title.
        property: String,
        /// The reader's refusal.
        source: Error,
    },
    /// The number of values is not the number of nodes or of intervals.
    #[error(
        "`{property}` holds {found} value(s) where the frame has {expected} {}(s) - reading it here would shift every one",
        element_word(.intervals)
    )]
    Count {
        /// The property's title.
        property: String,
        /// How many values the property holds.
        found: usize,
        /// How many the frame has.
        expected: usize,
        /// Whether the property hangs on the intervals.
        intervals: bool,
    },
    /// The property names a lookup table the package does not hold.
    #[error(
        "`{property}` names a category lookup ({}) that is not here, so its codes have no label; they are not written as if they were values",
        address(.uri, .uuid)
    )]
    LookupAbsent {
        /// The property's title.
        property: String,
        /// The uuid of the table.
        uuid: String,
        /// The uri of the table, when the property names one.
        uri: Option<String>,
    },
    /// The property names a lookup table that cannot be read.
    #[error(
        "`{property}` names a category lookup that cannot be read ({source}), so its codes have no label; they are not written as if they were values"
    )]
    LookupUnreadable {
        /// The property's title.
        property: String,
        /// The reader's refusal.
        source: Error,
    },
    /// The MDs of a representation cannot be compared with the frames' (carrying).
    #[error("{what}: {why}")]
    Incomparable {
        /// What was asked about.
        what: String,
        /// Why its MDs and the frames' cannot be compared.
        why: String,
    },
}

/// The representation a property hangs on, for a message.
fn shown(supporting: &Option<String>) -> &str {
    supporting.as_deref().unwrap_or("no representation")
}

/// `interval` or `node`, for a message.
fn element_word(intervals: &bool) -> &'static str {
    if *intervals { "interval" } else { "node" }
}

/// The address of a table, for a message: its uri when the property names one.
fn address<'a>(uri: &'a Option<String>, uuid: &'a str) -> &'a str {
    uri.as_deref().unwrap_or(uuid)
}

// ---------------------------------------------------------------------------
// The reading
// ---------------------------------------------------------------------------

/// The measured depth of each node of a frame, in the order the file gives them.
///
/// Read once and handed to [`of_with_mds`] for each property of the frame: the
/// array is the frame's, not the property's.
pub fn node_mds(frame: &dyn Reflect, source: &dyn ArraySource) -> Result<Vec<f64>, Refusal> {
    let Some(FieldValue::Object(array)) = frame.field(NODE_MD) else {
        return Err(Refusal::NotAFrame);
    };
    match values::read(array, source, NODE_MD)? {
        Values::Floating(values) => Ok(values),
        Values::Integer(values) => Ok(values.into_iter().map(|value| value as f64).collect()),
        _ => Err(Refusal::Read(Error::malformed(NODE_MD, "holds no numbers"))),
    }
}

/// The values of `property` along `frame`: its support, its values, its unit, its
/// range, and its table when it names one.
///
/// Reads the frame's `NodeMd` and then [`of_with_mds`]; a caller that reads many
/// properties of one frame reads the depths once ([`node_mds`]) and calls that.
pub fn of(
    frame: &dyn Reflect,
    property: &dyn Reflect,
    source: &dyn ArraySource,
) -> Result<FrameValues, Refusal> {
    let mds = node_mds(frame, source)?;
    let uuid = identity::uuid_of_reflect(frame).unwrap_or_default().to_string();
    of_with_mds(&uuid, &mds, property, source)
}

/// [`of`] for a caller that already holds the frame's uuid and measured depths.
pub fn of_with_mds(
    frame: &str,
    mds: &[f64],
    property: &dyn Reflect,
    source: &dyn ArraySource,
) -> Result<FrameValues, Refusal> {
    read_along(frame, mds, property, source, true)
}

/// [`of_with_mds`] for a property the caller **has already found hanging on the
/// frame** ([`crate::wellbore_table::hangs_on`]): the representation it names is not
/// asked again. Everything else is refused as it is there.
pub fn of_given(
    frame: &str,
    mds: &[f64],
    property: &dyn Reflect,
    source: &dyn ArraySource,
) -> Result<FrameValues, Refusal> {
    read_along(frame, mds, property, source, false)
}

fn read_along(
    frame: &str,
    mds: &[f64],
    property: &dyn Reflect,
    source: &dyn ArraySource,
    ask_the_support: bool,
) -> Result<FrameValues, Refusal> {
    let read = property::read(property)?;
    let title = read.title.clone().unwrap_or_else(|| read.uuid.clone());

    // **Only what hangs on this frame**: its values are placed on its nodes or its
    // intervals, and a property of another representation has no place here.
    if ask_the_support
        && !read
            .supporting_representation
            .as_deref()
            .is_some_and(|supporting| supporting.eq_ignore_ascii_case(frame))
    {
        return Err(Refusal::NotOnThisFrame {
            property: title,
            supporting: read.supporting_representation.clone(),
        });
    }
    let intervals = if read.indexable == IndexableElement::Nodes {
        false
    } else if read.indexable.is_frame_interval() {
        true
    } else {
        return Err(Refusal::Element {
            property: title,
            element: read.indexable,
        });
    };
    if read.values_per_element() != 1 {
        return Err(Refusal::Vector {
            property: title,
            per_element: read.values_per_element(),
        });
    }
    if read.has_realization_indices {
        return Err(Refusal::Realizations { property: title });
    }

    // The table, before the values: a code with no table to read it through is
    // refused, never written as if it were a value.
    let categories = match read.lookup.as_deref() {
        None => None,
        Some(uuid) => Some(categories_of(&title, uuid, read.lookup_uri.as_deref(), source)?),
    };

    let patch = match read.patches.as_slice() {
        [] => return Err(Refusal::NoValues { property: title }),
        [one] => one,
        many => {
            return Err(Refusal::Patches {
                property: title,
                count: many.len(),
            });
        }
    };
    let stored = patch.read(source).map_err(|source| Refusal::Values {
        property: title.clone(),
        source,
    })?;
    let expected = if intervals {
        mds.len().saturating_sub(1)
    } else {
        mds.len()
    };
    if stored.len() != expected {
        return Err(Refusal::Count {
            property: title,
            found: stored.len(),
            expected,
            intervals,
        });
    }

    let continuous = read.qualified_type.contains("ContinuousProperty");
    let values = convert(stored, values::null_value(patch.array), continuous);
    let support = if intervals {
        let (top, base) = if mds.len() < 2 {
            (Vec::new(), Vec::new())
        } else {
            (mds[..mds.len() - 1].to_vec(), mds[1..].to_vec())
        };
        Support::Intervals { top, base }
    } else {
        Support::Nodes { md: mds.to_vec() }
    };
    let range = FrameValues::range_of(&values);
    Ok(FrameValues {
        frame: frame.to_ascii_lowercase(),
        property: read.uuid.clone(),
        title,
        element: read.indexable,
        support,
        values,
        continuous,
        uom: read.uom.clone(),
        kind: read.kind.clone(),
        kind_uri: read.kind_uri.clone(),
        range,
        categories,
        time: read.time.clone(),
        realization: read.realization,
        derivation: None,
        notes: Vec::new(),
    })
}

/// The table of a property, read.
fn categories_of(
    title: &str,
    uuid: &str,
    uri: Option<&str>,
    source: &dyn ArraySource,
) -> Result<Categories, Refusal> {
    // By the uri the property names: the version named is the version read.
    let object = match uri {
        Some(uri) => source.object_at(uri),
        None => source.object(uuid),
    };
    let Some(object) = object else {
        return Err(Refusal::LookupAbsent {
            property: title.to_string(),
            uuid: uuid.to_string(),
            uri: uri.map(str::to_string),
        });
    };
    let table_type = object
        .as_reflect()
        .type_info()
        .name
        .trim_start_matches("obj_")
        .to_string();
    let (lookup, columns) =
        Lookup::read_described(object.as_reflect(), source).map_err(|source| Refusal::LookupUnreadable {
            property: title.to_string(),
            source,
        })?;
    let mut categories = Categories::new(uuid.to_string(), table_type, lookup, columns);
    categories.uri = uri.map(str::to_string);
    Ok(categories)
}

/// The stored values, as values: an integer is a code - unless the property is
/// continuous, where it is a number stored as an integer - and the file's absence
/// is `None`.
fn convert(stored: Values, null: Option<i64>, continuous: bool) -> Vec<Option<Value>> {
    match stored {
        Values::Floating(values) => values
            .into_iter()
            .map(|number| number.is_finite().then_some(Value::Number(number)))
            .collect(),
        Values::Integer(values) => values
            .into_iter()
            .map(|code| {
                if Some(code) == null {
                    None
                } else if continuous {
                    Some(Value::Number(code as f64))
                } else {
                    Some(Value::Code(code))
                }
            })
            .collect(),
        Values::Boolean(values) => values
            .into_iter()
            .map(|flag| Some(Value::Boolean(flag)))
            .collect(),
        Values::Text(values) => values.into_iter().map(|text| Some(Value::Text(text))).collect(),
    }
}
