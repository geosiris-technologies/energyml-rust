//! The justness fixes of the review of 15/11/2026 (lot B2, `energyml-domain`).
//!
//! Each test names the finding it pins (`[tranche id]`, from the annex of the
//! review), builds its objects in memory (no file is read), and ends with the
//! sabotage that makes it fail. The form of an assertion comes from the
//! specification where there is one, and the page is cited.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use energyml_core::identity;
use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::component_axes::{
    AxisOrigin, AxisRefusal, AxisSource, AxisSources, AxisSubject, AxisValues, ComponentAxis,
};
use energyml_domain::property::shape::ElementShape;
use energyml_domain::property::{self, IndexableElement};
use energyml_domain::source::{ArraySource, NoObjects, ObjectSource, Prefetched};
use energyml_domain::table::{Cell, Lookup, Rows};
use energyml_domain::values::{self, Values};
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_0_1 as resqml201;
use energyml_resqml_2_2 as resqml22;
use energyml_storage::array::{ArrayBuffer, ArrayData, ArrayLocation, ArrayMetadata, ArrayValues};

use crate::inherit::{citation, nowhere, u};

// ---------------------------------------------------------------------------
// Probes: objects with the optional fields a generated type never leaves out
// ---------------------------------------------------------------------------

/// The generated array types carry `Value` and `Count` as plain fields, so a
/// generated object can never be *missing* one. A document read leniently, or an
/// object built by hand through the reflection, can: these probes have the same
/// type names and the same field names with the fields optional.
mod probe {
    use energyml_core::prelude::*;

    #[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
    #[energyml(package = "eml", version = "2.3", xml_name = "BooleanConstantArray")]
    pub struct BooleanConstantArray {
        #[energyml(elem, xml_name = "Value")]
        pub value: Option<bool>,
        #[energyml(elem, xml_name = "Count")]
        pub count: Option<i64>,
    }

    #[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
    #[energyml(package = "eml", version = "2.3", xml_name = "StringConstantArray")]
    pub struct StringConstantArray {
        #[energyml(elem, xml_name = "Value")]
        pub value: Option<String>,
        #[energyml(elem, xml_name = "Count")]
        pub count: Option<i64>,
    }

    #[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
    #[energyml(package = "eml", version = "2.3", xml_name = "FloatingPointConstantArray")]
    pub struct FloatingPointConstantArray {
        #[energyml(elem, xml_name = "Value")]
        pub value: Option<f64>,
        #[energyml(elem, xml_name = "Count")]
        pub count: Option<i64>,
    }

    #[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
    #[energyml(package = "eml", version = "2.3", xml_name = "FloatingPointLatticeArray")]
    pub struct FloatingPointLatticeArray {
        #[energyml(elem, xml_name = "StartValue")]
        pub start_value: Option<f64>,
        #[energyml(elem, xml_name = "Offset")]
        pub offset: Vec<FloatingPointConstantArray>,
    }

    #[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
    #[energyml(package = "eml", version = "2.3", xml_name = "IntegerRangeArray")]
    pub struct IntegerRangeArray {
        #[energyml(elem, xml_name = "Value")]
        pub value: Option<i64>,
        #[energyml(elem, xml_name = "Count")]
        pub count: Option<i64>,
    }

    /// An inline numeric list whose elements are strings.
    #[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
    #[energyml(package = "eml", version = "2.3", xml_name = "FloatingPointXmlArray")]
    pub struct FloatingPointXmlArray {
        #[energyml(elem, xml_name = "Values")]
        pub values: Vec<String>,
    }

    /// An inline integer list whose elements are doubles.
    #[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
    #[energyml(package = "eml", version = "2.3", xml_name = "IntegerXmlArray")]
    pub struct IntegerXmlArray {
        #[energyml(elem, xml_name = "Values")]
        pub values: Vec<f64>,
    }

    /// An inline boolean list whose elements are integers.
    #[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
    #[energyml(package = "eml", version = "2.3", xml_name = "BooleanXmlArray")]
    pub struct BooleanXmlArray {
        #[energyml(elem, xml_name = "Values")]
        pub values: Vec<i64>,
    }
}

fn read(array: &dyn Reflect) -> energyml_domain::Result<Values> {
    values::read(array, &NoObjects, "the array")
}

fn refusal(result: energyml_domain::Result<Values>) -> String {
    format!("{}", result.expect_err("a refusal, not a value"))
}

// ---------------------------------------------------------------------------
// [domain-2 S12] interval edges on a wellbore frame
// ---------------------------------------------------------------------------

