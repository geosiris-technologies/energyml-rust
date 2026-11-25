//! The values of a property along a wellbore frame, read from objects built in
//! memory from the generated types - and the two readers rewritten on top of
//! them, `wellbore_table` and `well_strata`.
//!
//! Every object is written after the specification, not after a package: RESQML
//! TRG 2.2 §7.1 (a property hangs on an indexable element and holds
//! `ValueCountPerIndexableElement` values per element; the frame's intervals are
//! its `NodeCount - 1` node-to-node spans), common TRG 2.3 §9.1 p. 190 (a table
//! is columns, `ValueCountPerRow` values per row, a key column makes it a
//! lookup). Nothing is read from a file.
//!
//! # What it would take to make each one fail
//!
//! - comparing the element with `intervals` alone, as `wellbore_table` did (issue
//!   #34): the `intervals from datum` properties below are then refused;
//! - accepting a count that is not the support's, taking several patches or a
//!   vector for one value per element, or a property of another representation;
//! - reading a code by its position rather than its key, or by its label.

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_core::xml::XmlText;
use energyml_domain::frame_values::{self, Refusal, Support, Value};
use energyml_domain::source::Prefetched;
use energyml_domain::table::{CODE, Cell};
use energyml_domain::well_strata::{self, Known, Wanted};
use energyml_domain::wellbore_table;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_storage::array::ArrayLocation;

const FRAME: &str = "f0000000-0000-4000-8000-000000000001";
const TRAJECTORY: &str = "f0000000-0000-4000-8000-000000000002";
const OTHER_FRAME: &str = "f0000000-0000-4000-8000-000000000009";
const PROPERTY: &str = "c0000000-0000-4000-8000-000000000001";
const TABLE: &str = "c0000000-0000-4000-8000-000000000002";
const CODE_KIND: &str = "5f78f66a-ed1b-4827-a868-beb989febb31";
const THICKNESS_KIND: &str = "00000000-0000-0000-0000-0000000000a1";

// -- building blocks ---------------------------------------------------------

/// No array lives outside the document here: every array is inline XML.
fn nowhere(_: &dyn Reflect) -> Vec<ArrayLocation> {
    Vec::new()
}

fn boxed<T: DynObject + 'static>(object: T) -> Box<dyn DynObject> {
    Box::new(object)
}

/// A package held in memory.
fn held(objects: Vec<Box<dyn DynObject>>) -> Prefetched {
    let mut cache = Prefetched::new();
    for object in objects {
        cache.insert(object, &nowhere);
    }
    cache
}

fn dor(uuid: &str, qualified_type: &str, title: &str) -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: uuid.to_string(),
        qualified_type: qualified_type.to_string(),
        title: title.to_string(),
        ..Default::default()
    }
}

fn citation(title: &str) -> eml23::Citation {
    eml23::Citation {
        title: title.to_string(),
        ..Default::default()
    }
}

fn integers(values: &[i64]) -> eml23::IntegerXmlArray {
    eml23::IntegerXmlArray {
        count_per_value: 1,
        null_value: -1,
        values: values.to_vec(),
        ..Default::default()
    }
}

fn floats(values: &[f64]) -> eml23::FloatingPointXmlArray {
    eml23::FloatingPointXmlArray {
        count_per_value: 1,
        values: values.to_vec(),
        ..Default::default()
    }
}

/// A 2.2 frame at these measured depths.
fn frame(mds: &[f64]) -> resqml22::WellboreFrameRepresentation {
    resqml22::WellboreFrameRepresentation {
        uuid: FRAME.to_string(),
        citation: citation("F1 - frame"),
        node_count: mds.len() as i64,
        node_md: Some(eml23::AbstractFloatingPointArray::FloatingPointXmlArray(floats(
            mds,
        ))),
        trajectory: dor(TRAJECTORY, "resqml22.WellboreTrajectoryRepresentation", "F1"),
        ..Default::default()
    }
}

/// A continuous property, in metres, on this element of the frame.
fn continuous(element: eml23::IndexableElement, values: &[f64]) -> resqml22::ContinuousProperty {
    resqml22::ContinuousProperty {
        uuid: PROPERTY.to_string(),
        citation: citation("F1 - thickness"),
        indexable_element: element,
        value_count_per_indexable_element: vec![1],
        supporting_representation: dor(FRAME, "resqml22.WellboreFrameRepresentation", "F1 - frame"),
        uom: eml23::UnitOfMeasureExt::from_xml_text("m").expect("a unit"),
        values_for_patch: vec![resqml22::AbstractValueArray::FloatingPointXmlArray(floats(
            values,
        ))],
        ..Default::default()
    }
}

