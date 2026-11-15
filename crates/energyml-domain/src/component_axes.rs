//! What each dimension of a vector property's components stands for.
//!
//! RESQML has no field for the coordinate of a component: RESQML TRG 2.2 §7.1
//! p. 222 gives *labels* (`LabelPerComponent`) and nothing else. A matrix of
//! `[81, 81]` values per element is 6 561 numbers whose axes (a length from -40
//! to 40 m, 16 angles, 81 ranks) live elsewhere or nowhere. This module asks the
//! places they may live, in order.
//!
//! Sources are tried in order, the caller's first; the first that answers for a
//! dimension wins; a dimension no source answers is given its ranks, **and says
//! so** ([`ResolvedAxes::ranks_only`]). A source that applies and fails refuses:
//! an axis the caller or the file declared is never silently replaced by ranks.
//!
//! What this module never does: write an axis into a package (S2: a declaration
//! is a parameter, kept in a recipe or a view document), and apply a table it
//! only guessed ([`candidate_tables`] *offers*, [`AxisSources::resolve`]
//! *applies*, and the second does not call the first).
//!
//! # Convention B
//!
//! A property links its axes to a table by **one** facet `qualifier` whose value
//! is the full uri of that table ([`crate::property::qualifier_link`], the one
//! reading of the convention, ADR-0029). The table holds exactly one key column
//! and one value column per dimension, **in the order of the dimensions, the
//! slowest first**; that order is this library's interpretation (the standard
//! states none) and is recorded in ADR-0029. All the columns of a table have the
//! same number of rows, so a shape with unequal dimensions (`[3, 2]`) does not
//! fit a table and is refused by name.

use energyml_core::registry::DynObject;

use crate::package_index::{NodeState, PackageIndex};
use crate::property::link::{QualifierRefusal, qualifier_link};
use crate::property::shape::ElementShape;
use crate::property::{self, Facet, PropertyKindRef};
use crate::source::{ArraySource, ObjectSource};
use crate::table::{Column, Table};
use crate::units::{self, Dimension};
use crate::values::{self, Family, Values};

/// Two numbers written twice by one producer differ by their rounding only:
/// this is the relative tolerance with which an axis that must repeat (a paired
/// property constant along the index) and a full turn of angles are compared.
pub const PAIRED_AXIS_TOLERANCE: f64 = 1e-9;

/// The values along one axis.
#[derive(Clone, PartialEq, Debug)]
pub enum AxisValues {
    /// `start + i * step`, `i < count` (a WITSML `LogChannelAxis`).
    Regular {
        /// The first value.
        start: f64,
        /// The increment.
        step: f64,
        /// How many values.
        count: usize,
    },
    /// Listed numbers, finite.
    Listed(Vec<f64>),
    /// Names, not numbers (`LabelPerComponent` on a one-dimension shape).
    Labels(Vec<String>),
    /// `0 .. count`: nothing says more.
    Ranks(usize),
}

impl AxisValues {
    /// How many values the axis holds.
    pub fn len(&self) -> usize {
        match self {
            AxisValues::Regular { count, .. } => *count,
            AxisValues::Listed(values) => values.len(),
            AxisValues::Labels(labels) => labels.len(),
            AxisValues::Ranks(count) => *count,
        }
    }

    /// Whether the axis holds no value.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Where an axis was found.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AxisOrigin {
    /// `LabelPerComponent` of a one-dimension shape.
    LabelPerComponent,
    /// A property of the shape of the axis, constant along the index.
    PairedProperty {
        /// Its uri.
        uri: String,
    },
    /// Convention B: the property's single `qualifier` facet names the table.
    QualifierTable {
        /// The table's uri.
        table: String,
        /// The title of the value column read.
        column: String,
    },
    /// A table the caller declared.
    DeclaredTable {
        /// The table's uri.
        table: String,
        /// The title of the column read.
        column: String,
    },
    /// A WITSML `LogChannelAxis`.
    WitsmlAxisDefinition {
        /// The channel's uri.
        channel: String,
        /// The `uid` of the axis.
        uid: String,
    },
    /// No source answered: the ranks fill the dimension.
    Ranks,
}

