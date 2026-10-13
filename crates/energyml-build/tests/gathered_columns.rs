//! `textColumns` and `everything` — the text columns of a table gathered into a
//! `ColumnBasedTable`, one discrete property per frame pointing at its rows.
//!
//! The need: the other properties, which are not
//! continuous properties […] will be used with a discrete property
//! and a column-based table that will allow grouping all this information
//! rather than copying everything into different discrete properties; the
//! two modes as possible choices, and the columns no template
//! names carried in the column-based table to save all the information and
//! lose nothing of the CSV, in a table of their own with no declared kind.
//!
//! The control that carries the feature is
//! `every_node_decodes_to_the_cells_the_file_states`: a pointing property is a
//! rank, and a rank off by one names the neighbouring row — a valid package that
//! says the wrong thing. It decodes each node through the table the plan wrote
//! and confronts the result with the file's own cells, never with what this
//! module computed.

use serde_json::{Value, json};

use energyml_build::operation::{Choices, Identity, NewObject, Numbers, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_build::tabular::{self, Dialect};
use energyml_build::{osdu, tabular::Table};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

const STRA_KIND: &str = "22222222-2222-4222-8222-222222222222";
const LITHO_KIND: &str = "33333333-3333-4333-8333-333333333333";

fn when() -> When {
    When {
        creation: "2026-10-01T00:00:00Z".into(),
        originator: "test".into(),
    }
}

fn column(name: &str, uuid: &str, value_type: &str) -> Value {
    json!({
        "ColumnName": name,
        "ValueType": value_type,
        "PropertyType": { "PropertyTypeID": format!("namespace:reference-data--PropertyType:{uuid}:") },
    })
}

/// The name, the MD, a number and two text columns. `NOTE`, in the file, is in
/// no template.
fn template() -> Value {
    json!([
        column("SONDAGE", "348c9a4d-e3e4-4beb-8456-f07a46660de1", "string"),
        column("PROF", "c48c65d4-9680-4d10-903c-a1b2e30f66b5", "number"),
        column("PHI", "11111111-1111-4111-8111-111111111111", "number"),
        column("STRA", STRA_KIND, "string"),
        column("LITHO", LITHO_KIND, "string"),
    ])
}

/// Two wells. F1's second and F2's first rows state the same combination; F1's
/// 20 m node has no row; F2's 30 m row has empty text cells.
const FILE: &str = "SONDAGE,PROF,PHI,STRA,LITHO,NOTE\n\
F1,10,0.1,e6,marne,vu\n\
F1,30,0.2,e5,calcaire,\n\
F2,10,0.3,e5,calcaire,revu\n\
F2,30,0.4,,,\n";

fn frame(bag: Pieces, at: usize, title: &str, mds: &[f64]) -> Pieces {
    let name = if at == 0 {
        "frames".to_string()
    } else {
        format!("frames {}", at + 1)
    };
    let geometry = Geometry {
        kind: "resqml22.WellboreFrameRepresentation",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points: mds.iter().map(|md| [0.0, 0.0, -md]).collect(),
            topology: Topology::Points,
            closed: Vec::new(),
        }],
    };
    bag.with(
        name.clone(),
        Source::Shape {
            geometry,
            closed: None,
        },
    )
    .with(
        format!("{name} (object)"),
        Source::Object(Known {
            reference: Some(Ref::new(
                format!("uuid-{title}"),
                "resqml22.WellboreFrameRepresentation",
                title,
            )),
            node_mds: Some(mds.to_vec()),
            ..Known::default()
        }),
    )
}

fn read(text: &str) -> Table {
    tabular::read("essai.csv", text, Dialect::sniff(text)).expect("a table")
}

fn bag() -> Pieces {
    let template = osdu::from_json(&template()).expect("a template");
    let pieces = Pieces::empty().with(
        "table",
        Source::Table {
            name: "essai.csv".into(),
            table: read(FILE),
            template,
        },
    );
    let pieces = frame(pieces, 0, "F1 — frame", &[10.0, 20.0, 30.0]);
    frame(pieces, 1, "F2 — frame", &[10.0, 30.0])
}

fn plan(answers: Value) -> Plan {
    let mut choices = Choices::new();
    for (name, value) in answers.as_object().expect("an object") {
        choices = choices.set(name, value.clone());
    }
    Registry::new()
        .plan(
            "properties-from-table",
            &bag(),
            &choices,
            &Identity::derived(),
            &when(),
        )
        .unwrap_or_else(|why| panic!("a plan: {why}"))
}

