//! `table-schema`: how the tables of a package hold together.
//!
//! One box per table (its title, its columns with the key marked), one arrow per link: a value names a row by its
//! key or by its rank, a table names its own rows, a table is named row by row by its title, a column holds the uris
//! of objects, or an object of the package names the table (a `CategoryLookup`). The links are what the columns say
//! (`energyml_domain::table::LinkReaders`, the conventions of the registry); the graph is
//! `energyml_domain::table_structure::schema`; the placement is `energyml_sheet::draw::layered`. This module reads
//! the sheets of the index, hands them over, and lays out what comes back through [`crate::graph`].

use std::collections::{BTreeMap, BTreeSet, HashMap};

use energyml_domain::package_index::PackageIndex;
use energyml_domain::property_kind::Resolver;
use energyml_domain::table::{
    Cell, CellState, LinkDeclaration, LinkReaders, LinkTarget, Table, TableSheet, UriColumn, resolve_kinds,
    uri_column,
};
use energyml_domain::table_structure::{
    IndexEdge, Schema, SchemaLink, SchemaNode, SchemaNote, TableLinks, schema,
};
use energyml_sheet::draw::layered::LayerInput;
use serde_json::Value;

use crate::graph::{self, BoxLook, EdgeLook, Frame, LegendEntry, Line, Tone};
use crate::kind::{
    Drawn, ObjectSlot, Offer, Request, ViewContext, ViewKind, ViewNote, column_name, local_type, table_name,
};
use crate::refusal::ViewRefusal;
use crate::setting::ViewSetting;
use crate::style;

/// The id of the view.
pub const ID: &str = "table-schema";

/// The types it takes: a column-based table, and the two lookups of RESQML 2.0.1.
pub const TYPES: &[&str] = &["ColumnBasedTable", "StringTableLookup", "DoubleTableLookup"];

/// The fields of an object that name a table to look up: the references the schema draws from the index.
pub const LOOKUP_FIELDS: &[&str] = &["CategoryLookup", "Lookup"];

/// The most columns a box lists; the others are counted.
const MOST_COLUMNS: usize = 10;

/// The most tables a request names.
const MOST_TABLES: usize = 1_000;

/// The view.
pub struct TableSchemaView;

