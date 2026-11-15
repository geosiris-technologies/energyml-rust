//! Checking the links of a table, and of a set of tables.
//!
//! [`check_links`] follows every cell of every declared link column
//! ([`super::resolve()`]) and counts what it finds: rows found, keys that dangle,
//! keys that name several rows, titles that name no table, empty and absent
//! cells, links that cannot be followed. A column of object uris (convention E)
//! is checked against the package with [`crate::source::object_chip`]. Nothing
//! is repaired and nothing is hidden: a link followed under a warning is counted
//! **and** warned about.
//!
//! The set form, [`check_links_of_set`], repeats the table form on each table:
//! it is not a second computation.

use crate::source::{ObjectSource, Presence, object_chip};

use super::{
    Cell, CellState, ColumnCells, Convention, LinkDeclaration, LinkRefusal, LinkTarget, LinkWarning,
    Resolved, TableRefusal, TableSheet, TargetIndexes, resolve, uri_column,
};

/// What checking one link column found.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnReport {
    /// The column that links.
    pub column: usize,
    /// How the link is written.
    pub convention: Convention,
    /// The declaration's warnings, plus `KindDisagreesWithTarget` when the target contradicts the kind.
    pub warnings: Vec<LinkWarning>,
    /// Cells looked at.
    pub checked: usize,
    /// Cells that name a row, once.
    pub found: usize,
    /// Rows whose value names no row.
    pub dangling: Vec<usize>,
    /// Rows whose key is held by several rows of the target.
    pub ambiguous: Vec<usize>,
    /// Rows whose target is a title naming no table (D).
    pub external: Vec<usize>,
    /// Empty cells (a value, but no key).
    pub empty: usize,
    /// Absent cells.
    pub absent: usize,
    /// Rows the link could not be followed from, with why.
    pub unresolvable: Vec<(usize, LinkRefusal)>,
}

/// What checking one column of object uris found.
#[derive(Clone, Debug, PartialEq)]
pub struct UriColumnReport {
    /// The column.
    pub column: usize,
    /// Rows naming an object the package holds.
    pub present: usize,
    /// Rows naming an object the package does not hold.
    pub missing: Vec<usize>,
    /// Rows naming an object of another dataspace (not looked up).
    pub other_dataspace: usize,
    /// Rows whose value is not an object uri after all.
    pub refused: Vec<usize>,
}

/// What checking the links of one table found.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkReport {
    /// The table's uri.
    pub table_uri: String,
    /// One report per followed declaration, in column order.
    pub columns: Vec<ColumnReport>,
    /// Declarations not followed, by column.
    pub refused: Vec<(usize, LinkRefusal)>,
    /// Link columns that could not be read as cells, with why.
    pub unreadable: Vec<(usize, String)>,
    /// Duplicated keys of this table itself. Empty when the table is not among the `targets`
    /// handed over (it was not put in the set): the table cannot be asked without its index.
    pub key_duplicates: Vec<(Vec<Cell>, Vec<usize>)>,
    /// The columns of object uris that declare no link, checked against the package.
    pub uri_columns: Vec<UriColumnReport>,
}

impl LinkReport {
    /// Whether nothing dangles, nothing is ambiguous, nothing is refused or unreadable, nothing
    /// cannot be followed and every uri resolves.
    pub fn is_clean(&self) -> bool {
        self.refused.is_empty()
            && self.unreadable.is_empty()
            && self
                .columns
                .iter()
                .all(|c| c.dangling.is_empty() && c.ambiguous.is_empty() && c.unresolvable.is_empty())
            && self
                .uri_columns
                .iter()
                .all(|c| c.missing.is_empty() && c.refused.is_empty())
    }
}

