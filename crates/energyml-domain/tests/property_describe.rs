//! A property described slab by slab (`property::chunks`), what its file says of
//! it (`property::observe`), and a property read by the same slab as its sister
//! (`PairWalk`).
//!
//! Sources: RESQML TRG 2.2 §7 p. 221 and §7.1 p. 222 (the dimensions of the
//! values, top is slowest; `RealizationIndices`); RESQML Usage Guide 2.2 §4.2.2
//! p. 39 (the 1D order is the HDF5 order) and §6.6.3 p. 65 (the time count of a
//! series); EML TRG 2.3 §11.2 p. 284 (`FacetExt`), §11.11 p. 294 (one facet per
//! kind), §11.9 (quantity classes); RESQML TRG 2.2 §7.6 (the unit of a property
//! converts to that of its kind); `docs/adr/0019` (the `synthetic` mark);
//! `docs/adr/0042` (what a property says of itself). Every object is built
//! here, inline, and the datasets are held in memory: no file is read.

use std::cell::Cell;
use std::collections::BTreeMap;

use energyml_core::reflect::EnergymlType;
use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::property::chunks::{
    AxisId, ChunkSelection, ChunksRefusal, PairWalk, PairWalkRefusal, PropertyChunks, TimeChoice, keep_ordered_axes,
};
use energyml_domain::property::observe::{
    FACET_VALUE_KINDS, MarkedObject, PropertyObservation, SyntheticMark, UnitCheck, UnitNotCheckable,
    schema_facet_kind,
};
use energyml_domain::property::shape::{ComponentPick, ElementAxes, PropertyShape, Reduction, ShapeRefusal};
use energyml_domain::property::{self, observe};
use energyml_domain::property_kind::{Dictionary, KindIssue, Resolver};
use energyml_domain::source::{ArraySource, ObjectSource};
use energyml_domain::stats::cells::CellRule;
use energyml_domain::stats::describe::{Describer, Description, Observation, Plan, ValueChunks};
use energyml_domain::stats::modes::StatMethods;
use energyml_domain::stats::scale::{Scale, ScaleKind};
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_storage::array::{ArrayBuffer, ArrayData, ArrayLocation, ArrayMetadata, ArrayValues};

// ---------------------------------------------------------------------------
// A source in memory: datasets by path, objects by uuid, and counters
// ---------------------------------------------------------------------------

const PROPERTY: &str = "00000000-0000-0000-0000-0000000000d1";
const KIND: &str = "aaaa";
const REP: &str = "00000000-0000-0000-0000-0000000000d2";
const SERIES: &str = "00000000-0000-0000-0000-0000000000d3";
const LOOKUP: &str = "00000000-0000-0000-0000-0000000000d4";

/// Datasets `(dimensions, values row-major)` by path, and objects by uuid.
/// `read` applies `start` and `count` of the location and counts what it hands
/// back, so a test can tell a block from the whole array.
#[derive(Default)]
struct Datasets {
    sets: BTreeMap<String, (Vec<i64>, Vec<f64>)>,
    objects: Vec<(String, Box<dyn DynObject>)>,
    reads: Cell<usize>,
    values_returned: Cell<usize>,
}

impl Datasets {
    fn with(mut self, path: &str, dims: &[i64], values: Vec<f64>) -> Datasets {
        assert_eq!(dims.iter().product::<i64>() as usize, values.len(), "a test dataset that fills its shape");
        self.sets.insert(path.to_string(), (dims.to_vec(), values));
        self
    }

    fn object(mut self, uuid: &str, object: Box<dyn DynObject>) -> Datasets {
        self.objects.push((uuid.to_string(), object));
        self
    }
}

impl ObjectSource for Datasets {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        self.objects
            .iter()
            .find(|(held, _)| held.eq_ignore_ascii_case(uuid))
            .map(|(_, one)| one.clone_box())
    }
}

impl ArraySource for Datasets {
    fn locate(&self, object: &dyn Reflect) -> Vec<ArrayLocation> {
        energyml_storage::array::locate(object)
    }

