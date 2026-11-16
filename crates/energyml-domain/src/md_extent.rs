//! From which MD to which: the extent of anything that carries one, and the coverage of a
//! wellbore.
//!
//! An **extent** is an interval on an axis, with how its depths are counted when the file
//! says so. [`of_object`] reads the extent of one object (a trajectory, a frame, a WITSML
//! log, a geology interval) through a registry of readers; [`of_rows`] reads the extent of
//! each row of a table from its MD columns; [`coverage`] lays extents side by side.
//!
//! # What may be compared
//!
//! Two MDs are on one axis only when [`strata_by_md::comparable`] says so (the same
//! trajectory, or the same datum, domain and a unit that converts: common TRG 2.3 §6.48,
//! p. 134). That rule is not rewritten here. A coverage therefore has **one group per
//! comparable reference**, plus one band for the items whose reference is not stated (a
//! table column), and the items that cannot be compared are said, never merged.
//!
//! **An alignment is the person's assumption**, a parameter of the view (07 §4.5): "count
//! this item's MDs as that one's are counted". It is returned in [`Coverage::applied`] so
//! that the picture and the export can write it; it is never stored in the package.

use energyml_core::identity;
use energyml_core::reflect::fields;
use energyml_core::value::{FieldValue, Reflect};

use crate::source::ArraySource;
use crate::strata_by_md::{self, MdReference};
use crate::table::{
    Cell, CellState, ColumnCells, IntervalColumns, LinkDeclaration, LinkTarget, Resolved, TableRefusal,
    TargetIndexes, resolve,
};
use crate::units;

/// The MD bounds - 2.2, then 2.0.1 (the spellings `strata_by_md` reads).
const MD_MIN: &[&str] = &["MdMin", "StartMd"];
const MD_MAX: &[&str] = &["MdMax", "FinishMd"];
const MD_INTERVAL: &str = "MdInterval";
const PRIMARY_INDEX_INTERVAL: &str = "PrimaryIndexInterval";

/// Which axis an extent is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtentAxis {
    /// Measured depth.
    Md,
    /// True vertical depth.
    Tvd,
    /// Time.
    Time,
}

/// Where an extent comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtentBasis {
    /// An interval written in the XML.
    Declared,
    /// The minimum and maximum of values read (a frame's `NodeMd`).
    Measured,
    /// A row without MD took the extent of the row its link column names.
    ThroughLink {
        /// The link column.
        column: usize,
    },
}

/// An interval of depth.
#[derive(Clone, Debug, PartialEq)]
pub struct Extent {
    /// As declared: `from > to` is kept and said ([`Extent::is_reversed`]), never swapped.
    pub from: f64,
    /// The other end.
    pub to: f64,
    /// The unit, as the file spells it.
    pub uom: Option<String>,
    /// How these MDs are counted. `None`: nothing says (a table column).
    pub reference: Option<MdReference>,
    /// The axis.
    pub axis: ExtentAxis,
    /// Where it comes from.
    pub basis: ExtentBasis,
    /// The id of the reader that gave it.
    pub reader: &'static str,
}

impl Extent {
    /// Whether the interval is written the wrong way round (`from > to`). Kept as written.
    pub fn is_reversed(&self) -> bool {
        self.from > self.to
    }
}

/// Why an object gives no extent.
#[derive(Debug, thiserror::Error)]
pub enum ExtentRefusal {
    /// No reader finds an interval or readable MDs in the object.
    #[error("`{qualified_type}` states no interval and no readable MD")]
    NoExtent {
        /// The object's qualified type.
        qualified_type: String,
    },
    /// The object states an interval, on another axis.
    #[error("the interval is on the {axis:?} axis, not measured depth")]
    NotDepth {
        /// The axis it is on.
        axis: ExtentAxis,
    },
    /// How its MDs are counted could not be read.
    #[error("{0}")]
    Reference(#[from] crate::well_strata::Refusal),
    /// An interval shape this build does not read.
    #[error("the `{shape}` interval is not read by this build")]
    Unsupported {
        /// The shape's name.
        shape: String,
    },
}

/// One way an object states its extent.
pub trait ExtentReader: Send + Sync {
    /// A stable id (`declared-md-interval`, ...), or the caller's.
    fn id(&self) -> &'static str;
    /// `None`: not this reader's shape.
    fn read(&self, object: &dyn Reflect, source: &dyn ArraySource) -> Option<Result<Extent, ExtentRefusal>>;
}

/// The readers, the caller's first (ADR-0006).
pub struct ExtentReaders {
    entries: Vec<Box<dyn ExtentReader>>,
}

impl Default for ExtentReaders {
    fn default() -> Self {
        Self::builtin()
    }
}

impl ExtentReaders {
    /// No reader at all: every object fails saying so.
    pub fn none() -> Self {
        Self { entries: Vec::new() }
    }

