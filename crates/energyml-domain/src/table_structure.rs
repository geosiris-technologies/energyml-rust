//! How tables hold together: the graph of their links, the tree of the rows of a
//! self-referent table, the key-value shape of a sheet of properties.
//!
//! Every output is **indices and uris, never a label**: what a view writes next to a node
//! is the view's. Every refusal is named: a cycle of rows is a [`RowTreeRefusal::Cycle`]
//! with its rows, never a tree cut where it was convenient.
//!
//! The pieces:
//!
//! - [`schema`] - the graph of the tables of a set: a table links to another by key or by
//!   rank, to itself, to a table named row by row by its title, to objects by uri; objects
//!   that hold a reference to a table (a `CategoryLookup`) are edges of the package index;
//! - [`self_parents`], [`row_forest`], [`group_roots`] - the rows of a table whose column
//!   names a parent row, as a [`Forest`](crate::forest::Forest);
//! - [`kv_shape`] - whether a sheet is a key-value table (a LAS header, a list of
//!   parameters), and which column plays which role, the guesses said.

use std::collections::{BTreeMap, BTreeSet};

use energyml_core::identity;

use crate::forest::{Forest, Parent, forest};
use crate::property_kind::Resolution;
use crate::table::{
    CellState, ColumnCells, DESCRIPTION, IntervalColumns, KeyIndex, LinkDeclaration, LinkRefusal, LinkTarget,
    RowMatch, TableRefusal, TableSheet, UriColumn, descends_from, row_named_by,
};
use crate::values::Family;

/// What one table says of its links (from its sheet, and from its values when they were
/// read).
#[derive(Clone, Debug, PartialEq)]
pub struct TableLinks {
    /// The table's full uri.
    pub uri: String,
    /// The table's title.
    pub title: Option<String>,
    /// The links its columns declare and that are followed.
    pub declarations: Vec<LinkDeclaration>,
    /// The declarations that were refused, by column.
    pub refused: Vec<(usize, LinkRefusal)>,
    /// Convention E, when the values were read (empty otherwise).
    pub uri_columns: Vec<(usize, UriColumn)>,
    /// For each `ByTitleIn` link column: the distinct titles read in its sibling column
    /// (empty when the values were not read).
    pub titles_named: Vec<(usize, Vec<String>)>,
}

/// A reference the package index holds, from one object to another (a `CategoryLookup`,
/// for instance).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexEdge {
    /// The uri of the object that holds the reference.
    pub from: String,
    /// The uri of the table it names.
    pub to: String,
    /// Where in `from` the reference is written.
    pub path: String,
}

/// A node of the schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchemaNode {
    /// A table; `held` is false for a target named by a link and absent from the set.
    Table {
        /// The table's uri.
        uri: String,
        /// Its title, when held.
        title: Option<String>,
        /// Whether the set holds it.
        held: bool,
    },
    /// The objects a uri column names, grouped by qualified type.
    Objects {
        /// The qualified type, `witsml21.Wellbore`.
        qualified_type: String,
    },
    /// A source of an index edge that is not a table (a property naming a table).
    Object {
        /// The object's uri.
        uri: String,
    },
    /// A title (convention D) that names no table of the set: UDAR's `external file`.
    Unmatched {
        /// The title as written.
        label: String,
    },
}

/// How two nodes of the schema are linked.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SchemaLink {
    /// A value names a row by its key.
    Key,
    /// A value names a row by its rank.
    Rank,
    /// The table names its own rows: what makes a tree.
    SelfLoop,
    /// The target is named by its title, row by row.
    ByTitle,
    /// The values are uris of objects.
    UriValues,
    /// A reference written in an object, not in a table column.
    Index {
        /// Where the reference is written.
        path: String,
    },
}

/// One edge of the schema: every column of `from` that makes the same link to `to`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaEdge {
    /// The node it leaves (an index into [`Schema::nodes`]).
    pub from: usize,
    /// The node it reaches.
    pub to: usize,
    /// How.
    pub link: SchemaLink,
    /// The columns of `from` that make this edge, ascending (empty for an index edge).
    pub columns: Vec<usize>,
}

/// Something noticed while drawing the schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchemaNote {
    /// A title names several tables of the set: a title is not an identity, no edge is drawn.
    AmbiguousTitle {
        /// The title.
        title: String,
        /// The uris of the tables that carry it.
        tables: Vec<String>,
    },
}