    /// The whole shape of the dataset, whatever window the location names
    /// (what `EpcStore` answers).
    fn metadata(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayMetadata> {
        let (dims, _) = self.sets.get(&location.path_in_file).expect("a dataset the test holds");
        Ok(ArrayMetadata::new(dims.clone(), "double"))
    }

    fn read(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayData> {
        let (dims, values) = self.sets.get(&location.path_in_file).expect("a dataset the test holds");
        self.reads.set(self.reads.get() + 1);
        let (start, count) = if location.count.is_empty() {
            (vec![0; dims.len()], dims.clone())
        } else {
            (location.start.clone(), location.count.clone())
        };
        let rank = dims.len();
        assert!(start.len() == rank && count.len() == rank, "a window of the dataset's rank");
        for k in 0..rank {
            assert!(start[k] >= 0 && start[k] + count[k] <= dims[k], "a window inside the dataset");
        }
        let strides: Vec<usize> = (0..rank)
            .map(|k| dims[k + 1..].iter().product::<i64>() as usize)
            .collect();
        let total: i64 = count.iter().product();
        let mut out = Vec::with_capacity(total as usize);
        for linear in 0..total {
            let mut rest = linear;
            let mut at = vec![0i64; rank];
            for k in (0..rank).rev() {
                at[k] = rest % count[k];
                rest /= count[k];
            }
            let position: usize = (0..rank).map(|k| (start[k] + at[k]) as usize * strides[k]).sum();
            out.push(values[position]);
        }
        self.values_returned.set(self.values_returned.get() + out.len());
        Ok(ArrayData {
            metadata: ArrayMetadata::new(count, "double"),
            values: ArrayValues::F64(out),
        })
    }

    fn read_into(
        &self,
        _location: &ArrayLocation,
        _out: &mut ArrayBuffer<'_>,
    ) -> energyml_domain::Result<ArrayMetadata> {
        Err(energyml_domain::Error::unsupported(
            "reading into a buffer",
            "this test's source reads blocks",
        ))
    }
}

// ---------------------------------------------------------------------------
// Objects built by hand
// ---------------------------------------------------------------------------

fn citation() -> eml23::Citation {
    eml23::Citation {
        title: "t".into(),
        originator: "o".into(),
        creation: "2026-11-15T00:00:00Z".into(),
        format: "f".into(),
        ..Default::default()
    }
}

fn reference(uuid: &str, qualified_type: &str) -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: uuid.into(),
        qualified_type: qualified_type.into(),
        title: "r".into(),
        ..Default::default()
    }
}

fn unit(code: &str) -> eml23::UnitOfMeasureExt {
    eml23::UnitOfMeasureExt::from_xml_text(code).expect("a unit")
}

fn part(path: &str, dims: &[i64]) -> eml23::ExternalDataArray {
    eml23::ExternalDataArray {
        external_data_array_part: vec![eml23::ExternalDataArrayPart {
            count: dims.to_vec(),
            path_in_external_file: path.into(),
            start_index: vec![0; dims.len()],
            uri: "t.h5".into(),
            mime_type: Some("application/x-hdf5".into()),
        }],
    }
}

/// A floating-point array in the dataset `path` of shape `dims`.
fn floats(path: &str, dims: &[i64]) -> resqml22::AbstractValueArray {
    resqml22::AbstractValueArray::FloatingPointExternalArray(eml23::FloatingPointExternalArray {
        count_per_value: 1,
        values: part(path, dims),
        ..Default::default()
    })
}

/// An integer array with a `NullValue`.
fn integers(path: &str, dims: &[i64], null_value: i64) -> resqml22::AbstractValueArray {
    resqml22::AbstractValueArray::IntegerExternalArray(eml23::IntegerExternalArray {
        null_value,
        count_per_value: 1,
        values: part(path, dims),
        ..Default::default()
    })
}

/// A 2.2 continuous property, in metres, on nodes.
fn property_of(counts: Vec<i64>, arrays: Vec<resqml22::AbstractValueArray>) -> resqml22::ContinuousProperty {
    resqml22::ContinuousProperty {
        uuid: PROPERTY.into(),
        citation: citation(),
        indexable_element: eml23::IndexableElement::Nodes,
        value_count_per_indexable_element: counts,
        property_kind: reference(KIND, "eml23.PropertyKind"),
        supporting_representation: reference(REP, "resqml22.Grid2dRepresentation"),
        values_for_patch: arrays,
        uom: unit("m"),
        ..Default::default()
    }
}

/// The same property, dated by a series of instants.
fn series_property(counts: Vec<i64>, arrays: Vec<resqml22::AbstractValueArray>) -> resqml22::ContinuousProperty {
    resqml22::ContinuousProperty {
        time_or_interval_series: Some(eml23::TimeOrIntervalSeries {
            use_interval: false,
            time_series: reference(SERIES, "eml23.TimeSeries"),
        }),
        ..property_of(counts, arrays)
    }
}

fn uri() -> String {
    format!("eml:///resqml22.ContinuousProperty({PROPERTY})")
}

fn nodes(count: u64) -> ElementAxes {
    ElementAxes::Ordered(vec![count])
}

fn pick(pick: ComponentPick) -> ChunkSelection {
    ChunkSelection {
        pick,
        ..Default::default()
    }
}

fn refused(result: Result<PropertyChunks<'_>, ChunksRefusal>) -> ChunksRefusal {
    match result {
        Ok(_) => panic!("a refusal was expected"),
        Err(refusal) => refusal,
    }
}

/// Describe a source the way a panel does: step to the end, then finish.
fn describe(chunks: &dyn ValueChunks) -> Description {
    let mut describer = Describer::new(chunks, Plan::default(), StatMethods::builtin()).expect("a plan that fits");
    describer.step(chunks, usize::MAX).expect("a read");
    describer.finish().expect("a description")
}

fn close(a: f64, b: f64) {
    assert!((a - b).abs() <= 1e-9, "{a} is not {b}");
}

// ---------------------------------------------------------------------------
// PropertyChunks
// ---------------------------------------------------------------------------

/// Value of the dataset `[nodes, 3, 2]`: `100 n + 10 i + j`.
fn matrix_values(nodes: i64) -> Vec<f64> {
    let mut out = Vec::new();
    for n in 0..nodes {
        for i in 0..3 {
            for j in 0..2 {
                out.push((100 * n + 10 * i + j) as f64);
            }
        }
    }
    out
}