    /// `resqml-md-reference`, `declared-md-interval`, `primary-index-interval`, `node-md`.
    pub fn builtin() -> Self {
        Self {
            entries: vec![
                Box::new(ResqmlMdReference),
                Box::new(DeclaredMdInterval),
                Box::new(PrimaryIndexIntervalReader),
                Box::new(NodeMd),
            ],
        }
    }

    /// Add one, tried **before** everything already there (last pushed wins).
    pub fn push(&mut self, reader: Box<dyn ExtentReader>) -> &mut Self {
        self.entries.insert(0, reader);
        self
    }

    /// The ids, in the order tried.
    pub fn ids(&self) -> Vec<&'static str> {
        self.entries.iter().map(|reader| reader.id()).collect()
    }
}

/// E1: the extent of one object, by the first reader that answers.
pub fn of_object(
    object: &dyn Reflect,
    source: &dyn ArraySource,
    readers: &ExtentReaders,
) -> Result<Extent, ExtentRefusal> {
    for reader in &readers.entries {
        if let Some(answer) = reader.read(object, source) {
            return answer;
        }
    }
    Err(no_extent(object))
}

fn no_extent(object: &dyn Reflect) -> ExtentRefusal {
    let info = object.type_info();
    ExtentRefusal::NoExtent {
        qualified_type: info.qualified_type.unwrap_or(info.type_key).to_string(),
    }
}

// ---------------------------------------------------------------------------
// Reading what an object states
// ---------------------------------------------------------------------------

fn number_of(object: &dyn Reflect, names: &[&str]) -> Option<f64> {
    names.iter().find_map(|name| fields::number(object, name))
}

fn string_of(object: &dyn Reflect, name: &str) -> Option<String> {
    fields::text(object, name)
}

fn uuid_of(object: &dyn Reflect) -> Option<String> {
    identity::non_empty_uuid_of_reflect(object).map(|uuid| uuid.to_ascii_lowercase())
}

/// The uuid of the object a field of `object` references, lowercased.
fn reference_uuid(object: &dyn Reflect, name: &str) -> Option<String> {
    let FieldValue::Object(reference) = object.field(name)? else {
        return None;
    };
    uuid_of(reference)
}

/// The interval an object states by an `MdInterval` (EML 2.3 `MdInterval`: `MdMin`, `MdMax`,
/// `Uom`, `Datum`), with the reference it counts them by.
///
/// **The `trajectory` of the reference is the uuid of the object itself**: no trajectory
/// counts these MDs, so they are the object's own, and two objects that state no trajectory
/// are never the same trajectory (`comparable` would say so for two empty strings). An
/// object without a uuid has no reference at all.
fn declared_interval(
    owner: &dyn Reflect,
    interval: &dyn Reflect,
    reader: &'static str,
) -> Result<Extent, ExtentRefusal> {
    let (Some(from), Some(to)) = (number_of(interval, MD_MIN), number_of(interval, MD_MAX)) else {
        return Err(no_extent(owner));
    };
    let uom = string_of(interval, "Uom");
    let reference = uuid_of(owner).map(|trajectory| MdReference {
        trajectory,
        datum: reference_uuid(interval, "Datum"),
        uom: uom.clone(),
        domain: None,
    });
    Ok(Extent {
        from,
        to,
        uom,
        reference,
        axis: ExtentAxis::Md,
        basis: ExtentBasis::Declared,
        reader,
    })
}

