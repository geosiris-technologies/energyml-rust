//! `table-schema`: a box per table, an arrow per link, the objects that name a table, the refusals.
//!
//! Every package is built in memory. The links are written the way the producers write them (the description
//! convention of the UDAR tables, `key of the table `X`: <uri>, KeyColumn `k``; a `<stem>_table` column that names
//! the table of `<stem>_uid` by its title; a column of object uris), and a `CategoryLookup` of a property. What the
//! links are is the domain's (`LinkReaders`, `schema`): the tests confront what is drawn with what it says, and check
//! the layout in numbers on the display list.

use std::io::Cursor;
use std::sync::Arc;

use energyml_core::registry::DynObject;
use energyml_domain::package_index::PackageIndex;
use energyml_domain::table::{Convention, LinkDeclaration, LinkReaders, LinkTarget};
use energyml_domain::table_structure::{TableLinks, schema};
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_sheet::draw::{DisplayList, Element, Item, Rect, Segment};
use energyml_storage::EpcStore;
use energyml_views::table_schema::TableSchemaView;
use energyml_views::{Drawn, ViewAsk, ViewKinds, ViewRefusal, draw_list, draw_svg, redraw};
use serde_json::json;

use crate::fixtures::{NO_MAPS, Opened};

const ID: &str = "table-schema";
const GONE: &str = "eml:///eml23.ColumnBasedTable(00000000-0000-4000-8000-0000000000ee)";

fn kinds() -> ViewKinds {
    let mut kinds = ViewKinds::empty();
    kinds.register(Box::new(TableSchemaView));
    kinds
}

fn u(n: u32) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}

fn table_uri(n: u32) -> String {
    format!("eml:///eml23.ColumnBasedTable({})", u(n))
}

fn citation(title: &str) -> eml23::Citation {
    eml23::Citation {
        title: title.to_string(),
        ..Default::default()
    }
}

fn dor(uuid: &str, qualified_type: &str) -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: uuid.to_string(),
        qualified_type: qualified_type.to_string(),
        title: format!("t-{uuid}"),
        ..Default::default()
    }
}

fn empty_store() -> EpcStore {
    EpcStore::from_reader(
        Box::new(Cursor::new(
            energyml_storage::extract::empty_package(None).expect("an empty package"),
        )),
        "tables.epc",
        Arc::new(energyml_schemas::all()),
    )
    .expect("it opens")
}

fn put(store: &mut EpcStore, object: impl DynObject + 'static) {
    store.put(Box::new(object)).expect("it goes in");
}

/// A column of text, with the description a link convention reads.
fn column(title: &str, description: Option<&str>, values: &[&str]) -> eml23::Column {
    eml23::Column {
        title: Some(title.to_string()),
        description: description.map(str::to_string),
        value_count_per_row: 1,
        values: Some(eml23::AbstractValueArray::StringXmlArray(eml23::StringXmlArray {
            values: values.iter().map(|text| text.to_string()).collect(),
            count_per_value: 1,
            ..Default::default()
        })),
        ..Default::default()
    }
}

fn table(store: &mut EpcStore, n: u32, title: &str, keys: Vec<eml23::Column>, columns: Vec<eml23::Column>) {
    put(
        store,
        eml23::ColumnBasedTable {
            uuid: u(n),
            schema_version: "2.3".into(),
            citation: citation(title),
            key_column: keys,
            column: columns,
            ..Default::default()
        },
    );
}

fn property(store: &mut EpcStore, n: u32, lookup: u32) {
    put(
        store,
        resqml22::DiscreteProperty {
            uuid: u(n),
            schema_version: "2.2".into(),
            citation: citation(&format!("property {n}")),
            property_kind: dor(&u(900), "eml23.PropertyKind"),
            supporting_representation: dor(&u(901), "resqml22.WellboreTrajectoryRepresentation"),
            category_lookup: Some(dor(&u(lookup), "eml23.ColumnBasedTable")),
            ..Default::default()
        },
    );
}