/// One dimension of the components, and what it stands for.
#[derive(Clone, PartialEq, Debug)]
pub struct ComponentAxis {
    /// Which dimension of the [`ElementShape`].
    pub dim: usize,
    /// The name of the axis, when the source gives one.
    pub name: Option<String>,
    /// The property kind of the axis, as written.
    pub kind: Option<String>,
    /// The unit of the values.
    pub uom: Option<String>,
    /// The values along the axis.
    pub values: AxisValues,
    /// The period, in `uom`, when the axis is a full turn of angles (cyclic).
    pub cyclic: Option<f64>,
    /// Where it was found.
    pub origin: AxisOrigin,
}

/// An axis the caller declares (kept in a recipe or a view document, never
/// written to the package by this module: S2).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AxisLink {
    /// A column of a table, by its title, for one dimension.
    Table {
        /// The table's uri.
        table: String,
        /// The column's title, exactly.
        column: String,
        /// The dimension it gives.
        dim: usize,
    },
    /// A property of shape `[dims[dim]]` on the same support, constant along it
    /// (the angles of a directional measurement).
    PairedProperty {
        /// The paired property's uri.
        property: String,
        /// The dimension it gives.
        dim: usize,
    },
    /// A WITSML channel whose `AxisDefinition` gives the axis.
    WitsmlChannel {
        /// The channel's uri.
        channel: String,
    },
}

/// What the sources are asked about.
pub struct AxisSubject<'a> {
    /// Full uri of the property (or of the WITSML channel for a
    /// [`crate::series::matrix::MatrixChannel`] read from a log).
    pub uri: &'a str,
    /// The shape of the components.
    pub shape: &'a ElementShape,
    /// Its facets.
    pub facets: &'a [Facet],
    /// The axes the caller declares.
    pub links: &'a [AxisLink],
    /// Rows (elements) of the subject, when known: a paired property must have
    /// as many.
    pub rows: Option<u64>,
}

