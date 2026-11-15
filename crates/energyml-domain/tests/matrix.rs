//! A matrix of values per element along a well (`series::matrix`), whole or by
//! plane, and the array channels of a WITSML channel set.
//!
//! Sources: RESQML TRG 2.2 §7.1 p. 222 (*"First (left) given dimension is
//! slowest and last (right) given dimension is fastest"*; a vector of constant
//! size on all elements); RESQML Usage Guide 2.2 §4.2.2 p. 39 (the components
//! are the fastest dimension in the HDF5 storage); WITSML TRG 2.1 §7.12 p. 162
//! (`LogChannelAxis`: the order of several axes is not stated) and §7.2 p. 146
//! (`ChannelData.Data`, a JSON blob). The arrays live in a map of datasets the
//! test holds and **counts the reads of**: that a plane was read by block is
//! proved by the number of values the source returned, not supposed.

use std::cell::Cell;
use std::collections::BTreeMap;

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_core::xml::XmlText;
use energyml_domain::property::shape::{ComponentPick, ElementShape};
use energyml_domain::property::{self, Property};
use energyml_domain::series::matrix::{
    MatrixChannel, MatrixNote, MatrixRefusal, RowSupport, read_channel_set, read_matrix, read_plane,
};
use energyml_domain::series::{Refused, channel_set};
use energyml_domain::source::{ArraySource, NoObjects, ObjectSource};
use energyml_domain::values::Family;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_storage::{ArrayBuffer, ArrayData, ArrayLocation, ArrayMetadata, ArrayValues};
use energyml_witsml_2_1 as witsml;

// ---------------------------------------------------------------------------
// A source that counts
// ---------------------------------------------------------------------------

/// Datasets by path, read by block when the location carries a window, with the
/// number of reads and of values returned.
#[derive(Default)]
struct Datasets {
    held: BTreeMap<String, (Vec<i64>, Vec<f64>)>,
    reads: Cell<usize>,
    returned: Cell<usize>,
}

impl Datasets {
    fn with(path: &str, dims: &[i64], values: Vec<f64>) -> Datasets {
        let mut held = BTreeMap::new();
        held.insert(path.to_string(), (dims.to_vec(), values));
        Datasets {
            held,
            ..Default::default()
        }
    }
}

impl ObjectSource for Datasets {
    fn object(&self, _uuid: &str) -> Option<Box<dyn DynObject>> {
        None
    }
}

impl ArraySource for Datasets {
    fn locate(&self, object: &dyn Reflect) -> Vec<ArrayLocation> {
        energyml_storage::array::locate(object)
    }

    /// The whole dataset's shape, like a store does: the window is not applied.
    fn metadata(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayMetadata> {
        let (dims, _) = self.held.get(&location.path_in_file).expect("a known dataset");
        Ok(ArrayMetadata::new(dims.clone(), "double"))
    }

    fn read(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayData> {
        self.reads.set(self.reads.get() + 1);
        let (dims, values) = self.held.get(&location.path_in_file).expect("a known dataset");
        let (start, count) = if location.count.is_empty() {
            (vec![0; dims.len()], dims.clone())
        } else {
            (location.start.clone(), location.count.clone())
        };
        let mut out = Vec::new();
        let mut at = vec![0i64; dims.len()];
        loop {
            let mut linear = 0i64;
            for k in 0..dims.len() {
                linear = linear * dims[k] + start[k] + at[k];
            }
            out.push(values[linear as usize]);
            // Odometer over the block, last dimension fastest.
            let mut k = dims.len();
            loop {
                if k == 0 {
                    self.returned.set(self.returned.get() + out.len());
                    return Ok(ArrayData {
                        metadata: ArrayMetadata::new(count, "double"),
                        values: ArrayValues::F64(out),
                    });
                }
                k -= 1;
                at[k] += 1;
                if at[k] < count[k] {
                    break;
                }
                at[k] = 0;
            }
        }
    }

    fn read_into(
        &self,
        _location: &ArrayLocation,
        _out: &mut ArrayBuffer<'_>,
    ) -> energyml_domain::Result<ArrayMetadata> {
        Err(energyml_domain::Error::unsupported(
            "reading into a buffer",
            "this test's source reads by value",
        ))
    }
}

// ---------------------------------------------------------------------------
// Properties built by hand
// ---------------------------------------------------------------------------

fn citation(title: &str) -> eml23::Citation {
    eml23::Citation {
        title: title.into(),
        originator: "test".into(),
        creation: "2026-11-14T00:00:00Z".into(),
        format: "test".into(),
        ..Default::default()
    }
}

fn dor(uuid: &str, qualified_type: &str, title: &str) -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: uuid.into(),
        qualified_type: qualified_type.into(),
        title: title.into(),
        ..Default::default()
    }
}