/// The package of the tests:
/// - 1 `indexes`, with the key column `index_uid`;
/// - 2 `values`, whose `index` column names the key of 1, whose `other` column names a table the package does not
///   hold, whose `wellbore` column holds object uris, and whose `source_uid` column names tables by title
///   (`indexes`, and `external file`, which is no table);
/// - 3 `lookup`, the category table of the property 10;
/// - 4 `alone`, linked to nothing;
/// - 5 `children`, whose `parent` column names its own key.
fn store() -> EpcStore {
    let mut store = empty_store();
    table(
        &mut store,
        1,
        "indexes",
        vec![column("index_uid", None, &["a", "b", "c"])],
        vec![column("label", None, &["x", "y", "z"])],
    );
    let to_indexes = format!("key of the table `indexes`: {}, KeyColumn `index_uid`", table_uri(1));
    let to_gone = format!("key of the table `gone`: {GONE}, KeyColumn `id`");
    let wellbore = "eml:///witsml21.Wellbore(00000000-0000-4000-8000-0000000000b1)";
    let other_wellbore = "eml:///witsml21.Wellbore(00000000-0000-4000-8000-0000000000b2)";
    table(
        &mut store,
        2,
        "values",
        Vec::new(),
        vec![
            column("index", Some(&to_indexes), &["a", "b", "c"]),
            column("other", Some(&to_gone), &["1", "2", "3"]),
            column("wellbore", None, &[wellbore, wellbore, other_wellbore]),
            column("source_uid", None, &["a", "b", "c"]),
            column("source_table", None, &["indexes", "indexes", "external file"]),
        ],
    );
    table(
        &mut store,
        3,
        "lookup",
        vec![column("code", None, &["1", "2"])],
        vec![column("name", None, &["sand", "shale"])],
    );
    table(&mut store, 4, "alone", Vec::new(), vec![column("v", None, &["1"])]);
    let to_self = format!("key of this table (self reference): {}, KeyColumn `uid`", table_uri(5));
    table(
        &mut store,
        5,
        "children",
        vec![column("uid", None, &["r", "s"])],
        vec![column("parent", Some(&to_self), &["", "r"])],
    );
    property(&mut store, 10, 3);
    store
}

fn opened() -> Opened {
    Opened::of(store())
}

fn uri_in(index: &PackageIndex, n: u32) -> String {
    index.uri_of(&u(n)).expect("the package holds it").to_string()
}

fn drawn(opened: &Opened, ask: ViewAsk) -> Drawn {
    draw_list(&kinds(), &ask, &opened.context(&NO_MAPS))
        .expect("drawn")
        .0
}

fn refused(opened: &Opened, ask: ViewAsk) -> ViewRefusal {
    draw_list(&kinds(), &ask, &opened.context(&NO_MAPS)).expect_err("refused")
}

fn elements<'a>(list: &'a DisplayList, layer: &str) -> &'a [Element] {
    &list.layer(layer).expect("the layer").elements
}

fn element<'a>(list: &'a DisplayList, layer: &str, key: &str) -> &'a Element {
    elements(list, layer)
        .iter()
        .find(|element| element.key.as_deref() == Some(key))
        .unwrap_or_else(|| panic!("no element `{key}` in the layer `{layer}`"))
}

fn has(list: &DisplayList, layer: &str, key: &str) -> bool {
    elements(list, layer)
        .iter()
        .any(|element| element.key.as_deref() == Some(key))
}

fn rect_of(element: &Element) -> Rect {
    element
        .items
        .iter()
        .find_map(|item| match item {
            Item::Rect { rect, .. } => Some(*rect),
            _ => None,
        })
        .expect("a rectangle")
}

fn dash_of_box(element: &Element) -> Vec<f64> {
    element
        .items
        .iter()
        .find_map(|item| match item {
            Item::Rect {
                stroke: Some(stroke), ..
            } => Some(stroke.dash.clone()),
            _ => None,
        })
        .expect("an outlined rectangle")
}

fn texts_of(element: &Element) -> Vec<String> {
    element
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn data_of<'a>(element: &'a Element, name: &str) -> Option<&'a str> {
    element
        .data
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

/// The edges of a layer whose `link` is this word.
fn links<'a>(list: &'a DisplayList, word: &str) -> Vec<&'a Element> {
    elements(list, "edges")
        .iter()
        .filter(|element| data_of(element, "link") == Some(word))
        .collect()
}

fn notes_with(drawn: &Drawn, key: &str) -> Vec<String> {
    drawn
        .notes
        .iter()
        .filter(|note| note.key == key)
        .map(|note| note.says.clone())
        .collect()
}

