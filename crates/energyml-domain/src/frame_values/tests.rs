//! `frame_values` on values built by hand: the row of a table, a column as a
//! property, a value at an MD, values carried across representations.
//!
//! The form comes from the specification, the numbers are the tests' own:
//! common TRG 2.3 §9.1 p. 190 for the table (a key column makes it a lookup, row
//! identity is positional, the values of a column may be several per row), the
//! RESQML TRG 2.2 §7.1 for what an interval is (a node-to-node span, the frame's
//! `NodeCount - 1`). Reading a property from its object is tested in
//! `tests/frame_values.rs`, on the generated types.
//!
//! Each test says, in its last lines, what to break to see it fail.

use super::*;
use crate::along_md::BracketRefusal;
use crate::strata_by_md::MdReference;
use crate::table::{ColumnMeta, Lookup, Rows};

const KIND_THICKNESS: &str = "00000000-0000-0000-0000-0000000000a1";

/// A column of a table, as the tests write one.
struct Col {
    title: &'static str,
    key: bool,
    kind: Option<&'static str>,
    uom: Option<&'static str>,
    values: Values,
}

fn col(title: &'static str, key: bool, values: Values) -> Col {
    Col {
        title,
        key,
        kind: None,
        uom: None,
        values,
    }
}

fn categories(columns: Vec<Col>, table_type: &str) -> Categories {
    let rows = columns
        .iter()
        .map(|column| column.values.len())
        .min()
        .unwrap_or(0);
    let width = columns.len();
    let lookup = Lookup {
        titles: columns
            .iter()
            .map(|column| Some(column.title.to_string()))
            .collect(),
        key_columns: columns
            .iter()
            .enumerate()
            .filter(|(_, column)| column.key)
            .map(|(at, _)| at)
            .collect(),
        rows: Rows {
            columns: columns.iter().map(|column| Some(column.values.clone())).collect(),
            per_row: vec![1; width],
            nulls: vec![None; width],
            rows,
            notes: Vec::new(),
        },
        notes: Vec::new(),
    };
    let meta = columns
        .iter()
        .map(|column| ColumnMeta {
            title: Some(column.title.to_string()),
            description: None,
            uom: column.uom.map(str::to_string),
            kind: column.kind.map(|uuid| PropertyKindRef::Object {
                uuid: uuid.to_string(),
                title: None,
            }),
            key: column.key,
            values_per_row: 1,
        })
        .collect();
    Categories::new("table-0".to_string(), table_type.to_string(), lookup, meta)
}

/// The table of the tests: a key `10, 20, 35`, a name (the `code` kind) and a
/// thickness in metres, absent for the second row.
fn facies() -> Categories {
    categories(
        vec![
            col("STRA", true, Values::Integer(vec![10, 20, 35])),
            Col {
                kind: Some(CODE),
                ..col(
                    "Nom_STRA",
                    false,
                    Values::Text(vec!["Lutetian".into(), "Bartonian".into(), "Priabonian".into()]),
                )
            },
            Col {
                kind: Some(KIND_THICKNESS),
                uom: Some("m"),
                ..col("thickness", false, Values::Floating(vec![1.5, f64::NAN, 3.0]))
            },
        ],
        "ColumnBasedTable",
    )
}

fn values_of(support: Support, values: Vec<Option<Value>>, continuous: bool) -> FrameValues {
    FrameValues {
        frame: "f1".to_string(),
        property: "p1".to_string(),
        title: "P".to_string(),
        element: if support.is_intervals() {
            IndexableElement::Intervals
        } else {
            IndexableElement::Nodes
        },
        support,
        values,
        continuous,
        uom: Some("m".to_string()),
        kind: None,
        kind_uri: None,
        range: None,
        categories: None,
        time: None,
        realization: None,
        derivation: None,
        notes: Vec::new(),
    }
}

fn codes(codes: &[Option<i64>]) -> Vec<Option<Value>> {
    codes.iter().map(|code| code.map(Value::Code)).collect()
}