impl ViewKind for TableSchemaView {
    fn id(&self) -> &'static str {
        ID
    }

    fn name(&self) -> &'static str {
        "Table schema"
    }

    fn about(&self) -> &'static str {
        "how the tables of a package hold together: a box per table with its key column marked, an arrow per link (key, rank, own rows, title, object uris, lookup of a property)"
    }

    fn objects(&self) -> Vec<ObjectSlot> {
        vec![ObjectSlot {
            role: "tables",
            types: TYPES,
            least: 0,
            most: MOST_TABLES,
            about: "the tables to show; none: every table of the package",
        }]
    }

    fn settings(&self) -> Vec<ViewSetting> {
        let mut settings = graph::layout_settings();
        settings.push(
            ViewSetting::flag(
                "columns",
                "list the columns of a table in its box, the key columns marked",
            )
            .default(Value::Bool(true)),
        );
        settings.push(
            ViewSetting::flag(
                "values",
                "read the columns to find the columns of object uris and the tables a column names by title (reads arrays); off: only what the sheets say",
            )
            .default(Value::Bool(true)),
        );
        settings
    }

    fn size(&self) -> [f64; 2] {
        [900.0, 600.0]
    }

    fn offers(&self, index: &PackageIndex) -> Vec<Offer> {
        let tables = index.tables().count();
        if tables == 0 {
            return Vec::new();
        }
        vec![Offer {
            view: ID,
            objects: Vec::new(),
            title: format!("the schema of the {tables} tables of the package"),
            missing: Vec::new(),
            proposed: Vec::new(),
            part: None,
        }]
    }

    fn draw(&self, request: &Request, cx: &ViewContext<'_>) -> Result<Drawn, ViewRefusal> {
        let refuse = |why: String| ViewRefusal::computing(ID, why);
        let settings = &request.settings;
        let mut notes: Vec<ViewNote> = Vec::new();

        // The tables: those asked for, or every table the index holds, in the order of their uris.
        let asked = request.objects_of("tables");
        let mut sheets: Vec<&TableSheet> = Vec::new();
        if asked.is_empty() {
            let mut all: Vec<(&str, &TableSheet)> = cx.index.tables().collect();
            all.sort_by(|a, b| a.0.cmp(b.0));
            sheets.extend(all.into_iter().map(|(_, sheet)| sheet));
        } else {
            for uri in asked {
                let sheet = cx
                    .index
                    .table_sheet(uri)
                    .ok_or_else(|| refuse(format!("`{uri}` has no sheet: the table does not read")))?;
                if !sheets.iter().any(|held| held.uri == sheet.uri) {
                    sheets.push(sheet);
                }
            }
        }
        if sheets.is_empty() {
            return Err(refuse("the package holds no table".to_string()));
        }

        // What each table says of its links.
        let readers = LinkReaders::builtin();
        let resolver = Resolver::new(cx.source);
        let read_values = settings.flag("values");
        let mut links: Vec<TableLinks> = Vec::new();
        for sheet in &sheets {
            let kinds = resolve_kinds(sheet, &resolver);
            let mut declarations = Vec::new();
            let mut refused = Vec::new();
            for (column, found) in readers.declarations(sheet, &kinds) {
                match found {
                    Ok(declaration) => declarations.push(declaration),
                    Err(why) => refused.push((column, why)),
                }
            }
            let name = table_name(sheet);
            for (column, why) in &refused {
                notes.push(ViewNote {
                    key: "link-refused",
                    says: format!("table `{name}`, {}: {why}", column_name(sheet, *column)),
                });
            }
            let mut found = Values::default();
            if read_values {
                match read_columns(cx, sheet, &declarations) {
                    Ok(read) => {
                        if read.unread > 0 {
                            notes.push(ViewNote {
                                key: "columns-not-read",
                                says: format!(
                                    "table `{name}`: {} columns could not be read, so the object uris and the titles they hold are not drawn",
                                    read.unread
                                ),
                            });
                        }
                        found = read;
                    }
                    Err(why) => notes.push(ViewNote {
                        key: "table-values-not-read",
                        says: format!("table `{name}`: its values were not read ({why})"),
                    }),
                }
            }
            links.push(TableLinks {
                uri: sheet.uri.to_string(),
                title: sheet.title.as_deref().map(str::to_string),
                declarations,
                refused,
                uri_columns: found.uri_columns,
                titles_named: found.titles_named,
            });
        }

        // What objects say of tables: the lookups of the properties, from the index.
        let mut index_edges: Vec<IndexEdge> = Vec::new();
        for sheet in &sheets {
            // The full uri: two versions of one table are two tables.
            for pointer in cx.index.dependents_of(&sheet.uri) {
                let field = pointer.path.rsplit('.').next().unwrap_or(&pointer.path);
                let field = field.split('[').next().unwrap_or(field);
                if LOOKUP_FIELDS.contains(&field) {
                    index_edges.push(IndexEdge {
                        from: pointer.uri.clone(),
                        to: sheet.uri.to_string(),
                        path: pointer.path.clone(),
                    });
                }
            }
        }

        let found = schema(&links, &index_edges);
        for note in &found.notes {
            let SchemaNote::AmbiguousTitle { title, tables } = note;
            notes.push(ViewNote {
                key: "ambiguous-title",
                says: format!(
                    "`{title}` is the title of {} tables: a title is not an identity, no arrow is drawn to it",
                    tables.len()
                ),
            });
        }

        // The boxes, then the arrows.
        let (boxes, drawn_of) = boxes_of(cx, &found, &sheets, settings.flag("columns"));
        let (ends, looks) = edges_of(&found, &sheets, &boxes, &drawn_of);
        let input = LayerInput {
            nodes: boxes.iter().map(BoxLook::spec).collect(),
            edges: ends,
            fixed_layer: Vec::new(),
            pins: Vec::new(),
        };
        let placed = graph::place(ID, &input, &graph::layered_options(settings, None))?;
        let legend = legend_of(&looks);
        let list = graph::draw(
            ID,
            &format!("schema of {} tables", sheets.len()),
            &placed,
            &boxes,
            &looks.iter().map(|spec| spec.look.clone()).collect::<Vec<_>>(),
            &legend,
        )?;
        Ok(Drawn {
            list,
            notes: graph::capped(notes),
        })
    }
}

