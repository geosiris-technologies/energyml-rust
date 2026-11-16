//! What a view proposes for a first drawing, and the dimensions it needs declared: the curtain of `along-well`.
//!
//! The package is fabricated in memory: the straight hole of `fixtures::package`, a frame with two nodes, a
//! property of 3 x 3 values per node (a curtain candidate whose axes the file does not link), and tables of
//! axes. A table that goes with the curtain has one key column and two numeric value columns of 3 rows, one value
//! column per dimension, in the order of the dimensions (ADR 0029). Nothing is read from a file.

use energyml_core::xml::XmlText;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_section::along_well::AxisLinkAsk;
use energyml_sheet::draw::DisplayList;
use energyml_views::along_well::{AlongWell, ID};
use energyml_views::{
    Assist, AxisColumn, ViewAsk, ViewKind, ViewKinds, ViewRefusal, assist, axis_links_read, axis_links_value,
    draw_list,
};
use serde_json::{Value, json};

use crate::fixtures::{FRAME, NO_MAPS, Opened, frame_uri, package, uri};

const MATRIX: &str = "55555555-5555-4555-8555-555555555555";

fn kinds() -> ViewKinds {
    let mut kinds = ViewKinds::empty();
    kinds.register(Box::new(AlongWell));
    kinds
}

fn u(n: u32) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}

fn table_uri(n: u32) -> String {
    uri("eml23.ColumnBasedTable", &u(n))
}

fn matrix_uri() -> String {
    uri("resqml22.ContinuousProperty", MATRIX)
}

/// 3 x 3 values per node on the frame (2 nodes, 18 values, all different): a curtain, no axis stated by the file.
fn matrix_property() -> resqml22::ContinuousProperty {
    resqml22::ContinuousProperty {
        uuid: MATRIX.to_string(),
        schema_version: "2.2".into(),
        citation: eml23::Citation {
            title: "a matrix".to_string(),
            ..Default::default()
        },
        indexable_element: eml23::IndexableElement::Nodes,
        value_count_per_indexable_element: vec![3, 3],
        property_kind: eml23::DataObjectReference {
            uuid: "dddddddd-dddd-4ddd-8ddd-dddddddddddd".to_string(),
            qualified_type: "eml23.PropertyKind".to_string(),
            title: "a kind".to_string(),
            ..Default::default()
        },
        supporting_representation: eml23::DataObjectReference {
            uuid: FRAME.to_string(),
            qualified_type: "resqml22.WellboreFrameRepresentation".to_string(),
            title: "a frame".to_string(),
            ..Default::default()
        },
        uom: eml23::UnitOfMeasureExt::from_xml_text("ohm.m").expect("a unit"),
        values_for_patch: vec![resqml22::AbstractValueArray::FloatingPointXmlArray(
            eml23::FloatingPointXmlArray {
                values: (1..=18).map(f64::from).collect(),
                count_per_value: 1,
                ..Default::default()
            },
        )],
        ..Default::default()
    }
}

/// A column of numbers, in metres when `metres`.
fn numbers(title: &str, metres: bool, values: &[f64]) -> eml23::Column {
    eml23::Column {
        title: Some(title.to_string()),
        uom: metres.then_some(eml23::UnitOfMeasureExt::M),
        value_count_per_row: 1,
        values: Some(eml23::AbstractValueArray::FloatingPointXmlArray(
            eml23::FloatingPointXmlArray {
                values: values.to_vec(),
                count_per_value: 1,
                ..Default::default()
            },
        )),
        ..Default::default()
    }
}