/// The graph of a set of tables.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schema {
    /// Held tables (in input order), then the tables named and not held, the groups of
    /// objects, the objects that hold an index edge, and the unmatched titles.
    pub nodes: Vec<SchemaNode>,
    /// The edges, ordered by `(from, to, link)`.
    pub edges: Vec<SchemaEdge>,
    /// Held tables with no edge at all.
    pub isolated: Vec<usize>,
    /// What was noticed.
    pub notes: Vec<SchemaNote>,
}

/// Whether two addresses name the same object (uuid, and versions that do not contradict).
fn same_address(a: &str, b: &str) -> bool {
    let (uuid_a, version_a) = identity::parse_address(a);
    let (uuid_b, version_b) = identity::parse_address(b);
    uuid_a == uuid_b && identity::versions_agree(version_a.as_deref(), version_b.as_deref())
}

/// The nodes of the schema, found by address while the graph is built.
struct Nodes {
    nodes: Vec<SchemaNode>,
}

impl Nodes {
    /// The node of a table by its address, if there is one.
    fn table(&self, uri: &str) -> Option<usize> {
        self.nodes.iter().position(|node| match node {
            SchemaNode::Table { uri: held, .. } => same_address(held, uri),
            _ => false,
        })
    }

    /// The node of a table, added as a table the set does not hold when it is new.
    fn table_or_named(&mut self, uri: &str) -> usize {
        match self.table(uri) {
            Some(at) => at,
            None => {
                self.nodes.push(SchemaNode::Table {
                    uri: uri.to_string(),
                    title: None,
                    held: false,
                });
                self.nodes.len() - 1
            }
        }
    }

    fn object(&mut self, uri: &str) -> usize {
        let found = self.nodes.iter().position(|node| match node {
            SchemaNode::Object { uri: known } => same_address(known, uri),
            _ => false,
        });
        match found {
            Some(at) => at,
            None => {
                self.nodes.push(SchemaNode::Object { uri: uri.to_string() });
                self.nodes.len() - 1
            }
        }
    }

    fn position_of(&self, wanted: &SchemaNode) -> Option<usize> {
        self.nodes.iter().position(|node| node == wanted)
    }
}

