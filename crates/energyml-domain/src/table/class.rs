//! What a column holds, as a reader shows it: the class of a column, the
//! measured-depth columns of a table, and the pairs of columns a curve can be
//! drawn with.
//!
//! A column is classed by the **family of its array**, then by the **ancestry
//! of its property kind** (`Resolution.chain`, which covers the kinds the
//! package defines as well as the dictionary's), and never by its title: a
//! column titled `depth` whose kind is `length` is a length, and a column titled
//! `foo` whose kind is `measured depth` is a depth.

use crate::package_index::SheetKind;
use crate::property::PropertyKindRef;
use crate::property_kind::{Resolution, Resolver};
use crate::units;
use crate::values::Family;

use super::links::uri_column;
use super::{ColumnCells, ColumnSheet, TableSheet};

/// `identifier`: a value that names something (property kind dictionary, TSV line 967).
pub const IDENTIFIER: &str = "311dcaba-6512-4144-aa7c-79a9de19b4ab";
/// `code`: the short word a category is written with (TSV line 175). What
/// `frame_values` takes for the label column of a category table.
pub const CODE: &str = "5f78f66a-ed1b-4827-a868-beb989febb31";
/// `explicit index`: a pointer into a structure, a rank (TSV line 1762).
pub const EXPLICIT_INDEX: &str = "36f718c4-371e-4d5b-a8cb-3bc3a3a84ea0";
/// `measured depth` (TSV line 1286).
pub const MEASURED_DEPTH: &str = "c48c65d4-9680-4d10-903c-a1b2e30f66b5";
/// `true vertical depth` (TSV line 1316).
pub const TRUE_VERTICAL_DEPTH: &str = "c17d98b8-17f5-4a1b-9a1c-e95e06ea5628";
/// `description` (TSV line 606).
pub const DESCRIPTION: &str = "89230474-eb6e-417f-8fce-f2f00ee6fafb";

/// Which depth a depth column holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepthKind {
    /// Measured depth, along the well.
    Md,
    /// True vertical depth.
    Tvd,
}

/// What a column holds, as a reader shows it.
#[derive(Clone, Debug, PartialEq)]
pub enum ColumnClass {
    /// Free text.
    Text,
    /// Every non-empty value is a complete ETP uri of an object (convention E).
    Uri,
    /// Whole numbers (codes, ranks, counts).
    Integer,
    /// Real numbers, with the unit the column states.
    Real {
        /// The column's own `Uom`.
        uom: Option<String>,
    },
    /// A depth: the kind is `measured depth` or `true vertical depth`, or a descendant.
    Depth {
        /// The column's own `Uom`.
        uom: Option<String>,
        /// Which depth.
        kind: DepthKind,
    },
    /// Truth values.
    Boolean,
    /// `ValueCountPerRow = n > 1`: `n` values of class `of` per row.
    Vector {
        /// The class of one component.
        of: Box<ColumnClass>,
        /// Values per row.
        n: u64,
    },
    /// The family is not known without reading (no array type, no inline values).
    Unknown,
}

/// The MD columns of a table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntervalColumns {
    /// No measured-depth column.
    None,
    /// One MD column: a table of points.
    Point {
        /// The column.
        md: usize,
    },
    /// Two MD columns: `top` is the first in column order, `base` the second
    /// (the order, never the title).
    Pair {
        /// The first MD column.
        top: usize,
        /// The second MD column.
        base: usize,
    },
    /// Three or more: nothing is chosen.
    Ambiguous(Vec<usize>),
}

/// Whether a resolved kind is `uuid` or one of its descendants (`Resolution.chain`, kind first).
pub fn descends_from(kind: &Resolution, uuid: &str) -> bool {
    kind.chain.iter().any(|k| k.uuid.eq_ignore_ascii_case(uuid))
}

/// Each column's kind resolved through the package then the dictionary; `None` where the column
/// names no kind or the kind does not resolve.
pub fn resolve_kinds(sheet: &TableSheet, resolver: &Resolver<'_>) -> Vec<Option<Resolution>> {
    sheet
        .columns
        .iter()
        .map(|column| {
            let reference = match column.kind.as_ref()? {
                SheetKind::Object { uuid, title } => PropertyKindRef::Object {
                    uuid: uuid.to_string(),
                    title: title.as_ref().map(|t| t.to_string()),
                },
                SheetKind::Standard(name) => PropertyKindRef::Standard {
                    name: name.to_string(),
                },
            };
            resolver.resolve(&reference).ok()
        })
        .collect()
}