/// What reading the columns of a table gives the schema.
#[derive(Default)]
struct Values {
    uri_columns: Vec<(usize, UriColumn)>,
    titles_named: Vec<(usize, Vec<String>)>,
    /// How many columns would not read.
    unread: usize,
}

/// The columns of object uris, and the titles each column that names a table by title holds: the two things the
/// sheets cannot say, read by the library's own readers.
fn read_columns(
    cx: &ViewContext<'_>,
    sheet: &TableSheet,
    declarations: &[LinkDeclaration],
) -> Result<Values, String> {
    let object = cx.source.read_object_at(&sheet.uri)?;
    let table = Table::read(object.as_reflect()).map_err(|why| why.to_string())?;
    let cells = table.read_cells(cx.source);
    let mut found = Values {
        unread: cells.iter().filter(|read| read.is_err()).count(),
        ..Values::default()
    };
    let declared: BTreeSet<usize> = declarations
        .iter()
        .map(|declaration| declaration.column)
        .collect();
    for (column, read) in cells.iter().enumerate() {
        let Ok(read) = read else { continue };
        if declared.contains(&column) {
            continue;
        }
        if let Some(uris) = uri_column(read) {
            found.uri_columns.push((column, uris));
        }
    }
    for declaration in declarations {
        let LinkTarget::ByTitleIn { column } = declaration.target else {
            continue;
        };
        let Some(Ok(sibling)) = cells.get(column) else {
            continue;
        };
        let Ok(rows) = sibling.rows("") else { continue };
        let mut titles: BTreeSet<String> = BTreeSet::new();
        for row in 0..rows {
            if let Some(CellState::Value(Cell::Text(text))) = sibling.cell(row) {
                titles.insert(text);
            }
        }
        found
            .titles_named
            .push((declaration.column, titles.into_iter().collect()));
    }
    Ok(found)
}

/// The box of a table: what it is, its name, its columns.
fn table_box(sheet: &TableSheet, columns: bool) -> BoxLook {
    let mut lines = vec![
        Line::new(local_type(&sheet.qualified_type), Tone::Caption),
        Line::new(table_name(sheet), Tone::Body),
    ];
    if columns {
        for (at, column) in sheet.columns.iter().enumerate().take(MOST_COLUMNS) {
            let name = column_name(sheet, at);
            lines.push(if column.key {
                Line::new(format!("[key] {name}"), Tone::Key)
            } else {
                Line::new(name, Tone::Muted)
            });
        }
        if sheet.columns.len() > MOST_COLUMNS {
            lines.push(Line::new(
                format!("... and {} more columns", sheet.columns.len() - MOST_COLUMNS),
                Tone::Muted,
            ));
        }
    }
    let mut look = BoxLook::new(sheet.uri.to_string(), "table", lines);
    look.tip = format!("{} ({} columns)", sheet.uri, sheet.columns.len());
    look.data.push(("family".to_string(), "table".to_string()));
    look
}