/// RESQML Technical Usage Guide v2.2, section 4.2.1, Table 1 "RESQML Indexable
/// Elements and Applicable Representations" (printed page 38): `intervals` is
/// listed for "wellbore frames, column-layer grids", and `interval edges` for
/// "column-layer grids" only. The pair (wellbore frame, interval edges) is not
/// defined, so no count is computed for it: a refusal by name. The count of the
/// pair that is defined (intervals = NodeCount - 1) is unchanged.
///
/// Break it: put `IntervalEdges` back in the `Intervals | Interval |
/// IntervalsFromDatum` arm of `element_count` (wellbore frames).
#[test]
fn interval_edges_on_a_wellbore_frame_are_refused_not_counted() {
    let frame22 = resqml22::WellboreFrameRepresentation {
        uuid: u(1),
        node_count: 5,
        ..Default::default()
    };
    let frame201 = resqml201::WellboreFrameRepresentation {
        uuid: u(2),
        node_count: 5,
        ..Default::default()
    };
    for frame in [&frame22 as &dyn Reflect, &frame201 as &dyn Reflect] {
        assert_eq!(
            property::element_count(frame, IndexableElement::Nodes).ok(),
            Some(5)
        );
        assert_eq!(
            property::element_count(frame, IndexableElement::Intervals).ok(),
            Some(4)
        );
        let refused = property::element_count(frame, IndexableElement::IntervalEdges)
            .expect_err("interval edges are not defined on a wellbore frame");
        assert!(format!("{refused}").contains("interval edges"), "{refused}");
    }
}

// ---------------------------------------------------------------------------
// [domain-3 S4] constant, range and lattice arrays refuse what they lack
// ---------------------------------------------------------------------------

/// Break it: put `.unwrap_or(false)` back on the boolean `Value`.
#[test]
fn a_boolean_constant_with_no_value_is_refused() {
    let array = probe::BooleanConstantArray {
        value: None,
        count: Some(4),
    };
    assert!(refusal(read(&array)).contains("Value"));
    let array = probe::BooleanConstantArray {
        value: Some(true),
        count: Some(3),
    };
    assert_eq!(read(&array).unwrap(), Values::Boolean(vec![true; 3]));
}

/// An empty string is a value of a string constant; only an absent one is refused.
///
/// Break it: read the `Value` with `text` (a blank is absent there), or put
/// `.unwrap_or_default()` back.
#[test]
fn a_string_constant_with_no_value_is_refused_and_an_empty_one_is_kept() {
    let array = probe::StringConstantArray {
        value: None,
        count: Some(2),
    };
    assert!(refusal(read(&array)).contains("Value"));
    let array = probe::StringConstantArray {
        value: Some(String::new()),
        count: Some(2),
    };
    assert_eq!(read(&array).unwrap(), Values::Text(vec![String::new(); 2]));
}

/// A missing `Count` is a refusal in `read`, in `count`, and for a lattice step
/// and a range, never an empty array.
///
/// Break it: put `.unwrap_or(0)` back on a `Count` in `read`, `count`,
/// `lattice` or `lattice_dims`.
#[test]
fn a_missing_count_is_refused_everywhere_it_is_read() {
    let constant = probe::FloatingPointConstantArray {
        value: Some(1.0),
        count: None,
    };
    assert!(refusal(read(&constant)).contains("Count"));
    assert!(values::count(&constant, &NoObjects, "c").is_err());

    let range = probe::IntegerRangeArray {
        value: Some(5),
        count: None,
    };
    assert!(refusal(read(&range)).contains("Count"));
    assert!(values::count(&range, &NoObjects, "r").is_err());

    // A lattice step with no `Value`, and one with no `Count`.
    let step = |value, count| probe::FloatingPointConstantArray { value, count };
    let no_value = probe::FloatingPointLatticeArray {
        start_value: Some(0.0),
        offset: vec![step(None, Some(3))],
    };
    assert!(read(&no_value).is_err(), "{no_value:?}");
    let no_count = probe::FloatingPointLatticeArray {
        start_value: Some(0.0),
        offset: vec![step(Some(1.0), None)],
    };
    assert!(read(&no_count).is_err(), "{no_count:?}");
    // The dimensions are the counts alone, so a missing count is refused there too.
    assert!(
        values::stored_dims(&no_count, &NoObjects, "l").is_err(),
        "{no_count:?}"
    );
    // The complete form still reads: 0, 1, 2, 3 (`Count` is the number of steps).
    let lattice = probe::FloatingPointLatticeArray {
        start_value: Some(0.0),
        offset: vec![step(Some(1.0), Some(3))],
    };
    assert_eq!(
        read(&lattice).unwrap(),
        Values::Floating(vec![0.0, 1.0, 2.0, 3.0])
    );
}

// ---------------------------------------------------------------------------
// [cross-duplication C1] a huge Count is refused, not allocated
// ---------------------------------------------------------------------------

