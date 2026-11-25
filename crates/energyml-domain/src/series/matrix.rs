//! A matrix of values per element along a well, and planes of it.
//!
//! RESQML TRG 2.2 §7.1 p. 222: *"Values attached to a given element can be
//! either a scalar or a vector. The size of the vector is constant on all
//! elements"*, and a property may declare several dimensions of components
//! (`ValueCountPerIndexableElement`, the first given the slowest). A directional
//! resistivity is `[81, 81]` values per node; a radial measurement `[16]`.
//! This module reads such a property on a wellbore frame, whole or one plane at
//! a time, with **one** rule for what the rows are (nodes or intervals) and one
//! for the count.
//!
//! A type of its own rather than a field of [`super::Channel`], which stays
//! scalar (S9): every consumer of `Channel`, decimation included, reasons per
//! scalar value. The array channels of a WITSML channel set (those with an
//! `AxisDefinition`) are read here too, as [`MatrixChannel`]s
//! ([`read_channel_set`]); [`super::channel_set::read`] keeps refusing a cell
//! that is an array.
//!
//! What is refused, by name: a categorical or a non-numeric matrix, a property
//! whose time or realizations are a dimension (choose a step first), a support
//! that is neither the nodes nor the intervals of a frame, a count that is
//! neither support's, and two `AxisDefinition` on a channel (WITSML TRG 2.1
//! §7.12 p. 162 does not say which is the slowest).

use energyml_core::value::Reflect;
use serde_json::Value;

use super::channel_set::{self, Decode, read_number};
use super::{Index, IndexKind, Refused, monotone, number_of, objects};
use crate::property::shape::{
    ComponentPick, ComponentRefusal, CountFit, ElementAxes, ElementShape, PropertyShape, ShapeRefusal,
    count_fit, time_is_a_dimension,
};
use crate::property::{IndexableElement, Property};
use crate::source::ArraySource;
use crate::values::{self, Block, Family};

/// What the rows of a matrix stand for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowSupport {
    /// One row per node of the frame.
    Nodes,
    /// One row per interval between two nodes.
    Intervals,
    /// One row per sample of a WITSML index.
    Samples,
}

/// A note: what was read differs from what was declared, and it was
/// resolvable.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum MatrixNote {
    /// The values count as the other support than the indexable element says.
    SupportFromCount {
        /// The support the indexable element declares.
        declared: RowSupport,
        /// The support the count fits, and that was used.
        used: RowSupport,
    },
    /// The plane was cut from the whole array: the store does not hold it so
    /// that one block reads it.
    ReadWhole,
}

/// A matrix per element.
#[derive(Clone, PartialEq, Debug)]
pub struct MatrixChannel {
    /// Full uri of the property (or of the WITSML channel).
    pub uri: String,
    /// Its title.
    pub title: Option<String>,
    /// Its unit.
    pub uom: Option<String>,
    /// The shape of one element's components.
    pub shape: ElementShape,
    /// What the rows stand for.
    pub support: RowSupport,
    /// How many rows.
    pub rows: usize,
    /// Row-major `[row][linear component]`; `NaN` is absent and stays so.
    pub values: Vec<f64>,
    /// The index of a WITSML matrix; `None` for a RESQML property, whose
    /// depths are its frame's.
    pub index: Option<Index>,
    /// What was resolved on the way.
    pub notes: Vec<MatrixNote>,
}

/// Rows of picked values: a plane of the matrix.
#[derive(Clone, PartialEq, Debug)]
pub struct Plane {
    /// How many rows.
    pub rows: usize,
    /// Values per row ([`ComponentPick::width`]).
    pub cols: usize,
    /// The pick it was cut by.
    pub pick: ComponentPick,
    /// Row-major `[row][col]`.
    pub values: Vec<f64>,
    /// Non-finite values in it, counted.
    pub absent: usize,
    /// What was resolved on the way.
    pub notes: Vec<MatrixNote>,
}