fn unit(code: &str) -> eml23::UnitOfMeasureExt {
    eml23::UnitOfMeasureExt::from_xml_text(code).expect("a unit")
}

const URI: &str = "eml:///resqml22.ContinuousProperty(00000000-0000-0000-0000-0000000000d1)";

/// The values of a `[nodes, 3, 2]` dataset: `100 n + 10 i + j`.
fn cube(nodes: i64) -> Vec<f64> {
    let mut values = Vec::new();
    for n in 0..nodes {
        for i in 0..3 {
            for j in 0..2 {
                values.push((100 * n + 10 * i + j) as f64);
            }
        }
    }
    values
}

fn external(path: &str, dims: &[i64]) -> resqml22::AbstractValueArray {
    resqml22::AbstractValueArray::FloatingPointExternalArray(eml23::FloatingPointExternalArray {
        count_per_value: 1,
        values: eml23::ExternalDataArray {
            external_data_array_part: vec![eml23::ExternalDataArrayPart {
                count: dims.to_vec(),
                path_in_external_file: path.into(),
                start_index: vec![0; dims.len()],
                uri: "t.h5".into(),
                mime_type: Some("application/x-hdf5".into()),
            }],
        },
        ..Default::default()
    })
}

fn property(
    indexable: eml23::IndexableElement,
    per_element: Vec<i64>,
    array: resqml22::AbstractValueArray,
) -> resqml22::ContinuousProperty {
    resqml22::ContinuousProperty {
        uuid: "00000000-0000-0000-0000-0000000000d1".into(),
        citation: citation("Rh"),
        indexable_element: indexable,
        value_count_per_indexable_element: per_element,
        uom: unit("m"),
        values_for_patch: vec![array],
        ..Default::default()
    }
}

/// The `[4, 3, 2]` property of the examples, stored as `stored` says.
fn cube_property(stored: &[i64]) -> resqml22::ContinuousProperty {
    property(
        eml23::IndexableElement::Nodes,
        vec![3, 2],
        external("/rh", stored),
    )
}

fn along_first_dimension() -> ComponentPick {
    ComponentPick::Along {
        axis: 0,
        at: vec![0, 1],
    }
}

fn read(object: &resqml22::ContinuousProperty) -> Property<'_> {
    property::read(object).expect("a property")
}

// ---------------------------------------------------------------------------
// Planes
// ---------------------------------------------------------------------------

/// A plane read by block equals the plane cut from the whole matrix, and the
/// source returned **12 values** against the 24 of the whole (the partial read
/// is proved by the counter). Break it: ignore `start` in the block.
#[test]
fn reading_a_plane_equals_slicing_the_whole() {
    let object = cube_property(&[4, 3, 2]);
    let property = read(&object);
    let pick = along_first_dimension();

    let by_block = Datasets::with("/rh", &[4, 3, 2], cube(4));
    let plane = read_plane(URI, &property, 4, &pick, &by_block).expect("a plane");
    assert_eq!(by_block.reads.get(), 1);
    assert_eq!(by_block.returned.get(), 12);
    assert!(!plane.notes.contains(&MatrixNote::ReadWhole));

    let whole = Datasets::with("/rh", &[4, 3, 2], cube(4));
    let matrix = read_matrix(URI, &property, 4, &whole).expect("a matrix");
    let cut = matrix.plane(&pick).expect("a plane");
    assert_eq!(whole.returned.get(), 24);
    assert_eq!(plane.values, cut.values);
    assert_eq!((plane.rows, plane.cols), (4, 3));
}

/// RESQML TRG 2.2 §7.1 p. 222. Break it: invert the order of the dimensions in `ravel`.
#[test]
fn the_first_given_dimension_is_the_slowest_in_a_plane() {
    let object = cube_property(&[4, 3, 2]);
    let property = read(&object);
    let source = Datasets::with("/rh", &[4, 3, 2], cube(4));
    let matrix = read_matrix(URI, &property, 4, &source).expect("a matrix");
    let plane = matrix.plane(&along_first_dimension()).expect("a plane");
    assert_eq!(&plane.values[0..3], &[1.0, 11.0, 21.0]);
    // Node 2, the same line.
    assert_eq!(&plane.values[6..9], &[201.0, 211.0, 221.0]);
}