/// A `Count` of 10^12 asked for a `vec![value; 10^12]`: an abort of the host
/// (the wasm and MCP faces included). It is a refusal that names the ceiling.
///
/// Break it: remove the `MAX_COMPUTED_VALUES` test in `declared_count`.
#[test]
fn an_absurd_count_is_refused_before_anything_is_allocated() {
    let constant = eml23::FloatingPointConstantArray {
        value: 1.0,
        count: 1_000_000_000_000,
        ..Default::default()
    };
    assert!(refusal(read(&constant)).contains("ceiling"));
    assert!(values::count(&constant, &NoObjects, "c").is_err());

    let boolean = eml23::BooleanConstantArray {
        value: true,
        count: i64::MAX,
        ..Default::default()
    };
    assert!(refusal(read(&boolean)).contains("ceiling"));

    // 2^20 x 2^20 = 2^40 values from two small counts.
    let step = eml23::FloatingPointConstantArray {
        value: 1.0,
        count: 1 << 20,
        ..Default::default()
    };
    let lattice = eml23::FloatingPointLatticeArray {
        start_value: 0.0,
        offset: vec![step.clone(), step],
        ..Default::default()
    };
    assert!(refusal(read(&lattice)).contains("ceiling"));

    // A range that would run past `i64::MAX` is refused, not wrapped.
    let range = resqml201::IntegerRangeArray {
        value: i64::MAX,
        count: 5,
    };
    assert!(read(&range).is_err());
    let range = resqml201::IntegerRangeArray { value: 10, count: 3 };
    assert_eq!(read(&range).unwrap(), Values::Integer(vec![10, 11, 12]));
}

// ---------------------------------------------------------------------------
// [domain-3 S5] an inline list never drops an element
// ---------------------------------------------------------------------------

/// Break it: put `filter_map` back in the inline readers (return the kept
/// elements without comparing them with the ones the list holds).
#[test]
fn an_inline_element_of_the_wrong_type_is_a_refusal_not_a_shorter_array() {
    let numbers = probe::FloatingPointXmlArray {
        values: vec!["1.5".into(), "two".into(), "3.5".into()],
    };
    assert!(refusal(read(&numbers)).contains("none is dropped"));

    // 2.5 is not a whole number.
    let integers = probe::IntegerXmlArray {
        values: vec![1.0, 2.5, 3.0],
    };
    assert!(refusal(read(&integers)).contains("none is dropped"));
    let integers = probe::IntegerXmlArray {
        values: vec![1.0, 2.0],
    };
    assert_eq!(read(&integers).unwrap(), Values::Integer(vec![1, 2]));

    let booleans = probe::BooleanXmlArray { values: vec![1, 0] };
    assert!(refusal(read(&booleans)).contains("none is dropped"));

    // The typed forms read as before.
    let typed = eml23::FloatingPointXmlArray {
        count_per_value: 1,
        values: vec![1.0, 2.0, 3.0],
        ..Default::default()
    };
    assert_eq!(read(&typed).unwrap(), Values::Floating(vec![1.0, 2.0, 3.0]));
}

// ---------------------------------------------------------------------------
// [domain-3 S6] and [domain-3 S7] external arrays
// ---------------------------------------------------------------------------

/// One dataset, `t.h5:/s`, served to whatever part names it.
struct OneDataset(ArrayData);

impl OneDataset {
    fn of(values: ArrayValues) -> OneDataset {
        let dimensions = vec![values.len() as i64];
        let element_type = values.element_type().to_string();
        OneDataset(ArrayData {
            metadata: ArrayMetadata::new(dimensions, element_type),
            values,
        })
    }
}

impl ObjectSource for OneDataset {
    fn object(&self, _uuid: &str) -> Option<Box<dyn DynObject>> {
        None
    }
}

impl ArraySource for OneDataset {
    fn locate(&self, object: &dyn Reflect) -> Vec<ArrayLocation> {
        energyml_storage::array::locate(object)
    }
    fn metadata(&self, _location: &ArrayLocation) -> energyml_domain::Result<ArrayMetadata> {
        Ok(self.0.metadata.clone())
    }
    fn read(&self, _location: &ArrayLocation) -> energyml_domain::Result<ArrayData> {
        Ok(self.0.clone())
    }
    fn read_into(
        &self,
        _location: &ArrayLocation,
        _out: &mut ArrayBuffer<'_>,
    ) -> energyml_domain::Result<ArrayMetadata> {
        Err(energyml_domain::Error::unsupported(
            "reading into a buffer",
            "not in this test",
        ))
    }
}

fn integer_array(start: i64, count: i64) -> eml23::IntegerExternalArray {
    eml23::IntegerExternalArray {
        count_per_value: 1,
        values: eml23::ExternalDataArray {
            external_data_array_part: vec![eml23::ExternalDataArrayPart {
                count: vec![count],
                path_in_external_file: "/s".into(),
                start_index: vec![start],
                uri: "t.h5".into(),
                mime_type: Some("application/x-hdf5".into()),
            }],
        },
        ..Default::default()
    }
}