#[test]
fn a_package_with_tables_is_offered_once_and_one_without_is_not() {
    let opened = opened();
    let offers = kinds().offers(&opened.index);
    assert_eq!(offers.len(), 1);
    assert_eq!(offers[0].view, ID);
    assert!(offers[0].objects.is_empty());
    assert_eq!(opened.index.tables().count(), 5, "the five tables of the package");
    assert!(offers[0].title.contains('5'), "{}", offers[0].title);
    assert!(kinds().offers(&Opened::of(empty_store()).index).is_empty());
    // Break it: offer the view for a package of no table.
}

#[test]
fn a_box_per_table_lists_its_columns_with_the_key_marked() {
    let opened = opened();
    let list = drawn(&opened, ViewAsk::new(ID)).list;
    for n in 1..=5 {
        has_box(&list, &uri_in(&opened.index, n));
    }
    let indexes = uri_in(&opened.index, 1);
    let words = texts_of(element(&list, "nodes", &indexes));
    assert!(words.contains(&"ColumnBasedTable".to_string()), "{words:?}");
    assert!(words.contains(&"indexes".to_string()), "{words:?}");
    assert!(words.contains(&"[key] index_uid".to_string()), "{words:?}");
    assert!(words.contains(&"label".to_string()), "{words:?}");
    // The marks are the sheet's: as many as the key columns the index says the table has.
    for n in 1..=5 {
        let uri = uri_in(&opened.index, n);
        let sheet = opened.index.table_sheet(&uri).expect("a sheet");
        let marked = texts_of(element(&list, "nodes", &uri))
            .iter()
            .filter(|text| text.starts_with("[key] "))
            .count();
        assert_eq!(marked, sheet.columns.iter().filter(|column| column.key).count(), "{uri}");
    }
    // Without the columns, the box is its title.
    let bare = drawn(&opened, ViewAsk::new(ID).set("columns", json!(false))).list;
    let words = texts_of(element(&bare, "nodes", &indexes));
    assert!(words.contains(&"indexes".to_string()));
    assert!(!words.iter().any(|text| text.starts_with("[key]") || text == "label"), "{words:?}");
    // Break it: mark every first column, or every column.
}

fn has_box(list: &DisplayList, key: &str) {
    assert!(has(list, "nodes", key), "no box `{key}`");
}

#[test]
fn a_table_with_many_columns_lists_ten_and_counts_the_rest() {
    let mut store = empty_store();
    let titles: Vec<String> = (0..13).map(|at| format!("c{at}")).collect();
    let columns: Vec<eml23::Column> = titles.iter().map(|title| column(title, None, &["1"])).collect();
    table(&mut store, 41, "wide", Vec::new(), columns);
    let opened = Opened::of(store);
    let list = drawn(&opened, ViewAsk::new(ID)).list;
    let words = texts_of(element(&list, "nodes", &uri_in(&opened.index, 41)));
    assert!(words.contains(&"c9".to_string()) && !words.contains(&"c10".to_string()), "{words:?}");
    assert!(words.contains(&"... and 3 more columns".to_string()), "{words:?}");
    // Break it: list every column, and the box is as tall as the table is wide.
}