/// G1: the graph of a set of tables and of the objects that point at them.
pub fn schema(tables: &[TableLinks], index_edges: &[IndexEdge]) -> Schema {
    let mut nodes = Nodes {
        nodes: tables
            .iter()
            .map(|table| SchemaNode::Table {
                uri: table.uri.clone(),
                title: table.title.clone(),
                held: true,
            })
            .collect(),
    };
    let held = tables.len();

    // The tables a link or an index edge names and the set does not hold, in the order they
    // are met.
    for table in tables {
        for declaration in &table.declarations {
            if let LinkTarget::Key { table_uri } | LinkTarget::Rank { table_uri } = &declaration.target {
                nodes.table_or_named(table_uri);
            }
        }
    }
    for edge in index_edges {
        nodes.table_or_named(&edge.to);
    }

    // The groups of objects named by uri columns, by qualified type.
    let types: BTreeSet<&str> = tables
        .iter()
        .flat_map(|table| table.uri_columns.iter())
        .flat_map(|(_, column)| column.qualified_types.iter().map(String::as_str))
        .collect();
    for qualified_type in types {
        nodes.nodes.push(SchemaNode::Objects {
            qualified_type: qualified_type.to_string(),
        });
    }

    // The sources of an index edge that are not tables.
    for edge in index_edges {
        if nodes.table(&edge.from).is_none() {
            nodes.object(&edge.from);
        }
    }

    // Titles (convention D): which table of the set carries each, and the titles that
    // carry none.
    let mut notes: Vec<SchemaNote> = Vec::new();
    let mut unmatched: BTreeSet<String> = BTreeSet::new();
    let mut by_title: Vec<(usize, usize, usize)> = Vec::new(); // (from table, column, to table)
    for (from, table) in tables.iter().enumerate() {
        for (column, titles) in &table.titles_named {
            for title in titles {
                let carriers: Vec<usize> = (0..held)
                    .filter(|at| tables[*at].title.as_deref() == Some(title.as_str()))
                    .collect();
                match carriers.as_slice() {
                    [] => {
                        unmatched.insert(title.clone());
                    }
                    [only] => by_title.push((from, *column, *only)),
                    several => {
                        let note = SchemaNote::AmbiguousTitle {
                            title: title.clone(),
                            tables: several.iter().map(|at| tables[*at].uri.clone()).collect(),
                        };
                        if !notes.contains(&note) {
                            notes.push(note);
                        }
                    }
                }
            }
        }
    }
    for label in &unmatched {
        nodes.nodes.push(SchemaNode::Unmatched { label: label.clone() });
    }

    // The edges, one per `(from, to, link)`, every column that makes it kept.
    let mut edges: BTreeMap<(usize, usize, SchemaLink), Vec<usize>> = BTreeMap::new();
    for (from, table) in tables.iter().enumerate() {
        for declaration in &table.declarations {
            let (to, link) = match &declaration.target {
                LinkTarget::Key { table_uri } => (nodes.table_or_named(table_uri), SchemaLink::Key),
                LinkTarget::Rank { table_uri } => (nodes.table_or_named(table_uri), SchemaLink::Rank),
                LinkTarget::SameTable => (from, SchemaLink::SelfLoop),
                // Drawn from the titles read, below.
                LinkTarget::ByTitleIn { .. } | LinkTarget::Objects => continue,
            };
            edges.entry((from, to, link)).or_default().push(declaration.column);
        }
        for (column, column_uris) in &table.uri_columns {
            for qualified_type in &column_uris.qualified_types {
                let wanted = SchemaNode::Objects {
                    qualified_type: qualified_type.clone(),
                };
                if let Some(to) = nodes.position_of(&wanted) {
                    edges.entry((from, to, SchemaLink::UriValues)).or_default().push(*column);
                }
            }
        }
        for (column, titles) in &table.titles_named {
            for title in titles {
                let carried = (0..held).any(|at| tables[at].title.as_deref() == Some(title.as_str()));
                if carried {
                    continue;
                }
                let wanted = SchemaNode::Unmatched { label: title.clone() };
                if let Some(to) = nodes.position_of(&wanted) {
                    edges.entry((from, to, SchemaLink::ByTitle)).or_default().push(*column);
                }
            }
        }
    }
    for (from, column, to) in by_title {
        edges.entry((from, to, SchemaLink::ByTitle)).or_default().push(column);
    }
    for edge in index_edges {
        let from = match nodes.table(&edge.from) {
            Some(at) => at,
            None => nodes.object(&edge.from),
        };
        let to = nodes.table_or_named(&edge.to);
        edges
            .entry((from, to, SchemaLink::Index { path: edge.path.clone() }))
            .or_default();
    }

    let edges: Vec<SchemaEdge> = edges
        .into_iter()
        .map(|((from, to, link), mut columns)| {
            columns.sort_unstable();
            columns.dedup();
            SchemaEdge { from, to, link, columns }
        })
        .collect();
    let touched: BTreeSet<usize> = edges.iter().flat_map(|edge| [edge.from, edge.to]).collect();
    let isolated: Vec<usize> = (0..held).filter(|at| !touched.contains(at)).collect();

    Schema {
        nodes: nodes.nodes,
        edges,
        isolated,
        notes,
    }
}

/// G2: the columns linking the table to itself.
pub fn self_parents(declarations: &[LinkDeclaration]) -> Vec<usize> {
    declarations
        .iter()
        .filter(|declaration| declaration.target == LinkTarget::SameTable)
        .map(|declaration| declaration.column)
        .collect()
}

/// The rows of a table as a forest, and what was ambiguous on the way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowForest {
    /// The forest of the rows: roots, orphans, children, depths.
    pub forest: Forest,
    /// Rows whose parent key is duplicated in the table: the first row was taken, and this
    /// says so.
    pub ambiguous: Vec<usize>,
}