/// The class of one column: the family of its array (or of the cells when the
/// sheet cannot say), then its kind. A column with `ValueCountPerRow > 1` is a
/// `Vector` of the class of one component.
pub fn classify_column(
    column: &ColumnSheet,
    kind: Option<&Resolution>,
    cells: Option<&ColumnCells<'_>>,
) -> ColumnClass {
    let scalar = scalar_class(column, kind, cells);
    if column.values_per_row > 1 {
        ColumnClass::Vector {
            of: Box::new(scalar),
            n: column.values_per_row,
        }
    } else {
        scalar
    }
}

/// The class of one component, `values_per_row` ignored.
fn scalar_class(
    column: &ColumnSheet,
    kind: Option<&Resolution>,
    cells: Option<&ColumnCells<'_>>,
) -> ColumnClass {
    let family = column.family.or_else(|| cells.map(|c| c.family()));
    let Some(family) = family else {
        return ColumnClass::Unknown;
    };
    match family {
        Family::Text => {
            if cells.and_then(uri_column).is_some() {
                ColumnClass::Uri
            } else {
                ColumnClass::Text
            }
        }
        Family::Boolean => ColumnClass::Boolean,
        Family::Integer | Family::Floating => {
            let uom = column.uom.as_ref().map(|u| u.to_string());
            if let Some(kind) = kind {
                if descends_from(kind, MEASURED_DEPTH) {
                    return ColumnClass::Depth {
                        uom,
                        kind: DepthKind::Md,
                    };
                }
                if descends_from(kind, TRUE_VERTICAL_DEPTH) {
                    return ColumnClass::Depth {
                        uom,
                        kind: DepthKind::Tvd,
                    };
                }
            }
            if family == Family::Integer {
                ColumnClass::Integer
            } else {
                ColumnClass::Real { uom }
            }
        }
    }
}

/// The MD columns: kind `measured depth` or a descendant, scalar, numeric, and
/// a unit that is a length (`units::metres(1.0, uom).is_ok()`) or no unit stated.
///
/// A column of an MD kind whose unit is not a length is not an MD column: the
/// kind and the unit contradict each other, and nothing is guessed.
pub fn md_columns(sheet: &TableSheet, kinds: &[Option<Resolution>]) -> IntervalColumns {
    let found: Vec<usize> = sheet
        .columns
        .iter()
        .enumerate()
        .filter(|(at, column)| {
            column.values_per_row == 1
                && matches!(column.family, Some(Family::Integer | Family::Floating))
                && kinds
                    .get(*at)
                    .and_then(|k| k.as_ref())
                    .is_some_and(|k| descends_from(k, MEASURED_DEPTH))
                && column
                    .uom
                    .as_ref()
                    .is_none_or(|uom| units::metres(1.0, uom).is_ok())
        })
        .map(|(at, _)| at)
        .collect();
    match found.as_slice() {
        [] => IntervalColumns::None,
        [md] => IntervalColumns::Point { md: *md },
        [top, base] => IntervalColumns::Pair {
            top: *top,
            base: *base,
        },
        _ => IntervalColumns::Ambiguous(found),
    }
}

/// The pairs of columns a curve can be drawn with: `(x, y)`.
///
/// The numeric scalar columns (`Real`, `Depth`) only: an `Integer` column is a
/// code, not a curve. `x` is the one numeric key column when there is exactly
/// one, else the first numeric column; one pair for every other numeric column,
/// in column order. Fewer than two numeric columns: no pair.
pub fn curve_pairs(sheet: &TableSheet, classes: &[ColumnClass]) -> Vec<(usize, usize)> {
    let numeric: Vec<usize> = classes
        .iter()
        .enumerate()
        .filter(|(_, class)| matches!(class, ColumnClass::Real { .. } | ColumnClass::Depth { .. }))
        .map(|(at, _)| at)
        .collect();
    if numeric.len() < 2 {
        return Vec::new();
    }
    let numeric_keys: Vec<usize> = numeric
        .iter()
        .copied()
        .filter(|at| sheet.columns.get(*at).is_some_and(|c| c.key))
        .collect();
    let x = if numeric_keys.len() == 1 {
        numeric_keys[0]
    } else {
        numeric[0]
    };
    numeric
        .into_iter()
        .filter(|at| *at != x)
        .map(|y| (x, y))
        .collect()
}