/// 2^53 + 1 is the first integer a `f64` cannot hold; `i64::MAX` and a negative
/// neighbour of 2^53 are the ones a round trip through `f64` changes. A `u64`
/// above `i64::MAX` is refused by name: it used to saturate to `i64::MAX`.
///
/// Break it: route `widen`'s integer arm through `numbers(..)` and `x as i64`.
#[test]
fn a_64_bit_integer_makes_the_round_trip_exactly() {
    let wanted = vec![i64::MAX, (1 << 53) + 1, -((1 << 53) + 1), 7, i64::MIN];
    let array = integer_array(0, wanted.len() as i64);
    let source = OneDataset::of(ArrayValues::I64(wanted.clone()));
    assert_eq!(
        values::read(&array, &source, "ids").unwrap(),
        Values::Integer(wanted)
    );

    let source = OneDataset::of(ArrayValues::U64(vec![1, u64::MAX]));
    let array = integer_array(0, 2);
    let refused = values::read(&array, &source, "ids").expect_err("u64::MAX does not fit an i64");
    assert!(format!("{refused}").contains("does not fit"), "{refused}");

    // A `u64` that fits is read as it is.
    let source = OneDataset::of(ArrayValues::U64(vec![1, (1 << 62) + 1]));
    assert_eq!(
        values::read(&array, &source, "ids").unwrap(),
        Values::Integer(vec![1, (1 << 62) + 1])
    );
}

/// eml 2.3 `ExternalDataArrayPart` carries a `StartIndex` and a `Count`: a window
/// in the dataset (EML Technical Reference Guide 2.3, `ExternalDataArrayPart`).
/// `count`, `stored_dims` and the values read must agree on it.
///
/// Break it: sum `metadata.dimensions` (the whole dataset) again in `count`.
#[test]
fn count_honours_the_window_a_part_designates() {
    let source = OneDataset::of(ArrayValues::I64((0..10).collect()));
    let array = integer_array(2, 3);
    assert_eq!(values::count(&array, &source, "w").unwrap(), 3);
    assert_eq!(values::stored_dims(&array, &source, "w").unwrap(), vec![3]);

    // The whole dataset, when the part names no window narrower than it.
    let whole = integer_array(0, 10);
    assert_eq!(values::count(&whole, &source, "w").unwrap(), 10);
}

// ---------------------------------------------------------------------------
// [domain-3 S11] a null is no number
// ---------------------------------------------------------------------------

fn rows_of(columns: Vec<Values>, nulls: Vec<Option<i64>>) -> Rows {
    let width = columns.len();
    Rows {
        rows: columns.iter().map(Values::len).min().unwrap_or(0),
        columns: columns.into_iter().map(Some).collect(),
        per_row: vec![1; width],
        nulls,
        notes: Vec::new(),
    }
}

/// Common Technical Reference Guide 2.3, section 9.1 (p. 190): a `NullValue` is
/// how an integer column says a cell has no value, and `NaN` is a float's. The
/// two readings of a table (`Rows::cell` and `ColumnCells`) must agree on it.
///
/// Break it: read the cell straight from `Values` in `Rows::cell` again (return
/// `Cell::Number(v as f64)` without the null test).
#[test]
fn rows_cell_and_lookup_of_do_not_hand_back_a_null_as_a_number() {
    let rows = rows_of(
        vec![
            Values::Integer(vec![1, -999, 3]),
            Values::Floating(vec![0.5, f64::NAN, 2.5]),
            Values::Text(vec!["a".into(), String::new(), "c".into()]),
        ],
        vec![Some(-999), None, None],
    );
    assert_eq!(rows.cell(0, 0), Some(Cell::Number(1.0)));
    assert_eq!(rows.cell(1, 0), None, "the NullValue is no number");
    assert_eq!(rows.cell(1, 1), None, "NaN is no number");
    assert_eq!(
        rows.cell(1, 2),
        Some(Cell::Text(String::new())),
        "an empty text is a value"
    );
    assert_eq!(rows.cell(2, 1), Some(Cell::Number(2.5)));

    let lookup = Lookup {
        titles: vec![Some("code".into()), Some("rank".into())],
        key_columns: vec![0],
        rows: rows_of(
            vec![
                Values::Integer(vec![1, 2, 3]),
                Values::Integer(vec![10, -999, 30]),
            ],
            vec![None, Some(-999)],
        ),
        notes: Vec::new(),
    };
    assert_eq!(lookup.of(1.0), Some(vec![Some(Cell::Number(10.0))]));
    assert_eq!(
        lookup.of(2.0),
        Some(vec![None]),
        "the row exists, its value is absent: not the number -999"
    );
}

// ---------------------------------------------------------------------------
// [domain-3 S12] a code with no label is not written as a value
// ---------------------------------------------------------------------------

fn tabulate(
    package: &crate::integrity::Package,
    property: &resqml22::DiscreteProperty,
) -> energyml_domain::wellbore_table::WellTable {
    let frame = resqml22::WellboreFrameRepresentation {
        uuid: u(40),
        citation: citation("a frame"),
        node_count: 3,
        node_md: Some(eml23::AbstractFloatingPointArray::FloatingPointXmlArray(
            eml23::FloatingPointXmlArray {
                count_per_value: 1,
                values: vec![10.0, 20.0, 30.0],
                ..Default::default()
            },
        )),
        ..Default::default()
    };
    energyml_domain::wellbore_table::of_frame(&frame, &[property as &dyn Reflect], package)
        .expect("the frame tabulates")
}

