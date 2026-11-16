//! What a view **proposes** for a first drawing, and which dimensions of its objects it needs declared.
//!
//! Some values are not in the file: the roles of the two dimensions of a curtain, the table that holds its
//! coordinates. A request that does not give them is refused ([`crate::ViewRefusal::MissingSetting`]), and a
//! person who does not know what to give sees nothing. [`assist`] asks the view what it can *propose* from the
//! package: a [`Proposal`] is a value for a setting, with the reason it is proposed. It is never a value read from the
//! file, and a face says so wherever it shows one (the command line writes a note, a form marks the field).
//!
//! The same call lists the **dimensions** a view needs declared ([`DimensionAxes`]): for each, its size and the
//! numeric columns of the package's tables that could hold its positions ([`AxisColumn`]). A face draws one row per
//! dimension from it and composes the links setting with [`axis_links_value`], never writing the JSON shape itself;
//! [`axis_links_read`] is the way back.
//!
//! A proposal is made from the package alone and decides nothing: the request keeps what it gives, and a view that
//! has nothing to propose answers [`Assist::default`].

use energyml_domain::package_index::{NodeState, PackageIndex};
use energyml_domain::table::Table;
use energyml_domain::values::{self, Family};
use energyml_section::along_well::AxisLinkAsk;
use serde_json::Value;

use crate::ask::ViewAsk;
use crate::kind::{ViewContext, ViewKinds, local_type};
use crate::refusal::ViewRefusal;

/// A value the file does not state, proposed for a first drawing; always said as a proposal.
#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    /// The setting it is a value of (its declared name).
    pub setting: String,
    /// The value, as the setting takes it.
    pub value: Value,
    /// Why it is proposed, in one sentence (English).
    pub why: String,
}

/// A numeric column of a table of the package, a candidate for the positions of one dimension.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AxisColumn {
    /// The table, by its full uri.
    pub table: String,
    /// Its title, or its uri when it has none.
    pub table_title: String,
    /// The title of the column, exactly (the link names it so).
    pub column: String,
}

/// One dimension of a view's components that needs its positions and its role declared.
#[derive(Clone, Debug, PartialEq)]
pub struct DimensionAxes {
    /// The dimension, from 0.
    pub dim: usize,
    /// Its number of positions.
    pub size: usize,
    /// The JSON setting the link is kept in (`curtain_links`).
    pub links_setting: &'static str,
    /// The setting that says which axis it runs along (`slow_axis` or `fast_axis`).
    pub axis_setting: &'static str,
    /// The setting that says its sign (`slow_sign` or `fast_sign`).
    pub sign_setting: &'static str,
    /// The numeric value columns of the tables that have exactly `size` rows, key columns left out, by table (in
    /// the order of their uris) then by column (in the order of the table).
    pub candidates: Vec<AxisColumn>,
}

/// What a view proposes and which dimensions it needs declared.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Assist {
    /// The values proposed, each for a setting the file does not state.
    pub proposals: Vec<Proposal>,
    /// The dimensions to declare, in order.
    pub dimensions: Vec<DimensionAxes>,
}

/// What the view proposes and which dimensions it needs declared, for these objects: the answer of
/// [`crate::ViewKind::assist`], the objects named by uri or by bare uuid (the package index turns the latter into
/// the former).
///
/// # Errors
///
/// [`ViewRefusal::UnknownView`], for a view the registry does not hold. Nothing else: a view with nothing to
/// propose, or objects the package does not hold, answer an empty [`Assist`].
pub fn assist(kinds: &ViewKinds, ask: &ViewAsk, cx: &ViewContext<'_>) -> Result<Assist, ViewRefusal> {
    let Some(kind) = kinds.find(&ask.view) else {
        return Err(ViewRefusal::UnknownView {
            asked: ask.view.clone(),
            known: kinds.ids().join(", "),
        });
    };
    let objects: Vec<String> = ask
        .objects
        .iter()
        .map(|name| {
            cx.index
                .node(name.trim())
                .map(|node| node.uri.to_string())
                .unwrap_or_else(|| name.clone())
        })
        .collect();
    Ok(kind.assist(&objects, cx))
}

/// The wire value of the links setting, from `(dim, column)` pairs: a list of the table links the section reads,
/// `[{"table": {"table": uri, "column": title, "dim": n}}, ...]`, in the order given. A face never writes that
/// shape itself.
pub fn axis_links_value(links: &[(usize, AxisColumn)]) -> Value {
    let asks: Vec<AxisLinkAsk> = links
        .iter()
        .map(|(dim, column)| AxisLinkAsk::Table {
            table: column.table.clone(),
            column: column.column.clone(),
            dim: *dim,
        })
        .collect();
    serde_json::to_value(asks).unwrap_or_else(|_| Value::Array(Vec::new()))
}

/// The `(dim, table uri, column)` the links setting holds: its table links only. A link of another kind (a paired
/// property, a WITSML channel), an entry that is not a link and a value that is not a list are left out.
pub fn axis_links_read(value: &Value) -> Vec<(usize, String, String)> {
    let Some(entries) = value.as_array() else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| serde_json::from_value::<AxisLinkAsk>(entry.clone()).ok())
        .filter_map(|link| match link {
            AxisLinkAsk::Table { table, column, dim } => Some((dim, table, column)),
            _ => None,
        })
        .collect()
}