/// The interval a trajectory states, and how its MDs are counted.
fn trajectory_extent(object: &dyn Reflect, source: &dyn ArraySource) -> Result<Extent, ExtentRefusal> {
    let reference = strata_by_md::md_reference(object, source)?;
    let interval = match object.field(MD_INTERVAL) {
        Some(FieldValue::Object(interval)) => interval,
        _ => object,
    };
    let (Some(from), Some(to)) = (number_of(interval, MD_MIN), number_of(interval, MD_MAX)) else {
        return Err(no_extent(object));
    };
    Ok(Extent {
        from,
        to,
        uom: reference.uom.clone(),
        reference: Some(reference),
        axis: ExtentAxis::Md,
        basis: ExtentBasis::Declared,
        reader: "resqml-md-reference",
    })
}

/// The least and the greatest finite `NodeMd` of a frame.
fn frame_extent(object: &dyn Reflect, source: &dyn ArraySource) -> Result<Extent, ExtentRefusal> {
    let mds = strata_by_md::node_mds(object, source)?;
    let finite: Vec<f64> = mds.into_iter().filter(|md| md.is_finite()).collect();
    if finite.is_empty() {
        return Err(no_extent(object));
    }
    let from = finite.iter().copied().fold(f64::INFINITY, f64::min);
    let to = finite.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let reference = strata_by_md::md_reference(object, source)?;
    Ok(Extent {
        from,
        to,
        uom: reference.uom.clone(),
        reference: Some(reference),
        axis: ExtentAxis::Md,
        basis: ExtentBasis::Measured,
        reader: "node-md",
    })
}

/// A trajectory: the interval it states and how its MDs are counted (`strata_by_md`).
struct ResqmlMdReference;

impl ExtentReader for ResqmlMdReference {
    fn id(&self) -> &'static str {
        "resqml-md-reference"
    }

    fn read(&self, object: &dyn Reflect, source: &dyn ArraySource) -> Option<Result<Extent, ExtentRefusal>> {
        if object.type_info().type_key != "WellboreTrajectoryRepresentation" {
            return None;
        }
        Some(trajectory_extent(object, source))
    }
}

/// Any other object with an `MdInterval` field: WITSML 2.1 `Trajectory`, geology intervals.
struct DeclaredMdInterval;

impl ExtentReader for DeclaredMdInterval {
    fn id(&self) -> &'static str {
        "declared-md-interval"
    }

    fn read(&self, object: &dyn Reflect, _source: &dyn ArraySource) -> Option<Result<Extent, ExtentRefusal>> {
        let FieldValue::Object(interval) = object.field(MD_INTERVAL)? else {
            return None;
        };
        Some(declared_interval(object, interval, "declared-md-interval"))
    }
}

/// WITSML 2.1 `Log`, `ChannelSet` and `Channel`: the interval of their primary index.
struct PrimaryIndexIntervalReader;

impl ExtentReader for PrimaryIndexIntervalReader {
    fn id(&self) -> &'static str {
        "primary-index-interval"
    }

    fn read(&self, object: &dyn Reflect, _source: &dyn ArraySource) -> Option<Result<Extent, ExtentRefusal>> {
        if !matches!(object.type_info().type_key, "Log" | "ChannelSet" | "Channel") {
            return None;
        }
        // Reflection is transparent over a choice: the object is the concrete interval.
        let FieldValue::Object(interval) = object.field(PRIMARY_INDEX_INTERVAL)? else {
            return None;
        };
        let shape = interval.type_info().name;
        Some(match shape {
            "MdInterval" => declared_interval(object, interval, "primary-index-interval"),
            "DateTimeInterval" | "ElapsedTimeInterval" => Err(ExtentRefusal::NotDepth {
                axis: ExtentAxis::Time,
            }),
            "DatumTvdInterval" | "ReferencePointTvdInterval" => Err(ExtentRefusal::NotDepth {
                axis: ExtentAxis::Tvd,
            }),
            other => Err(ExtentRefusal::Unsupported {
                shape: other.to_string(),
            }),
        })
    }
}

/// A wellbore frame: the least and the greatest of its `NodeMd`.
///
/// `node_mds` reads only `WellboreFrameRepresentation` and `WellboreTrajectoryRepresentation`,
/// so this reader takes that one frame; the other frames that carry a `NodeMd` give no extent
/// until a reader is written and tested for each.
struct NodeMd;

impl ExtentReader for NodeMd {
    fn id(&self) -> &'static str {
        "node-md"
    }

    fn read(&self, object: &dyn Reflect, source: &dyn ArraySource) -> Option<Result<Extent, ExtentRefusal>> {
        if object.type_info().type_key != "WellboreFrameRepresentation" {
            return None;
        }
        Some(frame_extent(object, source))
    }
}