/// RESQML TRG 2.2 §7 p. 221; the plan "Delta_E = 0" of a matrix reads one plane.
/// Break it: pass `None` instead of the narrowed pick to `slab_block` (24 values are read).
#[test]
fn describing_a_plane_reads_only_the_plane() {
    let source = Datasets::default().with("/p", &[4, 3, 2], matrix_values(4));
    let object = property_of(vec![3, 2], vec![floats("/p", &[4, 3, 2])]);
    let read = property::read(&object).expect("a property");
    let chunks = PropertyChunks::new(
        &uri(),
        &read,
        nodes(4),
        None,
        None,
        &pick(ComponentPick::Along { axis: 0, at: vec![0, 1] }),
        &source,
    )
    .expect("chunks");
    assert_eq!(chunks.dims(), &[4, 3]);
    assert!(chunks.reads_narrowed());
    let description = describe(&chunks);
    assert_eq!(source.values_returned.get(), 12);
    let moments = description.moments.expect("moments");
    assert_eq!(moments.count, 12);
    // The mean of the 12 values of the plane, computed here: 100 x 1.5 + 10 x 1 + 1.
    let plane: Vec<f64> = (0..4)
        .flat_map(|n| (0..3).map(move |i| (100 * n + 10 * i + 1) as f64))
        .collect();
    close(moments.mean, plane.iter().sum::<f64>() / plane.len() as f64);
    close(moments.mean, 161.0);
}

/// Usage Guide 2.2 §4.2.2 p. 39: a flat store is the same data, read whole.
/// Break it: restrict a flat store (the block is wrong and `read_block` refuses it).
#[test]
fn a_flat_store_is_read_whole_and_says_so() {
    let source = Datasets::default().with("/p", &[24], matrix_values(4));
    let object = property_of(vec![3, 2], vec![floats("/p", &[24])]);
    let read = property::read(&object).expect("a property");
    let chunks = PropertyChunks::new(
        &uri(),
        &read,
        nodes(4),
        None,
        None,
        &pick(ComponentPick::Along { axis: 0, at: vec![0, 1] }),
        &source,
    )
    .expect("chunks");
    assert!(!chunks.reads_narrowed());
    let description = describe(&chunks);
    assert_eq!(source.values_returned.get(), 24);
    close(description.moments.expect("moments").mean, 161.0);
}

/// Usage Guide 2.2 §6.6.3 p. 65: one step of a series.
/// Break it: ignore `fixed` (the test reads `[0, 1, 2, 3]`).
#[test]
fn one_time_step_is_described_alone() {
    let values: Vec<f64> = (0..3).flat_map(|t| (0..4).map(move |n| (10 * t + n) as f64)).collect();
    let source = Datasets::default().with("/p", &[3, 4], values);
    let object = series_property(vec![1], vec![floats("/p", &[3, 4])]);
    let read = property::read(&object).expect("a property");
    let selection = ChunkSelection {
        time: TimeChoice::Step(2),
        ..Default::default()
    };
    let chunks = PropertyChunks::new(&uri(), &read, nodes(4), Some(3), None, &selection, &source).expect("chunks");
    assert_eq!(chunks.dims(), &[4]);
    assert_eq!(chunks.read(0, 4).expect("a read"), vec![20.0, 21.0, 22.0, 23.0]);
}

/// Usage Guide 2.2 §6.6.3 p. 65: with every step, time is the slowest dimension.
/// Break it: put time after the elements.
#[test]
fn every_time_step_keeps_time_slowest() {
    let values: Vec<f64> = (0..12).map(f64::from).collect();
    let source = Datasets::default().with("/p", &[3, 4], values);
    let object = series_property(vec![1], vec![floats("/p", &[3, 4])]);
    let read = property::read(&object).expect("a property");
    let chunks = PropertyChunks::new(&uri(), &read, nodes(4), Some(3), None, &ChunkSelection::default(), &source)
        .expect("chunks");
    assert_eq!(chunks.dims(), &[3, 4]);
    assert_eq!(chunks.axis_ids(), &[AxisId::Time, AxisId::Element { dim: 0, ordered: true }]);
    let refusal = refused(PropertyChunks::new(
        &uri(),
        &read,
        nodes(4),
        Some(3),
        None,
        &ChunkSelection {
            time: TimeChoice::Step(3),
            ..Default::default()
        },
        &source,
    ));
    assert!(matches!(refusal, ChunksRefusal::StepOutOfRange { step: 3, steps: 3 }), "{refusal}");
}