/// Check one table. `cells` is `Table::read_cells`; `declarations` is `LinkReaders::declarations`;
/// `objects`, when given, checks the uri columns (with the dataspace it stands for).
pub fn check_links(
    sheet: &TableSheet,
    cells: &[Result<ColumnCells<'_>, TableRefusal>],
    declarations: &[(usize, Result<LinkDeclaration, LinkRefusal>)],
    targets: &dyn TargetIndexes,
    objects: Option<(&dyn ObjectSource, Option<&str>)>,
) -> LinkReport {
    let own_uri: &str = &sheet.uri;
    let mut columns = Vec::new();
    let mut refused = Vec::new();
    let mut unreadable = Vec::new();
    let mut declared = Vec::new();

    for (column, declaration) in declarations {
        declared.push(*column);
        let declaration = match declaration {
            Ok(declaration) => declaration,
            Err(why) => {
                refused.push((*column, why.clone()));
                continue;
            }
        };
        let link_cells = match cells.get(declaration.column) {
            Some(Ok(link_cells)) => link_cells,
            Some(Err(why)) => {
                unreadable.push((declaration.column, why.to_string()));
                continue;
            }
            None => {
                unreadable.push((declaration.column, "the table has no such column".to_string()));
                continue;
            }
        };
        let name = title_of(sheet, declaration.column);
        let rows = match link_cells.rows(&name) {
            Ok(rows) => rows,
            Err(why) => {
                unreadable.push((declaration.column, why.to_string()));
                continue;
            }
        };
        let sibling_cells = match declaration.target {
            LinkTarget::ByTitleIn { column } => cells.get(column).and_then(|c| c.as_ref().ok()),
            _ => None,
        };

        let mut report = ColumnReport {
            column: declaration.column,
            convention: declaration.convention,
            warnings: declaration.warnings.clone(),
            checked: rows,
            found: 0,
            dangling: Vec::new(),
            ambiguous: Vec::new(),
            external: Vec::new(),
            empty: 0,
            absent: 0,
            unresolvable: Vec::new(),
        };
        for row in 0..rows {
            let state = link_cells.cell(row).unwrap_or(CellState::Unreadable);
            let sibling = sibling_cells.and_then(|c| c.cell(row));
            match resolve(declaration, &state, sibling.as_ref(), own_uri, targets) {
                Resolved::Row { .. } => report.found += 1,
                Resolved::Ambiguous { .. } => report.ambiguous.push(row),
                Resolved::Dangling { .. } => report.dangling.push(row),
                Resolved::External { .. } => report.external.push(row),
                Resolved::Empty => report.empty += 1,
                Resolved::Absent => report.absent += 1,
                Resolved::Unresolvable(why) => report.unresolvable.push((row, why)),
            }
        }
        if kind_disagrees(&declaration.target, targets)
            && !report.warnings.contains(&LinkWarning::KindDisagreesWithTarget)
        {
            report.warnings.push(LinkWarning::KindDisagreesWithTarget);
        }
        columns.push(report);
    }

    let key_duplicates = targets
        .target(own_uri)
        .map(|target| target.index.duplicates())
        .unwrap_or_default();

    let mut uri_columns = Vec::new();
    if let Some((source, here)) = objects {
        for (column, read) in cells.iter().enumerate() {
            let Ok(read) = read else { continue };
            if declared.contains(&column) || uri_column(read).is_none() {
                continue;
            }
            let name = title_of(sheet, column);
            let Ok(rows) = read.rows(&name) else { continue };
            let mut report = UriColumnReport {
                column,
                present: 0,
                missing: Vec::new(),
                other_dataspace: 0,
                refused: Vec::new(),
            };
            for row in 0..rows {
                let Some(CellState::Value(Cell::Text(text))) = read.cell(row) else {
                    continue;
                };
                match object_chip(&text, here, source) {
                    Ok(chip) => match chip.presence {
                        Presence::InPackage => report.present += 1,
                        Presence::NotInPackage => report.missing.push(row),
                        Presence::OtherDataspace { .. } => report.other_dataspace += 1,
                    },
                    Err(_) => report.refused.push(row),
                }
            }
            uri_columns.push(report);
        }
    }

    LinkReport {
        table_uri: own_uri.to_string(),
        columns,
        refused,
        unreadable,
        key_duplicates,
        uri_columns,
    }
}

/// One table of a set to check: its sheet, its columns as read, and the links its columns declare.
pub type TableToCheck<'a, 'c> = (
    &'a TableSheet,
    &'a [Result<ColumnCells<'c>, TableRefusal>],
    &'a [(usize, Result<LinkDeclaration, LinkRefusal>)],
);

/// One call for a set: `check_links` on each table, in order. The same function, called again: a
/// set report is never computed another way.
pub fn check_links_of_set(
    tables: &[TableToCheck<'_, '_>],
    targets: &dyn TargetIndexes,
    objects: Option<(&dyn ObjectSource, Option<&str>)>,
) -> Vec<LinkReport> {
    tables
        .iter()
        .map(|(sheet, cells, declarations)| check_links(sheet, cells, declarations, targets, objects))
        .collect()
}

/// The title of a column, or `column <i>` (its place from 0) when it has none.
fn title_of(sheet: &TableSheet, column: usize) -> String {
    sheet
        .columns
        .get(column)
        .and_then(|c| c.title.as_deref())
        .map(str::to_string)
        .unwrap_or_else(|| format!("column {column}"))
}

/// Whether the target contradicts what the link's kind says: a `Key` link into a table with no key
/// column, or a `Rank` link into a table with one. The target decides how the row is found; this
/// is only said.
fn kind_disagrees(target: &LinkTarget, targets: &dyn TargetIndexes) -> bool {
    match target {
        LinkTarget::Key { table_uri } => targets
            .target(table_uri)
            .is_some_and(|table| table.index.key_columns().is_empty()),
        LinkTarget::Rank { table_uri } => targets
            .target(table_uri)
            .is_some_and(|table| !table.index.key_columns().is_empty()),
        LinkTarget::SameTable | LinkTarget::ByTitleIn { .. } | LinkTarget::Objects => false,
    }
}