/// A discrete property naming the table, on this element of the frame.
fn discrete(element: eml23::IndexableElement, codes: &[i64]) -> resqml22::DiscreteProperty {
    resqml22::DiscreteProperty {
        uuid: PROPERTY.to_string(),
        citation: citation("F1 - STRA"),
        indexable_element: element,
        value_count_per_indexable_element: vec![1],
        supporting_representation: dor(FRAME, "resqml22.WellboreFrameRepresentation", "F1 - frame"),
        values_for_patch: vec![resqml22::AbstractValueArray::IntegerXmlArray(integers(codes))],
        category_lookup: Some(dor(TABLE, "eml23.ColumnBasedTable", "STRA")),
        ..Default::default()
    }
}

fn text_column(title: &str, kind: Option<&str>, values: &[&str]) -> eml23::Column {
    eml23::Column {
        title: Some(title.to_string()),
        value_count_per_row: 1,
        property_kind: kind
            .map(|uuid| dor(uuid, "eml23.PropertyKind", "code"))
            .unwrap_or_default(),
        values: Some(eml23::AbstractValueArray::StringXmlArray(eml23::StringXmlArray {
            count_per_value: 1,
            values: values.iter().map(|value| value.to_string()).collect(),
            ..Default::default()
        })),
        ..Default::default()
    }
}