/// A column of words.
fn words(title: &str, values: &[&str]) -> eml23::Column {
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

fn put_table(
    store: &mut energyml_storage::EpcStore,
    n: u32,
    title: &str,
    keys: Vec<eml23::Column>,
    columns: Vec<eml23::Column>,
) {
    let table = eml23::ColumnBasedTable {
        uuid: u(n),
        schema_version: "2.3".into(),
        citation: eml23::Citation {
            title: title.to_string(),
            ..Default::default()
        },
        key_column: keys,
        column: columns,
        ..Default::default()
    };
    store.put(Box::new(table)).expect("it goes in");
}

/// Table `n`: one key column and two value columns of 3 rows, named after the dimensions they would give.
fn axes_table(store: &mut energyml_storage::EpcStore, n: u32, title: &str) {
    put_table(
        store,
        n,
        title,
        vec![numbers("component_index", false, &[0.0, 1.0, 2.0])],
        vec![
            numbers("Delta_N_or_Upside", true, &[-1.0, 0.0, 1.0]),
            numbers("Delta_E_or_Horizontal", true, &[-2.0, 0.0, 2.0]),
        ],
    );
}

/// The package: the hole, the curtain candidate, and the tables `make` puts in.
fn package_with(make: impl FnOnce(&mut energyml_storage::EpcStore)) -> Opened {
    let mut store = package(50.0, &[20.0, 80.0]);
    store.put(Box::new(matrix_property())).expect("it goes in");
    make(&mut store);
    Opened::of(store)
}

fn curtain_ask() -> ViewAsk {
    ViewAsk::new(ID)
        .object(frame_uri())
        .object(matrix_uri())
        .sized(800.0, 600.0)
}

fn assisted(opened: &Opened, ask: &ViewAsk) -> Assist {
    assist(&kinds(), ask, &opened.context(&NO_MAPS)).expect("the view is known")
}

fn proposed<'a>(assist: &'a Assist, setting: &str) -> Option<&'a Value> {
    assist
        .proposals
        .iter()
        .find(|proposal| proposal.setting == setting)
        .map(|proposal| &proposal.value)
}

fn column(n: u32, title: &str, name: &str) -> AxisColumn {
    AxisColumn {
        table: table_uri(n),
        table_title: title.to_string(),
        column: name.to_string(),
    }
}

#[test]
fn the_only_table_that_goes_is_proposed_one_column_per_dimension_in_the_order_of_its_columns() {
    let opened = package_with(|store| axes_table(store, 1, "UDAR transverse axes"));
    let found = assisted(&opened, &curtain_ask());
    // ADR 0029: the dimension i takes the i-th value column of the table, the slowest first.
    let links = proposed(&found, "curtain_links").expect("the links are proposed");
    assert_eq!(
        axis_links_read(links),
        vec![
            (0, table_uri(1), "Delta_N_or_Upside".to_string()),
            (1, table_uri(1), "Delta_E_or_Horizontal".to_string()),
        ]
    );
    // The roles are a declaration, proposed.
    assert_eq!(proposed(&found, "slow_axis"), Some(&json!("high_side")));
    assert_eq!(proposed(&found, "slow_sign"), Some(&json!("positive")));
    assert_eq!(proposed(&found, "fast_axis"), Some(&json!("right")));
    assert_eq!(proposed(&found, "fast_sign"), Some(&json!("positive")));
    assert_eq!(found.proposals.len(), 5, "nothing else is proposed");
    assert!(
        found.proposals.iter().all(|proposal| !proposal.why.is_empty()),
        "a proposal always says why"
    );
    // The dimensions: sizes from the index, the roles' settings, the numeric value columns (the key left out).
    assert_eq!(found.dimensions.len(), 2);
    let (slow, fast) = (&found.dimensions[0], &found.dimensions[1]);
    assert_eq!((slow.dim, slow.size, fast.dim, fast.size), (0, 3, 1, 3));
    assert_eq!(slow.links_setting, "curtain_links");
    assert_eq!((slow.axis_setting, slow.sign_setting), ("slow_axis", "slow_sign"));
    assert_eq!((fast.axis_setting, fast.sign_setting), ("fast_axis", "fast_sign"));
    let expected = vec![
        column(1, "UDAR transverse axes", "Delta_N_or_Upside"),
        column(1, "UDAR transverse axes", "Delta_E_or_Horizontal"),
    ];
    assert_eq!(slow.candidates, expected);
    assert_eq!(fast.candidates, expected);
    // Break it: take the value columns in alphabetical order (the links would swap), or keep the key column among
    // the candidates.
}

#[test]
fn two_tables_that_go_are_a_choice_so_no_links_are_proposed_but_both_are_candidates() {
    let opened = package_with(|store| {
        axes_table(store, 1, "first axes");
        axes_table(store, 2, "second axes");
    });
    let found = assisted(&opened, &curtain_ask());
    assert_eq!(proposed(&found, "curtain_links"), None, "two tables: the person chooses");
    // The roles do not depend on the tables.
    assert_eq!(proposed(&found, "slow_axis"), Some(&json!("high_side")));
    for dimension in &found.dimensions {
        let tables: Vec<&str> = dimension
            .candidates
            .iter()
            .map(|candidate| candidate.table.as_str())
            .collect();
        assert_eq!(
            tables,
            vec![table_uri(1), table_uri(1), table_uri(2), table_uri(2)],
            "both tables, by uri, their columns in order"
        );
    }
    // Break it: propose the first table whatever the others (the links would appear here).
}