fn nodes_property(codes: &[i64]) -> resqml22::DiscreteProperty {
    let mut property = crate::integrity::discrete(&u(41), codes);
    property.indexable_element = eml23::IndexableElement::Nodes;
    property
}

/// The module's own doc: *"Writing the raw code into a CSV would be worse than an
/// absence: an integer looks like a measurement, and `3` reads as three."* A code
/// no row of the table names, and the array's own `NullValue`, are empty cells;
/// the unmatched code is counted in the notes.
///
/// Break it: put `.or_else(|| cell_text(&values, at))` back after `label_at`.
#[test]
fn a_code_no_row_names_is_an_empty_cell_and_is_said() {
    let package = crate::integrity::Package(vec![(
        crate::integrity::TABLE.to_string(),
        crate::integrity::boxed(crate::integrity::table()),
    )]);
    // 1 is a key of the table (its value column says 10), 7 is no key, and -1 is
    // the array's NullValue (`discrete` writes it so).
    let table = tabulate(&package, &nodes_property(&[1, 7, -1]));
    let column = table
        .columns
        .iter()
        .find(|column| column.title == "lithology")
        .expect("the property is a column");
    assert_eq!(column.values, vec![Some("10".to_string()), None, None]);
    assert!(
        table
            .notes
            .iter()
            .any(|note| note.contains("1 code(s) name no row") && note.contains('7')),
        "the unmatched code is in the notes: {:?}",
        table.notes
    );
}

/// A property that names a lookup the package does not hold is refused with a
/// note, not written as raw codes under a label's heading.
///
/// Break it: turn the `return Err` of the missing lookup back into `None`.
#[test]
fn a_property_whose_lookup_is_missing_is_refused_not_written_as_codes() {
    let package = crate::integrity::Package(Vec::new());
    let table = tabulate(&package, &nodes_property(&[1, 2, 3]));
    assert_eq!(table.columns.len(), 1, "the depth only: {:?}", table.columns);
    assert!(
        table.notes.iter().any(|note| note.contains("category lookup")),
        "{:?}",
        table.notes
    );
}

// ---------------------------------------------------------------------------
// [domain-1 S8] a source below a complete answer is not asked
// ---------------------------------------------------------------------------

struct Complete;

struct Failing(Arc<AtomicUsize>);

impl AxisSource for Complete {
    fn id(&self) -> &'static str {
        "complete"
    }
    fn axes(
        &self,
        subject: &AxisSubject<'_>,
        _objects: &dyn ObjectSource,
        _arrays: &dyn ArraySource,
    ) -> Option<Result<Vec<ComponentAxis>, AxisRefusal>> {
        Some(Ok(vec![ComponentAxis {
            dim: 0,
            name: None,
            kind: None,
            uom: None,
            values: AxisValues::Listed((0..subject.shape.dims[0]).map(|n| n as f64).collect()),
            cyclic: None,
            origin: AxisOrigin::PairedProperty {
                uri: "complete".into(),
            },
        }]))
    }
}

impl AxisSource for Failing {
    fn id(&self) -> &'static str {
        "failing"
    }
    fn axes(
        &self,
        _subject: &AxisSubject<'_>,
        _objects: &dyn ObjectSource,
        _arrays: &dyn ArraySource,
    ) -> Option<Result<Vec<ComponentAxis>, AxisRefusal>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Some(Err(AxisRefusal::OrderUnstated { axes: 2 }))
    }
}

/// The documented priority: the first source that answers a dimension wins. The
/// caller's source answers the only dimension, so the failing fallback below it
/// is not asked, and cannot turn the answer into a refusal.
///
/// Break it: remove the early `break` in `AxisSources::resolve`.
#[test]
fn a_failing_fallback_cannot_veto_a_complete_answer() {
    let asked = Arc::new(AtomicUsize::new(0));
    let mut sources = AxisSources::with_builtins();
    sources.register(Box::new(Failing(asked.clone())));
    sources.register(Box::new(Complete));
    let shape = ElementShape::of(&[3], &[]).expect("a shape");
    let subject = AxisSubject {
        uri: "p",
        shape: &shape,
        facets: &[],
        links: &[],
        rows: None,
    };
    let resolved = sources
        .resolve(
            &subject,
            &NoObjects as &dyn ObjectSource,
            &NoObjects as &dyn ArraySource,
        )
        .expect("the complete answer stands");
    assert_eq!(
        resolved.axes[0].origin,
        AxisOrigin::PairedProperty {
            uri: "complete".into()
        }
    );
    assert_eq!(asked.load(Ordering::SeqCst), 0, "the fallback was never asked");
}

// ---------------------------------------------------------------------------
// [domain-2 S13] a huge year is refused
// ---------------------------------------------------------------------------

