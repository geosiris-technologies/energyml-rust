//! How tables hold together, `energyml_domain::table_structure` (WP06 §4.12).
//!
//! **The form comes from the specification, the values are the test's own.** Common TRG 2.3
//! §9.1 p. 190: a `KeyColumn` makes a table a lookup table, and nothing in a column names
//! another table: the links are *our* reading (ADR 0030), so the structure is tested on
//! declarations written by hand, never on a package. A cycle of rows, an orphan, an empty
//! parent, a duplicated key: each is a case of 07 §2.3 (*"never cut in silence"*), and the
//! expected answer is written in the test.
//!
//! The last line of each comment says how to make the test fail.

use std::borrow::Cow;
use std::sync::Arc;

use energyml_domain::source::NoObjects;
use energyml_domain::table::{
    CellState, ColumnCells, ColumnSheet, Convention, IntervalColumns, KeyIndex, LinkDeclaration, LinkTarget,
    Lookup, TableRefusal, TableSheet, UriColumn,
};
use energyml_domain::table_structure::{
    IndexEdge, KvRefusal, KvShape, RowTreeRefusal, SchemaLink, SchemaNode, TableLinks, group_roots, kv_shape,
    row_forest, schema, self_parents,
};
use energyml_domain::values::{Family, Values};
use energyml_eml_2_3 as eml23;

type Cells = Vec<Result<ColumnCells<'static>, TableRefusal>>;

// -- building blocks ---------------------------------------------------------

fn table_uri(n: u8) -> String {
    format!("eml:///eml23.ColumnBasedTable(00000000-0000-4000-8000-0000000000{n:02})")
}

fn declaration(column: usize, target: LinkTarget) -> LinkDeclaration {
    LinkDeclaration {
        column,
        target,
        convention: Convention::KindAndFacet,
        warnings: Vec::new(),
        key_named: None,
    }
}

fn links(n: u8, title: &str, declarations: Vec<LinkDeclaration>) -> TableLinks {
    TableLinks {
        uri: table_uri(n),
        title: Some(title.to_string()),
        declarations,
        refused: Vec::new(),
        uri_columns: Vec::new(),
        titles_named: Vec::new(),
    }
}

fn texts(values: &[&str]) -> Result<ColumnCells<'static>, TableRefusal> {
    Ok(ColumnCells {
        values: Cow::Owned(Values::Text(values.iter().map(|text| text.to_string()).collect())),
        per_row: 1,
        null: None,
    })
}

fn string_column(title: &str, values: &[&str]) -> eml23::Column {
    eml23::Column {
        title: Some(title.to_string()),
        value_count_per_row: 1,
        values: Some(eml23::AbstractValueArray::StringXmlArray(eml23::StringXmlArray {
            values: values.iter().map(|text| text.to_string()).collect(),
            count_per_value: 1,
            ..Default::default()
        })),
        ..Default::default()
    }
}

fn read(table: eml23::ColumnBasedTable) -> Lookup {
    Lookup::read(&table, &NoObjects).expect("a table that reads")
}

fn citation() -> eml23::Citation {
    eml23::Citation {
        title: "a table".to_string(),
        originator: "test".to_string(),
        creation: "2026-11-14T00:00:00Z".to_string(),
        format: "test".to_string(),
        ..Default::default()
    }
}

/// The key index of a table whose one `KeyColumn` holds these keys.
fn keyed(keys: &[&str]) -> KeyIndex {
    read(eml23::ColumnBasedTable {
        uuid: "00000000-0000-4000-8000-0000000000aa".to_string(),
        citation: citation(),
        key_column: vec![string_column("uid", keys)],
        column: vec![string_column("parent_uid", keys)],
        ..Default::default()
    })
    .key_index()
}

/// The key index of a table with no `KeyColumn`: its rows are named by rank.
fn keyless(rows: usize) -> KeyIndex {
    let values: Vec<String> = (0..rows).map(|at| at.to_string()).collect();
    let values: Vec<&str> = values.iter().map(String::as_str).collect();
    read(eml23::ColumnBasedTable {
        uuid: "00000000-0000-4000-8000-0000000000bb".to_string(),
        citation: citation(),
        column: vec![string_column("label", &values)],
        ..Default::default()
    })
    .key_index()
}

fn sheet_column(title: &str, key: bool, family: Family) -> ColumnSheet {
    ColumnSheet {
        title: Some(Arc::from(title)),
        description: None,
        key,
        kind: None,
        uom: None,
        values_per_row: 1,
        facets: Box::new([]),
        family: Some(family),
        values: Some(3),
        external: false,
    }
}

fn sheet(columns: Vec<ColumnSheet>) -> TableSheet {
    TableSheet {
        uri: table_uri(1).into(),
        title: None,
        qualified_type: Arc::from("eml23.ColumnBasedTable"),
        columns: columns.into_boxed_slice(),
    }
}