/// One component, at its multi-index, by block: 4 values for 4 rows.
///
/// Break it: let the block start at 0 whatever the component.
#[test]
fn one_component_is_read_by_block() {
    let object = cube_property(&[4, 3, 2]);
    let property = read(&object);
    let source = Datasets::with("/rh", &[4, 3, 2], cube(4));
    let plane = read_plane(URI, &property, 4, &ComponentPick::At(vec![2, 1]), &source).expect("a plane");
    assert_eq!(plane.values, vec![21.0, 121.0, 221.0, 321.0]);
    assert_eq!(source.returned.get(), 4);
}

/// A flat store (`[24]`) is the same data, read whole, and the plane says so.
/// RESQML Usage Guide 2.2 §4.2.2 p. 39. Break it: attempt a block on `[24]`.
#[test]
fn a_flat_store_is_read_whole_and_says_so() {
    let object = cube_property(&[24]);
    let property = read(&object);
    let flat = Datasets::with("/rh", &[24], cube(4));
    let plane = read_plane(URI, &property, 4, &along_first_dimension(), &flat).expect("a plane");
    assert!(plane.notes.contains(&MatrixNote::ReadWhole));

    let nd_object = cube_property(&[4, 3, 2]);
    let nd_property = read(&nd_object);
    let nd = Datasets::with("/rh", &[4, 3, 2], cube(4));
    let by_block = read_plane(URI, &nd_property, 4, &along_first_dimension(), &nd).expect("a plane");
    assert_eq!(plane.values, by_block.values);
}

/// A reduction needs every component: the whole matrix is read, and says so.
///
/// Break it: read a block for a reduction.
#[test]
fn a_reduction_is_read_whole() {
    let object = cube_property(&[4, 3, 2]);
    let property = read(&object);
    let source = Datasets::with("/rh", &[4, 3, 2], cube(4));
    let plane = read_plane(
        URI,
        &property,
        4,
        &ComponentPick::Reduce(energyml_domain::property::shape::Reduction::Max),
        &source,
    )
    .expect("a plane");
    assert!(plane.notes.contains(&MatrixNote::ReadWhole));
    assert_eq!(source.returned.get(), 24);
    assert_eq!(plane.cols, 1);
    assert_eq!(plane.values, vec![21.0, 121.0, 221.0, 321.0]);
}

/// The values count as the intervals: found by their count, and noted.
///
/// Break it: refuse a count that is not the declared support's.
#[test]
fn interval_values_are_found_by_their_count() {
    let object = property(
        eml23::IndexableElement::Nodes,
        vec![3, 2],
        external("/rh", &[3, 3, 2]),
    );
    let property = read(&object);
    let source = Datasets::with("/rh", &[3, 3, 2], cube(3));
    let matrix = read_matrix(URI, &property, 4, &source).expect("a matrix");
    assert_eq!(matrix.support, RowSupport::Intervals);
    assert_eq!(matrix.rows, 3);
    assert_eq!(
        matrix.notes,
        vec![MatrixNote::SupportFromCount {
            declared: RowSupport::Nodes,
            used: RowSupport::Intervals
        }]
    );
}

/// Declared on the intervals, holding the nodes' count: also found by count.
///
/// Break it: trust the declaration over the count.
#[test]
fn node_values_declared_on_intervals_are_found_by_their_count() {
    let object = property(
        eml23::IndexableElement::Intervals,
        vec![3, 2],
        external("/rh", &[4, 3, 2]),
    );
    let property = read(&object);
    let source = Datasets::with("/rh", &[4, 3, 2], cube(4));
    let matrix = read_matrix(URI, &property, 4, &source).expect("a matrix");
    assert_eq!(matrix.support, RowSupport::Nodes);
    assert_eq!(matrix.rows, 4);
}

/// A count that is neither support's is refused with the three numbers.
///
/// Break it: read the values anyway.
#[test]
fn a_count_that_fits_neither_support_is_refused() {
    let object = property(
        eml23::IndexableElement::Nodes,
        vec![3, 2],
        external("/rh", &[5, 3, 2]),
    );
    let property = read(&object);
    let source = Datasets::with("/rh", &[5, 3, 2], cube(5));
    let refusal = read_matrix(URI, &property, 4, &source).expect_err("refused");
    assert!(
        matches!(
            refusal,
            MatrixRefusal::Count {
                values: 30,
                nodes: 4,
                intervals: 3,
                per_element: 6
            }
        ),
        "{refusal}"
    );
}