/// XML Schema Part 2, section 3.2.7 allows a year of any length; a year this
/// library cannot place exactly on an axis of `f64` seconds is a named refusal.
/// A year of 12 digits made `days * 86_400` overflow an `i64` (a panic in debug,
/// a plausible wrong instant in release).
///
/// Break it: remove the `MAX_YEAR` test in `unix_seconds`.
#[test]
fn a_year_of_twelve_digits_is_refused_and_an_ordinary_one_still_reads() {
    use energyml_domain::series::instant::unix_seconds;
    for text in ["123456789012-01-01T00:00:00Z", "-123456789012-01-01T00:00:00Z"] {
        let refused = unix_seconds(text).expect_err(text);
        assert!(refused.contains("outside"), "{refused}");
    }
    assert_eq!(unix_seconds("1970-01-02T00:00:00Z"), Ok(86_400.0));
    assert!(unix_seconds("9999-12-31T23:59:59Z").is_ok());
}

// ---------------------------------------------------------------------------
// [domain-1 S11] the identity of a finding is the uri
// ---------------------------------------------------------------------------

fn property_version(version: &str, values: i64) -> resqml22::ContinuousProperty {
    resqml22::ContinuousProperty {
        uuid: u(60),
        object_version: Some(version.to_string()),
        citation: citation("a property"),
        indexable_element: eml23::IndexableElement::Nodes,
        value_count_per_indexable_element: vec![1],
        supporting_representation: eml23::DataObjectReference {
            uuid: u(61),
            qualified_type: "resqml22.Grid2dRepresentation".into(),
            title: "g".into(),
            ..Default::default()
        },
        values_for_patch: vec![resqml22::AbstractValueArray::FloatingPointConstantArray(
            eml23::FloatingPointConstantArray {
                value: 1.0,
                count: values,
                ..Default::default()
            },
        )],
        ..Default::default()
    }
}

/// A package holding two versions of one property: each one's finding is its
/// own, told apart by uri (they share the uuid).
///
/// Break it: file the findings under `summary.uuid` again (`identity_of` in
/// `check.rs` returning the uuid), or copy the uuid into `CountIssue::uri`.
#[test]
fn two_versions_of_one_uuid_have_two_findings_with_two_uris() {
    use energyml_domain::check::{self, Options, Package, Part};
    use energyml_storage::resource::ObjectSummary;

    let grid = resqml22::Grid2dRepresentation {
        uuid: u(61),
        citation: citation("g"),
        slowest_axis_count: 3,
        fastest_axis_count: 4,
        ..Default::default()
    };
    // 7 values where the 12 nodes want 12: a count issue for each version.
    let (v1, v2) = (property_version("1", 7), property_version("2", 7));
    let uri_of = |object: &dyn Reflect| identity::uri_of_reflect(object).expect("a uri");
    let (grid_uri, uri1, uri2) = (uri_of(&grid), uri_of(&v1), uri_of(&v2));
    assert_ne!(uri1, uri2, "the two versions have two uris");

    let held = crate::integrity::Package(vec![
        (u(61), Box::new(grid) as Box<dyn DynObject>),
        (u(60), Box::new(v1) as Box<dyn DynObject>),
        (u(60), Box::new(v2) as Box<dyn DynObject>),
    ]);
    let summary = |uri: &str, uuid: String, ty: &str, locator: &str| {
        ObjectSummary::new(uri.to_string(), uuid, ty.to_string())
            .with_content_type("application/x-resqml+xml;version=2.2;type=x".to_string())
            .with_locator(locator.to_string())
    };
    let summaries = vec![
        summary(&grid_uri, u(61), "resqml22.Grid2dRepresentation", "grid.xml"),
        summary(&uri1, u(60), "resqml22.ContinuousProperty", "p1.xml"),
        summary(&uri2, u(60), "resqml22.ContinuousProperty", "p2.xml"),
    ];
    let parts: Vec<Part<'_>> = summaries
        .iter()
        .zip(&held.0)
        .map(|(summary, (_, object))| Part {
            summary,
            object: object.as_ref(),
            relationships: &[],
        })
        .collect();
    let package = Package {
        objects: &parts,
        summaries: &summaries,
        unreadable: &[],
        diagnostics: &[],
        source: &held,
        known: None,
    };
    let report = check::check_with_arrays(
        &package,
        &Options {
            schema: false,
            rules: true,
            extra_rules: Vec::new(),
        },
        Some(&held),
    );
    assert_eq!(report.counts.len(), 2, "{:?}", report.counts);
    let mut uris: Vec<&str> = report.counts.iter().map(|issue| issue.uri.as_str()).collect();
    uris.sort_unstable();
    let mut wanted = vec![uri1.as_str(), uri2.as_str()];
    wanted.sort_unstable();
    assert_eq!(uris, wanted, "one issue per version, told apart by uri");
    assert!(report.counts.iter().all(|issue| issue.uuid == u(60)));
}

