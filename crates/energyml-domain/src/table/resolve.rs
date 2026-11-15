//! Following a link cell: the row a value names in its target table, or why it
//! names none.
//!
//! The row is always found by [`row_named_by`] (**the** rule: by the table's one
//! key column, else by rank from 0), whatever the column's kind says: the target
//! decides, and a disagreement between kind and target is counted by
//! [`super::check_links`], not refused here.

use energyml_core::identity;

use crate::source::ArraySource;

use super::links::same_object;
use super::{Cell, CellState, KeyIndex, LinkDeclaration, LinkRefusal, LinkTarget, Lookup, RowMatch, row_named_by};

/// A table a link can land in: its rows and its key index.
pub struct TargetTable {
    /// The table's uri.
    pub uri: String,
    /// `Citation.Title`.
    pub title: Option<String>,
    /// Titles of the key columns, in order (for convention A's `KeyColumn`).
    pub key_titles: Vec<Option<String>>,
    /// The table, read.
    pub lookup: Lookup,
    /// Its key columns, hashed.
    pub index: KeyIndex,
}

/// Where the targets of the links come from.
pub trait TargetIndexes {
    /// The table this uri names (compared by uuid and version, `identity::parse_address`).
    fn target(&self, table_uri: &str) -> Option<&TargetTable>;
    /// The uris of the tables carrying exactly this title (convention D).
    fn titled(&self, title: &str) -> Vec<&str>;
}

/// The tables a caller has read, by uri.
#[derive(Default)]
pub struct TableSet {
    tables: Vec<TargetTable>,
}

impl TableSet {
    /// An empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Keep a table read with `Lookup::read`; its key index is built here, once.
    pub fn insert(&mut self, uri: &str, title: Option<String>, lookup: Lookup) {
        let key_titles = lookup
            .key_columns
            .iter()
            .map(|column| lookup.titles.get(*column).cloned().flatten())
            .collect();
        let index = lookup.key_index();
        self.tables.push(TargetTable {
            uri: uri.to_string(),
            title,
            key_titles,
            lookup,
            index,
        });
    }

    /// Read the tables these uris name through the one door (`ObjectSource::read_object_at`, then
    /// `Lookup::read`); what does not read is handed back with the reason.
    pub fn read(uris: &[&str], source: &dyn ArraySource) -> (TableSet, Vec<(String, String)>) {
        let mut set = TableSet::new();
        let mut refused = Vec::new();
        for uri in uris {
            let object = match source.read_object_at(uri) {
                Ok(object) => object,
                Err(why) => {
                    refused.push((uri.to_string(), why));
                    continue;
                }
            };
            let title = identity::title_of_reflect(object.as_reflect()).map(str::to_string);
            match Lookup::read(object.as_reflect(), source) {
                Ok(lookup) => set.insert(uri, title, lookup),
                Err(why) => refused.push((uri.to_string(), why.to_string())),
            }
        }
        (set, refused)
    }

    /// How many tables the set holds.
    pub fn len(&self) -> usize {
        self.tables.len()
    }

    /// Whether the set holds no table.
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }
}

impl TargetIndexes for TableSet {
    fn target(&self, table_uri: &str) -> Option<&TargetTable> {
        self.tables.iter().find(|table| same_object(&table.uri, table_uri))
    }

    fn titled(&self, title: &str) -> Vec<&str> {
        self.tables
            .iter()
            .filter(|table| table.title.as_deref() == Some(title))
            .map(|table| table.uri.as_str())
            .collect()
    }
}

/// Why a value names no row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DanglingWhy {
    /// The target has a key column and no row holds this key.
    NoSuchKey,
    /// A rank outside `0..rows`, a fraction, a negative number, or not a number at all.
    NotARank {
        /// The rows the target has.
        rows: usize,
    },
}