fn numbers(numbers: &[Option<f64>]) -> Vec<Option<Value>> {
    numbers.iter().map(|number| number.map(Value::Number)).collect()
}

fn nodes(md: &[f64]) -> Support {
    Support::Nodes { md: md.to_vec() }
}

/// The intervals of a frame at these node depths.
fn intervals(md: &[f64]) -> Support {
    Support::Intervals {
        top: md[..md.len() - 1].to_vec(),
        base: md[1..].to_vec(),
    }
}

// -- the row of a table ---------------------------------------------------------

/// A code names its row **by the key** (§9.1), and the row comes back whole, each
/// cell with the title, the kind and the unit of its column; a code no row holds
/// names nothing; the raw codes stay what the file stored.
///
/// Break it: in `Categories::row_of`, answer `Some(row + 1)` for `RowMatch::One`.
#[test]
fn a_code_gives_its_row_with_what_each_column_says_of_itself() {
    let mut property = values_of(
        nodes(&[0.0, 10.0, 20.0, 30.0]),
        codes(&[Some(20), Some(10), Some(15), None]),
        false,
    );
    property.categories = Some(facies());

    let row = property.row_at(0).expect("20 names the second row");
    assert_eq!(row.len(), 3);
    assert!(row[0].key && row[0].title.as_deref() == Some("STRA"));
    assert_eq!(row[0].value, Some(Cell::Number(20.0)));
    assert_eq!(row[1].title.as_deref(), Some("Nom_STRA"));
    assert_eq!(row[1].value, Some(Cell::Text("Bartonian".to_string())));
    assert!(matches!(&row[1].kind, Some(PropertyKindRef::Object { uuid, .. }) if uuid == CODE));
    assert_eq!(row[2].title.as_deref(), Some("thickness"));
    assert_eq!(row[2].uom.as_deref(), Some("m"));
    assert_eq!(row[2].value, None, "an absent cell is None, not NaN");

    assert_eq!(
        property.row_at(1).unwrap()[1].value,
        Some(Cell::Text("Lutetian".to_string()))
    );
    assert!(property.row_at(2).is_none(), "15 names no row");
    assert!(property.row_at(3).is_none(), "an absent value names none");
    assert_eq!(property.rows(), Some(vec![Some(1), Some(0), None, None]));
    assert_eq!(
        property.values[0],
        Some(Value::Code(20)),
        "the codes stay codes: the table is read through them, not instead of them"
    );
}

/// The label is one column among others: the one whose kind is `code`, else the
/// only text column, and none is guessed between two.
///
/// Break it: in `label_column`, return `texts.first().copied()` for any number of
/// text columns.
#[test]
fn the_label_is_the_code_column_or_the_only_text_column() {
    let table = facies();
    assert_eq!(table.label_column(), Some(1));
    assert_eq!(table.label(2).as_deref(), Some("Priabonian"));

    let one_text = categories(
        vec![
            col("k", true, Values::Integer(vec![1, 2])),
            col("name", false, Values::Text(vec!["a".into(), "b".into()])),
            col("share", false, Values::Floating(vec![0.1, 0.2])),
        ],
        "ColumnBasedTable",
    );
    assert_eq!(one_text.label_column(), Some(1), "the only text column");

    let two_texts = categories(
        vec![
            col("k", true, Values::Integer(vec![1, 2])),
            col("name", false, Values::Text(vec!["a".into(), "b".into()])),
            col("note", false, Values::Text(vec!["c".into(), "d".into()])),
        ],
        "ColumnBasedTable",
    );
    assert_eq!(
        two_texts.label_column(),
        None,
        "two texts and no kind: not guessed"
    );
}

/// The code of each row and its label: the key where the table has one, the rank where it has none, and nothing
/// for a row with no label.
///
/// Break it: in `Categories::labels`, take `row as f64` for the code of a keyed table too.
#[test]
fn a_table_gives_the_code_and_the_label_of_each_row() {
    assert_eq!(
        facies().labels(),
        vec![
            (10.0, "Lutetian".to_string()),
            (20.0, "Bartonian".to_string()),
            (35.0, "Priabonian".to_string())
        ]
    );
    let keyless = categories(
        vec![col("name", false, Values::Text(vec!["a".into(), "b".into()]))],
        "ColumnBasedTable",
    );
    assert_eq!(
        keyless.labels(),
        vec![(0.0, "a".to_string()), (1.0, "b".to_string())]
    );
}