/// Why an axis was refused.
#[derive(Debug, thiserror::Error)]
pub enum AxisRefusal {
    /// An axis whose length is not the dimension's.
    #[error("an axis of {axis} value(s) for dimension {dim}, which holds {shape}")]
    CountMismatch {
        /// The dimension.
        dim: usize,
        /// What the source gave (rows, for a paired property compared with the subject's rows).
        axis: usize,
        /// What the shape holds.
        shape: usize,
    },
    /// Several `AxisDefinition` on one channel.
    #[error(
        "{axes} `AxisDefinition` on one channel: WITSML TRG 2.1 §7.12 p. 162 does not say which is the slowest"
    )]
    OrderUnstated {
        /// How many axes the channel declares.
        axes: usize,
    },
    /// A paired axis that changes along the index.
    #[error(
        "the paired axis changes at row {row} (dimension {dim}): an axis that varies along the index is not an axis"
    )]
    VariesAlongIndex {
        /// The dimension.
        dim: usize,
        /// The first row that differs.
        row: usize,
    },
    /// A column that holds no number.
    #[error("the column `{column}` is not numeric")]
    NotNumeric {
        /// Its title (or the paired property's uri).
        column: String,
    },
    /// A value that is not finite.
    #[error("value {at} of the axis of dimension {dim} is not finite")]
    NotFinite {
        /// The dimension.
        dim: usize,
        /// The position of the value.
        at: usize,
    },
    /// Convention B refused by [`qualifier_link`].
    #[error(transparent)]
    Qualifier(#[from] QualifierRefusal),
    /// A linked table whose value columns are not as many as the dimensions.
    #[error("the table `{table}` holds {columns} value column(s) for a shape of {rank} dimension(s)")]
    TableColumns {
        /// The table's uri.
        table: String,
        /// Its value columns.
        columns: usize,
        /// The shape's dimensions.
        rank: usize,
    },
    /// A linked table without exactly one key column.
    #[error("the table `{table}` has {keys} key column(s): a linked table has exactly one (S3)")]
    TableKeys {
        /// The table's uri.
        table: String,
        /// Its key columns.
        keys: usize,
    },
    /// A column title the table does not hold.
    #[error("the table `{table}` has no column titled `{column}`")]
    NoSuchColumn {
        /// The table's uri.
        table: String,
        /// The title asked for.
        column: String,
    },
    /// A uri the object source does not hold.
    #[error("`{uri}` names nothing this source holds")]
    NotFound {
        /// The uri.
        uri: String,
    },
    /// A dimension past the shape's rank.
    #[error("dimension {dim} is past the shape's {rank}")]
    NoSuchDimension {
        /// The dimension asked for.
        dim: usize,
        /// The shape's rank.
        rank: usize,
    },
    /// Reading an object or an array failed.
    #[error(transparent)]
    Read(#[from] crate::Error),
}

/// One way of finding axes.
pub trait AxisSource: Send + Sync {
    /// Stable id, for a registry lookup and for a note.
    fn id(&self) -> &'static str;

    /// `None`: this source has nothing to say about the subject.
    /// `Some(Err)`: it applies and refuses. `Some(Ok(axes))`: axes for some
    /// dimensions.
    fn axes(
        &self,
        subject: &AxisSubject<'_>,
        objects: &dyn ObjectSource,
        arrays: &dyn ArraySource,
    ) -> Option<Result<Vec<ComponentAxis>, AxisRefusal>>;
}

/// The registry: built-ins, the caller's additions tried before them.
pub struct AxisSources(Vec<Box<dyn AxisSource>>);

/// Every dimension's axis, in dimension order.
#[derive(Clone, PartialEq, Debug)]
pub struct ResolvedAxes {
    /// One axis per dimension of the shape.
    pub axes: Vec<ComponentAxis>,
}

impl ResolvedAxes {
    /// The dimensions only ranks describe: what a view says ("axis not
    /// declared").
    pub fn ranks_only(&self) -> Vec<usize> {
        self.axes
            .iter()
            .filter(|axis| axis.origin == AxisOrigin::Ranks)
            .map(|axis| axis.dim)
            .collect()
    }
}

impl AxisSources {
    /// The built-ins, in this order: `declared-table`, `paired-property`,
    /// `witsml-axis-definition`, `qualifier-table`, `label-per-component`.
    /// Ranks are not a source: they fill what is left.
    ///
    /// What the caller **declared** comes first, then what **the file** says:
    /// the caller declares only what they saw, and if they contradict the file
    /// it is their gesture.
    pub fn with_builtins() -> Self {
        AxisSources(vec![
            Box::new(DeclaredTable),
            Box::new(PairedProperty),
            Box::new(WitsmlAxisDefinition),
            Box::new(QualifierTable),
            Box::new(LabelPerComponent),
        ])
    }

    /// Tried before every source already registered.
    pub fn register(&mut self, source: Box<dyn AxisSource>) {
        self.0.insert(0, source);
    }

    /// The source of this id.
    pub fn find(&self, id: &str) -> Option<&dyn AxisSource> {
        self.0.iter().find(|source| source.id() == id).map(|source| &**source)
    }

    /// Every dimension's axis; the first answer for a dimension wins.
    ///
    /// # Errors
    ///
    /// The refusal of the first source that applies and fails; a source
    /// answering for a dimension the shape does not have
    /// ([`AxisRefusal::NoSuchDimension`]); an axis whose length is not its
    /// dimension's ([`AxisRefusal::CountMismatch`]).
    pub fn resolve(
        &self,
        subject: &AxisSubject<'_>,
        objects: &dyn ObjectSource,
        arrays: &dyn ArraySource,
    ) -> Result<ResolvedAxes, AxisRefusal> {
        let rank = subject.shape.rank();
        let mut found: Vec<Option<ComponentAxis>> = vec![None; rank];
        for source in &self.0 {
            match source.axes(subject, objects, arrays) {
                None => continue,
                Some(Err(refusal)) => return Err(refusal),
                Some(Ok(axes)) => {
                    for axis in axes {
                        if axis.dim >= rank {
                            return Err(AxisRefusal::NoSuchDimension { dim: axis.dim, rank });
                        }
                        let place = axis.dim;
                        if found[place].is_none() {
                            found[place] = Some(axis);
                        }
                    }
                }
            }
        }
        let mut axes = Vec::with_capacity(rank);
        for (dim, place) in found.into_iter().enumerate() {
            let held = subject.shape.dims[dim];
            let axis = place.unwrap_or(ComponentAxis {
                dim,
                name: None,
                kind: None,
                uom: None,
                values: AxisValues::Ranks(held),
                cyclic: None,
                origin: AxisOrigin::Ranks,
            });
            if axis.values.len() != held {
                return Err(AxisRefusal::CountMismatch {
                    dim,
                    axis: axis.values.len(),
                    shape: held,
                });
            }
            axes.push(axis);
        }
        Ok(ResolvedAxes { axes })
    }
}

impl Default for AxisSources {
    fn default() -> Self {
        Self::with_builtins()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn read_object(objects: &dyn ObjectSource, uri: &str) -> Result<Box<dyn DynObject>, AxisRefusal> {
    objects
        .read_object_at(uri)
        .map_err(|_| AxisRefusal::NotFound { uri: uri.to_string() })
}

/// The title of the kind a column names, as written.
fn kind_text(kind: &Option<PropertyKindRef>) -> Option<String> {
    match kind.as_ref()? {
        PropertyKindRef::Object { title, .. } => title.clone(),
        PropertyKindRef::Standard { name } => Some(name.clone()),
    }
}

/// The numbers of a column, every one finite.
fn numbers_of(column: &Column<'_>, arrays: &dyn ArraySource, dim: usize) -> Result<Vec<f64>, AxisRefusal> {
    let title = column.title.clone().unwrap_or_default();
    let numbers = match column.read(arrays)? {
        Values::Floating(all) => all,
        Values::Integer(all) => all.into_iter().map(|value| value as f64).collect(),
        _ => return Err(AxisRefusal::NotNumeric { column: title }),
    };
    if let Some(at) = numbers.iter().position(|value| !value.is_finite()) {
        return Err(AxisRefusal::NotFinite { dim, at });
    }
    Ok(numbers)
}

/// A column of a table as an axis of one dimension.
fn axis_of_column(
    column: &Column<'_>,
    arrays: &dyn ArraySource,
    dim: usize,
    held: usize,
    origin: AxisOrigin,
) -> Result<ComponentAxis, AxisRefusal> {
    let numbers = numbers_of(column, arrays, dim)?;
    if numbers.len() != held {
        return Err(AxisRefusal::CountMismatch {
            dim,
            axis: numbers.len(),
            shape: held,
        });
    }
    Ok(ComponentAxis {
        dim,
        name: column.title.clone(),
        kind: kind_text(&column.kind),
        uom: column.uom.clone(),
        values: AxisValues::Listed(numbers),
        cyclic: None,
        origin,
    })
}

// ---------------------------------------------------------------------------
// The built-in sources
// ---------------------------------------------------------------------------

/// A table the caller declared, one column per dimension.
struct DeclaredTable;

impl AxisSource for DeclaredTable {
    fn id(&self) -> &'static str {
        "declared-table"
    }

    fn axes(
        &self,
        subject: &AxisSubject<'_>,
        objects: &dyn ObjectSource,
        arrays: &dyn ArraySource,
    ) -> Option<Result<Vec<ComponentAxis>, AxisRefusal>> {
        let links: Vec<(&String, &String, usize)> = subject
            .links
            .iter()
            .filter_map(|link| match link {
                AxisLink::Table { table, column, dim } => Some((table, column, *dim)),
                _ => None,
            })
            .collect();
        if links.is_empty() {
            return None;
        }
        Some(
            links
                .into_iter()
                .map(|(table_uri, title, dim)| {
                    let rank = subject.shape.rank();
                    if dim >= rank {
                        return Err(AxisRefusal::NoSuchDimension { dim, rank });
                    }
                    let object = read_object(objects, table_uri)?;
                    let table = Table::read(object.as_reflect())?;
                    let column = table
                        .columns
                        .iter()
                        .find(|column| column.title.as_deref() == Some(title.as_str()))
                        .ok_or_else(|| AxisRefusal::NoSuchColumn {
                            table: table_uri.clone(),
                            column: title.clone(),
                        })?;
                    axis_of_column(
                        column,
                        arrays,
                        dim,
                        subject.shape.dims[dim],
                        AxisOrigin::DeclaredTable {
                            table: table_uri.clone(),
                            column: title.clone(),
                        },
                    )
                })
                .collect(),
        )
    }
}

/// A property of the shape of one dimension, constant along the index.
struct PairedProperty;

impl AxisSource for PairedProperty {
    fn id(&self) -> &'static str {
        "paired-property"
    }

    fn axes(
        &self,
        subject: &AxisSubject<'_>,
        objects: &dyn ObjectSource,
        arrays: &dyn ArraySource,
    ) -> Option<Result<Vec<ComponentAxis>, AxisRefusal>> {
        let links: Vec<(&String, usize)> = subject
            .links
            .iter()
            .filter_map(|link| match link {
                AxisLink::PairedProperty { property, dim } => Some((property, *dim)),
                _ => None,
            })
            .collect();
        if links.is_empty() {
            return None;
        }
        Some(
            links
                .into_iter()
                .map(|(uri, dim)| paired_axis(subject, objects, arrays, uri, dim))
                .collect(),
        )
    }
}

fn paired_axis(
    subject: &AxisSubject<'_>,
    objects: &dyn ObjectSource,
    arrays: &dyn ArraySource,
    uri: &str,
    dim: usize,
) -> Result<ComponentAxis, AxisRefusal> {
    let rank = subject.shape.rank();
    if dim >= rank {
        return Err(AxisRefusal::NoSuchDimension { dim, rank });
    }
    let held = subject.shape.dims[dim];
    let object = read_object(objects, uri)?;
    let paired = property::read(object.as_reflect())?;
    let shape = ElementShape::of_property(&paired)
        .map_err(|why| crate::Error::malformed(uri, why.to_string()))?;
    if shape.rank() != 1 || shape.dims[0] != held {
        return Err(AxisRefusal::CountMismatch {
            dim,
            axis: shape.len(),
            shape: held,
        });
    }
    if paired.patches.len() != 1 {
        return Err(AxisRefusal::Read(crate::Error::unsupported(
            format!("a paired property in {} patches", paired.patches.len()),
            uri,
        )));
    }
    let numbers = paired.patches[0]
        .read(arrays)?
        .to_f64()
        .ok_or_else(|| AxisRefusal::NotNumeric {
            column: uri.to_string(),
        })?;
    if numbers.len() % held != 0 {
        return Err(AxisRefusal::CountMismatch {
            dim,
            axis: numbers.len(),
            shape: held,
        });
    }
    let rows = numbers.len() / held;
    if let Some(expected) = subject.rows
        && expected != rows as u64
    {
        // Here "axis" counts rows: the paired property must have as many rows
        // as the subject has elements.
        return Err(AxisRefusal::CountMismatch {
            dim,
            axis: rows,
            shape: expected as usize,
        });
    }

    // The reference row: the first with a finite value. A row that is all `NaN`
    // is an absent sample and is skipped.
    let all_absent = |row: usize| numbers[row * held..(row + 1) * held].iter().all(|value| value.is_nan());
    let Some(reference) = (0..rows).find(|row| !all_absent(*row)) else {
        return Err(AxisRefusal::NotFinite { dim, at: 0 });
    };
    let axis: Vec<f64> = numbers[reference * held..(reference + 1) * held].to_vec();
    if let Some(at) = axis.iter().position(|value| !value.is_finite()) {
        return Err(AxisRefusal::NotFinite { dim, at });
    }
    for row in 0..rows {
        if all_absent(row) {
            continue;
        }
        let same = numbers[row * held..(row + 1) * held]
            .iter()
            .zip(&axis)
            .all(|(found, expected)| (found - expected).abs() <= PAIRED_AXIS_TOLERANCE * expected.abs().max(1.0));
        if !same {
            return Err(AxisRefusal::VariesAlongIndex { dim, row });
        }
    }

    let uom = paired.uom.clone();
    let cyclic = uom.as_deref().and_then(|unit| full_turn(&axis, unit));
    Ok(ComponentAxis {
        dim,
        name: paired.title.clone(),
        kind: kind_text(&paired.kind),
        uom,
        values: AxisValues::Listed(axis),
        cyclic,
        origin: AxisOrigin::PairedProperty { uri: uri.to_string() },
    })
}

/// The period, in the file's unit, of angles that are regular and make a full
/// turn (`count x step == 2 pi`); `None` otherwise.
fn full_turn(values: &[f64], unit: &str) -> Option<f64> {
    if Dimension::of(unit) != Some(Dimension::PlaneAngle) || values.len() < 2 {
        return None;
    }
    let radians: Vec<f64> = values
        .iter()
        .map(|value| units::radians(*value, unit))
        .collect::<Result<_, _>>()
        .ok()?;
    let step = radians[1] - radians[0];
    let regular = radians
        .windows(2)
        .all(|pair| ((pair[1] - pair[0]) - step).abs() <= PAIRED_AXIS_TOLERANCE * step.abs().max(1.0));
    let turn = std::f64::consts::TAU;
    if !regular || (values.len() as f64 * step - turn).abs() > PAIRED_AXIS_TOLERANCE * turn {
        return None;
    }
    Some(values.len() as f64 * (values[1] - values[0]))
}

/// A WITSML channel's `AxisDefinition`.
struct WitsmlAxisDefinition;

impl AxisSource for WitsmlAxisDefinition {
    fn id(&self) -> &'static str {
        "witsml-axis-definition"
    }

    fn axes(
        &self,
        subject: &AxisSubject<'_>,
        objects: &dyn ObjectSource,
        _arrays: &dyn ArraySource,
    ) -> Option<Result<Vec<ComponentAxis>, AxisRefusal>> {
        let channels: Vec<&String> = subject
            .links
            .iter()
            .filter_map(|link| match link {
                AxisLink::WitsmlChannel { channel } => Some(channel),
                _ => None,
            })
            .collect();
        if channels.is_empty() {
            return None;
        }
        let mut answered = false;
        let mut out = Vec::new();
        for uri in channels {
            let object = match read_object(objects, uri) {
                Ok(object) => object,
                Err(refusal) => return Some(Err(refusal)),
            };
            let definitions = crate::series::objects(object.as_reflect(), "AxisDefinition");
            match definitions.len() {
                0 => continue,
                1 => {}
                axes => return Some(Err(AxisRefusal::OrderUnstated { axes })),
            }
            answered = true;
            let definition = definitions[0];
            // One axis cannot describe a shape of several dimensions.
            let count = crate::series::number_of(definition, "AxisCount").unwrap_or(0.0).max(0.0) as usize;
            if subject.shape.rank() != 1 {
                return Some(Err(AxisRefusal::CountMismatch {
                    dim: 0,
                    axis: count,
                    shape: subject.shape.rank(),
                }));
            }
            out.push(ComponentAxis {
                dim: 0,
                name: crate::series::text(definition, "AxisName"),
                kind: crate::series::text(definition, "AxisPropertyKind"),
                uom: crate::series::text(definition, "AxisUom"),
                values: AxisValues::Regular {
                    start: crate::series::number_of(definition, "AxisStart").unwrap_or(0.0),
                    step: crate::series::number_of(definition, "AxisSpacing").unwrap_or(0.0),
                    count,
                },
                cyclic: None,
                origin: AxisOrigin::WitsmlAxisDefinition {
                    channel: uri.clone(),
                    uid: crate::series::text(definition, "uid").unwrap_or_default(),
                },
            });
        }
        answered.then_some(Ok(out))
    }
}