/// What one link cell leads to.
#[derive(Clone, Debug, PartialEq)]
pub enum Resolved {
    /// The row the value names.
    Row {
        /// The target table's uri.
        table_uri: String,
        /// The row, from 0.
        row: usize,
    },
    /// The target key is duplicated: every row, the first is used and this is said.
    Ambiguous {
        /// The target table's uri.
        table_uri: String,
        /// Every row holding the key.
        rows: Vec<usize>,
    },
    /// The value names no row.
    Dangling {
        /// The target table's uri.
        table_uri: String,
        /// Why.
        why: DanglingWhy,
    },
    /// Convention D: a title that names no table of the set (UDAR writes `external file`).
    External {
        /// The title as written.
        label: String,
    },
    /// An empty string: names no row (a value, but not a key).
    Empty,
    /// An absent value (`NullValue`, `NaN`).
    Absent,
    /// The link cannot be followed from this cell.
    Unresolvable(LinkRefusal),
}

/// Resolve one cell of a link column. `own_uri` is the uri of the table the cell belongs to;
/// `sibling` is the cell of the `ByTitleIn` column on the same row.
///
/// A column of object uris (convention E, [`LinkTarget::Objects`]) does not resolve to a row: its
/// values are object chips (`source::object_chip`), and this answers
/// [`LinkRefusal::NotARowLink`].
pub fn resolve(
    link: &LinkDeclaration,
    value: &CellState,
    sibling: Option<&CellState>,
    own_uri: &str,
    targets: &dyn TargetIndexes,
) -> Resolved {
    let cell: &Cell = match value {
        CellState::Empty => return Resolved::Empty,
        CellState::Absent => return Resolved::Absent,
        CellState::Unreadable => return Resolved::Unresolvable(LinkRefusal::CellUnreadable),
        CellState::Vector(_) => return Resolved::Unresolvable(LinkRefusal::VectorValue),
        CellState::Value(cell) => cell,
    };

    let table_uri: String = match &link.target {
        LinkTarget::Key { table_uri } | LinkTarget::Rank { table_uri } => table_uri.clone(),
        LinkTarget::SameTable => own_uri.to_string(),
        LinkTarget::ByTitleIn { .. } => match sibling {
            Some(CellState::Value(Cell::Text(title))) => {
                let found = targets.titled(title);
                match found.len() {
                    0 => {
                        return Resolved::External {
                            label: title.clone(),
                        };
                    }
                    1 => found[0].to_string(),
                    count => {
                        return Resolved::Unresolvable(LinkRefusal::AmbiguousTitle {
                            title: title.clone(),
                            count,
                        });
                    }
                }
            }
            _ => {
                return Resolved::External {
                    label: String::new(),
                };
            }
        },
        LinkTarget::Objects => return Resolved::Unresolvable(LinkRefusal::NotARowLink),
    };

    let Some(target) = targets.target(&table_uri) else {
        return Resolved::Unresolvable(LinkRefusal::TargetUnknown { uri: table_uri });
    };

    if let Some(named) = &link.key_named
        && !target.key_titles.is_empty()
        && !target
            .key_titles
            .iter()
            .any(|title| title.as_deref().map(str::trim) == Some(named.trim()))
    {
        return Resolved::Unresolvable(LinkRefusal::KeyColumnMissing {
            named: named.clone(),
        });
    }

    match row_named_by(&target.index, cell) {
        Err(refusal) => {
            let super::NamingRefusal::SeveralKeyColumns { count } = refusal;
            Resolved::Unresolvable(LinkRefusal::AmbiguousTargetKey { keys: count })
        }
        Ok(RowMatch::One(row)) => Resolved::Row { table_uri, row },
        Ok(RowMatch::Several(rows)) => Resolved::Ambiguous { table_uri, rows },
        Ok(RowMatch::None) => {
            let why = if target.index.key_columns().is_empty() {
                DanglingWhy::NotARank {
                    rows: target.index.row_count(),
                }
            } else {
                DanglingWhy::NoSuchKey
            };
            Resolved::Dangling { table_uri, why }
        }
    }
}