/// The rule engine files a finding under the identity it was given: a uri gives
/// both the uuid and the uri, a bare uuid (the library's entry adapter) is its
/// own identity.
///
/// Break it: copy the id into `Finding::uuid` without reading the uuid from a uri.
#[test]
fn a_finding_filed_under_a_uri_carries_the_uuid_and_the_uri() {
    let bad = crate::integrity::discrete(&u(70), &[7]);
    let uri = identity::uri_of_reflect(&bad).expect("a uri");
    let package = crate::integrity::Package(vec![(
        crate::integrity::TABLE.to_string(),
        crate::integrity::boxed(crate::integrity::table()),
    )]);
    let found = |id: &str| {
        let pairs: Vec<(&str, &dyn Reflect)> = vec![(id, &bad as &dyn Reflect)];
        energyml_domain::integrity::run(&pairs, &package, Some(&package))
            .into_iter()
            .filter(|finding| finding.rule == "house/discrete-value-in-lookup")
            .collect::<Vec<_>>()
    };
    let by_uri = found(&uri);
    assert_eq!(by_uri.len(), 1, "{by_uri:?}");
    assert_eq!(by_uri[0].uuid, u(70));
    assert_eq!(by_uri[0].uri, uri);

    let by_uuid = found(&u(70));
    assert_eq!(by_uuid.len(), 1, "{by_uuid:?}");
    assert_eq!(
        (by_uuid[0].uuid.as_str(), by_uuid[0].uri.as_str()),
        (u(70).as_str(), u(70).as_str())
    );
}

// ---------------------------------------------------------------------------
// [domain-2 S4] a prefetch keeps every version of an object
// ---------------------------------------------------------------------------

/// Two versions of one uuid are two entries: a uri names the version it names, a
/// bare uuid is the entry adapter and answers the version inserted last, and a
/// uri naming a version the cache does not hold is refused, not answered with
/// another version.
///
/// Break it: key the cache by uuid again (`objects.insert(uuid, object)`).
#[test]
fn a_prefetch_keeps_two_versions_of_one_object_apart() {
    let (v1, v2) = (property_version("1", 7), property_version("2", 7));
    let (uri1, uri2) = (
        identity::uri_of_reflect(&v1).expect("a uri"),
        identity::uri_of_reflect(&v2).expect("a uri"),
    );
    let mut cache = Prefetched::new();
    cache.insert(Box::new(v1), &nowhere);
    cache.insert(Box::new(v2), &nowhere);
    assert_eq!(cache.len(), 2);

    let version_of = |object: Box<dyn DynObject>| {
        identity::object_version_of_reflect(object.as_reflect()).map(str::to_string)
    };
    assert_eq!(version_of(cache.object_at(&uri1).expect("v1")), Some("1".into()));
    assert_eq!(version_of(cache.object_at(&uri2).expect("v2")), Some("2".into()));
    assert_eq!(
        version_of(cache.object(&u(60)).expect("the last inserted")),
        Some("2".into())
    );
    let absent = uri1.replace("version='1'", "version='9'");
    assert_ne!(absent, uri1, "the uri spells its version: {uri1}");
    assert!(
        cache.object_at(&absent).is_none(),
        "a version the cache does not hold"
    );
    assert!(cache.read_object_at(&absent).is_err());

    // The same version again replaces it, it does not add a third.
    cache.insert(Box::new(property_version("2", 9)), &nowhere);
    assert_eq!(cache.len(), 2);
}

// ---------------------------------------------------------------------------
// [domain-3 S14] the version a reference names is the version read
// ---------------------------------------------------------------------------