/// A table with no key is read by rank from 0, and never a keyed one: the one
/// rule is `row_named_by`. A key column of text that reads as numbers names its
/// rows by those numbers, after the exact match.
///
/// Break it: delete the `text_numbers` fallback in `Categories::row_of`.
#[test]
fn a_code_names_a_row_by_rank_without_a_key_and_by_a_text_key_that_reads_as_a_number() {
    let keyless = categories(
        vec![col("name", false, Values::Text(vec!["a".into(), "b".into()]))],
        "ColumnBasedTable",
    );
    assert_eq!(keyless.row_of(&Value::Code(1)), Some(1));
    assert_eq!(keyless.row_of(&Value::Code(2)), None);

    let texts = categories(
        vec![
            col("k", true, Values::Text(vec!["1".into(), "x".into(), " 3".into()])),
            col(
                "name",
                false,
                Values::Text(vec!["a".into(), "b".into(), "c".into()]),
            ),
        ],
        "ColumnBasedTable",
    );
    assert_eq!(texts.row_of(&Value::Code(3)), Some(2));
    assert_eq!(texts.row_of(&Value::Code(2)), None);
    assert_eq!(texts.row_of(&Value::Text("x".to_string())), Some(1));
}

// -- a column as a property -----------------------------------------------------

fn with_table(codes_at_nodes: &[Option<i64>]) -> FrameValues {
    let mut property = values_of(nodes(&[0.0, 10.0, 20.0, 30.0]), codes(codes_at_nodes), false);
    property.categories = Some(facies());
    property
}

/// A numeric column gives a continuous property of the same support, in the unit
/// and of the kind of the column, nothing stored.
///
/// Break it: in `derive`, take `self.uom.clone()` instead of `meta.uom.clone()`
/// (the codes have no unit: the test then sees `None`, not metres from the column).
#[test]
fn a_numeric_column_is_a_continuous_property_of_the_same_support() {
    let mut property = with_table(&[Some(35), Some(10), None, Some(20)]);
    property.uom = None;
    let derived = property.derive(2).expect("the thickness column is numeric");
    assert_eq!(
        derived.values,
        vec![Some(Value::Number(3.0)), Some(Value::Number(1.5)), None, None],
        "a value that was absent stays absent; a cell that is absent in the column is absent"
    );
    assert_eq!(derived.uom.as_deref(), Some("m"));
    assert!(matches!(&derived.kind, Some(PropertyKindRef::Object { uuid, .. }) if uuid == KIND_THICKNESS));
    assert!(derived.continuous);
    assert!(derived.categories.is_none());
    assert_eq!(derived.support, property.support);
    assert_eq!(derived.range, Some(Range { min: 1.5, max: 3.0 }));
    assert_eq!(derived.title, "derived from P by thickness");
    let made_of = derived.derivation.expect("it says what it is made of");
    assert_eq!((made_of.property.as_str(), made_of.column), ("p1", 2));
}