/// Why a matrix was not read.
#[derive(Debug, thiserror::Error)]
pub enum MatrixRefusal {
    /// The count is neither the nodes' nor the intervals'.
    #[error(
        "{values} value(s) are neither {nodes} node(s) nor {intervals} interval(s) of {per_element} value(s) each"
    )]
    Count {
        /// Values the property holds.
        values: u64,
        /// Nodes of the frame.
        nodes: u64,
        /// Intervals of the frame.
        intervals: u64,
        /// Values per element declared.
        per_element: u64,
    },
    /// One value per element where several are declared.
    #[error("declares {declared} values per element and holds one: read it as a curve")]
    OnePerElement {
        /// Values per element declared.
        declared: u64,
    },
    /// Not numbers.
    #[error("a matrix of {family:?} values: only numbers make a matrix")]
    NotNumeric {
        /// The family the array holds.
        family: Family,
    },
    /// Codes of a lookup table.
    #[error("the property names a lookup table: an image of codes is not an image of values")]
    Categorical,
    /// Time is a dimension.
    #[error("the property's time is a dimension: choose a step before reading a matrix")]
    Time,
    /// Realizations are a dimension.
    #[error("the property declares realizations: choose one before reading a matrix")]
    Realizations,
    /// Not the nodes or the intervals of a frame.
    #[error("a matrix hangs on the nodes or the intervals of a frame, not on `{indexable}`")]
    Support {
        /// What the property hangs on.
        indexable: IndexableElement,
    },
    /// Not exactly one patch.
    #[error("{count} patch(es): a frame property has one")]
    Patches {
        /// How many patches.
        count: usize,
    },
    /// A shape that does not hold.
    #[error(transparent)]
    Shape(#[from] ShapeRefusal),
    /// A pick that does not fit the shape.
    #[error(transparent)]
    Pick(#[from] ComponentRefusal),
    /// Reading the array failed.
    #[error(transparent)]
    Read(#[from] crate::Error),
    /// The channel set does not read as rows.
    #[error(transparent)]
    Series(#[from] Refused),
    /// A cell of an array channel whose length is not the axis count.
    #[error("row {row}, `{channel}`: a cell of {found} value(s) where the axis counts {expected}")]
    CellLength {
        /// The 1-based row.
        row: usize,
        /// The channel's mnemonic.
        channel: String,
        /// Values the cell holds.
        found: usize,
        /// The axis count.
        expected: usize,
    },
    /// Several `AxisDefinition` on one channel.
    #[error(
        "the channel `{channel}` declares {axes} `AxisDefinition`: WITSML TRG 2.1 §7.12 p. 162 does not say which is the slowest"
    )]
    AxisOrderUnstated {
        /// The channel's mnemonic.
        channel: String,
        /// How many axes.
        axes: usize,
    },
}

/// What the two reads decide the same way: the shape, the support, the rows and
/// the notes.
struct Facts {
    shape: ElementShape,
    support: RowSupport,
    rows: u64,
    notes: Vec<MatrixNote>,
}

/// The guards both reads share, so that they decide alike.
fn matrix_facts(property: &Property<'_>, nodes: u64, values_count: u64) -> Result<Facts, MatrixRefusal> {
    if property.lookup.is_some() {
        return Err(MatrixRefusal::Categorical);
    }
    if time_is_a_dimension(property.time.as_ref()) {
        return Err(MatrixRefusal::Time);
    }
    if property.has_realization_indices {
        return Err(MatrixRefusal::Realizations);
    }
    let shape = ElementShape::of_property(property)?;
    let per = shape.len() as u64;
    let declared = match property.indexable {
        IndexableElement::Nodes => RowSupport::Nodes,
        indexable if indexable.is_frame_interval() || indexable == IndexableElement::IntervalEdges => {
            RowSupport::Intervals
        }
        indexable => return Err(MatrixRefusal::Support { indexable }),
    };
    let intervals = nodes.saturating_sub(1);
    let rows_of = |support: RowSupport| match support {
        RowSupport::Intervals => intervals,
        _ => nodes,
    };
    let other = match declared {
        RowSupport::Intervals => RowSupport::Nodes,
        _ => RowSupport::Intervals,
    };
    let (support, rows, notes) = match count_fit(rows_of(declared), per, values_count) {
        CountFit::Fits => (declared, rows_of(declared), Vec::new()),
        CountFit::OnePerElement { declared } => return Err(MatrixRefusal::OnePerElement { declared }),
        CountFit::Mismatch { .. } => {
            // A contradiction that the count resolves is a note, not a refusal.
            if count_fit(rows_of(other), per, values_count) == CountFit::Fits {
                (
                    other,
                    rows_of(other),
                    vec![MatrixNote::SupportFromCount {
                        declared,
                        used: other,
                    }],
                )
            } else {
                return Err(MatrixRefusal::Count {
                    values: values_count,
                    nodes,
                    intervals,
                    per_element: per,
                });
            }
        }
    };
    Ok(Facts {
        shape,
        support,
        rows,
        notes,
    })
}

/// The one patch of a frame property, and its count of values.
fn one_patch<'a, 'p>(
    property: &'p Property<'a>,
    source: &dyn ArraySource,
) -> Result<(&'p crate::property::Patch<'a>, u64), MatrixRefusal> {
    if property.patches.len() != 1 {
        return Err(MatrixRefusal::Patches {
            count: property.patches.len(),
        });
    }
    let patch = &property.patches[0];
    match values::family(patch.array) {
        Some(Family::Boolean) => {
            return Err(MatrixRefusal::NotNumeric {
                family: Family::Boolean,
            });
        }
        Some(Family::Text) => {
            return Err(MatrixRefusal::NotNumeric { family: Family::Text });
        }
        _ => {}
    }
    let count = values::count(patch.array, source, &patch_name(property))?;
    Ok((patch, count as u64))
}

fn patch_name(property: &Property<'_>) -> String {
    format!("the values of `{}`", property.uuid)
}

/// The whole matrix of a property on a frame of `nodes` nodes. `uri` is the
/// property's full uri (a `Property` carries its uuid only: in this library
/// the identity is the full uri, and a caller holding a uuid turns it into one
/// at the entry).
///
/// # Errors
///
/// A [`MatrixRefusal`] by name: a categorical, non-numeric, time-dimensioned or
/// realization-dimensioned property, a support that is not the frame's, a
/// count that is neither the nodes' nor the intervals', one value per element
/// where several are declared.
pub fn read_matrix(
    uri: &str,
    property: &Property<'_>,
    nodes: u64,
    source: &dyn ArraySource,
) -> Result<MatrixChannel, MatrixRefusal> {
    let (patch, count) = one_patch(property, source)?;
    let facts = matrix_facts(property, nodes, count)?;
    matrix_of(uri, property, patch, facts, source)
}

fn matrix_of(
    uri: &str,
    property: &Property<'_>,
    patch: &crate::property::Patch<'_>,
    facts: Facts,
    source: &dyn ArraySource,
) -> Result<MatrixChannel, MatrixRefusal> {
    let read = patch.read(source)?;
    let family = read.family();
    let values = read.to_f64().ok_or(MatrixRefusal::NotNumeric { family })?;
    Ok(MatrixChannel {
        uri: uri.to_string(),
        title: property.title.clone(),
        uom: property.uom.clone(),
        shape: facts.shape,
        support: facts.support,
        rows: facts.rows as usize,
        values,
        index: None,
        notes: facts.notes,
    })
}

/// One plane, reading as little as the store allows.
///
/// Read by **block** when the store keeps the rows on its first dimension and
/// each dimension of the components on its own, and the pick is one component
/// or one line of components; read whole, then cut, otherwise (a flattened
/// store, `All`, a reduction), and the plane says so
/// ([`MatrixNote::ReadWhole`]).
///
/// # Errors
///
/// The refusals of [`read_matrix`], plus a pick that does not fit the shape.
pub fn read_plane(
    uri: &str,
    property: &Property<'_>,
    nodes: u64,
    pick: &ComponentPick,
    source: &dyn ArraySource,
) -> Result<Plane, MatrixRefusal> {
    let (patch, count) = one_patch(property, source)?;
    let facts = matrix_facts(property, nodes, count)?;
    pick.check(&facts.shape)?;
    let rows = facts.rows as usize;

    if matches!(pick, ComponentPick::At(_) | ComponentPick::Along { .. }) {
        let at = patch_name(property);
        let stored = values::stored_dims(patch.array, source, &at)?;
        let declared = PropertyShape {
            realizations: None,
            time: None,
            elements: ElementAxes::Ordered(vec![facts.rows]),
            components: facts.shape.clone(),
        };
        let layout = declared.layout(&stored)?;
        let rank = facts.shape.rank();
        let by_dimension = layout.groups.len() == 1 + rank
            && layout.groups[0] == (0..1)
            && (0..rank).all(|k| layout.groups[1 + k] == (1 + k..2 + k));
        if by_dimension {
            let mut start = vec![0u64; 1 + rank];
            let mut block_count = vec![1u64; 1 + rank];
            block_count[0] = facts.rows;
            match pick {
                ComponentPick::At(at) => {
                    for (k, fixed) in at.iter().enumerate() {
                        start[1 + k] = *fixed as u64;
                    }
                }
                ComponentPick::Along { axis, at } => {
                    for (k, fixed) in at.iter().enumerate() {
                        if k != *axis {
                            start[1 + k] = *fixed as u64;
                        }
                    }
                    block_count[1 + *axis] = facts.shape.dims[*axis] as u64;
                }
                _ => {}
            }
            let block = Block {
                start,
                count: block_count,
            };
            let read = values::read_block(patch.array, source, &at, &block)?;
            let family = read.family();
            let values = read.to_f64().ok_or(MatrixRefusal::NotNumeric { family })?;
            let cols = pick.width(&facts.shape)?;
            let absent = values.iter().filter(|value| !value.is_finite()).count();
            return Ok(Plane {
                rows,
                cols,
                pick: pick.clone(),
                values,
                absent,
                notes: facts.notes,
            });
        }
    }

    let mut matrix = matrix_of(uri, property, patch, facts, source)?;
    matrix.notes.push(MatrixNote::ReadWhole);
    Ok(matrix.plane(pick)?)
}

impl MatrixChannel {
    /// The plane a pick cuts from the matrix in memory.
    ///
    /// # Errors
    ///
    /// A [`ComponentRefusal`] when the pick does not fit the shape.
    pub fn plane(&self, pick: &ComponentPick) -> Result<Plane, ComponentRefusal> {
        pick.check(&self.shape)?;
        let per = self.shape.len();
        let mut values = Vec::new();
        for row in 0..self.rows {
            values.extend(pick.select(&self.shape, &self.values[row * per..(row + 1) * per])?);
        }
        let absent = values.iter().filter(|value| !value.is_finite()).count();
        Ok(Plane {
            rows: self.rows,
            cols: pick.width(&self.shape)?,
            pick: pick.clone(),
            values,
            absent,
            notes: self.notes.clone(),
        })
    }

    /// One value: row `row`, component `at`. `None` out of range.
    pub fn cell(&self, row: usize, at: &[usize]) -> Option<f64> {
        if row >= self.rows {
            return None;
        }
        let linear = self.shape.ravel(at)?;
        self.values.get(row * self.shape.len() + linear).copied()
    }

    /// The values of one row (one element), all components.
    pub fn row(&self, row: usize) -> Option<&[f64]> {
        let per = self.shape.len();
        if row >= self.rows {
            return None;
        }
        self.values.get(row * per..(row + 1) * per)
    }
}

// ---------------------------------------------------------------------------
// WITSML channels with an axis
// ---------------------------------------------------------------------------

/// The array channels of a channel set (those with an `AxisDefinition`), each
/// as a matrix. The scalar channels are not here: [`channel_set::read_with`]
/// reads them.
///
/// The row layout is **inferred**, like the whole grammar of the rows of a
/// channel set (see [`channel_set`]): a row is the value of each index, then one
/// cell per channel; the cell of an array channel is a **flat** JSON array of
/// `AxisCount` numbers, or `null` for a missing sample. A cell of another length
/// is [`MatrixRefusal::CellLength`]; an array of arrays is refused by the cell
/// reader.
///
/// The axes are resolved by `component_axes` with
/// `AxisLink::WitsmlChannel { channel: <the uri of the channel> }`.
///
/// # Errors
///
/// [`MatrixRefusal::AxisOrderUnstated`] for several `AxisDefinition` on a
/// channel (their product would guess their order); the refusals of the set's
/// own reading ([`MatrixRefusal::Series`]).
pub fn read_channel_set(
    set: &dyn Reflect,
    arrays: &dyn ArraySource,
) -> Result<Vec<MatrixChannel>, MatrixRefusal> {
    if !channel_set::reads(set) {
        return Err(MatrixRefusal::Series(Refused::NotThisReader {
            expected: "a WITSML ChannelSet",
            found: set.type_info().type_key.to_string(),
        }));
    }
    let (indexes, listed, rows) = channel_set::rows_of(set, arrays)?;
    let width = indexes.len() + listed.len();
    let channels = objects(set, "Channel");

    // The primary index, as `channel_set::read_with` builds it.
    let primary = &indexes[0];
    let decode = if primary.kind == IndexKind::DateTime {
        Decode::Instant
    } else {
        Decode::Number
    };
    let mut cells_of_rows: Vec<&Vec<Value>> = Vec::with_capacity(rows.len());
    let mut index_values = Vec::with_capacity(rows.len());
    for (at, row) in rows.iter().enumerate() {
        let number = at + 1;
        let Value::Array(cells) = row else {
            return Err(Refused::NotRows {
                why: format!("row {number} is not an array"),
            }
            .into());
        };
        if cells.len() != width {
            return Err(Refused::RowLength {
                row: number,
                found: cells.len(),
                expected: width,
                indexes: indexes.len(),
                channels: listed.len(),
            }
            .into());
        }
        match read_number(&cells[0], decode, number, &primary.mnemonic)? {
            Some(value) if value.is_finite() => index_values.push(value),
            _ => {
                return Err(Refused::NullIndex {
                    row: number,
                    index: primary.mnemonic.clone(),
                }
                .into());
            }
        }
        cells_of_rows.push(cells);
    }
    let index = Index {
        kind: primary.kind,
        mnemonic: primary.mnemonic.clone(),
        uom: primary.uom.clone(),
        direction: primary.direction,
        datum: primary.datum.clone(),
        values: index_values,
    };
    monotone(&index)?;

    let mut out = Vec::new();
    for (rank, one) in listed.iter().enumerate() {
        let Some(channel) = channels.get(rank) else {
            continue;
        };
        let definitions = objects(*channel, "AxisDefinition");
        let count = match definitions.len() {
            0 => continue,
            1 => number_of(definitions[0], "AxisCount").unwrap_or(0.0).max(0.0) as usize,
            axes => {
                return Err(MatrixRefusal::AxisOrderUnstated {
                    channel: one.mnemonic.clone(),
                    axes,
                });
            }
        };
        let shape = ElementShape::of(&[count as u64], &[])?;
        let mut values = Vec::with_capacity(rows.len() * count);
        for (at, cells) in cells_of_rows.iter().enumerate() {
            let number = at + 1;
            match &cells[indexes.len() + rank] {
                Value::Null => values.extend(std::iter::repeat_n(f64::NAN, count)),
                Value::Array(items) => {
                    if items.len() != count {
                        return Err(MatrixRefusal::CellLength {
                            row: number,
                            channel: one.mnemonic.clone(),
                            found: items.len(),
                            expected: count,
                        });
                    }
                    for item in items {
                        values.push(
                            read_number(item, Decode::Number, number, &one.mnemonic)?.unwrap_or(f64::NAN),
                        );
                    }
                }
                _ => {
                    return Err(Refused::ValueKind {
                        row: number,
                        column: one.mnemonic.clone(),
                        expected: "an array",
                        found: "a scalar".to_string(),
                    }
                    .into());
                }
            }
        }
        let uuid = one.uuid.clone().ok_or_else(|| {
            crate::Error::malformed(
                one.mnemonic.clone(),
                "an array channel with no uuid: nothing gives it an identity",
            )
        })?;
        let qualified = channel.type_info().qualified_type.unwrap_or("witsml21.Channel");
        out.push(MatrixChannel {
            uri: energyml_core::identifiers::EtpUri::object(None, qualified, &uuid),
            title: Some(one.mnemonic.clone()),
            uom: one.uom.clone(),
            shape,
            support: RowSupport::Samples,
            rows: rows.len(),
            values,
            index: Some(index.clone()),
            notes: Vec::new(),
        });
    }
    Ok(out)
}