#[test]
fn every_link_is_an_arrow_of_its_kind_between_the_boxes_of_its_ends() {
    let opened = opened();
    let list = drawn(&opened, ViewAsk::new(ID)).list;
    let edge_key = |from: String, to: &str, link: &str| format!("{from}>{to}#{link}");
    let (indexes, values) = (uri_in(&opened.index, 1), uri_in(&opened.index, 2));

    // By key: `values.index` names the key of `indexes`, and `values.other` that of a table the package lacks.
    assert_eq!(links(&list, "key").len(), 2);
    let key = element(&list, "edges", &edge_key(values.clone(), &indexes, "key"));
    assert_eq!(data_of(key, "columns"), Some("0"));
    assert!(texts_of(key).contains(&"key: index".to_string()), "{:?}", texts_of(key));
    assert!(has(&list, "edges", &edge_key(values.clone(), GONE, "key")));
    // By title: `indexes` is a table of the set, `external file` is not one.
    assert_eq!(links(&list, "title").len(), 2);
    assert!(has(&list, "edges", &edge_key(values.clone(), &indexes, "title")));
    assert!(has(&list, "edges", &edge_key(values.clone(), "unmatched:external file", "title")));
    // By uris: the objects of one type a column names.
    assert_eq!(links(&list, "uris").len(), 1);
    assert!(has(&list, "edges", &edge_key(values.clone(), "objects:witsml21.Wellbore", "uris")));
    // A table that names its own rows: one loop.
    let children = uri_in(&opened.index, 5);
    assert_eq!(links(&list, "self").len(), 1);
    assert!(has(&list, "edges", &edge_key(children.clone(), &children, "self")));
    // A property that names the table by `CategoryLookup`: one arrow, from the property (alone of its type).
    assert_eq!(links(&list, "index").len(), 1);
    let property = uri_in(&opened.index, 10);
    assert!(has(
        &list,
        "edges",
        &edge_key(property, &uri_in(&opened.index, 3), "index:CategoryLookup")
    ));
    assert_eq!(elements(&list, "edges").len(), 7);
    // 5 tables, the one named and absent, the objects, the title no table has, the property: 9 boxes.
    assert_eq!(elements(&list, "nodes").len(), 9);

    // The curve runs from the right side of `values` to the left side of `indexes`, with the flow.
    let from = rect_of(element(&list, "nodes", &values));
    let to = rect_of(element(&list, "nodes", &indexes));
    assert!(from.x + from.w < to.x, "the source is a layer before the target");
    let (first, last) = path_ends(key);
    assert!((first[0] - (from.x + from.w)).abs() < 1.0e-9 && (first[1] - (from.y + from.h / 2.0)).abs() < 1.0e-9);
    assert!((last[0] - to.x).abs() < 1.0e-9 && (last[1] - (to.y + to.h / 2.0)).abs() < 1.0e-9);
    // Its head stands at the end of the curve.
    let tip = key.items.iter().find_map(|item| match item {
        Item::Polygon { points, .. } => Some(points[0]),
        _ => None,
    });
    assert_eq!(tip, Some(last));
    // Break it: join the centres of the boxes, or draw only the links of one kind.
}

fn path_ends(element: &Element) -> ([f64; 2], [f64; 2]) {
    element
        .items
        .iter()
        .find_map(|item| match item {
            Item::Path { path, .. } => {
                let first = match path.segments.first() {
                    Some(Segment::MoveTo(p)) => *p,
                    _ => return None,
                };
                let last = match path.segments.last() {
                    Some(Segment::CubicTo { to, .. }) => *to,
                    _ => return None,
                };
                Some((first, last))
            }
            _ => None,
        })
        .expect("a path of curves")
}

#[test]
fn the_links_drawn_are_the_graph_the_domain_makes_of_the_links_written_by_hand() {
    // Two tables, one key link: written as the domain's own tests write it, not read from a description.
    let (a, b) = (table_uri(1), table_uri(2));
    let by_hand = |uri: &str, title: &str, declarations: Vec<LinkDeclaration>| TableLinks {
        uri: uri.to_string(),
        title: Some(title.to_string()),
        declarations,
        refused: Vec::new(),
        uri_columns: Vec::new(),
        titles_named: Vec::new(),
    };
    let expected = schema(
        &[
            by_hand(&a, "indexes", Vec::new()),
            by_hand(
                &b,
                "values",
                vec![LinkDeclaration {
                    column: 0,
                    target: LinkTarget::Key { table_uri: a.clone() },
                    convention: Convention::DescriptionText,
                    warnings: Vec::new(),
                    key_named: None,
                }],
            ),
        ],
        &[],
    );
    let mut store = empty_store();
    table(&mut store, 1, "indexes", vec![column("index_uid", None, &["a"])], Vec::new());
    let to_indexes = format!("key of the table `indexes`: {a}, KeyColumn `index_uid`");
    table(
        &mut store,
        2,
        "values",
        Vec::new(),
        vec![column("index", Some(&to_indexes), &["a"])],
    );
    let opened = Opened::of(store);
    let list = drawn(&opened, ViewAsk::new(ID)).list;
    assert_eq!(elements(&list, "nodes").len(), expected.nodes.len());
    assert_eq!(elements(&list, "edges").len(), expected.edges.len());
    // And what the description says is what the domain's own reader reads of the sheet the index holds.
    let sheet = opened.index.table_sheet(&uri_in(&opened.index, 2)).expect("a sheet");
    let read = LinkReaders::builtin()
        .read_column(sheet, 0, None)
        .expect("the column declares a link")
        .expect("followed");
    assert!(matches!(read.target, LinkTarget::Key { .. }));
    // Break it: draw an edge for each column of the table, links or not.
}