/// Each refusal is named: no table, a function table, a column that does not
/// exist, is not numeric or holds several values per row, a repeated key, a code
/// with no row, a value that is not a code.
///
/// Break it: in `derive`, delete the `duplicate_keys` check (the repeated-key case
/// then answers instead of refusing).
#[test]
fn deriving_a_property_refuses_by_name() {
    let property = with_table(&[Some(10)]);
    assert!(
        matches!(property.derive(1), Err(DeriveRefusal::NotNumeric { .. })),
        "a text column"
    );
    assert!(matches!(
        property.derive(9),
        Err(DeriveRefusal::NoSuchColumn {
            column: 9,
            columns: 3
        })
    ));

    let mut several = with_table(&[Some(10)]);
    several.categories.as_mut().unwrap().columns[2].values_per_row = 2;
    assert!(matches!(
        several.derive(2),
        Err(DeriveRefusal::SeveralValuesPerRow { per_row: 2, .. })
    ));

    let mut none = values_of(nodes(&[0.0, 10.0]), codes(&[Some(1), Some(2)]), false);
    assert!(matches!(none.derive(0), Err(DeriveRefusal::NoTable { .. })));

    none.categories = Some(categories(
        vec![
            col("k", true, Values::Floating(vec![1.0, 2.0])),
            col("v", false, Values::Floating(vec![5.0, 6.0])),
        ],
        "DoubleTableLookup",
    ));
    assert!(
        matches!(none.derive(1), Err(DeriveRefusal::FunctionTable { .. })),
        "a function of a real key is not read per code"
    );

    let absent = with_table(&[Some(10), Some(15)]);
    match absent.derive(2).err() {
        Some(DeriveRefusal::KeyAbsent { at: 1, code }) => assert_eq!(code, "15"),
        other => panic!("15 names no row: {other:?}"),
    }

    let mut text = with_table(&[Some(10)]);
    text.values = vec![Some(Value::Text("x".to_string()))];
    assert!(matches!(text.derive(2), Err(DeriveRefusal::NotACode { at: 0 })));

    let mut repeated = with_table(&[Some(10)]);
    repeated.categories = Some(categories(
        vec![
            col("k", true, Values::Integer(vec![10, 10, 35])),
            col("v", false, Values::Floating(vec![1.0, 2.0, 3.0])),
        ],
        "ColumnBasedTable",
    ));
    assert!(matches!(
        repeated.derive(1),
        Err(DeriveRefusal::DuplicateKeys { .. })
    ));
}

// -- at an MD -------------------------------------------------------------------

/// On nodes, a continuous value is interpolated between two nodes and is the
/// node's own on a node; **never over a hole** (an absent neighbour, a gap wider
/// than the rule), and an absent value on a node is absent.
///
/// Break it: in `at_md_on_nodes`, replace `let (a, b) = (number(lower)?, number(upper)?);`
/// by `let (a, b) = (number(lower)?.or(Some(0.0)), number(upper)?.or(Some(0.0)));`.
#[test]
fn a_continuous_value_is_interpolated_between_nodes_and_never_over_a_hole() {
    let property = values_of(
        nodes(&[0.0, 10.0, 20.0, 30.0]),
        numbers(&[Some(10.0), Some(20.0), None, Some(40.0)]),
        true,
    );
    let middle = property.at_md(5.0, Side::Below, "linear").unwrap();
    assert_eq!(middle.value, Some(Value::Number(15.0)));
    assert_eq!(
        middle.placement,
        Placement::Between {
            lower: 0,
            upper: 1,
            t: 0.5
        }
    );
    let on_node = property.at_md(10.0, Side::Below, "linear").unwrap();
    assert_eq!(
        (on_node.value, on_node.placement),
        (Some(Value::Number(20.0)), Placement::Node(1))
    );
    assert_eq!(
        property.at_md(15.0, Side::Below, "linear"),
        Err(AtMdRefusal::MissingNeighbour {
            lower: 1,
            upper: 2,
            missing: 2
        })
    );
    let absent = property.at_md(20.0, Side::Below, "linear").unwrap();
    assert_eq!((absent.value, absent.placement), (None, Placement::Node(2)));
    assert!(matches!(
        property.at_md(35.0, Side::Below, "linear"),
        Err(AtMdRefusal::Placement(BracketRefusal::Outside { .. }))
    ));
}