// -- the schema --------------------------------------------------------------

#[test]
fn a_schema_groups_edges_and_keeps_isolated_tables() {
    // A (table 0) links to B (table 1) by two key columns; C (table 2) links to nothing.
    let a = links(
        1,
        "a",
        vec![
            declaration(
                1,
                LinkTarget::Key {
                    table_uri: table_uri(2),
                },
            ),
            declaration(
                2,
                LinkTarget::Key {
                    table_uri: table_uri(2),
                },
            ),
        ],
    );
    let found = schema(&[a, links(2, "b", vec![]), links(3, "c", vec![])], &[]);
    assert_eq!(
        found.edges.len(),
        1,
        "one edge per (from, to, link), not one per column"
    );
    let edge = &found.edges[0];
    assert_eq!((edge.from, edge.to, edge.link.clone()), (0, 1, SchemaLink::Key));
    assert_eq!(edge.columns, vec![1, 2]);
    assert_eq!(found.isolated, vec![2]);
    // Break it: one edge per column, and `edges.len()` is 2.
}

#[test]
fn a_target_the_set_does_not_hold_is_a_node_marked_not_held() {
    let a = links(
        1,
        "a",
        vec![declaration(
            1,
            LinkTarget::Rank {
                table_uri: table_uri(9),
            },
        )],
    );
    let found = schema(&[a], &[]);
    assert_eq!(found.nodes.len(), 2);
    assert!(matches!(&found.nodes[1], SchemaNode::Table { held: false, .. }));
    assert_eq!(found.edges[0].link, SchemaLink::Rank);
    // Break it: drop the node of a table the set does not hold, and the edge points nowhere.
}

#[test]
fn a_self_reference_is_a_loop_and_a_tree_column() {
    let own = declaration(2, LinkTarget::SameTable);
    let found = schema(&[links(1, "a", vec![own.clone()])], &[]);
    assert_eq!(found.edges.len(), 1);
    assert_eq!((found.edges[0].from, found.edges[0].to), (0, 0));
    assert_eq!(found.edges[0].link, SchemaLink::SelfLoop);
    assert_eq!(self_parents(&[own, declaration(0, LinkTarget::Objects)]), vec![2]);
    // Break it: make a self reference an edge to another table.
}

#[test]
fn a_title_naming_no_table_is_an_unmatched_node() {
    let mut source = links(
        1,
        "source",
        vec![declaration(3, LinkTarget::ByTitleIn { column: 4 })],
    );
    source.titles_named = vec![(3, vec!["NWDF indexes".to_string(), "external file".to_string()])];
    let found = schema(&[source, links(2, "NWDF indexes", vec![])], &[]);
    let by_title: Vec<_> = found
        .edges
        .iter()
        .filter(|edge| edge.link == SchemaLink::ByTitle)
        .collect();
    assert_eq!(by_title.len(), 2);
    let reached = |to: usize| &found.nodes[to];
    assert!(
        by_title
            .iter()
            .any(|edge| matches!(reached(edge.to), SchemaNode::Table { held: true, .. }))
    );
    assert!(by_title.iter().any(|edge| {
        matches!(reached(edge.to), SchemaNode::Unmatched { label } if label == "external file")
    }));
    // Break it: attach "external file" to the nearest title.
}

#[test]
fn a_title_carried_by_two_tables_is_a_note_and_draws_no_edge() {
    let mut source = links(1, "source", vec![]);
    source.titles_named = vec![(3, vec!["same".to_string()])];
    let found = schema(&[source, links(2, "same", vec![]), links(3, "same", vec![])], &[]);
    assert_eq!(found.notes.len(), 1, "a title is not an identity");
    assert!(found.edges.is_empty());
    // Break it: link the first table of that title.
}

#[test]
fn dictionaries_have_index_edges_and_no_table_arrow() {
    // Lutetia: dictionaries tied by `CategoryLookup`, not by a column.
    let tables = [
        links(1, "lithology", vec![]),
        links(2, "facies", vec![]),
        links(3, "unused", vec![]),
    ];
    let edges = [
        IndexEdge {
            from: "eml:///resqml22.DiscreteProperty(00000000-0000-4000-8000-0000000000e1)".to_string(),
            to: table_uri(1),
            path: "CategoryLookup".to_string(),
        },
        IndexEdge {
            from: "eml:///resqml22.DiscreteProperty(00000000-0000-4000-8000-0000000000e2)".to_string(),
            to: table_uri(2),
            path: "CategoryLookup".to_string(),
        },
    ];
    let found = schema(&tables, &edges);
    assert_eq!(found.edges.len(), 2);
    assert!(
        found
            .edges
            .iter()
            .all(|edge| matches!(edge.link, SchemaLink::Index { .. }))
    );
    assert!(found.edges.iter().all(|edge| edge.columns.is_empty()));
    assert!(
        found
            .nodes
            .iter()
            .filter(|node| matches!(node, SchemaNode::Object { .. }))
            .count()
            == 2,
        "the properties are nodes, and no arrow joins two tables"
    );
    assert_eq!(found.isolated, vec![2], "the table nothing names is isolated");
    // Break it: create an edge between tables from an index edge.
}

