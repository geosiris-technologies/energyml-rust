//! A table without its values: what lists and classifies the tables of a
//! package without reading one array.
//!
//! The package index keeps one [`TableSheet`] per table. The sheet says each
//! column's title, kind, unit, facets, family and - when it is known without a
//! file - its number of values. A count that needs a file is `None`, never a
//! guess.

use std::sync::Arc;

use energyml_core::identifiers::EtpUri;
use energyml_core::identity;
use energyml_core::value::Reflect;

use crate::package_index::SheetKind;
use crate::property::{Facet, PropertyKindRef};
use crate::source::NoObjects;
use crate::table::Table;
use crate::values::{self, Family, Values};

/// One table, without its values.
#[derive(Clone, Debug, PartialEq)]
pub struct TableSheet {
    /// The table's full uri - never a bare uuid inside (ADR-0003).
    pub uri: Box<str>,
    /// `Citation.Title`.
    pub title: Option<Arc<str>>,
    /// `eml23.ColumnBasedTable`, `resqml20.obj_StringTableLookup`, `resqml20.obj_DoubleTableLookup`.
    pub qualified_type: Arc<str>,
    /// Keys first, then the value columns - the order of `Table::columns`.
    pub columns: Box<[ColumnSheet]>,
}

/// One column, without its values.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnSheet {
    /// `Title`.
    pub title: Option<Arc<str>>,
    /// `Description` - what convention A reads.
    pub description: Option<Arc<str>>,
    /// Whether it is a `KeyColumn`.
    pub key: bool,
    /// The kind as written, unresolved (the index's own type).
    pub kind: Option<SheetKind>,
    /// `Uom`.
    pub uom: Option<Arc<str>>,
    /// `ValueCountPerRow`.
    pub values_per_row: u64,
    /// The facets, as `property::read_facets` reads them.
    pub facets: Box<[Facet]>,
    /// The family of the array, from its type alone (`values::family`); for a 2.0.1 lookup, the
    /// family of the inline values.
    pub family: Option<Family>,
    /// How many values, when it is known without a file: an inline array, a constant, a lattice.
    /// `None` for an external array (its dataset header has to be read, `values::count`).
    pub values: Option<usize>,
    /// Whether the values live in an external file (`values::is_external`).
    pub external: bool,
}

impl TableSheet {
    /// The sheet of a table already read. `uri` is the table's full uri.
    pub fn of_table(table: &Table<'_>, uri: &str) -> TableSheet {
        let columns: Vec<ColumnSheet> = table
            .columns
            .iter()
            .map(|column| {
                let kind = match &column.kind {
                    Some(PropertyKindRef::Object { uuid, title }) => Some(SheetKind::Object {
                        uuid: Arc::from(uuid.as_str()),
                        title: title.as_deref().map(Arc::from),
                    }),
                    Some(PropertyKindRef::Standard { name }) => {
                        Some(SheetKind::Standard(Arc::from(name.as_str())))
                    }
                    None => None,
                };
                let family = column
                    .array
                    .and_then(values::family)
                    .or_else(|| column.inline.as_ref().map(Values::family));
                let external = column.array.is_some_and(values::is_external);
                let count = match (&column.inline, column.array) {
                    (Some(inline), _) => Some(inline.len()),
                    (None, Some(array)) if !external => {
                        values::count(array, &NoObjects, "column").ok()
                    }
                    _ => None,
                };
                ColumnSheet {
                    title: column.title.as_deref().map(Arc::from),
                    description: column.description.as_deref().map(Arc::from),
                    key: column.key,
                    kind,
                    uom: column.uom.as_deref().map(Arc::from),
                    values_per_row: column.values_per_row.max(1),
                    facets: column.facets.clone().into_boxed_slice(),
                    family,
                    values: count,
                    external,
                }
            })
            .collect();
        TableSheet {
            uri: uri.into(),
            title: table.title.as_deref().map(Arc::from),
            qualified_type: Arc::from(table.qualified_type.as_str()),
            columns: columns.into_boxed_slice(),
        }
    }

    /// Read a table object and take its sheet; the uri is the object's own
    /// (`identity::uri_of_reflect`). Refused as `Table::read` refuses.
    pub fn of(object: &dyn Reflect) -> crate::Result<TableSheet> {
        let table = Table::read(object)?;
        let uri = identity::uri_of_reflect(object)
            .unwrap_or_else(|| EtpUri::object(None, &table.qualified_type, &table.uuid));
        Ok(TableSheet::of_table(&table, &uri))
    }

    /// The rows, when every column's count is known and they agree; `None` otherwise.
    pub fn row_count(&self) -> Option<usize> {
        let mut rows: Option<usize> = None;
        for column in self.columns.iter() {
            let n = column.values?;
            let per_row = column.values_per_row.max(1) as usize;
            if n % per_row != 0 {
                return None;
            }
            let here = n / per_row;
            match rows {
                None => rows = Some(here),
                Some(known) if known == here => {}
                Some(_) => return None,
            }
        }
        rows
    }

    /// The first column with exactly this title (case-sensitive, trimmed).
    pub fn column_titled(&self, title: &str) -> Option<usize> {
        let wanted = title.trim();
        self.columns
            .iter()
            .position(|column| column.title.as_deref().is_some_and(|own| own.trim() == wanted))
    }

    /// Whether every title of `titles` names a column - recognising a table by its column titles,
    /// with no array read (what a `VerbatimSource` asks). An empty list recognises nothing: `false`.
    pub fn has_titles(&self, titles: &[&str]) -> bool {
        !titles.is_empty() && titles.iter().all(|title| self.column_titled(title).is_some())
    }

    /// The key columns.
    pub fn key_columns(&self) -> Vec<usize> {
        self.columns
            .iter()
            .enumerate()
            .filter(|(_, column)| column.key)
            .map(|(at, _)| at)
            .collect()
    }
}