#[test]
fn a_table_named_and_not_held_and_a_title_no_table_has_are_dashed_boxes() {
    let opened = opened();
    let list = drawn(&opened, ViewAsk::new(ID)).list;
    for key in [GONE, "unmatched:external file"] {
        assert!(!dash_of_box(element(&list, "nodes", key)).is_empty(), "{key}");
    }
    let words = texts_of(element(&list, "nodes", "unmatched:external file"));
    assert!(words.contains(&"external file".to_string()), "{words:?}");
    // A table of the set is solid, and the one that is linked to nothing says so in its element.
    let alone = element(&list, "nodes", &uri_in(&opened.index, 4));
    assert!(dash_of_box(alone).is_empty());
    assert_eq!(data_of(alone, "isolated"), Some("true"));
    assert_eq!(data_of(element(&list, "nodes", &uri_in(&opened.index, 1)), "isolated"), None);
    // Break it: draw every box solid, or mark every table isolated.
}

#[test]
fn the_objects_that_name_a_table_are_one_box_that_counts_them() {
    let mut store = store();
    property(&mut store, 11, 3);
    property(&mut store, 12, 3);
    let opened = Opened::of(store);
    let lookup = uri_in(&opened.index, 3);
    // The domain's own answer: three objects name the table.
    assert_eq!(opened.index.dependents_of(&u(3)).len(), 3);
    let list = drawn(&opened, ViewAsk::new(ID)).list;
    let qualified = opened
        .index
        .node(&uri_in(&opened.index, 10))
        .expect("a node")
        .qualified_type
        .to_string();
    let group = format!("index:{qualified}");
    assert!(has(&list, "nodes", &group));
    assert!(texts_of(element(&list, "nodes", &group)).contains(&"3 x DiscreteProperty".to_string()));
    let arrows = links(&list, "index");
    assert_eq!(arrows.len(), 1, "one arrow for the three of them");
    assert!(texts_of(arrows[0]).contains(&"CategoryLookup (3)".to_string()), "{:?}", texts_of(arrows[0]));
    assert_eq!(arrows[0].key.as_deref(), Some(format!("{group}>{lookup}#index:CategoryLookup").as_str()));
    // Break it: draw one box and one arrow per object.
}

#[test]
fn a_title_two_tables_share_names_nothing_and_is_a_note() {
    let mut store = empty_store();
    table(&mut store, 21, "same", Vec::new(), vec![column("v", None, &["1"])]);
    table(&mut store, 22, "same", Vec::new(), vec![column("v", None, &["1"])]);
    table(
        &mut store,
        23,
        "src",
        Vec::new(),
        vec![
            column("source_uid", None, &["a"]),
            column("source_table", None, &["same"]),
        ],
    );
    let opened = Opened::of(store);
    let found = drawn(&opened, ViewAsk::new(ID));
    assert!(links(&found.list, "title").is_empty(), "a title is not an identity");
    let notes = notes_with(&found, "ambiguous-title");
    assert_eq!(notes.len(), 1, "{:?}", found.notes);
    assert!(notes[0].contains("`same`") && notes[0].contains("2 tables"), "{}", notes[0]);
    // Break it: link the first table that carries the title.
}

#[test]
fn a_link_the_reader_refuses_is_a_note_and_no_arrow() {
    let mut store = empty_store();
    // No backticks around the title: it starts like a declaration and is not one.
    let wrong = format!("key of the table indexes: {}, KeyColumn `index_uid`", table_uri(1));
    table(&mut store, 31, "bad", Vec::new(), vec![column("ref", Some(&wrong), &["a"])]);
    let opened = Opened::of(store);
    let sheet = opened.index.table_sheet(&uri_in(&opened.index, 31)).expect("a sheet");
    assert!(
        matches!(LinkReaders::builtin().read_column(sheet, 0, None), Some(Err(_))),
        "the domain's reader refuses it"
    );
    let found = drawn(&opened, ViewAsk::new(ID));
    assert!(elements(&found.list, "edges").is_empty());
    let notes = notes_with(&found, "link-refused");
    assert_eq!(notes.len(), 1, "{:?}", found.notes);
    assert!(notes[0].contains("bad") && notes[0].contains("ref"), "{}", notes[0]);
    // Break it: skip the refused declarations, and the link vanishes without a word.
}