// ---------------------------------------------------------------------------
// The rows of a table
// ---------------------------------------------------------------------------

/// A row's extent.
#[derive(Clone, Debug, PartialEq)]
pub struct RowExtent {
    /// The row.
    pub row: usize,
    /// Its extent, when its MD columns (or its link) give one.
    pub extent: Option<Extent>,
    /// One end of a pair is missing: no extent, and this says why.
    pub incomplete: bool,
}

/// How a row without MD inherits one (depth 1, 07 §4.4).
pub struct Inherit<'a> {
    /// The link column that names the row to inherit from.
    pub link: &'a LinkDeclaration,
    /// The uri of the table the rows belong to.
    pub own_uri: &'a str,
    /// Where the targets of links are.
    pub targets: &'a dyn TargetIndexes,
    /// The extent of a row of a target table, as the caller computed it with [`of_rows`].
    pub extent_of: &'a dyn Fn(&str, usize) -> Option<Extent>,
}

/// The number at `row` of a column, when it is a value that is a number.
fn number_at(cells: &[Result<ColumnCells<'_>, TableRefusal>], column: usize, row: usize) -> Option<f64> {
    match cells.get(column)?.as_ref().ok()?.cell(row)? {
        CellState::Value(Cell::Number(x)) => Some(x),
        _ => None,
    }
}

/// E3: each row's extent from its MD columns, else through a link.
///
/// A row of one MD column is the interval `md..md`; two MD columns are `top..base` in column
/// order. A missing value (`Absent`, `Empty`, not a number) in both makes the row inherit
/// when `inherit` is given: the cell of its link column is resolved, and the extent of the
/// row it names is taken with [`ExtentBasis::ThroughLink`]. Only one of the two missing is
/// an `incomplete` row with no extent. No MD column (or three, which choose nothing): every
/// row has no extent.
///
/// The extent has no reference (`None`): a table column says nothing of how its MDs are
/// counted. `uom` is the unit of the column.
pub fn of_rows(
    cells: &[Result<ColumnCells<'_>, TableRefusal>],
    columns: &IntervalColumns,
    uom: Option<&str>,
    inherit: Option<&Inherit<'_>>,
) -> Vec<RowExtent> {
    let rows = cells
        .iter()
        .filter_map(|column| column.as_ref().ok()?.rows("").ok())
        .min()
        .unwrap_or(0);
    let own = |from: f64, to: f64| Extent {
        from,
        to,
        uom: uom.map(str::to_string),
        reference: None,
        axis: ExtentAxis::Md,
        basis: ExtentBasis::Declared,
        reader: "table-rows",
    };
    let through = |row: usize| -> Option<Extent> {
        let inherit = inherit?;
        let link = inherit.link;
        let state = cells.get(link.column)?.as_ref().ok()?.cell(row)?;
        let sibling = match link.target {
            LinkTarget::ByTitleIn { column } => cells.get(column)?.as_ref().ok()?.cell(row),
            _ => None,
        };
        match resolve(link, &state, sibling.as_ref(), inherit.own_uri, inherit.targets) {
            Resolved::Row {
                table_uri,
                row: target,
            } => {
                let mut extent = (inherit.extent_of)(&table_uri, target)?;
                extent.basis = ExtentBasis::ThroughLink { column: link.column };
                Some(extent)
            }
            _ => None,
        }
    };

    (0..rows)
        .map(|row| match columns {
            IntervalColumns::None | IntervalColumns::Ambiguous(_) => RowExtent {
                row,
                extent: None,
                incomplete: false,
            },
            IntervalColumns::Point { md } => match number_at(cells, *md, row) {
                Some(md) => RowExtent {
                    row,
                    extent: Some(own(md, md)),
                    incomplete: false,
                },
                None => RowExtent {
                    row,
                    extent: through(row),
                    incomplete: false,
                },
            },
            IntervalColumns::Pair { top, base } => {
                match (number_at(cells, *top, row), number_at(cells, *base, row)) {
                    (Some(top), Some(base)) => RowExtent {
                        row,
                        extent: Some(own(top, base)),
                        incomplete: false,
                    },
                    (None, None) => RowExtent {
                        row,
                        extent: through(row),
                        incomplete: false,
                    },
                    _ => RowExtent {
                        row,
                        extent: None,
                        incomplete: true,
                    },
                }
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Coverage
// ---------------------------------------------------------------------------

/// Something drawn on the coverage: a key the caller chose (an object uri, `uri#row`) and
/// its extent.
#[derive(Clone, Debug, PartialEq)]
pub struct CoverageItem {
    /// The caller's key.
    pub key: String,
    /// The extent.
    pub extent: Extent,
}

/// **The person's** assumption: count `item`'s MDs as `onto`'s are counted. A parameter of
/// the view, written on the picture and in the export, never in the package (07 §4.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alignment {
    /// The key of the item whose MDs are re-read.
    pub item: String,
    /// The key of the item whose way of counting it takes.
    pub onto: String,
}

/// One bar of the coverage.
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    /// The item's key.
    pub key: String,
    /// In the group's unit.
    pub from: f64,
    /// In the group's unit.
    pub to: f64,
    /// Where the extent comes from.
    pub basis: ExtentBasis,
    /// Placed by an [`Alignment`].
    pub aligned: bool,
}

/// The items that share one axis.
#[derive(Clone, Debug, PartialEq)]
pub struct CoverageGroup {
    /// The reference of the first item of the group; `None` for the band of items whose
    /// reference is unknown.
    pub reference: Option<MdReference>,
    /// The unit of the bars.
    pub uom: Option<String>,
    /// The bars, in the order of the items.
    pub bars: Vec<Bar>,
}

/// Something the coverage says about an item.
#[derive(Clone, Debug, PartialEq)]
pub enum CoverageNote {
    /// Not in the same group, with `comparable`'s reason.
    Incomparable {
        /// The item.
        item: String,
        /// The first item of the group it was compared with.
        first_of_group: String,
        /// The reason, in a sentence.
        why: String,
    },
    /// An alignment naming an item that is not there.
    UnknownAlignment {
        /// The alignment, ignored.
        alignment: Alignment,
    },
    /// An interval written the wrong way round, drawn as written.
    Reversed {
        /// The item.
        item: String,
    },
    /// Off the MD axis (time, TVD), counted apart.
    OffAxis {
        /// The item.
        item: String,
        /// Its axis.
        axis: ExtentAxis,
    },
}

/// The coverage: groups of bars, and what was said on the way.
#[derive(Clone, Debug, PartialEq)]
pub struct Coverage {
    /// One group per comparable reference, and the band of unstated references.
    pub groups: Vec<CoverageGroup>,
    /// What was noticed.
    pub notes: Vec<CoverageNote>,
    /// The alignments that were applied, as given.
    pub applied: Vec<Alignment>,
}

/// E4: the items laid on axes.
///
/// For each item, in order:
///
/// 1. an extent off the MD axis is noted (`OffAxis`) and has no bar;
/// 2. a reversed extent is noted (`Reversed`) and drawn as written;
/// 3. an [`Alignment`] naming this item and an item that is present gives it the other's
///    reference (`aligned`); the other absent is noted (`UnknownAlignment`) and ignored.
///    The values are first brought into the other's unit when the two units are known
///    and differ: the alignment says "same counting", not "same unit";
/// 4. no reference: the band of unstated references, in the unit of its first item; a
///    value of another unit is converted when both units are known, and noted
///    (`Incomparable`) when one is unknown and the other not;
/// 5. otherwise the first group whose reference [`strata_by_md::comparable`] accepts takes
///    the bar, scaled by its factor; none does: a new group, and an `Incomparable` note
///    with the reason against the first group that has a reference.
pub fn coverage(items: Vec<CoverageItem>, alignments: &[Alignment]) -> Coverage {
    let mut groups: Vec<CoverageGroup> = Vec::new();
    let mut notes: Vec<CoverageNote> = Vec::new();
    let mut applied: Vec<Alignment> = Vec::new();
    let mut band: Option<usize> = None;

    for alignment in alignments {
        if !items.iter().any(|item| item.key == alignment.onto) {
            notes.push(CoverageNote::UnknownAlignment {
                alignment: alignment.clone(),
            });
        }
    }

    for item in &items {
        let extent = &item.extent;
        if extent.axis != ExtentAxis::Md {
            notes.push(CoverageNote::OffAxis {
                item: item.key.clone(),
                axis: extent.axis,
            });
            continue;
        }
        if extent.is_reversed() {
            notes.push(CoverageNote::Reversed {
                item: item.key.clone(),
            });
        }

        let (mut from, mut to) = (extent.from, extent.to);
        let mut reference = extent.reference.clone();
        let mut aligned = false;
        let alignment = alignments
            .iter()
            .find(|alignment| alignment.item == item.key)
            .and_then(|alignment| {
                items
                    .iter()
                    .find(|other| other.key == alignment.onto)
                    .map(|onto| (alignment, onto))
            });
        if let Some((alignment, onto)) = alignment {
            // The unit of the item is its own: only the counting is taken from `onto`.
            if let (Some(own), Some(theirs)) = (
                &extent.uom,
                onto.extent
                    .reference
                    .as_ref()
                    .and_then(|reference| reference.uom.as_ref()),
            ) && own != theirs
            {
                match (units::convert(from, own, theirs), units::convert(to, own, theirs)) {
                    (Ok(a), Ok(b)) => {
                        from = a;
                        to = b;
                    }
                    _ => {
                        notes.push(CoverageNote::Incomparable {
                            item: item.key.clone(),
                            first_of_group: onto.key.clone(),
                            why: format!(
                                "its MDs are in `{own}` and the item it is aligned onto counts in `{theirs}`, and this build cannot convert one into the other"
                            ),
                        });
                        continue;
                    }
                }
            }
            reference = onto.extent.reference.clone();
            aligned = true;
            applied.push(alignment.clone());
        }

        let bar = |from: f64, to: f64| Bar {
            key: item.key.clone(),
            from,
            to,
            basis: extent.basis,
            aligned,
        };

        match reference {
            None => {
                let at = *band.get_or_insert_with(|| {
                    groups.push(CoverageGroup {
                        reference: None,
                        uom: extent.uom.clone(),
                        bars: Vec::new(),
                    });
                    groups.len() - 1
                });
                let group_uom = groups[at].uom.clone();
                let first = groups[at].bars.first().map(|bar| bar.key.clone());
                let (from, to) = match (&extent.uom, &group_uom) {
                    (Some(own), Some(theirs)) if own != theirs => {
                        match (units::convert(from, own, theirs), units::convert(to, own, theirs)) {
                            (Ok(a), Ok(b)) => (a, b),
                            _ => {
                                notes.push(CoverageNote::Incomparable {
                                    item: item.key.clone(),
                                    first_of_group: first.unwrap_or_default(),
                                    why: format!(
                                        "its MDs are in `{own}` and the band's in `{theirs}`, and this build cannot convert one into the other"
                                    ),
                                });
                                (from, to)
                            }
                        }
                    }
                    (Some(_), None) | (None, Some(_)) => {
                        notes.push(CoverageNote::Incomparable {
                            item: item.key.clone(),
                            first_of_group: first.unwrap_or_default(),
                            why:
                                "the unit of one of the two is not stated, so their MDs are kept as written \
                                  and not compared"
                                    .to_string(),
                        });
                        (from, to)
                    }
                    _ => (from, to),
                };
                groups[at].bars.push(bar(from, to));
            }
            Some(effective) => {
                let mut fit = None;
                for (at, group) in groups.iter().enumerate() {
                    let Some(stated) = &group.reference else { continue };
                    if let Ok(factor) = strata_by_md::comparable(&effective, stated) {
                        fit = Some((at, factor));
                        break;
                    }
                }
                match fit {
                    Some((at, factor)) => groups[at].bars.push(bar(from * factor, to * factor)),
                    None => {
                        // The reason is the first group's: what the person is shown.
                        let against = groups.iter().find_map(|group| {
                            let stated = group.reference.as_ref()?;
                            let first = group.bars.first()?.key.clone();
                            Some((stated, first))
                        });
                        if let Some((stated, first)) = against
                            && let Err(why) = strata_by_md::comparable(&effective, stated)
                        {
                            notes.push(CoverageNote::Incomparable {
                                item: item.key.clone(),
                                first_of_group: first,
                                why,
                            });
                        }
                        groups.push(CoverageGroup {
                            uom: effective.uom.clone().or_else(|| extent.uom.clone()),
                            reference: Some(effective),
                            bars: vec![bar(from, to)],
                        });
                    }
                }
            }
        }
    }

    Coverage {
        groups,
        notes,
        applied,
    }
}