/// A column lists a rank at version 2; the package holds the rank at versions 2
/// and 1, the one held last being version 1. The rank read is version 2.
///
/// Break it: look the rank up by its bare uuid again (a map of the last object
/// held for each uuid) in `columns`.
#[test]
fn a_column_reads_the_rank_version_it_names() {
    use energyml_domain::stratigraphy;

    let rank = |version: &str| resqml22::StratigraphicColumnRankInterpretation {
        uuid: u(80),
        schema_version: "2.2".into(),
        object_version: Some(version.to_string()),
        citation: citation(&format!("rank, version {version}")),
        ..Default::default()
    };
    let column = resqml22::StratigraphicColumn {
        uuid: u(81),
        schema_version: "2.2".into(),
        citation: citation("the column"),
        ranks: vec![eml23::DataObjectReference {
            uuid: u(80),
            object_version: Some("2".to_string()),
            qualified_type: "resqml22.StratigraphicColumnRankInterpretation".into(),
            title: "as the reference spells it".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let (v2, v1) = (rank("2"), rank("1"));
    let objects: Vec<&dyn Reflect> = vec![&v2, &v1, &column];
    let all = stratigraphy::columns(objects.into_iter());
    let read = &all.columns[0].ranks[0];
    assert_eq!(read.rank.title, "rank, version 2", "{:?}", read.rank);
}

// ---------------------------------------------------------------------------
// [domain-2 S6] a carrier walk follows the version its uri names
// ---------------------------------------------------------------------------

const RESQML: &str = "http://www.energistics.org/energyml/data/resqmlv2";
const EML: &str = "http://www.energistics.org/energyml/data/commonv2";
const CT_FEATURE: &str = "application/x-resqml+xml;version=2.2;type=BoundaryFeature";
const CT_HORIZON: &str = "application/x-resqml+xml;version=2.2;type=HorizonInterpretation";

fn citation_xml(title: &str) -> String {
    format!(
        "<eml:Citation><eml:Title>{title}</eml:Title><eml:Originator>test</eml:Originator>\
         <eml:Creation>2026-11-15T00:00:00Z</eml:Creation><eml:Format>test</eml:Format></eml:Citation>"
    )
}

fn feature_xml(uuid: &str) -> String {
    format!(
        "<resqml:BoundaryFeature xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" uuid=\"{uuid}\" \
         schemaVersion=\"2.2\">{}<resqml:IsWellKnown>false</resqml:IsWellKnown></resqml:BoundaryFeature>",
        citation_xml("a surface")
    )
}

/// A horizon (at `version`, or the unversioned first one) interpreting `feature`.
fn horizon_xml(uuid: &str, version: Option<&str>, feature: &str) -> String {
    let version = version
        .map(|version| format!(" objectVersion=\"{version}\""))
        .unwrap_or_default();
    format!(
        "<resqml:HorizonInterpretation xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" uuid=\"{uuid}\" \
         schemaVersion=\"2.2\"{version}>{}<resqml:Domain>depth</resqml:Domain>\
         <resqml:InterpretedFeature><eml:Uuid>{feature}</eml:Uuid>\
         <eml:QualifiedType>resqml22.BoundaryFeature</eml:QualifiedType><eml:Title>a surface</eml:Title>\
         </resqml:InterpretedFeature></resqml:HorizonInterpretation>",
        citation_xml("a horizon")
    )
}

fn object_of(content_type: &str, xml: String) -> Box<dyn DynObject> {
    energyml_schemas::all()
        .read_xml_as(
            content_type,
            xml.as_bytes(),
            energyml_core::registry::ReadMode::Tolerant,
        )
        .expect("readable")
        .0
}

/// A horizon held in two versions, each interpreting **its own** feature: the
/// walk from either version reaches the feature that version names. By the
/// uuid alone, one of the two walks read the other version's references.
///
/// Break it: go back to `index.references_of(&identity::parse_address(node).0)`
/// in `carriers`.
#[test]
fn a_carrier_walk_from_each_version_reaches_that_versions_feature() {
    use energyml_domain::package_index::PackageIndex;
    use energyml_domain::provenance_graph::{CarrierRules, NodeKey, carriers};
    use energyml_storage::EpcStore;

    let (horizon, feature_a, feature_b) = (u(90), u(91), u(92));
    let registry = Arc::new(energyml_schemas::all());
    let empty = energyml_storage::extract::empty_package(Some(0)).expect("an empty package");
    let mut store = EpcStore::from_reader(
        Box::new(std::io::Cursor::new(empty)),
        "versions.epc",
        registry.clone(),
    )
    .expect("opens");
    store
        .put(object_of(CT_FEATURE, feature_xml(&feature_a)))
        .expect("feature a");
    store
        .put(object_of(CT_FEATURE, feature_xml(&feature_b)))
        .expect("feature b");
    store
        .put(object_of(CT_HORIZON, horizon_xml(&horizon, None, &feature_a)))
        .expect("the first horizon");
    store
        .put_as_new_version(object_of(
            CT_HORIZON,
            horizon_xml(&horizon, Some("2"), &feature_b),
        ))
        .expect("a second version of it");
    let mut bytes = std::io::Cursor::new(Vec::new());
    store
        .write_to(&mut bytes, &energyml_storage::epc::SaveOptions::default())
        .expect("written");
    let store = EpcStore::from_reader(
        Box::new(std::io::Cursor::new(bytes.into_inner())),
        "versions.epc",
        registry,
    )
    .expect("reads back");
    let index = PackageIndex::build(&store, store.revision());

    let versions: Vec<(String, Option<String>)> = index
        .nodes()
        .filter(|node| &*node.type_key == "HorizonInterpretation")
        .map(|node| (node.uri.to_string(), identity::parse_address(&node.uri).1))
        .collect();
    assert_eq!(versions.len(), 2, "two entries of one uuid: {versions:?}");
    let feature_uri = |uuid: &str| {
        index
            .nodes()
            .find(|node| node.uuid.eq_ignore_ascii_case(uuid))
            .map(|node| node.uri.to_string())
            .expect("the feature is in the package")
    };
    for (uri, version) in &versions {
        let wanted = if version.as_deref() == Some("2") {
            feature_uri(&feature_b)
        } else {
            feature_uri(&feature_a)
        };
        let edges = carriers(&index, std::slice::from_ref(uri), &CarrierRules::new(), 1);
        let reached: Vec<&NodeKey> = edges.iter().map(|edge| &edge.to).collect();
        assert_eq!(
            reached,
            vec![&NodeKey::Object(wanted)],
            "from {uri} (version {version:?})"
        );
    }
}
