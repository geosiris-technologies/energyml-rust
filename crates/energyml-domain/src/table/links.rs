//! What a column of a table links to: the conventions in which a link is
//! written, read through a registry of readers.
//!
//! The standard has no field for a link between a column and a table (EML 2.3
//! `Column` has a `PropertyKind`, a `Uom` and facets, nothing more). Producers
//! have written it in four ways, and each way is a **reader** of the registry
//! ([`LinkReaders`], ADR-0006: the caller's readers are tried first):
//!
//! - **B**, `kind-and-facet`: the kind is `identifier` (a value naming a row by
//!   its key) or `explicit index` (by its rank), and one facet of kind
//!   `qualifier` holds the uri of the target table. This library's own
//!   convention; its reading is [`crate::property::qualifier_link`], shared with
//!   the axes of a property, never read a second way here. Followed without a
//!   warning.
//! - **A**, `description-text`: the target written in the column's
//!   `Description` (`key of the table ...: <uri>, KeyColumn ...`). A
//!   producer's convention, **not the specification's**; followed with
//!   [`LinkWarning::ByConvention`].
//! - **D**, `sibling-column`: a sibling column `<stem>_table` names the target
//!   table row by row, by its **title**; followed with
//!   [`LinkWarning::TargetByTitle`] (a title is not an identity).
//! - **E**: the values are complete ETP uris of objects. Not a link to a row:
//!   see [`uri_column`].
//!
//! A declaration that looks like a link and cannot be read is a named
//! [`LinkRefusal`], never a guess. A declaration that is readable but odd (a kind
//! that is not an identifier) is **followed and warned about**, never refused:
//! detect and signal, do not forbid.

use std::collections::BTreeSet;

use energyml_core::identifiers::EtpUri;
use energyml_core::identity;

use crate::property::{QualifierRefusal, qualifier_link};
use crate::property_kind::Resolution;
use crate::values::Family;

use super::class::{EXPLICIT_INDEX, IDENTIFIER, descends_from};
use super::{Cell, CellState, ColumnCells, TableSheet};

/// How a link is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Convention {
    /// B: kind `identifier` / `explicit index` and one `qualifier` facet holding the target table's uri.
    KindAndFacet,
    /// A: the target written in the column's `Description` (the UDAR form). Read with a warning.
    DescriptionText,
    /// D: a sibling column `<stem>_table` names the target table, row by row, by its title. Read with a warning.
    SiblingColumn,
    /// E: the values are complete ETP uris of objects.
    UriValues,
    /// A reader registered by the caller, by its id.
    Other(&'static str),
}

/// What a link points at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkTarget {
    /// Another table, a value naming a row by its key (kind `identifier` or a descendant).
    Key {
        /// The target table's uri.
        table_uri: String,
    },
    /// Another table, a value naming a row by its rank from 0 (kind `explicit index` or a descendant).
    Rank {
        /// The target table's uri.
        table_uri: String,
    },
    /// The table itself - a self reference, what makes a tree.
    SameTable,
    /// The target table is named row by row, by its title, in the column `column` of the same table.
    ByTitleIn {
        /// The column holding the titles.
        column: usize,
    },
    /// Objects, by complete uri (convention E).
    Objects,
}

/// A link that is followed, and what was noticed on the way.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkDeclaration {
    /// The column that links.
    pub column: usize,
    /// What it points at.
    pub target: LinkTarget,
    /// How the link is written.
    pub convention: Convention,
    /// What was noticed; the link is followed all the same.
    pub warnings: Vec<LinkWarning>,
    /// The key column a description (A) names; `None` for B and D, whose key column is resolved.
    pub key_named: Option<String>,
}

/// Followed, and said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkWarning {
    /// Declared in the description (A), not by a facet (B).
    ByConvention,
    /// The target is named by a title (D): a title is not an identity.
    TargetByTitle,
    /// The column's kind is neither `identifier` nor `explicit index` nor one of their descendants
    /// (or names no kind): the declaration is followed all the same.
    KindNotAnIdentifier,
    /// The kind says by key and the target has no key, or by rank and the target has one; the target decides.
    KindDisagreesWithTarget,
}