/// A gap wider than the rule is not bridged (ADR-0027: three times the median
/// step), and a method that does not exist is named.
///
/// Break it: in `at_md_on_nodes`, delete the `GapTooWide` arm of the `match reason`
/// (the gap then answers with a method refusal, not the gap).
#[test]
fn a_gap_wider_than_the_rule_is_not_bridged() {
    let property = values_of(
        nodes(&[0.0, 1.0, 2.0, 3.0, 100.0]),
        numbers(&[Some(1.0), Some(2.0), Some(3.0), Some(4.0), Some(5.0)]),
        true,
    );
    assert_eq!(
        property.at_md(50.0, Side::Below, "linear"),
        Err(AtMdRefusal::GapTooWide {
            lower: 3,
            upper: 4,
            gap: 97.0,
            max: 3.0
        })
    );
    assert!(property.at_md(1.5, Side::Below, "linear").is_ok());
    assert_eq!(
        property.at_md(1.5, Side::Below, "cubic"),
        Err(AtMdRefusal::UnknownMethod {
            id: "cubic".to_string()
        })
    );
    let log = values_of(nodes(&[0.0, 10.0]), numbers(&[Some(10.0), Some(1000.0)]), true);
    let value = log.at_md(5.0, Side::Below, "log_linear").unwrap().value;
    assert!(
        matches!(value, Some(Value::Number(x)) if (x - 100.0).abs() < 1e-9),
        "{value:?}"
    );
}

/// A code is held at its node and never interpolated: between two nodes it is
/// refused, on a node it is the node's.
///
/// Break it: delete the `if !self.continuous` refusal in `at_md_on_nodes`.
#[test]
fn a_discrete_value_is_held_at_its_node_and_refused_between() {
    let property = values_of(
        nodes(&[0.0, 10.0, 20.0]),
        codes(&[Some(1), Some(2), Some(3)]),
        false,
    );
    assert_eq!(
        property.at_md(5.0, Side::Below, "linear"),
        Err(AtMdRefusal::DiscreteBetweenNodes { lower: 0, upper: 1 })
    );
    assert_eq!(
        property.at_md(10.0, Side::Below, "linear").unwrap().value,
        Some(Value::Code(2))
    );
}

/// On intervals: the interval that holds the MD; on a node between two, the side
/// asked; the deepest node closes the last interval; above the top there is no
/// interval above it.
///
/// Break it: in `at_md_in_intervals`, drop the `.min(count.saturating_sub(1))` of
/// the `Below` side (the deepest node then names an interval that does not exist).
#[test]
fn on_intervals_an_md_on_a_node_takes_the_side_asked() {
    let property = values_of(
        intervals(&[0.0, 10.0, 20.0, 30.0]),
        codes(&[Some(1), Some(2), None]),
        false,
    );
    let at = |md: f64, side: Side| property.at_md(md, side, "linear");
    assert_eq!(at(5.0, Side::Below).unwrap().placement, Placement::Interval(0));
    assert_eq!(at(10.0, Side::Below).unwrap().value, Some(Value::Code(2)));
    assert_eq!(at(10.0, Side::Above).unwrap().value, Some(Value::Code(1)));
    let deepest = at(30.0, Side::Below).unwrap();
    assert_eq!((deepest.placement, deepest.value), (Placement::Interval(2), None));
    assert_eq!(at(30.0, Side::Above).unwrap().placement, Placement::Interval(2));
    assert_eq!(at(0.0, Side::Below).unwrap().placement, Placement::Interval(0));
    assert_eq!(at(0.0, Side::Above), Err(AtMdRefusal::NothingAbove { md: 0.0 }));
    assert!(matches!(at(31.0, Side::Below), Err(AtMdRefusal::Placement(_))));
}

// -- carried across representations ---------------------------------------------

fn reference(trajectory: &str, datum: &str) -> MdReference {
    MdReference {
        trajectory: trajectory.to_string(),
        datum: Some(datum.to_string()),
        uom: Some("m".to_string()),
        domain: None,
    }
}

fn frame_holding(frame: &str, values: Vec<Option<Value>>) -> FrameValues {
    let mut held = values_of(intervals(&[0.0, 10.0, 20.0]), values, false);
    held.frame = frame.to_string();
    held.title = format!("P of {frame}");
    held
}