/// `[3, 2]` declared, one value per node held: a curve.
///
/// Break it: answer `Count` for this case.
#[test]
fn one_value_per_node_is_a_curve() {
    let object = property(eml23::IndexableElement::Nodes, vec![3, 2], external("/rh", &[4]));
    let property = read(&object);
    let source = Datasets::with("/rh", &[4], vec![1.0, 2.0, 3.0, 4.0]);
    let refusal = read_matrix(URI, &property, 4, &source).expect_err("refused");
    assert!(
        matches!(refusal, MatrixRefusal::OnePerElement { declared: 6 }),
        "{refusal}"
    );
}

fn constant(count: i64) -> resqml22::AbstractValueArray {
    resqml22::AbstractValueArray::IntegerConstantArray(eml23::IntegerConstantArray {
        value: 1,
        count,
        ..Default::default()
    })
}

/// Break it: read the codes of a lookup as values.
#[test]
fn refuses_a_categorical_matrix() {
    let object = resqml22::DiscreteProperty {
        uuid: "00000000-0000-0000-0000-0000000000d2".into(),
        citation: citation("facies"),
        indexable_element: eml23::IndexableElement::Nodes,
        value_count_per_indexable_element: vec![1],
        category_lookup: Some(dor(
            "00000000-0000-0000-0000-0000000000d3",
            "eml23.ColumnBasedTable",
            "lookup",
        )),
        values_for_patch: vec![constant(4)],
        ..Default::default()
    };
    let property = read_discrete(&object);
    let refusal = read_matrix(URI, &property, 4, &NoObjects).expect_err("refused");
    assert!(matches!(refusal, MatrixRefusal::Categorical), "{refusal}");
}

fn read_discrete(object: &resqml22::DiscreteProperty) -> Property<'_> {
    property::read(object).expect("a property")
}

/// Break it: turn a boolean into 0 and 1.
#[test]
fn refuses_a_boolean_matrix() {
    let object = property(
        eml23::IndexableElement::Nodes,
        vec![1],
        resqml22::AbstractValueArray::BooleanConstantArray(eml23::BooleanConstantArray {
            value: true,
            count: 4,
            ..Default::default()
        }),
    );
    let property = read(&object);
    let refusal = read_matrix(URI, &property, 4, &NoObjects).expect_err("refused");
    assert!(
        matches!(
            refusal,
            MatrixRefusal::NotNumeric {
                family: Family::Boolean
            }
        ),
        "{refusal}"
    );
}

/// Usage Guide 2.2 §6.6.3 p. 65: a property series has a time dimension.
/// Break it: read the first step silently.
#[test]
fn refuses_a_time_series_matrix() {
    let mut object = property(eml23::IndexableElement::Nodes, vec![1], constant(4));
    object.time_or_interval_series = Some(eml23::TimeOrIntervalSeries {
        use_interval: false,
        time_series: dor(
            "00000000-0000-0000-0000-0000000000d4",
            "eml23.TimeSeries",
            "times",
        ),
    });
    let property = read(&object);
    let refusal = read_matrix(URI, &property, 4, &NoObjects).expect_err("refused");
    assert!(matches!(refusal, MatrixRefusal::Time), "{refusal}");
}

/// A matrix hangs on the nodes or the intervals of a frame.
///
/// Break it: accept any indexable element.
#[test]
fn refuses_a_support_that_is_not_a_frames() {
    let object = property(eml23::IndexableElement::Cells, vec![1], constant(4));
    let property = read(&object);
    let refusal = read_matrix(URI, &property, 4, &NoObjects).expect_err("refused");
    assert!(matches!(refusal, MatrixRefusal::Support { .. }), "{refusal}");
}

/// Break it: index past the rows.
#[test]
fn cell_reads_row_and_component() {
    let object = cube_property(&[4, 3, 2]);
    let property = read(&object);
    let source = Datasets::with("/rh", &[4, 3, 2], cube(4));
    let matrix: MatrixChannel = read_matrix(URI, &property, 4, &source).expect("a matrix");
    assert_eq!(matrix.cell(2, &[1, 0]), Some(210.0));
    assert_eq!(matrix.cell(4, &[1, 0]), None);
    assert_eq!(matrix.cell(0, &[3, 0]), None);
    assert_eq!(
        matrix.row(1),
        Some(&[100.0, 101.0, 110.0, 111.0, 120.0, 121.0][..])
    );
    assert_eq!(matrix.shape, ElementShape::of(&[3, 2], &[]).expect("a shape"));
    assert_eq!(matrix.uri, URI);
}