#[test]
fn objects_named_by_uri_are_grouped_by_qualified_type() {
    let mut table = links(1, "runs", vec![]);
    table.uri_columns = vec![(
        2,
        UriColumn {
            uris: 3,
            empty: 0,
            qualified_types: vec!["eml23.Activity".to_string(), "witsml21.Wellbore".to_string()],
        },
    )];
    let found = schema(&[table], &[]);
    let groups: Vec<&str> = found
        .nodes
        .iter()
        .filter_map(|node| match node {
            SchemaNode::Objects { qualified_type } => Some(qualified_type.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        groups,
        vec!["eml23.Activity", "witsml21.Wellbore"],
        "sorted by type"
    );
    assert_eq!(found.edges.len(), 2);
    assert!(
        found
            .edges
            .iter()
            .all(|edge| edge.link == SchemaLink::UriValues && edge.columns == vec![2])
    );
    // Break it: one node per uri, and the groups are no longer two.
}

// -- the tree of the rows ----------------------------------------------------

fn parent_link() -> LinkDeclaration {
    declaration(1, LinkTarget::SameTable)
}

#[test]
fn a_cycle_of_rows_is_refused_with_its_rows() {
    // a -> b -> a: each row's parent is the other.
    let cells: Cells = vec![texts(&["a", "b"]), texts(&["b", "a"])];
    let refused = row_forest(&cells, &keyed(&["a", "b"]), &parent_link());
    match refused {
        Err(RowTreeRefusal::Cycle { rows }) => assert_eq!(rows, vec![vec![0, 1]]),
        other => panic!("expected the cycle to be refused, got {other:?}"),
    }
    // Break it: cut the cycle at its first row, and the tree comes back.
}

#[test]
fn an_orphan_parent_is_a_marked_root_of_the_row_tree() {
    let cells: Cells = vec![texts(&["a", "b"]), texts(&["", "zz"])];
    let tree = row_forest(&cells, &keyed(&["a", "b"]), &parent_link()).expect("a tree");
    assert_eq!(tree.forest.orphans, vec![1], "`zz` names no row");
    assert_eq!(tree.forest.roots, vec![0]);
    // Break it: make the orphan a root.
}

#[test]
fn an_empty_parent_is_a_root() {
    // S16 / T2: an empty string is a value, and it names no row: the row is a root.
    let cells: Cells = vec![texts(&["a", "b"]), texts(&["", "a"])];
    let tree = row_forest(&cells, &keyed(&["a", "b"]), &parent_link()).expect("a tree");
    assert_eq!(tree.forest.roots, vec![0]);
    assert!(
        tree.forest.orphans.is_empty(),
        "an empty parent is not a missing one"
    );
    assert_eq!(tree.forest.children[0], vec![1]);
    // Break it: read an empty parent as `Missing`, and the row is an orphan.
}

#[test]
fn a_duplicated_parent_key_takes_the_first_row_and_says_so() {
    let cells: Cells = vec![texts(&["a", "a", "b"]), texts(&["", "", "a"])];
    let tree = row_forest(&cells, &keyed(&["a", "a", "b"]), &parent_link()).expect("a tree");
    assert_eq!(tree.ambiguous, vec![2]);
    assert_eq!(
        tree.forest.children[0],
        vec![2],
        "the first row holding `a` is the parent"
    );
    // Break it: take the last row, or leave `ambiguous` empty.
}

#[test]
fn a_keyless_table_reads_its_parents_by_rank() {
    // No KeyColumn: a parent is a rank from 0 (house/table/row-rank-without-key). -1 is the
    // array's NullValue: the row has no parent.
    let parents = Ok(ColumnCells {
        values: Cow::Owned(Values::Integer(vec![-1, 0, 0])),
        per_row: 1,
        null: Some(-1),
    });
    let cells: Cells = vec![texts(&["x", "y", "z"]), parents];
    let tree = row_forest(&cells, &keyless(3), &parent_link()).expect("a tree");
    assert_eq!(tree.forest.roots, vec![0], "an absent parent is a root");
    assert_eq!(tree.forest.children[0], vec![1, 2]);
    // Break it: read a parent as a key on a table that has none, and every row is an orphan.
}

#[test]
fn a_column_that_is_not_self_referent_refuses_the_tree() {
    let cells: Cells = vec![texts(&["a"]), texts(&["a"])];
    let elsewhere = declaration(
        1,
        LinkTarget::Key {
            table_uri: table_uri(7),
        },
    );
    assert!(matches!(
        row_forest(&cells, &keyed(&["a"]), &elsewhere),
        Err(RowTreeRefusal::NotSelfReferent { column: 1 })
    ));
    // Break it: build the tree anyway.
}

#[test]
fn an_unreadable_parent_column_is_named() {
    let cells: Cells = vec![texts(&["a"]), Err(TableRefusal::NothingReadable)];
    assert!(matches!(
        row_forest(&cells, &keyed(&["a"]), &parent_link()),
        Err(RowTreeRefusal::ColumnUnreadable { column: 1, .. })
    ));
    // Break it: treat an unreadable column as all roots.
}

#[test]
fn roots_are_grouped_by_a_column_in_order_of_first_appearance() {
    let cells: Cells = vec![
        texts(&["a", "b", "c", "d"]),
        texts(&["", "a", "", ""]),
        texts(&["x", "x", "y", "x"]),
    ];
    let tree = row_forest(&cells, &keyed(&["a", "b", "c", "d"]), &parent_link()).expect("a tree");
    let groups = group_roots(&tree.forest, &cells, 2);
    assert_eq!(groups.len(), 2);
    assert_eq!(
        groups[0].roots,
        vec![0, 3],
        "rows 0 and 3 are roots under `x`; row 1 is not a root"
    );
    assert_eq!(groups[1].roots, vec![2]);
    assert!(matches!(&groups[1].value, CellState::Value(_)));
    // Break it: group the children too, and row 1 appears.
}

// -- the key-value shape -----------------------------------------------------

#[test]
fn a_las_header_is_key_value_and_says_its_guesses() {
    // mnemonic (key), value, unit, description - no kind says which is which.
    let table = sheet(vec![
        sheet_column("mnemonic", true, Family::Text),
        sheet_column("value", false, Family::Text),
        sheet_column("unit", false, Family::Text),
        sheet_column("description", false, Family::Text),
    ]);
    let shape = kv_shape(&table, &[], &IntervalColumns::None, &[None, None, None, None]).expect("key-value");
    assert_eq!(
        shape,
        KvShape {
            key: 0,
            value: Some(1),
            unit: Some(2),
            description: Some(3),
            guessed: vec![1, 2, 3],
        }
    );
    // Break it: leave `guessed` empty, and a role read from a title is no longer said.
}

#[test]
fn a_table_with_an_md_column_is_not_key_value() {
    let table = sheet(vec![
        sheet_column("k", true, Family::Text),
        sheet_column("v", false, Family::Text),
    ]);
    assert_eq!(
        kv_shape(&table, &[], &IntervalColumns::Point { md: 1 }, &[None, None]),
        Err(KvRefusal::DepthColumns)
    );
    // Break it: ignore `md`.
}

#[test]
fn a_numeric_column_or_a_link_column_or_a_second_key_is_not_key_value() {
    let numeric = sheet(vec![
        sheet_column("k", true, Family::Text),
        sheet_column("v", false, Family::Floating),
    ]);
    assert_eq!(
        kv_shape(&numeric, &[], &IntervalColumns::None, &[None, None]),
        Err(KvRefusal::NotText { column: 1 })
    );
    let text = sheet(vec![
        sheet_column("k", true, Family::Text),
        sheet_column("v", false, Family::Text),
    ]);
    let link = declaration(
        1,
        LinkTarget::Key {
            table_uri: table_uri(2),
        },
    );
    assert_eq!(
        kv_shape(&text, &[link], &IntervalColumns::None, &[None, None]),
        Err(KvRefusal::LinkColumn { column: 1 })
    );
    let two_keys = sheet(vec![
        sheet_column("k", true, Family::Text),
        sheet_column("v", true, Family::Text),
    ]);
    assert_eq!(
        kv_shape(&two_keys, &[], &IntervalColumns::None, &[None, None]),
        Err(KvRefusal::KeyColumns { count: 2 })
    );
    let wide = sheet(vec![
        sheet_column("k", true, Family::Text),
        sheet_column("a", false, Family::Text),
        sheet_column("b", false, Family::Text),
        sheet_column("c", false, Family::Text),
        sheet_column("d", false, Family::Text),
    ]);
    assert_eq!(
        kv_shape(
            &wide,
            &[],
            &IntervalColumns::None,
            &[None, None, None, None, None]
        ),
        Err(KvRefusal::TooManyColumns { count: 4 })
    );
    // Break it: drop one of the four tests, and its table is taken for a sheet of parameters.
}