/// Why a declaration is not followed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LinkRefusal {
    /// A value that looks like an ETP uri and is not one.
    #[error("the qualifier facet `{value}` looks like an ETP uri and does not parse as one")]
    MalformedUri {
        /// The value as written.
        value: String,
    },
    /// The uri names an object that is not a table.
    #[error("`{uri}` names a `{qualified_type}`, not a table")]
    NotATable {
        /// The uri as written.
        uri: String,
        /// The qualified type it names.
        qualified_type: String,
    },
    /// Several facets of kind `qualifier`, one of them a uri.
    #[error(
        "column {column} has {count} qualifier facets (common TRG 2.3 §11.11 p. 294: \"Don't use several facets of the same kind for the same property\")"
    )]
    SeveralQualifierFacets {
        /// The column.
        column: usize,
        /// How many `qualifier` facets it holds.
        count: usize,
    },
    /// A facet and a description name two different targets.
    #[error(
        "the facet names `{facet}` and the description names `{description}`: two targets, none followed"
    )]
    ConflictingDeclarations {
        /// The uri the facet names.
        facet: String,
        /// The uri the description names.
        description: String,
    },
    /// A description that starts like a declaration and does not follow its form.
    #[error("the description starts like a link declaration and does not follow its form: `{text}`")]
    UnreadableDeclaration {
        /// The description as written.
        text: String,
    },
    /// The target has several key columns: a value names a row by one key.
    #[error("the target table has {keys} key columns, and a value names one row by one key")]
    AmbiguousTargetKey {
        /// How many key columns the target has.
        keys: usize,
    },
    /// The description names a key column the target does not have.
    #[error("the description names the key column `{named}`, which is not the target's key column")]
    KeyColumnMissing {
        /// The key column the description names.
        named: String,
    },
    /// The target table is not among the tables handed over.
    #[error("the target table `{uri}` is not in this package")]
    TargetUnknown {
        /// The target's uri.
        uri: String,
    },
    /// A title shared by several tables.
    #[error("`{title}` is the title of {count} tables: a title is not an identity")]
    AmbiguousTitle {
        /// The title.
        title: String,
        /// How many tables carry it.
        count: usize,
    },
    /// Both `<stem>` and `<stem>_uid` exist beside `<stem>_table`.
    #[error("both `{first}` and `{second}` could be the column `{sibling}` names the table of")]
    AmbiguousSibling {
        /// The sibling table-name column.
        sibling: String,
        /// The first candidate.
        first: String,
        /// The second candidate.
        second: String,
    },
    /// A vector cell names no single row.
    #[error("a vector cell names no single row")]
    VectorValue,
    /// The cell could not be read.
    #[error("the link cell could not be read")]
    CellUnreadable,
    /// A column of object uris: its values are object chips, not rows.
    #[error("a column of object uris is not resolved to a row; its values are object chips")]
    NotARowLink,
}

/// One way of declaring a link.
pub trait LinkReader: Send + Sync {
    /// A stable id (`kind-and-facet`, `description-text`, `sibling-column`, or the caller's).
    fn id(&self) -> &'static str;
    /// `None`: this column says nothing in this reader's convention.
    fn read(
        &self,
        table: &TableSheet,
        column: usize,
        kind: Option<&Resolution>,
    ) -> Option<Result<LinkDeclaration, LinkRefusal>>;
}

/// The readers, tried in order; the caller's first (ADR-0006).
pub struct LinkReaders {
    entries: Vec<Box<dyn LinkReader>>,
}

impl Default for LinkReaders {
    fn default() -> Self {
        Self::builtin()
    }
}

impl LinkReaders {
    /// No reader at all.
    pub fn none() -> Self {
        Self { entries: Vec::new() }
    }

    /// B, then A, then D.
    pub fn builtin() -> Self {
        Self {
            entries: vec![
                Box::new(KindAndFacet),
                Box::new(DescriptionText),
                Box::new(SiblingColumn),
            ],
        }
    }

    /// Tried before everything already there (last pushed wins).
    pub fn push(&mut self, reader: Box<dyn LinkReader>) -> &mut Self {
        self.entries.insert(0, reader);
        self
    }

    /// Drop the reader of this id (a convention the person unchecked).
    pub fn without(&mut self, id: &str) -> &mut Self {
        self.entries.retain(|reader| reader.id() != id);
        self
    }