// ---------------------------------------------------------------------------
// The array channels of a WITSML channel set
// ---------------------------------------------------------------------------

fn md() -> witsml::ChannelIndex {
    witsml::ChannelIndex {
        index_kind: eml23::DataIndexKind::MeasuredDepth,
        mnemonic: "MD".into(),
        uom: unit("m"),
        direction: eml23::IndexDirection::Increasing,
        ..Default::default()
    }
}

fn axis(count: i64, uid: &str) -> witsml::LogChannelAxis {
    witsml::LogChannelAxis {
        axis_start: 0.0,
        axis_spacing: 1.0,
        axis_count: count,
        axis_name: Some("lateral".into()),
        axis_property_kind: "length".into(),
        axis_uom: unit("m"),
        uid: uid.into(),
    }
}

fn set_of(axes: Vec<witsml::LogChannelAxis>, data: &str) -> witsml::ChannelSet {
    let channel = witsml::AnyChannel::Channel(witsml::Channel {
        citation: citation("UDAR"),
        uuid: "00000000-0000-0000-0000-0000000000e1".into(),
        schema_version: "2.1".into(),
        mnemonic: "UDAR".into(),
        global_mnemonic: "UDAR".into(),
        data_kind: witsml::ChannelDataKind::Double,
        uom: unit("m"),
        channel_property_kind: dor(
            "00000000-0000-0000-0000-0000000000e2",
            "eml23.PropertyKind",
            "resistivity",
        ),
        index: vec![md()],
        axis_definition: axes,
        ..Default::default()
    });
    witsml::ChannelSet {
        citation: citation("a set"),
        uuid: "00000000-0000-0000-0000-0000000000e3".into(),
        schema_version: "2.1".into(),
        index: vec![md()],
        channel: vec![channel],
        data: Some(witsml::ChannelData {
            data: Some(data.into()),
            file_uri: None,
        }),
        ..Default::default()
    }
}

/// WITSML TRG 2.1 §7.12 p. 162 and §7.2 p. 146. Break it: drop the `null` of a cell.
#[test]
fn an_array_channel_reads_as_a_matrix() {
    let set = set_of(vec![axis(3, "a1")], "[[0,[1,2,3]],[10,[4,5,6]],[20,null]]");
    let channels = read_channel_set(&set, &NoObjects).expect("matrices");
    assert_eq!(channels.len(), 1);
    let matrix = &channels[0];
    assert_eq!(matrix.shape.dims, vec![3]);
    assert_eq!(matrix.rows, 3);
    assert_eq!(matrix.support, RowSupport::Samples);
    assert_eq!(matrix.cell(1, &[2]), Some(6.0));
    // A null cell is a row of `NaN`, never zeros.
    assert!(matrix.row(2).expect("a row").iter().all(|value| value.is_nan()));
    let index = matrix.index.as_ref().expect("the index of the set");
    assert_eq!(index.values, vec![0.0, 10.0, 20.0]);
    assert!(matrix.uri.contains("00000000-0000-0000-0000-0000000000e1"));
}

/// S9. Break it: multiply the two axes.
#[test]
fn refuses_two_axis_definitions() {
    let set = set_of(vec![axis(3, "a1"), axis(2, "a2")], "[[0,[1,2,3,4,5,6]]]");
    let refusal = read_channel_set(&set, &NoObjects).expect_err("refused");
    assert!(
        matches!(refusal, MatrixRefusal::AxisOrderUnstated { axes: 2, .. }),
        "{refusal}"
    );
}

/// Break it: pad a short cell.
#[test]
fn refuses_a_cell_of_the_wrong_length() {
    let set = set_of(vec![axis(3, "a1")], "[[0,[1,2,3]],[10,[4,5]]]");
    let refusal = read_channel_set(&set, &NoObjects).expect_err("refused");
    assert!(
        matches!(
            refusal,
            MatrixRefusal::CellLength {
                row: 2,
                found: 2,
                expected: 3,
                ..
            }
        ),
        "{refusal}"
    );
}

/// S9: the scalar reader keeps refusing an array cell.
///
/// Break it: let `channel_set::read` read the array channels.
#[test]
fn the_scalar_reader_still_refuses_an_array_cell() {
    let set = set_of(vec![axis(3, "a1")], "[[0,[1,2,3]]]");
    let refusal = channel_set::read(&set).expect_err("refused");
    assert!(matches!(refusal, Refused::ValueKind { .. }), "{refusal}");
}