/// A table keyed `10, 20, 35`: a name, and - when asked - a thickness in metres.
fn table(with_thickness: bool) -> eml23::ColumnBasedTable {
    let mut column = vec![text_column(
        "Nom_STRA",
        Some(CODE_KIND),
        &["Lutetian", "Bartonian", "Priabonian"],
    )];
    if with_thickness {
        column.push(eml23::Column {
            title: Some("thickness".to_string()),
            value_count_per_row: 1,
            uom: Some(eml23::UnitOfMeasureExt::from_xml_text("m").expect("a unit")),
            property_kind: dor(THICKNESS_KIND, "eml23.PropertyKind", "thickness"),
            values: Some(eml23::AbstractValueArray::FloatingPointXmlArray(floats(&[
                1.5,
                f64::NAN,
                3.0,
            ]))),
            ..Default::default()
        });
    }
    eml23::ColumnBasedTable {
        uuid: TABLE.to_string(),
        citation: citation("STRA"),
        column,
        key_column: vec![eml23::Column {
            title: Some("STRA".to_string()),
            value_count_per_row: 1,
            values: Some(eml23::AbstractValueArray::IntegerXmlArray(integers(&[
                10, 20, 35,
            ]))),
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn asked<T: DynObject + 'static>(mds: &[f64], property: &T) -> Result<frame_values::FrameValues, Refusal>
where
    T: Clone,
{
    frame_values::of(
        &frame(mds),
        property.as_reflect(),
        &held(vec![boxed(property.clone())]),
    )
}

// -- the support, and issue #34 ---------------------------------------------------

/// `intervals from datum` hangs on the intervals of a frame like `intervals`
/// does: the support, the values and the range are the same (the schema says what
/// a frame's intervals are; this spelling is one more way to say it).
///
/// Break it: in `IndexableElement::is_frame_interval`, delete
/// `| IndexableElement::IntervalsFromDatum`.
#[test]
fn intervals_from_datum_is_the_intervals_of_the_frame() {
    let mds = [0.0, 5.0, 12.0, 20.0];
    let plain = asked(
        &mds,
        &continuous(eml23::IndexableElement::Intervals, &[0.1, 0.2, 0.3]),
    )
    .expect("intervals");
    let from_datum = asked(
        &mds,
        &continuous(eml23::IndexableElement::IntervalsFromDatum, &[0.1, 0.2, 0.3]),
    )
    .expect("intervals from datum is read like intervals");

    assert_eq!(
        from_datum.support,
        Support::Intervals {
            top: vec![0.0, 5.0, 12.0],
            base: vec![5.0, 12.0, 20.0]
        }
    );
    assert_eq!(from_datum.support, plain.support);
    assert_eq!(from_datum.values, plain.values);
    assert_eq!(
        from_datum.values,
        vec![
            Some(Value::Number(0.1)),
            Some(Value::Number(0.2)),
            Some(Value::Number(0.3))
        ]
    );
    assert_eq!(from_datum.uom.as_deref(), Some("m"));
    assert_eq!(
        from_datum.range.map(|range| (range.min, range.max)),
        Some((0.1, 0.3))
    );
    assert_eq!(from_datum.frame, FRAME);
    assert!(from_datum.continuous);
}

/// Nodes carry one value per node; the file's absence is `None`, never a zero or
/// a `NaN`, and it is left out of the range.
///
/// Break it: in `convert`, keep every floating value (`Some(Value::Number(number))`).
#[test]
fn a_node_property_reads_its_nodes_and_leaves_an_absence_out_of_its_range() {
    let read = asked(
        &[0.0, 10.0, 20.0, 30.0],
        &continuous(eml23::IndexableElement::Nodes, &[1.0, f64::NAN, 3.0, 2.0]),
    )
    .expect("one value per node");
    assert_eq!(
        read.support,
        Support::Nodes {
            md: vec![0.0, 10.0, 20.0, 30.0]
        }
    );
    assert_eq!(
        read.values,
        vec![
            Some(Value::Number(1.0)),
            None,
            Some(Value::Number(3.0)),
            Some(Value::Number(2.0))
        ]
    );
    assert_eq!(read.range.map(|range| (range.min, range.max)), Some((1.0, 3.0)));
}

// -- what is refused, by name -----------------------------------------------------

/// A number of values that is not the number of nodes or of intervals would shift
/// every value by a rank: refused, and said how many were found and expected.
///
/// Break it: in `of_with_mds`, make the count test `if false && stored.len() != expected`.
#[test]
fn a_count_that_is_not_the_support_is_refused() {
    let refused = asked(
        &[0.0, 10.0, 20.0, 30.0],
        &continuous(eml23::IndexableElement::Nodes, &[1.0, 2.0, 3.0]),
    )
    .err()
    .expect("3 values for 4 nodes");
    assert!(
        matches!(
            refused,
            Refusal::Count {
                found: 3,
                expected: 4,
                intervals: false,
                ..
            }
        ),
        "{refused}"
    );
    let refused = asked(
        &[0.0, 10.0, 20.0, 30.0],
        &continuous(eml23::IndexableElement::Intervals, &[1.0, 2.0, 3.0, 4.0]),
    )
    .err()
    .expect("4 values for 3 intervals");
    assert!(
        matches!(
            refused,
            Refusal::Count {
                found: 4,
                expected: 3,
                intervals: true,
                ..
            }
        ),
        "{refused}"
    );
}

/// Several patches, several values per element and `RealizationIndices` each
/// mean a choice the property does not make for the caller.
///
/// Break it: in `of_with_mds`, delete the `Refusal::Vector` return (the vector then
/// fails as a count, and the first assertion no longer matches).
#[test]
fn a_property_with_several_patches_values_or_realizations_is_refused() {
    let mds = [0.0, 10.0, 20.0];

    let mut patches = continuous(eml23::IndexableElement::Nodes, &[1.0, 2.0, 3.0]);
    patches
        .values_for_patch
        .push(resqml22::AbstractValueArray::FloatingPointXmlArray(floats(&[
            4.0, 5.0, 6.0,
        ])));
    assert!(matches!(
        asked(&mds, &patches).err(),
        Some(Refusal::Patches { count: 2, .. })
    ));

    let mut vector = continuous(
        eml23::IndexableElement::Nodes,
        &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0],
    );
    vector.value_count_per_indexable_element = vec![3];
    assert!(matches!(
        asked(&mds, &vector).err(),
        Some(Refusal::Vector { per_element: 3, .. })
    ));

    let mut realizations = continuous(eml23::IndexableElement::Nodes, &[1.0, 2.0, 3.0]);
    realizations.realization_indices = Some(eml23::AbstractIntegerArray::IntegerXmlArray(integers(&[1, 2])));
    assert!(matches!(
        asked(&mds, &realizations).err(),
        Some(Refusal::Realizations { .. })
    ));

    let mut empty = continuous(eml23::IndexableElement::Nodes, &[]);
    empty.values_for_patch.clear();
    assert!(matches!(
        asked(&mds, &empty).err(),
        Some(Refusal::NoValues { .. })
    ));
}

/// A property of another representation, or one that hangs on neither the nodes
/// nor the intervals, has no place along this frame.
///
/// Break it: in `of_with_mds`, replace the `eq_ignore_ascii_case(frame)` test by
/// `true`.
#[test]
fn a_property_that_does_not_hang_on_this_frame_is_refused() {
    let mds = [0.0, 10.0, 20.0];
    let mut elsewhere = continuous(eml23::IndexableElement::Nodes, &[1.0, 2.0, 3.0]);
    elsewhere.supporting_representation =
        dor(OTHER_FRAME, "resqml22.WellboreFrameRepresentation", "F2 - frame");
    match asked(&mds, &elsewhere).err() {
        Some(Refusal::NotOnThisFrame { supporting, .. }) => {
            assert_eq!(supporting.as_deref(), Some(OTHER_FRAME));
        }
        other => panic!("another frame: {other:?}"),
    }
    // A caller that found the property by `hangs_on` does not ask again.
    let given = frame_values::of_given(FRAME, &mds, &elsewhere, &held(Vec::new()));
    assert!(given.is_ok(), "the caller already chose it");

    let cells = continuous(eml23::IndexableElement::Cells, &[1.0, 2.0, 3.0]);
    assert!(matches!(
        asked(&mds, &cells).err(),
        Some(Refusal::Element {
            element: energyml_domain::property::IndexableElement::Cells,
            ..
        })
    ));
}

/// A frame states its depths in `NodeMd`: an object that does not is no frame.
///
/// Break it: in `node_mds`, return `Ok(Vec::new())` instead of `NotAFrame`.
#[test]
fn what_is_no_frame_is_refused() {
    let not_a_frame = continuous(eml23::IndexableElement::Nodes, &[1.0]);
    let refused = frame_values::node_mds(&not_a_frame, &held(Vec::new())).expect_err("no NodeMd");
    assert!(matches!(refused, Refusal::NotAFrame), "{refused}");
}

// -- a categorical property ---------------------------------------------------------

/// The codes stay codes, and a code gives its row of the table - **by the key**,
/// the keys here are `10, 20, 35`, so a position lookup finds no row for any -
/// with each column's title, kind, unit and value.
///
/// Break it: in `Categories::row_of`, replace `Value::Code(code) => Cell::Number(*code as f64),`
/// by `Value::Code(code) => Cell::Number(*code as f64 - 10.0),` (the key read as a rank).
#[test]
fn a_categorical_property_gives_its_codes_and_the_row_each_names() {
    let property = discrete(eml23::IndexableElement::IntervalsFromDatum, &[20, 10, 35, -1]);
    let source = held(vec![
        boxed(frame(&[0.0, 5.0, 12.0, 20.0, 31.0])),
        boxed(table(true)),
        boxed(property.clone()),
    ]);
    let read = frame_values::of(&frame(&[0.0, 5.0, 12.0, 20.0, 31.0]), &property, &source)
        .expect("a property of the intervals that names its table");

    assert_eq!(
        read.values,
        vec![
            Some(Value::Code(20)),
            Some(Value::Code(10)),
            Some(Value::Code(35)),
            None
        ],
        "the codes, with the array's NullValue as an absence"
    );
    assert!(!read.continuous);
    assert_eq!(read.range.map(|range| (range.min, range.max)), Some((10.0, 35.0)));
    let categories = read.categories.as_ref().expect("it names a table");
    assert_eq!(categories.table, TABLE);
    assert_eq!(categories.table_type, "ColumnBasedTable");
    assert_eq!(read.rows(), Some(vec![Some(1), Some(0), Some(2), None]));

    let row = read.row_at(0).expect("20 names the second row");
    let titles: Vec<Option<&str>> = row.iter().map(|cell| cell.title.as_deref()).collect();
    assert_eq!(
        titles,
        [Some("STRA"), Some("Nom_STRA"), Some("thickness")],
        "the key first"
    );
    assert!(row[0].key && !row[1].key);
    assert_eq!(row[0].value, Some(Cell::Number(20.0)));
    assert_eq!(row[1].value, Some(Cell::Text("Bartonian".to_string())));
    assert!(matches!(&row[1].kind,
        Some(energyml_domain::property::PropertyKindRef::Object { uuid, .. }) if uuid == CODE));
    assert_eq!(row[2].uom.as_deref(), Some("m"));
    assert_eq!(
        row[2].value, None,
        "the thickness of that row is absent in the file"
    );
    assert_eq!(categories.label_column(), Some(1), "the column of kind `code`");
    assert_eq!(categories.label(1).as_deref(), Some("Bartonian"));
    assert!(read.row_at(3).is_none());
}

/// A property that names a table the package does not hold is refused - its codes
/// would be raw numbers under a label.
///
/// Break it: in `read_along`, replace `Some(uuid) => Some(categories_of(`..`)?),` by
/// `Some(_) => None,` (the property then reads without its table).
#[test]
fn a_table_the_package_does_not_hold_is_refused_by_name() {
    let property = discrete(eml23::IndexableElement::Nodes, &[10, 20]);
    let refused = frame_values::of(
        &frame(&[0.0, 10.0]),
        &property,
        &held(vec![boxed(property.clone())]),
    )
    .err()
    .expect("no table in the package");
    assert!(matches!(refused, Refusal::LookupAbsent { .. }), "{refused}");
    assert!(refused.to_string().contains(TABLE), "{refused}");
}

/// A column of the table as a property of the same support, and a code it cannot
/// name is refused.
///
/// Break it: in `FrameValues::derive`, replace `meta.uom.clone()` by `None`.
#[test]
fn a_numeric_column_of_the_table_is_a_property_of_the_same_support() {
    let mds = [0.0, 5.0, 12.0, 20.0, 31.0];
    let property = discrete(eml23::IndexableElement::Intervals, &[35, 10, 20, -1]);
    let source = held(vec![boxed(table(true)), boxed(property.clone())]);
    let read = frame_values::of(&frame(&mds), &property, &source).expect("read");

    let derived = read.derive(2).expect("the thickness column is numeric");
    assert_eq!(
        derived.values,
        vec![Some(Value::Number(3.0)), Some(Value::Number(1.5)), None, None]
    );
    assert_eq!(derived.uom.as_deref(), Some("m"));
    assert_eq!(derived.support, read.support);

    assert!(matches!(
        read.derive(1).err(),
        Some(frame_values::DeriveRefusal::NotNumeric { .. })
    ));
    let bad = discrete(eml23::IndexableElement::Intervals, &[35, 15, 20, -1]);
    let source = held(vec![boxed(table(true)), boxed(bad.clone())]);
    let read = frame_values::of(&frame(&mds), &bad, &source).expect("read");
    assert!(matches!(
        read.derive(2).err(),
        Some(frame_values::DeriveRefusal::KeyAbsent { at: 1, .. })
    ));
}

// -- the readers rewritten on top of it -----------------------------------------------

/// **Issue #34.** A frame whose properties are `intervals from datum` is a table of
/// passes - a top, a base, a value - exactly like one whose properties are
/// `intervals`; it was a table of nodes, every one of its properties refused.
///
/// Break it: in `wellbore_table::of_frame`, ask `read.indexable == IndexableElement::Intervals`
/// again instead of `read.indexable.is_frame_interval()`.
#[test]
fn a_frame_of_intervals_from_datum_tabulates_as_passes() {
    let mds = [0.0, 5.0, 12.0, 20.0];
    let property = continuous(eml23::IndexableElement::IntervalsFromDatum, &[0.1, 0.2, 0.3]);
    let source = held(vec![boxed(property.clone())]);
    let table =
        wellbore_table::of_frame(&frame(&mds), &[&property as &dyn Reflect], &source).expect("tabulated");

    assert_eq!(table.rows, 3, "one row per interval");
    assert!(table.notes.is_empty(), "{:?}", table.notes);
    let cells = |at: usize| -> Vec<Option<&str>> {
        table.columns[at]
            .values
            .iter()
            .map(|cell| cell.as_deref())
            .collect()
    };
    assert_eq!(table.columns[0].title, "top MD");
    assert_eq!(cells(0), [Some("0"), Some("5"), Some("12")]);
    assert_eq!(table.columns[1].title, "MD");
    assert_eq!(cells(1), [Some("5"), Some("12"), Some("20")]);
    assert_eq!(table.columns[2].title, "thickness");
    assert_eq!(cells(2), [Some("0.1"), Some("0.2"), Some("0.3")]);
    assert_eq!(table.columns[2].uom.as_deref(), Some("m"));
}

/// A table of passes holds no node property, and says so; a table of nodes holds
/// no pass property, and says so.
///
/// Break it: in `one_property`, delete the `found.support.is_intervals() != by_passes` test.
#[test]
fn a_node_property_has_no_row_in_a_table_of_passes() {
    let mds = [0.0, 5.0, 12.0, 20.0];
    let passes = continuous(eml23::IndexableElement::IntervalsFromDatum, &[0.1, 0.2, 0.3]);
    let mut nodes = continuous(eml23::IndexableElement::Nodes, &[1.0, 2.0, 3.0, 4.0]);
    nodes.uuid = "c0000000-0000-4000-8000-000000000003".to_string();
    nodes.citation = citation("F1 - porosity");
    let source = held(vec![boxed(passes.clone()), boxed(nodes.clone())]);
    let table = wellbore_table::of_frame(
        &frame(&mds),
        &[&passes as &dyn Reflect, &nodes as &dyn Reflect],
        &source,
    )
    .expect("tabulated");
    assert_eq!(table.columns.len(), 3, "top, base, and the one that has a row");
    assert_eq!(table.notes.len(), 1, "{:?}", table.notes);
    assert!(
        table.notes[0].contains("`F1 - porosity` hangs on nodes while this table has one row per interval"),
        "{}",
        table.notes[0]
    );
}

/// What a categorical property is written as: **the label, never the code**. A
/// table with one value column writes that column; a table with several writes
/// each of them (the code table of a CSV carries the code and the word); a code
/// no row names is left empty and said.
///
/// Break it: in `wellbore_table::one_property`, replace `.and_then(|value| label_of(categories, value))`
/// by `.and_then(value_text)` (the code is written instead of its label).
#[test]
fn a_category_is_written_as_its_label_and_a_code_no_row_names_is_said() {
    let mds = [0.0, 10.0, 20.0, 30.0];
    let mut property = discrete(eml23::IndexableElement::Nodes, &[20, 15, 10, -1]);
    property.citation = citation("F1 - STRA");

    let source = held(vec![boxed(table(false)), boxed(property.clone())]);
    let one =
        wellbore_table::of_frame(&frame(&mds), &[&property as &dyn Reflect], &source).expect("tabulated");
    assert_eq!(one.columns.len(), 2);
    assert_eq!(one.columns[1].title, "STRA");
    assert_eq!(
        one.columns[1].values,
        [
            Some("Bartonian".to_string()),
            None,
            Some("Lutetian".to_string()),
            None
        ],
        "15 names no row, and -1 is the NullValue"
    );
    assert_eq!(one.notes.len(), 1, "{:?}", one.notes);
    assert!(
        one.notes[0].contains("1 code(s) name no row") && one.notes[0].contains("(code(s) 15)"),
        "{}",
        one.notes[0]
    );

    let source = held(vec![boxed(table(true)), boxed(property.clone())]);
    let two =
        wellbore_table::of_frame(&frame(&mds), &[&property as &dyn Reflect], &source).expect("tabulated");
    let titles: Vec<&str> = two.columns.iter().map(|column| column.title.as_str()).collect();
    assert_eq!(
        titles,
        ["MD", "Nom_STRA", "thickness"],
        "each column that is not a key"
    );
    assert_eq!(
        two.columns[1].kind.as_deref(),
        None,
        "the property's kind goes to the first column, and this property names none"
    );
    assert_eq!(
        two.columns[2].values,
        [None, None, Some("1.5".to_string()), None],
        "the thickness of the row that 20 names is absent in the file; 10's is 1.5"
    );
}

/// `well_strata` reads a category through the same door: the units, the row of
/// each code and the refusal of a code no row holds are what they were, for
/// `intervals from datum` as for `intervals`.
///
/// Break it: in `well_strata::category`, replace `.row_of(value)` by
/// `.row_of(&frame_values::Value::Code(0))` (every interval then names the same row).
#[test]
fn a_category_property_on_intervals_from_datum_names_its_units_through_the_same_door() {
    let mds = [0.0, 5.0, 12.0, 20.0, 31.0];
    let named = |element| {
        let property = discrete(element, &[20, 10, 35, -1]);
        let frame = frame(&mds);
        let source = held(vec![
            boxed(frame.clone()),
            boxed(table(false)),
            boxed(property.clone()),
        ]);
        let strata = well_strata::of_frame(
            &frame,
            &Wanted::Category { property: &property },
            &source,
            &Known::default(),
        )
        .expect("the property names its rows");
        strata
            .sections
            .iter()
            .map(|section| section.unit.map(|at| strata.units[at].title.clone()))
            .collect::<Vec<_>>()
    };
    let from_datum = named(eml23::IndexableElement::IntervalsFromDatum);
    assert_eq!(
        from_datum,
        [
            Some("Bartonian".to_string()),
            Some("Lutetian".to_string()),
            Some("Priabonian".to_string()),
            None
        ]
    );
    assert_eq!(named(eml23::IndexableElement::Intervals), from_datum);
}