    /// The ids, in the order tried.
    pub fn ids(&self) -> Vec<&'static str> {
        self.entries.iter().map(|reader| reader.id()).collect()
    }

    /// The declaration of one column: the first reader that answers wins; a B answer is confronted
    /// with A when A is registered. A facet and a description that name two different tables are a
    /// [`LinkRefusal::ConflictingDeclarations`], not a choice; the same table twice is B alone.
    pub fn read_column(
        &self,
        table: &TableSheet,
        column: usize,
        kind: Option<&Resolution>,
    ) -> Option<Result<LinkDeclaration, LinkRefusal>> {
        let winner = self
            .entries
            .iter()
            .find_map(|reader| reader.read(table, column, kind))?;
        let Ok(b) = &winner else {
            return Some(winner);
        };
        if b.convention != Convention::KindAndFacet {
            return Some(winner);
        }
        let Some(a_reader) = self.entries.iter().find(|r| r.id() == "description-text") else {
            return Some(winner);
        };
        if let Some(Ok(a)) = a_reader.read(table, column, kind) {
            let facet = target_uri(&b.target, table);
            let description = target_uri(&a.target, table);
            if let (Some(facet), Some(description)) = (facet, description)
                && !same_object(&facet, &description)
            {
                return Some(Err(LinkRefusal::ConflictingDeclarations { facet, description }));
            }
        }
        Some(winner)
    }

    /// Every column that declares something, in column order. `kinds` is `resolve_kinds` of the table.
    pub fn declarations(
        &self,
        table: &TableSheet,
        kinds: &[Option<Resolution>],
    ) -> Vec<(usize, Result<LinkDeclaration, LinkRefusal>)> {
        (0..table.columns.len())
            .filter_map(|column| {
                let kind = kinds.get(column).and_then(|k| k.as_ref());
                self.read_column(table, column, kind)
                    .map(|declaration| (column, declaration))
            })
            .collect()
    }
}

/// The uri a declared target names, when it names one by uri.
fn target_uri(target: &LinkTarget, table: &TableSheet) -> Option<String> {
    match target {
        LinkTarget::Key { table_uri } | LinkTarget::Rank { table_uri } => Some(table_uri.clone()),
        LinkTarget::SameTable => Some(table.uri.to_string()),
        LinkTarget::ByTitleIn { .. } | LinkTarget::Objects => None,
    }
}

/// Whether two addresses name the same object: the same uuid, and versions that do not contradict.
pub(super) fn same_object(a: &str, b: &str) -> bool {
    identity::same_address(a, b)
}

/// The uri of a table the facet or the description names, checked to be a table, and what kind of
/// link it makes (key, rank, itself); with the kind warning when the kind is no identifier.
fn declare(
    table: &TableSheet,
    column: usize,
    uri: &str,
    kind: Option<&Resolution>,
    convention: Convention,
) -> Result<LinkDeclaration, LinkRefusal> {
    let uri = uri.trim();
    let parsed = EtpUri::parse(uri).map_err(|_| LinkRefusal::MalformedUri {
        value: uri.to_string(),
    })?;
    if parsed.uuid.is_none() {
        return Err(LinkRefusal::MalformedUri {
            value: uri.to_string(),
        });
    }
    let qualified = parsed.qualified_type();
    let is_table = qualified.as_ref().is_some_and(|q| {
        matches!(
            q.type_key(),
            "ColumnBasedTable" | "StringTableLookup" | "DoubleTableLookup"
        )
    });
    if !is_table {
        return Err(LinkRefusal::NotATable {
            uri: uri.to_string(),
            qualified_type: qualified.map(|q| q.to_string()).unwrap_or_default(),
        });
    }
    let by_rank = kind.is_some_and(|k| descends_from(k, EXPLICIT_INDEX));
    let target = if same_object(uri, &table.uri) {
        LinkTarget::SameTable
    } else if by_rank {
        LinkTarget::Rank {
            table_uri: uri.to_string(),
        }
    } else {
        LinkTarget::Key {
            table_uri: uri.to_string(),
        }
    };
    let mut warnings = Vec::new();
    let is_identifier =
        kind.is_some_and(|k| descends_from(k, IDENTIFIER) || descends_from(k, EXPLICIT_INDEX));
    if !is_identifier {
        warnings.push(LinkWarning::KindNotAnIdentifier);
    }
    Ok(LinkDeclaration {
        column,
        target,
        convention,
        warnings,
        key_named: None,
    })
}

/// Convention B.
struct KindAndFacet;

impl LinkReader for KindAndFacet {
    fn id(&self) -> &'static str {
        "kind-and-facet"
    }

    fn read(
        &self,
        table: &TableSheet,
        column: usize,
        kind: Option<&Resolution>,
    ) -> Option<Result<LinkDeclaration, LinkRefusal>> {
        let sheet = table.columns.get(column)?;
        match qualifier_link(&sheet.facets) {
            Ok(None) => None,
            Err(QualifierRefusal::Several { count }) => {
                Some(Err(LinkRefusal::SeveralQualifierFacets { column, count }))
            }
            Err(QualifierRefusal::MalformedUri { value }) => Some(Err(LinkRefusal::MalformedUri { value })),
            Ok(Some(uri)) => Some(declare(table, column, &uri, kind, Convention::KindAndFacet)),
        }
    }
}

/// What a description of the UDAR form says: whether it is the "this table" form, the uri, the key.
struct DescribedLink {
    this_table: bool,
    uri: String,
    key: String,
}