#[test]
fn reading_no_values_draws_the_links_the_sheets_say_and_no_more() {
    let opened = opened();
    let list = drawn(&opened, ViewAsk::new(ID).set("values", json!(false))).list;
    assert_eq!(links(&list, "key").len(), 2);
    assert_eq!(links(&list, "self").len(), 1);
    assert_eq!(links(&list, "index").len(), 1);
    assert!(links(&list, "uris").is_empty() && links(&list, "title").is_empty());
    assert!(!has(&list, "nodes", "unmatched:external file"));
    // Break it: read the values whatever the setting says.
}

#[test]
fn only_the_tables_asked_are_drawn_and_an_object_that_is_no_table_is_refused() {
    let opened = opened();
    let (indexes, values) = (uri_in(&opened.index, 1), uri_in(&opened.index, 2));
    let list = drawn(
        &opened,
        ViewAsk::new(ID).object(indexes.clone()).object(values.clone()),
    )
    .list;
    has_box(&list, &indexes);
    has_box(&list, &values);
    for n in [3, 4, 5] {
        assert!(!has(&list, "nodes", &uri_in(&opened.index, n)), "table {n}");
    }
    assert!(links(&list, "index").is_empty(), "the lookup is not in the set, nor the property that names it");
    // The links of the two tables are still drawn.
    assert_eq!(links(&list, "key").len(), 2);

    let why = refused(&opened, ViewAsk::new(ID).object(uri_in(&opened.index, 10)));
    assert!(matches!(&why, ViewRefusal::ObjectNotTaken { .. }), "{why}");
    // Break it: draw every table of the package whatever is asked.
}

#[test]
fn a_package_with_no_table_is_refused_not_drawn_empty() {
    let opened = Opened::of(empty_store());
    let why = refused(&opened, ViewAsk::new(ID));
    assert!(matches!(&why, ViewRefusal::Computing { .. }), "{why}");
    assert!(why.to_string().contains("no table"), "{why}");
    // Break it: return an empty display list.
}

#[test]
fn the_flow_turns_the_layers_into_rows() {
    let opened = opened();
    let (indexes, values) = (uri_in(&opened.index, 1), uri_in(&opened.index, 2));
    let along = drawn(&opened, ViewAsk::new(ID)).list;
    let (a, b) = (
        rect_of(element(&along, "nodes", &indexes)),
        rect_of(element(&along, "nodes", &values)),
    );
    assert!(b.x < a.x, "left to right: the table that names comes first");
    let down = drawn(&opened, ViewAsk::new(ID).set("flow", json!("top_down"))).list;
    let (a, b) = (
        rect_of(element(&down, "nodes", &indexes)),
        rect_of(element(&down, "nodes", &values)),
    );
    assert!(b.y < a.y, "top down: it comes above");
    // Break it: ignore the flow setting.
}

#[test]
fn a_setting_of_the_wrong_shape_is_refused_by_its_name() {
    let opened = opened();
    for (name, value) in [
        ("flow", json!("sideways")),
        ("ordering", json!("random")),
        ("columns", json!("yes")),
        ("values", json!(1)),
    ] {
        let why = refused(&opened, ViewAsk::new(ID).set(name, value.clone()));
        assert!(
            matches!(&why, ViewRefusal::BadSetting { name: named, .. } if named == name),
            "{name} = {value}: {why}"
        );
    }
    // Break it: take any word for a flow.
}

#[test]
fn the_same_request_draws_the_same_bytes_and_a_redraw_agrees() {
    let opened = opened();
    let kinds = kinds();
    let cx = opened.context(&NO_MAPS);
    let ask = ViewAsk::new(ID).set("ordering", json!("median"));
    let first = draw_svg(&kinds, &ask, &cx).expect("drawn");
    let second = draw_svg(&kinds, &ask, &cx).expect("drawn");
    assert_eq!(first.svg, second.svg);
    let again = redraw(&kinds, &first.svg, &cx).expect("redrawn");
    assert_eq!(again.svg, first.svg);
    // Break it: iterate a hash map when grouping the objects that name a table.
}