fn titles(made: &Plan) -> Vec<String> {
    made.objects.iter().map(|o| o.title.clone()).collect()
}

fn object<'a>(made: &'a Plan, title: &str) -> &'a NewObject {
    made.objects
        .iter()
        .find(|o| o.title == title)
        .unwrap_or_else(|| panic!("no `{title}` among {:?}", titles(made)))
}

fn codes(made: &Plan, title: &str) -> Vec<i64> {
    let path = format!("/resqml22/{}/values", object(made, title).uuid);
    match &made
        .datasets
        .iter()
        .find(|set| set.path == path)
        .expect("its array")
        .values
    {
        Numbers::I64(values) => values.clone(),
        other => panic!("integers, not {other:?}"),
    }
}

/// The table a pointing property names, found by the uuid it carries.
fn table_of<'a>(made: &'a Plan, pointer: &str) -> &'a NewObject {
    let uuid = object(made, pointer).json["CategoryLookup"]["Uuid"]
        .as_str()
        .expect("a lookup")
        .to_string();
    made.objects
        .iter()
        .find(|o| o.uuid == uuid)
        .unwrap_or_else(|| panic!("no table `{uuid}` — the lookup dangles"))
}

/// A table's column, by title: its kind uuid and its values.
fn table_column(table: &NewObject, title: &str) -> (String, Vec<String>) {
    let column = table.json["Column"]
        .as_array()
        .expect("columns")
        .iter()
        .find(|c| c["Title"] == title)
        .unwrap_or_else(|| panic!("no column `{title}` in `{}`", table.title));
    let values = column["Values"]["Values"]
        .as_array()
        .expect("values")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    (
        column["PropertyKind"]["Uuid"].as_str().unwrap().to_string(),
        values,
    )
}

/// To break: change the default — every package written before, Lutetia included,
/// would change objects.
#[test]
fn separate_stays_the_default_and_writes_what_it_wrote() {
    let made = plan(json!({}));
    for title in ["F1 — STRA", "F1 — LITHO", "F2 — STRA", "F2 — LITHO"] {
        assert_eq!(object(&made, title).qualified_type, "resqml22.DiscreteProperty");
    }
    assert!(
        !titles(&made).iter().any(|t| t.contains("text columns")),
        "{:?}",
        titles(&made)
    );
    assert!(!made.says.iter().any(|n| n.key == "gathered"));
    assert!(
        !titles(&made).iter().any(|t| t.contains("NOTE")),
        "an undeclared column is left out by default"
    );
}

/// What the file states at a node: `STRA` and `LITHO`, or nothing.
type Stated = Option<(&'static str, &'static str)>;

/// To break: shift a rank by one row, attach a property to the table
/// of another group, or write a row for a node without a value.
#[test]
fn every_node_decodes_to_the_cells_the_file_states() {
    for mode in ["shared-table", "table-per-well"] {
        let made = plan(json!({ "textColumns": mode }));
        // What the file states, node by node — `None` where it states nothing.
        let expected: [(&str, Vec<Stated>); 2] = [
            ("F1", vec![Some(("e6", "marne")), None, Some(("e5", "calcaire"))]),
            ("F2", vec![Some(("e5", "calcaire")), None]),
        ];
        for (well, nodes) in expected {
            let pointer = format!("{well} — text columns");
            let ranks = codes(&made, &pointer);
            assert_eq!(ranks.len(), nodes.len(), "{mode}: one value per node of {well}");
            let table = table_of(&made, &pointer);
            let (_, stra) = table_column(table, "STRA");
            let (_, litho) = table_column(table, "LITHO");
            for (node, (rank, wanted)) in ranks.iter().zip(&nodes).enumerate() {
                match wanted {
                    None => assert_eq!(*rank, -9999, "{mode}: {well} node {node} states nothing, a null"),
                    Some((s, l)) => {
                        let at = usize::try_from(*rank).expect("a rank, not a null");
                        assert_eq!(
                            (stra[at].as_str(), litho[at].as_str()),
                            (*s, *l),
                            "{mode}: {well} node {node}"
                        );
                    }
                }
            }
        }
    }
}

/// To break: a counter per borehole in the shared mode, or one more table.
#[test]
fn a_shared_table_holds_each_combination_once_for_the_whole_run() {
    let made = plan(json!({ "textColumns": "shared-table" }));
    let tables: Vec<&NewObject> = made
        .objects
        .iter()
        .filter(|o| o.qualified_type == "eml23.ColumnBasedTable")
        .collect();
    assert_eq!(tables.len(), 1, "one table for the run: {:?}", titles(&made));
    // `e5/calcaire` is met in F1 and in F2: one row, the same rank in both.
    let (_, stra) = table_column(tables[0], "STRA");
    assert_eq!(stra, ["e6", "e5"], "two distinct combinations, in the order met");
    assert_eq!(
        codes(&made, "F1 — text columns")[2],
        codes(&made, "F2 — text columns")[0]
    );
    // No placeholder is left behind: the lookup names the table.
    assert_eq!(table_of(&made, "F1 — text columns").uuid, tables[0].uuid);
}