/// RESQML IntegerExternalArray.NullValue: the describer counts the nulls itself,
/// unless the pick reduces them.
/// Break it: reduce without replacing the null (the mean is -331.67).
#[test]
fn a_null_value_is_left_to_the_describer_unless_reduced() {
    let source = Datasets::default().with("/p", &[3], vec![1.0, -999.0, 3.0]);
    let object = property_of(vec![1], vec![integers("/p", &[3], -999)]);
    let read = property::read(&object).expect("a property");
    let chunks =
        PropertyChunks::new(&uri(), &read, nodes(3), None, None, &ChunkSelection::default(), &source).expect("chunks");
    assert_eq!(chunks.null_value(), Some(-999));
    let description = describe(&chunks);
    assert!(
        description
            .observations
            .iter()
            .any(|o| matches!(o, Observation::NullValues { count: 1, .. })),
        "{:?}",
        description.observations
    );
    close(description.moments.expect("moments").mean, 2.0);

    // One node of three values: a reduction replaces the null before it is averaged.
    let source = Datasets::default().with("/p", &[1, 3], vec![1.0, -999.0, 3.0]);
    let object = property_of(vec![3], vec![integers("/p", &[1, 3], -999)]);
    let read = property::read(&object).expect("a property");
    let chunks = PropertyChunks::new(
        &uri(),
        &read,
        nodes(1),
        None,
        None,
        &pick(ComponentPick::Reduce(Reduction::Mean)),
        &source,
    )
    .expect("chunks");
    assert_eq!(chunks.read(0, 1).expect("a read"), vec![2.0]);
    assert_eq!(chunks.null_value(), None);
    assert_eq!(chunks.nulls_before_reduction(), 1);
}

/// RESQML TRG 2.2 §7.1 p. 222: `RealizationIndices` adds a dimension; one must be chosen.
/// Break it: take realization 0 when none is chosen.
#[test]
fn a_realization_must_be_chosen() {
    let source = Datasets::default();
    let mut object = property_of(vec![1], vec![floats("/p", &[2, 4])]);
    object.realization_indices = Some(eml23::AbstractIntegerArray::IntegerXmlArray(eml23::IntegerXmlArray {
        values: vec![0, 1],
        count_per_value: 1,
        ..Default::default()
    }));
    let read = property::read(&object).expect("a property");
    let refusal = refused(PropertyChunks::new(
        &uri(),
        &read,
        nodes(4),
        None,
        Some(2),
        &ChunkSelection::default(),
        &source,
    ));
    assert!(matches!(refusal, ChunksRefusal::RealizationNotChosen { count: 2 }), "{refusal}");
    let refusal = refused(PropertyChunks::new(
        &uri(),
        &read,
        nodes(4),
        None,
        Some(2),
        &ChunkSelection {
            realization: Some(2),
            ..Default::default()
        },
        &source,
    ));
    assert!(matches!(refusal, ChunksRefusal::RealizationOutOfRange { at: 2, count: 2 }), "{refusal}");
}

/// energyml-geometry scene rule: several values per element declared and one held is read as scalars.
/// Break it: return `Shape(StoredMismatch)` for `All`.
#[test]
fn one_value_per_element_is_read_as_scalars_only_whole() {
    let source = Datasets::default().with("/p", &[4], vec![1.0, 2.0, 3.0, 4.0]);
    let object = property_of(vec![2], vec![floats("/p", &[4])]);
    let read = property::read(&object).expect("a property");
    let chunks =
        PropertyChunks::new(&uri(), &read, nodes(4), None, None, &ChunkSelection::default(), &source).expect("chunks");
    assert_eq!(chunks.dims(), &[4]);
    assert_eq!(chunks.read_as_scalars(), Some(2));
    assert_eq!(chunks.read(0, 4).expect("a read"), vec![1.0, 2.0, 3.0, 4.0]);
    let refusal = refused(PropertyChunks::new(
        &uri(),
        &read,
        nodes(4),
        None,
        None,
        &pick(ComponentPick::At(vec![1])),
        &source,
    ));
    assert!(matches!(refusal, ChunksRefusal::OnePerElement { declared: 2 }), "{refusal}");
}

/// A lookup table names categories: codes are not described as measures.
/// Break it: drop the `lookup` test.
#[test]
fn refuses_a_categorical_property() {
    let source = Datasets::default().with("/p", &[4], vec![1.0, 2.0, 3.0, 4.0]);
    let object = resqml22::DiscreteProperty {
        uuid: PROPERTY.into(),
        citation: citation(),
        indexable_element: eml23::IndexableElement::Nodes,
        value_count_per_indexable_element: vec![1],
        property_kind: reference(KIND, "eml23.PropertyKind"),
        supporting_representation: reference(REP, "resqml22.Grid2dRepresentation"),
        values_for_patch: vec![integers("/p", &[4], -1)],
        category_lookup: Some(reference(LOOKUP, "eml23.StringTableLookup")),
        ..Default::default()
    };
    let read = property::read(&object).expect("a property");
    let refusal = refused(PropertyChunks::new(
        &uri(),
        &read,
        nodes(4),
        None,
        None,
        &ChunkSelection::default(),
        &source,
    ));
    assert!(matches!(refusal, ChunksRefusal::Categorical { .. }), "{refusal}");
}

/// Booleans are not numbers to describe.
/// Break it: let `Family::Boolean` through.
#[test]
fn refuses_a_boolean_property() {
    let source = Datasets::default();
    let object = property_of(
        vec![1],
        vec![resqml22::AbstractValueArray::BooleanConstantArray(eml23::BooleanConstantArray {
            value: true,
            count: 4,
            ..Default::default()
        })],
    );
    let read = property::read(&object).expect("a property");
    let refusal = refused(PropertyChunks::new(
        &uri(),
        &read,
        nodes(4),
        None,
        None,
        &ChunkSelection::default(),
        &source,
    ));
    assert!(matches!(refusal, ChunksRefusal::NotNumeric { .. }), "{refusal}");
}