/// A node takes the value of the interval it falls in (`[top, base)`, the last
/// `[top, base]`), or nothing: outside the frames it is [`Carry::Outside`], counted
/// and never given a neighbour's value.
///
/// Break it: in `carry`, treat `Err(AtMdRefusal::Placement(_))` as `held = true`
/// (the nodes outside then come back absent, not outside).
#[test]
fn a_node_takes_the_value_of_the_interval_it_falls_in_or_nothing() {
    let frame = frame_holding("f1", codes(&[Some(1), Some(2)]));
    let on_frame = reference("traj-f", "d");
    let target = reference("traj-t", "d");
    let stated = [Stated {
        values: &frame,
        reference: &on_frame,
    }];
    let carried = carry(
        "R",
        "a trajectory",
        &[-1.0, 0.0, 5.0, 10.0, 15.0, 20.0, 25.0],
        &target,
        &stated,
    )
    .expect("the same datum, unit and domain");
    assert_eq!(
        carried.nodes,
        vec![
            Carry::Outside,
            Carry::Value(Value::Code(1)),
            Carry::Value(Value::Code(1)),
            Carry::Value(Value::Code(2)),
            Carry::Value(Value::Code(2)),
            Carry::Value(Value::Code(2)),
            Carry::Outside,
        ]
    );
    assert_eq!((carried.named(), carried.outside()), (5, 2));
    assert!(
        carried.notes.iter().any(|note| note.contains("2 of 7")),
        "{:?}",
        carried.notes
    );
    assert_eq!(carried.from, vec!["f1".to_string()]);
}

/// MDs that count from two datums are never compared: the frame is set aside by
/// name and, none left, the representation is refused - the rule of
/// `strata_by_md`, not a second one.
///
/// Break it: in `carry`, replace `comparable(target, one.reference)` by
/// `Ok::<f64, String>(1.0)`.
#[test]
fn mds_from_another_datum_are_not_compared() {
    let frame = frame_holding("f1", codes(&[Some(1), Some(2)]));
    let on_frame = reference("traj-f", "datum-a");
    let target = reference("traj-t", "datum-b");
    let stated = [Stated {
        values: &frame,
        reference: &on_frame,
    }];
    let refused = carry("R", "a trajectory", &[5.0], &target, &stated);
    match refused {
        Err(Refusal::Incomparable { why, .. }) => assert!(why.contains("datum"), "{why}"),
        other => panic!("two datums: {:?}", other.map(|carried| carried.nodes)),
    }
}

/// Two frames naming two different values at one MD are a disagreement - the node
/// has none, neither frame is chosen; a frame that says nothing there does not
/// disagree with one that says something.
///
/// Break it: in `carry`, compare only `named.len() == 1` after keeping the first
/// value of the frames (take `named.truncate(1)` before the `match`).
#[test]
fn two_frames_that_name_two_values_disagree_and_one_that_says_nothing_does_not() {
    let one = frame_holding("f1", codes(&[Some(1), Some(2)]));
    let other = frame_holding("f2", codes(&[Some(1), Some(3)]));
    let silent = frame_holding("f3", codes(&[None, Some(2)]));
    let target = reference("traj-t", "d");
    let on_f1 = reference("traj-1", "d");
    let on_f2 = reference("traj-2", "d");
    let on_f3 = reference("traj-3", "d");

    let carried = carry(
        "R",
        "a trajectory",
        &[5.0, 15.0],
        &target,
        &[
            Stated {
                values: &one,
                reference: &on_f1,
            },
            Stated {
                values: &other,
                reference: &on_f2,
            },
        ],
    )
    .unwrap();
    assert_eq!(
        carried.nodes,
        vec![Carry::Value(Value::Code(1)), Carry::Disagreement]
    );
    assert_eq!(carried.disagreements(), 1);
    assert!(
        carried.notes.iter().any(|note| note.contains("`P of f1` says 2")),
        "{:?}",
        carried.notes
    );

    let carried = carry(
        "R",
        "a trajectory",
        &[5.0],
        &target,
        &[
            Stated {
                values: &one,
                reference: &on_f1,
            },
            Stated {
                values: &silent,
                reference: &on_f3,
            },
        ],
    )
    .unwrap();
    assert_eq!(
        carried.nodes,
        vec![Carry::Value(Value::Code(1))],
        "a null says: not known here"
    );
}