/// To break: share the table between boreholes in `table-per-well` mode.
#[test]
fn a_table_per_well_belongs_to_its_frame() {
    let made = plan(json!({ "textColumns": "table-per-well" }));
    let one = table_of(&made, "F1 — text columns");
    let two = table_of(&made, "F2 — text columns");
    assert_ne!(one.uuid, two.uuid);
    // A row per node that carries a value, in the frame's order.
    assert_eq!(table_column(one, "STRA").1, ["e6", "e5"]);
    assert_eq!(table_column(two, "STRA").1, ["e5"]);
    assert_eq!(codes(&made, "F1 — text columns"), [0, -9999, 1]);
}

/// To break: write the number column in the table, or leave a text column
/// as a property of its own.
#[test]
fn numbers_stay_continuous_and_text_leaves_its_own_properties() {
    let made = plan(json!({ "textColumns": "shared-table" }));
    assert_eq!(
        object(&made, "F1 — PHI").qualified_type,
        "resqml22.ContinuousProperty"
    );
    assert!(
        !titles(&made)
            .iter()
            .any(|t| t == "F1 — STRA" || t == "F1 — LITHO"),
        "{:?}",
        titles(&made)
    );
    assert!(made.says.iter().any(|n| n.key == "gathered"));
}

/// To break: give a column the kind of another's template, or an abstract
/// kind to the referring property.
#[test]
fn each_column_keeps_its_kind_and_the_pointer_is_a_multi_use_property() {
    let made = plan(json!({ "textColumns": "shared-table" }));
    let table = table_of(&made, "F1 — text columns");
    assert_eq!(table_column(table, "STRA").0, STRA_KIND);
    assert_eq!(table_column(table, "LITHO").0, LITHO_KIND);
    let pointer = object(&made, "F1 — text columns");
    // `multi use property`, concrete in the dictionary — its meaning is the table.
    assert_eq!(
        pointer.json["PropertyKind"]["Uuid"],
        "47a4319d-c8a4-45dc-8600-f96d4b289138"
    );
    assert_eq!(pointer.json["IndexableElement"], "nodes");
}

/// To break: mix the columns outside the template with the declared columns, give them
/// an invented kind, or carry them without `everything`.
#[test]
fn everything_carries_the_undeclared_columns_in_a_table_of_their_own() {
    for mode in ["shared-table", "table-per-well"] {
        let made = plan(json!({ "textColumns": mode, "everything": true }));
        let pointer = "F1 — columns not in the template";
        let table = table_of(&made, pointer);
        let (kind, notes) = table_column(table, "NOTE");
        assert_eq!(
            kind, "73f28137-03ef-45b4-8e85-31869c7d36fd",
            "{mode}: the dictionary's `unknown`"
        );
        let ranks = codes(&made, pointer);
        // F1 says `vu` at 10 m, nothing at 20 m, an empty cell at 30 m.
        assert_eq!(notes[usize::try_from(ranks[0]).unwrap()], "vu", "{mode}");
        assert_eq!(&ranks[1..], [-9999, -9999], "{mode}: no value, no row");
        assert!(
            table_of(&made, "F1 — text columns").json["Column"]
                .as_array()
                .unwrap()
                .iter()
                .all(|c| c["Title"] != "NOTE")
        );
        assert!(made.says.iter().any(|n| n.key == "columns-carried"), "{mode}");
    }
    // Without it the column is not written at all.
    let made = plan(json!({ "textColumns": "shared-table" }));
    assert!(!titles(&made).iter().any(|t| t.contains("not in the template")));
}

/// To break: leave `everything` without effect in `separate` mode.
#[test]
fn everything_works_with_separate_text_columns_too() {
    let made = plan(json!({ "everything": true }));
    assert_eq!(
        object(&made, "F1 — STRA").qualified_type,
        "resqml22.DiscreteProperty",
        "declared text stays separate"
    );
    let table = table_of(&made, "F1 — columns not in the template");
    assert_eq!(
        table_column(table, "NOTE").1,
        ["vu", "revu"],
        "one table for the run: separate is not per well"
    );
}