/// A property described one patch at a time.
/// Break it: describe the first patch of several.
#[test]
fn refuses_several_patches() {
    let source = Datasets::default();
    let object = property_of(vec![1], vec![floats("/a", &[4]), floats("/b", &[4])]);
    let read = property::read(&object).expect("a property");
    let refusal = refused(PropertyChunks::new(
        &uri(),
        &read,
        nodes(4),
        None,
        None,
        &ChunkSelection::default(),
        &source,
    ));
    assert!(matches!(refusal, ChunksRefusal::Patches { count: 2 }), "{refusal}");
}

/// View 05 §2.3: no variation is measured across elements that have no order.
/// Break it: drop the `AxisVariation` but keep the `Strongest` observation.
#[test]
fn an_unordered_axis_is_not_a_variation() {
    // [6 elements, 3 components]: 1000 per element, 1 per component.
    let values: Vec<f64> = (0..6).flat_map(|n| (0..3).map(move |c| (1000 * n + c) as f64)).collect();
    let source = Datasets::default().with("/p", &[6, 3], values);
    let object = property_of(vec![3], vec![floats("/p", &[6, 3])]);
    let read = property::read(&object).expect("a property");
    let chunks = PropertyChunks::new(
        &uri(),
        &read,
        ElementAxes::Unordered(6),
        None,
        None,
        &ChunkSelection::default(),
        &source,
    )
    .expect("chunks");
    assert_eq!(
        chunks.axis_ids(),
        &[AxisId::Element { dim: 0, ordered: false }, AxisId::Component { dim: 0 }]
    );
    let mut description = describe(&chunks);
    assert_eq!(description.strongest.map(|(axis, _)| axis), Some(0), "the elements dominate, before the drop");
    let dropped = keep_ordered_axes(&mut description, chunks.axis_ids());
    assert_eq!(dropped, vec![0]);
    assert!(description.axes.iter().all(|a| a.axis != 0));
    let names_axis_0 = |o: &Observation| match o {
        Observation::ConstantAlong { axis } | Observation::Strongest { axis, .. } => *axis == 0,
        Observation::Jump(jump) => jump.axis == 0,
        _ => false,
    };
    assert!(!description.observations.iter().any(names_axis_0), "{:?}", description.observations);
    assert_eq!(description.strongest, None, "one axis left: none dominates");

    // Other dimensions of the description than the ids: nothing changes.
    let before = description.clone();
    assert!(keep_ordered_axes(&mut description, &[AxisId::Time]).is_empty());
    assert_eq!(description, before);
}

// ---------------------------------------------------------------------------
// PairWalk
// ---------------------------------------------------------------------------

fn pair_sources() -> (Datasets, Datasets) {
    let rh = Datasets::default().with("/p", &[20], vec![10.0; 20]);
    let mut rv = vec![12.0; 18];
    rv.extend([9.0, 9.0]);
    let rv = Datasets::default().with("/p", &[20], rv);
    (rh, rv)
}

fn linear() -> Scale {
    Scale::new(ScaleKind::Linear, 0.0, 20.0, false).expect("a scale")
}

/// S10, the count only: Rh 10, Rv 12 eighteen times and 9 twice, the rule `< 0.95`.
/// Break it: read the sister from a block shifted by one slab.
#[test]
fn a_pair_reads_both_by_the_same_slab() {
    let (rh, rv) = pair_sources();
    let x_object = property_of(vec![1], vec![floats("/p", &[20])]);
    let y_object = property_of(vec![1], vec![floats("/p", &[20])]);
    let x_read = property::read(&x_object).expect("a property");
    let y_read = property::read(&y_object).expect("a property");
    let selection = ChunkSelection::default();
    let x = PropertyChunks::new(&uri(), &x_read, nodes(20), None, None, &selection, &rh).expect("x");
    let y = PropertyChunks::new(&uri(), &y_read, nodes(20), None, None, &selection, &rv).expect("y");
    let mut walk = PairWalk::new(&x, &y, linear(), linear(), 4, Some(CellRule::Below(0.95))).expect("a walk");
    for _ in 0..3 {
        walk.step(&x, &y, 7).expect("a step");
    }
    assert!(walk.progress().is_done());
    let summary = walk.finish().expect("a summary");
    assert_eq!(summary.pairs, 20);
    assert_eq!(summary.matched, 2);
    assert_eq!(summary.share_matched, Some(0.1));
    assert_eq!(rh.reads.get(), 3);
    assert_eq!(rv.reads.get(), 3);
    assert_eq!(rh.values_returned.get(), rv.values_returned.get());
}