/// Convention B: the table the property's single `qualifier` facet names.
struct QualifierTable;

impl AxisSource for QualifierTable {
    fn id(&self) -> &'static str {
        "qualifier-table"
    }

    fn axes(
        &self,
        subject: &AxisSubject<'_>,
        objects: &dyn ObjectSource,
        arrays: &dyn ArraySource,
    ) -> Option<Result<Vec<ComponentAxis>, AxisRefusal>> {
        let uri = match qualifier_link(subject.facets) {
            Ok(None) => return None,
            Ok(Some(uri)) => uri,
            Err(refusal) => return Some(Err(AxisRefusal::Qualifier(refusal))),
        };
        Some(qualifier_axes(subject, objects, arrays, &uri))
    }
}

fn qualifier_axes(
    subject: &AxisSubject<'_>,
    objects: &dyn ObjectSource,
    arrays: &dyn ArraySource,
    uri: &str,
) -> Result<Vec<ComponentAxis>, AxisRefusal> {
    let object = read_object(objects, uri)?;
    let table = Table::read(object.as_reflect())?;
    let keys = table.keys().len();
    if keys != 1 {
        return Err(AxisRefusal::TableKeys {
            table: uri.to_string(),
            keys,
        });
    }
    let columns = table.values();
    let rank = subject.shape.rank();
    if columns.len() != rank {
        return Err(AxisRefusal::TableColumns {
            table: uri.to_string(),
            columns: columns.len(),
            rank,
        });
    }
    columns
        .into_iter()
        .enumerate()
        .map(|(dim, column)| {
            axis_of_column(
                column,
                arrays,
                dim,
                subject.shape.dims[dim],
                AxisOrigin::QualifierTable {
                    table: uri.to_string(),
                    column: column.title.clone().unwrap_or_default(),
                },
            )
        })
        .collect()
}