/// A table of the package, as the axes of a dimension see it: its shape, never its numbers.
#[derive(Clone, Debug)]
pub(crate) struct AxisTable {
    /// The table's uri.
    pub uri: String,
    /// Its title, or its uri.
    pub title: String,
    /// Its rows: every column that has an array agrees on it.
    pub rows: usize,
    /// How many key columns.
    pub keys: usize,
    /// How many value columns (every column that is not a key), numeric or not.
    pub values: usize,
    /// The titles of the numeric value columns that hold one value per row, in the order of the table.
    pub numeric: Vec<String>,
}

impl AxisTable {
    /// The numeric value columns of this table, as candidates.
    fn columns(&self) -> impl Iterator<Item = AxisColumn> + '_ {
        self.numeric.iter().map(|column| AxisColumn {
            table: self.uri.clone(),
            table_title: self.title.clone(),
            column: column.clone(),
        })
    }
}

/// The tables of the package that can say something of an axis, by their uris. A table that cannot be read, that
/// has no array to count or whose columns disagree on their rows is left out: this is a proposal, not a check, and
/// the refusal of a bad link is the section's, by name, when it is drawn. The counts are metadata only: no array
/// is read (`values::count`).
pub(crate) fn axis_tables(cx: &ViewContext<'_>) -> Vec<AxisTable> {
    let mut out = Vec::new();
    for node in cx.index.nodes() {
        if &*node.type_key != "ColumnBasedTable" || node.state != NodeState::Present {
            continue;
        }
        let uri = node.uri.to_string();
        let Ok(object) = cx.source.read_object_at(&uri) else {
            continue;
        };
        let Ok(table) = Table::read(object.as_reflect()) else {
            continue;
        };
        let mut rows: Option<usize> = None;
        let mut agree = true;
        let mut keys = 0;
        let mut value_columns = 0;
        let mut numeric = Vec::new();
        for column in &table.columns {
            if column.key {
                keys += 1;
            } else {
                value_columns += 1;
            }
            // A 2.0.1 column with inline pairs has no array: it counts as a column, it gives no rows.
            let Some(array) = column.array else {
                continue;
            };
            let at = column.title.as_deref().unwrap_or("column");
            let Ok(count) = values::count(array, cx.source, at) else {
                agree = false;
                break;
            };
            if *rows.get_or_insert(count) != count {
                agree = false;
                break;
            }
            let is_number = matches!(values::family(array), Some(Family::Floating | Family::Integer));
            if !column.key
                && is_number
                && column.values_per_row == 1
                && let Some(title) = &column.title
            {
                numeric.push(title.clone());
            }
        }
        let Some(rows) = rows.filter(|_| agree) else {
            continue;
        };
        out.push(AxisTable {
            title: node
                .title
                .as_deref()
                .map(str::to_string)
                .unwrap_or_else(|| uri.clone()),
            uri,
            rows,
            keys,
            values: value_columns,
            numeric,
        });
    }
    out.sort_by(|a, b| a.uri.cmp(&b.uri));
    out
}

/// The numeric value columns of the tables that have exactly `size` rows.
pub(crate) fn columns_of_size(tables: &[AxisTable], size: usize) -> Vec<AxisColumn> {
    tables
        .iter()
        .filter(|table| table.rows == size)
        .flat_map(AxisTable::columns)
        .collect()
}

/// The one table that holds the axes of dimensions of these sizes by the convention of ADR 0029: one key column,
/// one numeric value column per dimension (nothing else), as many rows as every dimension has positions. `None`
/// when no table, or more than one, fits: several fitting tables are a choice for the person, never a guess.
pub(crate) fn the_table_that_fits<'a>(tables: &'a [AxisTable], sizes: &[usize]) -> Option<&'a AxisTable> {
    let mut fits = tables.iter().filter(|table| {
        table.keys == 1
            && table.values == sizes.len()
            && table.numeric.len() == sizes.len()
            && sizes.iter().all(|&size| size == table.rows)
    });
    match (fits.next(), fits.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    }
}

impl AxisTable {
    /// The `(dim, column)` pairs of this table: dimension `i` takes the `i`-th value column (ADR 0029: the order
    /// of the dimensions, the slowest first).
    pub(crate) fn links(&self) -> Vec<(usize, AxisColumn)> {
        self.columns().enumerate().collect()
    }
}

/// Whether [`assist`] will propose the links of a curtain whose dimensions have these sizes, **from the index
/// alone** (an offer has no array source): exactly one `ColumnBasedTable` goes with the curtain by the convention of
/// ADR 0029 (one key column, one numeric value column per dimension and nothing else, as many rows as every dimension
/// has positions), and no other table has a row count the index cannot tell (an external array: its header is
/// read at drawing time, so the answer is left to the drawing). It is the cautious side of [`the_table_that_fits`]:
/// what an offer says is proposed is always proposed by `assist`, never the other way round.
pub(crate) fn links_are_proposable(index: &PackageIndex, sizes: &[usize]) -> bool {
    let mut fits = 0;
    for (_, sheet) in index.tables() {
        if local_type(&sheet.qualified_type) != "ColumnBasedTable" {
            continue;
        }
        let Some(rows) = sheet.row_count() else {
            return false;
        };
        let keys = sheet.columns.iter().filter(|column| column.key).count();
        let values: Vec<_> = sheet.columns.iter().filter(|column| !column.key).collect();
        let numeric = values.iter().all(|column| {
            column.title.is_some()
                && column.values_per_row == 1
                && matches!(column.family, Some(Family::Floating | Family::Integer))
        });
        if keys == 1 && values.len() == sizes.len() && numeric && sizes.iter().all(|&size| size == rows) {
            fits += 1;
        }
    }
    fits == 1
}