/// View 05 §2.3: a pair is never aligned silently.
/// Break it: truncate to the shorter.
#[test]
fn siblings_of_two_shapes_are_refused() {
    let rh = Datasets::default().with("/p", &[20], vec![10.0; 20]);
    let rv = Datasets::default().with("/p", &[19], vec![10.0; 19]);
    let x_object = property_of(vec![1], vec![floats("/p", &[20])]);
    let y_object = property_of(vec![1], vec![floats("/p", &[19])]);
    let x_read = property::read(&x_object).expect("a property");
    let y_read = property::read(&y_object).expect("a property");
    let selection = ChunkSelection::default();
    let x = PropertyChunks::new(&uri(), &x_read, nodes(20), None, None, &selection, &rh).expect("x");
    let y = PropertyChunks::new(&uri(), &y_read, nodes(19), None, None, &selection, &rv).expect("y");
    match PairWalk::new(&x, &y, linear(), linear(), 4, None) {
        Err(PairWalkRefusal::SiblingShape { this, other }) => {
            assert_eq!(this, vec![20]);
            assert_eq!(other, vec![19]);
        }
        Err(other) => panic!("{other}"),
        Ok(_) => panic!("two shapes were paired"),
    }
}

/// RESQML IntegerExternalArray.NullValue: a null is not a value, so not a pair.
/// Break it: push the null as a value.
#[test]
fn a_null_value_is_not_a_pair() {
    let rh = Datasets::default().with("/p", &[20], vec![10.0; 20]);
    let mut values = vec![-999.0, -999.0];
    values.extend([12.0; 18]);
    let rv = Datasets::default().with("/p", &[20], values);
    let x_object = property_of(vec![1], vec![floats("/p", &[20])]);
    let y_object = property_of(vec![1], vec![integers("/p", &[20], -999)]);
    let x_read = property::read(&x_object).expect("a property");
    let y_read = property::read(&y_object).expect("a property");
    let selection = ChunkSelection::default();
    let x = PropertyChunks::new(&uri(), &x_read, nodes(20), None, None, &selection, &rh).expect("x");
    let y = PropertyChunks::new(&uri(), &y_read, nodes(20), None, None, &selection, &rv).expect("y");
    let mut walk = PairWalk::new(&x, &y, linear(), linear(), 4, Some(CellRule::Below(0.95))).expect("a walk");
    walk.step(&x, &y, usize::MAX).expect("a step");
    let summary = walk.finish().expect("a summary");
    assert_eq!(summary.dropped, 2);
    assert_eq!(summary.pairs, 18);
    assert_eq!(summary.matched, 0);
}

/// Nothing partial is returned.
/// Break it: return `Ok` from `finish` whatever the cursor.
#[test]
fn a_stopped_pair_walk_is_not_a_result() {
    let (rh, rv) = pair_sources();
    let x_object = property_of(vec![1], vec![floats("/p", &[20])]);
    let y_object = property_of(vec![1], vec![floats("/p", &[20])]);
    let x_read = property::read(&x_object).expect("a property");
    let y_read = property::read(&y_object).expect("a property");
    let selection = ChunkSelection::default();
    let x = PropertyChunks::new(&uri(), &x_read, nodes(20), None, None, &selection, &rh).expect("x");
    let y = PropertyChunks::new(&uri(), &y_read, nodes(20), None, None, &selection, &rv).expect("y");
    let mut walk = PairWalk::new(&x, &y, linear(), linear(), 4, None).expect("a walk");
    walk.step(&x, &y, 5).expect("a step");
    match walk.finish() {
        Err(PairWalkRefusal::Cancelled { done }) => close(done, 25.0),
        Err(other) => panic!("{other}"),
        Ok(_) => panic!("a stopped walk returned a summary"),
    }
}

// ---------------------------------------------------------------------------
// observations
// ---------------------------------------------------------------------------

fn facet(kind: eml23::FacetKind, value: eml23::FacetExt) -> eml23::PropertyKindFacet {
    eml23::PropertyKindFacet { facet: value, kind }
}

/// The dictionary of the tests: the kind `aaaa`, of the class `class`.
fn dictionary(class: &str) -> Dictionary {
    Dictionary::from_compact(&format!("aaaa\tdepth\t\t{class}\t0\t\n"))
}

/// Every observation of a property, with the kind in the class `class`.
fn observed(object: &resqml22::ContinuousProperty, class: &str, source: &Datasets) -> Vec<PropertyObservation> {
    let read = property::read(object).expect("a property");
    let dictionary = dictionary(class);
    let resolver = Resolver::with_dictionary(source, &dictionary);
    observe::observations(&uri(), &read, object, None, None, source, &resolver)
}

fn base() -> resqml22::ContinuousProperty {
    property_of(vec![1], vec![floats("/p", &[4])])
}

/// EML TRG 2.3 §11.2 p. 284: a value outside the list is the producer's.
/// Break it: ignore `is_extension`.
#[test]
fn an_extension_facet_is_said() {
    let mut object = base();
    object.facet = vec![
        facet(eml23::FacetKind::Direction, eml23::FacetExt::Other("nwdf:vertical".into())),
        facet(eml23::FacetKind::Direction, eml23::FacetExt::I),
    ];
    let seen = observed(&object, "length", &Datasets::default());
    let extensions: Vec<&PropertyObservation> = seen
        .iter()
        .filter(|o| matches!(o, PropertyObservation::ExtensionFacet { .. }))
        .collect();
    assert_eq!(
        extensions,
        vec![&PropertyObservation::ExtensionFacet {
            kind: "direction".into(),
            value: "nwdf:vertical".into()
        }]
    );
}