/// Parse the two exact forms; `None` when the text starts like a declaration and is neither.
///
/// ```text
/// key of the table `<title>`: <uri>, KeyColumn `<key>`
/// key of this table (self reference): <uri>, KeyColumn `<key>`
/// ```
fn parse_description(text: &str) -> Option<DescribedLink> {
    let (this_table, rest) = if let Some(after) = text.strip_prefix("key of the table `") {
        let (_title, after_title) = after.split_once('`')?;
        (false, after_title.strip_prefix(": ")?)
    } else {
        (true, text.strip_prefix("key of this table (self reference): ")?)
    };
    let (uri, after_uri) = rest.split_once(", KeyColumn `")?;
    let key = after_uri.strip_suffix('`')?;
    if uri.trim().is_empty() || key.is_empty() || key.contains('`') {
        return None;
    }
    Some(DescribedLink {
        this_table,
        uri: uri.trim().to_string(),
        key: key.to_string(),
    })
}

/// Convention A. A producer's convention, not the specification's.
struct DescriptionText;

impl LinkReader for DescriptionText {
    fn id(&self) -> &'static str {
        "description-text"
    }

    fn read(
        &self,
        table: &TableSheet,
        column: usize,
        kind: Option<&Resolution>,
    ) -> Option<Result<LinkDeclaration, LinkRefusal>> {
        let description = table.columns.get(column)?.description.as_deref()?;
        let text = description.trim();
        if !text.starts_with("key of ") {
            return None;
        }
        let Some(described) = parse_description(text) else {
            return Some(Err(LinkRefusal::UnreadableDeclaration {
                text: text.to_string(),
            }));
        };
        if described.this_table && !same_object(&described.uri, &table.uri) {
            return Some(Err(LinkRefusal::UnreadableDeclaration {
                text: text.to_string(),
            }));
        }
        Some(
            declare(table, column, &described.uri, kind, Convention::DescriptionText).map(
                |mut declaration| {
                    declaration.warnings.insert(0, LinkWarning::ByConvention);
                    declaration.key_named = Some(described.key);
                    declaration
                },
            ),
        )
    }
}

/// Convention D.
struct SiblingColumn;

impl LinkReader for SiblingColumn {
    fn id(&self) -> &'static str {
        "sibling-column"
    }

    fn read(
        &self,
        table: &TableSheet,
        column: usize,
        _kind: Option<&Resolution>,
    ) -> Option<Result<LinkDeclaration, LinkRefusal>> {
        let title = table.columns.get(column)?.title.as_deref()?.trim();
        let stem = title.strip_suffix("_uid").unwrap_or(title);
        if stem.is_empty() {
            return None;
        }
        let sibling = format!("{stem}_table");
        let at = table.column_titled(&sibling)?;
        if at == column {
            return None;
        }
        let uid = format!("{stem}_uid");
        if let (Some(first), Some(second)) = (table.column_titled(stem), table.column_titled(&uid))
            && first != second
        {
            return Some(Err(LinkRefusal::AmbiguousSibling {
                sibling,
                first: stem.to_string(),
                second: uid,
            }));
        }
        Some(Ok(LinkDeclaration {
            column,
            target: LinkTarget::ByTitleIn { column: at },
            convention: Convention::SiblingColumn,
            warnings: vec![LinkWarning::TargetByTitle],
            key_named: None,
        }))
    }
}

/// A column whose every non-empty value is a complete ETP uri of an object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UriColumn {
    /// Cells holding a uri.
    pub uris: usize,
    /// Empty cells (a value, S16, but not a uri).
    pub empty: usize,
    /// The distinct qualified types the uris name, sorted.
    pub qualified_types: Vec<String>,
}

/// Convention E: `Some` when the column is text, has at least one uri, and every non-empty, non-absent
/// value passes `EtpUri::parse` and names an object (a uuid).
///
/// Such a column is **not** a link to a row ([`super::resolve()`] answers
/// [`LinkRefusal::NotARowLink`]): its values are object chips (`source::object_chip`).
pub fn uri_column(cells: &ColumnCells<'_>) -> Option<UriColumn> {
    if cells.family() != Family::Text {
        return None;
    }
    let rows = cells.rows("").ok()?;
    let mut uris = 0;
    let mut empty = 0;
    let mut types = BTreeSet::new();
    for row in 0..rows {
        match cells.cell(row)? {
            CellState::Empty => empty += 1,
            CellState::Value(Cell::Text(text)) => {
                let parsed = EtpUri::parse(text.trim()).ok()?;
                parsed.uuid.as_ref()?;
                types.insert(parsed.qualified_type()?.to_string());
                uris += 1;
            }
            _ => return None,
        }
    }
    if uris == 0 {
        return None;
    }
    Some(UriColumn {
        uris,
        empty,
        qualified_types: types.into_iter().collect(),
    })
}