#[test]
fn a_table_of_the_wrong_length_or_shape_is_not_a_candidate_nor_proposed() {
    let opened = package_with(|store| {
        axes_table(store, 1, "axes");
        // Five rows where the curtain has three positions per dimension.
        put_table(
            store,
            2,
            "too long",
            vec![numbers("k", false, &[0.0, 1.0, 2.0, 3.0, 4.0])],
            vec![
                numbers("a", true, &[0.0, 1.0, 2.0, 3.0, 4.0]),
                numbers("b", true, &[0.0, 1.0, 2.0, 3.0, 4.0]),
            ],
        );
        // The right length, a column that is not a number: a candidate for the numbers only, not a table that goes.
        put_table(
            store,
            3,
            "labelled",
            vec![numbers("k", false, &[0.0, 1.0, 2.0])],
            vec![
                numbers("x", true, &[5.0, 6.0, 7.0]),
                words("label", &["a", "b", "c"]),
            ],
        );
    });
    let found = assisted(&opened, &curtain_ask());
    for dimension in &found.dimensions {
        assert!(
            dimension.candidates.iter().all(|candidate| candidate.table != table_uri(2)),
            "five rows is not three positions: {:?}",
            dimension.candidates
        );
        assert!(
            dimension.candidates.iter().all(|candidate| candidate.column != "k"),
            "a key column is not a column of positions"
        );
        assert!(
            dimension
                .candidates
                .iter()
                .all(|candidate| candidate.column != "label"),
            "a column of words is not a column of positions"
        );
        assert!(
            dimension.candidates.contains(&column(3, "labelled", "x")),
            "the numeric column of the right length is offered"
        );
    }
    // The table that goes is still the only one (the other two do not have a value column per dimension).
    let links = proposed(&found, "curtain_links").expect("the links are proposed");
    assert_eq!(axis_links_read(links)[0].1, table_uri(1));
    // Break it: accept any table with the right number of columns whatever its rows (the table of 5 rows would be
    // a second table that goes, and the links would no longer be proposed).
}

#[test]
fn the_hole_alone_proposes_nothing() {
    let opened = package_with(|store| axes_table(store, 1, "axes"));
    let hole = ViewAsk::new(ID).object(frame_uri());
    assert_eq!(assisted(&opened, &hole), Assist::default());
    // An object the package does not hold, a view with nothing to propose: nothing either, no refusal.
    let gone = ViewAsk::new(ID)
        .object(frame_uri())
        .object(uri("resqml22.ContinuousProperty", "99999999-9999-4999-8999-999999999999"));
    assert_eq!(assisted(&opened, &gone), Assist::default());
    // A view that is not in the registry is the one refusal.
    let unknown = ViewAsk::new("nowhere");
    assert!(matches!(
        assist(&kinds(), &unknown, &opened.context(&NO_MAPS)),
        Err(ViewRefusal::UnknownView { .. })
    ));
    // Break it: propose the roles for the frame alone (the hole would then carry four proposals).
}

#[test]
fn the_links_setting_is_written_and_read_back_as_the_same_couples() {
    let links = vec![
        (0, column(1, "UDAR transverse axes", "Delta_N_or_Upside")),
        (1, column(1, "UDAR transverse axes", "Delta_E_or_Horizontal")),
    ];
    let value = axis_links_value(&links);
    // The shape the section reads: the variant by its kebab-case name, its fields inside.
    assert_eq!(
        value,
        json!([
            {"table": {"table": table_uri(1), "column": "Delta_N_or_Upside", "dim": 0}},
            {"table": {"table": table_uri(1), "column": "Delta_E_or_Horizontal", "dim": 1}},
        ])
    );
    // Triangulation: the section's own type reads each entry back.
    let entries = value.as_array().expect("a list");
    let read: Vec<AxisLinkAsk> = entries
        .iter()
        .map(|entry| serde_json::from_value(entry.clone()).expect("the section reads it"))
        .collect();
    assert_eq!(
        read[0],
        AxisLinkAsk::Table {
            table: table_uri(1),
            column: "Delta_N_or_Upside".to_string(),
            dim: 0
        }
    );
    assert_eq!(
        axis_links_read(&value),
        vec![
            (0, table_uri(1), "Delta_N_or_Upside".to_string()),
            (1, table_uri(1), "Delta_E_or_Horizontal".to_string()),
        ]
    );
    // What is not a table link is left out, and so is what is not a link at all.
    let mixed = json!([
        {"paired-property": {"property": "eml:///resqml22.ContinuousProperty(x)", "dim": 0}},
        {"table": {"table": table_uri(2), "column": "c", "dim": 1}},
        {"witsml-channel": {"channel": "eml:///witsml21.Channel(y)"}},
        {"kind": "table"},
        7,
    ]);
    assert_eq!(axis_links_read(&mixed), vec![(1, table_uri(2), "c".to_string())]);
    assert!(axis_links_read(&json!({"table": {}})).is_empty(), "not a list");
    // Break it: write `{"kind": "table", ...}` (the section would refuse the setting), or read the links in the
    // order of their tables rather than the order given.
}