/// EML TRG 2.3 §11.2 p. 284: `K` is documented as a direction.
/// Break it: compare the value alone.
#[test]
fn a_standard_value_under_another_kind_is_said() {
    let mut object = base();
    object.facet = vec![facet(eml23::FacetKind::Qualifier, eml23::FacetExt::K)];
    let seen = observed(&object, "length", &Datasets::default());
    assert!(seen.contains(&PropertyObservation::FacetValueOfOtherKind {
        kind: "qualifier".into(),
        value: "K".into(),
        schema_kind: "direction",
    }));
    let mut object = base();
    object.facet = vec![facet(eml23::FacetKind::Direction, eml23::FacetExt::K)];
    let seen = observed(&object, "length", &Datasets::default());
    assert!(!seen.iter().any(|o| matches!(o, PropertyObservation::FacetValueOfOtherKind { .. })));
}

/// RESQML TRG 2.0.1 §6.36 p. 77: a 2.0.1 value is free text, the 2.3 list does not bind it.
/// Break it: remove the version filter.
#[test]
fn a_2_0_1_value_is_not_held_to_the_2_3_list() {
    let object = energyml_resqml_2_0_1::ContinuousProperty {
        uuid: PROPERTY.into(),
        facet: vec![energyml_resqml_2_0_1::PropertyKindFacet {
            facet: energyml_resqml_2_0_1::Facet::Qualifier,
            value: "K".into(),
        }],
        ..Default::default()
    };
    let read = property::read(&object).expect("a property");
    assert!(read.qualified_type.starts_with("resqml20."), "{}", read.qualified_type);
    let source = Datasets::default();
    let dictionary = dictionary("length");
    let resolver = Resolver::with_dictionary(&source, &dictionary);
    let seen = observe::observations(&uri(), &read, &object, None, None, &source, &resolver);
    assert!(!seen.iter().any(|o| matches!(o, PropertyObservation::FacetValueOfOtherKind { .. })), "{seen:?}");
}

/// EML TRG 2.3 §11.2 p. 284: the table is the schema's documentation.
/// Break it: change `cumulative` to `qualifier` in the table.
#[test]
fn the_facet_kind_table_agrees_with_the_schema_documentation() {
    let info = <eml23::PropertyKindFacet as EnergymlType>::type_info();
    let field = info.field("Facet").expect("the Facet field");
    let variants = field.ty.allowed_values();
    assert!(!variants.is_empty());
    for variant in variants.iter().filter(|v| !v.is_other) {
        let documented = variant
            .doc
            .strip_prefix("Applies to ")
            .and_then(|rest| rest.split_once(" facet kind"))
            .map(|(kind, _)| kind);
        assert_eq!(schema_facet_kind(variant.value), documented, "the value `{}`", variant.value);
    }
    for (value, kind) in FACET_VALUE_KINDS {
        let variant = variants
            .iter()
            .find(|v| !v.is_other && v.value == *value)
            .unwrap_or_else(|| panic!("`{value}` is not a value of FacetExt"));
        assert!(variant.doc.contains(kind), "`{value}` is documented as `{}`", variant.doc);
    }
}

/// EML TRG 2.3 §11.11 p. 294: do not use several facets of one kind.
/// Break it: count the values instead of the kinds.
#[test]
fn two_facets_of_one_kind_are_said() {
    let mut object = base();
    object.facet = vec![
        facet(eml23::FacetKind::Direction, eml23::FacetExt::I),
        facet(eml23::FacetKind::Direction, eml23::FacetExt::J),
        facet(eml23::FacetKind::Statistics, eml23::FacetExt::Maximum),
    ];
    let seen = observed(&object, "length", &Datasets::default());
    let repeated: Vec<&PropertyObservation> = seen
        .iter()
        .filter(|o| matches!(o, PropertyObservation::FacetKindRepeated { .. }))
        .collect();
    assert_eq!(
        repeated,
        vec![&PropertyObservation::FacetKindRepeated {
            kind: "direction".into(),
            count: 2
        }]
    );
}

fn unit_of(seen: &[PropertyObservation]) -> &UnitCheck {
    seen.iter()
        .find_map(|o| match o {
            PropertyObservation::Unit(check) => Some(check),
            _ => None,
        })
        .expect("a unit observation")
}

/// RESQML TRG 2.2 §7.6: the unit converts to that of the kind.
/// Break it: return `Checked` without looking at the issues.
#[test]
fn a_length_in_metres_is_checked() {
    let seen = observed(&base(), "length", &Datasets::default());
    assert_eq!(
        unit_of(&seen),
        &UnitCheck::Checked {
            uom: "m".into(),
            class: "length".into()
        }
    );
    let mut object = base();
    object.uom = unit("Pa");
    let seen = observed(&object, "length", &Datasets::default());
    assert!(
        matches!(unit_of(&seen), UnitCheck::Outside(KindIssue::UomOutsideQuantityClass { .. })),
        "{seen:?}"
    );
}

/// Common TRG §11.9: a class this build cannot convert is not "checked".
/// Break it: make the absence of `UomOutsideQuantityClass` a `Checked`.
#[test]
fn a_class_this_build_cannot_check_is_not_checked() {
    let mut object = base();
    object.uom = unit("degC");
    let seen = observed(&object, "thermodynamic temperature", &Datasets::default());
    assert_eq!(
        unit_of(&seen),
        &UnitCheck::NotCheckable(UnitNotCheckable::ClassNotInThisBuild {
            class: "thermodynamic temperature".into(),
            uom: "degC".into()
        })
    );
}