/// Why the rows of a table cannot be drawn as a tree.
#[derive(Debug, thiserror::Error)]
pub enum RowTreeRefusal {
    /// The declaration does not link the table to itself.
    #[error("column {column} does not link the table to itself")]
    NotSelfReferent {
        /// The column.
        column: usize,
    },
    /// The column holding the parents cannot be read. `cause` is the refusal's text: the
    /// cells are borrowed, and a refusal is not `Clone`.
    #[error("column {column} cannot be read: {cause}")]
    ColumnUnreadable {
        /// The column.
        column: usize,
        /// Why, in words.
        cause: String,
    },
    /// A parent column of vectors names no single row.
    #[error("column {column} holds vectors, and a parent is one row")]
    VectorParent {
        /// The column.
        column: usize,
    },
    /// A table with several key columns: a value does not name a row by one key.
    #[error("the table has {count} key columns, and a parent names one row by one key")]
    SeveralKeyColumns {
        /// How many key columns.
        count: usize,
    },
    /// Rows that are their own ancestors, each cycle once. Never cut in silence.
    #[error("rows {rows:?} are their own ancestors")]
    Cycle {
        /// Each cycle, from its smallest row, following the parents.
        rows: Vec<Vec<usize>>,
    },
}

/// G3: the tree of the rows by a self-referent column (`declaration.target == SameTable`).
///
/// A parent is read by `row_named_by`: by the table's one key column, or by rank when it
/// has none. An empty or absent parent makes a root; a parent that names no row makes an
/// orphan (a root **marked**); a duplicated key takes its first row and says so
/// ([`RowForest::ambiguous`]); a cycle is refused with its rows.
pub fn row_forest(
    cells: &[Result<ColumnCells<'_>, TableRefusal>],
    index: &KeyIndex,
    declaration: &LinkDeclaration,
) -> Result<RowForest, RowTreeRefusal> {
    let column = declaration.column;
    if declaration.target != LinkTarget::SameTable {
        return Err(RowTreeRefusal::NotSelfReferent { column });
    }
    let keys = index.key_columns().len();
    if keys >= 2 {
        return Err(RowTreeRefusal::SeveralKeyColumns { count: keys });
    }
    let parents_column = match cells.get(column) {
        Some(Ok(cells)) => cells,
        Some(Err(refusal)) => {
            return Err(RowTreeRefusal::ColumnUnreadable {
                column,
                cause: refusal.to_string(),
            });
        }
        None => {
            return Err(RowTreeRefusal::ColumnUnreadable {
                column,
                cause: "the table has no such column".to_string(),
            });
        }
    };
    let rows = parents_column
        .rows("")
        .map_err(|refusal| RowTreeRefusal::ColumnUnreadable {
            column,
            cause: refusal.to_string(),
        })?;

    let mut parents = Vec::with_capacity(rows);
    let mut ambiguous = Vec::new();
    for row in 0..rows {
        let parent = match parents_column.cell(row) {
            Some(CellState::Empty) | Some(CellState::Absent) => Parent::Root,
            Some(CellState::Vector(_)) => return Err(RowTreeRefusal::VectorParent { column }),
            Some(CellState::Value(cell)) => match row_named_by(index, &cell) {
                Ok(RowMatch::One(parent)) => Parent::Of(parent),
                Ok(RowMatch::Several(found)) => {
                    ambiguous.push(row);
                    Parent::Of(found[0])
                }
                Ok(RowMatch::None) => Parent::Missing,
                Err(_) => return Err(RowTreeRefusal::SeveralKeyColumns { count: keys }),
            },
            Some(CellState::Unreadable) | None => {
                return Err(RowTreeRefusal::ColumnUnreadable {
                    column,
                    cause: format!("the cell of row {row} cannot be read"),
                });
            }
        };
        parents.push(parent);
    }

    let forest = forest(&parents);
    if !forest.cycles.is_empty() {
        return Err(RowTreeRefusal::Cycle {
            rows: forest.cycles.clone(),
        });
    }
    Ok(RowForest { forest, ambiguous })
}

/// The roots that share a value.
#[derive(Clone, Debug, PartialEq)]
pub struct RootGroup {
    /// The value of the grouping column.
    pub value: CellState,
    /// The roots, in row order.
    pub roots: Vec<usize>,
}

/// G4: the roots grouped by the value of a column, groups in order of first appearance.
/// A column that cannot be read makes a single group of `Unreadable`.
pub fn group_roots(
    forest: &Forest,
    cells: &[Result<ColumnCells<'_>, TableRefusal>],
    by: usize,
) -> Vec<RootGroup> {
    let mut groups: Vec<RootGroup> = Vec::new();
    for root in &forest.roots {
        let value = match cells.get(by) {
            Some(Ok(column)) => column.cell(*root).unwrap_or(CellState::Unreadable),
            _ => CellState::Unreadable,
        };
        // A `CellState` is not `Hash`: the list of groups is short, `==` will do.
        match groups.iter_mut().find(|group| group.value == value) {
            Some(group) => group.roots.push(*root),
            None => groups.push(RootGroup {
                value,
                roots: vec![*root],
            }),
        }
    }
    groups
}

/// The roles of the columns of a key-value table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KvShape {
    /// The key column.
    pub key: usize,
    /// The value column.
    pub value: Option<usize>,
    /// The unit column.
    pub unit: Option<usize>,
    /// The description column.
    pub description: Option<usize>,
    /// Columns whose role was read from the title, or from the position, not from the
    /// kind: said, never silent. Ascending.
    pub guessed: Vec<usize>,
}

/// Why a table is not a key-value table.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum KvRefusal {
    /// A key-value table has exactly one key column.
    #[error("a key-value table has one key column; this one has {count}")]
    KeyColumns {
        /// How many key columns.
        count: usize,
    },
    /// A column that is not text.
    #[error("column {column} is not text")]
    NotText {
        /// The column.
        column: usize,
    },
    /// More than three columns besides the key.
    #[error("a key-value table has at most three columns besides its key; this one has {count}")]
    TooManyColumns {
        /// How many columns besides the key.
        count: usize,
    },
    /// A column that declares a link.
    #[error("column {column} declares a link")]
    LinkColumn {
        /// The column.
        column: usize,
    },
    /// The table has measured-depth columns: it is a table of intervals, not of parameters.
    #[error("the table has measured-depth columns")]
    DepthColumns,
}