/// A word a column list is written with: the titles of the columns of an edge, joined.
fn columns_said(sheet: Option<&TableSheet>, columns: &[usize]) -> String {
    match sheet {
        Some(sheet) => columns
            .iter()
            .map(|column| column_name(sheet, *column))
            .collect::<Vec<_>>()
            .join(", "),
        None => columns
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// The boxes, one per node of the schema (the objects that name a table grouped by type), and the box each node of
/// the schema is drawn as.
fn boxes_of(
    cx: &ViewContext<'_>,
    found: &Schema,
    sheets: &[&TableSheet],
    columns: bool,
) -> (Vec<BoxLook>, Vec<usize>) {
    let mut boxes: Vec<BoxLook> = Vec::new();
    let mut drawn_of: Vec<usize> = vec![usize::MAX; found.nodes.len()];
    // The objects that name a table, by their type: one box says how many.
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (at, node) in found.nodes.iter().enumerate() {
        match node {
            SchemaNode::Table { uri, held, .. } => {
                let look = match (*held, sheets.get(at)) {
                    (true, Some(sheet)) => table_box(sheet, columns),
                    _ => {
                        let (uuid, _) = energyml_core::identity::parse_address(uri);
                        let mut look = BoxLook::new(
                            uri.clone(),
                            "table",
                            vec![
                                Line::new("table not in the set", Tone::Caption),
                                Line::new(uuid, Tone::Body),
                            ],
                        );
                        look.frame = Frame::Ghost;
                        look.tip = uri.clone();
                        look.data.push(("family".to_string(), "table".to_string()));
                        look
                    }
                };
                drawn_of[at] = boxes.len();
                boxes.push(look);
            }
            SchemaNode::Objects { qualified_type } => {
                let mut look = BoxLook::new(
                    format!("objects:{qualified_type}"),
                    "objects",
                    vec![
                        Line::new("objects named by uri", Tone::Caption),
                        Line::new(qualified_type.clone(), Tone::Body),
                    ],
                );
                look.tip = format!("the objects a column of uris names, of type {qualified_type}");
                look.data.push(("family".to_string(), "objects".to_string()));
                drawn_of[at] = boxes.len();
                boxes.push(look);
            }
            SchemaNode::Unmatched { label } => {
                let mut look = BoxLook::new(
                    format!("unmatched:{label}"),
                    "unmatched",
                    vec![
                        Line::new("no table of this title", Tone::Caption),
                        Line::new(label.clone(), Tone::Body),
                    ],
                );
                look.frame = Frame::Ghost;
                look.tip = format!("a column names `{label}`, and no table of the set has that title");
                look.data.push(("family".to_string(), "unmatched".to_string()));
                drawn_of[at] = boxes.len();
                boxes.push(look);
            }
            SchemaNode::Object { uri } => {
                let qualified_type = cx
                    .index
                    .node(uri)
                    .map(|held| held.qualified_type.to_string())
                    .filter(|text| !text.is_empty())
                    .unwrap_or_else(|| "object".to_string());
                groups.entry(qualified_type).or_default().push(at);
            }
        }
    }
    for (qualified_type, members) in groups {
        let count = members.len();
        let single = match (count, members.first().map(|at| &found.nodes[*at])) {
            (1, Some(SchemaNode::Object { uri })) => Some(uri.clone()),
            _ => None,
        };
        let name = match &single {
            Some(uri) => cx
                .index
                .node(uri)
                .and_then(|held| held.title.as_deref().map(str::to_string))
                .unwrap_or_else(|| uri.clone()),
            None => format!("{count} x {}", local_type(&qualified_type)),
        };
        let mut look = BoxLook::new(
            single
                .clone()
                .unwrap_or_else(|| format!("index:{qualified_type}")),
            "objects",
            vec![
                Line::new(
                    format!("{} that names a table", local_type(&qualified_type)),
                    Tone::Caption,
                ),
                Line::new(name, Tone::Body),
            ],
        );
        look.tip = format!("{count} objects of type {qualified_type} hold a reference to a table");
        look.data.push(("family".to_string(), "objects".to_string()));
        for at in members {
            drawn_of[at] = boxes.len();
        }
        boxes.push(look);
    }
    for at in &found.isolated {
        if let Some(look) = drawn_of.get(*at).and_then(|drawn| boxes.get_mut(*drawn)) {
            look.data.push(("isolated".to_string(), "true".to_string()));
        }
    }
    (boxes, drawn_of)
}

/// One edge as drawn, with the way it is told in the legend.
struct EdgeSpec {
    word: &'static str,
    look: EdgeLook,
}

/// The word of a link, in the data of its element and the legend.
fn link_word(link: &SchemaLink) -> &'static str {
    match link {
        SchemaLink::Key => "key",
        SchemaLink::Rank => "rank",
        SchemaLink::SelfLoop => "self",
        SchemaLink::ByTitle => "title",
        SchemaLink::UriValues => "uris",
        SchemaLink::Index { .. } => "index",
    }
}

/// The links in the order the legend lists them, with what each says.
const LINKS: &[(&str, &str)] = &[
    ("key", "a value names a row by its key"),
    ("rank", "a value names a row by its rank"),
    ("self", "a table names its own rows"),
    ("title", "a table is named row by row, by its title"),
    ("uris", "a column holds the uris of objects"),
    ("index", "an object of the package names the table"),
];

fn link_stroke(link: &SchemaLink) -> (energyml_sheet::draw::Colour, Vec<f64>, f64) {
    match link {
        SchemaLink::Key => (style::INK, Vec::new(), 1.5),
        SchemaLink::Rank => (style::INK, vec![6.0, 3.0], 1.5),
        SchemaLink::SelfLoop => (style::INK, Vec::new(), 1.5),
        SchemaLink::ByTitle => (style::HIGHLIGHT, Vec::new(), 1.5),
        SchemaLink::UriValues => (style::SUBJECT, Vec::new(), 1.5),
        SchemaLink::Index { .. } => (style::NEUTRAL, Vec::new(), 1.2),
    }
}

/// The edges of the schema as the placement takes them (the two boxes each joins) and as they are drawn. An edge
/// from an object that names a table is one per box group and field: its label counts the objects.
fn edges_of(
    found: &Schema,
    sheets: &[&TableSheet],
    boxes: &[BoxLook],
    drawn_of: &[usize],
) -> (Vec<(usize, usize)>, Vec<EdgeSpec>) {
    let mut ends: Vec<(usize, usize)> = Vec::new();
    let mut specs: Vec<EdgeSpec> = Vec::new();
    // (from box, to box, field) of an edge from objects, to its place in `specs` and how many objects make it.
    let mut grouped: HashMap<(usize, usize, String), (usize, usize)> = HashMap::new();
    for edge in &found.edges {
        let (from, to) = (drawn_of[edge.from], drawn_of[edge.to]);
        let word = link_word(&edge.link);
        let (colour, dash, width) = link_stroke(&edge.link);
        let source_sheet = sheets.get(edge.from).copied();
        if let (SchemaLink::Index { path }, SchemaNode::Object { .. }) = (&edge.link, &found.nodes[edge.from])
        {
            if let Some((place, count)) = grouped.get_mut(&(from, to, path.clone())) {
                *count += 1;
                specs[*place].look.label = Some(format!("{path} ({count})"));
                continue;
            }
            grouped.insert((from, to, path.clone()), (specs.len(), 1));
        }
        let label = match &edge.link {
            SchemaLink::Key => format!("key: {}", columns_said(source_sheet, &edge.columns)),
            SchemaLink::Rank => format!("rank: {}", columns_said(source_sheet, &edge.columns)),
            SchemaLink::SelfLoop => format!("parent: {}", columns_said(source_sheet, &edge.columns)),
            SchemaLink::ByTitle => format!("by title: {}", columns_said(source_sheet, &edge.columns)),
            SchemaLink::UriValues => format!("uris: {}", columns_said(source_sheet, &edge.columns)),
            SchemaLink::Index { path } => path.clone(),
        };
        let link_key = match &edge.link {
            SchemaLink::Index { path } => format!("index:{path}"),
            other => link_word(other).to_string(),
        };
        let mut data = vec![("link".to_string(), word.to_string())];
        if !edge.columns.is_empty() {
            data.push((
                "columns".to_string(),
                edge.columns
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            ));
        }
        ends.push((from, to));
        specs.push(EdgeSpec {
            word,
            look: EdgeLook {
                key: format!("{}>{}#{link_key}", boxes[from].key, boxes[to].key),
                tip: format!(
                    "{}: {label}",
                    LINKS
                        .iter()
                        .find(|(w, _)| *w == word)
                        .map_or(word, |(_, says)| *says)
                ),
                colour,
                width,
                dash,
                label: Some(label),
                data,
            },
        });
    }
    (ends, specs)
}

/// The legend: the kinds of link there are.
fn legend_of(specs: &[EdgeSpec]) -> Vec<LegendEntry> {
    let mut legend = Vec::new();
    for (word, says) in LINKS {
        if let Some(spec) = specs.iter().find(|spec| spec.word == *word) {
            legend.push(LegendEntry {
                label: (*says).to_string(),
                colour: spec.look.colour,
                width: spec.look.width,
                dash: spec.look.dash.clone(),
            });
        }
    }
    legend
}