/// The settings that declare the curtain: its links and its four roles, given, no `lateral_index`.
fn declared() -> ViewAsk {
    curtain_ask()
        .set(
            "curtain_links",
            axis_links_value(&[
                (0, column(1, "axes", "Delta_N_or_Upside")),
                (1, column(1, "axes", "Delta_E_or_Horizontal")),
            ]),
        )
        .set("slow_axis", json!("high_side"))
        .set("slow_sign", json!("positive"))
        .set("fast_axis", json!("right"))
        .set("fast_sign", json!("positive"))
}

fn drawn(opened: &Opened, ask: &ViewAsk) -> DisplayList {
    draw_list(&kinds(), ask, &opened.context(&NO_MAPS))
        .expect("drawn")
        .0
        .list
}

#[test]
fn without_a_lateral_index_the_middle_row_of_the_lateral_dimension_is_drawn() {
    let opened = package_with(|store| axes_table(store, 1, "axes"));
    let by_default = drawn(&opened, &declared());
    // The fast dimension is lateral (the slow one runs along the high side): its offsets are -2, 0, 2 metres, so
    // the middle row (1) is the plane of the hole, and the row 0 is 2 m off it (a note says so in the picture).
    let middle = drawn(&opened, &declared().set("lateral_index", json!(1)));
    let edge = drawn(&opened, &declared().set("lateral_index", json!(0)));
    assert_eq!(by_default, middle, "no index is the middle row");
    assert_ne!(by_default, edge, "the edge row is another plane");
    // The help says what the default is.
    let settings = AlongWell.settings();
    let setting = settings
        .iter()
        .find(|setting| setting.name == "lateral_index")
        .expect("declared");
    assert!(setting.about.contains("middle"), "{}", setting.about);
    assert_eq!(setting.default, None, "no default: absent means the middle row");
    // The lateral dimension follows the roles: with the roles swapped the slow dimension is the lateral one,
    // and its middle row (1, offsets -1, 0, 1) is the plane of the hole.
    let swapped = declared()
        .set("slow_axis", json!("right"))
        .set("fast_axis", json!("high_side"));
    assert_eq!(
        drawn(&opened, &swapped),
        drawn(&opened, &swapped.clone().set("lateral_index", json!(1)))
    );
    // Break it: put the default back to row 0 (the edge row is 2 m off the plane of the hole and is drawn with a
    // note, so the first assertion fails).
}

#[test]
fn the_help_of_the_links_names_the_shape_the_section_reads() {
    let settings = AlongWell.settings();
    let about = settings
        .iter()
        .find(|setting| setting.name == "curtain_links")
        .expect("declared")
        .about;
    assert!(about.contains(r#"{"table": {"table": uri, "column": title, "dim": n}}"#), "{about}");
    assert!(about.contains("paired-property") && about.contains("witsml-channel"), "{about}");
    assert!(!about.contains(r#""kind""#), "the old, wrong shape: {about}");
    // Break it: write `{"kind": "table", ...}` back into the help.
}

#[test]
fn drawing_with_every_proposal_applied_succeeds_and_draws_the_curtain() {
    let opened = package_with(|store| axes_table(store, 1, "axes"));
    // What a face does: the proposals fill the settings the request does not give.
    let mut ask = curtain_ask();
    for proposal in assisted(&opened, &ask).proposals {
        ask = ask.set(proposal.setting, proposal.value);
    }
    let list = drawn(&opened, &ask);
    assert!(
        list.layers
            .iter()
            .any(|layer| layer.name == "curtain" && !layer.elements.is_empty()),
        "the curtain is drawn"
    );
    // Without them the same request is refused by name: the file does not say the roles.
    match draw_list(&kinds(), &curtain_ask(), &opened.context(&NO_MAPS)) {
        Err(ViewRefusal::MissingSetting { name, .. }) => assert_eq!(name, "slow_axis"),
        other => panic!("expected a missing setting, got {other:?}"),
    }
    // Break it: propose the links with the columns swapped across dimensions of unequal sizes, or drop the roles
    // from the proposals (the draw would be refused for `slow_axis`).
}