/// `LabelPerComponent` on a shape of one dimension.
struct LabelPerComponent;

impl AxisSource for LabelPerComponent {
    fn id(&self) -> &'static str {
        "label-per-component"
    }

    fn axes(
        &self,
        subject: &AxisSubject<'_>,
        _objects: &dyn ObjectSource,
        _arrays: &dyn ArraySource,
    ) -> Option<Result<Vec<ComponentAxis>, AxisRefusal>> {
        // Linearised labels of a shape of several dimensions do not make an axis
        // per dimension: `ElementShape::label_of` gives them cell by cell.
        if subject.shape.rank() != 1 {
            return None;
        }
        let labels = subject.shape.labels.as_ref()?;
        Some(Ok(vec![ComponentAxis {
            dim: 0,
            name: None,
            kind: None,
            uom: None,
            values: AxisValues::Labels(labels.clone()),
            cyclic: None,
            origin: AxisOrigin::LabelPerComponent,
        }]))
    }
}

// ---------------------------------------------------------------------------
// Offering tables
// ---------------------------------------------------------------------------

/// A table offered as the axes of a property: never applied by this module.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TableCandidate {
    /// The table's uri.
    pub table: String,
    /// Its title.
    pub title: Option<String>,
    /// Why it is offered: the `dataset:` tag both titles carry.
    pub tag: String,
    /// Rows of the table.
    pub rows: u64,
    /// Numeric value columns whose row count is that of a dimension:
    /// `(dim, column title)`.
    pub fits: Vec<(usize, String)>,
}