/// A discrete property has no unit: nothing to check, said.
/// Break it: say `Checked`.
#[test]
fn no_unit_is_not_checkable() {
    let object = resqml22::DiscreteProperty {
        uuid: PROPERTY.into(),
        citation: citation(),
        indexable_element: eml23::IndexableElement::Nodes,
        value_count_per_indexable_element: vec![1],
        property_kind: reference(KIND, "eml23.PropertyKind"),
        supporting_representation: reference(REP, "resqml22.Grid2dRepresentation"),
        values_for_patch: vec![integers("/p", &[4], -1)],
        ..Default::default()
    };
    let read = property::read(&object).expect("a property");
    assert_eq!(read.uom, None);
    let source = Datasets::default();
    let dictionary = dictionary("length");
    let resolver = Resolver::with_dictionary(&source, &dictionary);
    let seen = observe::observations(&uri(), &read, &object, None, None, &source, &resolver);
    assert_eq!(unit_of(&seen), &UnitCheck::NotCheckable(UnitNotCheckable::NoUnit));
}

fn mark(value: &str, description: Option<&str>) -> eml23::ExtensionNameValue {
    eml23::ExtensionNameValue {
        name: "synthetic".into(),
        value: eml23::StringMeasure {
            value: value.into(),
            uom: None,
        },
        description: description.map(str::to_string),
        ..Default::default()
    }
}

fn marks(seen: &[PropertyObservation]) -> Vec<(MarkedObject, SyntheticMark)> {
    seen.iter()
        .filter_map(|o| match o {
            PropertyObservation::Synthetic { on, mark, .. } => Some((*on, mark.clone())),
            _ => None,
        })
        .collect()
}

/// ADR-0019: a mark is said only when written, `true`, and never deduced.
/// Break it: say `Marked` as soon as the name is present.
#[test]
fn a_synthetic_mark_is_said_only_when_written() {
    let mut object = base();
    object.extension_name_value = vec![mark("true", Some("laid by a law"))];
    assert_eq!(
        marks(&observed(&object, "length", &Datasets::default())),
        vec![(
            MarkedObject::Property,
            SyntheticMark::Marked {
                description: Some("laid by a law".into())
            }
        )]
    );
    object.extension_name_value = vec![mark("false", None)];
    assert!(marks(&observed(&object, "length", &Datasets::default())).is_empty());
    object.extension_name_value = vec![mark("maybe", None)];
    assert_eq!(
        marks(&observed(&object, "length", &Datasets::default())),
        vec![(MarkedObject::Property, SyntheticMark::Unclear { value: "maybe".into() })]
    );
    object.extension_name_value = Vec::new();
    assert!(marks(&observed(&object, "length", &Datasets::default())).is_empty());

    // The same mark on the supporting representation.
    let representation = resqml22::Grid2dRepresentation {
        uuid: REP.into(),
        citation: citation(),
        extension_name_value: vec![mark("true", None)],
        ..Default::default()
    };
    let source = Datasets::default().object(REP, Box::new(representation));
    assert_eq!(
        marks(&observed(&object, "length", &source)),
        vec![(
            MarkedObject::SupportingRepresentation,
            SyntheticMark::Marked { description: None }
        )]
    );
}

/// RESQML TRG 2.2 §7.1: the declared count against the stored one, by name.
/// Break it: return `StoredDisagrees` for both.
#[test]
fn a_count_of_one_per_element_is_said() {
    let object = property_of(vec![2], vec![floats("/p", &[4])]);
    let read = property::read(&object).expect("a property");
    let shape = PropertyShape::of(&read, nodes(4), None, None).expect("a shape");
    let source = Datasets::default();
    let dictionary = dictionary("length");
    let resolver = Resolver::with_dictionary(&source, &dictionary);
    let said = |stored: &[u64]| {
        observe::observations(&uri(), &read, &object, Some(&shape), Some(stored), &source, &resolver)
    };
    let seen = said(&[4]);
    assert!(matches!(&seen[0], PropertyObservation::Shape { layout: None, .. }), "{seen:?}");
    assert_eq!(seen[1], PropertyObservation::DeclaredCountWrong { declared: 2 });
    let seen = said(&[5]);
    assert!(matches!(&seen[0], PropertyObservation::Shape { layout: None, .. }), "{seen:?}");
    assert!(
        matches!(&seen[1], PropertyObservation::StoredDisagrees(ShapeRefusal::StoredMismatch { .. })),
        "{seen:?}"
    );
    // A stored shape that regroups the declared one: its layout is kept.
    let seen = said(&[8]);
    assert!(matches!(&seen[0], PropertyObservation::Shape { layout: Some(_), .. }), "{seen:?}");
    // No shape given: the shape is not guessed.
    let none = observe::observations(&uri(), &read, &object, None, Some(&[4]), &source, &resolver);
    assert!(!none.iter().any(|o| matches!(o, PropertyObservation::Shape { .. })));
}