/// G5: one text key column and at most three other text columns, no link, no MD.
///
/// A column's role comes from its kind when the kind says (`description`); otherwise from
/// its title (`description`, `unit`, `uom`) or from its place (the first column left is the
/// value). Those are **guesses**, and each is listed in [`KvShape::guessed`].
pub fn kv_shape(
    sheet: &TableSheet,
    declarations: &[LinkDeclaration],
    md: &IntervalColumns,
    kinds: &[Option<Resolution>],
) -> Result<KvShape, KvRefusal> {
    if !matches!(md, IntervalColumns::None) {
        return Err(KvRefusal::DepthColumns);
    }
    let keys = sheet.key_columns();
    if keys.len() != 1 {
        return Err(KvRefusal::KeyColumns { count: keys.len() });
    }
    let key = keys[0];
    if let Some(declaration) = declarations
        .iter()
        .find(|declaration| declaration.column < sheet.columns.len())
    {
        return Err(KvRefusal::LinkColumn {
            column: declaration.column,
        });
    }
    for (at, column) in sheet.columns.iter().enumerate() {
        if column.family != Some(Family::Text) {
            return Err(KvRefusal::NotText { column: at });
        }
    }
    let others: Vec<usize> = (0..sheet.columns.len()).filter(|at| *at != key).collect();
    if others.len() > 3 {
        return Err(KvRefusal::TooManyColumns { count: others.len() });
    }

    let mut description = None;
    let mut unit = None;
    let mut guessed: Vec<usize> = Vec::new();
    let mut remaining: Vec<usize> = Vec::new();
    for at in others {
        let by_kind = kinds
            .get(at)
            .and_then(|kind| kind.as_ref())
            .is_some_and(|kind| descends_from(kind, DESCRIPTION));
        let title = sheet.columns[at]
            .title
            .as_deref()
            .map(|title| title.trim().to_lowercase())
            .unwrap_or_default();
        if by_kind && description.is_none() {
            description = Some(at);
        } else if title == "description" && description.is_none() {
            description = Some(at);
            guessed.push(at);
        } else if (title == "unit" || title == "uom") && unit.is_none() {
            unit = Some(at);
            guessed.push(at);
        } else {
            remaining.push(at);
        }
    }
    // Nothing says which column holds the value: the first one left, by position.
    let value = remaining.first().copied();
    if let Some(at) = value {
        guessed.push(at);
    }
    guessed.sort_unstable();
    guessed.dedup();

    Ok(KvShape {
        key,
        value,
        unit,
        description,
        guessed,
    })
}