/// The `dataset:` tag of a title: the first word that starts so, cut at a blank
/// or a parenthesis.
fn dataset_tag(title: &str) -> Option<&str> {
    title
        .split(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .find(|word| word.starts_with("dataset:") && word.len() > "dataset:".len())
}

/// Tables that may hold the axes of a property: offered, **never applied**.
///
/// A proposal is not a check: a table that cannot be read is skipped, and a
/// property whose title carries no `dataset:` tag has no candidate. The caller
/// chooses one and declares it ([`AxisLink::Table`]).
///
/// `TODO(verify)`: that the title of the property carries the `dataset:` tag
/// (the title of the table is the one seen in the mock-up).
pub fn candidate_tables(
    index: &PackageIndex,
    property: &str,
    shape: &ElementShape,
    objects: &dyn ObjectSource,
    arrays: &dyn ArraySource,
) -> Vec<TableCandidate> {
    let Some(sheet) = index.sheet(property) else {
        return Vec::new();
    };
    let Some(title) = sheet.title.as_deref() else {
        return Vec::new();
    };
    let Some(tag) = dataset_tag(title) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for node in index.nodes() {
        if &*node.type_key != "ColumnBasedTable" || node.state != NodeState::Present {
            continue;
        }
        if !node.title.as_deref().is_some_and(|name| name.contains(tag)) {
            continue;
        }
        let Ok(object) = objects.read_object_at(&node.uri) else {
            continue;
        };
        let Ok(table) = Table::read(object.as_reflect()) else {
            continue;
        };
        let mut rows = None;
        let mut fits = Vec::new();
        for column in table.columns.iter() {
            // A 2.0.1 column with inline pairs has no array: never an axis table.
            let Some(array) = column.array else {
                continue;
            };
            let at = column.title.as_deref().unwrap_or("column");
            let Ok(count) = values::count(array, arrays, at) else {
                continue;
            };
            if rows.is_none() {
                rows = Some(count as u64);
            }
            if column.key || !matches!(values::family(array), Some(Family::Floating | Family::Integer)) {
                continue;
            }
            for (dim, held) in shape.dims.iter().enumerate() {
                if *held == count {
                    fits.push((dim, column.title.clone().unwrap_or_default()));
                }
            }
        }
        if fits.is_empty() {
            continue;
        }
        out.push(TableCandidate {
            table: node.uri.to_string(),
            title: table.title.clone(),
            tag: tag.to_string(),
            rows: rows.unwrap_or(0),
            fits,
        });
    }
    out.sort_by(|a, b| a.table.cmp(&b.table));
    out
}
